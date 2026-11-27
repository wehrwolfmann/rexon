//! Port of the pure part of `rexon/app.py`: the entry point and the KDE
//! emergency rollback.
//!
//! In Python the module mixes decisions with execution: it runs
//! `kwriteconfig`, rewrites configs, pokes plasmashell. What moved here is
//! the part that does NOT depend on the system: parsing command-line
//! arguments into a route, detecting traces of a fork theme, panel layout
//! surgery (a pure transformation of the appletsrc text) and the argv of
//! the reset commands.
//!
//! The `--recover-kde` emergency rollback must work even when Qt is broken,
//! so neither the core nor the executor of this branch touches Qt.

use std::collections::BTreeSet;
use std::fmt;
use std::sync::Mutex;

use crate::i18n::{fill, n_, ntr, tr, trf};

// ============================================================================
// A constant string translated at the moment it is shown
// ============================================================================

/// A human-facing string that lives as a CONSTANT (`app::FONTS_RESET_LINE`)
/// but reaches the screen already translated, into the language chosen by
/// the time it is shown.
///
/// WHY A SEPARATE TYPE. These constants are used by the rollback executor
/// (`system_exec`), the login splash (`session_splash`), the tray and the
/// parity checks, and all of them pass them where a `&str` is expected
/// (`out(app::FONTS_RESET_LINE)`) or compare them with a string. `&TrLine`
/// coerces to `&str` by itself (through [`std::ops::Deref`]) and compares
/// with `String`, so the translation did not have to rewrite a single place
/// where they are shown. Inside lies the `msgid` (marked with [`n_`] so
/// `xgettext` sees it); the translation is fetched with [`tr`] on every
/// access.
///
/// The price of coercing to `&str`: the translated text settles in process
/// memory forever (one copy per "string x language" pair, and there are
/// about a dozen strings).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrLine(&'static str);

impl TrLine {
    /// A constant from a `msgid`; mark the `msgid` itself with `n_(…)`.
    pub const fn new(msgid: &'static str) -> Self {
        Self(msgid)
    }

    /// The source (English) string, the catalog key.
    pub const fn msgid(&self) -> &'static str {
        self.0
    }

    /// The translated text.
    pub fn text(&self) -> String {
        tr(self.0)
    }
}

/// The translated text with the lifetime of the program. Identical texts do
/// not pile up: the second time the already settled copy is returned.
fn interned(text: String) -> &'static str {
    static SEEN: Mutex<BTreeSet<&'static str>> = Mutex::new(BTreeSet::new());
    // A poisoned lock (a panic in another thread in the middle of an insert)
    // is no obstacle for the emergency route: the set is not corrupted by it.
    let mut seen = SEEN.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(known) = seen.get(text.as_str()) {
        return known;
    }
    let leaked: &'static str = Box::leak(text.into_boxed_str());
    seen.insert(leaked);
    leaked
}

impl std::ops::Deref for TrLine {
    type Target = str;
    fn deref(&self) -> &str {
        interned(self.text())
    }
}

impl AsRef<str> for TrLine {
    fn as_ref(&self) -> &str {
        self
    }
}

impl fmt::Display for TrLine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text())
    }
}

impl PartialEq<TrLine> for str {
    fn eq(&self, other: &TrLine) -> bool {
        self == other.text()
    }
}

impl PartialEq<TrLine> for String {
    fn eq(&self, other: &TrLine) -> bool {
        *self == other.text()
    }
}

impl PartialEq<&TrLine> for String {
    fn eq(&self, other: &&TrLine) -> bool {
        *self == other.text()
    }
}

// ============================================================================
// Command-line route
// ============================================================================

/// Standalone Qt tabs: flag -> executor module. The order matches the
/// original; priority with several flags at once relies on it too.
pub const SINGLE_TAB_FLAGS: &[(&str, &str)] = &[
    ("--appearance-qt", "rexon.appearance_page"),
    ("--extensions-qt", "rexon.qt_extensions.app"),
    ("--compare-qt", "rexon.qt_compare.app"),
    ("--settings-qt", "rexon.qt_settings.app"),
    ("--desktops-qt", "rexon.qt_desktops.app"),
];

/// What to do according to the parsed command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    /// `--apply-defaults`: autostart hook, apply the session's DE defaults.
    ApplyDefaults,
    /// `--sanitize-gtk`: repair the links in gtk-{3,4}.0/settings.ini.
    SanitizeGtk,
    /// A standalone Qt tab; the field is the module whose `main` is called.
    SingleTab { module: &'static str },
    /// `--session-splash`: the login splash (Qt is not started).
    SessionSplash,
    /// `--recover-kde`: emergency rollback of the Global Theme.
    RecoverKde,
    /// `--aeroshell-check [--login]`: AeroThemePlasma check without a window;
    /// the user unit calls it after package updates and at login.
    AeroshellCheck { login: bool },
    /// `--update-check [--login]`: the background "before and after an update"
    /// check ([`crate::update_guard`]); the user unit calls it, and with Plasma
    /// forks installed it also leads to the AeroThemePlasma check.
    UpdateCheck { login: bool },
    /// `--conky-set <name>`: apply a set of Conky widgets without a window
    /// ([`crate::conky_sets::cli_route`]).
    ConkySet,
    /// `--apply-day-night [--login]`: light or dark by schedule
    /// ([`crate::day_night::cli_route`]); the timer and autostart call it.
    DayNight,
    /// `rexon doctor`: why the login screen does not come up and what to do;
    /// it only looks ([`crate::login_rescue::doctor_live`]).
    Doctor,
    /// `--help` / `-h`: command-line help, printed and nothing else.
    Help,
    /// `--version` / `-V`: the Rexon version on one line.
    Version,
    /// No flags or `--qt`: the single window with tabs.
    MainWindow,
}

/// Help flags. Help and version come before every other route and without a
/// window: `rexon --help` is called from a text console and over SSH, where
/// there is no display, and until 30.10.2026 it fell through to the main
/// window: Qt found no platform and the process died with `abort` instead
/// of printing help.
pub const HELP_FLAGS: &[&str] = &["--help", "-h"];

/// Help as a word, like `rexon doctor`: `rexon help`.
pub const HELP_WORD: &str = "help";

/// Version flags (see [`HELP_FLAGS`]).
pub const VERSION_FLAGS: &[&str] = &["--version", "-V"];

/// Parse the arguments into a route. The order of checks is as in the
/// original: service hooks before the UI, `--recover-kde` last among the
/// flags, because it is the heaviest and must not intercept narrower modes.
///
/// `args` are the arguments WITHOUT the program name (`sys.argv[1:]`); the
/// original looks for flags across the whole `sys.argv`, but the program
/// name cannot be a flag.
pub fn route(args: &[String]) -> Route {
    let has = |flag: &str| args.iter().any(|a| a == flag);

    // Help and version beat everything: `rexon --recover-kde --help` must
    // explain the rollback, not run it. The word `help` counts only in first
    // position, like `doctor`: otherwise a Conky set named "help" would become
    // the help.
    if args.first().is_some_and(|a| a == HELP_WORD) || HELP_FLAGS.iter().any(|f| has(f)) {
        return Route::Help;
    }
    if VERSION_FLAGS.iter().any(|f| has(f)) {
        return Route::Version;
    }

    if has("--apply-defaults") {
        return Route::ApplyDefaults;
    }
    if has("--sanitize-gtk") {
        return Route::SanitizeGtk;
    }
    for (flag, module) in SINGLE_TAB_FLAGS {
        if has(flag) {
            return Route::SingleTab { module };
        }
    }
    if has("--session-splash") {
        return Route::SessionSplash;
    }
    if has(RECOVER_KDE_FLAG) {
        return Route::RecoverKde;
    }
    if has(crate::aeroshell::CHECK_FLAG) {
        return Route::AeroshellCheck {
            login: has(crate::aeroshell::LOGIN_FLAG),
        };
    }
    if args.iter().any(|a| a == crate::conky_sets::CLI_FLAG || a.starts_with(&format!("{}=", crate::conky_sets::CLI_FLAG))) {
        return Route::ConkySet;
    }
    if has(crate::day_night::CLI_FLAG) {
        return Route::DayNight;
    }
    if crate::login_rescue::is_doctor_args(args) {
        return Route::Doctor;
    }
    if has(crate::update_guard::CHECK_FLAG) {
        return Route::UpdateCheck {
            login: has(crate::aeroshell::LOGIN_FLAG),
        };
    }
    Route::MainWindow
}

/// The flag of the appearance emergency rollback. One for the route parser
/// and for the window that launches this route
/// ([`crate::window_menu::recover_argv`]): two copies of the flag would
/// drift apart silently, and the menu item would call a route that does not
/// exist.
pub const RECOVER_KDE_FLAG: &str = "--recover-kde";

/// Arguments passed to the main window: `--qt` is a historical alias of
/// "default behaviour" and does not reach the window.
pub fn main_window_args(args: &[String]) -> Vec<String> {
    args.iter().filter(|a| *a != "--qt").cloned().collect()
}

// ============================================================================
// Desktop shortcuts: `--page=<name>`
// ============================================================================
//
// THE DEFECT THIS CLOSES. The Rexon shortcut carries three quick actions
// (`installer::desktop_content`, also in `packaging/aur/rexon.desktop`):
// "Install a desktop" is `rexon --page=install`, "Manage backups" is
// `rexon --page=backups`, "Compare desktops" is `rexon --page=compare`.
// NOBODY parsed the `--page=` flag: neither `route` here nor Python.
// `route` returned `MainWindow`, `main_window_args` passed the string on
// untouched, Qt silently ignored the unknown argument, and all three
// items opened the same window on the default section. Right-clicking the
// shortcut -> "Manage backups" took the person to "Desktops" instead of
// the backups; "Compare" went to the same place. The menu makes a promise
// with no behaviour behind it, and that is exactly the silent deception
// rule #13 forbids.
//
// The table lives HERE because the names in the shortcut are written by a
// neighbouring module (`installer::desktop_content`), and both it and the
// parser must read one list. The guard `ярлык_не_обещает_неизвестных_страниц`
// in `tests/app_parity.rs` turns red if a `--page=` appears in the
// shortcut without an entry here.

/// Prefix of the shortcut flag: `--page=install`.
pub const PAGE_FLAG: &str = "--page=";

/// Name from the shortcut -> (main window section, subsection inside it).
///
/// The section is an identifier from [`crate::qt_main::SECTIONS`]; there is
/// no copy of the section list here, and the two lists are compared in the
/// tests. The subsection is empty when the section itself should open.
///
/// "DE comparison" has lived INSIDE "Desktops" since 08.07.2026, so
/// `compare` has the same section as `install`, and the subsection tells
/// them apart.
pub const PAGE_TARGETS: &[(&str, &str, &str)] = &[
    ("install", "desktops", ""),
    ("backups", "settings", ""),
    ("compare", "desktops", "compare"),
];

/// Where to open the window for `--page=…`: (section, subsection).
///
/// `None`: there is no flag at all, or the name is unknown. An unknown name
/// is deliberately not treated as a launch error: the window must open at
/// least on the default section rather than refuse the person over a typo
/// in someone else's shortcut.
pub fn start_page(args: &[String]) -> Option<(&'static str, &'static str)> {
    let name = args
        .iter()
        .find_map(|a| a.strip_prefix(PAGE_FLAG))?
        .trim()
        .to_lowercase();
    PAGE_TARGETS
        .iter()
        .find(|(page, _, _)| *page == name)
        .map(|(_, section, subpage)| (*section, *subpage))
}

// ============================================================================
// What starts Rexon: one truth for all launch lines written to disk
// ============================================================================

/// The program for a launch line that Rexon writes to disk: the `Exec=` of
/// autostart and of the shortcut, the `ExecStart=` of a systemd unit.
/// `None` means there is NOTHING to start it with, and then the line must
/// not be written: a dead entry comes to life at the next login, when
/// nobody is left to explain the breakage (rule #13: refuse out loud rather
/// than report success).
///
/// ⚠️ ONE PLACE FOR THE WHOLE PRODUCT (rule #12). All three writers of
/// launch lines meet here: the tray ([`crate::tray::autostart_exec`]), the
/// login splash ([`crate::session_splash::desktop_launcher_exec`]) and the
/// autostart of default applications
/// ([`crate::appdefaults::autostart_exec_for`]). They must not drift apart:
/// the "one Rexon per session" lock is shared by all the entries, and an
/// entry that starts a DIFFERENT entry point turns into a process that
/// knocks on a taken lock and silently exits.
///
/// The entry point itself is chosen by
/// [`crate::installer::launch_entry_live`], also one function for everyone,
/// and it knows every layout where the window binary can live: `PATH`
/// directories, the distribution package, a system install, a home folder
/// install, a developer tree behind the symlink of the installed directory
/// ([`crate::installer::main_window_candidates`]). There is no binary
/// search of its own here, and one must not be added.
///
/// ⛔ There is NO fallback "start an interpreter and call the module" here.
/// It lived in the tray and the splash until 16.09.2026
/// (`python3 -m rexon.app …`) and was dead by construction: the line from
/// an autostart `.desktop` entry runs without any working directory, and
/// the `rexon` package is not on the `sys.path` of the system interpreter,
/// so logging in silently did nothing. The same defect had already been
/// fixed for default applications (`appdefaults_exec`, item 1 of the
/// header).
///
/// ⭐ Fixed during the port: the path went into `Exec=` without quotes, and
/// a directory with a space split the command into two arguments.
pub fn launch_program(
    entry: &crate::installer::LaunchEntry,
    rexon_path: Option<&str>,
) -> Option<String> {
    use crate::installer::{desktop_exec_arg, LaunchEntry};
    match entry {
        LaunchEntry::MainWindow(_) => Some(entry.exec_program()),
        LaunchEntry::Python => nonempty(rexon_path).map(desktop_exec_arg),
    }
}

/// [`launch_program`] with its own flags: `<program> <flag>…`. The flags
/// need no quotes: they are our own words without spaces.
pub fn launch_command(
    entry: &crate::installer::LaunchEntry,
    rexon_path: Option<&str>,
    args: &[&str],
) -> Option<String> {
    let mut out = launch_program(entry, rexon_path)?;
    for a in args {
        out.push(' ');
        out.push_str(a);
    }
    Some(out)
}

/// The same for the `ExecStart=` of a systemd unit. Separate from
/// [`launch_command`] only because of quoting: in a Desktop Entry the key
/// value is unescaped BEFORE the arguments are parsed (the backslash is
/// doubled once more), while systemd would read the extra backslash
/// literally ([`crate::session_splash::systemd_exec_arg`]).
pub fn unit_launch_command(
    entry: &crate::installer::LaunchEntry,
    rexon_path: Option<&str>,
    args: &[&str],
) -> Option<String> {
    use crate::installer::LaunchEntry;
    let program = match entry {
        LaunchEntry::MainWindow(path) => path.as_str(),
        LaunchEntry::Python => nonempty(rexon_path)?,
    };
    let mut out = crate::session_splash::systemd_exec_arg(program);
    for a in args {
        out.push(' ');
        out.push_str(a);
    }
    Some(out)
}

/// A non-empty value without surrounding whitespace; an empty string means
/// "not found", not a path (the original's `shutil.which` returned `None`,
/// the bridge and the executors an empty string).
fn nonempty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|v| !v.is_empty())
}

/// The refusal when there is nothing to start Rexon with: the window binary
/// was not found in any layout, and there is no `rexon` launcher in `PATH`
/// either.
pub const MSG_NOTHING_TO_LAUNCH: &TrLine = &TrLine::new(n_(
    "Couldn't find anything to start Rexon with: no window binary (package, system \
     install, install in the home folder, developer tree) and no `rexon` launcher in \
     PATH. The autostart entry wasn't created — it would point to nothing.",
));

// ============================================================================
// Traces of a fork theme (Aero and relatives)
// ============================================================================

/// Distinctive tokens of a fork theme. NOT bare `aero`/`win7`: those catch
/// legitimate strings such as a wallpaper name and trigger a panel reset
/// for nothing.
pub const FORK_TOKENS: &[&str] = &[
    "wackyideas",
    "aeroshell",
    "aerothemeplasma",
    "sevenstart",
    "se7enaero",
    "win7os",
];

/// Whether the string contains a fork theme token (case-insensitive).
pub fn has_fork_token(text: &str) -> bool {
    let lower = text.to_lowercase();
    FORK_TOKENS.iter().any(|t| lower.contains(t))
}

/// Files where traces of a fork are looked for: the panel layout and the
/// SDDM configs. Expanding the `/etc/sddm.conf.d/*.conf` glob is up to the
/// executor.
pub fn fork_residue_files(home: &str) -> (String, &'static str, &'static str) {
    (appletsrc_path(home), "/etc/sddm.conf.d", "/etc/sddm.conf")
}

/// Path of the Plasma desktop and panel layout.
pub fn appletsrc_path(home: &str) -> String {
    appletsrc_path_xdg(None, home)
}

/// The same, honouring `$XDG_CONFIG_HOME` (the core's single config root).
pub fn appletsrc_path_xdg(xdg_config_home: Option<&str>, home: &str) -> String {
    format!(
        "{}/plasma-org.kde.plasma.desktop-appletsrc",
        crate::utils::config_home(xdg_config_home, home)
    )
}

/// Path of the backup placed next to it before panel surgery.
pub fn appletsrc_backup_path(appletsrc: &str) -> String {
    format!("{appletsrc}.rexon-bak")
}

/// Remove the fork theme's applet groups from the appletsrc text.
///
/// The KConfig file is split into sections "header + lines up to the next
/// header"; a section counts as foreign if it has a `plugin=` with a fork
/// token. Such sections are dropped TOGETHER with their nested subsections
/// (`[…][Configuration]` and deeper); otherwise the orphaned configuration
/// stays in the file as garbage.
///
/// Other applets, systemtray and its `extraItems`/`knownItems` are left
/// alone: the stock volume, battery and network come back by themselves.
///
/// Returns `(new text, removed plugins)`; if there is no fork, the text is
/// returned byte for byte unchanged.
pub fn strip_fork_applets(text: &str) -> (String, Vec<String>) {
    // Split into sections: (header, lines). The preamble before the first
    // header goes with `None` and is never dropped.
    let mut sections: Vec<(Option<String>, Vec<&str>)> = Vec::new();
    let mut cur_header: Option<String> = None;
    let mut cur_lines: Vec<&str> = Vec::new();

    for line in split_keepends(text) {
        let s = line.trim();
        if s.starts_with('[') && s.ends_with(']') {
            sections.push((cur_header.take(), std::mem::take(&mut cur_lines)));
            cur_header = Some(s.to_string());
            cur_lines.push(line);
        } else {
            cur_lines.push(line);
        }
    }
    sections.push((cur_header, cur_lines));

    // Headers of sections whose plugin belongs to the fork.
    let mut fork_headers: Vec<String> = Vec::new();
    let mut removed: Vec<String> = Vec::new();
    for (header, body) in &sections {
        let Some(header) = header else { continue };
        for line in body {
            let st = line.trim();
            if let Some(plugin) = st.strip_prefix("plugin=") {
                if has_fork_token(st) {
                    fork_headers.push(header.clone());
                    removed.push(plugin.to_string());
                    break;
                }
            }
        }
    }

    if fork_headers.is_empty() {
        return (text.to_string(), Vec::new());
    }

    let mut out = String::with_capacity(text.len());
    for (header, body) in &sections {
        if let Some(h) = header {
            if fork_headers.iter().any(|fh| h.starts_with(fh.as_str())) {
                continue; // drop the section together with its subsections
            }
        }
        for line in body {
            out.push_str(line);
        }
    }
    (out, removed)
}

/// Port of `str.splitlines(keepends=True)`: lines together with their endings.
fn split_keepends(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\n' {
            out.push(&text[start..=i]);
            i += 1;
            start = i;
        } else if bytes[i] == b'\r' {
            let end = if i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
                i + 1
            } else {
                i
            };
            out.push(&text[start..=end]);
            i = end + 1;
            start = i;
        } else {
            i += 1;
        }
    }
    if start < bytes.len() {
        out.push(&text[start..]);
    }
    out
}

// ============================================================================
// argv of the reset commands
// ============================================================================

/// kdeglobals keys removed when fonts are reset. `--delete` removes the
/// key, after which Plasma takes its own default: more reliable than
/// hardcoding a specific font.
pub const KDE_FONT_KEYS: &[&str] = &[
    "font",
    "fixed",
    "smallestReadableFont",
    "toolBarFont",
    "menuFont",
    "activeFont",
];

/// KWin effects from fork themes, removed by name. Other and user effects
/// are left alone: only these `<effect>Enabled` keys are deleted.
pub const KWIN_FORK_EFFECTS: &[&str] = &[
    "aeroglassblur",
    "aeroglide",
    "dimscreenaero",
    "smod",
    "smodglow",
    "smodanimations",
    "aerosnap",
];

/// argv that removes all font overrides (one call per key).
pub fn kde_font_reset_argv(kwriteconfig: &str) -> Vec<Vec<String>> {
    KDE_FONT_KEYS
        .iter()
        .map(|key| {
            vec![
                kwriteconfig.to_string(),
                "--file".into(),
                "kdeglobals".into(),
                "--group".into(),
                "General".into(),
                "--key".into(),
                (*key).to_string(),
                "--delete".into(),
            ]
        })
        .collect()
}

/// argv that removes the document font which the "Fonts" page writes in
/// KDE to the GNOME key (`qt_settings::DOCUMENT_FONT_GNOME`: Plasma has no
/// key of its own). `gsettings` is the found program path. Rexon does not
/// write the desktop icon label font in KDE, so there is nothing to remove
/// there.
pub fn document_font_reset_argv(gsettings: &str) -> Vec<String> {
    let (schema, key) = crate::qt_settings::DOCUMENT_FONT_GNOME;
    vec![gsettings.to_string(), "reset".into(), schema.to_string(), key.to_string()]
}

/// argv that removes the KWin effects of fork themes (one call per effect).
pub fn kwin_effect_reset_argv(kwriteconfig: &str) -> Vec<Vec<String>> {
    KWIN_FORK_EFFECTS
        .iter()
        .map(|eff| {
            vec![
                kwriteconfig.to_string(),
                "--file".into(),
                "kwinrc".into(),
                "--group".into(),
                "Plugins".into(),
                "--key".into(),
                format!("{eff}Enabled"),
                "--delete".into(),
            ]
        })
        .collect()
}

/// argv that makes the running KWin reread its configuration.
pub fn kwin_reconfigure_argv(qdbus: &str) -> Vec<String> {
    vec![
        qdbus.to_string(),
        "org.kde.KWin".into(),
        "/KWin".into(),
        "org.kde.KWin.reconfigure".into(),
    ]
}

/// argv that returns the widget style to Breeze.
pub fn widget_style_breeze_argv(kwriteconfig: &str) -> Vec<String> {
    vec![
        kwriteconfig.to_string(),
        "--file".into(),
        "kdeglobals".into(),
        "--group".into(),
        "KDE".into(),
        "--key".into(),
        "widgetStyle".into(),
        "Breeze".into(),
    ]
}

/// Whether a Kvantum theme looks like the Aero family. Legitimate themes
/// (Dracula and others) are left alone; only these signs count.
pub fn is_aero_kvantum_theme(theme: &str) -> bool {
    let t = theme.to_lowercase();
    ["aero", "seven", "win7", "windows7"]
        .iter()
        .any(|m| t.contains(m))
}

/// Candidates for the kwriteconfig binary in order of preference.
pub const KWRITECONFIG_BINS: &[&str] = &["kwriteconfig6", "kwriteconfig5"];
/// Candidates for the qdbus binary in order of preference.
pub const QDBUS_BINS: &[&str] = &["qdbus6", "qdbus"];

// ============================================================================
// --recover-kde: returning desktop wallpapers to the stock ones
// ============================================================================

/// Wallpaper directories that are candidates for "stock": the regular
/// Plasma Next first, then the CachyOS sets (the main machine). If none is
/// found, the executor looks for any wallpaper package itself.
pub const DEFAULT_WALLPAPER_DIRS: &[&str] = &[
    "/usr/share/wallpapers/Next",
    "/usr/share/wallpapers/cachyos-wallpapers",
    "/usr/share/wallpapers/Cachy",
];

/// The containment that gets the wallpaper if appletsrc has no desktop
/// containment at all (in Plasma it is number one by default).
pub const FALLBACK_CONTAINMENT: &str = "128";

/// Numbers of the DESKTOP containments from the appletsrc text, in order of
/// appearance, without repeats.
///
/// A `[Containments][<N>]` section is remembered, and if a `plugin=…` line
/// containing `folder` or `desktopcontainment` appears IN THAT SECTION
/// ITSELF, the number goes into the list. Panels (`plugin=org.kde.panel`)
/// are filtered out by this on their own.
///
/// ⚠️ Fix of a bug in the original (rule #16): Python did not reset the
/// "current containment" on a foreign section header, so the `plugin=` of
/// ANY nested section was credited to the previous containment. A "Folder"
/// applet (`org.kde.plasma.folder`) placed in a panel made the PANEL a
/// "desktop", and the emergency reset wrote `wallpaperplugin`/`Image` to it
/// and inflated the number of "desktops" in the report. Here any
/// non-containment header closes the section.
pub fn desktop_containment_ids(appletsrc_text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur: Option<String> = None;
    for line in appletsrc_text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            cur = containment_header_id(trimmed);
            continue;
        }
        if let Some(id) = cur.as_ref() {
            if line.starts_with("plugin=")
                && (line.contains("folder") || line.contains("desktopcontainment"))
                && !out.iter().any(|x| x == id)
            {
                out.push(id.clone());
            }
        }
    }
    out
}

/// The number from a line of exactly the form `[Containments][<digits>]`
/// (port of the regular expression `^\[Containments\]\[(\d+)\]$`). Nested
/// sections (`[Containments][2][Applets][5]`) deliberately do NOT match: an
/// applet has its own `plugin=`, and it must not pass itself off as a
/// desktop.
fn containment_header_id(line: &str) -> Option<String> {
    const PREFIX: &str = "[Containments][";
    let rest = line.strip_prefix(PREFIX)?.strip_suffix(']')?;
    if rest.is_empty() || !rest.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(rest.to_string())
}

/// argv that returns a containment's wallpaper plugin to the stock image.
pub fn wallpaper_plugin_reset_argv(kwriteconfig: &str, containment: &str) -> Vec<String> {
    vec![
        kwriteconfig.to_string(),
        "--file".into(),
        "plasma-org.kde.plasma.desktop-appletsrc".into(),
        "--group".into(),
        "Containments".into(),
        "--group".into(),
        containment.to_string(),
        "--key".into(),
        "wallpaperplugin".into(),
        "org.kde.image".into(),
    ]
}

/// argv that writes the containment's wallpaper image itself.
/// `wallpaper_dir` is the wallpaper package directory WITH a trailing slash
/// (that is how Plasma itself writes it).
pub fn wallpaper_image_argv(
    kwriteconfig: &str,
    containment: &str,
    wallpaper_dir: &str,
) -> Vec<String> {
    vec![
        kwriteconfig.to_string(),
        "--file".into(),
        "plasma-org.kde.plasma.desktop-appletsrc".into(),
        "--group".into(),
        "Containments".into(),
        "--group".into(),
        containment.to_string(),
        "--group".into(),
        "Wallpaper".into(),
        "--group".into(),
        "org.kde.image".into(),
        "--group".into(),
        "General".into(),
        "--key".into(),
        "Image".into(),
        wallpaper_dir.to_string(),
    ]
}

/// argv that restarts plasmashell so the written wallpapers are picked up.
/// `reset-failed` comes first; otherwise a repeated restart after a crash
/// is rejected.
pub fn plasmashell_restart_argv() -> Vec<Vec<String>> {
    vec![
        vec![
            "systemctl".into(),
            "--user".into(),
            "reset-failed".into(),
            "plasma-plasmashell.service".into(),
        ],
        vec![
            "systemctl".into(),
            "--user".into(),
            "restart".into(),
            "plasma-plasmashell.service".into(),
        ],
    ]
}

/// Hint for installing PySide6, printed when the Qt stack is not found.
pub const PYSIDE6_INSTALL_HINT: &[&str] = &[
    "Arch:   sudo pacman -S pyside6",
    "Debian: sudo apt install python3-pyside6",
    "Fedora: sudo dnf install python3-pyside6",
];

// ============================================================================
// One Rexon per session: a second launch does not spawn a second tray icon
// ============================================================================
//
// The whole mechanism moved to `rexon_core::single_instance`: next to the
// lock and socket addresses now live the `flock` itself (it used to sit in
// the Python binding and was unreachable from the Rust entry point), the
// "raise the window" listener and the knock on it. The analysis of the
// defects all this grew out of is in the header of that module.
//
// Only the old names remain here: `rexon/app.py` calls the binding by
// them, and renaming for the sake of the move would break the live product
// for no reason.

pub use crate::single_instance::lock_path as single_instance_lock_path;
pub use crate::single_instance::socket_path as single_instance_socket_path;

// ============================================================================
// --recover-kde: decisions and texts of the emergency rollback
// ============================================================================
//
// WHY THIS PART IS HERE. The order of steps and the trips outside live in
// the executor (`system_exec::SystemExec::recover_kde`), and here is
// everything decided WITHOUT the system: which theme is applied, whether
// the current desktop counts as Plasma, the argv of every command and
// every line the person will see. The split is the same as in the
// neighbouring routes: the texts are pinned by parity checks
// (`tests/app_parity.rs` reads `rexon/app.py` through `include_str!`), and
// a second copy of the words in the executor would drift from the original
// silently.
//
// ⚠️ The route is an emergency one: it is called when the appearance is
// already broken, and it must work without Qt at all. So no line here
// touches Qt. Translation goes through `crate::i18n`, which does not touch
// Qt: the `.mo` catalog is read by plain Rust, and if the catalog is
// missing or broken, `tr` returns the original English string without a
// panic. The emergency route then speaks English rather than staying
// silent.

/// The theme applied when there is no backup.
pub const RECOVER_DEFAULT_TARGET: &str = "org.kde.breezedark.desktop";

/// The colour scheme restored after the theme.
pub const RECOVER_COLORSCHEME: &str = "BreezeDark";

/// Signs of Plasma in `$XDG_CURRENT_DESKTOP` (case-insensitive).
pub const PLASMA_DESKTOP_MARKERS: &[&str] = &["kde", "plasma"];

/// Whether the appearance can be restored in THIS session.
///
/// The shortcut is named neutrally ("Restore desktop appearance"), but only
/// Plasma can do the restoring itself: it rolls back the Global Theme, the
/// panel layout, SDDM and Plymouth. On another desktop we honestly refuse
/// instead of pretending it worked (rule #13).
///
/// An empty variable does NOT mean "another desktop" but "unknown" (a run
/// from a tty, from a systemd unit, from a terminal without a session):
/// refusing there is not allowed, or the emergency route would stop working
/// in exactly the case it exists for, when the session no longer comes up.
pub fn is_plasma_desktop(xdg_current_desktop: &str) -> bool {
    if xdg_current_desktop.trim().is_empty() {
        return true;
    }
    let lower = xdg_current_desktop.to_lowercase();
    PLASMA_DESKTOP_MARKERS.iter().any(|m| lower.contains(m))
}

/// What to tell the person on another desktop (three lines in a row).
pub fn not_plasma_lines(xdg_current_desktop: &str) -> Vec<String> {
    vec![
        tr("⚠️  Emergency appearance recovery only supports KDE Plasma for now."),
        format!(
            "    {}",
            trf("Current desktop: %1. Nothing was changed.", &[&xdg_current_desktop])
        ),
        format!(
            "    {}",
            tr("On this desktop, roll the appearance back with the desktop's own tools \
                or on the Appearance tab in Rexon.")
        ),
    ]
}

/// Which theme is restored: the one saved before applying, or
/// [`RECOVER_DEFAULT_TARGET`].
///
/// ⚠️ Do not confuse with [`crate::window_menu::recover_target`]: that one
/// answers the DIALOG's question ("what will be applied", where an empty
/// backup is called "Breeze Dark (fallback)"), while this one answers the
/// COMMAND's question, and its answer goes into argv, where a human name
/// will not do.
pub fn recover_theme_target(saved: Option<&str>) -> String {
    match saved.map(str::trim) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => RECOVER_DEFAULT_TARGET.to_string(),
    }
}

/// How long we wait for the Global Theme to apply (Plasma's cache rebuild is not quick).
pub const LOOKANDFEEL_TIMEOUT_SECS: u64 = 30;
/// How long we wait for the colour scheme to apply.
pub const COLORSCHEME_TIMEOUT_SECS: u64 = 15;
/// How long we wait for a single `kwriteconfig` config edit.
pub const KWRITECONFIG_TIMEOUT_SECS: u64 = 10;
/// How long we wait for a full panel layout reset.
pub const PANEL_RESET_TIMEOUT_SECS: u64 = 30;
/// How long we wait for the session bus to answer `reconfigure`.
pub const KWIN_RECONFIGURE_TIMEOUT_SECS: u64 = 10;
/// Timeouts of the plasmashell restart, per command from
/// [`plasmashell_restart_argv`] (resetting the mark is quick, the restart
/// itself is not).
pub const PLASMASHELL_RESTART_TIMEOUTS: &[u64] = &[5, 15];

/// argv that applies the Global Theme.
pub fn lookandfeel_apply_argv(target: &str) -> Vec<String> {
    vec![
        "plasma-apply-lookandfeel".to_string(),
        "-a".to_string(),
        target.to_string(),
    ]
}

/// argv that applies the colour scheme after the theme.
pub fn colorscheme_apply_argv() -> Vec<String> {
    vec![
        "plasma-apply-colorscheme".to_string(),
        RECOVER_COLORSCHEME.to_string(),
    ]
}

/// argv of a full panel layout reset. ⚠️ Erases ALL user panels and
/// widgets; called only on traces of a fork theme.
pub fn panel_reset_layout_argv(tool: &str, target: &str) -> Vec<String> {
    vec![
        tool.to_string(),
        "-a".to_string(),
        target.to_string(),
        "--resetLayout".to_string(),
    ]
}

/// Warning about Plasma fork packages: a heading, the list, and, depending
/// on whether the full rollback command could be built, either the command
/// itself or an explanation of why there is none.
///
/// `preview_ok`/`preview` is the answer of `plasma_fork_reset_preview`.
/// When the target is not in the repositories, we do NOT print the command:
/// the person would run it by hand and get "target not found".
pub fn fork_warning_lines(forks: &[String], preview_ok: bool, preview: &str) -> Vec<String> {
    let mut out = vec![tr(
        "⚠️  REPLACEMENT PACKAGES for Plasma components are installed (they replace \
         parts of Plasma itself, and a theme reset does NOT remove them):",
    )];
    for f in forks {
        out.push(format!("      • {f}"));
    }
    if preview_ok {
        out.push(format!("    {}", tr("To bring back standard Plasma COMPLETELY, remove them:")));
        out.push(format!("      {preview}"));
        out.push(format!(
            "    {}",
            tr("(then log out and log back in). The theme reset below only changes the \
                look — the replaced parts of Plasma stay.")
        ));
    } else {
        for line in preview.lines() {
            out.push(format!("    {line}"));
        }
    }
    out
}

/// "Starting", printed BEFORE the theme is applied.
pub fn recovering_line(target: &str) -> String {
    trf("🛟 Restoring the KDE theme: %1", &[&target])
}

/// "Done", printed right after the theme was applied successfully.
pub fn restored_line(target: &str) -> String {
    trf("✅ Restored: %1", &[&target])
}

/// How many characters of someone else's stderr we show the person.
pub const RECOVER_STDERR_LIMIT: usize = 200;

/// "Failed": the head of someone else's stderr, cut to
/// [`RECOVER_STDERR_LIMIT`].
///
/// The cut is by CHARACTERS, not bytes: slicing in the middle of a
/// multibyte letter (the `plasma-apply-lookandfeel` messages are
/// translated) is a panic in Rust, so the emergency route would crash on
/// exactly the failure it came to explain.
pub fn apply_failed_line(stderr: &str) -> String {
    let head: String = stderr.chars().take(RECOVER_STDERR_LIMIT).collect();
    trf("❌ Error: %1", &[&head])
}

/// Result of the font reset.
pub const FONTS_RESET_LINE: &TrLine = &TrLine::new(n_("🔤 KDE fonts reset to the defaults."));

/// Result of removing the KWin effects.
pub const KWIN_EFFECTS_RESET_LINE: &TrLine =
    &TrLine::new(n_("🪟 KWin effects left by the Plasma replacements removed."));

/// To the log when the running KWin did not reread its config: the edit
/// landed in the file, but nothing changes on screen until the next login,
/// and the person is told so instead of "silently nothing happened".
///
/// The log is not translated (the translation rollout rule).
pub const KWIN_RECONFIGURE_FAILED_LOG: &str =
    "KWin reconfigure не прошёл: шина не ответила, а бинаря qdbus в системе нет. \
     Эффекты снимутся при следующем входе в систему.";

/// Path of the Kvantum config relative to the config root.
pub const KVANTUM_CONFIG_REL: &str = "Kvantum/kvantum.kvconfig";
/// Section and theme key in the Kvantum config.
pub const KVANTUM_THEME_SECTION: &str = "General";
/// Theme key in the Kvantum config.
pub const KVANTUM_THEME_KEY: &str = "theme";

/// Result of returning the widget style from Kvantum-Aero to Breeze.
pub fn kvantum_breeze_line(theme: &str) -> String {
    // TRANSLATORS: arrows ← → point the way they should in your language: in right-to-left languages swap them.
    trf("🎨 Kvantum theme “%1” (Aero) → Breeze style.", &[&theme])
}

/// The native Plymouth default could not be determined.
pub const PLYMOUTH_TARGET_UNKNOWN_LINE: &TrLine = &TrLine::new(n_(
    "⚠️ Couldn't determine the distribution's default Plymouth theme — skipping.",
));

/// The boot splash is already stock: nothing to do.
pub fn plymouth_already_stock_line(target: &str) -> String {
    trf("🐧 The boot splash is already the default one (%1).", &[&target])
}

/// The boot splash is being changed (the initramfs is rebuilt).
pub fn plymouth_switch_line(current: &str, target: &str) -> String {
    trf(
        // TRANSLATORS: arrows ← → point the way they should in your language: in right-to-left languages swap them.
        "🐧 Boot splash: “%1” → “%2” (rebuilding the initramfs; takes effect after a \
         reboot).",
        &[&current, &target],
    )
}

/// Answer of a successful step (Plymouth and SDDM answer the same way).
pub fn step_ok_line(message: &str) -> String {
    format!("   ✅ {message}")
}

/// Answer of a failed Plymouth reset.
pub fn plymouth_failed_line(message: &str) -> String {
    format!("   {}", trf("⚠️ Plymouth wasn't reset: %1", &[&message]))
}

/// How the person can reset the splash themselves if pkexec did not go through.
pub fn manual_command_line(command: &str) -> String {
    format!("   {}", trf("To do it by hand: %1", &[&command]))
}

/// The login screen is already stock. `theme` is the distribution default
/// theme, if it could be named (it matters to the person what they return
/// to).
pub fn sddm_already_stock_line(theme: Option<&str>) -> String {
    match theme {
        Some(t) if !t.is_empty() => trf(
            "🖥️ The login screen (SDDM) already has the standard settings (theme “%1”).",
            &[&t],
        ),
        _ => tr("🖥️ The login screen (SDDM) already has the standard settings."),
    }
}

/// What exactly is removed from `/etc/sddm.conf.d` and what the login screen returns to.
pub fn sddm_removing_line(to_remove: &[String], default_theme: Option<&str>) -> String {
    let files = to_remove.join(", ");
    match default_theme {
        Some(t) if !t.is_empty() => trf(
            // TRANSLATORS: arrows ← → point the way they should in your language: in right-to-left languages swap them.
            "🖥️ Login screen (SDDM): removing third-party settings [%1] → it goes back to “%2”.",
            &[&files, &t],
        ),
        _ => trf(
            // TRANSLATORS: arrows ← → point the way they should in your language: in right-to-left languages swap them.
            "🖥️ Login screen (SDDM): removing third-party settings [%1] → it goes back to \
             the distribution default.",
            &[&files],
        ),
    }
}

/// Answer of a failed login screen reset.
pub fn sddm_failed_line(message: &str) -> String {
    format!("   {}", trf("⚠️ SDDM wasn't reset: %1", &[&message]))
}

/// Forks or their traces were found: moving on to cleaning the panel layout.
pub const PANEL_AGGRESSIVE_LINE: &TrLine = &TrLine::new(n_(
    "🔧 Plasma replacements or their leftovers found — cleaning them off the panel.",
));

/// Result of the panel surgery: what was removed and where the backup lies.
pub fn applets_removed_line(removed: &[String], backup: &str) -> String {
    trf(
        "🧱 Broken widgets left by the Plasma replacements removed from the panel: %1 (%2). \
         Backup: %3",
        &[&removed.len(), &removed.join(", "), &backup],
    )
}

/// The panel could not be rewritten (no permission, out of space).
pub fn applets_rewrite_failed_line(error: &str) -> String {
    trf("⚠️ Couldn't rewrite the panel: %1", &[&error])
}

/// The system has no tool for a full layout reset.
pub const LOOKANDFEELTOOL_MISSING_LINE: &TrLine = &TrLine::new(n_(
    "⚠️ lookandfeeltool not found — the panel layout wasn't reset.",
));

/// Result of the full panel layout reset.
pub const PANEL_LAYOUT_RESET_LINE: &TrLine =
    &TrLine::new(n_("🧱 Panel layout reset to the default one (Kickoff)."));

/// Name of the full panel layout reset tool.
pub const LOOKANDFEELTOOL_BIN: &str = "lookandfeeltool";

/// Canonical name of the KDE config editor (the 6 -> 5 synonyms are known
/// to [`crate::utils::TOOL_SETS`]).
pub const KWRITECONFIG_TOOL: &str = "kwriteconfig6";

/// Where the wallpaper packages installed by the system live.
pub const WALLPAPERS_ROOT: &str = "/usr/share/wallpapers";

/// Subdirectory of a wallpaper package by which the package is recognised.
pub const WALLPAPER_PACKAGE_IMAGES: &str = "contents/images";

/// The wallpaper directory in the form Plasma itself writes it: WITH a
/// trailing slash. Without the slash Plasma treats the value as a path to a
/// single file.
pub fn wallpaper_dir_value(dir: &str) -> String {
    format!("{}/", dir.trim_end_matches('/'))
}

/// Choose the "stock" wallpaper directory: first the known candidates
/// ([`DEFAULT_WALLPAPER_DIRS`]) in their order, then ANY installed
/// wallpaper package alphabetically. Empty means the wallpaper is not
/// rewritten at all; only the plugin is returned to `org.kde.image`.
///
/// `is_dir` probes a candidate directory, `packages` are the names in
/// [`WALLPAPERS_ROOT`] (the caller sets the order; the original sorts),
/// `has_images` tells whether a package has [`WALLPAPER_PACKAGE_IMAGES`].
pub fn default_wallpaper_dir(
    is_dir: &dyn Fn(&str) -> bool,
    packages: &[String],
    has_images: &dyn Fn(&str) -> bool,
) -> String {
    for cand in DEFAULT_WALLPAPER_DIRS {
        if is_dir(cand) {
            return wallpaper_dir_value(cand);
        }
    }
    for name in packages {
        let pkg = format!("{WALLPAPERS_ROOT}/{name}");
        if has_images(&format!("{pkg}/{WALLPAPER_PACKAGE_IMAGES}")) {
            return wallpaper_dir_value(&pkg);
        }
    }
    String::new()
}

/// Result of the wallpaper reset: how many desktops were rewritten and to
/// what.
///
/// Empty `wallpaper_dir`: no name is given. The plugin was returned to the
/// image, but the image itself was not found, and lying about "reset to
/// such and such" is not allowed.
pub fn wallpaper_reset_line(desktops: usize, wallpaper_dir: &str) -> String {
    let name = wallpaper_dir.trim_end_matches('/').rsplit('/').next().unwrap_or("");
    let n = desktops as u64;
    if wallpaper_dir.is_empty() || name.is_empty() {
        ntr(
            "🖼️  Wallpaper reset to the default one (%n desktop)",
            "🖼️  Wallpaper reset to the default one (%n desktops)",
            n,
        )
    } else {
        fill(
            &ntr(
                "🖼️  Wallpaper reset to the default one (%n desktop): %1",
                "🖼️  Wallpaper reset to the default one (%n desktops): %1",
                n,
            ),
            &[&name],
        )
    }
}

/// Nothing to apply the theme with: `plasma-apply-lookandfeel` did not
/// start at all.
///
/// ⭐ The original had no line of this kind: it called `subprocess.run`
/// without a check, and a missing binary raised `FileNotFoundError` right
/// out of `main()`. The emergency route crashed with a stack trace exactly
/// where it must explain to the person what is going on (rule #13).
pub const LOOKANDFEEL_MISSING_LINE: &TrLine = &TrLine::new(n_(
    "❌ Error: couldn't start plasma-apply-lookandfeel — KDE Plasma wasn't found on \
     this system. Nothing was changed.",
));
