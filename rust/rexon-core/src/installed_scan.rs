//! Scanner of ACTUALLY installed appearance themes — port of
//! `rexon/appearance/installed_scan.py`.
//!
//! The hot module of the local categories of the «Appearance» tab (the
//! «Active | Downloaded | System» sub-switch + the «Application Style» /
//! «Kvantum» / «Splash» / «Lock Screen» sub-choices). Returns `Vec<ThemeItem>`
//! with the category_id of the real OCS category — the «Apply» button reuses
//! installer.apply() as is.
//!
//! Port principle (as in system.rs): THE FILE SYSTEM IS BEHIND AN INTERFACE.
//! Scanner input is `FsSnapshot`: a snapshot of the directory tree (name order
//! = the os.scandir order the Python reference saw) + the content of metadata
//! files. Parsing configs (metadata.json / metadata.desktop / index.theme /
//! *.colors / gtk settings.ini / kvantum.kvconfig / sddm-Current /
//! plymouth-Theme / pacman -Qm/-Qo output) is pure functions.
//!
//! Python's side dependencies are moved into parameters:
//!   * `_active_map()` (ThemeManager + kreadconfig6 + configs) — the map is
//!     passed ready-made; building it from READY source values is the pure
//!     `active_map_from_sources` (checked against Python on mocked sources);
//!   * pacman: `has_pacman` — a bool, `pacman -Qm` — `foreign` (None =
//!     unavailable → `ScanUnavailable`, like SystemScanUnavailable),
//!     `pacman -Qo` — the owner map (the `parse_qo_output` parser is pure);
//!   * the store cache (`appearance.cache`) — a list of (name, preview_url) +
//!     the `preview_file` closure (get_preview: url → local file);
//!   * `QStyleFactory.keys()` — the style list as a parameter (fallback —
//!     `WIDGET_STYLES`).
//!
//! NOT ported (execution — when hooked to live Python): calls to
//! pacman/kreadconfig6/QStyleFactory, ThemeManager (reading kdeglobals/kwinrc),
//! retries of `_foreign_pkgs` on db.lck (the retry policy is ported as the
//! pure function `foreign_pkgs_from_attempts`, the sleep/subprocess itself is
//! not), batching `_owner_pkgs` by 200 paths (the parser is pure, splitting
//! into batches is a call detail), the SQL preview cache, logging.
//!
//! Golden parity: `tests/installed_scan_parity.rs` — a snapshot of the real
//! theme tree of Wehrwolfmann's machine (+synthetic cases: broken metadata, a
//! theme without Name, a cursor theme in icons) and references from live
//! Python on the same sandbox; theme lists are compared FIELD BY FIELD
//! including order.

use std::collections::{HashMap, HashSet};

use serde_json::{Map, Value};

use crate::appearance_page::local_active_badge;
use crate::i18n::{n_, tr, trc};
use crate::models::ThemeItem;

// ============================================================================
// FsSnapshot — a snapshot of the file system
// ============================================================================

/// Tree snapshot: directory → entry names (in the snapshot's os.scandir order),
/// file → text content (binary previews in the fixtures are dummies).
pub struct FsSnapshot {
    dirs: HashMap<String, Vec<String>>,
    files: HashMap<String, String>,
}

impl FsSnapshot {
    /// From the JSON fixture `{"dirs": {path: [names]}, "files": {path: text}}`.
    pub fn from_json(v: &Value) -> Self {
        let mut dirs = HashMap::new();
        if let Some(obj) = v["dirs"].as_object() {
            for (k, names) in obj {
                let list = names
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|n| n.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();
                dirs.insert(k.clone(), list);
            }
        }
        let mut files = HashMap::new();
        if let Some(obj) = v["files"].as_object() {
            for (k, text) in obj {
                files.insert(k.clone(), text.as_str().unwrap_or("").to_string());
            }
        }
        FsSnapshot { dirs, files }
    }

    pub fn is_dir(&self, path: &str) -> bool {
        self.dirs.contains_key(path)
    }

    pub fn is_file(&self, path: &str) -> bool {
        self.files.contains_key(path)
    }

    pub fn read_text(&self, path: &str) -> Option<&str> {
        self.files.get(path).map(String::as_str)
    }

    /// Directory entry names in snapshot order ([] — no such directory).
    pub fn scandir(&self, path: &str) -> &[String] {
        self.dirs.get(path).map(Vec::as_slice).unwrap_or(&[])
    }
}

fn join(dir: &str, rel: &str) -> String {
    format!("{dir}/{rel}")
}

// ============================================================================
// Type table — port of _Type / TYPES
// ============================================================================

/// apply_kind → (cat_id, subfolder, enumeration method) — port of `_Type`.
pub struct ScanType {
    pub apply_kind: &'static str,
    pub cat_id: i64,
    pub label: &'static str,
    pub icon: &'static str,
    pub user_dir: String,
    pub sys_dir: String,
    /// "dir" | "colors" | "sub" (a theme directory with a `probe` subdirectory) |
    /// "file" (a theme directory with a `probe` file at its root) |
    /// "files" (files ending in `probe`) | "meta" (a directory with
    /// metadata) | "conky" (a Conky widget directory).
    pub entry: &'static str,
    /// Refinement for `entry` (subdirectory name, file suffix); usually empty.
    pub probe: &'static str,
}

/// Name of the OCS preview Rexon puts next to a theme when installing from
/// the Store — port of `REXON_PREVIEW_NAME`.
pub const REXON_PREVIEW_NAME: &str = ".rexon-preview.png";

/// Qt style engines (fallback without QStyleFactory) — port of `_WIDGET_STYLES`.
pub const WIDGET_STYLES: &[&str] = &[
    "Breeze", "Fusion", "kvantum", "kvantum-dark", "Windows", "Oxygen",
];

/// Available Qt styles — port of `_qstyle_factory`: the live list
/// `QStyleFactory::keys()` (built-in styles plus installed plugins), and if Qt
/// answered nothing — the fallback [`WIDGET_STYLES`].
///
/// The live list is mandatory: the fallback showed style chips that weren't on
/// the system (Oxygen without its plugin) and didn't show installed ones
/// (qt6ct-style and any third-party ones).
pub fn available_widget_styles(live_keys: &[String]) -> Vec<String> {
    let live: Vec<String> = live_keys
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
    if live.is_empty() {
        WIDGET_STYLES.iter().map(|s| s.to_string()).collect()
    } else {
        live
    }
}

/// The TYPES table for the given home/usr root (live: usr_root = "/usr").
pub fn types_for(home: &str, usr_root: &str) -> Vec<ScanType> {
    let t = |apply_kind, cat_id, label, icon, user: String, sys: String, entry| {
        ScanType { apply_kind, cat_id, label, icon, user_dir: user, sys_dir: sys, entry, probe: "" }
    };
    vec![
        t("global", 722, "Global Theme", "💎",
          format!("{home}/.local/share/plasma/look-and-feel"),
          format!("{usr_root}/share/plasma/look-and-feel"), "dir"),
        t("plasma", 104, "Plasma Style", "🖥️",
          format!("{home}/.local/share/plasma/desktoptheme"),
          format!("{usr_root}/share/plasma/desktoptheme"), "dir"),
        t("colors", 112, "Color Scheme", "🌈",
          format!("{home}/.local/share/color-schemes"),
          format!("{usr_root}/share/color-schemes"), "colors"),
        t("aurorae", 114, "Window Decorations (Aurorae)", "🪟",
          format!("{home}/.local/share/aurorae/themes"),
          format!("{usr_root}/share/aurorae/themes"), "dir"),
        t("icons", 132, "Icons", "📁",
          format!("{home}/.local/share/icons"),
          format!("{usr_root}/share/icons"), "dir"),
        t("cursor", 107, "Cursor", "🖱️",
          format!("{home}/.local/share/icons"),
          format!("{usr_root}/share/icons"), "dir"),
        t("kvantum", 123, "Kvantum", "🔲",
          format!("{home}/.config/Kvantum"),
          format!("{usr_root}/share/Kvantum"), "dir"),
        t("gtk", 135, "GTK Theme", "🎨",
          format!("{home}/.local/share/themes"),
          format!("{usr_root}/share/themes"), "dir"),
        // Sound themes: without this line they got into neither «Active», nor
        // «Downloaded», nor «System» (real case 03.10.2026).
        t("sounds", 316, "Sound Theme", "🔔",
          format!("{home}/.local/share/sounds"),
          format!("{usr_root}/share/sounds"), "dir"),
        t("sddm", 101, "SDDM (Login Screen)", "🔐",
          String::new(), format!("{usr_root}/share/sddm/themes"), "dir"),
        t("plymouth", 108, "Plymouth", "⚡",
          String::new(), format!("{usr_root}/share/plymouth/themes"), "dir"),
    ]
}

/// Types of the desktop environment sections (`categories::DE_SECTIONS`)
/// visible in `de` (empty — all). Directories, enumeration method and label
/// come from the matrix and the type table: there is no new list here (rule #12).
///
/// Window border and shell themes (a subdirectory in the theme directory) are
/// looked up both in `~/.themes` and in `~/.local/share/themes` — the
/// environments themselves read both directories, and installing from
/// «Appearance» puts GTK themes into the second.
pub fn de_types(home: &str, usr_root: &str, de: &str) -> Vec<ScanType> {
    use crate::de_customization::Payload;
    let mut out = Vec::new();
    for (cat_id, key) in crate::categories::DE_SECTIONS {
        if !de.is_empty() && !crate::categories::category_available(key, de) {
            continue;
        }
        let Some(kind) = crate::categories::category_kind(key) else { continue };
        let Some(row) = crate::de_customization::row(kind) else { continue };
        let (entry, probe) = match row.payload {
            Payload::ThemeSubdir { sub } => ("sub", sub),
            Payload::Files { ext } => ("files", ext),
            Payload::DirUuid { .. } | Payload::KPackage { .. } => ("meta", ""),
            Payload::Dir if kind == crate::de_apply::CONKY_KIND => ("conky", ""),
            // An LXQt theme is a directory with `lxqt-panel.qss`: without it LXQt
            // doesn't show it in its list. The sign is the same one the install
            // uses to find theme roots in an archive (one source).
            Payload::Dir => match crate::appearance_installer::de_theme_marker(key) {
                Some(crate::appearance_installer::DeThemeMarker::File(file)) => ("file", file),
                _ => ("dir", ""),
            },
        };
        let label = crate::constants::type_labels_ordered()
            .iter()
            .find(|(k, _)| *k == kind)
            .map(|(_, l)| *l)
            .unwrap_or(kind);
        let icon = crate::constants::type_icon(kind).unwrap_or("🧩");
        let base = ScanType {
            apply_kind: key,
            cat_id: *cat_id,
            label,
            icon,
            user_dir: row.dest_path(home),
            sys_dir: crate::de_apply::system_dir(row, usr_root),
            entry,
            probe,
        };
        let xdg_themes = (entry == "sub").then(|| ScanType {
            user_dir: format!("{home}/.local/share/themes"),
            sys_dir: String::new(),
            ..base_clone(&base)
        });
        out.push(base);
        out.extend(xdg_themes);
    }
    out
}

/// Types of one section for [`LiveScan::section`]: a desktop environment
/// section — from [`de_types`], a KDE section — the [`types_for`] row plus the
/// old directories read by both KDE and GTK: `~/.icons` for icons and cursors,
/// `~/.themes` for GTK themes (the «Downloaded» overview doesn't look there,
/// while themes installed by hand or by other installers often lie exactly there).
pub fn section_types(home: &str, usr_root: &str, apply_kind: &str) -> Vec<ScanType> {
    let mut types: Vec<ScanType> = types_for(home, usr_root)
        .into_iter()
        .chain(de_types(home, usr_root, ""))
        .filter(|t| t.apply_kind == apply_kind)
        .collect();
    let legacy = match apply_kind {
        "icons" | "cursor" => Some(format!("{home}/.icons")),
        "gtk" => Some(format!("{home}/.themes")),
        _ => None,
    };
    if let (Some(dir), Some(first)) = (legacy, types.first()) {
        let extra = ScanType { user_dir: dir, sys_dir: String::new(), ..base_clone(first) };
        types.push(extra);
    }
    types
}

/// A stamp of the directories the disk overviews read: the modification time
/// of each root (theme directories of all kinds and environments, `~/.themes`,
/// `~/.icons`, Kvantum). A new or removed theme in a root changes the root's
/// own time — the stamp differs, and the page's scan cache must be dropped:
/// otherwise a theme placed bypassing Rexon while the window was open didn't
/// show up on re-entering the section, only after «Refresh».
pub fn roots_stamp(home: &str, usr_root: &str) -> String {
    let mut roots: Vec<String> = Vec::new();
    let mut add = |d: &str| {
        if !d.is_empty() && !roots.iter().any(|r| r == d) {
            roots.push(d.to_string());
        }
    };
    for t in types_for(home, usr_root).iter().chain(de_types(home, usr_root, "").iter()) {
        add(&t.user_dir);
        add(&t.sys_dir);
    }
    for d in kvantum_dirs(home, usr_root) {
        add(&d);
    }
    add(&legacy_themes_dir(home));
    add(&format!("{home}/.icons"));
    roots
        .iter()
        .map(|d| {
            std::fs::metadata(d)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or_else(|| "-".to_string(), |t| t.as_nanos().to_string())
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn base_clone(t: &ScanType) -> ScanType {
    ScanType {
        apply_kind: t.apply_kind,
        cat_id: t.cat_id,
        label: t.label,
        icon: t.icon,
        user_dir: t.user_dir.clone(),
        sys_dir: t.sys_dir.clone(),
        entry: t.entry,
        probe: t.probe,
    }
}

// ── Links in `~/.themes` to themes from `~/.local/share/themes` ─────────
//
// «Appearance» installs GTK themes into `~/.local/share/themes` (section `gtk`,
// `categories`). It is read by GTK 3 and 4, xfwm4 and the XFCE appearance
// dialog (libxfce4util), Cinnamon and its settings, Openbox, labwc, GNOME
// User Themes and Marco 1.29 — but the MATE «Appearance» dialog
// (mate-control-center 1.28, `capplets/common/mate-theme-info.c`) looks only
// at `~/.themes` and `$XDG_DATA_DIRS/themes`: such a theme isn't in its list,
// and it can't be chosen there (test rig 16.10.2026). So every theme from
// there has a link with the same name in `~/.themes`. The link is relative —
// it survives moving the home directory. A taken name is left alone, a link
// to a vanished theme is removed; the link isn't shown in Rexon's lists —
// the theme is visible once, from its real directory.

/// The directory where «Appearance» installs GTK themes.
pub fn xdg_themes_dir(home: &str) -> String {
    format!("{}/.local/share/themes", home.trim_end_matches('/'))
}

/// The old theme directory that all environments read.
pub fn legacy_themes_dir(home: &str) -> String {
    format!("{}/.themes", home.trim_end_matches('/'))
}

/// Target of our link `~/.themes/<name>` — relative to `~/.themes` itself.
fn theme_link_target(name: &str) -> std::path::PathBuf {
    std::path::Path::new("../.local/share/themes").join(name)
}

/// Whether the link `~/.themes/<name>` is ours: it points exactly to
/// `~/.local/share/themes/<name>` (see [`sync_theme_links`]).
pub fn is_theme_link(home: &str, name: &str) -> bool {
    std::fs::read_link(join(&legacy_themes_dir(home), name))
        .is_ok_and(|t| t == theme_link_target(name))
}

/// Whether a theme lies in the directory: `index.theme`, GTK 2/3/4 styles or a
/// window border/shell subdirectory from the matrix (`metacity-1`, `xfwm4`, `cinnamon`, …).
fn looks_like_theme(dir: &std::path::Path) -> bool {
    use crate::de_customization::{Payload, ROWS};
    let subs = ROWS.iter().filter_map(|r| match r.payload {
        Payload::ThemeSubdir { sub } => Some(sub),
        _ => None,
    });
    dir.join("index.theme").is_file()
        || ["gtk-2.0"].into_iter().chain(GTK_PARTS.iter().copied()).chain(subs).any(|s| dir.join(s).is_dir())
}

/// Bring the `~/.themes` links in line with `~/.local/share/themes`: add the
/// missing ones, remove the ones pointing into the void. Returns lines for
/// the log (empty — there was nothing to do).
pub fn sync_theme_links(home: &str) -> Vec<String> {
    let xdg = xdg_themes_dir(home);
    let legacy = legacy_themes_dir(home);
    let mut log = Vec::new();
    for name in scandir_names(std::path::Path::new(&legacy)) {
        if is_theme_link(home, &name)
            && !std::path::Path::new(&join(&xdg, &name)).is_dir()
            && std::fs::remove_file(join(&legacy, &name)).is_ok()
        {
            log.push(format!("снята ссылка ~/.themes/{name}: темы больше нет"));
        }
    }
    let mut names = scandir_names(std::path::Path::new(&xdg));
    names.sort();
    for name in names {
        let src = std::path::PathBuf::from(join(&xdg, &name));
        if name.starts_with('.') || !src.is_dir() || !looks_like_theme(&src) {
            continue;
        }
        let dst = join(&legacy, &name);
        if std::fs::symlink_metadata(&dst).is_ok() {
            continue; // the name is taken: by an own theme, a link or anything else
        }
        let made = std::fs::create_dir_all(&legacy)
            .and_then(|()| std::os::unix::fs::symlink(theme_link_target(&name), &dst));
        match made {
            Ok(()) => log.push(format!("тема {name} видна в ~/.themes (ссылка)")),
            Err(e) => log.push(format!("не удалось поставить ссылку ~/.themes/{name}: {e}")),
        }
    }
    log
}

/// Remove our link `~/.themes/<name>` following the removed theme `removed`
/// (a path in `~/.local/share/themes`). Not our link or a foreign path — nothing.
pub fn drop_theme_link(home: &str, removed: &str) -> bool {
    let Some((dir, name)) = removed.trim_end_matches('/').rsplit_once('/') else { return false };
    dir == xdg_themes_dir(home)
        && is_theme_link(home, name)
        && std::fs::remove_file(join(&legacy_themes_dir(home), name)).is_ok()
}

/// Names of objects of a type in a directory: for "files" — files with the
/// right suffix (the full name, as the environment knows it), otherwise [`list_entries`].
pub fn entries_of(fs: &FsSnapshot, directory: &str, t: &ScanType) -> Vec<String> {
    if t.entry != "files" {
        return list_entries(fs, directory, t.entry);
    }
    if directory.is_empty() || !fs.is_dir(directory) {
        return Vec::new();
    }
    fs.scandir(directory)
        .iter()
        .filter(|n| n.ends_with(t.probe) && fs.is_file(&join(directory, n)))
        .cloned()
        .collect()
}

/// Names of the enabled kind from the active map: themes have one, add-ons and
/// Conky widgets — any number (separated by newlines in the map).
pub fn active_names<'m>(
    active_map: &'m HashMap<String, String>,
    apply_kind: &str,
) -> Vec<&'m str> {
    active_map
        .get(apply_kind)
        .map(|v| v.split('\n').filter(|n| !n.is_empty()).collect())
        .unwrap_or_default()
}

fn is_active(active_map: &HashMap<String, String>, apply_kind: &str, name: &str) -> bool {
    active_names(active_map, apply_kind).contains(&name)
}

/// Kvantum theme directories (user + system) — port of `_kvantum_dirs`.
pub fn kvantum_dirs(home: &str, usr_root: &str) -> [String; 2] {
    kvantum_dirs_xdg(None, home, usr_root)
}

/// The same, honouring `$XDG_CONFIG_HOME`.
pub fn kvantum_dirs_xdg(
    xdg_config_home: Option<&str>,
    home: &str,
    usr_root: &str,
) -> [String; 2] {
    [
        format!("{}/Kvantum", crate::utils::config_home(xdg_config_home, home)),
        format!("{usr_root}/share/Kvantum"),
    ]
}

/// look-and-feel directories (user + system) — port of `_lnf_dirs`.
pub fn lnf_dirs(home: &str, usr_root: &str) -> [String; 2] {
    [
        format!("{home}/.local/share/plasma/look-and-feel"),
        format!("{usr_root}/share/plasma/look-and-feel"),
    ]
}

// ============================================================================
// Sidecar preview — port of _sidecar_key / sidecar_preview_path
// ============================================================================

/// Safe file name of a sidecar preview — port of `_sidecar_key` (Unicode
/// letters stay: Python `str.isalnum`).
pub fn sidecar_key(apply_kind: &str, name: &str) -> String {
    let safe: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("{apply_kind}__{safe}.png")
}

/// Store picture directory for themes without their own directory (color
/// schemes) — in the «Appearance» cache ([`crate::appearance_cache::cache_dir`]).
pub fn sidecar_preview_dir(home: &str) -> String {
    format!("{}/local_previews", crate::appearance_cache::cache_dir(home))
}

/// Absolute path of a sidecar preview — port of `sidecar_preview_path`.
pub fn sidecar_preview_path(home: &str, apply_kind: &str, name: &str) -> String {
    format!("{}/{}", sidecar_preview_dir(home), sidecar_key(apply_kind, name))
}

// ============================================================================
// Enumerating and validating entries — port of _list_entries / _is_valid_lnf /
// _entry_valid / _is_cursor_dir
// ============================================================================

/// Theme names in a directory — port of `_list_entries`. entry="colors" → the
/// *.colors files (without suffix), otherwise subfolders. Quiet about a missing directory.
pub fn list_entries(fs: &FsSnapshot, directory: &str, entry: &str) -> Vec<String> {
    let mut out = Vec::new();
    if directory.is_empty() || !fs.is_dir(directory) {
        return out;
    }
    for name in fs.scandir(directory) {
        let full = join(directory, name);
        if entry == "colors" {
            if fs.is_file(&full) {
                if let Some(stem) = name.strip_suffix(".colors") {
                    out.push(stem.to_string());
                }
            }
        } else if fs.is_dir(&full) {
            out.push(name.clone());
        }
    }
    out
}

/// A look-and-feel is valid only with a meaningful Name in
/// metadata.json/.desktop — port of `_is_valid_lnf` (filters out junk
/// subdirectories without metadata).
///
/// Python nuance: a metadata.json with a NON-object at the top level (a
/// string/number) would crash the scan with AttributeError — here we treat it
/// as "not a theme" (false).
pub fn is_valid_lnf(fs: &FsSnapshot, theme_dir: &str) -> bool {
    let mj = join(theme_dir, "metadata.json");
    if fs.is_file(&mj) {
        let Some(text) = fs.read_text(&mj) else {
            return false;
        };
        let Ok(data) = serde_json::from_str::<Value>(text) else {
            return false; // broken json → not a theme
        };
        let name = data["KPlugin"]["Name"].as_str().unwrap_or("");
        return !name.trim().is_empty();
    }
    let md = join(theme_dir, "metadata.desktop");
    if fs.is_file(&md) {
        if let Some(text) = fs.read_text(&md) {
            for line in text.lines() {
                let s = line.trim();
                if let Some(v) = s.strip_prefix("Name=") {
                    if !v.trim().is_empty() {
                        return true;
                    }
                }
            }
        }
        return false;
    }
    false // neither metadata.json nor metadata.desktop → not a theme
}

/// Name keys for the user's language — from the most specific to the generic:
/// `ru_RU.UTF-8` → `["Name[ru_RU]", "Name[ru]", "Name"]`.
///
/// Both `metadata.json` and KDE's `metadata.desktop` mark a translation the
/// same way — with a suffix in square brackets using an UNDERSCORE
/// (`Name[pt_BR]`), so a hyphen in the locale is normalised to an underscore.
/// The `.UTF-8` tail and the `@valencia` modifier are dropped: keys don't
/// carry them.
pub fn locale_name_keys(locale: &str) -> Vec<String> {
    locale_keys("Name", locale)
}

/// The same rule for any key: `Comment`, `Description`.
pub(crate) fn locale_keys(base: &str, locale: &str) -> Vec<String> {
    let cleaned = locale
        .split(['.', '@'])
        .next()
        .unwrap_or("")
        .replace('-', "_");
    let cleaned = cleaned.trim();
    let mut keys = Vec::new();
    if !cleaned.is_empty() {
        keys.push(format!("{base}[{cleaned}]"));
        if let Some((lang, _region)) = cleaned.split_once('_') {
            if !lang.is_empty() {
                keys.push(format!("{base}[{lang}]"));
            }
        }
    }
    keys.push(base.to_string());
    keys
}

/// A name from an ini-like file by ALREADY computed language keys — the shared
/// parser for `metadata.desktop`, `index.theme` and `*.colors`.
///
/// The section is deliberately not asked for (rationale — at
/// [`theme_display_name`]). A live check on this machine: icons and cursors
/// keep the name in `[Icon Theme]`, GTK themes — sometimes in `[Desktop Entry]`
/// (WhiteSur, Dracula, Materia), sometimes in `[X-GNOME-Metatheme]` (Adwaita,
/// adw-gtk3), KDE color schemes — in `[General]`, the eleventh group. Tying it
/// to a group would lose every second kind of appearance.
///
/// The lookup order is by keys, not by file lines: a translation into the
/// user's language wins over the generic name even if it comes later.
fn name_from_ini(text: &str, keys: &[String]) -> Option<String> {
    for key in keys {
        let prefix = format!("{key}=");
        for line in text.lines() {
            if let Some(v) = line.trim().strip_prefix(&prefix) {
                if !v.trim().is_empty() {
                    return Some(v.trim().to_string());
                }
            }
        }
    }
    None
}

/// A human name from ONE ini file — for appearance kinds that have no theme
/// directory at all: a KDE color scheme is a `<name>.colors` file.
///
/// The language choice rule and the fallback are the same as in
/// [`theme_display_name`]: there are no second copies of the logic, both
/// doors lead to `locale_name_keys` + `name_from_ini`.
pub fn file_display_name(
    fs: &FsSnapshot,
    file_path: &str,
    fallback: &str,
    locale: &str,
) -> String {
    if file_path.is_empty() || !fs.is_file(file_path) {
        return fallback.to_string();
    }
    let keys = locale_name_keys(locale);
    fs.read_text(file_path)
        .and_then(|text| name_from_ini(text, &keys))
        .unwrap_or_else(|| fallback.to_string())
}

/// A theme's human name for the card: `metadata.json` → `KPlugin.Name`,
/// otherwise `metadata.desktop` → `Name=`, otherwise `index.theme` → `Name=`.
/// A translation into the user's language is preferred over the generic one
/// (`Name[ru]` before `Name`).
///
/// Three sources, not two, because KDE metadata is far from present for all
/// appearance kinds: icons, cursors and GTK themes have only `index.theme`
/// next to them. The order is from the most specific to the most generic:
/// `metadata.*` is written by the theme author for KDE, `index.theme` is
/// sometimes generated.
///
/// `fallback` is the directory name. It is also what is returned when there
/// is no metadata, the name is empty or the file is broken: a raw identifier
/// is better for the user than a card without a label. The card identifier
/// (`local:<kind>:<name>`), install paths and matching against the active
/// theme do NOT look at this name — it is only for display.
///
/// The file section is deliberately not asked for: a look-and-feel keeps its
/// name in `[Desktop Entry]`, SDDM themes — in `[SddmGreeterTheme]`, and tying
/// it to one group would lose half the themes. The first key match is taken,
/// and keys are tried by language — so a translation wins over the generic
/// name even if it comes later in the file.
pub fn theme_display_name(
    fs: &FsSnapshot,
    theme_dir: &str,
    fallback: &str,
    locale: &str,
) -> String {
    if theme_dir.is_empty() {
        return fallback.to_string();
    }
    let keys = locale_name_keys(locale);
    let mj = join(theme_dir, "metadata.json");
    if fs.is_file(&mj) {
        if let Some(Ok(data)) = fs.read_text(&mj).map(serde_json::from_str::<Value>) {
            for key in &keys {
                let from_kp = data
                    .get("KPlugin")
                    .and_then(Value::as_object)
                    .and_then(|kp| kp.get(key));
                if let Some(Value::String(v)) = from_kp.or_else(|| data.get(key)) {
                    if !v.trim().is_empty() {
                        return v.trim().to_string();
                    }
                }
            }
            // GNOME extensions and Cinnamon applets/desklets/extensions write the
            // name as lowercase `name` (untranslated): otherwise the card came out
            // with the uuid instead of the title (live checks GNOME 51.1 and
            // Cinnamon 6.6, 16.10.2026).
            if let Some(Value::String(v)) = data.get("name") {
                if !v.trim().is_empty() {
                    return v.trim().to_string();
                }
            }
        }
    }
    for path in [join(theme_dir, "metadata.desktop"), join(theme_dir, "index.theme"), plymouth_desc_path(theme_dir)] {
        if !fs.is_file(&path) {
            continue;
        }
        if let Some(name) = fs.read_text(&path).and_then(|t| name_from_ini(t, &keys)) {
            return name;
        }
    }
    fallback.to_string()
}

/// Description of a boot splash theme — `<dir>/<dir name>.plymouth` (group
/// `[Plymouth Theme]`: `Name=`, `Description=`, with translations). The
/// plymouth kcm reads the same file for the tile label. A directory of another
/// kind has no such file, and the probe simply misses.
fn plymouth_desc_path(theme_dir: &str) -> String {
    let dir = theme_dir.trim_end_matches('/');
    let name = dir.rsplit('/').next().unwrap_or("");
    join(dir, &format!("{name}.plymouth"))
}

/// The theme name without translation — for searching by the original title.
///
/// The same sources and the same order as in [`theme_display_name`], only the
/// language isn't asked: the bare `Name=` (`KPlugin.Name`) is taken. Empty
/// when it matches the displayed name — searching by it already happens.
pub fn theme_source_name(fs: &FsSnapshot, theme_dir: &str, fallback: &str, shown: &str) -> String {
    unless_shown(theme_display_name(fs, theme_dir, fallback, ""), shown)
}

/// The same for a file theme (`*.colors`) — the pair to [`file_display_name`].
pub fn file_source_name(fs: &FsSnapshot, file_path: &str, fallback: &str, shown: &str) -> String {
    unless_shown(file_display_name(fs, file_path, fallback, ""), shown)
}

fn unless_shown(source: String, shown: &str) -> String {
    if source == shown {
        String::new()
    } else {
        source
    }
}

/// A theme's description for the card — the same three sources and the same
/// language rule as in [`theme_display_name`]: `metadata.json` →
/// `KPlugin.Description`, otherwise `Comment=` from `metadata.desktop`,
/// otherwise from `index.theme` (sound, icon, cursor and GTK themes keep the
/// description only there). None — empty: the card itself will say "No description".
pub fn theme_description(fs: &FsSnapshot, theme_dir: &str, locale: &str) -> String {
    if theme_dir.is_empty() {
        return String::new();
    }
    let mj = join(theme_dir, "metadata.json");
    if fs.is_file(&mj) {
        if let Some(Ok(data)) = fs.read_text(&mj).map(serde_json::from_str::<Value>) {
            for key in &locale_keys("Description", locale) {
                let from_kp = data
                    .get("KPlugin")
                    .and_then(Value::as_object)
                    .and_then(|kp| kp.get(key));
                if let Some(Value::String(v)) = from_kp.or_else(|| data.get(key)) {
                    if !v.trim().is_empty() {
                        return v.trim().to_string();
                    }
                }
            }
        }
    }
    let keys = locale_keys("Comment", locale);
    for rel in ["metadata.desktop", "index.theme"] {
        let path = join(theme_dir, rel);
        if !fs.is_file(&path) {
            continue;
        }
        if let Some(c) = fs.read_text(&path).and_then(|t| name_from_ini(t, &keys)) {
            return c;
        }
    }
    let ply = plymouth_desc_path(theme_dir);
    if fs.is_file(&ply) {
        let keys = locale_keys("Description", locale);
        if let Some(c) = fs.read_text(&ply).and_then(|t| name_from_ini(t, &keys)) {
            return c;
        }
    }
    String::new()
}

fn entry_valid(t: &ScanType, fs: &FsSnapshot, base_dir: &str, name: &str) -> bool {
    if t.apply_kind == "global" {
        return is_valid_lnf(fs, &join(base_dir, name));
    }
    match t.entry {
        "sub" => return fs.is_dir(&join(&join(base_dir, name), t.probe)),
        "file" => return fs.is_file(&join(&join(base_dir, name), t.probe)),
        "meta" => {
            let dir = join(base_dir, name);
            return fs.is_file(&join(&dir, "metadata.json"))
                || fs.is_file(&join(&dir, "metadata.desktop"));
        }
        "conky" => {
            // A config file at the top or a nested folder (the config is deeper):
            // the button does the final lookup (`de_apply::conky_conf`). The file
            // name can be anything (`Jhalak.conf`, `Gotham`) — then the widget
            // is recognised by its content (`conky_scan`).
            let dir = join(base_dir, name);
            return fs.scandir(&dir).iter().any(|f| {
                f == "conky.conf"
                    || f.ends_with(".conkyrc")
                    || (f.to_lowercase().contains("conky") && f.ends_with(".conf"))
                    || fs.is_dir(&join(&dir, f))
            }) || !crate::conky_scan::config_files(&crate::animations::SystemIo, &dir).is_empty();
        }
        _ => {}
    }
    // A boot splash theme is a directory with a `<name>.plymouth` description:
    // only such ones are seen by `plymouth-set-default-theme --list` and the
    // plymouth kcm. An empty directory remains after removing a theme package,
    // and there is nothing to enable (`system_plan::plymouth_installed_themes`).
    if t.apply_kind == "plymouth" {
        return fs.is_file(&join(&join(base_dir, name), &format!("{name}.plymouth")));
    }
    // A login screen theme is a directory with `metadata.desktop`: SDDM takes the
    // theme's main file from it, and the sddm kcm doesn't show the theme without it.
    if t.apply_kind == "sddm" {
        return fs.is_file(&join(&join(base_dir, name), "metadata.desktop"));
    }
    if t.apply_kind == "sounds" {
        // A sound theme is only a directory with `index.theme` and `[Sound Theme]`:
        // `alsa`, `speech-dispatcher`, GNOME sets without index.theme and archive
        // wrappers are not themes for the system and can't be applied.
        return fs
            .read_text(&join(&join(base_dir, name), "index.theme"))
            .map(|t| t.lines().any(|l| l.trim() == "[Sound Theme]"))
            .unwrap_or(false);
    }
    true
}

/// A cursor theme ~ the presence of cursors/ — port of `_is_cursor_dir`
/// (splits the shared icons/ between the 'icons' and 'cursor' types).
pub fn is_cursor_dir(fs: &FsSnapshot, path: &str) -> bool {
    fs.is_dir(&join(path, "cursors"))
}

/// A pure cursor theme: has `cursors/`, but no icon directories in
/// `index.theme`. An icon set with its own cursors (Adwaita, elementary — the
/// default XFCE icons) is NOT considered a cursor theme and is visible in
/// «Icons»: previously any `cursors/` sent the set only to «Cursors», and the
/// environment's standard icons weren't in the list (GNOME and XFCE checks,
/// 16.10.2026).
pub fn is_cursor_only(fs: &FsSnapshot, path: &str) -> bool {
    is_cursor_dir(fs, path)
        && !fs
            .read_text(&join(path, "index.theme"))
            .is_some_and(|text| icon_theme_info(text).has_dirs)
}

// ============================================================================
// Preview — port of _find_preview / _preview_from_metadata
// ============================================================================

/// A best-effort local theme preview — port of `_find_preview`.
/// prefer="splash"/"lockscreen" — a category-dependent priority of
/// contents/previews/<prefer>.png (after the .rexon-preview.png sidecar).
pub fn find_preview(fs: &FsSnapshot, theme_dir: &str, prefer: Option<&str>) -> String {
    find_preview_in(fs, theme_dir, prefer, &[])
}

/// Theme subdirectories in which its kind keeps its thumbnail
/// (`<subdir>/thumbnail.png` — that is how GTK, Cinnamon, Metacity border and
/// GNOME shell themes ship their picture).
pub fn preview_subdirs(t: &ScanType) -> Vec<&'static str> {
    match (t.apply_kind, t.entry) {
        ("gtk", _) => vec!["gtk-3.0", "gtk-4.0", "gtk-3.20", "gtk-2.0"],
        (_, "sub") => vec![t.probe],
        _ => Vec::new(),
    }
}

/// [`find_preview`] with the kind's thumbnails in subdirectories `subs`
/// ([`preview_subdirs`]).
pub fn find_preview_in(
    fs: &FsSnapshot,
    theme_dir: &str,
    prefer: Option<&str>,
    subs: &[&str],
) -> String {
    if !fs.is_dir(theme_dir) {
        return String::new();
    }
    let mut cands: Vec<String> = vec![REXON_PREVIEW_NAME.to_string()];
    if let Some(p) = prefer {
        if p == "splash" || p == "lockscreen" {
            cands.push(format!("contents/previews/{p}.png"));
        }
    }
    cands.extend(
        [
            "screenshot.png", "preview.png", "thumbnail.png",
            "fullscreenpreview.png",
            "contents/previews/preview.png",
            "contents/previews/fullscreenpreview.png",
            "contents/screenshot.png",
            "contents/previews/lockscreen.png",
            "contents/previews/splash.png",
            "metadata/preview.png",
        ]
        .iter()
        .map(|s| s.to_string()),
    );
    cands.extend(subs.iter().map(|sub| format!("{sub}/thumbnail.png")));
    for c in &cands {
        let p = join(theme_dir, c);
        if fs.is_file(&p) {
            return p;
        }
    }
    let mp = preview_from_metadata(fs, theme_dir);
    if !mp.is_empty() {
        return mp;
    }
    // any *.png/*.jpg at the root that looks like a preview (screen/preview/thumb/…)
    for name in fs.scandir(theme_dir) {
        let full = join(theme_dir, name);
        if !fs.is_file(&full) {
            continue;
        }
        let low = name.to_lowercase();
        if (low.ends_with(".png") || low.ends_with(".jpg"))
            && ["screen", "preview", "thumb", "snapshot"]
                .iter()
                .any(|k| low.contains(k))
        {
            return full;
        }
    }
    String::new()
}

/// The Preview/Image field from the metadata.json/.desktop of a KPackage theme —
/// port of `_preview_from_metadata` (path relative to contents/, then the root).
pub fn preview_from_metadata(fs: &FsSnapshot, theme_dir: &str) -> String {
    let mj = join(theme_dir, "metadata.json");
    if fs.is_file(&mj) {
        if let Some(Ok(data)) = fs
            .read_text(&mj)
            .map(serde_json::from_str::<Value>)
        {
            for key in ["Preview", "Image"] {
                let rel = data["KPlugin"][key]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .or_else(|| data[key].as_str().filter(|s| !s.is_empty()));
                if let Some(rel) = rel {
                    for base in [join(theme_dir, "contents"), theme_dir.to_string()] {
                        let p = join(&base, rel);
                        if fs.is_file(&p) {
                            return p;
                        }
                    }
                }
            }
        }
    }
    let md = join(theme_dir, "metadata.desktop");
    if fs.is_file(&md) {
        if let Some(text) = fs.read_text(&md) {
            for line in text.lines() {
                let s = line.trim();
                // `Screenshot=` — a screenshot of an SDDM login screen theme.
                let rel = s
                    .strip_prefix("Preview=")
                    .or_else(|| s.strip_prefix("Image="))
                    .or_else(|| s.strip_prefix("Screenshot="));
                if let Some(rel) = rel {
                    let rel = rel.trim();
                    for base in [join(theme_dir, "contents"), theme_dir.to_string()] {
                        let p = join(&base, rel);
                        if fs.is_file(&p) {
                            return p;
                        }
                    }
                }
            }
        }
    }
    String::new()
}

// ============================================================================
// pacman classification: pure parsers — port of _foreign_pkgs / _owner_pkgs
// ============================================================================

/// Result of one `pacman -Qm` attempt for the pure retry policy.
pub enum QmAttempt {
    /// returncode == 0: stdout.
    Ok(String),
    /// returncode != 0: stderr.
    Fail(String),
    /// OSError / SubprocessError.
    Error,
}

/// Parsing `pacman -Qm` stdout: names of foreign packages.
pub fn parse_qm_output(stdout: &str) -> HashSet<String> {
    stdout
        .lines()
        .filter(|ln| !ln.trim().is_empty())
        .filter_map(|ln| ln.split_whitespace().next())
        .map(String::from)
        .collect()
}

/// The `_foreign_pkgs` policy on READY attempts (subprocess/sleep — at the
/// executor): up to 2 attempts; Fail with "lck"/"lock" in stderr → retry, any
/// other error → stop; nothing worked → None (→ ScanUnavailable in scan_system).
/// Returns (result, how many attempts were used).
pub fn foreign_pkgs_from_attempts(
    has_pacman: bool,
    attempts: &[QmAttempt],
) -> (Option<HashSet<String>>, usize) {
    if !has_pacman {
        return (Some(HashSet::new()), 0); // outside Arch there is no notion of foreign
    }
    let mut used = 0;
    for attempt in attempts.iter().take(2) {
        used += 1;
        match attempt {
            QmAttempt::Error => continue,
            QmAttempt::Ok(stdout) => return (Some(parse_qm_output(stdout)), used),
            QmAttempt::Fail(stderr) => {
                let low = stderr.to_lowercase();
                if low.contains("lck") || low.contains("lock") {
                    continue; // db.lck → wait and retry
                }
                break; // another error → a retry won't help
            }
        }
    }
    (None, used)
}

/// Parsing `LANG=C pacman -Qo` stdout: "<path> is owned by <pkg> <ver>" →
/// path.rstrip('/') → pkg — port of the `_owner_pkgs` parser (regex `_QO_LINE`).
/// Lines without an owner/errors/garbage are ignored.
pub fn parse_qo_output(stdout: &str) -> HashMap<String, String> {
    let mut owners = HashMap::new();
    for raw in stdout.lines() {
        let line = raw.trim();
        // `^(.*\S)\s+is owned by\s+(\S+)\s+\S+\s*$` — a greedy path: we look for
        // the LAST occurrence of "is owned by" surrounded by spaces and followed by
        // exactly two tokens (pkg ver).
        let needle = "is owned by";
        let mut found: Option<(String, String)> = None;
        let mut idx = 0;
        while let Some(pos) = line[idx..].find(needle) {
            let at = idx + pos;
            idx = at + needle.len();
            let before = &line[..at];
            let after = &line[at + needle.len()..];
            if !before.ends_with(char::is_whitespace)
                || !after.starts_with(char::is_whitespace)
            {
                continue;
            }
            let path = before.trim_end();
            if path.is_empty() {
                continue;
            }
            let toks: Vec<&str> = after.split_whitespace().collect();
            if toks.len() == 2 {
                found = Some((
                    path.trim_end_matches('/').to_string(),
                    toks[0].to_string(),
                ));
            }
        }
        if let Some((path, pkg)) = found {
            owners.insert(path, pkg);
        }
    }
    owners
}

// ============================================================================
// Active themes: building the map from ready sources — port of _active_map
// ============================================================================

/// `_clean`: empty/"—" → "".
fn clean(v: &str) -> &str {
    let v = v.trim();
    if v.is_empty() || v == "—" {
        ""
    } else {
        v
    }
}

/// `_active_map` sources in READY form: ThemeManager/kreadconfig6 values
/// (kreadconfig6 fields — Some(stdout) when rc==0) and config contents
/// (None = no file). `sddm_confs` — contents of EXISTING files in SDDM's read
/// order (`system_plan::sddm_theme_files`); `plymouth_confs` — by increasing
/// weight ([`plymouth_confs_by_weight`]).
#[derive(Default)]
pub struct ActiveMapSources<'a> {
    pub kde_global: &'a str,
    pub kde_plasma_style: &'a str,
    pub kde_colors: &'a str,
    pub icons: &'a str,
    pub mouse_cursor: &'a str,
    pub kcminput_cursor: &'a str,
    pub xcursor_env: &'a str,
    pub gtk_settings_ini: Option<&'a str>,
    pub widget_style: &'a str,
    pub splash: Option<&'a str>,
    pub lockscreen: Option<&'a str>,
    pub kwin_decoration_theme: Option<&'a str>,
    /// `kwinrc [org.kde.kdecoration2] library` (None — not read).
    pub kwin_decoration_library: Option<&'a str>,
    pub kvantum_kvconfig: Option<&'a str>,
    pub sddm_confs: Vec<&'a str>,
    pub plymouth_confs: Vec<&'a str>,
    /// `$QT_STYLE_OVERRIDE` of the current session — Qt reads it before anything
    /// else, so we read it too.
    pub qt_style_override_env: &'a str,
    /// Content of `~/.config/environment.d/rexon-qt-style.conf` (None — no file).
    /// Variables from there reach the session only after logging in again, but
    /// the person has already made the choice: not showing it would be lying
    /// that "nothing happened".
    pub qt_style_env_file: Option<&'a str>,
    /// `~/.config/lxqt/lxqt.conf` — `[Qt] style` (LXQt).
    pub lxqt_conf: Option<&'a str>,
    /// `~/.config/qt6ct/qt6ct.conf` or qt5ct — `[Appearance] style`.
    pub qtct_conf: Option<&'a str>,
    /// GTK, icon and cursor themes from the environment's NATIVE storage
    /// (gsettings for GNOME/Cinnamon/MATE/Budgie/Pantheon/COSMIC, xfconf for XFCE,
    /// DConfig for Deepin — [`native_role_values`]). It is the same key applying
    /// writes them to, so the value from here wins over kdeglobals and
    /// `settings.ini`. None — the environment has no storage of its own (KDE,
    /// LXQt, tiling WMs) or the key couldn't be read: then the old chain.
    pub native_gtk: Option<&'a str>,
    pub native_icons: Option<&'a str>,
    pub native_cursor: Option<&'a str>,
    /// Answer of the command reading the current environment's sound theme
    /// (`qt_settings::sound_read_argv_for`, None — nothing to read with).
    pub sound_theme: Option<&'a str>,
    /// The current desktop wallpaper: a path or a `file://` URI (None — couldn't
    /// find out). The PACK NAME is taken from it — see [`wallpaper_pack_name`].
    pub wallpaper_current: Option<&'a str>,
    /// Home directory — needed to recognise the wallpaper pack by the picture's path.
    pub home: &'a str,
    /// The usr root (live: `/usr`) — the other half of the same task.
    pub usr_root: &'a str,
}

/// SDDM configs from which the active theme map takes `[Theme] Current=`.
///
/// `plasma_login` — the login screen is drawn by Plasma Login Manager
/// (`system_plan::is_plasma_login`): it has no themes, and no SDDM theme is
/// active, whatever remained in `/etc/sddm.conf.d` from the old SDDM.
pub fn sddm_confs_for_active_map(confs: Vec<&str>, plasma_login: bool) -> Vec<&str> {
    if plasma_login {
        Vec::new()
    } else {
        confs
    }
}

/// Directories inside which wallpaper packs lie (user and system).
pub fn wallpaper_roots(home: &str, usr_root: &str) -> [String; 2] {
    [
        format!("{home}/.local/share/wallpapers"),
        format!("{usr_root}/share/wallpapers"),
    ]
}

/// Decode `%XX` in a `file://` URI (gsettings returns the wallpaper exactly
/// like that, and a path with Cyrillic won't match any pack without decoding).
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let hex = std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("");
            if let Ok(v) = u8::from_str_radix(hex, 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Name of the wallpaper PACK in which the current wallpaper picture lies.
///
/// That is the answer to "is the pack applied right now": a card compares its
/// name with this value exactly as icons compare theirs with the active icon
/// theme — one mechanism for all kinds, without a separate crutch for
/// wallpapers.
///
/// The first path segment after the wallpaper directory is taken, so it
/// works equally for a pack (`…/wallpapers/Pack/contents/images/1920x1080.png`
/// → `Pack`) and for a single picture (`…/wallpapers/Mountains.png` →
/// `Mountains.png` — exactly the name it has in the journal). A wallpaper
/// outside the wallpaper directories (one's own file from «Pictures») doesn't
/// count as a pack — empty.
pub fn wallpaper_pack_name(current: &str, home: &str, usr_root: &str) -> String {
    let raw = current.trim().trim_matches(|c| c == '\'' || c == '"').trim();
    let path = match raw.strip_prefix("file://") {
        Some(rest) => percent_decode(rest),
        None => raw.to_string(),
    };
    for root in wallpaper_roots(home, usr_root) {
        let Some(rest) = path.strip_prefix(&format!("{root}/")) else {
            continue;
        };
        let seg = rest.split('/').next().unwrap_or("");
        if !seg.is_empty() {
            return seg.to_string();
        }
    }
    String::new()
}

/// Value of a variable from an `environment.d` file (`NAME=value`, comments
/// with `#` are skipped).
fn env_file_value(content: Option<&str>, name: &str) -> Option<String> {
    let text = content?;
    let prefix = format!("{name}=");
    for line in text.lines() {
        let s = line.trim();
        if s.starts_with('#') {
            continue;
        }
        if let Some(rest) = s.strip_prefix(&prefix) {
            let v = clean(rest.trim_matches('"'));
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

/// Value of an ini file key in the given section (`lxqt.conf`, `qt6ct.conf`).
fn ini_value(content: Option<&str>, section: &str, key: &str) -> Option<String> {
    let text = content?;
    let mut here = false;
    let prefix = format!("{key}=");
    for line in text.lines() {
        let s = line.trim();
        if s.starts_with('[') && s.ends_with(']') {
            here = &s[1..s.len() - 1] == section;
            continue;
        }
        if !here || s.starts_with('#') {
            continue;
        }
        if let Some(rest) = s.strip_prefix(&prefix) {
            let v = clean(rest);
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

/// GTK, icon and cursor themes from the native storage of environment `de` —
/// `[gtk, icons, cursor]` for [`ActiveMapSources::native_gtk`] and neighbours.
///
/// Commands — [`crate::themes::native_role_read_cmd`] (the same schema table
/// as applying uses); `read` runs a command and returns stdout on success
/// (test rigs substitute it). For an environment without its own storage
/// `read` is not called at all — KDE doesn't pay a single extra process.
pub fn native_role_values(
    de: &str,
    mut read: impl FnMut(&crate::themes::Cmd) -> Option<String>,
) -> [Option<String>; 3] {
    let mut out: [Option<String>; 3] = Default::default();
    for (slot, role) in out.iter_mut().zip(crate::themes::NATIVE_STORE_ROLES) {
        let Some(cmd) = crate::themes::native_role_read_cmd(de, role) else {
            continue;
        };
        *slot = read(&cmd)
            .map(|raw| crate::themes::parse_native_role_output(de, &raw))
            .filter(|v| !clean(v).is_empty());
    }
    out
}

/// apply_kind → active theme name — port of `_active_map` (key insertion
/// order follows the Python code; serialised like its dict).
pub fn active_map_from_sources(src: &ActiveMapSources) -> Map<String, Value> {
    let mut out = Map::new();
    let put = |m: &mut Map<String, Value>, k: &str, v: &str| {
        m.insert(k.to_string(), Value::from(v));
    };
    if !clean(src.kde_global).is_empty() {
        put(&mut out, "global", clean(src.kde_global));
    }
    if !clean(src.kde_plasma_style).is_empty() {
        put(&mut out, "plasma", clean(src.kde_plasma_style));
    }
    if !clean(src.kde_colors).is_empty() {
        put(&mut out, "colors", clean(src.kde_colors));
    }
    // The value from the environment's native storage (see `native_*`) — first.
    // Previously outside KDE icons and cursor were taken only from kdeglobals,
    // and in MATE themes applied via gsettings didn't count as active: removal
    // wiped the applied theme (live MATE check, 16.10.2026).
    fn native(v: Option<&str>) -> Option<&str> {
        v.map(clean).filter(|v| !v.is_empty())
    }
    let icons = native(src.native_icons).unwrap_or_else(|| clean(src.icons));
    if !icons.is_empty() {
        put(&mut out, "icons", icons);
    }
    // cursor: native storage → Mouse/cursorTheme → kcminput → $XCURSOR_THEME
    let cur = if let Some(v) = native(src.native_cursor) {
        v
    } else {
        let a = clean(src.mouse_cursor);
        if !a.is_empty() {
            a
        } else {
            let b = clean(src.kcminput_cursor);
            if !b.is_empty() {
                b
            } else {
                clean(src.xcursor_env)
            }
        }
    };
    if !cur.is_empty() {
        put(&mut out, "cursor", cur);
    }
    // gtk — native storage, otherwise gtk-3.0/settings.ini [Settings]
    // gtk-theme-name (the first line).
    if let Some(v) = native(src.native_gtk) {
        put(&mut out, "gtk", v);
    } else if let Some(ini) = src.gtk_settings_ini {
        for line in ini.lines() {
            let s = line.trim();
            if s.starts_with("gtk-theme-name") {
                let v = match s.split_once('=') {
                    Some((_, rest)) => clean(rest),
                    None => "",
                };
                if !v.is_empty() {
                    put(&mut out, "gtk", v);
                }
                break;
            }
        }
    }
    // widgetstyle — "which style Qt programs are drawn with". Its source DIFFERS
    // between desktops, and the order here is Qt's own: first
    // QT_STYLE_OVERRIDE (Qt reads it before the platform theme), then our
    // environment.d file (the choice is made, it will be picked up at the next
    // login), then kdeglobals (KDE), lxqt.conf (LXQt) and the qt6ct config.
    //
    // Previously there was one source — kdeglobals: outside KDE it is empty, and
    // the «✅ Active» badge didn't appear on any Kvantum theme, even though the
    // theme was applied.
    let widget_style = {
        let env = clean(src.qt_style_override_env);
        if !env.is_empty() {
            env.to_string()
        } else if let Some(v) = env_file_value(src.qt_style_env_file, "QT_STYLE_OVERRIDE") {
            v
        } else if !clean(src.widget_style).is_empty() {
            clean(src.widget_style).to_string()
        } else if let Some(v) = ini_value(src.lxqt_conf, "Qt", "style") {
            v
        } else {
            ini_value(src.qtct_conf, "Appearance", "style").unwrap_or_default()
        }
    };
    if !widget_style.is_empty() {
        put(&mut out, "widgetstyle", &widget_style);
    }
    if let Some(v) = src.splash {
        if !clean(v.trim()).is_empty() {
            put(&mut out, "splash", clean(v.trim()));
        }
    }
    if let Some(v) = src.lockscreen {
        if !clean(v.trim()).is_empty() {
            put(&mut out, "lockscreen", clean(v.trim()));
        }
    }
    // aurorae — kwinrc theme = "__aurorae__svg__Name" (the suffix WITHOUT
    // cleaning: an empty suffix gives an empty value — Python behaviour kept).
    // Under the Breeze/Oxygen etc. module — `kdecoration` = `library/theme`
    // (`kdecoration::current`): the section highlights the built-in border tile.
    if let Some(theme) = src.kwin_decoration_theme {
        match crate::kdecoration::current(src.kwin_decoration_library, Some(theme)) {
            crate::kdecoration::Current::Svg(dir) => put(&mut out, "aurorae", &dir),
            crate::kdecoration::Current::Builtin(entry) => put(&mut out, "kdecoration", &entry),
            crate::kdecoration::Current::Unknown => {}
        }
    }
    // kvantum — kvantum.kvconfig [General] theme= (the first theme= line).
    if let Some(cfg) = src.kvantum_kvconfig {
        for line in cfg.lines() {
            let s = line.trim();
            if let Some(rest) = s.strip_prefix("theme=") {
                let v = clean(rest);
                if !v.is_empty() {
                    put(&mut out, "kvantum", v);
                }
                break;
            }
        }
    }
    // sddm — [Theme] Current=, the last one wins.
    //
    // ⭐ Fix of 28.09.2026. Previously /etc/plasmalogin.conf* was read here too,
    // and the key was caught in any group. Plasma Login Manager has no themes at
    // all (and no `[Theme] Current` key either), but after switching to it the
    // old `/etc/sddm.conf.d/kde_settings.conf` with `Current=breeze` remains —
    // and the tab marked "breeze" as the active theme of a login screen that is
    // not drawn by SDDM at all (the owner's live machine, snapshot of 28.09).
    // With Plasma Login Manager there is no active SDDM theme — the sources are
    // blanked by [`sddm_confs_for_active_map`]; `Current=` outside the `[Theme]`
    // group doesn't count as a theme — SDDM won't read it there either.
    if let Some(v) = sddm_current_from(&src.sddm_confs) {
        put(&mut out, "sddm", &v);
    }
    // sounds — the current environment's sound theme (kdeglobals[Sounds]Theme in
    // KDE, org.gnome.desktop.sound elsewhere). It is read by the same engine as
    // the «⚙️ Settings» tab — there is no second way to ask.
    if let Some(v) = src.sound_theme {
        let v = crate::qt_settings::parse_sound_theme(v);
        if !clean(&v).is_empty() {
            put(&mut out, "sounds", clean(&v));
        }
    }
    // wallpapers — not a theme name but the name of the PACK in which the
    // current wallpaper picture lies: a pack is "applied" exactly when the
    // desktop background is taken from it.
    if let Some(v) = src.wallpaper_current {
        let name = wallpaper_pack_name(v, src.home, src.usr_root);
        if !name.is_empty() {
            put(&mut out, "wallpapers", &name);
        }
    }
    // plymouth — plymouthd.conf [Daemon] Theme=, the last one wins.
    if let Some(v) = plymouth_current_from(&src.plymouth_confs) {
        put(&mut out, "plymouth", &v);
    }
    out
}

/// The login screen theme from SDDM files (in read order): `[Theme] Current=`,
/// the last one wins.
pub fn sddm_current_from(confs: &[&str]) -> Option<String> {
    let mut found = None;
    for conf in confs {
        let mut in_theme = false;
        for line in conf.lines() {
            let s = line.trim();
            if s.starts_with('[') {
                in_theme = s == "[Theme]";
                continue;
            }
            if !in_theme {
                continue;
            }
            if let Some(rest) = s.strip_prefix("Current=") {
                let v = clean(rest);
                if !v.is_empty() {
                    found = Some(v.to_string());
                }
            }
        }
    }
    found
}

/// The boot splash theme from `plymouthd.conf`: `Theme=`, the last one wins.
pub fn plymouth_current_from(confs: &[&str]) -> Option<String> {
    let mut found = None;
    for conf in confs {
        for line in conf.lines() {
            if let Some(rest) = line.trim().strip_prefix("Theme=") {
                let v = clean(rest);
                if !v.is_empty() {
                    found = Some(v.to_string());
                }
            }
        }
    }
    found
}

/// plymouthd settings by increasing weight — the input of
/// [`plymouth_current_from`] (the last `Theme=` wins): the package default
/// `plymouthd.defaults`, then the choice via `update-alternatives`
/// (`default.plymouth` in the Debian family, `alt_theme` — the name of the
/// directory the link leads to), then the old
/// `/usr/share/plymouth/plymouthd.conf` and, strongest of all,
/// `/etc/plymouth/plymouthd.conf` — written by `plymouth-set-default-theme`.
///
/// ⭐ Fix of 31.10.2026: previously only two files were read, and
/// `/usr/share/…/plymouthd.conf` overrode `/etc`, while without `Theme=` in
/// `/etc` (as on CachyOS until the theme is changed) there was no current one
/// at all — the section grid didn't highlight the theme plymouthd takes from
/// the package default.
pub fn plymouth_confs_by_weight(
    defaults: Option<String>,
    alt_theme: Option<&str>,
    legacy_conf: Option<String>,
    etc_conf: Option<String>,
) -> Vec<String> {
    let alt = alt_theme
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(|t| format!("[Daemon]\nTheme={t}\n"));
    [defaults, alt, legacy_conf, etc_conf].into_iter().flatten().collect()
}

/// The currently chosen system theme of kind `kind` (`sddm` | `plymouth`) — to
/// forbid «Remove» on the active one. The same files as the active theme map,
/// plus system defaults (`/usr/lib/sddm/sddm.conf.d`, `plymouthd.defaults`)
/// and the choice via `update-alternatives` (`default.plymouth`): here an extra
/// refusal is safer than a removed active theme. The root helper checks the
/// same thing once more.
pub fn live_system_theme_active(kind: &str) -> Vec<String> {
    let read_dir_sorted = |dir: &str| -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(dir)
            .map(|rd| {
                rd.flatten()
                    .map(|e| e.path().to_string_lossy().into_owned())
                    .filter(|p| p.ends_with(".conf"))
                    .collect()
            })
            .unwrap_or_default();
        v.sort();
        v
    };
    let read = |paths: &[String]| -> Vec<String> {
        paths.iter().filter_map(|p| std::fs::read_to_string(p).ok()).collect()
    };
    let mut out = Vec::new();
    match kind {
        "sddm" => {
            let mut paths = read_dir_sorted("/usr/lib/sddm/sddm.conf.d");
            paths.extend(read_dir_sorted("/etc/sddm.conf.d"));
            paths.push("/etc/sddm.conf".to_string());
            // Each file separately: any mention already means "chosen".
            for text in read(&paths) {
                out.extend(sddm_current_from(&[&text]));
            }
        }
        "plymouth" => {
            let paths: Vec<String> = [
                "/etc/plymouth/plymouthd.conf",
                "/usr/share/plymouth/plymouthd.defaults",
                "/usr/share/plymouth/plymouthd.conf",
            ]
            .iter()
            .map(|p| p.to_string())
            .collect();
            for text in read(&paths) {
                out.extend(plymouth_current_from(&[&text]));
            }
            let alt = format!("{}/default.plymouth", crate::system_plan::PLYMOUTH_THEMES_DIR);
            if let Ok(real) = std::fs::canonicalize(&alt) {
                let themes = std::fs::canonicalize(crate::system_plan::PLYMOUTH_THEMES_DIR)
                    .unwrap_or_else(|_| std::path::PathBuf::from(crate::system_plan::PLYMOUTH_THEMES_DIR));
                if let Ok(rest) = real.strip_prefix(&themes) {
                    if let Some(first) = rest.components().next() {
                        out.push(first.as_os_str().to_string_lossy().into_owned());
                    }
                }
            }
        }
        _ => {}
    }
    out
}

// ============================================================================
// Preview backfill from the store cache — port of _normalize_theme_name /
// _backfill_preview
// ============================================================================

/// Theme name normalisation for fuzzy folder↔store matching — port of
/// `_normalize_theme_name` (Unicode letters stay: Python `str.isalnum`).
pub fn normalize_theme_name(name: &str) -> String {
    let mut s = name.to_lowercase();
    for noise in [
        "gtk", "theme", "themes", "kde", "plasma", "icons", "icon",
        "cursors", "cursor", "color", "colors", "scheme", "global",
        "aurorae", "kvantum", "sddm", "plymouth",
    ] {
        s = s.replace(noise, "");
    }
    s.chars().filter(|c| c.is_alphanumeric()).collect()
}

/// Scanner environment: paths + FS snapshot + store cache.
pub struct ScanEnv<'a> {
    /// Home directory (live: `$HOME`; in fixtures — /sb/home).
    pub home: &'a str,
    /// The usr root (live: "/usr"; in fixtures — /sb/usr).
    pub usr_root: &'a str,
    pub fs: &'a FsSnapshot,
    /// (name, preview_url) of cached OCS themes — `cache.all_items_min()`.
    pub ocs_cache: &'a [(String, String)],
    /// `cache.get_preview(url)` → path of the local preview file (or None).
    pub preview_file: &'a dyn Fn(&str) -> Option<String>,
    /// What the store knows about what is installed: the section of each cache
    /// picture and the KNewStuff install registry ([`StoreRecords`]).
    pub store: &'a StoreRecords,
    /// The user's language (`ru_RU.UTF-8`) — it selects the theme name translation.
    /// A parameter rather than reading the environment inside: otherwise theme
    /// lists would depend on the machine's locale and the goldens would prove nothing.
    pub locale: &'a str,
    /// The environment's standard icon theme ([`LiveScan::with_de`]): its tile is
    /// visible even if the set is marked `Hidden=true` (Adwaita) — otherwise there
    /// would be no way to return to the environment's default icons.
    pub standard_icons: Option<&'a str>,
}

/// What the store knows about installed themes — raw material for the second
/// step of the preview chain (`local_preview`): the picture from the store record.
#[derive(Debug, Default, Clone)]
pub struct StoreRecords {
    /// Store cache picture URL → section key of its item (`categories`: `icons`,
    /// `kvantum`, …). Empty — the section is unknown, and name matching goes
    /// without it (the old behaviour).
    pub sections: HashMap<String, String>,
    /// KNewStuff install registry: installed path → URL of the item's picture
    /// ([`parse_kns_registry`]).
    pub installed: Vec<(String, String)>,
    /// Paths from Rexon's installed journal
    /// (`appearance_installer::installed_log_paths`). `None` — the journal wasn't
    /// read (scans without the store, references): then every theme in the home
    /// directory is labelled «⬇️ Downloaded», as before.
    pub rexon_paths: Option<Vec<String>>,
}

impl StoreRecords {
    /// Origin label of the theme in directory `path`: «⬇️ Downloaded» — if Rexon
    /// (journal) or the KDE store (KNewStuff registry) installed it, otherwise
    /// «🗂️ Installed manually». Themes placed by hand or by an old `.rxn` were
    /// never "downloaded", and calling them so would be untrue.
    fn origin_label(&self, path: &str) -> &'static str {
        let Some(journal) = &self.rexon_paths else {
            return DOWNLOADED_LABEL;
        };
        let path = path.trim_end_matches('/');
        let ours = |p: &str| {
            let p = p.trim_end_matches('/');
            p == path || p.starts_with(&format!("{path}/"))
        };
        if journal.iter().any(|p| ours(p)) || self.installed.iter().any(|(p, _)| ours(p)) {
            DOWNLOADED_LABEL
        } else {
            MANUAL_LABEL
        }
    }
}

/// A short note on a GTK theme tile when the icons or cursor the theme is
/// designed for (`index.theme`: `IconTheme=`, `CursorTheme=`) are not on the
/// system. `items` — the `themes::plan_theme_deps` plan («Apply» asks it too).
/// `None` — everything is in place or the theme asks for nothing.
pub fn missing_deps_note(items: &[crate::themes::DepItem]) -> Option<String> {
    use crate::themes::{DepRole, DepState};
    let missing = |role: DepRole| {
        items.iter().any(|i| i.role == role && !matches!(i.state, DepState::Installed(_)))
    };
    match (missing(DepRole::Icons), missing(DepRole::Cursor)) {
        (true, true) => Some(tr("⚠️ the theme's icons and cursor are missing")),
        (true, false) => Some(tr("⚠️ the theme's icons are missing")),
        (false, true) => Some(tr("⚠️ the theme's cursor is missing")),
        (false, false) => None,
    }
}

/// Append [`missing_deps_note`] to the GTK theme tile labels. `plan` — the
/// theme's requests by its directory name (live — `LiveInstaller::theme_deps_plan`).
pub fn stamp_missing_deps(items: &mut [ThemeItem], plan: &dyn Fn(&str) -> Vec<crate::themes::DepItem>) {
    for it in items.iter_mut() {
        let Some(name) = it.id.strip_prefix("local:gtk:") else { continue };
        if let Some(note) = missing_deps_note(&plan(name)) {
            it.author = format!("{} · {note}", it.author);
        }
    }
}

/// Label of a theme installed by Rexon or the store.
const DOWNLOADED_LABEL: &str = n_("⬇️ Downloaded");
/// Label of a theme in the home directory installed by neither Rexon nor the store.
const MANUAL_LABEL: &str = n_("🗂️ Installed manually");

impl StoreRecords {
    /// Empty records — for scans without the store.
    pub fn none() -> &'static StoreRecords {
        static NONE: std::sync::OnceLock<StoreRecords> = std::sync::OnceLock::new();
        NONE.get_or_init(StoreRecords::default)
    }
}

/// KNewStuff install registry records (`~/.local/share/knewstuff3/*.knsregistry`):
/// pairs "installed path — URL of the item's picture". The registry is kept by
/// «Get New…» in the KDE settings and by Discover; a `<stuff>` record keeps
/// paths in `<installedfile>`, the picture — in `<preview>` (small) and `<previewBig>`.
pub fn parse_kns_registry(xml: &str) -> Vec<(String, String)> {
    fn tags<'t>(block: &'t str, tag: &str) -> Vec<&'t str> {
        let (open, close) = (format!("<{tag}>"), format!("</{tag}>"));
        block
            .split(open.as_str())
            .skip(1)
            .filter_map(|rest| rest.split(close.as_str()).next())
            .collect()
    }
    let unescape = |v: &str| {
        v.trim()
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&apos;", "'")
            .replace("&amp;", "&")
    };
    let mut out = Vec::new();
    for block in xml.split("<stuff").skip(1) {
        let block = block.split("</stuff>").next().unwrap_or("");
        let url = tags(block, "preview")
            .into_iter()
            .chain(tags(block, "previewBig"))
            .map(unescape)
            .find(|u| !u.is_empty());
        let Some(url) = url else { continue };
        for path in tags(block, "installedfile") {
            // A directory recorded with a `/*` tail — "everything inside".
            let path = unescape(path);
            let path = path.trim_end_matches('*').trim_end_matches('/');
            if path.starts_with('/') {
                out.push((path.to_string(), url.clone()));
            }
        }
    }
    out
}

/// Picture of the item `path` was installed from (a theme directory or its
/// file), via the install registry — if it is already in the store cache.
pub fn installed_record_preview(env: &ScanEnv, path: &str) -> String {
    let path = path.trim_end_matches('/');
    if path.is_empty() {
        return String::new();
    }
    let inside = format!("{path}/");
    env.store
        .installed
        .iter()
        .filter(|(installed, _)| installed == path || installed.starts_with(&inside))
        .find_map(|(_, url)| (env.preview_file)(url).filter(|local| env.fs.is_file(local)))
        .unwrap_or_default()
}

/// A best-effort preview for an installed theme from the store cache — port of
/// `_backfill_preview`: a conservative fuzzy match (substring both ways, stem
/// ≥4), ambiguity (≠1 url) → "".
pub fn backfill_preview(env: &ScanEnv, name: &str) -> String {
    backfill_preview_for(env, "", name)
}

/// [`backfill_preview`] within section `kind`: a picture of an item from
/// another section doesn't fit the theme even if the names are similar.
/// Without this the Kvantum theme "Win7OS" showed the picture of the
/// "win7-icon-theme" icon set, and all `…@cinnamon.org` applets — the picture
/// of one global theme (12.10.2026). The picture's section is taken from
/// [`StoreRecords::sections`]; an empty `kind` or an empty map — matching
/// without a section, by the old rule (substring both ways). Within a section
/// the names must match in full.
pub fn backfill_preview_for(env: &ScanEnv, kind: &str, name: &str) -> String {
    let by_section = !kind.is_empty() && !env.store.sections.is_empty();
    let target = normalize_theme_name(name);
    if target.chars().count() < 4 {
        return String::new();
    }
    let mut matched: HashSet<&str> = HashSet::new();
    for (cache_name, url) in env.ocs_cache {
        if url.is_empty() {
            continue;
        }
        if by_section && env.store.sections.get(url).map(String::as_str) != Some(kind) {
            continue;
        }
        let norm = normalize_theme_name(cache_name);
        if norm.chars().count() < 4 {
            continue;
        }
        // Within a section — only the very same name: compared with a sample from
        // the theme's content, the picture of a similarly named item ("Orang" and
        // "Glassy-OS-3D-GTK - [ Orange ]") is worse than one drawn from the theme itself.
        let same = if by_section { target == norm } else { target.contains(&norm) || norm.contains(&target) };
        if same {
            matched.insert(url.as_str());
        }
    }
    if matched.len() != 1 {
        return String::new(); // 0 — not found; >1 — ambiguous
    }
    let url = matched.into_iter().next().unwrap();
    match (env.preview_file)(url) {
        Some(local) if env.fs.is_file(&local) => local,
        _ => String::new(),
    }
}

/// Preview for a local theme — port of `_resolve_preview`: a file in the theme →
/// sidecar cache → backfill from the store → "" (the UI draws a placeholder).
pub fn resolve_preview(
    env: &ScanEnv,
    apply_kind: &str,
    name: &str,
    theme_dir: &str,
    prefer: Option<&str>,
) -> String {
    resolve_preview_in(env, apply_kind, name, theme_dir, theme_dir, prefer, &[])
}

/// The first two steps of the single preview chain (`local_preview`) for ANY
/// theme kind: its own picture (a file in the theme directory named by the
/// metadata, the kind's thumbnail in a subdirectory from `subs`), then the
/// picture from the store record — placed at install time (sidecar), found via
/// the install registry for `install_path` and, last, by name in the store
/// cache within section `apply_kind`. Empty — `local_preview` draws the picture.
pub fn resolve_preview_in(
    env: &ScanEnv,
    apply_kind: &str,
    name: &str,
    theme_dir: &str,
    install_path: &str,
    prefer: Option<&str>,
    subs: &[&str],
) -> String {
    if !theme_dir.is_empty() {
        let p = find_preview_in(env.fs, theme_dir, prefer, subs);
        if !p.is_empty() {
            return p;
        }
    }
    let side = sidecar_preview_path(env.home, apply_kind, name);
    if env.fs.is_file(&side) {
        return side;
    }
    let recorded = installed_record_preview(env, install_path);
    if !recorded.is_empty() {
        return recorded;
    }
    backfill_preview_for(env, apply_kind, name)
}

/// A mark on the tile of a cursor theme converted from Windows (Wehrwolfmann's
/// decision of 03.10.2026: «🪟 From Windows»).
// TRANSLATORS: badge on a cursor theme tile: the theme was converted from Windows cursors.
const FROM_WINDOWS_LABEL: &str = n_("🪟 From Windows");

/// Was the cursor theme converted from Windows by the Rexon importer?
///
/// The sign is the line `X-Rexon-Source=windows` in `index.theme`
/// ([`crate::importers::WINDOWS_CURSOR_ORIGIN_KEY`]). Themes converted before
/// this line existed are recognised by the `Comment` the importer writes
/// itself, always the same way (`Imported from Windows cursors (N cursors)`).
pub fn is_windows_cursor_theme(fs: &FsSnapshot, theme_dir: &str) -> bool {
    if theme_dir.is_empty() {
        return false;
    }
    let Some(text) = fs.read_text(&join(theme_dir, "index.theme")) else {
        return false;
    };
    let key = crate::importers::WINDOWS_CURSOR_ORIGIN_KEY;
    if name_from_ini(text, &[key.to_string()]).is_some_and(|v| v.eq_ignore_ascii_case("windows")) {
        return true;
    }
    name_from_ini(text, &["Comment".to_string()])
        .is_some_and(|c| c.starts_with("Imported from Windows cursors ("))
}

// ============================================================================
// Building a ThemeItem — port of _mk_item / _mk_local_item
// ============================================================================

fn mk_item(
    env: &ScanEnv,
    t: &ScanType,
    name: &str,
    base_dir: &str,
    active: bool,
    source_label: &str,
) -> ThemeItem {
    let theme_dir = if t.entry == "colors" {
        String::new()
    } else {
        join(base_dir, name)
    };
    let install_path = if theme_dir.is_empty() {
        join(base_dir, name)
    } else {
        theme_dir.clone()
    };
    // For a color scheme the install registry remembers the `<name>.colors` file itself.
    let recorded_as = if t.entry == "colors" {
        format!("{install_path}.colors")
    } else {
        install_path.clone()
    };
    let preview = resolve_preview_in(
        env, t.apply_kind, name, &theme_dir, &recorded_as, None, &preview_subdirs(t),
    );
    let badge = if active { local_active_badge() } else { tr(source_label) };
    // The id carries the directory name (the theme is applied by it), the label —
    // the human name from the metadata. A color scheme has no directory at all:
    // the theme is a single `<name>.colors` file, and the name lies inside it.
    let (display, source_name) = if t.entry == "colors" {
        let file = format!("{}.colors", join(base_dir, name));
        let display = file_display_name(env.fs, &file, name, env.locale);
        let source = file_source_name(env.fs, &file, name, &display);
        (display, source)
    } else if t.entry == "files" {
        (name.strip_suffix(t.probe).unwrap_or(name).to_string(), String::new())
    } else {
        let display = theme_display_name(env.fs, &theme_dir, name, env.locale);
        let source = theme_source_name(env.fs, &theme_dir, name, &display);
        (display, source)
    };
    let mut description = theme_description(env.fs, &theme_dir, env.locale);
    // Cursors from Windows: the source mark on the tile and a description in the
    // interface language instead of the English `Comment` written by the importer.
    let mut origin = String::new();
    if t.apply_kind == "cursor" && is_windows_cursor_theme(env.fs, &theme_dir) {
        origin = format!(" · {}", tr(FROM_WINDOWS_LABEL));
        description = tr("Imported from Windows");
    }
    ThemeItem {
        id: format!("local:{}:{name}", t.apply_kind),
        name: display,
        description,
        author: format!("{} {} · {badge}{origin}", t.icon, type_label(t)),
        category_id: t.cat_id,
        installed: true,
        install_path,
        preview_local: preview,
        local_kind: active_kind(active),
        source_name,
        ..Default::default()
    }
}

// The signature mirrors the kwargs of Python `_mk_local_item` 1-to-1 (see parity).
//
// `display` — what the user will see; `name` — the directory name the card id
// is built from. The caller computes it: an application style has no theme
// directory at all, while Kvantum and look-and-feel have their own.
#[allow(clippy::too_many_arguments)]
fn mk_local_item(
    kind: &str,
    cat_id: i64,
    name: &str,
    display: &str,
    active: bool,
    label: &str,
    icon: &str,
    install_path: &str,
    preview: &str,
) -> ThemeItem {
    // label already contains the type's emoji; for the active badge we prefix icon
    // (otherwise there was the "🎛 🎛 …" bug — see the Python comment).
    let author = if active {
        format!("{icon} {}", local_active_badge())
    } else {
        tr(label)
    };
    ThemeItem {
        id: format!("local:{kind}:{name}"),
        name: display.to_string(),
        author,
        category_id: cat_id,
        installed: true,
        install_path: install_path.to_string(),
        preview_local: preview.to_string(),
        local_kind: active_kind(active),
        ..Default::default()
    }
}

/// The theme kind in the card label, in the interface language.
///
/// Translated with the "theme kind" context: the same English words ("GTK
/// Theme") are translated differently elsewhere in the program, while here the
/// Russian must stay as before. Proper names (Kvantum, Plymouth) — as is.
pub fn type_label(t: &ScanType) -> String {
    match t.apply_kind {
        "global" => trc("theme kind", "Global Theme"),
        "plasma" => trc("theme kind", "Plasma Style"),
        "colors" => trc("theme kind", "Color Scheme"),
        "aurorae" => trc("theme kind", "Window Decorations (Aurorae)"),
        "icons" => trc("theme kind", "Icons"),
        "cursor" => trc("theme kind", "Cursor"),
        "gtk" => trc("theme kind", "GTK Theme"),
        "sddm" => trc("theme kind", "SDDM (Login Screen)"),
        "sounds" => trc("theme kind", "Sound Theme"),
        // Desktop environment sections: the label is the type table's `msgid`.
        _ if crate::categories::is_de_section(t.cat_id) => tr(t.label),
        _ => t.label.to_string(),
    }
}

/// The "theme is enabled now" sign for the page — separate from the label.
///
/// The author's label ("… · ✅ Active") is translated, and recognising the
/// active theme by searching for the badge in it would tie behaviour to the
/// language. Inactive ones get no sign: what such a tile looks like is decided
/// by the section it is shown in (`appearance_page::scan_local_kind`).
fn active_kind(active: bool) -> Option<String> {
    active.then(|| "active".to_string())
}

// ============================================================================
// Public scanners
// ============================================================================

/// Themes applied NOW (the map from the KDE config) — port of `scan_active`.
pub fn scan_active(env: &ScanEnv, active_map: &HashMap<String, String>) -> Vec<ThemeItem> {
    scan_active_in(env, &types_for(env.home, env.usr_root), active_map)
}

/// [`scan_active`] over a given set of types (the live scan adds desktop
/// environment sections — [`de_types`]).
pub fn scan_active_in(
    env: &ScanEnv,
    types: &[ScanType],
    active_map: &HashMap<String, String>,
) -> Vec<ThemeItem> {
    let mut items = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for (ti, t) in types.iter().enumerate() {
        // The same kind further down the list (the section also looks at `~/.icons`,
        // `~/.themes`): if the theme wasn't found here, let that one look, rather
        // than a tile with a path into a directory where there is no theme.
        let later_same_kind = types[ti + 1..].iter().any(|o| o.apply_kind == t.apply_kind);
        for name in active_names(active_map, t.apply_kind) {
            // the directory where the active theme actually lies (user → sys)
            let mut base = String::new();
            for d in [&t.user_dir, &t.sys_dir] {
                if d.is_empty() {
                    continue;
                }
                let hit = if t.entry == "colors" {
                    env.fs.is_file(&join(d, &format!("{name}.colors")))
                } else if t.entry == "files" {
                    env.fs.is_file(&join(d, name))
                } else {
                    env.fs.is_dir(&join(d, name))
                };
                if hit {
                    base = d.clone();
                    break;
                }
            }
            if base.is_empty() && later_same_kind {
                continue;
            }
            if base.is_empty() {
                base = if !t.user_dir.is_empty() {
                    t.user_dir.clone()
                } else {
                    t.sys_dir.clone()
                };
            }
            if !entry_valid(t, env.fs, &base, name) {
                continue;
            }
            let key = format!("{}:{name}", t.apply_kind);
            if !seen.insert(key) {
                continue;
            }
            items.push(mk_item(env, t, name, &base, true, ""));
        }
    }
    items
}

/// User themes in ~/.local/share/…, NOT active — port of
/// `scan_downloaded`.
pub fn scan_downloaded(
    env: &ScanEnv,
    active_map: &HashMap<String, String>,
) -> Vec<ThemeItem> {
    scan_downloaded_in(env, &types_for(env.home, env.usr_root), active_map)
}

/// [`scan_downloaded`] over a given set of types.
pub fn scan_downloaded_in(
    env: &ScanEnv,
    types: &[ScanType],
    active_map: &HashMap<String, String>,
) -> Vec<ThemeItem> {
    let mut items = Vec::new();
    for t in types {
        if t.user_dir.is_empty() {
            continue;
        }
        for name in entries_of(env.fs, &t.user_dir, t) {
            if t.apply_kind == "cursor" {
                if !is_cursor_dir(env.fs, &join(&t.user_dir, &name)) {
                    continue;
                }
            } else if t.apply_kind == "icons"
                && is_cursor_only(env.fs, &join(&t.user_dir, &name))
            {
                continue; // pure cursor themes in icons/ are not duplicated into «Icons»
            }
            if is_active(active_map, t.apply_kind, &name) {
                continue;
            }
            if !entry_valid(t, env.fs, &t.user_dir, &name)
                || !entry_listed(env, t.apply_kind, &join(&t.user_dir, &name))
            {
                continue;
            }
            let label = env.store.origin_label(&join(&t.user_dir, &name));
            items.push(mk_item(env, t, &name, &t.user_dir, false, label));
        }
    }
    items
}

/// Result of `scan_system`: items + paths sent to `pacman -Qo`
/// (for checks and for the executor's call itself).
#[derive(Debug)]
pub struct SystemScan {
    pub items: Vec<ThemeItem>,
    pub qo_paths: Vec<String>,
}

/// Themes in /usr/share/… with a NATIVE owning package — port of `scan_system`.
///
/// `foreign` = None means "pacman -Qm unavailable" → Err with the
/// `SystemScanUnavailable` text (the UI shows an explicit status, AUR themes
/// don't leak into «System»). Without pacman classification is impossible → Ok(empty).
/// `owners` — the path→package map from `parse_qo_output` (keys with rstrip('/')).
pub fn scan_system(
    env: &ScanEnv,
    active_map: &HashMap<String, String>,
    has_pacman: bool,
    foreign: Option<&HashSet<String>>,
    owners: &HashMap<String, String>,
) -> Result<SystemScan, String> {
    if !has_pacman {
        return Ok(SystemScan { items: Vec::new(), qo_paths: Vec::new() });
    }
    let Some(foreign) = foreign else {
        return Err(
            tr("Couldn't tell where packages came from (pacman -Qm is unavailable). \
                System themes are hidden so that AUR themes are not shown as system ones."),
        );
    };

    let types = types_for(env.home, env.usr_root);
    scan_system_in(env, &types, active_map, foreign, owners)
}

/// [`scan_system`] over a given set of types (pacman is present, package
/// origin is known).
pub fn scan_system_in(
    env: &ScanEnv,
    types: &[ScanType],
    active_map: &HashMap<String, String>,
    foreign: &HashSet<String>,
    owners: &HashMap<String, String>,
) -> Result<SystemScan, String> {
    // 1) candidates (type, name, full path)
    let mut cands: Vec<(usize, String, String)> = Vec::new();
    for (ti, t) in types.iter().enumerate() {
        if t.sys_dir.is_empty() {
            continue;
        }
        for name in entries_of(env.fs, &t.sys_dir, t) {
            if t.apply_kind == "cursor" {
                if !is_cursor_dir(env.fs, &join(&t.sys_dir, &name)) {
                    continue;
                }
            } else if t.apply_kind == "icons"
                && is_cursor_only(env.fs, &join(&t.sys_dir, &name))
            {
                continue;
            }
            if is_active(active_map, t.apply_kind, &name) {
                continue;
            }
            if !entry_valid(t, env.fs, &t.sys_dir, &name)
                || !entry_listed(env, t.apply_kind, &join(&t.sys_dir, &name))
            {
                continue;
            }
            let full = if t.entry == "colors" {
                join(&t.sys_dir, &format!("{name}.colors"))
            } else {
                join(&t.sys_dir, &name)
            };
            cands.push((ti, name, full));
        }
    }
    let qo_paths: Vec<String> = cands.iter().map(|c| c.2.clone()).collect();

    // 2) filter by owners (one -Qo batch outside)
    let mut items = Vec::new();
    for (ti, name, full) in &cands {
        let Some(pkg) = owners.get(full.trim_end_matches('/')) else {
            continue; // copied by hand → not a system one
        };
        if foreign.contains(pkg) {
            continue; // AUR (foreign) → goes via «Store: AUR»
        }
        let t = &types[*ti];
        items.push(mk_item(env, t, name, &t.sys_dir, false, n_("📦 System")));
    }
    Ok(SystemScan { items, qo_paths })
}

/// A whole section (desktop environment or KDE): the ones enabled now, then
/// own ones (the user directory), then system ones. System ones are not split
/// by package here as in the «System» overview: the section shows everything
/// that can be enabled, and a file's origin doesn't affect the button. One
/// theme — one tile: a name found in two directories (`~/.icons` and
/// `~/.local/share/icons`) is shown by the first.
pub fn scan_section(
    env: &ScanEnv,
    types: &[ScanType],
    active_map: &HashMap<String, String>,
) -> Vec<ThemeItem> {
    let mut items = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let downloaded = scan_downloaded_in(env, types, active_map);
    for item in scan_active_in(env, types, active_map).into_iter().chain(downloaded) {
        if seen.insert(item.id.clone()) {
            items.push(item);
        }
    }
    for t in types {
        if t.sys_dir.is_empty() {
            continue;
        }
        for name in entries_of(env.fs, &t.sys_dir, t) {
            if is_active(active_map, t.apply_kind, &name) || !entry_valid(t, env.fs, &t.sys_dir, &name) {
                continue;
            }
            // Cursors and icons share the icons/ directory: a pure cursor theme —
            // only in «Cursors»; an icon set with its own cursors (Adwaita,
            // elementary) — in both sections (as in «Downloaded»).
            let dir = join(&t.sys_dir, &name);
            if (t.apply_kind == "cursor" && !is_cursor_dir(env.fs, &dir))
                || (t.apply_kind == "icons" && is_cursor_only(env.fs, &dir))
            {
                continue;
            }
            let item = mk_item(env, t, &name, &t.sys_dir, false, n_("📦 System"));
            if seen.insert(item.id.clone()) {
                items.push(item);
            }
        }
    }
    // The theme enabled now is never hidden — the person must see what they
    // have; the rest — by the common rule [`entry_listed`].
    items.retain(|it| {
        let kind = crate::de_apply::apply_kind_of_local_id(&it.id).unwrap_or("");
        it.local_kind.is_some() || entry_listed(env, kind, &it.install_path)
    });
    items
}

/// Whether `key` (`categories::DE_SECTIONS`) is one of the sections where
/// several things can be enabled at once and each is turned on and off on its
/// own: GNOME and Cinnamon extensions, Cinnamon applets and desklets, KWin
/// scripts and effects (the enabling method in the matrix is a list or a
/// module, not a single name) and Conky widgets (running — not running).
pub fn toggle_section(key: &str) -> bool {
    use crate::de_customization::Activate;
    let Some(kind) = crate::categories::category_kind(key) else { return false };
    if kind == crate::de_apply::CONKY_KIND {
        return true;
    }
    crate::de_customization::row(kind).is_some_and(|row| {
        matches!(row.activate, Activate::KwinPlugin | Activate::GsettingsList { .. } | Activate::GnomeExtension)
    })
}

/// Grid order of a toggle section ([`toggle_section`]) — by name, not
/// "enabled on top". Themes have one enabled, and its place first is clear;
/// but here «Disable» moved the card from the head of the list into the
/// middle, and the person lost it from under the cursor (live check 15.11,
/// GNOME extensions). The sort is stable: for equal names — by tile id.
pub fn order_toggle_section(key: &str, items: &mut [ThemeItem]) {
    if toggle_section(key) {
        items.sort_by_cached_key(|it| (it.name.to_lowercase(), it.id.clone()));
    }
}

/// Whether a theme of kind `kind` lies in directory `dir` — one rule for the
/// overviews («Downloaded», «System») and for the section grid.
///
/// The `…/share/themes` and `…/share/icons` directories are shared by several
/// kinds: the first holds both GTK themes and themes that have only window
/// borders (xfwm4, Metacity, Openbox) or a shell; the second — service icon
/// sets (`hicolor` with `Hidden=true`, `default` without its own directories).
/// Such an entry is not a GTK theme or an icon set: it can't be enabled, and
/// the KDE page doesn't show it in the grid. Borders and shells are shown by
/// their own sections — by their own subdirectory ([`de_types`]).
fn entry_listed(env: &ScanEnv, kind: &str, dir: &str) -> bool {
    let fs = env.fs;
    match kind {
        "icons" => fs.read_text(&join(dir, "index.theme")).is_some_and(|text| {
            let info = icon_theme_info(text);
            // The environment's standard set is visible even with `Hidden=true`
            // (Adwaita in GNOME): otherwise there'd be no way back to the default icons.
            let standard = env.standard_icons.is_some_and(|s| dir.rsplit('/').next() == Some(s));
            info.has_group && info.has_dirs && (!info.hidden || standard)
        }),
        // A GTK theme is where GTK will find the `gtk.css` stylesheet. A
        // directory with only `gtk-keys.css` (Default and Emacs from GTK — key
        // bindings) is not an appearance theme: choosing it changes nothing.
        // Default stays in XFCE «Window Borders» — by xfwm4.
        "gtk" => GTK_PARTS.iter().any(|part| fs.is_file(&join(dir, &format!("{part}/gtk.css")))),
        _ => true,
    }
}

/// Subdirectories by which a directory in `…/share/themes` counts as a GTK
/// theme: GTK 3 and GTK 4 stylesheets. A GTK 2-only theme isn't applied by
/// current environments (the KDE page doesn't show it either).
pub const GTK_PARTS: &[&str] = &["gtk-3.0", "gtk-4.0"];

/// What `index.theme` says about an icon set (the freedesktop Icon Theme
/// spec): the `[Icon Theme]` group, a non-empty list of icon directories,
/// `Hidden=true`. A visible set has the group and directories and isn't hidden.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct IconThemeInfo {
    has_group: bool,
    has_dirs: bool,
    hidden: bool,
}

fn icon_theme_info(text: &str) -> IconThemeInfo {
    let mut in_group = false;
    let mut info = IconThemeInfo::default();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_group = line == "[Icon Theme]";
            info.has_group |= in_group;
            continue;
        }
        if !in_group {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else { continue };
        let (key, value) = (key.trim(), value.trim());
        if key == "Hidden" && value.eq_ignore_ascii_case("true") {
            info.hidden = true;
        }
        if (key == "Directories" || key == "ScaledDirectories") && !value.is_empty() {
            info.has_dirs = true;
        }
    }
    info
}

/// System themes (login screen, boot splash) without an owning package — into
/// «Downloaded».
///
/// They don't get into «System» (only themes from repository packages are
/// there); and installing now only places the theme («Apply» chooses it,
/// decision of 04.10.2026) — without this scan an installed theme would have
/// nowhere to be found and applied. Two kinds of themes go here:
///   * installed by Rexon itself — the path is in the installed journal
///     (`journal_paths` — all journal paths);
///   * placed by hand or by another installer — `loose`, paths of directories
///     the package database answered "nobody's" about ([`LiveScan::loose_system_themes`]).
///     Previously such a theme was visible nowhere, and the person couldn't
///     remove it (acceptance 10.10.2026).
pub fn scan_rexon_system(
    env: &ScanEnv,
    types: &[ScanType],
    active_map: &HashMap<String, String>,
    journal_paths: &[String],
    loose: &HashSet<String>,
) -> Vec<ThemeItem> {
    let mut items = Vec::new();
    for t in types {
        if !t.user_dir.is_empty() || t.sys_dir.is_empty() {
            continue;
        }
        for name in entries_of(env.fs, &t.sys_dir, t) {
            let full = join(&t.sys_dir, &name);
            if !loose.contains(&full)
                && !journal_paths.iter().any(|p| p.trim_end_matches('/') == full)
            {
                continue;
            }
            if is_active(active_map, t.apply_kind, &name) || !entry_valid(t, env.fs, &t.sys_dir, &name) {
                continue;
            }
            let label = env.store.origin_label(&full);
            items.push(mk_item(env, t, &name, &t.sys_dir, false, label));
        }
    }
    items
}

/// Application style (Qt widget style) — port of `scan_widget_styles`.
/// `styles` — QStyleFactory.keys() (live) or the `WIDGET_STYLES` fallback.
pub fn scan_widget_styles(
    active_map: &HashMap<String, String>,
    styles: &[String],
    qt_only: bool,
) -> Vec<ThemeItem> {
    let active = active_map
        .get("widgetstyle")
        .map(|s| s.trim())
        .unwrap_or("");
    let mut items = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for name in styles {
        let low = name.to_lowercase();
        if seen.contains(&low) {
            continue;
        }
        if qt_only && low.starts_with("kvantum") {
            continue; // the Kvantum engine is a separate sub-choice
        }
        seen.insert(low.clone());
        let is_active = !active.is_empty() && low == active.to_lowercase();
        // An application style is a Qt engine, not a directory on disk: it has no
        // metadata with a human name and can't have any.
        items.push(mk_local_item(
            "widgetstyle", 901, name, name, is_active,
            n_("🎛️ Application Style"), "🎛️", "", "",
        ));
    }
    items
}

/// List of Kvantum themes (user + system) — port of `scan_kvantum_themes`.
/// Active — when widgetStyle == kvantum* AND the kvconfig theme matches.
pub fn scan_kvantum_themes(
    env: &ScanEnv,
    active_map: &HashMap<String, String>,
) -> Vec<ThemeItem> {
    let ws = active_map
        .get("widgetstyle")
        .map(|s| s.trim().to_lowercase())
        .unwrap_or_default();
    let kv_on = ws.starts_with("kvantum");
    let active_theme = if kv_on {
        active_map.get("kvantum").map(|s| s.trim()).unwrap_or("")
    } else {
        ""
    };
    let mut items = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for base in kvantum_dirs(env.home, env.usr_root) {
        for name in list_entries(env.fs, &base, "dir") {
            if !seen.insert(name.clone()) {
                continue;
            }
            let theme_dir = join(&base, &name);
            let is_active = !active_theme.is_empty() && name == active_theme;
            let preview =
                resolve_preview_in(env, "kvantum", &name, &theme_dir, &theme_dir, None, &[]);
            let display =
                theme_display_name(env.fs, &theme_dir, &name, env.locale);
            let mut item = mk_local_item(
                "kvantum", 123, &name, &display, is_active, "🔲 Kvantum", "🔲",
                &theme_dir, &preview,
            );
            item.description = theme_description(env.fs, &theme_dir, env.locale);
            item.source_name = theme_source_name(env.fs, &theme_dir, &name, &display);
            items.push(item);
        }
    }
    items
}

/// Splash / lock screen (lockscreen) — part of the look-and-feel;
/// port of `scan_lnf_component`.
pub fn scan_lnf_component(
    env: &ScanEnv,
    kind: &str,
    active_map: &HashMap<String, String>,
) -> Vec<ThemeItem> {
    let component = if kind == "splash" { "splash" } else { "lockscreen" };
    let (cat_id, icon, label) = if kind == "splash" {
        (902, "🌅", n_("🌅 KDE Splash Screen"))
    } else {
        (903, "🔒", n_("🔒 Lock Screen"))
    };
    let active = active_map.get(kind).map(|s| s.trim()).unwrap_or("");
    let mut items = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for base in lnf_dirs(env.home, env.usr_root) {
        for name in list_entries(env.fs, &base, "dir") {
            if seen.contains(&name) {
                continue;
            }
            let theme_dir = join(&base, &name);
            if !env
                .fs
                .is_dir(&join(&theme_dir, &format!("contents/{component}")))
            {
                continue;
            }
            seen.insert(name.clone());
            let is_active = !active.is_empty() && name == active;
            let preview = resolve_preview_in(
                env, kind, &name, &theme_dir, &theme_dir, Some(component), &[],
            );
            let display =
                theme_display_name(env.fs, &theme_dir, &name, env.locale);
            let mut item = mk_local_item(
                kind, cat_id, &name, &display, is_active, label, icon,
                &theme_dir, &preview,
            );
            item.description = theme_description(env.fs, &theme_dir, env.locale);
            item.source_name = theme_source_name(env.fs, &theme_dir, &name, &display);
            items.push(item);
        }
    }
    // The «None» tile — no splash is shown at login (as in the KDE module, last
    // in the grid). It has no directory: neither removal nor display.
    if kind == "splash" {
        let none = crate::appearance_installer::SPLASH_NONE;
        let mut item = mk_local_item(
            kind, cat_id, none, &crate::appearance_installer::splash_none_name(),
            active == none, label, icon, "", "",
        );
        item.description = crate::appearance_installer::splash_none_description();
        item.source_name = none.to_string();
        items.push(item);
    }
    items
}

// ============================================================================
// Live disk overview — port of the executor from
// `rexon/appearance/installed_scan.py` (`_rs_snapshot`, `_rs_active_sources`,
// `_rs_foreign_pkgs`, `_rs_owner_pkgs` and the `_rs_scan_*` wrappers)
// ============================================================================
//
// Everything above in this module is RULES over ready raw material. Here is
// the only place where raw material is obtained from the real machine:
// os.scandir over the theme tree, reading configs, running
// kreadconfig6/gsettings/the package manager. There is no second copy of the
// rules here and there must not be: the live executor only gathers the input
// and calls the same `scan_*` as the Python bridge.
//
// Without it the «Appearance» tab in a process without Python showed empty
// sections: the scanners exist, but nobody feeds them a snapshot and the
// active theme map.

/// `$HOME` — the same one Python substitutes (`os.path.expanduser("~")`).
fn live_home() -> String {
    std::env::var("HOME").unwrap_or_else(|_| ".".to_string())
}

/// An environment variable as a string (no variable — empty), like `os.environ.get`.
fn env_str(name: &str) -> String {
    std::env::var(name).unwrap_or_default()
}

/// `shutil.which` via the core's shared resolver (honours the process `$PATH`).
fn which_live(name: &str) -> Option<String> {
    crate::utils::which(name, std::env::var("PATH").ok().as_deref())
}

/// File text for the snapshot: any read error → empty (port of `_rs_read_text`).
/// The core then treats the metadata as broken — exactly like Python's error branches.
fn snap_read_text(path: &std::path::Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

/// File content or `None` — port of `_rs_read_optional` (no file / unreadable).
/// An empty file and a missing one are DIFFERENT answers: the
/// `active_map_from_sources` branches diverge on them.
fn read_optional(path: &str) -> Option<String> {
    let p = std::path::Path::new(path);
    if !p.is_file() {
        return None;
    }
    std::fs::read_to_string(p).ok()
}

/// Directory entry names in `readdir` order — the same order the Python
/// reference's `os.scandir` saw. There is NO sorting here on purpose: "the
/// first suitable preview" is chosen by this order, and sorting would move
/// the live overview away from the bridge goldens.
fn scandir_names(dir: &std::path::Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|e| e.file_name().to_str().map(str::to_string))
        .collect()
}

/// Preview candidates deeper than the theme's first level — port of
/// `_RS_PREVIEW_PROBES`. Only existing ones go into the snapshot:
/// `find_preview` asks `is_file` about them, and it answers BY KEY PRESENCE.
const PREVIEW_PROBES: &[&str] = &[
    "contents/previews/preview.png",
    "contents/previews/fullscreenpreview.png",
    "contents/screenshot.png",
    "contents/previews/lockscreen.png",
    "contents/previews/splash.png",
    "metadata/preview.png",
    // GTK stylesheets: by them a directory counts as a GTK theme
    // ([`entry_listed`]).
    "gtk-3.0/gtk.css",
    "gtk-4.0/gtk.css",
    // The kinds' thumbnails in theme subdirectories ([`preview_subdirs`]).
    "gtk-3.0/thumbnail.png",
    "gtk-4.0/thumbnail.png",
    "gtk-3.20/thumbnail.png",
    "gtk-2.0/thumbnail.png",
    "cinnamon/thumbnail.png",
    "metacity-1/thumbnail.png",
    "xfwm4/thumbnail.png",
    "gnome-shell/thumbnail.png",
];

/// look-and-feel components — port of `_RS_COMPONENT_PROBES`
/// (`scan_lnf_component` asks `is_dir` about them).
const COMPONENT_PROBES: &[&str] = &["contents/splash", "contents/lockscreen"];

/// Whether a JSON value is "truthy" by Python rules (`x or y`):
/// null / false / 0 / "" — false, everything else true.
fn json_truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(true),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// Relative `Preview=`/`Image=` paths from metadata — port of
/// `_rs_meta_preview_rels`. A superset of what [`preview_from_metadata`] will
/// read: the goal is to register the preview file in the snapshot.
fn meta_preview_rels(meta_json: Option<&str>, meta_desktop: Option<&str>) -> Vec<String> {
    let mut rels: Vec<String> = Vec::new();
    if let Some(text) = meta_json.filter(|t| !t.is_empty()) {
        if let Ok(Value::Object(data)) = serde_json::from_str::<Value>(text) {
            let kp = data.get("KPlugin").and_then(Value::as_object);
            for key in ["Preview", "Image"] {
                let from_kp = kp.and_then(|k| k.get(key)).filter(|v| json_truthy(v));
                let rel = from_kp.or_else(|| data.get(key));
                if let Some(Value::String(s)) = rel {
                    if !s.is_empty() {
                        rels.push(s.clone());
                    }
                }
            }
        }
    }
    if let Some(text) = meta_desktop.filter(|t| !t.is_empty()) {
        for line in text.lines() {
            let s = line.trim();
            if s.starts_with("Preview=") || s.starts_with("Image=") || s.starts_with("Screenshot=") {
                if let Some((_, rest)) = s.split_once('=') {
                    let rel = rest.trim();
                    if !rel.is_empty() {
                        rels.push(rel.to_string());
                    }
                }
            }
        }
    }
    rels
}

/// Snapshot parts before being assembled into [`FsSnapshot`] (port of the `dirs`/`files` dicts).
#[derive(Default)]
struct SnapParts {
    dirs: HashMap<String, Vec<String>>,
    files: HashMap<String, String>,
}

/// The directory of ONE theme into the snapshot — port of `_rs_snap_theme`:
/// children (files and subfolders) plus pinpoint preview and component probes at depth 2-3.
fn snap_theme(theme_dir: &std::path::Path, parts: &mut SnapParts) {
    let key = theme_dir.to_string_lossy().into_owned();
    if parts.dirs.contains_key(&key) {
        return;
    }
    let names = scandir_names(theme_dir);
    parts.dirs.insert(key, names.clone());
    let mut meta_json: Option<String> = None;
    let mut meta_desktop: Option<String> = None;
    for name in &names {
        let path = theme_dir.join(name);
        let path_key = path.to_string_lossy().into_owned();
        if path.is_dir() {
            parts.dirs.entry(path_key).or_default();
        } else if path.is_file() {
            if name == "metadata.json" || name == "metadata.desktop" {
                let text = snap_read_text(&path);
                if name == "metadata.json" {
                    meta_json = Some(text.clone());
                } else {
                    meta_desktop = Some(text.clone());
                }
                parts.files.insert(path_key, text);
            } else if name == "index.theme" || name.ends_with(".plymouth") {
                // Icons, cursors and GTK themes carry no KDE metadata: their
                // human name lies only here. A boot splash theme has its name
                // and description in `<name>.plymouth`.
                parts.files.insert(path_key, snap_read_text(&path));
            } else {
                // The text of ordinary files isn't read: the scanners only need
                // the fact they exist, and themes carry megabyte-sized .svg files.
                parts.files.insert(path_key, String::new());
            }
        }
    }
    for rel in PREVIEW_PROBES {
        let p = theme_dir.join(rel);
        if p.is_file() {
            parts.files.insert(p.to_string_lossy().into_owned(), String::new());
        }
    }
    for rel in COMPONENT_PROBES {
        let p = theme_dir.join(rel);
        if p.is_dir() {
            parts.dirs.entry(p.to_string_lossy().into_owned()).or_default();
        }
    }
    for rel in meta_preview_rels(meta_json.as_deref(), meta_desktop.as_deref()) {
        if rel.starts_with('/') {
            continue; // an absolute Preview= is exotic, outside the snapshot (as in the original)
        }
        for base in [theme_dir.join("contents"), theme_dir.to_path_buf()] {
            let p = base.join(&rel);
            if p.is_file() {
                parts.files.insert(p.to_string_lossy().into_owned(), String::new());
            }
        }
    }
}

/// Live snapshot of the theme tree — port of `_rs_snapshot`.
///
/// The depth is exactly what the scanners access (see the module header):
/// roots, their theme children, metadata as text and pinpoint probes. Walking
/// the whole `~/.local/share/icons` is not allowed — that is tens of
/// thousands of files on every opening of the tab.
///
/// `extra_files` — local preview files from the store cache: the backfill
/// target is checked via `is_file`, and an unregistered file would silently
/// become "nonexistent".
pub fn live_snapshot(roots: &[String], extra_files: &[String]) -> FsSnapshot {
    let mut parts = SnapParts::default();
    for root in roots {
        if root.is_empty() || parts.dirs.contains_key(root) {
            continue;
        }
        let root_path = std::path::Path::new(root);
        if !root_path.is_dir() {
            continue;
        }
        let mut names = scandir_names(root_path);
        // Our `~/.themes` → `~/.local/share/themes` links don't count as a
        // theme: the theme is the same, and it would appear twice in the lists.
        if let Some(home) = root.trim_end_matches('/').strip_suffix("/.themes") {
            names.retain(|n| !is_theme_link(home, n));
        }
        parts.dirs.insert(root.clone(), names.clone());
        for name in &names {
            let path = root_path.join(name);
            if path.is_file() {
                // A color scheme is a file, not a directory: the human name
                // lies inside it, and without the text the card would stay
                // labelled with the file name.
                let text = if name.ends_with(".colors") {
                    snap_read_text(&path)
                } else {
                    String::new()
                };
                parts.files.insert(path.to_string_lossy().into_owned(), text);
            } else if path.is_dir() {
                snap_theme(&path, &mut parts);
            }
        }
    }
    // sidecar preview cache (types without a theme directory — color schemes)
    let sidecar = sidecar_preview_dir(&live_home());
    let sidecar_path = std::path::Path::new(&sidecar);
    if sidecar_path.is_dir() {
        for name in scandir_names(sidecar_path) {
            let p = sidecar_path.join(name);
            if p.is_file() {
                parts.files.insert(p.to_string_lossy().into_owned(), String::new());
            }
        }
    }
    for p in extra_files {
        parts.files.insert(p.clone(), String::new());
    }
    FsSnapshot::from_parts(parts.dirs, parts.files)
}

impl FsSnapshot {
    /// A snapshot from live parts: directory → names, file → text.
    ///
    /// The pair to [`FsSnapshot::from_json`] for executors that assemble the
    /// snapshot themselves (the live disk overview, the installer). The fields are
    /// private for a reason: `is_dir`/`is_file` answer BY KEY PRESENCE, and putting
    /// half a tree here means silently getting "no such file".
    pub fn from_parts(
        dirs: HashMap<String, Vec<String>>,
        files: HashMap<String, String>,
    ) -> Self {
        FsSnapshot { dirs, files }
    }
}

// ── Running external commands ────────────────────────────────────────────

/// An external command's answer: exit code, stdout, stderr.
///
/// `None` instead of an answer — asking failed (no binary, timeout, the
/// process didn't start). This is NOT a non-zero code: for `pacman -Qm` "not
/// asked" and "asked, answered with an error" lead to DIFFERENT decisions
/// (port of `_run_pkg_query`).
fn run_capture(argv: &[String], timeout_secs: u64, lang_c: bool) -> Option<(i32, String, String)> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    let (head, args) = argv.split_first()?;
    // The same SINGLE name substitution point as in `safe_run`: on Plasma 5
    // `kreadconfig6` is called `kreadconfig5`.
    let resolved = crate::utils::tool_cmd(head, args, std::env::var("PATH").ok().as_deref());
    let (bin, rest) = resolved.split_first()?;
    let mut command = Command::new(bin);
    command
        .args(rest)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if lang_c {
        // A Russian locale translates both the manager's answer and the busy
        // database message — then the core's parsing finds nothing.
        crate::utils::parsed_output_locale(&mut command);
    }
    let mut child = command.spawn().ok()?;
    // Both pipes are read IN PARALLEL: `pacman -Qo` on two hundred paths writes
    // a line to stderr for every unowned file, and reading them one after the
    // other would hit a full pipe buffer — the process would hang forever.
    let out_pipe = child.stdout.take();
    let err_pipe = child.stderr.take();
    let out_thread = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut p) = out_pipe {
            let _ = p.read_to_end(&mut buf);
        }
        buf
    });
    let err_thread = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut p) = err_pipe {
            let _ = p.read_to_end(&mut buf);
        }
        buf
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break Some(st),
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            Err(_) => break None,
        }
    };
    let stdout = out_thread.join().unwrap_or_default();
    let stderr = err_thread.join().unwrap_or_default();
    let code = status?.code()?;
    Some((
        code,
        String::from_utf8_lossy(&stdout).into_owned(),
        String::from_utf8_lossy(&stderr).into_owned(),
    ))
}

/// Running with a timeout by command class (`utils::default_timeout_secs`) —
/// the same way `safe_run` applies it without an explicit `timeout`.
fn run_default(argv: &[String]) -> Option<(i32, String, String)> {
    let t = crate::utils::default_timeout_secs(argv);
    run_capture(argv, t, false)
}

/// A package engine query — port of `_run_pkg_query`: check the binary, take
/// argv, environment and timeout from the core's plan, run. No decisions here.
fn run_pkg_query(query: &crate::system_plan::InstalledPkgQuery) -> Option<(i32, String, String)> {
    which_live(&query.binary)?;
    let cmd = &query.cmd;
    let timeout = cmd
        .timeout_secs
        .unwrap_or_else(|| crate::utils::default_timeout_secs(&cmd.argv));
    run_capture(&cmd.argv, timeout, cmd.lang_c)
}

// ── Package manager: package origin and file owners ──────────────────────

/// One `pacman -Qm` attempt in the same form the Python reference
/// accumulated it ("ok"/"fail"/"error" + payload). Stored like this rather
/// than as a ready [`QmAttempt`] because the list is given to the retry policy
/// TWICE: by itself and with a virtual successful attempt at the end.
type AttemptRaw = (&'static str, String);

fn to_qm_attempts(raw: &[AttemptRaw]) -> Vec<QmAttempt> {
    raw.iter()
        .map(|(kind, payload)| match *kind {
            "ok" => QmAttempt::Ok(payload.clone()),
            "fail" => QmAttempt::Fail(payload.clone()),
            _ => QmAttempt::Error,
        })
        .collect()
}

/// The set of packages not from repositories — port of `_rs_foreign_pkgs`.
///
/// `Some` — determined (an empty set is an answer too). `None` — it couldn't
/// be determined reliably, and then «System» must not be shown: AUR themes
/// would leak into it silently.
fn live_foreign_pkgs(query: &crate::system_plan::InstalledPkgQuery) -> Option<HashSet<String>> {
    let mut attempts: Vec<AttemptRaw> = Vec::new();
    for _ in 0..2 {
        match run_pkg_query(query) {
            None => attempts.push(("error", String::new())),
            Some((0, out, _)) => attempts.push(("ok", out)),
            Some((_, _, err)) => attempts.push(("fail", err)),
        }
        let (result, _used) = foreign_pkgs_from_attempts(true, &to_qm_attempts(&attempts));
        if let Some(set) = result {
            return Some(set);
        }
        // None: a stop error or "retry"? We ask the same policy with a virtual
        // successful attempt: if it would continue, a retry is needed.
        let mut probe = attempts.clone();
        probe.push(("ok", String::new()));
        let (probe_result, _) = foreign_pkgs_from_attempts(true, &to_qm_attempts(&probe));
        if probe_result.is_none() {
            break; // a non-lck error → a retry won't help
        }
        if attempts.last().map(|a| a.0) == Some("fail") {
            // the database is busy with another transaction → wait and retry
            std::thread::sleep(std::time::Duration::from_millis(1500));
        }
    }
    None
}

/// `path → owning package` — port of `_rs_owner_pkgs`: the commands and
/// batching are set by the engine, parsing the answer — [`parse_qo_output`],
/// here is the run.
///
/// A non-zero code is normal: it only means part of the paths have no owner
/// ("error: No package owns …" goes to stderr and doesn't hinder parsing).
/// Paths from `paths` that have no owning package. Any uncertainty (no query
/// for the family, no program, the query didn't run) — empty.
fn live_loose_paths(family: crate::system::PkgFamily, paths: &[String]) -> HashSet<String> {
    let none = HashSet::new();
    if paths.is_empty() {
        return none;
    }
    let Some(queries) = crate::system_plan::file_owner_queries(family, paths) else {
        return none;
    };
    // "Nobody's" — only what the database said so itself (an error line with
    // the path): silence or a failed query doesn't make a path nobody's.
    let mut loose = HashSet::new();
    for query in &queries {
        let Some((_, _, err)) = run_pkg_query(query) else {
            return none;
        };
        loose.extend(parse_qo_unowned(&err));
    }
    paths.iter().filter(|p| loose.contains(p.trim_end_matches('/'))).cloned().collect()
}

/// Paths about which `pacman -Qo` (under `LANG=C`) answered "No package owns …".
pub fn parse_qo_unowned(stderr: &str) -> HashSet<String> {
    const MARK: &str = "No package owns ";
    stderr
        .lines()
        .filter_map(|line| line.find(MARK).map(|at| &line[at + MARK.len()..]))
        .map(|p| p.trim().trim_end_matches('/').to_string())
        .filter(|p| p.starts_with('/'))
        .collect()
}

fn live_owner_pkgs(
    family: crate::system::PkgFamily,
    paths: &[String],
) -> HashMap<String, String> {
    let mut owners = HashMap::new();
    let Some(queries) = crate::system_plan::file_owner_queries(family, paths) else {
        return owners;
    };
    for query in &queries {
        let out = run_pkg_query(query).map(|(_, out, _)| out).unwrap_or_default();
        owners.extend(parse_qo_output(&out));
    }
    owners
}

// ── Active theme map sources ─────────────────────────────────────────────

/// `active_map` sources read from the machine — the owning counterpart of
/// [`ActiveMapSources`] (which only holds references).
#[derive(Default)]
struct LiveSources {
    kde_global: String,
    kde_plasma_style: String,
    kde_colors: String,
    icons: String,
    mouse_cursor: String,
    kcminput_cursor: String,
    xcursor_env: String,
    widget_style: String,
    gtk_settings_ini: Option<String>,
    splash: Option<String>,
    lockscreen: Option<String>,
    kwin_decoration_theme: Option<String>,
    kwin_decoration_library: Option<String>,
    kvantum_kvconfig: Option<String>,
    qt_style_override_env: String,
    qt_style_env_file: Option<String>,
    lxqt_conf: Option<String>,
    qtct_conf: Option<String>,
    native_gtk: Option<String>,
    native_icons: Option<String>,
    native_cursor: Option<String>,
    sddm_confs: Vec<String>,
    plymouth_confs: Vec<String>,
    plasma_login: bool,
    sound_theme: Option<String>,
    wallpaper_current: Option<String>,
}

/// Live disk overview: the FS snapshot and machine queries at the call site.
///
/// The «Appearance» tab's action object keeps it between list refreshes: the
/// store cache is taken once (`LiveCache::all_items_min` /
/// `all_previews_min`), while the theme tree and the active theme map are
/// reread on every scan — themes are installed and applied while the tab is open.
pub struct LiveScan {
    pub home: String,
    pub usr_root: String,
    /// (name, preview URL) of cached store themes — raw material for the backfill.
    pub ocs_cache: Vec<(String, String)>,
    /// `preview URL → local file` (existing files only).
    pub preview_files: HashMap<String, String>,
    /// Store records about what is installed ([`LiveScan::with_store_records`]).
    pub store: StoreRecords,
    /// Whether the login screen is drawn by Plasma Login Manager. `None` — ask the
    /// machine at the call site (`appearance_installer::active_login_manager`, may
    /// call `systemctl`); a caller from the UI thread that doesn't need the SDDM
    /// theme sets the answer up front and doesn't go to `systemctl`.
    pub plasma_login: Option<bool>,
    /// The desktop the tab is looking at (`None` — without desktop environment
    /// sections, as before). With it the disk overviews include this desktop's
    /// environment sections ([`de_types`]) with a truthful "enabled now".
    pub de: Option<String>,
    /// Whether to lift, during the scan, themes that older installs placed one
    /// level deeper (`appearance_installer::migrate_nested_themes_once`). Test
    /// rigs that should only read the disk turn this off with [`LiveScan::read_only`].
    pub migrate: bool,
    /// The standard icon theme of environment `de` (the schema default, see
    /// [`crate::appearance_installer::live_standard_theme`]); `None` — not looked
    /// up, or the environment has none.
    pub standard_icons: Option<String>,
}

impl LiveScan {
    /// `ocs_cache`/`preview_files` — from the live store cache; empty ones are
    /// allowed (then the preview backfill simply won't work).
    pub fn new(
        ocs_cache: Vec<(String, String)>,
        preview_files: HashMap<String, String>,
    ) -> Self {
        LiveScan {
            home: live_home(),
            usr_root: "/usr".to_string(),
            ocs_cache,
            preview_files,
            store: StoreRecords::default(),
            plasma_login: None,
            de: None,
            migrate: true,
            standard_icons: None,
        }
    }

    /// The same scan with store records about what is installed: `sections` —
    /// pairs "cache picture URL — category number of its item"
    /// (`LiveCache::all_preview_categories`), the KNewStuff install registry is
    /// read from disk right here.
    pub fn with_store_records(mut self, sections: Vec<(String, i64)>) -> Self {
        let home = std::path::Path::new(&self.home);
        let cats = crate::categories::categories(home);
        let key_of = |id: i64| {
            cats.iter()
                .find(|c| c.id == id)
                .or_else(|| cats.iter().find(|c| c.id == crate::categories::section_id_for(id)))
                .map(|c| c.key.clone())
        };
        self.store.sections = sections
            .into_iter()
            .filter(|(url, _)| !url.is_empty())
            .map(|(url, id)| (url, key_of(id).unwrap_or_default()))
            .collect();
        self.store.rexon_paths = Some(crate::appearance_installer::installed_log_paths());
        let data = crate::utils::data_home(std::env::var("XDG_DATA_HOME").ok().as_deref(), &self.home);
        let registry = std::path::Path::new(&data).join("knewstuff3");
        for name in scandir_names(&registry) {
            if name.ends_with(".knsregistry") {
                self.store.installed.extend(parse_kns_registry(&snap_read_text(&registry.join(&name))));
            }
        }
        self
    }

    /// The same scan, but the disk is only read: nested themes are not lifted.
    pub fn read_only(mut self) -> Self {
        self.migrate = false;
        self
    }

    /// Lifting nested themes before the scan (once per process), unless turned off.
    fn migrate_nested(&self) {
        if self.migrate {
            crate::appearance_installer::migrate_nested_themes_once(&self.home);
        }
        self.link_themes();
    }

    /// `~/.themes` links to themes from `~/.local/share/themes`
    /// ([`sync_theme_links`]) — on every scan, unless it is read-only.
    fn link_themes(&self) {
        if self.migrate {
            for line in sync_theme_links(&self.home) {
                crate::log_info!("theme", "{line}");
            }
        }
    }

    /// The same scan with the sections of environment `de` (the Rexon desktop key).
    pub fn with_de(mut self, de: &str) -> Self {
        self.de = (!de.is_empty()).then(|| de.to_string());
        self.standard_icons = crate::appearance_installer::live_standard_theme(de, "icons");
        self
    }

    /// Disk overview types: the old ones plus the environment's sections, if it is set.
    fn types(&self) -> Vec<ScanType> {
        let mut types = types_for(&self.home, &self.usr_root);
        if let Some(de) = &self.de {
            types.extend(de_types(&self.home, &self.usr_root, de));
        }
        types
    }

    /// What is enabled now in the environment's sections — each section has its
    /// own reader (`de_apply::active_names`), values separated by newlines.
    fn de_active(&self) -> Vec<(String, String)> {
        let Some(de) = &self.de else { return Vec::new() };
        let ctx = crate::de_apply::Ctx {
            home: self.home.clone(),
            xdg_config_home: std::env::var("XDG_CONFIG_HOME").ok().filter(|v| !v.is_empty()),
            // The LXQt border theme is read from the `rc.xml` of the manager chosen
            // for the session: `window_manager` on X11, `compositor` on Wayland.
            wayland: crate::de_apply::Ctx::live().wayland,
            de: de.clone(),
        };
        let mut out = Vec::new();
        for (_, key) in crate::categories::DE_SECTIONS {
            if !crate::categories::category_available(key, de) {
                continue;
            }
            let Some(kind) = crate::categories::category_kind(key) else { continue };
            let names = crate::de_apply::active_names(&crate::animations::SystemIo, &ctx, kind);
            if !names.is_empty() {
                out.push((key.to_string(), names.join("\n")));
            }
        }
        out
    }

    /// All type directories (user + system) without repeats, in TYPES order —
    /// port of `_rs_scan_roots`.
    fn scan_roots(&self) -> Vec<String> {
        let mut roots: Vec<String> = Vec::new();
        for t in self.types() {
            for d in [t.user_dir, t.sys_dir] {
                if !d.is_empty() && !roots.contains(&d) {
                    roots.push(d);
                }
            }
        }
        roots
    }

    /// The project's `expand_user`: a leading `~/.config` goes to `$XDG_CONFIG_HOME`.
    fn expand(&self, path: &str) -> String {
        crate::utils::expand_user(
            path,
            &self.home,
            std::env::var("XDG_CONFIG_HOME").ok().as_deref(),
        )
    }

    /// Snapshot + scanner environment — port of `_rs_env`.
    ///
    /// `with_cache = false` — a scan without the store cache (previews only from
    /// the theme directory).
    fn with_env<R>(
        &self,
        roots: &[String],
        with_cache: bool,
        body: impl FnOnce(&ScanEnv) -> R,
    ) -> R {
        let no_cache: Vec<(String, String)> = Vec::new();
        let extra: Vec<String> = if with_cache {
            self.preview_files.values().cloned().collect()
        } else {
            Vec::new()
        };
        let fs = live_snapshot(roots, &extra);
        let preview_file = |url: &str| {
            if with_cache {
                self.preview_files.get(url).cloned()
            } else {
                None
            }
        };
        // The theme name is in Rexon's INTERFACE language, not the system locale:
        // the person chose the program's language, and `Name[<lang>]` must follow it.
        let locale = crate::i18n::current_language();
        let env = ScanEnv {
            home: &self.home,
            usr_root: &self.usr_root,
            fs: &fs,
            ocs_cache: if with_cache { &self.ocs_cache } else { &no_cache },
            preview_file: &preview_file,
            store: if with_cache { &self.store } else { StoreRecords::none() },
            locale: &locale,
            standard_icons: self.standard_icons.as_deref(),
        };
        body(&env)
    }

    /// Port of `ThemeManager._kde_get`: first `kreadconfig6` (it picks up system
    /// defaults and the active Look-and-Feel), then parsing `kdeglobals` directly.
    /// Empty — the value is nowhere.
    fn kde_get(&self, section: &str, key: &str) -> String {
        let cmd = crate::themes::kde_read_cmd(section, key);
        let timeout = cmd
            .timeout
            .unwrap_or_else(|| crate::utils::default_timeout_secs(&cmd.argv));
        if let Some((0, out, _)) = run_capture(&cmd.argv, timeout, false) {
            let value = out.trim();
            if !value.is_empty() {
                return value.to_string();
            }
        }
        let Some(content) = read_optional(&self.expand("~/.config/kdeglobals")) else {
            return String::new();
        };
        crate::themes::kdeglobals_value(&content, section, key).unwrap_or_default()
    }

    /// Port of `_rs_kread`: `kreadconfig6` stdout on code 0, otherwise `None`.
    /// The file here is ANY (ksplashrc, kwinrc), so the argv is our own, not
    /// `themes::kde_read_cmd` — that one is nailed to `kdeglobals` and `--default ""`.
    fn kread(&self, file: &str, group: &str, key: &str) -> Option<String> {
        let argv: Vec<String> = ["kreadconfig6", "--file", file, "--group", group, "--key", key]
            .iter()
            .map(|s| (*s).to_string())
            .collect();
        match run_default(&argv) {
            Some((0, out, _)) => Some(out),
            _ => None,
        }
    }

    /// Answer of the sound theme read command — port of `_rs_sound_theme`.
    /// What to read it with is known by the sound engine of the «⚙️ Settings» tab:
    /// there is no second way to ask.
    fn sound_theme(&self, de: &str) -> Option<String> {
        let kde_tool = which_live("kreadconfig6").unwrap_or_default();
        let de = if de.is_empty() { "kde" } else { de };
        let argv = crate::qt_settings::sound_read_argv_for(
            de,
            crate::qt_settings::SoundSetting::Theme,
            &kde_tool,
        )?;
        if argv.is_empty() {
            return None;
        }
        match run_default(&argv) {
            Some((0, out, _)) => Some(out),
            _ => None,
        }
    }

    /// The current desktop wallpaper — port of `_rs_current_wallpaper`.
    ///
    /// KDE: all containments of `plasma-org.kde.plasma.desktop-appletsrc` (the
    /// desktop number isn't guessed — parsing is in the wallpaper engine).
    /// Other desktops: the same gsettings branch the engine uses.
    fn current_wallpaper(&self, de: &str) -> Option<String> {
        if crate::wallpaper_engine::uses_plasma_config(de) {
            let text =
                read_optional(&self.expand("~/.config/plasma-org.kde.plasma.desktop-appletsrc"));
            let images =
                crate::wallpaper_engine::kde_wallpaper_images(text.as_deref().unwrap_or(""));
            return images.into_iter().next();
        }
        let (schema, key) = crate::wallpaper_engine::gsettings_schema(de)?;
        let argv = crate::wallpaper_engine::gsettings_get_argv(schema, key);
        let plain = match run_default(&argv) {
            Some((0, out, _)) if !crate::wallpaper_engine::gsettings_path_value(&out).is_empty() => Some(out),
            _ => None,
        };
        if plain.is_some() || schema != crate::wallpaper_engine::CINNAMON_BG_SCHEMA {
            return plain;
        }
        // Cinnamon 6.8+: `picture-uri` is empty, the picture is in the list.
        let list = crate::wallpaper_engine::gsettings_get_argv(schema, crate::wallpaper_engine::CINNAMON_BG_LIST_KEY);
        match run_default(&list) {
            Some((0, out, _)) => crate::wallpaper_engine::cinnamon_list_first_path(&out),
            _ => None,
        }
    }

    /// All sources of the active theme map — port of `_rs_active_sources`.
    ///
    /// The order and LAZINESS of side calls are the same as in the reference: the
    /// cursor chain asks `kcminputrc` and `$XCURSOR_THEME` only when the previous
    /// source is empty, otherwise the tab would read on every refresh files whose
    /// answer is certainly not needed.
    fn active_sources(&self) -> LiveSources {
        let mut src = LiveSources::default();
        let kde_cmds = crate::themes::get_current_kde_cmds();
        let mut kde_values: Vec<String> = Vec::with_capacity(3);
        for cmd in kde_cmds.iter() {
            let timeout = cmd
                .timeout
                .unwrap_or_else(|| crate::utils::default_timeout_secs(&cmd.argv));
            let value = match run_capture(&cmd.argv, timeout, false) {
                Some((0, out, _)) if !out.is_empty() => out.trim().to_string(),
                // "—" is the same "couldn't read" sign as in the Python reference;
                // `clean` higher up in the module turns it into empty.
                _ => "—".to_string(),
            };
            kde_values.push(value);
        }
        src.kde_global = kde_values[0].clone();
        src.kde_plasma_style = kde_values[1].clone();
        src.kde_colors = kde_values[2].clone();

        let de = crate::system::detect_current_de(
            &env_str("XDG_CURRENT_DESKTOP"),
            &env_str("DESKTOP_SESSION"),
            !env_str("WAYLAND_DISPLAY").is_empty(),
        )
        .unwrap_or_default();
        // An environment with its own appearance storage (gsettings, xfconf,
        // DConfig) is asked first; kdeglobals and other fallback sources are
        // read only if it gave no answer.
        let [native_gtk, native_icons, native_cursor] = native_role_values(&de, |cmd| {
            let timeout = cmd
                .timeout
                .unwrap_or_else(|| crate::utils::default_timeout_secs(&cmd.argv));
            match run_capture(&cmd.argv, timeout, false) {
                Some((0, out, _)) if !out.trim().is_empty() => Some(out),
                _ => None,
            }
        });
        if native_icons.is_none() {
            src.icons = self.kde_get("Icons", "Theme");
        }
        let mouse = if native_cursor.is_none() {
            self.kde_get("Mouse", "cursorTheme")
        } else {
            String::new()
        };
        if native_cursor.is_none() && clean(&mouse).is_empty() {
            let kcminputrc = read_optional(&self.expand("~/.config/kcminputrc"));
            src.kcminput_cursor = crate::themes::kde_kcminput_cursor(kcminputrc.as_deref());
            if clean(&src.kcminput_cursor).is_empty() {
                src.xcursor_env = env_str("XCURSOR_THEME");
            }
        }
        src.mouse_cursor = mouse;
        src.native_gtk = native_gtk;
        src.native_icons = native_icons;
        src.native_cursor = native_cursor;
        src.widget_style = self.kde_get("KDE", "widgetStyle");

        src.gtk_settings_ini = read_optional(&self.expand("~/.config/gtk-3.0/settings.ini"));
        // The splash is off (`Engine` is not KSplashQML) — the current tile is «None».
        let engine = self.kread("ksplashrc", "KSplash", "Engine");
        src.splash = crate::appearance_installer::splash_active_value(
            self.kread("ksplashrc", "KSplash", "Theme"),
            engine.as_deref(),
        );
        src.lockscreen = self.kread("kscreenlockerrc", "Greeter", "Theme");
        src.kwin_decoration_theme = self.kread("kwinrc", "org.kde.kdecoration2", "theme");
        src.kwin_decoration_library = self.kread("kwinrc", "org.kde.kdecoration2", "library");
        src.kvantum_kvconfig = read_optional(&self.expand("~/.config/Kvantum/kvantum.kvconfig"));
        src.qt_style_override_env = env_str("QT_STYLE_OVERRIDE");
        src.qt_style_env_file =
            read_optional(&self.expand("~/.config/environment.d/rexon-qt-style.conf"));
        src.lxqt_conf = read_optional(&self.expand("~/.config/lxqt/lxqt.conf"));
        src.qtct_conf = ["qt6ct", "qt5ct"]
            .iter()
            .find_map(|t| read_optional(&self.expand(&format!("~/.config/{t}/{t}.conf"))));

        // Plasma Login Manager files no longer go here: it has no
        // `[Theme] Current` key (see `active_map_from_sources`). The read order
        // is SDDM's own (`/usr/lib/sddm/sddm.conf.d`, `/etc/sddm.conf.d`,
        // `/etc/sddm.conf` last): otherwise `Current=` from `/etc/sddm.conf`
        // (CachyOS puts `breeze` there) lost to the directory's files, and the
        // wrong theme was highlighted, not the one on the login screen.
        let list = |dir: &str| scandir_names(std::path::Path::new(dir));
        let sddm_paths = crate::system_plan::sddm_theme_files(&list);
        src.sddm_confs = sddm_paths.iter().filter_map(|p| read_optional(p)).collect();
        src.plasma_login = self.plasma_login.unwrap_or_else(|| {
            crate::system_plan::is_plasma_login(
                &crate::appearance_installer::active_login_manager(),
            )
        });
        let alt = format!("{}/default.plymouth", crate::system_plan::PLYMOUTH_THEMES_DIR);
        let alt_theme = std::fs::canonicalize(&alt)
            .ok()
            .and_then(|real| crate::system_plan::plymouth_theme_from_link(real.to_str()));
        src.plymouth_confs = plymouth_confs_by_weight(
            read_optional(crate::system_plan::PLYMOUTH_DEFAULTS),
            alt_theme.as_deref(),
            read_optional("/usr/share/plymouth/plymouthd.conf"),
            read_optional("/etc/plymouth/plymouthd.conf"),
        );

        src.sound_theme = self.sound_theme(&de);
        src.wallpaper_current = self.current_wallpaper(&de);
        src
    }

    /// The map of themes applied now (port of `_rs_active_map`).
    pub fn active_map(&self) -> HashMap<String, String> {
        let raw = self.active_sources();
        let sources = ActiveMapSources {
            kde_global: &raw.kde_global,
            kde_plasma_style: &raw.kde_plasma_style,
            kde_colors: &raw.kde_colors,
            icons: &raw.icons,
            mouse_cursor: &raw.mouse_cursor,
            kcminput_cursor: &raw.kcminput_cursor,
            xcursor_env: &raw.xcursor_env,
            gtk_settings_ini: raw.gtk_settings_ini.as_deref(),
            widget_style: &raw.widget_style,
            splash: raw.splash.as_deref(),
            lockscreen: raw.lockscreen.as_deref(),
            kwin_decoration_theme: raw.kwin_decoration_theme.as_deref(),
            kwin_decoration_library: raw.kwin_decoration_library.as_deref(),
            kvantum_kvconfig: raw.kvantum_kvconfig.as_deref(),
            sddm_confs: sddm_confs_for_active_map(
                raw.sddm_confs.iter().map(String::as_str).collect(),
                raw.plasma_login,
            ),
            plymouth_confs: raw.plymouth_confs.iter().map(String::as_str).collect(),
            qt_style_override_env: &raw.qt_style_override_env,
            qt_style_env_file: raw.qt_style_env_file.as_deref(),
            lxqt_conf: raw.lxqt_conf.as_deref(),
            qtct_conf: raw.qtct_conf.as_deref(),
            native_gtk: raw.native_gtk.as_deref(),
            native_icons: raw.native_icons.as_deref(),
            native_cursor: raw.native_cursor.as_deref(),
            sound_theme: raw.sound_theme.as_deref(),
            wallpaper_current: raw.wallpaper_current.as_deref(),
            home: &self.home,
            usr_root: &self.usr_root,
        };
        let mut map: HashMap<String, String> = active_map_from_sources(&sources)
            .into_iter()
            .map(|(k, v)| (k, v.as_str().unwrap_or("").to_string()))
            .collect();
        map.extend(self.de_active());
        map
    }

    /// Themes applied NOW (any source).
    /// Conky context for the scan: the session's `$XDG_CONFIG_HOME` — only for
    /// one's own home, a scan of a foreign tree doesn't read live directories.
    fn conky_ctx(&self) -> crate::de_apply::Ctx {
        crate::de_apply::Ctx::for_home(&self.home)
    }

    /// A Conky package of several widgets — a tile per widget
    /// ([`crate::conky_scan::split_package_items`]).
    fn split_conky(&self, items: Vec<ThemeItem>, active_map: &HashMap<String, String>) -> Vec<ThemeItem> {
        if !items.iter().any(|it| it.id.starts_with("local:conky:")) {
            return items;
        }
        let running: Vec<String> = active_names(active_map, "conky").into_iter().map(str::to_string).collect();
        crate::conky_scan::split_package_items(&crate::animations::SystemIo, &self.conky_ctx(), items, &running)
    }

    pub fn active(&self, active_map: &HashMap<String, String>) -> Vec<ThemeItem> {
        self.migrate_nested();
        let types = self.types();
        let items = self.with_env(&self.scan_roots(), true, |env| scan_active_in(env, &types, active_map));
        let mut items = self.split_conky(items, active_map);
        // The built-in KDE border (Breeze by default) — only in a KDE session:
        // on another desktop `kwinrc` draws nothing.
        let kde = self.de.as_deref().and_then(crate::de_customization::de_from_id)
            == Some(crate::de_customization::De::Kde);
        if kde && active_map.contains_key("kdecoration") {
            items.extend(self.kdecorations(active_map).into_iter().filter(|it| it.local_kind.is_some()));
        }
        items
    }

    /// User themes in `~/.local/share/…`, NOT active.
    pub fn downloaded(&self, active_map: &HashMap<String, String>) -> Vec<ThemeItem> {
        // Themes that older installs placed one level deeper are visible only
        // after lifting (once per process; see appearance_installer).
        self.migrate_nested();
        let types = self.types();
        let journal = crate::appearance_installer::installed_log_paths();
        let loose = self.loose_system_themes();
        let items = self.with_env(&self.scan_roots(), true, |env| {
            let mut items = scan_downloaded_in(env, &types, active_map);
            items.extend(scan_rexon_system(env, &types, active_map, &journal, &loose));
            items
        });
        // Only the ones not running here (running ones are in «Enabled»).
        let mut items = self.split_conky(items, active_map);
        items.retain(|it| !(it.id.starts_with("local:conky:") && it.local_kind.as_deref() == Some("active")));
        items
    }

    /// Login screen and boot splash theme directories that have no owning
    /// package: they were placed by hand or by another installer.
    ///
    /// Empty if the package database doesn't answer "whose file is this" (a
    /// family without such a query, no program itself, the query didn't run):
    /// without the answer "nobody's" can't be told from "packaged", and a package
    /// theme in «Downloaded» with a «Remove» button would be a lie.
    pub fn loose_system_themes(&self) -> HashSet<String> {
        let mut paths: Vec<String> = Vec::new();
        for t in self.types() {
            if !t.user_dir.is_empty() || t.sys_dir.is_empty() {
                continue;
            }
            let Ok(dir) = std::fs::read_dir(&t.sys_dir) else { continue };
            for entry in dir.flatten() {
                // We don't dereference a link: the helper removes only an ordinary directory.
                if entry.file_type().map(|f| f.is_dir()).unwrap_or(false) {
                    paths.push(join(&t.sys_dir, &entry.file_name().to_string_lossy()));
                }
            }
        }
        live_loose_paths(crate::system::DistroDetector::detect_live().family_live(), &paths)
    }

    /// Themes in `/usr/share/…` with a package FROM THE REPOSITORIES, NOT active.
    ///
    /// `Err(text)` — package origin couldn't be determined (port of
    /// `SystemScanUnavailable`): the tab shows an explicit status, and AUR themes
    /// don't leak into «System». A system that fundamentally knows nothing about
    /// package origin honestly returns an empty list.
    pub fn system(
        &self,
        active_map: &HashMap<String, String>,
    ) -> Result<Vec<ThemeItem>, String> {
        let family = crate::system::DistroDetector::detect_live().family_live();
        let Some(query) = crate::system_plan::foreign_packages_query(family) else {
            return Ok(Vec::new()); // the family can't answer about origin
        };
        if which_live(&query.binary).is_none() {
            return Ok(Vec::new());
        }
        let foreign = live_foreign_pkgs(&query);
        // The scan has two phases: the first call with an empty owner map returns
        // paths for the "whose file is this" question, the second — the items.
        let types = self.types();
        self.with_env(&self.scan_roots(), true, |env| {
            let Some(foreign) = foreign.as_ref() else {
                // The same refusal as `scan_system` without a pacman -Qm answer.
                return scan_system(env, active_map, true, None, &HashMap::new()).map(|s| s.items);
            };
            let empty: HashMap<String, String> = HashMap::new();
            let first = scan_system_in(env, &types, active_map, foreign, &empty)?;
            let owners = live_owner_pkgs(family, &first.qo_paths);
            let second = scan_system_in(env, &types, active_map, foreign, &owners)?;
            Ok(second.items)
        })
    }

    /// The whole section `apply_kind` ([`scan_section`]): a desktop environment
    /// section (`categories::DE_SECTIONS`) or a KDE section with an installed grid
    /// (`categories::KDE_INSTALLED_SECTIONS`). The current theme — from
    /// `active_map` (the key is the same `apply_kind`), with `local_kind = "active"`.
    pub fn section(&self, apply_kind: &str, active_map: &HashMap<String, String>) -> Vec<ThemeItem> {
        let types = section_types(&self.home, &self.usr_root, apply_kind);
        if crate::categories::KDE_INSTALLED_SECTIONS.iter().any(|(_, k)| *k == apply_kind) {
            // As with «Downloaded»: themes older installs placed one level deeper
            // are visible only after lifting.
            self.migrate_nested();
        } else {
            self.link_themes();
        }
        let mut roots: Vec<String> = Vec::new();
        for t in &types {
            for d in [&t.user_dir, &t.sys_dir] {
                if !d.is_empty() && !roots.contains(d) {
                    roots.push(d.clone());
                }
            }
        }
        let mut items = self.with_env(&roots, true, |env| scan_section(env, &types, active_map));
        order_toggle_section(apply_kind, &mut items);
        // Built-in KDE borders (Breeze, Oxygen, QML Aurorae themes) — next to the
        // SVG themes, as in the KDE dialog; the current one — by `library` from `kwinrc`.
        if apply_kind == "aurorae" {
            let builtin = self.kdecorations(active_map);
            let (current, rest): (Vec<_>, Vec<_>) = builtin.into_iter().partition(|it| it.local_kind.is_some());
            items.splice(0..0, current);
            items.extend(rest);
        }
        // Conky widgets outside the Rexon directory (`~/.conky`, `~/.conkyrc`, own
        // folders) — as «🔎 Found on disk» tiles after our own.
        if apply_kind == "conky" {
            let ctx = self.conky_ctx();
            items = self.split_conky(items, active_map);
            items.extend(crate::conky_scan::live_found_items(&self.home));
            // The picture next to the config file and «Source»/«Authors» from
            // source.txt/credits.txt — both for our own and for those found on disk.
            for it in &mut items {
                crate::conky_scan::decorate_item(&crate::animations::SystemIo, &ctx, it);
            }
        }
        items
    }

    /// Tiles of built-in KDE borders ([`crate::kdecoration`]); the section puts
    /// the enabled one first, before the SVG themes.
    fn kdecorations(&self, active_map: &HashMap<String, String>) -> Vec<ThemeItem> {
        use crate::kdecoration as kd;
        let roots = if self.usr_root == "/usr" {
            kd::Roots::live(&self.home)
        } else {
            kd::Roots::under(&self.home, &self.usr_root)
        };
        let (decos, _) = kd::decorations(&kd::scan(&roots), &crate::i18n::current_language());
        let current = active_map.get("kdecoration").map(String::as_str).unwrap_or("");
        let Some(t) = types_for(&self.home, &self.usr_root).into_iter().find(|t| t.apply_kind == "aurorae") else {
            return Vec::new();
        };
        kd::section_items(&decos, current, t.cat_id, t.icon)
    }

    /// Kvantum themes (`~/.config/Kvantum` + `/usr/share/Kvantum`).
    pub fn kvantum(&self, active_map: &HashMap<String, String>) -> Vec<ThemeItem> {
        let roots = kvantum_dirs(&self.home, &self.usr_root).to_vec();
        self.with_env(&roots, true, |env| scan_kvantum_themes(env, active_map))
    }

    /// Splash (`kind = "splash"`) or lock screen (`kind = "lockscreen"`) from the
    /// look-and-feel.
    pub fn lnf(&self, kind: &str, active_map: &HashMap<String, String>) -> Vec<ThemeItem> {
        let roots = lnf_dirs(&self.home, &self.usr_root).to_vec();
        self.with_env(&roots, true, |env| scan_lnf_component(env, kind, active_map))
    }

    /// Application style (Qt widget style). `styles` — Qt style names; for a
    /// process without QtWidgets it is [`WIDGET_STYLES`]. Doesn't touch the disk:
    /// the style list comes from Qt itself.
    pub fn widget_styles(
        &self,
        active_map: &HashMap<String, String>,
        styles: &[String],
        qt_only: bool,
    ) -> Vec<ThemeItem> {
        scan_widget_styles(active_map, styles, qt_only)
    }
}

// ============================================================================
// Checks of the live executor
// ============================================================================
//
// What is checked is exactly what no golden on a fixture will catch: how the
// live snapshot puts KEYS. `is_dir`/`is_file` of [`FsSnapshot`] answer by key
// presence, so a forgotten registration of a subdirectory or a preview file
// is not "slightly different" but a silent "there is no such thing", and the
// tab's section goes empty without a single error.
#[cfg(test)]
mod tests {
    /// Toggle sections — by name (enabling doesn't move the card), theme
    /// sections — as they were (the enabled one first).
    #[test]
    fn toggle_sections_keep_order_by_name() {
        for key in ["gnomeext", "kwineffects", "kwinscripts", "cinnamonapplets", "cinnamondesklets", "cinnamonext", "conky"] {
            assert!(toggle_section(key), "{key}");
        }
        for key in ["gtk", "icons", "cursor", "global", "xfwm4", "gnomeshell", "tabbox", "nemoactions", ""] {
            assert!(!toggle_section(key), "{key}");
        }
        let tile = |id: &str, name: &str, active: bool| ThemeItem {
            id: format!("local:gnomeext:{id}"),
            name: name.into(),
            local_kind: active.then(|| "active".to_string()),
            ..Default::default()
        };
        let names = |items: &[ThemeItem]| items.iter().map(|i| i.name.clone()).collect::<Vec<_>>();
        // The overview puts the enabled one first: before and after «Disable» on Caffeine.
        let mut before = vec![tile("c", "Caffeine", true), tile("a", "appindicator", false), tile("d", "Dash to Dock", false)];
        let mut after = vec![tile("a", "appindicator", false), tile("c", "Caffeine", false), tile("d", "Dash to Dock", false)];
        order_toggle_section("gnomeext", &mut before);
        order_toggle_section("gnomeext", &mut after);
        assert_eq!(names(&before), ["appindicator", "Caffeine", "Dash to Dock"]);
        assert_eq!(names(&before), names(&after), "переключение не двигает карточки");
        // Equal names — by id, stably.
        let mut twins = vec![tile("z", "Same", true), tile("b", "Same", false)];
        order_toggle_section("gnomeext", &mut twins);
        assert_eq!(twins[0].id, "local:gnomeext:b");
        // Theme: the overview order is left alone.
        let mut themes = vec![tile("x", "Zeta", true), tile("y", "Alpha", false)];
        order_toggle_section("gtk", &mut themes);
        assert_eq!(names(&themes), ["Zeta", "Alpha"]);
    }

    /// A new theme in `~/.themes` changes the roots stamp.
    #[test]
    fn roots_stamp_sees_new_theme() {
        let home = std::env::temp_dir().join(format!("rexon-roots-stamp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(home.join(".themes")).unwrap();
        let h = home.to_string_lossy().to_string();
        let before = roots_stamp(&h, "/nonexistent-usr");
        assert_eq!(before, roots_stamp(&h, "/nonexistent-usr"), "без изменений — та же");
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::create_dir_all(home.join(".themes/Fresh/gtk-3.0")).unwrap();
        assert_ne!(before, roots_stamp(&h, "/nonexistent-usr"));
        let _ = std::fs::remove_dir_all(&home);
    }

    /// The live Qt list wins over the fallback; an empty answer — the whole fallback.
    #[test]
    fn available_widget_styles_prefers_live_qt_keys() {
        let live: Vec<String> = ["Windows", "Fusion", " Breeze ", "", "qt6ct-style"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(
            super::available_widget_styles(&live),
            vec!["Windows", "Fusion", "Breeze", "qt6ct-style"]
        );
        let fallback: Vec<String> =
            super::WIDGET_STYLES.iter().map(|s| s.to_string()).collect();
        assert_eq!(super::available_widget_styles(&[]), fallback);
        assert_eq!(super::available_widget_styles(&[" ".to_string()]), fallback);
    }

    use super::*;

    /// Our own directory for the check in `std::env::temp_dir()`.
    fn tmp_dir(tag: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!(
            "rexon-installed-scan-{tag}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("временный каталог");
        path
    }

    fn write(path: &std::path::Path, text: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("каталог файла");
        }
        std::fs::write(path, text).expect("запись файла");
    }

    fn s(p: &std::path::Path) -> String {
        p.to_string_lossy().into_owned()
    }

    /// Themes from `~/.local/share/themes` get a link in `~/.themes` (only it is
    /// read by the MATE «Appearance» dialog): a taken name is left alone, a
    /// non-theme gets no link, a link to a vanished theme is removed, the scan
    /// snapshot has no link (the theme is visible once), and removing the theme
    /// takes its link along.
    #[test]
    fn theme_links_mirror_xdg_themes_into_dot_themes() {
        let home = tmp_dir("links");
        let h = s(&home);
        let xdg = home.join(".local/share/themes");
        write(&xdg.join("Nord/gtk-3.0/gtk.css"), "x");
        write(&xdg.join("Frame/metacity-1/metacity-theme-3.xml"), "x");
        write(&xdg.join("Taken/gtk-3.0/gtk.css"), "x");
        write(&xdg.join("junk/readme.txt"), "x");
        write(&home.join(".themes/Taken/gtk-3.0/gtk.css"), "свой");
        std::fs::create_dir_all(home.join(".themes")).unwrap();
        std::os::unix::fs::symlink("../.local/share/themes/Gone", home.join(".themes/Gone")).unwrap();

        let log = sync_theme_links(&h);
        assert_eq!(log.len(), 3, "{log:?}");
        let legacy = home.join(".themes");
        for name in ["Nord", "Frame"] {
            assert!(is_theme_link(&h, name), "{name}");
            assert_eq!(
                std::fs::read_link(legacy.join(name)).unwrap(),
                std::path::Path::new("../.local/share/themes").join(name)
            );
            assert!(legacy.join(name).join(if name == "Nord" { "gtk-3.0" } else { "metacity-1" }).is_dir());
        }
        assert!(legacy.join("Taken").is_dir() && !is_theme_link(&h, "Taken"), "своя тема не тронута");
        assert!(std::fs::symlink_metadata(legacy.join("junk")).is_err(), "не тема — без ссылки");
        assert!(std::fs::symlink_metadata(legacy.join("Gone")).is_err(), "ссылка в пустоту снята");
        assert!(sync_theme_links(&h).is_empty(), "второй прогон ничего не меняет");

        let snap = live_snapshot(&[s(&legacy), s(&xdg)], &[]);
        assert_eq!(snap.scandir(&s(&legacy)), &["Taken".to_string()]);
        assert!(snap.scandir(&s(&xdg)).contains(&"Nord".to_string()));

        std::fs::remove_dir_all(xdg.join("Nord")).unwrap();
        assert!(drop_theme_link(&h, &s(&xdg.join("Nord"))));
        assert!(std::fs::symlink_metadata(legacy.join("Nord")).is_err());
        assert!(!drop_theme_link(&h, &s(&legacy.join("Taken"))), "чужой путь — ничего");
        assert!(legacy.join("Taken").is_dir());
        let _ = std::fs::remove_dir_all(&home);
    }

    /// Snapshot of the live tree: a theme subdirectory is registered EMPTY, text
    /// is read only for metadata, and the `Preview=` target gets into the snapshot.
    ///
    /// Catches three different misses at once: not registering a subdirectory
    /// (`contents/` would vanish, and `scan_lnf_component` wouldn't find a single
    /// theme), reading an ordinary file as text (themes carry megabyte `.svg`
    /// files, and the tab would stall on every opening) and not registering the
    /// file from `Preview=` (the preview would quietly become a placeholder).
    #[test]
    fn live_snapshot_keys_dirs_files_and_preview_target() {
        let root = tmp_dir("snap");
        let theme = root.join("Alpha");
        let meta = "[Desktop Entry]\nName=Alpha\nPreview=shot.png\n";
        write(&theme.join("metadata.desktop"), meta);
        write(&theme.join("big.svg"), "<svg/>");
        write(&theme.join("contents/shot.png"), "PNG");
        write(&root.join("loose.txt"), "x");

        let snap = live_snapshot(&[s(&root)], &[]);

        assert!(snap.is_dir(&s(&root)));
        let names = snap.scandir(&s(&root));
        assert!(names.contains(&"Alpha".to_string()));
        assert!(names.contains(&"loose.txt".to_string()));
        assert!(snap.is_dir(&s(&theme)));
        // the theme subdirectory is registered, but we didn't go deeper
        assert!(snap.is_dir(&s(&theme.join("contents"))));
        assert!(snap.scandir(&s(&theme.join("contents"))).is_empty());
        // metadata — as text, an ordinary file — only as a fact of existence
        assert_eq!(snap.read_text(&s(&theme.join("metadata.desktop"))), Some(meta));
        assert!(snap.is_file(&s(&theme.join("big.svg"))));
        assert_eq!(snap.read_text(&s(&theme.join("big.svg"))), Some(""));
        // the `Preview=` target is looked up relative to contents/ — and found
        assert!(snap.is_file(&s(&theme.join("contents/shot.png"))));
        assert_eq!(
            find_preview(&snap, &s(&theme), None),
            s(&theme.join("contents/shot.png"))
        );

        std::fs::remove_dir_all(&root).ok();
    }

    /// A nonexistent root — an empty snapshot without a panic, while preview files
    /// passed from outside still get into it.
    ///
    /// Both cases are real: the `/usr/share/sddm/themes` directory simply doesn't
    /// exist on a machine without SDDM, and preview files come from the store
    /// cache already checked and lie on disk outside the walk roots.
    #[test]
    fn live_snapshot_missing_root_is_empty_but_keeps_extra_files() {
        let missing = "/rexon-нет-такого-каталога/themes".to_string();
        let extra = "/var/cache/rexon/preview-42.png".to_string();

        let snap = live_snapshot(&[missing.clone()], std::slice::from_ref(&extra));

        assert!(!snap.is_dir(&missing));
        assert!(snap.scandir(&missing).is_empty());
        assert!(snap.is_file(&extra));
    }

    /// `from_parts` and `from_json` on the same data answer the same.
    ///
    /// The live overview goes through the first constructor, the bridge goldens
    /// through the second; if they diverged in even one answer, the fixture checks
    /// would stay green while the tab showed something else.
    #[test]
    fn from_parts_answers_like_from_json() {
        let dirs = HashMap::from([
            ("/t/themes".to_string(), vec!["A".to_string(), "note.txt".to_string()]),
            ("/t/themes/A".to_string(), vec!["metadata.json".to_string()]),
            ("/t/themes/A/contents".to_string(), Vec::new()),
        ]);
        let files = HashMap::from([
            ("/t/themes/note.txt".to_string(), String::new()),
            ("/t/themes/A/metadata.json".to_string(), "{\"x\":1}".to_string()),
        ]);
        let from_parts = FsSnapshot::from_parts(dirs.clone(), files.clone());
        let json = serde_json::json!({"dirs": dirs, "files": files});
        let from_json = FsSnapshot::from_json(&json);

        for path in [
            "/t/themes",
            "/t/themes/A",
            "/t/themes/A/contents",
            "/t/themes/note.txt",
            "/t/themes/A/metadata.json",
            "/t/нет",
        ] {
            assert_eq!(from_parts.is_dir(path), from_json.is_dir(path), "is_dir {path}");
            assert_eq!(from_parts.is_file(path), from_json.is_file(path), "is_file {path}");
            assert_eq!(
                from_parts.read_text(path),
                from_json.read_text(path),
                "read_text {path}"
            );
            assert_eq!(from_parts.scandir(path), from_json.scandir(path), "scandir {path}");
        }
    }

    /// Live look-and-feel overview: a theme with `contents/splash` gets into
    /// «Splash Screen» and is marked active, but not into «Lock Screen».
    ///
    /// This is an end-to-end check through a real disk: the snapshot is built by
    /// the live walk, and the decisions are made by the same `scan_lnf_component`
    /// as the bridge. Should the executor skip the `contents/splash` probe, the
    /// section would be empty.
    #[test]
    fn live_lnf_sees_splash_component_on_disk() {
        let base = tmp_dir("lnf");
        let home = base.join("home");
        let usr = base.join("usr");
        let theme = home.join(".local/share/plasma/look-and-feel/MyTheme");
        write(
            &theme.join("metadata.json"),
            "{\"KPlugin\": {\"Name\": \"My Theme\"}}",
        );
        std::fs::create_dir_all(theme.join("contents/splash")).expect("contents/splash");

        let scan = LiveScan {
            home: s(&home),
            usr_root: s(&usr),
            ocs_cache: Vec::new(),
            preview_files: HashMap::new(),
            store: StoreRecords::default(),
            plasma_login: Some(false),
            de: None,
            migrate: false,
            standard_icons: None,
        };
        let active = HashMap::from([("splash".to_string(), "MyTheme".to_string())]);

        let splash = scan.lnf("splash", &active);
        // The theme and the «None» tile (always last).
        assert_eq!(splash.len(), 2, "тема со splash обязана быть в списке");
        assert_eq!(splash[1].id, "local:splash:None");
        // The card has the name from the metadata, the id — the directory name.
        assert_eq!(splash[0].name, "My Theme");
        assert_eq!(splash[0].id, "local:splash:MyTheme");
        // The active one is recognised by the sign, not the label: the label is translated.
        assert_eq!(splash[0].local_kind.as_deref(), Some("active"), "автор: {}", splash[0].author);
        assert_eq!(splash[0].install_path, s(&theme));

        // the theme has no lockscreen directory — and it must not be in the list
        assert!(scan.lnf("lockscreen", &active).is_empty());

        std::fs::remove_dir_all(&base).ok();
    }

    // ── The theme's human name ────────────────────────────────────────
    //
    // In the «Appearance» tab the user saw the raw directory identifier
    // (`org.kde.breeze.desktop`), although the human name lies right next to it in
    // the metadata. The branches below are the whole name resolution order and
    // EVERY fallback: an empty fallback would turn the card into a nameless one.

    /// A snapshot from "path → text" pairs (directories are derived from file paths).
    fn snap_of(theme_dir: &str, files: &[(&str, &str)]) -> FsSnapshot {
        let mut dirs: HashMap<String, Vec<String>> = HashMap::new();
        let mut fmap: HashMap<String, String> = HashMap::new();
        dirs.insert(
            theme_dir.to_string(),
            files.iter().map(|(n, _)| n.to_string()).collect(),
        );
        for (name, text) in files {
            fmap.insert(join(theme_dir, name), (*text).to_string());
        }
        FsSnapshot { dirs, files: fmap }
    }

    #[test]
    fn description_comes_from_metadata_or_index_theme() {
        let fs = snap_of(
            "/t/ocean",
            &[("index.theme", "[Sound Theme]\nName=Ocean\nComment=Sounds of the sea\nComment[ru]=Звуки моря\n")],
        );
        assert_eq!(theme_description(&fs, "/t/ocean", "ru_RU.UTF-8"), "Звуки моря");
        assert_eq!(theme_description(&fs, "/t/ocean", "de_DE.UTF-8"), "Sounds of the sea");
        let fs = snap_of(
            "/t/lnf",
            &[("metadata.json", r#"{"KPlugin":{"Name":"A","Description":"Dark look"}}"#)],
        );
        assert_eq!(theme_description(&fs, "/t/lnf", "ru_RU.UTF-8"), "Dark look");
        let fs = snap_of("/t/bare", &[("index.theme", "[Icon Theme]\nName=Bare\n")]);
        assert_eq!(theme_description(&fs, "/t/bare", "ru_RU.UTF-8"), "");
        assert_eq!(theme_description(&fs, "", "ru_RU.UTF-8"), "");
    }

    #[test]
    fn display_name_prefers_json_kplugin_name() {
        let fs = snap_of(
            "/t/org.rexon.sessionsplash",
            &[(
                "metadata.json",
                "{\"KPlugin\": {\"Name\": \"Rexon Session Splash\"}}",
            )],
        );
        assert_eq!(
            theme_display_name(
                &fs,
                "/t/org.rexon.sessionsplash",
                "org.rexon.sessionsplash",
                "ru_RU.UTF-8",
            ),
            "Rexon Session Splash"
        );
    }

    #[test]
    fn display_name_falls_back_to_desktop_file() {
        // SDDM themes keep the name in `[SddmGreeterTheme]`, not in `[Desktop
        // Entry]`: tying to one group would lose the whole login screen.
        let fs = snap_of(
            "/t/breeze",
            &[("metadata.desktop", "[SddmGreeterTheme]\nName=Breeze\n")],
        );
        assert_eq!(
            theme_display_name(&fs, "/t/breeze", "breeze", "en_US.UTF-8"),
            "Breeze"
        );
    }

    #[test]
    fn display_name_prefers_translation_for_current_language() {
        let json = "{\"KPlugin\": {\"Name\": \"Breeze\", \"Name[ru]\": \"Бриз\"}}";
        let desktop = "[Desktop Entry]\nName=Elarun\nName[ru]=Эларун\n";
        let fs = snap_of("/t/a", &[("metadata.json", json)]);
        assert_eq!(theme_display_name(&fs, "/t/a", "a", "ru_RU.UTF-8"), "Бриз");
        // a different language → the generic name
        assert_eq!(theme_display_name(&fs, "/t/a", "a", "de_DE.UTF-8"), "Breeze");
        // in the desktop file the translation comes AFTER the generic name and still
        // wins: keys are tried by language, not by file lines
        let fs = snap_of("/t/b", &[("metadata.desktop", desktop)]);
        assert_eq!(theme_display_name(&fs, "/t/b", "b", "ru"), "Эларун");
    }

    #[test]
    fn display_name_reads_lowercase_name_of_shell_addons() {
        // metadata.json of a GNOME extension and a Cinnamon applet: the name is `name`.
        let json = "{\"uuid\": \"rexon-wallpaper@rexon.app\", \"name\": \"Rexon Wallpaper Engine\"}";
        let fs = snap_of("/t/x", &[("metadata.json", json)]);
        assert_eq!(
            theme_display_name(&fs, "/t/x", "rexon-wallpaper@rexon.app", "ru_RU.UTF-8"),
            "Rexon Wallpaper Engine"
        );
    }

    #[test]
    fn display_name_keeps_directory_name_when_metadata_is_useless() {
        // broken json
        let fs = snap_of("/t/x", &[("metadata.json", "{not json")]);
        assert_eq!(theme_display_name(&fs, "/t/x", "x", "ru_RU.UTF-8"), "x");
        // the name is there but empty (spaces) — both json and desktop
        let fs = snap_of("/t/y", &[("metadata.json", "{\"KPlugin\": {\"Name\": \"   \"}}")]);
        assert_eq!(theme_display_name(&fs, "/t/y", "y", "ru_RU.UTF-8"), "y");
        let fs = snap_of("/t/z", &[("metadata.desktop", "[Desktop Entry]\nName=\n")]);
        assert_eq!(theme_display_name(&fs, "/t/z", "z", "ru_RU.UTF-8"), "z");
        // json is there but has no name at all → try the desktop file next to it
        let fs = snap_of(
            "/t/w",
            &[
                ("metadata.json", "{\"KPlugin\": {\"Id\": \"w\"}}"),
                ("metadata.desktop", "[Desktop Entry]\nName=Запасное\n"),
            ],
        );
        assert_eq!(theme_display_name(&fs, "/t/w", "w", "ru_RU.UTF-8"), "Запасное");
        // no metadata at all — neither KDE nor index.theme
        let fs = snap_of("/t/v", &[("cursors", "")]);
        assert_eq!(theme_display_name(&fs, "/t/v", "v", "ru_RU.UTF-8"), "v");
        // index.theme is there but has no name (GTK themes sometimes have a bare
        // [X-GNOME-Metatheme] with a single GtkTheme=)
        let fs = snap_of(
            "/t/u",
            &[("index.theme", "[X-GNOME-Metatheme]\nGtkTheme=u\n")],
        );
        assert_eq!(theme_display_name(&fs, "/t/u", "u", "ru_RU.UTF-8"), "u");
        // there is no theme directory (color schemes are a file, not a folder)
        let fs = snap_of("/t/none", &[]);
        assert_eq!(theme_display_name(&fs, "", "BreezeDark", "ru_RU.UTF-8"), "BreezeDark");
    }

    // ── Icons, cursors, GTK themes: the name from index.theme ──────────
    //
    // They have no KDE metadata at all, and until now the user read the
    // directory name: "breeze_cursors" instead of "Breeze Dark", "adw-gtk3"
    // instead of "adw-gtk3". Samples are taken from Wehrwolfmann's LIVE machine.

    #[test]
    fn display_name_reads_index_theme_of_icons_and_cursors() {
        // /usr/share/icons/Papirus — the [Icon Theme] section
        let fs = snap_of(
            "/t/Papirus",
            &[(
                "index.theme",
                "[Icon Theme]\nName=Papirus\nComment=Papirus icon theme\n",
            )],
        );
        assert_eq!(
            theme_display_name(&fs, "/t/Papirus", "Papirus", "ru_RU.UTF-8"),
            "Papirus"
        );
        // /usr/share/icons/breeze_cursors — the directory name and the theme name DIFFER
        let fs = snap_of(
            "/t/breeze_cursors",
            &[(
                "index.theme",
                "[Icon Theme]\nName=Breeze Dark\nName[de]=Breeze Dunkel\n\
                 Name[ru]=Breeze, тёмный вариант\n",
            )],
        );
        assert_eq!(
            theme_display_name(&fs, "/t/breeze_cursors", "breeze_cursors", "en_US.UTF-8"),
            "Breeze Dark"
        );
        // the translation into the user's language wins although it comes later
        assert_eq!(
            theme_display_name(&fs, "/t/breeze_cursors", "breeze_cursors", "ru_RU.UTF-8"),
            "Breeze, тёмный вариант"
        );
    }

    #[test]
    fn display_name_reads_index_theme_of_gtk_themes() {
        // GTK themes put the name into DIFFERENT groups — tying to one would lose
        // half: adw-gtk3 in [X-GNOME-Metatheme], WhiteSur in [Desktop
        // Entry] (checked on this machine's /usr/share/themes).
        let fs = snap_of(
            "/t/adw-gtk3-dark",
            &[(
                "index.theme",
                "[X-GNOME-Metatheme]\nName=adw-gtk3-dark\nGtkTheme=adw-gtk3-dark\n",
            )],
        );
        assert_eq!(
            theme_display_name(&fs, "/t/adw-gtk3-dark", "adw-gtk3-dark", "ru_RU.UTF-8"),
            "adw-gtk3-dark"
        );
        let fs = snap_of(
            "/t/ContrastHigh",
            &[(
                "index.theme",
                "[Desktop Entry]\nType=X-GNOME-Metatheme\nName=High Contrast\n\
                 \n[X-GNOME-Metatheme]\nGtkTheme=ContrastHigh\n",
            )],
        );
        assert_eq!(
            theme_display_name(&fs, "/t/ContrastHigh", "ContrastHigh", "ru_RU.UTF-8"),
            "High Contrast"
        );
    }

    #[test]
    fn display_name_keeps_kde_metadata_ahead_of_index_theme() {
        // Source order: metadata.* is written by the theme author for KDE,
        // index.theme is sometimes generated — KDE metadata has priority.
        let fs = snap_of(
            "/t/both",
            &[
                ("metadata.json", "{\"KPlugin\": {\"Name\": \"Из KDE\"}}"),
                ("index.theme", "[Icon Theme]\nName=Из index\n"),
            ],
        );
        assert_eq!(theme_display_name(&fs, "/t/both", "both", "ru_RU.UTF-8"), "Из KDE");
        // metadata.desktop without a name → move on to index.theme
        let fs = snap_of(
            "/t/next",
            &[
                ("metadata.desktop", "[Desktop Entry]\nComment=нет имени\n"),
                ("index.theme", "[Icon Theme]\nName=Из index\n"),
            ],
        );
        assert_eq!(theme_display_name(&fs, "/t/next", "next", "ru_RU.UTF-8"), "Из index");
    }

    // ── Color schemes: the name from the *.colors file itself ─────────
    //
    // They have no theme directory at all — the theme is a single file. The name
    // lies in [General], the eleventh group (checked on this machine's
    // /usr/share/color-schemes), so the group is not asked for.

    #[test]
    fn colors_display_name_comes_from_the_file_itself() {
        let text = "[Colors:Window]\nBackgroundNormal=27,30,32\n\n\
                    [General]\nName=Breeze Dark\nName[de]=Breeze Dunkel\n\
                    Name[ru]=Breeze, тёмный вариант\nColorScheme=BreezeDark\n";
        let mut files = HashMap::new();
        files.insert("/sb/usr/share/color-schemes/BreezeDark.colors".to_string(), text.to_string());
        let fs = FsSnapshot { dirs: HashMap::new(), files };
        let path = "/sb/usr/share/color-schemes/BreezeDark.colors";
        assert_eq!(
            file_display_name(&fs, path, "BreezeDark", "en_US.UTF-8"),
            "Breeze Dark"
        );
        assert_eq!(
            file_display_name(&fs, path, "BreezeDark", "ru_RU.UTF-8"),
            "Breeze, тёмный вариант"
        );
        // no file → the file name (there are no nameless cards)
        assert_eq!(
            file_display_name(&fs, "/sb/usr/share/color-schemes/Нет.colors", "Нет", "ru"),
            "Нет"
        );
        // an empty path → fallback without touching the disk
        assert_eq!(file_display_name(&fs, "", "Нет", "ru"), "Нет");
        // the file is there but has no name (a truncated scheme) → the file name
        let mut files = HashMap::new();
        files.insert("/c/Пустая.colors".to_string(), "[General]\nColorScheme=X\n".to_string());
        let fs = FsSnapshot { dirs: HashMap::new(), files };
        assert_eq!(file_display_name(&fs, "/c/Пустая.colors", "Пустая", "ru"), "Пустая");
    }

    #[test]
    fn locale_name_keys_go_from_exact_to_general() {
        assert_eq!(
            locale_name_keys("ru_RU.UTF-8"),
            vec!["Name[ru_RU]", "Name[ru]", "Name"]
        );
        // the hyphen is normalised to an underscore — KDE keys use only that
        assert_eq!(locale_name_keys("pt-BR"), vec!["Name[pt_BR]", "Name[pt]", "Name"]);
        assert_eq!(locale_name_keys("ru"), vec!["Name[ru]", "Name"]);
        // the modifier is dropped
        assert_eq!(locale_name_keys("ca@valencia"), vec!["Name[ca]", "Name"]);
        // no language — only the generic name remains
        assert_eq!(locale_name_keys(""), vec!["Name"]);
    }

    /// The «System» scan on THIS machine gets as far as an answer.
    ///
    /// The whole live path is checked: detecting the package family, the "what is
    /// not from the repositories" query, the two-phase question about file owners.
    /// An error here means `SystemScanUnavailable` in the tab — that is, a section
    /// closed with a status instead of themes.
    #[test]
    #[cfg_attr(
        feature = "isolated-checks",
        ignore = "нужна живая машина: спрашивает настоящий пакетный менеджер о происхождении пакетов"
    )]
    fn live_system_scan_reaches_an_answer() {
        let scan = LiveScan::new(Vec::new(), HashMap::new());
        let items = scan
            .system(&HashMap::new())
            .expect("происхождение пакетов на этой машине определяется");
        // Everything returned must lie under the system root: otherwise user
        // themes went off into «System».
        for item in &items {
            assert!(
                item.install_path.starts_with(&scan.usr_root),
                "не системный путь: {}",
                item.install_path
            );
        }
    }

    /// Desktop environment sections: XFCE borders are looked up in ~/.themes and
    /// ~/.local/share/themes by the `xfwm4` subdirectory, Nemo actions — as files,
    /// the enabled ones (the active map, add-ons have several names) — with the
    /// "active" sign; a foreign environment gets no sections.
    #[test]
    fn de_sections_scan_by_matrix() {
        let base = tmp_dir("desect");
        let home = base.join("home");
        let usr = base.join("usr");
        write(&home.join(".themes/Arc/xfwm4/themerc"), "x");
        write(&home.join(".local/share/themes/Nord/xfwm4/themerc"), "x");
        write(&home.join(".themes/GtkOnly/gtk-3.0/gtk.css"), "x");
        write(&usr.join("share/themes/Default/xfwm4/themerc"), "x");
        write(&home.join(".local/share/nemo/actions/a.nemo_action"), "x");
        write(&home.join(".local/share/nemo/actions/b.nemo_action"), "x");
        write(&home.join(".local/share/nemo/actions/readme.txt"), "x");
        let scan = LiveScan {
            home: s(&home),
            usr_root: s(&usr),
            ocs_cache: Vec::new(),
            preview_files: HashMap::new(),
            store: StoreRecords::default(),
            plasma_login: Some(false),
            de: Some("xfce".into()),
            migrate: false,
            standard_icons: None,
        };
        let active = HashMap::from([
            ("xfwm4".to_string(), "Nord".to_string()),
            ("nemoactions".to_string(), "a.nemo_action\nb.nemo_action".to_string()),
        ]);
        let got: Vec<(String, Option<String>)> = scan
            .section("xfwm4", &active)
            .into_iter()
            .map(|it| (it.id, it.local_kind))
            .collect();
        assert_eq!(
            got,
            vec![
                ("local:xfwm4:Nord".to_string(), Some("active".to_string())),
                ("local:xfwm4:Arc".to_string(), None),
                ("local:xfwm4:Default".to_string(), None),
            ]
        );
        let nemo: Vec<String> = scan.section("nemoactions", &active).into_iter().map(|it| it.id).collect();
        assert_eq!(nemo, vec!["local:nemoactions:a.nemo_action", "local:nemoactions:b.nemo_action"]);
        // XFCE «Downloaded» sees borders but not Nemo actions (those are Cinnamon's).
        let dl: Vec<String> = scan.downloaded(&active).into_iter().map(|it| it.id).collect();
        assert!(dl.contains(&"local:xfwm4:Arc".to_string()), "{dl:?}");
        assert!(!dl.iter().any(|i| i.starts_with("local:nemoactions")), "{dl:?}");
    }

    /// An LXQt theme is a directory with `lxqt-panel.qss`, as in LXQt's own theme
    /// list (`LXQtTheme::allThemes`): a directory without it doesn't count as a
    /// theme either at home or in the system.
    #[test]
    fn lxqt_themes_need_panel_stylesheet() {
        let base = tmp_dir("lxqt-themes");
        let home = base.join("home");
        let usr = base.join("usr");
        write(&home.join(".local/share/lxqt/themes/Frost/lxqt-panel.qss"), "LXQtPanel {}");
        write(&home.join(".local/share/lxqt/themes/Frost/preview.png"), "x");
        write(&home.join(".local/share/lxqt/themes/Junk/readme.txt"), "x");
        write(&home.join(".local/share/lxqt/themes/Junk/openbox/themerc"), "x");
        write(&usr.join("share/lxqt/themes/ambiance/lxqt-panel.qss"), "x");
        write(&usr.join("share/lxqt/themes/wallpapers/a.png"), "x");
        let scan = LiveScan {
            home: s(&home),
            usr_root: s(&usr),
            ocs_cache: Vec::new(),
            preview_files: HashMap::new(),
            store: StoreRecords::default(),
            plasma_login: Some(false),
            de: Some("lxqt".into()),
            migrate: false,
            standard_icons: None,
        };
        let mut ids: Vec<String> =
            scan.section("lxqttheme", &HashMap::new()).into_iter().map(|it| it.id).collect();
        ids.sort();
        assert_eq!(ids, vec!["local:lxqttheme:Frost".to_string(), "local:lxqttheme:ambiance".to_string()]);
    }

    /// The «Conky Widgets» section also shows widgets outside the Rexon directory —
    /// as «🔎 Found on disk» tiles after our own, without repeats.
    #[test]
    fn conky_section_shows_widgets_found_on_disk() {
        let base = tmp_dir("conky-found");
        let home = base.join("home");
        let usr = base.join("usr");
        write(&home.join(".config/conky/Jhalak/Jhalak.conf"), "conky.text = [[x]]");
        write(&home.join(".conky/Gotham/Gotham"), "# Gotham\nTEXT\n${time}\n");
        write(&home.join(".conky/Gotham/rings.lua"), "x");
        write(&home.join(".conkyrc"), "conky.config = {}\n");
        write(&home.join(".conky/notes/readme.txt"), "просто текст");
        std::fs::create_dir_all(&usr).unwrap();
        let scan = LiveScan {
            home: s(&home),
            usr_root: s(&usr),
            ocs_cache: Vec::new(),
            preview_files: HashMap::new(),
            store: StoreRecords::default(),
            plasma_login: Some(false),
            de: Some("mate".into()),
            migrate: false,
            standard_icons: None,
        };
        // The picture next to the config file (with its name and without the suffix)
        // and the source from source.txt — for both our own and a found widget.
        write(&home.join(".config/conky/Jhalak/Jhalak.png"), "png");
        write(&home.join(".conky/Gotham/Gotham.jpg"), "jpg");
        write(&home.join(".conky/Gotham/source.txt"), "http://psyjunta.deviantart.com/art/Gotham-Conky-config-205465419\n");
        write(&home.join(".conky/Gotham/credits.txt"), "psyjunta <3");
        let h = s(&home);
        let items = scan.section("conky", &HashMap::new());
        let ids: Vec<String> = items.iter().map(|it| it.id.clone()).collect();
        assert_eq!(
            ids,
            vec![
                "local:conky:Jhalak".to_string(),
                format!("local:conky:{h}/.conkyrc"),
                format!("local:conky:{h}/.conky/Gotham"),
            ]
        );
        assert_eq!(items[0].preview_local, format!("{h}/.config/conky/Jhalak/Jhalak.png"));
        assert_eq!(items[2].preview_local, format!("{h}/.conky/Gotham/Gotham.jpg"));
        let d = &items[2].description;
        assert!(d.contains("<a href=\"http://psyjunta.deviantart.com/art/Gotham-Conky-config-205465419\">"), "{d}");
        assert!(d.contains("psyjunta &lt;3"), "{d}");
        assert!(items[1].description.is_empty());
    }

    /// Sound themes are visible in «Downloaded» and «Active»; an archive wrapper
    /// and a directory without `[Sound Theme]` don't count as themes.
    #[test]
    fn sound_themes_are_scanned_only_on_top_level_with_index() {
        let base = tmp_dir("sounds");
        let home = base.join("home");
        let usr = base.join("usr");
        let snd = home.join(".local/share/sounds");
        write(&snd.join("Ocean/index.theme"), "[Sound Theme]\nName=Ocean\n");
        write(&snd.join("Zorin/index.theme"), "[Sound Theme]\nName=Zorin\n");
        write(&snd.join("zorinos2/sounds/Zorin2/index.theme"), "[Sound Theme]\nName=Z2\n");
        write(&snd.join("gnome/default/alerts/bark.ogg"), "x");
        let scan = LiveScan {
            home: s(&home),
            usr_root: s(&usr),
            ocs_cache: Vec::new(),
            preview_files: HashMap::new(),
            store: StoreRecords::default(),
            plasma_login: Some(false),
            de: None,
            migrate: false,
            standard_icons: None,
        };
        let active = HashMap::from([("sounds".to_string(), "Zorin".to_string())]);
        let ids = |items: Vec<ThemeItem>| -> Vec<String> {
            let mut v: Vec<String> = items
                .into_iter()
                .filter(|i| i.install_path.contains("/sounds/"))
                .map(|i| i.install_path.rsplit('/').next().unwrap_or("").to_string())
                .collect();
            v.sort();
            v
        };
        assert_eq!(ids(scan.downloaded(&active)), ["Ocean"]);
        assert_eq!(ids(scan.active(&active)), ["Zorin"]);
    }


    // ── KDE sections with an installed grid (T3a) ─────────────────────
    //
    // Each section is in a temporary HOME and a substitute `/usr` root
    // (`LiveScan::usr_root`): the live profile and live themes are not read.

    /// A section scan in substitute HOME/`/usr`: (id, "active"), the current one —
    /// first, the rest — alphabetically (directory order depends on the FS).
    fn kde_section(
        tag: &str,
        key: &str,
        current: &str,
        fill: impl FnOnce(&std::path::Path, &std::path::Path),
    ) -> Vec<(String, bool)> {
        let base = tmp_dir(tag);
        let home = base.join("home");
        let usr = base.join("usr");
        std::fs::create_dir_all(&home).expect("home");
        std::fs::create_dir_all(&usr).expect("usr");
        fill(&home, &usr);
        let scan = LiveScan {
            home: s(&home),
            usr_root: s(&usr),
            ocs_cache: Vec::new(),
            preview_files: HashMap::new(),
            store: StoreRecords::default(),
            plasma_login: Some(false),
            de: None,
            migrate: false,
            standard_icons: None,
        };
        let active = HashMap::from([(key.to_string(), current.to_string())]);
        let items = scan.section(key, &active);
        // The core knows the section: the key resolves into a section scan.
        assert_eq!(
            crate::appearance_page::local_scan_plan(key),
            ("section".to_string(), key.to_string())
        );
        let cat = crate::categories::KDE_INSTALLED_SECTIONS
            .iter()
            .find(|(_, k)| *k == key)
            .map(|(id, _)| *id)
            .expect("раздел в списке KDE_INSTALLED_SECTIONS");
        let mut got: Vec<(String, bool)> = items
            .iter()
            .map(|it| {
                assert_eq!(it.category_id, cat, "{}", it.id);
                assert!(it.installed, "{}", it.id);
                (it.id.clone(), it.local_kind.as_deref() == Some("active"))
            })
            .collect();
        if got.len() > 1 {
            got[1..].sort();
        }
        got
    }

    fn row(id: &str, active: bool) -> (String, bool) {
        (id.to_string(), active)
    }

    fn lnf(dir: &std::path::Path, name: &str) {
        write(
            &dir.join("metadata.json"),
            &format!("{{\"KPlugin\": {{\"Name\": \"{name}\", \"Id\": \"{name}\"}}}}"),
        );
    }

    const ICON_INDEX: &str = "[Icon Theme]\nName=X\nDirectories=48x48/apps\n";

    #[test]
    fn kde_section_global_themes() {
        let got = kde_section("kde-global", "global", "org.kde.breezedark.desktop", |home, usr| {
            let user = home.join(".local/share/plasma/look-and-feel");
            lnf(&user.join("com.example.Mine"), "Mine");
            std::fs::create_dir_all(user.join("junk-without-metadata")).unwrap();
            let sys = usr.join("share/plasma/look-and-feel");
            lnf(&sys.join("org.kde.breezedark.desktop"), "Breeze Dark");
            lnf(&sys.join("org.kde.breeze.desktop"), "Breeze");
        });
        assert_eq!(
            got,
            vec![
                row("local:global:org.kde.breezedark.desktop", true),
                row("local:global:com.example.Mine", false),
                row("local:global:org.kde.breeze.desktop", false),
            ]
        );
    }

    #[test]
    fn kde_section_plasma_styles() {
        let got = kde_section("kde-plasma", "plasma", "Mine", |home, usr| {
            let user = home.join(".local/share/plasma/desktoptheme");
            write(&user.join("Mine/metadata.json"), "{\"KPlugin\": {\"Name\": \"Mine\"}}");
            let sys = usr.join("share/plasma/desktoptheme");
            write(&sys.join("default/metadata.json"), "{\"KPlugin\": {\"Name\": \"Breeze\"}}");
            write(&sys.join("oxygen/metadata.json"), "{\"KPlugin\": {\"Name\": \"Oxygen\"}}");
        });
        assert_eq!(
            got,
            vec![
                row("local:plasma:Mine", true),
                row("local:plasma:default", false),
                row("local:plasma:oxygen", false),
            ]
        );
    }

    #[test]
    fn kde_section_color_schemes() {
        let got = kde_section("kde-colors", "colors", "BreezeDark", |home, usr| {
            let user = home.join(".local/share/color-schemes");
            write(&user.join("Nord.colors"), "[General]\nName=Nord\n");
            write(&user.join("notes.txt"), "x");
            let sys = usr.join("share/color-schemes");
            write(&sys.join("BreezeDark.colors"), "[General]\nName=Breeze Dark\n");
            write(&sys.join("BreezeLight.colors"), "[General]\nName=Breeze Light\n");
        });
        assert_eq!(
            got,
            vec![
                row("local:colors:BreezeDark", true),
                row("local:colors:BreezeLight", false),
                row("local:colors:Nord", false),
            ]
        );
    }

    #[test]
    fn kde_section_aurorae_decorations() {
        let got = kde_section("kde-aurorae", "aurorae", "Sweet", |home, usr| {
            let user = home.join(".local/share/aurorae/themes");
            write(&user.join("Sweet/metadata.desktop"), "[Desktop Entry]\nName=Sweet\n");
            write(&user.join("Layan/metadata.desktop"), "[Desktop Entry]\nName=Layan\n");
            write(&usr.join("share/aurorae/themes/Plastik/metadata.desktop"), "[Desktop Entry]\nName=Plastik\n");
        });
        assert_eq!(
            got,
            vec![
                row("local:aurorae:Sweet", true),
                row("local:aurorae:Layan", false),
                row("local:aurorae:Plastik", false),
            ]
        );
    }

    /// Icons: cursor themes and service sets (`hicolor`, `default`) don't go into
    /// the grid; `~/.icons` is looked at together with `~/.local/share/icons`, one
    /// name in two places — one tile; the current one — from `~/.icons` too.
    #[test]
    fn kde_section_icons() {
        let got = kde_section("kde-icons", "icons", "Papirus", |home, usr| {
            write(&home.join(".icons/Papirus/index.theme"), ICON_INDEX);
            write(&home.join(".local/share/icons/Tela/index.theme"), ICON_INDEX);
            write(&home.join(".icons/Tela/index.theme"), ICON_INDEX);
            write(&home.join(".local/share/icons/Bibata/cursors/left_ptr"), "x");
            let sys = usr.join("share/icons");
            write(&sys.join("breeze/index.theme"), ICON_INDEX);
            write(&sys.join("hicolor/index.theme"), "[Icon Theme]\nName=Hicolor\nHidden=true\nDirectories=48x48/apps\n");
            write(&sys.join("default/index.theme"), "[Icon Theme]\nInherits=breeze_cursors\n");
            write(&sys.join("breeze_cursors/cursors/left_ptr"), "x");
            write(&sys.join("breeze_cursors/index.theme"), "[Icon Theme]\nName=Breeze\n");
        });
        assert_eq!(
            got,
            vec![
                row("local:icons:Papirus", true),
                row("local:icons:Tela", false),
                row("local:icons:breeze", false),
            ]
        );
    }

    /// The environment's default icons are visible: a set with its own cursors
    /// (elementary in XFCE, Adwaita) — in «Icons» and in «Cursors»; a hidden one
    /// (`Hidden=true`) — only if it is the environment's standard set (Adwaita in
    /// GNOME), other hidden ones (hicolor) — not.
    #[test]
    fn section_icons_shows_de_standard_and_icon_sets_with_cursors() {
        let base = tmp_dir("de-std-icons");
        let home = base.join("home");
        let usr = base.join("usr");
        let sys = usr.join("share/icons");
        write(&home.join(".icons/Papirus/index.theme"), ICON_INDEX);
        write(&sys.join("Adwaita/index.theme"), "[Icon Theme]\nName=Adwaita\nInherits=AdwaitaLegacy,hicolor\nHidden=true\nDirectories=16x16/apps\n");
        write(&sys.join("Adwaita/cursors/left_ptr"), "x");
        write(&sys.join("elementary/index.theme"), ICON_INDEX);
        write(&sys.join("elementary/cursors/left_ptr"), "x");
        write(&sys.join("hicolor/index.theme"), "[Icon Theme]\nName=Hicolor\nHidden=true\nDirectories=48x48/apps\n");
        write(&sys.join("breeze_cursors/cursors/left_ptr"), "x");
        write(&sys.join("breeze_cursors/index.theme"), "[Icon Theme]\nName=Breeze\n");
        let ids = |standard: Option<&str>, key: &str, current: &str| {
            let scan = LiveScan {
                home: s(&home),
                usr_root: s(&usr),
                ocs_cache: Vec::new(),
                preview_files: HashMap::new(),
                store: StoreRecords::default(),
                plasma_login: Some(false),
                de: None,
                migrate: false,
                standard_icons: standard.map(str::to_string),
            };
            let active = HashMap::from([(key.to_string(), current.to_string())]);
            let mut v: Vec<String> = scan.section(key, &active).into_iter().map(|it| it.id).collect();
            v.sort();
            v
        };
        assert_eq!(
            ids(Some("Adwaita"), "icons", "Papirus"),
            ["local:icons:Adwaita", "local:icons:Papirus", "local:icons:elementary"]
        );
        assert_eq!(ids(None, "icons", "Papirus"), ["local:icons:Papirus", "local:icons:elementary"]);
        assert_eq!(
            ids(None, "cursor", "breeze_cursors"),
            ["local:cursor:Adwaita", "local:cursor:breeze_cursors", "local:cursor:elementary"]
        );
        let fs = live_snapshot(&[s(&sys)], &[]);
        assert!(is_cursor_only(&fs, &s(&sys.join("breeze_cursors"))));
        assert!(!is_cursor_only(&fs, &s(&sys.join("Adwaita"))));
    }

    /// Cursors: only themes with `cursors/`, own and system ones.
    #[test]
    fn kde_section_cursors() {
        let got = kde_section("kde-cursor", "cursor", "breeze_cursors", |home, usr| {
            write(&home.join(".local/share/icons/Bibata/cursors/left_ptr"), "x");
            write(&home.join(".icons/Capitaine/cursors/left_ptr"), "x");
            write(&home.join(".local/share/icons/Tela/index.theme"), ICON_INDEX);
            let sys = usr.join("share/icons");
            write(&sys.join("breeze_cursors/cursors/left_ptr"), "x");
            write(&sys.join("Adwaita/cursors/left_ptr"), "x");
            write(&sys.join("breeze/index.theme"), ICON_INDEX);
        });
        assert_eq!(
            got,
            vec![
                row("local:cursor:breeze_cursors", true),
                row("local:cursor:Adwaita", false),
                row("local:cursor:Bibata", false),
                row("local:cursor:Capitaine", false),
            ]
        );
    }

    /// GTK: `~/.themes` too; themes without GTK 3/4 (borders only) are not shown.
    #[test]
    fn kde_section_gtk_themes() {
        let got = kde_section("kde-gtk", "gtk", "Breeze", |home, usr| {
            write(&home.join(".local/share/themes/Arc/gtk-3.0/gtk.css"), "x");
            write(&home.join(".themes/Orchis/gtk-4.0/gtk.css"), "x");
            write(&home.join(".themes/OnlyXfwm/xfwm4/themerc"), "x");
            let sys = usr.join("share/themes");
            write(&sys.join("Breeze/gtk-3.0/gtk.css"), "x");
            write(&sys.join("Adwaita/gtk-3.0/gtk.css"), "x");
            write(&sys.join("Default/xfwm4/themerc"), "x");
        });
        assert_eq!(
            got,
            vec![
                row("local:gtk:Breeze", true),
                row("local:gtk:Adwaita", false),
                row("local:gtk:Arc", false),
                row("local:gtk:Orchis", false),
            ]
        );
    }

    /// The overviews («Downloaded», «System») select themes by the same rule as
    /// the section grid: a directory in `share/themes` without GTK 3/4
    /// stylesheets (only xfwm4/Metacity/Openbox borders, only GTK 2) doesn't count
    /// as a GTK theme, a service icon set — as an icon set. Borders are still
    /// visible as their own kind — by their own subdirectory.
    #[test]
    fn overviews_list_only_real_gtk_themes_and_icon_sets() {
        let base = tmp_dir("overview-gtk");
        let (home, usr) = (base.join("home"), base.join("usr"));
        let user = home.join(".local/share/themes");
        write(&user.join("Arc/gtk-3.0/gtk.css"), "x");
        write(&user.join("Frames/xfwm4/themerc"), "x");
        write(&user.join("Old/gtk-2.0/gtkrc"), "x");
        let sys = usr.join("share/themes");
        write(&sys.join("Adwaita/gtk-4.0/gtk.css"), "x");
        write(&sys.join("Bear2/openbox-3/themerc"), "x");
        write(&sys.join("Atlanta/metacity-1/metacity-theme-1.xml"), "x");
        write(&sys.join("Both/gtk-3.0/gtk.css"), "x");
        write(&sys.join("Both/xfwm4/themerc"), "x");
        write(&home.join(".local/share/icons/Tela/index.theme"), ICON_INDEX);
        write(&home.join(".local/share/icons/hicolor/48x48/apps/x.png"), "x");
        let icons = usr.join("share/icons");
        write(&icons.join("breeze/index.theme"), ICON_INDEX);
        write(&icons.join("hicolor/index.theme"), "[Icon Theme]\nName=Hicolor\nHidden=true\nDirectories=48x48/apps\n");
        write(&icons.join("default/index.theme"), "[Icon Theme]\nInherits=breeze_cursors\n");

        let (home_s, usr_s) = (s(&home), s(&usr));
        let mut types = types_for(&home_s, &usr_s);
        types.extend(de_types(&home_s, &usr_s, "xfce"));
        let mut roots: Vec<String> = Vec::new();
        for t in &types {
            for d in [&t.user_dir, &t.sys_dir] {
                if !d.is_empty() && !roots.contains(d) {
                    roots.push(d.clone());
                }
            }
        }
        let fs = live_snapshot(&roots, &[]);
        let no_cache: Vec<(String, String)> = Vec::new();
        let preview_file = |_: &str| None;
        let env = ScanEnv {
            home: &home_s,
            usr_root: &usr_s,
            fs: &fs,
            ocs_cache: &no_cache,
            preview_file: &preview_file,
            store: StoreRecords::none(),
            locale: "en",
            standard_icons: None,
        };
        let ids = |items: Vec<ThemeItem>| {
            let mut ids: Vec<String> = items.into_iter().map(|it| it.id).collect();
            ids.sort();
            ids
        };
        let nothing_active: HashMap<String, String> = HashMap::new();
        assert_eq!(
            ids(scan_downloaded_in(&env, &types, &nothing_active)),
            ["local:gtk:Arc", "local:icons:Tela", "local:xfwm4:Frames"]
        );
        // «System»: the owner of each path is a package from the repositories.
        let first = scan_system_in(&env, &types, &nothing_active, &HashSet::new(), &HashMap::new()).unwrap();
        let owners: HashMap<String, String> =
            first.qo_paths.iter().map(|p| (p.clone(), "pkg".to_string())).collect();
        let system = scan_system_in(&env, &types, &nothing_active, &HashSet::new(), &owners).unwrap();
        assert_eq!(
            ids(system.items),
            ["local:gtk:Adwaita", "local:gtk:Both", "local:icons:breeze", "local:xfwm4:Both"]
        );
        // The theme enabled now is always visible, even without GTK 3/4 stylesheets.
        let active = HashMap::from([("gtk".to_string(), "Old".to_string())]);
        assert_eq!(ids(scan_active_in(&env, &types, &active)), ["local:gtk:Old"]);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn kde_section_sound_themes() {
        let got = kde_section("kde-sounds", "sounds", "ocean", |home, usr| {
            let user = home.join(".local/share/sounds");
            write(&user.join("Zorin/index.theme"), "[Sound Theme]\nName=Zorin\n");
            write(&user.join("gnome/default/alerts/bark.ogg"), "x");
            let sys = usr.join("share/sounds");
            write(&sys.join("ocean/index.theme"), "[Sound Theme]\nName=Ocean\n");
            write(&sys.join("freedesktop/index.theme"), "[Sound Theme]\nName=Freedesktop\n");
            write(&sys.join("alsa/Front_Center.wav"), "x");
        });
        assert_eq!(
            got,
            vec![
                row("local:sounds:ocean", true),
                row("local:sounds:Zorin", false),
                row("local:sounds:freedesktop", false),
            ]
        );
    }

    /// The current one is not in the map (nothing applied) — the same grid without
    /// a mark; KDE sections without an installed grid (wallpapers, fonts, widgets)
    /// get no section scan.
    #[test]
    fn kde_section_without_current_and_other_sections() {
        let base = tmp_dir("kde-nocur");
        let home = base.join("home");
        write(&home.join(".local/share/color-schemes/Nord.colors"), "[General]\nName=Nord\n");
        let scan = LiveScan {
            home: s(&home),
            usr_root: s(&base.join("usr")),
            ocs_cache: Vec::new(),
            preview_files: HashMap::new(),
            store: StoreRecords::default(),
            plasma_login: Some(false),
            de: None,
            migrate: false,
            standard_icons: None,
        };
        let items = scan.section("colors", &HashMap::new());
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].local_kind, None);
        for key in ["wallpapers", "fonts", "plasmoids"] {
            assert_eq!(
                crate::appearance_page::local_scan_plan(key),
                (String::new(), String::new()),
                "{key}"
            );
        }
        assert_eq!(crate::categories::KDE_INSTALLED_SECTIONS.len(), 10);
    }

    /// Login screen (like the sddm kcm): all SDDM themes from the system
    /// directory, the current one first and highlighted; a directory without
    /// `metadata.desktop` doesn't count as a theme.
    #[test]
    fn kde_section_sddm_themes() {
        let got = kde_section("kde-sddm", "sddm", "maldives", |_home, usr| {
            let sys = usr.join("share/sddm/themes");
            for t in ["breeze", "maldives", "maya"] {
                write(&sys.join(t).join("metadata.desktop"), &format!("[SddmGreeterTheme]\nName={t}-shown\nMainScript=Main.qml\n"));
            }
            write(&sys.join("leftover/Main.qml"), "x");
        });
        assert_eq!(
            got,
            vec![
                row("local:sddm:maldives", true),
                row("local:sddm:breeze", false),
                row("local:sddm:maya", false),
            ]
        );
    }

    /// Boot splash (like the plymouth kcm): themes with `<name>.plymouth`, the
    /// current one first and highlighted; an empty directory from a removed
    /// package is not shown. Name and description — from `.plymouth` in the interface language.
    #[test]
    fn kde_section_plymouth_themes() {
        let got = kde_section("kde-ply", "plymouth", "bgrt", |_home, usr| {
            let sys = usr.join("share/plymouth/themes");
            for t in ["bgrt", "spinner", "details"] {
                write(&sys.join(t).join(format!("{t}.plymouth")), "[Plymouth Theme]\nName=X\nModuleName=two-step\n");
            }
            std::fs::create_dir_all(sys.join("removed-package")).unwrap();
        });
        assert_eq!(
            got,
            vec![
                row("local:plymouth:bgrt", true),
                row("local:plymouth:details", false),
                row("local:plymouth:spinner", false),
            ]
        );

        let dir = tmp_dir("ply-name").join("cachyos-bootanimation");
        write(
            &dir.join("cachyos-bootanimation.plymouth"),
            "[Plymouth Theme]\nName=CachyOS Boot\nName[ru]=Загрузка CachyOS\n\
             Description=Animated logo\nDescription[ru]=Живой логотип\nModuleName=script\n\n\
             [script]\nImageDir=/x\n",
        );
        let d = s(&dir);
        let snap = live_snapshot(&[s(dir.parent().unwrap())], &[]);
        assert_eq!(theme_display_name(&snap, &d, "cachyos-bootanimation", "ru_RU"), "Загрузка CachyOS");
        assert_eq!(theme_display_name(&snap, &d, "cachyos-bootanimation", ""), "CachyOS Boot");
        assert_eq!(theme_description(&snap, &d, "ru"), "Живой логотип");
        assert_eq!(theme_description(&snap, &d, "de"), "Animated logo");
    }

    /// The current boot splash theme — as plymouthd sees it: `/etc` beats the
    /// choice via alternatives, which beats the package default; the old
    /// `/usr/share/plymouth/plymouthd.conf` doesn't override `/etc`.
    #[test]
    fn plymouth_current_follows_plymouthd_weight() {
        let cur = |defaults: Option<&str>, alt: Option<&str>, legacy: Option<&str>, etc: Option<&str>| {
            let confs = plymouth_confs_by_weight(
                defaults.map(str::to_string),
                alt,
                legacy.map(str::to_string),
                etc.map(str::to_string),
            );
            plymouth_current_from(&confs.iter().map(String::as_str).collect::<Vec<_>>())
        };
        let defaults = Some("[Daemon]\nTheme=bgrt\nShowDelay=0\n");
        // The theme wasn't changed: `/etc` has only a delay — the current one comes from the package default.
        assert_eq!(cur(defaults, None, None, Some("[Daemon]\nShowDelay=5\n")).as_deref(), Some("bgrt"));
        assert_eq!(cur(defaults, None, None, Some("[Daemon]\nTheme=spinner\n")).as_deref(), Some("spinner"));
        assert_eq!(cur(defaults, Some("ubuntu-logo"), None, None).as_deref(), Some("ubuntu-logo"));
        assert_eq!(cur(defaults, Some(" "), None, None).as_deref(), Some("bgrt"), "пустая ссылка не в счёт");
        assert_eq!(cur(defaults, Some("ubuntu-logo"), None, Some("[Daemon]\nTheme=glow\n")).as_deref(), Some("glow"));
        assert_eq!(cur(None, None, Some("[Daemon]\nTheme=old\n"), Some("[Daemon]\nTheme=new\n")).as_deref(), Some("new"));
        assert_eq!(cur(None, None, None, None), None);
    }

    /// KNewStuff install registry: the record's paths and the URL of its picture.
    #[test]
    fn kns_registry_gives_installed_paths_with_preview() {
        let xml = r#"<?xml version="1.0"?><hotnewstuffregistry>
<stuff category="Plasma Themes"><name>Nordic</name><installedfile>/h/.local/share/plasma/desktoptheme/Nordic/*</installedfile>
<installedfile>/h/.local/share/plasma/desktoptheme/Nordic-dark/*</installedfile>
<preview>https://x/a.png?x=1&amp;y=2</preview><previewBig>https://x/big.png</previewBig><status>installed</status></stuff>
<stuff category="Colors"><name>Sweet</name><installedfile>/h/.local/share/color-schemes/Sweet.colors</installedfile>
<preview></preview><previewBig>https://x/sweet.png</previewBig></stuff>
<stuff><name>Без картинки</name><installedfile>/h/x</installedfile></stuff></hotnewstuffregistry>"#;
        assert_eq!(
            parse_kns_registry(xml),
            vec![
                ("/h/.local/share/plasma/desktoptheme/Nordic".to_string(), "https://x/a.png?x=1&y=2".to_string()),
                ("/h/.local/share/plasma/desktoptheme/Nordic-dark".to_string(), "https://x/a.png?x=1&y=2".to_string()),
                ("/h/.local/share/color-schemes/Sweet.colors".to_string(), "https://x/sweet.png".to_string()),
            ]
        );
        assert!(parse_kns_registry("<hotnewstuffregistry/>").is_empty());
    }

    /// The second step of the preview chain: a registry record finds the picture
    /// by the theme's path, and name matching doesn't go outside its own section
    /// and requires the same name.
    #[test]
    fn store_picture_follows_install_record_and_section() {
        let fs = FsSnapshot::from_parts(
            HashMap::from([("/h/themes/Nordic".to_string(), Vec::new())]),
            ["/c/nordic.png", "/c/icons.png", "/c/orange.png"]
                .iter()
                .map(|p| (p.to_string(), String::new()))
                .collect(),
        );
        let files = HashMap::from([
            ("u-nordic".to_string(), "/c/nordic.png".to_string()),
            ("u-icons".to_string(), "/c/icons.png".to_string()),
            ("u-orange".to_string(), "/c/orange.png".to_string()),
        ]);
        let preview_file = |url: &str| files.get(url).cloned();
        let store = StoreRecords {
            sections: HashMap::from([
                ("u-icons".to_string(), "icons".to_string()),
                ("u-orange".to_string(), "gtk".to_string()),
            ]),
            installed: vec![("/h/themes/Nordic".to_string(), "u-nordic".to_string())],
            rexon_paths: None,
        };
        let ocs = vec![
            ("win7-icon-theme".to_string(), "u-icons".to_string()),
            ("Glassy GTK - [ Orange ]".to_string(), "u-orange".to_string()),
        ];
        let env = ScanEnv {
            home: "/h",
            usr_root: "/usr",
            fs: &fs,
            ocs_cache: &ocs,
            preview_file: &preview_file,
            store: &store,
            locale: "ru",
            standard_icons: None,
        };
        assert_eq!(resolve_preview_in(&env, "plasma", "Nordic", "/h/themes/Nordic", "/h/themes/Nordic", None, &[]), "/c/nordic.png");
        // The Kvantum theme "Win7" doesn't take the icon set's picture…
        assert_eq!(backfill_preview_for(&env, "kvantum", "Win7"), "");
        // …and within its own section a similar name isn't enough: "Orang" is not "…Orange".
        assert_eq!(backfill_preview_for(&env, "gtk", "Orang"), "");
        assert_eq!(backfill_preview_for(&env, "icons", "Win7 Icon Theme"), "/c/icons.png");
        // Without a section — the old substring rule.
        assert_eq!(backfill_preview(&env, "Win7"), "/c/icons.png");
    }

    /// Cursors converted from Windows carry «🪟 From Windows» on the tile and a
    /// description in the interface language — both by the new `X-Rexon-Source`
    /// line and by the `Comment` of themes converted earlier; an ordinary theme gets no mark.
    #[test]
    fn windows_cursor_theme_tile_carries_origin_badge() {
        let themes = [
            ("Neu", "[Icon Theme]\nName=Neu\nComment=Imported from Windows cursors (3 cursors)\nX-Rexon-Source=windows\n"),
            ("Alt", "[Icon Theme]\nName=Alt\nComment=Imported from Windows cursors (17 cursors)\n"),
            ("Bibata", "[Icon Theme]\nName=Bibata\nComment=Material cursors\n"),
        ];
        let mut dirs: HashMap<String, Vec<String>> = HashMap::new();
        let mut files: HashMap<String, String> = HashMap::new();
        dirs.insert("/h/.icons".into(), themes.iter().map(|(n, _)| n.to_string()).collect());
        for (name, index) in themes {
            dirs.insert(format!("/h/.icons/{name}"), vec!["cursors".into(), "index.theme".into()]);
            dirs.insert(format!("/h/.icons/{name}/cursors"), Vec::new());
            files.insert(format!("/h/.icons/{name}/index.theme"), index.to_string());
        }
        let fs = FsSnapshot::from_parts(dirs, files);
        assert!(is_windows_cursor_theme(&fs, "/h/.icons/Neu"));
        assert!(is_windows_cursor_theme(&fs, "/h/.icons/Alt"));
        assert!(!is_windows_cursor_theme(&fs, "/h/.icons/Bibata"));
        assert!(!is_windows_cursor_theme(&fs, "/h/.icons/Nichts"));
        assert!(!is_windows_cursor_theme(&fs, ""));

        let preview_file = |_: &str| None;
        let store = StoreRecords::default();
        let env = ScanEnv {
            home: "/h",
            usr_root: "/usr",
            fs: &fs,
            ocs_cache: &[],
            preview_file: &preview_file,
            store: &store,
            locale: "ru",
            standard_icons: None,
        };
        let cursor = types_for("/h", "/usr").into_iter().find(|t| t.apply_kind == "cursor").unwrap();
        let badge = tr(FROM_WINDOWS_LABEL);
        for name in ["Neu", "Alt"] {
            let item = mk_item(&env, &cursor, name, "/h/.icons", false, DOWNLOADED_LABEL);
            assert!(item.author.ends_with(&format!(" · {badge}")), "{name}: {}", item.author);
            assert_eq!(item.description, tr("Imported from Windows"), "{name}");
            // Active theme: the source mark stays next to "✅".
            let active = mk_item(&env, &cursor, name, "/h/.icons", true, "");
            assert!(active.author.contains(&local_active_badge()), "{}", active.author);
            assert!(active.author.ends_with(&badge), "{}", active.author);
        }
        let plain = mk_item(&env, &cursor, "Bibata", "/h/.icons", false, DOWNLOADED_LABEL);
        assert!(!plain.author.contains(&badge), "{}", plain.author);
        assert_eq!(plain.description, "Material cursors");
    }

    /// The first step of the chain: the kind's thumbnail in a theme subdirectory
    /// and the screenshot named by a login screen theme's metadata.
    #[test]
    fn own_picture_is_found_in_kind_subdir_and_sddm_metadata() {
        let home = tmp_dir("own-picture");
        let themes = home.join("themes");
        write(&themes.join("XP/gtk-3.0/thumbnail.png"), "");
        write(&themes.join("XP/gtk-3.0/gtk.css"), "");
        write(&themes.join("XP/cinnamon/thumbnail.png"), "");
        write(&themes.join("Login/metadata.desktop"), "[SddmGreeterTheme]\nName=Login\nScreenshot=shots/login.jpg\n");
        write(&themes.join("Login/shots/login.jpg"), "");
        let snap = live_snapshot(&[s(&themes)], &[]);
        let xp = s(&themes.join("XP"));
        assert_eq!(find_preview_in(&snap, &xp, None, &["gtk-3.0", "gtk-2.0"]), format!("{xp}/gtk-3.0/thumbnail.png"));
        assert_eq!(find_preview_in(&snap, &xp, None, &["cinnamon"]), format!("{xp}/cinnamon/thumbnail.png"));
        assert_eq!(find_preview_in(&snap, &xp, None, &["xfwm4"]), "", "чужая миниатюра виду не подходит");
        let login = s(&themes.join("Login"));
        assert_eq!(find_preview(&snap, &login, None), format!("{login}/shots/login.jpg"));
        let _ = std::fs::remove_dir_all(&home);
    }

    /// "Nobody's" — only what pacman said so itself; what belongs to a package and
    /// foreign error lines don't become nobody's.
    #[test]
    fn qo_unowned_takes_only_what_pacman_named() {
        let err = "error: No package owns /usr/share/sddm/themes/Apple-Sonoma-v2\n\
                   error: No package owns /usr/share/plymouth/themes/by-hand/\n\
                   error: failed to read file '/usr/share/sddm/themes/x': Permission denied\n";
        let got = parse_qo_unowned(err);
        assert_eq!(got.len(), 2);
        assert!(got.contains("/usr/share/sddm/themes/Apple-Sonoma-v2"));
        assert!(got.contains("/usr/share/plymouth/themes/by-hand"));
        assert!(parse_qo_unowned("").is_empty());
    }

    /// Environments with their own appearance storage: the gtk/icons/cursor roles
    /// are read from there, by the same schema table as applying (for MATE the
    /// cursor is `org.mate.peripherals-mouse`); KDE, LXQt and tiling WMs don't
    /// ask the storage at all.
    #[test]
    fn native_role_values_reads_de_store() {
        let mut asked: Vec<Vec<String>> = Vec::new();
        let got = native_role_values("mate", |cmd| {
            asked.push(cmd.argv.clone());
            Some(match cmd.argv[3].as_str() {
                "gtk-theme" => "'Menta'\n".to_string(),
                "icon-theme" => "'Papirus'\n".to_string(),
                _ => "'Bibata-Modern-Ice'\n".to_string(),
            })
        });
        assert_eq!(
            got,
            [Some("Menta".into()), Some("Papirus".into()), Some("Bibata-Modern-Ice".into())]
        );
        assert_eq!(asked[0], ["gsettings", "get", "org.mate.interface", "gtk-theme"]);
        assert_eq!(asked[1], ["gsettings", "get", "org.mate.interface", "icon-theme"]);
        assert_eq!(asked[2], ["gsettings", "get", "org.mate.peripherals-mouse", "cursor-theme"]);

        for (de, schema) in [
            ("gnome", "org.gnome.desktop.interface"),
            ("cinnamon", "org.cinnamon.desktop.interface"),
            ("budgie", "org.gnome.desktop.interface"),
            ("pantheon", "org.gnome.desktop.interface"),
            ("cosmic", "org.gnome.desktop.interface"),
        ] {
            let mut first = None;
            native_role_values(de, |cmd| {
                first.get_or_insert_with(|| cmd.argv.clone());
                None
            });
            assert_eq!(first.unwrap()[2], schema, "{de}");
        }

        let xf = native_role_values("xfce", |cmd| Some(format!("{}\n", cmd.argv[4])));
        assert_eq!(
            xf,
            [
                Some("/Net/ThemeName".into()),
                Some("/Net/IconThemeName".into()),
                Some("/Gtk/CursorThemeName".into())
            ]
        );
        let dd = native_role_values("deepin", |_| Some("s \"bloom\"\n".into()));
        assert_eq!(dd[1].as_deref(), Some("bloom"));

        for de in ["kde", "lxqt", "hyprland", "sway", "niri", ""] {
            let got = native_role_values(de, |_| panic!("{de}: хранилище спрошено"));
            assert_eq!(got, [None, None, None], "{de}");
        }
        // An empty answer and "—" don't count as a value.
        assert_eq!(native_role_values("mate", |_| Some("''\n".into())), [None, None, None]);
    }

    /// The native storage value wins over kdeglobals and `settings.ini`;
    /// without it — the old chain.
    #[test]
    fn active_map_prefers_native_store() {
        let ini = "[Settings]\ngtk-theme-name=Adwaita\n";
        let base = || ActiveMapSources {
            icons: "breeze",
            mouse_cursor: "breeze_cursors",
            gtk_settings_ini: Some(ini),
            ..ActiveMapSources::default()
        };
        let key = |src: &ActiveMapSources, k: &str| {
            active_map_from_sources(src).get(k).and_then(|v| v.as_str()).unwrap_or("").to_string()
        };
        let mate = ActiveMapSources {
            native_gtk: Some("Menta"),
            native_icons: Some("Papirus"),
            native_cursor: Some("Bibata-Modern-Ice"),
            ..base()
        };
        assert_eq!(key(&mate, "gtk"), "Menta");
        assert_eq!(key(&mate, "icons"), "Papirus");
        assert_eq!(key(&mate, "cursor"), "Bibata-Modern-Ice");
        let old = base();
        assert_eq!(key(&old, "gtk"), "Adwaita");
        assert_eq!(key(&old, "icons"), "breeze");
        assert_eq!(key(&old, "cursor"), "breeze_cursors");
        let blank = ActiveMapSources { native_icons: Some(" "), native_cursor: Some("—"), ..base() };
        assert_eq!(key(&blank, "icons"), "breeze");
        assert_eq!(key(&blank, "cursor"), "breeze_cursors");
    }

    /// «⬇️ Downloaded» — only what Rexon or the store installed; a theme placed by
    /// hand (or by an old .rxn) — «🗂️ Installed manually».
    #[test]
    fn origin_label_tells_downloaded_from_manual() {
        let store = StoreRecords {
            installed: vec![("/h/.local/share/themes/FromKns".to_string(), String::new())],
            rexon_paths: Some(vec!["/h/.local/share/themes/FromRexon/".to_string()]),
            ..StoreRecords::default()
        };
        assert_eq!(store.origin_label("/h/.local/share/themes/FromRexon"), DOWNLOADED_LABEL);
        assert_eq!(store.origin_label("/h/.local/share/themes/FromKns"), DOWNLOADED_LABEL);
        assert_eq!(store.origin_label("/h/.local/share/themes/Windows XP Luna"), MANUAL_LABEL);
        // The journal wasn't read — the old label.
        assert_eq!(StoreRecords::default().origin_label("/h/x"), DOWNLOADED_LABEL);
    }

    /// The "theme's icons/cursor missing" note — by the theme's request plan.
    #[test]
    fn missing_deps_note_names_what_is_missing() {
        use crate::themes::{DepItem, DepRole, DepState};
        let item = |role, state| DepItem { role, wanted: "X".into(), state };
        let both = [item(DepRole::Icons, DepState::Missing), item(DepRole::Cursor, DepState::Installable("p".into()))];
        assert!(missing_deps_note(&both).is_some_and(|n| n.contains("icons and cursor")));
        let icons = [item(DepRole::Icons, DepState::Missing), item(DepRole::Cursor, DepState::Installed("C".into()))];
        assert!(missing_deps_note(&icons).is_some_and(|n| n.contains("icons are")));
        assert_eq!(missing_deps_note(&[item(DepRole::Icons, DepState::Installed("I".into()))]), None);
        let mut items = vec![
            ThemeItem { id: "local:gtk:WhiteSur-Dark-nord".into(), author: "a".into(), ..Default::default() },
            ThemeItem { id: "local:marco:WhiteSur-Dark-nord".into(), author: "b".into(), ..Default::default() },
        ];
        stamp_missing_deps(&mut items, &|_| both.to_vec());
        assert!(items[0].author.starts_with("a · ⚠️"));
        assert_eq!(items[1].author, "b");
    }
}
