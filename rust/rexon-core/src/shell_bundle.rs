//! The «tiling compositor + shell» bundle (Noctalia, DankMaterialShell):
//! what to install, where and which line to write, how to remove exactly that line.
//!
//! Step 3 of shell support (owner's decision 01.10.2026): the Hyprland,
//! niri and sway cards have a «Shells» block on the right. This module holds
//! side-effect-free decisions; the executor is `system_de_exec` (the same
//! package path and the same password as desktop installation), labels are in `qt_desktops`.
//!
//! Facts (research 01.10.2026, noctalia 5.2, DMS 1.6.2, from sources and
//! packages):
//!   * neither shell brings its own session to the login screen: «compositor
//!     with a shell» is an autostart line in the compositor config;
//!   * Noctalia is in the official Arch repositories (`extra/noctalia`) and Fedora
//!     44+ (`noctalia`); `cachyos/noctalia-shell` is the OLD v4 on
//!     Quickshell, a different product, we do not install it;
//!   * DankMaterialShell is split in Arch: `dms-shell` requires
//!     `dms-shell-compositor`, which only `dms-shell-niri` and
//!     `dms-shell-hyprland` provide. There is no variant for sway — in the sway card DMS
//!     is inactive with an honest hint. On other distributions both shells come
//!     only from third-party repositories (COPR, OBS, PPA, the GURU overlay), and
//!     Rexon does not enable those itself;
//!   * the autostart line is a managed block of the shared helper
//!     `compositor_autostart` (markers, syntaxes, bit-for-bit reversibility);
//!   * DMS installs a `dms.service` unit with `WantedBy=graphical-session.target`:
//!     ⛔ `systemctl --user enable dms` would start it in Plasma too. We do not
//!     touch the unit at all — it is launched by a line in the config of the compositor
//!     where the person chose the shell (for DMS this is the standard «without systemd» path).
//!
//! ⭐ TWO MODES OF ONE COMPOSITOR. The main desktop stays itself:
//! the autostart line sits in the config permanently, but only fires if
//! the marker `<state>/rexon/shells/<compositor>-<shell>` exists. «Switch»
//! on a shell puts the marker and logs out of the session; «Switch» on the
//! desktop itself (the left part of the card) removes this compositor's markers. So
//! the desktop and the shell each have their own entry, although the login screen has one record.

use serde::Serialize;

use crate::compositor_autostart::{self as ca, Refusal, Syntax, TextEdit};
use crate::i18n::trf;
use crate::system::{DesktopShell, PkgFamily};

/// Compositors whose cards have the «Shells» block — the same set as
/// the «Tiling» section (`qt_desktops::TILING_IDS`). Plasma is not included:
/// for Noctalia it is «not first-class», DMS does not support it at all.
pub const SHELL_WMS: &[&str] = &["hyprland", "niri", "sway"];

/// Shells of a desktop card; empty — the card has no «Shells» block.
pub fn shells_for(de_id: &str) -> &'static [DesktopShell] {
    if SHELL_WMS.contains(&de_id) {
        DesktopShell::all()
    } else {
        &[]
    }
}

/// Shell by identifier (`DesktopShell::as_str`).
pub fn shell_by_id(id: &str) -> Option<DesktopShell> {
    DesktopShell::all().iter().copied().find(|s| s.as_str() == id)
}

/// The shell executable — it shows whether the package is installed.
pub fn binary(shell: DesktopShell) -> &'static str {
    match shell {
        DesktopShell::Noctalia => "noctalia",
        DesktopShell::DankMaterialShell => "dms",
    }
}

/// Launch command in the session (from their documentation: `noctalia`, `dms run`).
pub fn launch_command(shell: DesktopShell) -> &'static str {
    match shell {
        DesktopShell::Noctalia => "noctalia",
        DesktopShell::DankMaterialShell => "dms run",
    }
}

/// Why the shell cannot be installed in this card.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Unavailable {
    /// The card is not a tiling compositor.
    NotForThisDesktop,
    /// DMS under sway: repositories ship it only together with niri or
    /// Hyprland.
    NotPackagedForWm,
    /// The official repositories of this distribution have no such shell.
    NotInRepos,
}

/// Explanation for the person: why the shell cannot be installed here. One sentence —
/// it is shown both as the hint on the inactive button and as the executor's refusal.
pub fn unavailable_text(reason: Unavailable, shell: DesktopShell, wm_name: &str) -> String {
    match reason {
        Unavailable::NotForThisDesktop => trf(
            // TRANSLATORS: %1 is a desktop shell (Noctalia…), %2 a desktop.
            "%1 can't be added to %2.",
            &[&shell.label(), &wm_name],
        ),
        Unavailable::NotPackagedForWm => trf(
            // TRANSLATORS: %1 is a desktop shell, %2 a compositor (sway).
            "%1 isn't packaged for %2: your distribution ships it only together with niri or Hyprland.",
            &[&shell.label(), &wm_name],
        ),
        Unavailable::NotInRepos => trf(
            // TRANSLATORS: %1 is a desktop shell (Noctalia, DankMaterialShell).
            "%1 isn't in your distribution's official repositories.",
            &[&shell.label()],
        ),
    }
}

/// The desktop itself is needed first — hint on the inactive «Install» and the
/// executor's refusal.
pub fn install_desktop_first(wm_name: &str) -> String {
    // TRANSLATORS: %1 is a desktop (Hyprland, niri, sway).
    trf("Install %1 first", &[&wm_name])
}

/// Shell packages for this compositor and distribution — in installation order.
pub fn packages(
    shell: DesktopShell,
    wm: &str,
    family: PkgFamily,
) -> Result<Vec<&'static str>, Unavailable> {
    if !SHELL_WMS.contains(&wm) {
        return Err(Unavailable::NotForThisDesktop);
    }
    match shell {
        DesktopShell::Noctalia => match family {
            PkgFamily::Arch | PkgFamily::Fedora => Ok(vec!["noctalia"]),
            _ => Err(Unavailable::NotInRepos),
        },
        DesktopShell::DankMaterialShell => {
            if family != PkgFamily::Arch {
                return Err(Unavailable::NotInRepos);
            }
            match wm {
                "niri" => Ok(vec!["dms-shell", "dms-shell-niri"]),
                "hyprland" => Ok(vec!["dms-shell", "dms-shell-hyprland"]),
                _ => Err(Unavailable::NotPackagedForWm),
            }
        }
    }
}

/// All packages of all shells for all compositors of the family — the package
/// helper's install allowlist (`system_plan::pkg_helper_install_allowlist`).
pub fn all_packages(family: PkgFamily) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for shell in DesktopShell::all() {
        for wm in SHELL_WMS {
            for p in packages(*shell, wm, family).unwrap_or_default() {
                if !out.contains(&p) {
                    out.push(p);
                }
            }
        }
    }
    out
}

/// Packages that «Remove» uninstalls for this compositor.
///
/// `still_used_by` — other compositors whose config keeps the line of the same
/// shell: then the shared packages (`noctalia`, `dms-shell`) stay,
/// and only the compositor-specific one goes (`dms-shell-niri`).
pub fn packages_to_remove(
    shell: DesktopShell,
    wm: &str,
    family: PkgFamily,
    still_used_by: &[&str],
) -> Vec<&'static str> {
    let Ok(own) = packages(shell, wm, family) else {
        return Vec::new();
    };
    let shared: Vec<&'static str> = still_used_by
        .iter()
        .filter(|other| **other != wm)
        .filter_map(|other| packages(shell, other, family).ok())
        .flatten()
        .collect();
    // Dependents first (`dms-shell-niri` holds `dms-shell`): pacman removes
    // them in one transaction, but the order reads more naturally in the log.
    own.into_iter()
        .rev()
        .filter(|p| !shared.contains(p))
        .collect()
}

// ============================================================================
// Where the compositor config lives and what to write into it
// ============================================================================
//
// ⭐ The managed block is the product's SHARED helper
// (`crate::compositor_autostart`: `upsert_text`/`remove_text`/`launch_line`):
// the same markers, the same syntax parsing and the same bit-for-bit reversibility as
// Rexon's own autostart lines. There is no copy of it here. The file creation
// policy is its too: sway gets its own `config` with an `include` of the system one, while
// Hyprland and niri without a personal config are left alone — on first login they
// put their own, and a file with just our line would leave the person without key bindings.

/// The compositor's personal config: path, syntax, and what to seed it with if missing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigFile {
    pub path: String,
    pub syntax: Syntax,
    /// No file: `Some(preamble)` — may be created, `None` — may not.
    pub create_with: Option<Vec<String>>,
    pub exists: bool,
}

/// Files where the compositor may hold our block, in the compositor's own
/// search order (the same as `compositor_autostart::resolve_target`).
pub fn config_files(wm: &str, home: &str, xdg_config_home: Option<&str>) -> Vec<(String, Syntax)> {
    let cfg = crate::utils::config_home(xdg_config_home, home);
    let home = home.trim_end_matches('/');
    match wm {
        // sway also reads the i3 config: without these paths we would create our own
        // `sway/config`, and it would shadow the person's config.
        "sway" => vec![
            (format!("{home}/.sway/config"), Syntax::Sway),
            (format!("{cfg}/sway/config"), Syntax::Sway),
            (format!("{home}/.i3/config"), Syntax::Sway),
            (format!("{cfg}/i3/config"), Syntax::Sway),
        ],
        "niri" => vec![(format!("{cfg}/niri/config.kdl"), Syntax::NiriKdl)],
        // Hyprland 0.55+: hyprland.lua if present, otherwise hyprland.conf.
        "hyprland" => vec![
            (format!("{cfg}/hypr/hyprland.lua"), Syntax::HyprLua),
            (format!("{cfg}/hypr/hyprland.conf"), Syntax::HyprConf),
        ],
        _ => Vec::new(),
    }
}

/// The compositor's main config, by probing with `exists`.
pub fn config_target(
    wm: &str,
    home: &str,
    xdg_config_home: Option<&str>,
    exists: &dyn Fn(&str) -> bool,
) -> Option<ConfigFile> {
    let files = config_files(wm, home, xdg_config_home);
    if let Some((path, syntax)) = files.iter().find(|(p, _)| exists(p)) {
        return Some(ConfigFile {
            path: path.clone(),
            syntax: *syntax,
            create_with: None,
            exists: true,
        });
    }
    let (path, syntax) = match wm {
        "sway" => files.get(1)?.clone(),
        _ => files.first()?.clone(),
    };
    let create_with = match wm {
        // Our own file shadows the system one — so first of all we include it.
        "sway" if exists("/etc/sway/config") => {
            Some(vec![format!("include {}", crate::themes::sway_path("/etc/sway/config"))])
        }
        _ => None,
    };
    Some(ConfigFile {
        path,
        syntax,
        create_with,
        exists: false,
    })
}

/// Directory of «this compositor runs with this shell» markers.
pub fn marker_dir(home: &str, xdg_state_home: Option<&str>) -> String {
    let base = match xdg_state_home {
        Some(v) if !v.is_empty() => v.trim_end_matches('/').to_string(),
        _ => format!("{}/.local/state", home.trim_end_matches('/')),
    };
    format!("{base}/rexon/shells")
}

/// Marker of the «compositor with a shell» mode.
pub fn marker_path(wm: &str, shell: DesktopShell, home: &str, xdg_state_home: Option<&str>) -> String {
    format!("{}/{wm}-{}", marker_dir(home, xdg_state_home), shell.as_str())
}

/// Whether the marker path can be written into the launch line without inner quotes.
///
/// The whole command goes into `sh -c '…'`, and the shared helper escapes inner
/// quotes with a backslash, which sway strips before the shell sees it. So
/// the path must consist of shell-«safe» characters (letters of any
/// alphabet, digits, `_@%+=:./-`) — a home directory with a space or a quote
/// is rare, and we will not carry such a thing into the person's config.
pub fn path_is_config_safe(path: &str) -> bool {
    path.starts_with('/')
        && path
            .chars()
            .all(|c| c.is_alphanumeric() || "_@%+=:./-".contains(c))
}

/// argv of the autostart line: the shell starts only if the marker exists.
pub fn autostart_argv(shell: DesktopShell, marker: &str) -> Vec<String> {
    vec![
        "sh".to_string(),
        "-c".to_string(),
        format!("test -e {marker} && exec {}", launch_command(shell)),
    ]
}

/// Identifier of the shell's managed block (each has its own: both may be installed).
pub fn block_id(shell: DesktopShell) -> String {
    format!("shell-{}", shell.as_str())
}

/// Whether the text holds our block for this shell.
pub fn has_block(text: &str, syntax: Syntax, shell: DesktopShell) -> bool {
    ca::has_block(text, syntax, &block_id(shell))
}

/// What to do with the compositor config when installing a shell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnablePlan {
    /// Write the file (`created` — there was no personal config, it was created).
    Write {
        path: String,
        content: String,
        created: bool,
    },
    /// The block is already in place — write nothing.
    AlreadyThere { path: String },
    /// There is no personal config, and it may not be created.
    NoConfig,
    /// The line cannot be written into this config (characters, file layout).
    Refused { path: String, reason: Refusal },
}

/// Decide what to do with the config of compositor `wm` so that the shell
/// launches in the «with a shell» mode. `read` — reads a file (`None` — missing).
pub fn enable_plan(
    wm: &str,
    shell: DesktopShell,
    home: &str,
    xdg_config_home: Option<&str>,
    xdg_state_home: Option<&str>,
    read: &dyn Fn(&str) -> Option<String>,
) -> EnablePlan {
    let marker = marker_path(wm, shell, home, xdg_state_home);
    if !path_is_config_safe(&marker) {
        return EnablePlan::Refused {
            path: marker,
            reason: Refusal::BadCharacters,
        };
    }
    let exists = |p: &str| read(p).is_some();
    let Some(target) = config_target(wm, home, xdg_config_home, &exists) else {
        return EnablePlan::NoConfig;
    };
    let existing = read(&target.path);
    if existing.is_none() && target.create_with.is_none() {
        return EnablePlan::NoConfig;
    }
    let refused = |reason| EnablePlan::Refused {
        path: target.path.clone(),
        reason,
    };
    let body = match ca::launch_line(target.syntax, &block_id(shell), &autostart_argv(shell, &marker)) {
        Ok(b) => b,
        Err(r) => return refused(r),
    };
    let preamble = target.create_with.clone().unwrap_or_default();
    match ca::upsert_text(existing.as_deref(), target.syntax, &block_id(shell), &body, &preamble) {
        Ok(TextEdit::Write(content)) => EnablePlan::Write {
            path: target.path.clone(),
            content,
            created: existing.is_none(),
        },
        Ok(_) => EnablePlan::AlreadyThere {
            path: target.path.clone(),
        },
        Err(r) => refused(r),
    }
}

/// What to do with the config on «Remove».
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisablePlan {
    /// Config where the block was found (empty — the block is nowhere).
    pub path: String,
    /// `Unchanged` — nothing to touch, `Write` — new text, `Delete` —
    /// we created the file, and without the block it is empty.
    pub edit: TextEdit,
}

/// Find the shell block in the compositor configs and decide how to remove it.
pub fn disable_plan(
    wm: &str,
    shell: DesktopShell,
    home: &str,
    xdg_config_home: Option<&str>,
    read: &dyn Fn(&str) -> Option<String>,
) -> DisablePlan {
    for (path, syntax) in config_files(wm, home, xdg_config_home) {
        let Some(text) = read(&path) else { continue };
        if has_block(&text, syntax, shell) {
            return DisablePlan {
                edit: ca::remove_text(&text, syntax, &block_id(shell)),
                path,
            };
        }
    }
    DisablePlan {
        path: String::new(),
        edit: TextEdit::Unchanged,
    }
}

/// Whether the shell block is in this compositor's config.
pub fn block_installed(
    wm: &str,
    shell: DesktopShell,
    home: &str,
    xdg_config_home: Option<&str>,
    read: &dyn Fn(&str) -> Option<String>,
) -> bool {
    !disable_plan(wm, shell, home, xdg_config_home, read).path.is_empty()
}

// ============================================================================
// Warnings before installation
// ============================================================================

/// What to warn the person about BEFORE installation (research 01.10.2026).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Warning {
    /// Both shells write `gsettings … color-scheme` (light/dark) themselves —
    /// they will override Rexon's switch.
    ColorScheme,
    /// Both take over notifications and `StatusNotifierWatcher` (tray) if
    /// the name is free: one's own panel (waybar) may end up without icons.
    NotificationsAndTray,
    /// Both draw their own wallpaper on the background layer — over Rexon's wallpaper.
    OwnWallpaper,
    /// DMS under Hyprland with a live `hyprland.lua` moves `hyprland.conf` and
    /// `hypr/dms/*.conf` into `.dms-backups/` on EVERY launch.
    DmsMovesHyprlandConf,
}

/// Warnings for installing `shell` into `wm`. `hypr_lua` and `hypr_strays`
/// are probes of `~/.config/hypr` (DMS looks exactly there, bypassing
/// `XDG_CONFIG_HOME`): whether `hyprland.lua` exists and whether
/// `hyprland.conf` or `dms/*.conf` sit next to it.
pub fn install_warnings(
    shell: DesktopShell,
    wm: &str,
    hypr_lua: bool,
    hypr_strays: bool,
) -> Vec<Warning> {
    let mut out = vec![Warning::OwnWallpaper, Warning::ColorScheme, Warning::NotificationsAndTray];
    if shell == DesktopShell::DankMaterialShell && wm == "hyprland" && hypr_lua && hypr_strays {
        out.push(Warning::DmsMovesHyprlandConf);
    }
    out
}

// ============================================================================
// Shell state in the card
// ============================================================================

/// Everything the card needs to know about a shell — gathered by the caller.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ShellState {
    /// Whether the compositor itself is installed.
    pub wm_installed: bool,
    /// Whether the shell executable is installed.
    pub binary_present: bool,
    /// Whether our block is in the compositor config.
    pub block_present: bool,
    /// Whether the «compositor with this shell» marker exists.
    pub marker_on: bool,
    /// Whether the person is working in this compositor right now.
    pub is_current_wm: bool,
}

impl ShellState {
    /// The shell is installed as a bundle: both the package and the config line.
    pub fn installed(&self) -> bool {
        self.binary_present && self.block_present
    }

    /// The person is in this compositor with this shell right now.
    pub fn running_now(&self) -> bool {
        self.installed() && self.marker_on && self.is_current_wm
    }
}
