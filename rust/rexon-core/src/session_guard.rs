//! Pre-session guard of Plasma for AeroThemePlasma: «login without Aero» for one
//! login when the theme has drifted from an updated Plasma.
//!
//! WHY. After `pacman -Syu` the AeroThemePlasma forks stay built against the
//! previous Plasma. The worst cases: a KWin effect with a drifted ABI crashes `kwin_wayland` at load —
//! the wrapper restarts it up to 10 times and throws the person back to the login screen; a panel
//! widget crashes plasmashell — systemd gives up after three crashes, and the person
//! sees a black desktop with a cursor. Neither KWin nor plasmashell has
//! a safe mode.
//!
//! HOW. startplasma runs `~/.config/plasma-workspace/env/*.sh` BEFORE starting
//! KWin and plasmashell (`startplasma.cpp`, `runEnvironmentScripts()`). There
//! Rexon puts a thin script ([`env_script`]): it calls
//! `rexon --session-guard` with a 3-second cap and nothing more; if there is no binary
//! or it crashed, the login proceeds as usual (the login matters more than the guard). The guard:
//!   1. quickly checks the suite against the current libraries — THE SAME checks as
//!      the Rexon window ([`crate::aeroshell::check_modules_with`]: the version string in
//!      effects against KWin, the libplasma fork version against plasma-workspace,
//!      `ldd -r` of the suite's modules), plus `ldd -r /usr/bin/plasmashell`. The data
//!      comes from the pacman database as files (`/var/lib/pacman/local`), without a single
//!      pacman run; `ldd` runs in parallel. The result is [`decide`];
//!   2. a real drift — [`fall_back_to_stock`]: Aero effects get
//!      `kwinrc [Plugins] <id>Enabled=false`, and if the plasmashell side breaks —
//!      the stock panel layout instead of the layout with Aero
//!      widgets. Everything previous is in a copy in `$XDG_STATE_HOME/rexon/session-guard/`;
//!   3. after login — the notification «The theme is temporarily turned off… [Rebuild]
//!      [Details]» ([`notice_route_live`], a separate process with a delay: before
//!      login there is nobody to show notifications);
//!   4. after a successful rebuild ([`after_rebuild_live`]) the effects come back
//!      immediately, the layout — at the next login (plasmashell, while alive, keeps
//!      the layout in memory and on exit would write it over the restored one).
//!      The check matches again at login — the guard restores everything itself.
//!
//! FOOLPROOFING.
//!   * The copy comes before the edit; the edit is an atomic file replacement. A repeated login with
//!     the suite still broken does NOT overwrite the copy: the previous key values
//!     are already recorded, and the file now holds our `false`s — saving them would mean
//!     losing the person's settings. Only new keys are added.
//!   * Restoring is idempotent and careful: a key is restored only if it
//!     still holds our `false` (if the person enabled the effect themselves, their choice stays);
//!     a layout the person managed to change during the «login without Aero» is set aside
//!     before restoring as the copy `appletsrc.during-fallback`.
//!   * AeroThemePlasma is not installed (no shell package) — the guard stays silent.
//!     Outside Plasma it touches nothing; it touches only two files: `kwinrc` and
//!     `plasma-org.kde.plasma.desktop-appletsrc`.
//!   * A false alarm → only a notification. Substitution happens only on unambiguous
//!     evidence: the effect's version string is wrong (the KWin loader decides by it too),
//!     `ldd` cannot find a library or a symbol, the libplasma fork has the wrong version.
//!     Just a «private interface of the previous Qt» ([`crate::aeroshell::Report::at_risk_only`])
//!     or trouble in plasmashell itself without the libplasma fork (that is not Aero, changing
//!     its layout is useless) — only a notification.
//!   * «Check the theme at startup» is turned off in settings — the guard
//!     stays silent as well, and the script is removed.
//!
//! SHARED WITH THE SESSION WATCHDOG. «Roll the theme back to stock and remember how
//! it was» — [`fall_back_to_stock`] and its inverse [`restore_saved`]. The
//! session watchdog will call them too (stage 3: KWin/plasmashell crashes right after
//! login). It must not edit the layout mid-session (plasmashell is alive),
//! so it has [`request_fallback`]: the marker «stock theme from the next
//! login», applied by this guard before KWin. Restoring after
//! such a marker is NOT done by a matching check alone (the check matched also
//! the time everything crashed) — only after a rebuild or via
//! `rexon --session-guard-restore`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::aeroshell::{self, Marker, Report};
use crate::i18n::{tr, trf};
use crate::shell_palette::{ini_apply, ini_get, IniEdit};

// ============================================================================
// Names and paths
// ============================================================================

/// The guard flag (the script in `plasma-workspace/env` calls it).
pub const GUARD_FLAG: &str = "--session-guard";
/// An addition to [`GUARD_FLAG`]: only check and print the decision, change
/// nothing (a check in a VM and a timing measurement).
pub const DRY_RUN_FLAG: &str = "--dry-run";
/// Flag of the deferred notification after login.
pub const NOTICE_FLAG: &str = "--session-guard-notice";
/// Flag of manually restoring the saved theme.
pub const RESTORE_FLAG: &str = "--session-guard-restore";

/// Script name in `~/.config/plasma-workspace/env/`.
pub const ENV_SCRIPT_NAME: &str = "rexon-session-guard.sh";
/// The plasmashell panel and desktop layout.
pub const APPLETSRC_NAME: &str = "plasma-org.kde.plasma.desktop-appletsrc";
/// The KWin module enablement group.
pub const KWIN_PLUGINS_GROUP: &str = "Plugins";
/// The plasmashell binary — its `ldd -r` catches a drifted libplasma fork.
pub const PLASMASHELL_BIN: &str = "/usr/bin/plasmashell";
/// The pacman database of installed packages.
pub const PACMAN_LOCAL: &str = "/var/lib/pacman/local";
/// The guard's time cap in the script, seconds (do not slow down login).
pub const ENV_TIMEOUT_SECS: u64 = 3;
/// How long after login to show the notification, seconds: before that there is
/// nobody to show notifications (the notification service is plasmashell itself).
pub const NOTICE_DELAY_SECS: u64 = 15;
/// How much longer to keep trying to reach the notification service, seconds.
pub const NOTICE_RETRY_SECS: u64 = 120;
/// Cap of one `ldd -r`, seconds.
const LDD_TIMEOUT_SECS: u64 = 2;

/// The guard's files: Plasma settings and the copies directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuardPaths {
    /// `$XDG_CONFIG_HOME` (usually `~/.config`).
    pub config_home: PathBuf,
    /// `$XDG_STATE_HOME/rexon/session-guard`.
    pub state_dir: PathBuf,
}

impl GuardPaths {
    pub fn new(config_home: impl Into<PathBuf>, state_dir: impl Into<PathBuf>) -> Self {
        Self { config_home: config_home.into(), state_dir: state_dir.into() }
    }

    /// Paths of the live user.
    pub fn live() -> Self {
        let home = std::env::var("HOME").unwrap_or_default();
        let state = crate::utils::state_home(std::env::var("XDG_STATE_HOME").ok().as_deref(), &home);
        Self::new(crate::utils::config_home_env(), Path::new(&state).join("rexon/session-guard"))
    }

    pub fn kwinrc(&self) -> PathBuf {
        self.config_home.join("kwinrc")
    }
    pub fn appletsrc(&self) -> PathBuf {
        self.config_home.join(APPLETSRC_NAME)
    }
    pub fn env_script(&self) -> PathBuf {
        self.config_home.join("plasma-workspace/env").join(ENV_SCRIPT_NAME)
    }
    /// What was saved and why.
    pub fn saved_json(&self) -> PathBuf {
        self.state_dir.join("saved.json")
    }
    /// Copy of the person's layout before substitution.
    pub fn saved_appletsrc(&self) -> PathBuf {
        self.state_dir.join("appletsrc")
    }
    /// Full copy of `kwinrc` before the first edit (for the worst case, by hand).
    pub fn saved_kwinrc(&self) -> PathBuf {
        self.state_dir.join("kwinrc")
    }
    /// The layout the person changed during the «login without Aero» — set aside
    /// before restoring their real one.
    pub fn during_fallback_appletsrc(&self) -> PathBuf {
        self.state_dir.join("appletsrc.during-fallback")
    }
    /// A notification waiting to be shown after login.
    pub fn notice_json(&self) -> PathBuf {
        self.state_dir.join("notice.json")
    }
    /// The session watchdog's marker: stock theme from the next login.
    pub fn request_file(&self) -> PathBuf {
        self.state_dir.join("request")
    }
    /// Which «risk only» has already been reported (fingerprint).
    pub fn risk_stamp(&self) -> PathBuf {
        self.state_dir.join("risk-notified")
    }
}

// ============================================================================
// The pacman database as files: without running pacman
// ============================================================================
//
// `pacman -Qq`, `-Q`, `-Ql` are 3–15 runs of 10–40 ms each. Before KWin every
// few tens of milliseconds are login delay. The same database lies as text:
// `/var/lib/pacman/local/<name>-<version>-<release>/{desc,files}`.

/// Package name and version from a database directory name (`kwin-6.8.1-2.1` →
/// `("kwin", "6.8.1-2.1")`). The name may contain `-`, the version and release may not.
pub fn local_db_entry(dir_name: &str) -> Option<(String, String)> {
    let mut parts = dir_name.rsplitn(3, '-');
    let rel = parts.next()?;
    let ver = parts.next()?;
    let name = parts.next()?;
    (!name.is_empty() && !ver.is_empty() && !rel.is_empty()).then(|| (name.to_string(), format!("{ver}-{rel}")))
}

/// The database `files` file → output like `pacman -Ql <pkg>` (`<pkg> /path`), so that
/// the shared parsing follows ([`crate::aeroshell::package_files`]).
pub fn local_files_as_ql(package: &str, files_text: &str) -> String {
    let mut out = String::new();
    let mut in_files = false;
    for line in files_text.lines() {
        if line.starts_with('%') {
            in_files = line == "%FILES%";
            continue;
        }
        if in_files && !line.is_empty() {
            out.push_str(package);
            out.push_str(" /");
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// Installed packages: name → (version, database directory).
#[derive(Debug, Clone, Default)]
pub struct LocalDb {
    pub entries: Vec<(String, String, PathBuf)>,
}

impl LocalDb {
    /// Read the list of database directories. No database (not Arch) — empty.
    pub fn read(root: &Path) -> Self {
        let mut entries = Vec::new();
        if let Ok(rd) = std::fs::read_dir(root) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if let Some((pkg, ver)) = local_db_entry(&name) {
                    entries.push((pkg, ver, e.path()));
                }
            }
        }
        entries.sort();
        Self { entries }
    }
    pub fn names(&self) -> Vec<String> {
        self.entries.iter().map(|(n, _, _)| n.clone()).collect()
    }
    pub fn version(&self, package: &str) -> Option<String> {
        self.entries.iter().find(|(n, _, _)| n == package).map(|(_, v, _)| v.clone())
    }
    /// Output «like `pacman -Ql`» for a package, or None.
    pub fn file_list(&self, package: &str) -> Option<String> {
        let (_, _, dir) = self.entries.iter().find(|(n, _, _)| n == package)?;
        let text = std::fs::read_to_string(dir.join("files")).ok()?;
        Some(local_files_as_ql(package, &text))
    }
}

// ============================================================================
// What the suite consists of: KWin effects and Plasma widgets
// ============================================================================

/// A KWin effect module from the suite.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectModule {
    /// File name (`smodglow.so`) — that is how it is named in the check's problems.
    pub file_name: String,
    /// Module identifier for `kwinrc [Plugins] <id>Enabled`.
    pub id: String,
}

/// What of the suite is enabled in settings: effects and widgets.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AeroParts {
    pub effects: Vec<EffectModule>,
    /// Identifiers of the suite's Plasma widgets and containments.
    pub applets: Vec<String>,
}

/// KWin module identifier: `KPlugin.Id` from the embedded metadata, otherwise the file
/// name without `.so` (`KPluginMetaData` decides the same way).
///
/// Qt 6 embeds metadata as CBOR: the key `KPlugin` (`0x67`+7 letters), inside —
/// `Id` (`0x62 'I' 'd'`) and a text string. The old form (JSON as text) is also
/// understood.
pub fn effect_plugin_id(data: &[u8], file_name: &str) -> String {
    let stem = file_name.strip_suffix(".so").unwrap_or(file_name).to_string();
    let find = |hay: &[u8], needle: &[u8]| hay.windows(needle.len()).position(|w| w == needle);
    let valid = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b));
    if let Some(k) = find(data, b"\x67KPlugin") {
        let rest = &data[k..];
        if let Some(i) = find(rest, b"\x62Id") {
            let v = &rest[i + 3..];
            let (len, from) = match v.first() {
                Some(&b) if (0x60..0x78).contains(&b) => ((b - 0x60) as usize, 1),
                Some(&0x78) => (v.get(1).copied().unwrap_or(0) as usize, 2),
                _ => (0, 0),
            };
            if len > 0 && v.len() >= from + len {
                if let Ok(s) = std::str::from_utf8(&v[from..from + len]) {
                    if valid(s) {
                        return s.to_string();
                    }
                }
            }
        }
    }
    if let Some(k) = find(data, b"\"KPlugin\"") {
        let rest = &data[k..];
        if let Some(i) = find(rest, b"\"Id\"") {
            let tail = String::from_utf8_lossy(&rest[i + 4..rest.len().min(i + 4 + 256)]).to_string();
            if let Some(v) = tail.trim_start().strip_prefix(':').map(str::trim_start).and_then(|t| t.strip_prefix('"')) {
                if let Some((id, _)) = v.split_once('"') {
                    if valid(id) {
                        return id.to_string();
                    }
                }
            }
        }
    }
    stem
}

/// Widget identifier by a package file path: the directory
/// `/usr/share/plasma/plasmoids/<id>/…` or the module `…/plasma/applets/<id>.so`.
pub fn applet_id_of(path: &str) -> Option<String> {
    if let Some((_, rest)) = path.split_once("/share/plasma/plasmoids/") {
        let id = rest.split('/').next().unwrap_or("");
        return (!id.is_empty()).then(|| id.to_string());
    }
    if let Some((_, rest)) = path.split_once("/plasma/applets/") {
        let id = rest.strip_suffix(".so").filter(|r| !r.contains('/'))?;
        return (!id.is_empty()).then(|| id.to_string());
    }
    None
}

/// Composition of the suite from the files of its packages.
pub fn aero_parts(files: &[(String, Vec<String>)], read_file: &dyn Fn(&str) -> Option<Vec<u8>>) -> AeroParts {
    let mut parts = AeroParts::default();
    for (_, list) in files {
        for path in aeroshell::effect_plugin_paths(list) {
            let file_name = path.rsplit('/').next().unwrap_or(&path).to_string();
            let id = read_file(&path)
                .map(|d| effect_plugin_id(&d, &file_name))
                .unwrap_or_else(|| file_name.strip_suffix(".so").unwrap_or(&file_name).to_string());
            if !parts.effects.iter().any(|e| e.id == id) {
                parts.effects.push(EffectModule { file_name, id });
            }
        }
        for path in list {
            if let Some(id) = applet_id_of(path) {
                if !parts.applets.contains(&id) {
                    parts.applets.push(id);
                }
            }
        }
    }
    parts
}

// ============================================================================
// The «drifted or not» decision
// ============================================================================

/// Check result for the guard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// There is no AeroThemePlasma shell — the guard has nothing to do here.
    NotInstalled,
    /// Everything matches.
    Healthy,
    /// There is only a risk, or the trouble is not in the suite — notify, change nothing.
    NoticeOnly { why: Vec<String> },
    /// A real drift: what to turn off for this login.
    Broken { effects: bool, shell: bool, why: Vec<String> },
}

/// Module name from a check problem line (`smodglow.so: undefined symbol: …`).
fn problem_module(line: &str) -> &str {
    line.split(": ").next().unwrap_or(line)
}

/// Decide by the suite check ([`crate::aeroshell::check_modules_with`]) and
/// the `ldd -r` of plasmashell ([`crate::aeroshell::ldd_problems`]).
pub fn decide(report: &Report, plasmashell_problems: &[String], parts: &AeroParts) -> Verdict {
    if !report.packages.iter().any(|p| p.package.starts_with(aeroshell::SHELL_PACKAGE_PREFIX)) {
        return Verdict::NotInstalled;
    }
    let is_effect = |module: &str| parts.effects.iter().any(|e| e.file_name == module);
    // «Private interface of the previous Qt» is a risk, not a breakage: the module still
    // loads. Substituting settings over it is a false alarm.
    let certain = |line: &str| !line.contains(aeroshell::QT_PRIVATE_PROBLEM);
    let differs = |p: &aeroshell::PackageVerdict| matches!((&p.built_for, &p.installed), (Some(b), Some(i)) if b != i);

    let mut why = Vec::new();
    let mut effects = false;
    let mut shell = false;
    for p in &report.packages {
        if p.marker == Marker::KwinEffect && differs(p) {
            effects = true;
            why.push(format!(
                "{}: KWin {} ≠ {}",
                p.package,
                p.built_for.as_deref().unwrap_or("?"),
                p.installed.as_deref().unwrap_or("?")
            ));
        }
        if p.marker == Marker::PlasmaLibrary && differs(p) {
            shell = true;
            why.push(format!(
                "{}: libPlasma {} ≠ Plasma {}",
                p.package,
                p.built_for.as_deref().unwrap_or("?"),
                p.installed.as_deref().unwrap_or("?")
            ));
        }
        for line in p.problems.iter().filter(|l| certain(l)) {
            if is_effect(problem_module(line)) {
                effects = true;
            } else {
                shell = true;
            }
            why.push(format!("{}: {line}", p.package));
        }
    }
    let fork = report.packages.iter().any(|p| p.package.starts_with(aeroshell::LIBPLASMA_FORK_PREFIX));
    if !plasmashell_problems.is_empty() {
        for line in plasmashell_problems {
            why.push(format!("plasmashell: {line}"));
        }
        // plasmashell does not load while the libplasma fork is installed — it is the fork.
        // Without the fork the trouble is not in the suite (a Qt mix etc.): the Aero layout
        // has nothing to do with it, and changing it is a false alarm.
        shell |= fork;
    }
    if effects || shell {
        return Verdict::Broken { effects, shell, why };
    }
    if report.needs_rebuild || !plasmashell_problems.is_empty() {
        for p in report.packages.iter().filter(|p| p.stale) {
            for line in &p.problems {
                why.push(format!("{}: {line}", p.package));
            }
        }
        return Verdict::NoticeOnly { why };
    }
    Verdict::Healthy
}

// ============================================================================
// kwinrc: turn off effects with a copy and restore them
// ============================================================================

/// Previous value of one key (`None` — the key did not exist).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedKey {
    pub key: String,
    pub previous: Option<String>,
}

/// What the guard turns off: `false` is the value it writes.
const OFF: &str = "false";

fn enabled_key(id: &str) -> String {
    format!("{id}Enabled")
}

/// Turn off effects in the `kwinrc` text. Previous values are appended to
/// `saved` — only for keys not there yet (a repeated login does not
/// overwrite the copy with our own `false`s). An already disabled key is neither touched nor
/// remembered. → the new text, or None if there is nothing to change.
pub fn disable_effects(kwinrc: Option<&str>, ids: &[String], saved: &mut Vec<SavedKey>) -> Option<String> {
    let text = kwinrc.unwrap_or_default();
    let mut edits = Vec::new();
    for id in ids {
        let key = enabled_key(id);
        let current = ini_get(text, KWIN_PLUGINS_GROUP, &key);
        if current.as_deref() == Some(OFF) {
            continue;
        }
        if !saved.iter().any(|s| s.key == key) {
            saved.push(SavedKey { key: key.clone(), previous: current });
        }
        edits.push(IniEdit { group: KWIN_PLUGINS_GROUP.into(), key, value: Some(OFF.into()) });
    }
    (!edits.is_empty()).then(|| ini_apply(kwinrc, &edits))
}

/// Restore the saved keys. A key that no longer holds our `false` (the person
/// enabled the effect themselves) stays as is and goes into the second list.
/// → (the new text or None if there is nothing to change; the keys left alone).
pub fn restore_effects(kwinrc: Option<&str>, saved: &[SavedKey]) -> (Option<String>, Vec<String>) {
    let text = kwinrc.unwrap_or_default();
    let mut edits = Vec::new();
    let mut kept = Vec::new();
    for s in saved {
        let current = ini_get(text, KWIN_PLUGINS_GROUP, &s.key);
        if current.as_deref() != Some(OFF) {
            if current != s.previous {
                kept.push(s.key.clone());
            }
            continue;
        }
        if s.previous.as_deref() == Some(OFF) {
            continue;
        }
        edits.push(IniEdit { group: KWIN_PLUGINS_GROUP.into(), key: s.key.clone(), value: s.previous.clone() });
    }
    ((!edits.is_empty()).then(|| ini_apply(kwinrc, &edits)), kept)
}

// ============================================================================
// plasmashell layout: stock instead of the layout with Aero widgets
// ============================================================================
//
// Layout groups: `[Containments][N]` — a containment (desktop, panel,
// the system tray's inner containment), `[Containments][N][Applets][M]` —
// a widget in it, deeper — their settings. The stock layout is built from
// the person's layout: desktops stay (wallpapers, activities, screens), only
// Aero widgets are removed from them; all panels go away, replaced by one
// stock panel at the bottom, like Plasma's default. The person's layout itself
// lies in full in the copy and is restored as a file.

struct Group {
    name: Option<String>,
    lines: Vec<String>,
}

fn split_groups(text: &str) -> Vec<Group> {
    let mut out = vec![Group { name: None, lines: Vec::new() }];
    for line in text.lines() {
        let t = line.trim();
        if t.len() >= 2 && t.starts_with('[') && t.ends_with(']') {
            out.push(Group { name: Some(t[1..t.len() - 1].to_string()), lines: Vec::new() });
        } else {
            out.last_mut().expect("преамбула есть всегда").lines.push(line.to_string());
        }
    }
    out
}

fn group_value(g: &Group, key: &str) -> Option<String> {
    g.lines.iter().rev().find_map(|l| {
        let (k, v) = l.split_once('=')?;
        (k.trim() == key).then(|| v.trim().to_string())
    })
}

fn segments(name: &str) -> Vec<&str> {
    name.split("][").collect()
}

/// Whether the layout refers to the suite's widgets or containments.
pub fn layout_uses(appletsrc: &str, ids: &[String]) -> bool {
    split_groups(appletsrc).iter().any(|g| {
        let Some(name) = &g.name else { return false };
        let s = segments(name);
        let is_item = (s.len() == 2 && s[0] == "Containments") || (s.len() == 4 && s[0] == "Containments" && s[2] == "Applets");
        is_item && group_value(g, "plugin").is_some_and(|p| ids.contains(&p))
    })
}

/// Widgets of the stock Plasma panel (`org.kde.plasma.desktop` `layout.js`).
pub const STOCK_PANEL_APPLETS: &[&str] = &[
    "org.kde.plasma.kickoff",
    "org.kde.plasma.icontasks",
    "org.kde.plasma.marginsseparator",
    "org.kde.plasma.systemtray",
    "org.kde.plasma.digitalclock",
    "org.kde.plasma.showdesktop",
];
const STOCK_DESKTOP: &str = "org.kde.desktopcontainment";
const SYSTRAY_APPLET: &str = "org.kde.plasma.systemtray";
const SYSTRAY_CONTAINMENT: &str = "org.kde.plasma.private.systemtray";

/// Stock layout from the person's layout. None — the layout does not refer
/// to the suite, nothing to substitute.
pub fn stock_layout(appletsrc: &str, aero: &[String]) -> Option<String> {
    if !layout_uses(appletsrc, aero) {
        return None;
    }
    let groups = split_groups(appletsrc);
    let is_aero = |g: &Group| group_value(g, "plugin").is_some_and(|p| aero.contains(&p));
    let mut max_id: u64 = 0;
    let mut kept_containments: Vec<String> = Vec::new();
    let mut dropped_applets: Vec<(String, String)> = Vec::new();
    for g in &groups {
        let Some(name) = &g.name else { continue };
        let s = segments(name);
        if s.first() != Some(&"Containments") {
            continue;
        }
        for (i, seg) in s.iter().enumerate() {
            if (i == 1 || (i == 3 && s[2] == "Applets")) && seg.bytes().all(|b| b.is_ascii_digit()) {
                max_id = max_id.max(seg.parse().unwrap_or(0));
            }
        }
        if s.len() == 2 {
            let zero = |k: &str| group_value(g, k).is_none_or(|v| v == "0");
            let plugin = group_value(g, "plugin").unwrap_or_default();
            if zero("formfactor") && zero("location") && plugin != SYSTRAY_CONTAINMENT {
                kept_containments.push(s[1].to_string());
            }
        }
        if s.len() == 4 && s[2] == "Applets" && is_aero(g) {
            dropped_applets.push((s[1].to_string(), s[3].to_string()));
        }
    }

    let mut out = String::new();
    for g in &groups {
        let keep = match &g.name {
            None => true,
            Some(name) => {
                let s = segments(name);
                if s.first() != Some(&"Containments") || s.len() < 2 {
                    true
                } else if !kept_containments.iter().any(|c| c == s[1]) {
                    false
                } else {
                    !(s.len() >= 4 && s[2] == "Applets" && dropped_applets.iter().any(|(c, a)| c == s[1] && a == s[3]))
                }
            }
        };
        if !keep {
            continue;
        }
        if let Some(name) = &g.name {
            out.push_str(&format!("[{name}]\n"));
        }
        let desktop_aero = g.name.as_deref().is_some_and(|n| segments(n).len() == 2) && is_aero(g);
        for l in &g.lines {
            if desktop_aero && l.split_once('=').is_some_and(|(k, _)| k.trim() == "plugin") {
                out.push_str(&format!("plugin={STOCK_DESKTOP}\n"));
            } else {
                out.push_str(l);
                out.push('\n');
            }
        }
    }
    while out.ends_with("\n\n") {
        out.pop();
    }
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    if !out.trim().is_empty() {
        out.push('\n');
    }

    // The stock panel. A widget that the suite itself replaces (the same
    // identifier in its package) is not put into it: it is the broken one.
    let panel = max_id + 1;
    let mut next = panel;
    let mut order = Vec::new();
    let mut applets = String::new();
    let mut systray_inner = None;
    for id in STOCK_PANEL_APPLETS.iter().filter(|id| !aero.iter().any(|a| a == *id)) {
        next += 1;
        order.push(next.to_string());
        applets.push_str(&format!("[Containments][{panel}][Applets][{next}]\nimmutability=1\nplugin={id}\n\n"));
        if *id == SYSTRAY_APPLET && !aero.iter().any(|a| a == SYSTRAY_CONTAINMENT) {
            systray_inner = Some(next);
        }
    }
    out.push_str(&format!(
        "[Containments][{panel}]\nactivityId=\nformfactor=2\nimmutability=1\nlastScreen=0\nlocation=4\nplugin=org.kde.panel\nwallpaperplugin=org.kde.image\n\n"
    ));
    out.push_str(&applets);
    if let Some(tray) = systray_inner {
        next += 1;
        out.push_str(&format!("[Containments][{panel}][Applets][{tray}][Configuration]\nSystrayContainmentId={next}\n\n"));
        out.push_str(&format!(
            "[Containments][{next}]\nactivityId=\nformfactor=2\nimmutability=1\nlastScreen=0\nlocation=4\nplugin={SYSTRAY_CONTAINMENT}\nwallpaperplugin=org.kde.image\n\n"
        ));
    }
    out.push_str(&format!("[Containments][{panel}][General]\nAppletOrder={}\n", order.join(";")));
    Some(out)
}

// ============================================================================
// State: what was saved and why
// ============================================================================

/// Why the theme was rolled back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FallbackCause {
    /// The check drifted — it will match at login and everything will come back by itself.
    #[default]
    Mismatch,
    /// Requested by the session watchdog (crashes after login): the check matched back then,
    /// so a matching check by itself is no reason to restore.
    Requested { reason: String },
}

/// The person's saved theme.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SavedLook {
    pub cause: FallbackCause,
    /// When it was rolled back (seconds since the epoch).
    pub since: u64,
    /// Previous values of the `kwinrc [Plugins]` keys.
    pub effects: Vec<SavedKey>,
    /// The person's layout lies as a copy ([`GuardPaths::saved_appletsrc`]).
    pub layout: bool,
    /// Fingerprint of the stock layout we wrote — it shows whether the person
    /// changed it.
    pub layout_written: Option<String>,
    /// The rebuild succeeded — restore at login, even if the rollback was requested by
    /// the session watchdog.
    pub rebuilt: bool,
    /// Technical reasons (for «Details»).
    pub why: Vec<String>,
}

impl SavedLook {
    pub fn is_empty(&self) -> bool {
        self.effects.is_empty() && !self.layout
    }
}

/// Text fingerprint (FNV-1a, 64 bits): stable between builds.
pub fn fingerprint(text: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

fn read_text(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

/// Write a file atomically: next to it and by renaming. An interrupted write
/// leaves the previous file intact.
pub(crate) fn write_atomic(path: &Path, text: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let tmp = path.with_extension("rexon-tmp");
    std::fs::write(&tmp, text).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
}

/// Read what was saved. No file or it is corrupted — None.
pub fn load_saved(paths: &GuardPaths) -> Option<SavedLook> {
    serde_json::from_str(&read_text(&paths.saved_json())?).ok()
}

fn store_saved(paths: &GuardPaths, saved: &SavedLook) -> Result<(), String> {
    let text = serde_json::to_string_pretty(saved).map_err(|e| e.to_string())?;
    write_atomic(&paths.saved_json(), &text)
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

// ============================================================================
// SHARED: roll back to stock and remember; restore
// ============================================================================

/// What to roll back.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FallbackPlan {
    /// Identifiers of the KWin effects to turn off.
    pub effects: Vec<String>,
    /// Identifiers of the suite's widgets: a layout referring to them
    /// is replaced by the stock one. Empty — do not touch the layout.
    pub applets: Vec<String>,
    /// Technical reasons (for «Details»).
    pub why: Vec<String>,
}

impl FallbackPlan {
    /// The whole suite: both effects and layout (that is how the session watchdog rolls back).
    pub fn whole(parts: &AeroParts) -> Self {
        Self {
            effects: parts.effects.iter().map(|e| e.id.clone()).collect(),
            applets: parts.applets.clone(),
            why: Vec::new(),
        }
    }
}

/// What the rollback did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FallbackDone {
    /// Effects turned off now (`kwinrc` keys).
    pub effects_off: Vec<String>,
    /// The layout was substituted now.
    pub layout_replaced: bool,
}

impl FallbackDone {
    pub fn changed(&self) -> bool {
        !self.effects_off.is_empty() || self.layout_replaced
    }
}

/// Roll the theme back to stock and remember how it was.
///
/// A SHARED function: the pre-KWin guard calls it ([`guard_with`]) and the session
/// watchdog will too. Order for each file: a copy in `state_dir` (only if there is none
/// yet) → the «what was saved» record → editing the file by atomic replacement.
/// An interruption anywhere loses no settings: the copy always comes before the edit.
///
/// Idempotent: a repeated call with the theme already rolled back overwrites nothing
/// in the copies; it only finishes what got enabled again
/// (a new effect, a layout Plasma rebuilt from the Aero package).
///
/// ⚠️ The layout can only be edited while plasmashell is not running (it
/// keeps it in memory and would write over it) — mid-session call
/// [`request_fallback`].
pub fn fall_back_to_stock(paths: &GuardPaths, plan: &FallbackPlan, cause: FallbackCause) -> Result<FallbackDone, String> {
    let mut saved = load_saved(paths).unwrap_or_default();
    if saved.is_empty() {
        saved.cause = cause;
        saved.since = now_secs();
        saved.rebuilt = false;
    } else if matches!(cause, FallbackCause::Requested { .. }) {
        // A crash after login outweighs a drift: such a case is not restored
        // by a matching check alone.
        saved.cause = cause;
    }
    for w in &plan.why {
        if !saved.why.contains(w) {
            saved.why.push(w.clone());
        }
    }
    let mut done = FallbackDone::default();

    // ── kwinrc ──
    if !plan.effects.is_empty() {
        let kwinrc = read_text(&paths.kwinrc());
        let before = saved.effects.len();
        if let Some(new_text) = disable_effects(kwinrc.as_deref(), &plan.effects, &mut saved.effects) {
            if let Some(original) = &kwinrc {
                if !paths.saved_kwinrc().exists() {
                    write_atomic(&paths.saved_kwinrc(), original)?;
                }
            }
            store_saved(paths, &saved)?;
            write_atomic(&paths.kwinrc(), &new_text)?;
            done.effects_off = plan
                .effects
                .iter()
                .map(|id| enabled_key(id))
                .filter(|k| ini_get(kwinrc.as_deref().unwrap_or_default(), KWIN_PLUGINS_GROUP, k).as_deref() != Some(OFF))
                .collect();
        } else if saved.effects.len() != before {
            store_saved(paths, &saved)?;
        }
    }

    // ── layout ──
    if !plan.applets.is_empty() {
        if let Some(current) = read_text(&paths.appletsrc()) {
            if let Some(stock) = stock_layout(&current, &plan.applets) {
                // Only the first copy: if it already exists, the file now holds
                // NOT the person's layout (Plasma rebuilt it from the package).
                if !saved.layout || !paths.saved_appletsrc().exists() {
                    write_atomic(&paths.saved_appletsrc(), &current)?;
                    saved.layout = true;
                }
                saved.layout_written = Some(fingerprint(&stock));
                store_saved(paths, &saved)?;
                write_atomic(&paths.appletsrc(), &stock)?;
                done.layout_replaced = true;
            }
        }
    }
    if !saved.is_empty() {
        store_saved(paths, &saved)?;
    }
    Ok(done)
}

/// What to restore.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestoreScope {
    /// Everything saved (before plasmashell starts — at login or by hand).
    All,
    /// Only KWin effects (mid-session: plasmashell holds the layout).
    EffectsOnly,
}

/// What the restore brought back.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RestoreDone {
    /// There was something to restore.
    pub had_saved: bool,
    /// Restored `kwinrc` keys.
    pub effects: Vec<String>,
    /// Keys the person changed themselves — left as is.
    pub kept: Vec<String>,
    /// The person's layout was restored.
    pub layout: bool,
    /// The layout is still waiting to be restored (next login).
    pub layout_pending: bool,
}

/// Restore what [`fall_back_to_stock`] saved. Idempotent: if there is nothing
/// to restore — `had_saved = false` and not a single file touched.
pub fn restore_saved(paths: &GuardPaths, scope: RestoreScope) -> Result<RestoreDone, String> {
    let Some(mut saved) = load_saved(paths) else {
        return Ok(RestoreDone::default());
    };
    let mut done = RestoreDone { had_saved: !saved.is_empty(), ..Default::default() };

    if !saved.effects.is_empty() {
        let kwinrc = read_text(&paths.kwinrc());
        let (new_text, kept) = restore_effects(kwinrc.as_deref(), &saved.effects);
        if let Some(t) = new_text {
            write_atomic(&paths.kwinrc(), &t)?;
        }
        done.effects = saved.effects.iter().map(|s| s.key.clone()).filter(|k| !kept.contains(k)).collect();
        done.kept = kept;
        saved.effects.clear();
        store_saved(paths, &saved)?;
    }

    if saved.layout {
        if scope == RestoreScope::All {
            let copy = paths.saved_appletsrc();
            match read_text(&copy) {
                Some(original) => {
                    if let Some(current) = read_text(&paths.appletsrc()) {
                        if Some(fingerprint(&current)) != saved.layout_written && current != original {
                            write_atomic(&paths.during_fallback_appletsrc(), &current)?;
                        }
                    }
                    write_atomic(&paths.appletsrc(), &original)?;
                    done.layout = true;
                }
                // There is no copy (deleted by hand) — nothing to restore, and
                // no reason to remember it either.
                None => {}
            }
            saved.layout = false;
            saved.layout_written = None;
            let _ = std::fs::remove_file(&copy);
            store_saved(paths, &saved)?;
        } else {
            done.layout_pending = true;
        }
    }

    if saved.is_empty() {
        let _ = std::fs::remove_file(paths.saved_json());
        let _ = std::fs::remove_file(paths.saved_kwinrc());
    }
    Ok(done)
}

/// The session watchdog's marker: stock theme from the next login (it will be
/// applied by [`guard_with`] before KWin). `reason` — the human-readable reason for the
/// notification.
pub fn request_fallback(paths: &GuardPaths, reason: &str) -> Result<(), String> {
    write_atomic(&paths.request_file(), reason)
}

/// The rebuild succeeded: restore at the next login, whoever requested the rollback.
pub fn mark_rebuilt(paths: &GuardPaths) -> Result<(), String> {
    let Some(mut saved) = load_saved(paths) else { return Ok(()) };
    saved.rebuilt = true;
    store_saved(paths, &saved)
}

// ============================================================================
// The guard at login
// ============================================================================

/// Which notification to show after login.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PendingNotice {
    /// The theme is turned off for this login.
    TurnedOff { effects: bool, layout: bool, why: Vec<String>, reason: Option<String> },
    /// The saved theme was restored.
    Restored,
    /// Only a risk — nothing was changed.
    Risk { why: Vec<String> },
    /// Session watchdog: plasmashell crashed because of the suite, the layout will become
    /// stock from the next login ([`request_fallback`]).
    NextLogin { reason: String },
}

/// What the guard did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuardOutcome {
    pub verdict: Verdict,
    pub notice: Option<PendingNotice>,
    /// Lines for the log.
    pub log: Vec<String>,
}

/// The whole guard on a ready check: decision → rollback/restore → notification.
///
/// `risk_fingerprint` — fingerprint of the «risk only» case: we do not speak about the same thing
/// a second time.
pub fn guard_with(paths: &GuardPaths, verdict: Verdict, parts: &AeroParts, risk_fingerprint: &str) -> GuardOutcome {
    let mut log = Vec::new();
    let mut notice = None;
    if verdict == Verdict::NotInstalled {
        // The suite was removed, but the saved state remained: restoring a layout with
        // widgets that no longer exist is not allowed; leave it as is.
        return GuardOutcome { verdict, notice, log: vec!["session-guard: AeroThemePlasma is not installed".into()] };
    }
    let requested = read_text(&paths.request_file());
    if let Some(reason) = &requested {
        let plan = FallbackPlan::whole(parts);
        match fall_back_to_stock(paths, &plan, FallbackCause::Requested { reason: reason.trim().to_string() }) {
            Ok(done) => {
                let _ = std::fs::remove_file(paths.request_file());
                log.push(format!("session-guard: requested fallback: {done:?}"));
                notice = Some(PendingNotice::TurnedOff {
                    effects: !done.effects_off.is_empty(),
                    layout: done.layout_replaced,
                    why: Vec::new(),
                    reason: Some(reason.trim().to_string()),
                });
            }
            Err(e) => log.push(format!("session-guard: requested fallback failed: {e}")),
        }
        return GuardOutcome { verdict, notice, log };
    }
    match &verdict {
        Verdict::NotInstalled => {}
        Verdict::Healthy => {
            let saved = load_saved(paths).filter(|s| !s.is_empty());
            if let Some(s) = saved {
                let auto = s.rebuilt || s.cause == FallbackCause::Mismatch;
                if auto {
                    match restore_saved(paths, RestoreScope::All) {
                        Ok(done) => {
                            log.push(format!("session-guard: restored {done:?}"));
                            notice = Some(PendingNotice::Restored);
                        }
                        Err(e) => log.push(format!("session-guard: restore failed: {e}")),
                    }
                } else {
                    log.push("session-guard: kept the fallback requested after a crash".into());
                }
            }
            let _ = std::fs::remove_file(paths.risk_stamp());
        }
        Verdict::NoticeOnly { why } => {
            let last = read_text(&paths.risk_stamp());
            if last.as_deref().map(str::trim) != Some(risk_fingerprint) {
                notice = Some(PendingNotice::Risk { why: why.clone() });
                let _ = write_atomic(&paths.risk_stamp(), risk_fingerprint);
            }
            log.push(format!("session-guard: notice only: {why:?}"));
        }
        Verdict::Broken { effects, shell, why } => {
            let plan = FallbackPlan {
                effects: if *effects { parts.effects.iter().map(|e| e.id.clone()).collect() } else { Vec::new() },
                applets: if *shell { parts.applets.clone() } else { Vec::new() },
                why: why.clone(),
            };
            match fall_back_to_stock(paths, &plan, FallbackCause::Mismatch) {
                Ok(done) => {
                    log.push(format!("session-guard: fallback {done:?}; why {why:?}"));
                    // Already rolled back by the previous login — remind anyway: the person
                    // logged into the theme without Aero again.
                    let saved = load_saved(paths).unwrap_or_default();
                    if done.changed() || !saved.is_empty() {
                        notice = Some(PendingNotice::TurnedOff {
                            effects: !saved.effects.is_empty(),
                            layout: saved.layout,
                            why: why.clone(),
                            reason: None,
                        });
                    }
                }
                Err(e) => log.push(format!("session-guard: fallback failed: {e}")),
            }
        }
    }
    GuardOutcome { verdict, notice, log }
}

// ============================================================================
// The script in plasma-workspace/env
// ============================================================================

fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Script text. startplasma EXECUTES these files in a shared shell (via
/// `.`), so there is no `exit`, no `set -e`, no variables here: an exit or
/// an error would cut off other scripts and session variables. Output goes nowhere
/// (startplasma reads this shell's output). No binary (Rexon was removed) —
/// the script removes itself.
pub fn env_script(program: &str, script_path: &str) -> String {
    let bin = sh_quote(program);
    format!(
        "# Rexon: AeroThemePlasma check before KWin starts. Written and removed by Rexon.\n\
         # It never blocks the login: at most {ENV_TIMEOUT_SECS} s, every error is ignored.\n\
         if [ -x {bin} ]; then\n\
         \x20   if command -v timeout >/dev/null 2>&1; then\n\
         \x20       timeout -k 1 {ENV_TIMEOUT_SECS} {bin} {GUARD_FLAG} </dev/null >/dev/null 2>&1 || true\n\
         \x20   else\n\
         \x20       {bin} {GUARD_FLAG} </dev/null >/dev/null 2>&1 || true\n\
         \x20   fi\n\
         else\n\
         \x20   rm -f {} 2>/dev/null || true\n\
         fi\n",
        sh_quote(script_path)
    )
}

/// What to do with the script.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptChange {
    Nothing,
    Write(String),
    Remove,
}

/// Plan: `wanted` — AeroThemePlasma is installed and the check is not turned off;
/// `program` — the Rexon binary (None — nothing to call, the script is removed).
pub fn env_script_change(wanted: bool, program: Option<&str>, script_path: &str, existing: Option<&str>) -> ScriptChange {
    match (wanted, program) {
        (true, Some(p)) if p.starts_with('/') => {
            let text = env_script(p, script_path);
            if existing == Some(text.as_str()) {
                ScriptChange::Nothing
            } else {
                ScriptChange::Write(text)
            }
        }
        _ if existing.is_some() => ScriptChange::Remove,
        _ => ScriptChange::Nothing,
    }
}

/// Apply the script plan.
pub fn sync_env_script(paths: &GuardPaths, wanted: bool, program: Option<&str>) -> Result<ScriptChange, String> {
    let path = paths.env_script();
    let existing = read_text(&path);
    let change = env_script_change(wanted, program, &path.to_string_lossy(), existing.as_deref());
    match &change {
        ScriptChange::Nothing => {}
        ScriptChange::Write(text) => write_atomic(&path, text)?,
        ScriptChange::Remove => std::fs::remove_file(&path).map_err(|e| format!("{}: {e}", path.display()))?,
    }
    Ok(change)
}

/// The theme check is enabled in settings ([`aeroshell::CHECK_ON_START_SETTING`]).
pub(crate) fn check_enabled_live() -> bool {
    let saved = crate::settings::SettingsManager::new()
        .ok()
        .and_then(|m| m.get(aeroshell::CHECK_ON_START_SETTING).and_then(serde_json::Value::as_bool));
    aeroshell::check_on_start(saved)
}

/// Whether the AeroThemePlasma shell is installed (from the pacman database, without running pacman).
pub(crate) fn aero_installed(db: &LocalDb) -> bool {
    db.entries.iter().any(|(n, _, _)| n.starts_with(aeroshell::SHELL_PACKAGE_PREFIX))
}

/// Install or remove the script on the live machine. Called together with
/// the rest of the update watching (at Rexon startup).
pub fn sync_env_script_live() -> Result<ScriptChange, String> {
    let db = LocalDb::read(Path::new(PACMAN_LOCAL));
    let entry = crate::installer::launch_entry_live();
    let program = match &entry {
        crate::installer::LaunchEntry::MainWindow(p) => Some(p.clone()),
        crate::installer::LaunchEntry::Python => None,
    };
    let wanted = aero_installed(&db) && check_enabled_live();
    let paths = GuardPaths::live();
    // The session watchdog — under the same conditions and from the same point: both are installed
    // while AeroThemePlasma is installed, and removed together with it.
    let exec = crate::app::unit_launch_command(&entry, None, &[crate::session_watch::WATCH_FLAG]);
    let program_ref = program.as_deref();
    if let Err(e) = crate::session_watch::sync_unit_live(
        &paths.config_home.to_string_lossy(),
        wanted,
        exec.as_deref(),
        program_ref,
    ) {
        crate::log_warn!("session-watch", "юнит сторожа сеанса: {e}");
    }
    sync_env_script(&paths, wanted, program_ref)
}

// ============================================================================
// Live check (fast)
// ============================================================================

/// Result of the live check.
pub struct LiveFacts {
    pub report: Report,
    pub plasmashell_problems: Vec<String>,
    pub parts: AeroParts,
}

fn ldd_live(path: &str) -> Option<String> {
    let r = crate::utils::run(
        &crate::utils::RunSpec {
            argv: vec!["ldd".into(), "-r".into(), path.into()],
            output: crate::utils::RunOutput::Capture,
            timeout_secs: Some(LDD_TIMEOUT_SECS),
            lang_c: true,
            stderr_to_stdout: true,
            ..Default::default()
        },
        None,
    );
    matches!(r.outcome, crate::utils::RunOutcome::Ok | crate::utils::RunOutcome::Failed)
        .then(|| String::from_utf8_lossy(&r.stdout).to_string())
}

/// `ldd -r` over all paths at once (a thread per core): one `ldd` takes tens of
/// milliseconds, one after another over two dozen modules would add up to a second.
fn ldd_parallel(paths: &[String]) -> std::collections::HashMap<String, String> {
    let workers = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(16);
    let next = std::sync::atomic::AtomicUsize::new(0);
    let out = std::sync::Mutex::new(std::collections::HashMap::new());
    std::thread::scope(|s| {
        for _ in 0..workers.min(paths.len().max(1)) {
            s.spawn(|| loop {
                let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let Some(p) = paths.get(i) else { break };
                if let Some(text) = ldd_live(p) {
                    out.lock().expect("замок результатов").insert(p.clone(), text);
                }
            });
        }
    });
    out.into_inner().unwrap_or_default()
}

/// The check on the live machine. None — AeroThemePlasma is not installed (we go no further:
/// this is almost the whole guard path for everyone except owners of the suite).
pub fn facts_live() -> Option<LiveFacts> {
    let db = LocalDb::read(Path::new(PACMAN_LOCAL));
    if !aero_installed(&db) {
        return None;
    }
    let forks = crate::system_plan::filter_plasma_forks(&db.names());
    let files: Vec<(String, Vec<String>)> = forks
        .iter()
        .map(|p| (p.clone(), db.file_list(p).map(|o| aeroshell::package_files(&o)).unwrap_or_default()))
        .collect();
    let read_file = |p: &str| std::fs::read(p).ok();
    let parts = aero_parts(&files, &read_file);
    let has_ldd = crate::utils::which("ldd", None).is_some();
    let mut targets: Vec<String> = files.iter().flat_map(|(_, f)| aeroshell::qt_module_paths(f)).collect();
    targets.push(PLASMASHELL_BIN.to_string());
    let ldd = if has_ldd { ldd_parallel(&targets) } else { Default::default() };
    let file_list = |p: &str| db.file_list(p);
    let kwin = db.version("kwin");
    let plasma = db.version("plasma-workspace");
    let qt = db.version("qt6-base");
    let report = aeroshell::check_modules_with(
        &forks,
        kwin.as_deref(),
        plasma.as_deref(),
        qt.as_deref(),
        &file_list,
        &read_file,
        &|p| ldd.get(p).cloned(),
    );
    let plasmashell_problems = ldd.get(PLASMASHELL_BIN).map(|o| aeroshell::ldd_problems(o, &[])).unwrap_or_default();
    Some(LiveFacts { report, plasmashell_problems, parts })
}

/// The session is Plasma (or nothing has been said about the desktop yet: startplasma may
/// set the desktop name after the scripts).
fn plasma_session(xdg_current_desktop: Option<&str>) -> bool {
    xdg_current_desktop.is_none_or(|d| d.trim().is_empty() || d.split(':').any(|p| p.eq_ignore_ascii_case("KDE")))
}

/// `rexon --session-guard [--dry-run]` — called by the script before KWin.
/// Always 0: the login matters more than the guard.
pub fn guard_route_live(dry_run: bool, out: &mut dyn FnMut(&str)) -> i32 {
    let started = std::time::Instant::now();
    if !plasma_session(std::env::var("XDG_CURRENT_DESKTOP").ok().as_deref()) {
        out("session-guard: not a Plasma session");
        return 0;
    }
    if !check_enabled_live() {
        out("session-guard: disabled in settings");
        return 0;
    }
    let Some(facts) = facts_live() else {
        out("session-guard: AeroThemePlasma is not installed");
        return 0;
    };
    let verdict = decide(&facts.report, &facts.plasmashell_problems, &facts.parts);
    out(&format!("session-guard: {verdict:?} in {} ms", started.elapsed().as_millis()));
    if dry_run {
        out(&format!("session-guard: effects {:?}", facts.parts.effects));
        out(&format!("session-guard: applets {:?}", facts.parts.applets));
        return 0;
    }
    let advice = aeroshell::Advice::Rebuild { packages: facts.report.rebuild.clone() };
    let risk_fp = aeroshell::notify_fingerprint(&facts.report, &advice);
    let paths = GuardPaths::live();
    let outcome = guard_with(&paths, verdict, &facts.parts, &risk_fp);
    for l in &outcome.log {
        crate::log_info!("session-guard", "{l}");
        out(l);
    }
    if let Some(notice) = &outcome.notice {
        queue_notice_live(&paths, notice, out);
    }
    0
}

/// Queue the notification and a separate process that shows it once
/// the notification service comes up (the session watchdog calls this too).
pub(crate) fn queue_notice_live(paths: &GuardPaths, notice: &PendingNotice, out: &mut dyn FnMut(&str)) {
    if let Ok(text) = serde_json::to_string(notice) {
        if write_atomic(&paths.notice_json(), &text).is_ok() {
            spawn_notice_live(out);
        }
    }
}

/// The deferred notification is a separate process in its own group: the script
/// (and the `timeout` around the guard) neither waits for it nor kills it.
fn spawn_notice_live(out: &mut dyn FnMut(&str)) {
    use std::os::unix::process::CommandExt;
    let Ok(exe) = std::env::current_exe() else { return };
    let r = std::process::Command::new(exe)
        .arg(NOTICE_FLAG)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .process_group(0)
        .spawn();
    if let Err(e) = r {
        out(&format!("session-guard: notice process: {e}"));
    }
}

// ============================================================================
// Notification after login
// ============================================================================

/// Title: the theme is turned off for this login.
pub fn turned_off_title() -> String {
    tr("AeroThemePlasma Is Turned Off for Now")
}

/// Text: what happened and what to do.
pub fn turned_off_body(reason: Option<&str>) -> String {
    match reason {
        Some(r) if !r.is_empty() => trf(
            "Plasma failed to start with AeroThemePlasma (%1), so Rexon switched to the standard \
             Plasma look for now. Your settings are saved and come back after a rebuild.",
            &[&r],
        ),
        _ => tr("Plasma was updated, and AeroThemePlasma no longer fits it. To keep the desktop \
                 working, Rexon switched to the standard Plasma look for now. Your settings are \
                 saved and come back after a rebuild."),
    }
}

/// Details: what exactly was turned off.
pub fn turned_off_details(effects: bool, layout: bool, why: &[String]) -> String {
    let mut lines = Vec::new();
    if effects {
        lines.push(tr("The AeroThemePlasma effects are turned off in KWin."));
    }
    if layout {
        lines.push(tr("The panels are replaced with the standard Plasma panel; your layout is saved."));
    }
    lines.push(trf("To bring everything back without rebuilding: rexon %1", &[&RESTORE_FLAG]));
    let mut text = lines.join("\n");
    if !why.is_empty() {
        text.push_str("\n\n");
        text.push_str(&why.iter().take(6).map(|w| format!("• {w}")).collect::<Vec<_>>().join("\n"));
    }
    text
}

/// Title: the layout will become stock from the next login.
pub fn next_login_title() -> String {
    tr("AeroThemePlasma Turns Off at Next Login")
}

/// Text: plasmashell crashed because of the suite, the layout cannot be touched now.
pub fn next_login_body(reason: &str) -> String {
    trf(
        // TRANSLATORS: %1 says which AeroThemePlasma module crashed.
        "The Plasma desktop crashed because of AeroThemePlasma (%1). From the next login, \
         Rexon uses the standard Plasma panels until AeroThemePlasma is rebuilt. Your layout \
         is saved and comes back after a rebuild.",
        &[&reason],
    )
}

pub fn restored_title() -> String {
    tr("AeroThemePlasma Is Back")
}

pub fn restored_body() -> String {
    tr("AeroThemePlasma fits Plasma again, so Rexon brought back your saved look and panels.")
}

pub fn rebuild_button() -> String {
    tr("Rebuild")
}

pub fn details_button() -> String {
    tr("Details")
}

/// After a rebuild: the effects are back; the layout — at the next login.
pub fn after_rebuild_message(layout_pending: bool) -> String {
    if layout_pending {
        tr("Your AeroThemePlasma effects are back on. The panels come back at the next login.")
    } else {
        tr("Your AeroThemePlasma effects are back on.")
    }
}

/// Result of `rexon --session-guard-restore` for printing.
pub fn restore_report(done: &RestoreDone) -> String {
    if !done.had_saved {
        return tr("Nothing to restore: Rexon hasn't changed your Plasma look.");
    }
    let mut lines = vec![tr("Restored the saved Plasma look.")];
    if !done.kept.is_empty() {
        lines.push(trf("Left as you changed them: %1", &[&done.kept.join(", ")]));
    }
    if done.layout_pending {
        lines.push(tr("The panels come back at the next login."));
    }
    lines.join("\n")
}

/// notify-send with actions; waits for the notification service. → the pressed action.
fn notify_live(summary: &str, body: &str, actions: &[(&str, String)], wait: bool) -> Option<String> {
    crate::utils::which("notify-send", None)?;
    let mut argv: Vec<String> = vec![
        "notify-send".into(),
        format!("--app-name={}", crate::utils::NOTIFY_APP_NAME),
        "--icon=rexon".into(),
    ];
    if wait {
        argv.push("--urgency=critical".into());
        argv.push("--wait".into());
    }
    for (id, label) in actions {
        argv.push(format!("--action={id}={label}"));
    }
    argv.push(summary.into());
    argv.push(body.into());
    let started = std::time::Instant::now();
    loop {
        let r = crate::utils::run(
            &crate::utils::RunSpec {
                argv: argv.clone(),
                output: crate::utils::RunOutput::Capture,
                timeout_secs: Some(if wait { aeroshell::NOTIFY_WAIT_SECS } else { 10 }),
                ..Default::default()
            },
            None,
        );
        if r.returncode == Some(0) {
            return Some(String::from_utf8_lossy(&r.stdout).trim().to_string());
        }
        // There is no notification service yet (plasmashell is coming up) — wait.
        if started.elapsed().as_secs() > NOTICE_RETRY_SECS {
            return None;
        }
        std::thread::sleep(std::time::Duration::from_secs(10));
    }
}

/// `rexon --session-guard-notice` — the notification after login.
pub fn notice_route_live(out: &mut dyn FnMut(&str)) -> i32 {
    let paths = GuardPaths::live();
    let Some(text) = read_text(&paths.notice_json()) else { return 0 };
    let _ = std::fs::remove_file(paths.notice_json());
    let Ok(notice) = serde_json::from_str::<PendingNotice>(&text) else { return 0 };
    std::thread::sleep(std::time::Duration::from_secs(NOTICE_DELAY_SECS));
    let actions = [("rebuild", rebuild_button()), ("details", details_button())];
    let (summary, body, details) = match &notice {
        PendingNotice::TurnedOff { effects, layout, why, reason } => {
            (turned_off_title(), turned_off_body(reason.as_deref()), turned_off_details(*effects, *layout, why))
        }
        PendingNotice::Restored => {
            notify_live(&restored_title(), &restored_body(), &[], false);
            return 0;
        }
        PendingNotice::NextLogin { reason } => (
            next_login_title(),
            next_login_body(reason),
            trf("To bring everything back without rebuilding: rexon %1", &[&RESTORE_FLAG]),
        ),
        PendingNotice::Risk { why } => {
            let report_text = why.iter().take(6).map(|w| format!("• {w}")).collect::<Vec<_>>().join("\n");
            (
                crate::de_progress::aeroshell_risk_title(),
                tr("After the system update, AeroThemePlasma is still built for the previous version \
                    of the system. It works for now, but it can fail at any moment. Rexon can rebuild it."),
                report_text,
            )
        }
    };
    match notify_live(&summary, &body, &actions, true).as_deref() {
        Some("rebuild") => {
            // Rebuild — by the same path as the window: Rexon opens on the
            // check, offers a rebuild, the password goes through the polkit window.
            if let Err(e) = crate::installer::open_rexon_live(&[aeroshell::OPEN_FLAG]) {
                out(&format!("session-guard: {e}"));
            }
        }
        Some("details") => {
            if let Some(a) = notify_live(&summary, &details, &actions[..1], true) {
                if a == "rebuild" {
                    let _ = crate::installer::open_rexon_live(&[aeroshell::OPEN_FLAG]);
                }
            }
        }
        _ => {}
    }
    0
}

/// After a successful rebuild ([`crate::system_de_exec::rebuild_aeroshell_live`]):
/// the check matches — restore the effects now (and ask KWin to reread its
/// settings), the layout — at the next login. → a line for the window's summary.
pub fn after_rebuild_live() -> Option<String> {
    let paths = GuardPaths::live();
    load_saved(&paths).filter(|s| !s.is_empty())?;
    let facts = facts_live()?;
    let verdict = decide(&facts.report, &facts.plasmashell_problems, &facts.parts);
    if matches!(verdict, Verdict::Broken { .. }) {
        return None;
    }
    let _ = mark_rebuilt(&paths);
    let done = restore_saved(&paths, RestoreScope::EffectsOnly).ok()?;
    if !done.effects.is_empty() {
        reconfigure_kwin_live();
    }
    Some(after_rebuild_message(done.layout_pending))
}

/// Ask KWin to reread `kwinrc` (as `kwriteconfig` +
/// `qdbus org.kde.KWin /KWin reconfigure` does).
pub(crate) fn reconfigure_kwin_live() {
    if crate::utils::which("dbus-send", None).is_none() {
        return;
    }
    let _ = crate::utils::run(
        &crate::utils::RunSpec {
            argv: reconfigure_kwin_argv(),
            output: crate::utils::RunOutput::Capture,
            timeout_secs: Some(5),
            ..Default::default()
        },
        None,
    );
}

/// The command «KWin, reread your settings».
pub fn reconfigure_kwin_argv() -> Vec<String> {
    ["dbus-send", "--session", "--type=method_call", "--dest=org.kde.KWin", "/KWin", "org.kde.KWin.reconfigure"]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

// ============================================================================
// Uninstalling Rexon during a rollback
// ============================================================================
//
// Rexon is uninstalled while the theme is rolled back: there will be nobody
// to restore it later (the pre-KWin script without a binary removes itself). So on
// uninstall: effects — immediately (it is `kwinrc`, KWin will reread it), while
// the layout cannot be restored mid-session (plasmashell keeps it in memory and on
// exit would write over it) — it is restored by a ONE-SHOT pure sh script
// at the next login, before plasmashell, and the script removes itself. The copies lie in
// `$XDG_STATE_HOME/rexon/session-guard/` — uninstalling Rexon does not touch them.

/// One-shot layout restore at the next login (without Rexon). As with
/// [`env_script`]: no `exit`, no `set -e` — the file runs in startplasma's shared
/// shell.
pub fn restore_once_script(paths: &GuardPaths) -> String {
    let q = |p: PathBuf| sh_quote(&p.to_string_lossy());
    let saved = q(paths.saved_appletsrc());
    let current = q(paths.appletsrc());
    format!(
        "# Rexon was removed while AeroThemePlasma was turned off: put the saved panel\n\
         # layout back once, before plasmashell starts, then remove this file.\n\
         if [ -f {saved} ]; then\n\
         \x20   if [ -f {current} ]; then cp -f {current} {} 2>/dev/null || true; fi\n\
         \x20   cp -f {saved} {current} 2>/dev/null && rm -f {saved} {} {} 2>/dev/null\n\
         fi\n\
         rm -f {} 2>/dev/null || true\n",
        q(paths.during_fallback_appletsrc()),
        q(paths.saved_json()),
        q(paths.saved_kwinrc()),
        q(paths.env_script()),
    )
}

/// What was done when uninstalling Rexon.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UninstallRelease {
    pub restored: RestoreDone,
    /// The layout will be restored by the one-shot script at the next login.
    pub layout_next_login: bool,
}

/// Uninstalling Rexon: restore what was saved and remove the pre-KWin script.
/// Nothing saved — only remove the script and markers.
pub fn release_for_uninstall(paths: &GuardPaths) -> Result<UninstallRelease, String> {
    let restored = restore_saved(paths, RestoreScope::EffectsOnly)?;
    let _ = std::fs::remove_file(paths.request_file());
    let _ = std::fs::remove_file(paths.notice_json());
    let script = paths.env_script();
    let layout_next_login = restored.layout_pending && paths.saved_appletsrc().exists();
    if layout_next_login {
        write_atomic(&script, &restore_once_script(paths))?;
    } else if script.exists() {
        std::fs::remove_file(&script).map_err(|e| format!("{}: {e}", script.display()))?;
    }
    Ok(UninstallRelease { restored, layout_next_login })
}

/// The guard's command-line routes. None — these are not its flags.
pub fn cli_route(args: &[String], out: &mut dyn FnMut(&str)) -> Option<i32> {
    let has = |f: &str| args.iter().any(|a| a == f);
    if has(NOTICE_FLAG) {
        return Some(notice_route_live(out));
    }
    if has(RESTORE_FLAG) {
        // By hand, mid-session: plasmashell holds the layout — effects
        // now, the layout at the next login (the guard will restore it).
        let paths = GuardPaths::live();
        let _ = mark_rebuilt(&paths);
        return Some(match restore_saved(&paths, RestoreScope::EffectsOnly) {
            Ok(done) => {
                if !done.effects.is_empty() {
                    reconfigure_kwin_live();
                }
                for l in restore_report(&done).lines() {
                    out(l);
                }
                0
            }
            Err(e) => {
                out(&e);
                1
            }
        });
    }
    if has(GUARD_FLAG) {
        return Some(guard_route_live(has(DRY_RUN_FLAG), out));
    }
    if has(crate::session_watch::WATCH_FLAG) {
        return Some(crate::session_watch::watch_route_live(out));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aeroshell::PackageVerdict;

    struct Tmp(PathBuf);
    impl Tmp {
        fn new(tag: &str) -> Self {
            let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
            let p = std::env::temp_dir().join(format!("rexon-session-guard-{tag}-{}-{n}", std::process::id()));
            std::fs::create_dir_all(&p).unwrap();
            Tmp(p)
        }
        fn paths(&self) -> GuardPaths {
            GuardPaths::new(self.0.join("config"), self.0.join("state/rexon/session-guard"))
        }
    }
    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn put(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn pkg(name: &str, marker: Marker, built: Option<&str>, inst: Option<&str>, problems: &[&str]) -> PackageVerdict {
        let stale = matches!((built, inst), (Some(b), Some(i)) if b != i) || !problems.is_empty();
        PackageVerdict {
            package: name.into(),
            marker,
            built_for: built.map(Into::into),
            installed: inst.map(Into::into),
            compiled: true,
            stale,
            problems: problems.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn report(pkgs: Vec<PackageVerdict>) -> Report {
        let stale: Vec<String> = pkgs.iter().filter(|p| p.stale).map(|p| p.package.clone()).collect();
        Report { needs_rebuild: !stale.is_empty(), stale, packages: pkgs, ..Report::empty() }
    }

    fn parts() -> AeroParts {
        AeroParts {
            effects: vec![
                EffectModule { file_name: "smodglow.so".into(), id: "smodglow".into() },
                EffectModule { file_name: "aeroblur.so".into(), id: "aeroblur".into() },
            ],
            applets: vec!["io.gitgud.wackyideas.SevenStart".into(), "io.gitgud.wackyideas.seventasks".into()],
        }
    }

    const SHELL: &str = "aerothemeplasma-desktop-git";
    const KWINC: &str = "aeroshell-kwin-components-git";
    const LIBP: &str = "aeroshell-libplasma-git";

    // ── pacman database ──

    #[test]
    fn local_db_entry_splits_name_version_release() {
        assert_eq!(local_db_entry("kwin-6.8.1-2.1"), Some(("kwin".into(), "6.8.1-2.1".into())));
        assert_eq!(
            local_db_entry("aerothemeplasma-desktop-git-r123.abc-1"),
            Some(("aerothemeplasma-desktop-git".into(), "r123.abc-1".into()))
        );
        assert_eq!(local_db_entry("ALPM_DB_VERSION"), None);
        assert_eq!(local_db_entry("a-b"), None);
    }

    #[test]
    fn local_files_become_ql_lines_and_parse_like_pacman() {
        let files = "%FILES%\nusr/\nusr/lib/qt6/plugins/kwin/effects/plugins/smodglow.so\n\n%BACKUP%\netc/x\tabc\n";
        let ql = local_files_as_ql(KWINC, files);
        assert_eq!(ql, format!("{KWINC} /usr/\n{KWINC} /usr/lib/qt6/plugins/kwin/effects/plugins/smodglow.so\n"));
        assert_eq!(aeroshell::package_files(&ql), vec!["/usr/lib/qt6/plugins/kwin/effects/plugins/smodglow.so"]);
    }

    #[test]
    fn local_db_reads_versions_and_files_from_disk() {
        let t = Tmp::new("db");
        put(&t.0.join("local/kwin-6.8.1-2.1/desc"), "%NAME%\nkwin\n");
        put(&t.0.join("local/kwin-6.8.1-2.1/files"), "%FILES%\nusr/bin/kwin_wayland\n");
        let db = LocalDb::read(&t.0.join("local"));
        assert_eq!(db.version("kwin").as_deref(), Some("6.8.1-2.1"));
        assert_eq!(db.file_list("kwin").as_deref(), Some("kwin /usr/bin/kwin_wayland\n"));
        assert!(db.file_list("plasma-workspace").is_none());
        assert!(!aero_installed(&db));
    }

    // ── suite composition ──

    #[test]
    fn effect_id_comes_from_cbor_metadata_else_file_name() {
        let mut data = b"\x00junk\xa1\x67KPlugin\xa2\x64Name\x64Glow\x62Id\x68smodglow".to_vec();
        data.extend_from_slice(b"\x00tail");
        assert_eq!(effect_plugin_id(&data, "libsmodglow.so"), "smodglow");
        let json = br#"{"KPlugin": {"Name": "x", "Id": "aeroblur"}}"#;
        assert_eq!(effect_plugin_id(json, "x.so"), "aeroblur");
        assert_eq!(effect_plugin_id(b"nothing here", "smodsnap.so"), "smodsnap");
    }

    #[test]
    fn applet_ids_from_package_paths() {
        assert_eq!(
            applet_id_of("/usr/share/plasma/plasmoids/io.gitgud.wackyideas.SevenStart/contents/ui/main.qml").as_deref(),
            Some("io.gitgud.wackyideas.SevenStart")
        );
        assert_eq!(applet_id_of("/usr/lib/qt6/plugins/plasma/applets/org.kde.plasma.systemtray.so").as_deref(), Some("org.kde.plasma.systemtray"));
        assert_eq!(applet_id_of("/usr/lib/qt6/plugins/kwin/effects/plugins/smodglow.so"), None);
        let files = vec![(
            SHELL.to_string(),
            vec![
                "/usr/share/plasma/plasmoids/a.b/metadata.json".to_string(),
                "/usr/share/plasma/plasmoids/a.b/contents/ui/main.qml".to_string(),
                "/usr/lib/qt6/plugins/kwin/effects/plugins/smodglow.so".to_string(),
            ],
        )];
        let p = aero_parts(&files, &|_| None);
        assert_eq!(p.applets, vec!["a.b"]);
        assert_eq!(p.effects, vec![EffectModule { file_name: "smodglow.so".into(), id: "smodglow".into() }]);
    }

    // ── decision ──

    #[test]
    fn not_installed_without_the_shell_package() {
        let r = report(vec![pkg(KWINC, Marker::KwinEffect, Some("6.7.4"), Some("6.8.1"), &[])]);
        assert_eq!(decide(&r, &[], &parts()), Verdict::NotInstalled);
        assert_eq!(decide(&Report::empty(), &[], &parts()), Verdict::NotInstalled);
    }

    #[test]
    fn healthy_when_everything_fits() {
        let r = report(vec![
            pkg(SHELL, Marker::None, None, None, &[]),
            pkg(KWINC, Marker::KwinEffect, Some("6.8.1"), Some("6.8.1"), &[]),
        ]);
        assert_eq!(decide(&r, &[], &parts()), Verdict::Healthy);
    }

    #[test]
    fn effect_version_mismatch_turns_off_effects_only() {
        let r = report(vec![
            pkg(SHELL, Marker::None, None, None, &[]),
            pkg(KWINC, Marker::KwinEffect, Some("6.7.4"), Some("6.8.1"), &[]),
        ]);
        match decide(&r, &[], &parts()) {
            Verdict::Broken { effects: true, shell: false, why } => assert!(why[0].contains("6.7.4"), "{why:?}"),
            v => panic!("{v:?}"),
        }
    }

    #[test]
    fn unresolved_symbol_in_plasmoid_is_shell_side() {
        let r = report(vec![pkg(SHELL, Marker::None, None, None, &["libseventasks.so: undefined symbol: _ZN6Plasma3FooEv"])]);
        assert!(matches!(decide(&r, &[], &parts()), Verdict::Broken { effects: false, shell: true, .. }));
        let r = report(vec![
            pkg(SHELL, Marker::None, None, None, &[]),
            pkg(KWINC, Marker::KwinEffect, Some("6.8.1"), Some("6.8.1"), &["smodglow.so: libkwin.so.6: not found"]),
        ]);
        assert!(matches!(decide(&r, &[], &parts()), Verdict::Broken { effects: true, shell: false, .. }));
    }

    #[test]
    fn stale_libplasma_fork_is_shell_side() {
        let r = report(vec![
            pkg(SHELL, Marker::None, None, None, &[]),
            pkg(LIBP, Marker::PlasmaLibrary, Some("6.7.4"), Some("6.8.1"), &[]),
        ]);
        assert!(matches!(decide(&r, &[], &parts()), Verdict::Broken { effects: false, shell: true, .. }));
    }

    #[test]
    fn plasmashell_ldd_counts_only_with_the_fork() {
        let shell_bad = vec!["undefined symbol: _ZN6Plasma6Applet3fooEv".to_string()];
        let with_fork = report(vec![pkg(SHELL, Marker::None, None, None, &[]), pkg(LIBP, Marker::PlasmaLibrary, Some("6.8.1"), Some("6.8.1"), &[])]);
        assert!(matches!(decide(&with_fork, &shell_bad, &parts()), Verdict::Broken { shell: true, .. }));
        // Without the fork the trouble is not in the suite: only a notification.
        let no_fork = report(vec![pkg(SHELL, Marker::None, None, None, &[])]);
        assert!(matches!(decide(&no_fork, &shell_bad, &parts()), Verdict::NoticeOnly { .. }));
    }

    #[test]
    fn private_qt_risk_alone_is_notice_only() {
        let r = report(vec![pkg(
            SHELL,
            Marker::QtVersion,
            Some("6.11"),
            Some("6.12"),
            &["libnotifications.so: Qt 6.11 private API, installed Qt 6.12"],
        )]);
        assert!(r.at_risk_only());
        assert!(matches!(decide(&r, &[], &parts()), Verdict::NoticeOnly { .. }));
    }

    // ── kwinrc ──

    #[test]
    fn disable_effects_saves_previous_values_and_skips_already_off() {
        let kwinrc = "[Compositing]\nBackend=OpenGL\n\n[Plugins]\nsmodglowEnabled=true\nfooEnabled=false\naeroblurEnabled=false\n";
        let mut saved = Vec::new();
        let ids = vec!["smodglow".to_string(), "aeroblur".to_string(), "smodsnap".to_string()];
        let out = disable_effects(Some(kwinrc), &ids, &mut saved).unwrap();
        assert_eq!(ini_get(&out, "Plugins", "smodglowEnabled").as_deref(), Some("false"));
        assert_eq!(ini_get(&out, "Plugins", "smodsnapEnabled").as_deref(), Some("false"));
        assert_eq!(ini_get(&out, "Plugins", "fooEnabled").as_deref(), Some("false"));
        assert!(out.contains("Backend=OpenGL"));
        assert_eq!(
            saved,
            vec![
                SavedKey { key: "smodglowEnabled".into(), previous: Some("true".into()) },
                SavedKey { key: "smodsnapEnabled".into(), previous: None },
            ]
        );
        // A repeat on already disabled keys: nothing to change, the copy is untouched.
        let before = saved.clone();
        assert_eq!(disable_effects(Some(&out), &ids, &mut saved), None);
        assert_eq!(saved, before);
    }

    #[test]
    fn restore_effects_returns_only_our_false() {
        let kwinrc = "[Plugins]\nsmodglowEnabled=false\nsmodsnapEnabled=false\naeroblurEnabled=true\n";
        let saved = vec![
            SavedKey { key: "smodglowEnabled".into(), previous: Some("true".into()) },
            SavedKey { key: "smodsnapEnabled".into(), previous: None },
            SavedKey { key: "aeroblurEnabled".into(), previous: Some("false".into()) },
        ];
        let (out, kept) = restore_effects(Some(kwinrc), &saved);
        let out = out.unwrap();
        assert_eq!(ini_get(&out, "Plugins", "smodglowEnabled").as_deref(), Some("true"));
        assert_eq!(ini_get(&out, "Plugins", "smodsnapEnabled"), None);
        // The person enabled it themselves — left alone, and this is reported.
        assert_eq!(ini_get(&out, "Plugins", "aeroblurEnabled").as_deref(), Some("true"));
        assert_eq!(kept, vec!["aeroblurEnabled"]);
    }

    // ── layout ──

    const APPLETSRC: &str = "[ActionPlugins][0]\nRightButton;NoModifier=org.kde.contextmenu\n\n\
[Containments][1]\nactivityId=abc-123\nformfactor=0\nimmutability=1\nlastScreen=0\nlocation=0\nplugin=org.kde.desktopcontainment\nwallpaperplugin=org.kde.image\n\n\
[Containments][1][Applets][9]\nplugin=io.gitgud.wackyideas.SevenStart\n\n\
[Containments][1][Applets][10]\nplugin=org.kde.plasma.notes\n\n\
[Containments][1][Wallpaper][org.kde.image][General]\nImage=file:///home/u/win7.jpg\n\n\
[Containments][2]\nactivityId=\nformfactor=2\nlocation=4\nplugin=org.kde.panel\n\n\
[Containments][2][Applets][3]\nplugin=io.gitgud.wackyideas.SevenStart\n\n\
[Containments][2][Applets][4]\nplugin=io.gitgud.wackyideas.seventasks\n\n\
[Containments][2][General]\nAppletOrder=3;4\n\n\
[ScreenMapping]\nitemsOnDisabledScreens=\n";

    #[test]
    fn stock_layout_keeps_desktop_and_wallpaper_and_replaces_panels() {
        let a = parts().applets;
        let out = stock_layout(APPLETSRC, &a).unwrap();
        assert!(out.contains("[Containments][1]\nactivityId=abc-123"), "{out}");
        assert!(out.contains("Image=file:///home/u/win7.jpg"));
        assert!(out.contains("[Containments][1][Applets][10]\nplugin=org.kde.plasma.notes"));
        assert!(!out.contains("wackyideas"), "{out}");
        assert!(!out.contains("[Containments][2]"));
        assert!(out.contains("[ActionPlugins][0]") && out.contains("[ScreenMapping]"));
        // The new panel goes after the highest number (10).
        assert!(out.contains("[Containments][11]\nactivityId=\nformfactor=2"), "{out}");
        assert!(out.contains("plugin=org.kde.panel"));
        assert!(out.contains("[Containments][11][Applets][12]\nimmutability=1\nplugin=org.kde.plasma.kickoff"));
        assert!(out.contains("AppletOrder=12;13;14;15;16;17"));
        assert!(out.contains("SystrayContainmentId=18") && out.contains("plugin=org.kde.plasma.private.systemtray"));
        // Without the suite's widgets there is nothing to change.
        assert_eq!(stock_layout(&out, &a), None);
    }

    #[test]
    fn stock_panel_skips_applets_the_set_overrides() {
        let mut a = parts().applets;
        a.push("org.kde.plasma.systemtray".into());
        let out = stock_layout(APPLETSRC, &a).unwrap();
        assert!(!out.contains("plugin=org.kde.plasma.systemtray"));
        assert!(!out.contains("SystrayContainmentId"));
        assert!(out.contains("AppletOrder=12;13;14;15;16"));
    }

    // ── rollback and restore on a temporary directory ──

    fn plan() -> FallbackPlan {
        FallbackPlan::whole(&parts())
    }

    #[test]
    fn fallback_then_restore_round_trips_files_exactly() {
        let t = Tmp::new("roundtrip");
        let p = t.paths();
        let kwinrc = "[Plugins]\nsmodglowEnabled=true\nblurEnabled=false\n";
        put(&p.kwinrc(), kwinrc);
        put(&p.appletsrc(), APPLETSRC);

        let done = fall_back_to_stock(&p, &plan(), FallbackCause::Mismatch).unwrap();
        assert_eq!(done.effects_off, vec!["smodglowEnabled", "aeroblurEnabled"]);
        assert!(done.layout_replaced);
        assert_eq!(read_text(&p.saved_appletsrc()).as_deref(), Some(APPLETSRC));
        assert_eq!(read_text(&p.saved_kwinrc()).as_deref(), Some(kwinrc));
        let now = read_text(&p.kwinrc()).unwrap();
        assert_eq!(ini_get(&now, "Plugins", "smodglowEnabled").as_deref(), Some("false"));
        assert!(!read_text(&p.appletsrc()).unwrap().contains("wackyideas"));

        let back = restore_saved(&p, RestoreScope::All).unwrap();
        assert!(back.had_saved && back.layout && back.kept.is_empty());
        assert_eq!(read_text(&p.appletsrc()).as_deref(), Some(APPLETSRC));
        let k = read_text(&p.kwinrc()).unwrap();
        assert_eq!(ini_get(&k, "Plugins", "smodglowEnabled").as_deref(), Some("true"));
        assert_eq!(ini_get(&k, "Plugins", "aeroblurEnabled"), None);
        assert_eq!(ini_get(&k, "Plugins", "blurEnabled").as_deref(), Some("false"));
        assert!(!p.saved_json().exists() && !p.saved_appletsrc().exists() && !p.saved_kwinrc().exists());

        // A second restore touches nothing.
        let again = restore_saved(&p, RestoreScope::All).unwrap();
        assert!(!again.had_saved);
        assert_eq!(read_text(&p.appletsrc()).as_deref(), Some(APPLETSRC));
    }

    #[test]
    fn repeated_fallback_never_overwrites_the_copy() {
        let t = Tmp::new("repeat");
        let p = t.paths();
        put(&p.kwinrc(), "[Plugins]\nsmodglowEnabled=true\n");
        put(&p.appletsrc(), APPLETSRC);
        fall_back_to_stock(&p, &plan(), FallbackCause::Mismatch).unwrap();
        let saved1 = load_saved(&p).unwrap();

        // Second login, the suite is still broken; meanwhile Plasma rebuilt
        // the layout from the package (again with Aero widgets) — the copy of
        // the person's layout stays the same.
        let regenerated = APPLETSRC.replace("abc-123", "regenerated");
        put(&p.appletsrc(), &regenerated);
        let done = fall_back_to_stock(&p, &plan(), FallbackCause::Mismatch).unwrap();
        assert!(done.effects_off.is_empty());
        assert!(done.layout_replaced);
        assert_eq!(read_text(&p.saved_appletsrc()).as_deref(), Some(APPLETSRC));
        let saved2 = load_saved(&p).unwrap();
        assert_eq!(saved2.effects, saved1.effects);

        // Third login without changes — the files are not touched at all.
        let stock = read_text(&p.appletsrc()).unwrap();
        let done = fall_back_to_stock(&p, &plan(), FallbackCause::Mismatch).unwrap();
        assert!(!done.changed());
        assert_eq!(read_text(&p.appletsrc()).unwrap(), stock);

        restore_saved(&p, RestoreScope::All).unwrap();
        assert_eq!(read_text(&p.appletsrc()).as_deref(), Some(APPLETSRC));
        assert_eq!(ini_get(&read_text(&p.kwinrc()).unwrap(), "Plugins", "smodglowEnabled").as_deref(), Some("true"));
    }

    #[test]
    fn layout_changed_during_fallback_is_kept_aside() {
        let t = Tmp::new("aside");
        let p = t.paths();
        put(&p.appletsrc(), APPLETSRC);
        fall_back_to_stock(&p, &plan(), FallbackCause::Mismatch).unwrap();
        let tweaked = format!("{}\n[Containments][99]\nplugin=org.kde.panel\n", read_text(&p.appletsrc()).unwrap());
        put(&p.appletsrc(), &tweaked);
        restore_saved(&p, RestoreScope::All).unwrap();
        assert_eq!(read_text(&p.appletsrc()).as_deref(), Some(APPLETSRC));
        assert_eq!(read_text(&p.during_fallback_appletsrc()).as_deref(), Some(tweaked.as_str()));
    }

    #[test]
    fn effects_only_restore_leaves_layout_for_next_login() {
        let t = Tmp::new("effects-only");
        let p = t.paths();
        put(&p.kwinrc(), "[Plugins]\nsmodglowEnabled=true\n");
        put(&p.appletsrc(), APPLETSRC);
        fall_back_to_stock(&p, &plan(), FallbackCause::Mismatch).unwrap();
        let done = restore_saved(&p, RestoreScope::EffectsOnly).unwrap();
        assert!(done.layout_pending && !done.layout);
        assert_eq!(ini_get(&read_text(&p.kwinrc()).unwrap(), "Plugins", "smodglowEnabled").as_deref(), Some("true"));
        assert!(p.saved_appletsrc().exists());
        let done = restore_saved(&p, RestoreScope::All).unwrap();
        assert!(done.layout);
        assert_eq!(read_text(&p.appletsrc()).as_deref(), Some(APPLETSRC));
        assert!(!p.saved_json().exists());
    }

    #[test]
    fn nothing_to_change_leaves_no_state() {
        let t = Tmp::new("nothing");
        let p = t.paths();
        put(&p.kwinrc(), "[Plugins]\nsmodglowEnabled=false\naeroblurEnabled=false\n");
        put(&p.appletsrc(), "[Containments][1]\nplugin=org.kde.desktopcontainment\n");
        let done = fall_back_to_stock(&p, &plan(), FallbackCause::Mismatch).unwrap();
        assert!(!done.changed());
        assert!(!p.saved_json().exists());
        assert!(!restore_saved(&p, RestoreScope::All).unwrap().had_saved);
    }

    // ── the whole guard ──

    fn broken() -> Verdict {
        Verdict::Broken { effects: true, shell: true, why: vec!["x".into()] }
    }

    #[test]
    fn guard_falls_back_then_restores_when_check_fits_again() {
        let t = Tmp::new("guard");
        let p = t.paths();
        put(&p.kwinrc(), "[Plugins]\nsmodglowEnabled=true\n");
        put(&p.appletsrc(), APPLETSRC);
        let o = guard_with(&p, broken(), &parts(), "fp");
        assert!(matches!(o.notice, Some(PendingNotice::TurnedOff { effects: true, layout: true, .. })), "{o:?}");
        // A repeated login is still broken — remind again, do not touch the copy.
        let o = guard_with(&p, broken(), &parts(), "fp");
        assert!(matches!(o.notice, Some(PendingNotice::TurnedOff { .. })));
        assert_eq!(read_text(&p.saved_appletsrc()).as_deref(), Some(APPLETSRC));
        let o = guard_with(&p, Verdict::Healthy, &parts(), "fp");
        assert_eq!(o.notice, Some(PendingNotice::Restored));
        assert_eq!(read_text(&p.appletsrc()).as_deref(), Some(APPLETSRC));
        // And silence after that.
        assert_eq!(guard_with(&p, Verdict::Healthy, &parts(), "fp").notice, None);
    }

    #[test]
    fn guard_does_nothing_when_not_installed() {
        let t = Tmp::new("absent");
        let p = t.paths();
        put(&p.appletsrc(), APPLETSRC);
        request_fallback(&p, "crash").unwrap();
        let o = guard_with(&p, Verdict::NotInstalled, &parts(), "fp");
        assert_eq!(o.notice, None);
        assert_eq!(read_text(&p.appletsrc()).as_deref(), Some(APPLETSRC));
    }

    #[test]
    fn notice_only_does_not_touch_files_and_speaks_once() {
        let t = Tmp::new("risk");
        let p = t.paths();
        put(&p.kwinrc(), "[Plugins]\nsmodglowEnabled=true\n");
        put(&p.appletsrc(), APPLETSRC);
        let v = Verdict::NoticeOnly { why: vec!["risk".into()] };
        assert!(matches!(guard_with(&p, v.clone(), &parts(), "fp1").notice, Some(PendingNotice::Risk { .. })));
        assert_eq!(guard_with(&p, v.clone(), &parts(), "fp1").notice, None);
        assert!(guard_with(&p, v, &parts(), "fp2").notice.is_some());
        assert_eq!(read_text(&p.appletsrc()).as_deref(), Some(APPLETSRC));
        assert_eq!(read_text(&p.kwinrc()).as_deref(), Some("[Plugins]\nsmodglowEnabled=true\n"));
        assert!(!p.saved_json().exists());
    }

    #[test]
    fn requested_fallback_is_not_undone_by_a_fitting_check_until_rebuilt() {
        let t = Tmp::new("requested");
        let p = t.paths();
        put(&p.kwinrc(), "[Plugins]\nsmodglowEnabled=true\n");
        put(&p.appletsrc(), APPLETSRC);
        request_fallback(&p, "KWin crashed 3 times").unwrap();
        let o = guard_with(&p, Verdict::Healthy, &parts(), "fp");
        match o.notice {
            Some(PendingNotice::TurnedOff { reason: Some(r), .. }) => assert_eq!(r, "KWin crashed 3 times"),
            n => panic!("{n:?}"),
        }
        assert!(!p.request_file().exists());
        assert_eq!(guard_with(&p, Verdict::Healthy, &parts(), "fp").notice, None);
        assert!(p.saved_appletsrc().exists());
        mark_rebuilt(&p).unwrap();
        assert_eq!(guard_with(&p, Verdict::Healthy, &parts(), "fp").notice, Some(PendingNotice::Restored));
        assert_eq!(read_text(&p.appletsrc()).as_deref(), Some(APPLETSRC));
    }

    #[test]
    fn effects_only_breakage_keeps_the_layout() {
        let t = Tmp::new("effects-broken");
        let p = t.paths();
        put(&p.appletsrc(), APPLETSRC);
        let v = Verdict::Broken { effects: true, shell: false, why: vec![] };
        guard_with(&p, v, &parts(), "fp");
        assert_eq!(read_text(&p.appletsrc()).as_deref(), Some(APPLETSRC));
        let k = read_text(&p.kwinrc()).unwrap();
        assert_eq!(ini_get(&k, "Plugins", "smodglowEnabled").as_deref(), Some("false"));
    }

    // ── env script ──

    #[test]
    fn env_script_text_is_thin_and_safe_to_source() {
        let s = env_script("/usr/bin/rexon", "/home/u/.config/plasma-workspace/env/rexon-session-guard.sh");
        assert!(s.contains("if [ -x '/usr/bin/rexon' ]; then"));
        assert!(s.contains("timeout -k 1 3 '/usr/bin/rexon' --session-guard </dev/null >/dev/null 2>&1 || true"));
        assert!(s.contains("rm -f '/home/u/.config/plasma-workspace/env/rexon-session-guard.sh'"));
        // Runs in startplasma's shared shell: no exit, no set -e,
        // no environment variables.
        assert!(!s.contains("exit") && !s.contains("set -") && !s.contains("export"));
        let q = env_script("/opt/it's/rexon", "/x");
        assert!(q.contains(r"'/opt/it'\''s/rexon'"));
    }

    #[test]
    fn env_script_runs_sourced_without_breaking_the_shell() {
        if crate::utils::which("sh", None).is_none() {
            return;
        }
        let t = Tmp::new("script");
        let bin = t.0.join("rexon");
        let mark = t.0.join("ran");
        put(&bin, &format!("#!/bin/sh\necho \"$@\" > '{}'\n", mark.display()));
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        let script = t.0.join("env/rexon-session-guard.sh");
        put(&script, &env_script(&bin.to_string_lossy(), &script.to_string_lossy()));
        let run = |s: &Path| {
            std::process::Command::new("sh")
                .arg("-c")
                .arg(format!(". '{}'; echo alive", s.display()))
                .output()
                .unwrap()
        };
        let o = run(&script);
        assert_eq!(String::from_utf8_lossy(&o.stdout), "alive\n");
        assert_eq!(read_text(&mark).as_deref(), Some("--session-guard\n"));
        // The binary is gone — the script removes itself and still does not break the shell.
        std::fs::remove_file(&bin).unwrap();
        let o = run(&script);
        assert_eq!(String::from_utf8_lossy(&o.stdout), "alive\n");
        assert!(!script.exists());
    }

    #[test]
    fn env_script_sync_writes_keeps_and_removes() {
        let t = Tmp::new("sync");
        let p = t.paths();
        assert!(matches!(sync_env_script(&p, true, Some("/usr/bin/rexon")).unwrap(), ScriptChange::Write(_)));
        assert!(p.env_script().exists());
        assert_eq!(sync_env_script(&p, true, Some("/usr/bin/rexon")).unwrap(), ScriptChange::Nothing);
        assert_eq!(sync_env_script(&p, false, Some("/usr/bin/rexon")).unwrap(), ScriptChange::Remove);
        assert!(!p.env_script().exists());
        assert_eq!(sync_env_script(&p, false, None).unwrap(), ScriptChange::Nothing);
        // Nothing to call — the script is not installed.
        assert_eq!(sync_env_script(&p, true, None).unwrap(), ScriptChange::Nothing);
        assert_eq!(sync_env_script(&p, true, Some("rexon")).unwrap(), ScriptChange::Nothing);
    }

    #[test]
    fn plasma_session_detection() {
        assert!(plasma_session(None));
        assert!(plasma_session(Some("KDE")));
        assert!(plasma_session(Some("")));
        assert!(!plasma_session(Some("MATE")));
        assert!(!plasma_session(Some("GNOME:Unity")));
    }

    #[test]
    fn cli_route_ignores_foreign_flags() {
        assert_eq!(cli_route(&["--section=settings".to_string()], &mut |_| {}), None);
    }

    #[test]
    fn texts_are_plain() {
        assert!(turned_off_body(None).contains("standard Plasma look"));
        assert!(turned_off_body(Some("KWin crashed")).contains("KWin crashed"));
        let d = turned_off_details(true, true, &["a".into()]);
        assert!(d.contains(RESTORE_FLAG) && d.contains("• a"));
        assert!(restore_report(&RestoreDone::default()).starts_with("Nothing to restore"));
        assert!(after_rebuild_message(true).contains("next login"));
        assert!(next_login_body("“x” made the Plasma desktop crash").contains("“x” made the Plasma desktop crash"));
        let n = PendingNotice::NextLogin { reason: "r".into() };
        assert_eq!(serde_json::from_str::<PendingNotice>(&serde_json::to_string(&n).unwrap()).unwrap(), n);
    }

    // ── uninstalling Rexon during a rollback ──

    const LAYOUT: &str = "[Containments][1]\nformfactor=0\nlocation=0\nplugin=org.kde.desktopcontainment\n\n\
[Containments][2]\nformfactor=2\nlocation=4\nplugin=org.kde.panel\n\n[Containments][2][Applets][3]\nplugin=io.gitgud.wackyideas.SevenStart\n";

    #[test]
    fn uninstall_without_fallback_only_removes_the_script() {
        let t = Tmp::new("un-none");
        let p = t.paths();
        put(&p.env_script(), "guard");
        put(&p.request_file(), "KWin crashed");
        let r = release_for_uninstall(&p).unwrap();
        assert!(!r.restored.had_saved && !r.layout_next_login);
        assert!(!p.env_script().exists() && !p.request_file().exists());
        // Neither a script nor copies — a repeat breaks nothing either.
        assert_eq!(release_for_uninstall(&p).unwrap(), UninstallRelease::default());
    }

    #[test]
    fn uninstall_during_fallback_restores_effects_now_and_layout_at_next_login() {
        let t = Tmp::new("un-active");
        let p = t.paths();
        put(&p.kwinrc(), "[Plugins]\nsmodglowEnabled=true\n");
        put(&p.appletsrc(), LAYOUT);
        put(&p.env_script(), "guard");
        let plan = FallbackPlan::whole(&parts());
        let done = fall_back_to_stock(&p, &plan, FallbackCause::Requested { reason: "x".into() }).unwrap();
        assert!(done.layout_replaced);
        let stock = std::fs::read_to_string(p.appletsrc()).unwrap();

        let r = release_for_uninstall(&p).unwrap();
        assert!(r.restored.had_saved && r.layout_next_login, "{r:?}");
        let kwinrc = std::fs::read_to_string(p.kwinrc()).unwrap();
        assert!(kwinrc.contains("smodglowEnabled=true") && !kwinrc.contains("aeroblurEnabled"), "{kwinrc}");
        // Mid-session the layout is untouched; the script without Rexon will restore it.
        assert_eq!(std::fs::read_to_string(p.appletsrc()).unwrap(), stock);
        let script = std::fs::read_to_string(p.env_script()).unwrap();
        assert_eq!(script, restore_once_script(&p));
        assert!(!script.contains("exit") && !script.contains("set -e") && !script.contains("rexon --"));

        // Next login: startplasma runs the file via `.`.
        let st = std::process::Command::new("sh")
            .arg("-c")
            .arg(format!(". {} ; echo still-running", sh_quote(&p.env_script().to_string_lossy())))
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&st.stdout).trim(), "still-running", "сценарий не обрывает оболочку");
        assert_eq!(std::fs::read_to_string(p.appletsrc()).unwrap(), LAYOUT, "раскладка человека вернулась");
        assert_eq!(std::fs::read_to_string(p.during_fallback_appletsrc()).unwrap(), stock);
        assert!(!p.env_script().exists() && !p.saved_appletsrc().exists() && !p.saved_json().exists());
    }
}