//! The Rexon icon in the system tray: menu contents, labels, tooltip, icon —
//! and autostart at login (Rexon starts straight into the tray).
//!
//! BOUNDARY. Only decisions and texts live here: which item has which number, how it
//! is labelled in the customer's language, whether it is enabled, whether it is checked, where to look for
//! the icon, what to write into the autostart entry. Not a single line of Qt: the
//! `QSystemTrayIcon`, `QMenu` themselves and writing the file are on the executor
//! (`rexon-gui/src/tray.rs` on top of the `cpp/rexon_tray.cpp` shim). So the whole
//! module is checked by ordinary core tests, without a live tray.
//!
//! ⚠️ LABELS LIVE ONLY HERE. There must be no human text in the bridge, in QML or in the
//! shim — the same rule by which the tabs are built
//! («Settings» take the `qt_settings::TEXTS` catalog, «Compare Desktops» — the flat
//! `qt_compare` constants).
//!
//! WHY A TRAY AT ALL. Rexon starts the wallpaper engine (waywallen) with `--no-tray`
//! (`we_client::DAEMON_ARGS`), i.e. the engine deliberately does not show ITS OWN
//! icon: the product has one icon, and it is the Rexon icon. The price for that is
//! that the customer loses everything the engine's tray gave (pause, next wallpaper,
//! sound, rotation interval). So the menu below repeats the contents of the engine's tray and
//! adds its own: the Rexon window, jumping to a section and autostart.

use crate::i18n::{n_, tr};

// ============================================================================
// Item numbers
// ============================================================================
//
// The number is the only thing that travels between the Qt layer and this module: the shim
// returns the clicked number, the core decides what it means. The values are stable and
// spaced out by group, so that a new item does not shift its neighbours.

/// Open (raise) the main Rexon window.
pub const ID_OPEN: i32 = 1;
/// Start of the «Sections» range: `ID_SECTION_BASE + the tab's ordinal number`.
pub const ID_SECTION_BASE: i32 = 10;
/// Next wallpaper.
pub const ID_NEXT: i32 = 20;
/// Previous wallpaper.
pub const ID_PREV: i32 = 21;
/// Shuffling the queue (check box).
pub const ID_SHUFFLE: i32 = 30;
/// Start of the rotation interval range: `ID_ROTATE_BASE + index in [`ROTATE_PRESETS`]`.
pub const ID_ROTATE_BASE: i32 = 40;
/// Pause/resume the wallpaper.
pub const ID_PAUSE: i32 = 50;
/// Wallpaper sound.
pub const ID_MUTE: i32 = 51;
/// Pause/resume the Rexon wallpaper playlist (`wallpaper_playlist`).
pub const ID_PLAYLIST_PAUSE: i32 = 52;
/// Rescan the wallpaper library.
pub const ID_RESCAN: i32 = 60;
/// Start Rexon at login (check box).
pub const ID_AUTOSTART: i32 = 70;
/// The «Rexon can change the system…» line (not clickable, only visible
/// while there is a privilege session).
pub const ID_ADMIN_INFO: i32 = 80;
/// «Revoke Now»: close the privilege session (the next change will ask for the
/// password again).
pub const ID_REVOKE: i32 = 81;
/// Quit Rexon.
pub const ID_QUIT: i32 = 99;

// ============================================================================
// Labels
// ============================================================================

/// Menu labels: key → English `msgid` of the translation catalog.
///
/// The table is flat and KEYED: the key is visible in the executor's code, and the text
/// is translated where it is shown ([`label`] → `i18n::tr`). Russian and any
/// other language live in `po/<code>.po`, there is no second copy of the text here.
const LABELS: &[(&str, &str)] = &[
    ("open", n_("Open Rexon")),
    // TRANSLATORS: submenu of the tray icon listing the tabs of the main window.
    ("sections", n_("Sections")),
    ("next", n_("Next Wallpaper")),
    ("prev", n_("Previous Wallpaper")),
    ("shuffle", n_("Shuffle")),
    // TRANSLATORS: submenu of the tray icon with the wallpaper rotation intervals.
    ("rotate", n_("Change Wallpaper Every")),
    ("pause", n_("Pause")),
    ("resume", n_("Resume")),
    ("mute", n_("Mute")),
    ("unmute", n_("Unmute")),
    ("rescan", n_("Rescan Wallpapers")),
    // The Rexon wallpaper playlist (video, web, Flash, pictures, Razer Axon) —
    // the labels are the same as on the buttons of the «Wallpaper playlist» block.
    ("playlist.pause", n_("Pause Playlist")),
    ("playlist.resume", n_("Resume Playlist")),
    // One label with the check box in «Settings» (`qt_settings`, key
    // `tray.autostart`): it is one and the same setting.
    ("autostart", n_("Start at login")),
    ("quit", n_("Quit")),
    // Privilege session (B13): one label with the bar in «Settings»
    // (`qt_settings`, keys `admin.*`) — it is one and the same state.
    // TRANSLATORS: shown while Rexon may change the system without asking for the password again.
    ("admin.info", n_("Rexon can change the system until the window is closed")),
    // TRANSLATORS: button/menu item: stop letting Rexon change the system; next time it asks for the password.
    ("admin.revoke", n_("Revoke Now")),
    // Wallpaper rotation intervals.
    // TRANSLATORS: wallpaper rotation interval: never change the wallpaper.
    ("rot.off", n_("Off")),
    ("rot.30s", n_("30 Seconds")),
    ("rot.1m", n_("1 Minute")),
    ("rot.5m", n_("5 Minutes")),
    ("rot.15m", n_("15 Minutes")),
    ("rot.1h", n_("1 Hour")),
    // Sections of the main window — the identifiers are the same as in the main window
    // tab table (`crate::qt_main::SECTIONS`).
    ("tab.desktops", n_("Desktops")),
    ("tab.appearance", n_("Appearance")),
    ("tab.extensions", n_("Extensions")),
    // Product names are not translated — `tr` returns them as is.
    ("tab.wallpaper_engine", "Wallpaper Engine"),
    ("tab.steam", "Steam"),
    ("tab.settings", n_("Settings")),
    // Icon tooltip and the minimise notification.
    ("tip.title", "Rexon"),
    // TRANSLATORS: tooltip of the tray icon, under the name "Rexon".
    ("tip.body", n_("Desktop environment manager")),
    ("hidden.title", n_("Rexon is still running in the tray")),
    (
        "hidden.body",
        n_("The window is closed, but Rexon keeps running. \
            Click the icon to open it again, or choose “Quit” in the icon menu to exit."),
    ),
];

/// `msgid` of a label by key — for checks that need the source text, not
/// the translation into the process language.
pub fn label_msgid(key: &str) -> Option<&'static str> {
    LABELS.iter().find(|(k, _)| *k == key).map(|(_, m)| *m)
}

/// A label by key in the interface language. An unknown key gives an empty string:
/// staying silent here is safer than showing the key instead of the text.
pub fn label(key: &str) -> String {
    label_msgid(key).map(tr).unwrap_or_default()
}

/// Label of a main window section by its identifier (`desktops`,
/// `appearance`, …). An unknown identifier returns itself — so a new
/// tab ends up in the menu labelled at least somehow, not as an empty string.
pub fn section_label(tab_id: &str) -> String {
    let text = label(&format!("tab.{tab_id}"));
    if text.is_empty() {
        tab_id.to_string()
    } else {
        text
    }
}

// ============================================================================
// Label icons
// ============================================================================

/// Label icon: key of the [`LABELS`] table → the symbol with which the same label
/// appears in the MAIN WINDOW.
///
/// WHY A SEPARATE TABLE AND NOT IN THE LABEL ITSELF. The customer sees one and the same section
/// twice and in two forms: in the window's tab bar with an icon
/// («🖥 Desktops») and in the «Sections» submenu of the tray icon without it
/// («Desktops» — icons on some menu items and not on others
/// would read as noise). The text is ONE, and a second copy of it must not
/// be created: labels live only in [`LABELS`], here — only decoration.
///
/// ⚠️ WHAT IT WAS BEFORE 07.09.2026. The icons lived in Python — in the `_TABS` table
/// (`rexon/qt_main/window.py`), together with labels that the window should not have
/// at all. The core could not produce what the customer sees, and the window
/// on top of that was not translated, although the tray is: the skeleton check
/// (`tests/qt_main_parity.rs`) failed exactly on this. A new icon is a line
/// here, not a literal in the window.
///
/// ⚠️ U+FE0F after «🖥» and «🖼» is not garbage. These two symbols (U+1F5A5,
/// U+1F5BC) have text presentation by default: without U+FE0F font fallback gives them
/// to a monochrome symbol font, and in the tab bar they stood black and white
/// next to their colourful neighbours. Check a new icon the same way: if a symbol
/// has text presentation by default, U+FE0F after it is mandatory.
const ICONS: &[(&str, &str)] = &[
    ("tab.desktops", "🖥️"),
    ("tab.appearance", "🎨"),
    ("tab.extensions", "🧩"),
    ("tab.wallpaper_engine", "🖼️"),
    ("tab.steam", "🎮"),
    ("tab.settings", "⚙️"),
    ("quit", "🚪"),
    ("admin.info", "🔓"),
];

/// Label icon; for a key without an icon — an empty string.
pub fn icon(key: &str) -> &'static str {
    ICONS
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, glyph)| *glyph)
        .unwrap_or("")
}

/// The label with its icon in the customer's language — exactly the string that
/// the main window shows («🖥 Desktops», «🚪 Quit»).
///
/// Without an icon it returns the bare label, so it suits any key.
pub fn titled(key: &str) -> String {
    let text = label(key);
    let glyph = icon(key);
    if glyph.is_empty() || text.is_empty() {
        text
    } else {
        format!("{glyph} {text}")
    }
}

// ============================================================================
// Wallpaper rotation intervals
// ============================================================================

/// Presets of the «Change Wallpaper Every» submenu: label key and interval in seconds.
///
/// One table for everything: the labels, the radio mark and what goes to the daemon.
/// A new preset is one line here, nowhere else.
pub const ROTATE_PRESETS: &[(&str, i64)] = &[
    ("rot.off", 0),
    ("rot.30s", 30),
    ("rot.1m", 60),
    ("rot.5m", 300),
    ("rot.15m", 900),
    ("rot.1h", 3600),
];

/// Preset seconds by item number; `None` — the number is not from the interval range.
pub fn rotate_secs_for(id: i32) -> Option<i64> {
    let index = usize::try_from(id - ID_ROTATE_BASE).ok()?;
    ROTATE_PRESETS.get(index).map(|(_, secs)| *secs)
}

// ============================================================================
// State the menu is built from
// ============================================================================

/// A snapshot of the live state at the moment the menu is opened.
///
/// ⚠️ `paused` and `muted` are OUR bookkeeping, not the daemon's answer: the daemon's
/// bus interface provides the `Pause`/`Resume`/`Mute`/`Unmute` methods, but it has no
/// "currently paused" and "currently muted" properties at all (verified by introspecting
/// `org.waywallen.waywallen.Daemon1` version 0.3.7 — the same place that has
/// `QueueMode` and `RotationSecs`). The executor must reset both flags
/// when the daemon disappeared from the bus: a restarted daemon plays with sound.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TrayState {
    /// The wallpaper daemon is visible on the bus — otherwise the whole wallpaper range is greyed out.
    pub daemon: bool,
    /// The queue is shuffled (`QueueMode == "shuffle"`).
    pub shuffle: bool,
    /// Auto-rotation interval in seconds (`RotationSecs`).
    pub rotation_secs: i64,
    /// The wallpaper is paused (our bookkeeping, see the warning above).
    pub paused: bool,
    /// Wallpaper sound is muted (our bookkeeping).
    pub muted: bool,
    /// Rexon autostart at login is enabled.
    pub autostart: bool,
    /// The Rexon wallpaper playlist is playing: «Next/Previous» drive it, not
    /// the WE daemon (one entry point for both playlists).
    pub playlist: bool,
    /// The Rexon wallpaper playlist is paused.
    pub playlist_paused: bool,
    /// There is a window privilege session (`privilege::run::has_session`): the password has already
    /// been entered, and Rexon can change the system until the window is closed.
    pub admin_session: bool,
}

/// What kind of menu item — this is enough for the executor to build a `QMenu`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    /// An ordinary action.
    Action,
    /// A separator (the number and label mean nothing).
    Separator,
    /// A check box.
    Check,
    /// A radio button in a mutually exclusive group.
    Radio,
    /// Start of a nested menu; up to the paired [`ItemKind::SubmenuEnd`] the items
    /// go inside it.
    SubmenuBegin,
    /// End of a nested menu.
    SubmenuEnd,
}

/// A finished menu line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuItem {
    pub id: i32,
    pub kind: ItemKind,
    pub label: String,
    pub checked: bool,
    pub enabled: bool,
    /// The default item (bold) — the one a click on the icon performs.
    pub default: bool,
}

impl MenuItem {
    fn new(id: i32, kind: ItemKind, label: String) -> Self {
        MenuItem {
            id,
            kind,
            label,
            checked: false,
            enabled: true,
            default: false,
        }
    }

    fn sep() -> Self {
        MenuItem::new(0, ItemKind::Separator, String::new())
    }

    fn enabled(mut self, on: bool) -> Self {
        self.enabled = on;
        self
    }

    fn checked(mut self, on: bool) -> Self {
        self.checked = on;
        self
    }

    fn default(mut self) -> Self {
        self.default = true;
        self
    }
}

/// Contents of the icon menu.
///
/// `tab_ids` — identifiers of the main window tabs IN THE SAME ORDER in which
/// they stand in the window: the window skeleton passes them itself, so the «Sections» submenu
/// cannot drift apart from the window, even if a tab is added or reordered.
/// An empty list — the submenu is not built at all (there is no window with tabs).
///
/// Group order — as in the wallpaper engine's tray: verbs first, then
/// state toggles, then maintenance, quit last.
pub fn menu(tab_ids: &[String], state: TrayState) -> Vec<MenuItem> {
    let t = |key: &str| label(key);
    let mut out = vec![MenuItem::new(ID_OPEN, ItemKind::Action, t("open")).default()];

    if !tab_ids.is_empty() {
        out.push(MenuItem::new(0, ItemKind::SubmenuBegin, t("sections")));
        for (index, id) in tab_ids.iter().enumerate() {
            let number = ID_SECTION_BASE + index as i32;
            out.push(MenuItem::new(
                number,
                ItemKind::Action,
                section_label(id),
            ));
        }
        out.push(MenuItem::new(0, ItemKind::SubmenuEnd, String::new()));
    }

    // ── The right to change the system (B13): visible only while it exists — and
    // right there a way to drop it without closing the window.
    if state.admin_session {
        out.push(MenuItem::sep());
        out.push(MenuItem::new(ID_ADMIN_INFO, ItemKind::Action, titled("admin.info")).enabled(false));
        out.push(MenuItem::new(ID_REVOKE, ItemKind::Action, t("admin.revoke")));
    }

    // ── Wallpaper. The whole range is greyed out without the daemon: pressing «Next Wallpaper» when
    // there is nothing to play gives the customer nothing (rule #13 — foolproofing).
    let live = state.daemon;
    out.push(MenuItem::sep());
    // The Rexon playlist is playing — wallpaper rotation works even without the daemon.
    let switch = live || state.playlist;
    out.push(MenuItem::new(ID_NEXT, ItemKind::Action, t("next")).enabled(switch));
    out.push(MenuItem::new(ID_PREV, ItemKind::Action, t("prev")).enabled(switch));
    if state.playlist {
        out.push(MenuItem::new(
            ID_PLAYLIST_PAUSE,
            ItemKind::Action,
            t(if state.playlist_paused { "playlist.resume" } else { "playlist.pause" }),
        ));
    }
    out.push(
        MenuItem::new(ID_SHUFFLE, ItemKind::Check, t("shuffle"))
            .checked(state.shuffle)
            .enabled(live),
    );

    out.push(MenuItem::new(0, ItemKind::SubmenuBegin, t("rotate")).enabled(live));
    for (index, (key, secs)) in ROTATE_PRESETS.iter().enumerate() {
        out.push(
            MenuItem::new(ID_ROTATE_BASE + index as i32, ItemKind::Radio, t(key))
                .checked(state.rotation_secs == *secs)
                .enabled(live),
        );
    }
    out.push(MenuItem::new(0, ItemKind::SubmenuEnd, String::new()));

    out.push(MenuItem::sep());
    // A verb, not a check box: a check box labelled «Pause» does not answer the question
    // "checked — does that mean paused now or playing now".
    out.push(
        MenuItem::new(
            ID_PAUSE,
            ItemKind::Action,
            t(if state.paused { "resume" } else { "pause" }),
        )
        .enabled(live),
    );
    out.push(
        MenuItem::new(
            ID_MUTE,
            ItemKind::Action,
            t(if state.muted { "unmute" } else { "mute" }),
        )
        .enabled(live),
    );

    out.push(MenuItem::sep());
    out.push(MenuItem::new(ID_RESCAN, ItemKind::Action, t("rescan")).enabled(live));

    out.push(MenuItem::sep());
    out.push(MenuItem::new(ID_AUTOSTART, ItemKind::Check, t("autostart")).checked(state.autostart));

    out.push(MenuItem::sep());
    out.push(MenuItem::new(ID_QUIT, ItemKind::Action, t("quit")));
    out
}

// ============================================================================
// Icon
// ============================================================================

/// Icon name in the theme — the same as in `Icon=` of the application launcher
/// (`packaging/aur/rexon.desktop`) and under which the package puts files into
/// `hicolor`. The shell looks it up itself and picks the needed size itself.
pub const ICON_THEME_NAME: &str = "rexon";

/// How Rexon presents itself to the system — `QCoreApplication::applicationName`
/// at the entry point of the main window.
///
/// ⭐ WHY IT IS HERE, IN THE TRAY. In Qt this same name GOES ONTO THE BUS as the
/// `Id` property of the StatusNotifierItem icon, and there is no other lever: `Id` and `Title`
/// are both read from `QCoreApplication::applicationName()`
/// (`QStatusNotifierItemAdaptor::id`/`::title`, qtbase). And the shell remembers
/// the user's settings of the icon itself by `Id` — whether to show it in the
/// tray or hide it and in what order (in Plasma these are the `shownItems` and
/// `hiddenItems` lists of the system tray applet).
///
/// So the name must be CONSTANT and not depend on the language. The translated
/// window title (`qt_main::WINDOW_TITLE`) used to get here: a customer
/// who changed the language got an icon with a new `Id` — i.e. lost their own
/// settings and saw a "new" icon that came from nowhere.
///
/// ⚠️ The string is visible to a human too: the shell prints the same `Title` in the list
/// of icons («Entries» in the tray settings) and in the popup list of hidden ones.
/// Hence «Rexon», not `rexon`: a technical token must not be shown to the
/// customer (rule #13), and the product name suits the machine and the human
/// equally well. The human description lives in the icon tooltip
/// (`tooltip_title` + `tooltip_body`) — translating it does not get in the way of anything.
///
/// The value is the same as that of the Python owner of the same window in the shared
/// entry point table (`qt_app::ENTRY_IDENTITIES`, key `rexon.qt_main.app`);
/// the guard is the test `имя_на_шине_машинное_и_совпадает_с_таблицей_точек_входа`.
pub const APP_ID: &str = "Rexon";

/// Where to look for the icon file if the theme was not enough — from newest to oldest.
///
/// The same trick and the same reason as for the directory list of our `.so` files
/// (`qml_modules::rust_lib_candidates`): the customer has no repository
/// tree, and the developer has no installed package. An empty icon in the tray
/// is NOT SHOWN AT ALL —
/// the customer considers an icon without a picture a vanished application.
pub fn icon_candidates(repo_root: &str) -> Vec<String> {
    let mut out = Vec::new();
    if !repo_root.is_empty() {
        // Developer tree: the master copy and large sizes lie in the root.
        out.push(format!("{repo_root}/rexon_icon_256.png"));
        out.push(format!("{repo_root}/rexon_icon_128.png"));
        out.push(format!("{repo_root}/rexon_icon.png"));
    }
    out.push("/usr/lib/rexon/rexon_icon.png".to_string());
    out.push("/usr/local/lib/rexon/rexon_icon.png".to_string());
    out.push("/usr/share/icons/hicolor/256x256/apps/rexon.png".to_string());
    out.push("/usr/share/pixmaps/rexon.png".to_string());
    out
}

/// Root of the developer tree by the path of our own executable.
///
/// Needed only by the icon and only for one thing — finding the picture file when the icon
/// theme was not enough (`icon_candidates`). For the Python product the root
/// is computed from `__file__`, but a Rust binary has no `__file__`: it lies in
/// `<root>/rust/target/{debug,release}/rexon-main-window`, so the root is
/// three directories up, and only if the path really has `rust/target`.
///
/// An empty answer is not an error but an honest "there is no tree": the customer's binary
/// sits in `/usr/bin`, and the icon file will be found by the system paths of the same
/// list. Guessing "what if this is a tree" is not allowed: then the candidate list
/// would begin with three certainly dead paths for every customer.
pub fn repo_root_from_exe(exe: &str) -> String {
    let path = std::path::Path::new(exe);
    // <root>/rust/target/<profile>/<binary>
    let profile = match path.parent() {
        Some(p) => p,
        None => return String::new(),
    };
    let target = match profile.parent() {
        Some(p) if p.file_name().is_some_and(|n| n == "target") => p,
        _ => return String::new(),
    };
    let rust = match target.parent() {
        Some(p) if p.file_name().is_some_and(|n| n == "rust") => p,
        _ => return String::new(),
    };
    match rust.parent() {
        Some(root) if !root.as_os_str().is_empty() => root.to_string_lossy().into_owned(),
        _ => String::new(),
    }
}

/// Icon tooltip title.
pub fn tooltip_title() -> String {
    label("tip.title")
}

/// Icon tooltip text.
pub fn tooltip_body() -> String {
    label("tip.body")
}

/// Title and text of the "the window closed, Rexon stayed in the tray" notification.
///
/// Shown ONCE per installation: silently hiding the window is exactly the
/// "quietly killed the application" that must not happen, while showing it every
/// time is intrusive.
pub fn hidden_notice() -> (String, String) {
    (label("hidden.title"), label("hidden.body"))
}

/// Setting key "the minimise notification has already been shown".
pub const SEEN_HIDDEN_NOTICE_KEY: &str = "tray_hidden_notice_seen";

// ============================================================================
// Autostart at login
// ============================================================================

/// Flag "start straight into the tray, do not show the window".
pub const START_HIDDEN_FLAG: &str = "--tray";

/// Whether Rexon was asked to start straight into the tray (`args` — without the program name).
pub fn starts_hidden(args: &[String]) -> bool {
    args.iter().any(|a| a == START_HIDDEN_FLAG)
}

/// Path of the Rexon autostart entry.
///
/// The mechanism is the same as for the login splash (`session_splash::autostart_path`) —
/// a `.desktop` in the desktop's autostart directory. The entry is OUR OWN and separate:
/// the splash deliberately has another mechanism (a systemd unit instead of `.desktop`, because
/// `.desktop` starts only after `plasmashell`), the default applications switcher
/// has a third entry, and they must not be mixed into one.
pub fn autostart_path(xdg_config_home: Option<&str>, home: &str) -> String {
    let cfg = match xdg_config_home {
        Some(x) if !x.is_empty() => x.to_string(),
        _ => format!("{home}/.config"),
    };
    format!("{cfg}/autostart/rexon.desktop")
}

/// Launch command for the autostart entry.
///
/// ⚠️ The entry point is chosen by THE SAME function as for the launcher
/// ([`crate::installer::launch_entry_live`]) — it arrives here ready in
/// `entry`. The tray is the whole owner of the process, the single-instance lock is shared:
/// autostarting the Python tray with a launcher for the Rust window would turn a click on the
/// launcher into showing the Python window (the Rust binary would knock on the taken lock
/// and exit). So both are written by one rule.
///
/// Rust entry — `rexon-main-window --tray`. Python entry (fallback) — the
/// `rexon` launcher by full path: `rexon_path` — what `which rexon` found (empty or
/// `None` — not found).
///
/// `None` — nothing to launch; the autostart entry is then NOT written
/// ([`crate::app::MSG_NOTHING_TO_LAUNCH`]). ⛔The previous fallback
/// `python3 -m rexon.app --tray` was removed on 16.09.2026 — exactly why is written
/// at [`crate::app::launch_program`], where the choice moved entirely.
pub fn autostart_exec(
    entry: &crate::installer::LaunchEntry,
    rexon_path: Option<&str>,
) -> Option<String> {
    crate::app::launch_command(entry, rexon_path, &[START_HIDDEN_FLAG])
}

/// Labels of the autostart entry.
///
/// They are seen not by the customer in the Rexon window but by the desktop's autostart
/// editor, and it chooses the language ITSELF by the session locale — so the file gets
/// all catalog languages at once (`Comment[ru]=`), not the Rexon interface language
/// ([`crate::system_l10n::desktop_key`]).
// TRANSLATORS: Description of the Rexon login autostart entry (shown in the desktop's autostart settings).
const AUTOSTART_COMMENT: &str = n_("Start Rexon in the system tray at login");
// TRANSLATORS: Menu entry of Rexon: GenericName shown under the name "Rexon".
const AUTOSTART_GENERIC: &str = n_("Desktop Environment Manager");

/// Contents of the autostart entry.
pub fn autostart_desktop_content(exec: &str) -> String {
    use crate::system_l10n::desktop_key as key;
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Rexon\n\
         {generic}\
         {comment}\
         Exec={exec}\n\
         Icon={ICON_THEME_NAME}\n\
         Terminal=false\n\
         X-GNOME-Autostart-enabled=true\n",
        generic = key("GenericName", AUTOSTART_GENERIC),
        comment = key("Comment", AUTOSTART_COMMENT),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Module checks are in the source language: labels go through the catalog, and
    /// the language is the state of the whole process. Russian "as it was" is checked by
    /// `tests/qt_main_parity.rs` against the real `po/ru.po`.
    fn english() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
    }

    fn ids() -> Vec<String> {
        ["desktops", "appearance", "extensions", "wallpaper_engine", "steam", "settings"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    #[test]
    fn обязательный_минимум_на_месте() {
        english();
        let m = menu(&ids(), TrayState::default());
        assert!(m.iter().any(|i| i.id == ID_OPEN && i.default));
        assert!(m.iter().any(|i| i.id == ID_QUIT));
        // Quit is the last item: the place where people look for it.
        assert_eq!(m.last().unwrap().id, ID_QUIT);
    }

    #[test]
    fn подменю_разделов_повторяет_порядок_окна() {
        english();
        let m = menu(&ids(), TrayState::default());
        let inside: Vec<&MenuItem> = m
            .iter()
            .skip_while(|i| i.kind != ItemKind::SubmenuBegin)
            .skip(1)
            .take_while(|i| i.kind != ItemKind::SubmenuEnd)
            .collect();
        assert_eq!(inside.len(), 6);
        assert_eq!(inside[0].id, ID_SECTION_BASE);
        assert_eq!(inside[0].label, "Desktops");
        assert_eq!(inside[5].label, "Settings");
    }

    #[test]
    fn без_вкладок_подменю_разделов_не_строится() {
        english();
        let m = menu(&[], TrayState::default());
        assert!(!m
            .iter()
            .any(|i| i.kind == ItemKind::SubmenuBegin && i.label == "Sections"));
    }

    #[test]
    fn без_демона_полоса_обоев_погашена_а_окно_и_выход_живы() {
        english();
        let m = menu(&ids(), TrayState::default());
        let by = |id: i32| m.iter().find(|i| i.id == id).unwrap();
        for id in [ID_NEXT, ID_PREV, ID_SHUFFLE, ID_PAUSE, ID_MUTE, ID_RESCAN] {
            assert!(!by(id).enabled, "пункт {id} обязан быть погашен без демона");
        }
        assert!(by(ID_OPEN).enabled);
        assert!(by(ID_QUIT).enabled);
        assert!(by(ID_AUTOSTART).enabled);
    }

    #[test]
    fn плейлист_rexon_ведёт_смену_и_без_демона() {
        english();
        let m = menu(&ids(), TrayState { playlist: true, ..Default::default() });
        let by = |id: i32| m.iter().find(|i| i.id == id).unwrap();
        assert!(by(ID_NEXT).enabled && by(ID_PREV).enabled);
        assert_eq!(by(ID_PLAYLIST_PAUSE).label, "Pause Playlist");
        // The daemon range without the daemon is still greyed out.
        assert!(!by(ID_PAUSE).enabled);
        let paused = menu(&ids(), TrayState { playlist: true, playlist_paused: true, ..Default::default() });
        assert_eq!(paused.iter().find(|i| i.id == ID_PLAYLIST_PAUSE).unwrap().label, "Resume Playlist");
    }

    #[test]
    fn право_изменять_систему_видно_только_с_сеансом() {
        english();
        let without = menu(&ids(), TrayState::default());
        assert!(!without.iter().any(|i| i.id == ID_ADMIN_INFO || i.id == ID_REVOKE));
        let with = menu(&ids(), TrayState { admin_session: true, ..Default::default() });
        let info = with.iter().find(|i| i.id == ID_ADMIN_INFO).unwrap();
        assert!(!info.enabled, "строка состояния не нажимается");
        assert!(info.label.starts_with("🔓 "), "{}", info.label);
        let revoke = with.iter().find(|i| i.id == ID_REVOKE).unwrap();
        assert!(revoke.enabled);
        assert_eq!(revoke.label, "Revoke Now");
    }

    #[test]
    fn подписи_паузы_и_звука_переворачиваются() {
        english();
        let playing = TrayState { daemon: true, ..Default::default() };
        let paused = TrayState { daemon: true, paused: true, muted: true, ..Default::default() };
        let find = |st, id| {
            menu(&ids(), st)
                .into_iter()
                .find(|i| i.id == id)
                .unwrap()
                .label
        };
        assert_eq!(find(playing, ID_PAUSE), "Pause");
        assert_eq!(find(paused, ID_PAUSE), "Resume");
        assert_eq!(find(playing, ID_MUTE), "Mute");
        assert_eq!(find(paused, ID_MUTE), "Unmute");
    }

    #[test]
    fn отмечен_ровно_один_интервал_смены() {
        english();
        let st = TrayState { daemon: true, rotation_secs: 300, ..Default::default() };
        assert!(!menu(&ids(), st).iter().any(|i| i.id == ID_PLAYLIST_PAUSE), "без плейлиста пункта нет");
        let m = menu(&ids(), st);
        let marked: Vec<&MenuItem> = m
            .iter()
            .filter(|i| i.kind == ItemKind::Radio && i.checked)
            .collect();
        assert_eq!(marked.len(), 1);
        assert_eq!(marked[0].label, "5 Minutes");
        assert_eq!(rotate_secs_for(marked[0].id), Some(300));
    }

    #[test]
    fn чужой_интервал_демона_не_отмечает_ничего() {
        english();
        let st = TrayState { daemon: true, rotation_secs: 42, ..Default::default() };
        let m = menu(&ids(), st);
        assert!(!m.iter().any(|i| i.kind == ItemKind::Radio && i.checked));
    }

    #[test]
    fn номера_интервалов_разбираются_обратно() {
        english();
        for (index, (_, secs)) in ROTATE_PRESETS.iter().enumerate() {
            assert_eq!(rotate_secs_for(ID_ROTATE_BASE + index as i32), Some(*secs));
        }
        assert_eq!(rotate_secs_for(ID_ROTATE_BASE - 1), None);
        assert_eq!(
            rotate_secs_for(ID_ROTATE_BASE + ROTATE_PRESETS.len() as i32),
            None
        );
        // The interval range does not overlap the neighbouring groups.
        assert!(ID_ROTATE_BASE + ROTATE_PRESETS.len() as i32 <= ID_PAUSE);
    }

    #[test]
    fn английское_меню_без_кириллицы() {
        english();
        let st = TrayState { daemon: true, ..Default::default() };
        for item in menu(&ids(), st) {
            assert!(
                !item.label.chars().any(|c| ('а'..='я').contains(&c.to_ascii_lowercase())
                    || c == 'ё'),
                "английская подпись содержит кириллицу: {}",
                item.label
            );
        }
    }

    #[test]
    fn каждый_ключ_подписан() {
        english();
        for (key, msgid) in LABELS {
            assert!(!msgid.is_empty(), "нет подписи для {key}");
        }
        // Keys are unique — otherwise `label` silently takes the first one.
        let mut keys: Vec<&str> = LABELS.iter().map(|(k, _)| *k).collect();
        keys.sort_unstable();
        let before = keys.len();
        keys.dedup();
        assert_eq!(before, keys.len(), "в таблице подписей повторяется ключ");
    }

    /// An icon is attached to an EXISTING label: a key without a label is a
    /// typo, and an icon in an empty place would hide it.
    #[test]
    fn every_icon_has_a_label() {
        english();
        for (key, glyph) in ICONS {
            assert!(!glyph.is_empty(), "пустой значок у {key}");
            assert!(
                label_msgid(key).is_some(),
                "значок {glyph} висит на ключе {key}, которого нет в подписях"
            );
        }
    }

    /// The label with its icon is the label plus the icon, not a third string:
    /// the translation must reach the window the same way as the tray.
    #[test]
    fn titled_is_icon_plus_label() {
        english();
        assert_eq!(titled("tab.desktops"), "🖥️ Desktops");
        assert_eq!(titled("quit"), "🚪 Quit");
        // A key without an icon returns the bare label, not emptiness.
        assert_eq!(titled("open"), label("open"));
        assert!(titled("нет такого").is_empty());
    }

    /// The tray icon menu does NOT show icons: they are decoration of the window.
    #[test]
    fn tray_menu_stays_without_icons() {
        english();
        let ids: Vec<String> = vec!["desktops".into(), "settings".into()];
        for item in menu(&ids, TrayState::default()) {
            for (_, glyph) in ICONS {
                assert!(
                    !item.label.contains(glyph),
                    "в меню лотка пробрался значок {glyph}: {}",
                    item.label
                );
            }
        }
    }

    /// The autostart label in the icon menu and in the «Settings» section is ONE
    /// setting, and it must have one text: the customer must not
    /// guess whether it is one and the same (rule #12 — one engine, two doors).
    #[test]
    fn подпись_автозапуска_одна_на_меню_и_настройки() {
        english();
        // «Settings» hold the English `msgid` (Russian is in `po/ru.po`),
        // so the source text is compared: the English label of the icon menu.
        assert_eq!(
            label_msgid("autostart"),
            crate::qt_settings::text_msgid("tray.autostart")
        );
    }

    #[test]
    fn незнакомый_раздел_подписан_хоть_как_то() {
        english();
        assert_eq!(section_label("desktops"), "Desktops");
        assert_eq!(section_label("новая"), "новая");
    }

    #[test]
    fn флаг_свёрнутого_запуска() {
        english();
        assert!(starts_hidden(&["--tray".to_string()]));
        assert!(starts_hidden(&["--page=install".into(), "--tray".into()]));
        assert!(!starts_hidden(&[]));
        assert!(!starts_hidden(&["--traylike".to_string()]));
    }

    #[test]
    fn путь_записи_автозапуска() {
        english();
        assert_eq!(
            autostart_path(None, "/home/t"),
            "/home/t/.config/autostart/rexon.desktop"
        );
        assert_eq!(
            autostart_path(Some("/cfg"), "/home/t"),
            "/cfg/autostart/rexon.desktop"
        );
        // Our own entry, not someone else's: the splash and default applications are not touched.
        let mine = autostart_path(None, "/home/t");
        assert_ne!(mine, crate::session_splash::autostart_path(None, "/home/t"));
        assert!(!mine.ends_with("rexon-appdefaults.desktop"));
    }

    #[test]
    fn запись_автозапуска_поднимает_в_лоток() {
        english();
        use crate::installer::LaunchEntry;
        let py = LaunchEntry::Python;
        let exec = autostart_exec(&py, Some("/usr/local/bin/rexon")).expect("лаунчер нашёлся");
        assert_eq!(exec, "/usr/local/bin/rexon --tray");
        let body = autostart_desktop_content(&exec);
        assert!(body.starts_with("[Desktop Entry]\n"));
        assert!(body.contains("\nExec=/usr/local/bin/rexon --tray\n"));
        assert!(body.contains("\nIcon=rexon\n"));
        assert!(body.contains("\nComment[ru]=Запускать Rexon в системном лотке при входе\n"));
        assert!(body.ends_with('\n'));
        // ⛔Nothing to launch — there will be NO entry at all. Previously this gave
        // `python3 -m rexon.app --tray`: a line from autostart is executed
        // without a working directory, the `rexon` package is not in the `sys.path` of the system
        // interpreter — logging into the session silently did nothing.
        assert_eq!(autostart_exec(&py, None), None);
        assert_eq!(autostart_exec(&py, Some("")), None);
        assert_eq!(autostart_exec(&py, Some("   ")), None);
        // Rust entry: the tray raises the Rust window, the `rexon` launcher is not involved.
        let rust = LaunchEntry::MainWindow("/usr/lib/rexon/rexon-main-window".into());
        assert_eq!(
            autostart_exec(&rust, Some("/usr/local/bin/rexon")).as_deref(),
            Some("/usr/lib/rexon/rexon-main-window --tray")
        );
        assert_eq!(
            autostart_exec(&rust, None).as_deref(),
            Some("/usr/lib/rexon/rexon-main-window --tray")
        );
        // A space in the path does not split the command into two arguments.
        let spaced = LaunchEntry::MainWindow("/home/w/мой Rexon/rexon-main-window".into());
        assert_eq!(
            autostart_exec(&spaced, None).as_deref(),
            Some("\"/home/w/мой Rexon/rexon-main-window\" --tray")
        );
        // No entry point writes an interpreter into autostart.
        for e in [&py, &rust, &spaced] {
            if let Some(line) = autostart_exec(e, Some("/usr/local/bin/rexon")) {
                assert!(!line.contains("python"), "в автозапуске интерпретатор: {line}");
                assert!(!line.contains("-m rexon"), "в автозапуске модуль: {line}");
            }
        }
    }

    /// The name that Qt puts onto the bus as the icon's `Id` property must be
    /// machine-like and ONE per product. Both properties guard a live defect:
    /// the translated window title used to get there, and a customer who changed the
    /// language lost the tray icon settings (the shell remembers them by `Id`).
    #[test]
    fn имя_на_шине_машинное_и_совпадает_с_таблицей_точек_входа() {
        english();
        // Not the window title and not a translatable string at all.
        assert_ne!(APP_ID, crate::qt_main::window_title());
        assert!(APP_ID.is_ascii(), "имя на шине обязано быть латиницей");
        assert!(!APP_ID.contains(' '), "имя на шине — одно слово");
        // The product has no second name: the same entry as for the Python
        // owner of the main window.
        let entry = crate::qt_app::identity("rexon.qt_main.app")
            .expect("главное окно есть в таблице точек входа");
        assert_eq!(APP_ID, entry.application_name);
    }

    #[test]
    fn кандидаты_значка_покрывают_дерево_и_пакет() {
        english();
        let c = icon_candidates("/repo");
        assert_eq!(c[0], "/repo/rexon_icon_256.png");
        assert!(c.iter().any(|p| p == "/usr/share/pixmaps/rexon.png"));
        // Without the repository tree only package paths remain.
        assert!(icon_candidates("").iter().all(|p| p.starts_with("/usr/")));
    }

    /// Root of the developer tree by the binary path — and, more importantly, EMPTY where
    /// there is no tree. Lying here, the icon would start looking for the picture in three
    /// certainly dead paths for every customer.
    #[test]
    fn корень_дерева_считается_по_пути_бинаря() {
        english();
        assert_eq!(
            repo_root_from_exe("/home/u/Rexon/rexon_v5/rust/target/release/rexon-main-window"),
            "/home/u/Rexon/rexon_v5"
        );
        assert_eq!(
            repo_root_from_exe("/home/u/Rexon/rexon_v5/rust/target/debug/rexon-tray-probe"),
            "/home/u/Rexon/rexon_v5"
        );
        // The customer has no tree — and it must not be invented.
        assert_eq!(repo_root_from_exe("/usr/bin/rexon"), "");
        assert_eq!(repo_root_from_exe("/usr/lib/rexon/rexon-main-window"), "");
        assert_eq!(repo_root_from_exe(""), "");
        // Similar but not it: `target` without `rust` above it.
        assert_eq!(repo_root_from_exe("/opt/x/target/release/rexon"), "");
    }

    /// The icon file must be found by the root that the function above
    /// computed: the "root → candidates" link is easy to break apart.
    #[test]
    fn кандидаты_значка_собираются_из_посчитанного_корня() {
        english();
        let root = repo_root_from_exe("/tree/rust/target/release/rexon-main-window");
        let candidates = icon_candidates(&root);
        assert_eq!(candidates[0], "/tree/rexon_icon_256.png");
        // An empty root adds no dead paths — only system ones
        // remain.
        let system = icon_candidates("");
        assert!(system.iter().all(|p| p.starts_with("/usr/")));
    }
}
