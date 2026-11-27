//! «Title bar buttons» and «Border size» of the «Settings» tab — in all
//! environments by their native key.
//!
//! Buttons. The person chooses the SIDE of the control buttons (on the right, as in
//! Windows, or on the left, as in macOS) and the SET — all buttons of the KDE «Window
//! Decorations» module: window menu, application menu, «On all desktops»,
//! minimize, maximize, close, help, «Keep below others», «Keep above others»,
//! «Hide from screenshots…», spacer; beyond KDE — «Roll up into title bar»
//! (Marco and xfwm4 have it, KWin 6.8 has no letter for it). Help and
//! the control buttons stand on their side, the menu and the "pins" — on the
//! opposite one. Buttons the environment does not know (for GNOME
//! `appmenu`, `icon`) stay on their side as they were — so the
//! default layouts survive parsing and building without a single change.
//! What the environment lacks from the KDE set, the section names one by one
//! ([`unavailable_note`]).
//!
//! Where things live (checked against the schemas and binaries in the system):
//! * KDE — `kwinrc [org.kde.kdecoration2] ButtonsOnLeft/ButtonsOnRight`
//!   (KWin 6.8 letters, `decorations/settings.cpp`: M menu, N application
//!   menu, S on all desktops, H help, I minimize, A maximize,
//!   X close, F keep above, B keep below, E hide from screenshots, `_` spacer; by
//!   default `MSE` / `HIAX`). The module's three check boxes are there too — [`FLAGS`];
//! * GNOME, Pantheon — `org.gnome.desktop.wm.preferences button-layout`
//!   (GTK takes the same one for windows with a client-side title bar);
//! * Budgie — `com.solus-project.budgie-wm button-layout` + the GNOME key;
//! * Cinnamon — `org.cinnamon.desktop.wm.preferences button-layout`
//!   (Muffin also knows `spacer`);
//! * MATE — `org.mate.Marco.general button-layout` +
//!   `org.mate.interface gtk-decoration-layout` (GTK windows with CSD);
//!   Marco has `appmenu`, `stick`, `above`, `shade`, `spacer`, Muffin
//!   and Mutter have no `stick`/`above`/`shade`;
//! * XFCE — `xfwm4 /general/button_layout` (`O|SHMC`: O menu, H minimize,
//!   M maximize, C close, T on all desktops, S roll up into title bar,
//!   `|` the title; no spacer, help, «keep above» or «keep below»);
//!   xfsettingsd itself carries the layout over to `Gtk/DecorationLayout`;
//! * COSMIC — only «minimize» and «maximize» on/off
//!   (`com.system76.CosmicTk/v1/show_minimize`, `show_maximize`);
//! * Deepin (Treeland) — no layout of its own, Deepin frames and DTK
//!   programs do not change buttons: the shared GTK branch below, the section honestly
//!   says so ([`note`]);
//! * Hyprland, niri, river — the compositor has no title bars at all; sway has
//!   title bars, but no buttons on them (sway(5): `border normal` — border and
//!   title, no buttons in the description). Buttons are drawn only by
//!   GTK programs themselves: we write `gtk-decoration-layout` into `settings.ini` and the
//!   GNOME key (the GTK portal reads it). That is what we say ([`note`]);
//! * Openbox — `<theme><titleLayout>` in `openbox/rc.xml` (letters from the
//!   description in the system `rc.xml` of Openbox 3.6: N window icon — also the window
//!   menu, L title, I minimize, M maximize, C close, S roll up into
//!   title bar, D on all desktops; each at most once, by default
//!   `NLIMC`); re-read by `openbox --reconfigure`;
//! * labwc — `<theme><titlebar><layout>` in `labwc/rc.xml` (labwc-config(5)
//!   0.20: `icon` window icon, `menu` window menu, `iconify`, `max`, `close`,
//!   `shade`, `desk` on all desktops; the colon is mandatory, by default
//!   `icon:iconify,max,close`); re-read by `labwc --reconfigure`. For both, a personal
//!   `rc.xml` replaces the system one entirely, so without a personal one it
//!   is created as a copy of the system one (the same as for the title font,
//!   `qt_settings::rc_xml_with_title_font`);
//! * LXQt — the layout is written by the window manager chosen in its session
//!   ([`wm_key`] via `qt_settings::title_wm_for`): Openbox and labwc — as above,
//!   KWin, xfwm4 and Marco — with the same keys as in KDE, XFCE and MATE.
//!   An unknown manager — the label says Rexon does not write its buttons.
//! In KDE the same `gtk-decoration-layout` is duplicated in `settings.ini`, so that
//! GTK programs with CSD follow the layout even without the kde-gtk-config module.
//!
//! Border. KDE — `BorderSize` (None … Oversized) and `BorderSizeAuto`
//! ("as intended by the decoration", the default); Hyprland —
//! `general.border_size`; niri — the width of the focus ring and border; sway —
//! `default_border`/`default_floating_border <style> N` (sway(5); the style —
//! with or without a title — is taken from the person's config, only the
//! thickness is changed); labwc — `border.width` in `themerc-override` (labwc-theme(5)).
//! In MATE and XFCE the border is drawn by the Marco/xfwm4 window theme (they have their own
//! «Appearance» sections); in GNOME, Pantheon and Cinnamon — by the GTK theme: Mutter and Muffin
//! take title bars from the CSS of the program theme (Muffin has not read the `org.cinnamon.desktop.
//! wm.preferences theme` key since version 5.4 — it is no longer in `src/core/prefs.c`).
//! There is no separate thickness there, the section honestly says so.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::animations::{
    gsettings_get, gsettings_set, kconfig_value, kwin_reconfigure, kwrite, parse_state_line, read_own_wm_file,
    run_steps, state_line, uses_gtk_ini, wm_include, xfconf_get, xfconf_set, LookCtx, LookIo, LookStep,
};
use crate::i18n::{n_, tr, trf};
use crate::themes::WmSyntax;

/// A button.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Btn {
    Menu,
    Minimize,
    Maximize,
    Close,
    Sticky,
    Above,
    /// Application menu (KDE `N`, Marco `appmenu`).
    AppMenu,
    /// «What's this?» help (KDE `H`).
    Help,
    /// Below other windows (KDE `B`).
    Below,
    /// Roll up into title bar (Marco `shade`, xfwm4 `S`; KWin has no letter).
    Shade,
    /// Hide from screenshots and screen recordings (KDE `E`).
    ExcludeCapture,
    /// Spacer (KDE `_`, `spacer` in Marco and Muffin).
    Spacer,
    /// Native name of a button we do not manage — kept as is.
    Other(String),
}

/// Managed buttons: (id, label) — in the order of the KDE module list.
pub const MANAGED: &[(&str, &str)] = &[
    ("menu", n_("Window menu")),
    ("appmenu", n_("Application menu")),
    ("sticky", n_("On all desktops")),
    ("minimize", n_("Minimize")),
    ("maximize", n_("Maximize")),
    ("close", n_("Close")),
    ("help", n_("Context help")),
    ("below", n_("Keep below others")),
    ("above", n_("Keep above others")),
    ("shade", n_("Roll up into title bar")),
    ("exclude_capture", n_("Hide from screenshots and screen recordings")),
    ("spacer", n_("Spacer")),
];

/// The button set of the KDE «Window Decorations» module — the yardstick of "what is missing" in
/// the other environments («Roll up into title bar» is not part of it: KWin does not
/// have it).
const KDE_SET: &[&str] =
    &["menu", "appmenu", "sticky", "minimize", "maximize", "close", "help", "below", "above", "exclude_capture", "spacer"];

impl Btn {
    fn id(&self) -> &str {
        match self {
            Btn::Menu => "menu",
            Btn::Minimize => "minimize",
            Btn::Maximize => "maximize",
            Btn::Close => "close",
            Btn::Sticky => "sticky",
            Btn::Above => "above",
            Btn::AppMenu => "appmenu",
            Btn::Help => "help",
            Btn::Below => "below",
            Btn::Shade => "shade",
            Btn::ExcludeCapture => "exclude_capture",
            Btn::Spacer => "spacer",
            Btn::Other(_) => "",
        }
    }
    fn from_id(id: &str) -> Option<Btn> {
        Some(match id {
            "menu" => Btn::Menu,
            "minimize" => Btn::Minimize,
            "maximize" => Btn::Maximize,
            "close" => Btn::Close,
            "sticky" => Btn::Sticky,
            "above" => Btn::Above,
            "appmenu" => Btn::AppMenu,
            "help" => Btn::Help,
            "below" => Btn::Below,
            "shade" => Btn::Shade,
            "exclude_capture" => Btn::ExcludeCapture,
            "spacer" => Btn::Spacer,
            _ => return None,
        })
    }
}

/// Layout: the left and right sides of the title bar.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Layout {
    pub left: Vec<Btn>,
    pub right: Vec<Btn>,
}

/// How the environment writes the layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Codec {
    /// KWin letters, two strings.
    Kde,
    /// `menu,minimize:maximize,close` (GNOME, GTK, Marco, Muffin).
    Gnome,
    /// `O|HMC` xfwm4.
    Xfwm,
    /// Openbox `NLIMC`: letters, `L` — the window title between the sides.
    Openbox,
    /// `icon:iconify,max,close` labwc.
    Labwc,
}

fn kde_char(b: &Btn) -> String {
    match b {
        Btn::Menu => "M".into(),
        Btn::Minimize => "I".into(),
        Btn::Maximize => "A".into(),
        Btn::Close => "X".into(),
        Btn::Sticky => "S".into(),
        Btn::Above => "F".into(),
        Btn::AppMenu => "N".into(),
        Btn::Help => "H".into(),
        Btn::Below => "B".into(),
        Btn::ExcludeCapture => "E".into(),
        Btn::Spacer => "_".into(),
        // KWin 6.8 has no letter for «Roll up into title bar».
        Btn::Shade => String::new(),
        Btn::Other(s) => s.clone(),
    }
}

fn kde_btn(c: char) -> Btn {
    match c {
        'M' => Btn::Menu,
        'I' => Btn::Minimize,
        'A' => Btn::Maximize,
        'X' => Btn::Close,
        'S' => Btn::Sticky,
        'F' => Btn::Above,
        'N' => Btn::AppMenu,
        'H' => Btn::Help,
        'B' => Btn::Below,
        'E' => Btn::ExcludeCapture,
        '_' => Btn::Spacer,
        other => Btn::Other(other.to_string()),
    }
}

fn gnome_token(b: &Btn) -> String {
    match b {
        Btn::Menu => "menu".into(),
        Btn::Minimize => "minimize".into(),
        Btn::Maximize => "maximize".into(),
        Btn::Close => "close".into(),
        Btn::Sticky => "stick".into(),
        Btn::Above => "above".into(),
        Btn::AppMenu => "appmenu".into(),
        Btn::Shade => "shade".into(),
        Btn::Spacer => "spacer".into(),
        // This record has no help, «Keep below others» or «Hide from screenshots…».
        Btn::Help | Btn::Below | Btn::ExcludeCapture => String::new(),
        Btn::Other(s) => s.clone(),
    }
}

fn gnome_btn(t: &str) -> Btn {
    match t {
        "menu" => Btn::Menu,
        "minimize" => Btn::Minimize,
        "maximize" => Btn::Maximize,
        "close" => Btn::Close,
        "stick" => Btn::Sticky,
        "above" => Btn::Above,
        "appmenu" => Btn::AppMenu,
        "shade" => Btn::Shade,
        "spacer" => Btn::Spacer,
        other => Btn::Other(other.to_string()),
    }
}

fn xfwm_char(b: &Btn) -> String {
    match b {
        Btn::Menu => "O".into(),
        Btn::Minimize => "H".into(),
        Btn::Maximize => "M".into(),
        Btn::Close => "C".into(),
        Btn::Sticky => "T".into(),
        Btn::Shade => "S".into(),
        Btn::Above | Btn::AppMenu | Btn::Help | Btn::Below | Btn::ExcludeCapture | Btn::Spacer => String::new(),
        Btn::Other(s) => s.clone(),
    }
}

fn xfwm_btn(c: char) -> Btn {
    match c {
        'O' => Btn::Menu,
        'H' => Btn::Minimize,
        'M' => Btn::Maximize,
        'C' => Btn::Close,
        'T' => Btn::Sticky,
        'S' => Btn::Shade,
        other => Btn::Other(other.to_string()),
    }
}

fn openbox_char(b: &Btn) -> String {
    match b {
        Btn::Menu => "N".into(),
        Btn::Minimize => "I".into(),
        Btn::Maximize => "M".into(),
        Btn::Close => "C".into(),
        Btn::Shade => "S".into(),
        Btn::Sticky => "D".into(),
        Btn::Above | Btn::AppMenu | Btn::Help | Btn::Below | Btn::ExcludeCapture | Btn::Spacer => String::new(),
        Btn::Other(s) => s.clone(),
    }
}

fn openbox_btn(c: char) -> Btn {
    match c {
        'N' => Btn::Menu,
        'I' => Btn::Minimize,
        'M' => Btn::Maximize,
        'C' => Btn::Close,
        'S' => Btn::Shade,
        'D' => Btn::Sticky,
        other => Btn::Other(other.to_string()),
    }
}

fn labwc_token(b: &Btn) -> String {
    match b {
        Btn::Menu => "menu".into(),
        Btn::Minimize => "iconify".into(),
        Btn::Maximize => "max".into(),
        Btn::Close => "close".into(),
        Btn::Shade => "shade".into(),
        Btn::Sticky => "desk".into(),
        Btn::Above | Btn::AppMenu | Btn::Help | Btn::Below | Btn::ExcludeCapture | Btn::Spacer => String::new(),
        Btn::Other(s) => s.clone(),
    }
}

fn labwc_btn(t: &str) -> Btn {
    match t {
        "menu" => Btn::Menu,
        "iconify" => Btn::Minimize,
        "max" => Btn::Maximize,
        "close" => Btn::Close,
        "shade" => Btn::Shade,
        "desk" => Btn::Sticky,
        other => Btn::Other(other.to_string()),
    }
}

/// Parsing the native record. For KDE — `left` and `right` separately.
pub fn parse(codec: Codec, left: &str, right: &str) -> Layout {
    match codec {
        Codec::Kde => Layout { left: left.chars().map(kde_btn).collect(), right: right.chars().map(kde_btn).collect() },
        Codec::Gnome => {
            let (l, r) = left.split_once(':').unwrap_or(("", left));
            let side = |s: &str| s.split(',').map(str::trim).filter(|t| !t.is_empty()).map(gnome_btn).collect();
            Layout { left: side(l), right: side(r) }
        }
        Codec::Xfwm => {
            let (l, r) = left.split_once('|').unwrap_or(("", left));
            Layout { left: l.chars().map(xfwm_btn).collect(), right: r.chars().map(xfwm_btn).collect() }
        }
        // Without `L` Openbox draws no title and puts the buttons in a row on the left.
        Codec::Openbox => {
            let (l, r) = left.trim().split_once('L').unwrap_or((left.trim(), ""));
            Layout { left: l.chars().map(openbox_btn).collect(), right: r.chars().map(openbox_btn).collect() }
        }
        Codec::Labwc => {
            let (l, r) = left.split_once(':').unwrap_or(("", left));
            let side = |s: &str| s.split(',').map(str::trim).filter(|t| !t.is_empty()).map(labwc_btn).collect();
            Layout { left: side(l), right: side(r) }
        }
    }
}

/// Building the native record: (left, right) for KDE, one string in `.0` for the others.
pub fn format(codec: Codec, layout: &Layout) -> (String, String) {
    match codec {
        Codec::Kde => (
            layout.left.iter().map(kde_char).collect(),
            layout.right.iter().map(kde_char).collect(),
        ),
        Codec::Gnome => {
            let side = |v: &[Btn]| v.iter().map(gnome_token).filter(|t| !t.is_empty()).collect::<Vec<_>>().join(",");
            (format!("{}:{}", side(&layout.left), side(&layout.right)), String::new())
        }
        Codec::Xfwm => {
            let side = |v: &[Btn]| v.iter().map(xfwm_char).collect::<String>();
            (format!("{}|{}", side(&layout.left), side(&layout.right)), String::new())
        }
        Codec::Openbox => {
            let side = |v: &[Btn]| v.iter().map(openbox_char).collect::<String>();
            (format!("{}L{}", side(&layout.left), side(&layout.right)), String::new())
        }
        Codec::Labwc => {
            let side = |v: &[Btn]| v.iter().map(labwc_token).filter(|t| !t.is_empty()).collect::<Vec<_>>().join(",");
            (format!("{}:{}", side(&layout.left), side(&layout.right)), String::new())
        }
    }
}

/// The person's choice: managed buttons on each side of the title bar, in
/// order from left to right. Both sides are stored separately — a layout like
/// `close:maximize` (Pantheon) survives parsing and building as is, rather than
/// collapsing onto one side.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Choice {
    pub left: Vec<String>,
    pub right: Vec<String>,
}

impl Choice {
    /// All chosen buttons (the left side, then the right).
    pub fn all(&self) -> Vec<String> {
        self.left.iter().chain(self.right.iter()).cloned().collect()
    }
    fn side_mut(&mut self, side: &str) -> &mut Vec<String> {
        if side == "left" {
            &mut self.left
        } else {
            &mut self.right
        }
    }
}

fn is_control(b: &Btn) -> bool {
    matches!(b, Btn::Minimize | Btn::Maximize | Btn::Close)
}

/// What is chosen in the layout: managed buttons by side, as they stand.
pub fn choice_of(layout: &Layout) -> Choice {
    let ids = |v: &[Btn]| v.iter().map(Btn::id).filter(|id| !id.is_empty()).map(str::to_string).collect();
    Choice { left: ids(&layout.left), right: ids(&layout.right) }
}

/// Which side the control buttons are on: `left` — as in macOS, `right` —
/// as in Windows. By «Close», and without it — by the other control
/// buttons.
pub fn side_of(layout: &Layout) -> &'static str {
    if layout.left.contains(&Btn::Close) {
        "left"
    } else if layout.right.contains(&Btn::Close) {
        "right"
    } else if layout.left.iter().any(is_control) && !layout.right.iter().any(is_control) {
        "left"
    } else {
        "right"
    }
}

/// Button order on each side from left to right: on the left — as in macOS
/// («Close» at the edge), on the right — as in Windows («Close» at the edge, before it
/// «Maximize» and «Minimize»). Help stands next to the control buttons, as in the
/// KDE default layout `MSE|HIAX`; the menu and the "pins" — in the order of the
/// KDE module.
const LEFT_ORDER: &[&str] = &[
    "close", "minimize", "maximize", "help", "menu", "appmenu", "sticky", "shade", "below", "above", "exclude_capture", "spacer",
];
const RIGHT_ORDER: &[&str] = &[
    "menu", "appmenu", "sticky", "shade", "below", "above", "exclude_capture", "spacer", "help", "minimize", "maximize", "close",
];

/// Buttons that a ready-made layout puts on the control buttons' side
/// («Roll up into title bar» — as in the xfwm4 default `O|SHMC`).
const CONTROL_SIDE: &[&str] = &["shade", "help", "minimize", "maximize", "close"];

fn order_index(side: &str, id: &str) -> usize {
    let order = if side == "left" { LEFT_ORDER } else { RIGHT_ORDER };
    order.iter().position(|x| *x == id).unwrap_or(order.len())
}

/// A ready-made layout: control buttons on side `side`, the menu and the
/// "pins" — on the opposite one.
pub fn preset(side: &str, buttons: &[String]) -> Choice {
    let has = |id: &str| buttons.iter().any(|b| b == id);
    let other = if side == "left" { RIGHT_ORDER } else { LEFT_ORDER };
    let own = if side == "left" { LEFT_ORDER } else { RIGHT_ORDER };
    let controls: Vec<String> =
        own.iter().filter(|i| CONTROL_SIDE.contains(i) && has(i)).map(|i| i.to_string()).collect();
    let extras: Vec<String> =
        other.iter().filter(|i| !CONTROL_SIDE.contains(i) && has(i)).map(|i| i.to_string()).collect();
    if side == "left" {
        Choice { left: controls, right: extras }
    } else {
        Choice { left: extras, right: controls }
    }
}

/// Put (`on`) button `id` on side `side` or remove it. A button
/// lives on one side: one put here leaves the other. A new one takes its place
/// in the side's order ([`LEFT_ORDER`] / [`RIGHT_ORDER`]), those already standing do not
/// move. A spacer is not a button: it can be on both sides at once, and a
/// check box touches only its own side.
pub fn toggle(choice: &Choice, side: &str, id: &str, on: bool) -> Choice {
    let mut c = choice.clone();
    if id == "spacer" {
        c.side_mut(side).retain(|b| b != id);
    } else {
        c.left.retain(|b| b != id);
        c.right.retain(|b| b != id);
    }
    if on {
        let rank = order_index(side, id);
        let list = c.side_mut(side);
        let at = list.iter().position(|b| order_index(side, b) > rank).unwrap_or(list.len());
        list.insert(at, id.to_string());
    }
    c
}

/// The shelf of the drag-and-drop editor: the place of buttons that are not in the title bar.
pub const SHELF: &str = "shelf";

fn is_side(place: &str) -> bool {
    place == "left" || place == "right"
}

/// Moving a button, as by drag and drop in the KDE «Window Decorations» module: from
/// `from` (`left` / `right` with position `from_index`, or the shelf [`SHELF`]) to
/// side `to` at position `to_index` — the place in the side already without the moved
/// button (beyond the length — to the end) — or to the shelf (remove). A button stands in
/// one place: a placed one leaves the old one. A spacer is not a button: there can be
/// any number of them, from the shelf it is always taken new.
pub fn move_button(choice: &Choice, id: &str, from: &str, from_index: usize, to: &str, to_index: usize) -> Choice {
    let mut c = choice.clone();
    if is_side(from) {
        let list = c.side_mut(from);
        // The position is stale (the section was re-read) — take the first such button.
        let at = if list.get(from_index).map(String::as_str) == Some(id) {
            Some(from_index)
        } else {
            list.iter().position(|b| b == id)
        };
        if let Some(at) = at {
            list.remove(at);
        }
    }
    if id != "spacer" {
        c.left.retain(|b| b != id);
        c.right.retain(|b| b != id);
    }
    if is_side(to) {
        let list = c.side_mut(to);
        let at = to_index.min(list.len());
        list.insert(at, id.to_string());
    }
    c
}

/// A step with a button or key (gamepad, keyboard): `left` / `right` — to
/// the place further left or right along the title bar (the outermost button of a side crosses
/// over the title to the other side, next to the title), [`SHELF`] — remove. The answer is
/// the new choice and where the button is now (`None` — on the shelf or there is no step).
pub fn nudge(choice: &Choice, side: &str, index: usize, dir: &str) -> (Choice, Option<(String, usize)>) {
    let list = if side == "left" { &choice.left } else { &choice.right };
    let Some(id) = list.get(index).cloned() else {
        return (choice.clone(), None);
    };
    let target = match (side, dir) {
        (_, SHELF) => None,
        ("left", "left") | ("right", "left") if index > 0 => Some((side, index - 1)),
        ("right", "left") => Some(("left", choice.left.len())),
        ("left", "right") | ("right", "right") if index + 1 < list.len() => Some((side, index + 1)),
        ("left", "right") => Some(("right", 0)),
        // There is nowhere to step beyond the window edge.
        _ => return (choice.clone(), Some((side.to_string(), index))),
    };
    match target {
        Some((to, at)) => (move_button(choice, &id, side, index, to, at), Some((to.to_string(), at))),
        None => (move_button(choice, &id, side, index, SHELF, 0), None),
    }
}

/// A button from the shelf with the «Add on the left / right» button: it takes its place in the side's
/// order, as with a check box ([`toggle`]); a spacer — next to the window title.
pub fn add_button(choice: &Choice, id: &str, side: &str) -> Choice {
    if id != "spacer" {
        return toggle(choice, side, id, true);
    }
    let at = if side == "left" { choice.left.len() } else { 0 };
    move_button(choice, id, SHELF, 0, side, at)
}

/// The button icon on the title bar mock-up and on the shelf (a font glyph: decoration
/// icons, as in KDE, do not exist outside KWin).
pub fn glyph(id: &str) -> &'static str {
    match id {
        "menu" => "≡",
        "appmenu" => "⋮",
        "sticky" => "◉",
        "minimize" => "–",
        "maximize" => "□",
        "close" => "✕",
        "help" => "?",
        "below" => "▾",
        "above" => "▴",
        "shade" => "▔",
        "exclude_capture" => "⊘",
        _ => "",
    }
}

/// Why a shelf button is missing in this environment (tooltip of an unavailable one).
pub fn missing_reason(de: &str) -> String {
    let wm = match de {
        "kde" => "KWin",
        "mate" => "Marco",
        "xfce" => "xfwm4",
        "cinnamon-mint" => "Muffin",
        "openbox" => "Openbox",
        "labwc" => "labwc",
        "cosmic" => "COSMIC",
        _ => return tr("Title bars drawn by GTK have no such button."),
    };
    trf("%1 has no such title bar button.", &[&wm])
}

/// One side: managed buttons by the choice; foreign ones stay on it —
/// those that stood before the first managed one go before them, the rest — after.
/// For a side without managed buttons the foreign ones go to the window edge: on the left — before,
/// on the right — after.
fn build_side(chosen: &[String], current: &[Btn], left: bool) -> Vec<Btn> {
    let first_managed = current.iter().position(|b| !matches!(b, Btn::Other(_)));
    let (mut lead, mut trail) = (Vec::new(), Vec::new());
    for (i, b) in current.iter().enumerate() {
        if let Btn::Other(_) = b {
            let before = match first_managed {
                Some(f) => i < f,
                None => left,
            };
            if before {
                lead.push(b.clone());
            } else {
                trail.push(b.clone());
            }
        }
    }
    lead.extend(chosen.iter().filter_map(|id| Btn::from_id(id)));
    lead.extend(trail);
    lead
}

/// A layout by the choice; foreign buttons stay on their sides.
pub fn build(choice: &Choice, current: &Layout) -> Layout {
    Layout { left: build_side(&choice.left, &current.left, true), right: build_side(&choice.right, &current.right, false) }
}

/// Which buttons the environment supports.
///
/// * KDE — all buttons of the KWin 6.8 «Window Decorations» module ([`KDE_SET`]);
/// * MATE (Marco) — `appmenu`, `stick`, `above`, `shade`, `spacer` beyond the
///   common ones (the key description in the `org.mate.marco` schema and Marco's button functions);
/// * XFCE (xfwm4) — `T` on all desktops and `S` roll up into title bar;
/// * Cinnamon (Muffin) — the `spacer` (the key description in the schema);
/// * Openbox and labwc — window menu, on all desktops, minimize, maximize,
///   close and roll up into title bar (letters `NDIMCS` / labwc-config(5) words);
/// * GNOME, Budgie, Pantheon and others — what GTK also draws for windows with
///   their own title bar: menu, minimize, maximize, close (GTK does not know the
///   spacer, and in GNOME the title bars of almost all windows are drawn by GTK).
pub fn supported_buttons(de: &str) -> Vec<&'static str> {
    match de {
        "kde" => KDE_SET.to_vec(),
        "mate" => vec!["menu", "appmenu", "sticky", "minimize", "maximize", "close", "above", "shade", "spacer"],
        "xfce" => vec!["menu", "sticky", "minimize", "maximize", "close", "shade"],
        "cinnamon-mint" => vec!["menu", "minimize", "maximize", "close", "spacer"],
        "openbox" | "labwc" => vec!["menu", "sticky", "minimize", "maximize", "close", "shade"],
        "cosmic" => vec!["minimize", "maximize"],
        _ => vec!["menu", "minimize", "maximize", "close"],
    }
}

/// What the environment lacks from the KDE set — by name, with the reason. Empty — everything
/// is there (KDE). Where buttons are not written at all (the [`note`] label already
/// says so) — also empty.
pub fn unavailable_note(de: &str) -> String {
    let supported = supported_buttons(de);
    let missing: Vec<String> = MANAGED
        .iter()
        .filter(|(id, _)| KDE_SET.contains(id) && !supported.contains(id))
        .map(|(_, label)| tr(label))
        .collect();
    if missing.is_empty() {
        return String::new();
    }
    let list = missing.join(", ");
    match de {
        "mate" => trf("Marco, the MATE window manager, has no such title bar buttons: %1.", &[&list]),
        "xfce" => trf("xfwm4, the Xfce window manager, has no such title bar buttons: %1.", &[&list]),
        "cinnamon-mint" => trf("Muffin, the Cinnamon window manager, has no such title bar buttons: %1.", &[&list]),
        "openbox" => trf("Openbox has no such title bar buttons: %1.", &[&list]),
        "labwc" => trf("labwc has no such title bar buttons: %1.", &[&list]),
        "cosmic" => String::new(),
        _ => trf("Title bars drawn by GTK have no such buttons: %1.", &[&list]),
    }
}

fn codec(de: &str) -> Codec {
    match de {
        "kde" => Codec::Kde,
        "xfce" => Codec::Xfwm,
        "openbox" => Codec::Openbox,
        "labwc" => Codec::Labwc,
        _ => Codec::Gnome,
    }
}

/// Who writes the layout: the environment, and in LXQt — the window manager of its session
/// (`qt_settings::title_wm_for` by `lxqt/session.conf`): Openbox → `openbox`,
/// labwc → `labwc`, KWin → `kde`, xfwm4 → `xfce`, Marco → `mate` (the keys of their
/// native environments). Unknown or not chosen — `lxqt` as is.
pub fn wm_key(io: &dyn LookIo, ctx: &LookCtx) -> String {
    use crate::qt_settings::TitleWm;
    if ctx.de != "lxqt" {
        return ctx.de.clone();
    }
    let dirs = std::env::var("XDG_CONFIG_DIRS").unwrap_or_default();
    let wm = crate::qt_settings::session_title_wm("lxqt", &ctx.config_home, &dirs, ctx.wayland, &|p| io.read(p));
    match wm {
        Some(TitleWm::Openbox) => "openbox",
        Some(TitleWm::Labwc) => "labwc",
        Some(TitleWm::KWin) => "kde",
        Some(TitleWm::Xfwm4) => "xfce",
        Some(TitleWm::Marco) => "mate",
        Some(TitleWm::Sway) => "sway",
        None => "lxqt",
    }
    .to_string()
}

/// A context with the environment replaced by [`wm_key`].
fn wm_ctx(io: &dyn LookIo, ctx: &LookCtx) -> LookCtx {
    let mut c = ctx.clone();
    c.de = wm_key(io, ctx);
    c
}

/// A manager with the layout in `rc.xml` and the path of the layout node in `<theme>`.
fn rc_layout_node(de: &str) -> Option<(crate::qt_settings::TitleWm, &'static [&'static str])> {
    use crate::qt_settings::TitleWm;
    match de {
        "openbox" => Some((TitleWm::Openbox, &["titleLayout"][..])),
        "labwc" => Some((TitleWm::Labwc, &["titlebar", "layout"][..])),
        _ => None,
    }
}

/// The manager's own `rc.xml`, its text (the personal one or the system sample).
fn rc_xml_of(io: &dyn LookIo, ctx: &LookCtx, wm: crate::qt_settings::TitleWm) -> (String, Option<String>) {
    let dirs = std::env::var("XDG_CONFIG_DIRS").unwrap_or_default();
    let (own, text, _) = crate::qt_settings::rc_xml_source(wm, &ctx.config_home, &dirs, &|p| io.read(p));
    (own, text)
}

/// Why the layout cannot be written into `rc.xml` (`None` — it can): Openbox
/// has neither a personal nor a system file, or the file does not parse.
pub fn rc_refusal(io: &dyn LookIo, ctx: &LookCtx) -> Option<String> {
    let (wm, node) = rc_layout_node(&ctx.de)?;
    let (own, text) = rc_xml_of(io, ctx, wm);
    if crate::qt_settings::rc_xml_with_theme_value(text.as_deref(), wm, node, "x").is_some() {
        return None;
    }
    let label = crate::animations::de_label(&ctx.de);
    Some(if text.is_none() {
        trf("%1 has no settings file to change: %2 doesn't exist, and there is no system one to start from.", &[&label, &own])
    } else {
        trf("The %1 settings file %2 can't be read, so Rexon leaves it as it is.", &[&label, &own])
    })
}

/// The default GTK layout (`gtk-decoration-layout`).
const GTK_DEFAULT: &str = "menu:minimize,maximize,close";

/// Where the environment stores the layout: (schema, key) for gsettings.
fn gsettings_key(de: &str) -> (&'static str, &'static str) {
    match de {
        "cinnamon-mint" => ("org.cinnamon.desktop.wm.preferences", "button-layout"),
        "mate" => ("org.mate.Marco.general", "button-layout"),
        "budgie" => ("com.solus-project.budgie-wm", "button-layout"),
        _ => ("org.gnome.desktop.wm.preferences", "button-layout"),
    }
}

fn gtk_ini_layout(io: &dyn LookIo, ctx: &LookCtx) -> Option<String> {
    let path = crate::gtk_sanitizer::gtk_ini_path(&ctx.config_home, "3.0");
    crate::gtk_sanitizer::read_ini_settings(io.read(&path).as_deref()).get("gtk-decoration-layout").cloned()
}

fn cosmic_path(ctx: &LookCtx, key: &str) -> String {
    ctx.cfg(&format!("cosmic/com.system76.CosmicTk/v1/{key}"))
}

/// The layout as it stands now (the native record).
pub fn read_layout(io: &dyn LookIo, ctx: &LookCtx) -> Layout {
    let de = ctx.de.as_str();
    match de {
        "kde" => parse(
            Codec::Kde,
            &kconfig_value(io, ctx, "kwinrc", "org.kde.kdecoration2", "ButtonsOnLeft").unwrap_or_else(|| "MSE".into()),
            &kconfig_value(io, ctx, "kwinrc", "org.kde.kdecoration2", "ButtonsOnRight").unwrap_or_else(|| "HIAX".into()),
        ),
        "xfce" => parse(Codec::Xfwm, &xfconf_get(io, "xfwm4", "/general/button_layout").unwrap_or_else(|| "O|SHMC".into()), ""),
        "cosmic" => {
            let on = |k: &str| io.read(&cosmic_path(ctx, k)).map(|t| t.trim() != "false").unwrap_or(true);
            let mut right = Vec::new();
            if on("show_minimize") {
                right.push(Btn::Minimize);
            }
            if on("show_maximize") {
                right.push(Btn::Maximize);
            }
            right.push(Btn::Close);
            Layout { left: Vec::new(), right }
        }
        "gnome" | "budgie" | "pantheon" | "cinnamon-mint" | "mate" => {
            let (schema, key) = gsettings_key(de);
            parse(Codec::Gnome, &gsettings_get(io, schema, key).unwrap_or_else(|| GTK_DEFAULT.into()), "")
        }
        "openbox" | "labwc" => {
            let (wm, node) = rc_layout_node(de).expect("у openbox и labwc раскладка в rc.xml");
            let raw = rc_xml_of(io, ctx, wm)
                .1
                .and_then(|t| crate::qt_settings::rc_xml_theme_value(&t, node))
                .filter(|v| !v.trim().is_empty());
            match raw {
                Some(v) => parse(codec(de), &v, ""),
                None => default_layout(de),
            }
        }
        _ => parse(Codec::Gnome, &gtk_ini_layout(io, ctx).unwrap_or_else(|| GTK_DEFAULT.into()), ""),
    }
}

/// The layout for GTK (`gtk-decoration-layout`): only what GTK knows.
pub fn gtk_layout(layout: &Layout) -> String {
    let side = |v: &[Btn]| {
        v.iter()
            .filter(|b| matches!(b, Btn::Menu | Btn::Minimize | Btn::Maximize | Btn::Close))
            .map(gnome_token)
            .collect::<Vec<_>>()
            .join(",")
    };
    format!("{}:{}", side(&layout.left), side(&layout.right))
}

/// The layout write plan.
pub fn plan_layout(io: &dyn LookIo, ctx: &LookCtx, new: &Layout) -> Vec<LookStep> {
    let old = read_layout(io, ctx);
    let de = ctx.de.as_str();
    let mut steps = Vec::new();
    if &old == new {
        return steps;
    }
    match de {
        "kde" => {
            let (l, r) = format(Codec::Kde, new);
            let (ol, or) = format(Codec::Kde, &old);
            if l != ol {
                steps.push(kwrite("kwinrc", "org.kde.kdecoration2", "ButtonsOnLeft", Some(&l), false));
            }
            if r != or {
                steps.push(kwrite("kwinrc", "org.kde.kdecoration2", "ButtonsOnRight", Some(&r), false));
            }
            steps.push(kwin_reconfigure());
        }
        "xfce" => steps.push(xfconf_set("xfwm4", "/general/button_layout", "string", &format(Codec::Xfwm, new).0)),
        "cosmic" => {
            // COSMIC has only two check boxes — the side is not compared.
            let has = |l: &Layout, b: &Btn| l.right.contains(b) || l.left.contains(b);
            for (key, b) in [("show_minimize", Btn::Minimize), ("show_maximize", Btn::Maximize)] {
                if has(new, &b) != has(&old, &b) {
                    steps.push(LookStep::WriteFile { path: cosmic_path(ctx, key), content: has(new, &b).to_string() });
                }
            }
        }
        "openbox" | "labwc" => {
            let (wm, node) = rc_layout_node(de).expect("у openbox и labwc раскладка в rc.xml");
            let (own, text) = rc_xml_of(io, ctx, wm);
            let value = format(codec(de), new).0;
            // The file does not parse (or Openbox has none at all) — no steps,
            // the refusal with the reason is given by [`rc_refusal`].
            let Some(content) = crate::qt_settings::rc_xml_with_theme_value(text.as_deref(), wm, node, &value) else {
                return Vec::new();
            };
            steps.push(LookStep::WriteFile { path: own, content });
            steps.push(LookStep::Notify(vec![crate::animations::argv(&[wm.program(), "--reconfigure"])]));
            // GTK programs with their own title bar — the same shared layer.
            steps.push(LookStep::GtkIni { key: "gtk-decoration-layout".into(), value: gtk_layout(new) });
        }
        "gnome" | "budgie" | "pantheon" | "cinnamon-mint" | "mate" => {
            let (schema, key) = gsettings_key(de);
            let value = format!("'{}'", format(Codec::Gnome, new).0);
            steps.push(gsettings_set(schema, key, &value));
            if de == "budgie" {
                steps.push(gsettings_set("org.gnome.desktop.wm.preferences", "button-layout", &value));
            }
            if de == "mate" {
                steps.push(gsettings_set("org.mate.interface", "gtk-decoration-layout", &format!("'{}'", gtk_layout(new))));
            }
        }
        _ => {
            // A compositor without title bars: buttons are drawn only by GTK programs.
            // The GNOME key is read by the GTK portal — write it if there is gsettings
            // (failure is no problem: settings.ini is already written).
            steps.push(LookStep::GtkIni { key: "gtk-decoration-layout".into(), value: gtk_layout(new) });
            if io.have("gsettings") {
                steps.push(LookStep::Notify(vec![crate::animations::argv(&[
                    "gsettings",
                    "set",
                    "org.gnome.desktop.wm.preferences",
                    "button-layout",
                    &format!("'{}'", gtk_layout(new)),
                ])]));
            }
        }
    }
    if de == "kde" {
        // GTK programs with CSD — the same shared layer, without kde-gtk-config.
        steps.push(LookStep::GtkIni { key: "gtk-decoration-layout".into(), value: gtk_layout(new) });
    }
    steps
}

// ═══════════════════════════════════════════════════════════════════════════
// KDE title bar check boxes
// ═══════════════════════════════════════════════════════════════════════════

/// Check boxes of the «Titlebar Buttons» window of the KDE «Window Decorations» module: (id, the
/// `kwinrc [org.kde.kdecoration2]` key, default value, label). Keys
/// and defaults — from `kwindecorationsettings.kcfg` of KWin 6.8.
pub const FLAGS: &[(&str, &str, bool, &str)] = &[
    ("close_on_double_click", "CloseOnDoubleClickOnMenu", false, n_("Close windows by double-clicking the window menu button")),
    ("show_tooltips", "ShowToolTips", true, n_("Show title bar button tooltips")),
    (
        "always_show_exclude_capture",
        "AlwaysShowExcludeFromCapture",
        false,
        n_("Always show the “Hide from screenshots and screen recordings” button"),
    ),
];

const DECO_GROUP: &str = "org.kde.kdecoration2";

/// A check box for the section.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FlagOption {
    pub id: String,
    pub label: String,
    pub checked: bool,
    /// Can be changed (the "always show 'hide from screenshots'" one — only
    /// when the button itself is in the title bar, as in KDE).
    pub enabled: bool,
    /// Explanation under the check box (empty — none).
    pub hint: String,
}

/// KConfig boolean value (`true`, `1`, `on`, `yes` — true).
fn kconfig_bool(raw: Option<String>, default: bool) -> bool {
    match raw {
        Some(v) => matches!(v.trim().to_ascii_lowercase().as_str(), "true" | "1" | "on" | "yes"),
        None => default,
    }
}

fn flag_value(io: &dyn LookIo, ctx: &LookCtx, key: &str, default: bool) -> bool {
    kconfig_bool(kconfig_value(io, ctx, "kwinrc", DECO_GROUP, key), default)
}

fn has_button(layout: &Layout, b: &Btn) -> bool {
    layout.left.contains(b) || layout.right.contains(b)
}

/// The environment's check boxes: KDE has three KWin check boxes, the others have none
/// ([`flags_note`]).
pub fn flags(io: &dyn LookIo, ctx: &LookCtx, layout: &Layout) -> Vec<FlagOption> {
    if ctx.de != "kde" {
        return Vec::new();
    }
    FLAGS
        .iter()
        .map(|(id, key, default, label)| {
            let checked = flag_value(io, ctx, key, *default);
            let (enabled, hint) = match *id {
                "close_on_double_click" if checked => (true, tr("Click and hold the window menu button to show the menu.")),
                "always_show_exclude_capture" => {
                    let present = has_button(layout, &Btn::ExcludeCapture);
                    let base = tr("When off, the button appears only while the window is hidden from screenshots and screen recordings.");
                    if present {
                        (true, base)
                    } else {
                        (false, format!("{base} {}", tr("To turn this on, add the “Hide from screenshots and screen recordings” button to the title bar first.")))
                    }
                }
                _ => (true, String::new()),
            };
            FlagOption { id: id.to_string(), label: tr(label), checked, enabled, hint }
        })
        .collect()
}

/// Why there are no check boxes: these are KWin settings, other window managers
/// have no such thing (Marco, xfwm4, Mutter, Muffin — neither in gsettings schemas nor in
/// xfconf properties).
pub fn flags_note(de: &str) -> String {
    match de {
        "kde" => String::new(),
        other => trf(
            "%1 has no settings for closing windows by double-clicking the menu button, for title bar button tooltips or for the “Hide from screenshots” button: these are options of KWin, the KDE window manager.",
            &[&crate::animations::de_label(other)],
        ),
    }
}

/// The check box plan. The default value is not written but removed — as in the
/// KDE module, so that `kwinrc` does not accumulate extra lines.
pub fn plan_flag(io: &dyn LookIo, ctx: &LookCtx, id: &str, on: bool) -> Vec<LookStep> {
    if ctx.de != "kde" {
        return Vec::new();
    }
    let Some((_, key, default, _)) = FLAGS.iter().find(|(fid, ..)| *fid == id) else {
        return Vec::new();
    };
    if id == "always_show_exclude_capture" && !has_button(&read_layout(io, ctx), &Btn::ExcludeCapture) {
        return Vec::new();
    }
    if flag_value(io, ctx, key, *default) == on {
        return Vec::new();
    }
    let value = (on != *default).then_some(if on { "true" } else { "false" });
    vec![kwrite("kwinrc", DECO_GROUP, key, value, true), kwin_reconfigure()]
}

/// Returning the check boxes to the default: remove the written keys.
fn reset_flags_steps(io: &dyn LookIo, ctx: &LookCtx) -> Vec<LookStep> {
    if ctx.de != "kde" {
        return Vec::new();
    }
    let mut steps: Vec<LookStep> = FLAGS
        .iter()
        .filter(|(_, key, ..)| kconfig_value(io, ctx, "kwinrc", DECO_GROUP, key).is_some())
        .map(|(_, key, ..)| kwrite("kwinrc", DECO_GROUP, key, None, false))
        .collect();
    if !steps.is_empty() {
        steps.push(kwin_reconfigure());
    }
    steps
}

pub fn apply_flag(io: &dyn LookIo, ctx: &LookCtx, id: &str, on: bool) -> String {
    let ctx = &wm_ctx(io, ctx);
    finish(io, ctx, plan_flag(io, ctx, id, on))
}

// ═══════════════════════════════════════════════════════════════════════════
// Border
// ═══════════════════════════════════════════════════════════════════════════

/// KDE border sizes (KWin names) and labels as in KDE.
const KDE_BORDERS: &[(&str, &str)] = &[
    ("auto", n_("Theme's default")),
    ("None", n_("No borders")),
    ("NoSides", n_("No side borders")),
    ("Tiny", n_("Tiny")),
    ("Normal", n_("Normal")),
    ("Large", n_("Large")),
    ("VeryLarge", n_("Very large")),
    ("Huge", n_("Huge")),
    ("VeryHuge", n_("Very huge")),
    ("Oversized", n_("Oversized")),
];

/// Thickness in points for tiling compositors.
const PX_BORDERS: &[u32] = &[0, 1, 2, 3, 4, 5, 6, 8, 10];

pub const BORDER_TAG: &str = "window borders";
pub const BORDER_STEM: &str = "rexon-borders";

pub fn border_supported(de: &str) -> bool {
    matches!(de, "kde" | "hyprland" | "niri" | "sway" | "labwc")
}

/// The labwc theme override file (labwc-theme(5): `themerc-override` in the labwc
/// settings directory overrides theme keys).
pub fn labwc_override_path(ctx: &LookCtx) -> String {
    ctx.cfg("labwc/themerc-override")
}

/// The labwc border thickness key.
pub const LABWC_BORDER_KEY: &str = "border.width";

/// Value of a `key: value` key in the `themerc` text (empty — no key).
pub fn themerc_value(text: &str, key: &str) -> Option<String> {
    text.lines().rev().find_map(|line| {
        let (k, v) = line.trim().split_once(':')?;
        (k.trim() == key && !line.trim_start().starts_with('#')).then(|| v.trim().to_string())
    })
}

/// The `themerc` text with key `key` replaced by `value` (`None` — remove
/// the key). The person's other lines — as they were.
pub fn themerc_with(text: &str, key: &str, value: Option<&str>) -> String {
    let mut out: Vec<String> = text
        .lines()
        .filter(|line| {
            let t = line.trim();
            t.starts_with('#') || t.split_once(':').map(|(k, _)| k.trim() != key).unwrap_or(true)
        })
        .map(str::to_string)
        .collect();
    if let Some(v) = value {
        out.push(format!("{key}: {v}"));
    }
    let mut joined = out.join("\n");
    if !joined.is_empty() {
        joined.push('\n');
    }
    joined
}

/// The sway border style from the person's config: the last `default_border` and
/// `default_floating_border` (sway(5): `normal|none|pixel [<n>]`; by
/// default `normal`). The person chooses a thickness, so a border is needed: `none`
/// becomes `pixel` — without a title, as it was.
pub fn sway_border_styles(config: Option<&str>) -> (String, String) {
    let mut tiled = "normal".to_string();
    let mut floating = "normal".to_string();
    for line in config.unwrap_or("").lines() {
        let mut words = line.split_whitespace();
        let (Some(cmd), Some(style)) = (words.next(), words.next()) else {
            continue;
        };
        let style = match style {
            "normal" => "normal",
            "pixel" | "none" => "pixel",
            _ => continue,
        };
        match cmd {
            "default_border" => tiled = style.to_string(),
            "default_floating_border" => floating = style.to_string(),
            _ => {}
        }
    }
    (tiled, floating)
}

/// The sway border from the person's config as it is — for returning "as in the
/// config" by a command: style and thickness of the last `default_border` and
/// `default_floating_border`. Without them — the sway(5) default `normal 2`;
/// `none` carries no thickness.
pub fn sway_border_defaults(config: Option<&str>) -> (String, String) {
    let mut tiled = "normal 2".to_string();
    let mut floating = "normal 2".to_string();
    for line in config.unwrap_or("").lines() {
        let mut words = line.split_whitespace();
        let (Some(cmd), Some(style)) = (words.next(), words.next()) else {
            continue;
        };
        let value = match (style, words.next()) {
            ("none", _) => "none".to_string(),
            ("normal" | "pixel", Some(n)) if n.parse::<u32>().is_ok() => format!("{style} {n}"),
            ("normal" | "pixel", _) => format!("{style} 2"),
            _ => continue,
        };
        match cmd {
            "default_border" => tiled = value,
            "default_floating_border" => floating = value,
            _ => {}
        }
    }
    (tiled, floating)
}

/// Border options: (id, label).
pub fn border_options(de: &str) -> Vec<(String, String)> {
    match de {
        "kde" => KDE_BORDERS.iter().map(|(id, l)| (id.to_string(), tr(l))).collect(),
        "hyprland" | "niri" | "sway" | "labwc" => {
            let mut v = vec![("default".to_string(), tr("As in the config"))];
            v.extend(PX_BORDERS.iter().map(|p| (p.to_string(), trf("%1 px", &[p]))));
            v
        }
        _ => Vec::new(),
    }
}

pub fn read_border(io: &dyn LookIo, ctx: &LookCtx) -> String {
    match ctx.de.as_str() {
        "kde" => {
            let auto = kconfig_value(io, ctx, "kwinrc", "org.kde.kdecoration2", "BorderSizeAuto").map(|v| v != "false").unwrap_or(true);
            if auto {
                "auto".into()
            } else {
                kconfig_value(io, ctx, "kwinrc", "org.kde.kdecoration2", "BorderSize").unwrap_or_else(|| "Normal".into())
            }
        }
        "hyprland" | "niri" | "sway" => read_own_wm_file(io, ctx, &ctx.de, BORDER_STEM)
            .map(|(_, t)| parse_state_line(&t))
            .and_then(|m| m.get("border").cloned())
            .unwrap_or_else(|| "default".into()),
        "labwc" => io
            .read(&labwc_override_path(ctx))
            .and_then(|t| themerc_value(&t, LABWC_BORDER_KEY))
            .unwrap_or_else(|| "default".into()),
        _ => String::new(),
    }
}

/// Our border file for the compositor.
pub fn border_content(de: &str, syntax: WmSyntax, value: &str) -> String {
    border_content_styled(de, syntax, value, &("normal".to_string(), "normal".to_string()))
}

/// The same with the ready sway border styles ([`sway_border_styles`]).
pub fn border_content_styled(de: &str, syntax: WmSyntax, value: &str, sway: &(String, String)) -> String {
    let c = syntax.comment();
    let mut st = BTreeMap::new();
    st.insert("border".to_string(), value.to_string());
    let mut out = state_line(c, &st);
    out.push_str(&format!("{c} Rexon: window borders. This file is rewritten by Rexon; change borders in Rexon instead.\n"));
    if value == "default" {
        return out;
    }
    out.push_str(&match (de, syntax) {
        (_, WmSyntax::HyprLua) => format!("hl.config({{ general = {{ border_size = {value} }} }})\n"),
        (_, WmSyntax::HyprConf) => format!("general {{\n    border_size = {value}\n}}\n"),
        (_, WmSyntax::Sway) => {
            format!("default_border {} {value}\ndefault_floating_border {} {value}\n", sway.0, sway.1)
        }
        _ => format!("layout {{\n    focus-ring {{\n        width {value}\n    }}\n    border {{\n        width {value}\n    }}\n}}\n"),
    });
    out
}

pub fn plan_border(io: &dyn LookIo, ctx: &LookCtx, value: &str) -> Vec<LookStep> {
    let mut steps = Vec::new();
    if read_border(io, ctx) == value || !border_options(&ctx.de).iter().any(|(id, _)| id == value) {
        return steps;
    }
    match ctx.de.as_str() {
        "kde" => {
            if value == "auto" {
                steps.push(kwrite("kwinrc", "org.kde.kdecoration2", "BorderSizeAuto", None, true));
            } else {
                steps.push(kwrite("kwinrc", "org.kde.kdecoration2", "BorderSizeAuto", Some("false"), true));
                steps.push(kwrite("kwinrc", "org.kde.kdecoration2", "BorderSize", Some(value), false));
            }
            steps.push(kwin_reconfigure());
        }
        "hyprland" | "niri" => {
            let de = ctx.de.clone();
            let v = value.to_string();
            steps.push(LookStep::Wm(wm_include(ctx, &ctx.de, BORDER_TAG, BORDER_STEM, &move |s| border_content(&de, s, &v))));
        }
        "sway" => {
            // The style (with or without a title) — the one in the person's config.
            let config = crate::animations::wm_targets(ctx, "sway", BORDER_STEM, &|_| String::new())
                .into_iter()
                .find_map(|t| io.read(&t.config));
            let styles = sway_border_styles(config.as_deref());
            let v = value.to_string();
            let inc_styles = styles.clone();
            steps.push(LookStep::Wm(wm_include(ctx, "sway", BORDER_TAG, BORDER_STEM, &move |s| {
                border_content_styled("sway", s, &v, &inc_styles)
            })));
            // sway does not re-read its config by itself, and `default_border` applies
            // only to new windows (sway(5)). We tell the running sway the same thing
            // by commands: to new windows — `default_border`, to open ones —
            // `border`. Not `swaymsg reload`: it restarts sway's own swaybg,
            // and the background from its config lies ON TOP of Rexon's wallpaper (checked
            // on sway 1.12 headless). No session — silently skipped.
            let notify = |cmd: String| LookStep::Notify(vec![crate::animations::argv(&["swaymsg", &cmd])]);
            if value == "default" {
                let (tiled, floating) = sway_border_defaults(config.as_deref());
                steps.push(notify(format!("default_border {tiled}")));
                steps.push(notify(format!("default_floating_border {floating}")));
            } else {
                steps.push(notify(format!("default_border {} {value}", styles.0)));
                steps.push(notify(format!("default_floating_border {} {value}", styles.1)));
                for (criteria, style) in [("[tiling]", &styles.0), ("[floating]", &styles.1)] {
                    steps.push(notify(format!("{criteria} border {style} {value}")));
                }
            }
        }
        "labwc" => {
            let path = labwc_override_path(ctx);
            let before = io.read(&path).unwrap_or_default();
            let after = themerc_with(&before, LABWC_BORDER_KEY, (value != "default").then_some(value));
            steps.push(LookStep::WriteFile { path, content: after });
            // labwc(1): `--reconfigure` re-reads the config and the theme.
            steps.push(LookStep::Notify(vec![crate::animations::argv(&["labwc", "--reconfigure"])]));
        }
        _ => {}
    }
    steps
}

// ═══════════════════════════════════════════════════════════════════════════
// Snapshot and applying
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ButtonOption {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ButtonsState {
    pub de: String,
    /// The side is selectable (not on COSMIC).
    pub has_side: bool,
    /// Where the control buttons are: `right` (as in Windows) | `left` (as in macOS).
    pub side: String,
    pub buttons: Vec<ButtonOption>,
    /// The chosen buttons on the left and on the right side (in order).
    pub left: Vec<String>,
    pub right: Vec<String>,
    /// All chosen ones — for the ready-made "as in Windows / macOS" layouts.
    pub selected: Vec<String>,
    /// The current layout is one the choice does not convey verbatim.
    pub custom: String,
    pub note: String,
    /// Which KDE buttons the environment lacks — by name, with the reason.
    pub unavailable: String,
    /// Title bar check boxes (KDE only) and why the others do not have them.
    pub flags: Vec<FlagOption>,
    pub flags_note: String,
    pub has_border: bool,
    pub border: String,
    pub border_options: Vec<ButtonOption>,
    pub border_note: String,
    /// Drag-and-drop editor (title bar mock-up + shelf) — for environments with
    /// a side; COSMIC has only check boxes.
    pub drag: bool,
    /// Shelf: all buttons of the KDE module (and capable environments on top of them), in
    /// the order of the KDE list.
    pub shelf: Vec<ShelfButton>,
}

/// A shelf button of the drag-and-drop editor.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ShelfButton {
    pub id: String,
    pub label: String,
    pub glyph: String,
    /// Already in the title bar (a spacer — never: there can be any number of them).
    pub placed: bool,
    /// The environment supports such a button; if not, `reason` says why.
    pub available: bool,
    pub reason: String,
}

fn shelf_of(de: &str, choice: &Choice) -> Vec<ShelfButton> {
    let supported = supported_buttons(de);
    MANAGED
        .iter()
        .filter(|(id, _)| KDE_SET.contains(id) || supported.contains(id))
        .map(|(id, label)| {
            let available = supported.contains(id);
            ShelfButton {
                id: id.to_string(),
                label: tr(label),
                glyph: glyph(id).to_string(),
                placed: *id != "spacer" && choice.all().iter().any(|b| b == id),
                available,
                reason: if available { String::new() } else { missing_reason(de) },
            }
        })
        .collect()
}

fn current_text(io: &dyn LookIo, ctx: &LookCtx, layout: &Layout) -> String {
    let _ = io;
    match ctx.de.as_str() {
        "kde" => {
            let (l, r) = format(Codec::Kde, layout);
            format!("{l} | {r}")
        }
        "cosmic" => String::new(),
        "openbox" | "labwc" => format(codec(&ctx.de), layout).0,
        de if !uses_gtk_ini(de) || matches!(de, "gnome" | "budgie" | "pantheon") => format(codec(de), layout).0,
        _ => format(Codec::Gnome, layout).0,
    }
}

pub fn note(de: &str) -> String {
    match de {
        "kde" | "gnome" | "budgie" | "pantheon" | "cinnamon-mint" | "mate" | "xfce" | "openbox" | "labwc" => String::new(),
        "sway" => tr("sway's title bars have no buttons: windows are closed and moved with keyboard shortcuts. The layout applies to applications that draw their title bar themselves (GTK)."),
        // LXQt with a manager Rexon does not know ([`wm_key`]): with
        // Openbox, labwc, KWin, xfwm4 and Marco we do not get here.
        "lxqt" => tr("In LXQt, title bars are drawn by the window manager chosen in Session Settings. Rexon changes their buttons under Openbox, labwc, KWin, xfwm4 and Marco, and this session uses another one (or none is chosen): the layout applies only to applications that draw their title bar themselves (GTK)."),
        "cosmic" => tr("COSMIC keeps the Close button on the right; only Minimize and Maximize can be turned on or off."),
        // Treeland has no button layout of its own: Deepin frames and the windows
        // of DTK programs draw buttons their own way, the layout reaches only
        // GTK programs with their own title bar (the shared GTK branch).
        "deepin" => tr("Deepin's window frames and its own apps keep their buttons as they are: the layout applies only to applications that draw their title bar themselves (GTK)."),
        "hyprland" | "niri" | "river" => trf(
            "%1 draws no title bars of its own: the layout applies to applications that draw their title bar themselves (GTK).",
            &[&crate::animations::de_label(de)],
        ),
        // Unknown compositor: we claim nothing about its title bars.
        other => trf(
            "Rexon can't change the title bar buttons of %1: the layout applies only to applications that draw their title bar themselves (GTK).",
            &[&crate::animations::de_label(other)],
        ),
    }
}

pub fn border_note(de: &str) -> String {
    match de {
        "kde" | "hyprland" => String::new(),
        "niri" => tr("Sets the width of the focus ring and of the window border (if the border is turned on in the config)."),
        "sway" => tr("Only the width changes: whether windows have a title bar stays as set in the sway config."),
        "labwc" => String::new(),
        "budgie" | "mate" | "xfce" | "openbox" => {
            trf("In %1 the window border is drawn by the window theme; choose another window theme to change it.", &[&crate::animations::de_label(de)])
        }
        // Mutter and Muffin draw title bars and frames by the GTK theme CSS.
        "gnome" | "pantheon" | "cinnamon-mint" => trf(
            "In %1 window borders and title bars are drawn by the application theme (GTK); choose another GTK theme to change them.",
            &[&crate::animations::de_label(de)],
        ),
        "lxqt" => tr("In LXQt the window border is drawn by the window manager's theme (Openbox, KWin or labwc)."),
        other => trf("%1 has no window border setting.", &[&crate::animations::de_label(other)]),
    }
}

pub fn read_state(io: &dyn LookIo, ctx: &LookCtx) -> ButtonsState {
    let ctx = &wm_ctx(io, ctx);
    let de = ctx.de.as_str();
    let layout = read_layout(io, ctx);
    let choice = choice_of(&layout);
    let rebuilt = build(&choice, &layout);
    let custom = if rebuilt != layout { current_text(io, ctx, &layout) } else { String::new() };
    let supported = supported_buttons(de);
    let known = |v: &[String]| v.iter().filter(|b| supported.contains(&b.as_str())).cloned().collect::<Vec<_>>();
    ButtonsState {
        de: ctx.de.clone(),
        has_side: de != "cosmic",
        side: side_of(&layout).to_string(),
        buttons: MANAGED
            .iter()
            .filter(|(id, _)| supported.contains(id))
            .map(|(id, l)| ButtonOption { id: id.to_string(), label: tr(l) })
            .collect(),
        left: known(&choice.left),
        right: known(&choice.right),
        selected: known(&choice.all()),
        custom,
        note: note(de),
        unavailable: unavailable_note(de),
        flags: flags(io, ctx, &layout),
        flags_note: flags_note(de),
        has_border: border_supported(de),
        border: read_border(io, ctx),
        border_options: border_options(de).into_iter().map(|(id, label)| ButtonOption { id, label }).collect(),
        border_note: border_note(de),
        drag: de != "cosmic",
        shelf: shelf_of(de, &Choice { left: known(&choice.left), right: known(&choice.right) }),
    }
}

/// Apply the choice of buttons by side.
pub fn apply_choice(io: &dyn LookIo, ctx: &LookCtx, choice: &Choice) -> String {
    let ctx = &wm_ctx(io, ctx);
    if let Some(why) = rc_refusal(io, ctx) {
        return why;
    }
    let current = read_layout(io, ctx);
    let mut choice = choice.clone();
    // Buttons this environment cannot handle are not touched: they stay as they were —
    // on their own side.
    // Those that stood before the first capable one go before the chosen ones, the rest — after
    // (for GNOME `appmenu:close` → "as in macOS" gives `appmenu,close,…:`).
    let supported = supported_buttons(&ctx.de);
    let cur = choice_of(&current);
    for (side, ids) in [("left", &cur.left), ("right", &cur.right)] {
        let first_known = ids.iter().position(|b| supported.contains(&b.as_str()));
        let mut lead = Vec::new();
        for (i, b) in ids.iter().enumerate() {
            if !supported.contains(&b.as_str()) && !choice.all().contains(b) {
                let before = match first_known {
                    Some(f) => i < f,
                    None => side == "left",
                };
                if before {
                    lead.push(b.clone());
                } else {
                    choice.side_mut(side).push(b.clone());
                }
            }
        }
        let list = choice.side_mut(side);
        lead.append(list);
        *list = lead;
    }
    let new = build(&choice, &current);
    finish(io, ctx, plan_layout(io, ctx, &new))
}

/// The ready-made layout "as in Windows" (`right`) or "as in macOS" (`left`)
/// with the set `buttons`.
pub fn apply_preset(io: &dyn LookIo, ctx: &LookCtx, side: &str, buttons: &[String]) -> String {
    apply_choice(io, ctx, &preset(side, buttons))
}

/// Put or remove one button on side `side`; the rest stays as it is.
pub fn apply_toggle(io: &dyn LookIo, ctx: &LookCtx, side: &str, id: &str, on: bool) -> String {
    let ctx = &wm_ctx(io, ctx);
    let current = choice_of(&read_layout(io, ctx));
    apply_choice(io, ctx, &toggle(&current, side, id, on))
}

/// The choice as the section sees it: only the environment's capable buttons (positions
/// of the drag-and-drop editor are counted by it; [`apply_choice`] leaves
/// the rest in place itself).
fn shown_choice(io: &dyn LookIo, ctx: &LookCtx) -> Choice {
    let supported = supported_buttons(&ctx.de);
    let c = choice_of(&read_layout(io, ctx));
    let known = |v: &[String]| v.iter().filter(|b| supported.contains(&b.as_str())).cloned().collect();
    Choice { left: known(&c.left), right: known(&c.right) }
}

/// Drag and drop ([`move_button`]); a button the environment does not have
/// is refused with a reason.
pub fn apply_move(
    io: &dyn LookIo,
    ctx: &LookCtx,
    id: &str,
    from: &str,
    from_index: usize,
    to: &str,
    to_index: usize,
) -> String {
    let ctx = &wm_ctx(io, ctx);
    if is_side(to) && !supported_buttons(&ctx.de).contains(&id) {
        return missing_reason(&ctx.de);
    }
    let current = shown_choice(io, ctx);
    apply_choice(io, ctx, &move_button(&current, id, from, from_index, to, to_index))
}

/// A step with the "left / right / to the shelf" buttons ([`nudge`]): the message and where
/// the button is now (so that the selection follows it).
pub fn apply_nudge(io: &dyn LookIo, ctx: &LookCtx, side: &str, index: usize, dir: &str) -> (String, Option<(String, usize)>) {
    let ctx = &wm_ctx(io, ctx);
    let (next, at) = nudge(&shown_choice(io, ctx), side, index, dir);
    (apply_choice(io, ctx, &next), at)
}

/// "Add on the left / right" from the shelf ([`add_button`]).
pub fn apply_add(io: &dyn LookIo, ctx: &LookCtx, id: &str, side: &str) -> String {
    let ctx = &wm_ctx(io, ctx);
    if !supported_buttons(&ctx.de).contains(&id) {
        return missing_reason(&ctx.de);
    }
    apply_choice(io, ctx, &add_button(&shown_choice(io, ctx), id, side))
}

pub fn apply_border(io: &dyn LookIo, ctx: &LookCtx, value: &str) -> String {
    let ctx = &wm_ctx(io, ctx);
    finish(io, ctx, plan_border(io, ctx, value))
}

/// Restore the environment's default layout.
pub fn default_layout(de: &str) -> Layout {
    match de {
        "kde" => parse(Codec::Kde, "MSE", "HIAX"),
        "xfce" => parse(Codec::Xfwm, "O|SHMC", ""),
        "gnome" | "pantheon" => parse(Codec::Gnome, "appmenu:close", ""),
        "cinnamon-mint" => parse(Codec::Gnome, ":minimize,maximize,close", ""),
        "mate" => parse(Codec::Gnome, "menu:minimize,maximize,close", ""),
        "budgie" => parse(Codec::Gnome, "appmenu:minimize,maximize,close", ""),
        "cosmic" => Layout { left: Vec::new(), right: vec![Btn::Minimize, Btn::Maximize, Btn::Close] },
        // Defaults of the managers themselves: Openbox — `NLIMC` (its system
        // rc.xml), labwc — `icon:iconify,max,close` (labwc-config(5)).
        "openbox" => parse(Codec::Openbox, "NLIMC", ""),
        "labwc" => parse(Codec::Labwc, "icon:iconify,max,close", ""),
        _ => parse(Codec::Gnome, GTK_DEFAULT, ""),
    }
}

/// Returning the layout to the default: for gsettings and xfconf — by resetting the key
/// (the schema default with the distribution's adjustments, not our guess about it), for
/// KDE — by deleting the keys, for the rest — by writing the default layout.
pub fn reset_layout_steps(io: &dyn LookIo, ctx: &LookCtx) -> Vec<LookStep> {
    let de = ctx.de.as_str();
    let reset = |schema: &str, key: &str| LookStep::Cmd(crate::animations::argv(&["gsettings", "reset", schema, key]));
    let default_gtk = gtk_layout(&default_layout(de));
    match de {
        "kde" => {
            let mut steps = Vec::new();
            for key in ["ButtonsOnLeft", "ButtonsOnRight"] {
                if kconfig_value(io, ctx, "kwinrc", "org.kde.kdecoration2", key).is_some() {
                    steps.push(kwrite("kwinrc", "org.kde.kdecoration2", key, None, false));
                }
            }
            if steps.is_empty() {
                return steps;
            }
            steps.push(kwin_reconfigure());
            steps.push(LookStep::GtkIni { key: "gtk-decoration-layout".into(), value: default_gtk });
            steps
        }
        "gnome" | "budgie" | "pantheon" | "cinnamon-mint" | "mate" => {
            let (schema, key) = gsettings_key(de);
            let mut steps = vec![reset(schema, key)];
            if de == "budgie" {
                steps.push(reset("org.gnome.desktop.wm.preferences", "button-layout"));
            }
            if de == "mate" {
                steps.push(reset("org.mate.interface", "gtk-decoration-layout"));
            }
            steps
        }
        "xfce" => {
            if xfconf_get(io, "xfwm4", "/general/button_layout").is_none() {
                return Vec::new();
            }
            vec![LookStep::Cmd(crate::animations::argv(&["xfconf-query", "-c", "xfwm4", "-p", "/general/button_layout", "-r"]))]
        }
        _ => plan_layout(io, ctx, &default_layout(de)),
    }
}

pub fn apply_defaults(io: &dyn LookIo, ctx: &LookCtx) -> String {
    let ctx = &wm_ctx(io, ctx);
    if let Some(why) = rc_refusal(io, ctx) {
        return why;
    }
    let mut steps = reset_layout_steps(io, ctx);
    steps.extend(reset_flags_steps(io, ctx));
    let border = if ctx.de == "kde" { "auto" } else { "default" };
    if border_supported(&ctx.de) {
        steps.extend(plan_border(io, ctx, border));
    }
    finish(io, ctx, steps)
}

fn finish(io: &dyn LookIo, ctx: &LookCtx, steps: Vec<LookStep>) -> String {
    if steps.is_empty() {
        return String::new();
    }
    match run_steps(io, ctx, &steps) {
        Ok(()) => tr("Title bar settings applied."),
        Err(e) => e,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn defaults_roundtrip_unchanged() {
        for (codec, l, r) in [
            (Codec::Kde, "MSE", "HIAX"),
            (Codec::Kde, "", "IAX"),
            (Codec::Kde, "XA", "MI"),
            (Codec::Gnome, "appmenu:close", ""),
            (Codec::Gnome, ":minimize,maximize,close", ""),
            (Codec::Gnome, "menu:minimize,maximize,close", ""),
            (Codec::Gnome, "close,minimize,maximize:", ""),
            (Codec::Gnome, "close:maximize", ""),
            (Codec::Gnome, "close,minimize:maximize,menu", ""),
            (Codec::Xfwm, "O|SHMC", ""),
            (Codec::Xfwm, "CH|MO", ""),
        ] {
            let layout = parse(codec, l, r);
            let rebuilt = build(&choice_of(&layout), &layout);
            assert_eq!(rebuilt, layout, "{l} {r}");
            assert_eq!(format(codec, &rebuilt), if codec == Codec::Kde { (l.into(), r.into()) } else { (l.into(), String::new()) });
        }
    }

    /// Pantheon `close:maximize`: both sides are read as they are, and a menu
    /// put on the left stays on the left (it used to move to the right).
    #[test]
    fn two_sided_layout_is_kept() {
        let p = parse(Codec::Gnome, "close:maximize", "");
        assert_eq!(choice_of(&p), Choice { left: ids(&["close"]), right: ids(&["maximize"]) });
        let wanted = Choice { left: ids(&["close", "minimize", "maximize", "menu"]), right: vec![] };
        assert_eq!(format(Codec::Gnome, &build(&wanted, &p)).0, "close,minimize,maximize,menu:");
        // Buttons on both sides in any combination.
        let both = Choice { left: ids(&["close", "minimize"]), right: ids(&["maximize", "menu"]) };
        assert_eq!(format(Codec::Gnome, &build(&both, &p)).0, "close,minimize:maximize,menu");
        assert_eq!(gtk_layout(&build(&both, &p)), "close,minimize:maximize,menu");
    }

    #[test]
    fn two_sided_layout_in_every_codec() {
        let both = Choice { left: ids(&["close", "minimize"]), right: ids(&["maximize", "menu"]) };
        // KDE: ButtonsOnLeft / ButtonsOnRight; S, E and H are managed now —
        // not chosen means removed.
        let kde = build(&both, &parse(Codec::Kde, "MSE", "HIAX"));
        assert_eq!(format(Codec::Kde, &kde), ("XI".into(), "AM".into()));
        // GNOME / Cinnamon / MATE / Budgie / Pantheon: «a,b:c,d».
        let g = build(&both, &parse(Codec::Gnome, ":minimize,maximize,close", ""));
        assert_eq!(format(Codec::Gnome, &g).0, "close,minimize:maximize,menu");
        let mate = build(&Choice { left: ids(&["close", "sticky"]), right: ids(&["above"]) }, &parse(Codec::Gnome, "menu:minimize,maximize,close", ""));
        assert_eq!(format(Codec::Gnome, &mate).0, "close,stick:above");
        // XFCE: «ab|cd»; S («Roll up into title bar») is managed — not chosen.
        let x = build(&both, &parse(Codec::Xfwm, "O|SHMC", ""));
        assert_eq!(format(Codec::Xfwm, &x).0, "CH|MO");
    }

    #[test]
    fn presets_and_toggle() {
        let all = ids(&["menu", "minimize", "maximize", "close"]);
        assert_eq!(preset("right", &all), Choice { left: ids(&["menu"]), right: ids(&["minimize", "maximize", "close"]) });
        assert_eq!(preset("left", &all), Choice { left: ids(&["close", "minimize", "maximize"]), right: ids(&["menu"]) });
        // Putting the menu on the left in Pantheon: it does not appear on the right.
        let c = Choice { left: ids(&["close"]), right: ids(&["maximize"]) };
        let c = toggle(&c, "left", "menu", true);
        assert_eq!(c, Choice { left: ids(&["close", "menu"]), right: ids(&["maximize"]) });
        // «Minimize» on the left goes between «Close» and the menu, in order.
        let c = toggle(&c, "left", "minimize", true);
        assert_eq!(c.left, ids(&["close", "minimize", "menu"]));
        // Moving to the other side: it leaves the left and takes its place on the right at the edge.
        let c = toggle(&c, "right", "close", true);
        assert_eq!(c, Choice { left: ids(&["minimize", "menu"]), right: ids(&["maximize", "close"]) });
        // Removal.
        let c = toggle(&c, "right", "maximize", false);
        assert_eq!(c.right, ids(&["close"]));
    }

    /// A machine with one gsettings key — the button layout.
    struct GsIo(std::cell::RefCell<String>);

    impl LookIo for GsIo {
        fn run(&self, a: &[String]) -> (bool, String) {
            match (a[0].as_str(), a[1].as_str()) {
                ("gsettings", "get") => (true, format!("'{}'\n", self.0.borrow())),
                ("gsettings", "set") => {
                    *self.0.borrow_mut() = a[4].trim_matches('\'').to_string();
                    (true, String::new())
                }
                _ => (true, String::new()),
            }
        }
        fn have(&self, _: &str) -> bool {
            true
        }
        fn read(&self, _: &str) -> Option<String> {
            None
        }
        fn write(&self, _: &str, _: &str) -> bool {
            true
        }
        fn list_dir(&self, _: &str) -> Vec<String> {
            Vec::new()
        }
    }

    /// The live Pantheon path: the snapshot shows both sides, and the "menu
    /// on the left" check box writes `close,menu:maximize` without touching the right side.
    #[test]
    fn pantheon_state_and_toggle_keep_both_sides() {
        let io = GsIo(std::cell::RefCell::new("close:maximize".into()));
        let ctx = LookCtx::new("pantheon", "/home/t", None);
        let st = read_state(&io, &ctx);
        assert_eq!((st.left.clone(), st.right.clone()), (ids(&["close"]), ids(&["maximize"])));
        assert_eq!(st.side, "left");
        assert_eq!(st.custom, "", "раскладка передаётся выбором дословно");
        apply_toggle(&io, &ctx, "left", "menu", true);
        assert_eq!(*io.0.borrow(), "close,menu:maximize");
        apply_toggle(&io, &ctx, "right", "minimize", true);
        assert_eq!(*io.0.borrow(), "close,menu:minimize,maximize");
        // The ready-made "as in Windows" layout still gathers everything on the right.
        apply_preset(&io, &ctx, "right", &st.selected);
        assert_eq!(*io.0.borrow(), ":maximize,close");
    }

    #[test]
    fn mac_style_keeps_foreign_buttons() {
        // KDE by default: help next to the control buttons, the menu, «On all
        // desktops» and «Hide from screenshots…» — opposite; there and back without loss.
        let kde = parse(Codec::Kde, "MSE", "HIAX");
        let selected = choice_of(&kde).all();
        let mac = build(&preset("left", &selected), &kde);
        assert_eq!(format(Codec::Kde, &mac), ("XIAH".into(), "MSE".into()));
        assert_eq!(gtk_layout(&mac), "close,minimize,maximize:menu");
        assert_eq!(format(Codec::Kde, &build(&preset("right", &selected), &mac)), ("MSE".into(), "HIAX".into()));
        // A foreign button (GTK `icon`) stays on its side.
        let gnome = parse(Codec::Gnome, "icon:close", "");
        assert_eq!(format(Codec::Gnome, &build(&preset("left", &ids(&["close", "minimize"])), &gnome)).0, "icon,close,minimize:");
        // GNOME does not keep `appmenu` in its set — and the snapshot does not
        // lose it: it stays in front, where it stood.
        let io = GsIo(std::cell::RefCell::new("appmenu:close".into()));
        let ctx = LookCtx::new("gnome", "/home/t", None);
        apply_preset(&io, &ctx, "left", &ids(&["close", "minimize"]));
        assert_eq!(*io.0.borrow(), "appmenu,close,minimize:");
        // xfwm4: «Roll up into title bar» — next to the control buttons, as in `O|SHMC`.
        let x = parse(Codec::Xfwm, "O|SHMC", "");
        let all = choice_of(&x).all();
        assert_eq!(format(Codec::Xfwm, &build(&preset("right", &all), &x)).0, "O|SHMC");
        assert_eq!(format(Codec::Xfwm, &build(&preset("left", &all), &x)).0, "CHMS|O");
    }

    /// All buttons of the KDE module (11, with the spacer) survive parsing and building;
    /// a spacer can be on both sides, and a check box touches only its own.
    #[test]
    fn all_kde_buttons_and_spacer() {
        let kde = parse(Codec::Kde, "MN_SE", "HB_FIAX");
        let c = choice_of(&kde);
        assert_eq!(c.left, ids(&["menu", "appmenu", "spacer", "sticky", "exclude_capture"]));
        assert_eq!(c.right, ids(&["help", "below", "spacer", "above", "minimize", "maximize", "close"]));
        assert_eq!(build(&c, &kde), kde);
        assert_eq!(format(Codec::Kde, &build(&c, &kde)), ("MN_SE".into(), "HB_FIAX".into()));
        let off = toggle(&c, "left", "spacer", false);
        assert_eq!(off.left, ids(&["menu", "appmenu", "sticky", "exclude_capture"]));
        assert!(off.right.contains(&"spacer".to_string()), "справа промежуток остался");
        let on = toggle(&off, "left", "spacer", true);
        assert_eq!(on.left.last().map(String::as_str), Some("spacer"));
        // Help goes next to the control buttons, «Keep below others» — among the "pins".
        let c = toggle(&Choice { left: vec![], right: ids(&["minimize", "maximize", "close"]) }, "right", "help", true);
        assert_eq!(c.right, ids(&["help", "minimize", "maximize", "close"]));
        let c = toggle(&Choice { left: ids(&["menu", "sticky"]), right: vec![] }, "left", "below", true);
        assert_eq!(c.left, ids(&["menu", "sticky", "below"]));
    }

    /// Buttons that the environment's record lacks do not get into it: for GNOME —
    /// help, «Keep below others», «Hide from screenshots…»; for xfwm4 — also the spacer; for
    /// KDE — «Roll up into title bar» (KWin has no letter for it).
    #[test]
    fn codecs_skip_buttons_they_lack() {
        let l = Layout { left: vec![Btn::Help, Btn::Menu, Btn::Shade], right: vec![Btn::Below, Btn::Spacer, Btn::ExcludeCapture, Btn::Close] };
        assert_eq!(format(Codec::Gnome, &l).0, "menu,shade:spacer,close");
        assert_eq!(format(Codec::Xfwm, &l).0, "OS|C");
        assert_eq!(format(Codec::Kde, &l), ("HM".into(), "B_EX".into()));
        assert_eq!(parse(Codec::Gnome, "appmenu,spacer:shade,above,stick", "").left, vec![Btn::AppMenu, Btn::Spacer]);
        assert_eq!(parse(Codec::Xfwm, "OS|T", "").left, vec![Btn::Menu, Btn::Shade]);
        assert_eq!(format(Codec::Openbox, &l).0, "NSLC");
        assert_eq!(format(Codec::Labwc, &l).0, "menu,shade:close");
    }

    /// Openbox: the letters `NDSLIMC` (L — the title between the sides), labwc —
    /// the labwc-config(5) words separated by `:`; defaults survive parsing and
    /// building unchanged, a foreign one (`icon`) stays in place.
    #[test]
    fn openbox_and_labwc_codecs() {
        for (codec, raw) in [
            (Codec::Openbox, "NLIMC"),
            (Codec::Openbox, "CMIL"),
            (Codec::Openbox, "NDSLIMC"),
            (Codec::Labwc, "icon:iconify,max,close"),
            (Codec::Labwc, "close,max,iconify:menu,desk,shade"),
        ] {
            let l = parse(codec, raw, "");
            assert_eq!(format(codec, &l).0, raw, "{raw}");
            assert_eq!(build(&choice_of(&l), &l), l, "{raw}");
        }
        let ob = parse(Codec::Openbox, "NDLSIMC", "");
        assert_eq!(ob.left, vec![Btn::Menu, Btn::Sticky]);
        assert_eq!(ob.right, vec![Btn::Shade, Btn::Minimize, Btn::Maximize, Btn::Close]);
        // Without `L` Openbox puts everything on the left.
        assert_eq!(parse(Codec::Openbox, "NIMC", "").right, Vec::<Btn>::new());
        let lw = parse(Codec::Labwc, "icon:iconify,max,close", "");
        assert_eq!(lw.left, vec![Btn::Other("icon".into())]);
        // "As in macOS": control buttons on the left, the menu — on the right.
        let mac = build(&preset("left", &ids(&["menu", "minimize", "maximize", "close"])), &parse(Codec::Openbox, "NLIMC", ""));
        assert_eq!(format(Codec::Openbox, &mac).0, "CIMLN");
        let mac = build(&preset("left", &ids(&["minimize", "maximize", "close"])), &lw);
        assert_eq!(format(Codec::Labwc, &mac).0, "icon,close,iconify,max:");
        assert_eq!(default_layout("openbox"), parse(Codec::Openbox, "NLIMC", ""));
        assert_eq!(default_layout("labwc"), lw);
    }

    /// What is missing is named by name and with the window manager; for KDE — empty.
    #[test]
    fn unavailable_buttons_are_named() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        assert_eq!(unavailable_note("kde"), "");
        let mate = unavailable_note("mate");
        assert!(mate.contains("Marco") && mate.contains("Context help") && mate.contains("Keep below others"), "{mate}");
        assert!(!mate.contains("Spacer") && !mate.contains("Application menu"), "{mate}");
        let x = unavailable_note("xfce");
        assert!(x.contains("xfwm4") && x.contains("Spacer") && x.contains("Keep above others"), "{x}");
        assert!(unavailable_note("cinnamon-mint").contains("Muffin"));
        assert!(unavailable_note("gnome").contains("GTK"));
        // Each environment has only the buttons its record supports.
        let ob = unavailable_note("openbox");
        assert!(ob.contains("Openbox") && ob.contains("Keep above others") && !ob.contains("Roll up"), "{ob}");
        assert!(unavailable_note("labwc").starts_with("labwc"));
        for de in ["kde", "mate", "xfce", "cinnamon-mint", "gnome", "cosmic", "sway", "openbox", "labwc"] {
            for id in supported_buttons(de) {
                assert!(MANAGED.iter().any(|(m, _)| *m == id), "{de}: {id}");
            }
        }
    }

    /// Deepin: deepin-kwin frames do not change buttons — the section label says exactly
    /// that, rather than claiming that Deepin has no title bars at all.
    #[test]
    fn deepin_note_is_honest() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let n = note("deepin");
        assert!(n.contains("Deepin's window frames") && !n.contains("draws no title bars"), "{n}");
    }

    /// A file-backed `LookIo`: what is written is what is read; commands go to the log.
    #[derive(Default)]
    struct FileIo {
        files: std::cell::RefCell<BTreeMap<String, String>>,
        ran: std::cell::RefCell<Vec<Vec<String>>>,
    }

    impl LookIo for FileIo {
        fn run(&self, a: &[String]) -> (bool, String) {
            self.ran.borrow_mut().push(a.to_vec());
            (true, String::new())
        }
        fn have(&self, _: &str) -> bool {
            true
        }
        fn read(&self, p: &str) -> Option<String> {
            self.files.borrow().get(p).cloned()
        }
        fn write(&self, p: &str, t: &str) -> bool {
            self.files.borrow_mut().insert(p.to_string(), t.to_string());
            true
        }
        fn list_dir(&self, _: &str) -> Vec<String> {
            Vec::new()
        }
    }

    /// sway: there is a border (`default_border <style> N`, sway(5)); the style from
    /// the person's config is kept, open windows get it by a command.
    #[test]
    fn sway_border_keeps_the_title_bar_style() {
        assert!(border_supported("sway"));
        assert_eq!(sway_border_styles(None), ("normal".into(), "normal".into()));
        assert_eq!(
            sway_border_styles(Some("# default_border pixel 1\ndefault_border none\ndefault_floating_border normal 3\n")),
            ("pixel".into(), "normal".into())
        );
        let io = FileIo::default();
        let ctx = LookCtx::new("sway", "/h", Some("/c"));
        io.write("/c/sway/config", "default_border pixel 2\n");
        assert_eq!(apply_border(&io, &ctx, "4"), tr("Title bar settings applied."));
        let own = io.read("/c/sway/rexon-borders.conf").unwrap();
        assert!(own.contains("default_border pixel 4\ndefault_floating_border normal 4\n"), "{own}");
        assert!(io.read("/c/sway/config").unwrap().contains("include /c/sway/rexon-borders.conf"));
        assert_eq!(read_border(&io, &ctx), "4");
        {
            let ran = io.ran.borrow();
            let has = |c: &str| ran.contains(&vec!["swaymsg".to_string(), c.to_string()]);
            // `swaymsg reload` restarts sway's own swaybg, and its background
            // lies ON TOP of Rexon's wallpaper (checked on sway 1.12 headless):
            // new windows get the border by commands, without re-reading.
            assert!(!has("reload"), "{ran:?}");
            assert!(has("default_border pixel 4"), "{ran:?}");
            assert!(has("default_floating_border normal 4"), "{ran:?}");
            assert!(has("[tiling] border pixel 4"), "{ran:?}");
        }
        // "As in the config" returns the values from the person's config (sway's
        // default is `normal 2`), also without re-reading.
        io.ran.borrow_mut().clear();
        assert_eq!(apply_border(&io, &ctx, "default"), tr("Title bar settings applied."));
        let ran = io.ran.borrow();
        let has = |c: &str| ran.contains(&vec!["swaymsg".to_string(), c.to_string()]);
        assert!(!has("reload"), "{ran:?}");
        assert!(has("default_border pixel 2"), "{ran:?}");
        assert!(has("default_floating_border normal 2"), "{ran:?}");
        assert_eq!(
            sway_border_defaults(Some("default_border none\ndefault_floating_border pixel 3\n")),
            ("none".into(), "pixel 3".into())
        );
        assert_eq!(border_note("sway"), tr("Only the width changes: whether windows have a title bar stays as set in the sway config."));
    }

    /// sway does NOT create its own config (unlike niri and Hyprland): without
    /// a personal one it reads `/etc/sway/config`. Previously the border answered "start
    /// sway once so that it creates its config" — advice after which
    /// nothing changes. Now the system one is copied in place of the personal one (as
    /// sway(5) advises), and the border is set. sway also reads the i3 config — we
    /// do not shadow it.
    #[test]
    fn sway_without_own_config_seeds_it_from_system() {
        let io = FileIo::default();
        let ctx = LookCtx::new("sway", "/h", Some("/c"));
        io.write("/etc/sway/config", "set $mod Mod4\nbindsym $mod+q kill\n");
        assert_eq!(apply_border(&io, &ctx, "4"), tr("Title bar settings applied."));
        let cfg = io.read("/c/sway/config").unwrap();
        assert!(cfg.starts_with("set $mod Mod4\nbindsym $mod+q kill\n"), "{cfg}");
        assert!(cfg.contains("include /c/sway/rexon-borders.conf"), "{cfg}");
        assert_eq!(io.read("/etc/sway/config").unwrap(), "set $mod Mod4\nbindsym $mod+q kill\n");

        let io = FileIo::default();
        io.write("/etc/sway/config", "set $mod Mod4\n");
        io.write("/c/i3/config", "bindsym Mod4+q kill\n");
        assert_ne!(apply_border(&io, &ctx, "4"), tr("Title bar settings applied."));
        assert!(io.read("/c/sway/config").is_none(), "конфиг i3 заслонён");
    }

    /// labwc: `border.width` in `themerc-override` (labwc-theme(5)); other
    /// lines of the file are not touched, "as in the config" removes only our key.
    #[test]
    fn labwc_border_edits_themerc_override() {
        let io = FileIo::default();
        let ctx = LookCtx::new("labwc", "/h", Some("/c"));
        let path = "/c/labwc/themerc-override";
        io.write(path, "# мои правки\nwindow.active.title.bg.color: #222222\nborder.width: 1\n");
        assert_eq!(read_border(&io, &ctx), "1");
        apply_border(&io, &ctx, "3");
        assert_eq!(io.read(path).unwrap(), "# мои правки\nwindow.active.title.bg.color: #222222\nborder.width: 3\n");
        assert!(io.ran.borrow().contains(&vec!["labwc".to_string(), "--reconfigure".to_string()]));
        apply_border(&io, &ctx, "default");
        assert_eq!(io.read(path).unwrap(), "# мои правки\nwindow.active.title.bg.color: #222222\n");
        assert_eq!(read_border(&io, &ctx), "default");
    }

    /// Labels without falsehood: sway has title bars (without buttons), labwc and
    /// LXQt draw title bars with buttons; "no title bars" — only where
    /// there really are none. In GNOME/Cinnamon the border is drawn by the GTK theme.
    #[test]
    fn title_bar_notes_are_honest() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        for de in ["sway", "labwc", "lxqt", "wayfire"] {
            assert!(!note(de).contains("draws no title bars"), "{de}: {}", note(de));
        }
        assert!(note("sway").contains("no buttons"));
        // labwc and Openbox write their own buttons — no caveat; LXQt with
        // an unknown manager names which ones it writes for.
        assert_eq!(note("labwc"), "");
        assert_eq!(note("openbox"), "");
        assert!(note("lxqt").contains("Openbox") && note("lxqt").contains("labwc"));
        for de in ["hyprland", "niri", "river"] {
            assert!(note(de).contains("draws no title bars"), "{de}");
        }
        for de in ["gnome", "pantheon", "cinnamon-mint"] {
            assert!(border_note(de).contains("GTK"), "{de}");
        }
        assert!(border_note("lxqt").contains("window manager"));
        assert!(!border_note("labwc").contains("has no window border setting"));
    }

    fn ch(l: &[&str], r: &[&str]) -> Choice {
        Choice { left: ids(l), right: ids(r) }
    }

    /// Drag and drop as in the KDE module: within a side, between sides, from
    /// the shelf and to the shelf; the position is the place in the side already without the button.
    #[test]
    fn move_button_like_kde_drag() {
        let kde = ch(&["menu", "sticky", "exclude_capture"], &["help", "minimize", "maximize", "close"]);
        // Within a side: «Close» to the start of the right one.
        assert_eq!(move_button(&kde, "close", "right", 3, "right", 0).right, ids(&["close", "help", "minimize", "maximize"]));
        // Forward along the side: «Window menu» after «On all desktops».
        assert_eq!(move_button(&kde, "menu", "left", 0, "left", 1).left, ids(&["sticky", "menu", "exclude_capture"]));
        // Between sides: «Context help» to the end of the left one.
        let c = move_button(&kde, "help", "right", 0, "left", 9);
        assert_eq!(c, ch(&["menu", "sticky", "exclude_capture", "help"], &["minimize", "maximize", "close"]));
        // To the shelf — remove.
        assert_eq!(move_button(&kde, "sticky", "left", 1, SHELF, 0).left, ids(&["menu", "exclude_capture"]));
        // From the shelf — a new button at the position.
        assert_eq!(move_button(&kde, "above", SHELF, 0, "right", 1).right, ids(&["help", "above", "minimize", "maximize", "close"]));
        // An already placed one from the shelf is moved, not duplicated.
        let c = move_button(&kde, "close", SHELF, 0, "left", 0);
        assert_eq!(c, ch(&["close", "menu", "sticky", "exclude_capture"], &["help", "minimize", "maximize"]));
        // A stale position — the first such button is taken.
        assert_eq!(move_button(&kde, "close", "right", 0, SHELF, 0).right, ids(&["help", "minimize", "maximize"]));
    }

    /// A spacer is not a button: from the shelf it is always new, to the shelf goes the one
    /// that was dragged.
    #[test]
    fn spacers_are_many() {
        let c = ch(&["menu", "spacer"], &["spacer", "close"]);
        let two = move_button(&c, "spacer", SHELF, 0, "right", 2);
        assert_eq!(two.right, ids(&["spacer", "close", "spacer"]));
        assert_eq!(two.left, ids(&["menu", "spacer"]));
        assert_eq!(move_button(&two, "spacer", "right", 2, SHELF, 0), c);
        assert_eq!(move_button(&c, "spacer", "left", 1, "right", 2), ch(&["menu"], &["spacer", "close", "spacer"]));
        assert_eq!(add_button(&c, "spacer", "left").left, ids(&["menu", "spacer", "spacer"]));
        assert_eq!(add_button(&c, "spacer", "right").right, ids(&["spacer", "spacer", "close"]));
    }

    /// Steps with buttons: across the title to the other side, at the window edge — stop.
    #[test]
    fn nudge_steps_across_the_title() {
        let c = ch(&["menu", "sticky"], &["minimize", "close"]);
        let (n, at) = nudge(&c, "left", 1, "right");
        assert_eq!(n, ch(&["menu"], &["sticky", "minimize", "close"]));
        assert_eq!(at, Some(("right".into(), 0)));
        let (n, at) = nudge(&c, "right", 0, "left");
        assert_eq!(n, ch(&["menu", "sticky", "minimize"], &["close"]));
        assert_eq!(at, Some(("left".into(), 2)));
        let (n, at) = nudge(&c, "left", 0, "right");
        assert_eq!(n, ch(&["sticky", "menu"], &["minimize", "close"]));
        assert_eq!(at, Some(("left".into(), 1)));
        // Window edge.
        assert_eq!(nudge(&c, "left", 0, "left"), (c.clone(), Some(("left".into(), 0))));
        assert_eq!(nudge(&c, "right", 1, "right"), (c.clone(), Some(("right".into(), 1))));
        // To the shelf and past the list.
        assert_eq!(nudge(&c, "right", 1, SHELF), (ch(&["menu", "sticky"], &["minimize"]), None));
        assert_eq!(nudge(&c, "right", 7, "left"), (c.clone(), None));
    }

    /// Moving is reduced to the native strings of each window manager.
    #[test]
    fn moves_format_for_every_wm() {
        let c = move_button(&ch(&["menu"], &["minimize", "maximize", "close"]), "close", "right", 2, "left", 0);
        let layout = build(&c, &Layout::default());
        assert_eq!(format(Codec::Kde, &layout), ("XM".to_string(), "IA".to_string()));
        assert_eq!(format(Codec::Gnome, &layout).0, "close,menu:minimize,maximize");
        assert_eq!(format(Codec::Xfwm, &layout).0, "CO|HM");
        assert_eq!(format(Codec::Openbox, &layout).0, "CNLIM");
        assert_eq!(format(Codec::Labwc, &layout).0, "close,menu:iconify,max");
    }

    /// Shelf: all 11 KDE buttons; what the environment lacks is unavailable with
    /// a reason; placed ones are marked, the spacer — never.
    #[test]
    fn shelf_marks_missing_and_placed() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let c = ch(&["menu", "spacer"], &["close"]);
        let kde = shelf_of("kde", &c);
        assert_eq!(kde.len(), 11);
        assert!(kde.iter().all(|b| b.available && !b.glyph.is_empty() || b.id == "spacer"));
        assert!(kde.iter().find(|b| b.id == "menu").unwrap().placed);
        assert!(!kde.iter().find(|b| b.id == "spacer").unwrap().placed);
        let xfce = shelf_of("xfce", &c);
        assert_eq!(xfce.len(), 12, "сверх KDE — «свернуть в заголовок»");
        let help = xfce.iter().find(|b| b.id == "help").unwrap();
        assert!(!help.available && help.reason.contains("xfwm4"), "{help:?}");
        assert!(shelf_of("gnome", &c).iter().find(|b| b.id == "sticky").unwrap().reason.contains("GTK"));
    }
}
