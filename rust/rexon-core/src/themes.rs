//! Manager of GTK themes, icon themes and cursors — port of `rexon/themes.py`
//! (the `ThemeManager` class).
//!
//! Discovery / applying of GTK themes, icon sets, cursors (and Plasma KDE
//! themes) for all supported DEs. The catalog of popular themes is loaded from
//! the built-in `rexon/themes.json` + the user's `~/.config/rexon/themes.json`
//! (overrides/extends with `dict.update` semantics).
//!
//! Porting principle (as in `system.rs` / `installed_scan.rs` / `defaults.rs`):
//! ALL pure logic is deterministic input→output functions. Side effects (real
//! `subprocess`/`safe_run` of gsettings/xfconf/kwriteconfig/dbus/
//! plasma-apply/hyprctl, reading file system directories, writing
//! `settings.ini` / `~/.icons/default` / history) are NOT executed in the core.
//! They are expressed as:
//!   * PLANS of commands/writes ([`ApplyStep`], [`Cmd`], [`Plan`],
//!     [`KvantumPlan`]) — the executor runs them and computes `ok` itself;
//!   * a file system snapshot ([`crate::installed_scan::FsSnapshot`]) for the
//!     installed theme scanners;
//!   * ready source values (gsettings/kreadconfig6 output, config contents) for
//!     building the "current themes".
//!
//! BYTE-FOR-BYTE parity with Python: the same catalog merge logic (order,
//! `dict.update` key overriding), the same command argv and step order, the
//! same parsing of `index.theme` / `kcminputrc` / `settings.ini`, the same
//! `settings.ini` generation. The references were taken from LIVE Python
//! read-only (safe_run/subprocess mocked, HOME a temporary directory, the real
//! `rexon/themes.json` as the built-in catalog) — `tests/themes_parity.rs`.
//!
//! Preserved quirks of the original (no intentional divergence, parity):
//!   * [`resolve_schemas`] is CASE-SENSITIVE to the SCHEMAS keys/("xfce"/"kde"),
//!     while [`is_generic_wm`] is case-insensitive (`de.lower()`); the
//!     original's asymmetry is kept;
//!   * `hyprland` is NOT in `_NATIVE_DES` → [`is_generic_wm`] is `true` for it:
//!     apply writes `settings.ini` in addition to gsettings;
//!   * `lxqt` is in `_NATIVE_DES` but not in SCHEMAS → gsettings via the
//!     generic GTK path, but `settings.ini` is NOT written (not generic-wm);
//!   * [`update_gtk_settings_ini`] replaces the `{key}=` line in ANY section
//!     (without checking `[Settings]`) — the original's quirk is kept;
//!   * [`kde_font_to_display`] gives `10` for an unparsable size (like
//!     `except ValueError`).
//!
//! Addition 2026-07-30: ported the decisions of `install_theme_from_archive`
//! ([`pick_archive_dir`], [`archive_install_dest`]), the argv and text parsing
//! of `_kde_get` ([`kde_read_cmd`], [`kdeglobals_value`]) and all user messages
//! of the catalog/archive (`msg_*`).
//!
//! ⚠ Rule #16: [`pick_archive_dir`] takes the first folder of the archive
//! ALPHABETICALLY — the original took `os.scandir()[0]`, i.e. with several
//! folders it installed a random one (the order is set by the file system).
//!
//! NOT ported (execution — when connected to the live Python / a future
//! bridge): the real `safe_run`/`safe_check_command`, `os.scandir`,
//! `os.makedirs`, reading/writing `settings.ini` / `~/.icons/default` /
//! `theme_history.json`, the actual unpacking of theme archives and `copytree`,
//! calling `pkg_manager.install`, the cursor sanitizer on the live file system,
//! logging.

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::{Map, Value};

use crate::i18n::{tr, trc, trf};
use crate::installed_scan::FsSnapshot;

// ============================================================================
// Search directories (live values are built by the executor from $HOME)
// ============================================================================

/// `GTK_THEME_DIRS` — where to look for GTK themes (live: usr + `~/.themes`).
pub fn gtk_theme_dirs(home: &str) -> [String; 2] {
    ["/usr/share/themes".to_string(), format!("{home}/.themes")]
}

/// `ICON_DIRS` — where to look for icons/cursors (live: usr + `~/.local/share/icons`).
pub fn icon_dirs(home: &str) -> [String; 2] {
    [
        "/usr/share/icons".to_string(),
        format!("{home}/.local/share/icons"),
    ]
}

/// Path of the built-in theme catalog — port of `_BUILTIN_JSON` (next to
/// `themes.py`). `pkg_dir` is the `rexon` package directory (live: the
/// directory of `themes.py`).
pub fn builtin_json_path(pkg_dir: &str) -> String {
    format!("{pkg_dir}/themes.json")
}

/// Path of the user theme catalog — port of `_USER_JSON`.
pub fn user_json_path(home: &str) -> String {
    user_json_path_xdg(None, home)
}

/// The same, honoring `$XDG_CONFIG_HOME`.
pub fn user_json_path_xdg(xdg_config_home: Option<&str>, home: &str) -> String {
    format!(
        "{}/rexon/themes.json",
        crate::utils::config_home(xdg_config_home, home)
    )
}

// ============================================================================
// Catalog of popular themes: merging the built-in + user JSON
// ============================================================================

/// Port of `_load_themes_catalog`: `themes = {}` → `update(builtin["themes"])`
/// → `update(user["themes"])`. `dict.update` semantics: an existing key is
/// replaced in place (the order is kept), a new one is appended at the end.
///
/// `builtin`/`user` are the file contents (None = no file). Broken JSON, a
/// non-object top level or `themes` that is not an object are skipped (like
/// `except`/`data.get("themes", {})` in the original).
pub fn merge_catalogs(builtin: Option<&str>, user: Option<&str>) -> Map<String, Value> {
    let mut out = Map::new();
    for content in [builtin, user] {
        let Some(text) = content else { continue };
        let Ok(Value::Object(root)) = serde_json::from_str::<Value>(text) else {
            continue; // broken JSON / not an object → skip the file
        };
        if let Some(Value::Object(themes)) = root.get("themes") {
            for (k, v) in themes {
                out.insert(k.clone(), v.clone()); // IndexMap: replace in place / append
            }
        }
    }
    out
}

/// `json.dumps(catalog, indent=2, ensure_ascii=False)` — for byte-for-byte comparison.
pub fn serialize_catalog(catalog: &Map<String, Value>) -> String {
    serde_json::to_string_pretty(&Value::Object(catalog.clone())).expect("Value сериализуется")
}

// ============================================================================
// Resolving gsettings schemas by DE — port of SCHEMAS / _GENERIC_WM_SCHEMAS /
// _NATIVE_DES / _resolve_schemas / _is_generic_wm
// ============================================================================

/// (role, schema, prop) — roles `gtk`/`icons`/`cursor`/`font`. The role order
/// = the key order in the Python dict (matters for `get_current_themes`).
pub type SchemaEntry = (&'static str, &'static str, &'static str);

const GNOME_STACK: &[SchemaEntry] = &[
    ("gtk", "org.gnome.desktop.interface", "gtk-theme"),
    ("icons", "org.gnome.desktop.interface", "icon-theme"),
    ("cursor", "org.gnome.desktop.interface", "cursor-theme"),
    ("font", "org.gnome.desktop.interface", "font-name"),
];

const CINNAMON_STACK: &[SchemaEntry] = &[
    ("gtk", "org.cinnamon.desktop.interface", "gtk-theme"),
    ("icons", "org.cinnamon.desktop.interface", "icon-theme"),
    ("cursor", "org.cinnamon.desktop.interface", "cursor-theme"),
    ("font", "org.cinnamon.desktop.interface", "font-name"),
];

// MATE: the cursor is a separate schema org.mate.peripherals-mouse (not interface!).
const MATE_STACK: &[SchemaEntry] = &[
    ("gtk", "org.mate.interface", "gtk-theme"),
    ("icons", "org.mate.interface", "icon-theme"),
    ("cursor", "org.mate.peripherals-mouse", "cursor-theme"),
    ("font", "org.mate.interface", "font-name"),
];

/// Generic GTK path for tiling / wlroots WMs — port of `_GENERIC_WM_SCHEMAS`.
const GENERIC_WM_STACK: &[SchemaEntry] = GNOME_STACK;

/// The `SCHEMAS` table (de → set of roles). budgie/cosmic/hyprland rely on the
/// GNOME stack; cinnamon-mint is an alias of cinnamon.
const SCHEMAS: &[(&str, &[SchemaEntry])] = &[
    ("gnome", GNOME_STACK),
    ("cinnamon", CINNAMON_STACK),
    ("cinnamon-mint", CINNAMON_STACK),
    ("mate", MATE_STACK),
    ("budgie", GNOME_STACK),
    // Pantheon reads the GNOME keys (pantheon-default-settings overrides exactly
    // org.gnome.desktop.interface gtk-theme/icon-theme/cursor-theme/font-name for
    // `:Pantheon`). ⚠️ gtk-theme is overwritten by io.elementary.settings-daemon
    // at login, on accent change and — with the "auto" accent — on wallpaper
    // change; the condition is told to the person as a matrix caveat.
    ("pantheon", GNOME_STACK),
    ("cosmic", GNOME_STACK),
    ("hyprland", GNOME_STACK),
];

/// DEs with their own mechanism (not generic-WM) — port of `_NATIVE_DES` (lowercase).
const NATIVE_DES: &[&str] = &[
    "kde", "gnome", "cinnamon", "cinnamon-mint", "mate", "budgie", "pantheon",
    "cosmic", "lxqt", "xfce", "deepin",
];

/// Result of `_resolve_schemas`: a gsettings set of roles or a "non-gsettings"
/// backend (xfce → xfconf, kde → kdeglobals, deepin → DConfig read-only,
/// writing — stop [`ApplyStep::DdeTreelandGate`], see [`crate::dde`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Schemas {
    /// A set of roles (a native DE from SCHEMAS or the generic GTK path for a tiling WM).
    Gsettings(&'static [SchemaEntry]),
    /// xfce / kde — not managed via gsettings.
    NonGsettings,
}

impl Schemas {
    /// `schemas["role"]` → (schema, prop). None for a missing role / non-gsettings.
    pub fn role(&self, role: &str) -> Option<(&'static str, &'static str)> {
        match self {
            Schemas::Gsettings(list) => list
                .iter()
                .find(|(r, _, _)| *r == role)
                .map(|(_, s, p)| (*s, *p)),
            Schemas::NonGsettings => None,
        }
    }
}

/// Port of `_resolve_schemas` (CASE-SENSITIVE, like the original
/// `SCHEMAS.get(de)`): the set from SCHEMAS; for "xfce"/"kde" —
/// [`Schemas::NonGsettings`]; otherwise the generic GTK path for tiling/wlroots
/// WMs.
pub fn resolve_schemas(de: &str) -> Schemas {
    if let Some((_, list)) = SCHEMAS.iter().find(|(k, _)| *k == de) {
        return Schemas::Gsettings(list);
    }
    if de == "xfce" || de == "kde" || de == "deepin" {
        return Schemas::NonGsettings;
    }
    Schemas::Gsettings(GENERIC_WM_STACK)
}

/// Port of `_is_generic_wm` (CASE-INSENSITIVE: `(de or "").lower()`).
pub fn is_generic_wm(de: &str) -> bool {
    let low = de.to_lowercase();
    !NATIVE_DES.contains(&low.as_str())
}

// ============================================================================
// Theme apply plan — argv/ini/files (executed when connected to the bridge)
// ============================================================================

/// A command for `safe_run` — argv + an optional timeout (sec; None = the
/// default 30 s). Port of the `safe_run([...], timeout=...)` calls.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Cmd {
    pub argv: Vec<String>,
    pub timeout: Option<u64>,
}

fn cmd(argv: &[&str], timeout: Option<u64>) -> Cmd {
    Cmd {
        argv: argv.iter().map(|s| (*s).to_string()).collect(),
        timeout,
    }
}

/// A step of the apply plan. The tag encodes the execution condition; the step
/// order itself is deterministic by (theme, de). The executor keeps the `ok` flag.
/// ⚠️ The variant tags and field names are the SHAPE read by the plan executor
/// (the shim `defaults_set_terminal_plan`/`themes_apply_*_plan` and Python
/// behind it). It is declared here, at the type itself, and not rewritten as a
/// branch per field in the bridge: a new variant reaches the executor by itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "op")]
pub enum ApplyStep {
    /// The main call whose returncode determines `ok`.
    #[serde(rename = "primary")]
    Primary(Cmd),
    /// An auxiliary call that doesn't change `ok` (kde-gtk gsettings etc.).
    #[serde(rename = "besteffort")]
    BestEffort(Cmd),
    /// Runs only if `ok` is already true (kde-icon dbus notification).
    #[serde(rename = "ifok")]
    IfOk(Cmd),
    /// Runs only if `ok` is still false; success of any makes `ok=true`
    /// (kde-cursor fallback: kwriteconfig6 into kcminputrc/kdeglobals).
    #[serde(rename = "ifnotok")]
    IfNotOk(Cmd),
    /// Update `settings.ini` (read-modify-write via [`update_gtk_settings_ini`]);
    /// doesn't affect `ok`.
    #[serde(rename = "gtkini")]
    GtkIni { path: String, key: String, value: String },
    /// Hyprland cursor (only if `hyprctl` is present): read the size
    /// (`gsettings get org.gnome.desktop.interface cursor-size`, default "24"),
    /// then `hyprctl setcursor <theme> <size>` and
    /// `hyprctl keyword env XCURSOR_THEME,<theme>`. Doesn't affect `ok`.
    ///
    /// With `size` set, it is used and `gsettings get` is not needed. A successful
    /// `setcursor` is "changed now" in [`CursorReport::live`].
    #[serde(rename = "hyprctl_cursor")]
    HyprctlCursor { theme: String, size: Option<String> },
    /// A "change in the running session" command (`swaymsg …`): doesn't affect
    /// `ok`, its success goes into [`CursorReport::live`]. No session — the command
    /// simply fails.
    #[serde(rename = "live")]
    Live(Cmd),
    /// Write OUR file (the directory is created); success — `ok = true`. For
    /// desktops without their own setting, where this file is the real effect
    /// (instead of the former unconditional `ForceOk`).
    #[serde(rename = "write_setsok")]
    WriteSetsOk {
        dir: String,
        path: String,
        content: String,
    },
    /// Cursor into the tiling compositor's config as a managed block — see
    /// [`WmConfigPlan`]. Doesn't affect `ok`; the result goes into [`CursorReport::wm`].
    #[serde(rename = "wm_config")]
    WmConfig(WmConfigPlan),
    /// `ok = true` unconditionally (kde-gtk and tiling/wlroots WMs: `settings.ini`
    /// is authoritative, the write doesn't fail silently).
    #[serde(rename = "forceok")]
    ForceOk,
    /// Deepin: the executor stops the plan here — `ok = false`, not a single write.
    /// In a Treeland session themes are set only by the Control Center, Rexon
    /// doesn't support other Deepin sessions; the reason is
    /// [`crate::dde::refusal`].
    #[serde(rename = "dde_treeland_gate")]
    DdeTreelandGate,
    /// Only with `ok`: `makedirs(dir)` + write `index_path` (for the Xorg cursor
    /// `~/.icons/default/index.theme`).
    #[serde(rename = "write_ifok")]
    WriteIfOk {
        dir: String,
        path: String,
        content: String,
    },
    /// Only with `ok`: edit a key in an ini file in place
    /// ([`crate::defaults::set_ini_value`]) — other settings in the file are kept.
    /// Needed where the config is not maintained by Rexon (`lxqt.conf`,
    /// `qt6ct.conf`). `section` — with brackets (`"[Qt]"`).
    #[serde(rename = "iniset_ifok")]
    IniSetIfOk {
        path: String,
        section: String,
        key: String,
        value: String,
    },
    /// Only with `ok`: edit a key in someone else's ini file WITH A CONDITION (see
    /// [`ini_guarded_text`]): `only_if_file` — no file means the program is not
    /// configured, and there is no need to create it; `keep_existing` — the key is
    /// already set by the person, don't override. Needed for `qt6ct.conf`.
    #[serde(rename = "iniset_guarded")]
    IniSetGuarded {
        path: String,
        section: String,
        key: String,
        value: String,
        only_if_file: bool,
        keep_existing: bool,
    },
    /// Only with `ok`: remove a key from the GTK `settings.ini`
    /// ([`remove_gtk_settings_key`]) — then GTK takes the desktop's value.
    /// Return to the standard cursor ([`standard_cursor_plan`]).
    #[serde(rename = "gtk_ini_unset_ifok")]
    GtkIniUnsetIfOk { path: String, key: String },
    /// Only with `ok`: delete OUR file (cursor variables in `environment.d`); no
    /// file is not an error.
    #[serde(rename = "remove_ifok")]
    RemoveIfOk { path: String },
    /// Files `paths` will be rewritten by the following plan commands
    /// (`kwriteconfig6 --file …`, the ini editing script): take a "what was" copy
    /// under the click's undo, and mark the ones created by the commands after the
    /// run. So an «Apply» that writes via commands leaves a snapshot and an
    /// `applied` record, just like writing by our own hands. Doesn't affect `ok`.
    #[serde(rename = "guard")]
    Guard { paths: Vec<String> },
    /// Read back (`read`) what the commands wrote: `ok = true` only if the command
    /// succeeded and its answer (without the `gsettings` quotes) equals `expect`;
    /// otherwise `ok = false`. `kwriteconfig6` and `gsettings` can exit with zero
    /// having written nothing.
    #[serde(rename = "readback_setsok")]
    ReadBackSetsOk { read: Cmd, expect: String },
}

/// The GTK `settings.ini` without the lines of key `key` (the rest byte for
/// byte). `None` — no such key, no reason to rewrite the file.
pub fn remove_gtk_settings_key(existing: &str, key: &str) -> Option<String> {
    let key_eq = format!("{key}=");
    let mut found = false;
    let kept: String = existing
        .split_inclusive('\n')
        .filter(|line| {
            let hit = line.trim().starts_with(&key_eq);
            found |= hit;
            !hit
        })
        .collect();
    found.then_some(kept)
}

/// Whether the ini text has the key `key` with a non-empty value in section
/// `section` (with brackets, `"[Appearance]"`).
pub fn ini_has_key(text: &str, section: &str, key: &str) -> bool {
    let mut inside = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with('[') && t.ends_with(']') {
            inside = t == section;
            continue;
        }
        if inside {
            if let Some((k, v)) = t.split_once('=') {
                if k.trim() == key && !v.trim().is_empty() {
                    return true;
                }
            }
        }
    }
    false
}

/// Decision of the [`ApplyStep::IniSetGuarded`] step: the new file text or
/// `None` (write nothing). `existing` is the current text, `None` — no file.
pub fn ini_guarded_text(
    existing: Option<&str>,
    section: &str,
    key: &str,
    value: &str,
    only_if_file: bool,
    keep_existing: bool,
) -> Option<String> {
    if only_if_file && existing.is_none() {
        return None;
    }
    if keep_existing && existing.is_some_and(|t| ini_has_key(t, section, key)) {
        return None;
    }
    Some(crate::defaults::set_ini_value(existing, section, key, value))
}

/// Where to read the icon theme the desktop currently gives to programs (the
/// new qt6ct config gets it — see [`KvantumEnv::icon_theme`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IconThemeSource {
    /// Read command: `gsettings get …` or `xfconf-query …`.
    Cmd(Cmd),
    /// The GTK `settings.ini`, key `gtk-icon-theme-name`.
    GtkIni(String),
}

/// Source of the current icon theme — the same keys [`apply_icon_plan_xdg`]
/// writes it to.
pub fn current_icon_theme_source(de: &str, home: &str, xdg_config_home: Option<&str>) -> IconThemeSource {
    match resolve_schemas(de) {
        s @ Schemas::Gsettings(_) => {
            if let Some((schema, key)) = s.role("icons") {
                return IconThemeSource::Cmd(cmd(&["gsettings", "get", schema, key], Some(5)));
            }
        }
        Schemas::NonGsettings if de == "xfce" => {
            return IconThemeSource::Cmd(cmd(
                &["xfconf-query", "-c", "xsettings", "-p", "/Net/IconThemeName"],
                Some(5),
            ));
        }
        Schemas::NonGsettings if de == "deepin" => {
            return IconThemeSource::Cmd(Cmd {
                argv: crate::dde::dconfig_get_argv(crate::dde::ICON_THEME),
                timeout: Some(5),
            });
        }
        Schemas::NonGsettings => {}
    }
    IconThemeSource::GtkIni(gtk_ini_path(home, xdg_config_home, "gtk-3.0"))
}

/// Roles that a desktop with its own looks storage keeps itself: for them the
/// active themes map asks the desktop itself ([`native_role_read_cmd`]).
pub const NATIVE_STORE_ROLES: &[&str] = &["gtk", "icons", "cursor"];

/// Command to read the role `gtk`/`icons`/`cursor` from the desktop's NATIVE
/// storage — the same key the apply plans write to
/// ([`apply_gtk_plan_xdg`], [`apply_icon_plan_xdg`], [`apply_cursor_plan_sized`]):
/// gsettings by the schemas table (GNOME, Cinnamon, MATE, Budgie, Pantheon,
/// COSMIC), xfconf for XFCE, DConfig for Deepin.
///
/// `None` — the desktop has no such storage or it isn't the main one: KDE (the
/// map reads kdeglobals itself), LXQt and tiling WMs (their gsettings is only a
/// hint for GTK programs, the main thing for them is `settings.ini`, the
/// compositor config and environment variables).
pub fn native_role_read_cmd(de: &str, role: &str) -> Option<Cmd> {
    if !NATIVE_STORE_ROLES.contains(&role) || is_generic_wm(de) || de == "lxqt" {
        return None;
    }
    match resolve_schemas(de) {
        s @ Schemas::Gsettings(_) => {
            let (schema, key) = s.role(role)?;
            Some(cmd(&["gsettings", "get", schema, key], Some(5)))
        }
        Schemas::NonGsettings if de == "xfce" => {
            let prop = match role {
                "gtk" => "/Net/ThemeName",
                "icons" => "/Net/IconThemeName",
                _ => "/Gtk/CursorThemeName",
            };
            Some(cmd(&["xfconf-query", "-c", "xsettings", "-p", prop], Some(5)))
        }
        Schemas::NonGsettings if de == "deepin" => {
            let key = match role {
                "gtk" => crate::dde::GTK_THEME,
                "icons" => crate::dde::ICON_THEME,
                _ => crate::dde::CURSOR_THEME,
            };
            Some(Cmd { argv: crate::dde::dconfig_get_argv(key), timeout: Some(5) })
        }
        Schemas::NonGsettings => None,
    }
}

/// A command that names the STANDARD theme of role `role` of desktop `de` —
/// the one in place while the person hasn't chosen anything: the schema
/// default together with distribution overrides (`GSETTINGS_BACKEND=memory`
/// reads the schema bypassing written values). This is how the value is found
/// out, not by our table of guesses: Cinnamon on CachyOS has Papirus icons by
/// default, not Adwaita from the upstream schema.
///
/// `None` — the desktop has no gsettings schema for the role (KDE, XFCE,
/// Deepin, LXQt, tiling WMs): there the standard themes (Breeze, elementary)
/// are not hidden and don't fall under the cursor protection, no special
/// handling is needed.
pub fn standard_theme_read_cmd(de: &str, role: &str) -> Option<Cmd> {
    if !NATIVE_STORE_ROLES.contains(&role) || is_generic_wm(de) || de == "lxqt" {
        return None;
    }
    let (schema, key) = resolve_schemas(de).role(role)?;
    Some(cmd(&["env", "GSETTINGS_BACKEND=memory", "gsettings", "get", schema, key], Some(5)))
}

/// Answer of [`native_role_read_cmd`] → theme name: gsettings quotes and the
/// `dde-dconfig` type signature are stripped (`'Menta'\n` → `Menta`,
/// `s "Papirus"` → `Papirus`).
pub fn parse_native_role_output(de: &str, raw: &str) -> String {
    if de == "deepin" {
        crate::dde::parse_value(raw)
    } else {
        parse_icon_theme_output(raw)
    }
}

/// Answer of the read command → theme name (`'Papirus'\n` → `Papirus`).
pub fn parse_icon_theme_output(raw: &str) -> String {
    raw.trim().trim_matches(|c| c == '\'' || c == '"').trim().to_string()
}

/// Configs of the Qt style switchers that get the icon choice: without
/// `icon_theme` Qt programs under them take `hicolor`.
pub const QT_PLATFORM_THEME_TOOLS: &[&str] = &["qt6ct", "qt5ct"];

/// Theme apply plan: steps + the kind of history record (`_add_history` kind).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub steps: Vec<ApplyStep>,
    pub history_kind: &'static str,
}

/// Theme plan for Deepin: one stop before writing ([`ApplyStep::DdeTreelandGate`])
/// — in Treeland themes are set only by the Control Center, Rexon doesn't
/// support other Deepin sessions.
fn deepin_plan(history_kind: &'static str) -> Plan {
    Plan { steps: vec![ApplyStep::DdeTreelandGate], history_kind }
}

fn gtk_ini_path(home: &str, xdg_config_home: Option<&str>, gtk_ver: &str) -> String {
    format!(
        "{}/{gtk_ver}/settings.ini",
        crate::utils::config_home(xdg_config_home, home)
    )
}

fn gtk_ini_step(
    home: &str,
    xdg_config_home: Option<&str>,
    gtk_ver: &str,
    key: &str,
    value: &str,
) -> ApplyStep {
    ApplyStep::GtkIni {
        path: gtk_ini_path(home, xdg_config_home, gtk_ver),
        key: key.to_string(),
        value: value.to_string(),
    }
}

/// Edit a key in the LXQt config (`~/.config/lxqt/<file>`) in place — the file
/// is maintained by LXQt, other keys in it are kept.
fn lxqt_ini_step(
    home: &str,
    xdg_config_home: Option<&str>,
    file: &str,
    section: &str,
    key: &str,
    value: &str,
) -> ApplyStep {
    ApplyStep::IniSetIfOk {
        path: format!("{}/lxqt/{file}", crate::utils::config_home(xdg_config_home, home)),
        section: section.to_string(),
        key: key.to_string(),
        value: value.to_string(),
    }
}

/// Port of `apply_gtk_theme`. `home` expands `~`.
pub fn apply_gtk_plan(theme: &str, de: &str, home: &str) -> Plan {
    apply_gtk_plan_xdg(theme, de, home, None)
}

/// The same, honoring `$XDG_CONFIG_HOME` (the GTK `settings.ini` paths).
pub fn apply_gtk_plan_xdg(
    theme: &str,
    de: &str,
    home: &str,
    xdg_config_home: Option<&str>,
) -> Plan {
    apply_gtk_plan_scheme(theme, de, home, xdg_config_home, None)
}

/// Schema and key of the programs' light/dark mode: `org.gnome.desktop.
/// interface color-scheme` — read by libadwaita and the appearance portal
/// (`org.freedesktop.appearance color-scheme`). Set where the GTK theme is
/// written into the same GNOME schema (GNOME, Budgie, Pantheon, COSMIC, tiling
/// WMs): a dark GTK theme in light mode left libadwaita programs light (GNOME
/// check, 16.10.2026). In Cinnamon, MATE, XFCE and KDE the mode is managed by
/// the desktop itself (KDE — by the color scheme), we don't touch it.
pub const COLOR_SCHEME_KEY: (&str, &str) = ("org.gnome.desktop.interface", "color-scheme");

/// The [`COLOR_SCHEME_KEY`] value for a theme: dark — `prefer-dark`, light —
/// `default` (this is what "GNOME Settings → Appearance" writes: "Default" is
/// `default`, not `prefer-light`).
pub fn color_scheme_value(dark: bool) -> &'static str {
    if dark { "prefer-dark" } else { "default" }
}

/// Pantheon: the mode schema and key of the io.elementary.settings-daemon
/// service — written by "System Settings → Desktop → Appearance"
/// (switchboard-plug-pantheon-shell, `Appearance.vala`). The service passes the
/// value to AccountsService (Gala, the panel and elementary programs take the
/// mode from there) and to [`COLOR_SCHEME_KEY`]; writing the single GNOME key
/// left the shell light, and on a dark mode schedule the service reverted it.
pub const PANTHEON_COLOR_SCHEME_KEY: (&str, &str) =
    ("io.elementary.settings-daemon.prefers-color-scheme", "color-scheme");

/// The [`PANTHEON_COLOR_SCHEME_KEY`] value: "Default" (light) in elementary is
/// `no-preference`.
pub fn pantheon_color_scheme_value(dark: bool) -> &'static str {
    if dark { "prefer-dark" } else { "no-preference" }
}

/// Whether a GTK theme is dark. First — the window background color from its
/// GTK 3 stylesheet (`@define-color theme_bg_color …`, following the
/// `@bg_color`/`@window_bg_color` references in the same file) by the
/// product's common criterion [`crate::ui_base::is_dark_rgb`]; no color in the
/// file (the styles are in a GTK resource or included via `@import`) — by name:
/// "dark" in it (`Adwaita-dark`, `WhiteSur-Dark-nord`). The color matters more
/// than the name: "Windows XP Royale Dark" has a light window background.
pub fn gtk_theme_is_dark(name: &str, gtk_css: Option<&str>) -> bool {
    if let Some((r, g, b)) = gtk_css.and_then(gtk_css_window_bg) {
        return crate::ui_base::is_dark_rgb(r, g, b);
    }
    name.to_lowercase().contains("dark")
}

/// Window background color from `gtk.css`: `theme_bg_color`, `@name` references
/// resolved by `@define-color` of the same file (at most 8 steps, against loops).
fn gtk_css_window_bg(css: &str) -> Option<(u8, u8, u8)> {
    let mut colors: std::collections::HashMap<&str, &str> = std::collections::HashMap::new();
    for line in css.lines() {
        let Some(rest) = line.trim().strip_prefix("@define-color") else { continue };
        let mut parts = rest.trim().splitn(2, char::is_whitespace);
        let (Some(name), Some(value)) = (parts.next(), parts.next()) else { continue };
        let value = value.trim().trim_end_matches(';').trim();
        colors.entry(name).or_insert(value);
    }
    let mut value = *colors.get("theme_bg_color")?;
    for _ in 0..8 {
        match value.strip_prefix('@') {
            Some(alias) => value = colors.get(alias)?,
            None => return crate::ui_base::parse_hex(value),
        }
    }
    None
}

/// Like [`apply_gtk_plan_xdg`], plus the programs' mode: `dark` — whether the
/// theme is dark ([`gtk_theme_is_dark`]); `None` — don't touch the mode.
pub fn apply_gtk_plan_scheme(
    theme: &str,
    de: &str,
    home: &str,
    xdg_config_home: Option<&str>,
    dark: Option<bool>,
) -> Plan {
    if de == "deepin" {
        return deepin_plan("gtk");
    }
    let mut plan = apply_gtk_plan_steps(theme, de, home, xdg_config_home);
    let gnome_schema = resolve_schemas(de).role("gtk").is_some_and(|(s, _)| s == COLOR_SCHEME_KEY.0);
    if let (Some(dark), true) = (dark, gnome_schema) {
        // Only if the theme was written; doesn't affect ok.
        plan.steps.push(ApplyStep::IfOk(cmd(
            &["gsettings", "set", COLOR_SCHEME_KEY.0, COLOR_SCHEME_KEY.1, color_scheme_value(dark)],
            None,
        )));
        if de == "pantheon" {
            // No schema (the elementary service is not installed) — the step
            // simply fails; it doesn't affect ok.
            plan.steps.push(ApplyStep::IfOk(cmd(
                &[
                    "gsettings", "set", PANTHEON_COLOR_SCHEME_KEY.0, PANTHEON_COLOR_SCHEME_KEY.1,
                    pantheon_color_scheme_value(dark),
                ],
                None,
            )));
        }
    }
    plan
}

fn apply_gtk_plan_steps(
    theme: &str,
    de: &str,
    home: &str,
    xdg_config_home: Option<&str>,
) -> Plan {
    let mut steps = Vec::new();
    match resolve_schemas(de) {
        s @ Schemas::Gsettings(_) => {
            if let Some((schema, key)) = s.role("gtk") {
                steps.push(ApplyStep::Primary(cmd(
                    &["gsettings", "set", schema, key, theme],
                    None,
                )));
            }
        }
        Schemas::NonGsettings => {
            if de == "xfce" {
                steps.push(ApplyStep::Primary(cmd(
                    &["xfconf-query", "-c", "xsettings", "-p", "/Net/ThemeName", "-s", theme],
                    None,
                )));
            } else if de == "kde" {
                for v in ["gtk-3.0", "gtk-4.0"] {
                    steps.push(gtk_ini_step(home, xdg_config_home, v, "gtk-theme-name", theme));
                }
                steps.push(ApplyStep::BestEffort(cmd(
                    &["gsettings", "set", "org.gnome.desktop.interface", "gtk-theme", theme],
                    None,
                )));
                // The Plasma `gtkconfig` module (kded) hands the GTK theme to
                // running programs — via xsettingsd (X11/XWayland) and GSettings.
                // It doesn't learn about a manual write of `settings.ini`: a live
                // kded6 kept `Net/ThemeName "Breeze"` until re-login (D4, desktops
                // check 02.10.2026). The same call is made by the "Application
                // Style → GTK" page of System Settings. No kded — the step simply
                // fails, the files above are already written.
                steps.push(ApplyStep::BestEffort(cmd(
                    &[
                        "dbus-send", "--session", "--print-reply", "--dest=org.kde.GtkConfig",
                        "/GtkConfig", "org.kde.GtkConfig.setGtkTheme", &format!("string:{theme}"),
                    ],
                    Some(10),
                )));
                steps.push(ApplyStep::ForceOk);
            }
        }
    }
    // LXQt has no GTK theme storage of its own: lxqt-config-appearance writes the
    // gtk-3.0/gtk-4.0 `settings.ini` (and the same gsettings as above). Without
    // xsettingsd, GTK programs on LXQt read exactly the file — it is authoritative.
    if is_generic_wm(de) || de == "lxqt" {
        for v in ["gtk-3.0", "gtk-4.0"] {
            steps.push(gtk_ini_step(home, xdg_config_home, v, "gtk-theme-name", theme));
        }
        steps.push(ApplyStep::ForceOk);
    }
    Plan { steps, history_kind: "gtk" }
}

/// Port of `apply_icon_theme`. `valid` is the result of [`is_valid_icon_theme`]
/// (pre-flight): with `false` Python refuses — here `None` (no plan).
pub fn apply_icon_plan(theme: &str, de: &str, home: &str, valid: bool) -> Option<Plan> {
    apply_icon_plan_xdg(theme, de, home, None, valid)
}

/// The same, honoring `$XDG_CONFIG_HOME`.
pub fn apply_icon_plan_xdg(
    theme: &str,
    de: &str,
    home: &str,
    xdg_config_home: Option<&str>,
    valid: bool,
) -> Option<Plan> {
    if !valid {
        return None;
    }
    if de == "deepin" {
        return Some(deepin_plan("icons"));
    }
    let mut steps = Vec::new();
    match resolve_schemas(de) {
        s @ Schemas::Gsettings(_) => {
            if let Some((schema, key)) = s.role("icons") {
                steps.push(ApplyStep::Primary(cmd(
                    &["gsettings", "set", schema, key, theme],
                    None,
                )));
            }
        }
        Schemas::NonGsettings => {
            if de == "xfce" {
                steps.push(ApplyStep::Primary(cmd(
                    &["xfconf-query", "-c", "xsettings", "-p", "/Net/IconThemeName", "-s", theme],
                    None,
                )));
            } else if de == "kde" {
                // `--notify`: the Plasma `gtkconfig` module and running KDE
                // programs watch kdeglobals via KConfigWatcher, and the signal
                // is sent only on a write with notification. Without it GTK
                // programs under Plasma got the new icons only after re-login
                // (D4, desktops check 02.10.2026: with `--notify`
                // `org.gnome.desktop.interface icon-theme` changed at once).
                steps.push(ApplyStep::Primary(cmd(
                    &[
                        "kwriteconfig6", "--file", "kdeglobals", "--group", "Icons", "--key", "Theme",
                        "--notify", theme,
                    ],
                    None,
                )));
                steps.push(ApplyStep::IfOk(cmd(
                    &["dbus-send", "--type=signal", "/KIconLoader", "org.kde.KIconLoader.iconChanged", "int32:0"],
                    None,
                )));
            }
        }
    }
    if de == "cosmic" {
        // The panel, cosmic-files and other libcosmic programs take icons not from
        // gsettings but from their own key — "COSMIC Settings → Appearance" writes
        // them there too (cosmic-settings `drawer.rs`:
        // `tk_config.set::<String>("icon_theme", …)`). A successful write means the
        // icons changed for them too, even if gsettings above didn't answer.
        if let Some(step) = cosmic_icon_theme_step(theme, home, xdg_config_home) {
            steps.push(step);
        }
    }
    // On any DE — gtk-3.0 and gtk-4.0 settings.ini.
    for v in ["gtk-3.0", "gtk-4.0"] {
        steps.push(gtk_ini_step(home, xdg_config_home, v, "gtk-icon-theme-name", theme));
    }
    // For tiling WMs and LXQt the files are authoritative, not gsettings: without
    // dconf it fails, and previously `ForceOk` stood AFTER the "if ok" writes — qt6ct
    // and kdeglobals below were then silently skipped. Hence — before them.
    if is_generic_wm(de) || de == "lxqt" {
        steps.push(ApplyStep::ForceOk);
    }
    if de == "lxqt" {
        // The Qt part of LXQt (panel, pcmanfm-qt, lxqt-*) takes icons from its
        // `lxqt.conf [General] icon_theme` — lxqt-config-appearance writes there too,
        // and LXQt picks up the file on the fly. gsettings above is only for GTK.
        steps.push(lxqt_ini_step(home, xdg_config_home, "lxqt.conf", "[General]", "icon_theme", theme));
    }
    let cfg = crate::utils::config_home(xdg_config_home, home);
    if de != "kde" {
        // KDE programs (Dolphin, Kate, Okular — KIconThemes) also outside Plasma
        // take the icon set from `kdeglobals [Icons] Theme`; without this write
        // they stayed on Breeze in any desktop except KDE. Plasma writes the same
        // key, so the file is edited pointwise, other things are not touched.
        steps.push(ApplyStep::IniSetGuarded {
            path: format!("{cfg}/kdeglobals"),
            section: "[Icons]".into(),
            key: "Theme".into(),
            value: theme.into(),
            only_if_file: false,
            keep_existing: false,
        });
    }
    // Qt programs under qt6ct/qt5ct take icons from their config, not from the
    // desktop: without this write the icon choice didn't reach Qt (D2, desktops
    // check 02.10.2026). Only if the config already exists — as with fonts: no
    // file means the switcher is not configured.
    for tool in QT_PLATFORM_THEME_TOOLS {
        steps.push(ApplyStep::IniSetGuarded {
            path: format!("{cfg}/{tool}/{tool}.conf"),
            section: "[Appearance]".into(),
            key: "icon_theme".into(),
            value: theme.into(),
            only_if_file: true,
            keep_existing: false,
        });
    }
    Some(Plan { steps, history_kind: "icons" })
}

/// The `cosmic-config` key with the libcosmic icon theme — a path from
/// `$XDG_CONFIG_HOME`. Checked against libcosmic `src/config/mod.rs`
/// (`ID = "com.system76.CosmicTk"`, `#[version = 1]`, field `icon_theme:
/// String`) and `cosmic-config` (`Config::new`: `<config>/cosmic/<ID>/v<version>`,
/// one key — one file).
pub const COSMIC_TK_ICON_THEME_REL: &str = "cosmic/com.system76.CosmicTk/v1/icon_theme";

/// A string in a RON record — as `cosmic-config` writes it
/// (`ron::ser::to_string_pretty`): in double quotes, `\` and `"` escaped.
/// Control characters (newline etc.) in the theme name or path — `None`: COSMIC
/// would not read such a value as intended, and we won't write it.
pub fn cosmic_ron_string(value: &str) -> Option<String> {
    if value.is_empty() || value.chars().any(char::is_control) {
        return None;
    }
    Some(format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\"")))
}

/// Step that writes the icon theme into the COSMIC key ([`COSMIC_TK_ICON_THEME_REL`]).
fn cosmic_icon_theme_step(theme: &str, home: &str, xdg_config_home: Option<&str>) -> Option<ApplyStep> {
    let content = cosmic_ron_string(theme)?;
    let cfg = crate::utils::config_home(xdg_config_home, home);
    let path = format!("{cfg}/{COSMIC_TK_ICON_THEME_REL}");
    let dir = path.rsplit_once('/').map(|(d, _)| d.to_string()).unwrap_or_default();
    Some(ApplyStep::WriteSetsOk { dir, path, content })
}

/// Port of `apply_cursor_theme`. `safe` is the result of [`cursor_preflight_ok`]
/// (the sanitizer): with `false` Python refuses — here `None`.
pub fn apply_cursor_plan(theme: &str, de: &str, home: &str, safe: bool) -> Option<Plan> {
    apply_cursor_plan_xdg(theme, de, home, None, safe)
}

/// The same, honoring `$XDG_CONFIG_HOME`. The cursor size is not touched — see
/// [`apply_cursor_plan_sized`].
pub fn apply_cursor_plan_xdg(
    theme: &str,
    de: &str,
    home: &str,
    xdg_config_home: Option<&str>,
    safe: bool,
) -> Option<Plan> {
    apply_cursor_plan_sized(theme, None, de, home, xdg_config_home, safe)
}

// ============================================================================
// Cursor in all desktops: theme + size
// ============================================================================
//
// Who reads what (checked against primary sources 30.09.2026):
//   * KDE — `kcminputrc [Mouse] cursorTheme/cursorSize`, written by
//     `plasma-apply-cursortheme [--size N]` (help of the installed binary);
//   * GNOME/Budgie/Pantheon/COSMIC — `org.gnome.desktop.interface
//     cursor-theme/size` (Gala is the same Mutter),
//     Cinnamon — `org.cinnamon.desktop.interface`, MATE —
//     `org.mate.peripherals-mouse`; XFCE — xsettings `/Gtk/CursorThemeName`
//     and `/Gtk/CursorThemeSize` (xfsettingsd strings); LXQt —
//     `session.conf [Mouse] cursor_theme/cursor_size` (read by lxqt-session at
//     login);
//   * sway — `seat <name> xcursor_theme <theme> [<size>]` (sway-input(5));
//     seat0 also gives XCURSOR_THEME/XCURSOR_SIZE to its children and XWayland;
//     `include <path>` (sway(5)) — the last one to set it wins;
//   * niri — `cursor { xcursor-theme "…"; xcursor-size N }` (niri wiki,
//     Configuration: Miscellaneous), `include "file.kdl"` is positional since
//     25.11, sections are merged; the config is reread by itself, the cursor
//     changes on the fly (niri.rs: `cursor_manager.reload`);
//   * Hyprland — its own pointer at start takes HYPRCURSOR_THEME/SIZE, and if
//     there is no such hyprcursor theme — XCURSOR_THEME/SIZE
//     (CursorManager.cpp v0.56.2); `hyprctl setcursor` changes it in the
//     current session for XCursor themes too (fallback to XCursor with the same
//     name). The 0.56 config is `hyprland.lua` if it exists, otherwise the old
//     `hyprland.conf` (Jeremy.cpp v0.56.2); in Lua other files are included
//     with `require` (wiki: Configuring → Core), in the old one with `source =`;
//   * everything else (Qt without a platform theme, Electron, XWayland
//     programs) — XCURSOR_THEME/XCURSOR_SIZE, and without them the `default`
//     theme, i.e. `~/.icons/default/index.theme`. We put the variables into
//     `~/.config/environment.d` by the same mechanism as for the terminal and
//     the Qt style.

/// Cursor sizes to choose from — as in the KDE and GNOME settings.
pub const CURSOR_SIZES: &[u32] = &[24, 32, 40, 48, 64, 96];

/// The size when nobody set it.
pub const CURSOR_DEFAULT_SIZE: u32 = 24;

/// Rexon environment variables file for the cursor (next to `rexon-qt-style.conf`).
pub const CURSOR_ENV_FILE: &str = "rexon-cursor.conf";

/// Marker of the Rexon managed block in someone else's compositor config.
pub const CURSOR_BLOCK_TAG: &str = "cursor";

/// A tiling compositor whose own pointer is set in its config.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CursorWm {
    Sway,
    Niri,
    Hyprland,
}

impl CursorWm {
    /// By desktop identifier (`system::detect_current_de`).
    pub fn from_de(de: &str) -> Option<Self> {
        match de.to_lowercase().as_str() {
            "sway" => Some(CursorWm::Sway),
            "niri" => Some(CursorWm::Niri),
            "hyprland" => Some(CursorWm::Hyprland),
            _ => None,
        }
    }

    /// Human-readable name.
    pub fn label(self) -> &'static str {
        match self {
            CursorWm::Sway => "sway",
            CursorWm::Niri => "niri",
            CursorWm::Hyprland => "Hyprland",
        }
    }
}

/// Syntax of the compositor config — the comment, the include line of our file
/// and our file itself depend on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WmSyntax {
    Sway,
    Kdl,
    HyprLua,
    HyprConf,
}

impl WmSyntax {
    /// Start of a line comment.
    pub fn comment(self) -> &'static str {
        match self {
            WmSyntax::Sway | WmSyntax::HyprConf => "#",
            WmSyntax::Kdl => "//",
            WmSyntax::HyprLua => "--",
        }
    }
}

/// One possible compositor config and our file next to it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WmConfigFile {
    /// The compositor config (maintained by the person; we add one line).
    pub path: String,
    pub syntax: WmSyntax,
    /// Our file (we maintain it and rewrite it whole).
    pub own_path: String,
    pub own_content: String,
    /// The include line of our file — the only thing that goes into the person's
    /// config (inside the managed block).
    pub include_line: String,
}

/// Writing the cursor into a tiling compositor's config — what the
/// [`ApplyStep::WmConfig`] executor does:
///   1. `min_version`: the binary exists and is older — touch nothing (`include`
///      appeared in niri 25.11, an old niri won't read a config with it);
///   2. the first EXISTING file from `candidates` is taken (the compositor's own
///      search order). None exists: if there is a file from `foreign` (sway
///      reads the i3 config, and i3 doesn't know the `seat` command) — don't
///      touch; otherwise, if there is a `seed` (the system sway config), it is
///      copied in place of the personal one — sway itself recommends this, the
///      behaviour doesn't change; otherwise there is no personal config, the
///      compositor creates it itself on first start, and until then the cursor
///      comes via `~/.icons/default`;
///   3. our file is written, then a managed block goes into the config
///      ([`upsert_managed_block`]); before the first edit a copy
///      `<config>.rexon-bak` is put next to it;
///   4. `validate` (argv, the config path is appended at the end): if the
///      config was valid and isn't after the edit — the original text is
///      restored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WmConfigPlan {
    pub wm: CursorWm,
    pub candidates: Vec<WmConfigFile>,
    pub foreign: Vec<String>,
    pub seed: Option<WmSeed>,
    pub validate: Vec<String>,
    pub min_version: Option<WmVersionGate>,
}

/// What to seed a missing personal config with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WmSeed {
    pub from: String,
    /// Index in `candidates` whose place the copy takes.
    pub candidate: usize,
}

/// Minimal compositor version: argv of the version query and the threshold (year, month).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WmVersionGate {
    pub argv: Vec<String>,
    pub min: (u32, u32),
}

/// How writing into the compositor config ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WmConfigOutcome {
    /// The block is in place (written now or was already there).
    Written,
    /// No personal config — the compositor will create it itself on first start.
    NoConfig,
    /// The config is foreign (sway on the i3 config) — not touched.
    Foreign,
    /// The compositor is too old for `include`.
    TooOld,
    /// Writing failed (permissions, disk, a link into read-only storage).
    Failed,
    /// Written, the compositor's check rejected the config — restored as it was.
    RolledBack,
}

/// Result of running the cursor plan: the overall `ok` plus what is needed to
/// honestly tell the person where and when the cursor will change.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CursorReport {
    pub ok: bool,
    /// Whether the "change now" command succeeded (`swaymsg`, `hyprctl setcursor`);
    /// `None` — there was no such command in the plan.
    pub live: Option<bool>,
    /// Write into the compositor config: compositor, result, config path.
    pub wm: Option<(CursorWm, WmConfigOutcome, String)>,
    /// Why it wasn't applied, if the reason is known exactly (Deepin:
    /// [`crate::dde::refusal`]). It is shown instead of the generic "not applied".
    pub reason: Option<String>,
}

/// Whether a theme name can be put into the compositor config as is. Theme names
/// are directory names; quotes, backslash, comma (the `env =` separator in
/// Hyprland) and control characters don't occur in them, and if they do — we
/// won't carry such a thing into the person's config.
pub fn wm_safe_theme_name(theme: &str) -> bool {
    !theme.is_empty()
        && !theme.starts_with('-')
        && !theme.chars().any(|c| c.is_control() || matches!(c, '"' | '\\' | ',' | '\'' | '$' | '`'))
}

/// Theme name in a sway command: in quotes if it contains a space.
fn sway_quote(theme: &str) -> String {
    if theme.contains(' ') {
        format!("\"{theme}\"")
    } else {
        theme.to_string()
    }
}

/// A path in a sway `include`. The argument goes through three passes in sway:
/// ALL unescaped quotes are stripped from it, then `\` escaping is removed,
/// `$name` is replaced by a variable (`$$` is `$` itself), and only after that
/// the path goes to wordexp, which splits on spaces. So the quotes for wordexp
/// are written escaped (`\'`), and the whole argument is in double quotes so
/// that sway doesn't split it earlier. Verified with `sway --validate` (1.12)
/// on a path with a space, an apostrophe and `$`.
pub fn sway_path(path: &str) -> String {
    // Letters of any alphabet (`~/Загрузки`) are touched by neither sway nor wordexp.
    if path.chars().all(|c| c.is_alphanumeric() || "/._-+".contains(c)) {
        return path.to_string();
    }
    let words = format!("'{}'", path.replace('\'', "'\\''"));
    let escaped = words
        .replace('\\', "\\\\")
        .replace('\'', "\\'")
        .replace('"', "\\\"")
        .replace('$', "$$");
    format!("\"{escaped}\"")
}

/// Our file and the include line for the compositor config.
fn wm_config_file(
    syntax: WmSyntax,
    path: String,
    own_path: String,
    theme: &str,
    size: Option<u32>,
) -> WmConfigFile {
    let head = format!(
        "{} Rexon: cursor theme. This file is rewritten by Rexon; set the cursor in Rexon instead.\n",
        syntax.comment()
    );
    let (own_content, include_line) = match syntax {
        WmSyntax::Sway => {
            let size = size.map(|s| format!(" {s}")).unwrap_or_default();
            (
                format!("{head}seat * xcursor_theme {}{size}\n", sway_quote(theme)),
                format!("include {}", sway_path(&own_path)),
            )
        }
        WmSyntax::Kdl => {
            let size = size.map(|s| format!("    xcursor-size {s}\n")).unwrap_or_default();
            let name = own_path.rsplit('/').next().unwrap_or(&own_path).to_string();
            (
                format!("{head}cursor {{\n    xcursor-theme \"{theme}\"\n{size}}}\n"),
                // Relative to config.kdl itself: our file lies next to it.
                format!("include \"{name}\""),
            )
        }
        WmSyntax::HyprLua => {
            let mut body = format!(
                "{head}hl.env(\"XCURSOR_THEME\", \"{theme}\")\nhl.env(\"HYPRCURSOR_THEME\", \"{theme}\")\n"
            );
            if let Some(s) = size {
                body.push_str(&format!(
                    "hl.env(\"XCURSOR_SIZE\", \"{s}\")\nhl.env(\"HYPRCURSOR_SIZE\", \"{s}\")\n"
                ));
            }
            // `pcall`: should our file vanish, the person's config keeps loading
            // (a bare `require` of a missing module aborts the whole config).
            (body, "pcall(require, \"rexon-cursor\")".to_string())
        }
        WmSyntax::HyprConf => {
            let mut body =
                format!("{head}env = XCURSOR_THEME,{theme}\nenv = HYPRCURSOR_THEME,{theme}\n");
            if let Some(s) = size {
                body.push_str(&format!("env = XCURSOR_SIZE,{s}\nenv = HYPRCURSOR_SIZE,{s}\n"));
            }
            (body, format!("source = {own_path}"))
        }
    };
    WmConfigFile { path, syntax, own_path, own_content, include_line }
}

/// Plan for writing the cursor into the compositor config (see [`WmConfigPlan`]).
pub fn wm_cursor_config_plan(
    wm: CursorWm,
    theme: &str,
    size: Option<u32>,
    home: &str,
    xdg_config_home: Option<&str>,
) -> WmConfigPlan {
    let cfg = crate::utils::config_home(xdg_config_home, home);
    let home = home.trim_end_matches('/');
    let file = |syntax, path: String, own: String| wm_config_file(syntax, path, own, theme, size);
    match wm {
        CursorWm::Sway => WmConfigPlan {
            wm,
            // sway(1) search order: ~/.sway/config, $XDG_CONFIG_HOME/sway/config.
            candidates: vec![
                file(WmSyntax::Sway, format!("{home}/.sway/config"), format!("{home}/.sway/rexon-cursor.conf")),
                file(WmSyntax::Sway, format!("{cfg}/sway/config"), format!("{cfg}/sway/rexon-cursor.conf")),
            ],
            foreign: vec![format!("{home}/.i3/config"), format!("{cfg}/i3/config")],
            seed: Some(WmSeed { from: "/etc/sway/config".into(), candidate: 1 }),
            validate: vec!["sway".into(), "--validate".into(), "--config".into()],
            min_version: None,
        },
        CursorWm::Niri => WmConfigPlan {
            wm,
            candidates: vec![file(
                WmSyntax::Kdl,
                format!("{cfg}/niri/config.kdl"),
                format!("{cfg}/niri/rexon-cursor.kdl"),
            )],
            foreign: Vec::new(),
            seed: None,
            validate: vec!["niri".into(), "validate".into(), "--config".into()],
            min_version: Some(WmVersionGate {
                argv: vec!["niri".into(), "--version".into()],
                min: (25, 11),
            }),
        },
        CursorWm::Hyprland => WmConfigPlan {
            wm,
            // Hyprland 0.56: hyprland.lua if it exists, otherwise hyprland.conf.
            candidates: vec![
                file(WmSyntax::HyprLua, format!("{cfg}/hypr/hyprland.lua"), format!("{cfg}/hypr/rexon-cursor.lua")),
                file(WmSyntax::HyprConf, format!("{cfg}/hypr/hyprland.conf"), format!("{cfg}/hypr/rexon-cursor.conf")),
            ],
            foreign: Vec::new(),
            seed: None,
            validate: vec!["Hyprland".into(), "--verify-config".into(), "--config".into()],
            min_version: None,
        },
    }
}

/// The "year.month" version from the `niri --version` output (`niri 26.04 (8ed0da4)`).
pub fn parse_wm_version(out: &str) -> Option<(u32, u32)> {
    let word = out.split_whitespace().find(|w| w.chars().next().is_some_and(|c| c.is_ascii_digit()))?;
    let mut parts = word.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts
        .next()
        .map(|m| m.chars().take_while(char::is_ascii_digit).collect::<String>())
        .and_then(|m| m.parse().ok())
        .unwrap_or(0);
    Some((major, minor))
}

fn block_markers(comment: &str, tag: &str) -> (String, String) {
    (
        format!("{comment} >>> Rexon: {tag} (managed by Rexon, do not edit inside) >>>"),
        format!("{comment} <<< Rexon: {tag} <<<"),
    )
}

/// Insert or update the Rexon managed block in someone else's config.
///
/// The block goes at the END of the file: in sway, niri and Hyprland the last
/// set value wins, and everything the person wrote before it stays as it was.
/// An existing block is rewritten in place (applying again doesn't breed
/// lines). Everything outside the markers doesn't change by a single byte.
pub fn upsert_managed_block(existing: &str, comment: &str, tag: &str, body: &str) -> String {
    let (begin, end) = block_markers(comment, tag);
    let block = format!("{begin}\n{}\n{end}\n", body.trim_end());
    if let (Some(b), Some(e)) = (existing.find(&begin), existing.find(&end)) {
        if b < e {
            let after = &existing[e + end.len()..];
            let after = after.strip_prefix('\n').unwrap_or(after);
            return format!("{}{block}{after}", &existing[..b]);
        }
    }
    let mut out = existing.to_string();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    if !out.is_empty() {
        out.push('\n');
    }
    out.push_str(&block);
    out
}

/// Undo: remove the Rexon managed block (together with the empty line added by
/// [`upsert_managed_block`]). No block — the text as is.
pub fn remove_managed_block(existing: &str, comment: &str, tag: &str) -> String {
    let (begin, end) = block_markers(comment, tag);
    let (Some(b), Some(e)) = (existing.find(&begin), existing.find(&end)) else {
        return existing.to_string();
    };
    if b > e {
        return existing.to_string();
    }
    let before = &existing[..b];
    let before = before.strip_suffix("\n\n").map(|s| format!("{s}\n")).unwrap_or_else(|| before.to_string());
    let after = &existing[e + end.len()..];
    let after = after.strip_prefix('\n').unwrap_or(after);
    format!("{before}{after}")
}

/// Contents of `environment.d/rexon-cursor.conf`.
pub fn cursor_env_content(theme: &str, size: Option<u32>) -> String {
    let mut s = format!("XCURSOR_THEME={theme}\n");
    if let Some(n) = size {
        s.push_str(&format!("XCURSOR_SIZE={n}\n"));
    }
    s
}

/// Cursor apply plan: the theme and (if set) the size — in all desktops.
///
/// `size == None` — the size is not touched (this is how the old Python path calls it).
///
/// Where `ok` comes from the desktop's native key (KDE, gsettings, xfconf), it
/// stays so. For tiling compositors and other desktops without their own cursor
/// setting the former unconditional `ForceOk` is removed: `ok` comes from
/// writing OUR environment variables file (it is the real effect — after
/// re-login), and what changed at once and where, the executor collects into
/// [`CursorReport`] for an honest message ([`cursor_apply_message`]).
pub fn apply_cursor_plan_sized(
    theme: &str,
    size: Option<u32>,
    de: &str,
    home: &str,
    xdg_config_home: Option<&str>,
    safe: bool,
) -> Option<Plan> {
    if !safe {
        return None;
    }
    if de == "deepin" {
        return Some(deepin_plan("cursor"));
    }
    let cfg = crate::utils::config_home(xdg_config_home, home);
    let size_s = size.map(|n| n.to_string());
    let generic = is_generic_wm(de);
    let mut steps = Vec::new();
    match resolve_schemas(de) {
        s @ Schemas::Gsettings(_) => {
            if let Some((schema, key)) = s.role("cursor") {
                let set_theme = cmd(&["gsettings", "set", schema, key, theme], None);
                // For tiling WMs gsettings is only a hint for GTK programs via the
                // portal; without dconf it fails, and the compositor's cursor
                // doesn't depend on it. It doesn't decide.
                steps.push(if generic { ApplyStep::BestEffort(set_theme) } else { ApplyStep::Primary(set_theme) });
                if let Some(n) = &size_s {
                    // The size is in the same schema as the theme (for MATE it is
                    // `org.mate.peripherals-mouse`, not interface).
                    steps.push(ApplyStep::BestEffort(cmd(&["gsettings", "set", schema, "cursor-size", n], None)));
                }
            }
        }
        Schemas::NonGsettings => {
            if de == "xfce" {
                steps.push(ApplyStep::Primary(cmd(
                    &["xfconf-query", "-c", "xsettings", "-p", "/Gtk/CursorThemeName", "-s", theme],
                    None,
                )));
                if let Some(n) = &size_s {
                    // `-n -t int`: the property may not exist yet (the size was never changed).
                    steps.push(ApplyStep::BestEffort(cmd(
                        &["xfconf-query", "-c", "xsettings", "-p", "/Gtk/CursorThemeSize", "-n", "-t", "int", "-s", n],
                        None,
                    )));
                }
            } else if de == "kde" {
                let primary = match &size_s {
                    Some(n) => cmd(&["plasma-apply-cursortheme", "--size", n, theme], None),
                    None => cmd(&["plasma-apply-cursortheme", theme], None),
                };
                steps.push(ApplyStep::Primary(primary));
                steps.push(ApplyStep::IfNotOk(cmd(
                    &["kwriteconfig6", "--file", "kcminputrc", "--group", "Mouse", "--key", "cursorTheme", theme],
                    None,
                )));
                steps.push(ApplyStep::IfNotOk(cmd(
                    &["kwriteconfig6", "--file", "kdeglobals", "--group", "Mouse", "--key", "cursorTheme", theme],
                    None,
                )));
                if let Some(n) = &size_s {
                    steps.push(ApplyStep::IfOk(cmd(
                        &["kwriteconfig6", "--file", "kcminputrc", "--group", "Mouse", "--key", "cursorSize", n],
                        None,
                    )));
                }
            }
        }
    }
    // On any DE — gtk-3.0 and gtk-4.0 settings.ini.
    for v in ["gtk-3.0", "gtk-4.0"] {
        steps.push(gtk_ini_step(home, xdg_config_home, v, "gtk-cursor-theme-name", theme));
        if let Some(n) = &size_s {
            steps.push(gtk_ini_step(home, xdg_config_home, v, "gtk-cursor-theme-size", n));
        }
    }
    if de == "lxqt" {
        // The native LXQt key is `session.conf [Mouse] cursor_theme` (written by
        // lxqt-config-appearance and lxqt-config-input, read by lxqt-session at
        // login): the cursor of the whole session changes after re-login.
        steps.push(ApplyStep::ForceOk);
        steps.push(lxqt_ini_step(home, xdg_config_home, "session.conf", "[Mouse]", "cursor_theme", theme));
        if let Some(n) = &size_s {
            steps.push(lxqt_ini_step(home, xdg_config_home, "session.conf", "[Mouse]", "cursor_size", n));
        }
    }
    // Environment variables — for everyone except KDE: Plasma sets XCURSOR_* from
    // kcminputrc itself at login, our file there would be a second, lagging
    // source.
    if de != "kde" {
        let env = (
            format!("{cfg}/environment.d"),
            format!("{cfg}/environment.d/{CURSOR_ENV_FILE}"),
            cursor_env_content(theme, size),
        );
        steps.push(if generic {
            ApplyStep::WriteSetsOk { dir: env.0, path: env.1, content: env.2 }
        } else {
            ApplyStep::WriteIfOk { dir: env.0, path: env.1, content: env.2 }
        });
    }
    if de.eq_ignore_ascii_case("river") {
        // river: its own command changes the session pointer now and gives
        // XCURSOR_THEME/XCURSOR_SIZE to the programs it launches afterwards
        // (riverctl(1), `xcursor-theme <theme> [size]`). On the next login
        // river without its own line in `init` loads the `default` theme —
        // that is `~/.icons/default` below.
        let mut argv = vec!["riverctl", "xcursor-theme", theme];
        if let Some(n) = &size_s {
            argv.push(n);
        }
        steps.push(ApplyStep::Live(cmd(&argv, Some(5))));
    }
    if de.eq_ignore_ascii_case("wayfire") {
        // Wayfire: the native key `[input] cursor_theme`/`cursor_size`; it rereads
        // the config itself and reloads the cursor right away (`config_reloaded` →
        // `init_xcursor` in `seat/cursor.cpp`). Only into an existing config:
        // without it Wayfire takes the `default` theme, i.e. `~/.icons/default`.
        // Which of the two files it reads is decided by whether
        // `wayfire/wayfire.ini` exists — the existing one is edited.
        for path in [format!("{cfg}/wayfire/wayfire.ini"), format!("{cfg}/wayfire.ini")] {
            let mut keys = vec![("cursor_theme", theme.to_string())];
            if let Some(n) = &size_s {
                keys.push(("cursor_size", n.clone()));
            }
            for (key, value) in keys {
                steps.push(ApplyStep::IniSetGuarded {
                    path: path.clone(),
                    section: "[input]".into(),
                    key: key.into(),
                    value,
                    only_if_file: true,
                    keep_existing: false,
                });
            }
        }
    }
    if let Some(wm) = CursorWm::from_de(de) {
        // Now — in the running session.
        match wm {
            CursorWm::Sway => {
                let mut argv = vec!["swaymsg", "seat", "*", "xcursor_theme"];
                let quoted = sway_quote(theme);
                argv.push(&quoted);
                if let Some(n) = &size_s {
                    argv.push(n);
                }
                steps.push(ApplyStep::Live(cmd(&argv, Some(5))));
            }
            CursorWm::Hyprland => {
                steps.push(ApplyStep::HyprctlCursor { theme: theme.to_string(), size: size_s.clone() });
            }
            // niri rereads the config itself — no separate command needed.
            // Since 25.11 (ever since there is `include`) its watcher checks
            // the modification time of INCLUDED files too every 0.5 s, so a
            // rewritten `rexon-cursor.kdl` without touching config.kdl is
            // enough (checked in the 25.11 and 26.04 sources, `src/utils/watcher.rs`).
            CursorWm::Niri => {}
        }
        // And for good — in the compositor config.
        if wm_safe_theme_name(theme) {
            steps.push(ApplyStep::WmConfig(wm_cursor_config_plan(wm, theme, size, home, xdg_config_home)));
        }
    }
    // Final: with ok — ~/.icons/default/index.theme (the `default` theme, taken by
    // everyone who wasn't told otherwise: X11, XWayland, a compositor without its
    // own setting).
    let dir = format!("{home}/.icons/default");
    steps.push(ApplyStep::WriteIfOk {
        path: format!("{dir}/index.theme"),
        content: format!("[Icon Theme]\nInherits={theme}\n"),
        dir,
    });
    Some(Plan { steps, history_kind: "cursor" })
}

/// Restore the desktop's standard cursor — the one named by
/// [`standard_theme_read_cmd`]. Needed for themes from [`UNSAFE_CURSOR_THEMES`]:
/// Adwaita is the default cursor in GNOME, Budgie, Cinnamon, and the "theme is
/// unsafe" refusal left the person with no way back (GNOME and Cinnamon checks,
/// 16.10.2026).
///
/// The protection the list was introduced for is kept: the name of an unsafe
/// theme is never written explicitly. The desktop's native key is RESET to the
/// schema default (`gsettings reset` — the desktop itself sets it and lives
/// with it), and our former explicit settings are removed:
/// `gtk-cursor-theme-name` from the GTK `settings.ini` (GTK takes the desktop's
/// cursor) and our `environment.d` variables file (the desktop sets XCURSOR_*
/// itself). `~/.icons/default` is not touched: it may have been put there not
/// by Rexon, and GTK and the desktop take the cursor from their own key.
/// `None` — the desktop has no native gsettings cursor key (KDE, XFCE, tiling
/// WMs): there Adwaita is not the standard, and the former refusal stays.
pub fn standard_cursor_plan(
    de: &str,
    size: Option<u32>,
    home: &str,
    xdg_config_home: Option<&str>,
) -> Option<Plan> {
    if is_generic_wm(de) {
        return None;
    }
    let s @ Schemas::Gsettings(_) = resolve_schemas(de) else { return None };
    let (schema, key) = s.role("cursor")?;
    let mut steps = vec![ApplyStep::Primary(cmd(&["gsettings", "reset", schema, key], None))];
    if let Some(n) = size {
        steps.push(ApplyStep::BestEffort(cmd(&["gsettings", "set", schema, "cursor-size", &n.to_string()], None)));
    }
    for v in ["gtk-3.0", "gtk-4.0"] {
        steps.push(ApplyStep::GtkIniUnsetIfOk {
            path: gtk_ini_path(home, xdg_config_home, v),
            key: "gtk-cursor-theme-name".into(),
        });
    }
    let cfg = crate::utils::config_home(xdg_config_home, home);
    steps.push(ApplyStep::RemoveIfOk { path: format!("{cfg}/environment.d/{CURSOR_ENV_FILE}") });
    Some(Plan { steps, history_kind: "cursor" })
}

/// What to tell the person after applying the cursor — the truth about where it
/// has already changed and where it will change after re-login.
pub fn cursor_apply_message(de: &str, report: &CursorReport) -> String {
    if let (false, Some(reason)) = (report.ok, &report.reason) {
        return reason.clone();
    }
    if !report.ok {
        return tr("Cursor not applied (the theme may be unsafe)");
    }
    if let Some((wm, outcome, path)) = &report.wm {
        let name = wm.label();
        let live = report.live == Some(true);
        return match outcome {
            WmConfigOutcome::Written if *wm == CursorWm::Niri => trf(
                // TRANSLATORS: %1 is the compositor name (niri).
                "Cursor applied and saved in the %1 config — %1 picks it up on its own.",
                &[&name],
            ),
            WmConfigOutcome::Written if live => trf(
                // TRANSLATORS: %1 is the compositor name (sway, Hyprland).
                "Cursor applied and saved in the %1 config.",
                &[&name],
            ),
            WmConfigOutcome::Written => trf(
                // TRANSLATORS: %1 is the compositor name (sway, Hyprland).
                "Cursor saved in the %1 config. It shows up the next time you start %1.",
                &[&name],
            ),
            WmConfigOutcome::NoConfig => trf(
                // TRANSLATORS: %1 is the compositor name.
                "Cursor applied. %1 has no config of its own yet, so its pointer \
                 changes after you log in again.",
                &[&name],
            ),
            WmConfigOutcome::Foreign => trf(
                // TRANSLATORS: %1 is the compositor name (sway), %2 a file path.
                "Cursor applied for apps. %1 reads the i3 config (%2), which Rexon \
                 doesn't change — the pointer of %1 itself changes after you log in \
                 again only if that config doesn't set its own cursor.",
                &[&name, &path],
            ),
            WmConfigOutcome::TooOld => trf(
                // TRANSLATORS: %1 is the compositor name (niri).
                "Cursor applied for apps. This %1 version can't include extra config \
                 files — update %1 to change its own pointer from Rexon.",
                &[&name],
            ),
            WmConfigOutcome::Failed | WmConfigOutcome::RolledBack => trf(
                // TRANSLATORS: %1 is the compositor name, %2 its config file path.
                "Cursor applied for apps, but Rexon couldn't save it in the %1 config \
                 (%2) — the config was left as it was. The pointer of %1 itself \
                 changes after you log in again only if that config doesn't set \
                 its own cursor.",
                &[&name, &path],
            ),
        };
    }
    match de.to_lowercase().as_str() {
        "kde" => tr("Cursor applied"),
        "lxqt" => tr("Cursor applied. LXQt switches the whole session to it after you log in again."),
        // The compositor's live command succeeded (river: `riverctl xcursor-theme`)
        // — its own pointer changed too.
        d if is_generic_wm(d) && report.live == Some(true) => {
            tr("Cursor applied. Some Qt and Electron apps pick it up after you log in again.")
        }
        // COSMIC: it has no pointer setting of its own, but cosmic-comp at login
        // takes XCURSOR_THEME, and without it the `default` theme (`~/.icons/default`),
        // and Rexon has already written both.
        d if d == "cosmic" || is_generic_wm(d) => tr(
            "Cursor applied for apps. The compositor's own pointer and some Qt and \
             Electron apps change after you log in again.",
        ),
        _ => tr("Cursor applied. Some Qt and Electron apps pick it up after you log in again."),
    }
}

/// Where to read the current cursor theme or size from — in order, the first
/// non-empty one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CursorQuery {
    /// A command; the answer is its output without quotes and spaces.
    Cmd(Cmd),
    /// An ini file key (`section` without brackets).
    Ini { path: String, section: String, key: String },
    /// A variable from an `environment.d` file.
    EnvFile { path: String, key: String },
    /// A process environment variable.
    Env(String),
}

/// Where to get the current cursor theme (`size == false`) or size (`true`).
pub fn cursor_queries(de: &str, home: &str, xdg_config_home: Option<&str>, size: bool) -> Vec<CursorQuery> {
    let cfg = crate::utils::config_home(xdg_config_home, home);
    let ini = |path: String, section: &str, key: &str| CursorQuery::Ini {
        path,
        section: section.into(),
        key: key.into(),
    };
    let env_file = CursorQuery::EnvFile {
        path: format!("{cfg}/environment.d/{CURSOR_ENV_FILE}"),
        key: if size { "XCURSOR_SIZE" } else { "XCURSOR_THEME" }.into(),
    };
    let env = CursorQuery::Env(if size { "XCURSOR_SIZE" } else { "XCURSOR_THEME" }.into());
    let gs = |schema: &str| {
        CursorQuery::Cmd(cmd(
            &["gsettings", "get", schema, if size { "cursor-size" } else { "cursor-theme" }],
            Some(5),
        ))
    };
    let mut out = Vec::new();
    match resolve_schemas(de) {
        Schemas::NonGsettings if de == "kde" => {
            out.push(ini(format!("{cfg}/kcminputrc"), "Mouse", if size { "cursorSize" } else { "cursorTheme" }));
            out.push(env);
            return out;
        }
        Schemas::NonGsettings if de == "deepin" => {
            let key = if size { crate::dde::CURSOR_SIZE } else { crate::dde::CURSOR_THEME };
            out.push(CursorQuery::Cmd(Cmd { argv: crate::dde::dconfig_get_argv(key), timeout: Some(5) }));
            // The appearance service also writes the cursor into the GTK settings.ini.
            out.push(ini(
                gtk_ini_path(home, xdg_config_home, "gtk-3.0"),
                "Settings",
                if size { "gtk-cursor-theme-size" } else { "gtk-cursor-theme-name" },
            ));
        }
        Schemas::NonGsettings => {
            out.push(CursorQuery::Cmd(cmd(
                &[
                    "xfconf-query", "-c", "xsettings", "-p",
                    if size { "/Gtk/CursorThemeSize" } else { "/Gtk/CursorThemeName" },
                ],
                Some(5),
            )));
        }
        s @ Schemas::Gsettings(_) => {
            if de == "lxqt" {
                out.push(ini(format!("{cfg}/lxqt/session.conf"), "Mouse", if size { "cursor_size" } else { "cursor_theme" }));
            }
            if is_generic_wm(de) {
                // The last choice made in Rexon — the same one is in the compositor
                // config.
                out.push(env_file.clone());
            }
            if let Some((schema, _)) = s.role("cursor") {
                out.push(gs(schema));
            }
        }
    }
    out.push(env_file);
    out.push(env);
    if !size {
        out.push(ini(format!("{}/.icons/default/index.theme", home.trim_end_matches('/')), "Icon Theme", "Inherits"));
    }
    out
}

/// The value from a source's answer: without quotes, spaces and the gsettings
/// type (`uint32 24`). Empty — `None`.
pub fn clean_cursor_value(raw: &str) -> Option<String> {
    let v = raw.trim();
    let v = v.strip_prefix("uint32 ").or_else(|| v.strip_prefix("int32 ")).unwrap_or(v);
    let v = v.trim().trim_matches(|c| c == '\'' || c == '"').trim();
    if v.is_empty() {
        None
    } else {
        Some(v.to_string())
    }
}

/// Value of an ini file key in a section (`section` without brackets).
pub fn ini_key_value(content: &str, section: &str, key: &str) -> Option<String> {
    let mut here = false;
    for line in content.lines() {
        let s = line.trim();
        if s.starts_with('[') && s.ends_with(']') {
            here = &s[1..s.len() - 1] == section;
            continue;
        }
        if !here || s.starts_with('#') || s.starts_with(';') {
            continue;
        }
        if let Some((k, v)) = s.split_once('=') {
            if k.trim() == key {
                return clean_cursor_value(v);
            }
        }
    }
    None
}

/// Value of a variable from an `environment.d` file (`NAME=value`).
pub fn env_file_key_value(content: &str, key: &str) -> Option<String> {
    content
        .lines()
        .map(str::trim)
        .filter(|l| !l.starts_with('#'))
        .find_map(|l| l.strip_prefix(key)?.strip_prefix('='))
        .and_then(clean_cursor_value)
}

/// Cursor size from a source's answer — only reasonable numbers.
pub fn parse_cursor_size(raw: &str) -> Option<u32> {
    clean_cursor_value(raw)?.parse::<u32>().ok().filter(|n| (8..=512).contains(n))
}

/// argv of the two `hyprctl` calls with a known size — the expansion of
/// [`ApplyStep::HyprctlCursor`] (for tests; the executor does the
/// `gsettings get` cursor-size itself separately when the plan has no size).
pub fn hyprctl_cursor_cmds(theme: &str, size: &str) -> [Cmd; 2] {
    [
        cmd(&["hyprctl", "setcursor", theme, size], None),
        cmd(&["hyprctl", "keyword", "env", &format!("XCURSOR_THEME,{theme}")], None),
    ]
}

/// argv for reading the cursor size — part of [`ApplyStep::HyprctlCursor`].
pub fn cursor_size_get_cmd() -> Cmd {
    cmd(&["gsettings", "get", "org.gnome.desktop.interface", "cursor-size"], None)
}

// ============================================================================
// update_gtk_settings_ini — a pure read-modify-write transformation
// ============================================================================

/// Port of `_update_gtk_settings_ini`. `existing` is the file contents (None =
/// no file). The original's QUIRK is kept: the `{key}=` line is replaced in ANY
/// section (without checking that we are in `[Settings]`). The `[Settings]`
/// header is prepended if missing; the key is appended if not found.
pub fn update_gtk_settings_ini(existing: Option<&str>, key: &str, value: &str) -> String {
    // Python reads f.readlines() — lines WITH the trailing '\n' (except maybe the
    // last one). split_inclusive reproduces this.
    let lines: Vec<&str> = match existing {
        Some(text) => text.split_inclusive('\n').collect(),
        None => Vec::new(),
    };

    let mut new_lines: Vec<String> = Vec::new();
    let mut found = false;
    let mut has_header = false;
    let key_eq = format!("{key}=");

    for line in &lines {
        let stripped = line.trim();
        if stripped == "[Settings]" {
            has_header = true;
        }
        if stripped.starts_with(&key_eq) {
            new_lines.push(format!("{key}={value}\n"));
            found = true;
        } else {
            new_lines.push((*line).to_string());
        }
    }

    if !has_header {
        new_lines.insert(0, "[Settings]\n".to_string());
    }
    if !found {
        new_lines.push(format!("{key}={value}\n"));
    }

    new_lines.concat()
}

// ============================================================================
// Discovering installed themes — parsing index.theme (pure functions)
// ============================================================================

/// Port of `_is_valid_icon_theme`: a theme is valid if its `index.theme` has a
/// NON-EMPTY `Directories=`. Cursor themes don't have this key.
///
/// The FIRST of `icon_dirs` that has `{dir}/{name}/index.theme` decides (as in
/// the original). `fs` is a file system snapshot; an empty `name` → false.
pub fn is_valid_icon_theme(fs: &FsSnapshot, name: &str, icon_dirs: &[String]) -> bool {
    if name.is_empty() {
        return false;
    }
    for d in icon_dirs {
        let idx = format!("{d}/{name}/index.theme");
        if !fs.is_file(&idx) {
            continue;
        }
        // The first directory with index.theme decides the outcome.
        let Some(content) = fs.read_text(&idx) else {
            return false; // unreadable as OSError → False
        };
        for line in content.lines() {
            let s = line.trim();
            if s.starts_with("Directories=") && s != "Directories=" {
                return true;
            }
        }
        return false;
    }
    false
}

/// Port of `list_gtk_themes`: subfolders of `GTK_THEME_DIRS` with `gtk-3.0/` or
/// `gtk-2.0/`. The result is sorted and deduplicated (like `sorted(set)`).
pub fn list_gtk_themes(fs: &FsSnapshot, home: &str) -> Vec<String> {
    let mut themes: Vec<String> = Vec::new();
    for d in gtk_theme_dirs(home) {
        if !fs.is_dir(&d) {
            continue;
        }
        for name in fs.scandir(&d) {
            let base = format!("{d}/{name}");
            if !fs.is_dir(&base) {
                continue;
            }
            if fs.is_dir(&format!("{base}/gtk-3.0")) || fs.is_dir(&format!("{base}/gtk-2.0")) {
                themes.push(name.clone());
            }
        }
    }
    sorted_set(themes)
}

/// Port of `list_icon_themes`: subfolders of `ICON_DIRS` with `index.theme` that
/// pass [`is_valid_icon_theme`] (cursor themes without `Directories=` are
/// filtered out).
pub fn list_icon_themes(fs: &FsSnapshot, home: &str) -> Vec<String> {
    let dirs = icon_dirs(home);
    let mut icons: Vec<String> = Vec::new();
    for d in &dirs {
        if !fs.is_dir(d) {
            continue;
        }
        for name in fs.scandir(d) {
            let base = format!("{d}/{name}");
            if !fs.is_dir(&base) {
                continue;
            }
            if !fs.is_file(&format!("{base}/index.theme")) {
                continue;
            }
            if !is_valid_icon_theme(fs, name, &dirs) {
                continue;
            }
            icons.push(name.clone());
        }
    }
    sorted_set(icons)
}

/// Port of `list_cursor_themes`: subfolders of `ICON_DIRS` with `cursors/`.
pub fn list_cursor_themes(fs: &FsSnapshot, home: &str) -> Vec<String> {
    let mut cursors: Vec<String> = Vec::new();
    for d in icon_dirs(home) {
        if !fs.is_dir(&d) {
            continue;
        }
        for name in fs.scandir(&d) {
            let base = format!("{d}/{name}");
            if fs.is_dir(&base) && fs.is_dir(&format!("{base}/cursors")) {
                cursors.push(name.clone());
            }
        }
    }
    sorted_set(cursors)
}

fn sorted_set(mut v: Vec<String>) -> Vec<String> {
    v.sort();
    v.dedup();
    v
}

// ============================================================================
// KDE Plasma themes: directory scanners + apply plans
// ============================================================================

/// look-and-feel (global) directories — port of `_KDE_DIRS["global"]`.
pub fn kde_global_dirs(home: &str) -> [String; 2] {
    [
        format!("{home}/.local/share/plasma/look-and-feel"),
        "/usr/share/plasma/look-and-feel".to_string(),
    ]
}

/// Plasma Style directories — port of `_KDE_DIRS["plasma_style"]`.
pub fn kde_plasma_style_dirs(home: &str) -> [String; 2] {
    [
        format!("{home}/.local/share/plasma/desktoptheme"),
        "/usr/share/plasma/desktoptheme".to_string(),
    ]
}

/// Color scheme directories — port of `_KDE_DIRS["colors"]`.
pub fn kde_color_dirs(home: &str) -> [String; 2] {
    [
        format!("{home}/.local/share/color-schemes"),
        "/usr/share/color-schemes".to_string(),
    ]
}

/// Kvantum directories — port of `_KDE_DIRS["kvantum"]`.
pub fn kde_kvantum_dirs(home: &str) -> [String; 2] {
    kde_kvantum_dirs_xdg(None, home)
}

/// The same, honoring `$XDG_CONFIG_HOME`.
pub fn kde_kvantum_dirs_xdg(xdg_config_home: Option<&str>, home: &str) -> [String; 2] {
    [
        format!("{}/Kvantum", crate::utils::config_home(xdg_config_home, home)),
        "/usr/share/Kvantum".to_string(),
    ]
}

/// Port of `list_kde_global_themes`: subfolders with `metadata.json` OR
/// `metadata.desktop`. The Python operator precedence QUIRK is kept: the
/// condition `is_dir and isfile(json) or isfile(desktop)` — with
/// `metadata.desktop` present the theme is added EVEN if the entry is not a
/// directory.
pub fn list_kde_global_themes(fs: &FsSnapshot, home: &str) -> Vec<String> {
    kde_lnf_like_scan(fs, &kde_global_dirs(home))
}

/// Port of `list_kde_plasma_styles` (the same operator logic as global).
pub fn list_kde_plasma_styles(fs: &FsSnapshot, home: &str) -> Vec<String> {
    kde_lnf_like_scan(fs, &kde_plasma_style_dirs(home))
}

// Common traversal for global/plasma_style: `(is_dir and isfile(A)) or isfile(B)`.
fn kde_lnf_like_scan(fs: &FsSnapshot, dirs: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for d in dirs {
        if !fs.is_dir(d) {
            continue;
        }
        for name in fs.scandir(d) {
            let base = format!("{d}/{name}");
            let is_dir = fs.is_dir(&base);
            let has_json = fs.is_file(&format!("{base}/metadata.json"));
            let has_desktop = fs.is_file(&format!("{base}/metadata.desktop"));
            if (is_dir && has_json) || has_desktop {
                out.push(name.clone());
            }
        }
    }
    sorted_set(out)
}

/// Port of `list_kde_color_schemes`: `*.colors` files (without the suffix).
pub fn list_kde_color_schemes(fs: &FsSnapshot, home: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for d in kde_color_dirs(home) {
        if !fs.is_dir(&d) {
            continue;
        }
        for name in fs.scandir(&d) {
            let base = format!("{d}/{name}");
            if fs.is_file(&base) {
                if let Some(stem) = name.strip_suffix(".colors") {
                    out.push(stem.to_string());
                }
            }
        }
    }
    sorted_set(out)
}

/// Port of `list_kvantum_themes`: subfolders that have a `*.kvconfig` file.
/// `kv_files` — for each theme, the list of file names inside (port of
/// `os.listdir(entry.path)` + `f.endswith(".kvconfig")`); missing — counted as
/// empty.
pub fn list_kvantum_themes<F>(fs: &FsSnapshot, home: &str, dir_files: F) -> Vec<String>
where
    F: Fn(&str) -> Vec<String>,
{
    let mut out: Vec<String> = Vec::new();
    for d in kde_kvantum_dirs(home) {
        if !fs.is_dir(&d) {
            continue;
        }
        for name in fs.scandir(&d) {
            let base = format!("{d}/{name}");
            if !fs.is_dir(&base) {
                continue;
            }
            if dir_files(&base).iter().any(|f| f.ends_with(".kvconfig")) {
                out.push(name.clone());
            }
        }
    }
    sorted_set(out)
}

/// Plan for reading the current KDE themes — port of `get_current_kde_themes`
/// (three `kreadconfig6` calls; the executor reads stdout, `"—"` on empty/error).
pub fn get_current_kde_cmds() -> [Cmd; 3] {
    [
        cmd(&["kreadconfig6", "--file", "kdeglobals", "--group", "KDE", "--key", "LookAndFeelPackage"], None),
        cmd(&["kreadconfig6", "--file", "plasmarc", "--group", "Theme", "--key", "name"], None),
        cmd(&["kreadconfig6", "--file", "kdeglobals", "--group", "General", "--key", "ColorScheme"], None),
    ]
}

/// Port of `apply_kde_global_theme` (timeout=15, history "kde-global").
pub fn apply_kde_global_plan(theme: &str) -> Plan {
    Plan {
        steps: vec![ApplyStep::Primary(cmd(&["plasma-apply-lookandfeel", "-a", theme], Some(15)))],
        history_kind: "kde-global",
    }
}

// ── Global theme: «Apply “X”?» — appearance and layout ───────────────────
//
// Plasma 6.8, before applying a global theme, asks WHAT to take from it:
// «Appearance» (colors, styles, icons, cursors, fonts, splash, wallpaper,
// borders) and «Desktop and widget layout» (the panel and widget layout from
// `contents/layouts`, title bar buttons from `contents/layouts/defaults`,
// window placement). The layout is NOT checked by default: it wipes the
// current panels and widgets.
//
// ANY theme with appearance has a layout, as in KDE: Plasma takes the theme's
// missing files from the fallback package, and that is always Breeze —
// `org.kde.breeze.desktop`, hard-coded in the global theme package structure,
// not the default theme from kdeglobals. The panel layout of a theme without
// its own is the Breeze layout, and the checkbox caption says so directly
// ([`LnfLayout::desktop`] is false). The tile mark «Contains desktop layout»
// is only for a theme with its own layout.

/// The layout a global theme carries.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct LnfLayout {
    /// There is a `contents/layouts/*.js` script — its own panel and widget layout.
    pub desktop: bool,
    /// Title bar buttons (`layouts/defaults`, `[kwinrc][org.kde.kdecoration2]`).
    pub buttons: Option<(String, String)>,
    /// Window placement (`defaults`, `[kwinrc][Windows] Placement`).
    pub placement: Option<String>,
    /// `BorderlessMaximizedWindows` from `layouts/defaults`; `Some("")` — the file
    /// exists, the key doesn't (Plasma then turns "borderless" off), `None` — no
    /// file.
    pub borderless: Option<String>,
}

impl LnfLayout {
    pub fn any(&self) -> bool {
        self.desktop || self.buttons.is_some() || self.placement.is_some()
    }
}

/// A part of a global theme — a `KLookAndFeelManager::ContentFlags` flag of
/// Plasma 6.8 (the [`LnfPart::bit`] values are the same). The «Choose What to
/// Apply…» window shows them as checkboxes; Plasma 6.8 recognizes the wallpaper
/// ([`LnfPart::Wallpaper`]) but neither applies nor shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LnfPart {
    Colors,
    WidgetStyle,
    WindowDecoration,
    BorderSize,
    Icons,
    PlasmaTheme,
    Cursors,
    Fonts,
    WindowSwitcher,
    SplashScreen,
    Wallpaper,
    DesktopLayout,
    TitlebarLayout,
    WindowPlacement,
}

/// All appearance parts (Plasma `AppearanceSettings`).
pub const LNF_APPEARANCE_MASK: u32 = (1 << 12) - 1;
/// All layout parts (Plasma `LayoutSettings`).
pub const LNF_LAYOUT_MASK: u32 = (1 << 20) - (1 << 16);
/// Parts that afterwards ask KWin to reread the settings (Plasma
/// `KWinSettings`).
const LNF_KWIN_MASK: u32 = (1 << 7) | (1 << 2) | (1 << 18) | (1 << 17) | (1 << 11);

impl LnfPart {
    /// The order of the Plasma 6.8 window (`MoreOptions.qml`), wallpaper after
    /// appearance, layout last.
    pub const ALL: [LnfPart; 14] = [
        LnfPart::Colors,
        LnfPart::WidgetStyle,
        LnfPart::WindowDecoration,
        LnfPart::BorderSize,
        LnfPart::Icons,
        LnfPart::PlasmaTheme,
        LnfPart::Cursors,
        LnfPart::Fonts,
        LnfPart::WindowSwitcher,
        LnfPart::SplashScreen,
        LnfPart::Wallpaper,
        LnfPart::DesktopLayout,
        LnfPart::TitlebarLayout,
        LnfPart::WindowPlacement,
    ];

    /// Plasma flag.
    pub fn bit(self) -> u32 {
        match self {
            LnfPart::Colors => 1 << 0,
            LnfPart::WidgetStyle => 1 << 1,
            LnfPart::WindowDecoration => 1 << 2,
            LnfPart::Icons => 1 << 3,
            LnfPart::PlasmaTheme => 1 << 4,
            LnfPart::Cursors => 1 << 5,
            LnfPart::Fonts => 1 << 6,
            LnfPart::WindowSwitcher => 1 << 7,
            LnfPart::SplashScreen => 1 << 8,
            LnfPart::Wallpaper => 1 << 10,
            LnfPart::BorderSize => 1 << 11,
            LnfPart::DesktopLayout => 1 << 16,
            LnfPart::TitlebarLayout => 1 << 17,
            LnfPart::WindowPlacement => 1 << 18,
        }
    }

    pub fn is_layout(self) -> bool {
        self.bit() & LNF_LAYOUT_MASK != 0
    }

    /// Checkbox caption — the texts of the Plasma 6.8 window.
    pub fn label(self) -> String {
        match self {
            LnfPart::Colors => trc("global theme part", "Colors"),
            LnfPart::WidgetStyle => trc("global theme part", "Application Style"),
            LnfPart::WindowDecoration => trc("global theme part", "Window Decoration Style"),
            LnfPart::BorderSize => trc("global theme part", "Window Decoration Size"),
            LnfPart::Icons => trc("global theme part", "Icons"),
            LnfPart::PlasmaTheme => trc("global theme part", "Plasma Style"),
            LnfPart::Cursors => trc("global theme part", "Pointers"),
            LnfPart::Fonts => trc("global theme part", "Fonts"),
            LnfPart::WindowSwitcher => trc("global theme part", "Task Switcher"),
            LnfPart::SplashScreen => trc("global theme part", "Splash Screen"),
            LnfPart::Wallpaper => trc("global theme part", "Wallpaper"),
            LnfPart::DesktopLayout | LnfPart::WindowPlacement => trc("global theme part", "Desktop layout"),
            LnfPart::TitlebarLayout => trc("global theme part", "Titlebar Buttons layout"),
        }
    }
}

/// Mask of a list of parts.
pub fn lnf_mask(parts: &[LnfPart]) -> u32 {
    parts.iter().fold(0, |m, p| m | p.bit())
}

/// Parts of a mask in the [`LnfPart::ALL`] order.
pub fn lnf_parts_of(mask: u32) -> Vec<LnfPart> {
    LnfPart::ALL.into_iter().filter(|p| mask & p.bit() != 0).collect()
}

/// What a global theme carries. `appearance` and `layout` — without filling the
/// missing parts from Breeze (the checkbox caption and the tile mark); `parts`
/// and `values` — as Plasma sees them: if the theme lacks the `defaults` file,
/// `layouts/defaults`, the splash or the layout, it is taken from the fallback
/// package ([`LNF_FALLBACK_PACKAGE`]) if that one is on disk.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct LnfContents {
    /// At least one appearance setting.
    pub appearance: bool,
    pub layout: LnfLayout,
    /// Theme parts (`KLookAndFeelManager::packageContents`).
    pub parts: Vec<LnfPart>,
    /// The values the parts write: "group/…/key" of the `defaults` file
    /// (`kdeglobals/General/ColorScheme`), non-empty only.
    pub values: BTreeMap<String, String>,
    /// The theme has its own colors file (`contents/colors`).
    pub own_colors: bool,
}

impl LnfContents {
    pub fn mask(&self) -> u32 {
        lnf_mask(&self.parts)
    }

    pub fn has(&self, part: LnfPart) -> bool {
        self.parts.contains(&part)
    }

    fn value(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }
}

/// `defaults` keys read by the parts (`KLookAndFeelManager::save`).
const LNF_VALUE_KEYS: &[(&str, &str)] = &[
    ("kdeglobals/General", "ColorScheme"),
    ("kdeglobals/KDE", "widgetStyle"),
    ("kdeglobals/Icons", "Theme"),
    ("plasmarc/Theme", "name"),
    ("kcminputrc/Mouse", "cursorTheme"),
    ("kwinrc/WindowSwitcher", "LayoutName"),
    ("kwinrc/org.kde.kdecoration2", "library"),
    ("kwinrc/org.kde.kdecoration2", "theme"),
    ("kwinrc/org.kde.kdecoration2", "NoPlugin"),
    ("kwinrc/org.kde.kdecoration2", "BorderSize"),
    ("kdeglobals/General", "font"),
    ("kdeglobals/General", "fixed"),
    ("kdeglobals/General", "smallestReadableFont"),
    ("kdeglobals/General", "toolBarFont"),
    ("kdeglobals/General", "menuFont"),
    ("kdeglobals/WM", "activeFont"),
    ("ksplashrc/KSplash", "Theme"),
    ("Wallpaper", "Image"),
];

/// Directory of global theme `theme`: first the user's, then the system one —
/// the same order as Plasma's. `None` — no such theme.
pub fn lnf_package_dir(io: &dyn crate::animations::LookIo, home: &str, theme: &str) -> Option<String> {
    if theme.is_empty() || theme.contains('/') {
        return None;
    }
    kde_global_dirs(home)
        .into_iter()
        .map(|root| format!("{root}/{theme}"))
        .find(|dir| !io.list_dir(dir).is_empty())
}

/// Value of a key in a nested group of the theme's `defaults` file: the group
/// path "kdeglobals/General" is the header `[kdeglobals][General]`.
fn lnf_value(text: &str, group_path: &str, key: &str) -> Option<String> {
    let group = group_path.replace('/', "][");
    crate::shell_palette::ini_get(text, &group, key).filter(|v| !v.is_empty())
}

/// Contents of the global theme in directory `dir` — the same signs by which
/// Plasma 6.8 decides what the theme carries (without the fallback package).
pub fn lnf_contents(io: &dyn crate::animations::LookIo, dir: &str) -> LnfContents {
    lnf_contents_with(io, dir, None)
}

/// Contents of theme `theme` on machine `io` as Plasma sees them: missing files
/// come from the fallback package ([`LNF_FALLBACK_PACKAGE`]). `None` — the theme
/// is not on disk.
pub fn lnf_contents_live(io: &dyn crate::animations::LookIo, home: &str, theme: &str) -> Option<LnfContents> {
    let dir = lnf_package_dir(io, home, theme)?;
    let fallback = lnf_package_dir(io, home, LNF_FALLBACK_PACKAGE).filter(|f| *f != dir);
    Some(lnf_contents_with(io, &dir, fallback.as_deref()))
}

/// [`lnf_contents`] with the fallback package in directory `fallback`.
///
/// The part signs are Plasma 6.8 `KLookAndFeelManager::packageContents`:
/// colors — `ColorScheme` or its own `colors` file (only with `defaults`),
/// border — `library` or `NoPlugin`, border size — `BorderSize`, fonts — any
/// font in `[kdeglobals][General]` or `[kdeglobals][WM] activeFont`
/// (and the swapped groups Plasma searches in), splash — `splash/Splash.qml`, layout —
/// the `layouts` directory, title bar buttons — `layouts/defaults`.
pub fn lnf_contents_with(io: &dyn crate::animations::LookIo, dir: &str, fallback: Option<&str>) -> LnfContents {
    let contents = format!("{dir}/contents");
    let top = io.list_dir(&contents);
    let has = |name: &str| top.iter().any(|n| n == name);
    let defaults = io.read(&format!("{contents}/defaults"));
    let layout_defaults = io.read(&format!("{contents}/layouts/defaults"));
    // The theme's file, and if it's missing — the fallback package's (this is how KPackage looks).
    let fb = |rel: &str| fallback.and_then(|f| io.read(&format!("{f}/contents/{rel}")));
    let fb_has = |rel: &str, name: &str| {
        fallback.is_some_and(|f| {
            let dir = if rel.is_empty() { format!("{f}/contents") } else { format!("{f}/contents/{rel}") };
            io.list_dir(&dir).iter().any(|n| n == name)
        })
    };
    let pkg_defaults = defaults.clone().or_else(|| fb("defaults"));
    let pkg_layout_defaults = layout_defaults.clone().or_else(|| fb("layouts/defaults"));
    let d = pkg_defaults.as_deref().unwrap_or("");
    let provides = |group: &str, keys: &[&str]| keys.iter().any(|k| lnf_value(d, group, k).is_some());

    let own_splash = io.list_dir(&format!("{contents}/splash")).iter().any(|n| n == "Splash.qml");
    let own_colors = has("colors");
    let mut parts = Vec::new();
    let mut put = |part: LnfPart, on: bool| {
        if on {
            parts.push(part);
        }
    };
    put(LnfPart::SplashScreen, own_splash || fb_has("splash", "Splash.qml"));
    put(LnfPart::DesktopLayout, has("layouts") || fb_has("", "layouts"));
    if let Some(ld) = pkg_layout_defaults.as_deref() {
        put(
            LnfPart::TitlebarLayout,
            ["ButtonsOnLeft", "ButtonsOnRight"].iter().any(|k| lnf_value(ld, "kwinrc/org.kde.kdecoration2", k).is_some()),
        );
    }
    if pkg_defaults.is_some() {
        let deco = "kwinrc/org.kde.kdecoration2";
        put(LnfPart::Colors, provides("kdeglobals/General", &["ColorScheme"]) || own_colors || fb_has("", "colors"));
        put(LnfPart::WidgetStyle, provides("kdeglobals/KDE", &["widgetStyle"]));
        put(LnfPart::Icons, provides("kdeglobals/Icons", &["Theme"]));
        put(LnfPart::PlasmaTheme, provides("plasmarc/Theme", &["name"]));
        put(LnfPart::Wallpaper, provides("Wallpaper", &["Image"]));
        put(LnfPart::Cursors, provides("kcminputrc/Mouse", &["cursorTheme"]));
        put(LnfPart::WindowSwitcher, provides("kwinrc/WindowSwitcher", &["LayoutName"]));
        put(LnfPart::WindowDecoration, provides(deco, &["library", "NoPlugin"]));
        put(LnfPart::BorderSize, provides(deco, &["BorderSize"]));
        // ⭐ Fix: Plasma 6.8 looks for fonts in swapped groups
        // (`[WM] font…`, `[General] activeFont`), but writes from where themes
        // keep them (`[General] font…`, `[WM] activeFont`) — such a theme has no
        // «Fonts» checkbox in Plasma, and the fonts are not applied. Here both
        // places are recognized.
        let fonts = ["font", "fixed", "smallestReadableFont", "toolBarFont", "menuFont"];
        put(
            LnfPart::Fonts,
            provides("kdeglobals/WM", &fonts)
                || provides("kdeglobals/General", &["activeFont"])
                || provides("kdeglobals/General", &fonts)
                || provides("kdeglobals/WM", &["activeFont"]),
        );
        put(LnfPart::WindowPlacement, provides("kwinrc/Windows", &["Placement"]));
    }
    parts.sort_by_key(|p| LnfPart::ALL.iter().position(|q| q == p));
    let values = LNF_VALUE_KEYS
        .iter()
        .filter_map(|(group, key)| lnf_value(d, group, key).map(|v| (format!("{group}/{key}"), v)))
        .collect();

    // Appearance and layout — by the theme's own files (the "from Breeze"
    // checkbox caption, the tile mark), as before.
    let own = defaults.as_deref().unwrap_or("");
    let own_provides = |group: &str, keys: &[&str]| keys.iter().any(|k| lnf_value(own, group, k).is_some());
    let appearance = own_splash
        || own_colors
        || own_provides("kdeglobals/General", &["ColorScheme", "activeFont"])
        || own_provides("kdeglobals/KDE", &["widgetStyle"])
        || own_provides("kdeglobals/Icons", &["Theme"])
        || own_provides("plasmarc/Theme", &["name"])
        || own_provides("Wallpaper", &["Image"])
        || own_provides("kcminputrc/Mouse", &["cursorTheme"])
        || own_provides("kwinrc/WindowSwitcher", &["LayoutName"])
        || own_provides("kwinrc/org.kde.kdecoration2", &["library", "NoPlugin", "BorderSize"])
        || own_provides("kdeglobals/WM", &["font", "fixed", "smallestReadableFont", "toolBarFont", "menuFont"])
        || lnf_mask(&parts) & LNF_APPEARANCE_MASK != 0;

    // Buttons, "borderless" and window placement — from the package files (with
    // the fallback), as `KLookAndFeelManager::save` reads them.
    let ld = pkg_layout_defaults.as_deref().unwrap_or("");
    let deco = "kwinrc/org.kde.kdecoration2";
    let buttons = match (lnf_value(ld, deco, "ButtonsOnLeft"), lnf_value(ld, deco, "ButtonsOnRight")) {
        (None, None) => None,
        (l, r) => Some((l.unwrap_or_default(), r.unwrap_or_default())),
    };
    let layout = LnfLayout {
        desktop: lnf_has_desktop_layout(io, dir),
        buttons,
        placement: lnf_value(d, "kwinrc/Windows", "Placement"),
        borderless: pkg_layout_defaults
            .as_deref()
            .map(|t| lnf_value(t, "kwinrc/Windows", "BorderlessMaximizedWindows").unwrap_or_default()),
    };
    LnfContents { appearance, layout, parts, values, own_colors }
}

/// Whether the theme has its own panel and widget layout — the tile mark
/// «Contains desktop layout».
///
/// Its own layout is a `*.js` script in `contents/layouts`: title bar buttons
/// alone (`layouts/defaults`, this is how a theme saved outside Plasma carries
/// them) don't count as a panel layout.
pub fn lnf_has_desktop_layout(io: &dyn crate::animations::LookIo, dir: &str) -> bool {
    io.list_dir(&format!("{dir}/contents/layouts")).iter().any(|n| n.ends_with(".js"))
}

/// Tile caption of a theme with its own layout.
pub fn lnf_layout_badge() -> String {
    tr("Contains desktop layout")
}

/// Mark of a theme tile from disk: for a global theme with its own layout
/// (`item_id` is `local:global:…`, `install_path` is its directory) —
/// «Contains desktop layout», for others — empty.
pub fn lnf_tile_badge(item_id: &str, install_path: &str) -> String {
    if item_id.starts_with("local:global:")
        && !install_path.is_empty()
        && std::fs::read_dir(std::path::Path::new(install_path).join("contents/layouts"))
            .map(|d| d.flatten().any(|e| e.file_name().to_string_lossy().ends_with(".js")))
            .unwrap_or(false)
    {
        lnf_layout_badge()
    } else {
        String::new()
    }
}

/// What the person checked in the «Apply “X”?» window.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct GlobalChoice {
    pub appearance: bool,
    pub layout: bool,
    /// The detailed view of the window («Choose What to Apply…»): the checked parts
    /// as an [`LnfPart::bit`] mask. `None` — the simple view: appearance and layout
    /// as a whole.
    pub parts: Option<u32>,
}

/// The choice together with the theme contents — what the plan is built from.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct GlobalPress {
    pub choice: GlobalChoice,
    pub contents: LnfContents,
    /// Parts included by the plan of a Rexon section rather than the theme key
    /// ([`lnf_section_parts`]): the steps or the reason why it failed. Built by
    /// the live installer with the same probes as the section button.
    #[serde(skip)]
    pub sections: BTreeMap<LnfPart, Result<Vec<ApplyStep>, String>>,
}

/// A window row: a checkbox, an item line without a checkbox, or nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum GlobalRow {
    Checkbox,
    Bullet,
    Hidden,
}

/// A part checkbox in the detailed view of the window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalPartRow {
    pub id: LnfPart,
    /// What the checkbox includes: "Desktop layout" — together with window
    /// placement, as in Plasma.
    pub mask: u32,
    pub text: String,
    /// A layout part (the "Layout:" group), otherwise an appearance part.
    pub layout: bool,
    pub checked: bool,
    /// Outside Plasma a part may not carry over: the checkbox is disabled, `reason`
    /// says why.
    pub enabled: bool,
    pub reason: String,
}

/// The «Apply “X”?» window — everything the page draws.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalQuestion {
    pub title: String,
    pub heading: String,
    pub appearance_row: GlobalRow,
    pub appearance_text: String,
    pub appearance_checked: bool,
    pub layout_row: GlobalRow,
    pub layout_text: String,
    pub layout_checked: bool,
    /// Shown while the layout is checked.
    pub layout_warning: String,
    /// The theme carries nothing: the window explains, «Apply» is disabled.
    pub empty_text: String,
    pub apply_text: String,
    pub cancel_text: String,
    /// Detailed view: part checkboxes, first layout, then appearance (as in
    /// Plasma); parts the theme doesn't have are not in the list.
    pub parts: Vec<GlobalPartRow>,
    pub layout_heading: String,
    pub appearance_heading: String,
    /// The «Choose What to Apply…» / «Fewer Options…» button; available when the
    /// theme has appearance (as in Plasma).
    pub more_text: String,
    pub fewer_text: String,
    pub more_enabled: bool,
    /// Open the detailed view at once — outside Plasma, where it shows what carries over.
    pub detailed: bool,
    /// Outside Plasma: how the theme is applied on this desktop.
    pub outside_note: String,
}

/// The fallback Plasma global theme: everything the theme lacks is taken from
/// it, including the panel and widget layout.
pub const LNF_FALLBACK_PACKAGE: &str = "org.kde.breeze.desktop";

/// Parts available in the window: the theme's parts, and for a theme with
/// appearance also the layout (Breeze supplies what is missing).
pub fn lnf_available_mask(contents: &LnfContents) -> u32 {
    let mut mask = contents.mask();
    if contents.appearance || contents.layout.any() {
        mask |= LnfPart::DesktopLayout.bit();
    }
    mask
}

/// Why a theme part doesn't carry over to desktop `de` (not Plasma); `None` —
/// it carries over. What carries over is what Rexon has for any desktop: colors
/// (the Qt layer), application style, icons, cursors, fonts, title bar buttons.
pub fn lnf_part_outside(part: LnfPart, contents: &LnfContents, de: &str) -> Option<String> {
    use crate::de_customization::{de_from_id, De};
    let deepin = de_from_id(de) == Some(De::Deepin);
    let chameleon = || tr("Deepin programs are drawn by its own Chameleon style, and Rexon does not replace it");
    match part {
        LnfPart::Colors | LnfPart::WidgetStyle if deepin => Some(chameleon()),
        LnfPart::Colors if contents.value("kdeglobals/General/ColorScheme").is_none() => Some(lnf_own_colors_reason()),
        LnfPart::Fonts if deepin => Some(tr("In Deepin, fonts are changed only in its Control Center")),
        LnfPart::Colors | LnfPart::WidgetStyle | LnfPart::Icons | LnfPart::Cursors | LnfPart::Fonts => None,
        LnfPart::TitlebarLayout => None,
        LnfPart::WindowDecoration | LnfPart::BorderSize => {
            Some(tr("these window frames are drawn only by KWin, the Plasma window manager"))
        }
        LnfPart::WindowSwitcher => Some(tr("this task switcher (Alt+Tab) belongs to KWin, the Plasma window manager")),
        LnfPart::PlasmaTheme => {
            Some(tr("a Plasma Style draws Plasma's own panels and widgets, and this desktop has none of them"))
        }
        LnfPart::SplashScreen => Some(tr("this splash screen is shown only when Plasma starts")),
        LnfPart::Wallpaper => Some(tr("Plasma does not apply the wallpaper of a global theme either")),
        LnfPart::DesktopLayout | LnfPart::WindowPlacement => {
            Some(tr("the desktop and panel layout is a Plasma script that only Plasma can run"))
        }
    }
}

/// Theme colors — only as its own file, without a scheme name: they are brought
/// in only by applying the whole theme (`plasma-apply-lookandfeel`).
fn lnf_own_colors_reason() -> String {
    tr("the theme keeps its colors only in its own file, which only applying the whole theme in Plasma brings in")
}

/// Parts that can be checked on desktop `de`.
fn lnf_allowed_mask(contents: &LnfContents, de: &str) -> u32 {
    if de == "kde" {
        return u32::MAX;
    }
    LnfPart::ALL.into_iter().filter(|p| lnf_part_outside(*p, contents, de).is_none()).fold(0, |m, p| m | p.bit())
}

/// Parts included by the plan of a Rexon section — the same steps as the
/// «Apply» button in the «Icons», «Cursors»… sections: in Plasma — icons and
/// cursors (their plan checks the theme), outside Plasma — also colors (the Qt
/// layer), application style and title bar buttons.
pub fn lnf_section_parts(de: &str) -> &'static [LnfPart] {
    if de == "kde" {
        &[LnfPart::Icons, LnfPart::Cursors]
    } else {
        &[LnfPart::Colors, LnfPart::WidgetStyle, LnfPart::Icons, LnfPart::Cursors, LnfPart::TitlebarLayout]
    }
}

/// The name a part passes to the section plan: scheme, style, icon or cursor
/// theme. The lowercase style `breeze` becomes `Breeze`, as Plasma corrects it.
pub fn lnf_section_value(contents: &LnfContents, part: LnfPart) -> Option<String> {
    let key = match part {
        LnfPart::Colors => "kdeglobals/General/ColorScheme",
        LnfPart::WidgetStyle => "kdeglobals/KDE/widgetStyle",
        LnfPart::Icons => "kdeglobals/Icons/Theme",
        LnfPart::Cursors => "kcminputrc/Mouse/cursorTheme",
        _ => return None,
    };
    let v = contents.value(key)?;
    Some(if part == LnfPart::WidgetStyle && v == "breeze" { "Breeze".to_string() } else { v.to_string() })
}

/// The window for theme `display` with contents `contents` in Plasma.
/// `plasma_locked` — Plasma widgets are locked (`org.kde.PlasmaShell.immutable`):
/// then there is no layout checkbox, just as in Plasma.
pub fn global_question(display: &str, contents: &LnfContents, plasma_locked: bool) -> GlobalQuestion {
    global_question_in(display, contents, plasma_locked, "kde")
}

/// The window for desktop `de`. Outside Plasma the window is detailed at once:
/// parts that don't carry over here are disabled with a reason.
pub fn global_question_in(display: &str, contents: &LnfContents, plasma_locked: bool, de: &str) -> GlobalQuestion {
    let outside = de != "kde";
    let avail = lnf_available_mask(contents);
    let allowed = lnf_allowed_mask(contents, de);
    let a = contents.appearance;
    // The layout is available for any theme with appearance (the missing parts
    // from Breeze); a theme without appearance — only when it has its own layout.
    let l = a || contents.layout.any();
    let carried = avail & allowed;
    let (appearance_row, layout_row) = if outside {
        // Outside Plasma the layout doesn't carry over; appearance — as one line:
        // "what carries over" (the list is in the detailed view).
        (if carried & LNF_APPEARANCE_MASK != 0 { GlobalRow::Bullet } else { GlobalRow::Hidden }, GlobalRow::Hidden)
    } else {
        (
            match (a, l) {
                (true, true) => GlobalRow::Checkbox,
                (true, false) => GlobalRow::Bullet,
                _ => GlobalRow::Hidden,
            },
            match (a, l) {
                (true, true) if !plasma_locked => GlobalRow::Checkbox,
                (false, true) => GlobalRow::Bullet,
                _ => GlobalRow::Hidden,
            },
        )
    };

    // Detailed view — the checkboxes of the Plasma 6.8 `MoreOptions.qml`.
    let mut parts = Vec::new();
    let has_app_parts = avail & LNF_APPEARANCE_MASK & !LnfPart::Wallpaper.bit() != 0;
    let mut row = |part: LnfPart, mask: u32| {
        if avail & mask == 0 {
            return;
        }
        let reason = if outside { lnf_part_outside(part, contents, de) } else { None };
        let enabled = reason.is_none();
        // By default — everything the theme carries, except the layout when there is
        // appearance (`resetSelectedContents`).
        let checked = enabled && (!part.is_layout() || !has_app_parts);
        parts.push(GlobalPartRow {
            id: part,
            mask: avail & mask,
            text: part.label(),
            layout: part.is_layout(),
            checked,
            enabled,
            reason: reason.unwrap_or_default(),
        });
    };
    if !(plasma_locked && !outside) {
        row(LnfPart::DesktopLayout, LnfPart::DesktopLayout.bit() | LnfPart::WindowPlacement.bit());
    }
    row(LnfPart::TitlebarLayout, LnfPart::TitlebarLayout.bit());
    for part in LnfPart::ALL.into_iter().filter(|p| !p.is_layout() && *p != LnfPart::Wallpaper) {
        row(part, part.bit());
    }

    let empty_text = if outside {
        if carried == 0 {
            tr("Nothing in this Global Theme can be carried over to this desktop: what it changes exists only in Plasma.")
        } else {
            String::new()
        }
    } else if a || l {
        String::new()
    } else {
        tr("This Global Theme provides no settings that can be applied. It may be broken — \
            please tell its author.")
    };
    GlobalQuestion {
        title: trf("Apply “%1”?", &[&display]),
        heading: tr("The following will be applied by this Global Theme:"),
        appearance_row,
        appearance_text: tr("Appearance settings"),
        // Appearance is always checked when present; layout — only when the theme
        // has nothing else.
        appearance_checked: if outside { appearance_row != GlobalRow::Hidden } else { a },
        layout_row,
        layout_text: if contents.layout.desktop {
            tr("Desktop and widget layout")
        } else {
            tr("Desktop and widget layout (from Breeze — the theme has none of its own)")
        },
        layout_checked: !outside && l && !a,
        layout_warning: tr(
            "Applying the layout will delete the current desktops, panels, docks and widgets \
             and replace them with the ones from the theme.",
        ),
        empty_text,
        apply_text: tr("Apply"),
        cancel_text: trc("dialog button", "Cancel"),
        parts,
        layout_heading: tr("Layout settings:"),
        appearance_heading: tr("Appearance settings:"),
        more_text: tr("Choose what to apply…"),
        fewer_text: tr("Show fewer options…"),
        more_enabled: if outside { carried != 0 } else { a },
        detailed: outside && carried != 0,
        outside_note: if outside {
            tr("Outside Plasma, Rexon applies the parts of the theme one by one — the ones this desktop can use.")
        } else {
            String::new()
        },
    }
}

/// Whether Plasma widgets are locked: the `dbus-send --print-reply` answer to
/// `org.kde.PlasmaShell.immutable` ("boolean true").
pub fn parse_plasma_immutable(reply: &str) -> bool {
    reply.split_whitespace().last() == Some("true")
}

/// argv of the "are the widgets locked" query.
pub fn plasma_immutable_argv() -> Vec<String> {
    [
        "dbus-send",
        "--session",
        "--print-reply",
        "--dest=org.kde.plasmashell",
        "/PlasmaShell",
        "org.kde.PlasmaShell.immutable",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// The window for theme `theme` on machine `io` and desktop `de`. `None` — the
/// theme is not on disk.
pub fn global_question_live(
    io: &dyn crate::animations::LookIo,
    home: &str,
    theme: &str,
    display: &str,
    de: &str,
) -> Option<GlobalQuestion> {
    let contents = lnf_contents_live(io, home, theme)?;
    let locked = de == "kde" && io.have("dbus-send") && {
        let (ok, out) = io.run(&plasma_immutable_argv());
        ok && parse_plasma_immutable(&out)
    };
    Some(global_question_in(display, &contents, locked, de))
}

/// What to apply: checked ∩ what the theme carries ∩ what carries over to
/// desktop `de` (as in Plasma, a checkbox of a part that isn't there includes
/// nothing).
pub fn global_selection(press: &GlobalPress, de: &str) -> u32 {
    let c = &press.choice;
    let want = c.parts.unwrap_or_else(|| {
        (if c.appearance { LNF_APPEARANCE_MASK } else { 0 }) | (if c.layout { LNF_LAYOUT_MASK } else { 0 })
    });
    want & lnf_available_mask(&press.contents) & lnf_allowed_mask(&press.contents, de)
}

/// The plan by the person's choice and what of the checked failed (part — reason).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalPlan {
    pub plan: Plan,
    pub skipped: Vec<String>,
}

/// The «Apply» plan of a global theme in Plasma. `None` — nothing is checked.
pub fn apply_kde_global_choice_plan(theme: &str, press: &GlobalPress) -> Option<Plan> {
    apply_global_choice_plan(theme, press, "kde").map(|g| g.plan)
}

/// `kwriteconfig6 … --notify` — this is how `KLookAndFeelManager` writes the parts.
fn lnf_kwrite(file: &str, group: &str, key: &str, value: &str, notify: bool) -> Cmd {
    let mut argv = vec!["kwriteconfig6", "--file", file, "--group", group, "--key", key];
    if notify {
        argv.push("--notify");
    }
    argv.push(value);
    cmd(&argv, None)
}

/// Commands in a row: the first decides `ok`, the next ones only on its success.
fn lnf_chain(cmds: Vec<Cmd>) -> Vec<ApplyStep> {
    cmds.into_iter()
        .enumerate()
        .map(|(i, c)| if i == 0 { ApplyStep::Primary(c) } else { ApplyStep::IfOk(c) })
        .collect()
}

/// Steps of one appearance part in Plasma — the same keys and commands as in
/// Plasma 6.8 `KLookAndFeelManager::save`; icons and cursors — the section plan
/// ([`GlobalPress::sections`]). `Err` — why the part won't be applied.
fn kde_part_steps(theme: &str, press: &GlobalPress, part: LnfPart) -> Result<Vec<ApplyStep>, String> {
    let c = &press.contents;
    let deco = "org.kde.kdecoration2";
    Ok(match part {
        LnfPart::Icons | LnfPart::Cursors => {
            return press.sections.get(&part).cloned().unwrap_or_else(|| Err(tr("not applied")));
        }
        LnfPart::Colors => match c.value("kdeglobals/General/ColorScheme") {
            Some(name) => apply_kde_color_plan(name).steps,
            None => return Err(lnf_own_colors_reason()),
        },
        LnfPart::WidgetStyle => {
            let style = lnf_section_value(c, part).unwrap_or_default();
            let env = KvantumEnv { de_id: "kde", home: "", xdg_config_home: "", platform_theme: "", icon_theme: String::new(), qt_desktop_installed: false };
            apply_qt_style_plan(&style, &env).steps
        }
        LnfPart::PlasmaTheme => apply_kde_plasma_plan(c.value("plasmarc/Theme/name").unwrap_or_default()).steps,
        LnfPart::WindowSwitcher => lnf_chain(vec![lnf_kwrite(
            "kwinrc",
            "TabBox",
            "LayoutName",
            c.value("kwinrc/WindowSwitcher/LayoutName").unwrap_or_default(),
            false,
        )]),
        LnfPart::WindowDecoration => {
            let get = |k: &str, d: &'static str| c.value(&format!("kwinrc/{deco}/{k}")).unwrap_or(d).to_string();
            let no_plugin = if get("NoPlugin", "false") == "true" { "true" } else { "false" };
            lnf_chain(vec![
                lnf_kwrite("kwinrc", deco, "library", &get("library", "org.kde.breeze"), false),
                lnf_kwrite("kwinrc", deco, "theme", &get("theme", "Breeze"), true),
                lnf_kwrite("kwinrc", deco, "NoPlugin", no_plugin, true),
            ])
        }
        LnfPart::BorderSize => lnf_chain(vec![lnf_kwrite(
            "kwinrc",
            deco,
            "BorderSize",
            c.value(&format!("kwinrc/{deco}/BorderSize")).unwrap_or_default(),
            true,
        )]),
        LnfPart::Fonts => {
            let mut cmds: Vec<Cmd> = ["font", "fixed", "smallestReadableFont", "toolBarFont", "menuFont"]
                .into_iter()
                .filter_map(|k| {
                    c.value(&format!("kdeglobals/General/{k}")).map(|v| lnf_kwrite("kdeglobals", "General", k, v, true))
                })
                .collect();
            if let Some(v) = c.value("kdeglobals/WM/activeFont") {
                cmds.push(lnf_kwrite("kdeglobals", "WM", "activeFont", v, true));
            }
            if cmds.is_empty() {
                return Err(tr("the theme names no fonts that Plasma reads"));
            }
            cmds.push(cmd(
                &["dbus-send", "--session", "--type=signal", "/KDEPlatformTheme", "org.kde.KDEPlatformTheme.refreshFonts"],
                None,
            ));
            lnf_chain(cmds)
        }
        LnfPart::SplashScreen => {
            let splash = c.value("ksplashrc/KSplash/Theme").unwrap_or(theme);
            lnf_chain(vec![
                lnf_kwrite("ksplashrc", "KSplash", "Theme", splash, false),
                lnf_kwrite("ksplashrc", "KSplash", "Engine", "KSplashQML", false),
            ])
        }
        // Plasma 6.8 doesn't apply the wallpaper from a theme; layout parts are in
        // [`kde_layout_steps`].
        _ => Vec::new(),
    })
}

/// Steps of layout `lay` — as in Plasma: the layout via plasmashell (without
/// its own, Plasma substitutes Breeze itself), title bar buttons,
/// "borderless" for maximized windows, window placement.
fn kde_layout_steps(theme: &str, c: &LnfContents, lay: u32) -> Vec<ApplyStep> {
    let lay_bits = &c.layout;
    let kwrite = |file: &str, group: &str, key: &str, value: &str| ApplyStep::IfOk(lnf_kwrite(file, group, key, value, true));
    let has = |p: LnfPart| lay & p.bit() != 0;
    let mut steps = Vec::new();
    if has(LnfPart::DesktopLayout) {
        let arg = format!("string:{theme}");
        steps.push(ApplyStep::IfOk(cmd(
            &[
                "dbus-send",
                "--session",
                "--print-reply",
                "--dest=org.kde.plasmashell",
                "/PlasmaShell",
                "org.kde.PlasmaShell.loadLookAndFeelDefaultLayout",
                &arg,
            ],
            Some(30),
        )));
    }
    let deco = "org.kde.kdecoration2";
    if let (true, Some((left, right))) = (has(LnfPart::TitlebarLayout), &lay_bits.buttons) {
        steps.push(kwrite("kwinrc", deco, "ButtonsOnLeft", left));
        steps.push(kwrite("kwinrc", deco, "ButtonsOnRight", right));
    }
    if let (true, Some(value)) = (has(LnfPart::DesktopLayout), &lay_bits.borderless) {
        let value = if value.is_empty() { "false" } else { value.as_str() };
        steps.push(kwrite("kwinrc", "Windows", "BorderlessMaximizedWindows", value));
    }
    if let (true, Some(p)) = (has(LnfPart::WindowPlacement), &lay_bits.placement) {
        steps.push(kwrite("kwinrc", "Windows", "Placement", p));
    }
    steps
}

/// "KWin, reread the settings" — once at the end, as in Plasma.
fn kwin_reload_step() -> ApplyStep {
    ApplyStep::IfOk(cmd(&["dbus-send", "--session", "--type=signal", "/KWin", "org.kde.KWin.reloadConfig"], None))
}

/// The «Apply» plan of a global theme by the person's choice on desktop `de`.
/// `None` — nothing is checked.
///
/// Plasma (`apply_kde_global_choice_plan`):
/// * all of the theme's appearance — `plasma-apply-lookandfeel -a` (the same
///   `KLookAndFeelManager::save` as in the Plasma window); the whole layout —
///   plus `--resetLayout`, a part of the layout — its steps after that;
/// * a part of the appearance — `plasma-apply-lookandfeel` has no such mode,
///   so the steps are the same Plasma itself does for each checked part: the
///   theme is recorded as chosen, each part is written with its own keys and
///   commands (`plasma-apply-colorscheme`, `plasma-apply-desktoptheme`, icons
///   and cursors — by the section plan), then KWin rereads the settings.
///
/// Outside Plasma — the same parts as in «Distro Looks»: colors via the Qt
/// layer, application style, icons, cursors and title bar buttons — by the
/// Rexon section plan for this desktop, fonts — by the desktop's native keys.
pub fn apply_global_choice_plan(theme: &str, press: &GlobalPress, de: &str) -> Option<GlobalPlan> {
    let sel = global_selection(press, de);
    if sel == 0 {
        return None;
    }
    if de != "kde" {
        return Some(outside_global_plan(press, de, sel));
    }
    let c = &press.contents;
    let avail = lnf_available_mask(c);
    let selectable = avail & LNF_APPEARANCE_MASK & !LnfPart::Wallpaper.bit();
    let app = sel & LNF_APPEARANCE_MASK & !LnfPart::Wallpaper.bit();
    let lay = sel & LNF_LAYOUT_MASK;
    // The whole appearance of the theme (for a theme with only a wallpaper — that
    // too: Plasma doesn't apply it, but `-a` on such a theme does nothing extra).
    if sel & LNF_APPEARANCE_MASK != 0 && app == selectable {
        let mut plan = apply_kde_global_plan(theme);
        if lay != 0 && lay == avail & LNF_LAYOUT_MASK {
            if let Some(ApplyStep::Primary(c)) = plan.steps.first_mut() {
                c.argv.push("--resetLayout".into());
            }
        } else if lay != 0 {
            plan.steps.extend(kde_layout_steps(theme, c, lay));
            if lay & LNF_KWIN_MASK != 0 {
                plan.steps.push(kwin_reload_step());
            }
        }
        return Some(GlobalPlan { plan, skipped: Vec::new() });
    }
    let mut steps = vec![ApplyStep::Primary(cmd(
        &["kwriteconfig6", "--file", "kdeglobals", "--group", "KDE", "--key", "LookAndFeelPackage", theme],
        None,
    ))];
    steps.extend(kde_layout_steps(theme, c, lay));
    let mut skipped = Vec::new();
    for part in lnf_parts_of(app) {
        match kde_part_steps(theme, press, part) {
            Ok(s) => steps.extend(s),
            Err(why) => skipped.push(format!("{} — {why}", part.label())),
        }
    }
    if sel & LNF_KWIN_MASK != 0 {
        steps.push(kwin_reload_step());
    }
    Some(GlobalPlan { plan: Plan { steps, history_kind: "kde-global" }, skipped })
}

/// A KDE font (`Family,10,-1,5,…`) — family, size and style (weight and slant
/// from the QFont fields).
fn kde_font_parts(raw: &str) -> Option<(String, i32, crate::qt_settings::FontFace)> {
    let (family, size, face) = crate::qt_settings::parse_font_spec_face(raw);
    (!family.is_empty() && size > 0).then_some((family, size, face))
}

/// Outside Plasma: the steps of the checked parts one after another.
fn outside_global_plan(press: &GlobalPress, de: &str, sel: u32) -> GlobalPlan {
    let c = &press.contents;
    let mut steps = Vec::new();
    let mut skipped = Vec::new();
    for part in lnf_parts_of(sel) {
        let got = match part {
            LnfPart::Fonts => {
                // Interface, monospace and title — each desktop has its own
                // keys ([`crate::qt_settings::font_write_argv_face`]); weight
                // and slant go there as Pango words.
                let mut cmds = Vec::new();
                for (key, target) in
                    [("kdeglobals/General/font", "system"), ("kdeglobals/General/fixed", "mono"), ("kdeglobals/WM/activeFont", "title")]
                {
                    let Some((family, size, face)) = c.value(key).and_then(kde_font_parts) else { continue };
                    if !crate::qt_settings::font_target_available(de, target) {
                        continue;
                    }
                    if let Some(argv) = crate::qt_settings::font_write_argv_face(de, target, &family, size, face, "") {
                        cmds.push(Cmd { argv, timeout: None });
                    }
                }
                if cmds.is_empty() {
                    Err(tr("the theme names no fonts this desktop can use"))
                } else {
                    Ok(lnf_chain(cmds))
                }
            }
            _ => press.sections.get(&part).cloned().unwrap_or_else(|| Err(tr("not applied"))),
        };
        match got {
            Ok(s) => steps.extend(s),
            Err(why) => skipped.push(format!("{} — {why}", part.label())),
        }
    }
    GlobalPlan { plan: Plan { steps, history_kind: "kde-global" }, skipped }
}

/// Steps of the shared looks engine ([`crate::animations::LookStep`]) as steps
/// of the apply plan: this is how title bar buttons from the «Window title bar
/// buttons» section join a single global theme click. `Err` — a step the apply
/// plan can't do (an include into the compositor config).
pub fn look_steps_as_apply(steps: &[crate::animations::LookStep], config_home: &str) -> Result<Vec<ApplyStep>, String> {
    use crate::animations::LookStep;
    let mut out = Vec::new();
    for step in steps {
        match step {
            LookStep::Cmd(argv) => out.push(if out.is_empty() {
                ApplyStep::Primary(Cmd { argv: argv.clone(), timeout: None })
            } else {
                ApplyStep::IfOk(Cmd { argv: argv.clone(), timeout: None })
            }),
            LookStep::Notify(variants) => {
                if let Some(argv) = variants.first() {
                    out.push(ApplyStep::BestEffort(Cmd { argv: argv.clone(), timeout: None }));
                }
            }
            LookStep::GtkIni { key, value } => {
                for version in ["3.0", "4.0"] {
                    out.push(ApplyStep::GtkIni {
                        path: crate::gtk_sanitizer::gtk_ini_path(config_home, version),
                        key: key.clone(),
                        value: value.clone(),
                    });
                }
            }
            LookStep::WriteFile { path, content } => {
                let dir = std::path::Path::new(path).parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
                out.push(ApplyStep::WriteSetsOk { dir, path: path.clone(), content: content.clone() });
            }
            // Removing a directory (an abandoned extension copy of screen
            // effects) is not needed by a global theme — there is no plan for it.
            LookStep::Wm(_) | LookStep::RemoveTree { .. } => return Err(tr("not applied")),
        }
    }
    Ok(out)
}

/// The theme's title bar buttons (`ButtonsOnLeft`/`ButtonsOnRight`, KWin
/// letters) on the desktop from `ctx` — the same plan as the «Window title bar
/// buttons» section ([`crate::window_buttons::plan_layout`]). `Err` — why not.
pub fn lnf_buttons_steps(
    io: &dyn crate::animations::LookIo,
    ctx: &crate::animations::LookCtx,
    contents: &LnfContents,
) -> Result<Vec<ApplyStep>, String> {
    use crate::window_buttons as wb;
    let Some((left, right)) = &contents.layout.buttons else { return Err(tr("not applied")) };
    let mut wm = ctx.clone();
    wm.de = wb::wm_key(io, ctx);
    if let Some(why) = wb::rc_refusal(io, &wm) {
        return Err(why);
    }
    let layout = wb::parse(wb::Codec::Kde, left, right);
    look_steps_as_apply(&wb::plan_layout(io, &wm, &layout), &ctx.config_home)
}

/// Port of `apply_kde_plasma_style` (timeout=10, history "kde-plasma").
pub fn apply_kde_plasma_plan(theme: &str) -> Plan {
    Plan {
        steps: vec![ApplyStep::Primary(cmd(&["plasma-apply-desktoptheme", theme], Some(10)))],
        history_kind: "kde-plasma",
    }
}

/// Port of `apply_kde_color_scheme` (timeout=10, history "kde-colors").
pub fn apply_kde_color_plan(scheme: &str) -> Plan {
    Plan {
        steps: vec![ApplyStep::Primary(cmd(&["plasma-apply-colorscheme", scheme], Some(10)))],
        history_kind: "kde-colors",
    }
}

// ============================================================================
// Kvantum: a theme plus the Qt style engine
// ============================================================================
//
// `kvantummanager --set` writes only the Kvantum CHOICE (kvantum.kvconfig); it
// doesn't switch the Qt style engine. The second step is to tell Qt that the
// style is now Kvantum — and this is where desktops differ. Kvantum itself
// works everywhere: it is a regular Qt style plugin (`libkvantum.so`) that
// paints any Qt application regardless of the shell.

/// How the Qt style engine is chosen in this desktop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QtStyleWay {
    /// KDE: `widgetStyle` in `kdeglobals` plus a Plasma notification. The
    /// environment variable must NOT be set here — it would override the stock
    /// style choice.
    Kdeglobals,
    /// LXQt: `[Qt] style` in `~/.config/lxqt/lxqt.conf` — the same place
    /// lxqt-config-appearance writes to.
    LxqtConf,
    /// Others: no Qt style switcher of their own. The full path is `qt6ct` (style,
    /// fonts and icons for Qt applications), the fallback is the
    /// `QT_STYLE_OVERRIDE` variable.
    PlatformTheme,
    /// Deepin: DTK programs (and other Qt programs via Deepin's own platform theme)
    /// are drawn with its Chameleon style. Any session-wide variable —
    /// `QT_QPA_PLATFORMTHEME=qt6ct` or `QT_STYLE_OVERRIDE` — would take Chameleon
    /// away from the Control Center, the file manager and other Deepin programs.
    /// The Qt style is not switched here: the plan is empty, applying honestly
    /// fails, and the section is hidden in Deepin with an explanation
    /// (`de_customization::unavailable_note`).
    Deepin,
}

/// How the Qt style is switched on this desktop.
///
/// An unknown identifier — the generic path: the environment variable is
/// understood by Qt itself, not the shell, so it works even where we didn't
/// recognize the desktop.
pub fn qt_style_way(de_id: &str) -> QtStyleWay {
    use crate::de_customization::{de_from_id, De};
    match de_from_id(de_id) {
        Some(De::Kde) => QtStyleWay::Kdeglobals,
        Some(De::Lxqt) => QtStyleWay::LxqtConf,
        Some(De::Deepin) => QtStyleWay::Deepin,
        _ => QtStyleWay::PlatformTheme,
    }
}

/// Binary of the full Qt style switcher (also the key in the packages table).
pub const QT_STYLE_TOOL: &str = "qt6ct";

/// Package name of the Qt style switcher for the distribution family.
///
/// The names table is shared, `utils::TOOL_PACKAGES` (imagemagick, icoutils and
/// others live there too): starting a second "package by family" table would be
/// exactly the duplication rule #12 forbids.
///
/// `None` — nothing to install with: in NixOS packages are installed by editing
/// the configuration, Clear Linux has no bundle. This is not a refusal — the
/// theme is applied by the fallback path.
pub fn qt_style_helper_pkg(pkg_family: &str) -> Option<String> {
    let pkg = crate::utils::tool_package_name(QT_STYLE_TOOL, pkg_family);
    if pkg.is_empty() {
        None
    } else {
        Some(pkg)
    }
}

/// Rexon environment variables file for the Qt style (next to
/// `rexon-terminal.conf` — the same `~/.config/environment.d` mechanism as the
/// terminal's).
pub const QT_STYLE_ENV_FILE: &str = "rexon-qt-style.conf";

/// Where Rexon keeps session variables that must NOT reach every desktop
/// (relative to `$XDG_CONFIG_HOME`). systemd does not read this directory;
/// [`crate::compositor_autostart::sync_session_env`] mirrors it only into
/// desktop-bound places (the `~/.xprofile` block with `case` on the desktop,
/// the labwc `environment` files, Hyprland's `env =`).
pub const DESKTOP_BOUND_ENV_DIR: &str = "rexon/session-env";

/// Desktops with their own Qt integration (a platform theme of their own):
/// `QT_QPA_PLATFORMTHEME=qt6ct` or `QT_STYLE_OVERRIDE` in their session takes it
/// away from their programs.
pub const OWN_QT_THEME_DESKTOPS: &[&str] = &["kde", "lxqt", "deepin"];

/// Whether a desktop with its own Qt integration ([`OWN_QT_THEME_DESKTOPS`]) is
/// installed. `has_bin` — whether a program is in `PATH`.
pub fn own_qt_desktop_installed(has_bin: &dyn Fn(&str) -> bool) -> bool {
    OWN_QT_THEME_DESKTOPS
        .iter()
        .any(|de| crate::system::de_installed_binaries(de).iter().any(|b| has_bin(b)))
}

/// Both places the Qt style variables file may be in (shared first).
pub fn qt_style_env_paths(config_home: &str) -> [String; 2] {
    [
        format!("{config_home}/environment.d/{QT_STYLE_ENV_FILE}"),
        format!("{config_home}/{DESKTOP_BOUND_ENV_DIR}/{QT_STYLE_ENV_FILE}"),
    ]
}

/// Environment for the Kvantum theme apply plan.
pub struct KvantumEnv<'a> {
    /// Desktop identifier (`system::detect_current_de`).
    pub de_id: &'a str,
    pub home: &'a str,
    /// `$XDG_CONFIG_HOME` (empty — `~/.config`).
    pub xdg_config_home: &'a str,
    /// Name of a WORKING Qt style switcher (`qt6ct`/`qt5ct`), if the system has
    /// one. Empty — we take the fallback path via `QT_STYLE_OVERRIDE`: the person
    /// isn't left without a result, just without fonts and icons.
    pub platform_theme: &'a str,
    /// The icon theme the desktop currently gives to programs (empty — unknown).
    /// Goes into the qt6ct config if it has none of its own yet: without it Qt
    /// programs under qt6ct lose their icons (Papirus → hicolor, D2).
    pub icon_theme: String,
    /// A desktop with its own Qt integration is installed
    /// ([`own_qt_desktop_installed`]): then the variables do not go into the
    /// shared `environment.d` (see [`qt_style_switch_steps`]).
    pub qt_desktop_installed: bool,
}

/// Kvantum theme apply plan — port of `apply_kvantum_theme`, extended to all
/// desktops.
///
/// The first step is common: `kvantummanager --set` (it also determines `ok`
/// and the history record). The second depends on [`qt_style_way`]:
///   * KDE — `kdeglobals` + a Plasma notification (as before);
///   * LXQt — `[Qt] style` in its own `lxqt.conf`;
///   * others — `~/.config/environment.d`: `QT_QPA_PLATFORMTHEME=qt6ct` plus
///     the style in the qt6ct config if there is one, otherwise
///     `QT_STYLE_OVERRIDE`.
///
/// ⚠️ Environment variables are read at session login — the effect comes after
/// re-login. The person must be told so (the matrix's `condition_note`).
pub fn apply_kvantum_plan(theme: &str, env: &KvantumEnv) -> Plan {
    if qt_style_way(env.de_id) == QtStyleWay::Deepin {
        // Choosing a theme in Kvantum without switching the style shows nothing —
        // "Applied" would be a lie (see [`QtStyleWay::Deepin`]).
        return Plan { steps: Vec::new(), history_kind: "kvantum" };
    }
    let mut steps = vec![ApplyStep::Primary(cmd(
        &["kvantummanager", "--set", theme],
        Some(10),
    ))];
    steps.extend(qt_style_switch_steps("kvantum", env, true));
    Plan { steps, history_kind: "kvantum" }
}

/// Plan for choosing a Qt style by name (the «Application Style» section:
/// Breeze, Fusion, kvantum…) — in ALL desktops, by the same [`qt_style_way`]
/// branching as Kvantum. Previously the section wrote only `kdeglobals`, and
/// outside Plasma only programs with the KDE platform theme read it: «Apply»
/// was green, but the style didn't change.
///
/// `env` is the same environment as Kvantum's (the type name is historical).
/// Outside KDE and LXQt the style appears after re-login — as with Kvantum.
pub fn apply_qt_style_plan(style: &str, env: &KvantumEnv) -> Plan {
    Plan { steps: qt_style_switch_steps(style, env, false), history_kind: "widget-style" }
}

/// Steps "tell Qt which style to take" — shared by Kvantum and «Application
/// Style» (rule #12: one branching for all).
///
/// `after_primary` — these steps were already preceded by their own `Primary`
/// (for Kvantum — `kvantummanager --set`), and they run only on its success. If
/// not, the style write itself comes first: on KDE — `kwriteconfig6`
/// (`Primary`), in other desktops the write goes into a file we maintain
/// ourselves, hence `ForceOk`.
fn qt_style_switch_steps(style: &str, env: &KvantumEnv, after_primary: bool) -> Vec<ApplyStep> {
    let cfg = crate::utils::config_home(
        if env.xdg_config_home.is_empty() { None } else { Some(env.xdg_config_home) },
        env.home,
    );
    let mut steps = Vec::new();

    match qt_style_way(env.de_id) {
        QtStyleWay::Kdeglobals => {
            let write = cmd(
                &[
                    "kwriteconfig6", "--file", "kdeglobals", "--group", "KDE",
                    "--key", "widgetStyle", style,
                ],
                None,
            );
            steps.push(if after_primary { ApplyStep::IfOk(write) } else { ApplyStep::Primary(write) });
            // `notifyChange(2, 0)` — StyleChanged (KGlobalSettings::ChangeType:
            // 0 — palette, 1 — font, 2 — style). Previously 0 was sent, and
            // running Qt programs didn't change the style until restart
            // (D3, desktops check 02.10.2026: after 0 the program stayed
            // on breeze, after 2 it switched to fusion).
            steps.push(ApplyStep::IfOk(cmd(
                &[
                    "dbus-send", "--type=signal", "/KGlobalSettings",
                    "org.kde.KGlobalSettings.notifyChange", "int32:2", "int32:0",
                ],
                None,
            )));
        }
        QtStyleWay::LxqtConf => {
            if !after_primary {
                steps.push(ApplyStep::ForceOk);
            }
            steps.push(ApplyStep::IniSetIfOk {
                path: format!("{cfg}/lxqt/lxqt.conf"),
                section: "[Qt]".into(),
                key: "style".into(),
                value: style.into(),
            });
        }
        QtStyleWay::PlatformTheme => {
            if !after_primary {
                steps.push(ApplyStep::ForceOk);
            }
            let tool = env.platform_theme;
            let content = if tool.is_empty() {
                // The fallback path: Qt reads this variable itself, without any
                // packages. There will be a style, but no fonts and icons.
                format!("QT_STYLE_OVERRIDE={style}\n")
            } else {
                // The full path: the style is chosen in the qt6ct config, so
                // QT_STYLE_OVERRIDE is not written — otherwise the person changes
                // the style in qt6ct, and the variable silently overrides it.
                steps.push(ApplyStep::IniSetIfOk {
                    path: format!("{cfg}/{tool}/{tool}.conf"),
                    section: "[Appearance]".into(),
                    key: "style".into(),
                    value: style.into(),
                });
                // We don't override qt6ct's own icon choice; if there is none,
                // we put the desktop's current icons, otherwise after re-login
                // Qt programs stay with `hicolor` (D2).
                if !env.icon_theme.is_empty() {
                    steps.push(ApplyStep::IniSetGuarded {
                        path: format!("{cfg}/{tool}/{tool}.conf"),
                        section: "[Appearance]".into(),
                        key: "icon_theme".into(),
                        value: env.icon_theme.clone(),
                        only_if_file: false,
                        keep_existing: true,
                    });
                }
                format!("QT_QPA_PLATFORMTHEME={tool}\n")
            };
            // ⚠️ `~/.config/environment.d` is read by EVERY session systemd
            // starts, and Plasma explicitly imports it into its own environment
            // (`startplasma`, after its env scripts — nothing there can undo it).
            // With qt6ct there, «Kvantum in GNOME» moved Plasma's programs off
            // the KDE platform theme. So while a desktop with its own Qt
            // integration is installed, the variables go into Rexon's own
            // directory, which only desktop-bound places mirror
            // ([`DESKTOP_BOUND_ENV_DIR`]), and the shared file is removed.
            let [shared, bound] = qt_style_env_paths(&cfg);
            if env.qt_desktop_installed {
                steps.push(ApplyStep::WriteIfOk {
                    dir: format!("{cfg}/{DESKTOP_BOUND_ENV_DIR}"),
                    path: bound,
                    content,
                });
                steps.push(ApplyStep::RemoveIfOk { path: shared });
            } else {
                steps.push(ApplyStep::WriteIfOk {
                    dir: format!("{cfg}/environment.d"),
                    path: shared,
                    content,
                });
                steps.push(ApplyStep::RemoveIfOk { path: bound });
            }
        }
        // Neither a variable nor "success" — see [`QtStyleWay::Deepin`].
        QtStyleWay::Deepin => {}
    }
    steps
}

// There are deliberately no "install the package?" texts here. The customer
// should not see a question about an extra library and figure out why choosing
// a theme asks for a password: this is exactly the question where the answer
// "no" gives the worst result (rule #13, foolproofing — like SDDM, which gets
// installed BY ITSELF when a tiling desktop is installed). The package arrives
// by two silent paths — the `depends` of the Rexon package and the system
// installer (`installer::qt_style_install_cmd`), and if it is not in the
// repositories at all, `QT_STYLE_OVERRIDE` works. The only thing we tell the
// person is that the changes appear after re-login (the section caveat in the
// matrix).

// ============================================================================
// What a theme asks for in addition: an icon set and a cursor (pure part)
// ============================================================================
//
// A GTK theme with a metatheme (`index.theme`, section `[X-GNOME-Metatheme]`
// or `[Desktop Entry]`) names the icon set (`IconTheme=`) and the cursor
// (`CursorTheme=`) its author intended it with. Desktops don't read these keys
// themselves: `gsettings set … gtk-theme` changes only the widgets. So «Apply»
// of such a theme applies them too — with the same plan as the buttons of the
// «Icons» and «Cursors» sections ([`apply_icon_plan_xdg`],
// [`apply_cursor_plan_sized`]), i.e. in all desktops at once.
//
// ⛔ A name that is not on disk is NOT written into the desktop settings: GTK
// would silently fall back to hicolor/Adwaita, and Rexon would show "applied".
// No set — an honest warning, the current icons and cursor stay.

/// What exactly the theme asks for: an icon set or a cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DepRole {
    Icons,
    Cursor,
}

/// The icon set and cursor the theme asks for in addition (`None` — doesn't ask).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ThemeDeps {
    pub icons: Option<String>,
    pub cursor: Option<String>,
}

/// Where a metatheme keeps its keys. The order is as in mate-appearance and
/// gnome-control-center of former years: the main section, then the common one.
const DEPS_SECTIONS: [&str; 2] = ["X-GNOME-Metatheme", "Desktop Entry"];

/// Values that don't count as a request for a set: `hicolor` is the fallback
/// set any set falls back to (our `.msstyles` import also writes it when the
/// theme has no icons of its own), `default` is not a set name but a link to
/// the chosen cursor.
const NOT_A_REQUEST: &[&str] = &["hicolor", "default"];

/// What the theme asks for in addition — parsing its `index.theme`.
pub fn theme_deps(index_theme: &str) -> ThemeDeps {
    let get = |key: &str| {
        DEPS_SECTIONS
            .iter()
            .find_map(|section| ini_key_value(index_theme, section, key))
            .filter(|v| !NOT_A_REQUEST.iter().any(|n| n.eq_ignore_ascii_case(v)))
            // A set name is a directory name, not a path: "../x" from someone
            // else's index.theme would go into search, download and settings.
            .filter(|v| !v.contains('/') && v != "." && v != "..")
    };
    ThemeDeps { icons: get("IconTheme"), cursor: get("CursorTheme") }
}

/// A known set: under which names it occurs and which package installs it in
/// each distribution family (empty — no package or we don't know it).
struct KnownSet {
    role: DepRole,
    /// Names of one and the same set. The first is the main one. Different
    /// distributions put the same cursor under different names: the theme asks for
    /// "DMZ-White", while the Arch package `xcursor-vanilla-dmz` installs
    /// "Vanilla-DMZ".
    names: &'static [&'static str],
    /// A set with variants ("Papirus-Dark", "WhiteSur-dark"): the same package also
    /// brings the `<name>-<variant>` names.
    variants: bool,
    arch: &'static str,
    debian: &'static str,
    fedora: &'static str,
    suse: &'static str,
    /// Where the set can be put into the home directory from, without a password
    /// and without a build ([`dep_home_source`]).
    home: HomeSource,
}

/// A ready set that is installed into `~/.local/share/icons` by plain
/// unpacking — without a password, without a build and without third-party
/// install scripts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HomeSource {
    /// Only a system package (or nothing).
    None,
    /// A KDE store item (OCS): its number and the file name among the item's links.
    /// `names` — which sets the archive actually carries (case-insensitive): a
    /// variant missing from the archive is not installed from here.
    Ocs { content_id: &'static str, file: &'static str, names: &'static [&'static str] },
}

/// Where the set is installed into the home directory from ([`HomeSource`]).
/// `None` — the set is unknown to us or can't be installed this way.
pub fn dep_home_source(role: DepRole, wanted: &str) -> Option<HomeSource> {
    KNOWN_SETS
        .iter()
        .find(|s| known_set_matches(s, role, wanted))
        .map(|s| s.home)
        .filter(|h| match h {
            HomeSource::None => false,
            HomeSource::Ocs { names, .. } => names.iter().any(|n| n.eq_ignore_ascii_case(wanted.trim())),
        })
}

/// Table of known sets. Package names were checked on 16.10.2026: Arch —
/// `pacman -Si` and AUR RPC (`whitesur-*` and `mint-y-icons` on plain Arch are
/// only in the AUR, in CachyOS `mint-y-icons` is in the repository — the
/// executor picks the path by fact); Debian/Ubuntu (trixie, noble) — file lists
/// of packages.debian.org/packages.ubuntu.com; Fedora (f43, f44) — mdapi,
/// `dmz-cursor-themes` was dropped there; openSUSE (Tumbleweed, Leap 16.0) — rpm
/// from download.opensuse.org. WhiteSur is in none of the three.
const KNOWN_SETS: &[KnownSet] = &[
    KnownSet {
        role: DepRole::Cursor,
        names: &["DMZ-White", "Vanilla-DMZ", "dmz"],
        variants: false,
        arch: "xcursor-vanilla-dmz",
        debian: "dmz-cursor-theme",
        fedora: "",
        suse: "dmz-icon-theme-cursors",
        home: HomeSource::None,
    },
    KnownSet {
        role: DepRole::Cursor,
        names: &["DMZ-Black", "Vanilla-DMZ-AA", "dmz-aa"],
        variants: false,
        arch: "xcursor-vanilla-dmz-aa",
        debian: "dmz-cursor-theme",
        fedora: "",
        suse: "dmz-icon-theme-cursors",
        home: HomeSource::None,
    },
    KnownSet {
        role: DepRole::Cursor,
        names: &["Adwaita"],
        variants: false,
        arch: "adwaita-cursors",
        debian: "adwaita-icon-theme",
        fedora: "adwaita-cursor-theme",
        suse: "adwaita-icon-theme",
        home: HomeSource::None,
    },
    KnownSet {
        role: DepRole::Cursor,
        names: &["breeze_cursors"],
        variants: false,
        arch: "breeze",
        debian: "breeze-cursor-theme",
        fedora: "breeze-cursor-theme",
        suse: "breeze6-cursors",
        home: HomeSource::None,
    },
    KnownSet {
        role: DepRole::Cursor,
        names: &["WhiteSur-cursors"],
        variants: false,
        arch: "whitesur-cursor-theme-git",
        debian: "",
        fedora: "",
        suse: "",
        // The KDE store item "WhiteSur cursors" (same author): the archive
        // WhiteSur-cursors.tar.xz carries the built WhiteSur-cursors theme
        // (checked by unpacking 25.10.2026).
        home: HomeSource::Ocs { content_id: "1411743", file: "WhiteSur-cursors.tar.xz", names: &["WhiteSur-cursors"] },
    },
    // "gnome" is the name of the GNOME 2 icon set; in GNOME 3 Adwaita replaced it.
    KnownSet {
        role: DepRole::Icons,
        names: &["Adwaita", "gnome"],
        variants: false,
        arch: "adwaita-icon-theme",
        debian: "adwaita-icon-theme",
        fedora: "adwaita-icon-theme",
        suse: "adwaita-icon-theme",
        home: HomeSource::None,
    },
    KnownSet {
        role: DepRole::Icons,
        names: &["breeze"],
        variants: true,
        arch: "breeze-icons",
        debian: "breeze-icon-theme",
        fedora: "breeze-icon-theme",
        suse: "kf6-breeze-icons",
        home: HomeSource::None,
    },
    // In Fedora the Papirus variants are separate subpackages; the entries come
    // before the general one because the search takes the first match.
    KnownSet {
        role: DepRole::Icons,
        names: &["Papirus-Dark"],
        variants: false,
        arch: "papirus-icon-theme",
        debian: "papirus-icon-theme",
        fedora: "papirus-icon-theme-dark",
        suse: "papirus-icon-theme",
        home: HomeSource::None,
    },
    KnownSet {
        role: DepRole::Icons,
        names: &["Papirus-Light"],
        variants: false,
        arch: "papirus-icon-theme",
        debian: "papirus-icon-theme",
        fedora: "papirus-icon-theme-light",
        suse: "papirus-icon-theme",
        home: HomeSource::None,
    },
    KnownSet {
        role: DepRole::Icons,
        names: &["Papirus"],
        variants: true,
        arch: "papirus-icon-theme",
        debian: "papirus-icon-theme",
        fedora: "papirus-icon-theme",
        suse: "papirus-icon-theme",
        home: HomeSource::None,
    },
    KnownSet {
        role: DepRole::Icons,
        names: &["Mint-Y"],
        variants: true,
        arch: "mint-y-icons",
        debian: "mint-y-icons",
        fedora: "mint-y-icons",
        suse: "mint-y-icon-theme",
        home: HomeSource::None,
    },
    KnownSet {
        role: DepRole::Icons,
        names: &["WhiteSur"],
        variants: true,
        arch: "whitesur-icon-theme",
        debian: "",
        fedora: "",
        suse: "",
        // The KDE store item "WhiteSur icon theme" (same author): the archive
        // 01-WhiteSur.tar.xz carries WhiteSur, WhiteSur-dark and WhiteSur-light
        // (checked by unpacking 17.10.2026). The color variants are other files.
        home: HomeSource::Ocs {
            content_id: "1405756",
            file: "01-WhiteSur.tar.xz",
            names: &["WhiteSur", "WhiteSur-dark", "WhiteSur-light"],
        },
    },
];

/// Whether a set from the table fits the theme's request. Case doesn't matter:
/// the WhiteSur theme asks for "WhiteSur-Dark", and the package installs
/// "WhiteSur-dark".
fn known_set_matches(set: &KnownSet, role: DepRole, wanted: &str) -> bool {
    if set.role != role {
        return false;
    }
    let w = wanted.to_lowercase();
    set.names.iter().any(|n| {
        let n = n.to_lowercase();
        w == n || (set.variants && w.strip_prefix(&n).is_some_and(|rest| rest.starts_with('-')))
    })
}

/// Under which names to look for the set on disk: the request itself, then other
/// names of the same set ([`KNOWN_SETS`]). Without repeats.
pub fn dep_aliases(role: DepRole, wanted: &str) -> Vec<String> {
    let mut out = vec![wanted.to_string()];
    for set in KNOWN_SETS.iter().filter(|s| !s.variants && known_set_matches(s, role, wanted)) {
        for n in set.names {
            if !out.iter().any(|o| o == n) {
                out.push(n.to_string());
            }
        }
    }
    out
}

/// The package that installs the set in family `pkg_family` (`arch`, `debian`,
/// `fedora`, `suse`). `None` — the set is unknown to us or there is no package.
pub fn dep_package(role: DepRole, wanted: &str, pkg_family: &str) -> Option<String> {
    let set = KNOWN_SETS.iter().find(|s| known_set_matches(s, role, wanted))?;
    let pkg = match pkg_family.to_lowercase().as_str() {
        "arch" => set.arch,
        "debian" => set.debian,
        "fedora" => set.fedora,
        "suse" => set.suse,
        _ => "",
    };
    (!pkg.is_empty()).then(|| pkg.to_string())
}

/// All packages of the table's sets for family `pkg_family` — the install
/// whitelist of the package helper (`system_plan::pkg_helper_install_allowlist`):
/// it installs a theme's icons and cursor only from here.
pub fn known_set_packages(pkg_family: &str) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for set in KNOWN_SETS {
        let pkg = match pkg_family {
            "arch" => set.arch,
            "debian" => set.debian,
            "fedora" => set.fedora,
            "suse" => set.suse,
            _ => "",
        };
        if !pkg.is_empty() && !out.contains(&pkg) {
            out.push(pkg);
        }
    }
    out
}

/// Directories where icons and cursors are looked for — per the XDG spec:
/// `~/.icons`, `$XDG_DATA_HOME/icons`, then `<each of $XDG_DATA_DIRS>/icons`.
pub fn dep_search_roots(home: &str, data_home: &str, xdg_data_dirs: &str) -> Vec<String> {
    xdg_search_roots(home, ".icons", data_home, xdg_data_dirs, "icons")
}

/// GTK theme directories in GTK's search order: `~/.themes`,
/// `$XDG_DATA_HOME/themes`, then `<each of $XDG_DATA_DIRS>/themes`.
pub fn gtk_theme_search_roots(home: &str, data_home: &str, xdg_data_dirs: &str) -> Vec<String> {
    xdg_search_roots(home, ".themes", data_home, xdg_data_dirs, "themes")
}

fn xdg_search_roots(home: &str, legacy: &str, data_home: &str, xdg_data_dirs: &str, sub: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |p: String| {
        if !out.contains(&p) {
            out.push(p);
        }
    };
    let home = home.trim_end_matches('/');
    if home.starts_with('/') {
        push(format!("{home}/{legacy}"));
    }
    let data = data_home.trim_end_matches('/');
    if data.starts_with('/') {
        push(format!("{data}/{sub}"));
    }
    for d in crate::plugin_journal::data_dirs(xdg_data_dirs) {
        push(format!("{d}/{sub}"));
    }
    out
}

/// Whether directory `dir` is fit as a set of role `role`: a cursor has
/// `cursors/`, icons have `index.theme` with a non-empty `Directories=` (the
/// same measure as [`is_valid_icon_theme`]: a cursor theme with just an
/// `index.theme` doesn't count as an icon set).
fn dep_dir_usable(role: DepRole, dir: &str, is_dir: &dyn Fn(&str) -> bool, read: &dyn Fn(&str) -> Option<String>) -> bool {
    match role {
        DepRole::Cursor => is_dir(&format!("{dir}/cursors")),
        DepRole::Icons => read(&format!("{dir}/index.theme")).is_some_and(|text| {
            text.lines().map(str::trim).any(|s| s.starts_with("Directories=") && s != "Directories=")
        }),
    }
}

/// Name of an installed set that satisfies the theme's request: the exact name,
/// another name of the same set, then the same case-insensitively. `None` —
/// nothing suitable on disk.
pub fn find_installed_dep(
    role: DepRole,
    wanted: &str,
    roots: &[String],
    is_dir: &dyn Fn(&str) -> bool,
    list_dir: &dyn Fn(&str) -> Vec<String>,
    read: &dyn Fn(&str) -> Option<String>,
) -> Option<String> {
    let aliases = dep_aliases(role, wanted);
    for name in &aliases {
        if roots.iter().any(|r| dep_dir_usable(role, &format!("{r}/{name}"), is_dir, read)) {
            return Some(name.clone());
        }
    }
    for name in &aliases {
        for root in roots {
            for entry in list_dir(root) {
                if entry.eq_ignore_ascii_case(name) && dep_dir_usable(role, &format!("{root}/{entry}"), is_dir, read) {
                    return Some(entry);
                }
            }
        }
    }
    None
}

/// State of one request of a theme.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DepState {
    /// The set is installed — apply it under this name.
    Installed(String),
    /// The set is missing, but this package installs it.
    Installable(String),
    /// The set is missing, and Rexon doesn't know where to get it.
    Missing,
}

/// A theme's request and what about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DepItem {
    pub role: DepRole,
    pub wanted: String,
    pub state: DepState,
}

/// What to do with the theme's requests: `find` looks for an installed set
/// ([`find_installed_dep`]), the package — by the table for `pkg_family`.
pub fn plan_theme_deps(
    deps: &ThemeDeps,
    pkg_family: &str,
    find: &dyn Fn(DepRole, &str) -> Option<String>,
) -> Vec<DepItem> {
    let mut out = Vec::new();
    for (role, wanted) in [(DepRole::Icons, &deps.icons), (DepRole::Cursor, &deps.cursor)] {
        let Some(wanted) = wanted else { continue };
        let state = match find(role, wanted) {
            Some(name) => DepState::Installed(name),
            None => match dep_package(role, wanted, pkg_family) {
                Some(pkg) => DepState::Installable(pkg),
                None => DepState::Missing,
            },
        };
        out.push(DepItem { role, wanted: wanted.clone(), state });
    }
    out
}

/// "the icon set “X”" / "the cursor “X”" — for the phrases below.
pub fn dep_label(role: DepRole, name: &str) -> String {
    match role {
        // TRANSLATORS: part of a sentence, %1 is an icon set name.
        DepRole::Icons => trf("the icon set “%1”", &[&name]),
        // TRANSLATORS: part of a sentence, %1 is a cursor theme name.
        DepRole::Cursor => trf("the cursor “%1”", &[&name]),
    }
}

/// What to install before «Apply» of a GTK theme — BY ITSELF, without a
/// "Yes/No" question (the owner's decision 24.10; the protection is the polkit
/// password window): what is not on disk but can be installed as a package or
/// downloaded into the home directory. Icons — only when the person chose
/// «Download and turn on» (`icons_ok`, decision 3a). What can't be got anywhere
/// ([`DepState::Missing`] without a home source) is not handled here: the theme
/// is applied, and the result says what is missing.
pub fn deps_to_prepare(items: &[DepItem], icons_ok: bool) -> Vec<DepItem> {
    items
        .iter()
        .filter(|i| !matches!(i.state, DepState::Installed(_)))
        .filter(|i| i.role != DepRole::Icons || icons_ok)
        .filter(|i| matches!(i.state, DepState::Installable(_)) || dep_home_source(i.role, &i.wanted).is_some())
        .cloned()
        .collect()
}

/// «Apply» refusal: what comes with the theme didn't install ("all or nothing"
/// — a theme without its own cursor is not applied). `why` is the failure
/// reason; the password window was closed
/// ([`crate::appearance_installer::password_not_confirmed`]) — "changed their
/// mind": advice to press «Apply» again and enter the password.
pub fn deps_refusal(theme: &str, missing: &[DepItem], why: &str) -> String {
    let what: Vec<String> = missing.iter().map(|i| dep_label(i.role, &i.wanted)).collect();
    if why == crate::appearance_installer::password_not_confirmed() {
        return crate::i18n::fill(
            // TRANSLATORS: %1 is the theme name, %2 a list like “the cursor “X””.
            &crate::i18n::ntr(
                "The theme “%1” wasn't applied: it comes with %2, which isn't installed. Press “Apply” \
                 on the theme again and enter the password to install it.",
                "The theme “%1” wasn't applied: it comes with %2, which aren't installed. Press “Apply” \
                 on the theme again and enter the password to install them.",
                what.len() as u64,
            ),
            &[&theme, &what.join(", ")],
        );
    }
    trf(
        // TRANSLATORS: %1 is the theme name, %2 a list like “the cursor “X””,
        // %3 the reason.
        "The theme “%1” wasn't applied: couldn't install %2 that it comes with: %3.",
        &[&theme, &what.join(", "), &why],
    )
}

// ── Theme icons: ask, don't turn on silently ───────────────────────────────
//
// Decision of Wehrwolfmann 20.10.2026 (3a): a theme designed for its own icon
// set (the "like Windows XP" look) used to silently download it and turn it on
// instead of the one the person chose. Now — one question «Download and turn
// on» / «Keep mine» («Turn on» for an installed set). The answer is remembered
// for this theme (setting [`THEME_ICONS_CHOICE_KEY`], by theme name): on the
// next apply of the same theme we don't ask. «Keep mine» — the theme is
// applied whole except the icons.

/// Rexon setting: answers to the theme icons question, `{theme: turn on?}`.
pub const THEME_ICONS_CHOICE_KEY: &str = "theme_icons_choice";

/// The remembered answer for theme `theme` from the [`THEME_ICONS_CHOICE_KEY`]
/// setting value; `None` — not asked.
pub fn icons_choice_of(value: Option<&Value>, theme: &str) -> Option<bool> {
    value?.get(theme)?.as_bool()
}

/// The setting value with answer `yes` for theme `theme` (other answers stay).
pub fn icons_choice_with(value: Option<&Value>, theme: &str, yes: bool) -> Value {
    let mut map = value.and_then(Value::as_object).cloned().unwrap_or_default();
    map.insert(theme.to_string(), Value::Bool(yes));
    Value::Object(map)
}

/// The theme icons question: title, text, "yes" and "no" captions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IconsQuestion {
    pub title: String,
    pub text: String,
    pub yes: String,
    pub no: String,
}

/// Whether to ask about the icons of theme `theme` when applying it. `items` is
/// the [`plan_theme_deps`] plan, `current` — the icons in place now,
/// `remembered` — [`icons_choice_of`]. `None` — nothing to ask: the theme
/// doesn't ask for icons, they exist nowhere (the result will say so), they are
/// already in place, or the answer has already been given.
pub fn icons_question(theme: &str, items: &[DepItem], current: &str, remembered: Option<bool>) -> Option<IconsQuestion> {
    if remembered.is_some() {
        return None;
    }
    let item = items.iter().find(|i| i.role == DepRole::Icons)?;
    let installed = match &item.state {
        DepState::Installed(name) if name == current || item.wanted == current => return None,
        DepState::Installed(_) => true,
        DepState::Installable(_) => false,
        DepState::Missing if dep_home_source(DepRole::Icons, &item.wanted).is_some() => false,
        DepState::Missing => return None,
    };
    let (text, yes) = if installed {
        (
            // TRANSLATORS: %1 is the theme name, %2 is an icon set name.
            trf("The theme “%1” is made for the icons “%2”. Turn them on?", &[&theme, &item.wanted]),
            tr("Turn on"),
        )
    } else {
        (
            // TRANSLATORS: %1 is the theme name, %2 is an icon set name.
            trf("The theme “%1” is made for the icons “%2”. Download and turn them on?", &[&theme, &item.wanted]),
            tr("Download and turn on"),
        )
    };
    Some(IconsQuestion { title: tr("Theme icons"), text, yes, no: tr("Keep mine") })
}

/// Result line: the theme icons were not turned on per the «Keep mine» answer.
pub fn dep_icons_kept_line(wanted: &str) -> String {
    // TRANSLATORS: %1 is an icon set name.
    trf("Your icons stay — the theme's icons “%1” were not turned on.", &[&wanted])
}

/// Result line: the set was applied together with the theme.
pub fn dep_applied_line(role: DepRole, applied: &str, wanted: &str) -> String {
    if applied == wanted {
        trf("Also applied %1 that the theme comes with.", &[&dep_label(role, applied)])
    } else {
        trf(
            // TRANSLATORS: %1 is e.g. “the cursor “Vanilla-DMZ””, %2 is the name the theme asked for.
            "Also applied %1 that the theme comes with (the theme calls it “%2”).",
            &[&dep_label(role, applied), &wanted],
        )
    }
}

/// Result line: the set is missing, and there is nowhere to get it.
pub fn dep_missing_line(role: DepRole, wanted: &str) -> String {
    let (what, place) = match role {
        DepRole::Icons => (tr("Your current icons stay."), crate::i18n::trc("appearance section", "Icons")),
        DepRole::Cursor => (tr("Your current cursor stays."), crate::i18n::trc("appearance section", "Cursors")),
    };
    trf(
        // TRANSLATORS: %1 is e.g. “the icon set “X””, %2 is “Your current icons stay.”,
        // %3 is a section of Rexon's Appearance tab.
        "⚠️ The theme comes with %1, but it isn't installed and Rexon doesn't know where to get it. %2 You can look for it in “%3”.",
        &[&dep_label(role, wanted), &what, &place],
    )
}

/// Result line: the set was not installed (install declined or failed).
pub fn dep_not_installed_line(role: DepRole, wanted: &str, failure: Option<&str>) -> String {
    let what = match role {
        DepRole::Icons => tr("Your current icons stay."),
        DepRole::Cursor => tr("Your current cursor stays."),
    };
    match failure {
        None => trf(
            // TRANSLATORS: %1 is e.g. “the icon set “X””, %2 is “Your current icons stay.”
            "⚠️ The theme comes with %1, which isn't installed. %2 To install it, press “Apply” on the theme again.",
            &[&dep_label(role, wanted), &what],
        ),
        Some(why) => trf(
            // TRANSLATORS: %1 is e.g. “the icon set “X””, %2 is the reason, %3 is “Your current icons stay.”
            "⚠️ Couldn't install %1 that the theme comes with: %2. %3",
            &[&dep_label(role, wanted), &why, &what],
        ),
    }
}

/// Result line: the set is installed, but applying it failed.
pub fn dep_apply_failed_line(role: DepRole, name: &str) -> String {
    trf("⚠️ Couldn't apply %1 that the theme comes with.", &[&dep_label(role, name)])
}

// ============================================================================
// Installing a theme from the catalog — resolving the package name (pure part)
// ============================================================================

/// Port of the package resolution from `install_popular_theme`. Returns
/// (pkg_name, aur_name) for the given `pkg_family` (lowercase). The fallback to
/// `pkg_debian` for non-arch families is kept.
pub fn resolve_install(meta: &Value, pkg_family: &str) -> (String, String) {
    let family = pkg_family.to_lowercase();
    let key = match family.as_str() {
        "arch" => "pkg_arch",
        "debian" => "pkg_debian",
        "fedora" => "pkg_fedora",
        "suse" => "pkg_suse",
        "gentoo" => "pkg_gentoo",
        "alpine" => "pkg_alpine",
        "void" => "pkg_void",
        "solus" => "pkg_solus",
        "clear" => "pkg_clear",
        _ => "",
    };
    let meta_str = |k: &str| meta.get(k).and_then(Value::as_str).unwrap_or("").to_string();

    let mut pkg_name = if key.is_empty() { String::new() } else { meta_str(key) };
    // Fallback: pkg_debian for non-arch (Fedora/SUSE often match).
    if pkg_name.is_empty() && family != "arch" {
        pkg_name = meta_str("pkg_debian");
    }
    let aur_name = meta_str("aur");
    (pkg_name, aur_name)
}

/// argv of an AUR install — port of `[aur_helper, "-S", "--needed", "--noconfirm", aur]`.
pub fn aur_install_cmd(aur_helper: &str, aur_name: &str) -> Cmd {
    // One command for all of Rexon (privileges via the polkit window, answers
    // to the helper's questions) — `aur_install::install_argv`.
    Cmd {
        argv: crate::aur_install::install_argv(aur_helper, &[aur_name.to_string()], false),
        timeout: None,
    }
}

// ============================================================================
// Current themes (for display) — built from ready source values
// ============================================================================

/// Ready source values of `get_current_themes` (the executor fills only its
/// DE's branch). gsettings/xfconf values are already without quotes/spaces
/// (like `r.stdout.strip().strip("'\"")`); None = the key was not obtained.
#[derive(Default)]
pub struct CurrentSources<'a> {
    // gsettings branch (roles gtk/icons/cursor/font).
    pub gs_gtk: Option<&'a str>,
    pub gs_icons: Option<&'a str>,
    pub gs_cursor: Option<&'a str>,
    pub gs_font: Option<&'a str>,
    // xfce (xfconf-query).
    pub xf_gtk: Option<&'a str>,
    pub xf_icons: Option<&'a str>,
    pub xf_cursor: Option<&'a str>,
    pub xf_font: Option<&'a str>,
    // kde.
    pub kde_widget_style: Option<&'a str>,
    pub kde_icons: Option<&'a str>,
    pub kde_cursor_mouse: Option<&'a str>,
    pub kde_kcminput_cursor: &'a str,
    pub xcursor_env: &'a str,
    pub kde_font: Option<&'a str>,
    /// `_system_font_fallback()` — the already computed default font.
    pub font_fallback: &'a str,
}

/// Port of `get_current_themes`: build `{gtk, icons, cursor, font}` from ready
/// source values. Branching by DE — as in the original.
pub fn get_current_themes(de: &str, src: &CurrentSources) -> Map<String, Value> {
    let mut result = Map::new();
    let put = |m: &mut Map<String, Value>, k: &str, v: &str| {
        m.insert(k.to_string(), Value::from(v));
    };
    match resolve_schemas(de) {
        Schemas::Gsettings(list) => {
            for (role, _, _) in list {
                let v = match *role {
                    "gtk" => src.gs_gtk,
                    "icons" => src.gs_icons,
                    "cursor" => src.gs_cursor,
                    "font" => src.gs_font,
                    _ => None,
                };
                put(&mut result, role, v.unwrap_or("—"));
            }
            if result.get("font").and_then(Value::as_str) == Some("—") {
                put(&mut result, "font", src.font_fallback);
            }
        }
        Schemas::NonGsettings if de == "xfce" => {
            put(&mut result, "gtk", src.xf_gtk.unwrap_or("—"));
            put(&mut result, "icons", src.xf_icons.unwrap_or("—"));
            put(&mut result, "cursor", src.xf_cursor.unwrap_or("—"));
            put(&mut result, "font", src.xf_font.unwrap_or(src.font_fallback));
        }
        Schemas::NonGsettings if de == "kde" => {
            put(&mut result, "gtk", src.kde_widget_style.unwrap_or("—"));
            put(&mut result, "icons", src.kde_icons.unwrap_or("—"));
            // cursor: Mouse/cursorTheme → kcminputrc → $XCURSOR_THEME → breeze_cursors
            // (Python `a or b or c or "breeze_cursors"` semantics — an empty string
            // counts as false and moves on along the chain).
            let cursor = match src.kde_cursor_mouse {
                Some(s) if !s.is_empty() => s,
                _ if !src.kde_kcminput_cursor.is_empty() => src.kde_kcminput_cursor,
                _ if !src.xcursor_env.is_empty() => src.xcursor_env,
                _ => "breeze_cursors",
            };
            put(&mut result, "cursor", cursor);
            let font = match src.kde_font {
                Some(raw) if !raw.is_empty() => kde_font_to_display(raw),
                _ => src.font_fallback.to_string(),
            };
            put(&mut result, "font", &font);
        }
        Schemas::NonGsettings => {}
    }
    result
}

/// Port of `_kde_kcminput_cursor`: `cursorTheme=` from the `[Mouse]` section of
/// `kcminputrc`. `content` is the contents (None = no file) → empty.
pub fn kde_kcminput_cursor(content: Option<&str>) -> String {
    let Some(text) = content else {
        return String::new();
    };
    let mut section = String::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            section = line[1..line.len() - 1].to_string();
        } else if section == "Mouse" {
            if let Some(rest) = line.strip_prefix("cursorTheme=") {
                return rest.trim().to_string();
            }
        }
    }
    String::new()
}

/// Port of `_kde_font_to_display`: QFont `"Family,Size,-1,5,..."` → `"Family Size"`.
/// Empty → empty; no comma → as is; an unparsable size → 10.
pub fn kde_font_to_display(raw: &str) -> String {
    if raw.is_empty() {
        return String::new();
    }
    if !raw.contains(',') {
        return raw.to_string();
    }
    let parts: Vec<&str> = raw.split(',').collect();
    let family = parts[0].trim();
    // int(float(parts[1])) if len>1 else 10; ValueError → 10.
    let size = parts
        .get(1)
        .and_then(|s| s.trim().parse::<f64>().ok())
        .map(|f| f as i64)
        .unwrap_or(10);
    format!("{family} {size}")
}

/// Port of `_system_font_fallback`: `gtk-font-name=` from gtk-4.0, then gtk-3.0
/// `settings.ini`; otherwise the `fc-match` output; otherwise `"Sans 10"`.
/// `gtk4_ini` / `gtk3_ini` are the file contents (None = none); `fc_match` is
/// stdout (None/empty = unavailable).
pub fn system_font_fallback(
    gtk4_ini: Option<&str>,
    gtk3_ini: Option<&str>,
    fc_match: Option<&str>,
) -> String {
    for ini in [gtk4_ini, gtk3_ini] {
        let Some(text) = ini else { continue };
        for line in text.lines() {
            let s = line.trim();
            if let Some(rest) = s.strip_prefix("gtk-font-name=") {
                let val = rest.trim();
                if !val.is_empty() {
                    return val.to_string();
                }
            }
        }
    }
    if let Some(out) = fc_match {
        let out = out.trim();
        if !out.is_empty() {
            return out.to_string();
        }
    }
    "Sans 10".to_string()
}

// ============================================================================
// Cursor sanitizer — port of gtk_sanitizer (pre-flight of apply_cursor_theme)
// ============================================================================

/// Unsafe cursor themes — port of `UNSAFE_CURSOR_THEMES` (GTK4 4.22 SIGSEGV:
/// no `cursor.theme`, Inherits into an icon theme without `cursors/`).
pub const UNSAFE_CURSOR_THEMES: &[&str] = &["Adwaita", "AdwaitaLegacy"];

/// Port of `_is_cursor_theme_chain_safe`: the Inherits chain doesn't lead GTK4
/// into recursion. `theme_dir` is the theme directory; `parent_has_cursors` —
/// whether the parent has `cursors/` (checked by the executor in
/// /usr/share/icons and ~/.local/share/icons). Safe if: there is
/// `cursor.theme`; no `index.theme`; no `Inherits=`; or at least one
/// non-`hicolor` parent has `cursors/`.
pub fn is_cursor_theme_chain_safe<F>(
    fs: &FsSnapshot,
    theme_dir: &str,
    parent_has_cursors: F,
) -> bool
where
    F: Fn(&str) -> bool,
{
    if fs.is_file(&format!("{theme_dir}/cursor.theme")) {
        return true;
    }
    let idx = format!("{theme_dir}/index.theme");
    if !fs.is_file(&idx) {
        return true; // self-contained, no inheritance
    }
    let Some(content) = fs.read_text(&idx) else {
        return true; // OSError on read → safe (as in the original)
    };
    let mut parents: Vec<String> = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("Inherits=") {
            parents = rest.split(',').map(|p| p.trim().to_string()).collect();
            break;
        }
    }
    if parents.is_empty() {
        return true;
    }
    for parent in &parents {
        if parent == "hicolor" {
            continue; // a terminator for icons, a dead end for cursors
        }
        if parent_has_cursors(parent) {
            return true;
        }
    }
    false
}

/// Port of the `apply_cursor_theme` pre-flight: refuse an unsafe theme. `theme`
/// in [`UNSAFE_CURSOR_THEMES`] → false; otherwise for the first directory (of
/// `/usr/share/icons`, `~/.local/share/icons`) that has `<theme>/cursors/`,
/// check [`is_cursor_theme_chain_safe`]. `parent_has_cursors` is the same
/// callback. Returns `true` = may be applied.
pub fn cursor_preflight_ok<F>(
    fs: &FsSnapshot,
    theme: &str,
    home: &str,
    parent_has_cursors: F,
) -> bool
where
    F: Fn(&str) -> bool,
{
    if UNSAFE_CURSOR_THEMES.contains(&theme) {
        return false;
    }
    for base in ["/usr/share/icons".to_string(), format!("{home}/.local/share/icons")] {
        let d = format!("{base}/{theme}");
        if fs.is_dir(&d) && fs.is_dir(&format!("{d}/cursors")) {
            if !is_cursor_theme_chain_safe(fs, &d, &parent_has_cursors) {
                return false;
            }
            break;
        }
    }
    true
}

// ============================================================================
// Apply history — port of _add_history / _save_history / get_recent
// ============================================================================

/// Port of the `_add_history` record: `{"kind","theme","de","ts"}`. `ts`
/// (isoformat timespec=seconds) is non-deterministic → passed as a parameter.
pub fn history_entry(kind: &str, theme: &str, de: &str, ts: &str) -> Value {
    let mut m = Map::new();
    m.insert("kind".into(), Value::from(kind));
    m.insert("theme".into(), Value::from(theme));
    m.insert("de".into(), Value::from(de));
    m.insert("ts".into(), Value::from(ts));
    Value::Object(m)
}

/// Port of `_add_history`: insert at the beginning, truncate to 50.
pub fn push_history(history: &[Value], entry: Value) -> Vec<Value> {
    let mut out = Vec::with_capacity(history.len() + 1);
    out.push(entry);
    out.extend(history.iter().cloned());
    out.truncate(50);
    out
}

/// `json.dump(history, indent=2, ensure_ascii=False)` — port of `_save_history`.
pub fn serialize_history(history: &[Value]) -> String {
    serde_json::to_string_pretty(&Value::Array(history.to_vec())).expect("Value сериализуется")
}

/// Port of `get_recent`: `history[:n]`.
pub fn get_recent(history: &[Value], n: usize) -> Vec<Value> {
    history.iter().take(n).cloned().collect()
}

// ══════════════════════════════════════════════════════════════════════════
//  Installing a theme from an archive (port of `install_theme_from_archive`)
// ══════════════════════════════════════════════════════════════════════════

/// Where to unpack a theme from an archive: `base` is the root ("~/.themes" or
/// "~/.local/share/icons"), `dest` is the final theme directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveDest {
    pub base: String,
    pub dest: String,
}

/// Port of choosing the install location in `install_theme_from_archive`:
/// `kind == "gtk"` → `~/.themes` (outside XDG — GTK/Qt read it there),
/// otherwise the icons directory in the XDG data root.
pub fn archive_install_dest(
    kind: &str,
    name: &str,
    home: &str,
    xdg_data_home: Option<&str>,
) -> ArchiveDest {
    let base = if kind == "gtk" {
        crate::utils::expand_user_xdg("~/.themes", home, None, xdg_data_home)
    } else {
        crate::utils::expand_user_xdg(
            "~/.local/share/icons",
            home,
            None,
            xdg_data_home,
        )
    };
    let dest = format!("{}/{name}", base.trim_end_matches('/'));
    ArchiveDest { base, dest }
}

/// The theme folder inside the unpacked archive.
///
/// ⚠ Rule #16: the original took `[e for e in os.scandir(tmp) if e.is_dir()][0]`
/// — the `scandir` order is set by the file system, so for an archive with
/// several folders a RANDOM one was installed. The port takes the first one
/// alphabetically: the same result for a normal archive (one folder) and a
/// predictable one for the rest.
pub fn pick_archive_dir(names: &[String]) -> Option<String> {
    names.iter().min().cloned()
}

/// `install_theme_from_archive`: the archive has no folders at all.
pub fn msg_archive_no_dirs() -> String {
    crate::i18n::tr("The archive contains no folders")
}
/// Start of the failed unpacking message (followed by the exception text).
pub fn msg_extract_failed_prefix() -> String {
    format!("{}: ", crate::i18n::tr("Unpacking error"))
}
/// `install_popular_theme`: the theme is not in the built-in catalog.
pub fn msg_theme_not_in_catalog_prefix() -> String {
    format!("{}: ", crate::i18n::tr("Theme not found in the catalog"))
}
/// `install_popular_theme`: installed from the distribution repository.
pub fn msg_installed_from_repo_prefix() -> String {
    format!("{}: ", crate::i18n::tr("Theme installed from the repository"))
}
/// `install_popular_theme`: installed from the AUR.
pub fn msg_installed_from_aur_prefix() -> String {
    format!("{}: ", crate::i18n::tr("Theme installed from AUR"))
}
/// `install_popular_theme`: there is no package for this distribution.
pub fn msg_no_package_prefix() -> String {
    format!("{}: ", crate::i18n::tr("No package for this distribution"))
}
/// `install_popular_theme`: the install failed, followed by what to try by hand.
pub fn msg_install_failed_prefix() -> String {
    format!("{}: ", crate::i18n::tr("Couldn't install. Try it manually"))
}

// ══════════════════════════════════════════════════════════════════════════
//  Reading kdeglobals (port of `_kde_get`)
// ══════════════════════════════════════════════════════════════════════════

/// argv of `kreadconfig6` for a kdeglobals key — the path through KDE, to pick up
/// the system defaults and the active Look-and-Feel (not only the file in ~/.config).
pub fn kde_read_cmd(section: &str, key: &str) -> Cmd {
    Cmd {
        argv: vec![
            "kreadconfig6".into(),
            "--file".into(),
            "kdeglobals".into(),
            "--group".into(),
            section.to_string(),
            "--key".into(),
            key.to_string(),
            "--default".into(),
            String::new(),
        ],
        timeout: None,
    }
}

/// Fallback of `_kde_get`: a key value straight from the kdeglobals text.
///
/// Mirrors `configparser(strict=False)` on KDE data: the last occurrence of a
/// key in a section wins (duplicates in kdeglobals are normal), the value is
/// trimmed at the edges, `#`/`;` comments outside values are skipped,
/// `%(...)s` interpolation is NOT performed (like `configparser.get` without
/// vars — it wouldn't have worked for KDE values like `%d` anyway).
pub fn kdeglobals_value(content: &str, section: &str, key: &str) -> Option<String> {
    let mut cur = String::new();
    let mut found: Option<String> = None;
    for raw in content.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            cur = line[1..line.len() - 1].to_string();
            continue;
        }
        if cur != section {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            // configparser compares keys case-insensitively (optionxform).
            if k.trim().eq_ignore_ascii_case(key) {
                found = Some(v.trim().to_string());
            }
        }
    }
    found
}

// ============================================================================
// «Configure…» of an application style — as in the KDE "Application Style" module
// ============================================================================
//
// KDE (plasma-workspace 6.8.2, `kcms/style`): a style has its own settings
// when its description `kstyle/themes/<style>.themerc` has
// `[Misc] ConfigPage=<plugin>` (Breeze — `kstyle_config/breezestyleconfig`,
// Oxygen — `kstyle_config/kstyle_oxygen_config`); the module loads this plugin
// (`allocate_kstyle_config`) and shows it in a "Configure …" window. Only such
// styles have the button. The same plugin without the module is shown by the
// styles' own standalone programs — `breeze-settings6` and
// `oxygen-settings6`; Rexon calls them, they work outside KDE too. A plugin
// without its own program is opened from the module itself
// (`kcmshell6 kcm_style`). Kvantum is its own style without a `.themerc`: its
// settings are `kvantummanager`; for `qt6ct-style`/`qt5ct-style` — qt6ct/qt5ct
// themselves.

/// Programs that show the style settings plugin without the KDE module.
const STYLE_CONFIG_PROGRAMS: &[(&str, &str)] = &[
    ("kstyle_config/breezestyleconfig", "breeze-settings6"),
    ("kstyle_config/kstyle_oxygen_config", "oxygen-settings6"),
];

/// What opens the style settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StyleConfigButton {
    /// Whether to show the button (only for application styles and Kvantum themes).
    pub visible: bool,
    /// Whether it can be pressed: there is something to open it with.
    pub enabled: bool,
    pub text: String,
    /// What will open — or an honest reason why there is nothing to open it with.
    pub tooltip: String,
    /// Command (empty — can't be pressed).
    pub argv: Vec<String>,
}

/// `ConfigPage` of styles from the `.themerc` descriptions: the key is the style
/// name (`[KDE] WidgetStyle`) in lowercase.
pub fn style_config_pages(themercs: &[String]) -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    for text in themercs {
        let style = ini_value_ci(text, "KDE", "WidgetStyle");
        let page = ini_value_ci(text, "Misc", "ConfigPage");
        if let (Some(style), Some(page)) = (style, page) {
            if !style.is_empty() && !page.is_empty() {
                out.insert(style.to_lowercase(), page);
            }
        }
    }
    out
}

/// `kstyle/themes` directories per XDG (the user's first, as in QStandardPaths).
pub fn style_themerc_dirs(home: &str, xdg_data_home: Option<&str>, xdg_data_dirs: Option<&str>) -> Vec<String> {
    let mut out = Vec::new();
    let data_home = xdg_data_home
        .filter(|s| !s.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("{home}/.local/share"));
    out.push(format!("{data_home}/kstyle/themes"));
    let dirs = xdg_data_dirs.filter(|s| !s.trim().is_empty()).unwrap_or("/usr/local/share:/usr/share");
    for d in dirs.split(':').filter(|d| !d.is_empty()) {
        out.push(format!("{}/kstyle/themes", d.trim_end_matches('/')));
    }
    out
}

/// The «Configure…» button of a tile in the «Application Style» section. `kind`
/// is the tile kind (`widgetstyle` — a Qt style, `kvantum` — a Kvantum theme),
/// `name` — the style or theme name, `pages` — [`style_config_pages`], `has` —
/// whether the program is in PATH.
pub fn style_config_button(
    kind: &str,
    name: &str,
    pages: &std::collections::HashMap<String, String>,
    has: &dyn Fn(&str) -> bool,
) -> StyleConfigButton {
    let hidden = StyleConfigButton {
        visible: false,
        enabled: false,
        text: String::new(),
        tooltip: String::new(),
        argv: Vec::new(),
    };
    if kind != "widgetstyle" && kind != "kvantum" {
        return hidden;
    }
    let low = name.trim().to_lowercase();
    let open = |argv: Vec<String>, what: String| StyleConfigButton {
        visible: true,
        enabled: true,
        text: tr("Configure…"),
        tooltip: trf("Opens %1", &[&what]),
        argv,
    };
    let missing = |reason: String| StyleConfigButton {
        visible: true,
        enabled: false,
        text: tr("Configure…"),
        tooltip: reason,
        argv: Vec::new(),
    };
    // Kvantum: both the engine itself (`kvantum`, `kvantum-dark`) and its themes.
    if kind == "kvantum" || low.starts_with("kvantum") {
        return if has("kvantummanager") {
            open(vec!["kvantummanager".into()], tr("Kvantum Manager"))
        } else {
            missing(tr("Kvantum settings are made in Kvantum Manager, and it isn't installed (package kvantum)."))
        };
    }
    for (style, program, title) in [("qt6ct-style", "qt6ct", "qt6ct"), ("qt5ct-style", "qt5ct", "qt5ct")] {
        if low == style {
            return if has(program) {
                open(vec![program.into()], title.to_string())
            } else {
                missing(trf("This style is set up in %1, and it isn't installed.", &[&title]))
            };
        }
    }
    let Some(page) = pages.get(&low) else {
        return missing(tr("This style has no settings of its own."));
    };
    if let Some((_, program)) = STYLE_CONFIG_PROGRAMS.iter().find(|(p, _)| p == page) {
        if has(program) {
            return open(vec![(*program).into()], trf("settings of the %1 style", &[&name.trim()]));
        }
    }
    if has("kcmshell6") {
        return open(
            vec!["kcmshell6".into(), "kcm_style".into()],
            tr("the KDE “Application Style” module — its “Configure…” button is next to the style"),
        );
    }
    missing(tr("The settings of this style open only from KDE System Settings, and they aren't installed."))
}

/// [`style_config_button`] on the live system: style descriptions from disk,
/// programs by PATH.
pub fn style_config_button_live(kind: &str, name: &str) -> StyleConfigButton {
    let home = std::env::var("HOME").unwrap_or_default();
    let dirs = style_themerc_dirs(
        &home,
        std::env::var("XDG_DATA_HOME").ok().as_deref(),
        std::env::var("XDG_DATA_DIRS").ok().as_deref(),
    );
    let mut texts = Vec::new();
    for dir in dirs {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for entry in rd.flatten() {
            let p = entry.path();
            if p.extension().is_some_and(|e| e == "themerc") {
                if let Ok(t) = std::fs::read_to_string(&p) {
                    texts.push(t);
                }
            }
        }
    }
    let pages = style_config_pages(&texts);
    let path = std::env::var("PATH").ok();
    style_config_button(kind, name, &pages, &|p| crate::utils::which(p, path.as_deref()).is_some())
}

/// INI key value, case-insensitive in group and key names.
fn ini_value_ci(text: &str, group: &str, key: &str) -> Option<String> {
    let mut here = false;
    for line in text.lines() {
        let s = line.trim();
        if s.starts_with('[') && s.ends_with(']') {
            here = s[1..s.len() - 1].eq_ignore_ascii_case(group);
            continue;
        }
        if here {
            if let Some((k, v)) = s.split_once('=') {
                if k.trim().eq_ignore_ascii_case(key) {
                    return Some(v.trim().to_string());
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod deps_tests {
    use super::*;

    /// Decision 3a: one question about the theme icons; a remembered answer, icons
    /// already in place and a set that exists nowhere give no question; for an
    /// installed set — «Turn on», for a missing one — «Download and turn on».
    #[test]
    fn theme_icons_are_asked_once_and_remembered() {
        let icons = |state: DepState| vec![DepItem { role: DepRole::Icons, wanted: "Windows XP".into(), state }];
        let pkg = icons(DepState::Installable("xp-icons".into()));
        let q = icons_question("Luna", &pkg, "breeze", None).expect("спросить");
        assert_eq!(q.yes, tr("Download and turn on"));
        assert_eq!(q.no, tr("Keep mine"));
        assert!(q.text.contains("Luna") && q.text.contains("Windows XP"), "{}", q.text);
        // The answer is remembered — don't ask, whatever it was.
        assert_eq!(icons_question("Luna", &pkg, "breeze", Some(true)), None);
        assert_eq!(icons_question("Luna", &pkg, "breeze", Some(false)), None);
        // The set is already installed — the same question, but «Turn on».
        let here = icons(DepState::Installed("Windows XP".into()));
        assert_eq!(icons_question("Luna", &here, "breeze", None).expect("спросить").yes, tr("Turn on"));
        // It is already on — nothing to ask.
        assert_eq!(icons_question("Luna", &here, "Windows XP", None), None);
        // Exists nowhere — the result will tell about it, not a question.
        assert_eq!(icons_question("Luna", &icons(DepState::Missing), "breeze", None), None);
        // The theme doesn't ask for icons.
        let cursor = vec![DepItem { role: DepRole::Cursor, wanted: "DMZ-White".into(), state: DepState::Missing }];
        assert_eq!(icons_question("Luna", &cursor, "breeze", None), None);

        // Memory by theme name, other answers are not lost.
        let v = icons_choice_with(None, "Luna", false);
        assert_eq!(icons_choice_of(Some(&v), "Luna"), Some(false));
        let v = icons_choice_with(Some(&v), "Royale", true);
        assert_eq!(icons_choice_of(Some(&v), "Luna"), Some(false));
        assert_eq!(icons_choice_of(Some(&v), "Royale"), Some(true));
        assert_eq!(icons_choice_of(Some(&v), "Other"), None);
        assert_eq!(icons_choice_of(None, "Luna"), None);
    }

    /// The WhiteSur home archive carries only WhiteSur, -dark and -light: a color
    /// variant can't be got from it — so it isn't downloaded (otherwise three
    /// needless sets in ~/.local/share/icons and "didn't bring it" on every
    /// «Apply»).
    #[test]
    fn home_archive_is_offered_only_for_names_it_brings() {
        for name in ["WhiteSur", "WhiteSur-Dark", "whitesur-light"] {
            assert!(dep_home_source(DepRole::Icons, name).is_some(), "{name}");
        }
        for name in ["WhiteSur-purple-dark", "WhiteSur-nord"] {
            assert_eq!(dep_home_source(DepRole::Icons, name), None, "{name}");
        }
        // The package still knows the variants meanwhile.
        assert!(KNOWN_SETS.iter().any(|s| known_set_matches(s, DepRole::Icons, "WhiteSur-nord")));
    }

    /// A set name from someone else's index.theme is a single directory name, not a path.
    #[test]
    fn theme_deps_ignore_path_like_names() {
        let d = theme_deps("[Desktop Entry]\nIconTheme=WhiteSur-/../../x\nCursorTheme=../c\n");
        assert_eq!(d, ThemeDeps::default());
    }
    use std::collections::{HashMap, HashSet};

    /// A fake disk: directories and files with text.
    struct Disk {
        dirs: HashSet<String>,
        files: HashMap<String, String>,
    }

    impl Disk {
        fn new(dirs: &[&str], files: &[(&str, &str)]) -> Self {
            Disk {
                dirs: dirs.iter().map(|d| d.to_string()).collect(),
                files: files.iter().map(|(p, t)| (p.to_string(), t.to_string())).collect(),
            }
        }
        fn find(&self, role: DepRole, wanted: &str, roots: &[String]) -> Option<String> {
            let list = |root: &str| -> Vec<String> {
                let prefix = format!("{root}/");
                self.dirs
                    .iter()
                    .filter_map(|d| d.strip_prefix(&prefix))
                    .filter(|rest| !rest.contains('/'))
                    .map(str::to_string)
                    .collect()
            };
            find_installed_dep(
                role,
                wanted,
                roots,
                &|p| self.dirs.contains(p),
                &list,
                &|p| self.files.get(p).cloned(),
            )
        }
    }

    const LUNA: &str = "[Desktop Entry]\nType=X-GNOME-Metatheme\nName=Windows XP Luna\n\n\
        [X-GNOME-Metatheme]\nGtkTheme=Windows XP Luna\nMetacityTheme=Windows XP Luna\n\
        IconTheme=Windows XP\nCursorTheme=DMZ-White\n";

    fn roots() -> Vec<String> {
        dep_search_roots("/h", "/h/.local/share", "")
    }

    #[test]
    fn deps_read_from_metatheme_and_desktop_entry() {
        assert_eq!(
            theme_deps(LUNA),
            ThemeDeps { icons: Some("Windows XP".into()), cursor: Some("DMZ-White".into()) }
        );
        // Keys in [Desktop Entry] (some themes write them so) — also a request.
        let de = theme_deps("[Desktop Entry]\nName=X\nIconTheme=Papirus-Dark\n");
        assert_eq!(de.icons.as_deref(), Some("Papirus-Dark"));
        assert_eq!(de.cursor, None);
        // An empty value, hicolor (our .msstyles import) and a foreign section — not a request.
        let none = theme_deps("[X-GNOME-Metatheme]\nIconTheme=hicolor\nCursorTheme=\n[Other]\nIconTheme=Papirus\n");
        assert_eq!(none, ThemeDeps::default());
    }

    #[test]
    fn search_roots_follow_xdg() {
        assert_eq!(
            dep_search_roots("/h", "/h/.local/share", "/opt/share:/usr/share/"),
            vec!["/h/.icons", "/h/.local/share/icons", "/opt/share/icons", "/usr/share/icons"]
        );
        // Without XDG_DATA_DIRS — the value per the spec.
        assert_eq!(
            gtk_theme_search_roots("/h", "/d", ""),
            vec!["/h/.themes", "/d/themes", "/usr/local/share/themes", "/usr/share/themes"]
        );
    }

    #[test]
    fn dmz_white_is_vanilla_dmz() {
        // The theme asks for "DMZ-White", the Arch package installed "Vanilla-DMZ".
        let disk = Disk::new(&["/usr/share/icons/Vanilla-DMZ", "/usr/share/icons/Vanilla-DMZ/cursors"], &[]);
        assert_eq!(disk.find(DepRole::Cursor, "DMZ-White", &roots()).as_deref(), Some("Vanilla-DMZ"));
        // Black DMZ is a different set: the white one can't stand in for it.
        assert_eq!(disk.find(DepRole::Cursor, "DMZ-Black", &roots()), None);
        // The exact name beats the alias.
        let both = Disk::new(
            &[
                "/usr/share/icons/Vanilla-DMZ",
                "/usr/share/icons/Vanilla-DMZ/cursors",
                "/h/.icons/DMZ-White",
                "/h/.icons/DMZ-White/cursors",
            ],
            &[],
        );
        assert_eq!(both.find(DepRole::Cursor, "DMZ-White", &roots()).as_deref(), Some("DMZ-White"));
    }

    #[test]
    fn icons_need_directories_and_case_is_forgiven() {
        let disk = Disk::new(
            &["/h/.local/share/icons/WhiteSur-dark", "/h/.icons/Cursorish", "/h/.icons/Cursorish/cursors"],
            &[
                ("/h/.local/share/icons/WhiteSur-dark/index.theme", "[Icon Theme]\nName=WhiteSur-dark\nDirectories=48x48/apps\n"),
                // A cursor theme with an index.theme without Directories= is not an icon set.
                ("/h/.icons/Cursorish/index.theme", "[Icon Theme]\nName=Cursorish\n"),
            ],
        );
        assert_eq!(disk.find(DepRole::Icons, "WhiteSur-Dark", &roots()).as_deref(), Some("WhiteSur-dark"));
        assert_eq!(disk.find(DepRole::Icons, "Cursorish", &roots()), None);
        assert_eq!(disk.find(DepRole::Cursor, "Cursorish", &roots()).as_deref(), Some("Cursorish"));
    }

    #[test]
    fn plan_installed_installable_missing() {
        let disk = Disk::new(
            &["/h/.local/share/icons/Windows XP"],
            &[("/h/.local/share/icons/Windows XP/index.theme", "[Icon Theme]\nDirectories=16x16/apps\n")],
        );
        let find = |role: DepRole, wanted: &str| disk.find(role, wanted, &roots());
        let plan = plan_theme_deps(&theme_deps(LUNA), "arch", &find);
        assert_eq!(
            plan,
            vec![
                DepItem { role: DepRole::Icons, wanted: "Windows XP".into(), state: DepState::Installed("Windows XP".into()) },
                DepItem {
                    role: DepRole::Cursor,
                    wanted: "DMZ-White".into(),
                    state: DepState::Installable("xcursor-vanilla-dmz".into()),
                },
            ]
        );
        // Nothing installed, a family without a package — an honest "no".
        let empty = Disk::new(&[], &[]);
        let find = |role: DepRole, wanted: &str| empty.find(role, wanted, &roots());
        let plan = plan_theme_deps(&theme_deps(LUNA), "gentoo", &find);
        assert!(plan.iter().all(|i| i.state == DepState::Missing));
        // Only what can be installed gets installed by itself.
        assert!(deps_to_prepare(&plan, true).iter().all(|i| dep_home_source(i.role, &i.wanted).is_some()));
    }

    /// «Apply» of a GTK theme: the cursor is installed by itself, without a
    /// question; the icons — per the «Download and turn on» answer; if something
    /// didn't install, the theme is not applied.
    #[test]
    fn theme_deps_install_without_question_all_or_nothing() {
        let cursor = DepItem { role: DepRole::Cursor, wanted: "DMZ-White".into(), state: DepState::Installable("xcursor-vanilla-dmz".into()) };
        let icons = DepItem { role: DepRole::Icons, wanted: "Mint-Y".into(), state: DepState::Installable("mint-y-icons".into()) };
        let have = DepItem { role: DepRole::Icons, wanted: "Adwaita".into(), state: DepState::Installed("Adwaita".into()) };
        let nowhere = DepItem { role: DepRole::Cursor, wanted: "No-Such-Cursor".into(), state: DepState::Missing };
        let items = vec![cursor.clone(), icons.clone(), have, nowhere];
        assert_eq!(deps_to_prepare(&items, false), vec![cursor.clone()]);
        assert_eq!(deps_to_prepare(&items, true), vec![cursor.clone(), icons.clone()]);
        // The password was not confirmed — advice to retry; the reason is not repeated.
        let pw = crate::appearance_installer::password_not_confirmed();
        let text = deps_refusal("Luna", &[cursor.clone()], &pw);
        assert!(text.contains("Luna") && text.contains("DMZ-White") && !text.contains(&pw), "{text}");
        // The package is not in the official repositories (only the AUR) — the theme
        // is not applied, and it is said what is missing.
        let text = deps_refusal("Luna", &[icons], "not in the official repositories: mint-y-icons");
        assert!(text.contains("Mint-Y") && text.contains("mint-y-icons"), "{text}");
    }

    #[test]
    fn packages_by_family_and_variants() {
        assert_eq!(dep_package(DepRole::Cursor, "DMZ-White", "debian").as_deref(), Some("dmz-cursor-theme"));
        assert_eq!(dep_package(DepRole::Icons, "Mint-Y", "arch").as_deref(), Some("mint-y-icons"));
        assert_eq!(dep_package(DepRole::Icons, "WhiteSur-Dark", "arch").as_deref(), Some("whitesur-icon-theme"));
        assert_eq!(dep_package(DepRole::Cursor, "WhiteSur-cursors", "arch").as_deref(), Some("whitesur-cursor-theme-git"));
        assert_eq!(dep_package(DepRole::Icons, "Papirus-Dark", "fedora").as_deref(), Some("papirus-icon-theme-dark"));
        assert_eq!(dep_package(DepRole::Icons, "Papirus-Dark", "debian").as_deref(), Some("papirus-icon-theme"));
        assert_eq!(dep_package(DepRole::Icons, "Papirus", "fedora").as_deref(), Some("papirus-icon-theme"));
        assert_eq!(dep_package(DepRole::Cursor, "DMZ-White", "fedora"), None);
        assert_eq!(dep_package(DepRole::Cursor, "DMZ-White", "suse").as_deref(), Some("dmz-icon-theme-cursors"));
        assert_eq!(dep_package(DepRole::Icons, "Windows XP", "arch"), None);
        assert_eq!(dep_package(DepRole::Icons, "Mint-Y-Dark", "debian").as_deref(), Some("mint-y-icons"));
        assert_eq!(dep_package(DepRole::Icons, "WhiteSur", "debian"), None);
        // "Papirusoid" is not a Papirus variant.
        assert_eq!(dep_package(DepRole::Icons, "Papirusoid", "arch"), None);
    }
}
