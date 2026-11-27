//! "MATE panel": ready-made panel layouts (`mate-panel`) with one button — "Like
//! Windows", classic, as in Mint, Fedora, Ubuntu MATE and so on.
//!
//! All the logic is here, not tied to a tab: the list with labels, marking the
//! current one, preparing and applying, restoring the previous panel. The tab
//! ("Settings" now, "Appearance" later) only shows the [`read_state`] snapshot
//! and calls [`apply`] / [`restore`].
//!
//! How mate-panel handles layouts (mate-panel 1.28, the `--layout` and `--reset`
//! options from `mate-panel --help`; behavior measured on an Xvfb test stand on
//! 16.10.2026):
//!
//! * a layout is a `<name>.layout` file in key-file format: `[Toplevel <id>]`
//!   groups (panels) and `[Object <id>]` groups (applets, launchers, menus). It
//!   is looked for ONLY in mate-panel's own directory
//!   (`/usr/share/mate-panel/layouts`); mate-panel has no user directory. On the
//!   other hand, the `org.mate.panel default-layout` key also accepts an absolute
//!   path — that is what we use for our fixed copy;
//! * `mate-panel --layout <name|path> --reset` writes `default-layout` and
//!   empties the lists of panels and objects. The running panel notices the
//!   empty list by itself, removes the old panels and builds new ones from the
//!   layout — no restart (`--replace`) is needed. The applets, launchers and
//!   settings of the previous panel are lost in the process — hence the
//!   snapshot before applying;
//! * Warning: a group with an unknown key is dropped ENTIRELY. Layouts from
//!   ubuntu-mate-settings (redmond, familiar, eleven…) write
//!   `relative-to-edge=end`, which mate-panel 1.28 does not know: without a fix
//!   "Like Windows" lost its launchers, the clock and the notification area;
//! * Warning: an applet that is not on the system (the Brisk menu, indicators)
//!   is greeted by the panel with a "There was a problem loading the applet…"
//!   dialog — the person got a panel without a menu and an error dialog.
//!
//! So before applying, the layout is PREPARED ([`prepare`]):
//! `relative-to-edge=end` → `panel-right-stick=true`, unknown keys and groups
//! are removed, a missing menu → MATE's built-in menu button, missing
//! indicators → the clock, other missing applets and launchers of programs that
//! are not installed are removed. If there is nothing to fix, the system layout
//! itself is applied by name; otherwise — the fixed copy
//! `~/.local/share/mate-panel/layouts/rexon-<name>.layout` by path.
//!
//! Restoring: before applying, Rexon takes `dconf dump /org/mate/panel/`. The
//! person's original panel is the snapshot before the FIRST apply, in
//! `~/.local/share/rexon/backups/mate-panel.dconf`; subsequent applies do not
//! touch it and put their own snapshot next to it (`….last`) — the copy rule
//! `backup::panel_snapshot_slot`. "Restore the previous panel" loads the
//! original one ([`restore`]) — the running panel picks this up too without a
//! restart (measured on the stand); after a restore, the next apply takes the
//! original snapshot again.

use std::collections::BTreeSet;

use serde::Serialize;

use crate::animations::{argv, gsettings_get, LookIo};
use crate::i18n::{n_, tr, trf};

/// mate-panel's own layout directory.
pub const SYSTEM_LAYOUTS_DIR: &str = "/usr/share/mate-panel/layouts";
/// Default directory of applet descriptions (overridden by
/// `MATE_PANEL_APPLETS_DIR`, a colon-separated list).
pub const SYSTEM_APPLETS_DIR: &str = "/usr/share/mate-panel/applets";
/// The dconf branch with the panel settings.
pub const PANEL_DCONF_DIR: &str = "/org/mate/panel/";
/// Prefix of our fixed copies: a copy is recognized by it and does not get into
/// the list a second time.
const COPY_PREFIX: &str = "rexon-";

/// Panel keys that mate-panel 1.28 accepts in a layout.
const TOPLEVEL_KEYS: &[&str] = &[
    "name",
    "screen",
    "monitor",
    "expand",
    "orientation",
    "size",
    "x",
    "y",
    "x-right",
    "y-bottom",
    "x-centered",
    "y-centered",
    "auto-hide",
    "enable-animations",
    "enable-buttons",
    "enable-arrows",
    "hide-delay",
    "unhide-delay",
    "auto-hide-size",
    "animation-speed",
];

/// Panel object keys that mate-panel 1.28 accepts in a layout.
const OBJECT_KEYS: &[&str] = &[
    "object-type",
    "toplevel-id",
    "position",
    "panel-right-stick",
    "locked",
    "applet-iid",
    "attached-toplevel-id",
    "tooltip",
    "use-custom-icon",
    "custom-icon",
    "use-menu-path",
    "menu-path",
    "has-arrow",
    "launcher-location",
    "action-type",
];

/// The clock — missing indicators are replaced with it (in Ubuntu MATE layouts
/// the clock lives inside the indicators).
const CLOCK_IID: &str = "ClockAppletFactory::ClockApplet";

/// The MATE Menu applet (package `mate-menu`, the Ubuntu MATE menu).
pub const MATE_MENU_IID: &str = "MateMenuAppletFactory::MateMenuApplet";

/// The "show desktop" button of mate-panel itself.
const SHOW_DESKTOP_IID: &str = "WnckletFactory::ShowDesktopApplet";

/// The caption next to the MATE Menu icon on the panel: `gsettings set
/// org.mate.mate-menu applet-text <text>` (a word, not a logo).
pub fn mate_menu_label_argv(text: &str) -> Vec<String> {
    argv(&["gsettings", "set", "org.mate.mate-menu", "applet-text", text])
}

/// The Brisk menu. Rexon no longer installs it (Wehrwolfmann's decision of
/// 24.11.2026: the project is abandoned, Solus dropped it): in a layout and on
/// the live panel it is always replaced with the MATE Menu, or with MATE's
/// built-in menu button when `mate-menu` is not on the system — never with
/// nothing ([`prepare`], [`brisk_migration`]).
pub const BRISK_MENU_IID: &str = "BriskMenuFactory::BriskMenu";

/// Third-party menus: if they are missing, MATE's built-in menu button
/// (`object-type=menu`), which always exists, takes their place.
const MENU_IIDS: &[&str] = &[
    "MintMenuAppletFactory::MintMenuApplet",
    "MateMenuAppletFactory::MateMenuApplet",
];

/// Indicators (clock, sound, network in one applet).
const INDICATOR_IIDS: &[&str] = &[
    "IndicatorAppletCompleteFactory::IndicatorAppletComplete",
    "IndicatorAppletFactory::IndicatorApplet",
];

/// Human-readable applet names — for the "will be instead" and "will not be
/// there" notes. An unknown applet is called by its own name from `applet-iid`.
const APPLET_LABELS: &[(&str, &str)] = &[
    (BRISK_MENU_IID, n_("Brisk menu")),
    ("MintMenuAppletFactory::MintMenuApplet", n_("Mint menu")),
    ("MateMenuAppletFactory::MateMenuApplet", n_("MATE Menu applet")),
    ("IndicatorAppletCompleteFactory::IndicatorAppletComplete", n_("indicators")),
    ("IndicatorAppletFactory::IndicatorApplet", n_("indicators")),
    ("AppmenuAppletFactory::AppmenuApplet", n_("global app menu")),
    ("WindowButtonsAppletFactory::WindowButtonsApplet", n_("window buttons on the panel")),
    ("MateWindowPickerFactory::MateWindowPicker", n_("window picker")),
    ("StickyNotesAppletFactory::StickyNotesApplet", n_("sticky notes")),
    ("GvcAppletFactory::GvcApplet", n_("volume control")),
    ("TrashAppletFactory::TrashApplet", n_("trash")),
    ("DriveMountAppletFactory::DriveMountApplet", n_("drive mounter")),
    ("WnckletFactory::WorkspaceSwitcherApplet", n_("workspace switcher")),
    ("WnckletFactory::WindowListApplet", n_("window list")),
    ("WnckletFactory::ShowDesktopApplet", n_("show desktop button")),
    ("NotificationAreaAppletFactory::NotificationArea", n_("notification area")),
    (CLOCK_IID, n_("clock")),
];

/// Known layouts: file name, title and description. The order is the display
/// order ("Like Windows" first: that is what people come here for). The
/// descriptions were written from the contents of the files; an unknown layout
/// gets a description from its composition ([`auto_description`]).
const KNOWN_LAYOUTS: &[(&str, &str, &str)] = &[
    (
        "redmond",
        n_("Like Windows"),
        n_("One panel at the bottom: menu on the left, open windows in the middle, \
            notification area and clock on the right."),
    ),
    (
        "default",
        n_("Classic MATE"),
        n_("Two panels, as in GNOME 2: Applications, Places and System menus and the clock at \
            the top; open windows and workspaces at the bottom."),
    ),
    (
        "linuxmint",
        n_("Like Linux Mint"),
        n_("One panel at the bottom: Mint menu on the left, open windows in the middle, clock \
            on the right."),
    ),
    (
        "opensuse",
        n_("Like openSUSE"),
        n_("One panel at the bottom: menu and sticky notes on the left, open windows in the \
            middle, workspaces and clock on the right."),
    ),
    (
        "familiar",
        n_("Familiar"),
        n_("Two panels: menu, Firefox and indicators at the top; open windows, workspaces and \
            trash at the bottom."),
    ),
    (
        "ubuntu-mate",
        n_("Traditional Ubuntu MATE"),
        n_("Two panels: Applications, Places and System menus and indicators at the top; open \
            windows, workspaces and trash at the bottom."),
    ),
    (
        "ubuntu",
        n_("Like old Ubuntu"),
        n_("Two panels: menus, Firefox, clock and a shutdown button at the top; open windows, \
            workspaces and trash at the bottom."),
    ),
    (
        "fedora",
        n_("Like Fedora"),
        n_("Two panels: menus, file manager, terminal and Firefox buttons, volume and clock at \
            the top; open windows and workspaces at the bottom."),
    ),
    (
        "mageia",
        n_("Like Mageia"),
        n_("Two panels: menus, Mageia Control Center, terminal and Firefox at the top; open \
            windows and workspaces at the bottom."),
    ),
    (
        "contemporary",
        n_("Contemporary"),
        n_("Two panels: menu, the menu of the active app and indicators at the top; open \
            windows, workspaces and trash at the bottom."),
    ),
    (
        "eleven",
        n_("Like macOS"),
        n_("One panel at the top with the menu and the menu of the active app; apps are \
            launched from a dock at the bottom."),
    ),
    (
        "pantheon",
        n_("Like elementary OS"),
        n_("A thin panel at the top with the menu and indicators; apps are launched from a dock \
            at the bottom."),
    ),
    (
        "mutiny",
        n_("Like Unity"),
        n_("A panel at the top with the menu and indicators; apps are launched from a dock on \
            the left."),
    ),
    (
        "netbook",
        n_("Netbook"),
        n_("One panel at the top for small screens: menu, window picker, indicators and \
            window buttons."),
    ),
];

// ═══════════════════════════════════════════════════════════════════════════
// Missing layout parts: where to install them from
// ═══════════════════════════════════════════════════════════════════════════

/// Where a layout part is installed from in a distribution family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartSource {
    /// A package from the distribution's repositories.
    Repo(&'static str),
    /// AUR only (it gets built). `orphan` — the package has no maintainer.
    Aur { package: &'static str, orphan: bool },
    /// Nowhere to get it from.
    Missing,
}

/// A layout part (`applet-iid`) and its package per family
/// (`system::PkgFamily::as_str`: `arch`, `debian` — Ubuntu too, `fedora`,
/// `suse`).
pub struct PanelPart {
    pub iid: &'static str,
    pub arch: PartSource,
    pub debian: PartSource,
    pub fedora: PartSource,
    pub suse: PartSource,
}

/// Table of layout parts. Checked on 19.10.2026 against the repositories
/// themselves: archlinux.org and AUR RPC (`mate-indicator-applet` and
/// `mate-window-buttons-applet` have no maintainer in AUR), Debian trixie
/// (packages.debian.org), Ubuntu noble (packages.ubuntu.com), Fedora f43
/// (mdapi), openSUSE Factory (api.opensuse.org: the name of the built rpm).
/// Program launchers (Firefox, Evolution) are not layout parts, they are not
/// here. The layout does not start the Plank dock (Rexon does not enable it) —
/// it is not here either. The Brisk menu is not here on purpose: it is never
/// installed, the MATE Menu takes its place ([`BRISK_MENU_IID`]).
pub const PANEL_PARTS: &[PanelPart] = {
    use PartSource::{Aur, Missing, Repo};
    &[
        PanelPart {
            iid: "MintMenuAppletFactory::MintMenuApplet",
            arch: Aur { package: "mintmenu", orphan: false },
            debian: Missing,
            fedora: Missing,
            suse: Missing,
        },
        PanelPart {
            iid: "MateMenuAppletFactory::MateMenuApplet",
            arch: Aur { package: "mate-menu", orphan: false },
            debian: Repo("mate-menu"),
            fedora: Repo("mate-menu"),
            suse: Repo("mate-menu"),
        },
        PanelPart {
            iid: "IndicatorAppletCompleteFactory::IndicatorAppletComplete",
            arch: Aur { package: "mate-indicator-applet", orphan: true },
            debian: Repo("mate-indicator-applet"),
            fedora: Missing,
            suse: Repo("mate-applet-indicator"),
        },
        PanelPart {
            iid: "IndicatorAppletFactory::IndicatorApplet",
            arch: Aur { package: "mate-indicator-applet", orphan: true },
            debian: Repo("mate-indicator-applet"),
            fedora: Missing,
            suse: Repo("mate-applet-indicator"),
        },
        PanelPart {
            iid: "AppmenuAppletFactory::AppmenuApplet",
            arch: Aur { package: "vala-panel-appmenu-mate", orphan: false },
            debian: Repo("mate-applet-appmenu"),
            fedora: Missing,
            suse: Missing,
        },
        PanelPart {
            iid: "WindowButtonsAppletFactory::WindowButtonsApplet",
            arch: Aur { package: "mate-window-buttons-applet", orphan: true },
            debian: Repo("mate-window-buttons-applet"),
            fedora: Missing,
            suse: Missing,
        },
        PanelPart {
            iid: "MateWindowPickerFactory::MateWindowPicker",
            arch: Repo("mate-netbook"),
            debian: Repo("mate-netbook"),
            fedora: Missing,
            suse: Repo("mate-netbook"),
        },
        PanelPart {
            iid: "DriveMountAppletFactory::DriveMountApplet",
            arch: Repo("mate-applets"),
            debian: Repo("mate-applets"),
            fedora: Repo("mate-applets"),
            suse: Repo("mate-applets"),
        },
        PanelPart {
            iid: "StickyNotesAppletFactory::StickyNotesApplet",
            arch: Repo("mate-applets"),
            debian: Repo("mate-applets"),
            fedora: Repo("mate-applets"),
            suse: Repo("mate-applets"),
        },
        PanelPart {
            iid: "TrashAppletFactory::TrashApplet",
            arch: Repo("mate-applets"),
            debian: Repo("mate-applets"),
            fedora: Repo("mate-applets"),
            suse: Repo("mate-applets"),
        },
        PanelPart {
            iid: "GvcAppletFactory::GvcApplet",
            arch: Repo("mate-media"),
            debian: Repo("mate-media"),
            fedora: Repo("mate-media"),
            suse: Repo("mate-media"),
        },
    ]
};

/// Where to install part `iid` from in family `family`.
pub fn part_source(iid: &str, family: &str) -> PartSource {
    let Some(part) = PANEL_PARTS.iter().find(|p| p.iid == iid) else {
        return PartSource::Missing;
    };
    match family {
        "arch" => part.arch,
        "debian" => part.debian,
        "fedora" => part.fedora,
        "suse" => part.suse,
        _ => PartSource::Missing,
    }
}

/// All packages of layout parts for a family — both from the repositories and
/// those marked "AUR only" (on CachyOS some of them are in a repository, and
/// then the package helper installs them). The helper's install allow-list
/// (`system_plan::pkg_helper_install_allowlist`).
pub fn all_part_packages(family: &str) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for part in PANEL_PARTS {
        let pkg = match part_source(part.iid, family) {
            PartSource::Repo(p) => p,
            PartSource::Aur { package, .. } => package,
            PartSource::Missing => continue,
        };
        if !out.contains(&pkg) {
            out.push(pkg);
        }
    }
    out
}

/// Applets of layout `text` that are not on the system (`installed` —
/// [`installed_applets`]), without duplicates, in file order.
pub fn missing_parts(text: &str, installed: &BTreeSet<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for g in parse_keyfile(text) {
        if !g.is_object() || g.get("object-type") != Some("applet") {
            continue;
        }
        let mut iid = g.get("applet-iid").unwrap_or("").to_string();
        // The Brisk menu is not installed any more: what the layout needs in its
        // place is the MATE Menu (and without it the built-in menu button).
        if iid == BRISK_MENU_IID {
            iid = MATE_MENU_IID.to_string();
        }
        if !iid.is_empty() && !installed.contains(&iid) && !out.contains(&iid) {
            out.push(iid);
        }
    }
    out
}

/// What of the missing parts can be installed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PartsOffer {
    /// Repository packages (the administrator password dialog).
    pub repo: Vec<String>,
    /// AUR packages: name and "no maintainer".
    pub aur: Vec<(String, bool)>,
    /// What of the layout they will bring — human-readable names.
    pub labels: Vec<String>,
    /// Those of `labels` that only AUR packages bring.
    pub aur_labels: Vec<String>,
}

impl PartsOffer {
    pub fn is_empty(&self) -> bool {
        self.repo.is_empty() && self.aur.is_empty()
    }
}

/// What of the missing `missing` can be installed in family `family`. An empty
/// family — Rexon does not install packages here (SteamOS, an immutable system,
/// an unknown package manager).
pub fn parts_offer(missing: &[String], family: &str) -> PartsOffer {
    let mut offer = PartsOffer::default();
    for iid in missing {
        let label = applet_label(iid);
        let added = match part_source(iid, family) {
            PartSource::Repo(p) => {
                if !offer.repo.iter().any(|x| x == p) {
                    offer.repo.push(p.to_string());
                }
                true
            }
            PartSource::Aur { package, orphan } => {
                if !offer.aur.iter().any(|(x, _)| x == package) {
                    offer.aur.push((package.to_string(), orphan));
                }
                if !offer.aur_labels.contains(&label) {
                    offer.aur_labels.push(label.clone());
                }
                true
            }
            PartSource::Missing => false,
        };
        if added && !offer.labels.contains(&label) {
            offer.labels.push(label);
        }
    }
    offer
}

impl PartsOffer {
    /// What Rexon installs BY ITSELF, without a question (the owner's decision of
    /// 24.10; the protection is the polkit password dialog): packages from the
    /// official repositories. Building from AUR without a question is not allowed —
    /// that stays with [`parts_question`].
    pub fn repo_only(&self) -> PartsOffer {
        let labels = self.labels.iter().filter(|l| !self.aur_labels.contains(l)).cloned().collect();
        PartsOffer { repo: self.repo.clone(), aur: Vec::new(), labels, aur_labels: Vec::new() }
    }
}

/// The question before applying layout `id` that lacks parts: (title, text).
/// It is asked only about building from AUR: repository packages are installed
/// without it. `None` — there is nothing to ask about.
pub fn parts_question(id: &str, offer: &PartsOffer) -> Option<(String, String)> {
    if offer.aur.is_empty() {
        return None;
    }
    let mut parts = vec![trf(
        "The “%1” layout needs %2. Install them together with the layout?",
        &[&title_of(id), &offer.labels.join(", ")],
    )];
    if !offer.repo.is_empty() {
        parts.push(trf(
            "Packages: %1 (an administrator password may be requested).",
            &[&offer.repo.join(", ")],
        ));
    }
    if !offer.aur.is_empty() {
        let names: Vec<&str> = offer.aur.iter().map(|(n, _)| n.as_str()).collect();
        parts.push(trf(
            "Built from AUR: %1 — this takes a few minutes.",
            &[&names.join(", ")],
        ));
        let orphans: Vec<&str> = offer.aur.iter().filter(|(_, o)| *o).map(|(n, _)| n.as_str()).collect();
        if !orphans.is_empty() {
            parts.push(trf(
                "%1 has no maintainer in AUR: nobody keeps the package up to date, and it may fail to build.",
                &[&orphans.join(", ")],
            ));
        }
    }
    parts.push(tr("“No” — the parts from AUR are replaced, as described under the layout."));
    Some((tr("Install the missing parts?"), parts.join("\n\n")))
}

/// An offer for the live system: the family and the prohibitions — as for the
/// Conky package (`de_apply::live_conky_package`).
pub fn live_parts_offer(io: &dyn LookIo, ctx: &PanelCtx, id: &str) -> PartsOffer {
    let Some((_, path)) = list_layouts(io, ctx).into_iter().find(|(name, _)| name == id) else {
        return PartsOffer::default();
    };
    let Some(text) = io.read(&path) else {
        return PartsOffer::default();
    };
    let detector = crate::system::DistroDetector::detect_live();
    let family = if detector.is_steamos() || detector.is_immutable() {
        String::new()
    } else {
        detector.family_live().as_str().to_string()
    };
    let installed = installed_applets(io, &ctx.applet_dirs);
    parts_offer(&missing_parts(&text, &installed), &family)
}

/// Install part packages from the official repositories (password dialog) — the
/// same way as GTK theme icons and cursors; this path does not build from AUR.
pub fn live_install_repo_parts(packages: &[String]) -> Result<(), String> {
    crate::appearance_installer::live_install_repo_packages(packages, crate::privilege::op::PkgSource::MatePanel)
}

/// Apply refusal: the parts being installed did not install ("all or nothing").
/// The password dialog was closed — "changed my mind": advice to apply again
/// and enter the password; otherwise — the reason.
pub fn parts_refusal(id: &str, offer: &PartsOffer, why: &str) -> String {
    if why == crate::appearance_installer::password_not_confirmed() {
        return crate::i18n::fill(
            // TRANSLATORS: %1 is the layout name, %2 a list of panel parts.
            &crate::i18n::ntr(
                "The “%1” layout wasn't applied: it needs %2, which isn't installed. Apply the layout \
                 again and enter the password to install it.",
                "The “%1” layout wasn't applied: it needs %2, which aren't installed. Apply the layout \
                 again and enter the password to install them.",
                offer.labels.len() as u64,
            ),
            &[&title_of(id), &offer.labels.join(", ")],
        );
    }
    trf(
        // TRANSLATORS: %1 is the layout name, %2 a list of panel parts, %3 the reason.
        "The “%1” layout wasn't applied: couldn't install %2: %3.",
        &[&title_of(id), &offer.labels.join(", "), &why],
    )
}

/// Install the parts (`install`), then apply the layout — it will pick up
/// everything that got installed (mate-panel watches the applet directory
/// itself). "All or nothing": if something did not install, the layout is NOT
/// applied, and it is said why and what to do. Parts that cannot be obtained
/// anywhere (or from AUR, whose build the person declined) are replaced by the
/// layout as described under it.
pub fn apply_with_parts(
    io: &dyn LookIo,
    ctx: &PanelCtx,
    id: &str,
    offer: &PartsOffer,
    install: &mut dyn FnMut(&PartsOffer) -> Result<(), String>,
) -> String {
    if !offer.is_empty() {
        if let Err(why) = install(offer) {
            // A menu has a stand-in that always exists — the built-in menu button.
            // A menu that did not install (the AUR mate-menu has been failing) does
            // not cost the person the layout: it is applied with the button, and
            // it is said why. Closing the password dialog is still "changed my
            // mind" — nothing is applied then.
            if why != crate::appearance_installer::password_not_confirmed() && only_menus_missing(io, ctx, id, offer) {
                let note = trf(
                    // TRANSLATORS: %1 is the reason the install failed.
                    "The MATE Menu couldn't be installed (%1) — the standard MATE menu button is used instead.",
                    &[&why],
                );
                return format!("{}\n{note}", apply(io, ctx, id));
            }
            return parts_refusal(id, offer, &why);
        }
    }
    apply(io, ctx, id)
}

/// Whether everything of `offer` that layout `id` still lacks after the install
/// is a menu (it has the built-in button as a stand-in, see [`prepare`]). Parts
/// that were not in the offer (nowhere to get them) are replaced or left out as
/// described under the layout anyway.
fn only_menus_missing(io: &dyn LookIo, ctx: &PanelCtx, id: &str, offer: &PartsOffer) -> bool {
    let Some(text) = list_layouts(io, ctx).into_iter().find(|(name, _)| name == id).and_then(|(_, p)| io.read(&p)) else {
        return false;
    };
    let installed = installed_applets(io, &ctx.applet_dirs);
    missing_parts(&text, &installed)
        .iter()
        .filter(|iid| offer.labels.contains(&applet_label(iid)))
        .all(|iid| MENU_IIDS.contains(&iid.as_str()))
}

// ═══════════════════════════════════════════════════════════════════════════
// Where to look
// ═══════════════════════════════════════════════════════════════════════════

/// Where layouts, applets and our files lie.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelCtx {
    /// The environment as the core names it (`system::detect_current_de`).
    pub de: String,
    /// `$XDG_DATA_HOME` or `~/.local/share`.
    pub data_home: String,
    /// Layout directories in search order: the first file found with the name wins.
    pub layout_dirs: Vec<String>,
    /// Directories of applet descriptions (`*.mate-panel-applet`).
    pub applet_dirs: Vec<String>,
}

impl PanelCtx {
    /// `xdg_data_dirs` and `applets_env` — the values of `XDG_DATA_DIRS` and
    /// `MATE_PANEL_APPLETS_DIR` (empty — not set).
    pub fn new(de: &str, home: &str, xdg_data_home: Option<&str>, xdg_data_dirs: &str, applets_env: &str) -> Self {
        let data_home = crate::utils::data_home(xdg_data_home, home);
        // mate-panel's own directory — first: by name the panel looks only
        // there. The other system data directories are for builds with a
        // different prefix (/usr/local, NixOS); their layouts are applied by path.
        let mut layout_dirs = vec![SYSTEM_LAYOUTS_DIR.to_string()];
        let data_dirs = if xdg_data_dirs.trim().is_empty() { "/usr/local/share:/usr/share" } else { xdg_data_dirs };
        for dir in data_dirs.split(':').filter(|d| !d.is_empty()) {
            let candidate = format!("{}/mate-panel/layouts", dir.trim_end_matches('/'));
            if !layout_dirs.contains(&candidate) {
                layout_dirs.push(candidate);
            }
        }
        // The person's own layouts — last: they do not override the name of a
        // system layout.
        let own = copies_dir_of(&data_home);
        if !layout_dirs.contains(&own) {
            layout_dirs.push(own);
        }
        let applet_dirs: Vec<String> = applets_env.split(':').filter(|d| !d.is_empty()).map(str::to_string).collect();
        let applet_dirs = if applet_dirs.is_empty() { vec![SYSTEM_APPLETS_DIR.to_string()] } else { applet_dirs };
        PanelCtx { de: de.to_string(), data_home, layout_dirs, applet_dirs }
    }

    /// The "Panel layout" section of the "Appearance" tab: the desktop is the one
    /// chosen in "Current environment" ([`crate::animations::LookCtx::for_look`]).
    pub fn for_look() -> Self {
        let mut ctx = Self::from_env();
        ctx.de = crate::animations::LookCtx::for_look().de;
        ctx
    }

    /// From the process environment (window, test stand).
    pub fn from_env() -> Self {
        let env = |n: &str| std::env::var(n).unwrap_or_default();
        let de = crate::animations::LookCtx::from_env().de;
        let xdg = env("XDG_DATA_HOME");
        PanelCtx::new(
            &de,
            &env("HOME"),
            if xdg.is_empty() { None } else { Some(&xdg) },
            &env("XDG_DATA_DIRS"),
            &env("MATE_PANEL_APPLETS_DIR"),
        )
    }

    /// Directory of the fixed copies.
    pub fn copies_dir(&self) -> String {
        copies_dir_of(&self.data_home)
    }

    /// Snapshot of the panel before the last apply.
    pub fn backup_path(&self) -> String {
        format!("{}/rexon/backups/mate-panel.dconf", self.data_home.trim_end_matches('/'))
    }
}

fn copies_dir_of(data_home: &str) -> String {
    format!("{}/mate-panel/layouts", data_home.trim_end_matches('/'))
}

// ═══════════════════════════════════════════════════════════════════════════
// Parsing key-file
// ═══════════════════════════════════════════════════════════════════════════

/// A key-file group with its keys in file order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub name: String,
    pub keys: Vec<(String, String)>,
}

impl Group {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.keys.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    fn set(&mut self, key: &str, value: &str) {
        match self.keys.iter_mut().find(|(k, _)| k == key) {
            Some(slot) => slot.1 = value.to_string(),
            None => self.keys.push((key.to_string(), value.to_string())),
        }
    }

    fn remove(&mut self, key: &str) {
        self.keys.retain(|(k, _)| k != key);
    }

    /// The name after the group prefix (`[Object briskmenu]` → `briskmenu`), if the
    /// group is of this kind.
    fn suffix(&self, prefix: &str) -> Option<&str> {
        let rest = self.name.strip_prefix(prefix)?;
        (rest.is_empty() || rest.starts_with(char::is_whitespace)).then(|| rest.trim())
    }

    fn is_toplevel(&self) -> bool {
        self.suffix("Toplevel").is_some()
    }

    fn is_object(&self) -> bool {
        self.suffix("Object").is_some()
    }
}

/// Parse a key-file (GKeyFile format): groups and keys in file order, comments
/// and empty lines are skipped.
pub fn parse_keyfile(text: &str) -> Vec<Group> {
    let mut groups: Vec<Group> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            groups.push(Group { name: name.trim().to_string(), keys: Vec::new() });
            continue;
        }
        let (Some(group), Some((key, value))) = (groups.last_mut(), line.split_once('=')) else {
            continue;
        };
        group.keys.push((key.trim().to_string(), value.trim().to_string()));
    }
    groups
}

fn write_keyfile(groups: &[Group]) -> String {
    let mut out = String::new();
    for (i, group) in groups.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(&format!("[{}]\n", group.name));
        for (k, v) in &group.keys {
            out.push_str(&format!("{k}={v}\n"));
        }
    }
    out
}

// ═══════════════════════════════════════════════════════════════════════════
// Applets on the system
// ═══════════════════════════════════════════════════════════════════════════

/// Applets present on the system, as `applet-iid` (`Factory::Applet`): in a
/// `*.mate-panel-applet` file the `[Applet Factory]` group names the factory
/// (`Id=`), the other groups are applets.
pub fn installed_applets(io: &dyn LookIo, dirs: &[String]) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for dir in dirs {
        for name in io.list_dir(dir) {
            if !name.ends_with(".mate-panel-applet") {
                continue;
            }
            let Some(text) = io.read(&format!("{}/{name}", dir.trim_end_matches('/'))) else {
                continue;
            };
            out.extend(applets_in_description(&text));
        }
    }
    out
}

/// The applets (`Factory::Applet`) one `*.mate-panel-applet` file describes.
pub fn applets_in_description(text: &str) -> Vec<String> {
    let groups = parse_keyfile(text);
    let Some(factory) = groups.iter().find(|g| g.name == "Applet Factory").and_then(|g| g.get("Id")) else {
        return Vec::new();
    };
    groups.iter().filter(|g| g.name != "Applet Factory").map(|g| format!("{factory}::{}", g.name)).collect()
}

/// Human-readable applet name.
pub fn applet_label(iid: &str) -> String {
    match APPLET_LABELS.iter().find(|(id, _)| *id == iid) {
        Some((_, label)) => tr(label),
        None => iid.rsplit("::").next().unwrap_or(iid).to_string(),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Preparing a layout
// ═══════════════════════════════════════════════════════════════════════════

/// A layout ready to be applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prepared {
    /// Text of the layout that this system's mate-panel will accept.
    pub text: String,
    /// Whether anything was edited (no — the original file can be applied).
    pub changed: bool,
    /// How many panels it has.
    pub panels: usize,
    /// Notes for the person: what was replaced and what will not be there.
    pub notes: Vec<String>,
}

/// Prepare a layout for applying on THIS system (see the module description):
/// `installed` — applets on the system ([`installed_applets`]), `file_exists` —
/// whether a launcher's file exists.
pub fn prepare(text: &str, installed: &BTreeSet<String>, file_exists: &dyn Fn(&str) -> bool) -> Prepared {
    let groups = parse_keyfile(text);
    let mut changed = false;
    let mut notes = Vec::new();
    let mut dropped: Vec<String> = Vec::new();
    let clock_available = installed.contains(CLOCK_IID);
    let mut has_clock = groups.iter().any(|g| g.is_object() && g.get("applet-iid") == Some(CLOCK_IID));
    let mut out: Vec<Group> = Vec::new();

    for mut group in groups {
        let allowed = if group.is_toplevel() {
            TOPLEVEL_KEYS
        } else if group.is_object() {
            OBJECT_KEYS
        } else {
            // `[Customsetting …]` and anything else mate-panel does not read.
            changed = true;
            continue;
        };
        if group.is_object() {
            // A GNOME Panel key: mate-panel does not know it and would drop the
            // object entirely. The meaning is the same as `panel-right-stick`.
            if let Some(edge) = group.get("relative-to-edge").map(str::to_string) {
                if edge == "end" && group.get("panel-right-stick").is_none() {
                    group.set("panel-right-stick", "true");
                }
                group.remove("relative-to-edge");
                changed = true;
            }
        }
        let before = group.keys.len();
        group.keys.retain(|(k, _)| allowed.contains(&k.as_str()));
        changed |= group.keys.len() != before;

        if group.is_object() {
            match group.get("object-type").unwrap_or("") {
                "applet" => {
                    let iid = group.get("applet-iid").unwrap_or("").to_string();
                    if iid == BRISK_MENU_IID {
                        // Even when Brisk itself is still on the system: Rexon does
                        // not put it on a panel any more.
                        changed = true;
                        notes.push(replace_brisk(&mut group, installed.contains(MATE_MENU_IID)));
                    } else if !installed.contains(&iid) {
                        changed = true;
                        if MENU_IIDS.contains(&iid.as_str()) {
                            group.set("object-type", "menu");
                            group.remove("applet-iid");
                            notes.push(trf(
                                "%1 is not installed — the standard MATE menu button will be used instead.",
                                &[&capitalize(&applet_label(&iid))],
                            ));
                        } else if INDICATOR_IIDS.contains(&iid.as_str()) && clock_available && !has_clock {
                            group.set("applet-iid", CLOCK_IID);
                            has_clock = true;
                            notes.push(tr("Indicators are not installed — a clock will be shown instead."));
                        } else {
                            let label = applet_label(&iid);
                            if !dropped.contains(&label) {
                                dropped.push(label);
                            }
                            continue;
                        }
                    }
                }
                "launcher" => {
                    // A launcher of a program that is not installed: the panel would skip
                    // it anyway (complaining in the log) — we remove it ourselves.
                    let location = group.get("launcher-location").unwrap_or("");
                    if location.starts_with('/') && !file_exists(location) {
                        changed = true;
                        continue;
                    }
                }
                _ => {}
            }
        }
        out.push(group);
    }
    if !dropped.is_empty() {
        notes.push(trf("Not installed and will be left out: %1.", &[&dropped.join(", ")]));
    }
    let panels = out.iter().filter(|g| g.is_toplevel()).count();
    Prepared { text: write_keyfile(&out), changed, panels, notes }
}

/// Turn the Brisk menu object `group` into the MATE Menu applet (`mate_menu` —
/// the applet is on the system) or into the built-in menu button; the note for
/// the person.
fn replace_brisk(group: &mut Group, mate_menu: bool) -> String {
    if mate_menu {
        group.set("applet-iid", MATE_MENU_IID);
        tr("The Brisk menu is no longer supported — the MATE Menu will be used instead.")
    } else {
        group.set("object-type", "menu");
        group.remove("applet-iid");
        tr("The Brisk menu is no longer supported — the standard MATE menu button will be used instead.")
    }
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Labels
// ═══════════════════════════════════════════════════════════════════════════

/// Description of an unknown layout from its composition: where the panels are.
pub fn auto_description(text: &str) -> String {
    let mut sides: Vec<String> = Vec::new();
    for group in parse_keyfile(text).iter().filter(|g| g.is_toplevel()) {
        let side = match group.get("orientation").unwrap_or("top") {
            "bottom" => tr("at the bottom"),
            "left" => tr("on the left"),
            "right" => tr("on the right"),
            _ => tr("at the top"),
        };
        if !sides.contains(&side) {
            sides.push(side);
        }
    }
    if sides.is_empty() {
        return tr("A panel layout without panels.");
    }
    trf("Panels: %1.", &[&sides.join(", ")])
}

fn title_of(id: &str) -> String {
    match KNOWN_LAYOUTS.iter().find(|(name, _, _)| *name == id) {
        Some((_, title, _)) => tr(title),
        None => capitalize(&id.replace(['-', '_'], " ")),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Section snapshot
// ═══════════════════════════════════════════════════════════════════════════

/// One layout in the list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LayoutEntry {
    /// File name without `.layout`.
    pub id: String,
    pub title: String,
    pub description: String,
    /// What will differ from the layout's design (replaced, skipped).
    pub notes: Vec<String>,
    /// Selected now (`default-layout`).
    pub current: bool,
    /// Full path of the original file.
    #[serde(skip)]
    pub path: String,
}

/// Snapshot of the "MATE panel" section.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PanelState {
    /// Show the section: a MATE session and mate-panel are present.
    pub available: bool,
    pub layouts: Vec<LayoutEntry>,
    /// Title of the current layout (empty — we do not know).
    pub current_title: String,
    /// There is a snapshot of the previous panel — it can be restored.
    pub can_restore: bool,
}

/// Layouts on the system: known ones in the order of [`KNOWN_LAYOUTS`], the rest
/// alphabetically. Our copies (`rexon-*`) are not included in the list.
pub fn list_layouts(io: &dyn LookIo, ctx: &PanelCtx) -> Vec<(String, String)> {
    let mut found: Vec<(String, String)> = Vec::new();
    for dir in &ctx.layout_dirs {
        for name in io.list_dir(dir) {
            let Some(id) = name.strip_suffix(".layout") else {
                continue;
            };
            if id.is_empty() || id.starts_with(COPY_PREFIX) || found.iter().any(|(f, _)| f == id) {
                continue;
            }
            found.push((id.to_string(), format!("{}/{name}", dir.trim_end_matches('/'))));
        }
    }
    let rank = |id: &str| KNOWN_LAYOUTS.iter().position(|(n, _, _)| *n == id).unwrap_or(usize::MAX);
    found.sort_by(|a, b| rank(&a.0).cmp(&rank(&b.0)).then_with(|| a.0.cmp(&b.0)));
    found
}

/// Layout name from the `default-layout` value (a name, a path to a file or a
/// path to our copy).
pub fn layout_id_from_setting(value: &str) -> String {
    let value = value.trim();
    if !value.starts_with('/') {
        return value.to_string();
    }
    let file = value.rsplit('/').next().unwrap_or(value);
    let stem = file.strip_suffix(".layout").unwrap_or(file);
    stem.strip_prefix(COPY_PREFIX).unwrap_or(stem).to_string()
}

/// Whether to show the section.
pub fn is_available(io: &dyn LookIo, ctx: &PanelCtx) -> bool {
    ctx.de == "mate" && io.have("mate-panel")
}

/// Section snapshot.
pub fn read_state(io: &dyn LookIo, ctx: &PanelCtx) -> PanelState {
    let mut st = PanelState { available: false, layouts: Vec::new(), current_title: String::new(), can_restore: false };
    if !is_available(io, ctx) {
        return st;
    }
    st.available = true;
    let current = gsettings_get(io, "org.mate.panel", "default-layout")
        .map(|v| layout_id_from_setting(&v))
        .unwrap_or_default();
    let installed = installed_applets(io, &ctx.applet_dirs);
    let exists = |path: &str| io.read(path).is_some();
    for (id, path) in list_layouts(io, ctx) {
        let Some(text) = io.read(&path) else {
            continue;
        };
        let description = match KNOWN_LAYOUTS.iter().find(|(name, _, _)| *name == id) {
            Some((_, _, desc)) => tr(desc),
            None => auto_description(&text),
        };
        let mut notes = prepare(&text, &installed, &exists).notes;
        let dock = path.strip_suffix(".layout").map(|p| format!("{p}.dock")).unwrap_or_default();
        if io.read(&dock).is_some() {
            notes.push(tr("Designed together with the Plank dock — Rexon doesn't turn the dock on."));
        }
        if id == current {
            st.current_title = title_of(&id);
        }
        st.layouts.push(LayoutEntry { title: title_of(&id), current: id == current, description, notes, path, id });
    }
    st.can_restore = io.read(&ctx.backup_path()).is_some();
    st
}

/// Confirmation text before applying: (title, question).
pub fn confirm_texts(id: &str) -> (String, String) {
    (
        tr("Replace the MATE panel?"),
        trf(
            "The current panel will be replaced with the “%1” layout. Its applets, launchers and \
             panel settings will be reset to what this layout has. Rexon saves the current panel \
             first, so you can bring it back with “Restore previous panel”.",
            &[&title_of(id)],
        ),
    )
}

/// Confirmation text before restoring the previous panel: (title, question).
pub fn restore_confirm_texts() -> (String, String) {
    (
        tr("Restore the previous MATE panel?"),
        tr("The panel will go back to how it was before Rexon first changed its layout. \
            Changes made to the panel since then will be lost."),
    )
}

// ═══════════════════════════════════════════════════════════════════════════
// Applying and restoring
// ═══════════════════════════════════════════════════════════════════════════

/// Apply layout `id` → a message for the person.
pub fn apply(io: &dyn LookIo, ctx: &PanelCtx, id: &str) -> String {
    if !is_available(io, ctx) {
        return tr("⚠️ The MATE panel is not available in this session.");
    }
    let title = title_of(id);
    let Some((_, path)) = list_layouts(io, ctx).into_iter().find(|(name, _)| name == id) else {
        return trf("⚠️ Panel layout “%1” was not found.", &[&title]);
    };
    let Some(text) = io.read(&path) else {
        return trf("⚠️ Could not read panel layout “%1”.", &[&title]);
    };
    let installed = installed_applets(io, &ctx.applet_dirs);
    let prepared = prepare(&text, &installed, &|p: &str| io.read(p).is_some());
    if prepared.panels == 0 {
        return trf("⚠️ Panel layout “%1” has no panels.", &[&title]);
    }

    // Snapshot of the previous panel — before any changes. If it failed, we
    // do not touch anything. Subsequent applies do not overwrite the person's
    // original panel (the snapshot before the first apply): their snapshot
    // lies next to it.
    let backup = ctx.backup_path();
    let mark = crate::backup::panel_original_returned_mark(&backup);
    let slot = crate::backup::panel_snapshot_slot(io.read(&backup).is_some(), io.read(&mark).as_deref());
    let snapshot = match slot {
        crate::backup::PanelSnapshotSlot::Original => backup.clone(),
        crate::backup::PanelSnapshotSlot::Last => crate::backup::panel_last_snapshot_path(&backup),
    };
    let (ok, dump) = io.run(&argv(&["dconf", "dump", PANEL_DCONF_DIR]));
    if !ok || !io.write(&snapshot, &dump) {
        let why = if ok { snapshot } else { dump };
        return trf("⚠️ Could not save the current panel, so nothing was changed: %1", &[&why]);
    }
    if slot == crate::backup::PanelSnapshotSlot::Original && io.read(&mark).is_some() {
        io.write(&mark, "");
    }

    let target = if !prepared.changed && path == format!("{SYSTEM_LAYOUTS_DIR}/{id}.layout") {
        id.to_string()
    } else {
        let copy = format!("{}/{COPY_PREFIX}{id}.layout", ctx.copies_dir());
        if !io.write(&copy, &prepared.text) {
            return trf("⚠️ Could not write file %1", &[&copy]);
        }
        copy
    };
    let (ok, out) = io.run(&argv(&["mate-panel", "--layout", &target, "--reset"]));
    if !ok {
        return trf("⚠️ Could not apply panel layout “%1”: %2", &[&title, &out]);
    }
    trf("✅ MATE panel: layout “%1” applied.", &[&title])
}

/// "Save panel" of the "Desktops" tab: take a copy of the panel into the same
/// file as before applying a layout ([`PanelCtx::backup_path`]) — MATE has a
/// single copy, and "Restore panel" and "Restore the previous panel" restore the
/// same thing. `(succeeded, path or reason)`.
///
/// The copy was saved by the person themselves — it is their panel: subsequent
/// layout applies do not overwrite it (`backup::panel_snapshot_slot`).
pub fn save_snapshot(io: &dyn LookIo, ctx: &PanelCtx) -> (bool, String) {
    let (ok, dump) = io.run(&argv(&["dconf", "dump", PANEL_DCONF_DIR]));
    if !ok {
        return (false, dump);
    }
    let path = ctx.backup_path();
    if io.write(&path, &dump) {
        let mark = crate::backup::panel_original_returned_mark(&path);
        if io.read(&mark).is_some() {
            io.write(&mark, "");
        }
        (true, path)
    } else {
        (false, path)
    }
}

/// Restore the original panel — as it was before the first layout apply (or the
/// one saved with "Save panel"). After a restore, the next apply takes the
/// original snapshot again.
pub fn restore(io: &dyn LookIo, ctx: &PanelCtx) -> String {
    restore_from(io, ctx, &ctx.backup_path())
}

/// Restore the panel from dump file `backup`: the layout section's original
/// copy ([`PanelCtx::backup_path`]) or its `.last` neighbour. Only restoring
/// the original places the "returned" mark.
pub fn restore_from(io: &dyn LookIo, ctx: &PanelCtx, backup: &str) -> String {
    if !is_available(io, ctx) {
        return tr("⚠️ The MATE panel is not available in this session.");
    }
    let backup = backup.to_string();
    if io.read(&backup).is_none() {
        return tr("⚠️ There is no saved panel to restore.");
    }
    // Insurance: what is set now. The steps below destroy the panel before
    // loading the snapshot, and a corrupted snapshot would leave it empty and
    // reset.
    let undo = format!("{backup}.before-restore");
    let (ok, now) = io.run(&argv(&["dconf", "dump", PANEL_DCONF_DIR]));
    if !ok || !io.write(&undo, &now) {
        let why = if ok { undo } else { now };
        return trf("⚠️ Could not save the current panel, so nothing was changed: %1", &[&why]);
    }
    // First — an empty object list: the running panel removes all applets and
    // erases their branches. Without this, an object with the same name in
    // both layouts (`notification-area` is in both "Like Windows" and the
    // classic one) is not reloaded by the panel, and it stayed on a foreign
    // panel (measured on the stand). We wait until the panel has managed (the
    // object branch became empty); if there is no panel, there is no one to
    // wait for, and after 3 s we move on.
    let (ok, out) = io.run(&empty_objects_argv());
    if !ok {
        return trf("⚠️ Could not restore the previous panel: %1", &[&out]);
    }
    for _ in 0..30 {
        let (ok, out) = io.run(&argv(&["dconf", "list", "/org/mate/panel/objects/"]));
        if !ok || out.trim().is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    // Panels and objects — wiped clean: otherwise keys that are not in the
    // snapshot (it stores only differences from the defaults) would remain
    // from the layout. `default-layout` too: a snapshot with its default value
    // does not carry it.
    for step in &load_steps(&backup) {
        let (ok, out) = io.run(step);
        if !ok {
            // Restore the panel as it was before the click.
            for back in rollback_steps(&undo) {
                io.run(&back);
            }
            return trf("⚠️ Could not restore the previous panel: %1", &[&out]);
        }
    }
    // The original came back: the next apply takes it again. If the mark was
    // not written, the original simply stays the same (that is the safe side).
    if backup == ctx.backup_path() {
        io.write(&crate::backup::panel_original_returned_mark(&backup), crate::backup::PANEL_ORIGINAL_RETURNED);
    }
    tr("✅ MATE panel restored to how it was before.")
}

/// The first step of a restore on a live panel: an empty object list, so the
/// running panel drops all applets (see [`restore_from`] for why).
pub fn empty_objects_argv() -> Vec<String> {
    argv(&["dconf", "write", "/org/mate/panel/general/object-id-list", "@as []"])
}

/// Panels and objects wiped clean, then dump `file` loaded: one list for both
/// the layout section and the panel protection copies.
pub fn load_steps(file: &str) -> Vec<Vec<String>> {
    vec![
        argv(&["dconf", "reset", "-f", "/org/mate/panel/toplevels/"]),
        argv(&["dconf", "reset", "-f", "/org/mate/panel/objects/"]),
        argv(&["dconf", "reset", "/org/mate/panel/general/default-layout"]),
        argv(&["sh", "-c", "dconf load /org/mate/panel/ < \"$1\"", "sh", file]),
    ]
}

/// Put the panel back from insurance dump `undo` after a failed load.
pub fn rollback_steps(undo: &str) -> Vec<Vec<String>> {
    vec![
        argv(&["dconf", "reset", "-f", "/org/mate/panel/toplevels/"]),
        argv(&["dconf", "reset", "-f", "/org/mate/panel/objects/"]),
        argv(&["sh", "-c", "dconf load /org/mate/panel/ < \"$1\"", "sh", undo]),
    ]
}

// ═══════════════════════════════════════════════════════════════════════════
// Brisk menu on the live panel → MATE Menu
// ═══════════════════════════════════════════════════════════════════════════
//
// Earlier releases put the Brisk menu on the panel (the "Like Windows" layout,
// the AUR set of the MATE install). Rexon does not install Brisk any more, so a
// panel that still has it is moved to the MATE Menu (or to the built-in menu
// button) by "Check my desktop" — the panel never stays without a menu.
//
// How: the replacement object is written under a NEW id first, then the id
// takes the old one's place in `object-id-list`, then the old object's branch
// is erased. The running mate-panel follows `object-id-list` by itself: it
// unloads the ids that left the list and loads the new ones from their keys —
// so the menu is swapped without a restart and without resetting the rest of
// the panel. (Changing `applet-iid` of a loaded object in place is not picked
// up by the panel: the applet is chosen when the object is loaded.)

/// What [`brisk_migration`] does on the panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BriskMigration {
    /// Ids of the Brisk objects that are replaced.
    pub replaced: Vec<String>,
    /// The commands, in order (`dconf write` / `dconf reset`).
    pub commands: Vec<Vec<String>>,
}

/// Parse a GVariant string array as `dconf` prints it (`['a', 'b']`, `@as []`).
fn gvariant_strings(value: &str) -> Vec<String> {
    let v = value.trim().trim_start_matches("@as").trim();
    let inner = v.strip_prefix('[').and_then(|v| v.strip_suffix(']')).unwrap_or("");
    inner
        .split(',')
        .map(|item| item.trim().trim_matches(|c| c == '\'' || c == '"').to_string())
        .filter(|item| !item.is_empty())
        .collect()
}

fn gvariant_string_array(items: &[String]) -> String {
    if items.is_empty() {
        return "@as []".to_string();
    }
    let quoted: Vec<String> = items.iter().map(|i| format!("'{i}'")).collect();
    format!("[{}]", quoted.join(", "))
}

/// The plan for moving the panel's Brisk menu objects to the MATE Menu applet
/// (`mate_menu` — its applet is on the system) or to the built-in menu button.
/// `dump` — `dconf dump /org/mate/panel/`. `None` — there is no Brisk menu on
/// the panel.
pub fn brisk_migration(dump: &str, mate_menu: bool) -> Option<BriskMigration> {
    let groups = parse_keyfile(dump);
    let object_id = |g: &Group| g.name.strip_prefix("objects/").filter(|id| !id.is_empty() && !id.contains('/')).map(str::to_string);
    let brisk: Vec<(String, &Group)> = groups
        .iter()
        .filter(|g| g.get("applet-iid").map(|v| v.trim_matches('\'')) == Some(BRISK_MENU_IID))
        .filter_map(|g| object_id(g).map(|id| (id, g)))
        .collect();
    if brisk.is_empty() {
        return None;
    }
    let mut taken: BTreeSet<String> = groups.iter().filter_map(object_id).collect();
    let mut list = groups
        .iter()
        .find(|g| g.name == "general")
        .and_then(|g| g.get("object-id-list"))
        .map(gvariant_strings);
    let mut commands = Vec::new();
    let mut replaced = Vec::new();
    for (old, group) in brisk {
        let mut new = format!("{old}-menu");
        let mut n = 2;
        while taken.contains(&new) {
            new = format!("{old}-menu-{n}");
            n += 1;
        }
        taken.insert(new.clone());
        let key = |k: &str| format!("/org/mate/panel/objects/{new}/{k}");
        if mate_menu {
            commands.push(argv(&["dconf", "write", &key("object-type"), "'applet'"]));
            commands.push(argv(&["dconf", "write", &key("applet-iid"), &format!("'{MATE_MENU_IID}'")]));
        } else {
            commands.push(argv(&["dconf", "write", &key("object-type"), "'menu'"]));
        }
        // Where the old menu stood, as it was (the values are copied in GVariant form).
        for k in ["toplevel-id", "position", "panel-right-stick", "locked"] {
            if let Some(v) = group.get(k) {
                commands.push(argv(&["dconf", "write", &key(k), v]));
            }
        }
        if let Some(ids) = list.as_mut() {
            if let Some(at) = ids.iter().position(|i| *i == old) {
                ids[at] = new.clone();
            }
        }
        replaced.push(old);
    }
    // `object-id-list` missing from the dump is its schema default, which has no
    // Brisk menu: then the Brisk objects are not on the panel and are only erased.
    if let Some(ids) = &list {
        commands.push(argv(&["dconf", "write", "/org/mate/panel/general/object-id-list", &gvariant_string_array(ids)]));
    }
    for old in &replaced {
        commands.push(argv(&["dconf", "reset", "-f", &format!("/org/mate/panel/objects/{old}/")]));
    }
    Some(BriskMigration { replaced, commands })
}

// ═══════════════════════════════════════════════════════════════════════════
// Custom layout ("Like in a distribution" looks)
// ═══════════════════════════════════════════════════════════════════════════
//
// A look describes panels by roles (`native/desktop-model.json`: menu,
// windows, clock, tray, launchers), and [`own_layout_text`] assembles a
// mate-panel layout from them — only from what mate-panel itself has (the menu
// button, the window list, the clock, the notification area, launchers), so a
// "failed to load the applet" dialog never appears. Applying and restoring are
// the same `mate-panel --layout <path> --reset` and `dconf dump` snapshot as
// for the ready-made layouts above, but as one shell command per step: they
// are executed by the shared run of the look, not by [`LookIo`].

/// An object of the custom layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnObject {
    /// MATE's built-in main menu button.
    MainMenu,
    /// The MATE Menu applet (`mate-menu`) — put only when the package is on the
    /// system, otherwise [`OwnObject::MainMenu`] takes its place.
    MateMenu,
    /// The "show desktop" button (part of mate-panel's own Wnck applets).
    ShowDesktop,
    WindowList,
    Clock,
    NotificationArea,
    /// A launcher: the full path to a `.desktop` file.
    Launcher(String),
}

/// A panel of the custom layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnPanel {
    /// Panel name in the layout (`top`, `dock`).
    pub id: String,
    pub top: bool,
    pub size: u32,
    /// Full width; otherwise — fitted to the contents.
    pub expand: bool,
    /// In the middle of the edge (a dock).
    pub centered: bool,
    /// Left to right: objects at the start of the edge.
    pub start: Vec<OwnObject>,
    /// Right to left from the end of the edge (`panel-right-stick`).
    pub end: Vec<OwnObject>,
}

/// Text of a mate-panel layout from panels `panels`.
pub fn own_layout_text(panels: &[OwnPanel]) -> String {
    let mut groups: Vec<Group> = Vec::new();
    let mut objects: Vec<Group> = Vec::new();
    for p in panels {
        let mut keys = vec![
            ("expand".to_string(), p.expand.to_string()),
            ("orientation".to_string(), if p.top { "top" } else { "bottom" }.to_string()),
            ("size".to_string(), p.size.to_string()),
        ];
        if p.centered {
            keys.push(("x-centered".to_string(), "true".to_string()));
        }
        // mate-panel places a "fitted to contents" panel by coordinates and derives
        // the edge from them too: without `y-bottom` the dock moved to the top (Xvfb
        // stand 31.10.2026, mate-panel 1.28.7: `orientation='top'` after applying).
        if !p.expand {
            keys.push((if p.top { "y" } else { "y-bottom" }.to_string(), "0".to_string()));
        }
        groups.push(Group { name: format!("Toplevel {}", p.id), keys });
        for (stick, list) in [(false, &p.start), (true, &p.end)] {
            for (i, o) in list.iter().enumerate() {
                let (name, mut keys) = match o {
                    OwnObject::MainMenu => ("menu".to_string(), vec![("object-type", "menu".to_string())]),
                    OwnObject::MateMenu => (
                        "mate-menu".to_string(),
                        vec![("object-type", "applet".to_string()), ("applet-iid", MATE_MENU_IID.to_string())],
                    ),
                    OwnObject::ShowDesktop => (
                        "show-desktop".to_string(),
                        vec![("object-type", "applet".to_string()), ("applet-iid", SHOW_DESKTOP_IID.to_string())],
                    ),
                    OwnObject::WindowList => (
                        "window-list".to_string(),
                        vec![("object-type", "applet".to_string()), ("applet-iid", "WnckletFactory::WindowListApplet".to_string())],
                    ),
                    OwnObject::Clock => (
                        "clock".to_string(),
                        vec![("object-type", "applet".to_string()), ("applet-iid", CLOCK_IID.to_string())],
                    ),
                    OwnObject::NotificationArea => (
                        "notification-area".to_string(),
                        vec![
                            ("object-type", "applet".to_string()),
                            ("applet-iid", "NotificationAreaAppletFactory::NotificationArea".to_string()),
                        ],
                    ),
                    OwnObject::Launcher(path) => {
                        let stem = std::path::Path::new(path)
                            .file_stem()
                            .map(|s| s.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        (
                            format!("launcher-{stem}"),
                            vec![("object-type", "launcher".to_string()), ("launcher-location", path.clone())],
                        )
                    }
                };
                keys.push(("toplevel-id", p.id.clone()));
                keys.push(("position", (i * 10).to_string()));
                if stick {
                    keys.push(("panel-right-stick", "true".to_string()));
                }
                keys.push(("locked", "true".to_string()));
                objects.push(Group {
                    name: format!("Object {}-{name}", p.id),
                    keys: keys.into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
                });
            }
        }
    }
    groups.extend(objects);
    write_keyfile(&groups)
}

/// Take a panel snapshot into `dump` and apply layout `layout` — as one
/// command: without a snapshot the layout is not applied (as with [`apply`]).
pub fn own_apply_argv(layout: &str, dump: &str) -> Vec<String> {
    argv(&[
        "sh",
        "-c",
        "mkdir -p \"$(dirname \"$2\")\" && dconf dump /org/mate/panel/ > \"$2\" && mate-panel --layout \"$1\" --reset",
        "sh",
        layout,
        dump,
    ])
}

/// Restore the panel from snapshot `dump` — the same sequence as in [`restore`]
/// (an empty object list, wait until the panel removes them, panels and objects
/// wiped clean, load the snapshot); the snapshot is deleted after loading.
pub fn own_restore_argv(dump: &str) -> Vec<String> {
    argv(&[
        "sh",
        "-c",
        "[ -f \"$1\" ] || exit 0; dconf write /org/mate/panel/general/object-id-list '@as []'; i=0; \
         while [ $i -lt 30 ] && [ -n \"$(dconf list /org/mate/panel/objects/)\" ]; do sleep 0.1; i=$((i+1)); done; \
         dconf reset -f /org/mate/panel/toplevels/; dconf reset -f /org/mate/panel/objects/; \
         dconf reset /org/mate/panel/general/default-layout; dconf load /org/mate/panel/ < \"$1\" && rm -f \"$1\"",
        "sh",
        dump,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    /// A copy of `/usr/share/mate-panel/layouts/redmond.layout` (ubuntu-mate-settings).
    const REDMOND: &str = "[Toplevel bottom]
expand=true
orientation=bottom
size=28

[Object briskmenu]
object-type=applet
applet-iid=BriskMenuFactory::BriskMenu
toplevel-id=bottom
position=0
locked=true

[Object showdesktopapplet]
locked=true
position=10
toplevel-id=bottom
applet-iid=WnckletFactory::ShowDesktopApplet
object-type=applet

[Object firefox]
locked=true
position=20
launcher-location=/usr/share/applications/firefox.desktop
toplevel-id=bottom
object-type=launcher
menu-path=applications:/

[Object evolution]
locked=true
position=30
launcher-location=/usr/share/applications/org.gnome.Evolution.desktop
toplevel-id=bottom
object-type=launcher
menu-path=applications:/

[Object window-list]
object-type=applet
applet-iid=WnckletFactory::WindowListApplet
toplevel-id=bottom
position=40
locked=true

[Object drivemountapplet]
object-type=applet
applet-iid=DriveMountAppletFactory::DriveMountApplet
toplevel-id=bottom
position=20
relative-to-edge=end
locked=true

[Object notification-area]
object-type=applet
applet-iid=NotificationAreaAppletFactory::NotificationArea
toplevel-id=bottom
position=10
relative-to-edge=end
locked=true

[Object indicatorappletcomplete]
object-type=applet
applet-iid=IndicatorAppletCompleteFactory::IndicatorAppletComplete
toplevel-id=bottom
position=0
relative-to-edge=end
locked=true
";

    /// A copy of `/usr/share/mate-panel/layouts/linuxmint.layout` (mate-panel 1.28.7).
    const LINUXMINT: &str = "[Toplevel bottom]
expand=true
orientation=bottom
size=24

[Object menu-bar]
object-type=applet
applet-iid=MintMenuAppletFactory::MintMenuApplet
toplevel-id=bottom
position=0
locked=true

[Object show-desktop]
object-type=applet
applet-iid=WnckletFactory::ShowDesktopApplet
toplevel-id=bottom
position=1
locked=true

[Object window-list]
object-type=applet
applet-iid=WnckletFactory::WindowListApplet
toplevel-id=bottom
position=20
locked=true

[Object notification-area]
object-type=applet
applet-iid=NotificationAreaAppletFactory::NotificationArea
toplevel-id=bottom
position=10
panel-right-stick=true
locked=true

[Object clock]
object-type=applet
applet-iid=ClockAppletFactory::ClockApplet
toplevel-id=bottom
position=0
panel-right-stick=true
locked=true
";

    /// A copy of `/usr/share/mate-panel/layouts/default.layout` (mate-panel 1.28.7).
    const DEFAULT: &str = "[Toplevel top]
expand=true
orientation=top
size=24

[Toplevel bottom]
expand=true
orientation=bottom
size=24

[Object menu-bar]
object-type=menu-bar
toplevel-id=top
position=0
locked=true

[Object notification-area]
object-type=applet
applet-iid=NotificationAreaAppletFactory::NotificationArea
toplevel-id=top
position=10
panel-right-stick=true
locked=true

[Object clock]
object-type=applet
applet-iid=ClockAppletFactory::ClockApplet
toplevel-id=top
position=0
panel-right-stick=true
locked=true

[Object show-desktop]
object-type=applet
applet-iid=WnckletFactory::ShowDesktopApplet
toplevel-id=bottom
position=0
locked=true

[Object window-list]
object-type=applet
applet-iid=WnckletFactory::WindowListApplet
toplevel-id=bottom
position=20
locked=true

[Object workspace-switcher]
object-type=applet
applet-iid=WnckletFactory::WorkspaceSwitcherApplet
toplevel-id=bottom
position=0
panel-right-stick=true
locked=true
";

    /// A shortened copy of `org.mate.panel.Wncklet.mate-panel-applet`.
    const WNCKLET: &str = "[Applet Factory]
Id=WnckletFactory
InProcess=false
Location=/usr/lib/mate-panel/wnck-applet
Name=Window Navigation Applet Factory

[WindowMenuApplet]
Name=Window Selector

[WorkspaceSwitcherApplet]
Name=Workspace Switcher

[WindowListApplet]
Name=Window List

[ShowDesktopApplet]
Name=Show Desktop
";

    /// Applets of this machine (MATE without mate-menu, mintmenu and indicators).
    fn plain_mate() -> BTreeSet<String> {
        [
            "WnckletFactory::WindowMenuApplet",
            "WnckletFactory::WorkspaceSwitcherApplet",
            "WnckletFactory::WindowListApplet",
            "WnckletFactory::ShowDesktopApplet",
            "ClockAppletFactory::ClockApplet",
            "NotificationAreaAppletFactory::NotificationArea",
            "DriveMountAppletFactory::DriveMountApplet",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    }

    fn firefox_only(path: &str) -> bool {
        path == "/usr/share/applications/firefox.desktop"
    }

    fn object<'a>(groups: &'a [Group], id: &str) -> Option<&'a Group> {
        groups.iter().find(|g| g.name == format!("Object {id}"))
    }

    #[test]
    fn keyfile_keeps_groups_and_key_order() {
        let groups = parse_keyfile(REDMOND);
        assert_eq!(groups.len(), 9);
        assert_eq!(groups[0].name, "Toplevel bottom");
        assert_eq!(groups[0].get("orientation"), Some("bottom"));
        assert_eq!(groups[2].keys[0], ("locked".to_string(), "true".to_string()));
        // Building and parsing agree.
        assert_eq!(parse_keyfile(&write_keyfile(&groups)), groups);
    }

    #[test]
    fn applets_are_read_as_factory_and_applet() {
        struct Dir;
        impl LookIo for Dir {
            fn run(&self, _: &[String]) -> (bool, String) {
                (false, String::new())
            }
            fn have(&self, _: &str) -> bool {
                false
            }
            fn read(&self, path: &str) -> Option<String> {
                (path == "/a/org.mate.panel.Wncklet.mate-panel-applet").then(|| WNCKLET.to_string())
            }
            fn write(&self, _: &str, _: &str) -> bool {
                false
            }
            fn list_dir(&self, path: &str) -> Vec<String> {
                if path == "/a" {
                    vec!["org.mate.panel.Wncklet.mate-panel-applet".into(), "README".into()]
                } else {
                    Vec::new()
                }
            }
        }
        let got = installed_applets(&Dir, &["/a".to_string()]);
        assert!(got.contains("WnckletFactory::WindowListApplet"));
        assert!(got.contains("WnckletFactory::ShowDesktopApplet"));
        assert_eq!(got.len(), 4);
    }

    #[test]
    fn redmond_is_fixed_for_plain_mate() {
        let p = prepare(REDMOND, &plain_mate(), &firefox_only);
        assert!(p.changed);
        assert_eq!(p.panels, 1);
        let groups = parse_keyfile(&p.text);

        // Not a single key that mate-panel 1.28 does not know.
        assert!(!p.text.contains("relative-to-edge"));
        // The right edge is preserved in meaning.
        let tray = object(&groups, "notification-area").expect("область уведомлений осталась");
        assert_eq!(tray.get("panel-right-stick"), Some("true"));
        assert_eq!(object(&groups, "drivemountapplet").unwrap().get("panel-right-stick"), Some("true"));

        // Brisk menu without mate-menu on the system → the built-in menu button.
        let menu = object(&groups, "briskmenu").expect("меню осталось");
        assert_eq!(menu.get("object-type"), Some("menu"));
        assert_eq!(menu.get("applet-iid"), None);
        assert!(p.notes[0].contains("Brisk"), "{:?}", p.notes);

        // Indicators → the clock, on the right.
        let clock = object(&groups, "indicatorappletcomplete").expect("вместо индикаторов — часы");
        assert_eq!(clock.get("applet-iid"), Some(CLOCK_IID));
        assert_eq!(clock.get("panel-right-stick"), Some("true"));

        // No Evolution — no launcher; Firefox is there — the launcher is there.
        assert!(object(&groups, "evolution").is_none());
        assert!(object(&groups, "firefox").is_some());
        assert_eq!(p.notes.len(), 2, "{:?}", p.notes);
    }

    /// Everything is on the system, Brisk too — and still the Brisk menu does not
    /// get onto the panel: the MATE Menu takes its place (Wehrwolfmann, 24.11.2026).
    #[test]
    fn redmond_with_everything_installed_gets_mate_menu_instead_of_brisk() {
        let mut all = plain_mate();
        all.insert(BRISK_MENU_IID.into());
        all.insert(MATE_MENU_IID.into());
        all.insert("IndicatorAppletCompleteFactory::IndicatorAppletComplete".into());
        let p = prepare(REDMOND, &all, &|_: &str| true);
        assert!(p.changed);
        assert_eq!(p.notes, [tr("The Brisk menu is no longer supported — the MATE Menu will be used instead.")]);
        let groups = parse_keyfile(&p.text);
        let menu = object(&groups, "briskmenu").unwrap();
        assert_eq!(menu.get("applet-iid"), Some(MATE_MENU_IID));
        assert_eq!(menu.get("object-type"), Some("applet"));
        assert!(!p.text.contains("Brisk"), "{}", p.text);
        assert_eq!(groups.len(), 9);
    }

    /// Brisk is still installed, mate-menu is not: the built-in menu button, not
    /// Brisk and not an empty place.
    #[test]
    fn brisk_without_mate_menu_becomes_the_menu_button() {
        let mut all = plain_mate();
        all.insert(BRISK_MENU_IID.into());
        let p = prepare(REDMOND, &all, &|_: &str| true);
        let groups = parse_keyfile(&p.text);
        assert_eq!(object(&groups, "briskmenu").unwrap().get("object-type"), Some("menu"));
        assert!(!p.text.contains("Brisk"), "{}", p.text);
    }

    #[test]
    fn default_layout_needs_no_changes() {
        let p = prepare(DEFAULT, &plain_mate(), &|_: &str| true);
        assert!(!p.changed);
        assert!(p.notes.is_empty());
        assert_eq!(p.panels, 2);
    }

    #[test]
    fn linuxmint_without_mintmenu_gets_the_menu_button_and_keeps_its_clock() {
        let p = prepare(LINUXMINT, &plain_mate(), &|_: &str| true);
        let groups = parse_keyfile(&p.text);
        assert_eq!(object(&groups, "menu-bar").unwrap().get("object-type"), Some("menu"));
        assert_eq!(groups.iter().filter(|g| g.get("applet-iid") == Some(CLOCK_IID)).count(), 1);
    }

    #[test]
    fn missing_indicators_are_dropped_when_the_layout_has_a_clock() {
        let text = format!(
            "{LINUXMINT}\n[Object ind]\nobject-type=applet\napplet-iid=IndicatorAppletCompleteFactory::IndicatorAppletComplete\ntoplevel-id=bottom\nposition=0\n\n[Customsetting windowcontrollayout]\nmate-general=menu:minimize\n"
        );
        let p = prepare(&text, &plain_mate(), &|_: &str| true);
        let groups = parse_keyfile(&p.text);
        assert!(object(&groups, "ind").is_none());
        assert!(!p.text.contains("Customsetting"));
        assert_eq!(groups.iter().filter(|g| g.get("applet-iid") == Some(CLOCK_IID)).count(), 1);
    }

    #[test]
    fn setting_value_maps_back_to_the_layout_name() {
        assert_eq!(layout_id_from_setting("redmond"), "redmond");
        assert_eq!(layout_id_from_setting("/usr/share/mate-panel/layouts/fedora.layout"), "fedora");
        assert_eq!(layout_id_from_setting("/home/w/.local/share/mate-panel/layouts/rexon-redmond.layout"), "redmond");
    }

    #[test]
    fn auto_description_names_panel_sides() {
        assert!(auto_description(DEFAULT).contains(", "));
        assert_eq!(auto_description("[Object x]\nobject-type=menu\n"), tr("A panel layout without panels."));
    }

    // ── Snapshot, apply, restore on a substitute machine ────────────────

    #[derive(Default)]
    struct Machine {
        files: RefCell<BTreeMap<String, String>>,
        runs: RefCell<Vec<Vec<String>>>,
        dconf_fails: bool,
        /// `dconf load` of the snapshot fails (the snapshot is corrupted).
        snapshot_load_fails: bool,
    }

    impl LookIo for Machine {
        fn run(&self, argv: &[String]) -> (bool, String) {
            self.runs.borrow_mut().push(argv.to_vec());
            if self.snapshot_load_fails
                && argv.first().map(String::as_str) == Some("sh")
                && argv.last().is_some_and(|p| p.ends_with("mate-panel.dconf"))
            {
                return (false, "error: invalid key file".into());
            }
            match argv.first().map(String::as_str) {
                Some("gsettings") => (true, "'default'\n".into()),
                Some("dconf") if self.dconf_fails => (false, "dconf: no bus".into()),
                // The panel has already removed the applets: the object branch is empty.
                Some("dconf") if argv[1] != "dump" => (true, String::new()),
                Some("dconf") => (true, "[general]\ntoplevel-id-list=['top']\n".into()),
                _ => (true, String::new()),
            }
        }
        fn have(&self, tool: &str) -> bool {
            matches!(tool, "mate-panel" | "gsettings")
        }
        fn read(&self, path: &str) -> Option<String> {
            self.files.borrow().get(path).cloned()
        }
        fn write(&self, path: &str, text: &str) -> bool {
            self.files.borrow_mut().insert(path.into(), text.into());
            true
        }
        fn list_dir(&self, path: &str) -> Vec<String> {
            let prefix = format!("{}/", path.trim_end_matches('/'));
            self.files
                .borrow()
                .keys()
                .filter_map(|k| k.strip_prefix(&prefix))
                .filter(|rest| !rest.contains('/'))
                .map(str::to_string)
                .collect()
        }
    }

    fn machine() -> (Machine, PanelCtx) {
        let m = Machine::default();
        for (name, text) in [("redmond.layout", REDMOND), ("default.layout", DEFAULT), ("zeta.layout", DEFAULT)] {
            m.write(&format!("{SYSTEM_LAYOUTS_DIR}/{name}"), text);
        }
        m.write(&format!("{SYSTEM_APPLETS_DIR}/org.mate.panel.Wncklet.mate-panel-applet"), WNCKLET);
        m.write(
            &format!("{SYSTEM_APPLETS_DIR}/org.mate.panel.ClockApplet.mate-panel-applet"),
            "[Applet Factory]\nId=ClockAppletFactory\n\n[ClockApplet]\nName=Clock\n",
        );
        m.write(
            &format!("{SYSTEM_APPLETS_DIR}/org.mate.panel.NotificationAreaApplet.mate-panel-applet"),
            "[Applet Factory]\nId=NotificationAreaAppletFactory\n\n[NotificationArea]\nName=Notification Area\n",
        );
        let ctx = PanelCtx::new("mate", "/home/w", None, "/usr/share", "");
        // Our previous copy must not get into the list a second time.
        m.write(&format!("{}/rexon-redmond.layout", ctx.copies_dir()), "[Toplevel x]\n");
        (m, ctx)
    }

    #[test]
    fn state_lists_layouts_known_first_and_marks_current() {
        let (m, ctx) = machine();
        let st = read_state(&m, &ctx);
        assert!(st.available);
        let ids: Vec<&str> = st.layouts.iter().map(|l| l.id.as_str()).collect();
        assert_eq!(ids, ["redmond", "default", "zeta"]);
        assert!(st.layouts[1].current);
        assert_eq!(st.current_title, tr("Classic MATE"));
        assert_eq!(st.layouts[0].title, tr("Like Windows"));
        assert!(!st.layouts[0].notes.is_empty(), "without mate-menu and the indicators the layout must explain what is missing");
        assert!(!st.can_restore);
    }

    #[test]
    fn section_is_hidden_outside_mate() {
        let (m, mut ctx) = machine();
        ctx.de = "xfce".into();
        assert!(!read_state(&m, &ctx).available);
    }

    #[test]
    fn apply_saves_the_panel_and_uses_a_fixed_copy() {
        let (m, ctx) = machine();
        let msg = apply(&m, &ctx, "redmond");
        assert!(msg.starts_with('✅'), "{msg}");
        assert!(m.read(&ctx.backup_path()).unwrap().contains("toplevel-id-list"));
        let copy = format!("{}/rexon-redmond.layout", ctx.copies_dir());
        assert!(!m.read(&copy).unwrap().contains("relative-to-edge"));
        let runs = m.runs.borrow();
        assert_eq!(runs.last().unwrap(), &argv(&["mate-panel", "--layout", &copy, "--reset"]));
        // The snapshot comes before applying.
        let dump = runs.iter().position(|r| r[0] == "dconf").unwrap();
        assert!(dump < runs.len() - 1);
        drop(runs);
        assert!(read_state(&m, &ctx).can_restore);
    }

    #[test]
    fn apply_of_a_clean_system_layout_goes_by_name() {
        let (m, ctx) = machine();
        apply(&m, &ctx, "default");
        assert_eq!(m.runs.borrow().last().unwrap(), &argv(&["mate-panel", "--layout", "default", "--reset"]));
    }

    #[test]
    fn apply_refuses_without_a_snapshot() {
        let (mut m, ctx) = machine();
        m.dconf_fails = true;
        let msg = apply(&m, &ctx, "redmond");
        assert!(msg.starts_with("⚠️"), "{msg}");
        assert!(!m.runs.borrow().iter().any(|r| r[0] == "mate-panel"));
    }

    #[test]
    fn restore_resets_then_loads_the_snapshot() {
        let (m, ctx) = machine();
        assert!(restore(&m, &ctx).starts_with("⚠️"), "снимка ещё нет");
        m.write(&ctx.backup_path(), "[general]\n");
        assert!(restore(&m, &ctx).starts_with('✅'));
        let runs = m.runs.borrow();
        let load = runs.last().unwrap();
        assert_eq!(load[0], "sh");
        assert_eq!(load.last().unwrap(), &ctx.backup_path());
        let pos = |want: Vec<String>| runs.iter().position(|r| *r == want).expect("шаг есть");
        // First the panel removes the applets, then the branches are erased and the
        // snapshot is loaded.
        let emptied = pos(argv(&["dconf", "write", "/org/mate/panel/general/object-id-list", "@as []"]));
        let reset = pos(argv(&["dconf", "reset", "-f", "/org/mate/panel/objects/"]));
        assert!(emptied < reset);
    }

    /// The snapshot did not load — the panel returns to what it was before the
    /// click instead of staying empty and reset.
    #[test]
    fn failed_restore_puts_the_panel_back() {
        let (mut m, ctx) = machine();
        m.snapshot_load_fails = true;
        m.write(&ctx.backup_path(), "[general]\nbroken");
        assert!(restore(&m, &ctx).starts_with("⚠️"));
        let runs = m.runs.borrow();
        let loads: Vec<&Vec<String>> = runs.iter().filter(|r| r[0] == "sh").collect();
        assert_eq!(loads.len(), 2, "после неудачи нужна загрузка страховки: {runs:?}");
        let undo = loads[1].last().unwrap();
        assert_ne!(undo, &ctx.backup_path());
        // The insurance is what was there before the click (the `dconf dump` output).
        assert_eq!(m.read(undo).as_deref(), Some("[general]\ntoplevel-id-list=['top']\n"));
    }

    // ── Missing layout parts ──────────────────────────────────────────────

    /// The table "layout part → package per family → source", checked against the
    /// repositories on 19.10.2026 (archlinux.org and AUR RPC, packages.debian.org
    /// trixie, packages.ubuntu.com noble, Fedora mdapi f43, openSUSE Factory).
    #[test]
    fn parts_table_matches_the_repositories() {
        use PartSource::{Aur, Missing, Repo};
        let ind = "IndicatorAppletCompleteFactory::IndicatorAppletComplete";
        let mint = "MintMenuAppletFactory::MintMenuApplet";
        // The Brisk menu is never installed (24.11.2026) — in no family.
        for f in ["arch", "debian", "fedora", "suse"] {
            assert_eq!(part_source(BRISK_MENU_IID, f), Missing, "{f}");
            assert!(!all_part_packages(f).iter().any(|p| p.contains("brisk")), "{f}");
        }
        assert_eq!(part_source(MATE_MENU_IID, "arch"), Aur { package: "mate-menu", orphan: false });
        for f in ["debian", "fedora", "suse"] {
            assert_eq!(part_source(MATE_MENU_IID, f), Repo("mate-menu"), "{f}");
        }
        assert_eq!(part_source(ind, "arch"), Aur { package: "mate-indicator-applet", orphan: true });
        assert_eq!(part_source(ind, "debian"), Repo("mate-indicator-applet"));
        assert_eq!(part_source(ind, "suse"), Repo("mate-applet-indicator"));
        assert_eq!(part_source(mint, "arch"), Aur { package: "mintmenu", orphan: false });
        for f in ["debian", "fedora", "suse"] {
            assert_eq!(part_source(mint, f), Missing, "{f}");
        }
        assert_eq!(part_source("DriveMountAppletFactory::DriveMountApplet", "arch"), Repo("mate-applets"));
        assert_eq!(part_source("MateWindowPickerFactory::MateWindowPicker", "arch"), Repo("mate-netbook"));
        assert_eq!(part_source("Unknown::Thing", "arch"), Missing);
        // Every part that `prepare` replaces or drops is in the table.
        for iid in MENU_IIDS.iter().chain(INDICATOR_IIDS) {
            assert!(PANEL_PARTS.iter().any(|p| p.iid == *iid), "{iid}");
        }
    }

    #[test]
    fn missing_parts_of_like_windows_are_offered_by_family() {
        let (m, ctx) = machine();
        let installed = installed_applets(&m, &ctx.applet_dirs);
        let missing = missing_parts(REDMOND, &installed);
        // The layout's Brisk menu asks for the MATE Menu, not for Brisk.
        assert!(missing.contains(&MATE_MENU_IID.to_string()), "{missing:?}");
        assert!(!missing.iter().any(|i| i == BRISK_MENU_IID), "{missing:?}");
        assert!(missing.contains(&"IndicatorAppletCompleteFactory::IndicatorAppletComplete".to_string()));
        assert!(!missing.iter().any(|i| i.starts_with("Wncklet")), "стоящее не просится: {missing:?}");

        let arch = parts_offer(&missing, "arch");
        assert!(arch.aur.contains(&("mate-menu".to_string(), false)), "{arch:?}");
        assert!(!arch.aur.iter().any(|(p, _)| p.contains("brisk")), "{arch:?}");
        assert!(arch.aur.contains(&("mate-indicator-applet".to_string(), true)));
        assert!(arch.repo.contains(&"mate-applets".to_string()), "{arch:?}");
        let (title, text) = parts_question("redmond", &arch).expect("есть что собирать из AUR");
        assert!(!title.is_empty());
        assert!(text.contains("AUR"), "{text}");
        assert!(text.contains("mate-indicator-applet"), "сирота названа: {text}");
        // Only what comes from the repositories is installed by itself, without a question.
        let auto = arch.repo_only();
        assert!(auto.aur.is_empty() && auto.repo == arch.repo, "{auto:?}");
        assert!(!auto.labels.is_empty() && auto.labels.iter().all(|l| !arch.aur_labels.contains(l)), "{auto:?}");

        let debian = parts_offer(&missing, "debian");
        assert!(debian.aur.is_empty() && debian.repo.contains(&"mate-menu".to_string()), "{debian:?}");
        // Everything is in the repositories — there is no question, the parts install
        // themselves.
        assert!(parts_question("redmond", &debian).is_none());
        // Fedora: there are no indicators and no Mint menu — the question is only
        // about what exists.
        let fedora = parts_offer(&["MintMenuAppletFactory::MintMenuApplet".to_string()], "fedora");
        assert!(fedora.is_empty() && parts_question("redmond", &fedora).is_none());
        // An immutable system / an unknown package manager — do not offer.
        assert!(parts_offer(&missing, "").is_empty());
    }

    /// The parts are installed BEFORE applying, and the layout comes up with them;
    /// if they did not install, the layout is NOT applied ("all or nothing"), and it
    /// is said why.
    #[test]
    fn apply_with_parts_installs_first_all_or_nothing() {
        let (m, ctx) = machine();
        let offer = parts_offer(&[MATE_MENU_IID.to_string()], "debian");
        let mut calls = Vec::new();
        let out = apply_with_parts(&m, &ctx, "redmond", &offer, &mut |o: &PartsOffer| {
            calls.push(o.repo.clone());
            Ok(())
        });
        assert_eq!(calls, [vec!["mate-menu".to_string()]]);
        assert!(out.starts_with('✅'), "{out}");
        let (files, runs) = (m.files.borrow().clone(), m.runs.borrow().len());
        // A part without a stand-in (indicators from the repository) did not install.
        let indicators = parts_offer(&["IndicatorAppletCompleteFactory::IndicatorAppletComplete".to_string()], "debian");
        let out = apply_with_parts(&m, &ctx, "redmond", &indicators, &mut |_| Err("no network".into()));
        assert!(out.contains("no network") && !out.contains('✅'), "{out}");
        assert_eq!((m.files.borrow().clone(), m.runs.borrow().len()), (files, runs), "раскладка не применялась");
        // The password dialog was closed — advice to repeat, the reason is not repeated.
        let pw = crate::appearance_installer::password_not_confirmed();
        let out = apply_with_parts(&m, &ctx, "redmond", &offer, &mut |_| Err(pw.clone()));
        assert!(!out.contains(&pw) && !out.contains('✅') && out.contains(&offer.labels[0]), "{out}");
        // Nothing to install — the installer is not called.
        let mut called = false;
        apply_with_parts(&m, &ctx, "redmond", &PartsOffer::default(), &mut |_| {
            called = true;
            Ok(())
        });
        assert!(!called);
    }

    /// mate-menu did not install (the AUR package has been failing): the layout is
    /// still applied — with the built-in menu button — and the reason is said.
    /// Closing the password dialog still applies nothing.
    #[test]
    fn failed_mate_menu_falls_back_to_the_menu_button() {
        let (m, ctx) = machine();
        let offer = parts_offer(&[MATE_MENU_IID.to_string()], "arch");
        assert_eq!(offer.aur, [("mate-menu".to_string(), false)]);
        let out = apply_with_parts(&m, &ctx, "redmond", &offer, &mut |_| Err("gschemas.compiled exists in filesystem".into()));
        assert!(out.starts_with('✅'), "{out}");
        let note = trf(
            "The MATE Menu couldn't be installed (%1) — the standard MATE menu button is used instead.",
            &[&"gschemas.compiled exists in filesystem"],
        );
        assert!(out.ends_with(&note), "{out}");
        let copy = m.read(&format!("{}/rexon-redmond.layout", ctx.copies_dir())).unwrap();
        let groups = parse_keyfile(&copy);
        assert_eq!(object(&groups, "briskmenu").unwrap().get("object-type"), Some("menu"));

        let (m, ctx) = machine();
        let pw = crate::appearance_installer::password_not_confirmed();
        let out = apply_with_parts(&m, &ctx, "redmond", &offer, &mut |_| Err(pw.clone()));
        assert!(!out.contains('✅'), "{out}");
        assert!(!m.runs.borrow().iter().any(|r| r[0] == "mate-panel"));
    }

    /// The original panel (the snapshot before the first apply) is not overwritten
    /// by subsequent applies; after a restore it is taken again. A snapshot taken by
    /// an earlier release (without the mark) is the original.
    #[test]
    fn original_panel_survives_later_layouts_until_restored() {
        let (m, ctx) = machine();
        let backup = ctx.backup_path();
        let last = crate::backup::panel_last_snapshot_path(&backup);
        m.write(&backup, "[general]\nисходная\n");
        assert!(apply(&m, &ctx, "redmond").starts_with('✅'));
        assert!(apply(&m, &ctx, "default").starts_with('✅'));
        assert_eq!(m.read(&backup).as_deref(), Some("[general]\nисходная\n"), "исходная цела");
        assert_eq!(m.read(&last).as_deref(), Some("[general]\ntoplevel-id-list=['top']\n"));
        // "Restore the previous panel" loads the original.
        assert!(restore(&m, &ctx).starts_with('✅'));
        assert_eq!(m.runs.borrow().last().unwrap().last().unwrap(), &backup);
        // After a restore the first apply takes the original again…
        assert!(apply(&m, &ctx, "redmond").starts_with('✅'));
        assert_eq!(m.read(&backup).as_deref(), Some("[general]\ntoplevel-id-list=['top']\n"));
        // …and the next one no longer touches it.
        m.write(&backup, "[general]\nвторая исходная\n");
        assert!(apply(&m, &ctx, "default").starts_with('✅'));
        assert_eq!(m.read(&backup).as_deref(), Some("[general]\nвторая исходная\n"));
    }

    /// After a restore the "Save panel" copy is protected from applies again.
    #[test]
    fn manual_save_after_restore_is_kept_by_layouts() {
        let (m, ctx) = machine();
        let backup = ctx.backup_path();
        m.write(&backup, "[general]\n");
        assert!(restore(&m, &ctx).starts_with('✅'));
        assert!(save_snapshot(&m, &ctx).0);
        m.write(&backup, "[general]\nсохранённая\n");
        assert!(apply(&m, &ctx, "redmond").starts_with('✅'));
        assert_eq!(m.read(&backup).as_deref(), Some("[general]\nсохранённая\n"));
    }

    /// "Save panel" and "Restore the previous panel" see the same copy.
    #[test]
    fn saved_panel_is_the_same_copy_restore_uses() {
        let (m, ctx) = machine();
        let (ok, path) = save_snapshot(&m, &ctx);
        assert!(ok);
        assert_eq!(path, ctx.backup_path());
        assert_eq!(m.read(&path).as_deref(), Some("[general]\ntoplevel-id-list=['top']\n"));
        assert!(read_state(&m, &ctx).can_restore);
    }

    // ── Brisk menu on the live panel ──────────────────────────────────────

    const PANEL_WITH_BRISK: &str = "[general]
object-id-list=['briskmenu', 'window-list', 'clock']
toplevel-id-list=['bottom']

[objects/briskmenu]
applet-iid='BriskMenuFactory::BriskMenu'
locked=true
object-type='applet'
position=0
toplevel-id='bottom'

[objects/briskmenu/prefs]
label-visible=true

[objects/clock]
applet-iid='ClockAppletFactory::ClockApplet'
object-type='applet'
panel-right-stick=true
toplevel-id='bottom'
";

    #[test]
    fn brisk_on_the_panel_moves_to_mate_menu_in_place() {
        let m = brisk_migration(PANEL_WITH_BRISK, true).expect("the panel has Brisk");
        assert_eq!(m.replaced, ["briskmenu"]);
        let w = |k: &str, v: &str| argv(&["dconf", "write", &format!("/org/mate/panel/objects/briskmenu-menu/{k}"), v]);
        assert_eq!(
            m.commands,
            [
                w("object-type", "'applet'"),
                w("applet-iid", "'MateMenuAppletFactory::MateMenuApplet'"),
                w("toplevel-id", "'bottom'"),
                w("position", "0"),
                w("locked", "true"),
                argv(&[
                    "dconf",
                    "write",
                    "/org/mate/panel/general/object-id-list",
                    "['briskmenu-menu', 'window-list', 'clock']",
                ]),
                argv(&["dconf", "reset", "-f", "/org/mate/panel/objects/briskmenu/"]),
            ]
        );
    }

    #[test]
    fn brisk_without_mate_menu_becomes_the_builtin_button_on_the_panel() {
        let m = brisk_migration(PANEL_WITH_BRISK, false).unwrap();
        assert_eq!(m.commands[0], argv(&["dconf", "write", "/org/mate/panel/objects/briskmenu-menu/object-type", "'menu'"]));
        assert!(!m.commands.iter().any(|c| c.iter().any(|a| a.contains("applet-iid"))), "{:?}", m.commands);
    }

    #[test]
    fn no_brisk_no_changes_and_new_ids_do_not_clash() {
        assert!(brisk_migration("[general]\nobject-id-list=['clock']\n\n[objects/clock]\napplet-iid='ClockAppletFactory::ClockApplet'\n", true).is_none());
        assert!(brisk_migration("", true).is_none());
        // A taken name gets a number; a Brisk object outside the list is only erased
        // from dconf and does not get into the list.
        let dump = "[general]\nobject-id-list=['a']\n\n[objects/a]\napplet-iid='BriskMenuFactory::BriskMenu'\n\n[objects/a-menu]\nobject-type='menu'\n\n[objects/b]\napplet-iid='BriskMenuFactory::BriskMenu'\n";
        let m = brisk_migration(dump, true).unwrap();
        assert_eq!(m.replaced, ["a", "b"]);
        assert!(m.commands.contains(&argv(&["dconf", "write", "/org/mate/panel/general/object-id-list", "['a-menu-2']"])), "{:?}", m.commands);
        assert!(m.commands.contains(&argv(&["dconf", "reset", "-f", "/org/mate/panel/objects/b/"])));
        assert_eq!(gvariant_strings("@as []"), Vec::<String>::new());
        assert_eq!(gvariant_string_array(&[]), "@as []");
    }

}
