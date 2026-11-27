//! The menu of the main Rexon window: its items, their labels, availability and all
//! the texts of the dialogs behind those items.
//!
//! WHY A SEPARATE MODULE. The core already knows the window skeleton — the section order,
//! title and size live in [`crate::qt_main`]. But the menu under the
//! «☰» button was known ONLY to Python: `RexonMainWindow._build_menubar` built four
//! items, `_on_recover_kde`, `_on_about` and `_on_uninstall` kept to themselves
//! all the human text — the question before resetting the KDE theme, the about box
//! with the license attribution, the list of files to be removed and the labels of the three
//! uninstall buttons. The Rust skeleton had not a single line of the menu: the customer uses it,
//! and the Rust owner did not show it at all.
//!
//! DECISIONS live here: which items exist, in what order, where the separator is,
//! what is available and in which words to speak. The markup (`qml/WindowMenu.qml`,
//! `qml/WindowMenuDialogs.qml`) only draws what it is told from here, and
//! the executor (`rexon-gui/src/window_menu.rs`) only runs things.
//!
//! ⚠️ THERE IS NO SECOND COPY OF «QUIT» HERE. The «🚪 Quit» item is also in the
//! tray icon menu, and its label with the icon is built by [`crate::tray::titled`] from
//! the `quit` key. Exactly this drift was caught by the skeleton check on 07.09.2026
//! (the window knew «🚪 Quit», the core — «Quit» untitled), and rule #12 forbids
//! creating a copy again.
//!
//! LANGUAGE. All labels and dialog texts go through the translation catalog
//! (`crate::i18n`): the source language is English, Russian lives in `po/ru.po`
//! verbatim as the Python window showed it (checked by
//! `tests/window_menu_parity.rs`). The former constants became functions:
//! a constant can be forgotten to be translated, a function cannot. The choice is parsed by
//! identifiers (`ID_*`, `FORKS_BUTTON_*`), the code does not compare labels.

use crate::i18n::{n_, ntr, tr, trc, trf};
use crate::installer::UserDataKind;
use crate::tray;

// ============================================================================
// Button and item identifiers
// ============================================================================

/// Label of the menu button itself — the "hamburger" in the window's menu bar.
/// Source: `bar.addMenu("☰")` in `_build_menubar`.
pub const MENU_BUTTON: &str = "☰";

/// Restore the KDE theme from a backup.
pub const ID_RECOVER_KDE: &str = "recover_kde";
/// About the program.
pub const ID_ABOUT: &str = "about";
/// Remove Rexon from the system. An uninstall button is appropriate: Rexon is a PROGRAM, not
/// a game (games do not get one).
pub const ID_UNINSTALL: &str = "uninstall";
/// Quit the application.
pub const ID_QUIT: &str = "quit";

// ============================================================================
// Menu contents
// ============================================================================

/// One item of the window menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuItem {
    /// Identifier — by it the window owner understands what was clicked. Neither the markup
    /// nor the executor compare labels: the label is translated, the decision is
    /// not.
    pub id: &'static str,
    /// Exactly the string the customer sees.
    pub label: String,
    /// Keyboard shortcut in Qt notation («Ctrl+Q»); empty — no shortcut.
    pub shortcut: &'static str,
    /// A separator stands before this item.
    ///
    /// The list is deliberately not cluttered with a separate "separator item":
    /// the markup would need two kinds of delegate for one list, and
    /// a separator is a property of adjacency, not an item of its own.
    pub separator_before: bool,
}

/// Menu items IN MENU ORDER — the single source of truth about the contents.
///
/// Order and separator — as in `_build_menubar`: theme reset, about,
/// uninstall, separator, quit. «Quit» is set apart by a line because it is
/// the only one that is irreversible instantly and without questions.
pub fn menu_items() -> Vec<MenuItem> {
    vec![
        MenuItem {
            id: ID_RECOVER_KDE,
            label: format!("🛟 {}", recover_title()),
            shortcut: "",
            separator_before: false,
        },
        MenuItem {
            id: ID_ABOUT,
            // TRANSLATORS: item of the main window menu; the dialog it opens
            // is titled "About Rexon" without this context.
            label: format!("ℹ️ {}", trc("window menu", "About Rexon")),
            shortcut: "",
            separator_before: false,
        },
        MenuItem {
            id: ID_UNINSTALL,
            label: format!("🗑️ {}", uninstall_title()),
            shortcut: "",
            separator_before: false,
        },
        MenuItem {
            // The label with its icon is taken from the tray — there is no second copy.
            id: ID_QUIT,
            label: tray::titled("quit"),
            shortcut: "Ctrl+Q",
            separator_before: true,
        },
    ]
}

/// Menu items for THIS session: on a foreign desktop without «Restore
/// KDE Theme».
///
/// Live check in MATE: the item was in the menu, and clicking it led only
/// to the refusal "works only in KDE Plasma". Promising in the menu what the route
/// certainly will not do is an extra line and an extra question for the customer.
/// The decision is the same as that of the route itself ([`recover_allowed`]), so the
/// menu and the refusal will not diverge. The full contents are still in [`menu_items`].
pub fn menu_items_for(xdg_current_desktop: &str) -> Vec<MenuItem> {
    let recover = recover_allowed(xdg_current_desktop);
    menu_items()
        .into_iter()
        .filter(|i| recover || i.id != ID_RECOVER_KDE)
        .collect()
}

/// Whether a menu item is available.
///
/// The product has no unavailable items: none of the four sets
/// `setEnabled(False)`, and «Uninstall Rexon» opens even when Rexon is not
/// installed at all — then the uninstall dialog itself says so
/// («Rexon is not installed»), not a grey menu line. The question is still
/// asked of the core, not decided in the markup: as soon as an item gets a
/// condition, only one place will need editing.
///
/// An unknown identifier is unavailable: showing an item with nothing
/// behind it is a promise nobody will keep.
pub fn is_enabled(id: &str) -> bool {
    matches!(id, ID_RECOVER_KDE | ID_ABOUT | ID_UNINSTALL | ID_QUIT)
}

/// Whether the item is dangerous: behind it is something irreversible — uninstalling the program.
///
/// The markup paints such an item on hover and focus with the danger button color
/// (`destructive_tone`), not the usual highlight: a person who reached it with
/// the arrows or a gamepad must see that it is not «About». The core decides,
/// not the markup: labels must not be compared (they are translated), and an
/// identifier in the markup is a second place that will be forgotten to be fixed.
///
/// «Quit» is not included: it closes the window but erases nothing.
pub fn is_destructive(id: &str) -> bool {
    id == ID_UNINSTALL
}

// ============================================================================
// «About» (port of `_on_about`)
// ============================================================================

/// Title of the about window.
pub fn about_title() -> String {
    tr("About Rexon")
}

/// About box button: open the log file.
pub fn log_open_label() -> String {
    // TRANSLATORS: button in the "About Rexon" dialog; opens the Rexon log file.
    tr("Open log")
}

/// About box button: copy the end of the log to the clipboard.
pub fn log_copy_label() -> String {
    // TRANSLATORS: button in the "About Rexon" dialog; copies the end of the log to the clipboard.
    tr("Copy log")
}

/// About box line after copying.
pub fn log_copied_text() -> String {
    tr("The log is copied — paste it into a message to the developer.")
}

/// Where the log lies and how to enable the verbose one.
pub fn log_location_text(path: &str) -> String {
    // TRANSLATORS: %1 is the path of the log file; REXON_LOG=debug is an environment variable, keep it as is.
    trf("Log: %1 (detailed: start Rexon with REXON_LOG=debug)", &[&path])
}

/// How much of the log tail to copy: this much can be read by eye in a message
/// and fits into an email, and the last runs are enough for an analysis.
pub const LOG_COPY_BYTES: u64 = 200 * 1024;

/// Product version for a human — «0.1.1». Built from [`crate::constants`], not
/// rewritten as a string: `rexon.__version__` and the core must match.
pub fn version_string() -> String {
    let (major, minor, patch) = crate::constants::REXON_VERSION;
    format!("{major}.{minor}.{patch}")
}

/// The about box body, rich text — exactly what `QMessageBox.about` shows.
///
/// ⚠️ The paragraph about Qt and KDE Frameworks is a LICENSE ATTRIBUTION, not
/// decoration: both libraries are LGPL, and stating dynamic linking is a condition
/// of using them when distributing Rexon. Editing this paragraph is editing
/// legal text, not interface text.
pub fn about_html() -> String {
    format!(
        "<h2>Rexon {version}</h2>\
         <p>{tagline}</p>\
         <p>{features}</p>\
         <p>{support}</p>\
         <p>{copyright}</p>\
         <p style='color:#888;font-size:11px'>{attribution}</p>",
        version = version_string(),
        tagline = tr("Desktop environment manager for Linux"),
        features = tr("Install, remove and configure desktop environments, themes, icons, \
             cursors, fonts, wallpapers (including Wallpaper&nbsp;Engine), the login \
             screen and boot splash (SDDM/Plymouth), widgets and extensions — all in \
             one place."),
        support = tr("<b>Supported:</b> KDE, GNOME, Cinnamon, XFCE, MATE, Budgie, \
             Pantheon, Deepin, LXQt, COSMIC, Wayfire, Hyprland, sway, niri, River"),
        copyright = tr("© 2026 Wehrwolfmann · Apache License 2.0"),
        // TRANSLATORS: license attribution required by the LGPL; keep the
        // library names and license identifiers exactly as they are.
        attribution = tr("Uses Qt (LGPL-3.0) and KDE&nbsp;Frameworks \
             (LGPL-2.1+), dynamically linked."),
    )
}

// ============================================================================
// «About»: credits — the third-party programs Rexon stands on
// ============================================================================

/// A third-party program without which part of Rexon would not exist — one line
/// of the credits section in the about box.
///
/// WHY IN PLAIN SIGHT. Rexon's live wallpapers run on someone else's engine (waywallen,
/// Open Wallpaper Engine and others). The NOTICE file records this legally, but
/// nobody except lawyers reads it; the authors must be visible to the customer where
/// they look for "who made this" — in «About».
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThirdPartyComponent {
    /// The name as the author writes it. Not translated.
    pub name: &'static str,
    /// What this program does in Rexon — in the customer's words. Translated
    /// where it is shown (`tr(c.purpose)`), here only marked for `xgettext`.
    pub purpose: &'static str,
    /// License as an SPDX identifier. A legal record — not translated.
    pub license: &'static str,
    /// Project page; in the about box — a link that opens in the browser.
    pub url: &'static str,
    /// Beginning of the section header of this program in the NOTICE file. By it
    /// the guard (`tests/window_menu_parity.rs`) checks the table against NOTICE:
    /// the section exists, the link and license in it are the same.
    pub notice_heading: &'static str,
}

/// The credits are the ONLY list in the code. The order is as in the
/// «Credits» section of the README. The markup keeps no list of its own: the about box is built by
/// [`about_credits_html`].
pub const THIRD_PARTY: &[ThirdPartyComponent] = &[
    ThirdPartyComponent {
        name: "waywallen",
        // TRANSLATORS: what this third-party program does in Rexon ("About Rexon" → credits).
        purpose: n_("The live wallpaper engine: it plays wallpapers on your desktop. \
                     The Live wallpapers page is built on its interface."),
        license: "MIT",
        url: "https://github.com/hypengw/waywallen",
        notice_heading: "waywallen\n",
    },
    ThirdPartyComponent {
        name: "Open Wallpaper Engine",
        // TRANSLATORS: what this third-party program does in Rexon ("About Rexon" → credits).
        purpose: n_("Plays Wallpaper Engine scene and web wallpapers."),
        license: "GPL-2.0-only",
        url: "https://github.com/waywallen/open-wallpaper-engine",
        notice_heading: "open-wallpaper-engine ",
    },
    ThirdPartyComponent {
        name: "waywallen-display",
        // TRANSLATORS: what this third-party program does in Rexon ("About Rexon" → credits).
        purpose: n_("Shows live wallpapers on the KDE Plasma desktop."),
        // Upstream records the license in two ways (waywallen-display#70), the package
        // ships both texts — and the about box names both.
        license: "MIT / GPL-2.0-or-later",
        url: "https://github.com/waywallen/waywallen-display",
        notice_heading: "waywallen-display ",
    },
    ThirdPartyComponent {
        name: "Ruffle",
        // TRANSLATORS: what this third-party program does in Rexon ("About Rexon" → credits).
        purpose: n_("Plays Flash wallpapers."),
        license: "MIT OR Apache-2.0",
        url: "https://github.com/ruffle-rs/ruffle",
        notice_heading: "Ruffle\n",
    },
    ThirdPartyComponent {
        name: "QmlMaterial",
        // TRANSLATORS: what this third-party program does in Rexon ("About Rexon" → credits).
        purpose: n_("Gives the Live wallpapers page its look: buttons, lists and icons."),
        license: "MIT",
        url: "https://github.com/hypengw/QmlMaterial",
        notice_heading: "QmlMaterial ",
    },
    ThirdPartyComponent {
        name: "QExtra",
        // TRANSLATORS: what this third-party program does in Rexon ("About Rexon" → credits).
        purpose: n_("Shows wallpaper previews and animated pictures on the Live wallpapers page."),
        license: "MIT OR Apache-2.0",
        url: "https://github.com/hypengw/QExtra",
        notice_heading: "QExtra ",
    },
    ThirdPartyComponent {
        name: "Mozilla Translations",
        // TRANSLATORS: what this third-party program does in Rexon ("About Rexon" → credits).
        purpose: n_("The language model that translates descriptions — the same kind Firefox uses."),
        license: "MPL-2.0",
        url: "https://github.com/mozilla/translations",
        notice_heading: "Модель перевода Firefox ",
    },
    ThirdPartyComponent {
        name: "fxtranslate",
        // TRANSLATORS: what this third-party program does in Rexon ("About Rexon" → credits).
        purpose: n_("Translates English descriptions of themes and wallpapers right on \
                     your computer, without sending them anywhere."),
        license: "MPL-2.0",
        url: "https://crates.io/crates/fxtranslate",
        notice_heading: "fxtranslate ",
    },
    ThirdPartyComponent {
        name: "mpv",
        // TRANSLATORS: what this third-party program does in Rexon ("About Rexon" → credits).
        purpose: n_("Plays the startup movie at login."),
        license: "GPL-2.0-or-later",
        url: "https://mpv.io",
        notice_heading: "mpv ",
    },
];

/// The credits section of the about box, rich text: a header, a couple of words on why, one
/// line per program (the name is a link, what it does, the license) and where to look
/// for the full list. Links are opened by the markup (`onLinkActivated` →
/// `Qt.openUrlExternally`), like all links in the menu windows.
pub fn about_credits_html() -> String {
    let items: String = THIRD_PARTY.iter().map(credit_html).collect();
    format!(
        "<h3>{heading}</h3><p>{intro}</p>{items}\
         <p style='color:#888;font-size:11px'>{disclaimer}</p>\
         <p style='color:#888;font-size:11px'>{full}</p>",
        // TRANSLATORS: heading of the credits section in "About Rexon": the free programs Rexon is built on.
        heading = tr("Thanks"),
        // TRANSLATORS: intro of the credits section in "About Rexon".
        intro = tr("Live wallpapers and some other parts of Rexon are built on these \
             free programs. Thank you to their authors! Each program keeps its own license."),
        disclaimer = not_affiliated_text(),
        full = credits_full_list_text(),
    )
}

/// Disclaimer of affiliation with Wallpaper Engine and Valve. A legal line, not
/// decoration: the «Wallpaper Engine» mark appears in the Rexon interface (the wallpaper
/// tab), and without this disclaimer it could be taken as a sign of partnership. The text
/// is verbatim the same as in the README (the Credits section) — the check is held by
/// `tests/window_menu_parity.rs`.
pub fn not_affiliated_text() -> String {
    // TRANSLATORS: legal notice under the credits in "About Rexon"; keep the names Rexon, Wallpaper Engine and Valve as they are.
    tr("Rexon is not affiliated with, endorsed by or sponsored by Wallpaper Engine, \
        its developers, or Valve.")
}

/// One credits line, rich text: the name as a link, what it does, the license.
pub fn credit_html(c: &ThirdPartyComponent) -> String {
    format!(
        "<p><b><a href='{url}'>{name}</a></b> — {purpose}<br>\
         <span style='color:#888'>{license}</span></p>",
        url = c.url,
        name = c.name,
        purpose = tr(c.purpose),
        // TRANSLATORS: "About Rexon" → credits; %1 is a license identifier such as MIT or GPL-2.0-only, keep it as is.
        license = trf("License: %1", &[&c.license]),
    )
}

/// Where to look for the full list and the license texts.
pub fn credits_full_list_text() -> String {
    // TRANSLATORS: last line of the credits in "About Rexon"; NOTICE is a file name, keep it as is.
    tr("The full list of components and the license texts are in the NOTICE \
        file that comes with Rexon.")
}

/// The whole body of the «About» window: the about text and the credits under it. The about text
/// [`about_html`] is held verbatim by the parity check — the credits section
/// is appended to it, not inserted inside.
pub fn about_dialog_html() -> String {
    format!("{}{}", about_html(), about_credits_html())
}

// ============================================================================
// «Restore KDE Theme» (port of `_on_recover_kde`)
// ============================================================================

/// Path of the theme backup relative to the home folder.
pub const RECOVER_BACKUP_REL: &str = ".config/rexon/last_kde_lookandfeel.txt";

/// What is applied if there is no backup.
pub const RECOVER_FALLBACK_TARGET: &str = "Breeze Dark (fallback)";

/// Full path of the backup.
///
/// Computed by the same expander as all user paths of the project
/// ([`crate::utils::expand_user`]): with a non-standard `$XDG_CONFIG_HOME`
/// a hard-coded `~/.config` would read the wrong file, and the dialog would name the wrong theme.
pub fn recover_backup_path(home: &str, xdg_config_home: Option<&str>) -> String {
    crate::utils::expand_user(
        &format!("~/{RECOVER_BACKUP_REL}"),
        home,
        xdg_config_home,
    )
}

/// Which theme will be applied: the saved one or the fallback.
///
/// An empty or missing backup means the fallback theme, not an empty name:
/// the dialog must name the target, otherwise the person clicks "yes" blindly.
pub fn recover_target(saved: Option<&str>) -> String {
    match saved.map(str::trim) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => RECOVER_FALLBACK_TARGET.to_string(),
    }
}

/// Title of the question before the reset; also the menu item label without the icon.
pub fn recover_title() -> String {
    tr("Restore KDE Theme")
}

/// The question before the reset, rich text.
///
/// ⚠️ The wallpaper is mentioned DELIBERATELY and in bold: a theme reset stops live and
/// video wallpapers, and a person who is not warned about it will take the vanished
/// wallpaper for a Rexon breakage (rule #13).
pub fn recover_question(target: &str) -> String {
    // TRANSLATORS: rich text; %1 is the name of the theme to apply.
    trf(
        "This theme will be applied: <b>%1</b><br><br>\
         Your <b>wallpaper will also be reset to the KDE default</b> (any \
         video or live wallpapers will stop), and the desktop will look the \
         way it did right after installing the system.<br><br>\
         Use this if an applied theme broke your system \
         (black windows, white elements, apps crashing).",
        &[&target],
    )
}

/// Name of the Rust binary of the main window.
pub const MAIN_WINDOW_BIN: &str = "rexon-main-window";

/// Whom to call if our own binary is not recognised: the product's shared entry point.
pub const RECOVER_FALLBACK_BIN: &str = "rexon";

/// The tail the Linux kernel appends to `/proc/self/exe` when the binary file
/// was replaced on disk after launch (an ordinary rebuild).
const DELETED_EXE_SUFFIX: &str = " (deleted)";

/// Reset command: OUR OWN window binary with the `--recover-kde` flag.
///
/// ⭐ Fix during the port. Here used to be `rexon --recover-kde`, that
/// is, the Rust window handed the emergency rollback to the Python entry — although the same
/// route lives entirely in the core ([`crate::cli::run_headless`] →
/// [`crate::system_exec::SystemExec::recover_kde`]) and the window binary handles
/// it before Qt comes up. The shared entry is taken only if our own binary is not
/// recognised (the window is embedded in a foreign process).
///
/// `own_exe` — the path of our own executable (`std::env::current_exe`).
/// The « (deleted)» tail is stripped: after a rebuild the same path already holds
/// the new binary, and that is what must be called, not a non-existent name with the tail.
pub fn recover_argv(own_exe: Option<&str>) -> Vec<String> {
    let own = own_exe
        .map(|p| p.strip_suffix(DELETED_EXE_SUFFIX).unwrap_or(p))
        .filter(|p| {
            std::path::Path::new(p).file_name().and_then(|n| n.to_str())
                == Some(MAIN_WINDOW_BIN)
        });
    vec![
        own.unwrap_or(RECOVER_FALLBACK_BIN).to_string(),
        crate::app::RECOVER_KDE_FLAG.to_string(),
    ]
}

/// Whether the theme may be reset in this session. The decision is the same as that of the
/// route itself ([`crate::app::is_plasma_desktop`]): the window has no right to promise
/// "recovery started" where the route will refuse.
pub fn recover_allowed(xdg_current_desktop: &str) -> bool {
    crate::app::is_plasma_desktop(xdg_current_desktop)
}

/// What to say in the window on a foreign desktop.
///
/// ⭐ Fix during the port: Python ran the rollback blindly and reported
/// «Recovery Started», while the route on non-Plasma silently exited with
/// code 1 — the refusal went to an output stream the person does not see.
pub fn recover_not_plasma_text(xdg_current_desktop: &str) -> String {
    // TRANSLATORS: %1 is the name of the running desktop (e.g. GNOME).
    trf(
        "Theme recovery currently works only on KDE Plasma.\n\
         You are running: %1. Nothing was changed.\n\n\
         On this desktop, restore the look in the desktop's own settings \
         or on the Appearance tab.",
        &[&xdg_current_desktop],
    )
}

// ── Packages that replace parts of Plasma (AeroThemePlasma / AeroShell) ────
//
// Port of `_maybe_handle_plasma_forks`. Such packages are not a theme: they replace
// the Plasma programs themselves, and a theme reset will not remove them. Before resetting,
// the window says so and offers to bring back standard Plasma.
//
// The texts are put more simply than in Python: the words "fork", "stock", "pacman" and
// library names mean nothing to the customer (rule #13). The meaning and
// order of the steps are the same.

/// Title of the warning about replacement packages.
pub fn forks_title() -> String {
    tr("Replacement Plasma Components Installed")
}

/// Warning text, rich text. Package names are escaped: they
/// come from the package manager, not from this file.
pub fn forks_text(forks: &[String]) -> String {
    let list = forks
        .iter()
        .map(|f| format!("&bull; <code>{}</code>", crate::ui_base::escape_dialog_text(f)))
        .collect::<Vec<_>>()
        .join("<br>");
    // TRANSLATORS: rich text; %1 is the list of package names.
    trf(
        "These <b>AeroThemePlasma / AeroShell</b> packages are installed:<br><br>\
         %1<br><br>\
         They are <b>not a theme</b>: they replace parts of Plasma itself — \
         the panel, the password prompt, window management. As long as they \
         are installed, <b>resetting the theme will not remove them</b>. To \
         get standard Plasma back, they have to be removed and the standard \
         packages reinstalled.",
        &[&list],
    )
}

/// The "remove and bring back standard Plasma" button. The default button, as in
/// the product: behind it there is one more question with «No» as the default.
pub fn forks_remove_label() -> String {
    format!("🧹 {}", tr("Remove Them and Restore Standard Plasma"))
}
/// The "only reset the theme" button — deliberate cosmetics on top of the replacements.
pub fn forks_theme_label() -> String {
    tr("Reset Theme Only")
}
/// The cancel button — the same label as in the uninstall window.
pub fn forks_cancel_label() -> String {
    uninstall_cancel_label()
}

/// Refusal title: there is nothing to restore the standard packages with.
pub fn forks_blocked_title() -> String {
    tr("Standard Plasma Cannot Be Restored Right Now")
}

/// Title of the last question before removal.
pub fn forks_confirm_title() -> String {
    tr("Remove Replacement Plasma Components?")
}

/// The last question before removal, rich text: exactly the command that
/// will be executed (the core builds it), and what will happen afterwards.
///
/// The command is escaped: it contains `&&`. The words "the window may freeze"
/// from Python are deliberately absent here: removal runs in the background, the window does not stall.
pub fn forks_confirm_text(command: &str) -> String {
    let command = crate::ui_base::escape_dialog_text(command);
    // TRANSLATORS: rich text; %1 is the command that will be run.
    trf(
        "This will run (you will be asked for your password once):<br><br>\
         <code>%1</code><br><br>\
         It takes up to a minute. Afterwards you need to <b>log out and log \
         back in</b>.<br>\
         The Windows 7 style panel and widgets (SevenStart) will be gone.",
        &[&command],
    )
}

/// Title of the "removal in progress" message.
pub fn forks_running_title() -> String {
    tr("Removal in Progress")
}
/// Text of the "removal in progress" message (plain text).
pub fn forks_running_text() -> String {
    tr("A password prompt will appear in a moment. \
        When the removal is finished, the result will be shown here.")
}

/// Result title: it worked.
pub fn forks_done_title() -> String {
    tr("Replacements Removed")
}
/// The "it worked" result (plain text). The menu item is named verbatim — its
/// label is substituted, not rewritten as a second copy.
pub fn forks_done_text() -> String {
    // TRANSLATORS: %1 is the name of the "Restore KDE Theme" menu item.
    trf(
        "The replacement packages have been removed and the standard Plasma \
         components restored.\n\n\
         Log out and log back in to start standard Plasma. \
         After logging in, run “%1” again if needed.",
        &[&recover_title()],
    )
}
/// Result title: it did not work. The text is the core's explanation
/// ([`crate::system_plan::plasma_fork_reset_outcome`]).
pub fn forks_failed_title() -> String {
    tr("Could Not Remove the Replacements")
}

/// Identifiers of the three buttons of the replacement packages window. The choice is parsed by
/// them, not by the label: the label changes, the identifier does not.
pub const FORKS_BUTTON_REMOVE: &str = "remove";
pub const FORKS_BUTTON_THEME: &str = "theme";
pub const FORKS_BUTTON_CANCEL: &str = "cancel";

/// A button of the replacement packages window: what to write, which role to give it in the row
/// of buttons (`accept` / `action` / `reject`) and whether it is the default.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ForksButton {
    pub id: &'static str,
    pub label: String,
    pub role: &'static str,
    pub default: bool,
}

/// The whole replacement packages window — one for both product windows (Python and
/// Rust): neither of them has its own words or its own button order.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ForksPrompt {
    pub title: String,
    /// Rich text.
    pub text: String,
    pub buttons: Vec<ForksButton>,
}

/// The replacement packages window. `None` — no replacements, straight to the theme reset question.
///
/// The default button is removing the replacements, as in the product: behind it there is one more
/// question, and there the default is «No».
pub fn forks_prompt(forks: &[String]) -> Option<ForksPrompt> {
    if forks.is_empty() {
        return None;
    }
    let button = |id, label, role, default| ForksButton { id, label, role, default };
    Some(ForksPrompt {
        title: forks_title(),
        text: forks_text(forks),
        buttons: vec![
            button(FORKS_BUTTON_REMOVE, forks_remove_label(), "accept", true),
            button(FORKS_BUTTON_THEME, forks_theme_label(), "action", false),
            button(FORKS_BUTTON_CANCEL, forks_cancel_label(), "reject", false),
        ],
    })
}

/// What to do after the choice in the replacement packages window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ForksNext {
    /// Ask once more, showing the command, and only then remove.
    ConfirmRemoval,
    /// Deliberately only cosmetics: then the usual theme reset question.
    ThemeQuestion,
    /// Do nothing.
    Stop,
}

impl ForksNext {
    /// Step name for the windows: `confirm_removal` / `theme_question` / `stop`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ConfirmRemoval => "confirm_removal",
            Self::ThemeQuestion => "theme_question",
            Self::Stop => "stop",
        }
    }
}

/// The next step by the clicked button. Unknown or empty (the window was closed with
/// Esc or the close button) — do nothing: system packages must not be removed without
/// an explicit "yes".
pub fn forks_next(button_id: &str) -> ForksNext {
    match button_id {
        FORKS_BUTTON_REMOVE => ForksNext::ConfirmRemoval,
        FORKS_BUTTON_THEME => ForksNext::ThemeQuestion,
        _ => ForksNext::Stop,
    }
}

/// The step after "remove": either the last question or a refusal with an explanation.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ForksPreviewStep {
    /// `true` — ask the question (`text` — rich text with the command); `false` —
    /// there is nothing to restore the standard packages with, show `text` as plain text and
    /// run nothing.
    pub ok: bool,
    pub title: String,
    pub text: String,
    /// What to say while the removal is running (empty on refusal).
    pub running_title: String,
    pub running_text: String,
}

/// The step after "remove" by the answer of the core's preview
/// ([`crate::system_exec::SystemExec::plasma_fork_reset_preview`]):
/// `message` — the command when `ok`, an explanation on refusal.
pub fn forks_preview_step(ok: bool, message: &str) -> ForksPreviewStep {
    if ok {
        ForksPreviewStep {
            ok,
            title: forks_confirm_title(),
            text: forks_confirm_text(message),
            running_title: forks_running_title(),
            running_text: forks_running_text(),
        }
    } else {
        ForksPreviewStep {
            ok,
            title: forks_blocked_title(),
            text: message.to_string(),
            running_title: String::new(),
            running_text: String::new(),
        }
    }
}

/// Title and text of the removal result.
pub fn forks_finished(ok: bool, message: &str) -> (String, String) {
    if ok {
        (forks_done_title(), forks_done_text())
    } else {
        (forks_failed_title(), message.to_string())
    }
}

/// Title of the "it started" message.
pub fn recover_started_title() -> String {
    tr("Recovery Started")
}

/// Text of the "it started" message.
pub fn recover_started_text(target: &str) -> String {
    // TRANSLATORS: %1 is the name of the theme being applied.
    trf(
        "Applying: %1\n\n\
         KDE Plasma will switch the theme and reset the wallpaper to the \
         default; plasmashell will restart in 1–2 seconds.",
        &[&target],
    )
}

/// Title of the launch failure message.
pub fn error_title() -> String {
    tr("Error")
}

/// Text of the launch failure message.
pub fn recover_failed_text(error: &str) -> String {
    // TRANSLATORS: %1 is the system error message.
    trf("Could not start rexon: %1", &[&error])
}

// ============================================================================
// «Uninstall Rexon» (port of `_on_uninstall` + the `RexonUninstaller` texts)
// ============================================================================

/// Title of the uninstall window; also the menu item label without the icon.
pub fn uninstall_title() -> String {
    tr("Uninstall Rexon")
}

/// First line of the uninstall window.
pub fn uninstall_text() -> String {
    tr("Rexon's files and folders will be removed")
}

/// How many paths are shown as a list before the rest collapses into a count.
///
/// Fifteen is not a round number: the full list of a system install is more than
/// forty paths together with the polkit policies of the helpers, and the question window would go
/// past the edge of the screen, i.e. the person would not see the «Cancel» button.
pub const UNINSTALL_FILES_SHOWN: usize = 15;

/// The list of what is removed, as the uninstall window prints it.
pub fn uninstall_files_text(files: &[String]) -> String {
    let mut out: String = files
        .iter()
        .take(UNINSTALL_FILES_SHOWN)
        .map(|f| format!("  • {f}"))
        .collect::<Vec<_>>()
        .join("\n");
    if files.len() > UNINSTALL_FILES_SHOWN {
        let rest = (files.len() - UNINSTALL_FILES_SHOWN) as u64;
        // TRANSLATORS: last line of a cut list of files: "… 3 more".
        out.push_str(&format!("\n  • … {}", ntr("%n more", "%n more", rest)));
    }
    out
}

/// What is removed with either choice — the first line under the list.
pub fn uninstall_always_line() -> String {
    tr("Whichever you choose, the program itself (listed above) and Rexon's \
        temporary files are removed.")
}

/// What «Remove Everything» adds. The button is named verbatim — its label
/// is substituted, not rewritten as a second copy.
pub fn uninstall_delete_all_line() -> String {
    // TRANSLATORS: %1 is the "Remove Everything" button.
    trf(
        "“%1” also removes Rexon's settings, desktop backups, and installed \
         wallpapers, themes and add-ons.",
        &[&uninstall_delete_label()],
    )
}

/// The warning under the «Remove Everything» line: the wallpaper and the login screen theme
/// that are on the desktop right now lie in the Rexon folder and go away together
/// with it. Without this phrase a person clicks «Remove Everything» and gets
/// an unexpectedly bare desktop (Wehrwolfmann's decision of 16.09.2026).
pub fn uninstall_delete_all_active_line() -> String {
    // TRANSLATORS: %1 is the "Remove Everything" button.
    trf(
        "If your current wallpaper or login screen theme came from Rexon, \
         “%1” removes them too, and the desktop goes back to its standard look.",
        &[&uninstall_delete_label()],
    )
}

/// What stays with "keep" — the line under the list of the uninstall window.
/// Previously the question «Keep DE backups?» stood here: jargon, and
/// silence about the settings (Wehrwolfmann's decision of 15.09.2026 — "keep"
/// leaves both them and the installed wallpapers and themes; the cache always goes).
pub fn uninstall_keep_hint() -> String {
    // TRANSLATORS: %1 is the "Uninstall but Keep Settings and Backups" button.
    trf(
        "“%1” keeps your settings, backups, and installed wallpapers, themes \
         and add-ons: if you install Rexon again later, it picks up where you \
         left off.",
        &[&uninstall_keep_label()],
    )
}

/// Title of the details: where the person's data lies.
pub fn uninstall_data_paths_title() -> String {
    tr("Where it is stored:")
}
/// Category mark in the details: removed with either choice.
pub fn uninstall_data_always_mark() -> String {
    tr("removed with either choice")
}
/// Category mark in the details: only with «Remove Everything».
pub fn uninstall_data_full_mark() -> String {
    // TRANSLATORS: %1 is the "Remove Everything" button.
    trf("only with “%1”", &[&uninstall_delete_label()])
}

/// Details: paths of the person's data by category, in
/// [`UserDataKind::ALL`] order. There are no empty categories.
pub fn uninstall_data_paths_text(data: &[(UserDataKind, String)]) -> String {
    let mut lines = Vec::new();
    for kind in UserDataKind::ALL {
        let paths: Vec<&String> = data.iter().filter(|(k, _)| *k == kind).map(|(_, p)| p).collect();
        if paths.is_empty() {
            continue;
        }
        let mark = if kind.removed_when_keeping() {
            uninstall_data_always_mark()
        } else {
            uninstall_data_full_mark()
        };
        lines.push(format!("{} — {mark}", kind.title()));
        lines.extend(paths.into_iter().map(|p| format!("  • {p}")));
    }
    lines.join("\n")
}

/// The explanation under the first line: the program's list, what each
/// choice removes in words for a human, and the data paths — as details at the end.
pub fn uninstall_informative(files: &[String], data: &[(UserDataKind, String)]) -> String {
    // Inside <pre> a line break stays a line break: the shared
    // escaper turns it into <br>, and a literal «<br>» from a path it has
    // already turned into &lt;br&gt;, so the reverse replacement touches only ours.
    let esc = |s: &str| crate::ui_base::escape_dialog_text(s).replace("<br>", "\n");
    // The path blocks are wrapping (`dialog_pre`): a long path in a bare <pre>
    // went past the right edge of the window, and the tail of what will be removed was not visible.
    let pre = |s: &str| crate::ui_base::dialog_pre(&esc(s));
    let mut out = format!(
        "{}<p>{}</p><p>{}</p><p>{}</p><p>{}</p>",
        pre(&uninstall_files_text(files)),
        uninstall_always_line(),
        uninstall_delete_all_line(),
        uninstall_delete_all_active_line(),
        uninstall_keep_hint(),
    );
    if !data.is_empty() {
        out.push_str(&format!(
            "<p>{}</p>{}",
            uninstall_data_paths_title(),
            pre(&uninstall_data_paths_text(data))
        ));
    }
    out
}

/// "Uninstall the program but keep the settings and desktop backups" —
/// a button that is safe by meaning. The label is long on purpose:
/// the person must see on the button itself what will be kept (previously —
/// «Uninstall (keep backups)», and the settings were wiped all the same).
pub fn uninstall_keep_label() -> String {
    tr("Uninstall but Keep Settings and Backups")
}
/// "Wipe everything, including settings and backups" — irreversible.
pub fn uninstall_delete_label() -> String {
    tr("Remove Everything")
}
/// Refusal. In the product it is also the default button: foolproofing —
/// an accidental Enter must not wipe the program.
pub fn uninstall_cancel_label() -> String {
    // TRANSLATORS: the button that closes a dialog without doing anything.
    trc("dialog button", "Cancel")
}

/// The default button of the uninstall window — «Cancel».
pub const UNINSTALL_DEFAULT_ID: &str = "cancel";

/// Title of the successful uninstall message.
pub fn uninstall_done_title() -> String {
    tr("Uninstall Complete")
}

/// Text of the successful uninstall message: the core's result plus a warning about
/// the window closing.
pub fn uninstall_done_text(message: &str) -> String {
    // TRANSLATORS: %1 is the uninstall summary.
    trf("%1\n\nRexon will now close.", &[&message])
}

/// Title of the uninstall failure message.
pub fn uninstall_failed_title() -> String {
    tr("Uninstall Failed")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Module checks are in the source language: texts go through the catalog, and the language
    /// is the state of the whole process. Russian "as it was" is checked by
    /// `tests/window_menu_parity.rs` against the real `po/ru.po`.
    fn english() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
    }

    #[test]
    fn menu_has_four_items_in_product_order_with_one_separator() {
        english();
        let ids: Vec<&str> = menu_items().iter().map(|i| i.id).collect();
        assert_eq!(ids, vec![ID_RECOVER_KDE, ID_ABOUT, ID_UNINSTALL, ID_QUIT]);
        let seps: Vec<&str> = menu_items()
            .iter()
            .filter(|i| i.separator_before)
            .map(|i| i.id)
            .collect();
        assert_eq!(seps, vec![ID_QUIT], "разделитель стоит только перед «Выходом»");
    }

    #[test]
    fn recover_item_only_in_plasma_session() {
        english();
        let ids = |d: &str| -> Vec<&str> { menu_items_for(d).iter().map(|i| i.id).collect() };
        assert_eq!(ids("KDE"), vec![ID_RECOVER_KDE, ID_ABOUT, ID_UNINSTALL, ID_QUIT]);
        assert_eq!(ids("MATE"), vec![ID_ABOUT, ID_UNINSTALL, ID_QUIT]);
        assert_eq!(ids("ubuntu:GNOME"), vec![ID_ABOUT, ID_UNINSTALL, ID_QUIT]);
        let seps: Vec<&str> = menu_items_for("MATE")
            .iter()
            .filter(|i| i.separator_before)
            .map(|i| i.id)
            .collect();
        assert_eq!(seps, vec![ID_QUIT]);
    }

    /// The «Quit» label must come from the tray, not from a copy here.
    #[test]
    fn quit_label_comes_from_tray() {
        english();
        let quit = menu_items().into_iter().find(|i| i.id == ID_QUIT).unwrap();
        assert_eq!(quit.label, tray::titled("quit"));
    }

    #[test]
    fn only_quit_has_a_shortcut() {
        english();
        let with: Vec<&str> = menu_items()
            .iter()
            .filter(|i| !i.shortcut.is_empty())
            .map(|i| i.id)
            .collect();
        assert_eq!(with, vec![ID_QUIT]);
    }

    #[test]
    fn unknown_menu_id_is_not_available() {
        english();
        assert!(!is_enabled("нет-такого-пункта"));
        for item in menu_items() {
            assert!(is_enabled(item.id), "пункт «{}» обязан быть доступен", item.id);
        }
    }

    #[test]
    fn only_uninstall_is_destructive() {
        english();
        let danger: Vec<&str> = menu_items()
            .iter()
            .filter(|i| is_destructive(i.id))
            .map(|i| i.id)
            .collect();
        assert_eq!(danger, vec![ID_UNINSTALL]);
        assert!(!is_destructive("нет-такого-пункта"));
    }

    #[test]
    fn recover_calls_own_window_binary_not_python_entry() {
        english();
        let flag = crate::app::RECOVER_KDE_FLAG;
        assert_eq!(
            recover_argv(Some("/usr/lib/rexon/rexon-main-window")),
            vec!["/usr/lib/rexon/rexon-main-window", flag]
        );
        // After a rebuild the Linux kernel appends a tail — the file to call is the one
        // that lies at this path now.
        assert_eq!(
            recover_argv(Some("/t/rust/target/release/rexon-main-window (deleted)")),
            vec!["/t/rust/target/release/rexon-main-window", flag]
        );
        // A window in a foreign process (python3) — only the product's shared entry point.
        assert_eq!(recover_argv(Some("/usr/bin/python3.14")), vec!["rexon", flag]);
        assert_eq!(recover_argv(None), vec!["rexon", flag]);
        // The flag is the same as in the route parser: otherwise the menu item would call a route
        // the binary does not know.
        assert_eq!(
            crate::app::route(&[flag.to_string()]),
            crate::app::Route::RecoverKde
        );
    }

    #[test]
    fn recover_refuses_outside_plasma_and_says_why() {
        english();
        assert!(recover_allowed("KDE"));
        assert!(recover_allowed(""));
        assert!(!recover_allowed("GNOME"));
        let t = recover_not_plasma_text("GNOME");
        assert!(t.contains("GNOME") && t.contains("Nothing was changed"), "{t}");
    }

    #[test]
    fn forks_texts_name_packages_escaped_and_warn_about_logout() {
        english();
        let t = forks_text(&["aeroshell-libplasma-git".into(), "a<b".into()]);
        assert!(t.contains("<code>aeroshell-libplasma-git</code>"), "{t}");
        assert!(t.contains("a&lt;b") && !t.contains("a<b"), "{t}");
        assert!(t.contains("resetting the theme will not remove them"), "{t}");
        let c = forks_confirm_text("sudo pacman -Rns x && sudo pacman -S y");
        assert!(c.contains("&amp;&amp;") && !c.contains(" && "), "{c}");
        assert!(c.contains("log out and log back in"), "{c}");
        // Jargon that means nothing to the customer does not get into the window's own words
        // (the command in <code> is not the window's words but what will be executed).
        for own in [forks_title(), forks_remove_label(), forks_blocked_title(),
                    forks_confirm_title(), forks_done_text(), forks_running_text()] {
            for jargon in ["fork", "stock", "pacman", "logout"] {
                assert!(!own.to_lowercase().contains(jargon), "«{jargon}» в «{own}»");
            }
        }
        assert_eq!(forks_finished(true, "лог").0, forks_done_title());
        assert_eq!(
            forks_finished(false, "объяснение ядра"),
            (forks_failed_title(), "объяснение ядра".to_string())
        );
    }

    #[test]
    fn forks_steps_are_decided_here_for_both_windows() {
        english();
        assert_eq!(forks_prompt(&[]), None, "замен нет — окна нет");
        let p = forks_prompt(&["aeroshell-libplasma-git".into()]).unwrap();
        let ids: Vec<_> = p.buttons.iter().map(|b| b.id).collect();
        assert_eq!(ids, [FORKS_BUTTON_REMOVE, FORKS_BUTTON_THEME, FORKS_BUTTON_CANCEL]);
        let defaults: Vec<_> = p.buttons.iter().filter(|b| b.default).map(|b| b.id).collect();
        assert_eq!(defaults, [FORKS_BUTTON_REMOVE]);
        assert_eq!(forks_next(FORKS_BUTTON_REMOVE), ForksNext::ConfirmRemoval);
        assert_eq!(forks_next(FORKS_BUTTON_THEME), ForksNext::ThemeQuestion);
        assert_eq!(forks_next(FORKS_BUTTON_CANCEL), ForksNext::Stop);
        // Esc / close button: system packages must not be removed without an explicit "yes".
        assert_eq!(forks_next(""), ForksNext::Stop);
        assert_eq!(
            serde_json::to_value(ForksNext::ThemeQuestion).unwrap(),
            ForksNext::ThemeQuestion.as_str()
        );
        let ask = forks_preview_step(true, "a && b");
        assert!(ask.ok && ask.title == forks_confirm_title() && ask.text.contains("&amp;&amp;"));
        let no = forks_preview_step(false, "нечем вернуть");
        assert!(!no.ok && no.title == forks_blocked_title() && no.text == "нечем вернуть");
        assert!(no.running_text.is_empty());
    }

    #[test]
    fn recover_target_falls_back_when_backup_is_empty() {
        english();
        assert_eq!(recover_target(None), RECOVER_FALLBACK_TARGET);
        assert_eq!(recover_target(Some("   ")), RECOVER_FALLBACK_TARGET);
        assert_eq!(recover_target(Some(" Breeze \n")), "Breeze");
    }

    /// The rest must be counted, not dropped: a silently truncated list
    /// would lie about what is removed.
    #[test]
    fn long_file_list_is_cut_and_the_rest_is_counted() {
        english();
        let files: Vec<String> = (0..20).map(|i| format!("/tmp/rexon-{i}")).collect();
        let text = uninstall_files_text(&files);
        assert_eq!(text.lines().count(), UNINSTALL_FILES_SHOWN + 1);
        assert!(text.ends_with("  • … 5 more"), "{text}");
        // A short list does not get a count of the rest.
        let short = uninstall_files_text(&files[..3]);
        assert!(!short.contains("more"), "{short}");
    }

    /// Decision of 15.09.2026: the window says in words what each choice removes, and
    /// the data paths go as details, marked with the choice under which they go away.
    #[test]
    fn uninstall_prompt_says_what_each_choice_removes() {
        english();
        use UserDataKind::*;
        let data = vec![
            (Cache, "/h/.cache/rexon".to_string()),
            (Settings, "/h/.config/rexon".into()),
            (Backups, "/h/.local/share/rexon/backups".into()),
            (Content, "/h/.local/share/rexon/wallpapers".into()),
            (Content, "/h/.local/share/rexon/a&b".into()),
        ];
        let text = uninstall_informative(&["/h/.local/lib/rexon".to_string()], &data);
        for must in [
            uninstall_always_line(),
            uninstall_delete_all_line(),
            uninstall_delete_all_active_line(),
            uninstall_keep_hint(),
            uninstall_data_paths_title(),
        ] {
            assert!(text.contains(&must), "нет «{must}»: {text}");
        }
        // The "keep" line names the button verbatim — the person recognises it.
        assert!(uninstall_keep_hint().contains(&uninstall_keep_label()));
        assert!(uninstall_delete_all_line().contains(&uninstall_delete_label()));
        // Words for a human — without paths and without "cache".
        // The warning names the button verbatim and says what will happen to the desktop.
        assert!(uninstall_delete_all_active_line().contains(&uninstall_delete_label()));
        assert!(uninstall_delete_all_active_line().contains("login screen"));
        for words in [
            uninstall_always_line(),
            uninstall_delete_all_line(),
            uninstall_delete_all_active_line(),
            uninstall_keep_hint(),
        ] {
            assert!(!words.contains('/') && !words.to_lowercase().contains("cache"), "{words}");
        }
        for w in ["temporary files", "wallpapers", "themes", "settings", "backups"] {
            let all = format!("{} {}", uninstall_always_line(), uninstall_delete_all_line()).to_lowercase();
            assert!(all.contains(w), "окно молчит про «{w}»");
        }
        let paths = uninstall_data_paths_text(&data);
        assert_eq!(
            paths,
            "Rexon's temporary files — removed with either choice\n  • /h/.cache/rexon\n\
             Rexon settings — only with “Remove Everything”\n  • /h/.config/rexon\n\
             Desktop backups — only with “Remove Everything”\n  • /h/.local/share/rexon/backups\n\
             Installed wallpapers, themes and add-ons — only with “Remove Everything”\n  • /h/.local/share/rexon/wallpapers\n  • /h/.local/share/rexon/a&b"
        );
        assert!(text.contains("a&amp;b") && !text.contains("a&b"), "пути экранированы: {text}");
        // No data — no details.
        assert!(!uninstall_informative(&[], &[]).contains(&uninstall_data_paths_title()));
    }

    #[test]
    fn about_names_the_same_version_as_the_crate_constant() {
        english();
        assert!(about_html().contains(&format!("<h2>Rexon {}</h2>", version_string())));
    }
}
