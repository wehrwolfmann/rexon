//! The desktop customization matrix: "type → where to put it → what to enable it with →
//! what to restart" — the single source of truth for the universal `.rxn`.
//!
//! Why a separate module. Before it, Rexon had TWO parallel type tables
//! (`constants.rs` — `.rxn` paths, `categories.rs` — the "Appearance" catalog), both
//! tailored to KDE and to "just copy the directory". For GNOME, Cinnamon,
//! XFCE, MATE and LXQt copying is NOT ENOUGH: installation means different things there —
//! a GNOME shell extension is enabled with `gnome-extensions enable` and requires
//! logging in again on Wayland, a Cinnamon applet is appended as a line to the
//! `org.cinnamon enabled-applets` list with its own instance id counter, an
//! xfwm4 theme is enabled with `xfconf-query`, a KWin script — with a key in `kwinrc` plus
//! `reconfigure` over D-Bus. Piling this up as `if` branches in the installer is the same
//! duplication of logic that rule #12 forbids; so the differences are described as
//! DATA here, and the installer (`ext_manager`) stays a single generic path.
//!
//! The table also covers what CANNOT be installed via `.rxn` (XFCE,
//! MATE, Budgie, LXQt applets — native plugins that must be compiled; a GDM theme —
//! repacking a `gresource` as root). Such rows are marked [`Support`] and do not
//! get into `VALID_TYPES`: the user is honestly told the reason, instead of
//! a silent "installed" after which nothing appears.
//!
//! Human labels for the developer documentation are derived FROM the machine
//! fields ([`Activate::hint`], [`Refresh::hint`]) — so the text does not drift apart
//! from what the installer actually does.
//!
//! ⚠️ The matrix describes ALL `.rxn` appearance types, not only those whose installation
//! it handles itself (`Setup::Matrix`). Previously "safe appearance" (GTK
//! themes, icons, cursors, fonts, wallpapers…) lived as a separate list in
//! `constants.rs` — two tables about the same thing, exactly what
//! rule #12 forbids. Now there is ONE list: `VALID_TYPES`, the "Appearance" category and
//! install paths are DERIVED from here.
//!
//! Each row answers TWO different questions, and both are mandatory:
//!   * [`Row::de`] — whose domain it is, i.e. where the file goes and what
//!     enables it (a GTK theme's directory is shared — [`De::Any`], a plasmoid's is
//!     KDE's);
//!   * [`Row::applies`] — on which desktops the type makes sense at all (a GTK theme
//!     works EVERYWHERE, including KDE — Firefox, GIMP, Inkscape live under Plasma,
//!     and Budgie's look is defined by it; a plasmoid — KDE only).
//!
//! These are different facts: a shared directory does not mean "works everywhere" (a Kvantum theme
//! lives in its own `~/.config/Kvantum`, but makes sense only where a Qt style
//! is chosen natively), and a KDE directory does not mean "KDE only".
//!
//! LANGUAGE. Texts for the buyer ([`De::display_label`], [`Applies::hint`],
//! [`Caveat::text`], [`unavailable_note`], [`condition_note`]) go through
//! the translation catalog (`crate::i18n`): the source language is English, Russian is in
//! `po/ru.po`. The matrix row fields (`what`, `note`, activation and
//! restart hints, refusal reasons, [`De::label`]) are a reference for the
//! developer: they are used to build `docs/de_customization_matrix.md`, they are
//! not shown to the buyer and therefore are plain English, outside the translation
//! catalog. If one of them is ever shown in the interface, it must move to `n_`/`tr`
//! like the texts above.

use crate::i18n::{n_, tr, trf};

// ============================================================================
// Desktops
// ============================================================================

/// The desktop a customization type is tied to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum De {
    Kde,
    Gnome,
    Cinnamon,
    Xfce,
    Mate,
    Budgie,
    /// Pantheon (elementary OS): Gala on Mutter, the look — via GNOME keys.
    Pantheon,
    /// Deepin (DDE): the look — the DDE service on the session bus and DConfig
    /// ([`crate::dde`]).
    Deepin,
    Lxqt,
    Cosmic,
    /// Compositors on wlr-layer-shell (sway, Hyprland, niri, river, labwc…).
    ///
    /// This is the canonical answer to the question "what kind of compositor is this". Next to it in
    /// the product live SIMILAR but different sets — they must not be merged, each
    /// answers its own question:
    ///
    /// * [`crate::appdefaults::is_tiling_wm`] — "has no native program set of its
    ///   own" (plus any UNknown desktop);
    /// * [`crate::wallpaper_backend::DAEMON_LAYER_SHELL_DESKTOPS`] — "the wallpaper
    ///   daemon will bring up layer-shell itself" (cosmic legitimately is there, while it is not
    ///   here, and labwc is not);
    /// * `wallpaper_engine._WLROOTS_DE` in Python — "a static picture is set by
    ///   an external swww/hyprpaper/swaybg/wbg daemon" (the widest: also wayfire);
    /// * `qt_desktops._TILING_IDS` — "which cards to show in the
    ///   "Tiling" section and in what order" (the contents are checked against the core by a guard).
    Wlroots,
    /// Works everywhere (shared freedesktop directories: icons, fonts, cursors).
    Any,
}

impl De {
    pub fn key(self) -> &'static str {
        match self {
            De::Kde => "kde",
            De::Gnome => "gnome",
            De::Cinnamon => "cinnamon",
            De::Xfce => "xfce",
            De::Mate => "mate",
            De::Budgie => "budgie",
            De::Pantheon => "pantheon",
            De::Deepin => "deepin",
            De::Lxqt => "lxqt",
            De::Cosmic => "cosmic",
            De::Wlroots => "wlroots",
            De::Any => "any",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            De::Kde => "KDE Plasma",
            De::Gnome => "GNOME",
            De::Cinnamon => "Cinnamon",
            De::Xfce => "XFCE",
            De::Mate => "MATE",
            De::Budgie => "Budgie",
            De::Pantheon => "Pantheon",
            De::Deepin => "Deepin",
            De::Lxqt => "LXQt",
            De::Cosmic => "COSMIC",
            De::Wlroots => "Tiling and Wayfire (sway/Hyprland/niri/River)",
            De::Any => "Any desktop",
        }
    }

    /// The label in the interface language (rule #13: to the person — in their language).
    /// [`De::label`] — the same label for the developer reference.
    pub fn display_label(self) -> String {
        match self {
            De::Wlroots => tr("Tiling and Wayfire (sway/Hyprland/niri/River)"),
            De::Any => tr("Any desktop"),
            // The rest are proper names, they are the same in all languages.
            other => other.label().to_string(),
        }
    }

    /// All real desktops of the matrix (without [`De::Any`] — that is not a desktop but
    /// "a shared directory"). The order is as in the enum.
    pub const ALL: &'static [De] = &[
        De::Kde,
        De::Gnome,
        De::Cinnamon,
        De::Xfce,
        De::Mate,
        De::Budgie,
        De::Pantheon,
        De::Deepin,
        De::Lxqt,
        De::Cosmic,
        De::Wlroots,
    ];
}

/// A Rexon desktop identifier (`system::DE_DATABASE`) → a matrix
/// desktop. Tiling ones are merged into [`De::Wlroots`]: they are styled the same way, and
/// are listed separately only in the interface — the buyer thinks of their own
/// desktop, not of a protocol name.
pub fn de_from_id(de_id: &str) -> Option<De> {
    Some(match de_id {
        "kde" => De::Kde,
        "gnome" => De::Gnome,
        "cinnamon-mint" | "cinnamon" => De::Cinnamon,
        "xfce" => De::Xfce,
        "mate" => De::Mate,
        "budgie" => De::Budgie,
        "pantheon" => De::Pantheon,
        "deepin" => De::Deepin,
        "lxqt" => De::Lxqt,
        "cosmic" => De::Cosmic,
        // wayfire is a wlroots compositor too; previously only the wallpaper engine
        // knew it, and with the shared desktop detection (`system`) it would
        // fall through to "unknown".
        "hyprland" | "sway" | "niri" | "river" | "labwc" | "wayfire" => De::Wlroots,
        _ => return None,
    })
}

// ============================================================================
// Applicability
// ============================================================================

/// Where a caveat applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaveatAt {
    /// Only on the listed desktops.
    In(&'static [De]),
    /// On all except the listed ones.
    Except(&'static [De]),
}

/// A caveat: the type works, but under a condition — and the condition must be named IN ADVANCE.
///
/// The second role next to [`unavailable_note`]: that one explains why the section is absent
/// entirely, this one — under what conditions it works. We will not hide what works instead
/// of explaining the condition: the person needs sound in a tiling
/// compositor and a Kvantum theme under GNOME, not an empty catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Caveat {
    pub at: CaveatAt,
    /// The condition text — a catalog `msgid` (rule #13: in the person's language);
    /// translated in [`Caveat::text`].
    pub msgid: &'static str,
}

impl Caveat {
    /// Whether the caveat applies on this desktop.
    pub fn covers(&self, de: De) -> bool {
        match self.at {
            CaveatAt::In(xs) => xs.contains(&de),
            CaveatAt::Except(xs) => de != De::Any && !xs.contains(&de),
        }
    }

    /// Desktops where the caveat applies — for the interface and documentation.
    pub fn list(&self) -> Vec<De> {
        De::ALL.iter().copied().filter(|d| self.covers(*d)).collect()
    }

    /// The condition text in the interface language.
    pub fn text(&self) -> String {
        tr(self.msgid)
    }
}

/// Desktops where an appearance type makes sense at all.
///
/// Answers the question "will we show this to the buyer", not "where will we put
/// the file": the latter is [`Row::de`]. When adding a new type, both must be answered —
/// the struct fields are mandatory, no defaults are provided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Applies {
    /// On all desktops Rexon can install.
    Everywhere,
    /// Everywhere except the listed ones.
    EverywhereExcept(&'static [De]),
    /// Only on the listed ones.
    Only(&'static [De]),
    /// Everywhere, but on some desktops — with a caveat: it works under a condition
    /// that must be named to the person (see [`Caveat`]).
    EverywhereWith(Caveat),
    /// Like [`Applies::EverywhereWith`], but on the listed desktops the type
    /// does not exist at all (why — [`unavailable_note`], its own reason in
    /// [`UNAVAILABLE_REASONS`]).
    EverywhereWithExcept(&'static [De], Caveat),
}

impl Applies {
    /// Whether the type makes sense on this desktop.
    pub fn covers(self, de: De) -> bool {
        match self {
            Applies::Everywhere | Applies::EverywhereWith(_) => de != De::Any,
            Applies::EverywhereExcept(xs) | Applies::EverywhereWithExcept(xs, _) => {
                de != De::Any && !xs.contains(&de)
            }
            Applies::Only(xs) => xs.contains(&de),
        }
    }

    /// The caveat for this desktop — `None` when there are no conditions.
    pub fn caveat(self, de: De) -> Option<Caveat> {
        match self {
            Applies::EverywhereWith(c) if c.covers(de) => Some(c),
            Applies::EverywhereWithExcept(xs, c) if c.covers(de) && !xs.contains(&de) => Some(c),
            _ => None,
        }
    }

    /// The list of applicable desktops — for the interface and documentation.
    pub fn list(self) -> Vec<De> {
        De::ALL.iter().copied().filter(|d| self.covers(*d)).collect()
    }

    /// The desktops listed comma-separated in the interface language.
    pub fn hint(self) -> String {
        let names = |xs: Vec<De>| -> String {
            xs.into_iter()
                .map(|d| d.display_label())
                .collect::<Vec<_>>()
                .join(", ")
        };
        match self {
            Applies::Everywhere => tr("in every desktop"),
            Applies::EverywhereWith(c) => trf(
                // TRANSLATORS: %1 is a list of desktops, e.g. “GNOME, XFCE”.
                "in every desktop (with a caveat in %1)",
                &[&names(c.list())],
            ),
            _ => names(self.list()),
        }
    }
}

// ============================================================================
// Who installs
// ============================================================================

/// Who performs the type's installation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Setup {
    /// The matrix's generic path: copy by [`Payload`] and enable with
    /// [`Activate`] — one code path for all desktops (rule #12).
    Matrix,
    /// The shared appearance installer: copying plus its own steps that cannot be
    /// expressed by the table (pkexec for login and boot screens,
    /// `~/.icons/default` for cursors, parsing theme bundles).
    Appearance,
}

// ============================================================================
// Support
// ============================================================================

/// Whether the type can be installed from `.rxn` with user rights.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Support {
    /// Installs from `.rxn` without root — the type is in `VALID_TYPES`.
    Rxn,
    /// Technically installable, but only as root (editing system directories).
    RootOnly(&'static str),
    /// Not distributed via `.rxn`: a native plugin must be built for the current
    /// version of the desktop.
    NeedsBuild(&'static str),
}

impl Support {
    pub fn installable(self) -> bool {
        matches!(self, Support::Rxn)
    }

    /// The refusal reason — developer-reference text for the matrix document
    /// (rule #13: explain rather than silently do nothing).
    pub fn reason(self) -> Option<&'static str> {
        match self {
            Support::Rxn => None,
            Support::RootOnly(r) | Support::NeedsBuild(r) => Some(r),
        }
    }
}

// ============================================================================
// What lies inside data/
// ============================================================================

/// The shape of the payload in `data/` of a `.rxn` package.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Payload {
    /// Each `data/` subfolder is a standalone object, the directory name = the object
    /// name (themes, look-and-feel, cursors).
    Dir,
    /// The destination directory name is taken from the metadata, not from the folder name:
    /// GNOME/Cinnamon require the directory to be named exactly as the `uuid`.
    DirUuid { meta: &'static str, field: &'static str },
    /// A KDE package: installed with `kpackagetool6 --type <ptype>`, the directory name is
    /// `KPlugin.Id` from `metadata.json`. Copying is the fallback path.
    KPackage { ptype: &'static str, id_field: &'static str },
    /// A theme "add-on" over the `~/.themes/<Name>` directory: the `.rxn` contains
    /// a ready theme directory, what matters is the `sub` subdirectory.
    ThemeSubdir { sub: &'static str },
    /// Individual files with the given extension (Nemo actions).
    Files { ext: &'static str },
}

// ============================================================================
// What to enable it with
// ============================================================================

/// The activation method after copying.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Activate {
    /// Nothing is needed — the object is picked up by itself.
    Auto,
    /// Rexon applies it itself — with the "Apply" button in the appearance catalog
    /// (rule #13: pressed the button — the theme is set, no commands by hand).
    /// The specific command depends on the desktop and lives in the theme installer.
    Rexon,
    /// Only manually from the desktop's settings (the user's choice, not ours
    /// to make for them).
    Manual(&'static str),
    /// `kwriteconfig6 --file <file> --group <group> --key <key> <name>`.
    KdeConfig {
        file: &'static str,
        group: &'static str,
        key: &'static str,
        /// An additional key=value pair of the same file/group (the splash
        /// engine: `Engine=KSplashQML`).
        extra: Option<(&'static str, &'static str)>,
    },
    /// The `<id>Enabled` key in `kwinrc` of the `Plugins` group (KWin scripts and effects).
    KwinPlugin,
    /// `gsettings set <schema> <key> '<name>'`.
    Gsettings { schema: &'static str, key: &'static str },
    /// Append an element to a `gsettings` list (applets/desklets/extensions).
    GsettingsList {
        schema: &'static str,
        key: &'static str,
        entry: EntryFmt,
        /// The instance id counter key (`next-applet-id`).
        counter: Option<&'static str>,
    },
    /// `gnome-extensions enable <uuid>`; the fallback — the
    /// `org.gnome.shell enabled-extensions` list.
    GnomeExtension,
    /// `xfconf-query -c <channel> -p <prop> -s <name>`.
    Xfconf { channel: &'static str, prop: &'static str },
    /// A key in an ini configuration file (`~/.config/lxqt/lxqt.conf`).
    IniKey { file: &'static str, group: &'static str, key: &'static str },
    /// `<theme><name>` in the `rc.xml` of the session's window manager — Openbox
    /// (`openbox/rc.xml`) or labwc (`labwc/rc.xml`), then its
    /// `--reconfigure`. Which one is decided by the session
    /// (`qt_settings::session_title_wm`), the file is edited by `crate::de_apply`.
    RcXmlTheme,
}

/// The format of a `gsettings` list element string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryFmt {
    /// Just the `uuid` (GNOME and Cinnamon extensions).
    Uuid,
    /// `panel1:<zone>:<order>:<uuid>:<instance number>` — a Cinnamon applet.
    CinnamonApplet,
    /// `<uuid>:<instance number>:<x>:<y>` — a Cinnamon desklet.
    CinnamonDesklet,
}

impl Activate {
    /// A human-readable "what to enable it with" — for the developer documentation.
    pub fn hint(self) -> String {
        match self {
            Activate::Auto => "picked up automatically".into(),
            Activate::Rexon => "Rexon applies it itself (the “Apply” button)".into(),
            Activate::Manual(where_) => format!("manually: {where_}"),
            Activate::KdeConfig { file, group, key, extra } => {
                let mut s = format!("kwriteconfig6 --file {file} --group {group} --key {key} <name>");
                if let Some((k, v)) = extra {
                    s.push_str(&format!(" (+ {k}={v})"));
                }
                s
            }
            Activate::KwinPlugin => {
                "kwriteconfig6 --file kwinrc --group Plugins --key <id>Enabled true".into()
            }
            Activate::Gsettings { schema, key } => {
                format!("gsettings set {schema} {key} '<name>'")
            }
            Activate::GsettingsList { schema, key, entry, counter } => {
                let e = match entry {
                    EntryFmt::Uuid => "<uuid>",
                    EntryFmt::CinnamonApplet => "panel1:<zone>:<order>:<uuid>:<number>",
                    EntryFmt::CinnamonDesklet => "<uuid>:<number>:<x>:<y>",
                };
                let c = counter
                    .map(|c| format!(" (+ counter {c})"))
                    .unwrap_or_default();
                format!("add “{e}” to the list {schema} {key}{c}")
            }
            Activate::GnomeExtension => "gnome-extensions enable <uuid>".into(),
            Activate::Xfconf { channel, prop } => {
                format!("xfconf-query -c {channel} -p {prop} -s <name>")
            }
            Activate::IniKey { file, group, key } => {
                format!("{file}: [{group}] {key}=<name>")
            }
            Activate::RcXmlTheme => {
                "node theme/name = <name> in openbox/rc.xml or labwc/rc.xml + --reconfigure".into()
            }
        }
    }
}

// ============================================================================
// What to restart
// ============================================================================

/// What needs restarting for the change to become visible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refresh {
    /// Nothing — applied on the fly.
    Nothing,
    /// `qdbus org.kde.KWin /KWin reconfigure`.
    KwinReconfigure,
    /// `plasmashell --replace` (restarting the Plasma shell).
    PlasmashellReplace,
    /// `cinnamon --replace` on X11; on Wayland — only logging in again.
    CinnamonReplace,
    /// GNOME: on X11 `Alt+F2 → r`, on Wayland the shell cannot be restarted —
    /// logging in to the session again is needed.
    GnomeShell,
    /// Restarting the LXQt panel.
    LxqtPanel,
    /// Applied at the next login.
    NextLogin,
    /// Applied at the next screen lock.
    NextLock,
}

impl Refresh {
    pub fn hint(self) -> &'static str {
        match self {
            Refresh::Nothing => "nothing, applied immediately",
            Refresh::KwinReconfigure => "qdbus org.kde.KWin /KWin reconfigure",
            Refresh::PlasmashellReplace => "plasmashell --replace",
            Refresh::CinnamonReplace => "cinnamon --replace (Wayland — log out and back in)",
            Refresh::GnomeShell => "Alt+F2 → r on X11; on Wayland — log out and back in",
            Refresh::LxqtPanel => "restart lxqt-panel",
            Refresh::NextLogin => "next login",
            Refresh::NextLock => "next screen lock",
        }
    }
}

// ============================================================================
// A matrix row
// ============================================================================

/// One matrix row: a customization type on a specific desktop.
#[derive(Debug, Clone, Copy)]
pub struct Row {
    /// The `.rxn` type (`VALID_TYPES`). An empty string — the type is not distributed via
    /// `.rxn` (see [`Row::support`]).
    pub kind: &'static str,
    /// Whose domain: where the file goes and what enables it. [`De::Any`] —
    /// a shared directory (freedesktop or the program's own). NOT to be confused with
    /// applicability: see [`Row::applies`].
    pub de: De,
    /// On which desktops the type makes sense. A separate fact from [`Row::de`]:
    /// a GTK theme lies in a shared directory and works everywhere, a Kvantum theme lies
    /// in a shared directory but makes sense only in KDE and LXQt.
    pub applies: Applies,
    /// What it is, in Russian (rule #13 — no jargon in the interface).
    pub what: &'static str,
    /// Where to put it: a path from `$HOME` (without the leading `~/`) or an absolute
    /// system path for rows that require root. Empty — there is nothing to install,
    /// the content is applied in place (wallpapers).
    pub dest: &'static str,
    pub payload: Payload,
    pub activate: Activate,
    pub refresh: Refresh,
    pub support: Support,
    /// Who handles installation: the matrix's generic path or the shared appearance
    /// installer.
    pub setup: Setup,
    /// Pitfalls that would otherwise surface for the user.
    pub note: &'static str,
}

impl Row {
    /// The absolute destination path from the given `$HOME`.
    pub fn dest_path(&self, home: &str) -> String {
        if self.dest.starts_with('/') {
            self.dest.to_string()
        } else {
            format!("{home}/{}", self.dest)
        }
    }

    /// Whether the type makes sense on this desktop.
    pub fn covers(&self, de: De) -> bool {
        self.applies.covers(de)
    }

    /// The condition under which the type works on this desktop (`None` — no
    /// conditions).
    pub fn caveat(&self, de: De) -> Option<Caveat> {
        self.applies.caveat(de)
    }
}

/// The full customization matrix: both what we install and what we do not.
///
/// The order is by desktop (KDE → GNOME → Cinnamon → XFCE → MATE → Budgie →
/// Pantheon → Deepin → LXQt → COSMIC → tiling), within a desktop from the most common to the rare.
pub const ROWS: &[Row] = &[
    // ── Shared by all desktops ──────────────────────────────────────────
    // freedesktop directories (and the programs' own directories): where to put it is
    // the same everywhere. Applicability is still stated explicitly: a shared directory does not
    // mean "works everywhere" (Kvantum).
    Row {
        kind: "theme-gtk",
        de: De::Any,
        applies: Applies::EverywhereWith(Caveat {
            at: CaveatAt::In(&[De::Pantheon]),
            msgid: n_("Pantheon's own settings service puts the elementary style back \
                 over the GTK theme: at every login, whenever you change the accent \
                 color and — with the automatic accent — whenever the wallpaper \
                 changes. Until then your theme stays."),
        }),
        what: "GTK theme (the look of GTK app windows and widgets)",
        dest: ".themes",
        payload: Payload::Dir,
        activate: Activate::Rexon,
        refresh: Refresh::Nothing,
        support: Support::Rxn,
        setup: Setup::Appearance,
        note: "Needed EVERYWHERE, including KDE: Firefox, GIMP and Inkscape live under \
               Plasma, and its system settings have the “GNOME/GTK Application \
               Style” module (kde-gtk-config). The look of Budgie is defined \
               precisely by the GTK theme. ⚠️ Pantheon: the \
               io.elementary.settings-daemon service (AccentColorManager) writes its \
               own io.elementary.stylesheet.<color> to org.gnome.desktop.interface \
               gtk-theme at every start, on every accent change and — with the \
               “auto” accent — on every wallpaper change; it has no off switch.",
    },
    Row {
        kind: "theme-icons",
        de: De::Any,
        applies: Applies::Everywhere,
        what: "Icon set",
        dest: ".local/share/icons",
        payload: Payload::Dir,
        activate: Activate::Rexon,
        refresh: Refresh::Nothing,
        support: Support::Rxn,
        setup: Setup::Appearance,
        note: "The directory is shared with cursors: both are “icon themes” per the \
               freedesktop specification.",
    },
    Row {
        kind: "theme-cursors",
        de: De::Any,
        applies: Applies::Everywhere,
        what: "Mouse pointers",
        dest: ".local/share/icons",
        payload: Payload::Dir,
        activate: Activate::Rexon,
        refresh: Refresh::Nothing,
        support: Support::Rxn,
        setup: Setup::Appearance,
        note: "Besides the choice in the desktop settings, ~/.icons/default is written \
               — otherwise apps on plain X11 and some Wayland clients keep the old \
               cursor.",
    },
    Row {
        kind: "theme-sounds",
        de: De::Any,
        applies: Applies::EverywhereWith(Caveat {
            at: CaveatAt::In(&[De::Wlroots]),
            msgid: n_("Event sounds are played by a notification daemon (swaync, \
                 mako, dunst) or by the app itself, not by the desktop shell. \
                 On tiling compositors you start that daemon yourself — until \
                 you do, the theme stays silent."),
        }),
        what: "Sound theme (system event sounds)",
        dest: ".local/share/sounds",
        payload: Payload::Dir,
        // Rexon applies the theme itself — both tabs, "Appearance" and
        // "Settings", write the desktop's native key with the shared plan
        // `qt_settings::sound_plan` (D11, desktop check of 02.10.2026:
        // the matrix still said "manually").
        activate: Activate::Rexon,
        refresh: Refresh::Nothing,
        support: Support::Rxn,
        setup: Setup::Appearance,
        note: "The directory is shared per the freedesktop specification, and the \
               sound theme does not belong to the shell: notification daemons and \
               libcanberra programs pick it up — exactly as GTK and Qt apps pick up \
               icons and cursors.",
    },
    Row {
        kind: "theme-kvantum",
        de: De::Any,
        // Deepin: a session-wide Qt style would take Chameleon away from DTK programs
        // (`themes::QtStyleWay::Deepin`) — the section is absent there, the reason is named.
        applies: Applies::EverywhereWithExcept(&[De::Deepin], Caveat {
            at: CaveatAt::Except(&[De::Kde, De::Lxqt, De::Deepin]),
            msgid: n_("Every Qt app (VLC, qBittorrent, Krita, OBS) will pick \
                 the theme up. This desktop has no Qt style switch of its own, \
                 so Rexon sets one up for you — but the change only shows up \
                 after your next login."),
        }),
        what: "Kvantum theme (the look of Qt apps)",
        dest: ".config/Kvantum",
        payload: Payload::Dir,
        activate: Activate::Rexon,
        refresh: Refresh::Nothing,
        support: Support::Rxn,
        setup: Setup::Appearance,
        note: "Kvantum is a Qt style engine (libkvantum.so in the Qt plugin \
               directory); it paints ANY Qt app regardless of the shell: under GNOME \
               that means VLC, qBittorrent, Krita, OBS. The difference between \
               desktops is not whether it works but WHAT selects the style: KDE — \
               widgetStyle in kdeglobals, LXQt — [Qt] style in lxqt.conf, the rest — \
               QT_STYLE_OVERRIDE in ~/.config/environment.d. Deepin has no such \
               section: a session-wide variable would take the Chameleon style away \
               from DTK programs.",
    },
    Row {
        kind: "theme-login-screen",
        de: De::Any,
        applies: Applies::Everywhere,
        what: "Login screen theme",
        dest: ".local/share/rexon/login-themes",
        payload: Payload::Dir,
        activate: Activate::Rexon,
        refresh: Refresh::NextLogin,
        support: Support::Rxn,
        setup: Setup::Appearance,
        note: "The login screen is drawn by the display manager, not by the desktop: \
               Rexon detects which one is installed (SDDM, GDM or LightDM) and asks \
               for the password only to write to the system directory.",
    },
    Row {
        kind: "theme-plymouth",
        de: De::Any,
        applies: Applies::Everywhere,
        what: "Boot splash (Plymouth)",
        dest: ".local/share/rexon/plymouth-themes",
        payload: Payload::Dir,
        activate: Activate::Rexon,
        refresh: Refresh::NextLogin,
        support: Support::Rxn,
        setup: Setup::Appearance,
        note: "Shown before login and does not depend on the desktop at all; \
               installing it requires the password and rebuilding the kernel image.",
    },
    Row {
        kind: "theme-conky",
        de: De::Any,
        applies: Applies::Everywhere,
        what: "Conky widget (clock, sensors on the desktop)",
        dest: ".config/conky",
        payload: Payload::Dir,
        activate: Activate::Manual("run conky with this config file"),
        refresh: Refresh::Nothing,
        support: Support::Rxn,
        setup: Setup::Appearance,
        note: "Conky is a separate program, not part of the desktop, so it also works \
               where the desktop has no widgets of its own.",
    },
    Row {
        kind: "theme-bundle",
        de: De::Any,
        applies: Applies::Everywhere,
        what: "Theme bundle (several themes in one package)",
        dest: ".local/share/rexon/bundles",
        payload: Payload::Dir,
        activate: Activate::Rexon,
        refresh: Refresh::Nothing,
        support: Support::Rxn,
        setup: Setup::Appearance,
        note: "Inside are ordinary types from this same table; each is installed by \
               its own path, and those unfit for the current desktop are skipped.",
    },
    Row {
        kind: "wallpaper-pack",
        de: De::Any,
        applies: Applies::Everywhere,
        what: "Wallpaper pack",
        dest: "",
        payload: Payload::Dir,
        activate: Activate::Rexon,
        refresh: Refresh::Nothing,
        support: Support::Rxn,
        setup: Setup::Appearance,
        note: "There is no install directory of its own: the wallpapers are applied \
               straight from the package — Rexon's wallpaper engine sets them, the \
               same way on every desktop.",
    },
    Row {
        kind: "wallpaper-video-adaptive",
        de: De::Any,
        applies: Applies::EverywhereWith(Caveat {
            at: CaveatAt::In(&[De::Pantheon]),
            msgid: n_("Live wallpapers work in the Pantheon “Classic Session” (X11). \
                 In the “Secure Session” (Wayland) the Gala window manager doesn't \
                 let other programs draw the desktop background, so only a still \
                 picture can be set there."),
        }),
        what: "Live wallpapers (video per resolution and language)",
        dest: ".local/share/rexon/wallpapers",
        payload: Payload::Dir,
        activate: Activate::Rexon,
        refresh: Refresh::Nothing,
        support: Support::Rxn,
        setup: Setup::Appearance,
        note: "Drawn by Rexon's wallpaper engine, not by the desktop, so it also works \
               in GNOME and Cinnamon (through our shell extension) and on tiling \
               compositors (the layer-shell layer). Not in Pantheon on Wayland: Gala \
               knows only its own pantheon-desktop-shell protocol, has no \
               wlr-layer-shell and does not load extension modules \
               (`wallpaper_backend::pick` → Unsupported).",
    },
    Row {
        kind: "font-pack",
        de: De::Any,
        applies: Applies::Everywhere,
        what: "Font pack",
        dest: ".local/share/fonts",
        payload: Payload::Dir,
        activate: Activate::Auto,
        refresh: Refresh::Nothing,
        support: Support::Rxn,
        setup: Setup::Appearance,
        note: "",
    },
    // ── KDE Plasma ──────────────────────────────────────────────────────
    Row {
        kind: "theme-kde-global",
        de: De::Kde,
        // Outside Plasma — part by part, by the same paths as "As in the
        // distribution" (`themes::apply_global_choice_plan`): colors, application
        // style, icons, cursors, fonts, title buttons; what does not
        // carry over, the "Apply "X"?" window names with a reason.
        applies: Applies::EverywhereWith(Caveat {
            at: CaveatAt::Except(&[De::Kde]),
            msgid: n_("A global theme is a Plasma package. Here Rexon applies its parts one by one — \
                 colors, application style, icons, pointers, fonts and title bar buttons; \
                 the window that opens before applying says what carries over."),
        }),
        what: "Full Plasma theme (Global Theme)",
        dest: ".local/share/plasma/look-and-feel",
        payload: Payload::Dir,
        activate: Activate::Rexon,
        refresh: Refresh::Nothing,
        support: Support::Rxn,
        setup: Setup::Appearance,
        note: "Changes everything at once — colors, the Plasma style, icons, the \
               splash screen and the lock screen; that is why Rexon takes a panel \
               snapshot before applying it (rule #13).",
    },
    Row {
        kind: "theme-kde-colors",
        de: De::Kde,
        // Outside Plasma the scheme is applied by Rexon's layer (`shell_palette::apply_scheme`):
        // kdeglobals for KDE programs, the qt6ct palette (in LXQt — its native
        // palette) for other Qt programs. In Deepin the section is absent — the reason
        // is in `UNAVAILABLE_REASONS`.
        applies: Applies::EverywhereWithExcept(&[De::Deepin], Caveat {
            at: CaveatAt::Except(&[De::Kde, De::Deepin]),
            // TRANSLATORS: Outside Plasma: the Color Schemes section note (GTK apps can't see KDE color schemes).
            msgid: n_("Outside Plasma, Rexon passes the scheme to KDE apps (Dolphin, \
                 Kate, Okular) and to the other Qt apps — some of them only pick it up \
                 after your next login. GTK apps (Firefox, GIMP) and most of the \
                 desktop's own apps don't read KDE color schemes and keep their own \
                 colors — for GTK apps, pick a matching theme in “GTK Themes”."),
        }),
        what: "Color scheme",
        dest: ".local/share/color-schemes",
        payload: Payload::Files { ext: ".colors" },
        activate: Activate::Rexon,
        refresh: Refresh::Nothing,
        support: Support::Rxn,
        setup: Setup::Appearance,
        note: "The package holds separate .colors files, not directories. Outside \
               Plasma, Rexon merges the scheme's colors into ~/.config/kdeglobals with \
               the same keys Plasma uses, plus [UiSettings] ColorScheme (otherwise KDE \
               programs with a “Color Scheme” menu set Breeze on their own), and the \
               Qt palette: LXQt — [Palette] in lxqt.conf, the rest — qt6ct/qt5ct \
               colors/*.conf. GTK programs do not read KDE schemes.",
    },
    Row {
        kind: "theme-kde-plasma",
        de: De::Kde,
        applies: Applies::Only(&[De::Kde]),
        what: "Plasma style (panel, menus, popups)",
        dest: ".local/share/plasma/desktoptheme",
        payload: Payload::Dir,
        activate: Activate::Rexon,
        refresh: Refresh::Nothing,
        support: Support::Rxn,
        setup: Setup::Appearance,
        note: "",
    },
    Row {
        kind: "theme-window-decoration",
        de: De::Kde,
        applies: Applies::Only(&[De::Kde]),
        what: "Window decorations (Aurorae)",
        dest: ".local/share/aurorae/themes",
        payload: Payload::Dir,
        activate: Activate::Rexon,
        refresh: Refresh::KwinReconfigure,
        support: Support::Rxn,
        setup: Setup::Appearance,
        note: "Aurorae is specifically KWin's decoration engine; on other desktops \
               frames are drawn by their own means (xfwm4, Marco, GTK).",
    },
    Row {
        kind: "theme-plasmoid",
        de: De::Kde,
        applies: Applies::Only(&[De::Kde]),
        what: "Plasma widget (plasmoid)",
        dest: ".local/share/plasma/plasmoids",
        payload: Payload::KPackage {
            ptype: "Plasma/Applet",
            id_field: "KPlugin.Id",
        },
        activate: Activate::Manual("add the widget to a panel or the desktop"),
        refresh: Refresh::Nothing,
        support: Support::Rxn,
        setup: Setup::Appearance,
        note: "In GNOME the role of plasmoids is played by shell extensions, in \
               Cinnamon — by applets and desklets: these are DIFFERENT types, not \
               compatible with each other.",
    },
    Row {
        kind: "theme-kde-splash",
        de: De::Kde,
        applies: Applies::Only(&[De::Kde]),
        what: "Session splash screen (KSplash)",
        dest: ".local/share/plasma/look-and-feel",
        payload: Payload::Dir,
        activate: Activate::KdeConfig {
            file: "ksplashrc",
            group: "KSplash",
            key: "Theme",
            extra: Some(("Engine", "KSplashQML")),
        },
        refresh: Refresh::NextLogin,
        support: Support::Rxn,
        setup: Setup::Matrix,
        note: "KDE has no separate package type for it: a splash screen is a \
               look-and-feel package with a single contents/splash directory.",
    },
    Row {
        kind: "theme-kde-lockscreen",
        de: De::Kde,
        applies: Applies::Only(&[De::Kde]),
        what: "Lock screen",
        dest: ".local/share/plasma/look-and-feel",
        payload: Payload::Dir,
        activate: Activate::KdeConfig {
            file: "kscreenlockerrc",
            group: "Greeter",
            key: "Theme",
            extra: None,
        },
        refresh: Refresh::NextLock,
        support: Support::Rxn,
        setup: Setup::Matrix,
        note: "The lock screen has no category of its own in the KDE Store either — it \
               is always part of a look-and-feel package (contents/lockscreen).",
    },
    Row {
        kind: "kde-kwin-script",
        de: De::Kde,
        applies: Applies::Only(&[De::Kde]),
        what: "KWin script (window behavior, tiling, gestures)",
        dest: ".local/share/kwin/scripts",
        payload: Payload::KPackage { ptype: "KWin/Script", id_field: "KPlugin.Id" },
        activate: Activate::KwinPlugin,
        refresh: Refresh::KwinReconfigure,
        support: Support::Rxn,
        setup: Setup::Matrix,
        note: "The key name in kwinrc is not the directory name but KPlugin.Id from metadata.json.",
    },
    Row {
        kind: "kde-kwin-effect",
        de: De::Kde,
        applies: Applies::Only(&[De::Kde]),
        what: "KWin desktop effect",
        dest: ".local/share/kwin/effects",
        payload: Payload::KPackage { ptype: "KWin/Effect", id_field: "KPlugin.Id" },
        activate: Activate::KwinPlugin,
        refresh: Refresh::KwinReconfigure,
        support: Support::Rxn,
        setup: Setup::Matrix,
        note: "QML effects load without building; C++ effects are native plugins and \
               are not distributed via .rxn.",
    },
    Row {
        kind: "kde-window-switcher",
        de: De::Kde,
        applies: Applies::Only(&[De::Kde]),
        what: "Window switcher (Alt+Tab)",
        dest: ".local/share/kwin/tabbox",
        payload: Payload::KPackage { ptype: "KWin/WindowSwitcher", id_field: "KPlugin.Id" },
        activate: Activate::KdeConfig {
            file: "kwinrc",
            group: "TabBox",
            key: "LayoutName",
            extra: None,
        },
        refresh: Refresh::KwinReconfigure,
        support: Support::Rxn,
        setup: Setup::Matrix,
        note: "",
    },
    Row {
        kind: "kde-wallpaper-plugin",
        de: De::Kde,
        applies: Applies::Only(&[De::Kde]),
        what: "Plasma wallpaper plugin (a custom background engine)",
        dest: ".local/share/plasma/wallpapers",
        payload: Payload::KPackage { ptype: "Plasma/Wallpaper", id_field: "KPlugin.Id" },
        activate: Activate::Manual(
            "right-click the desktop → “Configure Desktop” → wallpaper type",
        ),
        refresh: Refresh::PlasmashellReplace,
        support: Support::Rxn,
        setup: Setup::Matrix,
        note: "The plugin choice is stored in the configuration of EACH containment \
               (plasma-org.kde.plasma.desktop-appletsrc); switching it automatically \
               is unsafe — it can wipe the desktop's settings.",
    },
    // ── GNOME ───────────────────────────────────────────────────────────
    Row {
        kind: "gnome-shell-extension",
        de: De::Gnome,
        applies: Applies::Only(&[De::Gnome]),
        what: "GNOME Shell extension",
        dest: ".local/share/gnome-shell/extensions",
        payload: Payload::DirUuid { meta: "metadata.json", field: "uuid" },
        activate: Activate::GnomeExtension,
        refresh: Refresh::GnomeShell,
        support: Support::Rxn,
        setup: Setup::Matrix,
        note: "The directory must be named exactly after the uuid. Settings schemas \
               must be compiled (glib-compile-schemas schemas/). If the shell version \
               is missing from shell-version, the extension silently fails to load.",
    },
    Row {
        kind: "theme-gnome-shell",
        de: De::Gnome,
        applies: Applies::Only(&[De::Gnome]),
        what: "GNOME Shell theme (top bar, menus)",
        dest: ".themes",
        payload: Payload::ThemeSubdir { sub: "gnome-shell" },
        activate: Activate::Gsettings {
            schema: "org.gnome.shell.extensions.user-theme",
            key: "name",
        },
        refresh: Refresh::Nothing,
        support: Support::Rxn,
        setup: Setup::Matrix,
        note: "Works only with the User Themes extension enabled \
               (user-theme@gnome-shell-extensions.gcampax.github.com) — without it the \
               key exists but the theme is not applied.",
    },
    Row {
        kind: "",
        de: De::Gnome,
        applies: Applies::Only(&[De::Gnome]),
        what: "GDM login screen theme",
        dest: "/usr/share/gnome-shell/gnome-shell-theme.gresource",
        payload: Payload::ThemeSubdir { sub: "gnome-shell" },
        activate: Activate::Manual("replace the system gresource"),
        refresh: Refresh::NextLogin,
        support: Support::RootOnly(
            "the GDM theme is baked into a binary gresource: the system file has to be \
             unpacked, its styles replaced, rebuilt with glib-compile-resources and \
             put into /usr/share as root",
        ),
        setup: Setup::Matrix,
        note: "A gnome-shell update overwrites the replaced file — this breaks the \
               login screen until the theme is reinstalled.",
    },
    // ── Cinnamon ────────────────────────────────────────────────────────
    Row {
        kind: "cinnamon-applet",
        de: De::Cinnamon,
        applies: Applies::Only(&[De::Cinnamon]),
        what: "Cinnamon panel applet",
        dest: ".local/share/cinnamon/applets",
        payload: Payload::DirUuid { meta: "metadata.json", field: "uuid" },
        activate: Activate::GsettingsList {
            schema: "org.cinnamon",
            key: "enabled-applets",
            entry: EntryFmt::CinnamonApplet,
            counter: Some("next-applet-id"),
        },
        refresh: Refresh::CinnamonReplace,
        support: Support::Rxn,
        setup: Setup::Matrix,
        note: "Each instance has its own number; the next-applet-id counter must be \
               incremented, otherwise the next applet gets the same number and the \
               settings of the two applets get mixed up.",
    },
    Row {
        kind: "cinnamon-desklet",
        de: De::Cinnamon,
        applies: Applies::Only(&[De::Cinnamon]),
        what: "Desklet (a widget on the desktop)",
        dest: ".local/share/cinnamon/desklets",
        payload: Payload::DirUuid { meta: "metadata.json", field: "uuid" },
        activate: Activate::GsettingsList {
            schema: "org.cinnamon",
            key: "enabled-desklets",
            entry: EntryFmt::CinnamonDesklet,
            counter: Some("next-desklet-id"),
        },
        refresh: Refresh::CinnamonReplace,
        support: Support::Rxn,
        setup: Setup::Matrix,
        note: "The entry stores coordinates on the desktop — we place it in the \
               top-left corner with an offset so the desklet does not end up under the \
               panel.",
    },
    Row {
        kind: "cinnamon-extension",
        de: De::Cinnamon,
        applies: Applies::Only(&[De::Cinnamon]),
        what: "Cinnamon extension",
        dest: ".local/share/cinnamon/extensions",
        payload: Payload::DirUuid { meta: "metadata.json", field: "uuid" },
        activate: Activate::GsettingsList {
            schema: "org.cinnamon",
            key: "enabled-extensions",
            entry: EntryFmt::Uuid,
            counter: None,
        },
        refresh: Refresh::CinnamonReplace,
        support: Support::Rxn,
        setup: Setup::Matrix,
        note: "Cinnamon applets, desklets and extensions are THREE different types \
               with different directories and different enable lists.",
    },
    Row {
        kind: "theme-cinnamon",
        de: De::Cinnamon,
        applies: Applies::Only(&[De::Cinnamon]),
        what: "Cinnamon Shell theme",
        dest: ".themes",
        payload: Payload::ThemeSubdir { sub: "cinnamon" },
        activate: Activate::Gsettings { schema: "org.cinnamon.theme", key: "name" },
        refresh: Refresh::Nothing,
        support: Support::Rxn,
        setup: Setup::Matrix,
        note: "cinnamon.css is incompatible with gnome-shell.css: they share almost no \
               class names, so a GNOME theme in Cinnamon gives a bare look.",
    },
    Row {
        kind: "nemo-action",
        de: De::Cinnamon,
        applies: Applies::Only(&[De::Cinnamon]),
        what: "Context menu action for the Nemo file manager",
        dest: ".local/share/nemo/actions",
        payload: Payload::Files { ext: ".nemo_action" },
        activate: Activate::Auto,
        refresh: Refresh::Nothing,
        support: Support::Rxn,
        setup: Setup::Matrix,
        note: "Nemo watches the directory — the action appears without a restart.",
    },
    // ── XFCE ────────────────────────────────────────────────────────────
    Row {
        kind: "theme-xfwm4",
        de: De::Xfce,
        applies: Applies::Only(&[De::Xfce]),
        what: "xfwm4 window decoration theme",
        dest: ".themes",
        payload: Payload::ThemeSubdir { sub: "xfwm4" },
        activate: Activate::Xfconf { channel: "xfwm4", prop: "/general/theme" },
        refresh: Refresh::Nothing,
        support: Support::Rxn,
        setup: Setup::Matrix,
        note: "The XFCE window theme is separate from the GTK theme: they are chosen \
               by two different settings, and a .rxn can carry both.",
    },
    Row {
        kind: "",
        de: De::Xfce,
        applies: Applies::Only(&[De::Xfce]),
        what: "XFCE panel plugin",
        dest: "/usr/lib/xfce4/panel/plugins",
        payload: Payload::Dir,
        activate: Activate::Manual("add the plugin to the panel"),
        refresh: Refresh::Nothing,
        support: Support::NeedsBuild(
            "an XFCE panel plugin is a native .so library built for a specific \
             xfce4-panel version; it has to be compiled and put into a system \
             directory",
        ),
        setup: Setup::Matrix,
        note: "The exception is external script plugins (genmon, generic monitor): \
               that is not a plugin but a setting of an already installed plugin.",
    },
    // ── MATE ────────────────────────────────────────────────────────────
    Row {
        kind: "theme-marco",
        de: De::Mate,
        applies: Applies::Only(&[De::Mate]),
        what: "Marco window decoration theme",
        dest: ".themes",
        payload: Payload::ThemeSubdir { sub: "metacity-1" },
        activate: Activate::Gsettings { schema: "org.mate.Marco.general", key: "theme" },
        refresh: Refresh::Nothing,
        support: Support::Rxn,
        setup: Setup::Matrix,
        note: "Marco is a fork of Metacity, which is why the theme directory is called \
               metacity-1; the same themes work in MATE and in old GNOME 2. Cinnamon \
               has not read them since version 5.4: Muffin takes its frames from the \
               GTK theme.",
    },
    Row {
        kind: "",
        de: De::Mate,
        applies: Applies::Only(&[De::Mate]),
        what: "MATE panel applet",
        dest: "/usr/share/mate-panel/applets",
        payload: Payload::Dir,
        activate: Activate::Manual("add the applet to the panel"),
        refresh: Refresh::Nothing,
        support: Support::NeedsBuild(
            "a MATE applet needs a .mate-panel-applet descriptor and a D-Bus service \
             in /usr/share/dbus-1/services — both directories are system ones, root \
             only",
        ),
        setup: Setup::Matrix,
        note: "",
    },
    // ── Budgie ──────────────────────────────────────────────────────────
    Row {
        kind: "",
        de: De::Budgie,
        applies: Applies::Only(&[De::Budgie]),
        what: "Budgie panel applet",
        dest: "/usr/lib/budgie-desktop/plugins",
        payload: Payload::Dir,
        activate: Activate::Manual("add the applet to the panel"),
        refresh: Refresh::Nothing,
        support: Support::NeedsBuild(
            "Budgie applets are libpeas plugins built for a specific budgie-desktop \
             version; the directory is a system one",
        ),
        setup: Setup::Matrix,
        note: "The look of Budgie is defined by the GTK theme — and that one installs \
               via .rxn (the “GTK theme” type).",
    },
    // ── Pantheon ────────────────────────────────────────────────────────
    Row {
        kind: "",
        de: De::Pantheon,
        applies: Applies::Only(&[De::Pantheon]),
        what: "Wingpanel indicator",
        dest: "/usr/lib/wingpanel",
        payload: Payload::Dir,
        activate: Activate::Manual("the panel picks indicators up by itself"),
        refresh: Refresh::NextLogin,
        support: Support::NeedsBuild(
            "Wingpanel indicators are Vala shared libraries built for a specific \
             wingpanel version; the directory is a system one",
        ),
        setup: Setup::Matrix,
        note: "Pantheon has no shell themes of its own: the look is set by the GTK \
               theme (io.elementary.stylesheet.<accent>), the accent \
               (io.elementary.settings-daemon.interface accent-color, including \
               “auto” — from the wallpaper) and the scheduled dark mode \
               (io.elementary.settings-daemon.prefers-color-scheme). The dock is Plank \
               (net.launchpad.plank.dock.settings, themes in \
               ~/.local/share/plank/themes) — not yet distributed as a separate .rxn \
               type.",
    },
    // ── Deepin ──────────────────────────────────────────────────────────
    Row {
        kind: "",
        de: De::Deepin,
        applies: Applies::Only(&[De::Deepin]),
        what: "Deepin global theme",
        dest: ".local/share/deepin-themes",
        payload: Payload::Dir,
        activate: Activate::Manual("Control Center → Personalization → Theme"),
        refresh: Refresh::Nothing,
        support: Support::NeedsBuild(
            "a Deepin global theme (index.theme with a [Deepin Theme] section) refers \
             to window, icon and cursor themes by name; it is not yet distributed as a \
             separate .rxn type",
        ),
        setup: Setup::Matrix,
        note: "Rexon supports Deepin only on Treeland (a Wayland session that Rexon \
               installs itself); the X11 session with deepin-kwin and other Deepin \
               sessions get an honest refusal with a migration path. In Treeland the \
               GTK, icon and cursor themes, fonts, corners, transparency and window \
               accent are set only by the Control Center (Treeland's own privileged \
               protocol) — Rexon names where to change them. Desktop and lock screen \
               wallpapers go through the Control Center; sounds — through Deepin's own \
               DConfig keys. The live wallpaper daemon does not bring up its \
               background layer in Treeland (DDE is not on its list).",
    },
    // ── LXQt ────────────────────────────────────────────────────────────
    Row {
        kind: "theme-lxqt",
        de: De::Lxqt,
        applies: Applies::Only(&[De::Lxqt]),
        what: "LXQt theme (panel and shell styles)",
        dest: ".local/share/lxqt/themes",
        payload: Payload::Dir,
        activate: Activate::IniKey {
            file: ".config/lxqt/lxqt.conf",
            group: "General",
            key: "theme",
        },
        refresh: Refresh::LxqtPanel,
        support: Support::Rxn,
        setup: Setup::Matrix,
        note: "lxqt-panel.qss is mandatory in the theme directory — without it the \
               panel keeps the old style.",
    },
    // Openbox window frames — labwc reads them too (labwc-theme(5): a theme is looked up
    // in `<name>/labwc/`, then in `<name>/openbox-3/`). Openbox is the window manager of
    // LXQt on X11, labwc — on Wayland and a separate session; among the tiling ones
    // the section is visible only in labwc (`categories::category_available`).
    Row {
        kind: "theme-openbox",
        de: De::Lxqt,
        applies: Applies::Only(&[De::Lxqt, De::Wlroots]),
        what: "Openbox and labwc window frame theme",
        dest: ".themes",
        payload: Payload::ThemeSubdir { sub: "openbox-3" },
        activate: Activate::RcXmlTheme,
        refresh: Refresh::Nothing,
        support: Support::Rxn,
        setup: Setup::Matrix,
        note: "An Openbox frame theme is themerc plus .xbm button images in openbox-3; \
               labwc understands the same format. A user rc.xml replaces the system \
               one entirely, so if there is none it is created as a copy of the system \
               one.",
    },
    Row {
        kind: "",
        de: De::Lxqt,
        applies: Applies::Only(&[De::Lxqt]),
        what: "LXQt panel plugin",
        dest: "/usr/lib/lxqt-panel",
        payload: Payload::Dir,
        activate: Activate::Manual("add the widget to the panel"),
        refresh: Refresh::LxqtPanel,
        support: Support::NeedsBuild(
            "an LXQt panel plugin is a C++ library plus a .desktop descriptor, built \
             for a specific lxqt-panel version",
        ),
        setup: Setup::Matrix,
        note: "",
    },
    // ── COSMIC and tiling ───────────────────────────────────────────────
    Row {
        kind: "",
        de: De::Cosmic,
        applies: Applies::Only(&[De::Cosmic]),
        what: "COSMIC theme",
        dest: ".config/cosmic",
        payload: Payload::Dir,
        activate: Activate::Manual("COSMIC appearance settings"),
        refresh: Refresh::Nothing,
        support: Support::NeedsBuild(
            "the COSMIC theme format (RON files in ~/.config/cosmic) still changes \
             from release to release — too early to pin it down in .rxn",
        ),
        setup: Setup::Matrix,
        note: "COSMIC wallpapers and icons are already set today — with native keys: \
               the picture in com.system76.CosmicBackground/v1/all (drawn by \
               cosmic-bg), the icon set in com.system76.CosmicTk/v1/icon_theme.",
    },
    Row {
        kind: "",
        de: De::Wlroots,
        applies: Applies::Only(&[De::Wlroots]),
        what: "Tiling compositor configuration",
        dest: ".config",
        payload: Payload::Dir,
        activate: Activate::Manual("edit the compositor's personal config"),
        refresh: Refresh::NextLogin,
        support: Support::NeedsBuild(
            "on sway/Hyprland/niri the look is defined by a personal config file, not \
             by an installable package: replacing it with a package would mean \
             overwriting someone's settings and leaving the user without a desktop",
        ),
        setup: Setup::Matrix,
        note: "Wallpapers and the panel are installed separately here (layer-shell); \
               that works independently of the config.",
    },
];

// ============================================================================
// Access to the table
// ============================================================================

/// A matrix row by `.rxn` type. Rows without a type (unsupported) are never
/// returned.
pub fn row(kind: &str) -> Option<&'static Row> {
    if kind.is_empty() {
        return None;
    }
    ROWS.iter().find(|r| r.kind == kind)
}

/// A row for the generic desktop installer: only types whose installation
/// the matrix handles itself (`Setup::Matrix`). The rest of the appearance is installed by the shared
/// theme installer — it has its own steps (a password for system directories, parsing
/// bundles), which the table cannot express.
pub fn de_managed_row(kind: &str) -> Option<&'static Row> {
    row(kind).filter(|r| matches!(r.setup, Setup::Matrix))
}

/// All `.rxn` appearance types the matrix describes and Rexon installs (in
/// declaration order). The source of `VALID_TYPES` and the "Appearance" category.
pub fn kinds() -> Vec<&'static str> {
    ROWS.iter()
        .filter(|r| r.support.installable() && !r.kind.is_empty())
        .map(|r| r.kind)
        .collect()
}

/// Types the matrix's generic path installs and enables (`DE_CUSTOMIZATION_TYPES`).
pub fn de_managed_kinds() -> Vec<&'static str> {
    ROWS.iter()
        .filter(|r| {
            r.support.installable() && !r.kind.is_empty() && matches!(r.setup, Setup::Matrix)
        })
        .map(|r| r.kind)
        .collect()
}

/// All rows applicable on the given desktop (including non-installable ones —
/// they are shown by the "what else exists" section and the refusal explanation).
///
/// What is checked is APPLICABILITY, not "whose domain": KDE gets the
/// GTK theme too (a shared directory, needed everywhere), GNOME does not get a plasmoid.
pub fn rows_for(de: De) -> Vec<&'static Row> {
    ROWS.iter().filter(|r| r.applies.covers(de)).collect()
}

/// Rows whose domain is the given desktop (`Row::de`). Needed for the per-desktop
/// breakdown in the documentation, where "where to put it" matters, not "where it makes
/// sense".
pub fn rows_hosted_by(de: De) -> Vec<&'static Row> {
    ROWS.iter().filter(|r| r.de == de).collect()
}

/// A type's install directory relative to `$HOME` — the source for `INSTALL_PATHS`.
/// `None` — the type is not in the matrix OR it has no install directory at all
/// (wallpapers are applied in place).
pub fn dest_suffix(kind: &str) -> Option<&'static str> {
    row(kind).map(|r| r.dest).filter(|d| !d.is_empty())
}

/// Specific reasons why a type is absent on a desktop — where the generic "this does
/// nothing here" would be untrue (it would do something, but break something else).
pub const UNAVAILABLE_REASONS: &[(&str, De, &str)] = &[
    (
        "theme-kvantum",
        De::Deepin,
        // TRANSLATORS: Why the Kvantum / Application Style sections are missing in Deepin.
        n_("Deepin's own apps (Control Center, file manager and the rest) draw \
            themselves with Deepin's Chameleon style. Switching the Qt style for the \
            whole session would take it away from them and break their look, so in \
            Deepin Rexon leaves the Qt style alone."),
    ),
    (
        "theme-kde-colors",
        De::Deepin,
        // TRANSLATORS: Why the Color Schemes section is missing in Deepin.
        n_("Deepin colors its apps itself, with its Chameleon style and its own \
            accent color in the Control Center. A KDE color scheme would only reach \
            a few KDE apps and leave the rest of the desktop mismatched, so in Deepin \
            Rexon leaves the colors to Deepin."),
    ),
    // Metacity/Marco and xfwm4 frames in Cinnamon: the generic "this exists only in
    // MATE" would suggest that Cinnamon has no frames at all. It does, but since
    // Cinnamon 5.4 (Muffin rebuilt on Mutter 3.36) they are drawn by the GTK theme:
    // Muffin reads `org.cinnamon.desktop.interface gtk-theme`, does not load
    // `metacity-1` themes, and the "Window borders" choice was removed from the theme settings
    // (cinnamon bef2930e). The key `org.cinnamon.desktop.wm.preferences theme`
    // remained in the schema, but only the fallback copy of the high-contrast mode
    // reads it.
    ("theme-marco", De::Cinnamon, CINNAMON_BORDERS_FROM_GTK),
    ("theme-xfwm4", De::Cinnamon, CINNAMON_BORDERS_FROM_GTK),
];

// TRANSLATORS: Why the Metacity/Marco and xfwm4 window border sections are missing in Cinnamon. “GTK Themes” is the Rexon section name, “Applications” the item in Cinnamon's own Themes settings.
const CINNAMON_BORDERS_FROM_GTK: &str = n_("In Cinnamon, window borders come from the GTK theme: since Cinnamon 5.4 \
     its window manager Muffin no longer loads separate border themes (Metacity, Marco, xfwm4). \
     To change the borders, pick a theme in “GTK Themes” — it's the same as “Applications” in \
     Cinnamon's own Themes settings.");

/// Why this appearance is absent on the chosen desktop — text for the person
/// (rule #13: explain rather than show an empty screen). `None` — the type
/// works on this desktop, nothing to explain.
///
/// `here` — what the buyer calls the desktop ("Sway", not "Tiling"):
/// they chose their desktop, not a group of compositors, and must see exactly it
/// in the explanation. Empty — the group label is substituted.
pub fn unavailable_note(kind: &str, de: De, here: &str) -> Option<String> {
    let row = row(kind)?;
    if row.applies.covers(de) {
        return None;
    }
    if let Some((_, _, why)) = UNAVAILABLE_REASONS.iter().find(|(k, d, _)| *k == kind && *d == de) {
        return Some(tr(why));
    }
    let where_ = row.applies.hint();
    let here = if here.is_empty() { de.display_label() } else { here.to_string() };
    Some(trf(
        // TRANSLATORS: %1 is a list of desktops, %2 the desktop selected now.
        "This kind of customization only exists in: %1.\n\
         Your selected desktop is %2, where it simply does nothing, so we \
         don't offer it.",
        &[&where_, &here],
    ))
}

/// Under what condition this type works on the chosen desktop — text for the
/// person. `None` — there are no conditions, nothing to say.
///
/// The difference from [`unavailable_note`]: there the section is ABSENT and the reason is explained,
/// here the section EXISTS and what is needed for it to work is explained. Hiding
/// what works instead of explaining the condition is exactly the mistake because of which
/// sound themes were not shown to tiling ones, and the Kvantum theme — to everyone except KDE
/// and LXQt.
pub fn condition_note(kind: &str, de: De) -> Option<String> {
    Some(row(kind)?.caveat(de)?.text())
}

/// Compositors Rexon cannot pass session variables to: their
/// settings have no place for them, and they do not read `~/.config/environment.d` (D1,
/// desktop check of 02.10.2026; who reads what —
/// `compositor_autostart::sync_session_env`).
const NO_SESSION_ENV_IDS: &[&str] = &["sway", "river", "wayfire"];

// TRANSLATORS: Appended to the Qt-style caveat on window managers Rexon can't pass environment variables to. %1 is the window manager (sway, river…).
const NO_SESSION_ENV_NOTE: &str = n_("%1 is the exception: it has no setting Rexon could put this in. \
     Start %1 through uwsm, or add the lines from \
     ~/.config/environment.d/rexon-qt-style.conf to its environment yourself.");

/// [`condition_note`] by desktop identifier (`sway`, `xfce`…): where
/// session variables do not reach the compositor, the caveat says so
/// directly instead of promising "after the next login".
pub fn condition_note_id(kind: &str, de_id: &str) -> Option<String> {
    let note = condition_note(kind, de_from_id(de_id)?)?;
    // A color scheme outside Plasma also drives Qt programs through the variable
    // `QT_QPA_PLATFORMTHEME` in the same file (`shell_palette::plan_scheme_layer`).
    if matches!(kind, "theme-kvantum" | "theme-kde-colors") && NO_SESSION_ENV_IDS.contains(&de_id) {
        return Some(format!("{note} {}", crate::i18n::trf(NO_SESSION_ENV_NOTE, &[&de_id])));
    }
    Some(note)
}

// ============================================================================
// Activation scripts
// ============================================================================
//
// gsettings lists cannot be changed with one command (there is no list-add), and the pure
// core has nothing to read the current value with — so the change is done as
// a small idempotent shell script, as was already done for
// Plymouth and SDDM. The scripts are pure functions: their output is checked by tests.

/// Escaping a value for shell single quotes.
fn sq(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// The shared part: read a gsettings list into the variable `items`, one
/// element per line (element quotes are preserved).
fn read_list_snippet(schema: &str, key: &str) -> String {
    format!(
        "cur=$(gsettings get {schema} {key} 2>/dev/null || echo '@as []')\n\
         items=$(printf '%s' \"$cur\" | sed 's/^@as //; s/^\\[//; s/\\]$//' \
         | tr ',' '\\n' | sed 's/^ *//; s/ *$//' | grep -v '^$')\n"
    )
}

/// Script: add an element to a gsettings list if it is not there yet.
///
/// `dup_marker` — the substring by which an already added element is looked up (for
/// Cinnamon applets it is `:uuid:`, otherwise the uuid itself): a repeated installation must
/// not multiply duplicates in the panel.
pub fn gsettings_list_add_script(
    schema: &str,
    key: &str,
    entry: &str,
    dup_marker: &str,
) -> String {
    format!(
        "{read}\
         if printf '%s\\n' \"$items\" | grep -qF {marker}; then exit 0; fi\n\
         items=$(printf '%s\\n%s' \"$items\" {entry} | grep -v '^$')\n\
         new=$(printf '%s' \"$items\" | paste -sd, -)\n\
         gsettings set {schema} {key} \"[$new]\"\n",
        read = read_list_snippet(schema, key),
        marker = sq(dup_marker),
        entry = sq(&format!("'{entry}'")),
    )
}

/// Script: remove from a gsettings list all elements containing a substring.
pub fn gsettings_list_remove_script(schema: &str, key: &str, marker: &str) -> String {
    format!(
        "{read}\
         items=$(printf '%s\\n' \"$items\" | grep -vF {marker} | grep -v '^$')\n\
         new=$(printf '%s' \"$items\" | paste -sd, -)\n\
         gsettings set {schema} {key} \"[$new]\"\n",
        read = read_list_snippet(schema, key),
        marker = sq(marker),
    )
}

/// Script: remove the element `entry` from a gsettings list — only an exact
/// match (`'a@b'` does not touch `'xa@b'`), and write the key only if
/// the element was there: an untouched key stays at its default value.
pub fn gsettings_list_drop_exact_script(schema: &str, key: &str, entry: &str) -> String {
    format!(
        "{read}\
         if printf '%s\\n' \"$items\" | grep -qxF {quoted}; then\n\
         \x20 items=$(printf '%s\\n' \"$items\" | grep -vxF {quoted} | grep -v '^$')\n\
         \x20 new=$(printf '%s' \"$items\" | paste -sd, -)\n\
         \x20 gsettings set {schema} {key} \"[$new]\"\n\
         fi\n",
        read = read_list_snippet(schema, key),
        quoted = sq(&format!("'{entry}'")),
    )
}

/// The script enabling a Cinnamon applet — at the START of the right zone of the first
/// panel, as the Cinnamon settings window places it (Wehrwolfmann's decision of
/// 20.10.2026): applets of the same zone shift by one place, the new one takes
/// place zero. The instance number comes from the `next-applet-id` counter, but not
/// less than "the largest taken + 1" (for stock entries the counter can be 0 while
/// numbers are taken — the settings of two applets would stick together); the counter
/// is incremented. The same rules as `cinnamon_spices::applet_enable_front`.
pub fn cinnamon_applet_enable_script(uuid: &str) -> String {
    format!(
        "{read}\
         if printf '%s\\n' \"$items\" | grep -qF {marker}; then exit 0; fi\n\
         id=$(gsettings get org.cinnamon next-applet-id 2>/dev/null || echo 0)\n\
         case \"$id\" in ''|*[!0-9]*) id=0;; esac\n\
         max=$(printf '%s\\n' \"$items\" | tr -d \"'\" | awk -F: 'BEGIN{{m=-1}} $5 ~ /^[0-9]+$/ {{if ($5+0>m) m=$5+0}} END{{print m}}')\n\
         if [ \"$id\" -le \"$max\" ]; then id=$((max + 1)); fi\n\
         items=$(printf '%s\\n' \"$items\" | awk -F: 'BEGIN{{OFS=\":\"}} $1==\"\\047panel1\" && $2==\"right\" && $3 ~ /^[0-9]+$/ {{$3=$3+1}} {{print}}')\n\
         entry=\"'panel1:right:0:{uuid}:$id'\"\n\
         items=$(printf '%s\\n%s' \"$items\" \"$entry\" | grep -v '^$')\n\
         new=$(printf '%s' \"$items\" | paste -sd, -)\n\
         gsettings set org.cinnamon enabled-applets \"[$new]\"\n\
         gsettings set org.cinnamon next-applet-id $((id + 1))\n",
        read = read_list_snippet("org.cinnamon", "enabled-applets"),
        marker = sq(&format!(":{uuid}:")),
    )
}

/// The script enabling a Cinnamon desklet (the coordinates are the top left corner with
/// an offset, so the widget does not end up under the panel).
pub fn cinnamon_desklet_enable_script(uuid: &str) -> String {
    format!(
        "{read}\
         if printf '%s\\n' \"$items\" | grep -qF {marker}; then exit 0; fi\n\
         id=$(gsettings get org.cinnamon next-desklet-id 2>/dev/null || echo 0)\n\
         entry=\"'{uuid}:$id:100:100'\"\n\
         items=$(printf '%s\\n%s' \"$items\" \"$entry\" | grep -v '^$')\n\
         new=$(printf '%s' \"$items\" | paste -sd, -)\n\
         gsettings set org.cinnamon enabled-desklets \"[$new]\"\n\
         gsettings set org.cinnamon next-desklet-id $((id + 1))\n",
        read = read_list_snippet("org.cinnamon", "enabled-desklets"),
        marker = sq(&format!("'{uuid}:")),
    )
}

/// The script enabling a GNOME extension: first the standard utility, and if it is
/// missing — editing the `enabled-extensions` list. It also warns
/// if the current shell version is not in `shell-version` (otherwise the extension
/// silently does not load, and the person thinks the installation did not work).
pub fn gnome_extension_enable_script(uuid: &str, dest_dir: &str) -> String {
    // The person sees the warning (the install step output), so it comes from
    // the translation catalog. In the script it stands in shell double quotes:
    // the translation is escaped, and the variable `$ver` takes the place of `%1`.
    let warning = tr(
        // TRANSLATORS: %1 is a GNOME version number.
        "WARNING: the extension doesn't declare support for GNOME %1 — it \
         may not start",
    )
    .replace('\\', "\\\\")
    .replace('"', "\\\"")
    .replace('`', "\\`")
    .replace('$', "\\$")
    .replace("%1", "$ver");
    format!(
        "meta={meta}\n\
         if command -v gnome-shell >/dev/null 2>&1 && [ -f \"$meta\" ]; then\n\
         \x20 ver=$(gnome-shell --version 2>/dev/null | sed 's/[^0-9.]//g' | cut -d. -f1)\n\
         \x20 if [ -n \"$ver\" ] && ! grep -q \"\\\"$ver\" \"$meta\"; then\n\
         \x20   echo \"{warning}\" >&2\n\
         \x20 fi\n\
         fi\n\
         if command -v glib-compile-schemas >/dev/null 2>&1 && [ -d {schemas} ]; then\n\
         \x20 glib-compile-schemas {schemas} >/dev/null 2>&1 || true\n\
         fi\n\
         if command -v gnome-extensions >/dev/null 2>&1; then\n\
         \x20 gnome-extensions enable {uuid_q} && exit 0\n\
         fi\n\
         {undisable}\
         {fallback}",
        meta = sq(&format!("{dest_dir}/metadata.json")),
        schemas = sq(&format!("{dest_dir}/schemas")),
        uuid_q = sq(uuid),
        // First from `disabled-extensions`: it is stronger than `enabled-extensions`,
        // and a system one disabled earlier would otherwise stay disabled.
        undisable = gsettings_list_drop_exact_script("org.gnome.shell", "disabled-extensions", uuid),
        fallback =
            gsettings_list_add_script("org.gnome.shell", "enabled-extensions", uuid, uuid),
    )
}

/// The script disabling a GNOME extension (the reverse of the previous one): first
/// the standard utility; without it — the settings lists themselves, as GNOME edits them:
/// the uuid leaves `enabled-extensions` and goes into `disabled-extensions`.
/// The latter is needed for system extensions enabled by default — they are not in
/// their own `enabled-extensions` (they are enabled by the session mode or the default
/// value), and only `disabled-extensions` turns them off. Reversible:
/// the enabling script removes the uuid from there.
pub fn gnome_extension_disable_script(uuid: &str) -> String {
    format!(
        "if command -v gnome-extensions >/dev/null 2>&1; then\n\
         \x20 gnome-extensions disable {uuid_q} 2>/dev/null && exit 0\n\
         fi\n\
         {unenable}\
         {disable}",
        uuid_q = sq(uuid),
        unenable = gsettings_list_drop_exact_script("org.gnome.shell", "enabled-extensions", uuid),
        disable =
            gsettings_list_add_script("org.gnome.shell", "disabled-extensions", uuid, &format!("'{uuid}'")),
    )
}

/// The script writing a key into an ini configuration file (LXQt): the section is created,
/// an existing key is replaced, the file is created if missing.
pub fn ini_set_script(path: &str, group: &str, key: &str, value: &str) -> String {
    format!(
        "f={path}\n\
         mkdir -p \"$(dirname \"$f\")\"\n\
         [ -f \"$f\" ] || printf '[%s]\\n' {group} > \"$f\"\n\
         grep -q \"^\\[{group}\\]\" \"$f\" || printf '\\n[%s]\\n' {group} >> \"$f\"\n\
         if grep -q \"^{key}=\" \"$f\"; then\n\
         \x20 sed -i \"s|^{key}=.*|{key}={value}|\" \"$f\"\n\
         else\n\
         \x20 sed -i \"0,/^\\[{group}\\]/s||[{group}]\\n{key}={value}|\" \"$f\"\n\
         fi\n",
        path = sq(path),
        group = group,
        key = key,
        value = value,
    )
}

/// The script resetting a KDE key if it points to the object being removed.
///
/// This is not cosmetics but foolproofing (rule #13): if the lock screen
/// package is removed while `kscreenlockerrc` still points to it, the lock
/// screen will not build — the person will be locked out in front of a black screen.
pub fn kde_config_reset_if_script(
    file: &str,
    group: &str,
    key: &str,
    value: &str,
) -> String {
    format!(
        "cur=$(kreadconfig6 --file {file} --group {group} --key {key} 2>/dev/null)\n\
         if [ \"$cur\" = {value} ]; then\n\
         \x20 kwriteconfig6 --file {file} --group {group} --key {key} --delete\n\
         fi\n",
        file = sq(file),
        group = sq(group),
        key = sq(key),
        value = sq(value),
    )
}

/// The script resetting a scalar gsettings key if it points to our
/// theme: a blind reset would wipe out a theme the user chose themselves.
pub fn gsettings_reset_if_script(schema: &str, key: &str, value: &str) -> String {
    format!(
        "cur=$(gsettings get {schema} {key} 2>/dev/null | sed \"s/^'//; s/'$//\")\n\
         if [ \"$cur\" = {value} ]; then gsettings reset {schema} {key}; fi\n",
        value = sq(value),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_installable_row_has_kind_and_dest() {
        for r in ROWS {
            if r.support.installable() {
                assert!(!r.kind.is_empty(), "installable row without a type: {}", r.what);
                assert!(!r.dest.starts_with('/'), "{}: a user type cannot install into a system directory", r.kind);
            } else {
                assert!(r.kind.is_empty(), "non-installable row with a type: {}", r.kind);
                assert!(r.support.reason().is_some());
            }
        }
    }

    /// Each row answers BOTH questions: where to put it and where it makes
    /// sense. The second cannot be forgotten — the field is mandatory; here we check
    /// that the answer is also meaningful.
    #[test]
    fn every_row_answers_both_questions() {
        for r in ROWS {
            let list = r.applies.list();
            assert!(
                !list.is_empty(),
                "{}: applicability is empty — is the type needed nowhere?",
                if r.kind.is_empty() { r.what } else { r.kind }
            );
            if r.de != De::Any {
                assert!(
                    r.applies.covers(r.de),
                    "{}: lives in the {} directory but does not apply there",
                    r.what,
                    r.de.label()
                );
            }
        }
    }

    /// A GTK theme is needed EVERYWHERE, including KDE (Firefox, GIMP, Inkscape under Plasma
    /// plus the standard kde-gtk-config module), and Budgie's look is defined
    /// by it. Removing it from KDE is a regression, so we check it explicitly.
    #[test]
    fn gtk_theme_applies_everywhere() {
        let gtk = row("theme-gtk").unwrap();
        for de in De::ALL {
            assert!(gtk.covers(*de), "the GTK theme is missing from {}", de.label());
        }
        assert!(row("theme-plasmoid").unwrap().covers(De::Kde));
        assert!(!row("theme-plasmoid").unwrap().covers(De::Gnome));
    }

    #[test]
    fn rows_for_uses_applicability() {
        let kde: Vec<&str> = rows_for(De::Kde).iter().map(|r| r.kind).collect();
        assert!(kde.contains(&"theme-gtk"));
        assert!(kde.contains(&"theme-plasmoid"));
        let gnome: Vec<&str> = rows_for(De::Gnome).iter().map(|r| r.kind).collect();
        assert!(gnome.contains(&"theme-gtk"));
        assert!(!gnome.contains(&"theme-plasmoid"));
        // Tiling: we do not distribute their own config, but wallpapers, fonts and sounds —
        // by all means (a sound theme is played by the notification daemon, not the shell).
        let tiling: Vec<&str> = rows_for(De::Wlroots).iter().map(|r| r.kind).collect();
        assert!(tiling.contains(&"wallpaper-pack"));
        assert!(tiling.contains(&"font-pack"));
        assert!(tiling.contains(&"theme-sounds"));
    }

    /// Kvantum is a Qt style engine, not part of a shell: Qt applications (VLC,
    /// qBittorrent, Krita, OBS) live on any desktop. Hiding it from
    /// GNOME is the same mistake as hiding the GTK theme from KDE.
    ///
    /// The only exception is Deepin: there a session-wide Qt style would take
    /// Chameleon away from DTK programs (`themes::QtStyleWay::Deepin`), and the section is
    /// hidden with its own reason, not with the generic "this does nothing here".
    #[test]
    fn kvantum_applies_everywhere() {
        let kv = row("theme-kvantum").unwrap();
        for de in De::ALL {
            if *de == De::Deepin {
                assert!(!kv.covers(*de), "Kvantum in Deepin would break Chameleon");
                assert!(UNAVAILABLE_REASONS.iter().any(|(k, d, _)| *k == "theme-kvantum" && *d == De::Deepin));
                continue;
            }
            assert!(kv.covers(*de), "Kvantum is missing from {}", de.label());
        }
        // Where the Qt style is chosen natively — no caveat; where it is not — there is one.
        assert!(kv.caveat(De::Kde).is_none());
        assert!(kv.caveat(De::Lxqt).is_none());
        for de in [De::Gnome, De::Xfce, De::Wlroots, De::Cosmic] {
            assert!(kv.caveat(de).is_some(), "{} without a caveat", de.label());
        }
    }

    /// A sound theme lies in a shared freedesktop directory and does not belong to the
    /// shell — so it is applicable everywhere, but on tiling ones with a caveat.
    #[test]
    fn sounds_apply_everywhere_with_caveat_in_tiling() {
        let s = row("theme-sounds").unwrap();
        for de in De::ALL {
            assert!(s.covers(*de), "sounds are missing from {}", de.label());
        }
        // The language is the source one: Russian is checked by `tests/categories_parity.rs`
        // against the real `po/ru.po`.
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let note = condition_note("theme-sounds", De::Wlroots).unwrap();
        assert!(note.contains("swaync"), "{note}");
        assert!(condition_note("theme-sounds", De::Kde).is_none());
        assert!(condition_note("theme-icons", De::Wlroots).is_none());
        // Rexon applies it, not the person by hand (D11).
        assert_eq!(s.activate, Activate::Rexon);
    }

    #[test]
    fn unavailable_note_explains_and_stays_quiet_when_fine() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        assert!(unavailable_note("theme-gtk", De::Kde, "").is_none());
        let note = unavailable_note("theme-plasmoid", De::Gnome, "").unwrap();
        assert!(note.contains("KDE Plasma") && note.contains("GNOME"), "{note}");
        // The person chose Sway — no need to tell them about "tiling".
        let sway = unavailable_note("theme-cinnamon", De::Wlroots, "Sway").unwrap();
        assert!(sway.contains("Sway") && !sway.contains("Tiling"), "{sway}");
    }

    /// Marco and xfwm4 frames in Cinnamon: not "this exists only in MATE/XFCE",
    /// but where to go — Cinnamon's frames are drawn by the GTK theme.
    #[test]
    fn cinnamon_borders_point_to_gtk_theme() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        for kind in ["theme-marco", "theme-xfwm4"] {
            assert!(!row(kind).unwrap().covers(De::Cinnamon), "{kind}");
            let note = unavailable_note(kind, De::Cinnamon, "Cinnamon").unwrap();
            assert!(note.contains("GTK Themes") && note.contains("Muffin"), "{kind}: {note}");
            assert!(!note.contains("only exists in"), "{kind}: {note}");
        }
        // The same — on the sidebar section screen.
        for key in ["marco", "xfwm4"] {
            assert!(!crate::categories::category_available(key, "cinnamon"), "{key}");
            let note = crate::categories::category_note(std::path::Path::new("/h"), key, "cinnamon").unwrap();
            assert!(note.contains("GTK Themes") && !note.contains("only exists in"), "{key}: {note}");
        }
        // On other desktops — the old generic explanation.
        let gnome = unavailable_note("theme-marco", De::Gnome, "").unwrap();
        assert!(gnome.contains("only exists in") && gnome.contains("MATE"), "{gnome}");
    }

    #[test]
    fn de_ids_map_to_matrix() {
        assert_eq!(de_from_id("cinnamon-mint"), Some(De::Cinnamon));
        for id in ["hyprland", "sway", "niri", "river", "labwc", "wayfire"] {
            assert_eq!(de_from_id(id), Some(De::Wlroots), "{id}");
        }
        assert_eq!(de_from_id("unknown-wm"), None);
    }

    #[test]
    fn kinds_are_unique() {
        let k = kinds();
        let mut sorted = k.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(k.len(), sorted.len(), "duplicate types in the matrix");
    }

    #[test]
    fn lookup_ignores_empty_kind() {
        assert!(row("").is_none());
        assert!(row("kde-kwin-script").is_some());
        assert_eq!(dest_suffix("cinnamon-applet"), Some(".local/share/cinnamon/applets"));
    }

    #[test]
    fn dest_path_expands_home() {
        let r = row("theme-lxqt").unwrap();
        assert_eq!(r.dest_path("/home/t"), "/home/t/.local/share/lxqt/themes");
    }

    #[test]
    fn hints_describe_real_commands() {
        let r = row("theme-xfwm4").unwrap();
        assert_eq!(
            r.activate.hint(),
            "xfconf-query -c xfwm4 -p /general/theme -s <name>"
        );
        assert_eq!(r.refresh.hint(), "nothing, applied immediately");
    }

    #[test]
    fn applet_script_is_idempotent_and_bumps_counter() {
        let s = cinnamon_applet_enable_script("weather@mockturtl");
        assert!(s.contains("':weather@mockturtl:'"));
        assert!(s.contains("next-applet-id $((id + 1))"));
        assert!(s.contains("panel1:right:0:weather@mockturtl:$id"), "at the start of the right zone");
        assert!(s.contains("{$3=$3+1}"), "zone neighbours are shifted");
    }

    /// The applet script run by real bash with a fake `gsettings`: the new one goes to
    /// the start of the right zone, the neighbors are shifted, the number is not taken, the counter grew.
    #[test]
    fn applet_script_runs_and_puts_new_applet_first() {
        use std::os::unix::fs::PermissionsExt;
        let have = |c: &str| std::process::Command::new("sh").arg("-c").arg(format!("command -v {c}")).output().is_ok_and(|o| o.status.success());
        if !(have("bash") && have("awk") && have("paste")) {
            return;
        }
        let dir = std::env::temp_dir().join(format!("rexon-applet-script-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let fake = dir.join("gsettings");
        std::fs::write(
            &fake,
            "#!/bin/sh\n\
             if [ \"$1\" = get ] && [ \"$3\" = enabled-applets ]; then\n\
             echo \"['panel1:left:0:menu@cinnamon.org', 'panel1:right:0:systray@cinnamon.org:3', 'panel1:right:1:calendar@cinnamon.org:7', 'panel2:right:0:other@x:2']\"; exit 0; fi\n\
             if [ \"$1\" = get ]; then echo 0; exit 0; fi\n\
             echo \"$@\" >> \"$(dirname \"$0\")/log\"\n",
        )
        .unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let path = format!("{}:{}", dir.display(), std::env::var("PATH").unwrap_or_default());
        let out = std::process::Command::new("bash")
            .arg("-c")
            .arg(cinnamon_applet_enable_script("weather@mockturtl"))
            .env("PATH", path)
            .output()
            .unwrap();
        let log = std::fs::read_to_string(dir.join("log")).unwrap_or_default();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        assert!(
            log.contains(
                "set org.cinnamon enabled-applets ['panel1:left:0:menu@cinnamon.org','panel1:right:1:systray@cinnamon.org:3',\
                 'panel1:right:2:calendar@cinnamon.org:7','panel2:right:0:other@x:2','panel1:right:0:weather@mockturtl:8']"
            ),
            "{log}"
        );
        assert!(log.contains("set org.cinnamon next-applet-id 9"), "{log}");
    }

    #[test]
    fn list_scripts_quote_values() {
        let add = gsettings_list_add_script("org.cinnamon", "enabled-extensions", "a'b", "a'b");
        assert!(add.contains("'a'\\''b'"));
        let rm = gsettings_list_remove_script("org.cinnamon", "enabled-extensions", "x@y");
        assert!(rm.contains("grep -vF 'x@y'"));
    }

    #[test]
    fn gnome_script_compiles_schemas_and_falls_back() {
        let s = gnome_extension_enable_script("e@x", "/home/t/.local/share/gnome-shell/extensions/e@x");
        assert!(s.contains("glib-compile-schemas"));
        assert!(s.contains("gnome-extensions enable 'e@x'"));
        assert!(s.contains("gsettings set org.gnome.shell enabled-extensions"));
    }

    /// Without a working `gnome-extensions` (here it refuses) disabling
    /// edits both lists, as GNOME itself does, and enabling — back; neighboring
    /// uuids (and those similar by substring) are intact. Real bash, fake
    /// `gsettings`/`gnome-extensions`/`gnome-shell` — live settings are not
    /// touched.
    #[test]
    fn gnome_disable_without_tool_uses_disabled_list_and_is_reversible() {
        use std::os::unix::fs::PermissionsExt;
        let have = |c: &str| std::process::Command::new("sh").arg("-c").arg(format!("command -v {c}")).output().is_ok_and(|o| o.status.success());
        if !(have("bash") && have("paste") && have("sed")) {
            return;
        }
        let dir = std::env::temp_dir().join(format!("rexon-gnome-disable-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let tool = |name: &str, body: &str| {
            let f = dir.join(name);
            std::fs::write(&f, body).unwrap();
            std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o755)).unwrap();
        };
        tool(
            "gsettings",
            "#!/bin/sh\nd=\"$(dirname \"$0\")\"\n\
             if [ \"$1\" = get ]; then if [ -f \"$d/$3\" ]; then cat \"$d/$3\"; else echo '@as []'; fi; exit 0; fi\n\
             if [ \"$1\" = set ]; then printf '%s\\n' \"$4\" > \"$d/$3\"; echo \"set $3\" >> \"$d/log\"; exit 0; fi\nexit 1\n",
        );
        tool("gnome-extensions", "#!/bin/sh\nexit 1\n");
        tool("gnome-shell", "#!/bin/sh\necho 'GNOME Shell 47.0'\n");
        std::fs::write(dir.join("enabled-extensions"), "['a@x', 'sys@gnome.org']\n").unwrap();
        std::fs::write(dir.join("disabled-extensions"), "['xsys@gnome.org']\n").unwrap();
        let path = format!("{}:{}", dir.display(), std::env::var("PATH").unwrap_or_default());
        let run = |script: String| {
            let out = std::process::Command::new("bash").arg("-c").arg(script).env("PATH", &path).output().unwrap();
            assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        };
        let read = |key: &str| std::fs::read_to_string(dir.join(key)).unwrap().trim().to_string();

        run(gnome_extension_disable_script("sys@gnome.org"));
        assert_eq!(read("enabled-extensions"), "['a@x']");
        assert_eq!(read("disabled-extensions"), "['xsys@gnome.org','sys@gnome.org']");
        // A repeat duplicates nothing.
        run(gnome_extension_disable_script("sys@gnome.org"));
        assert_eq!(read("disabled-extensions"), "['xsys@gnome.org','sys@gnome.org']");

        run(gnome_extension_enable_script("sys@gnome.org", "/nonexistent/sys@gnome.org"));
        assert_eq!(read("disabled-extensions"), "['xsys@gnome.org']");
        assert_eq!(read("enabled-extensions"), "['a@x','sys@gnome.org']");
        // Enabling something that is not in disabled-extensions does not write that key.
        let log_before = std::fs::read_to_string(dir.join("log")).unwrap_or_default();
        run(gnome_extension_enable_script("b@x", "/nonexistent/b@x"));
        let log = std::fs::read_to_string(dir.join("log")).unwrap_or_default();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(!log[log_before.len()..].contains("set disabled-extensions"), "{log}");
    }

    #[test]
    fn reset_only_touches_our_theme() {
        let s = gsettings_reset_if_script("org.cinnamon.theme", "name", "Mint-Y");
        assert!(s.contains("if [ \"$cur\" = 'Mint-Y' ]"));
        assert!(s.contains("gsettings reset org.cinnamon.theme name"));
    }
}
