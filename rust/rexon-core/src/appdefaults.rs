//! Port of `rexon/appdefaults.py` — automatic switching of default applications
//! when the DE changes (the `DEAppSwitcher` class).
//!
//! Here is the PURE logic: the `DE → category → .desktop` tables, building
//! the per-DE `mimeapps.list` text, detecting the DE from the environment, building
//! the argv of DE-specific commands (kdeglobals/gsettings) and the XFCE
//! `helpers.rc` text, the texts of the autostart `.desktop` and the apply script. Nothing
//! is executed: checks for existing `.desktop` files and present binaries
//! come in as closures (`exists` / `which` — like `qg`/`qi` in
//! `system_plan::remove_plan`), and the result is expressed as structures
//! (`ProfileOutcome`, `ApplyDeSpecific` — command argv + file contents).
//! Live subprocesses / file writes stay with the executor (Python
//! through the bridge for now, rexon-gui later).
//!
//! Ported:
//!   * the `DE_NATIVE_APPS` tables (8 DEs) + a generic set for tiling/wlroots WMs
//!     (`wm_app_set`), `MIME_GROUPS`, `_FALLBACKS`, per-DE
//!     `mimeapps.list` names, `_XDG_DESKTOP_TO_DE`, `_FM_DESKTOP_CANDIDATES`;
//!   * `resolve_de_apps` / `resolve_mimeapps_filename` / `is_tiling_wm` /
//!     `detect_current_de` / `fm_short_name` / `default_file_manager_for_de`;
//!   * `resolve_desktop` (with a fallback chain), `generate_profile` (the
//!     mimeapps.list text byte for byte), `apply_de_specific` (+ the private
//!     `_apply_kde/_gnome/_mate/_cinnamon/_xfce/_generic_wm`),
//!     `get_installed_des`, `get_de_apps_info`;
//!   * the autostart `.desktop` text (`autostart_desktop_content`); the old
//!     python3 bash apply script was removed on 02.10.2026 — autostart calls the binary
//!     with `--apply-defaults`;
//!   * the pure part of `set_file_manager_default` (choosing the `.desktop` target +
//!     the `xdg-mime default/query` argv) and regex helpers for the
//!     `org.freedesktop.FileManager1` override (`fm1_exec_basename` / `fm1_matches` /
//!     `busctl_pid`, busctl argv).
//!
//! NOT ported (execution — when hooked up to live Python/the bridge):
//!   the actual `glob` of `.desktop` directories, running `xdg-mime`/`busctl`,
//!   writing `mimeapps.list` / `helpers.rc` / autostart, reading
//!   `/proc/<pid>/comm` and `os.kill` in `_override_filemanager1`,
//!   orchestrating `apply_for_current_de` / `on_switch_de` /
//!   `generate_all_profiles` / `install_autostart` (glue around the pure
//!   functions + file checks).

use crate::defaults::{terminal_exec_name, TerminalHome};
use crate::i18n::{tr, trf};
use serde::Serialize;

// ============================================================================
// DE → category → .desktop tables
// ============================================================================

type AppTable = &'static [(&'static str, &'static str)];

const KDE_APPS: AppTable = &[
    ("file_manager", "org.kde.dolphin.desktop"),
    ("terminal", "org.kde.konsole.desktop"),
    ("text_editor", "org.kde.kate.desktop"),
    ("image_viewer", "org.kde.gwenview.desktop"),
    ("video_player", "org.kde.haruna.desktop"),
    ("pdf_viewer", "org.kde.okular.desktop"),
    ("archive_manager", "org.kde.ark.desktop"),
    ("music_player", "org.kde.elisa.desktop"),
    ("screenshot", "org.kde.spectacle.desktop"),
];

const GNOME_APPS: AppTable = &[
    ("file_manager", "org.gnome.Nautilus.desktop"),
    ("terminal", "org.gnome.Console.desktop"),
    ("text_editor", "org.gnome.TextEditor.desktop"),
    ("image_viewer", "org.gnome.Loupe.desktop"),
    ("video_player", "org.gnome.Showtime.desktop"),
    ("pdf_viewer", "org.gnome.Papers.desktop"),
    ("archive_manager", "org.gnome.FileRoller.desktop"),
];

const MATE_APPS: AppTable = &[
    ("file_manager", "caja.desktop"),
    ("terminal", "mate-terminal.desktop"),
    ("text_editor", "pluma.desktop"),
    ("image_viewer", "eom.desktop"),
    ("pdf_viewer", "atril.desktop"),
    ("archive_manager", "engrampa.desktop"),
];

const COSMIC_APPS: AppTable = &[
    ("file_manager", "com.system76.CosmicFiles.desktop"),
    ("terminal", "com.system76.CosmicTerm.desktop"),
    ("text_editor", "com.system76.CosmicEdit.desktop"),
    ("video_player", "com.system76.CosmicPlayer.desktop"),
];

const CINNAMON_APPS: AppTable = &[
    ("file_manager", "nemo.desktop"),
    ("terminal", "org.gnome.Terminal.desktop"),
    ("text_editor", "xed.desktop"),
    ("image_viewer", "xviewer.desktop"),
    ("video_player", "xplayer.desktop"),
    ("pdf_viewer", "xreader.desktop"),
];

const XFCE_APPS: AppTable = &[
    ("file_manager", "thunar.desktop"),
    ("terminal", "xfce4-terminal.desktop"),
    ("text_editor", "mousepad.desktop"),
    ("image_viewer", "ristretto.desktop"),
    ("video_player", "parole.desktop"),
    ("archive_manager", "xarchiver.desktop"),
];

// Budgie: the programs of Ubuntu Budgie 26.04 (its `defaults.list`), installed by the
// `budgie` package set in `system::DE_DATABASE`. The `.desktop` names are checked against
// the Arch package file lists. Ubuntu Budgie splits music: FLAC/OGG —
// Lollypop, MP3/M4A/WAV — VLC; we have one category, and it holds the desktop's
// music program, Lollypop.
const BUDGIE_APPS: AppTable = &[
    ("file_manager", "nemo.desktop"),
    ("terminal", "xfce4-terminal.desktop"),
    ("text_editor", "org.gnome.gedit.desktop"),
    ("image_viewer", "org.gnome.gThumb.desktop"),
    ("video_player", "vlc.desktop"),
    ("pdf_viewer", "atril.desktop"),
    ("archive_manager", "org.gnome.FileRoller.desktop"),
    ("music_player", "org.gnome.Lollypop.desktop"),
];

// Pantheon: elementary's native programs from the `pantheon` group (the `.desktop`
// identifiers are `meson.project_name()` of their repositories; the picture viewer is
// a separate `.viewer` of Photos). Pantheon has no PDF reader or archiver of its own —
// so we do not write them.
const PANTHEON_APPS: AppTable = &[
    ("file_manager", "io.elementary.files.desktop"),
    ("terminal", "io.elementary.terminal.desktop"),
    ("text_editor", "io.elementary.code.desktop"),
    ("image_viewer", "io.elementary.photos.viewer.desktop"),
    ("video_player", "io.elementary.videos.desktop"),
];

// Deepin: programs of the `deepin` and `deepin-extra` groups (the `.desktop` names
// are checked against the linuxdeepin repositories). The groups have no PDF reader of their own.
const DEEPIN_APPS: AppTable = &[
    ("file_manager", "dde-file-manager.desktop"),
    ("terminal", "deepin-terminal.desktop"),
    ("text_editor", "deepin-editor.desktop"),
    ("image_viewer", "deepin-image-viewer.desktop"),
    ("video_player", "deepin-movie.desktop"),
    ("archive_manager", "deepin-compressor.desktop"),
];

const LXQT_APPS: AppTable = &[
    ("file_manager", "pcmanfm-qt.desktop"),
    ("terminal", "qterminal.desktop"),
    ("text_editor", "featherpad.desktop"),
    ("image_viewer", "lximage-qt.desktop"),
    ("archive_manager", "lxqt-archiver.desktop"),
];

/// Native applications of a known DE. None → a tiling/wlroots WM (generic).
/// Table order = Python dict order (matters for iteration).
fn native_apps(de_id: &str) -> Option<AppTable> {
    match de_id {
        "kde" => Some(KDE_APPS),
        "gnome" => Some(GNOME_APPS),
        "mate" => Some(MATE_APPS),
        "cosmic" => Some(COSMIC_APPS),
        "cinnamon-mint" => Some(CINNAMON_APPS),
        "xfce" => Some(XFCE_APPS),
        "budgie" => Some(BUDGIE_APPS),
        "pantheon" => Some(PANTHEON_APPS),
        "deepin" => Some(DEEPIN_APPS),
        "lxqt" => Some(LXQT_APPS),
        _ => None,
    }
}

/// The generic set's terminal per WM — port of `_WM_TERMINALS` (packages from system.py).
fn wm_terminal(wm_id: &str) -> &'static str {
    match wm_id {
        "hyprland" => "kitty.desktop",
        "niri" => "alacritty.desktop",
        "sway" => "foot.desktop",
        // Both are installed with foot (`system::DE_DATABASE`); it is also what Super+Enter
        // of the starter River init calls.
        "river" => "foot.desktop",
        "wayfire" => "foot.desktop",
        _ => "xterm.desktop",
    }
}

/// The generic set of defaults for a tiling / wlroots WM (any) —
/// port of `_wm_app_set`. One shared set, the terminal per WM (rule #12).
fn wm_app_set(wm_id: &str) -> Vec<(&'static str, String)> {
    vec![
        ("file_manager", "thunar.desktop".to_string()),
        ("terminal", wm_terminal(wm_id).to_string()),
        ("text_editor", "mousepad.desktop".to_string()),
        ("image_viewer", "org.gnome.Loupe.desktop".to_string()),
        ("pdf_viewer", "org.gnome.Papers.desktop".to_string()),
        ("archive_manager", "xarchiver.desktop".to_string()),
    ]
}

/// Legacy / fallback `.desktop` names — port of `_FALLBACKS` (order matters).
fn fallbacks(desktop_id: &str) -> &'static [&'static str] {
    match desktop_id {
        "org.gnome.Console.desktop" => {
            &["org.gnome.Terminal.desktop", "gnome-terminal.desktop"]
        }
        "org.gnome.TextEditor.desktop" => &["org.gnome.gedit.desktop", "gedit.desktop"],
        "org.gnome.Loupe.desktop" => &["org.gnome.eog.desktop", "eog.desktop"],
        "org.gnome.Showtime.desktop" => &["org.gnome.Totem.desktop", "totem.desktop"],
        "org.gnome.Papers.desktop" => &["org.gnome.Evince.desktop", "evince.desktop"],
        "org.kde.haruna.desktop" => &["org.kde.dragonplayer.desktop"],
        "kitty.desktop" => &["foot.desktop", "alacritty.desktop"],
        _ => &[],
    }
}

// ============================================================================
// Category → MIME types (port of MIME_GROUPS, order as in Python)
// ============================================================================

fn mime_group(category: &str) -> &'static [&'static str] {
    match category {
        "file_manager" => &["inode/directory"],
        "text_editor" => &[
            "text/plain", "text/x-csrc", "text/x-python", "text/x-shellscript",
            "text/markdown", "text/css", "text/csv", "text/xml",
            "application/json", "application/xml", "application/x-yaml",
            "application/toml", "application/javascript",
        ],
        "image_viewer" => &[
            "image/jpeg", "image/png", "image/gif", "image/bmp", "image/svg+xml",
            "image/tiff", "image/webp", "image/avif", "image/heif", "image/jxl",
            "image/x-icon", "image/vnd.microsoft.icon",
        ],
        "video_player" => &[
            "video/mp4", "video/x-matroska", "video/webm", "video/x-msvideo",
            "video/mpeg", "video/quicktime", "video/ogg", "video/x-flv",
            "video/3gpp", "video/mp2t", "video/x-ms-wmv",
        ],
        "pdf_viewer" => &["application/pdf", "application/x-pdf"],
        "archive_manager" => &[
            "application/zip", "application/gzip", "application/x-tar",
            "application/x-compressed-tar", "application/x-bzip-compressed-tar",
            "application/x-xz-compressed-tar", "application/x-zstd-compressed-tar",
            "application/x-7z-compressed", "application/vnd.rar",
        ],
        "music_player" => &[
            "audio/mpeg", "audio/ogg", "audio/flac", "audio/x-wav", "audio/mp4",
            "audio/aac", "audio/opus", "audio/x-vorbis+ogg",
        ],
        "browser" => &[
            "x-scheme-handler/http", "x-scheme-handler/https", "text/html",
            "application/xhtml+xml", "x-scheme-handler/about",
            "x-scheme-handler/unknown",
        ],
        _ => &[],
    }
}

// ============================================================================
// Per-DE mimeapps.list names, XDG detection, DE order
// ============================================================================

/// DE order as in `DE_NATIVE_APPS` after WM registration (8 basic ones +
/// hyprland/niri/sway via `setdefault`; River and Wayfire — since 24.10). Matters
/// for `get_installed_des` and the substring search in `detect_current_de`.
const DE_ORDER: &[&str] = &[
    "kde", "gnome", "mate", "cosmic", "cinnamon-mint", "xfce", "budgie",
    "pantheon", "deepin", "lxqt", "hyprland", "niri", "sway", "river", "wayfire",
];

/// The set of "known" (non-generic) DEs — port of `_KNOWN_DE_IDS`.
fn is_known_de(de_id: &str) -> bool {
    DE_ORDER.contains(&de_id)
}

/// Answers the question "has no native application set of its own" — port of
/// `_is_tiling_wm`.
///
/// ⚠️ The name is narrower than the meaning. The condition "or an unknown DE at all" is here
/// on purpose: for such a desktop we have no program list of our own anyway,
/// and the same shared set as for tiling ones fits. So the predicate answers
/// "use the generic set and `<id>-mimeapps.list`", NOT "this is a tiling
/// compositor".
///
/// The question "is it a tiling compositor" is decided by another source —
/// [`crate::de_customization::de_from_id`] → `De::Wlroots`. Do not confuse them: they
/// have different contents (here any unknown desktop gives `true`).
///
/// ⚠️ Wayfire has no native programs either, but it is NOT tiling and sits in
/// its own section "Compositors with effects" (`qt_desktops::TILING_IDS`
/// is checked against this predicate),
/// so it is not here. It gets the shared program set by another
/// criterion — "no set of its own" (`native_apps`), see `apply_de_specific`.
pub fn is_tiling_wm(de_id: &str) -> bool {
    matches!(de_id, "hyprland" | "niri" | "sway" | "river") || !is_known_de(de_id)
}

/// Native applications of a DE; for a tiling/wlroots WM — the shared set.
/// Port of `_resolve_de_apps`.
pub fn resolve_de_apps(de_id: &str) -> Vec<(&'static str, String)> {
    match native_apps(de_id) {
        Some(t) => t.iter().map(|(k, v)| (*k, v.to_string())).collect(),
        None => wm_app_set(de_id),
    }
}

/// A category's value in the set (like `apps.get(category)`).
fn app_get<'a>(apps: &'a [(&'static str, String)], category: &str) -> Option<&'a str> {
    apps.iter().find(|(k, _)| *k == category).map(|(_, v)| v.as_str())
}

/// Per-DE mimeapps.list name — port of `_resolve_mimeapps_filename`.
pub fn resolve_mimeapps_filename(de_id: &str) -> String {
    let known = match de_id {
        "kde" => Some("kde-mimeapps.list"),
        "gnome" => Some("gnome-mimeapps.list"),
        "mate" => Some("mate-mimeapps.list"),
        "cosmic" => Some("cosmic-mimeapps.list"),
        "cinnamon-mint" => Some("x-cinnamon-mimeapps.list"),
        "xfce" => Some("xfce-mimeapps.list"),
        "budgie" => Some("budgie-mimeapps.list"),
        "pantheon" => Some("pantheon-mimeapps.list"),
        // `XDG_CURRENT_DESKTOP=DDE` — the file is named after it, not after "deepin".
        "deepin" => Some("dde-mimeapps.list"),
        "hyprland" => Some("hyprland-mimeapps.list"),
        "niri" => Some("niri-mimeapps.list"),
        "sway" => Some("sway-mimeapps.list"),
        // `XDG_CURRENT_DESKTOP=river` (Rexon's starter init) and `wayfire`
        // (Wayfire sets it itself).
        "river" => Some("river-mimeapps.list"),
        "wayfire" => Some("wayfire-mimeapps.list"),
        _ => None,
    };
    // lxqt and an unknown WM: `{de_id}-mimeapps.list` (dict.get(...) or default).
    known.map(str::to_string).unwrap_or_else(|| format!("{de_id}-mimeapps.list"))
}

// ── A desktop's foreign autostart — only in that desktop ─────────────────
//
// Some desktops bring programs that start at login to ANY
// desktop: their records in `/etc/xdg/autostart` are not limited by `OnlyShowIn=`.
// A live case on 08.11.2026: Deepin brought the fcitx5 input manager and its
// helper; in MATE fcitx5 hijacked the keyboard layouts (it had English and Russian
// in different groups — Alt+Shift stopped switching), and the MATE layout
// icon disappeared. The cure is a user copy of the record with
// `OnlyShowIn=` of the desktop that brought it (`~/.config/autostart`
// shadows the system record with the same name).

/// An autostart record that belongs only in its own desktop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConfinedAutostart {
    /// File name in `/etc/xdg/autostart`.
    pub file: &'static str,
    /// The `OnlyShowIn=` value (names from the desktop's `XDG_CURRENT_DESKTOP`).
    pub only_show_in: &'static str,
    /// Restrict only if this package is installed AS A DEPENDENCY: then the
    /// desktop brought it. One installed by the person themselves (for example, fcitx5 for
    /// Chinese input) is not touched. `None` — the desktop's own helper,
    /// always restricted.
    pub only_if_dependency: Option<&'static str>,
}

/// Table: what to restrict and to which desktop.
pub const CONFINED_AUTOSTARTS: &[ConfinedAutostart] = &[
    ConfinedAutostart {
        file: "fcitx5-helper.desktop",
        only_show_in: "Deepin;DDE;",
        only_if_dependency: None,
    },
    ConfinedAutostart {
        file: "org.fcitx.Fcitx5.desktop",
        only_show_in: "Deepin;DDE;",
        only_if_dependency: Some("fcitx5"),
    },
];

/// A mark in the user copy: Rexon created it.
pub const CONFINED_AUTOSTART_MARK: &str = "X-Rexon-Confined=true";

/// Whether a copy is needed: the system record is not restricted to desktops, and the person
/// has no copy of their own (they might have created it themselves — then it is their call).
pub fn needs_confined_autostart(system_text: &str, user_text: Option<&str>) -> bool {
    if user_text.is_some() {
        return false;
    }
    !system_text
        .lines()
        .any(|l| l.trim_start().starts_with("OnlyShowIn=") || l.trim_start().starts_with("NotShowIn="))
}

/// Text of the user copy: the system record without its own `OnlyShowIn=`/
/// `NotShowIn=`, plus the desktop restriction and the Rexon mark.
pub fn confined_autostart_text(system_text: &str, only_show_in: &str) -> String {
    let mut out = String::new();
    for line in system_text.lines() {
        let t = line.trim_start();
        if t.starts_with("OnlyShowIn=") || t.starts_with("NotShowIn=") {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out.push_str(&format!("OnlyShowIn={only_show_in}\n{CONFINED_AUTOSTART_MARK}\n"));
    out
}

/// Name of the desktop's own terminal list — `<desktop>-xdg-terminals.list`, where
/// the desktop is the FIRST element of `XDG_CURRENT_DESKTOP` in lowercase: its list
/// is read by `xdg-terminal-exec` before all others. Accepts both the raw
/// `XDG_CURRENT_DESKTOP` (`MATE`, `X-Cinnamon`, `Budgie:GNOME`) and
/// a Rexon identifier (`cinnamon-mint` → `x-cinnamon-…`, `deepin` →
/// `dde-…`) — by the same naming rule as `<desktop>-mimeapps.list`.
/// `None` — the desktop is unknown (empty environment): only the shared list remains.
pub fn terminals_list_filename(desktop: &str) -> Option<String> {
    let token = desktop.split(':').next().unwrap_or("").trim().to_lowercase();
    if token.is_empty() {
        return None;
    }
    let prefix = if is_known_de(&token) {
        let mime = resolve_mimeapps_filename(&token);
        mime.trim_end_matches("mimeapps.list").to_string()
    } else {
        format!("{token}-")
    };
    Some(format!("{prefix}xdg-terminals.list"))
}

/// What to do at session login with the desktop's own terminal list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalsListPlan {
    /// Write the desktop's native terminal (`content` — the whole file text).
    Write { filename: String, terminal: String, content: String },
    /// The person chose a terminal for this desktop in Rexon — do not touch.
    KeepChosen { filename: String, terminal: String },
    /// The native terminal is already written — do nothing.
    UpToDate { filename: String },
    /// There is no native terminal (tiling WMs, unknown desktops) or it is not
    /// installed, or the desktop name is unknown — we do not maintain the list.
    Nothing,
}

/// The "desktop's own terminal list" decision at session login.
///
/// A foreign list (without the Rexon mark) and an earlier "native" one are rewritten
/// with the desktop's native terminal: it was exactly a foreign `mate-xdg-terminals.list` with
/// GNOME Terminal that opened programs with `Terminal=true` outside mate-terminal
/// (08.11.2026). A list marked "chosen by the person" — a deliberate choice in
/// the "Settings" tab — stays as is. The native terminal is the same one
/// [`apply_de_specific`] writes to the desktop's native place (tiling WMs have none
/// of their own: their terminal is set by the compositor config).
///
/// `desktop` — the raw `XDG_CURRENT_DESKTOP`; `existing` — the current text of
/// the list (None — no file).
pub fn native_terminals_list_plan(
    de_id: &str,
    desktop: &str,
    existing: Option<&str>,
    exists: &dyn Fn(&str) -> bool,
) -> TerminalsListPlan {
    let Some(filename) = terminals_list_filename(desktop) else {
        return TerminalsListPlan::Nothing;
    };
    if let Some(terminal) = existing.and_then(crate::defaults::chosen_terminal_in_list) {
        return TerminalsListPlan::KeepChosen { filename, terminal };
    }
    let Some(apps) = native_apps(de_id) else {
        return TerminalsListPlan::Nothing;
    };
    let Some(terminal) = apps
        .iter()
        .find(|(k, _)| *k == "terminal")
        .and_then(|(_, t)| resolve_desktop(t, exists))
    else {
        return TerminalsListPlan::Nothing;
    };
    let content = crate::defaults::desktop_terminals_list_text(&terminal, false);
    if existing == Some(content.as_str()) {
        return TerminalsListPlan::UpToDate { filename };
    }
    TerminalsListPlan::Write { filename, terminal, content }
}

/// Detect the current DE — port of `_detect_current_de`. The input is environment
/// values (`XDG_CURRENT_DESKTOP`, `DESKTOP_SESSION`, presence of
/// `WAYLAND_DISPLAY`) instead of reading `os.environ` inside.
///
/// There is no desktop table of its own here anymore — there is one for the whole product
/// ([`crate::system::detect_current_de`]). Two defects of the original were fixed
/// along the way: tokens were compared CASE-SENSITIVELY (`kde` instead of `KDE` was not
/// recognized), and the session name knew neither `plasma` nor `cinnamon` — the most
/// common `DESKTOP_SESSION` values on Plasma and on Mint gave "don't know".
pub fn detect_current_de(
    xdg_current_desktop: &str,
    desktop_session: &str,
    has_wayland: bool,
) -> Option<String> {
    crate::system::detect_current_de(xdg_current_desktop, desktop_session, has_wayland)
}

// ============================================================================
// resolve_desktop + short file manager names
// ============================================================================

/// Return `desktop_id` if it exists, otherwise the first existing
/// fallback, otherwise None — port of `_resolve_desktop`.
pub fn resolve_desktop(desktop_id: &str, exists: &dyn Fn(&str) -> bool) -> Option<String> {
    if exists(desktop_id) {
        return Some(desktop_id.to_string());
    }
    for fb in fallbacks(desktop_id) {
        if exists(fb) {
            return Some((*fb).to_string());
        }
    }
    None
}

/// Short names of file managers Rexon knows — IN ORDER of
/// preference. One list per program: both parsing the
/// `.desktop` name and the set of options in the "Settings" tab are built from it (rule #12 —
/// a new file manager is added in one place).
pub const FM_SHORT_NAMES: &[&str] = &[
    "dolphin",
    "nautilus",
    "nemo",
    "thunar",
    "caja",
    "pcmanfm",
    "pcmanfm-qt",
    "cosmic-files",
    "io.elementary.files",
    "dde-file-manager",
];

/// Short FM name (as SettingsManager stores it) → `.desktop` candidates.
/// The first existing one wins. Port of `_FM_DESKTOP_CANDIDATES` (order!).
fn fm_desktop_candidates(short: &str) -> Option<&'static [&'static str]> {
    match short {
        "dolphin" => Some(&["org.kde.dolphin.desktop", "dolphin.desktop"]),
        "nautilus" => Some(&["org.gnome.Nautilus.desktop", "nautilus.desktop"]),
        "nemo" => Some(&["nemo.desktop"]),
        "thunar" => Some(&["thunar.desktop", "Thunar.desktop"]),
        "caja" => Some(&["caja.desktop", "caja-browser.desktop"]),
        "pcmanfm" => Some(&["pcmanfm.desktop"]),
        "pcmanfm-qt" => Some(&["pcmanfm-qt.desktop"]),
        "cosmic-files" => {
            Some(&["com.system76.CosmicFiles.desktop", "cosmic-files.desktop"])
        }
        "io.elementary.files" => Some(&["io.elementary.files.desktop"]),
        "dde-file-manager" => Some(&["dde-file-manager.desktop"]),
        _ => None,
    }
}

/// "org.kde.dolphin.desktop" → "dolphin" — port of `_fm_short_name`.
pub fn fm_short_name(fm_id: &str, desktop: &str) -> String {
    if !fm_id.ends_with(".desktop") {
        return fm_id.to_lowercase();
    }
    // The order of the _FM_DESKTOP_CANDIDATES dict.
    for short in FM_SHORT_NAMES {
        if let Some(cands) = fm_desktop_candidates(short) {
            if cands.contains(&desktop) {
                return short.to_string();
            }
        }
    }
    // os.path.splitext(desktop)[0].split(".")[-1].lower()
    let stem = desktop.strip_suffix(".desktop").unwrap_or(desktop);
    stem.rsplit('.').next().unwrap_or(stem).to_lowercase()
}

/// "auto" mode: DE → short name of the native file manager —
/// port of `default_file_manager_for_de`. `which` — `shutil.which` (presence of
/// a binary); the iteration branch is effectively unreachable (resolve_de_apps always
/// gives a file_manager), kept for parity.
pub fn default_file_manager_for_de(de: Option<&str>, which: &dyn Fn(&str) -> bool) -> String {
    let de_l = de.unwrap_or("").to_lowercase();
    let apps = resolve_de_apps(&de_l);
    if let Some(desktop) = app_get(&apps, "file_manager") {
        return fm_short_name(desktop, desktop);
    }
    for cand in ["dolphin", "thunar", "nautilus", "nemo", "caja", "pcmanfm-qt", "pcmanfm"] {
        if which(cand) {
            return cand.to_string();
        }
    }
    "dolphin".to_string()
}

// ============================================================================
// generate_profile — the per-DE mimeapps.list text
// ============================================================================

/// The result of `generate_profile`. The executor appends `filepath =
/// ~/.config/{filename}`: on `Write` it writes `content` and returns
/// `(true, filepath)`; `Skip`/`NoApps` are ready `(true|false, message)`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ProfileOutcome {
    /// The DE is not installed (file_manager not found) → `(true, message)`.
    Skip { message: String },
    /// No applications available → `(false, message)`. Practically unreachable
    /// (file_manager gives at least one association), kept for parity.
    NoApps { message: String },
    /// Write `content` to `~/.config/{filename}`; `count` associations.
    Write { filename: String, content: String, count: usize },
}

/// Port of `DEAppSwitcher.generate_profile`. `exists` — `_desktop_exists`.
pub fn generate_profile(de_id: &str, exists: &dyn Fn(&str) -> bool) -> ProfileOutcome {
    let apps = resolve_de_apps(de_id);
    let filename = resolve_mimeapps_filename(de_id);

    // The "is the DE installed" check: the DE's file_manager not found → silent skip.
    if let Some(fm) = app_get(&apps, "file_manager") {
        if !exists(fm) {
            return ProfileOutcome::Skip {
                // TRANSLATORS: %1 is a desktop environment id (kde, gnome…).
                message: trf("%1: not installed, skipping", &[&de_id]),
            };
        }
    }

    // MIME → .desktop (a Python dict; the result is sorted by MIME).
    let mut associations: Vec<(&'static str, String)> = Vec::new();
    for (category, desktop_id) in &apps {
        if *category == "terminal" || *category == "screenshot" {
            continue; // terminal/screenshot — not via MIME
        }
        let Some(resolved) = resolve_desktop(desktop_id, exists) else {
            continue;
        };
        for mime in mime_group(category) {
            // dict[mime] = resolved (overwritten on repeat; MIME groups
            // do not overlap, so there are no overwrites).
            if let Some(e) = associations.iter_mut().find(|(m, _)| m == mime) {
                e.1 = resolved.clone();
            } else {
                associations.push((mime, resolved.clone()));
            }
        }
    }
    // We do not overwrite the browser (Firefox/Chrome are cross-platform).

    if associations.is_empty() {
        return ProfileOutcome::NoApps {
            // The desktop name for the person, not the identifier ("for unknown").
            message: crate::screenshots::place_de_name(
                // TRANSLATORS: %1 is the name of a desktop environment.
                &tr("No apps available for %1"),
                "%1",
                &crate::screenshots::display_name(de_id),
            ),
        };
    }

    let count = associations.len();
    associations.sort_by(|a, b| a.0.cmp(b.0));
    let mut lines = vec!["[Default Applications]".to_string()];
    for (mime, desktop) in &associations {
        lines.push(format!("{mime}={desktop}"));
    }
    let content = lines.join("\n") + "\n";
    ProfileOutcome::Write { filename, content, count }
}

// ============================================================================
// apply_de_specific — DE-specific terminal settings
// ============================================================================

/// DE-specific plan: argv of `safe_run` commands in call order, the contents of
/// XFCE `helpers.rc` (if DE = xfce), logs. Port of `apply_de_specific`.
#[derive(Debug, Clone, PartialEq, Serialize, Default)]
pub struct ApplyDeSpecific {
    /// The argv of each `safe_run(..., capture_output=True)`.
    pub commands: Vec<Vec<String>>,
    /// New contents of `~/.config/xfce4/helpers.rc` (written by the executor).
    pub helpers_rc: Option<String>,
    /// In-place edits of foreign configs (LXQt: `lxqt-qtxdg.conf`). Paths
    /// start with `~/.config/` — the executor expands them with the same
    /// `expand` as `helpers.rc` (taking `$XDG_CONFIG_HOME` into account).
    pub file_steps: Vec<crate::defaults::TermStep>,
    pub logs: Vec<String>,
}

/// Commands (`Run`) from the shared terminal table [`crate::defaults::terminal_home_steps`].
fn terminal_commands(home: TerminalHome, term: &str) -> Vec<Vec<String>> {
    crate::defaults::terminal_home_steps(home, term, "~/.config")
        .into_iter()
        .filter_map(|s| match s {
            crate::defaults::TermStep::Run { argv } => Some(argv),
            _ => None,
        })
        .collect()
}


/// Port of `apply_de_specific`. `exists`/`which` — `_desktop_exists`/
/// `shutil.which`; `xfce_helpers_existing` — the existing `helpers.rc` as read
/// (None if there is no file; only taken into account for xfce).
pub fn apply_de_specific(
    de_id: &str,
    exists: &dyn Fn(&str) -> bool,
    which: &dyn Fn(&str) -> bool,
    xfce_helpers_existing: Option<&str>,
) -> ApplyDeSpecific {
    apply_de_specific_with_terminal(
        de_id,
        None,
        exists,
        which,
        xfce_helpers_existing,
        &crate::defaults::xfce_system_helper_exists,
    )
}

/// The same, but with the terminal the person chose for this desktop in the
/// "Settings" tab (`chosen_terminal` — from the desktop's own list marked
/// "chosen by the person", see [`native_terminals_list_plan`]). The person's choice
/// takes priority over the native terminal (Wehrwolfmann's decision of 08.11.2026): it goes into
/// ALL the desktop's native places. If the chosen terminal is not installed, the native one stays,
/// and the log says so. `xfce_helper_exists` — whether the system has an exo
/// helper for the program (for a chosen XFCE terminal without a helper
/// our own `custom-TerminalEmulator` is created, as in the "Settings" tab).
pub fn apply_de_specific_with_terminal(
    de_id: &str,
    chosen_terminal: Option<&str>,
    exists: &dyn Fn(&str) -> bool,
    which: &dyn Fn(&str) -> bool,
    xfce_helpers_existing: Option<&str>,
    xfce_helper_exists: &dyn Fn(&str) -> bool,
) -> ApplyDeSpecific {
    let mut apps = resolve_de_apps(de_id);
    let native = app_get(&apps, "terminal").map(str::to_string);
    let mut notes = Vec::new();
    let chosen = match chosen_terminal {
        Some(c) if exists(c) => Some(c.to_string()),
        Some(c) => {
            let native_name = native.as_deref().map(terminal_exec_name).unwrap_or_default();
            notes.push(trf(
                "ℹ️ Terminal: %1 (%2 isn't installed)",
                &[&native_name, &terminal_exec_name(c)],
            ));
            None
        }
        None => None,
    };
    if let Some(c) = &chosen {
        match apps.iter_mut().find(|(k, _)| *k == "terminal") {
            Some(e) => e.1 = c.clone(),
            None => apps.push(("terminal", c.clone())),
        }
    }
    let mut out = apply_de_specific_for(de_id, &apps, exists, which, xfce_helpers_existing);
    // XFCE: the helper name in `helpers.rc` is written by `apply_xfce`, but a program without
    // a system helper needs its own — the same steps as in the "Settings" tab.
    if let (Some(c), "xfce") = (&chosen, de_id) {
        out.file_steps.extend(crate::defaults::terminal_home_steps_with(
            TerminalHome::Xfce,
            c,
            "~/.config",
            xfce_helper_exists,
        ));
    }
    notes.append(&mut out.logs);
    out.logs = notes;
    out
}

fn apply_de_specific_for(
    de_id: &str,
    apps: &[(&'static str, String)],
    exists: &dyn Fn(&str) -> bool,
    which: &dyn Fn(&str) -> bool,
    xfce_helpers_existing: Option<&str>,
) -> ApplyDeSpecific {
    let terminal = app_get(apps, "terminal").and_then(|t| resolve_desktop(t, exists));
    let terminal = terminal.as_deref();

    match de_id {
        "kde" => apply_kde(terminal, which),
        "gnome" => apply_gnome(terminal, which),
        "mate" => apply_mate(terminal, which),
        "cinnamon-mint" => apply_cinnamon(terminal, which),
        "xfce" => apply_xfce(apps, xfce_helpers_existing),
        "lxqt" => apply_lxqt(terminal),
        // Budgie: the GNOME schema, and also the Cinnamon schema — Nemo reads it
        // (`crate::defaults::terminal_extra_homes`).
        "budgie" => {
            let mut out = apply_gnome(terminal, which);
            if let (Some(term), true) = (terminal, which("gsettings")) {
                for extra in crate::defaults::terminal_extra_homes(de_id) {
                    out.commands.extend(terminal_commands(*extra, term));
                }
            }
            out
        }
        // Pantheon: the io.elementary.* schemas have no terminal key of their own —
        // the native place is the same as GNOME's.
        "pantheon" => apply_gnome(terminal, which),
        "deepin" => apply_deepin(terminal, which),
        "cosmic" => ApplyDeSpecific {
            logs: vec![tr("ℹ️ COSMIC: default apps via XDG MIME (no gsettings)")],
            ..Default::default()
        },
        // No program set of its own — tiling ones, Wayfire and unknown ones.
        _ if native_apps(de_id).is_none() => apply_generic_wm(de_id, terminal),
        _ => ApplyDeSpecific::default(),
    }
}

fn apply_kde(terminal: Option<&str>, which: &dyn Fn(&str) -> bool) -> ApplyDeSpecific {
    let mut out = ApplyDeSpecific::default();
    if !which("kwriteconfig6") {
        out.logs.push(tr("⚠️ kwriteconfig6 not found"));
        return out;
    }
    if let Some(term) = terminal {
        out.commands = terminal_commands(TerminalHome::Kde, term);
        out.logs.push(trf("✅ KDE terminal: %1", &[&terminal_exec_name(term)]));
    }
    out
}

fn apply_gnome(terminal: Option<&str>, which: &dyn Fn(&str) -> bool) -> ApplyDeSpecific {
    let mut out = ApplyDeSpecific::default();
    if !which("gsettings") {
        out.logs.push(tr("⚠️ gsettings not found"));
        return out;
    }
    if let Some(term) = terminal {
        out.commands = terminal_commands(TerminalHome::Gnome, term);
        out.logs.push(trf("✅ GNOME terminal: %1", &[&terminal_exec_name(term)]));
    }
    out
}

/// Deepin: the terminal is its own schema `com.deepin.desktop.default-applications.terminal`.
fn apply_deepin(terminal: Option<&str>, which: &dyn Fn(&str) -> bool) -> ApplyDeSpecific {
    let mut out = ApplyDeSpecific::default();
    if !which("gsettings") {
        out.logs.push(tr("⚠️ gsettings not found"));
        return out;
    }
    if let Some(term) = terminal {
        out.commands = terminal_commands(TerminalHome::Deepin, term);
        out.logs.push(trf("✅ Deepin terminal: %1", &[&terminal_exec_name(term)]));
    }
    out
}

fn apply_mate(terminal: Option<&str>, which: &dyn Fn(&str) -> bool) -> ApplyDeSpecific {
    let mut out = ApplyDeSpecific::default();
    if !which("gsettings") {
        out.logs.push(tr("⚠️ gsettings not found"));
        return out;
    }
    if let Some(term) = terminal {
        out.commands = terminal_commands(TerminalHome::Mate, term);
        out.logs.push(trf("✅ MATE terminal: %1", &[&terminal_exec_name(term)]));
    }
    out
}

fn apply_cinnamon(terminal: Option<&str>, which: &dyn Fn(&str) -> bool) -> ApplyDeSpecific {
    let mut out = ApplyDeSpecific::default();
    if !which("gsettings") {
        out.logs.push(tr("⚠️ gsettings not found"));
        return out;
    }
    if let Some(term) = terminal {
        out.commands = terminal_commands(TerminalHome::Cinnamon, term);
        out.logs.push(trf("✅ Cinnamon terminal: %1", &[&terminal_exec_name(term)]));
    }
    out
}

/// LXQt: the libqtxdg terminal (`lxqt-qtxdg.conf`) — used by the panel, lxqt-runner
/// and "Open in terminal" of libqtxdg programs.
fn apply_lxqt(terminal: Option<&str>) -> ApplyDeSpecific {
    let mut out = ApplyDeSpecific::default();
    if let Some(term) = terminal {
        out.file_steps = crate::defaults::terminal_home_steps(TerminalHome::Lxqt, term, "~/.config");
        out.logs.push(trf("✅ LXQt terminal: %1", &[&terminal_exec_name(term)]));
    }
    out
}

/// Name of the XFCE file manager helper by `.desktop` — the program's short
/// name ([`fm_short_name`]), which the system helpers are also named after
/// (`/usr/share/xfce4/helpers/thunar.desktop`).
///
/// Fix of 02.10.2026: the `desktop_to_helper` port wrote `Thunar` capitalized,
/// while the helper file is `thunar.desktop`; on a case-sensitive file system
/// exo did not find it, and instead of Thunar a program chooser window opened.
/// XFCE's own default (`/etc/xdg/xfce4/helpers.rc`) is `FileManager=thunar`.
fn xfce_helper_name(desktop_id: &str) -> String {
    fm_short_name(desktop_id, desktop_id)
}

/// "Default file manager" steps in the desktop's native place beyond
/// `xdg-mime` — shared by the "Settings" tab and the test bench. Currently this is XFCE:
/// `helpers.rc FileManager=` (read by `exo-open --launch FileManager`,
/// the XFCE panel and desktop), and when there is no system helper — our own
/// `custom-FileManager` (see [`crate::defaults::xfce_helper_steps`]).
/// `desktop` — `XDG_CURRENT_DESKTOP` or a Rexon identifier; `cfg` —
/// the settings directory.
pub fn file_manager_home_steps(
    fm_desktop: &str,
    desktop: &str,
    cfg: &str,
    xfce_helper_exists: &dyn Fn(&str) -> bool,
) -> Vec<crate::defaults::TermStep> {
    if crate::defaults::terminal_home(desktop) != TerminalHome::Xfce {
        return Vec::new();
    }
    let name = xfce_helper_name(fm_desktop);
    let has = xfce_helper_exists(&name);
    crate::defaults::xfce_helper_steps(crate::defaults::XfceHelper::FileManager, &name, cfg, has)
}

fn apply_xfce(apps: &[(&'static str, String)], existing: Option<&str>) -> ApplyDeSpecific {
    let mut out = ApplyDeSpecific::default();
    // Existing k=v pairs (a dict — overwritten; the result is sorted).
    let mut lines: Vec<(String, String)> = Vec::new();
    if let Some(text) = existing {
        for line in text.lines() {
            let line = line.trim();
            if line.contains('=') && !line.starts_with('#') {
                if let Some((k, v)) = line.split_once('=') {
                    let k = k.trim().to_string();
                    let v = v.trim().to_string();
                    if let Some(e) = lines.iter_mut().find(|(ek, _)| *ek == k) {
                        e.1 = v;
                    } else {
                        lines.push((k, v));
                    }
                }
            }
        }
    }
    // Category → helper key (Python dict order).
    for (cat, helper_key) in [("terminal", "TerminalEmulator"), ("file_manager", "FileManager")] {
        if let Some(desktop_id) = app_get(apps, cat) {
            // The terminal helper name — from the shared table (the same one the
            // "Settings" tab writes): exo helper names are lowercase.
            let helper_name = if cat == "terminal" {
                terminal_exec_name(desktop_id)
            } else {
                xfce_helper_name(desktop_id)
            };
            if let Some(e) = lines.iter_mut().find(|(k, _)| k == helper_key) {
                e.1 = helper_name;
            } else {
                lines.push((helper_key.to_string(), helper_name));
            }
        }
    }
    lines.sort_by(|a, b| a.0.cmp(&b.0));
    let content: String = lines.iter().map(|(k, v)| format!("{k}={v}\n")).collect();
    out.helpers_rc = Some(content);
    out.logs.push(tr("✅ XFCE helpers.rc updated"));
    out
}

fn apply_generic_wm(de_id: &str, terminal: Option<&str>) -> ApplyDeSpecific {
    let mut out = ApplyDeSpecific::default();
    if let Some(term) = terminal {
        let term_bin = terminal_exec_name(term);
        out.logs.push(trf(
            // TRANSLATORS: %1 is a window manager id (sway, hyprland…), %2 a terminal program.
            "ℹ️ %1: default terminal is %2 (set it in the window manager's config)",
            &[&de_id, &term_bin],
        ));
    }
    // TRANSLATORS: %1 is a window manager id (sway, hyprland…).
    out.logs.push(trf("ℹ️ %1: default apps via XDG MIME", &[&de_id]));
    out
}

// ============================================================================
// Information / list of installed DEs
// ============================================================================

/// List of DEs for which at least a file_manager was found — port of
/// `get_installed_des` (iteration in `DE_ORDER` order).
pub fn installed_des(exists: &dyn Fn(&str) -> bool) -> Vec<String> {
    let mut out = Vec::new();
    for de_id in DE_ORDER {
        let apps = resolve_de_apps(de_id);
        if let Some(fm) = app_get(&apps, "file_manager") {
            if exists(fm) {
                out.push((*de_id).to_string());
            }
        }
    }
    out
}

/// The table `category → resolved_desktop_id | None` — port of `get_de_apps_info`.
pub fn de_apps_info(
    de_id: &str,
    exists: &dyn Fn(&str) -> bool,
) -> Vec<(String, Option<String>)> {
    let apps = resolve_de_apps(de_id);
    apps.iter()
        .map(|(cat, desktop)| ((*cat).to_string(), resolve_desktop(desktop, exists)))
        .collect()
}

// ============================================================================
// Autostart / apply script (texts)
// ============================================================================

/// The program for the defaults autostart `Exec=` (the `--apply-defaults` flag
/// is added by [`autostart_desktop_content`]). `None` — nothing to launch with.
///
/// ⚠️ The entry point is chosen by THE SAME function as for the shortcut and the tray
/// ([`crate::installer::launch_entry_live`]) — it arrives here ready.
/// The Rust entry is the window binary: it handles `--apply-defaults` without a window
/// (`cli::run_headless`), and no interpreter is brought up at session login.
/// The Python entry (rollback) — as before: `rexon` from `PATH`, otherwise the
/// running binary itself.
///
/// ⭐ Fix during the port: the path went into `Exec=` without quotes, and a directory with
/// a space split the command into two arguments. Now any entry follows the
/// Desktop Entry rules ([`crate::installer::desktop_exec_arg`]).
pub fn autostart_exec_for(
    entry: &crate::installer::LaunchEntry,
    rexon_path: Option<&str>,
    self_exe: Option<&str>,
) -> Option<String> {
    use crate::installer::desktop_exec_arg;
    // The shared rule is [`crate::app::launch_program`], ONE for the tray,
    // the login splash and this record. Only the last link is our own here:
    // the running Rexon binary itself, which handles
    // `--apply-defaults` — no intermediary is needed for this route.
    crate::app::launch_program(entry, rexon_path).or_else(|| {
        self_exe
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(desktop_exec_arg)
    })
}

// TRANSLATORS: Description of the autostart entry that applies Rexon's default applications at login (shown in the desktop's autostart settings).
const AUTOSTART_COMMENT: &str = crate::i18n::n_("Switches default applications for the current desktop environment");

/// The autostart `.desktop` template — port of `_AUTOSTART_DESKTOP`.
///
/// `Comment` — in all catalog languages ([`crate::system_l10n::desktop_key`]):
/// previously it was Russian only, without a language key, and an English buyer
/// saw Russian.
///
/// ⛔ There is NO launch phase (`X-GNOME-Autostart-Phase`) here, and it must not
/// be brought back. The old template carried `Initialization`, and gnome-session 49+
/// does not execute third-party records with a phase at all ("sets
/// X-GNOME-Autostart-Phase, but gnome-session no longer manages session
/// services" in the journal) — in GNOME the defaults were silently not applied at login.
/// Nobody needs an early start anymore: mimeapps are read when a file
/// is opened, and the FileManager1 slot is taken from a live process by
/// [`crate::appdefaults_exec::DeAppSwitcher::ensure_filemanager1`]. There is one record
/// for all sessions (`~/.config/autostart`), so "a phase only for
/// Cinnamon/MATE" is impossible — a common denominator is needed. Old files
/// are fixed by [`heal_autostart_text`].
pub fn autostart_desktop_content(exec_path: &str) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Rexon App Defaults\n\
         {comment}\
         Exec={exec_path} --apply-defaults\n\
         Terminal=false\n\
         NoDisplay=true\n",
        comment = crate::system_l10n::desktop_key("Comment", AUTOSTART_COMMENT),
    )
}

/// The gnome-session phase key that must not be in our record (see
/// [`autostart_desktop_content`]).
pub const LEGACY_AUTOSTART_PHASE_KEY: &str = "X-GNOME-Autostart-Phase";

/// Fixing the defaults autostart record the person already has: remove the phase
/// line, leave everything else (Exec, `NotShowIn=` from the compositor mirror,
/// the translation) as is. `None` — nothing to fix.
///
/// Rewriting "on the next desktop switch" is not enough: in GNOME 49+ the
/// record itself is not executed, and the GNOME session will never fix it. So
/// the fix is also called by session login to any other desktop
/// ([`crate::appdefaults_exec::DeAppSwitcher::apply_for_current_de`]):
/// there the old record is executed and fixes itself for the next login to
/// GNOME. Switching the desktop through Rexon rewrites the whole record.
pub fn heal_autostart_text(text: &str) -> Option<String> {
    let is_phase = |l: &str| {
        l.trim_start()
            .strip_prefix(LEGACY_AUTOSTART_PHASE_KEY)
            .is_some_and(|rest| rest.trim_start().starts_with('='))
    };
    if !text.lines().any(is_phase) {
        return None;
    }
    Some(text.split_inclusive('\n').filter(|l| !is_phase(l)).collect())
}

// ============================================================================
// set_file_manager_default (pure part) + FileManager1 regex helpers
// ============================================================================

pub const FM1_NAME: &str = "org.freedesktop.FileManager1";

/// Choosing the target `.desktop` for the default FM — the pure part of
/// `set_file_manager_default` (before the subprocess). None → no candidate found.
pub fn fm_default_target(fm_id: &str, exists: &dyn Fn(&str) -> bool) -> Option<String> {
    let candidates: Vec<String> = if fm_id.ends_with(".desktop") {
        vec![fm_id.to_string()]
    } else {
        match fm_desktop_candidates(&fm_id.to_lowercase()) {
            Some(c) => c.iter().map(|s| s.to_string()).collect(),
            None => vec![format!("{fm_id}.desktop")],
        }
    };
    candidates.into_iter().find(|c| exists(c))
}

/// argv `xdg-mime default <desktop> inode/directory`.
pub fn xdg_mime_default_cmd(desktop: &str) -> Vec<String> {
    vec![
        "xdg-mime".into(), "default".into(), desktop.into(),
        "inode/directory".into(),
    ]
}

/// argv `xdg-mime query default inode/directory`.
pub fn xdg_mime_query_cmd() -> Vec<String> {
    vec![
        "xdg-mime".into(), "query".into(), "default".into(),
        "inode/directory".into(),
    ]
}

/// `re.search(r"^Exec=(\S+)", txt, re.M)` → basename(group(1)). None — none.
pub fn fm1_exec_basename(txt: &str) -> Option<String> {
    for line in txt.split('\n') {
        if let Some(rest) = line.strip_prefix("Exec=") {
            // \S+ — up to the first whitespace character.
            let val: &str = rest.split_whitespace().next().unwrap_or("");
            if val.is_empty() {
                return None; // \S+ requires at least one character
            }
            let base = val.rsplit('/').next().unwrap_or(val);
            return Some(base.to_string());
        }
    }
    None
}

/// The condition for installing the FileManager1 override: `Name=<FM1>` in the text AND
/// basename(Exec) == binary — port of the condition in `_override_filemanager1`.
pub fn fm1_matches(txt: &str, binary: &str) -> bool {
    txt.contains(&format!("Name={FM1_NAME}"))
        && fm1_exec_basename(txt).as_deref() == Some(binary)
}

/// `re.search(r"^PID=(\d+)", stdout, re.M)` → int. Port of the busctl parsing.
pub fn busctl_pid(stdout: &str) -> Option<i64> {
    for line in stdout.split('\n') {
        if let Some(rest) = line.strip_prefix("PID=") {
            let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            if !digits.is_empty() {
                return digits.parse().ok();
            }
        }
    }
    None
}

/// argv `busctl --user --no-pager status <name>`.
pub fn busctl_status_cmd(name: &str) -> Vec<String> {
    vec![
        "busctl".into(), "--user".into(), "--no-pager".into(), "status".into(),
        name.into(),
    ]
}

/// argv for reloading the bus configuration (`busctl ... ReloadConfig`).
pub fn busctl_reload_cmd() -> Vec<String> {
    vec![
        "busctl".into(), "--user".into(), "call".into(),
        "org.freedesktop.DBus".into(), "/org/freedesktop/DBus".into(),
        "org.freedesktop.DBus".into(), "ReloadConfig".into(),
    ]
}

// ============================================================================
// The `org.freedesktop.FileManager1` slot: who holds it and how to get it back
// ============================================================================
//
// WHY THIS IS HERE. The "Open containing folder" item BYPASSES mimeapps: in
// applications it is a call of the `ShowItems` method by the name `org.freedesktop.FileManager1`
// on the user bus. While a LIVE process holds the name, D-Bus does not look at
// `.service` files at all — neither the vendor ones nor our override in
// `~/.local/share/dbus-1/services`. So correct mimeapps are NOT ENOUGH:
// a properly configured machine still opens someone else's file manager
// if it managed to claim the name first (in our case KDE restored such a session from
// the saved session — `Thunar --daemon` from `ksmserverrc`).
//
// SEVERAL packages claim the name at once (four on our machine: nautilus,
// dolphin, nemo, Thunar), so "whoever is first gets the folders". Here is the pure
// part of the cure: identify the owner, decide whether it is ours or foreign, and whether it may
// be killed. The actions are in [`crate::appdefaults_exec`].

/// How a file manager behaves around the FileManager1 name: how it is stopped
/// the standard way and by what signs its service mode (no windows) is recognized.
///
/// One table for the product (rule #12): a new manager is added here, not
/// as a branch in the executor flow. Completeness is guarded by a check — every name from
/// [`FM_SHORT_NAMES`] must have a record here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FmDaemonInfo {
    /// The manager's short name — a key shared with [`FM_SHORT_NAMES`].
    pub short: &'static str,
    /// The standard quit flag (`thunar --quit`). Empty — no quit of its own,
    /// what remains is a gentle signal to exactly the PID that holds the name.
    pub quit_flag: &'static str,
    /// Signs of service mode in the process command line.
    pub daemon_flags: &'static [&'static str],
}

/// The "manager → how it is stopped the standard way → what reveals service mode" table.
pub const FM_DAEMONS: &[FmDaemonInfo] = &[
    FmDaemonInfo { short: "dolphin", quit_flag: "", daemon_flags: &["--daemon"] },
    FmDaemonInfo {
        short: "nautilus",
        quit_flag: "--quit",
        daemon_flags: &["--gapplication-service"],
    },
    FmDaemonInfo {
        short: "nemo",
        quit_flag: "--quit",
        daemon_flags: &["--gapplication-service", "--no-default-window"],
    },
    FmDaemonInfo {
        short: "thunar",
        quit_flag: "--quit",
        daemon_flags: &["--daemon", "--gapplication-service"],
    },
    FmDaemonInfo {
        short: "caja",
        quit_flag: "--quit",
        daemon_flags: &["--gapplication-service", "--no-default-window"],
    },
    FmDaemonInfo { short: "pcmanfm", quit_flag: "--quit", daemon_flags: &["--daemon-mode"] },
    FmDaemonInfo {
        short: "pcmanfm-qt",
        quit_flag: "--quit",
        daemon_flags: &["--daemon-mode"],
    },
    FmDaemonInfo { short: "cosmic-files", quit_flag: "", daemon_flags: &["--daemon"] },
    // Pantheon Files: short name = binary name (the process is recognized by it);
    // `--quit` is the program's own switch (src/Application.vala), D-Bus brings it up
    // as a service with `--gapplication-service`.
    FmDaemonInfo {
        short: "io.elementary.files",
        quit_flag: "--quit",
        daemon_flags: &["--gapplication-service"],
    },
    // Deepin File Manager: no quit switch of its own; the windowless service is `-d`
    // (`backendOption`, src/apps/dde-file-manager/commandparser.cpp).
    FmDaemonInfo { short: "dde-file-manager", quit_flag: "", daemon_flags: &["-d"] },
];

/// A table record by short name.
pub fn fm_daemon_info(short: &str) -> Option<&'static FmDaemonInfo> {
    let lower = short.to_lowercase();
    FM_DAEMONS.iter().find(|d| d.short == lower)
}

/// The last path segment (`/usr/bin/Thunar` → `Thunar`).
fn path_basename(value: &str) -> &str {
    value.rsplit('/').next().unwrap_or(value)
}

/// Identify a file manager by how the process named itself: the bus `Comm`
/// (truncated by the kernel to 15 characters) and the full `Exe` path.
///
/// ⚠️ Case matters here: Thunar's `/proc/<pid>/comm` is `Thunar` with
/// a capital letter, while the short names in [`FM_SHORT_NAMES`] are lowercase. The old
/// owner check compared them as is and never fired on Thunar
/// — exactly the defect that let the foreign daemon live.
pub fn fm_short_from_process(comm: &str, exe: &str) -> Option<String> {
    for candidate in [path_basename(exe), comm] {
        let name = candidate.trim().to_lowercase();
        if name.is_empty() {
            continue;
        }
        if let Some(hit) = FM_SHORT_NAMES.iter().find(|s| **s == name) {
            return Some((*hit).to_string());
        }
    }
    None
}

/// What the bus said about the name's owner — parsing `busctl status`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Fm1OwnerFacts {
    pub pid: i64,
    /// `Comm=` (truncated by the kernel to 15 characters).
    pub comm: String,
    /// `Exe=` — the full binary path.
    pub exe: String,
    /// The whole `CommandLine=`, as the bus gave it.
    pub cmdline: String,
}

impl Fm1OwnerFacts {
    /// How to name the owner to the person.
    pub fn display(&self) -> String {
        if !self.exe.is_empty() {
            return path_basename(&self.exe).to_string();
        }
        if !self.comm.is_empty() {
            return self.comm.clone();
        }
        format!("pid {}", self.pid)
    }
}

/// Parsing the output of `busctl --user status <name>`: `PID`, `Comm`, `Exe`,
/// `CommandLine`. `None` — no owner in the output (nobody holds the name).
///
/// Replaces the old [`busctl_pid`]: one PID is not enough — by it one can neither
/// identify the manager nor tell service mode from open windows.
pub fn busctl_owner(stdout: &str) -> Option<Fm1OwnerFacts> {
    let mut facts = Fm1OwnerFacts::default();
    for line in stdout.split('\n') {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "PID" => {
                let digits: String =
                    value.chars().take_while(|c| c.is_ascii_digit()).collect();
                facts.pid = digits.parse().unwrap_or(0);
            }
            "Comm" => facts.comm = value.to_string(),
            "Exe" => facts.exe = value.to_string(),
            "CommandLine" => facts.cmdline = value.to_string(),
            _ => {}
        }
    }
    (facts.pid > 0).then_some(facts)
}

/// The name's owner together with the window observation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fm1Owner {
    pub facts: Fm1OwnerFacts,
    /// Whether the process has windows on screen. `None` — could NOT find out
    /// (a Wayland session: a window list with PIDs cannot be obtained there). Then the decision
    /// is made by the command line, and by default — in the person's favor.
    pub has_windows: Option<bool>,
}

/// The verdict on the `org.freedesktop.FileManager1` slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fm1Verdict {
    /// Nobody holds the name — D-Bus activation will give it out, i.e. our override.
    Free,
    /// The name is held by the very manager assigned as the default. Nothing to do.
    Owned { short: String, pid: i64 },
    /// The name is held by something that is not a file manager (or one unknown to us) — none of our business.
    Unknown { process: String, pid: i64 },
    /// A foreign manager, but with OPEN WINDOWS: the person has work there, killing it
    /// is not allowed (rule #13 — foolproofing works both ways).
    Busy { short: String, pid: i64 },
    /// A foreign manager in service mode — we take the slot back.
    Evict {
        short: String,
        pid: i64,
        /// How to stop it the standard way; empty — the manager has no quit of its own.
        quit: Vec<String>,
    },
}

/// Whether the command line has a service-mode sign.
fn cmdline_is_daemon(cmdline: &str, short: &str) -> bool {
    let Some(info) = fm_daemon_info(short) else {
        return false;
    };
    cmdline
        .split_whitespace()
        .any(|token| info.daemon_flags.contains(&token))
}

/// argv of the standard quit: the owner's binary (not the same-named one from `PATH`) plus
/// the flag from the table. Empty — nothing to stop it with, a signal by PID remains.
pub fn fm_quit_argv(short: &str, exe: &str) -> Vec<String> {
    let Some(info) = fm_daemon_info(short) else {
        return Vec::new();
    };
    if info.quit_flag.is_empty() {
        return Vec::new();
    }
    let binary = if exe.is_empty() { short } else { exe };
    vec![binary.to_string(), info.quit_flag.to_string()]
}

/// The slot decision: compare the name's owner with the assigned default.
///
/// `desired_short` — the short name of the manager Rexon considers the
/// default for `inode/directory`.
///
/// ⛔ We stop ONLY service mode. The order of arguments: the window observation
/// is stronger than the command line, and when there is no observation at all, the command
/// line decides, and the absence of the service flag reads as "the person has a window open".
pub fn fm1_verdict(owner: Option<&Fm1Owner>, desired_short: &str) -> Fm1Verdict {
    let Some(owner) = owner else {
        return Fm1Verdict::Free;
    };
    let pid = owner.facts.pid;
    let Some(short) = fm_short_from_process(&owner.facts.comm, &owner.facts.exe) else {
        return Fm1Verdict::Unknown { process: owner.facts.display(), pid };
    };
    if short == desired_short.trim().to_lowercase() {
        return Fm1Verdict::Owned { short, pid };
    }
    let daemon = match owner.has_windows {
        Some(true) => false,
        Some(false) => true,
        None => cmdline_is_daemon(&owner.facts.cmdline, &short),
    };
    if !daemon {
        return Fm1Verdict::Busy { short, pid };
    }
    let quit = fm_quit_argv(&short, &owner.facts.exe);
    Fm1Verdict::Evict { short, pid, quit }
}

// ── Saved KDE session ────────────────────────────────────────────────────

/// Path of the saved KDE session file relative to `$XDG_CONFIG_HOME`.
pub const KSMSERVERRC_NAME: &str = "ksmserverrc";

/// A key and its ordinal number: `restartCommand12=…` → `("restartCommand", "12")`.
fn ksmserver_key_index(line: &str) -> Option<(&str, &str)> {
    let key = line.split_once('=')?.0;
    let digits_at = key.len() - key.chars().rev().take_while(|c| c.is_ascii_digit()).count();
    if digits_at == key.len() || digits_at == 0 {
        return None;
    }
    Some((&key[..digits_at], &key[digits_at..]))
}

/// Group name from a `[Name]` line.
fn ini_group(line: &str) -> Option<&str> {
    let t = line.trim();
    t.strip_prefix('[')?.strip_suffix(']')
}

/// A saved session group (`[Session: …]`, `[LegacySession: …]`).
fn is_session_group(group: &str) -> bool {
    group.starts_with("Session:") || group.starts_with("LegacySession:")
}

/// Remove a manager from the saved KDE session: erase its record from
/// `[Session: …]` and add the name to `excludeApps` of the `[General]` group.
///
/// WHY BOTH ACTIONS. KDE brings a stopped daemon back at the next
/// login — it is recorded in the saved session together with its launch command
/// (`Thunar,--sm-client-id,…,--daemon`). Erasing the record is not enough: the session
/// is saved again at logout, and the daemon comes back. `excludeApps` is exactly the
/// standard KDE list by which the session manager neither restores nor
/// saves an application.
///
/// The record number is NOT renumbered, and `count` stays the same: KDE
/// skips a record with an empty `program`, while renumbering would shift other
/// lines — an extra risk in someone else's file.
///
/// `None` — nothing to change (no record and the name is already excluded).
pub fn ksmserver_forget(text: &str, short: &str) -> Option<String> {
    let short_l = short.trim().to_lowercase();
    if short_l.is_empty() {
        return None;
    }

    // 1. Find the record numbers of this manager in the saved session groups.
    let mut doomed: Vec<(String, String)> = Vec::new();
    let mut group = String::new();
    let mut has_general = false;
    let mut has_exclude = false;
    for line in text.lines() {
        if let Some(g) = ini_group(line) {
            group = g.to_string();
            has_general |= group == "General";
            continue;
        }
        if group == "General" && line.trim_start().starts_with("excludeApps=") {
            has_exclude = true;
        }
        if !is_session_group(&group) {
            continue;
        }
        let Some((key, index)) = ksmserver_key_index(line) else {
            continue;
        };
        if key != "program" {
            continue;
        }
        let value = line.split_once('=').map(|(_, v)| v).unwrap_or("");
        if path_basename(value.trim()).to_lowercase() == short_l {
            doomed.push((group.clone(), index.to_string()));
        }
    }

    // 2. Rewrite: drop the lines of doomed records, extend excludeApps.
    let mut out: Vec<String> = Vec::new();
    let mut group = String::new();
    let mut changed = false;
    for line in text.lines() {
        if let Some(g) = ini_group(line) {
            group = g.to_string();
            out.push(line.to_string());
            if group == "General" && !has_exclude {
                out.push(format!("excludeApps={short_l}"));
                changed = true;
            }
            continue;
        }
        if group == "General" {
            if let Some(rest) = line.trim_start().strip_prefix("excludeApps=") {
                let mut apps: Vec<String> = rest
                    .split(',')
                    .map(|s| s.trim().to_lowercase())
                    .filter(|s| !s.is_empty())
                    .collect();
                if !apps.contains(&short_l) {
                    apps.push(short_l.clone());
                    changed = true;
                }
                out.push(format!("excludeApps={}", apps.join(",")));
                continue;
            }
        }
        if is_session_group(&group) {
            if let Some((_, index)) = ksmserver_key_index(line) {
                if doomed.iter().any(|(g, i)| *g == group && i == index) {
                    changed = true;
                    continue;
                }
            }
        }
        out.push(line.to_string());
    }

    if !has_general {
        let mut head = vec!["[General]".to_string(), format!("excludeApps={short_l}"), String::new()];
        head.extend(out);
        out = head;
        changed = true;
    }

    if !changed {
        return None;
    }
    let mut text_out = out.join("\n");
    text_out.push('\n');
    Some(text_out)
}
