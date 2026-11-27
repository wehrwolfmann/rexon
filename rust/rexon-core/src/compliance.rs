//! Compliance Check — checking and auto-fixing DE settings, port of
//! `rexon/compliance.py` (the `ComplianceCheck` class).
//!
//! On Rexon start / DE switch: the current DE is detected, a set of checks is run
//! for it (packages, gsettings, files, mimeapps), and
//! mismatches are fixed automatically. 8 DEs are supported: MATE, KDE, GNOME,
//! COSMIC, XFCE, Cinnamon, Budgie, Hyprland.
//!
//! The porting principle (as in `system_plan.rs` / `importers.rs`): all PURE logic is
//! deterministic input→output functions (check tables, state parsing,
//! building argv, the "does it need fixing" decision, generating settings.ini and
//! the layouts bash script). Side effects are NOT executed here: every
//! `safe_run`/`shutil`/`os`/`open`/`tempfile` of the original is exactly one method
//! of the [`ComplianceIo`] trait. In parity tests the trait replays a log of live
//! Python; in the live bridge — real I/O. Observations (a gsettings value,
//! package presence, file contents, helper presence) come from the trait
//! or as a parameter.
//!
//! BYTE-FOR-BYTE parity with Python: the same argv (including `pkexec …`), the same
//! check order, the same log text. The references were taken from live Python
//! read-only (`tests/golden/compliance_cases.json`); nothing privileged was
//! executed.
//!
//! Preserved quirks of the original (a deliberate divergence kept for
//! parity):
//!   * `_check_mate` returns `mate_terminal="failed"` / `mate_cursor="failed"`
//!     when `gsettings get == None`, but on `== None` for the cc locale it is already
//!     `mate_cc_locale="failed"` — the "gsettings unavailable" logic is smeared,
//!     kept as is;
//!   * `_check_xfce` ALWAYS gives `xfce_helpers="ok"` for helpers.rc (even
//!     when `TerminalEmulator=` is not found or there is no file) — only the log text
//!     changes; kept;
//!   * `_patch_settings_ini`: if there was no `[Settings]` section, it is
//!     inserted at the start, then `key=value` at index `insert_at+1`
//!     (Python's `list.insert` clamps going past the length) — the arithmetic is kept;
//!   * `detect_current_de` matches the first part of `XDG_CURRENT_DESKTOP` (split
//!     by `:`) found in the table — the order of parts determines the result.
//!
//! Autostart mode (`allow_privileged = false`): at login Rexon
//! has no right to ask for a password or go online. Diagnostics still
//! run fully (we honestly look at what is missing), while the fixing itself —
//! installing packages, the AUR helper, `git clone`, copying into system
//! directories via the helper/`pkexec` — is postponed: the step gets the status
//! `needs_action` and ONE log line for the whole run. From the interface the check
//! is called with `allow_privileged = true` and fixes as before.
//!
//! The package manager family gate: package names in the checks are Arch ones
//! (`ttf-ubuntu-font-family`, `cosmic-wallpapers`, `ayatana-indicator-*`…),
//! and installation goes through `pacman`/an AUR helper. On any other family
//! such steps are NOT executed at all (neither package checks nor `pkexec`), they get
//! the status `skipped` and give ONE log line for the whole run. Non-package
//! checks (gsettings, config files, icon themes) work everywhere.
//! The family is passed as a parameter — like `pkg_helper_available`.
//!
//! NOT ported (execution — when hooked up to live Python): the actual
//! `subprocess.run`/`safe_run`, `shutil.which/copy2/rmtree`, reading/writing
//! `~/.config`, `tempfile.mkdtemp/mkstemp`, git clone of layouts,
//! `pkexec` — all expressed as trait calls. Helper presence
//! (`_pkg_helper_available`/`_stage_helper_available`) is passed as a parameter.

use crate::i18n::{n_, ntr, tr, trc, trf};
use crate::system::PkgFamily;

// ============================================================================
// Constant tables (order = as in Python, matters for the check/log order)
// ============================================================================

/// `ComplianceCheck.MATE_EXPECTED_LAYOUTS` — the 8 Ubuntu MATE layouts.
pub const MATE_EXPECTED_LAYOUTS: &[&str] = &[
    "familiar", "redmond", "cupertino", "mutiny",
    "pantheon", "contemporary", "netbook", "traditional",
];

/// `ComplianceCheck.AYATANA_PACKAGES` — the Ayatana stack.
pub const AYATANA_PACKAGES: &[&str] = &[
    "ayatana-indicator-application",
    "ayatana-indicator-session",
    "ayatana-indicator-datetime",
    "ayatana-indicator-sound",
    "ayatana-indicator-notifications",
];

/// `ComplianceCheck.MATE_AUR_PACKAGES` — core + appmenu.
///
/// ⛔ No `brisk-menu` (Wehrwolfmann's decision of 24.11.2026: abandoned upstream):
/// a panel that still has it is moved to the MATE Menu by the last MATE step.
/// `mate-menu` is not in this list either: it is installed on its own (see
/// [`MATE_MENU_PACKAGE`]), so that its failure does not take the whole set down.
pub const MATE_AUR_PACKAGES: &[&str] = &[
    "mate-tweak",
    "mate-indicator-applet",
    // Not the shared `vala-panel-appmenu` — it pulls in budgie-desktop and vala-panel.
    "vala-panel-appmenu-mate",
    "vala-panel-appmenu-registrar",
];

/// The MATE Menu (`mate-menu`, AUR): the menu that replaces the Brisk menu on
/// the panel ([`Compliance::check_mate_brisk_menu`]).
pub const MATE_MENU_PACKAGE: &str = "mate-menu";

/// Packages the compliance fix installs the standard way (`install_pkg`
/// → the package helper): Ubuntu fonts (MATE), COSMIC wallpapers and icons,
/// Budgie extras. The helper's install allowlist
/// (`system_plan::pkg_helper_install_allowlist`) takes them from here; a new name in
/// `install_pkg` without a record here will be rejected by the helper — guarded by
/// a `debug_assert` in `install_pkg` itself.
pub const FIX_PACKAGES: &[&str] = &[
    "ttf-ubuntu-font-family", "cosmic-wallpapers", "cosmic-icon-theme", "budgie-extras",
];

/// `ComplianceCheck._ICON_FALLBACKS` — in descending order of preference per DE.
pub const ICON_FALLBACKS: &[(&str, &[&str])] = &[
    ("kde", &["breeze-dark", "breeze", "Papirus-Dark", "Adwaita"]),
    ("gnome", &["Adwaita", "Papirus-Dark"]),
    ("cinnamon", &["Mint-Y", "Adwaita", "Papirus-Dark"]),
    ("cinnamon-mint", &["Mint-Y", "Adwaita", "Papirus-Dark"]),
    ("mate", &["mate", "Adwaita", "Papirus-Dark"]),
    ("xfce", &["Adwaita", "Papirus-Dark"]),
    ("budgie", &["Adwaita", "Papirus-Dark"]),
    ("cosmic", &["Cosmic", "Adwaita", "Papirus-Dark"]),
    ("hyprland", &["Papirus-Dark", "Adwaita"]),
];

/// The default of `_ICON_FALLBACKS.get(de, ("Adwaita", "Papirus-Dark"))`.
const ICON_FALLBACKS_DEFAULT: &[&str] = &["Adwaita", "Papirus-Dark"];

/// Snap → Arch substitutions in a `.layout` (order matters) —
/// `_download_mate_layouts.snap_map`.
pub const MATE_LAYOUT_SNAP_MAP: &[(&str, &str)] = &[
    ("firefox_firefox.desktop", "firefox.desktop"),
    ("thunderbird_thunderbird.desktop", "thunderbird.desktop"),
    ("snap-store_snap-store.desktop", "org.gnome.Software.desktop"),
    ("snap-store_ubuntu-mate-welcome.desktop", "org.gnome.Software.desktop"),
    ("software-properties-drivers.desktop", "cachyos-hello.desktop"),
];

/// `_check_hyprland` — component packages. Order as in the Python dict.
const HYPRLAND_COMPONENTS: &[&str] = &[
    "hyprland", "waybar", "wofi", "kitty", "grim", "slurp", "wl-clipboard",
];

/// The Hyprland component label for the check log. A context is used because
/// "Terminal" and "Panel" are already in the catalog with another meaning (the desktop
/// comparison strings).
pub fn hyprland_component_label(pkg: &str) -> String {
    match pkg {
        "hyprland" => trc("Hyprland component", "Compositor"),
        "waybar" => trc("Hyprland component", "Panel"),
        "wofi" => trc("Hyprland component", "Launcher"),
        "kitty" => trc("Hyprland component", "Terminal"),
        "grim" => trc("Hyprland component", "Screenshots"),
        "slurp" => trc("Hyprland component", "Area selection"),
        "wl-clipboard" => trc("Hyprland component", "Clipboard"),
        other => other.to_string(),
    }
}

/// The status of a skipped step (not an "error" — just not our distribution).
pub const SKIPPED: &str = "skipped";

/// The single log line about skipping package steps on non-Arch —
/// without jargon, one for the whole check run.
pub fn pkg_unsupported_note() -> String {
    l2(tr("ℹ️ Checking and installing system packages is only available on \
Arch/CachyOS for now — all other settings were checked"))
}

/// The status of a step that needs installing: not an error and not "the distribution is not
/// supported", but "a button in the application needs pressing".
pub const NEEDS_ACTION: &str = "needs_action";

/// The single log line about postponed fixing at login.
/// The button label is the same core string written on the button itself.
pub fn privileged_deferred_note() -> String {
    let button = crate::qt_settings::text("compl.button", &[]).unwrap_or_default();
    l2(trf(
        // TRANSLATORS: %1 is the label of the “Check and Install Missing” button.
        "ℹ️ Some parts are missing — Rexon will install them when you click \
“%1” in Settings; we don't ask for a password at login",
        &[&button],
    ))
}

/// A log line with the step indent (two spaces).
fn l2(s: String) -> String {
    format!("  {s}")
}

/// A log line with the step detail indent (four spaces).
fn l4(s: String) -> String {
    format!("    {s}")
}

// ============================================================================
// The executor trait
// ============================================================================

/// The result of `safe_run(..., capture_output=True)`. `ok` = returncode == 0.
/// `None` from [`ComplianceIo::run`] — safe_run returned None (timeout/exception).
#[derive(Debug, Clone, PartialEq)]
pub struct RunResult {
    pub ok: bool,
    pub stdout: String,
}

/// An `os.scandir` entry: name + `entry.is_dir()`.
#[derive(Debug, Clone, PartialEq)]
pub struct ScanEntry {
    pub name: String,
    pub is_dir: bool,
}

/// The executor interface: each method is exactly one side call or
/// observation of the Python original, in execution order. In parity tests —
/// a log replay; in the live bridge — real I/O.
pub trait ComplianceIo {
    /// `safe_run(argv, capture_output=True[, timeout=t])`; None — safe_run
    /// returned None.
    fn run(&mut self, argv: &[String], timeout: Option<u64>) -> Option<RunResult>;
    /// `shutil.which(name) is not None`.
    fn which(&mut self, name: &str) -> bool;
    /// `os.environ.get(name, "")`.
    fn env(&mut self, name: &str) -> String;
    /// `os.path.exists`.
    fn exists(&mut self, path: &str) -> bool;
    /// `os.path.isfile`.
    fn is_file(&mut self, path: &str) -> bool;
    /// `os.path.isdir`.
    fn is_dir(&mut self, path: &str) -> bool;
    /// `open(path, "r", encoding="utf-8"[, errors=...]).read()`; None — OSError.
    fn read_text(&mut self, path: &str) -> Option<String>;
    /// `os.scandir(path)` → entries; None — OSError.
    fn scandir(&mut self, path: &str) -> Option<Vec<ScanEntry>>;
    /// `os.listdir(path)`.
    fn listdir(&mut self, path: &str) -> Vec<String>;
    /// `os.makedirs(path, exist_ok=True)`.
    fn makedirs(&mut self, path: &str);
    /// `open(path, "w").write(content)`.
    fn write_text(&mut self, path: &str, content: &str);
    /// `os.replace(src, dst)`.
    fn replace(&mut self, src: &str, dst: &str);
    /// `os.path.realpath(path)`.
    fn realpath(&mut self, path: &str) -> String;
    /// `tempfile.mkdtemp(prefix=prefix[, dir=dir])` → path.
    fn mkdtemp(&mut self, prefix: &str, dir: Option<&str>) -> String;
    /// `tempfile.mkstemp(prefix=prefix, suffix=suffix)` → path (the fd is closed
    /// by the executor).
    fn mkstemp(&mut self, prefix: &str, suffix: &str) -> String;
    /// `os.chmod(path, mode)`.
    fn chmod(&mut self, path: &str, mode: u32);
    /// `os.remove(path)`.
    fn remove_file(&mut self, path: &str);
    /// `shutil.rmtree(path, ignore_errors=True)`.
    fn remove_tree(&mut self, path: &str);
    /// `shutil.copy2(src, dst)`.
    fn copy_file(&mut self, src: &str, dst: &str);
    /// A root step: the operation `op` through the window's privilege session
    /// ([`crate::privilege::run`]; `files` — the operation's file paths), otherwise
    /// the old command `argv`. By default — the old command: checks without
    /// a window (login) and fakes live the old way.
    fn run_priv(
        &mut self,
        op: &crate::privilege::op::PrivOp,
        files: &[String],
        argv: &[String],
        timeout: Option<u64>,
    ) -> Option<RunResult> {
        let _ = (op, files);
        self.run(argv, timeout)
    }
}

/// Ordered check results `{check: status}` (insertion order =
/// Python dict order).
pub type Results = Vec<(String, String)>;

fn put(results: &mut Results, key: &str, status: &str) {
    // Python dict: a repeated key updates the value, the order is preserved.
    if let Some(e) = results.iter_mut().find(|(k, _)| k == key) {
        e.1 = status.to_string();
    } else {
        results.push((key.to_string(), status.to_string()));
    }
}

// ============================================================================
// Pure helpers
// ============================================================================

/// `os.path.expanduser` for our inputs (`~` / `~/…`) — the core's single
/// expander: the `~/.config/…` branch respects `$XDG_CONFIG_HOME`.
fn expanduser(home: &str, xdg_config_home: Option<&str>, p: &str) -> String {
    crate::utils::expand_user(p, home, xdg_config_home)
}

/// `str.strip(chars)` — strip any characters of the set from both ends.
fn strip_chars(s: &str, chars: &str) -> String {
    s.trim_matches(|c| chars.contains(c)).to_string()
}

/// `_ICON_DIRS` for the given HOME.
fn icon_dirs(home: &str) -> [String; 3] {
    [
        expanduser(home, None, "~/.local/share/icons"),
        expanduser(home, None, "~/.icons"),
        "/usr/share/icons".to_string(),
    ]
}

/// `detect_current_de` — by the value of `XDG_CURRENT_DESKTOP`.
///
/// There is no desktop table of its own here anymore: there is one for the whole product —
/// [`crate::system::detect_current_de`]. This also closed the hole due to which
/// tiling ones got no compliance checks: sway, niri and LXQt were missing from
/// the old table, and the function returned `None`.
pub fn detect_current_de(xdg_current_desktop: &str) -> Option<String> {
    crate::system::detect_current_de(xdg_current_desktop, "", false)
}

/// `_pkg_install_arch` — the argv for installing one package on Arch.
///
/// There is no hand-written fallback here anymore. It repeated `pkexec` + the
/// [`crate::system_plan::install_cmd`] table word for word, and such a copy lives
/// exactly until the first table edit: in a neighboring place of the same kind
/// (`wallpaper_engine`) the copy had already diverged — it lost `--needed`. The whole answer
/// is given by the engine ([`crate::system_plan::install_argv`]), which also decides
/// "the named org.rexon.pkg path or raw pkexec".
///
/// Arch here is not a narrowing of the engine but the subject of the compliance checks: the
/// fix paths in this module rely on pacman and AUR helpers and do not run at all on other
/// families ([`Compliance::pkg_steps_supported`]).
///
/// `None` — nothing to install (an empty package name).
pub fn pkg_install_arch(pkg: &str, pkg_helper_available: bool) -> Option<Vec<String>> {
    if pkg.is_empty() {
        return None;
    }
    crate::system_plan::install_argv(PkgFamily::Arch, &[pkg.to_string()], pkg_helper_available)
}

/// AUR helpers in order of preference. One point for the module and for
/// the completion executor (`system_de_exec`), which recognizes by these names
/// the fix command among the check's other launches.
pub const AUR_HELPERS: &[&str] = &["yay", "paru", "trizen", "pikaur"];

/// The argv for installing AUR packages — `_install_aur_packages`.
fn aur_install_argv(helper: &str, packages: &[String]) -> Vec<String> {
    crate::aur_install::install_argv(helper, packages, false)
}

/// Whether a `Directories=` line (non-empty) in a `.tex`-like index.theme is valid.
fn line_has_directories(s: &str) -> bool {
    s.starts_with("Directories=") && s != "Directories="
}

/// `_patch_settings_ini` — pure generation of the new settings.ini contents.
/// `existing` — the current file contents (None — no file / OSError).
pub fn compute_settings_ini(existing: Option<&str>, key: &str, value: &str) -> String {
    let mut lines: Vec<String> = match existing {
        Some(c) => split_lines(c),
        None => Vec::new(),
    };
    let mut has_section = false;
    let mut replaced = false;
    for line in lines.iter_mut() {
        let s = line.trim();
        if s == "[Settings]" {
            has_section = true;
        } else if has_section && s.starts_with(&format!("{key}=")) {
            *line = format!("{key}={value}");
            replaced = true;
            break;
        }
    }
    if !has_section {
        lines.insert(0, "[Settings]".to_string());
    }
    if !replaced {
        let insert_at = if lines.first().map(|l| l.trim()) == Some("[Settings]") {
            1
        } else {
            0
        };
        let idx = (insert_at + 1).min(lines.len());
        lines.insert(idx, format!("{key}={value}"));
    }
    format!("{}\n", lines.join("\n"))
}

/// `str.splitlines()` — by `\n` without a trailing empty element.
fn split_lines(s: &str) -> Vec<String> {
    // Python splitlines(): without a trailing empty segment.
    let mut out: Vec<String> = s.split('\n').map(|l| l.to_string()).collect();
    if s.ends_with('\n') {
        out.pop();
    }
    if out.len() == 1 && out[0].is_empty() && s.is_empty() {
        out.clear();
    }
    out
}

/// Apply the snap→Arch substitutions to a layout's text — `_download_mate_layouts`.
pub fn apply_snap_map(text: &str) -> String {
    let mut txt = text.to_string();
    for (snap_n, arch_n) in MATE_LAYOUT_SNAP_MAP {
        txt = txt.replace(
            &format!("/var/lib/snapd/desktop/applications/{snap_n}"),
            &format!("/usr/share/applications/{arch_n}"),
        );
    }
    txt
}

/// `shlex.quote` — minimal POSIX escaping (as in CPython).
fn shlex_quote(s: &str) -> String {
    if s.is_empty() {
        return "''".to_string();
    }
    let safe = s
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '@' | '%' | '_' | '-' | '+' | '=' | ':' | ',' | '.' | '/'));
    if safe {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\"'\"'"))
    }
}

/// The fallback script for copying layouts (the raw pkexec path) —
/// `_download_mate_layouts`. `staged_files` — `os.listdir(staged)`.
pub fn mate_layouts_bash_script(
    staged: &str,
    staged_files: &[String],
    layouts_dst: &str,
) -> String {
    let qdst = shlex_quote(layouts_dst);
    let mut slines: Vec<String> = vec![
        "#!/bin/bash".to_string(),
        "set -e".to_string(),
        format!("install -d {qdst}"),
    ];
    for f in staged_files {
        let src = shlex_quote(&join(staged, f));
        let dst = shlex_quote(&format!("{layouts_dst}/"));
        slines.push(format!("cp -f {src} {dst}"));
    }
    slines.push(format!("chmod -R a+rX {qdst}"));
    format!("{}\n", slines.join("\n"))
}

/// `os.path.join` (two-argument POSIX).
fn join(a: &str, b: &str) -> String {
    if b.starts_with('/') {
        b.to_string()
    } else if a.is_empty() || a.ends_with('/') {
        format!("{a}{b}")
    } else {
        format!("{a}/{b}")
    }
}

/// `os.path.basename`.
fn basename(p: &str) -> &str {
    match p.rfind('/') {
        Some(i) => &p[i + 1..],
        None => p,
    }
}

/// `os.path.dirname`.
fn dirname(p: &str) -> String {
    match p.rfind('/') {
        Some(0) => "/".to_string(),
        Some(i) => p[..i].to_string(),
        None => String::new(),
    }
}

// ============================================================================
// The main checks class
// ============================================================================

/// Package state for the checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PkgState {
    Installed,
    Missing,
    /// The check was not performed: the package manager family is not Arch, while
    /// the package names in the checks are Arch ones.
    Unsupported,
}

/// How a fix attempt ended (installing a package, AUR, completing
/// the layouts).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixOutcome {
    /// Fixed.
    Done,
    /// Tried and failed.
    Failed,
    /// Did not try: autostart is running, password and network are forbidden at login.
    Deferred,
}

impl FixOutcome {
    /// The step status for the results table.
    pub fn status(self) -> &'static str {
        match self {
            FixOutcome::Done => "fixed",
            FixOutcome::Failed => "failed",
            FixOutcome::Deferred => NEEDS_ACTION,
        }
    }
}

/// Whether a DE has a set of checks (`ComplianceCheck._DE_CHECKS`) — a free
/// function, so it can be asked without building a check context.
pub fn has_checks(de_id: &str) -> bool {
    matches!(
        de_id,
        "mate" | "kde" | "gnome" | "cosmic" | "xfce" | "cinnamon-mint" | "budgie"
            | "hyprland"
    )
}

/// What exactly is installed by the "Check and install missing" button —
/// a label for the person by check key.
///
/// The keys are exactly those the check can give [`NEEDS_ACTION`] to (everything
/// fixed with an administrator password or a download from the network). The question window
/// lists what is missing in these words, not by key names: the buyer must not
/// read `mate_aur_pkgs`. The completeness of the table is held by the check
/// `system_de_exec::tests::every_deferred_fix_has_a_label` — it runs
/// the checks of all desktops on an "empty" machine and demands a label for EVERY
/// postponed key, so a new fix without a label turns red immediately.
///
/// The labels are the original (English) catalog strings marked with `n_`;
/// [`fix_label`] translates them.
pub const FIX_LABELS: &[(&str, &str)] = &[
    ("mate_layouts", n_("MATE panel layouts (downloaded from the internet)")),
    ("mate_yaru_theme", n_("Yaru theme for MATE (from the AUR)")),
    ("mate_ubuntu_fonts", n_("Ubuntu fonts")),
    ("mate_aur_pkgs", n_("MATE panel add-ons and system tray icons (from the AUR)")),
    ("mate_menu", n_("MATE Menu in place of the Brisk menu (from the AUR)")),
    ("cosmic_wallpapers", n_("COSMIC wallpapers")),
    ("cosmic_icons", n_("COSMIC icons")),
    ("cinnamon_themes", n_("Linux Mint themes (from the AUR)")),
    ("budgie_extras", n_("Budgie add-ons")),
];

/// The label of what is missing by check key (in the interface language); `None` —
/// the key is not from the table.
pub fn fix_label(key: &str) -> Option<String> {
    FIX_LABELS.iter().find(|(k, _)| *k == key).map(|(_, v)| tr(v))
}

/// What is left to install after the check: keys with the status
/// [`NEEDS_ACTION`] in check order and their labels. A key without a label is not
/// lost — it is shown as is (the check will not let one appear).
pub fn deferred_fixes(results: &[(String, String)]) -> Vec<(String, String)> {
    results
        .iter()
        .filter(|(_, status)| status == NEEDS_ACTION)
        .map(|(key, _)| (key.clone(), fix_label(key).unwrap_or_else(|| key.clone())))
        .collect()
}

/// `run_for_current_de`: the current DE was not recognized by `XDG_CURRENT_DESKTOP`.
pub fn msg_unknown_de() -> String {
    tr("⚠️ Couldn't detect the current desktop")
}

/// The header of one DE in the shared run (`run_for_installed_des`): a `═` frame,
/// a line with the DE name and the frame again. A single point (rule #12) — the bridge
/// takes it too for the Python loop, so the logs of both paths do not drift apart.
pub fn de_banner_lines(de_id: &str) -> Vec<String> {
    let sep = "═".repeat(50);
    vec![format!("\n{sep}"), l2(trf("Desktop: %1", &[&de_id])), sep]
}

/// The check context: HOME + package manager family + permission for
/// privileged actions + presence of privileged helpers
/// (passed as parameters — `family`, `allow_privileged`,
/// `_pkg_helper_available` / `_stage_helper_available` of live Python).
pub struct Compliance<'a, IO: ComplianceIo> {
    pub io: &'a mut IO,
    pub home: String,
    /// `$XDG_CONFIG_HOME` (empty/None — the standard `~/.config`).
    pub xdg_config_home: Option<String>,
    /// The package manager family of the current distribution.
    pub pkg_family: PkgFamily,
    /// Whether fixing is allowed: installing packages, calling the AUR helper, cloning
    /// a repository, copying into system directories via `pkexec`.
    /// `false` — autostart at login: diagnostics only.
    pub allow_privileged: bool,
    pub pkg_helper_available: bool,
    pub stage_helper_available: bool,
    /// The line about skipping package steps has already been printed in this run.
    pkg_note_done: bool,
    /// The line about postponed fixing has already been printed in this run.
    deferred_note_done: bool,
    /// The terminal (`.desktop`) the person chose for the checked desktop in
    /// the "Settings" tab — from the desktop's own list marked "chosen by the
    /// person" ([`crate::defaults::chosen_terminal_in_list`]). The person's choice
    /// takes priority over the native terminal (Wehrwolfmann's decision of 08.11.2026):
    /// the terminal check puts it, not the native one, into the desktop's native place.
    chosen_terminal: Option<String>,
}

impl<'a, IO: ComplianceIo> Compliance<'a, IO> {
    pub fn new(
        io: &'a mut IO,
        home: &str,
        pkg_family: PkgFamily,
        allow_privileged: bool,
        pkg_helper_available: bool,
        stage_helper_available: bool,
    ) -> Self {
        Self::with_xdg(
            io,
            home,
            None,
            pkg_family,
            allow_privileged,
            pkg_helper_available,
            stage_helper_available,
        )
    }

    /// The same, but with an explicit `$XDG_CONFIG_HOME` (a live call from Python).
    #[allow(clippy::too_many_arguments)]
    pub fn with_xdg(
        io: &'a mut IO,
        home: &str,
        xdg_config_home: Option<&str>,
        pkg_family: PkgFamily,
        allow_privileged: bool,
        pkg_helper_available: bool,
        stage_helper_available: bool,
    ) -> Self {
        Compliance {
            io,
            home: home.to_string(),
            xdg_config_home: xdg_config_home.map(str::to_string),
            pkg_family,
            allow_privileged,
            pkg_helper_available,
            stage_helper_available,
            pkg_note_done: false,
            deferred_note_done: false,
            chosen_terminal: None,
        }
    }

    /// The person's choice from `<desktop>-xdg-terminals.list` (`desktop` — the raw
    /// `XDG_CURRENT_DESKTOP` or a Rexon identifier).
    fn read_chosen_terminal(&mut self, desktop: &str) -> Option<String> {
        let name = crate::appdefaults::terminals_list_filename(desktop)?;
        let path = self.expand(&format!("~/.config/{name}"));
        let text = self.io.read_text(&path)?;
        crate::defaults::chosen_terminal_in_list(&text)
    }

    /// Which terminal must be in the desktop's native place: the one chosen by the person
    /// (if installed) or the native `native`. Chosen but not installed
    /// — the native one, and the log says so.
    fn wanted_terminal(&mut self, native: &str, log: &mut Vec<String>) -> (String, bool) {
        let Some(chosen) = self.chosen_terminal.clone() else {
            return (native.to_string(), false);
        };
        let exec = crate::defaults::terminal_exec_name(&chosen);
        if self.io.which(&exec) {
            (exec, true)
        } else {
            log.push(l4(trf("ℹ️ Terminal: %1 (%2 isn't installed)", &[&native, &exec])));
            (native.to_string(), false)
        }
    }

    fn expand(&self, p: &str) -> String {
        expanduser(&self.home, self.xdg_config_home.as_deref(), p)
    }

    // ── The package manager family gate ─────────────────────────────────

    /// Whether package steps may run: package names and install commands
    /// are Arch ones, so only on Arch/CachyOS (and derivatives).
    pub fn pkg_steps_supported(&self) -> bool {
        self.pkg_family == PkgFamily::Arch
    }

    /// One clear line for the whole run about skipping package steps.
    fn note_pkg_unsupported(&mut self, log: &mut Vec<String>) {
        if !self.pkg_note_done {
            self.pkg_note_done = true;
            log.push(pkg_unsupported_note());
        }
    }

    /// Skip a package step: the mark (once) + the `skipped` status for
    /// each key of the step. A shared point for all DEs.
    fn skip_pkg_step<S: AsRef<str>>(
        &mut self,
        results: &mut Results,
        keys: &[S],
        log: &mut Vec<String>,
    ) {
        self.note_pkg_unsupported(log);
        for k in keys {
            put(results, k.as_ref(), SKIPPED);
        }
    }

    // ── The privileged actions gate (autostart at login) ────────────────

    /// One clear line for the whole run about postponed fixing.
    fn note_deferred(&mut self, log: &mut Vec<String>) {
        if !self.deferred_note_done {
            self.deferred_note_done = true;
            log.push(privileged_deferred_note());
        }
    }

    /// Whether fixing is allowed now (installing/copying as root, going online).
    /// If not — marks the run and returns `false`: the caller returns
    /// [`FixOutcome::Deferred`]. A single point for all kinds of fixes.
    fn may_fix(&mut self, log: &mut Vec<String>) -> bool {
        if self.allow_privileged {
            return true;
        }
        self.note_deferred(log);
        false
    }

    // ── Check utilities ─────────────────────────────────────────────────

    /// Package state. On non-Arch the command is NOT run at all — the single
    /// gate point for all checks (the package names in them are Arch ones).
    ///
    /// The command is given by the shared engine [`crate::system_plan::query_cmd`] — the same
    /// one used by `PackageManager.is_installed` and the check of
    /// the Plymouth theme provider package; the decision is the shared
    /// [`crate::system_plan::pkg_installed_from_run`]. Previously there was a
    /// hard-coded `pacman -Qi <pkg>` with its own outcome parsing here: a third copy
    /// of one and the same question. The answer does not change with the replacement — `pacman -Q`
    /// and `pacman -Qi` give the same return code both for an installed
    /// package and a missing one; the command output is not read here at all.
    ///
    /// An `Err` from the engine (the UNKNOWN family) is unreachable behind the gate, but
    /// is treated as "the check is unavailable", not as "not installed":
    /// inventing a package's absence because of an unfamiliar distribution is not allowed —
    /// it would lead to installing under pkexec right at login.
    pub fn pkg_state(&mut self, pkg: &str) -> PkgState {
        if !self.pkg_steps_supported() {
            return PkgState::Unsupported;
        }
        let Ok(argv) = crate::system_plan::query_cmd(self.pkg_family, pkg) else {
            return PkgState::Unsupported;
        };
        let ok = self.io.run(&argv, None).map(|r| r.ok);
        if crate::system_plan::pkg_installed_from_run(ok) {
            PkgState::Installed
        } else {
            PkgState::Missing
        }
    }

    fn is_pkg_installed(&mut self, pkg: &str) -> bool {
        self.pkg_state(pkg) == PkgState::Installed
    }

    /// Missing packages from the list; `None` — the check is unavailable
    /// (non-Arch).
    fn missing_pkgs(&mut self, pkgs: &[&str]) -> Option<Vec<String>> {
        if !self.pkg_steps_supported() {
            return None;
        }
        Some(
            pkgs.iter()
                .filter(|p| !self.is_pkg_installed(p))
                .map(|p| p.to_string())
                .collect(),
        )
    }

    fn find_aur_helper(&mut self) -> Option<&'static str> {
        AUR_HELPERS
            .iter()
            .copied()
            .find(|h| self.io.which(h))
    }

    /// Installing one package the standard Arch way — a single point instead of
    /// a scattering of `pkg_install_arch` + `io.run` across the checks.
    fn install_pkg(&mut self, pkg: &str, log: &mut Vec<String>) -> FixOutcome {
        if !self.pkg_steps_supported() {
            self.note_pkg_unsupported(log);
            return FixOutcome::Deferred;
        }
        if !self.may_fix(log) {
            return FixOutcome::Deferred;
        }
        debug_assert!(FIX_PACKAGES.contains(&pkg), "{pkg} не в FIX_PACKAGES");
        // An empty package name is a caller defect, not "nothing to do":
        // there is no command, nothing to run, the fix failed.
        let Some(argv) = pkg_install_arch(pkg, self.pkg_helper_available) else {
            return FixOutcome::Failed;
        };
        // The fix table — PkgSource::DesktopRepair (FIX_PACKAGES).
        let op = crate::privilege::op::PrivOp::PkgInstall {
            family: PkgFamily::Arch,
            packages: vec![pkg.to_string()],
            source: crate::privilege::op::PkgSource::DesktopRepair,
        };
        if matches!(self.io.run_priv(&op, &[], &argv, None), Some(r) if r.ok) {
            FixOutcome::Done
        } else {
            FixOutcome::Failed
        }
    }

    fn install_aur_packages(&mut self, packages: &[String], log: &mut Vec<String>) -> FixOutcome {
        if !self.pkg_steps_supported() {
            // Foolproofing: an AUR helper does not exist outside Arch.
            self.note_pkg_unsupported(log);
            return FixOutcome::Deferred;
        }
        if !self.may_fix(log) {
            return FixOutcome::Deferred;
        }
        let helper = match self.find_aur_helper() {
            Some(h) => h,
            None => {
                log.push(l4(tr("⚠️ yay/paru not found — install the packages manually")));
                return FixOutcome::Failed;
            }
        };
        log.push(l4(trf("📥 Installing with %1...", &[&helper])));
        let argv = aur_install_argv(helper, packages);
        if let Some(r) = self.io.run(&argv, None) {
            if r.ok {
                log.push(l4(trf("✅ Installed: %1", &[&packages.join(", ")])));
                return FixOutcome::Done;
            }
        }
        log.push(l4(tr("⚠️ Couldn't install (try installing manually)")));
        FixOutcome::Failed
    }

    fn gsettings_get(&mut self, schema: &str, key: &str) -> Option<String> {
        let argv = vec![
            "gsettings".to_string(),
            "get".to_string(),
            schema.to_string(),
            key.to_string(),
        ];
        match self.io.run(&argv, None) {
            Some(r) if r.ok => Some(r.stdout.trim().to_string()),
            _ => None,
        }
    }

    fn gsettings_set(&mut self, schema: &str, key: &str, value: &str) -> bool {
        let argv = vec![
            "gsettings".to_string(),
            "set".to_string(),
            schema.to_string(),
            key.to_string(),
            value.to_string(),
        ];
        matches!(self.io.run(&argv, None), Some(r) if r.ok)
    }

    // ── Icon theme validity ─────────────────────────────────────────────

    fn icon_theme_index_path(&mut self, name: &str) -> Option<String> {
        if name.is_empty() {
            return None;
        }
        for base in icon_dirs(&self.home) {
            let p = join(&join(&base, name), "index.theme");
            if self.io.is_file(&p) {
                return Some(p);
            }
        }
        None
    }

    fn is_valid_icon_theme(&mut self, name: &str) -> bool {
        let path = match self.icon_theme_index_path(name) {
            Some(p) => p,
            None => return false,
        };
        let content = match self.io.read_text(&path) {
            Some(c) => c,
            None => return false,
        };
        for line in content.split('\n') {
            let s = line.trim();
            if line_has_directories(s) {
                return true;
            }
        }
        false
    }

    fn pick_fallback_icon_theme(&mut self, de: &str) -> Option<String> {
        let names: Vec<String> = ICON_FALLBACKS
            .iter()
            .find(|(k, _)| *k == de)
            .map(|(_, v)| *v)
            .unwrap_or(ICON_FALLBACKS_DEFAULT)
            .iter()
            .map(|s| s.to_string())
            .collect();
        for name in names {
            if self.is_valid_icon_theme(&name) {
                return Some(name);
            }
        }
        // The last chance — any installed valid theme.
        for base in icon_dirs(&self.home) {
            if !self.io.is_dir(&base) {
                continue;
            }
            let entries = match self.io.scandir(&base) {
                Some(e) => e,
                None => continue,
            };
            for entry in entries {
                if entry.is_dir && self.is_valid_icon_theme(&entry.name) {
                    return Some(entry.name);
                }
            }
        }
        None
    }

    fn read_current_icon_theme(&mut self, de: &str) -> Option<String> {
        if matches!(de, "gnome" | "budgie" | "cosmic") {
            return self
                .gsettings_get("org.gnome.desktop.interface", "icon-theme")
                .map(|v| strip_chars(&v, "'\""));
        }
        if matches!(de, "cinnamon" | "cinnamon-mint") {
            return self
                .gsettings_get("org.cinnamon.desktop.interface", "icon-theme")
                .map(|v| strip_chars(&v, "'\""));
        }
        if de == "mate" {
            return self
                .gsettings_get("org.mate.interface", "icon-theme")
                .map(|v| strip_chars(&v, "'\""));
        }
        if de == "xfce" {
            let argv = vec![
                "xfconf-query".to_string(),
                "-c".to_string(),
                "xsettings".to_string(),
                "-p".to_string(),
                "/Net/IconThemeName".to_string(),
            ];
            return match self.io.run(&argv, None) {
                Some(r) if r.ok => Some(r.stdout.trim().to_string()),
                _ => None,
            };
        }
        if de == "kde" {
            let argv = vec![
                "kreadconfig6".to_string(),
                "--file".to_string(),
                "kdeglobals".to_string(),
                "--group".to_string(),
                "Icons".to_string(),
                "--key".to_string(),
                "Theme".to_string(),
                "--default".to_string(),
                "".to_string(),
            ];
            return match self.io.run(&argv, None) {
                Some(r) if r.ok => Some(r.stdout.trim().to_string()),
                _ => None,
            };
        }
        None
    }

    fn write_icon_theme(&mut self, de: &str, theme: &str) -> bool {
        let mut ok = false;
        if matches!(de, "gnome" | "budgie" | "cosmic") {
            ok = self.gsettings_set("org.gnome.desktop.interface", "icon-theme", theme);
        } else if matches!(de, "cinnamon" | "cinnamon-mint") {
            ok = self.gsettings_set("org.cinnamon.desktop.interface", "icon-theme", theme);
        } else if de == "mate" {
            ok = self.gsettings_set("org.mate.interface", "icon-theme", theme);
        } else if de == "xfce" {
            let argv = vec![
                "xfconf-query".to_string(),
                "-c".to_string(),
                "xsettings".to_string(),
                "-p".to_string(),
                "/Net/IconThemeName".to_string(),
                "-s".to_string(),
                theme.to_string(),
            ];
            ok = matches!(self.io.run(&argv, None), Some(r) if r.ok);
        } else if de == "kde" {
            let argv = vec![
                "kwriteconfig6".to_string(),
                "--file".to_string(),
                "kdeglobals".to_string(),
                "--group".to_string(),
                "Icons".to_string(),
                "--key".to_string(),
                "Theme".to_string(),
                theme.to_string(),
            ];
            ok = matches!(self.io.run(&argv, None), Some(r) if r.ok);
        }
        // Sync to settings.ini for GTK applications.
        for gtk_ver in ["gtk-3.0", "gtk-4.0"] {
            let ini = self.expand(&format!("~/.config/{gtk_ver}/settings.ini"));
            self.patch_settings_ini(&ini, "gtk-icon-theme-name", theme);
        }
        ok
    }

    fn patch_settings_ini(&mut self, path: &str, key: &str, value: &str) {
        self.io.makedirs(&dirname(path));
        let existing = if self.io.is_file(path) {
            self.io.read_text(path)
        } else {
            None
        };
        let content = compute_settings_ini(existing.as_deref(), key, value);
        let tmp = format!("{path}.tmp");
        self.io.write_text(&tmp, &content);
        self.io.replace(&tmp, path);
    }

    fn check_icon_theme_valid(&mut self, de: &str, log: &mut Vec<String>) -> String {
        log.push(l2(tr("🖼️ Checking the icon theme...")));
        let mut sources: Vec<(String, Option<String>)> =
            vec![(de.to_string(), self.read_current_icon_theme(de))];
        if de == "kde" {
            if let Some(gs) = self.gsettings_get("org.gnome.desktop.interface", "icon-theme") {
                let gs = strip_chars(&gs, "'\"");
                if !gs.is_empty() && Some(&gs) != sources[0].1.as_ref() {
                    sources.push(("__gsettings__".to_string(), Some(gs)));
                }
            }
        }

        let mut fixed = false;
        let mut failed = false;
        let mut any_problem = false;
        for (tag, current) in sources.clone() {
            let label = if tag == "kde" {
                "kdeglobals"
            } else if tag == "__gsettings__" {
                "gsettings"
            } else {
                tag.as_str()
            };
            let current = match current {
                Some(c) if !c.is_empty() => c,
                _ => {
                    log.push(l4(trf("ℹ️ %1: no icon theme set", &[&label])));
                    continue;
                }
            };
            if self.is_valid_icon_theme(&current) {
                log.push(l4(trf("✅ %1: %2 — valid", &[&label, &current])));
                continue;
            }
            any_problem = true;
            log.push(l4(trf(
                "⚠️ %1: “%2” is not valid (no Directories= line, or the theme isn't on disk)",
                &[&label, &current],
            )));
            let fallback = match self.pick_fallback_icon_theme(de) {
                Some(f) => f,
                None => {
                    log.push(l4(tr("❌ Couldn't find any valid icon theme")));
                    failed = true;
                    continue;
                }
            };
            let ok_w = if tag == "__gsettings__" {
                self.gsettings_set("org.gnome.desktop.interface", "icon-theme", &fallback)
            } else {
                self.write_icon_theme(de, &fallback)
            };
            if ok_w {
                log.push(format!("    ✅ {label}: {current} → {fallback}"));
                fixed = true;
            } else {
                log.push(l4(trf("❌ %1: couldn't write %2", &[&label, &fallback])));
                failed = true;
            }
        }

        if !any_problem {
            return "ok".to_string();
        }
        if failed {
            return "failed".to_string();
        }
        if fixed { "fixed" } else { "ok" }.to_string()
    }

    // ── MATE ────────────────────────────────────────────────────────────

    fn check_mate(&mut self, log: &mut Vec<String>) -> Results {
        let mut results: Results = Vec::new();
        let lang = self.io.env("LANG");

        // 1. Control Center localization
        if lang.starts_with("ru") {
            log.push(l2(tr("🌐 Checking the Control Center translation...")));
            // Not an interface label but DATA: the Russian names of the MATE Control
            // Center's "Common tasks" that are written to gsettings
            // (the check runs only under a Russian locale). Do not translate.
            let expected = "['Изменить тему;mate-appearance-properties.desktop', \
                'Предпочтительные приложения;mate-default-applications-properties.desktop', \
                'Информация о системе MATE;mate-system-info.desktop']";
            let current = self.gsettings_get("org.mate.control-center", "cc-actions-list");
            match current {
                Some(cur) => {
                    if cur.contains("Change Theme") || cur.contains("Set Preferred") {
                        self.gsettings_set(
                            "org.mate.control-center",
                            "cc-actions-list",
                            expected,
                        );
                        log.push(l4(tr("✅ Fixed: “Common Tasks” are now translated")));
                        put(&mut results, "mate_cc_locale", "fixed");
                    } else {
                        log.push(l4(tr("✅ Translation is fine")));
                        put(&mut results, "mate_cc_locale", "ok");
                    }
                }
                None => {
                    log.push(l4(trf("⚠️ gsettings is unavailable (not running %1?)", &[&"MATE"])));
                    put(&mut results, "mate_cc_locale", "failed");
                }
            }
        }

        // 2. Default terminal
        log.push(l2(tr("🖥️ Checking the default terminal...")));
        let current_term = self.gsettings_get("org.mate.applications-terminal", "exec");
        match current_term {
            Some(ct) => {
                let ct = strip_chars(&ct, "'");
                let (want, _) = self.wanted_terminal("mate-terminal", log);
                if ct == want {
                    log.push(l4(trf("✅ Terminal: %1", &[&want])));
                    put(&mut results, "mate_terminal", "ok");
                } else if self.io.which(&want) {
                    self.gsettings_set("org.mate.applications-terminal", "exec", &want);
                    self.gsettings_set(
                        "org.mate.applications-terminal",
                        "exec-arg",
                        crate::defaults::terminal_exec_arg(&want),
                    );
                    // TRANSLATORS: arrows ← → point the way they should in your language: in right-to-left languages swap them.
                    log.push(l4(trf("✅ Fixed: %1 → %2", &[&ct, &want])));
                    put(&mut results, "mate_terminal", "fixed");
                } else {
                    log.push(l4(trf("ℹ️ Terminal: %1 (%2 isn't installed)", &[&ct, &want])));
                    put(&mut results, "mate_terminal", "ok");
                }
            }
            None => {
                put(&mut results, "mate_terminal", "failed");
            }
        }

        // 3. All 8 layouts. Completing them puts files into a system directory
        //    via the privileged helper and substitutes Arch .desktop files —
        //    so this step is under the family gate too (otherwise a password prompt
        //    at every login on a foreign distribution).
        if self.pkg_steps_supported() {
            log.push(l2(tr("📐 Checking all 8 Ubuntu MATE panel layouts...")));
            let layouts_dir = "/usr/share/mate-panel/layouts";
            let missing_layouts: Vec<&str> = MATE_EXPECTED_LAYOUTS
                .iter()
                .copied()
                .filter(|name| {
                    let p = join(layouts_dir, &format!("{name}.layout"));
                    !self.io.exists(&p)
                })
                .collect();
            if missing_layouts.is_empty() {
                // TRANSLATORS: arrows ← → point the way they should in your language: in right-to-left languages swap them.
                log.push(l4(tr("✅ All 8 layouts are present (Familiar → Traditional)")));
                put(&mut results, "mate_layouts", "ok");
            } else {
                log.push(l4(trf(
                    "⚠️ Missing (%1): %2",
                    &[&missing_layouts.len(), &missing_layouts.join(", ")],
                )));
                if !self.may_fix(log) {
                    // Autostart: neither git clone (network) nor pkexec (password).
                    put(&mut results, "mate_layouts", NEEDS_ACTION);
                } else if self.io.which("git") {
                    log.push(l4(tr("📥 Downloading layouts from ubuntu-mate-settings...")));
                    let ok = self.download_mate_layouts(log);
                    put(&mut results, "mate_layouts", if ok { "fixed" } else { "failed" });
                } else {
                    // The package name — by THIS check's family: on Fedora and
                    // openSUSE git is called git-core (rule #13).
                    let hint = crate::utils::tool_package_hint("git", self.pkg_family.as_str());
                    log.push(
                        l4(trf("⚠️ git isn't installed — skipping. %1", &[&hint]))
                            .trim_end()
                            .to_string(),
                    );
                    put(&mut results, "mate_layouts", "failed");
                }
            }
        } else {
            self.skip_pkg_step(&mut results, &["mate_layouts"], log);
        }

        // 4. gschema override
        log.push(l2(tr("🎨 Checking the gschema override (Yaru-MATE, Ubuntu fonts)...")));
        let gschema_path =
            "/usr/share/glib-2.0/schemas/90_rexon-mate-settings.gschema.override";
        if self.io.exists(gschema_path) {
            log.push(l4(tr("✅ gschema override is installed")));
            put(&mut results, "mate_gschema", "ok");
        } else {
            log.push(l4(tr("⚠️ gschema override is missing")));
            log.push(l4(tr("ℹ️ Reinstall MATE with Rexon to set it up automatically")));
            put(&mut results, "mate_gschema", "failed");
        }

        // 5. The Yaru-MATE theme
        // ⛔ The old names `yaru-mate-gtk-theme` / `yaru-mate-icon-theme`
        // do NOT exist either in the AUR or in the repositories (RPC 2026-09-01,
        // resultcount 0) — installation failed every time.
        let yaru_pkgs = ["ubuntu-mate-themes", "ubuntu-mate-icon-themes"];
        match self.missing_pkgs(&yaru_pkgs) {
            None => self.skip_pkg_step(&mut results, &["mate_yaru_theme"], log),
            Some(yaru_missing) => {
                log.push(l2(tr("🎨 Checking the Yaru-MATE theme...")));
                if yaru_missing.is_empty() {
                    log.push(l4(tr("✅ Yaru-MATE theme is installed")));
                    put(&mut results, "mate_yaru_theme", "ok");
                } else {
                    log.push(l4(trf("⚠️ Not installed: %1", &[&yaru_missing.join(", ")])));
                    let out = self.install_aur_packages(&yaru_missing, log);
                    put(&mut results, "mate_yaru_theme", out.status());
                }
            }
        }

        // 6. Ubuntu fonts
        let ubuntu_font_pkg = "ttf-ubuntu-font-family";
        match self.pkg_state(ubuntu_font_pkg) {
            PkgState::Unsupported => {
                self.skip_pkg_step(&mut results, &["mate_ubuntu_fonts"], log)
            }
            PkgState::Installed => {
                log.push(l2(tr("🔤 Checking Ubuntu fonts...")));
                log.push(l4(tr("✅ Ubuntu fonts are installed")));
                put(&mut results, "mate_ubuntu_fonts", "ok");
            }
            PkgState::Missing => {
                log.push(l2(tr("🔤 Checking Ubuntu fonts...")));
                log.push(l4(tr("⚠️ Ubuntu fonts aren't installed")));
                match self.install_pkg(ubuntu_font_pkg, log) {
                    FixOutcome::Done => {
                        log.push(l4(tr("✅ Ubuntu fonts are installed")));
                        put(&mut results, "mate_ubuntu_fonts", "fixed");
                    }
                    FixOutcome::Failed => {
                        log.push(l4(tr("⚠️ Installation failed")));
                        put(&mut results, "mate_ubuntu_fonts", "failed");
                    }
                    FixOutcome::Deferred => {
                        put(&mut results, "mate_ubuntu_fonts", NEEDS_ACTION);
                    }
                }
            }
        }

        // 7. MATE + Ayatana AUR packages
        let mut all_mate_aur: Vec<&str> = MATE_AUR_PACKAGES.to_vec();
        all_mate_aur.extend_from_slice(AYATANA_PACKAGES);
        match self.missing_pkgs(&all_mate_aur) {
            None => self.skip_pkg_step(&mut results, &["mate_aur_pkgs"], log),
            Some(missing_pkgs) => {
                log.push(l2(tr("📦 Checking MATE AUR packages and the Ayatana stack...")));
                if missing_pkgs.is_empty() {
                    log.push(l4(tr(
                        "✅ All AUR packages are installed (including the Ayatana stack)",
                    )));
                    put(&mut results, "mate_aur_pkgs", "ok");
                } else {
                    let head: Vec<String> = missing_pkgs.iter().take(5).cloned().collect();
                    let ellipsis = if missing_pkgs.len() > 5 { "..." } else { "" };
                    log.push(l4(trf(
                        "⚠️ Not installed (%1): %2%3",
                        &[&missing_pkgs.len(), &head.join(", "), &ellipsis],
                    )));
                    let out = self.install_aur_packages(&missing_pkgs, log);
                    put(&mut results, "mate_aur_pkgs", out.status());
                }
            }
        }

        // 8. The Yaru cursor
        log.push(l2(tr("🖱️ Checking the Yaru cursor...")));
        let cursor_val = self.gsettings_get("org.mate.peripherals-mouse", "cursor-theme");
        match cursor_val {
            Some(cv) => {
                let cv = strip_chars(&cv, "'");
                if cv == "Yaru" {
                    log.push(l4(trf("✅ Cursor: %1", &[&"Yaru"])));
                    put(&mut results, "mate_cursor", "ok");
                } else {
                    let yaru_cursor_path = "/usr/share/icons/Yaru/cursors";
                    if self.io.is_dir(yaru_cursor_path) {
                        self.gsettings_set(
                            "org.mate.peripherals-mouse",
                            "cursor-theme",
                            "Yaru",
                        );
                        log.push(l4(trf("✅ Fixed: %1 → %2", &[&cv, &"Yaru"])));
                        put(&mut results, "mate_cursor", "fixed");
                    } else {
                        log.push(l4(trf("ℹ️ Cursor: %1 (%2 isn't installed)", &[&cv, &"Yaru"])));
                        put(&mut results, "mate_cursor", "ok");
                    }
                }
            }
            None => {
                put(&mut results, "mate_cursor", "failed");
            }
        }

        // 9. mate-system-info localization
        if lang.starts_with("ru") {
            log.push(l2(tr("🌐 Checking the mate-utils translation...")));
            let mo_path = "/usr/share/locale/ru/LC_MESSAGES/mate-utils.mo";
            if self.io.exists(mo_path) {
                log.push(l4(tr("ℹ️ Translation file mate-utils.mo is present")));
                log.push(l4(tr(
                    "ℹ️ Device Name/Memory/Processor stay untranslated — a MATE bug, not Rexon's",
                )));
                put(&mut results, "mate_system_info_locale", "ok");
            } else {
                log.push(l4(tr("⚠️ Translation file mate-utils.mo is missing")));
                put(&mut results, "mate_system_info_locale", "failed");
            }
        }

        // 10. The Brisk menu on the panel → the MATE Menu.
        self.check_mate_brisk_menu(&mut results, log);

        results
    }

    /// Applets on the system (`Factory::Applet`) — from mate-panel's applet
    /// directory, as [`crate::mate_panel::installed_applets`] reads them.
    fn mate_applets(&mut self) -> std::collections::BTreeSet<String> {
        let dir = crate::mate_panel::SYSTEM_APPLETS_DIR;
        let mut out = std::collections::BTreeSet::new();
        for name in self.io.listdir(dir) {
            if !name.ends_with(".mate-panel-applet") {
                continue;
            }
            if let Some(text) = self.io.read_text(&join(dir, &name)) {
                out.extend(crate::mate_panel::applets_in_description(&text));
            }
        }
        out
    }

    /// The Brisk menu on the live panel → the MATE Menu (Wehrwolfmann's decision
    /// of 24.11.2026: Brisk is abandoned and Rexon no longer installs it).
    /// Without mate-menu on the system Rexon builds it from the AUR on its own
    /// (its own transaction); if it does not install — the built-in menu
    /// button, so the panel never stays without a menu. At login (no password,
    /// no network) a still working Brisk menu is left until "Check my desktop";
    /// a Brisk menu whose applet is gone is replaced right away. No Brisk on the
    /// panel — the step says nothing.
    fn check_mate_brisk_menu(&mut self, results: &mut Results, log: &mut Vec<String>) {
        let dump_argv: Vec<String> =
            ["dconf", "dump", crate::mate_panel::PANEL_DCONF_DIR].iter().map(|s| s.to_string()).collect();
        let Some(dump) = self.io.run(&dump_argv, None).filter(|r| r.ok).map(|r| r.stdout) else {
            return;
        };
        if crate::mate_panel::brisk_migration(&dump, false).is_none() {
            return;
        }
        log.push(l2(tr("🧭 Checking the panel menu...")));
        log.push(l4(tr("⚠️ The panel has the Brisk menu, which is no longer supported")));
        let mut applets = self.mate_applets();
        if !applets.contains(crate::mate_panel::MATE_MENU_IID) {
            let brisk_works = applets.contains(crate::mate_panel::BRISK_MENU_IID);
            match self.pkg_state(MATE_MENU_PACKAGE) {
                PkgState::Missing => match self.install_aur_packages(&[MATE_MENU_PACKAGE.to_string()], log) {
                    FixOutcome::Done => applets = self.mate_applets(),
                    FixOutcome::Failed => log.push(l4(tr(
                        "ℹ️ The MATE Menu couldn't be installed — the standard MATE menu button is used instead",
                    ))),
                    // At login: a working Brisk menu waits for "Check my desktop".
                    FixOutcome::Deferred if brisk_works => {
                        put(results, "mate_menu", NEEDS_ACTION);
                        return;
                    }
                    FixOutcome::Deferred => {}
                },
                // Installed, but its applet is not where mate-panel looks, or a
                // family without package steps: the built-in button.
                PkgState::Installed | PkgState::Unsupported => {}
            }
        }
        let mate_menu = applets.contains(crate::mate_panel::MATE_MENU_IID);
        let Some(plan) = crate::mate_panel::brisk_migration(&dump, mate_menu) else {
            return;
        };
        for argv in &plan.commands {
            if !matches!(self.io.run(argv, None), Some(r) if r.ok) {
                log.push(l4(tr("⚠️ Couldn't replace the Brisk menu on the panel")));
                put(results, "mate_menu", "failed");
                return;
            }
        }
        log.push(l4(if mate_menu {
            // TRANSLATORS: arrows ← → point the way they should in your language: in right-to-left languages swap them.
            tr("✅ Fixed: Brisk menu → MATE Menu")
        } else {
            // TRANSLATORS: arrows ← → point the way they should in your language: in right-to-left languages swap them.
            tr("✅ Fixed: Brisk menu → standard MATE menu")
        }));
        put(results, "mate_menu", "fixed");
    }

    fn download_mate_layouts(&mut self, log: &mut Vec<String>) -> bool {
        // Foolproofing: this cannot be reached at login —
        // here there is both the network (git clone) and writing to a system directory.
        if !self.may_fix(log) {
            return false;
        }
        let tmp_repo = self.io.mkdtemp("rexon_ums_", None);
        let layouts_dst = "/usr/share/mate-panel/layouts";

        // if os.path.isdir(tmp_repo): rmtree — mkdtemp always creates the directory.
        if self.io.is_dir(&tmp_repo) {
            self.io.remove_tree(&tmp_repo);
        }

        let clone_argv = vec![
            "git".to_string(),
            "clone".to_string(),
            "--depth".to_string(),
            "1".to_string(),
            "https://github.com/ubuntu-mate/ubuntu-mate-settings.git".to_string(),
            tmp_repo.clone(),
        ];
        let r = self.io.run(&clone_argv, Some(120));
        if !matches!(&r, Some(rr) if rr.ok) {
            log.push(l4(tr("⚠️ Couldn't clone the repository")));
            return false;
        }

        let layouts_src = join(
            &join(&join(&join(&tmp_repo, "usr"), "share"), "mate-panel"),
            "layouts",
        );
        if !self.io.is_dir(&layouts_src) {
            log.push(l4(tr("⚠️ Layouts folder not found")));
            return false;
        }

        let files: Vec<String> = self
            .io
            .listdir(&layouts_src)
            .into_iter()
            .filter(|f| f.ends_with(".layout") || f.ends_with(".dock"))
            .collect();
        if files.is_empty() {
            return false;
        }

        let staged = self.io.mkdtemp("rexon_layouts_stage_", Some("/tmp"));
        for f in &files {
            let src = join(&layouts_src, f);
            let dst = join(&staged, basename(f));
            self.io.copy_file(&src, &dst);
            if f.ends_with(".layout") {
                let fp = join(&staged, basename(f));
                if let Some(txt) = self.io.read_text(&fp) {
                    let txt = apply_snap_map(&txt);
                    self.io.write_text(&fp, &txt);
                }
            }
        }

        // named org.rexon.stage, otherwise the hardened raw pkexec.
        let staged_is_dir = self.io.is_dir(&staged);
        let staged_real = self.io.realpath(&staged);
        let named = crate::system_plan::privileged_stage_cmd(
            "mate-layouts",
            "rexon",
            &staged,
            staged_is_dir,
            Some(&staged_real),
            self.stage_helper_available,
        );
        let r2 = if let Some(named) = named {
            let op = crate::privilege::op::PrivOp::Stage {
                category: "mate-layouts".to_string(),
                name: "rexon".to_string(),
                source: crate::privilege::op::LocalFile { slot: 0, label: staged.clone() },
            };
            self.io.run_priv(&op, std::slice::from_ref(&staged), &named, Some(120))
        } else {
            let staged_files = self.io.listdir(&staged);
            let script = mate_layouts_bash_script(&staged, &staged_files, layouts_dst);
            let tmp_sh = self.io.mkstemp("rexon_layouts_", ".sh");
            self.io.write_text(&tmp_sh, &script);
            self.io.chmod(&tmp_sh, 0o755);
            let r2 = self.io.run(
                &["pkexec".to_string(), "bash".to_string(), tmp_sh.clone()],
                Some(120),
            );
            self.io.remove_file(&tmp_sh);
            r2
        };

        self.io.remove_tree(&staged);
        if self.io.is_dir(&tmp_repo) {
            self.io.remove_tree(&tmp_repo);
        }

        if matches!(&r2, Some(rr) if rr.ok) {
            let n_layout = files.iter().filter(|f| f.ends_with(".layout")).count();
            log.push(l4(ntr(
                "✅ Copied %n layout (paths adapted)",
                "✅ Copied %n layouts (paths adapted)",
                n_layout as u64,
            )));
            true
        } else {
            log.push(l4(tr("⚠️ Administrator access was declined or copying failed")));
            false
        }
    }

    // ── KDE ─────────────────────────────────────────────────────────────

    fn check_kde(&mut self, log: &mut Vec<String>) -> Results {
        let mut results: Results = Vec::new();

        let icon = self.check_icon_theme_valid("kde", log);
        put(&mut results, "kde_icon_theme", &icon);

        log.push(l2(trf("🖥️ Checking the %1 terminal...", &[&"KDE"])));
        let kdeglobals = self.expand("~/.config/kdeglobals");
        let (want, by_choice) = self.wanted_terminal("konsole", log);
        let want_service = match (&self.chosen_terminal, by_choice) {
            (Some(c), true) => c.clone(),
            _ => "org.kde.konsole.desktop".to_string(),
        };
        if self.io.exists(&kdeglobals) {
            let content = self.io.read_text(&kdeglobals).unwrap_or_default();
            if content.contains(&format!("TerminalApplication={want}")) {
                log.push(l4(trf("✅ Terminal: %1", &[&want])));
                put(&mut results, "kde_terminal", "ok");
            } else if self.io.which(&want) {
                if self.io.which("kwriteconfig6") {
                    self.io.run(
                        &[
                            "kwriteconfig6".to_string(),
                            "--file".to_string(),
                            "kdeglobals".to_string(),
                            "--group".to_string(),
                            "General".to_string(),
                            "--key".to_string(),
                            "TerminalApplication".to_string(),
                            want.clone(),
                        ],
                        None,
                    );
                    self.io.run(
                        &[
                            "kwriteconfig6".to_string(),
                            "--file".to_string(),
                            "kdeglobals".to_string(),
                            "--group".to_string(),
                            "General".to_string(),
                            "--key".to_string(),
                            "TerminalService".to_string(),
                            want_service.clone(),
                        ],
                        None,
                    );
                    // TRANSLATORS: arrows ← → point the way they should in your language: in right-to-left languages swap them.
                    log.push(l4(trf("✅ Fixed: terminal → %1", &[&want])));
                    put(&mut results, "kde_terminal", "fixed");
                } else {
                    log.push(l4(trf("⚠️ %1 not found", &[&"kwriteconfig6"])));
                    put(&mut results, "kde_terminal", "failed");
                }
            } else {
                log.push(l4(trf("ℹ️ %1 isn't installed", &[&want])));
                put(&mut results, "kde_terminal", "ok");
            }
        } else {
            log.push(l4(trf("ℹ️ %1 not found (first launch?)", &[&"kdeglobals"])));
            put(&mut results, "kde_terminal", "ok");
        }

        if self.pkg_steps_supported() {
            log.push(l2(tr("🔑 Checking the display manager...")));
            if self.is_pkg_installed("plasma-login-manager") {
                log.push(l4(trf("✅ %1 is installed", &[&"Plasma Login Manager"])));
                put(&mut results, "kde_sddm", "ok");
            } else if self.is_pkg_installed("sddm") {
                log.push(l4(trf("✅ %1 is installed", &[&"SDDM"])));
                put(&mut results, "kde_sddm", "ok");
            } else {
                log.push(l4(tr(
                    "⚠️ No display manager (SDDM / Plasma Login Manager) is installed",
                )));
                put(&mut results, "kde_sddm", "failed");
            }
        } else {
            self.skip_pkg_step(&mut results, &["kde_sddm"], log);
        }

        results
    }

    // ── GNOME ───────────────────────────────────────────────────────────

    fn check_gnome(&mut self, log: &mut Vec<String>) -> Results {
        let mut results: Results = Vec::new();

        let icon = self.check_icon_theme_valid("gnome", log);
        put(&mut results, "gnome_icon_theme", &icon);

        log.push(l2(trf("🖥️ Checking the %1 terminal...", &[&"GNOME"])));
        let term_val = self.gsettings_get(
            "org.gnome.desktop.default-applications.terminal",
            "exec",
        );
        match term_val {
            Some(tv) => {
                let tv = strip_chars(&tv, "'");
                let (chosen, by_choice) = self.wanted_terminal("gnome-terminal", log);
                let gnome_terms = ["gnome-terminal", "kgx", "org.gnome.Console"];
                if by_choice {
                    // The person's choice: it must be the one in the schema.
                    if tv == chosen {
                        log.push(l4(trf("✅ Terminal: %1", &[&tv])));
                        put(&mut results, "gnome_terminal", "ok");
                    } else {
                        let schema = "org.gnome.desktop.default-applications.terminal";
                        self.gsettings_set(schema, "exec", &chosen);
                        self.gsettings_set(schema, "exec-arg", crate::defaults::terminal_exec_arg(&chosen));
                        // TRANSLATORS: arrows ← → point the way they should in your language: in right-to-left languages swap them.
                        log.push(l4(trf("✅ Fixed: %1 → %2", &[&tv, &chosen])));
                        put(&mut results, "gnome_terminal", "fixed");
                    }
                } else if gnome_terms.contains(&tv.as_str()) {
                    log.push(l4(trf("✅ Terminal: %1", &[&tv])));
                    put(&mut results, "gnome_terminal", "ok");
                } else {
                    let mut done = false;
                    for t in gnome_terms {
                        if self.io.which(t) {
                            self.gsettings_set(
                                "org.gnome.desktop.default-applications.terminal",
                                "exec",
                                t,
                            );
                            log.push(l4(trf("✅ Fixed: %1 → %2", &[&tv, &t])));
                            put(&mut results, "gnome_terminal", "fixed");
                            done = true;
                            break;
                        }
                    }
                    if !done {
                        log.push(l4(trf("ℹ️ Terminal: %1", &[&tv])));
                        put(&mut results, "gnome_terminal", "ok");
                    }
                }
            }
            None => {
                log.push(l4(trf("⚠️ gsettings is unavailable (not running %1?)", &[&"GNOME"])));
                put(&mut results, "gnome_terminal", "failed");
            }
        }

        if self.pkg_steps_supported() {
            log.push(l2(tr("🔑 Checking the display manager...")));
            if self.is_pkg_installed("gdm") {
                log.push(l4(trf("✅ %1 is installed", &[&"GDM"])));
                put(&mut results, "gnome_gdm", "ok");
            } else {
                log.push(l4(trf("⚠️ %1 isn't installed", &[&"GDM"])));
                put(&mut results, "gnome_gdm", "failed");
            }

            log.push(l2(trf("🔧 Checking %1...", &[&"gnome-tweaks"])));
            if self.is_pkg_installed("gnome-tweaks") {
                log.push(l4(trf("✅ %1 is installed", &[&"gnome-tweaks"])));
                put(&mut results, "gnome_tweaks", "ok");
            } else {
                log.push(l4(trf("⚠️ %1 isn't installed", &[&"gnome-tweaks"])));
                put(&mut results, "gnome_tweaks", "failed");
            }
        } else {
            self.skip_pkg_step(&mut results, &["gnome_gdm", "gnome_tweaks"], log);
        }

        results
    }

    // ── COSMIC ──────────────────────────────────────────────────────────

    fn check_cosmic(&mut self, log: &mut Vec<String>) -> Results {
        let mut results: Results = Vec::new();

        // The whole COSMIC check set is package-based (wallpapers/icons/keyring).
        if !self.pkg_steps_supported() {
            self.skip_pkg_step(
                &mut results,
                &["cosmic_wallpapers", "cosmic_icons", "cosmic_keyring"],
                log,
            );
            return results;
        }

        log.push(l2(tr("🖼️ Checking COSMIC wallpapers...")));
        if self.is_pkg_installed("cosmic-wallpapers") {
            log.push(l4(trf("✅ %1 is installed", &[&"cosmic-wallpapers"])));
            put(&mut results, "cosmic_wallpapers", "ok");
        } else {
            log.push(l4(if self.allow_privileged {
                trf("⚠️ %1 isn't installed — installing...", &[&"cosmic-wallpapers"])
            } else {
                trf("⚠️ %1 isn't installed", &[&"cosmic-wallpapers"])
            }));
            match self.install_pkg("cosmic-wallpapers", log) {
                FixOutcome::Done => {
                    log.push(l4(trf("✅ %1 is installed", &[&"cosmic-wallpapers"])));
                    put(&mut results, "cosmic_wallpapers", "fixed");
                }
                FixOutcome::Failed => {
                    log.push(l4(tr("⚠️ Installation failed")));
                    put(&mut results, "cosmic_wallpapers", "failed");
                }
                FixOutcome::Deferred => {
                    put(&mut results, "cosmic_wallpapers", NEEDS_ACTION);
                }
            }
        }

        log.push(l2(tr("🎨 Checking COSMIC icons...")));
        if self.is_pkg_installed("cosmic-icon-theme") {
            log.push(l4(trf("✅ %1 is installed", &[&"cosmic-icon-theme"])));
            put(&mut results, "cosmic_icons", "ok");
        } else {
            log.push(l4(trf("⚠️ %1 isn't installed", &[&"cosmic-icon-theme"])));
            match self.install_pkg("cosmic-icon-theme", log) {
                FixOutcome::Done => {
                    log.push(l4(trf("✅ %1 is installed", &[&"cosmic-icon-theme"])));
                    put(&mut results, "cosmic_icons", "fixed");
                }
                FixOutcome::Failed => put(&mut results, "cosmic_icons", "failed"),
                FixOutcome::Deferred => put(&mut results, "cosmic_icons", NEEDS_ACTION),
            }
        }

        log.push(l2(trf("🔐 Checking %1...", &[&"gnome-keyring"])));
        if self.is_pkg_installed("gnome-keyring") {
            log.push(l4(trf("✅ %1 is installed", &[&"gnome-keyring"])));
            put(&mut results, "cosmic_keyring", "ok");
        } else {
            log.push(l4(trf("⚠️ %1 isn't installed", &[&"gnome-keyring"])));
            put(&mut results, "cosmic_keyring", "failed");
        }

        results
    }

    // ── XFCE ────────────────────────────────────────────────────────────

    fn check_xfce(&mut self, log: &mut Vec<String>) -> Results {
        let mut results: Results = Vec::new();

        log.push(l2(tr("🖥️ Checking XFCE helpers.rc...")));
        let helpers_path = self.expand("~/.config/xfce4/helpers.rc");
        if self.io.exists(&helpers_path) {
            let content = self.io.read_text(&helpers_path).unwrap_or_default();
            if content.contains("TerminalEmulator=") {
                log.push(l4(tr("✅ helpers.rc is configured")));
                put(&mut results, "xfce_helpers", "ok");
            } else {
                log.push(l4(tr("ℹ️ TerminalEmulator isn't set")));
                put(&mut results, "xfce_helpers", "ok");
            }
        } else {
            log.push(l4(tr("ℹ️ helpers.rc doesn't exist (first launch?)")));
            put(&mut results, "xfce_helpers", "ok");
        }

        if self.pkg_steps_supported() {
            log.push(l2(tr("🔑 Checking the display manager...")));
            if self.is_pkg_installed("lightdm") {
                log.push(l4(trf("✅ %1 is installed", &[&"LightDM"])));
                put(&mut results, "xfce_lightdm", "ok");
            } else {
                log.push(l4(trf("⚠️ %1 isn't installed", &[&"LightDM"])));
                put(&mut results, "xfce_lightdm", "failed");
            }
        } else {
            self.skip_pkg_step(&mut results, &["xfce_lightdm"], log);
        }

        results
    }

    // ── Cinnamon (Mint) ─────────────────────────────────────────────────

    fn check_cinnamon(&mut self, log: &mut Vec<String>) -> Results {
        let mut results: Results = Vec::new();

        // The whole Cinnamon check set is package-based (Nemo/Mint themes/LightDM).
        if !self.pkg_steps_supported() {
            self.skip_pkg_step(
                &mut results,
                &["cinnamon_nemo", "cinnamon_themes", "cinnamon_lightdm"],
                log,
            );
            return results;
        }

        log.push(l2(tr("📁 Checking the Nemo file manager...")));
        if self.is_pkg_installed("nemo") {
            log.push(l4(trf("✅ %1 is installed", &[&"Nemo"])));
            put(&mut results, "cinnamon_nemo", "ok");
        } else {
            log.push(l4(trf("⚠️ %1 isn't installed", &[&"Nemo"])));
            put(&mut results, "cinnamon_nemo", "failed");
        }

        log.push(l2(tr("🎨 Checking Linux Mint themes...")));
        let mint_theme_pkgs = ["mint-themes", "mint-y-icons"];
        let mint_missing: Vec<String> = mint_theme_pkgs
            .iter()
            .filter(|p| !self.is_pkg_installed(p))
            .map(|p| p.to_string())
            .collect();
        if mint_missing.is_empty() {
            log.push(l4(tr("✅ Mint themes are installed")));
            put(&mut results, "cinnamon_themes", "ok");
        } else {
            log.push(l4(trf("⚠️ Not installed: %1", &[&mint_missing.join(", ")])));
            let out = self.install_aur_packages(&mint_missing, log);
            put(&mut results, "cinnamon_themes", out.status());
        }

        log.push(l2(tr("🔑 Checking the display manager...")));
        if self.is_pkg_installed("lightdm") {
            log.push(l4(trf("✅ %1 is installed", &[&"LightDM"])));
            put(&mut results, "cinnamon_lightdm", "ok");
        } else {
            log.push(l4(trf("⚠️ %1 isn't installed", &[&"LightDM"])));
            put(&mut results, "cinnamon_lightdm", "failed");
        }

        results
    }

    // ── Budgie ──────────────────────────────────────────────────────────

    fn check_budgie(&mut self, log: &mut Vec<String>) -> Results {
        let mut results: Results = Vec::new();

        log.push(l2(trf("🖥️ Checking the %1 terminal...", &[&"Budgie"])));
        let term_val = self.gsettings_get(
            "org.gnome.desktop.default-applications.terminal",
            "exec",
        );
        match term_val {
            Some(tv) => {
                log.push(l4(trf("✅ Terminal: %1", &[&strip_chars(&tv, "'")])));
                put(&mut results, "budgie_terminal", "ok");
            }
            None => {
                log.push(l4(tr("⚠️ gsettings is unavailable")));
                put(&mut results, "budgie_terminal", "failed");
            }
        }

        if self.pkg_steps_supported() {
            log.push(l2(trf("📦 Checking %1...", &[&"budgie-extras"])));
            if self.is_pkg_installed("budgie-extras") {
                log.push(l4(trf("✅ %1 is installed", &[&"budgie-extras"])));
                put(&mut results, "budgie_extras", "ok");
            } else {
                log.push(l4(trf("⚠️ %1 isn't installed", &[&"budgie-extras"])));
                // ⚠️ It used to be installed by an AUR helper — and on a machine without
                // yay/paru the fix failed for nothing: `budgie-extras` is in
                // extra, pacman takes it itself.
                let out = self.install_pkg("budgie-extras", log);
                put(&mut results, "budgie_extras", out.status());
            }

            log.push(l2(trf("📶 Checking %1...", &[&"network-manager-applet"])));
            if self.is_pkg_installed("network-manager-applet") {
                log.push(l4(trf("✅ %1 is installed", &[&"network-manager-applet"])));
                put(&mut results, "budgie_nm_applet", "ok");
            } else {
                log.push(l4(trf("⚠️ %1 isn't installed", &[&"network-manager-applet"])));
                put(&mut results, "budgie_nm_applet", "failed");
            }
        } else {
            self.skip_pkg_step(&mut results, &["budgie_extras", "budgie_nm_applet"], log);
        }

        results
    }

    // ── Hyprland ────────────────────────────────────────────────────────

    fn check_hyprland(&mut self, log: &mut Vec<String>) -> Results {
        let mut results: Results = Vec::new();

        if self.pkg_steps_supported() {
            log.push(l2(tr("🧱 Checking Hyprland components...")));
            for pkg in HYPRLAND_COMPONENTS {
                let label = hyprland_component_label(pkg);
                if self.is_pkg_installed(pkg) {
                    log.push(format!("    ✅ {label}: {pkg}"));
                    put(&mut results, &format!("hyprland_{pkg}"), "ok");
                } else {
                    log.push(l4(trf("⚠️ %1: %2 isn't installed", &[&label, pkg])));
                    put(&mut results, &format!("hyprland_{pkg}"), "failed");
                }
            }
        } else {
            let keys: Vec<String> = HYPRLAND_COMPONENTS
                .iter()
                .map(|pkg| format!("hyprland_{pkg}"))
                .collect();
            self.skip_pkg_step(&mut results, &keys, log);
        }

        log.push(l2(tr("📝 Checking the configuration...")));
        let hypr_cfg = self.expand("~/.config/hypr/hyprland.conf");
        if self.io.exists(&hypr_cfg) {
            log.push(l4(trf("✅ %1 exists", &[&"hyprland.conf"])));
            put(&mut results, "hyprland_config", "ok");
        } else {
            log.push(l4(trf("⚠️ %1 not found", &[&"hyprland.conf"])));
            log.push(l4(trf(
                "ℹ️ Create it: %1",
                &[&"cp /usr/share/hyprland/hyprland.conf ~/.config/hypr/"],
            )));
            put(&mut results, "hyprland_config", "failed");
        }

        if self.pkg_steps_supported() {
            log.push(l2(trf("🌐 Checking %1...", &[&"xdg-desktop-portal"])));
            if self.is_pkg_installed("xdg-desktop-portal-hyprland") {
                log.push(l4(trf("✅ %1 is installed", &[&"xdg-desktop-portal-hyprland"])));
                put(&mut results, "hyprland_portal", "ok");
            } else {
                log.push(l4(trf("⚠️ %1 isn't installed", &[&"xdg-desktop-portal-hyprland"])));
                put(&mut results, "hyprland_portal", "failed");
            }

            log.push(l2(tr("🔐 Checking the polkit agent...")));
            if self.is_pkg_installed("polkit-gnome") {
                log.push(l4(trf("✅ %1 is installed", &[&"polkit-gnome"])));
                put(&mut results, "hyprland_polkit", "ok");
            } else {
                log.push(l4(trf("⚠️ %1 isn't installed", &[&"polkit-gnome"])));
                put(&mut results, "hyprland_polkit", "failed");
            }
        } else {
            self.skip_pkg_step(&mut results, &["hyprland_portal", "hyprland_polkit"], log);
        }

        results
    }

    // ── Dispatch ────────────────────────────────────────────────────────

    /// Whether there is a check set for a DE (`_DE_CHECKS.get(de_id)` — without running).
    pub fn has_handler(de_id: &str) -> bool {
        has_checks(de_id)
    }

    fn dispatch(&mut self, de_id: &str, log: &mut Vec<String>) -> Results {
        match de_id {
            "mate" => self.check_mate(log),
            "kde" => self.check_kde(log),
            "gnome" => self.check_gnome(log),
            "cosmic" => self.check_cosmic(log),
            "xfce" => self.check_xfce(log),
            "cinnamon-mint" => self.check_cinnamon(log),
            "budgie" => self.check_budgie(log),
            "hyprland" => self.check_hyprland(log),
            _ => Vec::new(),
        }
    }

    /// `run_for_de` — run all checks for a DE. Returns the results and
    /// writes the log to `log`.
    pub fn run_for_de(&mut self, de_id: &str, log: &mut Vec<String>) -> Results {
        if !Self::has_handler(de_id) {
            log.push(trf("ℹ️ No checks for this desktop: %1", &[&de_id]));
            return Vec::new();
        }
        log.push(trf("🔍 Checking desktop %1...", &[&de_id]));
        let results = self.dispatch(de_id, log);

        let fixed = results.iter().filter(|(_, v)| v == "fixed").count();
        let failed = results.iter().filter(|(_, v)| v == "failed").count();
        let ok = results.iter().filter(|(_, v)| v == "ok").count();
        let skipped = results.iter().filter(|(_, v)| v == SKIPPED).count();
        let needs = results.iter().filter(|(_, v)| v == NEEDS_ACTION).count();
        // One string per variant rather than a sentence from pieces: the order of parts
        // and the punctuation between them is the translator's call (in right-to-left languages and with
        // a different word order the ", %1 skipped" suffix at the end does not work).
        let summary = match (skipped > 0, needs > 0) {
            (false, false) => trf(
                // TRANSLATORS: %1 checks passed, %2 fixed, %3 failed.
                "✅ Check finished: %1 OK, %2 fixed, %3 failed",
                &[&ok, &fixed, &failed],
            ),
            (true, false) => trf(
                // TRANSLATORS: %1 checks passed, %2 fixed, %3 failed, %4 skipped.
                "✅ Check finished: %1 OK, %2 fixed, %3 failed, %4 skipped",
                &[&ok, &fixed, &failed, &skipped],
            ),
            (false, true) => trf(
                // TRANSLATORS: %1 checks passed, %2 fixed, %3 failed, %4 need a program installed first.
                "✅ Check finished: %1 OK, %2 fixed, %3 failed, %4 to install",
                &[&ok, &fixed, &failed, &needs],
            ),
            (true, true) => trf(
                // TRANSLATORS: %1 checks passed, %2 fixed, %3 failed, %4 skipped, %5 need a program installed first.
                "✅ Check finished: %1 OK, %2 fixed, %3 failed, %4 skipped, %5 to install",
                &[&ok, &fixed, &failed, &skipped, &needs],
            ),
        };
        log.push(summary);
        results
    }

    /// `run_for_current_de` — detect the current DE and run the checks.
    pub fn run_for_current_de(
        &mut self,
        xdg_current_desktop: &str,
        log: &mut Vec<String>,
    ) -> Results {
        match detect_current_de(xdg_current_desktop) {
            Some(de_id) => {
                self.chosen_terminal = self.read_chosen_terminal(xdg_current_desktop);
                let results = self.run_for_de(&de_id, log);
                self.chosen_terminal = None;
                results
            }
            None => {
                log.push(msg_unknown_de());
                Vec::new()
            }
        }
    }

    /// `run_for_installed_des` — run compliance for all installed DEs.
    pub fn run_for_installed_des(
        &mut self,
        installed_de_ids: &[String],
        log: &mut Vec<String>,
    ) -> Vec<(String, Results)> {
        let mut all_results: Vec<(String, Results)> = Vec::new();
        for de_id in installed_de_ids {
            if Self::has_handler(de_id) {
                log.extend(de_banner_lines(de_id));
                self.chosen_terminal = self.read_chosen_terminal(de_id);
                let results = self.run_for_de(de_id, log);
                self.chosen_terminal = None;
                all_results.push((de_id.clone(), results));
            }
        }
        all_results
    }
}
