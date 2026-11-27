//! Session looks watcher: a GTK theme lacks its own icons and cursor.
//!
//! WHY. The theme is changed not only in Rexon: the MATE "Appearance" window,
//! GNOME, Cinnamon, XFCE settings. A metatheme (`index.theme`) names an icon set
//! (`IconTheme=`) and a cursor (`CursorTheme=`), the desktop writes them — but
//! the sets themselves are not on the system. GTK silently falls back to
//! hicolor/Adwaita, and the person sees "the theme looks somehow wrong" (a live
//! case 17.10.2026: MATE, WhiteSur-Dark-nord asks for WhiteSur-Dark and
//! WhiteSur-cursors, neither is installed).
//!
//! WHAT IT DOES. While Rexon runs, the watcher waits for writes to the files
//! where desktops keep their looks (the dconf database, xfconf, kdeglobals, the
//! GTK `settings.ini` and neighbours — [`watch_targets`]) — via inotify,
//! without polling. After a quiet period ([`DEBOUNCE_MS`]) it reads the current
//! theme, icons and cursor (with the same commands as the active themes map of
//! «Appearance») and, if something is missing on disk, acts in order (decision of
//! Wehrwolfmann 17.10.2026):
//! 1. the set goes into the home directory without a password and without a
//!    build (a ready archive from the KDE store, `themes::dep_home_source`) —
//!    download, unpack, apply silently and say so in one short notification
//!    (setting [`AUTO_HOME_KEY`]; off — a question instead);
//! 2. the set exists only as a system package or in the AUR — a notification
//!    with an «Install» button (the shared installer path
//!    `LiveInstaller::apply_dep_items`, the same as «Apply» in Rexon);
//! 3. the set exists nowhere — an honest notification, «Use standard» (if
//!    those are installed) and «Keep as is».
//!
//! WHAT IT DOES NOT DO.
//! * It doesn't nag: it asks about one theme once — the answer (any, including
//!   a closed notification) is remembered in the Rexon settings ([`ASKED_KEY`])
//!   until the theme changes.
//! * It doesn't ask after «Apply» in Rexon itself: the question was already
//!   asked there, and the installer marks the theme ([`note_rexon_applied`]).
//! * ⛔ It doesn't write into the settings the name of a set that is not on
//!   disk: the "standard" ones are offered only if they are installed themselves.
//! * Without `notify-send` there are no buttons (the notification service sends
//!   the click as a signal, and `dbus-send` doesn't catch it) — then a text with
//!   a hint where to install.
//!
//! Decisions are pure functions below; everything that goes outside is in the
//! "live" part.

use std::collections::HashMap;

use serde_json::{json, Value};

use crate::fs_watch::{Wake, Watch, WatchTarget};
use crate::i18n::{tr, trf};
use crate::themes::{DepItem, DepRole, DepState};

/// Rexon settings key: which theme and which sets have already been asked about.
pub const ASKED_KEY: &str = "theme_deps_asked";
/// Rexon settings key: «Download icons and cursors for the theme automatically».
pub const AUTO_HOME_KEY: &str = "theme_deps_auto_download";
/// Quiet period after the last write, ms: the "Appearance" window writes the
/// theme, icons and cursor as three keys in a row — ask once, about all of them.
pub const DEBOUNCE_MS: i32 = 3000;
/// First look after start, seconds: on session login the notification service
/// and the desktop don't come up at once.
pub const STARTUP_DELAY_SECS: u64 = 20;
/// How often to retry setting up watches on directories that did not exist at
/// start (dconf creates its own on the first write), ms. This is not polling of
/// the settings: it only checks whether the directory appeared.
pub const RETRY_WATCH_MS: i32 = 10 * 60 * 1000;

// ── what the desktop shows now ──────────────────────────────────────────────

/// Theme, icons and cursor from the desktop settings (empty — could not be read).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Looks {
    pub gtk: String,
    pub icons: String,
    pub cursor: String,
}

/// What is missing for one role and what can be done about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Need {
    /// The set and its state: `Installable(package)` or `Missing`.
    pub item: DepItem,
    /// The theme itself asks for it (`index.theme`).
    pub from_theme: bool,
    /// An installed set to switch to instead of it («Use standard»). Only when the
    /// settings hold a name that is missing: if the recorded set is installed, there
    /// is no reason to change it.
    pub fallback: Option<String>,
    /// The set goes into the home directory without a password and without a build
    /// ([`crate::themes::dep_home_source`]).
    pub home: bool,
}

fn same_name(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// What is missing: the theme's requests (`theme_items`, built by the installer —
/// `LiveInstaller::theme_deps_plan`) and the sets recorded in the settings
/// (`setting_items` — the same [`DepItem`] for the current names). `standard` is
/// the standard set of the desktop's role installed on disk.
///
/// At most one item per role:
/// * the settings hold a name that is missing — an item about it (if the theme
///   asks for another set that can be installed — about the theme's set);
///   the fallback is the theme's set if it is installed, otherwise the standard
///   one;
/// * the recorded set is installed, and the theme asks for a missing one — an
///   item about the theme's set, without a fallback (the current one works
///   anyway).
pub fn needs(
    theme_items: &[DepItem],
    setting_items: &[DepItem],
    standard: &dyn Fn(DepRole) -> Option<String>,
) -> Vec<Need> {
    let mut out = Vec::new();
    for role in [DepRole::Icons, DepRole::Cursor] {
        let t = theme_items.iter().find(|i| i.role == role);
        let s = setting_items.iter().find(|i| i.role == role);
        let broken = s.filter(|s| !matches!(s.state, DepState::Installed(_)));
        if let Some(s) = broken {
            let theme_installed = t.and_then(|t| match &t.state {
                DepState::Installed(name) if !same_name(&t.wanted, &s.wanted) => Some(name.clone()),
                _ => None,
            });
            let theme_other = t.filter(|t| {
                !same_name(&t.wanted, &s.wanted) && matches!(t.state, DepState::Installable(_))
            });
            let (item, from_theme) = match theme_other {
                Some(t) if !matches!(s.state, DepState::Installable(_)) => (t.clone(), true),
                _ => (s.clone(), t.is_some_and(|t| same_name(&t.wanted, &s.wanted))),
            };
            let fallback = theme_installed
                .or_else(|| standard(role))
                .filter(|f| !same_name(f, &item.wanted));
            let home = crate::themes::dep_home_source(item.role, &item.wanted).is_some();
            out.push(Need { item, from_theme, fallback, home });
            continue;
        }
        if let Some(t) = t.filter(|t| !matches!(t.state, DepState::Installed(_))) {
            let home = crate::themes::dep_home_source(t.role, &t.wanted).is_some();
            out.push(Need { item: t.clone(), from_theme: true, fallback: None, home });
        }
    }
    out
}

// ── what was already asked ──────────────────────────────────────────────────

/// "What was asked" memory: the theme and set keys ([`need_key`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Asked {
    pub theme: String,
    pub keys: Vec<String>,
}

fn role_key(role: DepRole) -> &'static str {
    match role {
        DepRole::Icons => "icons",
        DepRole::Cursor => "cursor",
    }
}

/// Key of a set in memory: role and name, case-insensitive.
pub fn need_key(role: DepRole, wanted: &str) -> String {
    format!("{}:{}", role_key(role), wanted.to_lowercase())
}

impl Asked {
    /// From the Rexon settings (`None`/garbage — nothing was asked).
    pub fn from_value(v: Option<&Value>) -> Self {
        let Some(v) = v else { return Self::default() };
        Asked {
            theme: v.get("theme").and_then(Value::as_str).unwrap_or("").to_string(),
            keys: v
                .get("asked")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(|k| k.as_str().map(str::to_string)).collect())
                .unwrap_or_default(),
        }
    }

    pub fn to_value(&self) -> Value {
        json!({ "theme": self.theme, "asked": self.keys })
    }

    /// Memory for theme `theme`: the theme changed — previous answers are forgotten.
    pub fn for_theme(self, theme: &str) -> Self {
        // An empty name is a one-off read failure, not a theme change: the memory stays.
        if self.theme == theme || theme.trim().is_empty() {
            self
        } else {
            Asked { theme: theme.to_string(), keys: Vec::new() }
        }
    }

    /// Items not asked about yet.
    pub fn unasked(&self, needs: &[Need]) -> Vec<Need> {
        needs
            .iter()
            .filter(|n| !self.keys.contains(&need_key(n.item.role, &n.item.wanted)))
            .cloned()
            .collect()
    }

    /// Mark that `needs` were asked about.
    pub fn remember(&mut self, needs: &[Need]) {
        for n in needs {
            let k = need_key(n.item.role, &n.item.wanted);
            if !self.keys.contains(&k) {
                self.keys.push(k);
            }
        }
    }
}

// ── notification ────────────────────────────────────────────────────────────

/// The «Install» button.
pub const ACTION_INSTALL: &str = "install";
/// The «Use standard» button.
pub const ACTION_STANDARD: &str = "standard";
/// The «Keep as is» button.
pub const ACTION_KEEP: &str = "keep";

/// Notification: title, text and buttons `(key, caption)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub summary: String,
    pub body: String,
    pub actions: Vec<(String, String)>,
}

/// "the icons “X”" / "the cursor “X”".
pub fn set_label(role: DepRole, name: &str) -> String {
    match role {
        // TRANSLATORS: part of a sentence, %1 is an icon set name.
        DepRole::Icons => trf("icons “%1”", &[&name]),
        // TRANSLATORS: part of a sentence, %1 is a cursor theme name.
        DepRole::Cursor => trf("cursor “%1”", &[&name]),
    }
}

fn join_labels(labels: &[String]) -> String {
    match labels {
        [] => String::new(),
        [one] => one.clone(),
        [first, rest @ ..] => {
            // TRANSLATORS: joins two parts of a list: “icons “X” and cursor “Y””.
            trf("%1 and %2", &[first, &join_labels(rest)])
        }
    }
}

fn labels<'a>(needs: impl Iterator<Item = &'a Need>) -> Vec<String> {
    needs.map(|n| set_label(n.item.role, &n.item.wanted)).collect()
}

/// Title of the watcher's notifications.
pub fn notice_title() -> String {
    tr("Missing icons or cursor")
}

/// Title of the silent download notification.
pub fn notice_title_done() -> String {
    tr("Icons and cursor downloaded")
}

/// Notification about `needs` of theme `theme`. `None` — nothing to say.
///
/// Three groups: what goes into the home directory without a password (items
/// get here only when the silent download is off or failed —
/// [`auto_home_enabled`]), what is installed as a package (password, for the
/// AUR a build), and what exists nowhere.
pub fn notice(theme: &str, needs: &[Need]) -> Option<Notice> {
    if needs.is_empty() {
        return None;
    }
    let home: Vec<&Need> = needs.iter().filter(|n| n.home).collect();
    let package: Vec<&Need> = needs
        .iter()
        .filter(|n| !n.home && matches!(n.item.state, DepState::Installable(_)))
        .collect();
    let missing: Vec<&Need> =
        needs.iter().filter(|n| !n.home && matches!(n.item.state, DepState::Missing)).collect();
    let of_theme = |n: &&&Need| n.from_theme && !theme.is_empty();
    let mut parts: Vec<String> = Vec::new();
    let ask = |group: &[&Need], parts: &mut Vec<String>| {
        let by_theme = labels(group.iter().filter(of_theme).copied());
        if !by_theme.is_empty() {
            // TRANSLATORS: %1 is a GTK theme name, %2 is e.g. “icons “X” and cursor “Y””.
            parts.push(trf("The theme “%1” needs %2 — install?", &[&theme, &join_labels(&by_theme)]));
        }
        let by_setting = labels(group.iter().filter(|n| !of_theme(n)).copied());
        if !by_setting.is_empty() {
            parts.push(trf(
                // TRANSLATORS: %1 is e.g. “icons “X” and cursor “Y””.
                "The desktop settings use %1, which isn't on this computer — install?",
                &[&join_labels(&by_setting)],
            ));
        }
    };
    ask(&home, &mut parts);
    if !home.is_empty() {
        parts.push(tr("It is downloaded into your home folder, no password needed."));
    }
    ask(&package, &mut parts);
    let mut packages: Vec<String> = Vec::new();
    for n in &package {
        if let DepState::Installable(p) = &n.item.state {
            if !packages.contains(p) {
                packages.push(p.clone());
            }
        }
    }
    if !packages.is_empty() {
        parts.push(trf(
            "Packages: %1 (an administrator password may be requested).",
            &[&packages.join(", ")],
        ));
    }
    if !missing.is_empty() {
        parts.push(trf(
            // TRANSLATORS: %1 is e.g. “icons “X” and cursor “Y””.
            "Not found in the repositories Rexon knows: %1.",
            &[&join_labels(&labels(missing.iter().copied()))],
        ));
    }
    let fallbacks: Vec<String> =
        needs.iter().filter_map(|n| n.fallback.as_ref().map(|f| set_label(n.item.role, f))).collect();
    if !fallbacks.is_empty() {
        parts.push(trf(
            // TRANSLATORS: %1 is e.g. “icons “Menta” and cursor “mate-black””.
            "You can switch to the standard ones (%1) or keep the current ones.",
            &[&join_labels(&fallbacks)],
        ));
    } else if !missing.is_empty() {
        parts.push(tr("Your current icons and cursor stay."));
    }
    let mut actions = Vec::new();
    if !home.is_empty() || !package.is_empty() {
        actions.push((ACTION_INSTALL.to_string(), tr("Install")));
    }
    if !fallbacks.is_empty() {
        actions.push((ACTION_STANDARD.to_string(), tr("Use standard")));
    }
    actions.push((ACTION_KEEP.to_string(), tr("Keep as is")));
    Some(Notice { summary: notice_title(), body: parts.join("\n"), actions })
}

/// Result of the silent download: "For the theme “X” the icons “A” and the cursor “B” were downloaded."
pub fn downloaded_text(theme: &str, done: &[(DepRole, String)]) -> String {
    let list = join_labels(&done.iter().map(|(r, n)| set_label(*r, n)).collect::<Vec<_>>());
    if theme.is_empty() {
        // TRANSLATORS: %1 is e.g. “icons “X” and cursor “Y””.
        trf("Downloaded and applied %1.", &[&list])
    } else {
        // TRANSLATORS: %1 is a GTK theme name, %2 is e.g. “icons “X” and cursor “Y””.
        trf("Downloaded %2 for the theme “%1”.", &[&theme, &list])
    }
}

/// Hint in a notification without buttons (no `notify-send`).
pub fn no_buttons_hint() -> String {
    tr("To install it, press “Apply” on this theme in Rexon's “Appearance” tab.")
}

/// argv of a `notify-send` that waits for an answer: the clicked button arrives
/// in stdout as its key ([`answer_from_stdout`]). Important — doesn't expire on
/// its own.
pub fn notify_argv(exe: &str, notice: &Notice) -> Vec<String> {
    crate::utils::notify_ask_argv(exe, &notice.summary, &notice.body, &notice.actions)
}

/// The clicked button from the `notify-send --wait` output. `None` — closed
/// without an answer (that is an answer too: "not now", we won't ask again).
pub fn answer_from_stdout(stdout: &str) -> Option<&'static str> {
    let last = stdout.lines().map(str::trim).rfind(|l| !l.is_empty())?;
    [ACTION_INSTALL, ACTION_STANDARD, ACTION_KEEP].into_iter().find(|a| *a == last)
}

/// Result after «Use standard»: a line per set.
pub fn standard_line(role: DepRole, name: &str, ok: bool) -> String {
    if ok {
        trf("Switched to %1.", &[&set_label(role, name)])
    } else {
        trf("⚠️ Couldn't switch to %1.", &[&set_label(role, name)])
    }
}

// ── what to watch ───────────────────────────────────────────────────────────

/// Which kernel events mean "the looks may have changed": a write and a file
/// appearing (including `rename` over it — this is how the dconf database and
/// `settings.ini` are rewritten by replacement). The watch is shared
/// (`fs_watch`), on DIRECTORIES: a watch on the file itself goes deaf after the
/// very first replacement.
const WATCH_EVENTS: u32 = libc::IN_CLOSE_WRITE | libc::IN_MOVED_TO | libc::IN_CREATE;

/// Where to look in all desktops at once (an extra watch costs nothing, and the
/// desktop doesn't change mid-session): the dconf database (GNOME, MATE,
/// Cinnamon, Budgie, COSMIC — `gsettings`), its `keyfile` replacement, xfconf
/// (XFCE), kdeglobals and kcminputrc (KDE), lxqt.conf (LXQt), the GTK
/// `settings.ini` and the default cursor in `~/.icons/default` (tiling WMs).
pub fn watch_targets(home: &str, config_home: &str) -> Vec<WatchTarget> {
    let c = config_home.trim_end_matches('/');
    let h = home.trim_end_matches('/');
    let t = |dir: String, names: &[&str]| WatchTarget { dir, names: names.iter().map(|n| n.to_string()).collect() };
    vec![
        t(c.to_string(), &["kdeglobals", "kcminputrc"]),
        t(format!("{c}/dconf"), &["user"]),
        t(format!("{c}/glib-2.0/settings"), &["keyfile"]),
        t(format!("{c}/gtk-3.0"), &["settings.ini"]),
        t(format!("{c}/gtk-4.0"), &["settings.ini"]),
        t(format!("{c}/xfce4/xfconf/xfce-perchannel-xml"), &["xsettings.xml"]),
        t(format!("{c}/lxqt"), &["lxqt.conf"]),
        t(format!("{h}/.icons/default"), &["index.theme"]),
    ]
}

// ════════════════════════════════════════════════════════════════════════════
//  Live part
// ════════════════════════════════════════════════════════════════════════════

/// One memory writer at a time: the watcher and «Apply» live in one process.
fn asked_lock() -> &'static std::sync::Mutex<()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    &LOCK
}

/// Whether to download icons and cursors for the theme ourselves (the Rexon
/// setting [`AUTO_HOME_KEY`], default — yes). Off — instead of a silent
/// download, a notification with an «Install» button.
pub fn auto_home_enabled() -> bool {
    crate::settings::SettingsManager::new()
        .ok()
        .and_then(|m| m.get(AUTO_HOME_KEY).and_then(Value::as_bool))
        .unwrap_or(true)
}

/// The remembered answer to the question about the icons of theme `theme`
/// (`themes::THEME_ICONS_CHOICE_KEY`); `None` — not asked.
pub fn live_icons_choice(theme: &str) -> Option<bool> {
    let m = crate::settings::SettingsManager::new().ok()?;
    crate::themes::icons_choice_of(m.get(crate::themes::THEME_ICONS_CHOICE_KEY), theme)
}

/// Remember the answer to the question about the icons of theme `theme`.
pub fn remember_icons_choice(theme: &str, yes: bool) {
    if theme.trim().is_empty() {
        return;
    }
    let _guard = asked_lock().lock();
    match crate::settings::SettingsManager::new() {
        Ok(mut m) => {
            let value = crate::themes::icons_choice_with(m.get(crate::themes::THEME_ICONS_CHOICE_KEY), theme, yes);
            if let Err(e) = m.set(crate::themes::THEME_ICONS_CHOICE_KEY, value) {
                crate::log_warn!("theme", "ответ о значках темы не записан: {e}");
            }
        }
        Err(e) => crate::log_warn!("theme", "настройки не открылись: {e}"),
    }
}

/// The item is the icons the theme itself asks for (they get a question with
/// memory, decision 3a), not a set from the desktop settings.
fn is_theme_icons(n: &Need) -> bool {
    n.from_theme && n.item.role == DepRole::Icons
}

/// Take into account the remembered answer about the theme icons: «Keep mine» —
/// don't ask about them at all; no answer — don't download silently, ask with a
/// button. Returns `(what to ask about, whether the theme icons may be
/// downloaded silently)`.
pub fn apply_icons_choice(ask: Vec<Need>, choice: Option<bool>) -> (Vec<Need>, bool) {
    let ask = ask.into_iter().filter(|n| !(is_theme_icons(n) && choice == Some(false))).collect();
    (ask, choice == Some(true))
}

fn load_asked() -> Asked {
    crate::settings::SettingsManager::new()
        .map(|m| Asked::from_value(m.get(ASKED_KEY)))
        .unwrap_or_default()
}

fn save_asked(asked: &Asked) {
    match crate::settings::SettingsManager::new() {
        Ok(mut m) => {
            if let Err(e) = m.set(ASKED_KEY, asked.to_value()) {
                crate::log_warn!("theme", "сторож оформления: память ответов не записана: {e}");
            }
        }
        Err(e) => crate::log_warn!("theme", "сторож оформления: настройки не открылись: {e}"),
    }
}

fn run_stdout(argv: &[String], timeout: Option<u64>) -> Option<String> {
    let path = std::env::var("PATH").ok();
    let report = crate::utils::run(
        &crate::utils::RunSpec {
            argv: argv.to_vec(),
            output: crate::utils::RunOutput::Capture,
            timeout_secs: timeout,
            ..crate::utils::RunSpec::default()
        },
        path.as_deref(),
    );
    (report.returncode == Some(0)).then(|| crate::utils::decode_text(&report.stdout))
}

fn live_de() -> String {
    let env = |n: &str| std::env::var(n).unwrap_or_default();
    crate::system::detect_current_de(
        &env("XDG_CURRENT_DESKTOP"),
        &env("DESKTOP_SESSION"),
        !env("WAYLAND_DISPLAY").is_empty(),
    )
    .unwrap_or_else(|| "kde".to_string())
}

/// Theme, icons and cursor of desktop `de`: the native storage (gsettings,
/// xfconf, DConfig) — with the same commands as the active themes map; what is
/// not there (KDE, LXQt, tiling WMs) — from the map itself.
pub fn live_looks(de: &str) -> Looks {
    let [gtk, icons, cursor] =
        crate::installed_scan::native_role_values(de, |c| run_stdout(&c.argv, c.timeout));
    let mut looks = Looks {
        gtk: gtk.unwrap_or_default(),
        icons: icons.unwrap_or_default(),
        cursor: cursor.unwrap_or_default(),
    };
    if looks.gtk.is_empty() || looks.icons.is_empty() || looks.cursor.is_empty() {
        let map = crate::installed_scan::LiveScan::new(Vec::new(), HashMap::new()).active_map();
        let fill = |slot: &mut String, key: &str| {
            if slot.is_empty() {
                *slot = map.get(key).cloned().unwrap_or_default();
            }
        };
        fill(&mut looks.gtk, "gtk");
        fill(&mut looks.icons, "icons");
        fill(&mut looks.cursor, "cursor");
    }
    looks
}

/// Setting values that do not count as a request for a set (as with a
/// metatheme): the fallback set and the link to the chosen cursor.
fn is_real_set_name(name: &str) -> bool {
    !name.is_empty() && !["hicolor", "default"].iter().any(|n| n.eq_ignore_ascii_case(name))
}

/// The settings hold a set that is installed under a DIFFERENT spelling
/// (`WhiteSur-Dark`, while on disk it is `WhiteSur-dark`; another name of the
/// same set): GTK looks for the directory by the exact name and silently falls
/// back. Then — the name from disk; `None` — nothing to change.
pub fn spelling_fix(setting: &str, on_disk: Option<&str>) -> Option<String> {
    on_disk.filter(|d| *d != setting).map(str::to_string)
}

/// Fix the spelling of sets in the settings ([`spelling_fix`]). `true` —
/// something was written (the desktop will send a write, and the watcher will
/// look again).
fn live_fix_spelling(de: &str, looks: &Looks) -> bool {
    let home = std::env::var("HOME").unwrap_or_default();
    let data = crate::utils::data_home(std::env::var("XDG_DATA_HOME").ok().as_deref().filter(|v| !v.is_empty()), &home);
    let roots = crate::themes::dep_search_roots(&home, &data, &std::env::var("XDG_DATA_DIRS").unwrap_or_default());
    let mut fixed = false;
    for (role, name) in [(DepRole::Icons, &looks.icons), (DepRole::Cursor, &looks.cursor)] {
        if !is_real_set_name(name) {
            continue;
        }
        let found = crate::appearance_installer::live_find_dep(role, name, &roots);
        let Some(right) = spelling_fix(name, found.as_deref()) else { continue };
        let log = Box::new(|line: &str| crate::log_info!("theme", "{line}"));
        let mut installer = crate::appearance_installer::LiveInstaller::new(de, log);
        let ok = installer.apply_installed_set(role, &right);
        crate::log_info!("theme", "сторож оформления: «{name}» на диске зовётся «{right}» — {}", if ok { "исправлено" } else { "исправить не вышло" });
        fixed |= ok;
    }
    fixed
}

/// What is missing in a session with `looks` — on the live disk.
fn live_needs(installer: &crate::appearance_installer::LiveInstaller, looks: &Looks) -> Vec<Need> {
    let home = std::env::var("HOME").unwrap_or_default();
    let data = crate::utils::data_home(std::env::var("XDG_DATA_HOME").ok().as_deref().filter(|v| !v.is_empty()), &home);
    let roots = crate::themes::dep_search_roots(&home, &data, &std::env::var("XDG_DATA_DIRS").unwrap_or_default());
    let family = crate::appearance_installer::live_deps_family();
    let find = |role: DepRole, name: &str| crate::appearance_installer::live_find_dep(role, name, &roots);
    let theme_items = if looks.gtk.is_empty() { Vec::new() } else { installer.theme_deps_plan(&looks.gtk) };
    let mut setting_items = Vec::new();
    for (role, name) in [(DepRole::Icons, &looks.icons), (DepRole::Cursor, &looks.cursor)] {
        if !is_real_set_name(name) {
            continue;
        }
        let deps = match role {
            DepRole::Icons => crate::themes::ThemeDeps { icons: Some(name.clone()), cursor: None },
            DepRole::Cursor => crate::themes::ThemeDeps { icons: None, cursor: Some(name.clone()) },
        };
        setting_items.extend(crate::themes::plan_theme_deps(&deps, &family, &|r, w| find(r, w)));
    }
    let de = installer.de_id().to_string();
    let standard = |role: DepRole| {
        let name = crate::appearance_installer::live_standard_theme(&de, role_key(role))?;
        find(role, &name)
    };
    needs(&theme_items, &setting_items, &standard)
}

/// «Apply» of a GTK theme in Rexon itself: the question about what is missing is
/// asked there, and the watcher doesn't ask a second time about the same theme
/// and the same sets.
pub fn note_rexon_applied(installer: &crate::appearance_installer::LiveInstaller, theme: &str) {
    let mut looks = live_looks(installer.de_id());
    looks.gtk = theme.to_string();
    let found = live_needs(installer, &looks);
    let _guard = asked_lock().lock();
    let mut asked = load_asked().for_theme(theme);
    asked.remember(&found);
    save_asked(&asked);
}

/// Show the notification and wait for the answer. `None` — closed without an
/// answer or there are no buttons at all.
fn live_ask(notice: &Notice) -> Option<&'static str> {
    // Wait for an answer even for a day: the notification is important and doesn't
    // expire on its own. No buttons (no `notify-send` or it is old) — a hint on
    // what to do yourself.
    let key = crate::utils::notify_ask_live(&notice.summary, &notice.body, &notice.actions, &no_buttons_hint(), 24 * 3600)?;
    answer_from_stdout(&key)
}

/// Carry out the person's answer for `asked_needs` of theme `theme`.
fn live_act(answer: &str, theme: &str, asked_needs: &[Need]) {
    let now = live_looks(&live_de());
    if now.gtk != theme {
        crate::log_info!("theme", "сторож оформления: тема уже другая («{}»), ответ «{answer}» не исполняется", now.gtk);
        return;
    }
    let log = Box::new(|line: &str| crate::log_info!("theme", "{line}"));
    let mut installer = crate::appearance_installer::LiveInstaller::new("", log);
    let lines: Vec<String> = match answer {
        ACTION_INSTALL => {
            let items: Vec<DepItem> = asked_needs
                .iter()
                .filter(|n| n.home || matches!(n.item.state, DepState::Installable(_)))
                .map(|n| n.item.clone())
                .collect();
            installer.allow_deps_install(true);
            if asked_needs.iter().any(is_theme_icons) {
                remember_icons_choice(theme, true);
            }
            installer.set_icons_choice(Some(true));
            installer.apply_dep_items(&items)
        }
        ACTION_KEEP => {
            if asked_needs.iter().any(is_theme_icons) {
                remember_icons_choice(theme, false);
            }
            Vec::new()
        }
        ACTION_STANDARD => asked_needs
            .iter()
            .filter_map(|n| {
                let name = n.fallback.as_ref()?;
                let ok = installer.apply_installed_set(n.item.role, name);
                Some(standard_line(n.item.role, name, ok))
            })
            .collect(),
        _ => Vec::new(),
    };
    if lines.is_empty() {
        return;
    }
    for line in &lines {
        crate::log_info!("theme", "сторож оформления: {line}");
    }
    crate::utils::notify_live(&notice_title(), &lines.join("\n"));
}

/// Result of one item of the silent download.
#[derive(Debug)]
enum Silent {
    /// Downloaded and applied — goes into the short notification.
    Done(DepRole, String),
    /// Downloaded but not applied: it is installed — nothing to ask about it.
    Settled,
    /// Not downloaded (network, failure) — stays a "download into the home
    /// directory" question, with an «Install» button to retry, not "exists nowhere".
    Ask(Need),
}

/// `result` — `install_home_set` + whether applying succeeded.
fn silent_step(n: Need, result: Result<(String, bool), String>) -> Silent {
    match result {
        Ok((name, true)) => Silent::Done(n.item.role, name),
        Ok((name, false)) => {
            crate::log_warn!("theme", "сторож оформления: «{name}» докачан, но не применился");
            Silent::Settled
        }
        Err(_) => Silent::Ask(n),
    }
}

/// Silent download ([`auto_home_enabled`]): what goes into the home directory
/// without a password — download, unpack and apply without a question, and say
/// what was done in one short notification. Returns what still has to be asked
/// about: packages, what exists nowhere, and what failed to download (it goes on
/// as a regular item — as a package or "exists nowhere").
fn live_silent_downloads(de: &str, theme: &str, ask: Vec<Need>) -> Vec<Need> {
    let (ask, icons_silent) = apply_icons_choice(ask, live_icons_choice(theme));
    if !ask.iter().any(|n| n.home) || !auto_home_enabled() {
        return ask;
    }
    let log = Box::new(|line: &str| crate::log_info!("theme", "{line}"));
    let mut installer = crate::appearance_installer::LiveInstaller::new(de, log);
    let mut rest = Vec::new();
    let mut done: Vec<(DepRole, String)> = Vec::new();
    for n in ask {
        if !n.home || (is_theme_icons(&n) && !icons_silent) {
            rest.push(n);
            continue;
        }
        let result = installer
            .install_home_set(n.item.role, &n.item.wanted)
            .map(|name| {
                let applied = installer.apply_installed_set(n.item.role, &name);
                (name, applied)
            });
        if let Err(e) = &result {
            crate::log_warn!("theme", "сторож оформления: «{}» не докачан: {e}", n.item.wanted);
        }
        match silent_step(n, result) {
            Silent::Done(role, name) => done.push((role, name)),
            Silent::Settled => {}
            Silent::Ask(n) => rest.push(n),
        }
    }
    if !done.is_empty() {
        let text = downloaded_text(theme, &done);
        crate::log_info!("theme", "сторож оформления: {text}");
        crate::utils::notify_live(&notice_title_done(), &text);
    }
    rest
}

/// One look at the session: ask if there is something to ask and it wasn't asked yet.
fn live_check(last: &mut Option<Looks>) {
    let de = live_de();
    let looks = live_looks(&de);
    if last.as_ref() == Some(&looks) {
        return;
    }
    // The theme could not be read (the session hasn't answered yet) — nothing to
    // judge, and such a read doesn't go into `last`, so that the next one isn't
    // taken for a change.
    if looks.gtk.trim().is_empty() {
        return;
    }
    *last = Some(looks.clone());
    if live_fix_spelling(&de, &looks) {
        return;
    }
    let installer = crate::appearance_installer::LiveInstaller::new(&de, Box::new(|_: &str| {}));
    let found = live_needs(&installer, &looks);
    let ask = {
        let _guard = asked_lock().lock();
        let before = load_asked();
        let mut asked = before.clone().for_theme(&looks.gtk);
        let ask = asked.unasked(&found);
        // Remember BEFORE showing: should Rexon crash while the notification is up, we
        // won't ask about the same thing again on the next start.
        asked.remember(&ask);
        if asked != before {
            save_asked(&asked);
        }
        ask
    };
    let ask = live_silent_downloads(&de, &looks.gtk, ask);
    let Some(notice) = notice(&looks.gtk, &ask) else { return };
    crate::log_info!("theme", "сторож оформления: тема «{}», не хватает: {:?}", looks.gtk, ask);
    if let Some(answer) = live_ask(&notice) {
        crate::log_info!("theme", "сторож оформления: ответ «{answer}»");
        live_act(answer, &looks.gtk, &ask);
    }
}

/// The watcher thread: lives as long as Rexon.
pub fn run_forever() {
    watch_loop(STARTUP_DELAY_SECS, RETRY_WATCH_MS);
}

/// The watcher loop: the first look after `delay_secs`, then on writes to the
/// looks files; without writes, every `idle_ms` check whether directories
/// without a watch have appeared.
fn watch_loop(delay_secs: u64, idle_ms: i32) {
    std::thread::sleep(std::time::Duration::from_secs(delay_secs));
    let home = std::env::var("HOME").unwrap_or_default();
    let config = crate::utils::config_home(std::env::var("XDG_CONFIG_HOME").ok().as_deref().filter(|v| !v.is_empty()), &home);
    let Some(mut watch) = Watch::with_events(watch_targets(&home, &config), WATCH_EVENTS) else {
        crate::log_warn!("theme", "сторож оформления не запущен: inotify недоступен");
        return;
    };
    let mut last: Option<Looks> = None;
    live_check(&mut last);
    let mut deadline: Option<std::time::Instant> = None;
    loop {
        let timeout = match deadline {
            Some(d) => d.saturating_duration_since(std::time::Instant::now()).as_millis().min(i32::MAX as u128) as i32,
            None => idle_ms,
        };
        let wake = watch.wait_wake(timeout);
        let (next, act) = loop_step(deadline, wake, std::time::Instant::now());
        deadline = next;
        match act {
            LoopDo::Wait => {}
            LoopDo::Check => live_check(&mut last),
            LoopDo::Rescan => watch.rescan(),
        }
    }
}

/// What the watcher loop should do after waking up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LoopDo {
    Wait,
    Check,
    Rescan,
}

/// A loop step: `deadline` is the end of the quiet pause after a write (none —
/// wait for writes). A write to a looks file extends the pause; an unrelated
/// event doesn't cut it short — previously the check went off at once, on a
/// half-written theme.
fn loop_step(
    deadline: Option<std::time::Instant>,
    wake: Wake,
    now: std::time::Instant,
) -> (Option<std::time::Instant>, LoopDo) {
    match (wake, deadline) {
        (Wake::Relevant, _) => (Some(now + std::time::Duration::from_millis(DEBOUNCE_MS as u64)), LoopDo::Wait),
        (_, Some(d)) if now >= d => (None, LoopDo::Check),
        (_, Some(d)) => (Some(d), LoopDo::Wait),
        (Wake::Other, None) => (None, LoopDo::Wait),
        (Wake::Timeout, None) => (None, LoopDo::Rescan),
    }
}

/// Start the watcher once per process.
pub fn spawn_once() {
    static STARTED: std::sync::Once = std::sync::Once::new();
    STARTED.call_once(|| {
        if let Err(e) = std::thread::Builder::new().name("theme-watch".into()).spawn(run_forever) {
            crate::log_warn!("theme", "сторож оформления не запущен: {e}");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(role: DepRole, wanted: &str, state: DepState) -> DepItem {
        DepItem { role, wanted: wanted.to_string(), state }
    }
    fn pkg(p: &str) -> DepState {
        DepState::Installable(p.to_string())
    }
    fn installed(n: &str) -> DepState {
        DepState::Installed(n.to_string())
    }
    fn no_standard(_: DepRole) -> Option<String> {
        None
    }

    /// The session watcher and theme icons (decision 3a): «Keep mine» — don't ask
    /// about them; no answer — don't download silently; «Download and turn on» —
    /// may be done silently. A set from the desktop settings (not the theme's) is
    /// not affected by the answer.
    #[test]
    fn theme_icons_follow_the_remembered_answer() {
        let n = |from_theme: bool, role: DepRole| Need {
            item: item(role, "Windows XP", pkg("xp")),
            from_theme,
            fallback: None,
            home: true,
        };
        let all = || vec![n(true, DepRole::Icons), n(false, DepRole::Icons), n(true, DepRole::Cursor)];
        let (ask, silent) = apply_icons_choice(all(), Some(false));
        assert_eq!(ask.len(), 2);
        assert!(!ask.iter().any(|x| x.from_theme && x.item.role == DepRole::Icons));
        assert!(!silent);
        let (ask, silent) = apply_icons_choice(all(), None);
        assert_eq!(ask.len(), 3);
        assert!(!silent);
        assert!(apply_icons_choice(all(), Some(true)).1);
    }

    /// «Keep mine»: the theme icons are neither downloaded nor turned on, the result
    /// says the person's own ones stayed.
    #[test]
    fn keep_mine_leaves_icons_untouched() {
        let mut installer = crate::appearance_installer::LiveInstaller::new("kde", Box::new(|_: &str| {}));
        installer.set_icons_choice(Some(false));
        let lines = installer.apply_dep_items(&[item(DepRole::Icons, "Windows XP", installed("Windows XP"))]);
        assert_eq!(lines, [crate::themes::dep_icons_kept_line("Windows XP")]);
        // Undecided — also not touched, but nothing is said either.
        installer.set_icons_choice(None);
        assert!(installer.apply_dep_items(&[item(DepRole::Icons, "Windows XP", installed("Windows XP"))]).is_empty());
    }

    /// A one-off empty theme read (gsettings didn't answer at login) doesn't erase
    /// the answers memory: otherwise the same sets would be asked about again.
    #[test]
    fn empty_theme_reading_keeps_the_answers() {
        let a = Asked { theme: "X".into(), keys: vec!["icons:y".into()] };
        assert_eq!(a.clone().for_theme(""), a);
        assert!(a.for_theme("Z").keys.is_empty(), "смена темы — память с нуля");
    }

    /// The silent download failed (the network isn't up yet) — the item stays
    /// "download into the home directory" with an «Install» button, not "exists
    /// nowhere" without a button.
    #[test]
    fn failed_silent_download_still_offers_install() {
        let n = Need {
            item: item(DepRole::Icons, "WhiteSur-Dark", DepState::Missing),
            from_theme: true,
            fallback: None,
            home: true,
        };
        let Silent::Ask(left) = silent_step(n, Err("network".into())) else {
            panic!("неудача докачки должна остаться вопросом");
        };
        let note = notice("WhiteSur-Dark", &[left]).expect("есть о чём сказать");
        assert!(note.actions.iter().any(|(id, _)| id == ACTION_INSTALL), "{note:?}");
        assert!(!note.body.contains("Not found"), "{}", note.body);
        // Downloaded and applied / downloaded but not applied.
        let n2 = Need { item: item(DepRole::Cursor, "c", DepState::Missing), from_theme: true, fallback: None, home: true };
        assert!(matches!(silent_step(n2.clone(), Ok(("c".into(), true))), Silent::Done(DepRole::Cursor, _)));
        assert!(matches!(silent_step(n2, Ok(("c".into(), false))), Silent::Settled));
    }

    /// An unrelated write into a watched directory doesn't cut the quiet pause short:
    /// no check until it ends.
    #[test]
    fn unrelated_write_does_not_cut_the_pause_short() {
        let now = std::time::Instant::now();
        let later = now + std::time::Duration::from_millis(DEBOUNCE_MS as u64);
        assert_eq!(loop_step(Some(later), Wake::Other, now), (Some(later), LoopDo::Wait));
        assert_eq!(loop_step(Some(now), Wake::Timeout, now), (None, LoopDo::Check));
        assert_eq!(loop_step(None, Wake::Timeout, now), (None, LoopDo::Rescan));
        let (d, act) = loop_step(None, Wake::Relevant, now);
        assert!(d.is_some() && act == LoopDo::Wait);
        // The pause is over, and unrelated writes keep coming — check anyway.
        assert_eq!(loop_step(Some(now), Wake::Other, now), (None, LoopDo::Check));
    }

    /// The owner's live case: MATE, the theme asks for WhiteSur-Dark and
    /// WhiteSur-cursors, the desktop wrote the same names, neither is installed.
    #[test]
    fn whitesur_case_asks_once_about_both() {
        let theme = [
            item(DepRole::Icons, "WhiteSur-Dark", pkg("whitesur-icon-theme")),
            item(DepRole::Cursor, "WhiteSur-cursors", pkg("whitesur-cursor-theme-git")),
        ];
        let setting = theme.clone();
        let std_ = |r: DepRole| match r {
            DepRole::Icons => Some("menta".to_string()),
            DepRole::Cursor => Some("mate-black".to_string()),
        };
        let n = needs(&theme, &setting, &std_);
        assert_eq!(n.len(), 2);
        assert!(n.iter().all(|n| n.from_theme));
        assert_eq!(n[0].fallback.as_deref(), Some("menta"));
        let note = notice("WhiteSur-Dark-nord", &n).unwrap();
        let keys: Vec<&str> = note.actions.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, [ACTION_INSTALL, ACTION_STANDARD, ACTION_KEEP]);
        assert!(note.body.contains("WhiteSur-Dark-nord"), "{}", note.body);
        // Both sets go into home without a password: packages are out of the question.
        assert!(n.iter().all(|n| n.home));
        assert!(!note.body.contains("whitesur-icon-theme"), "{}", note.body);

        let mut asked = Asked::default().for_theme("WhiteSur-Dark-nord");
        assert_eq!(asked.unasked(&n).len(), 2);
        asked.remember(&n);
        assert!(asked.unasked(&n).is_empty(), "второй раз о той же теме не спрашиваем");
        // The memory survives the settings.
        let back = Asked::from_value(Some(&asked.to_value())).for_theme("WhiteSur-Dark-nord");
        assert!(back.unasked(&n).is_empty());
        // The theme changed — the answer is forgotten.
        assert_eq!(back.for_theme("Adwaita").unasked(&n).len(), 2);
    }

    /// The set exists only as a package (password, for the AUR a build): a
    /// notification with a button and the package list, not a word about the home
    /// directory.
    #[test]
    fn package_only_set_is_asked_with_packages() {
        let theme = [item(DepRole::Icons, "Papirus-Dark", pkg("papirus-icon-theme"))];
        let n = needs(&theme, &[], &no_standard);
        assert_eq!(n.len(), 1);
        assert!(!n[0].home);
        let note = notice("Arc-Dark", &n).unwrap();
        assert!(note.body.contains("papirus-icon-theme"), "{}", note.body);
        let keys: Vec<&str> = note.actions.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, [ACTION_INSTALL, ACTION_KEEP]);
    }

    /// Home sources of WhiteSur are in the table, the others have none.
    #[test]
    fn home_sources_of_known_sets() {
        use crate::themes::{dep_home_source, HomeSource};
        assert!(matches!(
            dep_home_source(DepRole::Icons, "WhiteSur-Dark"),
            Some(HomeSource::Ocs { content_id: "1405756", .. })
        ));
        assert!(matches!(
            dep_home_source(DepRole::Cursor, "WhiteSur-cursors"),
            Some(HomeSource::Ocs { content_id: "1411743", .. })
        ));
        assert_eq!(dep_home_source(DepRole::Icons, "Papirus"), None);
        assert_eq!(dep_home_source(DepRole::Icons, "Unknown-Set"), None);
        let done = [(DepRole::Icons, "WhiteSur-dark".to_string()), (DepRole::Cursor, "WhiteSur-Cursors".to_string())];
        let text = downloaded_text("WhiteSur-Dark-nord", &done);
        assert!(text.contains("WhiteSur-Dark-nord") && text.contains("WhiteSur-Cursors"), "{text}");
    }

    #[test]
    fn nothing_to_ask_when_all_installed() {
        let theme = [item(DepRole::Icons, "Papirus", installed("Papirus"))];
        let setting = [item(DepRole::Icons, "Papirus", installed("Papirus"))];
        assert!(needs(&theme, &setting, &no_standard).is_empty());
        assert!(notice("Arc", &[]).is_none());
    }

    /// The person chose working icons themselves, the theme asks for missing ones:
    /// ask about the theme's set, but don't offer "standard" — the current ones are
    /// alive anyway.
    #[test]
    fn working_setting_gets_no_fallback() {
        let theme = [item(DepRole::Icons, "WhiteSur-Dark", pkg("whitesur-icon-theme"))];
        let setting = [item(DepRole::Icons, "Papirus", installed("Papirus"))];
        let n = needs(&theme, &setting, &|_| Some("menta".into()));
        assert_eq!(n.len(), 1);
        assert_eq!(n[0].fallback, None);
        let note = notice("WhiteSur-Dark-nord", &n).unwrap();
        assert!(!note.actions.iter().any(|(k, _)| k == ACTION_STANDARD));
    }

    /// The set exists nowhere: say so honestly and offer the standard one only if it
    /// is installed; without a standard one — only «Keep as is».
    #[test]
    fn missing_everywhere_offers_standard_or_keep() {
        let setting = [item(DepRole::Cursor, "Foo-cursors", DepState::Missing)];
        let n = needs(&[], &setting, &|_| Some("Adwaita".into()));
        assert_eq!(n[0].fallback.as_deref(), Some("Adwaita"));
        let note = notice("", &n).unwrap();
        let keys: Vec<&str> = note.actions.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, [ACTION_STANDARD, ACTION_KEEP]);
        assert!(note.body.contains("Foo-cursors"));

        let n = needs(&[], &setting, &no_standard);
        let note = notice("", &n).unwrap();
        let keys: Vec<&str> = note.actions.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, [ACTION_KEEP]);
    }

    /// The settings hold a broken name, and the theme's own set is installed — the
    /// fallback is exactly that one, not the desktop's standard.
    #[test]
    fn theme_set_installed_is_preferred_fallback() {
        let theme = [item(DepRole::Icons, "Papirus-Dark", installed("Papirus-Dark"))];
        let setting = [item(DepRole::Icons, "Gone", DepState::Missing)];
        let n = needs(&theme, &setting, &|_| Some("menta".into()));
        assert_eq!(n.len(), 1);
        assert_eq!(n[0].item.wanted, "Gone");
        assert_eq!(n[0].fallback.as_deref(), Some("Papirus-Dark"));
    }

    /// The standard set is the missing one — don't offer that same one.
    #[test]
    fn fallback_never_equals_the_missing_set() {
        let setting = [item(DepRole::Cursor, "Adwaita", DepState::Missing)];
        let n = needs(&[], &setting, &|_| Some("adwaita".into()));
        assert_eq!(n[0].fallback, None);
    }

    #[test]
    fn answers_and_argv() {
        assert_eq!(answer_from_stdout("install\n"), Some(ACTION_INSTALL));
        assert_eq!(answer_from_stdout("12\nstandard\n"), Some(ACTION_STANDARD));
        assert_eq!(answer_from_stdout(""), None);
        assert_eq!(answer_from_stdout("rm -rf\n"), None);
        let note = Notice {
            summary: "S".into(),
            body: "B".into(),
            actions: vec![(ACTION_INSTALL.into(), "Установить".into())],
        };
        let argv = notify_argv("/usr/bin/notify-send", &note);
        assert!(argv.contains(&"--wait".to_string()));
        assert!(argv.contains(&"--action=install=Установить".to_string()));
        assert_eq!(&argv[argv.len() - 2..], ["S", "B"]);
    }

    #[test]
    fn watch_targets_cover_every_store() {
        let t = watch_targets("/h", "/h/.config/");
        let has = |dir: &str, name: &str| t.iter().any(|w| w.dir == dir && w.names.iter().any(|n| n == name));
        assert!(has("/h/.config/dconf", "user"));
        assert!(has("/h/.config/glib-2.0/settings", "keyfile"));
        assert!(has("/h/.config", "kdeglobals"));
        assert!(has("/h/.config/xfce4/xfconf/xfce-perchannel-xml", "xsettings.xml"));
        assert!(has("/h/.config/gtk-3.0", "settings.ini"));
        assert!(has("/h/.config/lxqt", "lxqt.conf"));
    }

    #[test]
    fn spelling_is_fixed_only_when_disk_differs() {
        assert_eq!(spelling_fix("WhiteSur-Dark", Some("WhiteSur-dark")).as_deref(), Some("WhiteSur-dark"));
        assert_eq!(spelling_fix("DMZ-White", Some("Vanilla-DMZ")).as_deref(), Some("Vanilla-DMZ"));
        assert_eq!(spelling_fix("Papirus", Some("Papirus")), None);
        assert_eq!(spelling_fix("Gone", None), None);
    }

    #[test]
    fn not_a_set_name() {
        assert!(!is_real_set_name(""));
        assert!(!is_real_set_name("hicolor"));
        assert!(!is_real_set_name("default"));
        assert!(is_real_set_name("WhiteSur-cursors"));
    }

    /// Watcher test bench — NOT for a regular run: live gsettings and notification.
    /// Only under `dbus-run-session` with `GSETTINGS_BACKEND=keyfile`, temporary
    /// `HOME`/`XDG_*` (the `HOME` path must contain `deps-stand`) and
    /// `notify-send`, `pkexec`, `paru`/`yay`, `pacman` replaced in `PATH`:
    /// `REXON_WATCH_STAND_SECS=<how long to wait> cargo test -p rexon-core --lib
    /// theme_watch_stand -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn theme_watch_stand() {
        let home = std::env::var("HOME").unwrap_or_default();
        let Ok(secs) = std::env::var("REXON_WATCH_STAND_SECS") else { return };
        assert!(home.contains("deps-stand"), "стенд только во временном HOME, а не в {home}");
        let secs: u64 = secs.parse().unwrap_or(30);
        std::thread::spawn(|| watch_loop(0, 1000));
        std::thread::sleep(std::time::Duration::from_secs(secs));
        println!("память: {:?}", load_asked());
    }
}
