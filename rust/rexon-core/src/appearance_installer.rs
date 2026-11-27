//! Installer/applier for appearance catalog themes — port of
//! `rexon/appearance/installer.py` (the pure part).
//!
//! The original's install flow:
//!   download the archive → unpack into temp → detect the actual type →
//!   copy into the target folder (by cat_id) → apply via ThemeManager →
//!   gtk-update-icon-cache → gtk_sanitizer → log.
//!
//! What is ported is the DETERMINISTIC LOGIC, without execution:
//!   * detecting the actual type of an unpacked KDE theme
//!     ([`detect_actual_kde_type`], port of `ocs_store._detect_actual_kde_type`,
//!     which the installer calls) — from a tree snapshot;
//!   * resolving the content root / theme name / Plymouth theme name
//!     ([`find_content_root`], [`theme_id_from`], [`plymouth_theme_name`]);
//!   * detecting an item delivered as one bare file ([`single_file_accepted`],
//!     per-kind rules — [`kind_single_file_rule`]) and the safety check of
//!     archive entries ([`safe_extract_entry`], anti-traversal);
//!   * the mappings `actual → target directory` / `actual → apply_kind` /
//!     `apply_kind → rexon_type` ([`actual_type_dir`], [`actual_to_kind`],
//!     [`kind_to_rexon_type`]) — keyed by apply_kind, not by OCS cat_id;
//!   * the install PLAN as a pure sequence of shared-plan steps
//!     ([`ExtStep`], the same one `.rxn` uses):
//!     choosing the target folder with reclassification by actual type, copies
//!     of `.colors`/the directory, the `gtk-update-icon-cache` argv, the log record —
//!     message texts byte-for-byte with Python ([`plan_install`]);
//!   * a single-file item ([`plan_single_file`]): computing the file name
//!     (basename → url → fallback name, guaranteeing the extension the category
//!     expects) and where it goes (the kind's directory or its own subfolder);
//!   * a system SDDM/Plymouth theme ([`plan_root_install`]): validation
//!     gates (PLM / no pkexec / no helper / SteamOS), theme name, `pkexec`
//!     argv, message texts/return codes;
//!   * applying ([`apply_action_for`] + config write plans
//!     [`widget_style_plan`]/[`lnf_component_plan`]/[`lock_wallpaper_plan`]/
//!     [`aurorae_plan`], detecting an outdated Global Theme
//!     [`global_theme_missing_metadata_json`], message texts per apply_kind);
//!   * picking a picture from a wallpaper pack ([`pack_images`], [`pick_pack_image`]) —
//!     a deterministic rule for the screen resolution; a sound theme
//!     is applied by the shared sound engine ([`crate::qt_settings::sound_theme_plan`]);
//!   * which kinds have an "applied right now" state at all ([`kind_has_active_state`])
//!     and which accept a single file instead of an archive
//!     ([`kind_single_file_rule`]);
//!   * uninstalling ([`plan_uninstall`]): the plan for removing paths from the log.
//!
//! The FILE SYSTEM is behind the [`FsSnapshot`] interface (as in `installed_scan.rs`):
//! the input is a snapshot of the unpacked tree in the reference's `os.scandir` order.
//!
//! NOT ported (execution — stays in Python): downloading, the actual
//! unpacking/copying (`shutil`), `subprocess`/`pkexec`, the actual apply
//! via `ThemeManager` (write plans are handed to the caller), `gtk_sanitizer`,
//! the persistent log (`record_installed`/`load_installed_log` — I/O), and
//! `tempfile.mkdtemp` itself (paths come from the executor). Bugs found in the
//! original are fixed right here (marked "fixed a bug of the original").

use std::path::Path;

use serde::Serialize;
use serde_json::Value;

use crate::categories::category_by_kind;
use crate::ext_manager::ExtStep;
use crate::i18n::{tr, trc, trf};
use crate::installed_scan::FsSnapshot;
use crate::models::{ThemeCategory, ThemeItem};
use crate::privilege::op::{PkgSource, PrivOp};

// ============================================================================
// Constants: picture extensions and magic signatures
// ============================================================================

/// Extensions of single wallpaper files — port of `_IMAGE_EXTS`.
pub const IMAGE_EXTS: &[&str] = &[
    ".png", ".jpg", ".jpeg", ".webp", ".bmp", ".gif", ".tif", ".tiff", ".avif",
    ".jxl", ".apng", ".svg",
];

/// Magic signatures of common picture formats — port of `_IMAGE_MAGIC`.
pub const IMAGE_MAGIC: &[&[u8]] = &[
    b"\x89PNG\r\n\x1a\n", // PNG
    b"\xff\xd8\xff",      // JPEG
    b"GIF87a",
    b"GIF89a",
    b"BM",       // BMP
    b"II*\x00",  // TIFF LE
    b"MM\x00*",  // TIFF BE
];

/// KDE color scheme extensions: a scheme is ONE file, and the Store serves it
/// that way (an archive around a lone `.colors` is not always there).
pub const COLOR_SCHEME_EXTS: &[&str] = &[".colors"];

/// Font file extensions: a font is also self-contained in one file.
pub const FONT_EXTS: &[&str] = &[
    ".ttf", ".otf", ".ttc", ".otc", ".pfb", ".pfa", ".pcf", ".bdf", ".woff",
    ".woff2",
];

/// Kvantum theme extensions: `.kvconfig` describes the whole theme.
pub const KVANTUM_EXTS: &[&str] = &[".kvconfig"];

// ============================================================================
// Path helpers (os.path semantics)
// ============================================================================

fn join(dir: &str, rel: &str) -> String {
    format!("{dir}/{rel}")
}

/// `os.path.join(a, b)`: an absolute `b` (leading `/`) replaces `a`; otherwise
/// joined with `/` (without doubling if `a` already ends with `/`).
fn py_join(a: &str, b: &str) -> String {
    if b.starts_with('/') {
        b.to_string()
    } else if a.is_empty() || a.ends_with('/') {
        format!("{a}{b}")
    } else {
        format!("{a}/{b}")
    }
}

/// `os.path.basename` (the last component after `/`).
fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// `str.rstrip("/")`.
fn rstrip_slashes(s: &str) -> &str {
    s.trim_end_matches('/')
}

/// `os.path.splitext` — (root, ext). Leading dots of the basename do not count
/// as an extension separator (as in CPython): `.bashrc` → (".bashrc", "").
fn py_splitext(name: &str) -> (String, String) {
    let base = basename(name);
    let prefix_len = name.len() - base.len();
    // index of the last dot in base
    let Some(dot) = base.rfind('.') else {
        return (name.to_string(), String::new());
    };
    // all characters before the dot are dots → no extension (leading dots)
    if base[..dot].chars().all(|c| c == '.') {
        return (name.to_string(), String::new());
    }
    let split_at = prefix_len + dot;
    (name[..split_at].to_string(), name[split_at..].to_string())
}

/// Lowercased extension from a name/URL (without query/fragment) — port of
/// `_file_ext`.
pub fn file_ext(name: &str) -> String {
    if name.is_empty() {
        return String::new();
    }
    let base = name.split('?').next().unwrap_or(name);
    let base = base.split('#').next().unwrap_or(base);
    py_splitext(base).1.to_lowercase()
}

// ============================================================================
// apply_kind / actual-type mappings
// ============================================================================

/// apply_kind → rexon_type for the log.
///
/// Every appearance kind has its OWN type. The two catch-alls that used to be
/// here are gone: `wallpaper-pack` lumped wallpapers, sounds, fonts and widgets
/// together (a sound theme from the Store went into the log as a wallpaper pack),
/// and `plugin` — the splash, the lock screen, the GTK theme and the application
/// style. The log type tells the tab which row of the active-theme map to match
/// the name against, and with a catch-all that question had no answer.
/// Wehrwolfmann 2026-08-22.
///
/// `plugin` stays the default — but now only for what really is outside
/// the kind table.
pub fn kind_to_rexon_type(apply_kind: &str) -> &'static str {
    match apply_kind {
        "global" => "theme-kde-global",
        "plasma" => "theme-kde-plasma",
        "colors" => "theme-kde-colors",
        "aurorae" => "theme-kde-aurorae",
        "icons" => "theme-icons",
        "cursor" => "theme-cursors",
        "kvantum" => "theme-kvantum",
        // Wallpapers: a pack, a single picture and the lock screen background are
        // all one content type; the record's apply_kind tells them apart.
        "wallpapers" | "lockwallpaper" => "wallpaper-pack",
        "wallpaper_plugins" => "kde-wallpaper-plugin",
        "sounds" => "theme-sounds",
        "fonts" => "font-pack",
        "plasmoids" => "theme-plasmoid",
        "gtk" => "theme-gtk",
        "widgetstyle" => "theme-widget-style",
        "splash" => "theme-kde-splash",
        "lockscreen" => "theme-kde-lockscreen",
        "sddm" => "theme-sddm",
        "plymouth" => "theme-plymouth",
        // Desktop sections (KWin scripts and effects, the window switcher from
        // the store): the type is the same as the section's in the matrix, no second list.
        k if crate::categories::DE_SECTIONS.iter().any(|(_, key)| *key == k) => {
            crate::categories::category_kind(k).unwrap_or("plugin")
        }
        _ => "plugin",
    }
}

/// Kinds that have a notion of "applied RIGHT NOW", i.e. those that
/// appear as a key in the active-theme map
/// ([`crate::installed_scan::active_map_from_sources`]).
///
/// The tab asks this before reading configs: fonts, widgets and wallpaper
/// plugins never have an active theme, and there is no point calling
/// kreadconfig6 for them. The list lives next to the map, not as a twin list
/// in the UI (rule #12): add a kind to the map — add it here.
pub fn kind_has_active_state(apply_kind: &str) -> bool {
    matches!(
        apply_kind,
        "global"
            | "plasma"
            | "colors"
            | "icons"
            | "cursor"
            | "gtk"
            | "widgetstyle"
            | "splash"
            | "lockscreen"
            | "aurorae"
            | "kvantum"
            | "sddm"
            | "plymouth"
            | "sounds"
            | "wallpapers"
    ) || de_section_has_active_theme(apply_kind)
}

/// A desktop section with a theme (xfwm4 and Marco frames, GNOME, Cinnamon
/// and LXQt shells, the Alt+Tab layout): one desktop key value — it does have
/// "applied now", and the active-theme map keeps it under the section key
/// (`installed_scan::LiveScan::de_active`). Without this, an item of such a
/// section installed and applied from the store stayed at "✅ Installed".
/// Extensions and Conky are not included: they have a list of enabled items
/// rather than one name.
fn de_section_has_active_theme(apply_kind: &str) -> bool {
    crate::categories::DE_SECTIONS.iter().any(|(_, key)| *key == apply_kind)
        && crate::categories::category_kind(apply_kind)
            .and_then(crate::de_apply::mode)
            .is_some_and(|m| matches!(m, crate::de_apply::Mode::Theme(_)))
}

/// The rule "the Store served one bare file rather than an archive" for an
/// appearance kind.
///
/// Wallpapers were not the exception, just the first case noticed: a color
/// scheme, a font and a Kvantum theme are also self-contained in one file, and
/// some items are published exactly like that — without a zip/tar wrapper. The
/// unpacker used to fire on them and the user got "Could not unpack the archive"
/// on a perfectly good item. Wehrwolfmann 2026-08-29.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SingleFileRule {
    /// Extensions the kind accepts as a bare file. The first is the one used
    /// to complete the name if the download has no extension at all.
    pub exts: &'static [&'static str],
    /// Whether this kind's content has a reliable signature in the first bytes:
    /// the name from the Store is sometimes empty, and then only the signature
    /// decides. We know such a signature only for pictures.
    pub has_magic: bool,
    /// Put the file into a subfolder named after the stem (Kvantum looks for a theme as
    /// `<dir>/<name>/<name>.kvconfig`) rather than directly into the kind's directory.
    pub own_dir: bool,
    /// The theme name is the file name's stem. Wallpapers are the opposite: a
    /// wallpaper is addressed by the whole file, and a cut extension would break the apply path.
    pub name_is_stem: bool,
    /// What to build the name from when the Store gave neither a name nor a link.
    pub fallback_stem: &'static str,
}

/// Single-file rule for a kind — `None` if the kind only comes as an archive
/// (icons, cursors, Global Theme and anything else that physically is a tree).
pub fn kind_single_file_rule(apply_kind: &str) -> Option<SingleFileRule> {
    match apply_kind {
        "wallpapers" | "wallpaper_plugins" | "lockwallpaper" => Some(SingleFileRule {
            exts: IMAGE_EXTS,
            has_magic: true,
            own_dir: false,
            name_is_stem: false,
            fallback_stem: "wallpaper",
        }),
        "colors" => Some(SingleFileRule {
            exts: COLOR_SCHEME_EXTS,
            has_magic: false,
            own_dir: false,
            name_is_stem: true,
            fallback_stem: "color-scheme",
        }),
        "fonts" => Some(SingleFileRule {
            exts: FONT_EXTS,
            has_magic: false,
            own_dir: false,
            name_is_stem: true,
            fallback_stem: "font",
        }),
        "kvantum" => Some(SingleFileRule {
            exts: KVANTUM_EXTS,
            has_magic: false,
            own_dir: true,
            name_is_stem: true,
            fallback_stem: "kvantum",
        }),
        _ => None,
    }
}

/// Actual type → apply_kind — port of `_actual_to_kind` (default `"none"`).
pub fn actual_to_kind(actual: &str) -> &'static str {
    match actual {
        "look-and-feel" => "global",
        "plasma-theme" => "plasma",
        "color-scheme" => "colors",
        "aurorae" => "aurorae",
        "kvantum" => "kvantum",
        "icons" => "icons",
        _ => "none",
    }
}

/// Actual type → target install directory — port of `_ACTUAL_TYPE_DIR`
/// (`_dir_for(kind) = category_by_kind(kind).install_dir`). `None` if the type
/// is outside the table (does not take part in reclassification).
pub fn actual_type_dir(home: &Path, actual: &str) -> Option<String> {
    let kind = match actual {
        "look-and-feel" => "global",
        "plasma-theme" => "plasma",
        "color-scheme" => "colors",
        "aurorae" => "aurorae",
        "kvantum" => "kvantum",
        "icons" => "icons",
        _ => return None,
    };
    Some(
        category_by_kind(home, kind)
            .map(|c| c.install_dir)
            .unwrap_or_default(),
    )
}

// ============================================================================
// Detecting the actual type of a KDE theme — port of _detect_actual_kde_type
// ============================================================================

/// Actual type of an unpacked KDE theme by the directory contents — port of
/// `ocs_store._detect_actual_kde_type` (the installer calls it). `None` —
/// could not be determined.
pub fn detect_actual_kde_type(fs: &FsSnapshot, content_root: &str) -> Option<String> {
    if !fs.is_dir(content_root) {
        return None;
    }
    let files: &[String] = fs.scandir(content_root);
    let is_dir_child = |name: &str| fs.is_dir(&join(content_root, name));

    // Bundle: ≥2 KDE components as top-level FOLDERS.
    const BUNDLE_MARKERS: &[&str] = &[
        "aurorae", "color-schemes", "kvantum", "Kvantum", "plasma", "icons",
        "wallpaper", "wallpapers", "sddm",
    ];
    let subdir_marker_hits = files
        .iter()
        .filter(|f| is_dir_child(f) && BUNDLE_MARKERS.contains(&f.as_str()))
        .count();
    if subdir_marker_hits >= 2 {
        return Some("bundle".to_string());
    }

    // Color scheme — .colors in the root.
    if files.iter().any(|f| f.ends_with(".colors")) {
        return Some("color-scheme".to_string());
    }

    // Aurorae — the characteristic window SVGs in the root.
    const AURORAE_MARKERS: &[&str] = &[
        "decoration.svg", "decoration.svgz", "close.svg", "minimize.svg",
        "maximize.svg",
    ];
    if files.iter().any(|f| AURORAE_MARKERS.contains(&f.as_str())) {
        return Some("aurorae".to_string());
    }

    // Kvantum — .kvconfig.
    if files.iter().any(|f| f.ends_with(".kvconfig")) {
        return Some("kvantum".to_string());
    }

    // Icon theme — index.theme + a typical icon subfolder.
    if files.iter().any(|f| f == "index.theme") {
        const ICON_SUBDIRS: &[&str] = &[
            "apps", "places", "mimetypes", "actions", "categories", "devices",
            "status",
        ];
        if files
            .iter()
            .any(|f| is_dir_child(f) && ICON_SUBDIRS.contains(&f.as_str()))
        {
            return Some("icons".to_string());
        }
    }

    // Plasma look-and-feel: contents/defaults.
    let contents_dir = join(content_root, "contents");
    if fs.is_dir(&contents_dir) {
        if fs.is_file(&join(&contents_dir, "defaults")) {
            return Some("look-and-feel".to_string());
        }
        const PLASMA_STYLE_SUBDIRS: &[&str] =
            &["widgets", "dialogs", "frames", "icons", "translucent"];
        if fs
            .scandir(&contents_dir)
            .iter()
            .any(|f| PLASMA_STYLE_SUBDIRS.contains(&f.as_str()))
        {
            return Some("plasma-theme".to_string());
        }
    }

    // metadata.desktop with an explicit ServiceType.
    let metadata = join(content_root, "metadata.desktop");
    if fs.is_file(&metadata) {
        if let Some(content) = fs.read_text(&metadata) {
            if content.contains("Plasma/LookAndFeel") {
                return Some("look-and-feel".to_string());
            }
            if content.contains("Plasma/Theme") {
                return Some("plasma-theme".to_string());
            }
        }
    }

    None
}

// ============================================================================
// Parsing the unpacked tree
// ============================================================================

/// Descend into the single top-level folder — port of `_find_content_root`.
/// Hidden (`.`-prefixed) entries are ignored in the count.
pub fn find_content_root(fs: &FsSnapshot, extract_dir: &str) -> String {
    let entries: Vec<&String> = fs
        .scandir(extract_dir)
        .iter()
        .filter(|e| !e.starts_with('.'))
        .collect();
    if entries.len() == 1 {
        let single = join(extract_dir, entries[0]);
        if fs.is_dir(&single) {
            return single;
        }
    }
    extract_dir.to_string()
}

// ============================================================================
// Theme roots inside an unpacked archive
// ============================================================================
//
// ⛔ Defect caught live on 03.10.2026: an archive was installed "as is" — the
// whole wrapper went into the kind's directory. An example from the Store: a sound theme
// landed as `~/.local/share/sounds/zorinos2/sounds/{Zorin,Zorin2}/index.theme`, next to
// wallpapers, xml and a ReadMe. A theme below the top level is invisible to Rexon, KDE
// and libcanberra alike: they all look for `<kind dir>/<Name>/index.theme`. Icons,
// cursors, GTK themes and Aurorae have the same class of problem — authors' archives
// often carry several variants under a common folder (`Orchis/Orchis-Dark/gtk-3.0`).
//
// One solution for all these kinds: find EVERY directory in the unpacked tree that
// is itself a theme of its kind, and install each at the top level of the kind's
// directory under its own name. Nothing else from the archive is
// copied.

/// Kinds for which installation looks for theme roots ([`find_theme_roots`]).
pub fn kind_uses_theme_roots(apply_kind: &str) -> bool {
    matches!(apply_kind, "sounds" | "icons" | "cursor" | "gtk" | "aurorae")
        || de_theme_marker(apply_kind).is_some()
}

/// How the theme root of a desktop section is recognized.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeThemeMarker {
    /// A subdirectory in the theme root (`xfwm4/`, `metacity-1/`, `gnome-shell/`,
    /// `cinnamon/`) — a matrix row with [`crate::de_customization::Payload::ThemeSubdir`].
    Subdir(&'static str),
    /// A file in the theme root: an LXQt theme without `lxqt-panel.qss` does not change the panel
    /// (note on the `theme-lxqt` matrix row).
    File(&'static str),
}

/// The file without which an LXQt theme directory is not an LXQt theme.
const LXQT_THEME_FILE: &str = "lxqt-panel.qss";

/// Theme root marker for the desktop section `apply_kind` (`xfwm4`, `marco`,
/// `gnomeshell`, `cinnamontheme`, `lxqttheme`); `None` — the section is not a theme.
///
/// The subdirectory comes from the matrix row, there is no second list (rule #12):
/// for a new `ThemeSubdir` row, archive layout will just work.
///
/// ⛔ Defect caught by the test bench on real pling archives on 17.10.2026:
/// desktop sections were installed via the "copy the directory as is" branch. An archive with
/// several variants (`Win8-Metacity`: 16 themes in the root; `ChrButtons/
/// Chr*/xfwm4`; `Marble-shell`; `QTStep`) landed as a single `extracted` folder
/// or as the wrapper, and neither Rexon nor the desktop itself saw the themes in it.
pub fn de_theme_marker(apply_kind: &str) -> Option<DeThemeMarker> {
    if !crate::categories::DE_SECTIONS.iter().any(|(_, key)| *key == apply_kind) {
        return None;
    }
    let kind = crate::categories::category_kind(apply_kind)?;
    let row = crate::de_customization::row(kind)?;
    match row.payload {
        crate::de_customization::Payload::ThemeSubdir { sub } => Some(DeThemeMarker::Subdir(sub)),
        _ if kind == "theme-lxqt" => Some(DeThemeMarker::File(LXQT_THEME_FILE)),
        _ => None,
    }
}

/// Directories by which a GTK theme is recognized (they sit right in its root).
const GTK_THEME_MARKERS: &[&str] = &[
    "gtk-2.0", "gtk-3.0", "gtk-4.0", "gnome-shell", "xfwm4", "metacity-1",
    "cinnamon",
];

/// Whether the `index.theme` of directory `dir` has the section `section` (`[Sound Theme]`,
/// `[Icon Theme]`). Compared ignoring whitespace around the line.
fn index_theme_has(fs: &FsSnapshot, dir: &str, section: &str) -> bool {
    fs.read_text(&join(dir, "index.theme"))
        .map(|t| t.lines().any(|l| l.trim() == section))
        .unwrap_or(false)
}

/// Whether directory `dir` IS ITSELF a theme of kind `apply_kind`:
/// * sounds — `index.theme` with a `[Sound Theme]` section (the freedesktop
///   sound-theme specification);
/// * icons and cursors — `index.theme` with `[Icon Theme]` or a `cursors/`
///   subdirectory (cursors do not always have `index.theme`). Both kinds are
///   installed into the same `icons` directory, so the markers are shared: an
///   "icons + cursors" archive installs whole;
/// * GTK — the root has `gtk-3.0/`, `gtk-2.0/`, `gnome-shell/` and so on;
/// * Aurorae — `decoration.svg(z)` in the root;
/// * desktop sections — per [`de_theme_marker`]: a frame or shell subdirectory,
///   for LXQt — `lxqt-panel.qss`.
pub fn is_theme_root(fs: &FsSnapshot, dir: &str, apply_kind: &str) -> bool {
    let has_child_dir = |name: &str| fs.is_dir(&join(dir, name));
    match apply_kind {
        "sounds" => index_theme_has(fs, dir, "[Sound Theme]"),
        "icons" | "cursor" => index_theme_has(fs, dir, "[Icon Theme]") || has_child_dir("cursors"),
        "gtk" => GTK_THEME_MARKERS.iter().any(|m| has_child_dir(m)),
        "aurorae" => {
            fs.is_file(&join(dir, "decoration.svg")) || fs.is_file(&join(dir, "decoration.svgz"))
        }
        other => match de_theme_marker(other) {
            Some(DeThemeMarker::Subdir(sub)) => has_child_dir(sub),
            Some(DeThemeMarker::File(name)) => fs.is_file(&join(dir, name)),
            None => false,
        },
    }
}

/// All theme roots of kind `apply_kind` under `root` (including `root` itself), in
/// snapshot traversal order. The traversal does not descend into a found root: there is
/// no theme inside a theme, and `stereo/` or `cursors/` are its own parts. Hidden
/// directories (`.git` and the like) are skipped.
pub fn find_theme_roots(fs: &FsSnapshot, root: &str, apply_kind: &str) -> Vec<String> {
    fn walk(fs: &FsSnapshot, dir: &str, kind: &str, depth: usize, out: &mut Vec<String>) {
        if is_theme_root(fs, dir, kind) {
            out.push(dir.to_string());
            return;
        }
        // Guard against insane nesting (and loops in the snapshot).
        if depth >= 12 {
            return;
        }
        for name in fs.scandir(dir) {
            if name.starts_with('.') {
                continue;
            }
            let child = join(dir, name);
            if fs.is_dir(&child) {
                walk(fs, &child, kind, depth + 1, out);
            }
        }
    }
    let mut out = Vec::new();
    if fs.is_dir(root) {
        walk(fs, root, apply_kind, 0, &mut out);
    }
    out
}

/// All `*.colors` under `root` in snapshot traversal order: `(path, file name)`.
fn colors_files(fs: &FsSnapshot, root: &str) -> Vec<(String, String)> {
    fn walk(fs: &FsSnapshot, dir: &str, depth: usize, out: &mut Vec<(String, String)>) {
        for name in fs.scandir(dir) {
            let path = join(dir, name);
            if fs.is_dir(&path) {
                if depth < 12 && !name.starts_with('.') {
                    walk(fs, &path, depth + 1, out);
                }
            } else if name.ends_with(".colors") {
                out.push((path, name.clone()));
            }
        }
    }
    let mut out = Vec::new();
    walk(fs, root, 0, &mut out);
    out
}

/// Whether the path of a single file `dest` is taken by someone else: something is
/// there, this purchase did not install it (`replaceable`), and it is not the same file byte
/// for byte (the snapshot reads `.colors` texts — [`install_tree_snapshot`]).
fn file_taken(fs: &FsSnapshot, replaceable: Option<&[String]>, src: &str, dest: &str) -> bool {
    if !(fs.is_file(dest) || fs.is_dir(dest)) {
        return false;
    }
    if replaceable.is_some_and(|own| own.iter().any(|p| rstrip_slashes(p) == dest)) {
        return false;
    }
    let same = !fs.is_dir(dest)
        && fs.read_text(src).is_some_and(|t| !t.is_empty() && fs.read_text(dest) == Some(t));
    !same
}

/// Why a theme from the archive was not installed (see [`plan_theme_roots`]).
fn skipped_note(name: &str, reason: SkipReason) -> String {
    match reason {
        SkipReason::DuplicateInArchive => trf(
            "“%1” skipped: the archive contains two themes with this name",
            &[&name],
        ),
        SkipReason::TakenByOther => trf(
            "“%1” skipped: a different theme with this name is already installed",
            &[&name],
        ),
        SkipReason::SharedIconSet => trf(
            "“%1” skipped: this is the shared icon set of all programs, a theme must not replace it",
            &[&name],
        ),
    }
}

/// An icon or cursor theme named after a shared set (`hicolor`, `locolor`):
/// all programs share such a directory in the profile, and its `index.theme`
/// would shadow the system one — icons would vanish across the whole desktop (defect of
/// 09.11.2026, [`crate::icon_hygiene`]).
fn is_shared_icon_set(apply_kind: &str, name: &str) -> bool {
    matches!(apply_kind, "icons" | "cursor") && crate::icon_hygiene::is_reserved_icon_set(name)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SkipReason {
    DuplicateInArchive,
    TakenByOther,
    /// A shared icon set (`hicolor`, `locolor`) — not a theme.
    SharedIconSet,
}

/// The message "the archive has no theme of this kind" — only for sounds: for other
/// kinds, installation without found roots goes the old way.
pub fn no_sound_theme_message() -> String {
    tr("The archive contains no sound theme: no index.theme with a [Sound Theme] section was found.")
}

/// The [`plan_install_with`] branch for kinds with theme roots: each root goes to the
/// top level of `dest_base` under its own name.
///
/// `replaceable` — paths this same purchase installed earlier (from the log):
/// they may be removed and installed again. `None` — the old "a taken name
/// is overwritten" behavior. With `Some`, a directory taken by SOMEONE ELSE's theme (installed
/// by hand or by another purchase) is not touched — the theme is skipped with an honest
/// line in the message instead of silently overwriting someone else's.
/// Whether a string is fit to be one directory name: not empty, not `.`/`..`, without
/// a path separator or NUL.
fn is_single_path_component(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains('/') && !name.contains('\0')
}

fn plan_theme_roots(
    fs: &FsSnapshot,
    roots: &[(String, String)],
    dest_base: &str,
    apply_kind: &str,
    replaceable: Option<&[String]>,
    reclassified: Option<String>,
) -> InstallOutcome {
    let mut steps = vec![ExtStep::MakeDirs { path: dest_base.to_string() }];
    let mut names: Vec<String> = Vec::new();
    let mut paths: Vec<String> = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    for (root, name) in roots {
        let name = name.clone();
        // The theme directory name is one path component, otherwise `dest` goes
        // above the kind's directory (`..`) or into it itself (`.`).
        if !is_single_path_component(&name) {
            notes.push(skipped_note(&name, SkipReason::TakenByOther));
            continue;
        }
        if names.contains(&name) {
            notes.push(skipped_note(&name, SkipReason::DuplicateInArchive));
            continue;
        }
        if is_shared_icon_set(apply_kind, &name) {
            notes.push(skipped_note(&name, SkipReason::SharedIconSet));
            continue;
        }
        let dest = join(dest_base, &name);
        // A window frame (shell) named like an already installed GTK theme: goes
        // as a subfolder inside it, as in Mint and Xubuntu (`Arc/xfwm4`).
        if let Some(nest) = plan_nested_part(fs, root, &name, &dest, dest_base, apply_kind, replaceable) {
            match nest {
                Ok((part_steps, part)) => {
                    steps.extend(part_steps);
                    names.push(name);
                    paths.push(part);
                }
                Err(reason) => notes.push(skipped_note(&name, reason)),
            }
            continue;
        }
        let exists = fs.is_dir(&dest) || fs.is_file(&dest);
        if exists {
            if let Some(own) = replaceable {
                if !own.iter().any(|p| rstrip_slashes(p) == dest) {
                    notes.push(skipped_note(&name, SkipReason::TakenByOther));
                    continue;
                }
            }
            steps.push(ExtStep::RemoveTree { path: dest.clone(), safe: false, ignore_errors: true });
        }
        steps.push(ExtStep::CopyTree { src: root.clone(), dest: dest.clone() });
        if apply_kind == "icons" || apply_kind == "cursor" {
            steps.push(ExtStep::Run {
                argv: vec![
                    "gtk-update-icon-cache".into(),
                    "-f".into(),
                    "-t".into(),
                    dest.clone(),
                ],
                timeout_secs: Some(60),
            });
        }
        names.push(name);
        paths.push(dest);
    }
    if names.is_empty() {
        return InstallOutcome::err(notes.join("\n"));
    }
    let mut message = trf("Installed: %1", &[&names.join(", ")]);
    if !notes.is_empty() {
        message = format!("{message}\n{}", notes.join("\n"));
    }
    InstallOutcome {
        ok: true,
        message,
        names,
        steps,
        reclassified,
        record: Some(RecordInfo {
            rexon_type: kind_to_rexon_type(apply_kind).to_string(),
            apply_kind: apply_kind.to_string(),
            paths,
        }),
    }
}

/// The theme folder named `name` that a desktop section part will go into
/// (`xfwm4/`, `metacity-1/`, `gnome-shell/`, `cinnamon/`): the real GTK theme
/// folder in `~/.local/share/themes` (our link in `~/.themes` points to it),
/// otherwise `~/.themes/<name>` itself. `None` — there is no folder.
fn nest_host(fs: &FsSnapshot, name: &str, dest: &str, dest_base: &str) -> Option<String> {
    if let Some(home) = rstrip_slashes(dest_base).strip_suffix("/.themes") {
        let xdg = join(&crate::installed_scan::xdg_themes_dir(home), name);
        if fs.is_dir(&xdg) {
            return Some(xdg);
        }
    }
    fs.is_dir(dest).then(|| dest.to_string())
}

/// Decision 3c by Wehrwolfmann, 20.10.2026: a window frame (Marco — `metacity-1/`,
/// xfwm4 — `xfwm4/`; likewise the GNOME and Cinnamon shells) with the same name as
/// an already installed GTK theme goes INSIDE its folder as its own subfolder —
/// the standard layout of freedesktop themes (Mint-Y, Greybird). It cannot go in a
/// separate folder: the name is taken, and `~/.themes/<name>` is our link
/// to the GTK theme.
///
/// `None` — nowhere to nest (no folder with that name, the section is not of this kind, or
/// it is an earlier separate folder of this same purchase): the usual path. Otherwise —
/// the steps and the part's path for the log, or the reason to skip: the subfolder already exists
/// and it is not this purchase's.
fn plan_nested_part(
    fs: &FsSnapshot,
    root: &str,
    name: &str,
    dest: &str,
    dest_base: &str,
    apply_kind: &str,
    replaceable: Option<&[String]>,
) -> Option<Result<(Vec<ExtStep>, String), SkipReason>> {
    let Some(DeThemeMarker::Subdir(sub)) = de_theme_marker(apply_kind) else { return None };
    let own = |p: &str| replaceable.is_some_and(|own| own.iter().any(|o| rstrip_slashes(o) == p));
    if own(dest) {
        return None; // our own separate folder — reinstalled whole
    }
    let host = nest_host(fs, name, dest, dest_base)?;
    let part = join(&host, sub);
    let mut steps = Vec::new();
    if fs.is_dir(&part) || fs.is_file(&part) {
        // Someone else's subfolder (the GTK theme's own frame, installed by hand) is not
        // overwritten; without a log (`None`) — the old "a taken one is
        // overwritten", but only the subfolder itself.
        if replaceable.is_some() && !own(&part) {
            return Some(Err(SkipReason::TakenByOther));
        }
        steps.push(ExtStep::RemoveTree { path: part.clone(), safe: false, ignore_errors: true });
    }
    steps.push(ExtStep::CopyTree { src: join(root, sub), dest: part.clone() });
    Some(Ok((steps, part)))
}

/// Theme name from metadata.desktop/json, otherwise the folder name — port of `_theme_id_from`.
pub fn theme_id_from(fs: &FsSnapshot, content_root: &str) -> String {
    let theme_id = basename(rstrip_slashes(content_root)).to_string();
    let md = join(content_root, "metadata.desktop");
    let mj = join(content_root, "metadata.json");
    if fs.is_file(&md) {
        if let Some(text) = fs.read_text(&md) {
            for line in text.split_inclusive('\n') {
                // Python iterates the file's lines (with '\n'); .strip() removes it.
                if line.trim().starts_with("X-KDE-PluginInfo-Name=") {
                    // split("=", 1)[1].strip()
                    let val = line.split_once('=').map(|(_, v)| v).unwrap_or("").trim();
                    // A name from foreign metadata — only as a single directory name:
                    // "../.." would lead the installation out of the kind's directory.
                    return if is_single_path_component(val) { val.to_string() } else { theme_id };
                }
            }
        }
    } else if fs.is_file(&mj) {
        if let Some(text) = fs.read_text(&mj) {
            if let Ok(Value::Object(meta)) = serde_json::from_str::<Value>(text) {
                if let Some(id) = meta
                    .get("KPlugin")
                    .and_then(Value::as_object)
                    .and_then(|kp| kp.get("Id"))
                    .and_then(Value::as_str)
                    .filter(|id| is_single_path_component(id))
                {
                    return id.to_string();
                }
            }
            // not an object / no KPlugin.Id / parse error → folder name.
        }
    }
    theme_id
}

/// Plymouth theme name from `<name>.plymouth` in the root — port of `_plymouth_theme_name`
/// (the first in `os.listdir` order; `""` — no such file).
pub fn plymouth_theme_name(fs: &FsSnapshot, content_root: &str) -> String {
    for f in fs.scandir(content_root) {
        if let Some(stem) = f.strip_suffix(".plymouth") {
            return stem.to_string();
        }
    }
    String::new()
}

// ============================================================================
// Detecting an item delivered as one file / unpacking safety
// ============================================================================

/// True if the downloaded file is a single picture (not an archive).
///
/// A component of [`single_file_accepted`]: unlike other single files, a picture's
/// content is recognized by its signature, and that is the only way to recognize
/// a wallpaper when the Store gave no file name.
///
/// `is_archive` — the result of `zipfile.is_zipfile(path) or tarfile.is_tarfile`
/// (the executor computes it on the real file; an archive takes priority).
/// `head` — the first ≤16 bytes of the file. `hints` — download_name / download_url.
pub fn looks_like_image(is_archive: bool, head: &[u8], hints: &[&str]) -> bool {
    if is_archive {
        return false;
    }
    for h in hints {
        if IMAGE_EXTS.contains(&file_ext(h).as_str()) {
            return true;
        }
    }
    if IMAGE_MAGIC.iter().any(|sig| head.starts_with(sig)) {
        return true;
    }
    // WEBP: "RIFF"…"WEBP" (bytes 0..3 = RIFF, 8..11 = WEBP).
    if head.len() >= 12 && &head[..4] == b"RIFF" && &head[8..12] == b"WEBP" {
        return true;
    }
    false
}

/// Whether to install the download as ONE bare file instead of unpacking it.
///
/// Decided by two facts, both required:
///   1. there is nothing to unpack with — `is_archive` (a real zip/tar probe by the executor)
///      is false, i.e. the unpacker could do nothing but fail on this file;
///   2. the file looks like what the category itself expects — an extension from its rule
///      ([`kind_single_file_rule`]), and for pictures also a content signature,
///      because the name from the Store can be empty.
///
/// This fork used to exist only for wallpapers, and any other kind
/// published as a bare file ran into "Could not unpack the archive".
///
/// `head` — the first ≤16 bytes of the file, `hints` — download_name / download_url.
pub fn single_file_accepted(
    apply_kind: &str,
    is_archive: bool,
    head: &[u8],
    hints: &[&str],
) -> bool {
    if is_archive {
        return false;
    }
    let Some(rule) = kind_single_file_rule(apply_kind) else {
        return false;
    };
    if hints
        .iter()
        .any(|h| rule.exts.contains(&file_ext(h).as_str()))
    {
        return true;
    }
    rule.has_magic && looks_like_image(is_archive, head, hints)
}

/// Whether an archive entry is safe relative to the unpack directory — port of
/// the `_safe` check from `_safe_extract` (anti path-traversal).
///
/// `dest_abs` — `os.path.abspath(dest)` (the unpack directory), `name` — the name
/// of the entry inside the archive. True if the target == dest_abs or is under `dest_abs/`.
pub fn safe_extract_entry(dest_abs: &str, name: &str) -> bool {
    let target = normpath(&py_join(dest_abs, name));
    target == dest_abs || target.starts_with(&format!("{dest_abs}/"))
}

/// Lexical normalization of an absolute path (like `os.path.abspath` without
/// touching the FS): collapses `.`/`..`/repeated `/`. The leading `/` is kept.
fn normpath(path: &str) -> String {
    let absolute = path.starts_with('/');
    let mut stack: Vec<&str> = Vec::new();
    for comp in path.split('/') {
        match comp {
            "" | "." => {}
            ".." => {
                if matches!(stack.last(), Some(&s) if s != "..") {
                    stack.pop();
                } else if !absolute {
                    stack.push("..");
                }
            }
            other => stack.push(other),
        }
    }
    let joined = stack.join("/");
    if absolute {
        format!("/{joined}")
    } else if joined.is_empty() {
        ".".to_string()
    } else {
        joined
    }
}

// ============================================================================
// Install plan
// ============================================================================

/// A record for the installed log (what the executor must record).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RecordInfo {
    pub apply_kind: String,
    pub rexon_type: String,
    pub paths: Vec<String>,
}

/// Result of install planning — the original's `(ok, message, names)` plus
/// the step plan and the log record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InstallOutcome {
    pub ok: bool,
    pub message: String,
    pub names: Vec<String>,
    pub steps: Vec<ExtStep>,
    /// Reclassification by actual type (`None` if there was none).
    pub reclassified: Option<String>,
    /// Log record (only on success).
    pub record: Option<RecordInfo>,
}

impl InstallOutcome {
    fn err(message: impl Into<String>) -> Self {
        InstallOutcome {
            ok: false,
            message: message.into(),
            names: Vec::new(),
            steps: Vec::new(),
            reclassified: None,
            record: None,
        }
    }
}

/// apply_kinds whose category is exact and for which reclassification is forbidden.
/// The contents of container kinds (wallpapers, sounds, fonts, widgets, wallpaper plugins)
/// look like anything, and "detecting the actual type" would lead them into
/// someone else's directory. Desktop sections (KWin scripts and effects, the window
/// switcher — a KPackage with `metadata.json`) are exact too: only the category
/// knows their place — they are checked by [`keeps_category`], not by this list.
const NO_RECLASSIFY: &[&str] = &[
    "colors", "none", "wallpapers", "wallpaper_plugins", "fonts", "sounds",
    "plasmoids", "lockwallpaper", "splash", "lockscreen",
];

/// Whether a kind is installed as a KPackage — the matrix decides (`Payload::KPackage`):
/// Plasma widgets, wallpaper plugins, KWin scripts and effects, the window switcher.
pub(crate) fn is_kpackage_kind(apply_kind: &str) -> bool {
    crate::categories::category_kind(apply_kind)
        .and_then(crate::de_customization::row)
        .is_some_and(|r| matches!(r.payload, crate::de_customization::Payload::KPackage { .. }))
}

/// Refusal for a KPackage-kind item that is not a ready package; `None` —
/// the package is ready (or the kind is not installed as a KPackage). Ready means the
/// package description and a `contents/` directory in the root: KNewStuff in Discover requires the same.
///
/// Live store measurements on 04.10.2026 — without the check, the tree landed in
/// the kind's directory as junk, and "Enable"/"Add widget" then silently found
/// nothing:
/// * KWin effects (719): seven of 46 are C++ sources or pacman packages;
/// * widgets (705): of 80 items, three are C++ sources with CMake
///   (`awesome-widgets-4.1.0-src.tar.xz`, `jarvis-main.zip`) plus a Dolphin
///   menu with a script, published under widgets (`Print-Selected-image`);
/// * wallpaper plugins (715): of 40, one is a pacman package
///   (`WallpaperEngine-kde6-…-x86-64.pkg.tar.zst`), it landed as a `usr` directory.
///
/// There are two reasons, and the person is told different things: sources or a package for
/// one system (needs building) — [`not_a_package_message`]; no ready package
/// at all — [`no_package_inside_message`].
fn kpackage_refusal(fs: &FsSnapshot, apply_kind: &str, content_root: &str) -> Option<String> {
    if !is_kpackage_kind(apply_kind) {
        return None;
    }
    let described = fs.is_file(&join(content_root, "metadata.json"))
        || fs.is_file(&join(content_root, "metadata.desktop"));
    if described && fs.is_dir(&join(content_root, "contents")) {
        return None;
    }
    Some(if has_build_sources(fs, content_root) {
        not_a_package_message()
    } else {
        no_package_inside_message()
    })
}

/// Sources that need building (CMake, Meson, C++), or a pacman package.
///
/// A pacman package is recognized by `.PKGINFO`: it sits in the root of the unpacked tree
/// next to `usr/`, while [`find_content_root`] takes `usr/` as the content root
/// (it skips service names starting with a dot) — so we also look one level
/// up. Traversal depth is limited: sources sit at the very top of the archive.
fn has_build_sources(fs: &FsSnapshot, content_root: &str) -> bool {
    let parent = rstrip_slashes(content_root).rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    let pacman = fs.is_file(&join(content_root, ".PKGINFO"))
        || (!parent.is_empty() && fs.is_file(&join(parent, ".PKGINFO")));
    if pacman {
        return true;
    }
    fn walk(fs: &FsSnapshot, dir: &str, depth: usize) -> bool {
        fs.scandir(dir).iter().any(|name| {
            let path = join(dir, name);
            if fs.is_dir(&path) {
                depth < 3 && walk(fs, &path, depth + 1)
            } else {
                name == "CMakeLists.txt"
                    || name == "meson.build"
                    || [".cpp", ".cc", ".cxx"].iter().any(|e| name.ends_with(e))
            }
        })
    }
    walk(fs, content_root, 0)
}

/// A hint at the end of a refusal: what to do next. The item page button
/// is already in the card ([`crate::appearance_page::detail_open_site_btn`]),
/// and that is usually where the author explains how to install their work.
fn author_page_hint() -> String {
    trf(
        "How to install it is usually written on the author's page — the “%1” button in the item's card.",
        &[&crate::appearance_page::detail_open_site_btn()],
    )
}

/// Install refusal for an item that is not a ready package: sources or a
/// package for one system.
pub fn not_a_package_message() -> String {
    format!(
        "{} {}",
        tr("This isn't a ready-to-use package: the author published source code (or a \
            package built for one particular system), and it has to be built first. \
            Rexon can't install it as is."),
        author_page_hint()
    )
}

/// Install refusal for an item that has no ready package of this kind
/// at all (something else was published in the widgets section).
pub fn no_package_inside_message() -> String {
    format!(
        "{} {}",
        tr("There's no ready-to-install package of this kind in this download, so Rexon \
            can't tell what to install from it."),
        author_page_hint()
    )
}

/// Whether reclassification by content is forbidden for the kind.
fn keeps_category(apply_kind: &str) -> bool {
    NO_RECLASSIFY.contains(&apply_kind)
        || crate::categories::DE_SECTIONS.iter().any(|(_, key)| *key == apply_kind)
}

/// Port of the "archive" branch of `ThemeInstaller.install` (after unpacking): choosing
/// the target folder with reclassification, copies, `gtk-update-icon-cache`, the record.
///
/// `home` expands `~` during reclassification; `cat` — the category by
/// `item.category_id`; `content_root` — the root of the unpacked tree (its snapshot is in
/// `fs`). The requires_root/single-file gate is outside (see
/// [`plan_root_install`] / [`plan_single_file`]).
pub fn plan_install(
    home: &Path,
    fs: &FsSnapshot,
    cat: &ThemeCategory,
    content_root: &str,
) -> InstallOutcome {
    plan_install_with(home, fs, cat, content_root, None)
}

/// [`plan_install`] with a check for taken names: `replaceable` — paths that
/// this same purchase installed (from the log). A theme directory taken by something else is not
/// overwritten (see [`plan_theme_roots`]). The live installer calls
/// exactly this form.
pub fn plan_install_with(
    home: &Path,
    fs: &FsSnapshot,
    cat: &ThemeCategory,
    content_root: &str,
    replaceable: Option<&[String]>,
) -> InstallOutcome {
    plan_install_inner(home, fs, cat, content_root, replaceable, None)
}

/// Where the unpacked tree came from: the unpack directory and the theme name for the case
/// when the theme has no folder of its own in the archive.
#[derive(Debug, Clone, Copy)]
pub struct ArchiveOrigin<'a> {
    /// The directory the archive was unpacked into (its name is a service one, `extracted`).
    pub extract_dir: &'a str,
    /// The theme name, if its root is the unpack directory itself
    /// ([`archive_theme_name`]).
    pub fallback_name: &'a str,
}

/// Theme name for an archive without its own folder: the file name from the store without
/// the archive extension and without the old gnome-look number prefix
/// (`99823-Miu.tar.gz` → `Miu`), otherwise the item title.
///
/// ⛔ Test bench defect of 17.10.2026: the archive `Rosepine-B-GS-Dark.tar.xz` carries
/// `gnome-shell/` right in its root, and `Marble-shell.zip` carries variants without a wrapper.
/// The unpack directory is called `extracted`, and the theme landed as
/// `~/.themes/extracted` (or `~/.themes/gnome-shell`).
pub fn archive_theme_name(download_name: &str, item_name: &str) -> String {
    let file = basename(rstrip_slashes(download_name));
    let lower = file.to_lowercase();
    let mut stem = crate::ocs::ARCHIVE_EXTS
        .iter()
        .find(|e| lower.ends_with(**e))
        .map(|e| &file[..file.len() - e.len()])
        .unwrap_or(file);
    if let Some((num, rest)) = stem.split_once('-') {
        if !num.is_empty() && num.bytes().all(|b| b.is_ascii_digit()) && !rest.trim().is_empty() {
            stem = rest;
        }
    }
    // The already trimmed name is checked: " .." after trimming is "..", and
    // `~/.themes/..` is HOME itself.
    let ok = |n: &str| {
        let n = n.trim();
        !n.is_empty() && !n.starts_with('.') && !n.contains('/') && !n.contains('\0')
    };
    if ok(stem) {
        return stem.trim().to_string();
    }
    let name = item_name.trim().replace('/', "-");
    if ok(&name) {
        return name;
    }
    "theme".to_string()
}

/// [`plan_install_with`] for an archive unpacked into `extract_dir`: theme roots
/// are searched from the unpack directory itself (a theme without its own folder is still a
/// theme), and a nameless root gets `fallback_name`. The live installer
/// calls exactly this form.
pub fn plan_install_archive(
    home: &Path,
    fs: &FsSnapshot,
    cat: &ThemeCategory,
    origin: ArchiveOrigin,
    replaceable: Option<&[String]>,
) -> InstallOutcome {
    let content_root = find_content_root(fs, origin.extract_dir);
    plan_install_inner(home, fs, cat, &content_root, replaceable, Some(origin))
}

fn plan_install_inner(
    home: &Path,
    fs: &FsSnapshot,
    cat: &ThemeCategory,
    content_root: &str,
    replaceable: Option<&[String]>,
    origin: Option<ArchiveOrigin>,
) -> InstallOutcome {
    // Name of root `dir`: the unpack directory has no name of its own.
    let name_of = |dir: &str, own: String| match origin {
        Some(o) if rstrip_slashes(dir) == rstrip_slashes(o.extract_dir) => o.fallback_name.to_string(),
        _ => own,
    };
    let actual = detect_actual_kde_type(fs, content_root);

    let mut dest_base = cat.install_dir.clone();
    let mut apply_kind = cat.apply_kind.clone();
    let mut reclassified: Option<String> = None;

    if let Some(actual) = actual.as_deref() {
        if !keeps_category(&cat.apply_kind) {
            if let Some(actual_dir) = actual_type_dir(home, actual) {
                // `actual in _ACTUAL_TYPE_DIR` is true exactly when
                // actual_type_dir returned Some.
                if actual_dir != dest_base {
                    dest_base = actual_dir;
                    apply_kind = actual_to_kind(actual).to_string();
                    reclassified = Some(actual.to_string());
                }
            }
        }
    }

    if let Some(refusal) = kpackage_refusal(fs, &apply_kind, content_root) {
        return InstallOutcome::err(refusal);
    }

    let mut steps = vec![ExtStep::MakeDirs { path: dest_base.clone() }];

    // color-scheme: copy .colors files — from ANY depth of the archive (the same
    // nesting defect as with theme roots: a scheme in `Pack/colors/X.colors`
    // used to give ".colors file not found"). A second one with the same name is skipped.
    if apply_kind == "colors" {
        let mut names: Vec<String> = Vec::new();
        let mut paths = Vec::new();
        let mut notes: Vec<String> = Vec::new();
        let mut seen: Vec<String> = Vec::new();
        for (src, f) in colors_files(fs, content_root) {
            if let Some(stem) = f.strip_suffix(".colors") {
                if seen.iter().any(|n| n == stem) {
                    continue;
                }
                seen.push(stem.to_string());
                let dst = join(&dest_base, &f);
                // The owner rule for files (as with `.rxn`): if there is a scheme in place
                // that this purchase did not install, with different content, it is the
                // person's or another purchase's scheme: it is not overwritten.
                if replaceable.is_some() && file_taken(fs, replaceable, &src, &dst) {
                    notes.push(skipped_note(stem, SkipReason::TakenByOther));
                    continue;
                }
                steps.push(ExtStep::CopyFile { src, dest: dst.clone() });
                names.push(stem.to_string());
                paths.push(dst);
            }
        }
        if names.is_empty() {
            if !notes.is_empty() {
                return InstallOutcome::err(notes.join("\n"));
            }
            return InstallOutcome::err(tr("No .colors file found"));
        }
        let mut message = trf("Installed: %1", &[&names.join(", ")]);
        if !notes.is_empty() {
            message = format!("{message}\n{}", notes.join("\n"));
        }
        return InstallOutcome {
            ok: true,
            message,
            names,
            steps,
            reclassified,
            record: Some(RecordInfo {
                rexon_type: kind_to_rexon_type(&apply_kind).to_string(),
                apply_kind,
                paths,
            }),
        };
    }

    // Sounds, icons, cursors, GTK, Aurorae: each theme root goes to the top
    // level of the kind's directory; the archive wrapper and everything extra stay out.
    if kind_uses_theme_roots(&apply_kind) {
        // From the unpack directory, not from the single folder in it: an archive
        // `gnome-shell/…` without a wrapper is still a shell theme.
        let search_from = origin.map(|o| o.extract_dir).unwrap_or(content_root);
        let roots = find_theme_roots(fs, search_from, &apply_kind);
        let single_top = roots.len() == 1 && roots[0] == content_root;
        if roots.is_empty() && apply_kind == "sounds" {
            return InstallOutcome::err(no_sound_theme_message());
        }
        // A desktop section with not a single theme of its kind in the archive (fonts and
        // wallpapers posing as a Cinnamon theme, an Openbox theme in the LXQt section): nothing
        // to install — the desktop would not show a junk directory anyway.
        if roots.is_empty() && de_theme_marker(&apply_kind).is_some() {
            return InstallOutcome::err(no_package_inside_message());
        }
        if !roots.is_empty() && (!single_top || replaceable.is_some()) {
            let pairs: Vec<(String, String)> = if single_top {
                // A theme in the root itself: the old name (metadata or folder).
                vec![(content_root.to_string(), name_of(content_root, theme_id_from(fs, content_root)))]
            } else {
                roots
                    .iter()
                    .map(|r| (r.clone(), name_of(r, basename(rstrip_slashes(r)).to_string())))
                    .collect()
            };
            return plan_theme_roots(fs, &pairs, &dest_base, &apply_kind, replaceable, reclassified);
        }
        // No roots found (icons without index.theme and the like) — the old path.
    }

    // A Conky theme pack (`.cmtp.7z` and archives laid out the same way): widgets
    // sit as folders in `.conky/` (`.Conky/`, `.config/conky/`), fonts — in
    // `.fonts/`. Each widget becomes its own tile.
    if apply_kind == "conky" {
        let search_from = origin.map(|o| o.extract_dir).unwrap_or(content_root);
        let parts = conky_pack_parts(fs, search_from);
        if !parts.is_empty() {
            let pack = origin.map(|o| o.fallback_name).unwrap_or_else(|| basename(rstrip_slashes(content_root)));
            return plan_conky_pack(home, fs, search_from, &parts, &dest_base, pack, replaceable);
        }
    }

    // wallpapers/fonts and everything else — copy the directory.
    let own_id = theme_id_from(fs, content_root);
    let theme_id = if own_id == basename(rstrip_slashes(content_root)) {
        name_of(content_root, own_id)
    } else {
        own_id
    };
    if is_shared_icon_set(&apply_kind, &theme_id) {
        return InstallOutcome::err(skipped_note(&theme_id, SkipReason::SharedIconSet));
    }
    let dest = join(&dest_base, &theme_id);
    // The owner rule (as with theme roots and `.rxn`): only what the shared log
    // recorded for this purchase is removed. A directory (file, link) with
    // the same name, installed by the person or by another purchase, is not
    // touched; there is nothing more to install — a refusal with a reason, the profile is intact.
    // The store does not put a `.rexon-owner` marker: its record of what is its own is the log,
    // and a marker file would land inside wallpapers, fonts and themes foreign to Rexon.
    // `None` — the old "a taken one is overwritten" behavior (the Python reference).
    let own = |p: &str| replaceable.is_none_or(|own| own.iter().any(|o| rstrip_slashes(o) == p));
    let mut notes: Vec<String> = Vec::new();
    if fs.is_dir(&dest) || fs.is_file(&dest) {
        if !own(&dest) {
            return InstallOutcome::err(skipped_note(&theme_id, SkipReason::TakenByOther));
        }
        steps.push(ExtStep::RemoveTree { path: dest.clone(), safe: false, ignore_errors: true });
    }
    steps.push(ExtStep::CopyTree {
        src: content_root.to_string(),
        dest: dest.clone(),
    });
    let mut paths = vec![dest.clone()];

    // A Conky widget draws with fonts the author put into the archive (`fonts/`,
    // `.fonts/` of Conky manager packs) and asked to install by hand. They are
    // installed into the widget's OWN folder inside the fonts directory: a font with the same name
    // belonging to the person or another widget is not overwritten, and "Remove"
    // removes exactly these files (the folder is recorded in the log next to the widget).
    if apply_kind == "conky" {
        let fonts = conky_archive_fonts(fs, content_root);
        let font_dir = format!("{}/.local/share/fonts/rexon-conky-{theme_id}", home.to_string_lossy());
        // A fonts folder with the same name from another purchase (or the person) is not
        // removed and not added to: the widget is installed without fonts, with
        // a note, and the foreign folder does not get into this purchase's log.
        let font_dir_taken = (fs.is_dir(&font_dir) || fs.is_file(&font_dir)) && !own(&font_dir);
        if !fonts.is_empty() && font_dir_taken {
            notes.push(skipped_note(basename(&font_dir), SkipReason::TakenByOther));
        } else if !fonts.is_empty() {
            if fs.is_dir(&font_dir) {
                steps.push(ExtStep::RemoveTree { path: font_dir.clone(), safe: false, ignore_errors: true });
            }
            steps.push(ExtStep::MakeDirs { path: font_dir.clone() });
            let mut taken: Vec<String> = Vec::new();
            for src in fonts {
                let name = basename(&src).to_string();
                if taken.contains(&name) {
                    continue;
                }
                steps.push(ExtStep::CopyFile { src, dest: join(&font_dir, &name) });
                taken.push(name);
            }
            steps.push(ExtStep::Run {
                argv: vec!["fc-cache".into(), "-f".into(), font_dir.clone()],
                timeout_secs: Some(60),
            });
            paths.push(font_dir);
        }
    }

    if apply_kind == "icons" || apply_kind == "cursor" {
        steps.push(ExtStep::Run {
            argv: vec![
                "gtk-update-icon-cache".into(),
                "-f".into(),
                "-t".into(),
                dest.clone(),
            ],
            timeout_secs: Some(60),
        });
    }

    let note = match &reclassified {
        Some(r) => format!(" ({})", trf("recognized as %1", &[r])),
        None => String::new(),
    };
    let mut message = trf("Installed: %1", &[&format!("{theme_id}{note}")]);
    if !notes.is_empty() {
        message = format!("{message}\n{}", notes.join("\n"));
    }
    InstallOutcome {
        ok: true,
        message,
        names: vec![theme_id],
        steps,
        reclassified,
        record: Some(RecordInfo {
            rexon_type: kind_to_rexon_type(&apply_kind).to_string(),
            apply_kind,
            paths,
        }),
    }
}

/// Directories of a Conky theme pack that hold widgets: this is how the conky
/// manager program lays out an archive (relative to the home folder), and some
/// store archives are built the same way (`.Conky/conkyx` + `.fonts/`).
pub const CONKY_PACK_DIRS: [&str; 4] = [".conky", ".Conky", ".config/conky", ".config/Conky"];

/// One part of a Conky theme pack: a widget folder or a bare config file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConkyPackPart {
    /// Path in the unpacked tree.
    pub src: String,
    /// Widget name (the folder in `~/.config/conky/`).
    pub name: String,
    /// A bare file — goes inside a folder with the same name.
    pub loose_file: bool,
}

/// A document, not a widget: `README`, `LICENSE`, `*.txt`…
fn is_pack_document(name: &str) -> bool {
    let l = name.to_ascii_lowercase();
    ["readme", "license", "licence", "copying", "changelog", "credits", "source"].iter().any(|p| l.starts_with(p))
        || l.ends_with(".txt")
        || l.ends_with(".nfo")
}

/// Parts of a Conky theme pack in the unpacked `root` (`root` itself and one wrapper
/// folder in it). Empty — this is not a pack but a single widget.
///
/// A fonts folder (`fonts`, only fonts inside) does not count as a widget: its
/// contents are installed as fonts. A bare file counts as a widget unless by name it is a
/// script, a picture, a font or a document (the section checks the content when
/// showing it: the install snapshot does not hold file texts).
pub fn conky_pack_parts(fs: &FsSnapshot, root: &str) -> Vec<ConkyPackPart> {
    let root = rstrip_slashes(root);
    let mut bases = vec![root.to_string()];
    let wrapper = find_content_root(fs, root);
    if wrapper != root {
        bases.push(wrapper);
    }
    let mut out: Vec<ConkyPackPart> = Vec::new();
    for base in &bases {
        for container in CONKY_PACK_DIRS {
            let dir = join(base, container);
            if !fs.is_dir(&dir) {
                continue;
            }
            let mut names: Vec<&String> = fs.scandir(&dir).iter().collect();
            names.sort();
            for name in names {
                if name.starts_with('.') {
                    continue;
                }
                let src = join(&dir, name);
                if fs.is_dir(&src) {
                    let only_fonts = {
                        let fonts = conky_archive_fonts(fs, &src);
                        !fonts.is_empty() && name.eq_ignore_ascii_case("fonts")
                    };
                    if only_fonts {
                        continue;
                    }
                    out.push(ConkyPackPart { src, name: name.clone(), loose_file: false });
                } else if fs.is_file(&src)
                    && !crate::conky_scan::skipped_name(name)
                    && !crate::constants::is_supported_font(name)
                    && !is_pack_document(name)
                {
                    out.push(ConkyPackPart { src, name: name.clone(), loose_file: true });
                }
            }
        }
    }
    out
}

/// Install plan for a Conky theme pack: each part is its own folder in
/// `dest_base`, all of the pack's fonts go into one folder
/// `~/.local/share/fonts/rexon-conky-<pack>/` (the item's "Remove" removes it).
/// A name taken by someone else's widget is not overwritten — the part is skipped with
/// an explanation, as with themes that have several roots.
fn plan_conky_pack(
    home: &Path,
    fs: &FsSnapshot,
    root: &str,
    parts: &[ConkyPackPart],
    dest_base: &str,
    pack: &str,
    replaceable: Option<&[String]>,
) -> InstallOutcome {
    let mut steps = vec![ExtStep::MakeDirs { path: dest_base.to_string() }];
    let mut names: Vec<String> = Vec::new();
    let mut paths: Vec<String> = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    for part in parts {
        if !is_single_path_component(&part.name) {
            notes.push(skipped_note(&part.name, SkipReason::TakenByOther));
            continue;
        }
        if names.contains(&part.name) {
            notes.push(skipped_note(&part.name, SkipReason::DuplicateInArchive));
            continue;
        }
        let dest = join(dest_base, &part.name);
        if fs.is_dir(&dest) || fs.is_file(&dest) {
            let own = replaceable.is_none_or(|own| own.iter().any(|p| rstrip_slashes(p) == dest));
            if !own {
                notes.push(skipped_note(&part.name, SkipReason::TakenByOther));
                continue;
            }
            steps.push(ExtStep::RemoveTree { path: dest.clone(), safe: false, ignore_errors: true });
        }
        if part.loose_file {
            steps.push(ExtStep::MakeDirs { path: dest.clone() });
            steps.push(ExtStep::CopyFile { src: part.src.clone(), dest: join(&dest, &part.name) });
        } else {
            steps.push(ExtStep::CopyTree { src: part.src.clone(), dest: dest.clone() });
        }
        names.push(part.name.clone());
        paths.push(dest);
    }
    if names.is_empty() {
        return InstallOutcome::err(notes.join("\n"));
    }
    let fonts = conky_archive_fonts(fs, root);
    let pack = pack.strip_suffix(".cmtp").unwrap_or(pack);
    let pack = if is_single_path_component(pack) { pack } else { names[0].as_str() };
    let font_dir = format!("{}/.local/share/fonts/rexon-conky-{pack}", home.to_string_lossy());
    let font_dir_present = fs.is_dir(&font_dir) || fs.is_file(&font_dir);
    let font_dir_own = replaceable.is_none_or(|own| own.iter().any(|p| rstrip_slashes(p) == font_dir));
    if !fonts.is_empty() && font_dir_present && !font_dir_own {
        // Someone else's fonts folder with the same name (another purchase, the person) is not
        // added to and not written into this purchase's log — previously fonts
        // landed in it, and "Remove" of this purchase removed it entirely.
        notes.push(skipped_note(basename(&font_dir), SkipReason::TakenByOther));
    } else if !fonts.is_empty() {
        if fs.is_dir(&font_dir) {
            steps.push(ExtStep::RemoveTree { path: font_dir.clone(), safe: false, ignore_errors: true });
        }
        steps.push(ExtStep::MakeDirs { path: font_dir.clone() });
        let mut taken: Vec<String> = Vec::new();
        for src in fonts {
            let name = basename(&src).to_string();
            if taken.contains(&name) {
                continue;
            }
            steps.push(ExtStep::CopyFile { src, dest: join(&font_dir, &name) });
            taken.push(name);
        }
        steps.push(ExtStep::Run {
            argv: vec!["fc-cache".into(), "-f".into(), font_dir.clone()],
            timeout_secs: Some(60),
        });
        paths.push(font_dir);
    }
    let mut message = trf("Installed: %1", &[&names.join(", ")]);
    if !notes.is_empty() {
        message = format!("{message}\n{}", notes.join("\n"));
    }
    InstallOutcome {
        ok: true,
        message,
        names,
        steps,
        reclassified: None,
        record: Some(RecordInfo {
            rexon_type: kind_to_rexon_type("conky").to_string(),
            apply_kind: "conky".to_string(),
            paths,
        }),
    }
}

/// Font files in an unpacked Conky widget (up to 5 levels deep, in
/// traversal order).
fn conky_archive_fonts(fs: &FsSnapshot, root: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![(root.to_string(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        let mut names: Vec<&String> = fs.scandir(&dir).iter().collect();
        names.sort();
        for n in names.into_iter().rev() {
            let p = join(&dir, n);
            if fs.is_dir(&p) {
                if depth < 5 {
                    stack.push((p, depth + 1));
                }
            } else if fs.is_file(&p) && crate::constants::is_supported_font(n) {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

/// Installing an item that the Store served as ONE bare file (not an archive).
///
/// There is nothing to unpack — the file goes into the kind's directory (and for Kvantum, where
/// a theme is looked up as `<name>/<name>.kvconfig`, into a subfolder named after the theme).
/// The "is this the case at all" gate — [`single_file_accepted`].
///
/// `archive_path` — the downloaded file: it is the copy source (explicit in the step,
/// rather than "empty source = what was downloaded", as it was before the installers
/// were merged).
pub fn plan_single_file(item: &ThemeItem, cat: &ThemeCategory, archive_path: &str) -> InstallOutcome {
    plan_single_file_with(item, cat, archive_path, &|_| false)
}

/// [`plan_single_file`] with the owner rule: `taken(path)` — the place of the
/// path being written (a file or a theme folder) holds something foreign: this purchase did not
/// install it, and it is not the same file byte for byte. Then there is nothing to install —
/// a refusal with a reason, nothing on disk is touched. The live installer calls
/// exactly this form.
pub fn plan_single_file_with(
    item: &ThemeItem,
    cat: &ThemeCategory,
    archive_path: &str,
    taken: &dyn Fn(&str) -> bool,
) -> InstallOutcome {
    let Some(rule) = kind_single_file_rule(&cat.apply_kind) else {
        return InstallOutcome::err(trf(
            "The “%1” type can't be installed from a single file",
            &[&cat.apply_kind],
        ));
    };
    let fname = single_file_fname(item, cat);
    let stem = basename(&py_splitext(&fname).0).to_string();

    // The directory that will have to be created, and the file's path in it.
    let dest_base = if rule.own_dir {
        join(&cat.install_dir, &stem)
    } else {
        cat.install_dir.clone()
    };
    let dest = join(&dest_base, &fname);
    // The log gets what uninstalling will remove: for an own folder — the whole
    // folder, otherwise the file itself. Leave the file here with own_dir — and after
    // uninstalling an empty theme folder would be left dangling in the kind's directory.
    let recorded = if rule.own_dir { dest_base.clone() } else { dest.clone() };
    let name = if rule.name_is_stem { stem } else { fname };
    if taken(&recorded) {
        return InstallOutcome::err(skipped_note(&name, SkipReason::TakenByOther));
    }

    let steps = vec![
        ExtStep::MakeDirs { path: dest_base },
        ExtStep::CopyFile { src: archive_path.to_string(), dest },
    ];
    InstallOutcome {
        ok: true,
        message: trf("Installed: %1", &[&name]),
        names: vec![name],
        steps,
        reclassified: None,
        record: Some(RecordInfo {
            // Fixed a bug of the original: the kind was written as a hard-coded "none" regardless of the
            // category, and a single picture for the lock background went into the
            // log as the same kind as a regular wallpaper.
            rexon_type: kind_to_rexon_type(&cat.apply_kind).to_string(),
            apply_kind: cat.apply_kind.clone(),
            paths: vec![recorded],
        }),
    }
}

/// File name for a single-file item: basename of download_name → basename of url →
/// the kind's fallback name; the extension the category expects is appended.
///
/// The extension matters not cosmetically: it is how KDE finds the content at all
/// (`plasma-apply-colorscheme` looks for `.colors`, fontconfig for font ones), and
/// a file without it is silently not picked up.
pub fn single_file_fname(item: &ThemeItem, cat: &ThemeCategory) -> String {
    let rule = kind_single_file_rule(&cat.apply_kind).unwrap_or(SingleFileRule {
        exts: IMAGE_EXTS,
        has_magic: true,
        own_dir: false,
        name_is_stem: false,
        fallback_stem: "wallpaper",
    });
    let mut fname = item.download_name.trim().to_string();
    if fname.is_empty() {
        let tail = item
            .download_url
            .split('?')
            .next()
            .unwrap_or("")
            .split('#')
            .next()
            .unwrap_or("");
        fname = basename(tail).trim().to_string();
    }
    fname = basename(&fname).to_string();
    if !rule.exts.contains(&file_ext(&fname).as_str()) {
        let id_part = if item.id.is_empty() { "kde" } else { &item.id };
        let fallback = format!("{}-{}", rule.fallback_stem, id_part);
        let stem_src = if fname.is_empty() { fallback.clone() } else { fname };
        let stem = py_splitext(&stem_src).0;
        let stem = if stem.is_empty() { fallback } else { stem };
        fname = format!("{stem}{}", rule.exts[0]);
    }
    fname
}

// ============================================================================
// System installation (SDDM / Plymouth) — plan and gates
// ============================================================================

/// The system installation environment — all external probes are injected.
pub struct RootEnv<'a> {
    /// Whether Plasma Login Manager (plasmalogin) is active — blocks SDDM themes.
    pub is_plasmalogin: bool,
    /// `shutil.which("pkexec")` != None.
    pub has_pkexec: bool,
    /// SYSTEM_HELPER path (for the error text).
    pub system_helper_path: &'a str,
    /// The helper is in place (the executable at [`crate::installer::system_helper_path`]).
    pub system_helper_exists: bool,
    /// SteamOS / immutable (/usr read-only).
    pub is_steamos_or_immutable: bool,
    /// Specifically SteamOS (Steam Deck and Steam Machine) — it has its own refusal
    /// reason for the person (`distro_compat::SteamOsBlock`).
    pub is_steamos: bool,
    /// Debian package family (→ argv "debian"), otherwise "arch".
    pub family_is_debian: bool,
}

/// System themes are installed by Rexon's system part, and it is missing — refusal before unpacking.
pub fn root_system_part_missing_message() -> String {
    // ⭐ 28.09.2026: the advice "reinstall into the system" and the promise "one password
    // per session" were removed — the system part ships in the package too, and the password lasts
    // minutes, not a session.
    tr("Login screen (SDDM) and boot splash (Plymouth) themes are installed by Rexon's \
        system component, and it isn't installed. Nothing was changed.\nUpdate or \
        reinstall Rexon — the system component comes with it.")
}

/// The `_install_root` gates BEFORE unpacking. `Some(outcome)` — an early refusal with
/// the exact text; `None` — may proceed to [`plan_root_install`].
pub fn root_preflight(env: &RootEnv, cat: &ThemeCategory) -> Option<InstallOutcome> {
    let kind = cat.apply_kind.as_str();
    // ⭐ 28.09.2026: a read-only system is checked FIRST. Previously
    // it came after "no Rexon system part", and on SteamOS, where the Steam
    // build has no such part and cannot have one, the person got the advice
    // "reinstall Rexon into the system" — impossible to follow.
    if env.is_steamos {
        let block = if kind == "plymouth" {
            crate::distro_compat::SteamOsBlock::BootSplash
        } else {
            crate::distro_compat::SteamOsBlock::LoginScreen
        };
        return Some(InstallOutcome::err(block.reason()));
    }
    if env.is_steamos_or_immutable {
        return Some(InstallOutcome::err(trf(
            "“%1”: system themes can't be installed on SteamOS or an immutable \
             system (/usr is read-only).",
            &[&cat.name],
        )));
    }
    if kind == "sddm" && env.is_plasmalogin {
        return Some(InstallOutcome::err(tr(
            // TRANSLATORS: arrows ← → point the way they should in your language: in right-to-left languages swap them.
            "Plasma Login Manager (Plasma 6.6+) is active, and it doesn't support \
             SDDM QML themes. Change the login screen look in “System Settings → \
             Login Screen”.",
        )));
    }
    // ⭐ Fix of 15.09.2026: previously "pkexec not found — … Install the polkit
    // package." — a service word and advice that did not work on Debian and
    // Ubuntu. The text is shared with fork restore, Plymouth and SDDM.
    if !env.has_pkexec {
        return Some(InstallOutcome::err(crate::system_plan::pkexec_missing_message()));
    }
    // ⭐ Fix of 15.09.2026: previously "The Rexon system helper is not installed
    // (<path>)…" — a service word and a file path in text meant for the buyer.
    // The path is still needed, but only in the plan's argv below.
    if !env.system_helper_exists {
        return Some(InstallOutcome::err(root_system_part_missing_message()));
    }
    None
}

/// A successful system install plan (after unpacking and the gates).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RootPlan {
    /// `pkexec <helper> <kind> <theme_id> <content_root> <family>`.
    pub argv: Vec<String>,
    pub timeout_secs: u64,
    pub theme_id: String,
    pub dest: String,
    /// Message on success (returncode == 0).
    pub success_message: String,
    pub rexon_type: String,
}

/// Port of `_install_root` after unpacking: theme name + `pkexec` argv.
/// `Err(outcome)` — Plymouth without `<name>.plymouth`. `content_root` — the snapshot is in `fs`.
/// (`InstallOutcome` is boxed — clippy::result_large_err.)
pub fn plan_root_install(
    env: &RootEnv,
    fs: &FsSnapshot,
    cat: &ThemeCategory,
    content_root: &str,
) -> Result<RootPlan, Box<InstallOutcome>> {
    plan_root_install_with(env, fs, cat, content_root, &|_| false)
}

/// [`plan_root_install`] with the owner rule: `taken(path)` — the place of the
/// system theme directory (`/usr/share/<kind>/themes/<name>`) holds something foreign:
/// this purchase did not install it (a theme from a system package, the person, another
/// purchase). The helper copies over it (`cp --remove-destination`), so the
/// decision is made here, before the password window: a refusal with a reason, nothing
/// touched. The live installer calls exactly this form.
pub fn plan_root_install_with(
    env: &RootEnv,
    fs: &FsSnapshot,
    cat: &ThemeCategory,
    content_root: &str,
    taken: &dyn Fn(&str) -> bool,
) -> Result<RootPlan, Box<InstallOutcome>> {
    let kind = cat.apply_kind.as_str();
    let theme_id = if kind == "plymouth" {
        let id = plymouth_theme_name(fs, content_root);
        if id.is_empty() {
            return Err(Box::new(InstallOutcome::err(tr(
                "No <name>.plymouth file in the archive — this isn't a Plymouth theme.",
            ))));
        }
        id
    } else {
        // SDDM: metadata.desktop/Main.qml sanity — a warning only,
        // does not affect the plan.
        theme_id_from(fs, content_root)
    };

    let dest = join(&cat.install_dir, &theme_id);
    if taken(&dest) {
        return Err(Box::new(InstallOutcome::err(skipped_note(&theme_id, SkipReason::TakenByOther))));
    }
    let family_arg = if env.family_is_debian { "debian" } else { "arch" };
    let applied_note = if kind == "sddm" {
        tr("Takes effect the next time you log in (SDDM screen).")
    } else {
        tr("Takes effect after a REBOOT (initramfs was rebuilt).")
    };

    Ok(RootPlan {
        argv: [
            vec!["pkexec".to_string(), env.system_helper_path.to_string()],
            // Language of the helper's messages (decision of 30.09.2026), as in ext_manager.
            crate::system_l10n::lang_args(env.system_helper_path),
            vec![
                kind.to_string(),
                theme_id.clone(),
                content_root.to_string(),
                family_arg.to_string(),
            ],
        ]
        .concat(),
        timeout_secs: 900,
        success_message: trf("✅ Installed: %1. %2", &[&theme_id, &applied_note]),
        rexon_type: kind_to_rexon_type(kind).to_string(),
        theme_id,
        dest,
    })
}

/// polkit cancellation message (returncode 126/127) — port of the `_install_root` line.
///
/// ⭐ Fix of 15.09.2026: previously it was "Installation cancelled (pkexec/polkit)" —
/// service words in parentheses. The reason is now named in the same words as
/// the other password refusals (`system_plan::pkexec_denied_message`).
pub fn root_cancelled_message() -> String {
    tr("Installation canceled: the administrator password was not confirmed.")
}

/// Install error message (returncode != 0) — port of the `_install_root` line:
/// the last ≤3 lines of stderr joined with " / ", otherwise "see the log".
pub fn root_error_message(returncode: i32, stderr: &str) -> String {
    let err = stderr.trim();
    let tail: Vec<&str> = if err.is_empty() {
        Vec::new()
    } else {
        let lines: Vec<&str> = err.lines().collect();
        lines[lines.len().saturating_sub(3)..].to_vec()
    };
    let detail = if tail.is_empty() {
        tr("see the log")
    } else {
        tail.join(" / ")
    };
    trf("Installation error (code %1): %2", &[&returncode, &detail])
}

/// "Apply" for a system theme — decisions and the command line; the executor and
/// the directory probe are outside (the checks substitute their own "pkexec").
///
/// `theme_dir_exists` — whether the directory `/usr/share/<kind>/themes/<theme>` exists:
/// only what is already installed can be selected, and the person learns this before the password
/// window. `run` — a run with a timeout (for Plymouth, rebuilding the kernel image
/// takes minutes).
pub fn system_theme_apply_with(
    env: &RootEnv,
    cat: &ThemeCategory,
    theme: &str,
    lang_args: &[String],
    theme_dir_exists: &dyn Fn(&str) -> bool,
    run: &dyn Fn(&[String], u64) -> crate::utils::RunReport,
) -> (bool, String) {
    if let Some(pre) = root_preflight(env, cat) {
        return (false, pre.message);
    }
    let kind = cat.apply_kind.as_str();
    if !crate::ext_manager::is_safe_sys_name(theme) || !theme_dir_exists(&join(&cat.install_dir, theme)) {
        return (
            false,
            trf("“%1” isn't installed in the system — install it first.", &[&theme]),
        );
    }
    let family = if env.family_is_debian { "debian" } else { "arch" };
    let argv = crate::de_apply::system_apply_argv(env.system_helper_path, lang_args, kind, theme, family);
    let report = run(&argv, 900);
    if report.outcome == RunOutcome::Timeout {
        return (false, tr("Applying timed out (rebuilding initramfs?)"));
    }
    let Some(rc) = report.returncode else {
        return (false, trf("Couldn't apply: %1", &[&report.error]));
    };
    if rc == 126 || rc == 127 {
        return (false, tr("Not applied: the administrator password was not confirmed."));
    }
    if rc != 0 {
        let err = crate::utils::decode_text(&report.stderr);
        let err = err.trim();
        let tail: Vec<&str> = err.lines().rev().take(3).collect::<Vec<_>>().into_iter().rev().collect();
        let detail = if tail.is_empty() { tr("see the log") } else { tail.join(" / ") };
        return (false, trf("Couldn't apply (code %1): %2", &[&rc, &detail]));
    }
    let note = if kind == "sddm" {
        tr("Takes effect the next time you log in (SDDM screen).")
    } else {
        tr("Takes effect after a REBOOT (initramfs was rebuilt).")
    };
    (true, format!("{}. {note}", trf("✅ Applied: %1", &[&theme])))
}

/// Result of "Apply" for an SDDM theme when the login screen is currently shown by another
/// manager (LightDM, GDM, greetd…): "on the next login" would be untrue —
/// the theme is selected for SDDM and takes effect only if it becomes the login screen.
/// `None` — SDDM is showing it or the manager is unknown (the old text).
/// Plasma Login Manager does not get here: it has its own section and an early refusal
/// ([`root_preflight`]).
pub fn sddm_elsewhere_note(login_unit: &str) -> Option<String> {
    let unit = login_unit.trim();
    let label = crate::categories::login_manager_label(unit);
    (unit != "sddm" && !label.is_empty()).then(|| {
        trf(
            "Right now it's shown by %1, and the themes here are for SDDM: \
             they only take effect if SDDM becomes the login screen.",
            &[&label],
        )
    })
}

/// "Remove" for a system theme — decisions and the command line; the executor, the list
/// of currently selected themes (`active`) and the run are outside.
///
/// The currently selected theme is not removed: the login screen would be left without a theme,
/// and the splash without files at the next kernel image build. The person learns
/// this before the password window; the helper running as root checks the same again and additionally
/// does not touch directories owned by system packages.
pub fn system_theme_remove_with(
    env: &RootEnv,
    cat: &ThemeCategory,
    theme: &str,
    lang_args: &[String],
    active: &[String],
    run: &dyn Fn(&[String], u64) -> crate::utils::RunReport,
) -> (bool, String) {
    let kind = cat.apply_kind.as_str();
    if !crate::ext_manager::is_safe_sys_name(theme) {
        return (false, trf("Couldn't remove “%1”: unexpected theme name.", &[&theme]));
    }
    if active.iter().any(|a| a == theme) {
        let text = if kind == "plymouth" {
            trf(
                "“%1” is the current boot splash theme. Apply another one first, then \
                 remove this one.",
                &[&theme],
            )
        } else {
            trf(
                "“%1” is the current login screen theme. Apply another one first, then \
                 remove this one.",
                &[&theme],
            )
        };
        return (false, text);
    }
    if let Some(pre) = root_preflight(env, cat) {
        return (false, pre.message);
    }
    let family = if env.family_is_debian { "debian" } else { "arch" };
    let argv = crate::de_apply::system_remove_argv(env.system_helper_path, lang_args, kind, theme, family);
    let report = run(&argv, crate::installer::SYSTEM_INSTALL_TIMEOUT_SECS);
    if report.outcome == RunOutcome::Timeout {
        return (false, trf("Couldn't remove “%1”: it took too long.", &[&theme]));
    }
    let Some(rc) = report.returncode else {
        return (false, trf("Couldn't remove: %1", &[&report.error]));
    };
    if rc == 126 || rc == 127 {
        return (false, tr("Not removed: the administrator password was not confirmed."));
    }
    if rc != 0 {
        let err = crate::utils::decode_text(&report.stderr);
        let err = err.trim();
        let tail: Vec<&str> = err.lines().rev().take(3).collect::<Vec<_>>().into_iter().rev().collect();
        let detail = if tail.is_empty() { tr("see the log") } else { tail.join(" / ") };
        return (false, trf("Couldn't remove (code %1): %2", &[&rc, &detail]));
    }
    (true, String::new())
}

// ============================================================================
// Applying (apply)
// ============================================================================

/// The apply action for a category's apply_kind — port of the `apply` dispatcher.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyAction {
    /// Delegation to ThemeManager (themes.rs plans) by apply_kind.
    KdeGlobal,
    KdePlasma,
    KdeColors,
    Cursor,
    Icons,
    Kvantum,
    Aurorae,
    Gtk,
    WidgetStyle,
    Splash,
    Lockscreen,
    LockWallpaper,
    /// A single picture → desktop background (real wallpapers).
    WallpaperBackground,
    /// A wallpaper pack (a directory of pictures) → desktop background: the picture
    /// is picked from the pack deterministically ([`pick_pack_image`]).
    WallpaperPack,
    /// A sound theme → the theme name into the current desktop's sound setting.
    SoundTheme,
    /// A desktop section (xfwm4/Marco frames, shell themes, the window
    /// switcher, extensions, Conky): "Apply" / "Enable" / "Start" —
    /// [`crate::de_apply::press`] per the customization matrix.
    DeSection,
    /// The login screen (SDDM) or the boot splash (Plymouth): selecting an already
    /// installed theme via the system helper (mode `apply-<kind>`).
    SystemTheme,
    /// No apply needed (fonts/widgets/wallpaper plugins/archive plugins).
    Noop,
}

/// Choosing the apply action — port of the `if kind == …` chain in `apply`.
/// `cat` — the category by `item.category_id` (None → apply_kind "none").
pub fn apply_action_for(cat: Option<&ThemeCategory>, theme_name: &str) -> ApplyAction {
    let kind = cat.map(|c| c.apply_kind.as_str()).unwrap_or("none");
    match kind {
        "global" => ApplyAction::KdeGlobal,
        "plasma" => ApplyAction::KdePlasma,
        "colors" => ApplyAction::KdeColors,
        "cursor" => ApplyAction::Cursor,
        "icons" => ApplyAction::Icons,
        "kvantum" => ApplyAction::Kvantum,
        "aurorae" => ApplyAction::Aurorae,
        "gtk" => ApplyAction::Gtk,
        "widgetstyle" => ApplyAction::WidgetStyle,
        "splash" => ApplyAction::Splash,
        "lockscreen" => ApplyAction::Lockscreen,
        "lockwallpaper" => ApplyAction::LockWallpaper,
        "sounds" => ApplyAction::SoundTheme,
        // Wallpapers: a single picture goes straight to the background, a directory is a pack (we
        // pick the picture from it ourselves). Wallpaper plugins also arrive as a single
        // picture, but their directory is a kpackage, nothing to apply.
        _ if is_wallpaper_image(cat, theme_name) => ApplyAction::WallpaperBackground,
        "wallpapers" => ApplyAction::WallpaperPack,
        // The login screen and the boot splash: installation now only puts the
        // theme into the system directory; "Apply" selects it (04.10.2026).
        "sddm" | "plymouth" => ApplyAction::SystemTheme,
        // Desktop sections — per the matrix: a section has a button if its type
        // is enabled by a desktop key, a list or a launch (`de_apply::mode`).
        k if crate::categories::category_kind(k)
            .and_then(crate::de_apply::mode)
            .is_some() =>
        {
            ApplyAction::DeSection
        }
        _ => ApplyAction::Noop,
    }
}

/// True if apply should set the picture as the desktop background — port of
/// `_is_wallpaper_image`.
pub fn is_wallpaper_image(cat: Option<&ThemeCategory>, theme_name: &str) -> bool {
    let Some(cat) = cat else { return false };
    // The wallpapers section (both the regular one and plugins): the same place where the Store serves
    // single pictures. The lock background is not included — it has its own
    // action (`LockWallpaper`).
    if !matches!(cat.apply_kind.as_str(), "wallpapers" | "wallpaper_plugins") {
        return false;
    }
    IMAGE_EXTS.contains(&file_ext(theme_name).as_str())
}

/// Whether the "Apply" button of the "Appearance" tab turns on a `.rxn` type — i.e.
/// whether it has a section whose [`apply_action_for`] does something.
///
/// ⛔ Installation ONLY puts such a type on disk, and the person makes the choice with
/// the button (defect of 03.10.2026: installing a cursor theme from `.rxn` silently
/// wrote `~/.icons/default`, and the cursor of X11 programs changed before
/// "Apply"; colors, the Plasma style, the global theme, Kvantum, window frames, the splash and
/// the lock screen were applied silently the same way). The answer is derived
/// from the same sections and the same dispatcher as the button itself — there is no second
/// list (rule #12): once a section gets "Apply", installing its
/// type stops turning the theme on by itself.
pub fn applied_by_button(rxn_kind: &str) -> bool {
    crate::categories::categories(Path::new("/")).iter().any(|cat| {
        crate::categories::category_kind(&cat.key) == Some(rxn_kind)
            && apply_action_for(Some(cat), "") != ApplyAction::Noop
    })
}

/// Message of a delegating apply_kind by the `ok` result — port of the `apply` lines
/// (for KdeGlobal/KdePlasma/KdeColors/Cursor/Icons/Kvantum/Aurorae/Gtk/
/// Splash/Lockscreen/Noop). LockWallpaper/WallpaperBackground have their own
/// messages (see the corresponding functions), not covered here.
pub fn apply_message(action: &ApplyAction, ok: bool) -> String {
    match action {
        ApplyAction::KdeGlobal => {
            if ok { tr("Global Theme applied") } else { tr("Couldn't apply the Global Theme") }
        }
        ApplyAction::KdePlasma => {
            if ok { tr("Plasma Style applied") } else { tr("Couldn't apply the Plasma Style") }
        }
        ApplyAction::KdeColors => {
            if ok { tr("Color scheme applied") } else { tr("Couldn't apply the scheme") }
        }
        ApplyAction::Cursor => {
            if ok {
                tr("Cursor applied")
            } else {
                tr("Cursor not applied (the theme may be unsafe)")
            }
        }
        ApplyAction::Icons => {
            if ok { tr("Icons applied") } else { tr("Icons not applied (invalid theme)") }
        }
        ApplyAction::Kvantum => {
            if ok { tr("Kvantum applied") } else { tr("Couldn't apply Kvantum") }
        }
        ApplyAction::Aurorae => {
            if ok { tr("Window decoration applied") } else { tr("Couldn't apply Aurorae") }
        }
        ApplyAction::Gtk => {
            if ok { tr("GTK theme applied") } else { tr("Couldn't apply the GTK theme") }
        }
        ApplyAction::WidgetStyle => {
            if ok {
                tr("Application style applied")
            } else {
                tr("Couldn't apply the application style")
            }
        }
        ApplyAction::Splash => {
            if ok {
                tr("Splash screen selected — takes effect the next time you log in")
            } else {
                tr("Couldn't apply the splash screen")
            }
        }
        ApplyAction::Lockscreen => {
            if ok {
                tr("Lock screen selected — takes effect the next time the screen locks")
            } else {
                tr("Couldn't apply the lock screen")
            }
        }
        ApplyAction::SoundTheme => {
            if ok {
                tr("Sound theme applied")
            } else {
                tr("Couldn't apply the sound theme")
            }
        }
        ApplyAction::Noop => tr("Installed (this type doesn't need to be applied)"),
        ApplyAction::LockWallpaper
        | ApplyAction::WallpaperBackground
        | ApplyAction::WallpaperPack
        | ApplyAction::DeSection
        | ApplyAction::SystemTheme => String::new(),
    }
}

/// Message about the gtk sanitizer's work after apply — port of the line
/// `f"🛡 GTK sanitizer: fixed {n} links"` (n = number of fixes, >0).
pub fn gtk_sanitizer_message(changes: usize) -> String {
    trf("🛡️ GTK sanitizer: fixed %1 links", &[&changes])
}

// ── apply: picking a picture from a wallpaper pack ─────────────────────────────

/// The standard pictures subfolder of a KDE wallpaper pack (KPackage).
const PACK_IMAGES_SUBDIR: &str = "contents/images";

/// All pictures of a wallpaper pack in snapshot order.
///
/// If the pack has the standard `contents/images` subfolder (the KPackage layout
/// used by KDE Store wallpapers) — only it is taken: next to it lie
/// logos, previews and placeholder pictures, and they must not be mixed into the choice.
/// Otherwise the whole pack tree is traversed. Our own preview, which Rexon puts next to
/// the theme when installing from the Store, is excluded from the candidates — otherwise the background
/// would become a screenshot of the card.
pub fn pack_images(fs: &FsSnapshot, pack_dir: &str) -> Vec<String> {
    let pack_dir = rstrip_slashes(pack_dir);
    let standard = join(pack_dir, PACK_IMAGES_SUBDIR);
    let root = if fs.is_dir(&standard) { standard } else { pack_dir.to_string() };
    let mut out = Vec::new();
    collect_images(fs, &root, &mut out);
    out
}

/// Recursive picture collection (in directory snapshot order, deeper after the files
/// of the same level — the traversal order does not affect the choice, it is fully
/// determined by the sort key of [`pick_pack_image`]).
fn collect_images(fs: &FsSnapshot, dir: &str, out: &mut Vec<String>) {
    let mut subdirs = Vec::new();
    for name in fs.scandir(dir) {
        let path = join(dir, name);
        if fs.is_dir(&path) {
            subdirs.push(path);
            continue;
        }
        if name == crate::installed_scan::REXON_PREVIEW_NAME {
            continue;
        }
        if IMAGE_EXTS.contains(&file_ext(name).as_str()) {
            out.push(path);
        }
    }
    for d in subdirs {
        collect_images(fs, &d, out);
    }
}

/// Resolution from a file name like `1920x1080.png` (the standard picture name in
/// KDE packs). `None` — the name has no resolution.
fn size_from_name(path: &str) -> Option<(u64, u64)> {
    let stem = py_splitext(basename(path)).0;
    let stem = basename(&stem).to_string();
    let (w, h) = stem.split_once(['x', 'X'])?;
    if w.is_empty() || h.is_empty() {
        return None;
    }
    let w: u64 = w.parse().ok()?;
    let h: u64 = h.parse().ok()?;
    if w == 0 || h == 0 {
        return None;
    }
    Some((w, h))
}

/// "Natural" sort key of a name: numbers compare as numbers
/// (`10.png` after `9.png`, not before it).
fn natural_key(name: &str) -> Vec<(u64, String)> {
    let mut out = Vec::new();
    let mut chars = name.chars().peekable();
    let mut text = String::new();
    while let Some(&c) = chars.peek() {
        if c.is_ascii_digit() {
            let mut num = String::new();
            while let Some(&d) = chars.peek() {
                if !d.is_ascii_digit() {
                    break;
                }
                num.push(d);
                chars.next();
            }
            // Numbers longer than u64 are taken by their tail — picture names never have them,
            // but crashing on such a name is not allowed.
            out.push((num.parse::<u64>().unwrap_or(u64::MAX), std::mem::take(&mut text)));
        } else {
            text.push(c);
            chars.next();
        }
    }
    if !text.is_empty() {
        out.push((0, text));
    }
    out
}

/// Which pack picture to set as the background — a DETERMINISTIC rule.
///
/// Key order (the first one decides):
///   1. **whether the name has a resolution** — `WxH.ext` files (the standard layout
///      of KDE packs) come before ones without a size in the name: for them we know for sure
///      whether they fit the screen;
///   2. **aspect ratio difference from the screen** — a picture with the wrong aspect ratio
///      is either cropped or letterboxed, and that is immediately visible;
///   3. **area difference from the screen** — of two with the same ratio we take
///      the one closer to the screen resolution: scaling in either direction
///      spoils the picture, and the "closest" one is spoiled the least;
///   4. **natural sort of the name** — removes the remaining ambiguity,
///      so that the same pack on the same machine always gives the same
///      picture.
///
/// A zero-size screen (nobody to ask) — ratio and area
/// cannot be compared, only the name sort remains.
pub fn pick_pack_image(files: &[String], screen_w: u64, screen_h: u64) -> Option<String> {
    let known_screen = screen_w > 0 && screen_h > 0;
    let screen_ar = if known_screen {
        screen_w as f64 / screen_h as f64
    } else {
        0.0
    };
    let screen_area = (screen_w * screen_h) as f64;
    let mut best: Option<(PickKey, &String)> = None;
    for f in files {
        let size = size_from_name(f);
        let (ar_diff, area_diff) = match (size, known_screen) {
            (Some((w, h)), true) => (
                (w as f64 / h as f64 - screen_ar).abs(),
                ((w * h) as f64 - screen_area).abs(),
            ),
            _ => (0.0, 0.0),
        };
        let key = PickKey {
            no_size: size.is_none(),
            ar_diff,
            area_diff,
            name: natural_key(basename(f)),
        };
        if best.as_ref().is_none_or(|(b, _)| key.better_than(b)) {
            best = Some((key, f));
        }
    }
    best.map(|(_, f)| f.clone())
}

/// Pack picture selection key: the smaller in field order, the better.
struct PickKey {
    no_size: bool,
    ar_diff: f64,
    area_diff: f64,
    name: Vec<(u64, String)>,
}

impl PickKey {
    fn better_than(&self, other: &PickKey) -> bool {
        let a = (self.no_size, self.ar_diff, self.area_diff);
        let b = (other.no_size, other.ar_diff, other.area_diff);
        a < b || (a == b && self.name < other.name)
    }
}

/// Message when the pack has not a single picture: nothing to set, but the
/// installation happened — the card honestly stays at "✓ Installed".
pub fn pack_no_images_message() -> String {
    tr("Installed. This pack has no images — nothing to apply.")
}

// ── apply: config write plans (kwriteconfig6 / dbus) ──────────────────────────

/// One command of an apply plan (argv + an optional timeout).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CfgCmd {
    pub argv: Vec<String>,
}

fn cfg(parts: &[&str]) -> CfgCmd {
    CfgCmd { argv: parts.iter().map(|s| (*s).to_string()).collect() }
}

/// Port of `_apply_widget_style` — the plan form for KDE (`kwriteconfig6` +
/// a dbus notification on success). Kept for the bridge and the Python reference;
/// the executor goes through [`crate::themes::apply_qt_style_plan`], which
/// picks the write by desktop, and for KDE gives exactly these same two commands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WidgetStylePlan {
    pub primary: CfgCmd,
    pub notify_if_ok: CfgCmd,
}

pub fn widget_style_plan(name: &str) -> WidgetStylePlan {
    let plan = crate::themes::apply_qt_style_plan(
        name,
        &crate::themes::KvantumEnv { de_id: "kde", home: "", xdg_config_home: "", platform_theme: "", icon_theme: String::new() },
    );
    let argv = |i: usize| match &plan.steps[i] {
        ApplyStep::Primary(c) | ApplyStep::IfOk(c) => CfgCmd { argv: c.argv.clone() },
        other => unreachable!("KDE-путь стиля Qt — только команды, а не {other:?}"),
    };
    WidgetStylePlan { primary: argv(0), notify_if_ok: argv(1) }
}

/// Port of `_apply_lnf_component`: writing the theme name into ksplashrc[KSplash]Theme
/// (`component == "splash"`) or kscreenlockerrc[Greeter]Theme (otherwise).
pub fn lnf_component_plan(component: &str, name: &str) -> CfgCmd {
    if component == "splash" {
        cfg(&[
            "kwriteconfig6", "--file", "ksplashrc", "--group", "KSplash",
            "--key", "Theme", name,
        ])
    } else {
        cfg(&[
            "kwriteconfig6", "--file", "kscreenlockerrc", "--group", "Greeter",
            "--key", "Theme", name,
        ])
    }
}

// ── KDE splash: "None" and "Preview" ──────────────────────────────────────────
//
// Checked against the Plasma 6.8 sources (plasma-desktop `kcms/ksplash/kcm.cpp`,
// plasma-workspace `startkde/startplasma.cpp`, `ksplash/ksplashqml`):
//   * KDE's "None" tile is a theme named `None`; on save the module writes
//     BOTH keys: `[KSplash] Theme=None` and `Engine=none` (any other theme gets
//     `Engine=KSplashQML`). At login startplasma starts ksplashqml only
//     when `Engine == KSplashQML` (no key — also KSplashQML);
//   * "Preview" is `ksplashqml <theme> --test`: a full-screen splash
//     goes through its stages and closes by itself, on a click or Esc (the module
//     does not pass the `--window` flag).

/// The "theme" name of the "None" tile — the same the KDE module writes.
pub const SPLASH_NONE: &str = "None";
/// `Engine` of a disabled splash — as in the KDE module.
pub const SPLASH_ENGINE_NONE: &str = "none";

/// The `[KSplash] Engine` value for the selected theme.
pub fn splash_engine_for(theme: &str) -> &'static str {
    if theme == SPLASH_NONE {
        SPLASH_ENGINE_NONE
    } else {
        crate::session_splash::KSPLASH_ENGINE
    }
}

/// Writing `[KSplash] Engine` for the selected theme. Without it, after "None" no
/// other splash would come back: startplasma looks at `Engine`.
pub fn splash_engine_plan(theme: &str) -> CfgCmd {
    cfg(&[
        "kwriteconfig6", "--file", "ksplashrc", "--group", "KSplash",
        "--key", "Engine", splash_engine_for(theme),
    ])
}

/// Which tile of the "Splash" section counts as current according to `ksplashrc`.
///
/// `Engine` is set and is not `KSplashQML` — the splash does not run at login
/// at all (startplasma decides so), the current one is "None", whatever is in
/// `Theme`. Otherwise — the `Theme` value as is.
pub fn splash_active_value(theme: Option<String>, engine: Option<&str>) -> Option<String> {
    match engine.map(str::trim) {
        Some(e) if !e.is_empty() && e != crate::session_splash::KSPLASH_ENGINE => {
            Some(SPLASH_NONE.to_string())
        }
        _ => theme,
    }
}

/// The "Preview splash" command — the same as the KDE module's.
/// `None` — nothing to show (the "None" tile, an empty or odd name).
pub fn splash_demo_argv(theme: &str) -> Option<Vec<String>> {
    let theme = theme.trim();
    if theme.is_empty() || theme == SPLASH_NONE || theme.starts_with('-') || theme.contains('/') {
        return None;
    }
    Some(vec![
        crate::session_splash::KSPLASH_PROCESS.to_string(),
        theme.to_string(),
        "--test".to_string(),
    ])
}

/// Label of the splash preview button on a tile.
pub fn splash_demo_label() -> String {
    // TRANSLATORS: button on a KDE splash screen tile: shows the splash screen full screen for a few seconds.
    tr("Show Splash Screen")
}

/// Tooltip of the splash preview button.
pub fn splash_demo_tooltip() -> String {
    tr("Shows the splash screen full screen for a few seconds. Click or press Esc to close it.")
}

/// Preview failed; `detail` — what the program said (may be empty).
pub fn splash_demo_failed(detail: &str) -> String {
    let head = tr("Failed to show the splash screen preview.");
    // The program can be verbose: the person gets the last lines, where the cause is.
    let lines: Vec<&str> = detail.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    if lines.is_empty() {
        head
    } else {
        format!("{head}\n{}", lines[lines.len().saturating_sub(5)..].join("\n"))
    }
}

/// Nothing to preview with: no `ksplashqml` (the KDE splash comes with Plasma).
pub fn splash_demo_missing() -> String {
    tr("Can't show the splash screen: ksplashqml (part of Plasma) isn't installed.")
}

/// Title of the "None" tile.
pub fn splash_none_name() -> String {
    // TRANSLATORS: tile in the KDE splash screen section that turns the splash screen off.
    trc("splash screen", "None")
}

/// Description of the "None" tile — as in the KDE module.
pub fn splash_none_description() -> String {
    tr("No splash screen will be shown")
}

/// Result of applying the "None" tile.
pub fn splash_off_message() -> String {
    tr("Splash screen turned off — takes effect the next time you log in")
}

/// Port of `_apply_aurorae`: the primary kwriteconfig6 theme + (only when ok) two
/// attempts to reconfigure KWin (qdbus / qdbus6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AuroraePlan {
    pub primary: CfgCmd,
    pub reconfigure_if_ok: Vec<CfgCmd>,
}

pub fn aurorae_plan(name: &str) -> AuroraePlan {
    let theme_val = format!("__aurorae__svg__{name}");
    AuroraePlan {
        primary: CfgCmd {
            argv: vec![
                "kwriteconfig6".into(), "--file".into(), "kwinrc".into(),
                "--group".into(), "org.kde.kdecoration2".into(), "--key".into(),
                "theme".into(), theme_val,
            ],
        },
        reconfigure_if_ok: vec![
            cfg(&["qdbus", "org.kde.KWin", "/KWin", "reconfigure"]),
            cfg(&["qdbus6", "org.kde.KWin", "/KWin", "reconfigure"]),
        ],
    }
}

/// Port of `_apply_lock_wallpaper`: lock screen wallpaper.
/// `install_dir` = `cat.install_dir`. `Err(msg)` — the file is missing (a single
/// picture is required). `Ok(plan)` — writing the picture and the accompanying writes.
///
/// KDE — `kscreenlockerrc` (Image, then PreviewImage and WallpaperPlugin);
/// Deepin — only in a Treeland session, via the Control Center
/// ([`lock_wallpaper_plan_in`]). Where else this mode works —
/// [`crate::categories::lock_wallpaper_supported`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LockWallpaperPlan {
    /// A file:// URI with percent-encoding of non-ASCII (`Path.as_uri`).
    pub uri: String,
    /// Writing the picture (its returncode determines ok/the message).
    pub image: CfgCmd,
    /// Best-effort writes after it (KDE: PreviewImage and
    /// WallpaperPlugin=org.kde.image; Deepin — none).
    pub extra: Vec<CfgCmd>,
    pub success_message: String,
    pub failure_message: String,
}

/// `path_is_file` — whether the file `install_dir/theme_name` exists (probe outside).
/// `de` — the desktop identifier (`deepin`, `kde`…); everything except Deepin is
/// the KDE path, as before.
pub fn lock_wallpaper_plan(
    de: &str,
    install_dir: &str,
    theme_name: &str,
    path_is_file: bool,
) -> Result<LockWallpaperPlan, String> {
    lock_wallpaper_plan_in(de, false, install_dir, theme_name, path_is_file)
}

/// The same, taking the session into account: `treeland` — whether a Treeland session is running. In it, the
/// Deepin lock screen is drawn by Treeland, and the Control Center hands it the wallpaper
/// ([`crate::dde::control_center_wallpaper_argv`]); Rexon does not support other Deepin
/// sessions — a refusal ([`crate::dde::not_treeland_message`]).
pub fn lock_wallpaper_plan_in(
    de: &str,
    treeland: bool,
    install_dir: &str,
    theme_name: &str,
    path_is_file: bool,
) -> Result<LockWallpaperPlan, String> {
    let path = join(install_dir, theme_name);
    if !path_is_file {
        return Err(trf(
            "Wallpaper file not found: %1. The lock screen background supports a \
             single picture only.",
            &[&path],
        ));
    }
    let uri = path_as_uri(&path);
    let success_message =
        tr("Lock screen wallpaper applied — you'll see it the next time the screen locks.");
    let failure_message = tr("Couldn't write the lock screen wallpaper");
    if crate::de_customization::de_from_id(de) == Some(crate::de_customization::De::Deepin) && treeland {
        return Ok(LockWallpaperPlan {
            image: CfgCmd { argv: crate::dde::control_center_wallpaper_argv(&path, true) },
            extra: Vec::new(),
            success_message: tr("Lock screen wallpaper handed to Deepin's Control Center — it sets it on the screen selected there."),
            failure_message: crate::dde::control_center_failed_message(),
            uri,
        });
    }
    if crate::de_customization::de_from_id(de) == Some(crate::de_customization::De::Deepin) {
        return Err(crate::dde::not_treeland_message());
    }
    let w = |key: &str, value: &str| CfgCmd {
        argv: vec![
            "kwriteconfig6".into(), "--file".into(), "kscreenlockerrc".into(),
            "--group".into(), "Greeter".into(), "--group".into(),
            "Wallpaper".into(), "--group".into(), "org.kde.image".into(),
            "--group".into(), "General".into(), "--key".into(), key.into(),
            value.into(),
        ],
    };
    Ok(LockWallpaperPlan {
        image: w("Image", &uri),
        extra: vec![
            w("PreviewImage", &uri),
            cfg(&[
                "kwriteconfig6", "--file", "kscreenlockerrc", "--group", "Greeter",
                "--key", "WallpaperPlugin", "org.kde.image",
            ]),
        ],
        success_message,
        failure_message,
        uri,
    })
}

/// `pathlib.Path(abs).as_uri()`: `file://` + percent-encoding
/// (unreserved `A-Za-z0-9_.-~` and `/` are not encoded).
pub fn path_as_uri(abs_path: &str) -> String {
    let mut out = String::from("file://");
    for &b in abs_path.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-' | b'~' | b'/') {
            out.push(b as char);
        } else {
            out.push('%');
            out.push_str(&format!("{b:02X}"));
        }
    }
    out
}

/// Whether the installed Global Theme is outdated (Plasma 5 without metadata.json) — port of
/// `_global_theme_missing_metadata_json`. Checks the system and
/// user look-and-feel directories from the snapshot.
pub fn global_theme_missing_metadata_json(
    fs: &FsSnapshot,
    home: &str,
    theme_name: &str,
) -> bool {
    let roots = [
        "/usr/share/plasma/look-and-feel".to_string(),
        format!("{home}/.local/share/plasma/look-and-feel"),
    ];
    for root in roots {
        let d = join(&root, theme_name);
        if fs.is_dir(&d) {
            return !fs.is_file(&join(&d, "metadata.json"));
        }
    }
    false
}

// ============================================================================
// Uninstalling
// ============================================================================

/// Uninstall plan from a log record — port of `uninstall`. `paths` — from
/// `rec["paths"]`; `is_dir`/`is_file` — existence probes (outside).
/// Directories → RemoveTree, files → RemoveFile; nonexistent ones — skipped.
pub fn plan_uninstall(
    paths: &[String],
    is_dir: &dyn Fn(&str) -> bool,
    is_file: &dyn Fn(&str) -> bool,
) -> Vec<ExtStep> {
    let mut steps = Vec::new();
    for p in paths {
        if is_dir(p) {
            steps.push(ExtStep::RemoveTree { path: p.clone(), safe: false, ignore_errors: true });
        } else if is_file(p) {
            steps.push(ExtStep::RemoveFile { path: p.clone() });
        }
    }
    steps
}

/// Uninstall message — port of the `uninstall` line
/// (`f"Removed ({removed} paths): {name}"`). `removed` = number of removed paths.
pub fn uninstall_message(removed: usize, name: &str) -> String {
    trf("Removed (%1 paths): %2", &[&removed, &name])
}

// ============================================================================
// Where to put a screenshot downloaded from the Store — port of _save_ocs_preview
// ============================================================================

/// The path a downloaded OCS screenshot is copied to, so that the local scan shows
/// it instead of a placeholder ("Option 1") — port of the `_save_ocs_preview` DECISION.
///
/// * a type with a directory → `<theme_dir>/.rexon-preview.png`;
/// * a color scheme (`apply_kind == "colors"`, no directory) or a file path →
///   sidecar cache `~/.cache/rexon/local_previews/<kind>__<name>.png`;
/// * `None` — no name at all (nowhere to copy, the original quietly returns).
///
/// The copying itself (`shutil.copyfile`), `makedirs` and finding the source
/// (`_ocs_preview_source`: `preview_local` or a file from the preview cache) are side
/// effects, they stay with the caller. `theme_dir_is_dir` — the observation
/// `os.path.isdir(theme_dir)`.
pub fn ocs_preview_dest(
    home: &str,
    apply_kind: &str,
    theme_dir: &str,
    theme_dir_is_dir: bool,
    item_name: &str,
) -> Option<String> {
    if apply_kind != "colors" && !theme_dir.is_empty() && theme_dir_is_dir {
        // os.path.join does not double the separator if the left part already ends with "/".
        let sep = if theme_dir.ends_with('/') { "" } else { "/" };
        return Some(format!(
            "{theme_dir}{sep}{}",
            crate::installed_scan::REXON_PREVIEW_NAME
        ));
    }
    // Theme name: its own, or the last path segment (without trailing slashes) —
    // port of `item.name or os.path.basename(theme_dir.rstrip("/"))`.
    let name = if item_name.is_empty() {
        let trimmed = theme_dir.trim_end_matches('/');
        trimmed.rsplit('/').next().unwrap_or("").to_string()
    } else {
        item_name.to_string()
    };
    if name.is_empty() {
        return None;
    }
    Some(crate::installed_scan::sidecar_preview_path(home, apply_kind, &name))
}

// ============================================================================
// LIVE EXECUTOR: installing, applying and uninstalling appearance themes
// ============================================================================
//
// WHY. Everything above is DECISIONS: where to put a theme, what to apply it with, what to
// delete. Until now the core could not DO anything: the executor of these plans
// was Python (`rexon/appearance/installer.py`, the `ThemeInstaller` class), and
// so the "Appearance" tab in a process without Python could neither install
// a theme nor apply it nor remove it — the buttons were there, but nothing behind them.
//
// The executor is written here: the same side calls Python made
// (`shutil`/`os`/`subprocess`/`pkexec`), but in Rust. The split is the same and it is
// fundamental — **the planner decides, the executor does**:
//   * what to copy and what to run — [`plan_install`] / [`plan_single_file`] /
//     [`plan_root_install`] / [`plan_uninstall`];
//   * what to apply with — [`apply_action_for`] plus the [`crate::themes`] plans;
//   * the FS snapshot for the planner — [`install_tree_snapshot`];
//   * running the steps — [`exec_install_steps`] and [`exec_apply_plan`];
//   * whole pipelines — [`LiveInstaller`].
//
// ⛔ THERE IS NO SECOND COPY OF THE RULES HERE. This section makes not a single decision of
// "which directory", "which name", "which config key": they are all already made
// above or by the neighbors. The installer is the most dangerous place for silent divergence
// (two copies deciding WHERE to copy and WHAT to delete), and that is exactly why the boundary
// is drawn so strictly.
//
// A live WE wallpaper is stopped before a static one by the shared wallpaper executor
// ([`crate::wallpaper_exec::WallpaperExec::apply`] → the process hook
// `wallpaper_exec::set_we_stopper`, set by the Qt window): there is no second
// copy of stopping here.
//
// ⚠️ WHAT DOES NOT WORK LIVE (named, not hushed up):
//   * wallpaper plan steps that start a HOLDING process (`swww`, `swaybg`, `wbg`,
//     `hyprpaper`): such a process must outlive the call, and such processes
//     are tracked by the wallpaper engine, whose live executor is not in the core.
//     Such a step answers with an honest refusal in words, not with silence.

use std::path::PathBuf;

use crate::themes::ApplyStep;
use crate::utils::{RunOutcome, RunOutput, RunReport, RunSpec};

/// Actual unpack types that have their own install directory — the keys of
/// [`actual_type_dir`] (port of `_ACTUAL_TYPE_DIR`).
///
/// The executor needs the list for exactly one thing: to give the planner a snapshot of ALL
/// directories the installation may move to after reclassification — otherwise
/// [`plan_install`] will not see an already installed theme and will not remove it before copying.
/// The divergence guard against the table is the `actual_types_have_dirs` check.
const ACTUAL_TYPES_WITH_DIR: &[&str] = &[
    "look-and-feel", "plasma-theme", "color-scheme", "aurorae", "kvantum",
    "icons",
];

/// Names of files whose text goes into the tree snapshot — port of `_META_NAMES`.
///
/// `index.theme` was added on 03.10.2026: by its section (`[Sound Theme]`,
/// `[Icon Theme]`) [`find_theme_roots`] recognizes theme roots in an archive.
const SNAPSHOT_META_NAMES: &[&str] = &["metadata.desktop", "metadata.json", "index.theme"];

// ── environment odds and ends ──────────────────────────────────────────────────

/// `$HOME` — the same one Python substituted (`os.path.expanduser("~")`).
fn live_home() -> String {
    std::env::var("HOME").unwrap_or_else(|_| ".".to_string())
}

/// An environment variable's value or an empty string.
fn env_str(name: &str) -> String {
    std::env::var(name).unwrap_or_default()
}

/// `$PATH` for finding binaries (`None` means there is no such variable at all).
fn path_env() -> Option<String> {
    std::env::var("PATH").ok()
}

/// All paths of the installed log (what Rexon itself installed and where):
/// by them the disk scan recognises system themes installed by Rexon
/// ([`crate::installed_scan::scan_rexon_system`]).
pub fn installed_log_paths() -> Vec<String> {
    let log = crate::ocs_store::install_log_path_xdg(xdg_config_home().as_deref(), &env_str("HOME"));
    // Read through the common install log (step 3 of merging the installers).
    crate::install_journal::store_log_view(&log)
        .values()
        .filter_map(|e| e.get("paths")?.as_array().cloned())
        .flatten()
        .filter_map(|p| p.as_str().map(str::to_string))
        .collect()
}

/// `$XDG_CONFIG_HOME`, if it is set.
fn xdg_config_home() -> Option<String> {
    let v = env_str("XDG_CONFIG_HOME");
    if v.is_empty() { None } else { Some(v) }
}

/// `$XDG_DATA_HOME`, if it is set.
fn xdg_data_home() -> Option<String> {
    let v = env_str("XDG_DATA_HOME");
    if v.is_empty() { None } else { Some(v) }
}

/// The installed log timestamp: a port of
/// `datetime.datetime.now().isoformat(timespec="seconds")`: LOCAL time
/// without a zone. UTC will not do here: the log is shown to the person in
/// the theme card.
fn now_local_iso() -> String {
    // There is one local clock in the product for everyone (rule #12): there
    // is no `localtime_r` of its own here any more, the common
    // `Timestamp::now_local` is called. `microsecond: 0` is the original's
    // `timespec="seconds"`: `isoformat` prints the fraction only when there
    // is one.
    match crate::qt_app::Timestamp::now_local() {
        Some(now) => crate::qt_app::Timestamp { microsecond: 0, ..now }.isoformat(),
        None => {
            // The zone cannot be read: better UTC with a marker than an empty string.
            let secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            crate::engine_update::now_iso(secs)
        }
    }
}

/// Running an external command and capturing its output, through the
/// product's ONLY launch point ([`crate::utils::run`]): it also substitutes
/// binary synonyms (`kwriteconfig6` -> `kwriteconfig5` on Plasma 5).
///
/// A system theme helper command (installation, "Apply", "Remove") first
/// goes to the window's privilege session
/// ([`crate::ext_exec::run_system_argv_session`]: one password); if there
/// is none, it goes the old way. A refusal by the helper is not repeated
/// via the old way (Z5).
fn run_capture(argv: &[String], timeout_secs: Option<u64>) -> RunReport {
    if let Some(report) = crate::ext_exec::run_system_argv_session(argv, timeout_secs) {
        return report;
    }
    crate::utils::run(
        &RunSpec {
            argv: argv.to_vec(),
            output: RunOutput::Capture,
            timeout_secs,
            ..RunSpec::default()
        },
        path_env().as_deref(),
    )
}

/// `safe_run(argv).returncode == 0` means the command succeeded.
fn run_ok(argv: &[String], timeout_secs: Option<u64>) -> bool {
    run_capture(argv, timeout_secs).returncode == Some(0)
}

/// Whether GTK theme `theme` is dark ([`crate::themes::gtk_theme_is_dark`]):
/// the GTK 3 style sheet (otherwise GTK 4) from the first themes directory
/// where the theme exists.
fn gtk_theme_dark_live(theme: &str, home: &str) -> bool {
    let data = crate::utils::data_home(xdg_data_home().as_deref(), home);
    let css = crate::gtk_sanitizer::theme_dirs("gtk-theme", home, &data)
        .into_iter()
        .map(|base| format!("{base}/{theme}"))
        .find(|dir| Path::new(dir).is_dir())
        .and_then(|dir| {
            ["gtk-3.0", "gtk-4.0"].iter().find_map(|v| read_text_lossy(&format!("{dir}/{v}/gtk.css")))
        });
    crate::themes::gtk_theme_is_dark(theme, css.as_deref())
}

/// The standard theme of role `role` (`icons`, `cursor`) in environment
/// `de`: the machine's answer to [`crate::themes::standard_theme_read_cmd`].
/// `None` means the environment has no schema for the role or gsettings
/// did not answer.
pub fn live_standard_theme(de: &str, role: &str) -> Option<String> {
    let cmd = crate::themes::standard_theme_read_cmd(de, role)?;
    let report = run_capture(&cmd.argv, cmd.timeout);
    if report.returncode != Some(0) {
        return None;
    }
    let name = crate::themes::parse_native_role_output(de, &crate::utils::decode_text(&report.stdout));
    (!name.is_empty()).then_some(name)
}

/// The package family for installing a theme's missing icons and cursor.
/// On SteamOS and immutable systems a system package will not install, so
/// an empty string: Rexon honestly says the set is missing instead of a
/// password window that would achieve nothing.
pub(crate) fn live_deps_family() -> String {
    let detector = crate::system::DistroDetector::detect_live();
    if detector.is_steamos() || detector.is_immutable() {
        return String::new();
    }
    detector.family_live().as_str().to_string()
}

/// The installed set for a theme's request, on the live disk
/// ([`crate::themes::find_installed_dep`]).
pub(crate) fn live_find_dep(role: crate::themes::DepRole, wanted: &str, roots: &[String]) -> Option<String> {
    let is_dir = |p: &str| Path::new(p).is_dir();
    let list = |p: &str| -> Vec<String> {
        std::fs::read_dir(p)
            .map(|rd| rd.filter_map(|e| e.ok()?.file_name().into_string().ok()).collect())
            .unwrap_or_default()
    };
    let read = |p: &str| read_text_lossy(p);
    crate::themes::find_installed_dep(role, wanted, roots, &is_dir, &list, &read)
}

/// Install packages with a theme's icons and cursor the same way as all of
/// Rexon's package installation: a repository package via
/// [`crate::system_plan::install_argv`] (the `org.rexon.pkg` helper or
/// `pkexec`), on Arch a package not in the repositories via the AUR
/// helper. `Err` is a reason for the person.
///
/// What actually got installed is decided not by the exit code but by the
/// disk: the caller looks for the set again.
pub(crate) fn live_install_dep_packages(packages: &[String]) -> Result<(), String> {
    live_install_packages(packages, true, PkgSource::ThemeSets)
}

/// The same as [`live_install_dep_packages`], but only from the official
/// repositories: a package that is not there is NOT built from the AUR
/// (Rexon does not start an AUR build without a question) but named in
/// `Err`, after the others are installed.
///
/// `source` is the core table the names come from ([`PkgSource`]): the
/// window's privilege session helper checks them against it.
pub(crate) fn live_install_repo_packages(packages: &[String], source: PkgSource) -> Result<(), String> {
    live_install_packages(packages, false, source)
}

/// The privilege session operation for repository packages: the same names
/// and the same family as the old command [`crate::system_plan::install_argv`].
fn repo_install_op(family: crate::system::PkgFamily, repo: &[String], source: PkgSource) -> PrivOp {
    PrivOp::PkgInstall { family, packages: repo.to_vec(), source }
}

fn live_install_packages(packages: &[String], allow_aur: bool, source: PkgSource) -> Result<(), String> {
    use crate::system::PkgFamily;
    use crate::system_plan as sp;
    let family = crate::system::DistroDetector::detect_live().family_live();
    let mut waiter = sp::LiveLockWait::new(|_: &str, _: &str| {});
    let (free, why) = sp::wait_for_pkg_lock(&mut waiter, &sp::lock_paths_live(family), sp::PKG_LOCK_TIMEOUT_SECS);
    if !free {
        return Err(why);
    }
    // In CachyOS `mint-y-icons` is in the repository, on plain Arch only in
    // the AUR: the path is chosen by fact (`pacman -Si`), not by the table.
    let (repo, aur): (Vec<String>, Vec<String>) = if family == PkgFamily::Arch {
        packages
            .iter()
            .cloned()
            .partition(|p| run_ok(&["pacman".to_string(), "-Si".to_string(), p.clone()], Some(30)))
    } else {
        (packages.to_vec(), Vec::new())
    };
    let helper_ok = have("pkexec")
        && sp::is_exec_file(sp::pkg_helper_path())
        && Path::new(sp::PKG_POLICY_PATH).exists();
    let mut commands: Vec<Vec<String>> = Vec::new();
    if !repo.is_empty() {
        let Some(argv) = sp::install_argv(family, &repo, helper_ok) else {
            return Err(tr("Rexon doesn't know the package manager of this system"));
        };
        // The window's privilege session (one password); if there is none, the
        // old command. A refusal by the helper is not repeated via the old
        // command (Z5). Without `pkexec` (NixOS, a user profile) no privileges
        // are needed at all.
        let op = repo_install_op(family, &repo, source);
        let session = if argv.first().map(String::as_str) == Some("pkexec") {
            crate::privilege::run::run_live(&op, &[], None, &mut |_| {})
        } else {
            None
        };
        match session {
            Some(out) => match out.returncode {
                Some(0) => {}
                Some(126) | Some(127) => return Err(password_not_confirmed()),
                _ => return Err(tr("the package manager reported an error")),
            },
            None => commands.push(argv),
        }
    }
    if !aur.is_empty() && allow_aur {
        let Some(helper) = crate::compliance::AUR_HELPERS.iter().find(|h| have(h)) else {
            return Err(crate::appearance_page::aur_no_builder());
        };
        commands.push(crate::aur_install::install_argv(helper, &aur, false));
    }
    for argv in commands {
        let report = crate::utils::run(
            &RunSpec {
                argv,
                output: RunOutput::Capture,
                timeout_secs: Some(3600),
                lang_c: true,
                stderr_to_stdout: true,
                ..RunSpec::default()
            },
            path_env().as_deref(),
        );
        match report.returncode {
            Some(0) => {}
            // pkexec: the window was closed (126) or the password was not accepted (127).
            Some(126) | Some(127) => return Err(password_not_confirmed()),
            _ => return Err(tr("the package manager reported an error")),
        }
    }
    if !aur.is_empty() && !allow_aur {
        // TRANSLATORS: %1 is a list of packages.
        return Err(trf("not in the official repositories: %1", &[&aur.join(", ")]));
    }
    Ok(())
}

/// The reason a package installation failed when the polkit password
/// window was closed or the password not accepted: by it the caller tells
/// "the person changed their mind" from a package manager error.
pub fn password_not_confirmed() -> String {
    tr("the administrator password wasn't confirmed")
}

/// Whether a program is in the system (`shutil.which`).
fn have(binary: &str) -> bool {
    crate::utils::which(binary, path_env().as_deref()).is_some()
}

/// A unique temporary directory (`tempfile.mkdtemp(prefix=…)`).
fn mkdtemp_in(base: &Path, prefix: &str) -> Result<PathBuf, String> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    for attempt in 0..64u64 {
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let path =
            base.join(format!("{prefix}{}_{stamp}_{}", std::process::id(), n + attempt));
        match std::fs::create_dir_all(&path) {
            Ok(()) => return Ok(path),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(trf("couldn't create a temporary folder: %1", &[&e])),
        }
    }
    Err(tr("couldn't create a temporary folder: all names are taken"))
}

/// The file text with invalid bytes replaced (`errors="replace"`); `None`
/// means it could not be read (the analogue of `except OSError`).
fn read_text_lossy(path: &str) -> Option<String> {
    std::fs::read(path)
        .ok()
        .map(|raw| String::from_utf8_lossy(&raw).into_owned())
}

// ── installation step executor ──────────────────────────────────────────────

/// Execute a plan of installation steps: a port of `_rs_exec_steps`.
///
/// ⭐ Fixed during the port: in the original the `Run` step
/// (`gtk-update-icon-cache`) broke the WHOLE installation if the program
/// was not in the system: `subprocess.run` raised `FileNotFoundError`, and
/// `install` returned "Installation error" AFTER the theme was already
/// copied, never reaching the log write. The theme ended up on disk but
/// not in the installed list, and there was nothing to remove it with.
/// Here a failure of an external command is not a plan error: the files
/// are already in place, the icon cache will be rebuilt at the next login.
pub fn exec_install_steps(steps: &[ExtStep]) -> Result<(), String> {
    use crate::install_plan::{exec, Ctx, Failure};
    let plan: Vec<crate::install_plan::Step> = steps.iter().cloned().map(Into::into).collect();
    let ctx = Ctx::live(true, false, None, false);
    exec(&plan, &mut Vec::new(), &ctx).map(|_| ()).map_err(|f| match f {
        Failure::MakeDirs { path, err } | Failure::Remove { path, err } => format!("{path}: {err}"),
        Failure::Copy { src, dest, err } => format!("{src} → {dest}: {err}"),
        // The store plan has no other steps; the text is the same as for the rest.
        Failure::Text(text) => text,
        Failure::Symlink { link: path, err }
        | Failure::Write { path, err }
        | Failure::Unpack { archive: path, err }
        | Failure::Prepare { source: path, err } => format!("{path}: {err}"),
    })
}

// ── snapshot of the unpacked tree ───────────────────────────────────────────

/// A tree snapshot for the planner: a port of `_rs_tree_snapshot`.
///
/// Recursively `root` (text is read ONLY for
/// `metadata.json`/`metadata.desktop`; the other files go into the snapshot
/// as dummies, since the planner only needs the fact that they exist),
/// plus NON-recursively the children of the `dest_dirs` directories: by
/// them [`plan_install`] learns that a theme with this name is already
/// installed and removes it.
///
/// Names inside a directory are sorted. The original had the `os.scandir`
/// order here, i.e. the filesystem order: both the choice of "the first
/// <name>.plymouth" and the order of names in the colour scheme
/// installation message depend on it, and it differs from machine to
/// machine. Sorting makes the answer the same everywhere (the same
/// argument as in `ext_exec`).
pub fn install_tree_snapshot(root: &str, dest_dirs: &[String]) -> FsSnapshot {
    let mut dirs = serde_json::Map::new();
    let mut files = serde_json::Map::new();

    fn names_of(dir: &str) -> Option<Vec<String>> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .ok()?
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        Some(names)
    }

    fn rec(
        dir: &str,
        dirs: &mut serde_json::Map<String, Value>,
        files: &mut serde_json::Map<String, Value>,
    ) {
        let Some(names) = names_of(dir) else { return };
        dirs.insert(dir.to_string(), Value::from(names.clone()));
        for name in names {
            let path = join(dir, &name);
            let Ok(md) = std::fs::symlink_metadata(&path) else { continue };
            if md.is_dir() {
                rec(&path, dirs, files);
            } else {
                let text = if SNAPSHOT_META_NAMES.contains(&name.as_str()) || name.ends_with(".colors") {
                    read_text_lossy(&path).unwrap_or_default()
                } else {
                    String::new()
                };
                files.insert(path, Value::from(text));
            }
        }
    }

    rec(root, &mut dirs, &mut files);

    for base in dest_dirs {
        if base.is_empty() {
            continue;
        }
        let Some(names) = names_of(base) else { continue };
        dirs.entry(base.clone())
            .or_insert_with(|| Value::from(names.clone()));
        for name in names {
            let path = join(base, &name);
            let Ok(md) = std::fs::symlink_metadata(&path) else { continue };
            // A folder (and a link to a folder: our `~/.themes/<name>` links)
            // comes with its first-level names: by them window frames decide
            // whether to land as a subfolder in a GTK theme ([`plan_nested_part`]).
            let dir_like = md.is_dir() || (md.file_type().is_symlink() && Path::new(&path).is_dir());
            if dir_like {
                let inner = names_of(&path).unwrap_or_default();
                for child in &inner {
                    let child_path = join(&path, child);
                    if Path::new(&child_path).is_dir() {
                        dirs.entry(child_path).or_insert_with(|| Value::Array(Vec::new()));
                    } else {
                        files.entry(child_path).or_insert_with(|| Value::from(""));
                    }
                }
                dirs.entry(path).or_insert_with(|| Value::from(inner));
            } else {
                // A colour scheme in place comes with its text: one identical byte
                // for byte counts as ours ([`file_taken`]); a link as a dummy.
                let text = if md.is_file() && name.ends_with(".colors") {
                    read_text_lossy(&path).unwrap_or_default()
                } else {
                    String::new()
                };
                files.entry(path).or_insert_with(|| Value::from(text));
            }
        }
    }

    FsSnapshot::from_json(&serde_json::json!({ "dirs": dirs, "files": files }))
}

// ── lifting nested themes: repairing already broken installations ───────────
//
// Installations before 03.10.2026 put the archive "as is" (see
// [`find_theme_roots`]): the owner's disk has
// `~/.local/share/sounds/zorinos2/sounds/Zorin/…` and
// `~/.local/share/sounds/Sounds/Zorin/…`. The migration finds such
// wrappers (only OUR OWN: with a Rexon preview `.rexon-preview.png` inside
// or recorded in the installed log), lifts each theme to the top level of
// the kind's directory by renaming (atomic within one filesystem) and
// carries the rest of the wrapper (wallpapers, xml, ReadMe) away to
// `<data>/rexon/theme-leftovers/<kind>/`: nothing is deleted. Each step is
// written to the migration log, and the installed log entry is rewritten
// to the new paths so that "Remove" removes everything.
//
// A taken name at the top is an honest skip: the theme stays in the
// wrapper, the wrapper stays in place, and the log gets a line about the
// conflict.

/// Moving one theme root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NestedMove {
    pub from: String,
    pub to: String,
}

/// The plan for lifting themes out of one wrapper.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct NestedWrapperPlan {
    pub kind: String,
    pub wrapper: String,
    pub moves: Vec<NestedMove>,
    /// Roots that cannot be lifted: the name at the top is taken (or repeats
    /// within the same wrapper). They stay in place.
    pub conflicts: Vec<NestedMove>,
    /// Where to carry the rest of the wrapper; `None` means the wrapper stays (there is a conflict).
    pub leftovers_to: Option<String>,
}

/// A pure decision for one wrapper `<base>/<wrapper_name>`.
///
/// `None` means nothing to do: the directory is itself a theme (installed
/// correctly) or there are no themes inside. `exists` is the observation
/// "the path at the top is taken".
pub fn plan_nested_wrapper(
    fs: &FsSnapshot,
    base: &str,
    wrapper_name: &str,
    kind: &str,
    exists: &dyn Fn(&str) -> bool,
    leftovers_dest: &str,
) -> Option<NestedWrapperPlan> {
    let wrapper = join(base, wrapper_name);
    if is_theme_root(fs, &wrapper, kind) {
        return None;
    }
    let roots = find_theme_roots(fs, &wrapper, kind);
    if roots.is_empty() {
        return None;
    }
    let mut plan = NestedWrapperPlan {
        kind: kind.to_string(),
        wrapper: wrapper.clone(),
        ..Default::default()
    };
    let mut taken: Vec<String> = Vec::new();
    for root in roots {
        let name = basename(rstrip_slashes(&root)).to_string();
        let to = join(base, &name);
        // The wrapper's own name is taken by itself: lifting `Sounds/Sounds`
        // over `Sounds` cannot be done by renaming, an honest conflict.
        if taken.contains(&to) || exists(&to) || to == wrapper {
            plan.conflicts.push(NestedMove { from: root, to });
            continue;
        }
        taken.push(to.clone());
        plan.moves.push(NestedMove { from: root, to });
    }
    if plan.conflicts.is_empty() {
        plan.leftovers_to = Some(leftovers_dest.to_string());
    }
    Some(plan)
}

/// The migration log: `<data>/rexon/theme-migration.log`.
pub fn nested_migration_log_path(data_home: &str) -> String {
    format!("{}/rexon/theme-migration.log", data_home.trim_end_matches('/'))
}

/// Where wrapper leftovers go: `<data>/rexon/theme-leftovers`.
pub fn nested_leftovers_dir(data_home: &str) -> String {
    format!("{}/rexon/theme-leftovers", data_home.trim_end_matches('/'))
}

/// Lifting nested themes under `home`: the execution.
///
/// Touches ONLY the directories of kinds from [`kind_uses_theme_roots`]
/// and only wrappers with a Rexon marker or from the installed log.
/// Returns log lines (empty means there was nothing to do); they are also
/// appended to [`nested_migration_log_path`].
pub fn live_migrate_nested_themes(home: &str) -> Vec<String> {
    let data = crate::utils::data_home(xdg_data_home().as_deref(), home);
    let log_path =
        crate::ocs_store::install_log_path_xdg(xdg_config_home().as_deref(), home);
    migrate_nested_themes_at(home, &data, &log_path)
}

/// The body of [`live_migrate_nested_themes`] with explicit directories:
/// `data` is the data directory (the migration log, leftovers), `log_path`
/// the installed log. Parity checks call it on a fake HOME without
/// touching the process environment.
pub fn migrate_nested_themes_at(home: &str, data: &str, log_path: &str) -> Vec<String> {
    let home_p = Path::new(home);
    let data = data.to_string();
    let log_path = log_path.to_string();
    let mut journal = crate::install_journal::store_log_view(&log_path);
    let recorded: Vec<String> = journal
        .values()
        .filter_map(|e| e.get("paths").and_then(Value::as_array))
        .flatten()
        .filter_map(Value::as_str)
        .map(|p| rstrip_slashes(p).to_string())
        .collect();

    let mut lines: Vec<String> = Vec::new();
    let mut journal_changed = false;
    let mut seen_bases: Vec<String> = Vec::new();
    for kind in ["sounds", "icons", "gtk", "aurorae"] {
        let Some(cat) = category_by_kind(home_p, kind) else { continue };
        let base = rstrip_slashes(&cat.install_dir).to_string();
        if base.is_empty() || seen_bases.contains(&base) {
            continue;
        }
        seen_bases.push(base.clone());
        let Ok(rd) = std::fs::read_dir(&base) else { continue };
        let mut names: Vec<String> = rd
            .flatten()
            .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| !n.starts_with('.'))
            .collect();
        names.sort();
        for name in names {
            let wrapper = join(&base, &name);
            let marker = join(&wrapper, crate::installed_scan::REXON_PREVIEW_NAME);
            let ours = Path::new(&marker).is_file() || recorded.contains(&wrapper);
            if !ours {
                continue;
            }
            let snap = install_tree_snapshot(&wrapper, &[]);
            let leftovers = unique_path(&format!("{}/{kind}/{name}", nested_leftovers_dir(&data)));
            let exists = |p: &str| Path::new(p).exists();
            let Some(mut plan) = plan_nested_wrapper(&snap, &base, &name, kind, &exists, &leftovers)
            else {
                continue;
            };
            let mut moved: Vec<String> = Vec::new();
            for mv in std::mem::take(&mut plan.moves) {
                match std::fs::rename(&mv.from, &mv.to) {
                    Ok(()) => {
                        lines.push(format!("перенесено: {} → {}", mv.from, mv.to));
                        if Path::new(&marker).is_file() {
                            let dst = join(&mv.to, crate::installed_scan::REXON_PREVIEW_NAME);
                            if !Path::new(&dst).exists() {
                                let _ = std::fs::copy(&marker, &dst);
                            }
                        }
                        moved.push(mv.to);
                    }
                    Err(e) => {
                        lines.push(format!("НЕ перенесено ({e}): {} → {}", mv.from, mv.to));
                        plan.leftovers_to = None;
                    }
                }
            }
            for c in &plan.conflicts {
                lines.push(format!("конфликт, оставлено на месте: {} (занято {})", c.from, c.to));
            }
            let mut wrapper_now = wrapper.clone();
            if let Some(dest) = &plan.leftovers_to {
                let parent = Path::new(dest).parent().map(Path::to_path_buf);
                let ok = parent.map(|p| std::fs::create_dir_all(p).is_ok()).unwrap_or(false)
                    && std::fs::rename(&wrapper, dest).is_ok();
                if ok {
                    lines.push(format!("остаток обёртки: {wrapper} → {dest}"));
                    wrapper_now = dest.clone();
                } else {
                    lines.push(format!("остаток обёртки оставлен на месте: {wrapper}"));
                }
            }
            // The installed log: wrapper -> lifted themes (+ where the leftovers went).
            for entry in journal.values_mut() {
                let Some(paths) = entry.get_mut("paths").and_then(Value::as_array_mut) else {
                    continue;
                };
                if !paths.iter().any(|p| p.as_str().map(rstrip_slashes) == Some(wrapper.as_str())) {
                    continue;
                }
                let mut next: Vec<Value> = paths
                    .iter()
                    .filter(|p| p.as_str().map(rstrip_slashes) != Some(wrapper.as_str()))
                    .cloned()
                    .collect();
                next.extend(moved.iter().map(|m| Value::from(m.clone())));
                next.push(Value::from(wrapper_now.clone()));
                *paths = next;
                journal_changed = true;
            }
        }
    }
    if journal_changed {
        if let Err(e) = crate::install_journal::put_store_log(&log_path, &journal) {
            lines.push(format!("журнал установленного не переписан: {e}"));
        }
    }
    if !lines.is_empty() {
        let log = nested_migration_log_path(&data);
        if let Some(dir) = Path::new(&log).parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let stamp = now_local_iso();
        let mut text = read_text_lossy(&log).unwrap_or_default();
        for l in &lines {
            text.push_str(&format!("{stamp} {l}\n"));
        }
        let _ = crate::utils::atomic_write(&log, &text);
    }
    lines
}

/// A free name: `path`, otherwise `path-2`, `path-3`, …
fn unique_path(path: &str) -> String {
    if !Path::new(path).exists() {
        return path.to_string();
    }
    (2..).map(|n| format!("{path}-{n}")).find(|p| !Path::new(p).exists()).unwrap()
}

/// [`live_migrate_nested_themes`] no more than once per `home` per
/// process, for calling from the disk scan.
pub fn migrate_nested_themes_once(home: &str) -> Vec<String> {
    use std::sync::Mutex;
    static DONE: Mutex<Vec<String>> = Mutex::new(Vec::new());
    {
        let Ok(mut done) = DONE.lock() else { return Vec::new() };
        if done.iter().any(|h| h == home) {
            return Vec::new();
        }
        done.push(home.to_string());
    }
    live_migrate_nested_themes(home)
}

/// A mini snapshot of look-and-feel directories for
/// [`global_theme_missing_metadata_json`]: a port of `_rs_lnf_snapshot`.
fn lnf_snapshot(home: &str, theme_name: &str) -> FsSnapshot {
    let mut dirs = serde_json::Map::new();
    let mut files = serde_json::Map::new();
    let roots = [
        "/usr/share/plasma/look-and-feel".to_string(),
        format!("{home}/.local/share/plasma/look-and-feel"),
    ];
    for root in roots {
        let d = join(&root, theme_name);
        if Path::new(&d).is_dir() {
            dirs.insert(d.clone(), Value::Array(Vec::new()));
            let mj = join(&d, "metadata.json");
            if Path::new(&mj).is_file() {
                files.insert(mj, Value::from(""));
            }
        }
    }
    FsSnapshot::from_json(&serde_json::json!({ "dirs": dirs, "files": files }))
}

/// Whether a Global Theme is installed in the outdated Plasma 5 format
/// (without `metadata.json`): a live port of
/// `ThemeInstaller._global_theme_missing_metadata_json`.
///
/// Plasma 6 (KPackage v2) requires `metadata.json` with
/// `KPackageStructure="Plasma/LookAndFeel"`; a theme with only
/// `metadata.desktop` is not registered and silently does not apply. The
/// "Appearance" tab needs the answer synchronously, hence a separate
/// function and not only a branch inside applying.
pub fn live_global_theme_missing_metadata_json(home: &str, theme_name: &str) -> bool {
    global_theme_missing_metadata_json(&lnf_snapshot(home, theme_name), home, theme_name)
}

// ── unpacking ───────────────────────────────────────────────────────────────

/// The absolute normalised path of the unpack directory (`os.path.abspath`).
fn abs_dest(dest: &str) -> String {
    if dest.starts_with('/') {
        normpath(dest)
    } else {
        let cwd = std::env::current_dir()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| ".".to_string());
        normpath(&py_join(&cwd, dest))
    }
}

/// Entry names of a ZIP archive; `None` means it is not a ZIP.
fn zip_entry_names(archive: &str) -> Option<Vec<String>> {
    let file = std::fs::File::open(archive).ok()?;
    let mut zf = zip::ZipArchive::new(file).ok()?;
    let mut names = Vec::with_capacity(zf.len());
    for i in 0..zf.len() {
        names.push(zf.by_index(i).ok()?.name().to_string());
    }
    Some(names)
}

/// Entry names of a TAR archive via the system `tar -tf`; `None` means not
/// a TAR or there is no `tar` in the system (the caller tells these cases
/// apart by [`have`]).
fn tar_entry_names(archive: &str) -> Option<Vec<String>> {
    if !have("tar") {
        return None;
    }
    let argv = vec!["tar".to_string(), "-tf".to_string(), archive.to_string()];
    let report = run_capture(&argv, Some(120));
    if report.returncode != Some(0) {
        return None;
    }
    Some(
        crate::utils::decode_text(&report.stdout)
            .lines()
            .map(str::to_string)
            .filter(|l| !l.is_empty())
            .collect(),
    )
}

/// The 7z archive signature in the first bytes of a file.
const SEVEN_ZIP_MAGIC: &[u8] = b"7z\xBC\xAF\x27\x1C";

/// What to unpack a 7z archive with: `bsdtar` (libarchive; always installed
/// on Arch and SteamOS, since pacman depends on it) or 7-Zip under any of
/// its names.
fn seven_zip_tool() -> Option<&'static str> {
    ["bsdtar", "7z", "7za", "7zz"].into_iter().find(|b| have(b))
}

/// Whether it is a 7z archive (by the signature at the start of the file, not by name).
fn is_seven_zip(archive: &str) -> bool {
    read_head(archive, SEVEN_ZIP_MAGIC.len()).starts_with(SEVEN_ZIP_MAGIC)
}

/// There is nothing to unpack 7z with: a refusal for the person with the
/// packages of their distribution family (`family` is `system::PkgFamily`;
/// an unknown one gets names as in most distributions).
pub fn seven_zip_missing(family: &str) -> String {
    trf(
        // TRANSLATORS: %1 and %2 are package names, e.g. “libarchive-tools” and “7zip”.
        "To unpack .7z archives you need the bsdtar program (package %1) or 7-Zip (package %2)",
        &[
            &crate::utils::tool_package_name("bsdtar", family),
            &crate::utils::tool_package_name("7z", family),
        ],
    )
}

/// The 7z unpacker package (bsdtar) Rexon can install on a system of family
/// `family`; `None` means it cannot (a family without an entry in the
/// package table).
pub fn seven_zip_package(family: &str) -> Option<&'static str> {
    crate::utils::TOOL_PACKAGES
        .iter()
        .find(|(bin, _)| *bin == "bsdtar")?
        .1
        .iter()
        .find(|(f, _)| *f == family)
        .map(|(_, pkg)| *pkg)
}

/// [`seven_zip_package`] on THIS machine; on SteamOS and immutable systems a
/// system package will not install, so `None`.
pub fn live_seven_zip_package() -> Option<&'static str> {
    let detector = crate::system::DistroDetector::detect_live();
    if detector.is_steamos() || detector.is_immutable() {
        return None;
    }
    seven_zip_package(detector.family_live().as_str())
}

/// Whether the machine has something to unpack 7z with.
pub fn have_seven_zip_tool() -> bool {
    seven_zip_tool().is_some()
}

/// The 7z unpacker package Rexon installs before installing an item BY
/// ITSELF, without a "Yes/No" question (the owner's decision of 24.10; the
/// protection is the polkit password window). `None` means nothing to
/// install: the file is not `.7z`, the unpacker exists, or Rexon cannot
/// install it here (then the installation refuses with the words of
/// [`seven_zip_missing`]).
pub fn seven_zip_install_package(file: &str, have_tool: bool, package: Option<&str>) -> Option<String> {
    if have_tool || !file.to_lowercase().ends_with(".7z") {
        return None;
    }
    package.map(str::to_string)
}

/// Install bsdtar the same way as all of Rexon's package installation, only
/// from the official repositories ([`live_install_repo_packages`]), and
/// check the fact. `Err` is a reason for the person.
pub fn install_seven_zip_tool_live() -> Result<(), String> {
    let Some(package) = live_seven_zip_package() else {
        return Err(tr("Rexon can't install programs on this system by itself."));
    };
    live_install_repo_packages(&[package.to_string()], PkgSource::Tools)?;
    if !have_seven_zip_tool() {
        return Err(trf("the %1 package was installed, but the bsdtar program didn't appear", &[&package]));
    }
    Ok(())
}

/// The result when bsdtar did not install: the item is not installed at
/// all (there is nothing to unpack it with anyway), and it says why and
/// what to do. A closed password window means "changed my mind": the
/// advice is to repeat the installation and enter the password.
pub fn seven_zip_install_failed(why: &str) -> String {
    if why == password_not_confirmed() {
        return tr(
            "Nothing was installed: the file is a .7z archive, and unpacking it needs the bsdtar \
             program. Install again and enter the password — Rexon will install bsdtar first.",
        );
    }
    trf("Nothing was installed: couldn't install bsdtar to unpack the .7z archive: %1.", &[&why])
}

/// Entries of a 7z archive.
///
/// ⛔ A bench defect of 17.10.2026: `.7z` is in [`crate::ocs::ARCHIVE_EXTS`]
/// (the store chooses such a file as an archive), while the unpacker knew
/// only zip and tar: "Numix Holo" (xfwm4) and "BlueSky" (Cinnamon) answered
/// "unknown archive format".
///
/// `(name, symbolic link?)`; `None` means not 7z or nothing to unpack it
/// with.
fn seven_zip_entries(archive: &str) -> Option<Vec<(String, bool)>> {
    if !is_seven_zip(archive) {
        return None;
    }
    seven_zip_entries_with(seven_zip_tool()?, archive)
}

/// [`seven_zip_entries`] with program `tool` (`bsdtar`, `7z`, `7za`, `7zz`).
fn seven_zip_entries_with(tool: &str, archive: &str) -> Option<Vec<(String, bool)>> {
    let listing = |argv: Vec<String>| {
        let report = run_capture(&argv, Some(120));
        (report.returncode == Some(0)).then(|| crate::utils::decode_text(&report.stdout))
    };
    if tool == "bsdtar" {
        // `-tf` gives names, `-tvf` the same entries in the same order with
        // permissions (`l…` for a link).
        let names = listing(vec![tool.into(), "-tf".into(), archive.into()])?;
        let long = listing(vec![tool.into(), "-tvf".into(), archive.into()])?;
        let names: Vec<&str> = names.lines().filter(|l| !l.is_empty()).collect();
        let modes: Vec<bool> = long.lines().filter(|l| !l.is_empty()).map(|l| l.starts_with('l')).collect();
        return Some(names.iter().enumerate().map(|(i, n)| (n.to_string(), modes.get(i) == Some(&true))).collect());
    }
    let text = listing(vec![tool.into(), "l".into(), "-ba".into(), "-slt".into(), "--".into(), archive.into()])?;
    Some(parse_seven_zip_slt(&text, archive))
}

/// Parsing `7z l -ba -slt` (p7zip and 7-Zip): entries by `Path = `, a link
/// has Unix permissions in `Attributes = ` starting with `l`
/// (`A lrwxrwxrwx`).
fn parse_seven_zip_slt(text: &str, archive: &str) -> Vec<(String, bool)> {
    let mut out: Vec<(String, bool)> = Vec::new();
    for line in text.lines() {
        if let Some(name) = line.strip_prefix("Path = ") {
            if !name.is_empty() && name != archive {
                out.push((name.to_string(), false));
            }
        } else if let Some(attrs) = line.strip_prefix("Attributes = ") {
            let link = attrs.split_whitespace().any(|w| w.len() == 10 && w.starts_with('l'));
            if let Some(last) = out.last_mut() {
                last.1 |= link;
            }
        }
    }
    out
}

/// An archive entry that would be written THROUGH a link from the same
/// archive (`d -> /somewhere`, then `d/file`): the unpacker would write the
/// file outside the unpack directory. `None` means there are none.
pub fn entry_under_archive_link(entries: &[(String, bool)]) -> Option<String> {
    let links: Vec<String> =
        entries.iter().filter(|(_, l)| *l).map(|(n, _)| format!("{}/", n.trim_end_matches('/'))).collect();
    entries.iter().map(|(n, _)| n).find(|n| links.iter().any(|l| n.starts_with(l.as_str()))).cloned()
}

/// Whether a link leads outside the unpack directory `dest_abs`: `link` is
/// the link path (under `dest_abs`), `target` where it leads. An absolute
/// target always leads outside (the theme does not know where it will be
/// installed); a relative one is resolved against the link's directory.
pub fn link_escapes(dest_abs: &str, link: &str, target: &str) -> bool {
    if target.starts_with('/') {
        return true;
    }
    let dir = link.rsplit_once('/').map(|(d, _)| d).unwrap_or(dest_abs);
    let resolved = normpath(&py_join(dir, target));
    !(resolved == dest_abs || resolved.starts_with(&format!("{dest_abs}/")))
}

/// After unpacking: a link that leads outside ([`link_escapes`]),
/// `(path, target)`. bsdtar puts such links as is, 7-Zip rewrites an
/// absolute one to point inside, p7zip as is; a theme with a link to
/// `/etc` or into HOME would travel to `~/.themes` together with it.
fn escaping_link_in(dest_abs: &str) -> Option<(String, String)> {
    fn walk(dest_abs: &str, dir: &Path, depth: usize) -> Option<(String, String)> {
        for entry in std::fs::read_dir(dir).ok()?.flatten() {
            let path = entry.path();
            let Ok(md) = std::fs::symlink_metadata(&path) else { continue };
            if md.file_type().is_symlink() {
                let target = std::fs::read_link(&path).map(|t| t.to_string_lossy().into_owned()).unwrap_or_default();
                let p = path.to_string_lossy().into_owned();
                if link_escapes(dest_abs, &p, &target) {
                    return Some((p, target));
                }
            } else if md.is_dir() && depth < 64 {
                if let Some(found) = walk(dest_abs, &path, depth + 1) {
                    return Some(found);
                }
            }
        }
        None
    }
    walk(dest_abs, Path::new(dest_abs), 0)
}

/// Whether a file can be unpacked at all: a port of the probe
/// `zipfile.is_zipfile(path) or tarfile.is_tarfile(path)` (plus 7z by
/// signature: even when there is nothing to unpack it with, it is an
/// archive, not a "bare theme file", and unpacking will say what is
/// missing).
fn is_archive_file(archive: &str) -> bool {
    zip_entry_names(archive).is_some() || tar_entry_names(archive).is_some() || is_seven_zip(archive)
}

/// Safely unpack zip/tar.* into `dest`: a live port of `_safe_extract`.
///
/// Every entry name goes through [`safe_extract_entry`] BEFORE extraction:
/// protection against "../" is not a place where two slightly different
/// checks are acceptable. ZIP is extracted by the common
/// [`crate::ext_utils::safe_zip_extract`] (it has its own second ZipSlip
/// check, by design), TAR by the system `tar` with `--no-same-owner`.
///
/// ⭐ Fixed during the port: on ANY failure the original passed up a
/// single "Could not unpack the archive" and put the real reason into the
/// log, which the customer does not look at. "A dangerous entry in the
/// archive", "no tar program" and "the file is corrupted" are three
/// different answers, and now they are different.
pub fn live_safe_extract(archive: &str, dest: &str) -> Result<(), String> {
    let result = live_safe_extract_any(archive, dest);
    // Permissions from the archive follow one rule for all formats: x bits
    // stay, setuid/setgid/sticky and write for group and others are stripped.
    if result.is_ok() {
        crate::ext_utils::clamp_extracted_modes(Path::new(dest));
    }
    result
}

/// Unpacking by format: the body of [`live_safe_extract`] without fixing permissions.
fn live_safe_extract_any(archive: &str, dest: &str) -> Result<(), String> {
    std::fs::create_dir_all(dest).map_err(|e| format!("{dest}: {e}"))?;
    let dest_abs = abs_dest(dest);

    if let Some(names) = zip_entry_names(archive) {
        for name in &names {
            if !safe_extract_entry(&dest_abs, name) {
                return Err(trf("unsafe entry in the archive: %1", &[name]));
            }
        }
        return crate::ext_utils::safe_zip_extract(Path::new(archive), Path::new(dest))
            .map_err(|e| e.to_string());
    }

    if let Some(entries) = seven_zip_entries(archive) {
        return seven_zip_extract(seven_zip_tool().unwrap_or("bsdtar"), &entries, archive, dest, &dest_abs);
    }

    // 7z, but nothing to unpack it with: say WHAT to install, not "unknown
    // format" (the bench of 17.10.2026 without bsdtar and 7-Zip).
    if is_seven_zip(archive) && !have_seven_zip_tool() {
        let family = crate::system::DistroDetector::detect_live().family_live();
        return Err(seven_zip_missing(family.as_str()));
    }
    live_safe_extract_tar(archive, dest, &dest_abs)
}

/// Unpack 7z with program `tool`: names (path traversal), entries through
/// an archive link, after unpacking, links leading outside.
fn seven_zip_extract(tool: &str, entries: &[(String, bool)], archive: &str, dest: &str, dest_abs: &str) -> Result<(), String> {
    for (name, _) in entries {
        if !safe_extract_entry(dest_abs, name) {
            return Err(trf("unsafe entry in the archive: %1", &[name]));
        }
    }
    // An entry through a link of the same archive is the same path traversal.
    if let Some(name) = entry_under_archive_link(entries) {
        return Err(trf("unsafe entry in the archive: %1", &[&name]));
    }
    let argv: Vec<String> = if tool == "bsdtar" {
        vec![tool.into(), "-xf".into(), archive.into(), "-C".into(), dest.into(), "--no-same-owner".into()]
    } else {
        vec![tool.into(), "x".into(), "-y".into(), format!("-o{dest}"), "--".into(), archive.into()]
    };
    let report = run_capture(&argv, Some(300));
    // A link leading outside is a refusal, like path traversal: 7-Zip also
    // exits with code 2 on such a link ("Dangerous link path was
    // ignored"), and the person cares about the reason, not the code.
    if let Some((link, target)) = escaping_link_in(dest_abs) {
        let rel = link.strip_prefix(&format!("{dest_abs}/")).unwrap_or(&link).to_string();
        return Err(trf("unsafe entry in the archive: %1", &[&format!("{rel} → {target}")]));
    }
    if report.returncode == Some(0) {
        return Ok(());
    }
    let out = crate::utils::decode_text(&report.stderr);
    let out = if out.trim().is_empty() { crate::utils::decode_text(&report.stdout) } else { out };
    let why = out.lines().filter(|l| l.contains("ERROR")).collect::<Vec<_>>().join("; ");
    Err(format!("{tool}: {}", if why.is_empty() { out.trim().to_string() } else { why }))
}

/// The tar branch of [`live_safe_extract`].
fn live_safe_extract_tar(archive: &str, dest: &str, dest_abs: &str) -> Result<(), String> {
    if !have("tar") {
        return Err(tr(
            "the tar program was not found, so there's nothing to unpack this archive \
             with. Install the tar package.",
        ));
    }
    let Some(names) = tar_entry_names(archive) else {
        return Err(trf("unknown archive format: %1", &[&archive]));
    };
    for name in &names {
        if !safe_extract_entry(dest_abs, name) {
            return Err(trf("unsafe entry in the archive: %1", &[name]));
        }
    }
    let argv = vec![
        "tar".to_string(),
        "-xf".to_string(),
        archive.to_string(),
        "-C".to_string(),
        dest.to_string(),
        "--no-same-owner".to_string(),
    ];
    let report = run_capture(&argv, Some(300));
    if report.returncode == Some(0) {
        return Ok(());
    }
    let err = crate::utils::decode_text(&report.stderr);
    let detail = err.trim();
    Err(if detail.is_empty() {
        tr("tar couldn't unpack the archive")
    } else {
        format!("tar: {detail}")
    })
}

// ── apply plan executor (`crate::themes::ApplyStep`) ────────────────────────

/// The live filesystem for the GTK/KDE sanitizer.
struct LiveGtkFs;

impl crate::gtk_sanitizer::GtkFs for LiveGtkFs {
    fn is_dir(&mut self, path: &str) -> bool {
        Path::new(path).is_dir()
    }
    fn is_file(&mut self, path: &str) -> bool {
        Path::new(path).is_file()
    }
    fn read_text(&mut self, path: &str) -> Option<String> {
        read_text_lossy(path)
    }
}

/// CRITICAL after ANY application: repair broken references in
/// `gtk-{3,4}.0/settings.ini` and in KDE configs: a port of `sanitize_all`.
///
/// Without this GTK4 applications loop looking for icons of a theme that
/// is no longer on disk. The decisions (what is broken and what to replace
/// it with) belong entirely to [`crate::gtk_sanitizer`]; here only
/// reading, writing and `kwriteconfig6`. Returns the list of edits; their
/// number goes into [`gtk_sanitizer_message`].
pub fn live_gtk_sanitize(home: &str) -> Vec<String> {
    use crate::gtk_sanitizer as gsan;

    let cfg = crate::utils::config_home(xdg_config_home().as_deref(), home);
    let data = crate::utils::data_home(xdg_data_home().as_deref(), home);
    let mut fs = LiveGtkFs;
    let mut changes = gtk_ini_sanitize_at(home, &cfg, &data);

    let kde = gsan::read_kde_ini(read_text_lossy(&format!("{cfg}/kdeglobals")).as_deref());
    let kcm = gsan::read_kcminput_cursor(read_text_lossy(&format!("{cfg}/kcminputrc")).as_deref());
    let edits = gsan::plan_kde_edits(&mut fs, &kde, &kcm, home, &data);
    if !edits.is_empty() {
        let Some(tool) = crate::utils::resolve_tool("kwriteconfig6", &[], path_env().as_deref())
        else {
            return changes;
        };
        for edit in edits {
            if run_ok(&gsan::kwriteconfig_argv(&tool, &edit), None) {
                changes.push(edit.change);
            }
        }
    }
    changes
}

/// Repairing `gtk-{3,4}.0/settings.ini` with explicit directories: the half
/// of [`live_gtk_sanitize`] that does not run other programs; parity checks
/// call it on a fake HOME.
pub fn gtk_ini_sanitize_at(home: &str, cfg: &str, data: &str) -> Vec<String> {
    use crate::gtk_sanitizer as gsan;
    let mut fs = LiveGtkFs;
    let mut changes: Vec<String> = Vec::new();
    for ver in gsan::GTK_VERSIONS {
        let ini = gsan::gtk_ini_path(cfg, ver);
        if !Path::new(&ini).is_file() {
            continue;
        }
        let Some(text) = read_text_lossy(&ini) else {
            // The file exists but cannot be read: rewriting it is NOT allowed:
            // rebuilding the text without the source would wipe all other keys.
            continue;
        };
        let settings = gsan::read_ini_settings(Some(&text));
        for edit in gsan::plan_gtk_ini_edits(&mut fs, &ini, &settings, home, data) {
            // The file is reread for every key, as in the original.
            let existing = read_text_lossy(&ini);
            let content = gsan::write_ini_setting(existing.as_deref(), &edit.key, &edit.value);
            if std::fs::write(&ini, content).is_ok() {
                changes.push(edit.change);
            }
        }
    }
    changes
}

/// The sanitizer's remarks about references it does NOT fix: a sound theme
/// from `gtk-{3,4}.0/settings.ini` that is not on disk
/// ([`crate::gtk_sanitizer::plan_gtk_ini_notes`]). Read only: not a single
/// file changes; the lines go to the log at window startup.
pub fn live_gtk_notes(home: &str) -> Vec<String> {
    let cfg = crate::utils::config_home(xdg_config_home().as_deref(), home);
    let data = crate::utils::data_home(xdg_data_home().as_deref(), home);
    gtk_notes_at(home, &cfg, &data)
}

/// The body of [`live_gtk_notes`] with explicit directories: parity checks
/// call it on a fake HOME without touching the process environment.
pub fn gtk_notes_at(home: &str, cfg: &str, data: &str) -> Vec<String> {
    use crate::gtk_sanitizer as gsan;
    let mut fs = LiveGtkFs;
    let mut notes = Vec::new();
    for ver in gsan::GTK_VERSIONS {
        let ini = gsan::gtk_ini_path(cfg, ver);
        let Some(text) = read_text_lossy(&ini) else { continue };
        let settings = gsan::read_ini_settings(Some(&text));
        notes.extend(gsan::plan_gtk_ini_notes(&mut fs, &ini, &settings, home, data));
    }
    notes
}

/// Read the cursor size (`gsettings get …cursor-size`), default "24".
fn cursor_size(run: &dyn Fn(&[String], Option<u64>) -> RunReport) -> String {
    let cmd = crate::themes::cursor_size_get_cmd();
    let report = run(&cmd.argv, cmd.timeout);
    if report.returncode == Some(0) {
        let raw = crate::utils::decode_text(&report.stdout);
        let value = raw.trim().trim_matches(|c| c == '\'' || c == '"').trim();
        if !value.is_empty() {
            return value.to_string();
        }
    }
    "24".to_string()
}

/// The environment for choosing a Qt style (Kvantum and "Application
/// style" share one): a working Qt style switcher is looked for in `PATH`;
/// if empty, we take the `QT_STYLE_OVERRIDE` fallback path.
fn qt_style_env<'a>(
    de: &'a str,
    home: &'a str,
    xdg: Option<&'a str>,
) -> crate::themes::KvantumEnv<'a> {
    let platform_theme = ["qt6ct", "qt5ct"].into_iter().find(|t| have(t)).unwrap_or("");
    // The environment's current icons, for a new qt6ct config (D2). Read only
    // when this config is really going to be needed.
    let icon_theme = if !platform_theme.is_empty()
        && crate::themes::qt_style_way(de) == crate::themes::QtStyleWay::PlatformTheme
    {
        current_icon_theme(de, home, xdg)
    } else {
        String::new()
    };
    crate::themes::KvantumEnv {
        de_id: de,
        home,
        xdg_config_home: xdg.unwrap_or(""),
        platform_theme,
        icon_theme,
    }
}

/// The icon theme the environment gives programs now (empty means it could
/// not be read). The core decides the source.
fn current_icon_theme(de: &str, home: &str, xdg: Option<&str>) -> String {
    use crate::themes::IconThemeSource as S;
    match crate::themes::current_icon_theme_source(de, home, xdg) {
        S::Cmd(c) => {
            if !have(&c.argv[0]) {
                return String::new();
            }
            let r = run_capture(&c.argv, c.timeout);
            if r.returncode != Some(0) {
                return String::new();
            }
            crate::themes::parse_icon_theme_output(&crate::utils::decode_text(&r.stdout))
        }
        S::GtkIni(path) => crate::gtk_sanitizer::read_ini_settings(read_text_lossy(&path).as_deref())
            .get("gtk-icon-theme-name")
            .cloned()
            .unwrap_or_default(),
    }
}

/// Execute a theme apply plan: an exact port of `ThemeManager._exec_apply_plan`.
///
/// The `ok` flag follows the core's tag semantics strictly: `Primary` sets
/// it, a successful `IfNotOk`, a successful `WriteSetsOk` write and the
/// unconditional `ForceOk` raise it; `BestEffort` does not touch it. The
/// order and argv come from the plan; there are no decisions of its own
/// here.
pub fn exec_apply_plan(steps: &[ApplyStep]) -> bool {
    exec_apply_plan_report(steps).ok
}

/// The same as [`exec_apply_plan`], but with details for an honest cursor
/// message: whether it changed right away and what happened to the
/// compositor config.
pub fn exec_apply_plan_report(steps: &[ApplyStep]) -> crate::themes::CursorReport {
    exec_apply_plan_report_in(steps, crate::dde::treeland_session_live())
}

/// The same with an explicit Treeland session flag (the stop reason of
/// [`ApplyStep::DdeTreelandGate`]).
fn exec_apply_plan_report_in(steps: &[ApplyStep], treeland: bool) -> crate::themes::CursorReport {
    let plan: Vec<crate::install_plan::Step> = steps.iter().cloned().map(Into::into).collect();
    let ctx = crate::install_plan::Ctx::live(true, false, None, treeland);
    match crate::install_plan::exec(&plan, &mut Vec::new(), &ctx) {
        Ok(report) => report.apply,
        // A write failed: the executor has already returned everything the press
        // managed to write; the person gets the reason.
        Err(f) => press_failed_report(&f),
    }
}

/// The result of an "Apply" press cut off by a file write failure: not
/// applied, the reason is which file was not written (the press's writes
/// have already been returned by the common executor).
pub(crate) fn press_failed_report(f: &crate::install_plan::Failure) -> crate::themes::CursorReport {
    crate::themes::CursorReport {
        ok: false,
        reason: Some(trf(
            "Not applied: %1. The files this action had changed are back as they were.",
            &[&crate::install_plan::failure_text(f)],
        )),
        ..Default::default()
    }
}

/// Running the enabling layer of the common plan
/// ([`crate::install_plan::exec`]): the `ok` flag follows the core's tag
/// semantics strictly: `Primary` sets it, a successful `IfNotOk`, a
/// successful `WriteSetsOk` write and the unconditional `ForceOk` raise
/// it; `BestEffort` does not touch it. The order and argv come from the
/// plan; there are no decisions of its own here.
pub(crate) struct ApplyRun<'r> {
    report: crate::themes::CursorReport,
    ok: bool,
    /// Whether anything was written to `environment.d`: then the variables
    /// must be mirrored into sessions that do not read this directory (D1).
    env_written: bool,
    /// Whether a Treeland session is running (the stop reason of [`ApplyStep::DdeTreelandGate`]).
    treeland: bool,
    /// Running commands and finding programs are shared by the whole plan
    /// ([`crate::install_plan::Ctx::run`], [`crate::install_plan::Ctx::have`]).
    run: &'r dyn Fn(&[String], Option<u64>) -> RunReport,
    have: &'r dyn Fn(&str) -> bool,
    /// Mirror `environment.d` ([`crate::install_plan::Ctx::sync_env`]).
    sync_env: &'r dyn Fn(&mut crate::install_plan::Undo),
    /// Files under [`ApplyStep::Guard`] that did not exist before the
    /// commands: after the run, what the commands created is marked in the
    /// rollback ([`Self::settle`]).
    guarded: Vec<(PathBuf, PathBuf)>,
}

impl<'r> ApplyRun<'r> {
    pub(crate) fn new(
        treeland: bool,
        run: &'r dyn Fn(&[String], Option<u64>) -> RunReport,
        have: &'r dyn Fn(&str) -> bool,
        sync_env: &'r dyn Fn(&mut crate::install_plan::Undo),
    ) -> Self {
        ApplyRun { report: Default::default(), ok: false, env_written: false, treeland, run, have, sync_env, guarded: Vec::new() }
    }

    fn ok_run(&self, argv: &[String], timeout_secs: Option<u64>) -> bool {
        (self.run)(argv, timeout_secs).returncode == Some(0)
    }

    /// Execute a step; `Ok(false)` means the plan is stopped (Treeland).
    ///
    /// File writes (ini, our files, the compositor config) go under the `undo`
    /// rollback (step 5a of merging the installers): a failed write is `Err`,
    /// and the common executor returns everything this press managed to write
    /// ("all or nothing" within one press). Previously a write error was
    /// silently skipped, and the press left the configs half done.
    pub(crate) fn step(
        &mut self,
        step: &ApplyStep,
        undo: &mut crate::install_plan::Undo,
    ) -> Result<bool, crate::install_plan::Failure> {
        use crate::install_plan::Failure;
        fn write(path: &str) -> impl FnOnce(std::io::Error) -> Failure + '_ {
            move |e| Failure::Write { path: path.to_string(), err: e.to_string() }
        }
        fn mkdir(path: &str) -> impl FnOnce(std::io::Error) -> Failure + '_ {
            move |e| Failure::MakeDirs { path: path.to_string(), err: e.to_string() }
        }
        match step {
            ApplyStep::DdeTreelandGate => {
                self.report.reason = Some(crate::dde::refusal(self.treeland));
                self.ok = false;
                return Ok(false);
            }
            // Commands that change desktop values (`gsettings`, `dconf`,
            // `xfconf-query`) go through the rollback: the previous value is taken
            // before them ([`crate::install_plan::Undo::run_kept`]).
            ApplyStep::Primary(c) => self.ok = undo.run_kept(self.run, &c.argv, c.timeout).returncode == Some(0),
            ApplyStep::BestEffort(c) => {
                let _ = undo.run_kept(self.run, &c.argv, c.timeout);
            }
            ApplyStep::IfOk(c) => {
                if self.ok {
                    let _ = undo.run_kept(self.run, &c.argv, c.timeout);
                }
            }
            ApplyStep::IfNotOk(c) => {
                if !self.ok && undo.run_kept(self.run, &c.argv, c.timeout).returncode == Some(0) {
                    self.ok = true;
                }
            }
            ApplyStep::GtkIni { path, key, value } => {
                let existing = if Path::new(path).is_file() { read_text_lossy(path) } else { None };
                let content = crate::themes::update_gtk_settings_ini(existing.as_deref(), key, value);
                undo.write_text(Path::new(path), &content, false).map_err(write(path))?;
            }
            ApplyStep::HyprctlCursor { theme, size } => {
                if (self.have)("hyprctl") {
                    let size = size.clone().unwrap_or_else(|| cursor_size(self.run));
                    let [set, env] = crate::themes::hyprctl_cursor_cmds(theme, &size);
                    let live = self.ok_run(&set.argv, set.timeout);
                    let _ = self.ok_run(&env.argv, env.timeout);
                    let size_env = vec![
                        "hyprctl".to_string(),
                        "keyword".to_string(),
                        "env".to_string(),
                        format!("XCURSOR_SIZE,{size}"),
                    ];
                    let _ = self.ok_run(&size_env, None);
                    self.report.live = Some(self.report.live.unwrap_or(false) || live);
                } else {
                    self.report.live.get_or_insert(false);
                }
            }
            ApplyStep::Live(c) => {
                let live = (self.have)(&c.argv[0]) && self.ok_run(&c.argv, c.timeout);
                self.report.live = Some(self.report.live.unwrap_or(false) || live);
            }
            ApplyStep::ForceOk => self.ok = true,
            ApplyStep::WriteIfOk { dir, path, content } => {
                if self.ok {
                    undo.make_dirs(Path::new(dir)).map_err(mkdir(dir))?;
                    undo.write_text(Path::new(path), content, false).map_err(write(path))?;
                    self.env_written |= crate::compositor_autostart::is_environment_d_path(path);
                }
            }
            ApplyStep::WriteSetsOk { dir, path, content } => {
                undo.make_dirs(Path::new(dir)).map_err(mkdir(dir))?;
                undo.write_text(Path::new(path), content, true).map_err(write(path))?;
                self.ok = true;
                self.env_written |= crate::compositor_autostart::is_environment_d_path(path);
            }
            ApplyStep::IniSetIfOk { path, section, key, value } => {
                if self.ok {
                    // We do not own the file: editing one key, not rewriting it.
                    let existing = if Path::new(path).exists() { read_text_lossy(path) } else { None };
                    let content = crate::defaults::set_ini_value(existing.as_deref(), section, key, value);
                    undo.write_text(Path::new(path), &content, false).map_err(write(path))?;
                }
            }
            ApplyStep::IniSetGuarded { path, section, key, value, only_if_file, keep_existing } => {
                if self.ok {
                    let existing = if Path::new(path).exists() { read_text_lossy(path) } else { None };
                    if let Some(text) = crate::themes::ini_guarded_text(
                        existing.as_deref(),
                        section,
                        key,
                        value,
                        *only_if_file,
                        *keep_existing,
                    ) {
                        undo.write_text(Path::new(path), &text, false).map_err(write(path))?;
                    }
                }
            }
            ApplyStep::WmConfig(plan) => {
                let (outcome, path) = exec_wm_config(plan, self.run, self.have, undo);
                self.report.wm = Some((plan.wm, outcome, path));
            }
            ApplyStep::GtkIniUnsetIfOk { path, key } => {
                if self.ok {
                    if let Some(text) = read_text_lossy(path)
                        .and_then(|t| crate::themes::remove_gtk_settings_key(&t, key))
                    {
                        undo.write_text(Path::new(path), &text, true).map_err(write(path))?;
                    }
                }
            }
            ApplyStep::RemoveIfOk { path } => {
                if self.ok {
                    undo.remove_file(Path::new(path))
                        .map_err(|e| Failure::Remove { path: path.clone(), err: e.to_string() })?;
                }
            }
            ApplyStep::Guard { paths } => {
                let paths: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();
                let missing = undo.guard_external(&paths);
                self.guarded.extend(missing);
            }
            ApplyStep::ReadBackSetsOk { read, expect } => {
                let report = (self.run)(&read.argv, read.timeout);
                let raw = if report.returncode == Some(0) {
                    crate::utils::decode_text(&report.stdout)
                } else {
                    String::new()
                };
                self.ok = report.returncode == Some(0) && crate::qt_settings::parse_sound_theme(&raw) == *expect;
            }
        }
        Ok(true)
    }

    /// Mark in the rollback the files created by commands after
    /// [`ApplyStep::Guard`] (also called on a failure in the middle of the
    /// plan; then the rollback removes them too).
    pub(crate) fn settle(&mut self, undo: &mut crate::install_plan::Undo) {
        undo.after_external(std::mem::take(&mut self.guarded));
    }

    pub(crate) fn finish(mut self, undo: &mut crate::install_plan::Undo) -> crate::themes::CursorReport {
        self.settle(undo);
        if self.env_written {
            (self.sync_env)(undo);
        }
        self.report.ok = self.ok;
        self.report
    }
}

/// Execute [`crate::themes::WmConfigPlan`]: our file + a managed block in
/// the compositor config, with a copy before the first edit and a rollback
/// if the compositor's own check rejected the result. Returns the result
/// and the config path.
///
/// The writes are under the press's `undo` rollback: a failure LATER in
/// the same press returns them too. `Failed` itself does not break the
/// press (the result is reported to the person separately,
/// [`crate::themes::CursorReport::wm`]).
fn exec_wm_config(
    plan: &crate::themes::WmConfigPlan,
    run: &dyn Fn(&[String], Option<u64>) -> RunReport,
    have: &dyn Fn(&str) -> bool,
    undo: &mut crate::install_plan::Undo,
) -> (crate::themes::WmConfigOutcome, String) {
    use crate::themes::WmConfigOutcome as O;
    let first_path = plan.candidates.first().map(|c| c.path.clone()).unwrap_or_default();
    // 1. The version: an old compositor will not read the include line.
    if let Some(gate) = &plan.min_version {
        if have(&gate.argv[0]) {
            let out = run(&gate.argv, Some(10));
            let text = crate::utils::decode_text(&out.stdout);
            if let Some(v) = crate::themes::parse_wm_version(&text) {
                if v < gate.min {
                    return (O::TooOld, first_path);
                }
            }
        }
    }
    // 2. Which config the compositor will read.
    let mut chosen = plan.candidates.iter().find(|c| Path::new(&c.path).is_file());
    if chosen.is_none() {
        if let Some(f) = plan.foreign.iter().find(|f| Path::new(f.as_str()).is_file()) {
            return (O::Foreign, f.clone());
        }
        match &plan.seed {
            Some(seed) if Path::new(&seed.from).is_file() => {
                let Some(target) = plan.candidates.get(seed.candidate) else {
                    return (O::NoConfig, first_path);
                };
                let Some(text) = read_text_lossy(&seed.from) else {
                    return (O::Failed, target.path.clone());
                };
                if undo.write_text(Path::new(&target.path), &text, true).is_err() {
                    return (O::Failed, target.path.clone());
                }
                chosen = Some(target);
            }
            _ => return (O::NoConfig, first_path),
        }
    }
    let Some(file) = chosen else {
        return (O::NoConfig, first_path);
    };
    let path = file.path.clone();
    // 3. Our file, then the include line.
    if undo.write_text(Path::new(&file.own_path), &file.own_content, true).is_err() {
        return (O::Failed, path);
    }
    let Some(before) = read_text_lossy(&path) else {
        return (O::Failed, path);
    };
    let after = crate::themes::upsert_managed_block(
        &before,
        file.syntax.comment(),
        crate::themes::CURSOR_BLOCK_TAG,
        &file.include_line,
    );
    if after == before {
        return (O::Written, path);
    }
    let backup = format!("{path}.rexon-bak");
    if !Path::new(&backup).exists() && undo.write_text(Path::new(&backup), &before, true).is_err() {
        return (O::Failed, path);
    }
    // 4. A check by the compositor itself, before and after.
    let validator = plan.validate.first().filter(|bin| have(bin)).is_some();
    let valid = |p: &str| -> bool {
        let mut argv = plan.validate.clone();
        argv.push(p.to_string());
        run(&argv, Some(30)).returncode == Some(0)
    };
    let was_valid = validator && valid(&path);
    if undo.write_text(Path::new(&path), &after, true).is_err() {
        return (O::Failed, path);
    }
    if was_valid && !valid(&path) {
        // A copy of the previous one is already taken by the rollback: a plain write here.
        let _ = crate::utils::atomic_write(&path, &before);
        return (O::RolledBack, path);
    }
    (O::Written, path)
}

/// The current theme (`size == false`) or cursor size in this environment:
/// the first answer from the core's list of sources
/// ([`crate::themes::cursor_queries`]).
pub(crate) fn live_cursor_value(de: &str, home: &str, xdg: Option<&str>, size: bool) -> Option<String> {
    use crate::themes::CursorQuery as Q;
    for q in crate::themes::cursor_queries(de, home, xdg, size) {
        let value = match q {
            Q::Cmd(c) => {
                if !have(&c.argv[0]) {
                    continue;
                }
                let out = run_capture(&c.argv, c.timeout);
                if out.returncode != Some(0) {
                    continue;
                }
                crate::themes::clean_cursor_value(&crate::utils::decode_text(&out.stdout))
            }
            Q::Ini { path, section, key } => read_text_lossy(&path)
                .and_then(|t| crate::themes::ini_key_value(&t, &section, &key)),
            Q::EnvFile { path, key } => read_text_lossy(&path)
                .and_then(|t| crate::themes::env_file_key_value(&t, &key)),
            Q::Env(name) => std::env::var(&name).ok().and_then(|v| crate::themes::clean_cursor_value(&v)),
        };
        let value = match value {
            Some(v) if size && crate::themes::parse_cursor_size(&v).is_none() => None,
            v => v,
        };
        if value.is_some() {
            return value;
        }
    }
    None
}

/// argv for "KWin, reread the config" (a port of `utils.kwin_reconfigure`)
/// with the first bus tool that exists. `None` means there is none.
///
/// We go to the session bus rather than to the `qdbus`/`qdbus6` list from
/// [`aurorae_plan`]: those binaries do not exist on most systems at all,
/// while `dbus-send` exists wherever the bus itself does. The order of
/// tools and the argv building are shared ([`crate::utils`]); there is no
/// copy of its own here.
fn kwin_reconfigure_argv() -> Option<Vec<String>> {
    let path = path_env();
    crate::utils::DBUS_TOOL_PREFERENCE.iter().find_map(|(tool, kind)| {
        let exe = crate::utils::resolve_tool(tool, &[], path.as_deref())?;
        let (dest, obj, method) = crate::utils::KWIN_RECONFIGURE;
        Some(crate::utils::dbus_call_argv(*kind, &exe, dest, obj, method, &[]))
    })
}

// ── The plan of pressing "Apply" (step 5a) ──────────────────────────────────

/// The plan of one "Apply" press.
#[derive(Debug, Clone, PartialEq)]
pub enum PressPlan {
    /// Steps of the common plan ([`crate::install_plan::exec`]); on success, a
    /// theme history entry `(kind, desktop)` if the action has one.
    Steps {
        steps: Vec<crate::install_plan::Step>,
        history: Option<(&'static str, String)>,
        /// What to tell the person after the run.
        say: PressSay,
    },
    /// An environment section button (a theme, an add-on): steps of the common
    /// plan, commands through the substitutable `de_apply` machine; the result
    /// is not the command codes but the check "what the environment returns
    /// after the write" ([`crate::de_apply::check`]).
    Desktop { steps: Vec<crate::install_plan::Step>, check: crate::de_apply::Check },
    /// A refusal before the first step: nothing is launched or written.
    /// `None` means the common "not applied" ([`apply_message`]).
    Refused(Option<String>),
    /// Nothing to do: success (fonts, plasmoids…).
    Nothing,
    /// The press is still executed its own way, not by the common plan:
    /// colours in Plasma (`plasma-apply-colorscheme`, then removing our key
    /// from `kdeglobals`, whose text is known only AFTER the command), the
    /// desktop wallpaper, Conky (starting processes, `de_apply`), the login and
    /// boot screens (the root helper).
    Own,
}

/// The result text of a press built by a plan ([`PressPlan::Steps`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PressSay {
    /// The common "applied"/"not applied" ([`apply_message`]); on failure, the
    /// reason if it is known.
    Plain,
    /// The cursor: what changed right away and what happened to the compositor
    /// config ([`crate::themes::cursor_apply_message`]).
    Cursor,
    /// On success this text (colours via Rexon layers: what Qt programs got,
    /// [`crate::shell_palette::layer_applied_msg`]); on failure, the reason.
    Done(String),
    /// On success `done`; on failure the reason from the run (a write failure),
    /// and without one `failed` (lock screen wallpaper, sound without a check
    /// command).
    Pair { done: String, failed: String },
}

/// Live facts for a sound theme ([`PressFacts::sound`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SoundFacts {
    /// A theme with this name exists in the sound theme directories.
    pub exists: bool,
    /// The `kwriteconfig6`/`kreadconfig6` found (empty means none).
    pub kwrite: String,
    pub kread: String,
    pub has_gsettings: bool,
}

/// Live facts for the lock screen wallpaper ([`PressFacts::lock`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LockFacts {
    /// The section directory (the picture lies there).
    pub install_dir: String,
    /// The picture `install_dir/<theme>` is a file.
    pub is_file: bool,
    /// A Treeland session (Deepin: the Control Center provides the wallpaper;
    /// outside Treeland Deepin has no lock screen wallpaper).
    pub treeland: bool,
}

/// Files that the `kwriteconfig` commands from `cmds` will rewrite
/// (`--file name` -> `<config>/name`), once each, in command order. Their
/// "what was there" is taken by [`crate::themes::ApplyStep::Guard`]: the
/// commands themselves bypass the rollback.
pub fn kconfig_files<'a>(cmds: impl IntoIterator<Item = &'a [String]>, config: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for argv in cmds {
        let is_kwrite = argv.first().is_some_and(|a| basename(a).starts_with("kwriteconfig"));
        let file = argv.iter().position(|a| a == "--file").and_then(|i| argv.get(i + 1));
        if let (true, Some(file)) = (is_kwrite, file) {
            let path = if file.starts_with('/') { file.clone() } else { format!("{}/{file}", config.trim_end_matches('/')) };
            if !out.contains(&path) {
                out.push(path);
            }
        }
    }
    out
}

/// Live facts for [`press_plan_with`]: everything that requires querying
/// the machine is captured in advance ([`LiveInstaller::press_plan`]); in
/// checks it is fake.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PressFacts {
    pub de: String,
    pub home: String,
    pub xdg_config_home: Option<String>,
    /// The global theme has `metadata.json` (Plasma 6 will read it).
    pub global_theme_ok: bool,
    /// The icon theme is valid ([`crate::themes::is_valid_icon_theme`]).
    pub icons_valid: bool,
    /// The GTK theme is dark ([`crate::themes::gtk_theme_is_dark`]).
    pub gtk_dark: bool,
    /// The GTK theme in GTK 4/libadwaita programs ([`crate::gtk4_theme`]);
    /// `None` means not gathered, the plan is as before.
    pub gtk4: Option<crate::gtk4_theme::Facts>,
    /// The Qt style switcher in the system (`qt6ct`/`qt5ct`, empty means none).
    pub qt_platform_theme: String,
    /// The environment's current icon theme, for a new qt6ct config.
    pub qt_icon_theme: String,
    /// argv for "KWin, reread the config"; `None` means nothing to call it with.
    pub kwin_reconfigure: Option<Vec<String>>,
    /// The `library` of an Aurorae SVG theme on this machine
    /// ([`crate::kdecoration::svg_library`]); empty means the old engine.
    pub aurorae_library: String,
    /// The cursor theme is safe (`Inherits` does not lead nowhere, a port of
    /// `_cursor_preflight`).
    pub cursor_safe: bool,
    /// The cursor theme is the environment's default cursor (Adwaita in GNOME
    /// and Cinnamon): an unsafe one of this kind is not a refusal but a reset
    /// of the native key.
    pub cursor_standard: bool,
    /// The cursor size: when choosing a theme, the current one (it is needed
    /// in full: environment variables, compositor config), when changing the
    /// size, the new one.
    pub cursor_size: Option<u32>,
    /// Colours outside Plasma: the plan of Rexon layers
    /// ([`crate::shell_palette::scheme_layer_press`]); `Err` is a refusal (no
    /// scheme), `None` means not gathered.
    pub colors: Option<Result<crate::shell_palette::SchemePress, String>>,
    /// The sound theme: whether it exists and what to write it with; `None` means not gathered.
    pub sound: Option<SoundFacts>,
    /// Lock screen wallpaper: where the picture is; `None` means not gathered.
    pub lock: Option<LockFacts>,
    /// An environment section: the button plan ([`crate::de_apply::press_plan`]);
    /// `None` means not gathered (its own way).
    pub desktop: Option<crate::de_apply::Plan>,
    /// A global theme: what the person ticked in the "Apply "X"?" window and
    /// what the theme carries; `None` means there was no window, the look is
    /// applied (`plasma-apply-lookandfeel -a`, as before).
    pub global: Option<crate::themes::GlobalPress>,
}

/// The plan of pressing "Apply" for action `action` on theme `theme`: a
/// pure function: the builders are the same ones the old `apply` called,
/// the order and argv did not change (checked by
/// `appearance_apply_plans_parity.rs`).
pub fn press_plan_with(action: &ApplyAction, theme: &str, facts: &PressFacts) -> PressPlan {
    use crate::install_plan::Step;
    use crate::themes::Cmd;
    let de = facts.de.as_str();
    let home = facts.home.as_str();
    let xdg = facts.xdg_config_home.as_deref();
    let qt_env = || crate::themes::KvantumEnv {
        de_id: de,
        home,
        xdg_config_home: xdg.unwrap_or(""),
        platform_theme: &facts.qt_platform_theme,
        icon_theme: facts.qt_icon_theme.clone(),
    };
    let steps = |plan: Vec<crate::themes::ApplyStep>| plan.into_iter().map(Step::from).collect::<Vec<_>>();
    let with_history = |plan: crate::themes::Plan, de: &str| PressPlan::Steps {
        steps: steps(plan.steps),
        history: Some((plan.history_kind, de.to_string())),
        say: PressSay::Plain,
    };
    let primary = |argv: Vec<String>| Step::from(crate::themes::ApplyStep::Primary(Cmd { argv, timeout: None }));
    match action {
        ApplyAction::KdeGlobal => {
            if !facts.global_theme_ok {
                return PressPlan::Refused(Some(trf(
                    "The theme “%1” uses the old Plasma 5 format (metadata.desktop \
                     only, no metadata.json) — Plasma 6 can't apply it. You need a \
                     version made for Plasma 6.",
                    &[&theme],
                )));
            }
            let Some(press) = &facts.global else {
                return with_history(crate::themes::apply_kde_global_plan(theme), "kde");
            };
            // Parts ticked in the "Apply "X"?" window (outside Plasma, those that
            // carry over to this desktop).
            let Some(got) = crate::themes::apply_global_choice_plan(theme, press, de) else {
                return PressPlan::Refused(Some(tr("Nothing is selected to apply.")));
            };
            let skipped = got.skipped.join("; ");
            if got.plan.steps.is_empty() {
                return PressPlan::Refused(Some(if skipped.is_empty() {
                    tr("Nothing is selected to apply.")
                } else {
                    trf("Nothing from the Global Theme was applied: %1.", &[&skipped])
                }));
            }
            match with_history(got.plan, de) {
                PressPlan::Steps { steps, history, .. } if !skipped.is_empty() => PressPlan::Steps {
                    steps,
                    history,
                    say: PressSay::Pair {
                        done: trf("Global Theme applied, except: %1.", &[&skipped]),
                        failed: apply_message(action, false),
                    },
                },
                other => other,
            }
        }
        ApplyAction::KdePlasma => with_history(crate::themes::apply_kde_plasma_plan(theme), "kde"),
        ApplyAction::Icons => {
            match crate::themes::apply_icon_plan_xdg(theme, de, home, xdg, facts.icons_valid) {
                Some(plan) => with_history(plan, de),
                None => PressPlan::Refused(None),
            }
        }
        ApplyAction::Kvantum => with_history(crate::themes::apply_kvantum_plan(theme, &qt_env()), "kde"),
        ApplyAction::Gtk => {
            let plan = with_history(crate::themes::apply_gtk_plan_scheme(theme, de, home, xdg, Some(facts.gtk_dark)), de);
            match (plan, &facts.gtk4) {
                (PressPlan::Steps { mut steps, history, say }, Some(gtk4)) => {
                    crate::gtk4_theme::extend_press(&mut steps, theme, de, gtk4);
                    PressPlan::Steps { steps, history, say }
                }
                (plan, _) => plan,
            }
        }
        // The decoration module (`library`), then the theme, then (only on
        // success) KWin rereads the config. Without `library` an Aurorae theme
        // did not switch on under Breeze: KWin draws the module from `library`,
        // and it is the module that reads `theme`. A built-in decoration tile
        // (Breeze, Oxygen) is `library/theme` ([`crate::kdecoration::choice`]).
        // Frames, the Qt style and splashes have no history, as before.
        ApplyAction::Aurorae => {
            let library = if facts.aurorae_library.is_empty() {
                crate::kdecoration::AURORAE_V1
            } else {
                facts.aurorae_library.as_str()
            };
            let [write_library, write_theme] = crate::kdecoration::write_argv(theme, library);
            let mut plan = vec![
                primary(write_library),
                Step::from(crate::themes::ApplyStep::IfOk(Cmd { argv: write_theme, timeout: None })),
            ];
            if let Some(argv) = facts.kwin_reconfigure.clone() {
                plan.push(Step::from(crate::themes::ApplyStep::IfOk(Cmd { argv, timeout: Some(10) })));
            }
            PressPlan::Steps { steps: plan, history: None, say: PressSay::Plain }
        }
        ApplyAction::WidgetStyle => PressPlan::Steps {
            steps: steps(crate::themes::apply_qt_style_plan(theme, &qt_env()).steps),
            history: None,
            say: PressSay::Plain,
        },
        // The splash, like the KDE module: the theme and right away `Engine`
        // ("None" gets `none`, otherwise `KSplashQML`, so that the splash comes
        // back after "None").
        ApplyAction::Splash => PressPlan::Steps {
            steps: vec![
                primary(lnf_component_plan("splash", theme).argv),
                Step::from(crate::themes::ApplyStep::IfOk(Cmd { argv: splash_engine_plan(theme).argv, timeout: None })),
            ],
            history: None,
            say: if theme == SPLASH_NONE { PressSay::Done(splash_off_message()) } else { PressSay::Plain },
        },
        ApplyAction::Lockscreen => PressPlan::Steps {
            steps: vec![primary(lnf_component_plan("lockscreen", theme).argv)],
            history: None,
            say: PressSay::Plain,
        },
        ApplyAction::Noop => PressPlan::Nothing,
        // An unsafe theme that is the environment's default cursor: a reset of
        // the native key (the theme name is not written anywhere explicitly).
        // Otherwise for an unsafe theme no plan is built: a refusal without a
        // single write.
        ApplyAction::Cursor => {
            let size = facts.cursor_size;
            let standard = (!facts.cursor_safe && facts.cursor_standard)
                .then(|| crate::themes::standard_cursor_plan(de, size, home, xdg))
                .flatten();
            match standard
                .or_else(|| crate::themes::apply_cursor_plan_sized(theme, size, de, home, xdg, facts.cursor_safe))
            {
                Some(plan) => PressPlan::Steps {
                    steps: steps(plan.steps),
                    history: Some((plan.history_kind, de.to_string())),
                    say: PressSay::Cursor,
                },
                None => PressPlan::Refused(None),
            }
        }
        // Outside Plasma, Rexon layers via the common plan; in Plasma, its own way.
        ApplyAction::KdeColors => match crate::shell_palette::scheme_way(de) {
            crate::shell_palette::SchemeWay::Layer { .. } => match &facts.colors {
                Some(Ok(press)) => PressPlan::Steps {
                    steps: steps(press.steps.clone()),
                    history: Some(("kde-colors", de.to_string())),
                    say: PressSay::Done(press.applied_msg()),
                },
                Some(Err(m)) => PressPlan::Refused(Some(m.clone())),
                None => PressPlan::Refused(None),
            },
            _ => PressPlan::Own,
        },
        // A sound theme goes through the common sound engine (the same as in
        // the "Settings" tab): writes by commands, then the name read back. A
        // nonexistent name is not written at all: the system would silently
        // play `freedesktop`, while the tab would say "applied" (a live case on
        // 03.10: kdeglobals named the theme "Sounds", which does not exist).
        ApplyAction::SoundTheme => {
            let Some(sf) = &facts.sound else { return PressPlan::Refused(None) };
            if !sf.exists {
                return PressPlan::Refused(Some(crate::qt_settings::sound_theme_missing_message(theme)));
            }
            let plan = crate::qt_settings::sound_theme_plan(de, theme, &sf.kwrite, &sf.kread, sf.has_gsettings);
            if plan.write.is_empty() {
                return PressPlan::Refused(Some(tr(
                    "Nothing to write the sound theme with: neither kwriteconfig6 nor \
                     gsettings is available.",
                )));
            }
            let config = crate::utils::config_home(xdg, home);
            let guard = kconfig_files(plan.write.iter().map(Vec::as_slice), &config);
            let mut v: Vec<Step> = Vec::new();
            if !guard.is_empty() {
                v.push(Step::from(crate::themes::ApplyStep::Guard { paths: guard }));
            }
            v.extend(plan.write.iter().map(|argv| {
                Step::from(crate::themes::ApplyStep::BestEffort(Cmd { argv: argv.clone(), timeout: None }))
            }));
            let say = match plan.verify {
                Some(read) => {
                    v.push(Step::from(crate::themes::ApplyStep::ReadBackSetsOk {
                        read: Cmd { argv: read, timeout: None },
                        expect: theme.to_string(),
                    }));
                    PressSay::Plain
                }
                None => PressSay::Pair {
                    done: apply_message(action, true),
                    failed: tr(
                        "Sound theme written, but there's nothing to check it with (no \
                         kreadconfig6).",
                    ),
                },
            };
            PressPlan::Steps { steps: v, history: None, say }
        }
        // Lock screen wallpaper: the picture (`ok` depends on it), then the
        // auxiliary keys: they run even on failure, as in the old own path.
        // `kscreenlockerrc` will be rewritten by `kwriteconfig6` bypassing the
        // rollback, so its "what was there" is taken by `Guard`.
        ApplyAction::LockWallpaper => {
            let Some(lf) = &facts.lock else { return PressPlan::Refused(None) };
            match lock_wallpaper_plan_in(de, lf.treeland, &lf.install_dir, theme, lf.is_file) {
                Err(e) => PressPlan::Refused(Some(e)),
                Ok(plan) => {
                    let config = crate::utils::config_home(xdg, home);
                    let cmds = std::iter::once(&plan.image).chain(&plan.extra);
                    let guard = kconfig_files(cmds.map(|c| c.argv.as_slice()), &config);
                    let mut v: Vec<Step> = Vec::new();
                    if !guard.is_empty() {
                        v.push(Step::from(crate::themes::ApplyStep::Guard { paths: guard }));
                    }
                    v.push(primary(plan.image.argv.clone()));
                    v.extend(plan.extra.iter().map(|c| {
                        Step::from(crate::themes::ApplyStep::BestEffort(Cmd { argv: c.argv.clone(), timeout: None }))
                    }));
                    PressPlan::Steps {
                        steps: v,
                        history: None,
                        say: PressSay::Pair { done: plan.success_message, failed: plan.failure_message },
                    }
                }
            }
        }
        // An environment section: a theme and an add-on are steps of the common
        // plan with an environment check; Conky (processes) goes its own way.
        ApplyAction::DeSection => match facts.desktop.clone() {
            Some(crate::de_apply::Plan::Refused(m)) => PressPlan::Refused(Some(m)),
            Some(crate::de_apply::Plan::Steps { steps, check }) => PressPlan::Desktop { steps, check },
            Some(crate::de_apply::Plan::Own) | None => PressPlan::Own,
        },
        ApplyAction::WallpaperBackground
        | ApplyAction::WallpaperPack
        | ApplyAction::SystemTheme => PressPlan::Own,
    }
}


// ── environment detectors for system installation ───────────────────────────

/// `(realpath of the display-manager.service symlink, whether the target
/// is alive)`: a port of `_dm_symlink_target`. A dangling symlink after
/// `pacman -R sddm` gives `(target, false)`, and the core decides based on
/// it.
fn dm_symlink_target() -> (Option<String>, bool) {
    let link = Path::new(crate::system_plan::DM_SYMLINK);
    if link.symlink_metadata().is_err() && !link.exists() {
        return (None, false);
    }
    match std::fs::canonicalize(link) {
        Ok(target) => {
            let s = target.to_string_lossy().into_owned();
            let alive = target.exists();
            (Some(s), alive)
        }
        // The target does not resolve, the symlink is dangling: we name the path anyway.
        Err(_) => (
            std::fs::read_link(link)
                .ok()
                .map(|t| t.to_string_lossy().into_owned()),
            false,
        ),
    }
}

/// The name of the active login screen unit: a port of
/// `system.active_login_manager`.
///
/// Needed for one question: whether the login screen is drawn by Plasma
/// Login Manager (Plasma 6.6+), which does NOT load SDDM QML themes:
/// installing such a theme must be stopped before pkexec, the "Appearance"
/// section must be shown differently, and the SDDM theme must not be marked
/// active.
///
/// ⚠️ May query `systemctl` (up to five units, 5 s each): call it in a
/// worker thread, NOT in the UI thread. The fast path is a live
/// `display-manager.service` symlink; then there are no processes at all.
pub fn active_login_manager() -> String {
    let (target, alive) = dm_symlink_target();
    let has_systemctl = have("systemctl");
    let is_enabled = |unit: &str| -> Option<String> {
        let argv = vec![
            "systemctl".to_string(),
            "is-enabled".to_string(),
            format!("{unit}.service"),
        ];
        let report = run_capture(&argv, Some(5));
        if matches!(report.outcome, RunOutcome::Error | RunOutcome::Timeout) {
            return None;
        }
        Some(crate::utils::decode_text(&report.stdout).trim().to_string())
    };
    crate::system_plan::active_login_manager_from(
        target.as_deref(),
        alive,
        has_systemctl,
        &is_enabled,
    )
}

// ============================================================================
// LiveInstaller: the pipelines of the "Appearance" tab
// ============================================================================

/// The live theme installer: the executor of this module's plans.
pub struct LiveInstaller {
    /// The desktop we apply for (empty means detect it ourselves).
    de: String,
    home: String,
    log: Box<dyn FnMut(&str) + Send>,
    /// Whether missing icons and the cursor of a theme may be installed on
    /// "Apply" ([`LiveInstaller::allow_deps_install`]).
    deps_install: bool,
    /// Whether to enable the icons the theme was designed for
    /// ([`LiveInstaller::set_icons_choice`]); `None` means undecided, icons are
    /// not touched.
    icons_choice: Option<bool>,
    /// The choice in the "Apply "X"?" window of a global theme
    /// ([`LiveInstaller::set_global_choice`]); `None` means there was no
    /// window.
    global_choice: Option<crate::themes::GlobalChoice>,
}

impl LiveInstaller {
    /// `log` is a log line to the interface (in Python it is `on_log`).
    ///
    /// An empty `de` ASKS the system rather than assumes KDE: the former
    /// hardcoded fallback sent applying GTK themes, icons and cursors down the
    /// KDE path for everyone who calls the installer without an argument;
    /// under GNOME this meant writing to `kdeglobals` instead of `gsettings`.
    pub fn new(de: &str, log: Box<dyn FnMut(&str) + Send>) -> Self {
        let de = if de.is_empty() {
            crate::system::detect_current_de(
                &env_str("XDG_CURRENT_DESKTOP"),
                &env_str("DESKTOP_SESSION"),
                !env_str("WAYLAND_DISPLAY").is_empty(),
            )
            .unwrap_or_else(|| "kde".to_string())
        } else {
            de.to_string()
        };
        LiveInstaller { de, home: live_home(), log, deps_install: false, icons_choice: None, global_choice: None }
    }

    /// A line to the interface log.
    fn say(&mut self, message: &str) {
        (self.log)(message);
    }

    fn home_path(&self) -> PathBuf {
        PathBuf::from(&self.home)
    }

    // ── INSTALLATION ────────────────────────────────────────────────────────

    /// Port of `ThemeInstaller.install`: `(success, message, theme names)`.
    ///
    /// Does NOT apply: applying is a separate step (see [`Self::install_and_apply`]).
    pub fn install(
        &mut self,
        item: &crate::models::ThemeItem,
        archive_path: &str,
    ) -> (bool, String, Vec<String>) {
        let home = self.home_path();
        let Some(cat) = crate::categories::category_by_id(&home, item.category_id) else {
            return (
                false,
                trf("Unknown category: %1", &[&item.category_id]),
                Vec::new(),
            );
        };
        if cat.requires_root {
            // SDDM/Plymouth: system paths and activation require root, a
            // separate path through the polkit helper.
            return self.install_root(item, &cat, archive_path);
        }

        // The item is delivered as one bare file (not an archive): a colour
        // scheme, a font, a Kvantum theme and wallpapers are self-contained,
        // and some items simply have no zip/tar wrapper. Nothing to unpack.
        let is_archive = is_archive_file(archive_path);
        let head = read_head(archive_path, 16);
        if single_file_accepted(
            &cat.apply_kind,
            is_archive,
            &head,
            &[&item.download_name, &item.download_url],
        ) {
            return self.install_single_file(item, &cat, archive_path);
        }

        let work = match mkdtemp_in(&std::env::temp_dir(), "rexon-ocs-") {
            Ok(p) => p,
            Err(e) => return (false, trf("Installation error: %1", &[&e]), Vec::new()),
        };
        let extract_dir = work.join("extracted");
        let extract_dir_s = extract_dir.to_string_lossy().into_owned();
        let result = match live_safe_extract(archive_path, &extract_dir_s) {
            Ok(()) => self.install_archive(item, &cat, &extract_dir_s),
            Err(e) => (false, trf("Couldn't unpack the archive: %1", &[&e]), Vec::new()),
        };
        let _ = std::fs::remove_dir_all(&work);
        result
    }

    /// Install what is already unpacked (`extract_dir`): the same "archive"
    /// branch as for a store archive, without unpacking again. Needed by
    /// sources that choose what to install themselves ("Install from file…",
    /// `local_install`).
    pub fn install_extracted(
        &mut self,
        item: &crate::models::ThemeItem,
        extract_dir: &str,
    ) -> (bool, String, Vec<String>) {
        let home = self.home_path();
        let Some(cat) = crate::categories::category_by_id(&home, item.category_id) else {
            return (
                false,
                trf("Unknown category: %1", &[&item.category_id]),
                Vec::new(),
            );
        };
        // A system kind (SDDM, Plymouth) uses the same helper as a store
        // archive, with the same checks before the password window. The helper
        // takes only a directory under `/tmp`: that is where "Install from
        // file…" unpacks (`crate::local_install`).
        if cat.requires_root {
            let env = Self::live_root_env(&cat.apply_kind);
            if let Some(pre) = root_preflight(&env, &cat) {
                return (false, pre.message, Vec::new());
            }
            return self.install_root_staged(item, &cat, &env, extract_dir);
        }
        self.install_archive(item, &cat, extract_dir)
    }

    /// The "archive" installation branch: the plan from [`plan_install`], execution here.
    fn install_archive(
        &mut self,
        item: &crate::models::ThemeItem,
        cat: &ThemeCategory,
        extract_dir: &str,
    ) -> (bool, String, Vec<String>) {
        let home = self.home_path();
        // The snapshot must cover both the category directory and ALL
        // directories where the installation may go after reclassification;
        // otherwise an already installed theme will not be noticed and will not
        // be removed before copying.
        let mut dest_dirs = vec![cat.install_dir.clone()];
        // Window frames and shells also land inside a GTK theme with the same
        // name (`plan_nested_part`): its directory goes into the snapshot too.
        if matches!(de_theme_marker(&cat.apply_kind), Some(DeThemeMarker::Subdir(_))) {
            dest_dirs.push(crate::installed_scan::xdg_themes_dir(&self.home));
        }
        for actual in ACTUAL_TYPES_WITH_DIR {
            if let Some(dir) = actual_type_dir(&home, actual) {
                if !dest_dirs.contains(&dir) {
                    dest_dirs.push(dir);
                }
            }
        }
        // A Conky widget puts fonts into `~/.local/share/fonts/rexon-conky-<name>`:
        // the plan learns from the snapshot whether that folder is taken by
        // someone else.
        if cat.apply_kind == "conky" {
            let fonts = format!("{}/.local/share/fonts", self.home);
            if !dest_dirs.contains(&fonts) {
                dest_dirs.push(fonts);
            }
        }
        let snap = install_tree_snapshot(extract_dir, &dest_dirs);
        // What this same purchase installed before: only that may be removed and
        // installed again, not something taken by a foreign theme.
        let own = self.recorded_paths(&item.id);
        let fallback_name = archive_theme_name(&item.download_name, &item.name);
        let origin = ArchiveOrigin { extract_dir, fallback_name: &fallback_name };
        let out = plan_install_archive(&home, &snap, cat, origin, Some(&own));
        if !out.ok {
            return (false, out.message, Vec::new());
        }
        let Some(record) = out.record.clone() else {
            return (false, out.message, Vec::new());
        };
        if let Some(actual) = &out.reclassified {
            let dir = record
                .paths
                .first()
                .map(|p| rstrip_slashes(p).rsplit_once('/').map(|(d, _)| d.to_string()))
                .unwrap_or_default()
                .unwrap_or_default();
            // TRANSLATORS: arrows ← → point the way they should in your language: in right-to-left languages swap them.
            let line = trf("Store category %1, actually %2 → %3", &[&cat.key, &actual, &dir]);
            self.say(&line);
        }
        if let Err(e) = exec_install_steps(&out.steps) {
            return (false, trf("Installation error: %1", &[&e]), Vec::new());
        }
        // An icon or cursor theme was installed: also check the profile's shared
        // `hicolor` set: a truncated `index.theme` in it hides the icons of the
        // whole desktop ([`crate::icon_hygiene`]).
        if matches!(record.apply_kind.as_str(), "icons" | "cursor") {
            if let Some(line) = crate::icon_hygiene::heal_live() {
                self.say(&line);
            }
        }
        // A Conky widget written for `~/.conky/<Name>/`: the paths are pointed at
        // the folder where it was installed (`de_apply::conky_repair_paths`).
        if record.apply_kind == "conky" {
            for p in &record.paths {
                crate::de_apply::conky_repair_paths(&self.home, p);
            }
        }
        self.record(item, &record.apply_kind, &record.paths);
        if record.apply_kind == "colors" {
            let line = trf("Color scheme installed: %1", &[&out.names.join(", ")]);
            self.say(&line);
        } else {
            let line = out.message.clone();
            self.say(&line);
        }
        (true, out.message, out.names)
    }

    /// An item as one bare file: the plan from [`plan_single_file`], the copy here.
    fn install_single_file(
        &mut self,
        item: &crate::models::ThemeItem,
        cat: &ThemeCategory,
        archive_path: &str,
    ) -> (bool, String, Vec<String>) {
        // What this same purchase installed before can be replaced; a foreign
        // file (or theme folder) with the same name cannot, unless it is the
        // same file byte for byte.
        let own = self.recorded_paths(&item.id);
        let taken = |p: &str| -> bool {
            let Ok(md) = std::fs::symlink_metadata(p) else { return false };
            if own.iter().any(|o| rstrip_slashes(o) == p) {
                return false;
            }
            let same = md.file_type().is_file()
                && std::fs::read(p).ok().is_some_and(|b| std::fs::read(archive_path).ok() == Some(b));
            !same
        };
        let plan = plan_single_file_with(item, cat, archive_path, &taken);
        if !plan.ok {
            return (false, plan.message, Vec::new());
        }
        let Some(record) = plan.record.clone() else {
            return (false, plan.message, Vec::new());
        };
        if let Err(e) = exec_install_steps(&plan.steps) {
            return (false, trf("File installation error: %1", &[&e]), Vec::new());
        }
        self.record(item, &record.apply_kind, &record.paths);
        let line = trf(
            "Installed as a single file: %1",
            &[&plan.names.first().map(String::as_str).unwrap_or("")],
        );
        self.say(&line);
        (true, plan.message, plan.names)
    }

    /// A system theme (SDDM/Plymouth) through the Rexon polkit helper.
    ///
    /// The password is asked less often than for every theme: the polkit
    /// action `org.rexon.system-install` is declared with `auth_admin_keep`
    /// (minutes for the Rexon window) and covered by the `org.rexon.pkg`
    /// "umbrella" ([`crate::system_plan::PKG_IMPLIES`]). Gates, the theme name
    /// and argv are in the core; environment observations and `pkexec` itself
    /// are here.
    fn install_root(
        &mut self,
        item: &crate::models::ThemeItem,
        cat: &ThemeCategory,
        archive_path: &str,
    ) -> (bool, String, Vec<String>) {
        let env = Self::live_root_env(&cat.apply_kind);
        if let Some(pre) = root_preflight(&env, cat) {
            return (false, pre.message, Vec::new());
        }

        // The helper accepts a staged path only under /tmp|/var/tmp: the
        // temporary directory must lie there, not in the user's $TMPDIR.
        let work = match mkdtemp_in(Path::new("/tmp"), "rexon-ocs-root-") {
            Ok(p) => p,
            Err(e) => return (false, trf("Installation error: %1", &[&e]), Vec::new()),
        };
        let extract_dir = work.join("extracted");
        let extract_dir_s = extract_dir.to_string_lossy().into_owned();
        let result = match live_safe_extract(archive_path, &extract_dir_s) {
            Ok(()) => self.install_root_staged(item, cat, &env, &extract_dir_s),
            Err(e) => (false, trf("Couldn't unpack the archive: %1", &[&e]), Vec::new()),
        };
        let _ = std::fs::remove_dir_all(&work);
        result
    }

    fn install_root_staged(
        &mut self,
        item: &crate::models::ThemeItem,
        cat: &ThemeCategory,
        env: &RootEnv,
        extract_dir: &str,
    ) -> (bool, String, Vec<String>) {
        let snap = install_tree_snapshot(extract_dir, &[]);
        let content_root = find_content_root(&snap, extract_dir);
        // A system theme directory with the same name that this purchase did
        // not install (a package's theme, the person's, another purchase's) is
        // not overwritten.
        let own = self.recorded_paths(&item.id);
        let taken = |p: &str| -> bool {
            std::fs::symlink_metadata(p).is_ok() && !own.iter().any(|o| rstrip_slashes(o) == p)
        };
        let plan = match plan_root_install_with(env, &snap, cat, &content_root, &taken) {
            Ok(p) => p,
            Err(outcome) => return (false, outcome.message.clone(), Vec::new()),
        };

        // The same check the root helper will make, before the password window:
        // a link in the theme, a `.so` or a foreign Plymouth module (code run as
        // root at boot) are rejected right away and with a reason.
        if let Some(problem) = crate::installer::system_theme_problem(
            &cat.apply_kind,
            Path::new(&content_root),
            Some(&plan.theme_id),
        ) {
            return (false, problem.message(), Vec::new());
        }

        if cat.apply_kind != "plymouth" {
            let has_md = Path::new(&join(&content_root, "metadata.desktop")).is_file();
            let has_qml = Path::new(&join(&content_root, "Main.qml")).is_file();
            if !has_md && !has_qml {
                // ⭐ Fixed during the port: the original wrote this to the log
                // file, which the customer does not look at, while the point is
                // that the theme will most likely not work. We tell them directly.
                let line = trf(
                    "⚠️ SDDM theme without metadata.desktop or Main.qml: %1 — the \
                     login screen may not pick it up.",
                    &[&plan.theme_id],
                );
                self.say(&line);
            }
        }

        let line = trf(
            "🔐 Installing “%1”: %2 (an administrator password may be requested)…",
            &[&cat.name, &plan.theme_id],
        );
        self.say(&line);

        let report = run_capture(&plan.argv, Some(plan.timeout_secs));
        if report.outcome == RunOutcome::Timeout {
            return (
                false,
                tr("Installation timed out (rebuilding initramfs?)"),
                Vec::new(),
            );
        }
        let Some(rc) = report.returncode else {
            // pkexec did not start at all: the launch error text is more useful
            // than silence (the original dropped an exception into the log here).
            return (
                false,
                trf("Installation error: %1", &[&report.error]),
                Vec::new(),
            );
        };
        // 126: polkit authorisation not obtained/cancelled; 127: not started.
        if rc == 126 || rc == 127 {
            return (false, root_cancelled_message().to_string(), Vec::new());
        }
        if rc != 0 {
            let err = crate::utils::decode_text(&report.stderr);
            return (false, root_error_message(rc, &err), Vec::new());
        }

        self.record(item, &cat.apply_kind, &[plan.dest.clone()]);
        let line = plan.success_message.clone();
        self.say(&line);
        (true, plan.success_message, vec![plan.theme_id])
    }

    // ── installed log ───────────────────────────────────────────────────────

    /// Paths written to the log for purchase `content_id` (empty means there were none).
    fn recorded_paths(&self, content_id: &str) -> Vec<String> {
        if content_id.is_empty() {
            return Vec::new();
        }
        let log_path =
            crate::ocs_store::install_log_path_xdg(xdg_config_home().as_deref(), &self.home);
        let data = crate::install_journal::store_log_view(&log_path);
        data.get(content_id)
            .and_then(|e| e.get("paths"))
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
            .unwrap_or_default()
    }

    /// Writing to the installed log: a port of `_record`.
    ///
    /// ⭐ Fixed during the port: on a failed log write the original only
    /// whispered into the log file. The theme stayed on disk, but the installed
    /// list did not see it: the card showed "not installed", and there was
    /// nothing to remove the theme with. Now the failure is visible to the
    /// person.
    fn record(
        &mut self,
        item: &crate::models::ThemeItem,
        apply_kind: &str,
        paths: &[String],
    ) {
        // The theme went into `~/.local/share/themes`: right away a link in
        // `~/.themes` too, for those who read only that (MATE's "Appearance"
        // window).
        let xdg_themes = crate::installed_scan::xdg_themes_dir(&self.home);
        if paths.iter().any(|p| p.rsplit_once('/').is_some_and(|(d, _)| d == xdg_themes)) {
            for line in crate::installed_scan::sync_theme_links(&self.home) {
                crate::log_info!("theme", "{line}");
            }
        }
        if item.id.is_empty() {
            return; // without a content_id the log has nothing to remember
        }
        let entry = crate::ocs_store::InstalledItem {
            content_id: item.id.clone(),
            name: item.name.clone(),
            rexon_type: kind_to_rexon_type(apply_kind).to_string(),
            author: item.author.clone(),
            description: item.description.clone(),
            summary: String::new(),
            version: item.version.clone(),
            license: item.license.clone(),
            preview_url: item.preview_url.clone(),
            score: item.rating,
            downloads: item.downloads,
            tags: Vec::new(),
            web_url: String::new(),
            provider_id: "kde-look".to_string(),
            category_id: item.category_id,
        };
        let log_path =
            crate::ocs_store::install_log_path_xdg(xdg_config_home().as_deref(), &self.home);
        // The common install log is written, and `ocs_installed.json` is its
        // mirror (step 5b of merging the installers).
        let mut data = crate::install_journal::store_log_view(&log_path);
        crate::ocs_store::record_installed(&mut data, &entry, paths, &now_local_iso());
        if let Err(e) = crate::install_journal::put_store_log(&log_path, &data) {
            let line = trf(
                "⚠️ The installed-themes log wasn't written (%1) — the theme is on \
                 disk, but it won't be in the list of installed themes.",
                &[&e],
            );
            self.say(&line);
        }
        // The Store screenshot next to the theme is best-effort: the local scan
        // does not find previews for GTK/Plasma/icons/cursors (the theme files
        // have none), and we have already downloaded it.
        self.save_ocs_preview(item, apply_kind, paths);
    }

    /// Put the screenshot downloaded from the Store where the installed scan
    /// looks for it: a port of `_save_ocs_preview` (the address is decided by
    /// [`ocs_preview_dest`]).
    fn save_ocs_preview(
        &mut self,
        item: &crate::models::ThemeItem,
        apply_kind: &str,
        paths: &[String],
    ) {
        let src = self.ocs_preview_source(item);
        if src.is_empty() {
            return; // not installed through the Store, or the preview was not downloaded
        }
        // A purchase of several themes (an archive with two sound themes, GTK
        // variants) gets a preview for each: the scan shows them as separate
        // cards.
        let dirs: Vec<&String> = paths.iter().filter(|p| Path::new(p).is_dir()).collect();
        let targets: Vec<String> = if apply_kind != "colors" && dirs.len() > 1 {
            dirs.into_iter().cloned().collect()
        } else {
            vec![paths.first().cloned().unwrap_or_default()]
        };
        for theme_dir in targets {
            let is_dir = !theme_dir.is_empty() && Path::new(&theme_dir).is_dir();
            let Some(dst) =
                ocs_preview_dest(&self.home, apply_kind, &theme_dir, is_dir, &item.name)
            else {
                continue; // no name: nowhere to copy
            };
            // The caller creates the sidecar cache directory; its address comes
            // from the scan itself, so there is no second copy of the path in the
            // project.
            let probe = crate::installed_scan::sidecar_preview_path(&self.home, "k", "n");
            if let Some((sidecar_dir, _)) = probe.rsplit_once('/') {
                if dst.starts_with(sidecar_dir) {
                    let _ = std::fs::create_dir_all(sidecar_dir);
                }
            }
            let _ = std::fs::copy(&src, &dst);
        }
    }

    /// The path of the downloaded OCS screenshot: `preview_local`, otherwise the
    /// file from the preview cache by `preview_url`; `""` means there is none.
    fn ocs_preview_source(&self, item: &crate::models::ThemeItem) -> String {
        let local = item.preview_local.trim();
        if !local.is_empty() && Path::new(local).is_file() {
            return local.to_string();
        }
        let url = item.preview_url.trim();
        if url.is_empty() {
            return String::new();
        }
        crate::appearance_cache::LiveCache::open(&self.home)
            .get_preview(url)
            .unwrap_or_default()
    }

    /// Record an application in the theme history: a port of
    /// `ThemeManager._add_history` (`~/.config/rexon/theme_history.json`,
    /// inserted at the start, trimmed to 50).
    fn add_history(&mut self, kind: &str, theme: &str, de: &str) {
        let cfg = crate::utils::config_home(xdg_config_home().as_deref(), &self.home);
        let path = format!("{cfg}/rexon/theme_history.json");
        let existing: Vec<Value> = read_text_lossy(&path)
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default();
        let entry = crate::themes::history_entry(kind, theme, de, &now_local_iso());
        let updated = crate::themes::push_history(&existing, entry);
        if let Some(dir) = Path::new(&path).parent() {
            if std::fs::create_dir_all(dir).is_err() {
                return;
            }
        }
        let _ = crate::utils::atomic_write(&path, &crate::themes::serialize_history(&updated));
    }

    /// Whether an icon theme is valid: a port of `_is_valid_icon_theme` (a
    /// snapshot of `index.theme`, the decision by
    /// [`crate::themes::is_valid_icon_theme`]).
    fn icon_theme_valid(&self, theme: &str) -> bool {
        let dirs = crate::themes::icon_dirs(&self.home);
        let mut files = serde_json::Map::new();
        for d in &dirs {
            let idx = format!("{d}/{theme}/index.theme");
            if Path::new(&idx).is_file() {
                files.insert(idx.clone(), Value::from(read_text_lossy(&idx).unwrap_or_default()));
            }
        }
        let fs = FsSnapshot::from_json(&serde_json::json!({ "dirs": {}, "files": files }));
        crate::themes::is_valid_icon_theme(&fs, theme, &dirs)
    }

    /// Whether it is safe to apply a cursor theme: a port of
    /// `_cursor_preflight` (a GTK4 crash guard: the `Inherits` chain must not
    /// lead nowhere).
    fn cursor_safe(&self, theme: &str) -> bool {
        let bases = crate::themes::icon_dirs(&self.home);
        let mut dirs = serde_json::Map::new();
        let mut files = serde_json::Map::new();
        for base in &bases {
            let d = format!("{base}/{theme}");
            if !Path::new(&d).is_dir() {
                continue;
            }
            dirs.insert(d.clone(), Value::Array(Vec::new()));
            let cur = format!("{d}/cursors");
            if Path::new(&cur).is_dir() {
                dirs.insert(cur, Value::Array(Vec::new()));
            }
            let ct = format!("{d}/cursor.theme");
            if Path::new(&ct).is_file() {
                files.insert(ct, Value::from(""));
            }
            let idx = format!("{d}/index.theme");
            if Path::new(&idx).is_file() {
                files.insert(idx.clone(), Value::from(read_text_lossy(&idx).unwrap_or_default()));
            }
        }
        let fs = FsSnapshot::from_json(&serde_json::json!({ "dirs": dirs, "files": files }));
        let parent_has_cursors = |parent: &str| {
            bases
                .iter()
                .any(|base| Path::new(&format!("{base}/{parent}/cursors")).is_dir())
        };
        crate::themes::cursor_preflight_ok(&fs, theme, &self.home, parent_has_cursors)
    }

    /// Execute a [`crate::themes`] plan and record history on success.
    /// Apply a cursor: the theme and the size in all environments, `(success,
    /// message)`. An unsafe theme is a refusal without a single write.
    ///
    /// The same press plan as "Apply" in the cursors section
    /// ([`press_plan_with`], the size is `size`), the same executor with
    /// rollback; success leaves a "what was there" snapshot of the `cursor`
    /// section in the common log.
    fn apply_cursor(&mut self, theme: &str, size: Option<u32>) -> (bool, String) {
        let action = ApplyAction::Cursor;
        let mut facts = PressFacts {
            de: self.de.clone(),
            home: self.home.clone(),
            xdg_config_home: xdg_config_home(),
            cursor_size: size,
            ..PressFacts::default()
        };
        self.cursor_facts(theme, &mut facts);
        let plan = press_plan_with(&action, theme, &facts);
        let cat = crate::categories::category_by_kind(&self.home_path(), "cursor");
        let (ok, msg) = self.run_press("", cat.as_ref(), theme, &action, plan);
        (ok, msg.unwrap_or_else(|| apply_message(&action, ok).to_string()))
    }

    /// Change only the cursor size: the theme stays current. The same path as
    /// choosing a theme, so the size reaches the same places: the
    /// environment's native key, GTK, environment variables, the compositor
    /// config.
    pub fn apply_cursor_size(&mut self, size: u32) -> (bool, String) {
        let xdg = xdg_config_home();
        let Some(theme) = live_cursor_value(&self.de, &self.home, xdg.as_deref(), false) else {
            let text = tr("Couldn't find out which cursor theme is in use — pick a cursor theme first.");
            self.say(&text);
            return (false, text);
        };
        let (ok, msg) = self.apply_cursor(&theme, Some(size));
        let changes = live_gtk_sanitize(&self.home);
        if !changes.is_empty() {
            let line = gtk_sanitizer_message(changes.len());
            self.say(&line);
        }
        self.say(&msg);
        (ok, msg)
    }

    /// The cursor theme enabled in this environment (`None` means cannot tell).
    pub fn active_cursor_theme(&self) -> Option<String> {
        let xdg = xdg_config_home();
        live_cursor_value(&self.de, &self.home, xdg.as_deref(), false)
    }

    /// Reapply the same cursor theme after its files were replaced in place
    /// (converted again from the Windows originals). The same plan as "Apply"
    /// in the cursors section, but without a history entry: the person chose
    /// nothing, and "Restore as it was" stays as before.
    pub fn refresh_cursor(&mut self, theme: &str) -> bool {
        let mut facts = PressFacts {
            de: self.de.clone(),
            home: self.home.clone(),
            xdg_config_home: xdg_config_home(),
            ..PressFacts::default()
        };
        self.cursor_facts(theme, &mut facts);
        match press_plan_with(&ApplyAction::Cursor, theme, &facts) {
            PressPlan::Steps { steps, .. } => self.exec_press("", None, theme, &steps).ok,
            _ => false,
        }
    }

    /// The current cursor size in this environment (`None` means nobody set it).
    pub fn cursor_size(&self) -> Option<u32> {
        let xdg = xdg_config_home();
        live_cursor_value(&self.de, &self.home, xdg.as_deref(), true)
            .and_then(|v| crate::themes::parse_cursor_size(&v))
    }

    // ── WHAT THE THEME ASKS FOR ON TOP (icons and cursor) ──────────────────

    /// Allow installing a theme's missing icons and cursor on "Apply" (after
    /// [`Self::prepare_theme_deps`] there is nothing left to install there;
    /// the session guard installs on the notification's "Install" button).
    pub fn allow_deps_install(&mut self, yes: bool) {
        self.deps_install = yes;
    }

    /// What to apply from a global theme: the answer of the "Apply "X"?" window.
    pub fn set_global_choice(&mut self, choice: Option<crate::themes::GlobalChoice>) {
        self.global_choice = choice;
    }

    /// The answer to the question about the theme's icons
    /// ([`Self::theme_icons_question`]): `Some(true)` means download (without a
    /// separate consent to the package) and enable, `Some(false)` means keep
    /// your own, `None` means undecided (icons are not touched; "Apply" takes
    /// the theme's remembered answer).
    pub fn set_icons_choice(&mut self, yes: Option<bool>) {
        self.icons_choice = yes;
    }

    /// The question "The theme is designed for the "X" icons. Download and
    /// enable?" before "Apply" for GTK theme `item`, in the UI thread BEFORE
    /// the background work. `None` means there is nothing to ask
    /// ([`crate::themes::icons_question`]: the theme asks for no icons, they
    /// are already installed, or the answer for the theme is remembered).
    pub fn theme_icons_question(&self, item: &crate::models::ThemeItem, theme_name: &str) -> Option<crate::themes::IconsQuestion> {
        let cat = crate::categories::category_by_id(&self.home_path(), item.category_id);
        if apply_action_for(cat.as_ref(), theme_name) != ApplyAction::Gtk {
            return None;
        }
        let plan = self.theme_deps_plan(theme_name);
        if !plan.iter().any(|i| i.role == crate::themes::DepRole::Icons) {
            return None;
        }
        let current = crate::theme_watch::live_looks(&self.de).icons;
        crate::themes::icons_question(theme_name, &plan, &current, crate::theme_watch::live_icons_choice(theme_name))
    }

    /// The requests of GTK theme `theme` to the live system: what is
    /// installed, what can be installed, what is available nowhere. Empty
    /// means the theme has no metatheme or asks for nothing.
    pub fn theme_deps_plan(&self, theme: &str) -> Vec<crate::themes::DepItem> {
        let data = crate::utils::data_home(xdg_data_home().as_deref(), &self.home);
        let dirs = env_str("XDG_DATA_DIRS");
        let index = crate::themes::gtk_theme_search_roots(&self.home, &data, &dirs)
            .into_iter()
            .map(|root| format!("{root}/{theme}/index.theme"))
            .find_map(|p| read_text_lossy(&p));
        let Some(index) = index else { return Vec::new() };
        let deps = crate::themes::theme_deps(&index);
        let roots = crate::themes::dep_search_roots(&self.home, &data, &dirs);
        crate::themes::plan_theme_deps(&deps, &live_deps_family(), &|role, wanted| {
            live_find_dep(role, wanted, &roots)
        })
    }

    /// Before "Apply" for GTK theme `item`, install what it comes with and
    /// what is missing (the cursor; icons if "Download and enable" was
    /// chosen) BY ITSELF, without a "Yes/No" question (the protection is the
    /// polkit password window). First into the home directory without a
    /// password where possible; the rest as packages only from the official
    /// repositories (there is no AUR build without a question).
    ///
    /// "All or nothing": if something did not install, `Err` with a reason and
    /// advice, and the theme is NOT applied. What got installed is decided by
    /// the disk, not by the exit code.
    pub fn prepare_theme_deps(&mut self, item: &crate::models::ThemeItem, theme: &str) -> Result<(), String> {
        use crate::themes::{DepState, deps_to_prepare};
        let cat = crate::categories::category_by_id(&self.home_path(), item.category_id);
        if apply_action_for(cat.as_ref(), theme) != ApplyAction::Gtk {
            return Ok(());
        }
        if self.icons_choice.is_none() {
            self.icons_choice = crate::theme_watch::live_icons_choice(theme);
        }
        let wanted = deps_to_prepare(&self.theme_deps_plan(theme), self.icons_choice == Some(true));
        if wanted.is_empty() {
            return Ok(());
        }
        let mut why: Option<String> = None;
        let mut packages: Vec<String> = Vec::new();
        for i in &wanted {
            if crate::themes::dep_home_source(i.role, &i.wanted).is_some() {
                match self.install_home_set(i.role, &i.wanted) {
                    Ok(_) => continue,
                    Err(e) => why = Some(e),
                }
            }
            if let DepState::Installable(p) = &i.state {
                if !packages.contains(p) {
                    packages.push(p.clone());
                }
            }
        }
        if !packages.is_empty() {
            let line = trf(
                "📥 Installing what the theme comes with: %1 (an administrator password may be requested)…",
                &[&packages.join(", ")],
            );
            self.say(&line);
            if let Err(e) = live_install_repo_packages(&packages, PkgSource::ThemeSets) {
                why = Some(e);
            }
        }
        // Everything installed is in place: from here "Apply" takes what is ready.
        let now = self.theme_deps_plan(theme);
        let missing: Vec<crate::themes::DepItem> = wanted
            .into_iter()
            .filter(|w| !now.iter().any(|n| n.role == w.role && matches!(n.state, DepState::Installed(_))))
            .collect();
        self.deps_install = true;
        if missing.is_empty() {
            return Ok(());
        }
        let why = why.unwrap_or_else(|| tr("the package didn't bring it"));
        Err(crate::themes::deps_refusal(theme, &missing, &why))
    }

    /// Apply the icons and cursor GTK theme `theme` comes with (installing
    /// what is missing if the person agreed). The result lines are for the
    /// "Apply" message.
    ///
    /// ⛔ A name that is not on disk is not written to the settings: the
    /// previous icons and cursor stay, and the person is told why.
    fn apply_theme_deps(&mut self, theme: &str) -> Vec<String> {
        // The question about what is missing was asked here, in Rexon: the
        // session guard (`theme_watch`) does not ask about the same theme again.
        crate::theme_watch::note_rexon_applied(self, theme);
        if self.icons_choice.is_none() {
            self.icons_choice = crate::theme_watch::live_icons_choice(theme);
        }
        let items = self.theme_deps_plan(theme);
        self.apply_dep_items(&items)
    }

    /// The environment this installer applies themes for.
    pub fn de_id(&self) -> &str {
        &self.de
    }

    /// Install (if the person agreed, [`Self::allow_deps_install`]) and apply
    /// the sets `items`: the common path of "Apply" for a GTK theme and of the
    /// "Install" button in the session guard notification (`theme_watch`).
    /// The result lines are for the person.
    ///
    /// ⛔ A name that is not on disk is not written to the settings.
    pub fn apply_dep_items(&mut self, items: &[crate::themes::DepItem]) -> Vec<String> {
        if items.is_empty() {
            return Vec::new();
        }
        // First what goes into the home directory without a password and
        // without building (Wehrwolfmann's decision of 17.10.2026): silently,
        // unless turned off in the settings, or with the person's consent. If
        // that failed, the usual package path follows.
        let mut items: Vec<crate::themes::DepItem> = items.to_vec();
        // The theme's icons only by the person's answer (decision 3a): "Keep
        // mine" or undecided means the set is neither downloaded nor enabled.
        let mut lines = Vec::new();
        let icons_ok = self.icons_choice == Some(true);
        if !icons_ok {
            if self.icons_choice == Some(false) {
                for i in items.iter().filter(|i| i.role == crate::themes::DepRole::Icons) {
                    lines.push(crate::themes::dep_icons_kept_line(&i.wanted));
                }
            }
            items.retain(|i| i.role != crate::themes::DepRole::Icons);
        }
        // Consent to installation: icons by the answer to their own question,
        // the rest by the common question about packages.
        let deps_install = self.deps_install;
        let consent = move |role: crate::themes::DepRole| deps_install || (icons_ok && role == crate::themes::DepRole::Icons);
        let auto_home = crate::theme_watch::auto_home_enabled();
        {
            for item in items.iter_mut() {
                if matches!(item.state, crate::themes::DepState::Installed(_))
                    || crate::themes::dep_home_source(item.role, &item.wanted).is_none()
                    || !(consent(item.role) || auto_home)
                {
                    continue;
                }
                match self.install_home_set(item.role, &item.wanted) {
                    Ok(name) => item.state = crate::themes::DepState::Installed(name),
                    Err(e) => {
                        let line = trf("⚠️ Couldn't download %1: %2", &[&crate::themes::dep_label(item.role, &item.wanted), &e]);
                        self.say(&line);
                    }
                }
            }
        }
        let mut install_error: Option<String> = None;
        let packages: Vec<String> = items
            .iter()
            .filter(|i| consent(i.role))
            .filter_map(|i| match &i.state {
                crate::themes::DepState::Installable(p) => Some(p.clone()),
                _ => None,
            })
            .fold(Vec::new(), |mut acc, p| {
                if !acc.contains(&p) {
                    acc.push(p);
                }
                acc
            });
        if !packages.is_empty() {
            let line = trf(
                "📥 Installing what the theme comes with: %1 (an administrator password may be requested)…",
                &[&packages.join(", ")],
            );
            self.say(&line);
            if let Err(e) = live_install_dep_packages(&packages) {
                install_error = Some(e);
            }
        }
        let data = crate::utils::data_home(xdg_data_home().as_deref(), &self.home);
        let roots = crate::themes::dep_search_roots(&self.home, &data, &env_str("XDG_DATA_DIRS"));
        let didnt_bring = tr("the package didn't bring it");
        for item in &items {
            use crate::themes::DepState;
            let name = match &item.state {
                DepState::Installed(name) => Some(name.clone()),
                // The truth is the disk after installation, not the helper's exit code.
                DepState::Installable(_) if consent(item.role) => live_find_dep(item.role, &item.wanted, &roots),
                _ => None,
            };
            let Some(name) = name else {
                lines.push(match &item.state {
                    DepState::Missing => crate::themes::dep_missing_line(item.role, &item.wanted),
                    _ => crate::themes::dep_not_installed_line(
                        item.role,
                        &item.wanted,
                        consent(item.role).then_some(install_error.as_deref().unwrap_or(&didnt_bring)),
                    ),
                });
                continue;
            };
            let ok = match item.role {
                crate::themes::DepRole::Icons => self.apply_icons_named(&name, &roots),
                crate::themes::DepRole::Cursor => {
                    let xdg = xdg_config_home();
                    let size = live_cursor_value(&self.de, &self.home, xdg.as_deref(), true)
                        .and_then(|v| crate::themes::parse_cursor_size(&v));
                    self.apply_cursor(&name, size).0
                }
            };
            lines.push(if ok {
                crate::themes::dep_applied_line(item.role, &name, &item.wanted)
            } else {
                crate::themes::dep_apply_failed_line(item.role, &name)
            });
        }
        lines
    }

    /// Apply an icon set already found on disk in one of `roots` (where GTK
    /// looks for it, including `~/.icons`).
    fn apply_icons_named(&mut self, name: &str, roots: &[String]) -> bool {
        let valid = self.icon_theme_valid(name) || {
            let read = |p: &str| read_text_lossy(p);
            let is_dir = |p: &str| Path::new(p).is_dir();
            crate::themes::find_installed_dep(crate::themes::DepRole::Icons, name, roots, &is_dir, &|_| Vec::new(), &read)
                .as_deref()
                == Some(name)
        };
        let de = self.de.clone();
        let home = self.home.clone();
        match crate::themes::apply_icon_plan_xdg(name, &de, &home, xdg_config_home().as_deref(), valid) {
            Some(plan) => self.run_theme_plan(&plan, name, &de),
            None => false,
        }
    }

    /// Put set `wanted` into the home directory from its home source
    /// ([`crate::themes::dep_home_source`]): only download and unpack, never
    /// run foreign install scripts, and find it on disk. `Ok(set name on
    /// disk)`; `Err` is a reason for the person.
    ///
    /// It is installed by the same installer as store themes, with an entry
    /// in the installed log, so "Remove" in "Appearance" will remove it.
    pub fn install_home_set(&mut self, role: crate::themes::DepRole, wanted: &str) -> Result<String, String> {
        use crate::themes::HomeSource;
        let Some(source) = crate::themes::dep_home_source(role, wanted) else {
            return Err(tr("Rexon doesn't know where to download it"));
        };
        let label = crate::themes::dep_label(role, wanted);
        self.say(&trf("📥 Downloading %1 into your home folder…", &[&label]));
        let failed = || tr("the download failed");
        match source {
            HomeSource::None => return Err(tr("Rexon doesn't know where to download it")),
            HomeSource::Ocs { content_id, file, .. } => {
                let category = match role {
                    crate::themes::DepRole::Icons => 132,
                    crate::themes::DepRole::Cursor => 107,
                };
                let io = crate::ocs::LiveOcsIo::new(&self.home);
                let preview_dir = io.preview_dir();
                let mut client = crate::ocs::OcsClient::new(io, &preview_dir);
                let work = mkdtemp_in(&std::env::temp_dir(), "rexon-deps-").map_err(|e| e.to_string())?;
                let dest = work.join(file).to_string_lossy().into_owned();
                let link_of = |item: &crate::models::ThemeItem| {
                    item.download_links
                        .iter()
                        .position(|l| l.get("name").and_then(serde_json::Value::as_str) == Some(file))
                };
                // Details come from the store cache (a day), gently on the site. The
                // store's file links are signed and expire over time: if the download
                // by the cache failed, once more with fresh details.
                let mut downloaded = None;
                for fresh in [false, true] {
                    let content = if fresh {
                        client.get_item(content_id)
                    } else {
                        client.get_item_cached(content_id, crate::ocs::DETAIL_TTL_SECS)
                    };
                    let Some(mut item) = content.map(|c| c.item) else { continue };
                    item.category_id = category;
                    let Some(idx) = link_of(&item) else { continue };
                    if client.download(&item, &dest, idx as isize, false) {
                        downloaded = Some(item);
                        break;
                    }
                }
                let result = match downloaded {
                    None => Err(failed()),
                    Some(item) => {
                        let (ok, msg, _names) = self.install(&item, &dest);
                        if ok { Ok(()) } else { Err(msg) }
                    }
                };
                let _ = std::fs::remove_dir_all(&work);
                result?;
            }
        }
        let data = crate::utils::data_home(xdg_data_home().as_deref(), &self.home);
        let roots = crate::themes::dep_search_roots(&self.home, &data, &env_str("XDG_DATA_DIRS"));
        // The truth is the disk after unpacking.
        live_find_dep(role, wanted, &roots).ok_or_else(|| tr("the download didn't bring it"))
    }

    /// Apply an already installed set `name` of role `role` (icons or cursor)
    /// the same way as the GTK theme sets in [`Self::apply_dep_items`]. If the
    /// set is not on disk, `false` without a single write.
    pub fn apply_installed_set(&mut self, role: crate::themes::DepRole, name: &str) -> bool {
        let data = crate::utils::data_home(xdg_data_home().as_deref(), &self.home);
        let roots = crate::themes::dep_search_roots(&self.home, &data, &env_str("XDG_DATA_DIRS"));
        let Some(found) = live_find_dep(role, name, &roots) else {
            return false;
        };
        match role {
            crate::themes::DepRole::Icons => self.apply_icons_named(&found, &roots),
            crate::themes::DepRole::Cursor => {
                let xdg = xdg_config_home();
                let size = live_cursor_value(&self.de, &self.home, xdg.as_deref(), true)
                    .and_then(|v| crate::themes::parse_cursor_size(&v));
                self.apply_cursor(&found, size).0
            }
        }
    }

    fn run_theme_plan(&mut self, plan: &crate::themes::Plan, theme: &str, de: &str) -> bool {
        self.run_theme_plan_report(plan, theme, de).ok
    }

    /// The same, with the refusal reason if it is known for sure (Deepin).
    fn run_theme_plan_report(
        &mut self,
        plan: &crate::themes::Plan,
        theme: &str,
        de: &str,
    ) -> crate::themes::CursorReport {
        let report = exec_apply_plan_report(&plan.steps);
        if report.ok {
            self.add_history(plan.history_kind, theme, de);
        }
        report
    }

    // ── APPLYING ────────────────────────────────────────────────────────────

    /// Port of `ThemeInstaller.apply`: `(success, message)`.
    ///
    /// The dispatcher is [`apply_action_for`]. Step 5a of merging the
    /// installers: a press first BUILDS the plan ([`Self::press_plan`], without
    /// a single launch or write), then the common [`crate::install_plan::exec`]
    /// executes it: the press's file writes under the common rollback,
    /// commands through `Ctx::run`. Actions still executed their own way
    /// ([`PressPlan::Own`]: colours in Plasma, desktop wallpaper, Conky, the
    /// login screen) go to [`Self::apply_own`]. After ANY application the GTK
    /// sanitizer must run: it repairs broken `gtk-{3,4}.0` links that would
    /// otherwise crash GTK4 applications.
    pub fn apply(
        &mut self,
        item: &crate::models::ThemeItem,
        theme_name: &str,
    ) -> (bool, String) {
        let home = self.home_path();
        let cat = crate::categories::category_by_id(&home, item.category_id);
        let action = apply_action_for(cat.as_ref(), theme_name);
        let home_s = self.home.clone();

        let plan = self.press_plan(cat.as_ref(), &action, theme_name);
        let (ok, msg) = self.run_press(&item.id, cat.as_ref(), theme_name, &action, plan);

        let mut message = msg.unwrap_or_else(|| apply_message(&action, ok).to_string());

        // A colour change (a GTK theme, a global theme, a colour scheme) must
        // not leave a terminal that takes the theme's colours unreadable: pin
        // the pair under the same section's entry, and "Restore as it was" will
        // restore it too.
        if ok && matches!(action, ApplyAction::Gtk | ApplyAction::KdeColors | ApplyAction::KdeGlobal) {
            if let Some(cat) = cat.as_ref() {
                if let Some(line) = crate::terminal_guard::guard_live(&self.de, &cat.key, theme_name, None) {
                    message.push('\n');
                    message.push_str(&line);
                }
            }
        }

        // CRITICAL: repair the gtk configs after ANY application.
        let changes = live_gtk_sanitize(&home_s);
        if !changes.is_empty() {
            let line = gtk_sanitizer_message(changes.len());
            self.say(&line);
        }
        self.say(&message);

        // An enabled add-on or a started widget is not a theme change.
        if ok && action != ApplyAction::DeSection {
            self.notify_theme_changed(theme_name);
        }
        (ok, message)
    }

    /// The plan of pressing "Apply", without a single command launch or write
    /// (step 5a). Live probes (whether `metadata.json` exists, whether the icon
    /// theme is valid, whether the GTK theme is dark, the Qt style switcher,
    /// what to call KWin with) are taken here and only for their own action;
    /// [`press_plan_with`] builds the plan from them.
    pub fn press_plan(&self, cat: Option<&ThemeCategory>, action: &ApplyAction, theme_name: &str) -> PressPlan {
        let de = self.de.as_str();
        let home = self.home.as_str();
        let xdg = xdg_config_home();
        let mut facts = PressFacts {
            de: de.to_string(),
            home: home.to_string(),
            xdg_config_home: xdg.clone(),
            ..PressFacts::default()
        };
        match action {
            ApplyAction::KdeGlobal => {
                facts.global_theme_ok = !live_global_theme_missing_metadata_json(home, theme_name);
                // Outside Plasma `plasma-apply-lookandfeel` must not be called:
                // without the window (a press not from "Appearance"), the look that
                // carries over.
                let choice = self.global_choice.or_else(|| {
                    (de != "kde").then_some(crate::themes::GlobalChoice { appearance: true, layout: false, parts: None })
                });
                facts.global = choice.map(|choice| {
                    let io = crate::animations::SystemIo;
                    let contents = crate::themes::lnf_contents_live(&io, home, theme_name).unwrap_or_default();
                    let mut press = crate::themes::GlobalPress { choice, contents, ..Default::default() };
                    press.sections = self.global_sections(&press);
                    press
                });
            }
            ApplyAction::Icons => facts.icons_valid = self.icon_theme_valid(theme_name),
            ApplyAction::Gtk => {
                facts.gtk_dark = gtk_theme_dark_live(theme_name, home);
                facts.gtk4 = Some(crate::gtk4_theme::facts_live(theme_name, home, xdg.as_deref()));
            }
            ApplyAction::Kvantum | ApplyAction::WidgetStyle => {
                let env = qt_style_env(de, home, xdg.as_deref());
                facts.qt_platform_theme = env.platform_theme.to_string();
                facts.qt_icon_theme = env.icon_theme;
            }
            ApplyAction::Aurorae => {
                facts.kwin_reconfigure = kwin_reconfigure_argv();
                facts.aurorae_library = crate::kdecoration::live_svg_library().to_string();
            }
            ApplyAction::Cursor => {
                self.cursor_facts(theme_name, &mut facts);
                // The size is not changed: take the current one.
                facts.cursor_size = live_cursor_value(de, home, xdg.as_deref(), true)
                    .and_then(|v| crate::themes::parse_cursor_size(&v));
            }
            ApplyAction::DeSection => {
                let kind = cat.and_then(|c| crate::categories::category_kind(&c.key)).unwrap_or("");
                facts.desktop = Some(crate::de_apply::press_plan(
                    &crate::animations::SystemIo,
                    &crate::de_apply::Ctx::live(),
                    kind,
                    theme_name,
                ));
            }
            ApplyAction::LockWallpaper => {
                let install_dir = cat.map(|c| c.install_dir.clone()).unwrap_or_default();
                let is_file = Path::new(&join(&install_dir, theme_name)).is_file();
                facts.lock = Some(LockFacts { install_dir, is_file, treeland: crate::dde::treeland_session_live() });
            }
            ApplyAction::SoundTheme => {
                let dirs = crate::qt_settings::live_xdg_sound_theme_dirs();
                // ⭐ Fixed during the port: the original looked strictly for
                // `kwriteconfig6` and refused to write at all on Plasma 5, though
                // `kwriteconfig5` exists there. We use the common synonym resolver.
                let path = path_env();
                let tool = |t: &str| crate::utils::resolve_tool(t, &[], path.as_deref()).unwrap_or_default();
                facts.sound = Some(SoundFacts {
                    exists: crate::qt_settings::sound_theme_exists(&dirs, theme_name),
                    kwrite: tool("kwriteconfig6"),
                    kread: tool("kreadconfig6"),
                    has_gsettings: have("gsettings"),
                });
            }
            ApplyAction::KdeColors => {
                if let crate::shell_palette::SchemeWay::Layer { lxqt } = crate::shell_palette::scheme_way(de) {
                    let ctx = crate::shell_palette::SchemeCtx::live(de);
                    facts.colors = Some(crate::shell_palette::scheme_layer_press(
                        &crate::animations::SystemIo,
                        &ctx,
                        lxqt,
                        theme_name,
                    ));
                }
            }
            _ => {}
        }
        press_plan_with(action, theme_name, &facts)
    }

    /// Section plans for the parts of a global theme the person ticked
    /// ([`crate::themes::lnf_section_parts`]): the same "Apply" button of the
    /// "Icons", "Cursors", "Colour schemes", "Application style" sections with
    /// its probes; title bar buttons use the "Title bar buttons" section plan.
    fn global_sections(
        &self,
        press: &crate::themes::GlobalPress,
    ) -> std::collections::BTreeMap<crate::themes::LnfPart, Result<Vec<crate::themes::ApplyStep>, String>> {
        use crate::themes::LnfPart;
        let de = self.de.as_str();
        let sel = crate::themes::global_selection(press, de);
        let mut out = std::collections::BTreeMap::new();
        for &part in crate::themes::lnf_section_parts(de) {
            if sel & part.bit() == 0 {
                continue;
            }
            let got = if part == LnfPart::TitlebarLayout {
                let io = crate::animations::SystemIo;
                // The desktop is the one the theme is installed for, not the session.
                let ctx = crate::animations::LookCtx::from_env().for_chosen(de);
                crate::themes::lnf_buttons_steps(&io, &ctx, &press.contents)
            } else {
                let action = match part {
                    LnfPart::Colors => ApplyAction::KdeColors,
                    LnfPart::WidgetStyle => ApplyAction::WidgetStyle,
                    LnfPart::Icons => ApplyAction::Icons,
                    _ => ApplyAction::Cursor,
                };
                let name = crate::themes::lnf_section_value(&press.contents, part).unwrap_or_default();
                match self.press_plan(None, &action, &name) {
                    PressPlan::Steps { steps, .. } => Ok(steps
                        .into_iter()
                        .filter_map(|s| match s {
                            crate::install_plan::Step::Apply(a) => Some(a),
                            crate::install_plan::Step::File(_) => None,
                        })
                        .collect()),
                    PressPlan::Refused(why) => Err(why.unwrap_or_else(|| apply_message(&action, false))),
                    _ => Err(apply_message(&action, false)),
                }
            };
            out.insert(part, got);
        }
        out
    }

    /// Cursor probes for the plan: whether the theme is safe and whether it is
    /// the environment's default cursor (asked only if it is unsafe).
    fn cursor_facts(&self, theme: &str, facts: &mut PressFacts) {
        facts.cursor_safe = self.cursor_safe(theme);
        facts.cursor_standard =
            !facts.cursor_safe && live_standard_theme(&self.de, "cursor").as_deref() == Some(theme);
    }

    /// Execute the press plan: `(success, message)`; `None` means the common
    /// [`apply_message`]. The common path of "Apply" and of the cursor buttons
    /// (the set theme of a GTK theme, a size change).
    fn run_press(
        &mut self,
        item_id: &str,
        cat: Option<&ThemeCategory>,
        theme_name: &str,
        action: &ApplyAction,
        plan: PressPlan,
    ) -> (bool, Option<String>) {
        match plan {
            PressPlan::Steps { steps, history, say } => {
                let report = self.exec_press(item_id, cat, theme_name, &steps);
                let ok = report.ok;
                let mut msg = match say {
                    PressSay::Plain => report.reason.clone().filter(|_| !ok),
                    PressSay::Cursor => Some(crate::themes::cursor_apply_message(&self.de, &report)),
                    PressSay::Done(text) if ok => Some(text),
                    PressSay::Done(_) => report.reason.clone(),
                    PressSay::Pair { done, .. } if ok => Some(done),
                    PressSay::Pair { failed, .. } => Some(report.reason.clone().unwrap_or(failed)),
                };
                if ok {
                    if let Some((kind, de)) = history {
                        self.add_history(kind, theme_name, &de);
                    }
                }
                // The icons and cursor the GTK theme was designed with, the same
                // way as the section buttons, in all environments.
                if ok && *action == ApplyAction::Gtk {
                    let lines = self.apply_theme_deps(theme_name);
                    if !lines.is_empty() {
                        msg = Some(
                            std::iter::once(apply_message(action, true))
                                .chain(lines)
                                .collect::<Vec<_>>()
                                .join("\n"),
                        );
                    }
                }
                (ok, msg)
            }
            PressPlan::Desktop { steps, check } => {
                let (ok, msg) = self.exec_desktop(item_id, cat, theme_name, &steps, &check);
                (ok, Some(msg))
            }
            PressPlan::Refused(m) => (false, m),
            PressPlan::Nothing => (true, None),
            PressPlan::Own => self.apply_own(action, cat, theme_name),
        }
    }

    /// An environment section button through the common executor under
    /// rollback: commands via the live `de_apply` machine (`SystemIo`), the
    /// result via the environment check. A file step failure returns what was
    /// written; success records a "what was there" snapshot and the section
    /// entry in the common log ([`Self::record_press`]); not accepted by the
    /// environment means what was written stays (as before), and the copies
    /// are erased.
    fn exec_desktop(
        &mut self,
        item_id: &str,
        cat: Option<&ThemeCategory>,
        theme_name: &str,
        steps: &[crate::install_plan::Step],
        check: &crate::de_apply::Check,
    ) -> (bool, String) {
        use crate::install_plan::Undo;
        let io = crate::animations::SystemIo;
        let ctx = crate::de_apply::Ctx::live();
        let mut undo = Undo::new().keeping_values();
        match crate::de_apply::exec_plan(&io, steps, &mut undo) {
            Err(f) => {
                undo.finish(false);
                (false, press_failed_report(&f).reason.unwrap_or_default())
            }
            Ok(err) => {
                let (ok, msg) = crate::de_apply::check(&io, &ctx, check, err);
                match cat {
                    Some(cat) if ok => self.record_press(item_id, cat, theme_name, undo),
                    _ => undo.finish(true),
                }
                (ok, msg)
            }
        }
    }

    /// Execute the press plan with the common executor on the live machine.
    ///
    /// A write failure returns everything the press wrote (the common
    /// rollback). On success the copies set aside by the rollback are not
    /// erased but become the "what was there" snapshot, and the section entry
    /// goes to the common log ([`Self::record_press`]). Not applied
    /// (`ok = false`): as before, what was written stays, the copies are
    /// erased.
    fn exec_press(
        &mut self,
        item_id: &str,
        cat: Option<&ThemeCategory>,
        theme_name: &str,
        steps: &[crate::install_plan::Step],
    ) -> crate::themes::CursorReport {
        use crate::install_plan::{exec_undoable, Ctx, Undo};
        let ctx = Ctx::live(true, false, None, crate::dde::treeland_session_live());
        let mut undo = Undo::new().keeping_values();
        match exec_undoable(steps, &mut Vec::new(), &ctx, &mut undo) {
            Err(f) => {
                undo.finish(false);
                press_failed_report(&f)
            }
            Ok(report) => {
                match cat {
                    Some(cat) if report.apply.ok => self.record_press(item_id, cat, theme_name, undo),
                    _ => undo.finish(true),
                }
                report.apply
            }
        }
    }

    /// The "what was there" snapshot of a successful press and the section
    /// entry in the common log ([`crate::install_journal::Applied`]): files and
    /// the previous desktop values (`gsettings`/`dconf`, `xfconf`). If the
    /// press touched neither, there is nothing to record.
    fn record_press(&mut self, item_id: &str, cat: &ThemeCategory, theme: &str, mut undo: crate::install_plan::Undo) {
        use crate::install_journal as journal;
        let rexon = PathBuf::from(crate::utils::config_home(xdg_config_home().as_deref(), &self.home)).join("rexon");
        let dir = journal::applied_snapshot_dir(&rexon, &cat.key);
        let first = std::fs::read_dir(&dir)
            .map(|rd| {
                rd.flatten()
                    .filter_map(|e| e.file_name().to_str().and_then(|n| n.parse::<usize>().ok()))
                    .max()
                    .map_or(1, |n| n + 1)
            })
            .unwrap_or(1);
        let values = undo.take_values();
        let touched = undo.keep_into(&dir, first);
        if touched.is_empty() && values.is_empty() {
            return;
        }
        let key = journal::key_of(journal::Source::Store, item_id);
        let item = (!item_id.is_empty() && journal::view_at(&rexon).items.contains_key(&key)).then_some(key);
        let text = |p: &Path| p.to_string_lossy().into_owned();
        let applied = journal::Applied {
            section: cat.key.clone(),
            theme: theme.to_string(),
            item,
            applied_at: Some(now_local_iso()),
            snapshot: text(&dir),
            files: touched
                .iter()
                .map(|t| journal::AppliedFile { path: text(&t.path), before: t.before.as_deref().map(text), dir: t.dir })
                .collect(),
        };
        let values = values.into_iter().map(journal::AppliedValue::from).collect();
        if let Err(e) = journal::record_applied_values_at(&rexon, applied, values) {
            crate::log_warn!("install", "«Применить» не записано в общий журнал ({}) — {e}", rexon.display());
        }
    }

    /// "Apply" actions still executed their own way ([`PressPlan::Own`]):
    /// `(success, message)`.
    fn apply_own(
        &mut self,
        action: &ApplyAction,
        cat: Option<&ThemeCategory>,
        theme_name: &str,
    ) -> (bool, Option<String>) {
        let de = self.de.clone();
        let mut ok = false;
        let msg: Option<String>;
        match action {
            ApplyAction::KdeColors => {
                // Only Plasma comes here (`plasma-apply-colorscheme`); outside
                // Plasma the press plan builds the Rexon layers.
                let (o, m) = crate::shell_palette::apply_scheme_live(&de, theme_name);
                ok = o;
                if ok {
                    self.add_history("kde-colors", theme_name, &de);
                }
                msg = Some(m);
            }
            ApplyAction::WallpaperBackground => {
                let install_dir = cat.map(|c| c.install_dir.clone()).unwrap_or_default();
                let (o, m) = self.apply_wallpaper_background(&join(&install_dir, theme_name));
                ok = o;
                msg = Some(m);
            }
            ApplyAction::WallpaperPack => {
                let install_dir = cat.map(|c| c.install_dir.clone()).unwrap_or_default();
                let (o, m) = self.apply_wallpaper_pack(&install_dir, theme_name);
                ok = o;
                msg = Some(m);
            }
            // Only Conky comes here (starting and stopping processes); a theme and
            // an add-on are built by the press plan.
            ApplyAction::DeSection => {
                let kind = cat
                    .as_ref()
                    .and_then(|c| crate::categories::category_kind(&c.key))
                    .unwrap_or("");
                let (o, m) = crate::de_apply::press(
                    &crate::animations::SystemIo,
                    &crate::de_apply::Ctx::live(),
                    kind,
                    theme_name,
                );
                ok = o;
                msg = Some(m);
            }
            ApplyAction::SystemTheme => {
                let (o, m) = match cat {
                    Some(c) => self.apply_system_theme(c, theme_name),
                    None => (false, apply_message(&ApplyAction::Noop, false)),
                };
                ok = o;
                msg = Some(m);
            }
            // The rest is built by the plan (`press_plan`) and does not come here.
            _ => msg = Some(apply_message(action, false)),
        }
        (ok, msg)
    }

    /// The system part's environment for the login screen and the boot
    /// splash: one for installation and for "Apply".
    fn live_root_env(kind: &str) -> RootEnv<'static> {
        let is_plasmalogin =
            kind == "sddm" && crate::system_plan::is_plasma_login(&active_login_manager());
        let detector = crate::system::DistroDetector::detect_live();
        RootEnv {
            is_plasmalogin,
            has_pkexec: have("pkexec"),
            system_helper_path: crate::installer::system_helper_path(),
            system_helper_exists: crate::system_plan::is_exec_file(
                crate::installer::system_helper_path(),
            ),
            is_steamos_or_immutable: detector.is_steamos() || detector.is_immutable(),
            is_steamos: crate::distro_compat::is_steamos_id(&detector.distro_id()),
            family_is_debian: detector.family_live() == crate::system::PkgFamily::Debian,
        }
    }

    /// "Apply" for a login screen (SDDM) or boot splash (Plymouth) theme: the
    /// root helper selects a theme already lying in the system directory. The
    /// password goes through the same polkit action as installation
    /// (`auth_admin_keep`): "Install and apply" asks for it once.
    fn apply_system_theme(&mut self, cat: &ThemeCategory, theme: &str) -> (bool, String) {
        let env = Self::live_root_env(&cat.apply_kind);
        let line = trf(
            "🔐 Applying “%1” (an administrator password may be requested)…",
            &[&theme],
        );
        self.say(&line);
        let (ok, msg) = system_theme_apply_with(
            &env,
            cat,
            theme,
            &crate::system_l10n::lang_args(env.system_helper_path),
            &|p| Path::new(p).is_dir(),
            &|argv, timeout| run_capture(argv, Some(timeout)),
        );
        if ok && cat.apply_kind == "sddm" {
            if let Some(note) = sddm_elsewhere_note(&active_login_manager()) {
                return (true, format!("{}. {note}", trf("✅ Applied: %1", &[&theme])));
            }
        }
        (ok, msg)
    }

    /// Tell the bridge the theme has changed (`bridge.json`, the `current_theme` field).
    fn notify_theme_changed(&mut self, theme_name: &str) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        let bridge = crate::rexon_bridge::RexonBridge::new_default();
        let _ = bridge.ensure_dirs();
        let _ = bridge.notify_theme_changed(
            theme_name,
            &crate::engine_update::now_iso(now.as_secs() as i64),
            now.as_millis() as u64,
        );
    }

    /// Set a picture as the desktop background with the process's COMMON
    /// wallpaper executor ([`crate::wallpaper_exec::live_slot`]), the same as
    /// in the "Wallpapers", "Settings" and "Steam" tabs; there is no second
    /// one in the project.
    ///
    /// ⭐ Fix (rule #12): previously a cut-down plan executor of its own lived
    /// here. It could not start the swww daemon or hold swaybg/wbg and
    /// answered "set it in the Wallpaper Engine tab", so on Hyprland (without
    /// a running hyprpaper), niri, river and Wayfire store wallpapers were
    /// never set; it did not check the pcmanfm-qt write either (LXQt, D12). The
    /// common executor also removes our live wallpapers (video, web) before
    /// the picture: previously a static one landed under their surface.
    fn apply_wallpaper_background(&mut self, path: &str) -> (bool, String) {
        if !Path::new(path).is_file() {
            return (false, trf("Wallpaper file not found: %1", &[&path]));
        }
        let slot = crate::wallpaper_exec::live_slot();
        let (ok, detail) = apply_static_with_slot(&slot, path);
        if ok {
            return (true, tr("✅ Wallpaper set as the desktop background."));
        }
        // Graceful degradation: the installation itself happened, the background did not take.
        (
            true,
            trf(
                "Installed to the wallpaper folder. ⚠️ Couldn't set it as the \
                 background automatically (%1).",
                &[&detail],
            ),
        )
    }

    /// Set as the background a picture FROM a wallpaper PACK: which one is
    /// decided by the core ([`pick_pack_image`]); setting the background goes
    /// through the same common engine.
    ///
    /// The screen size comes from the common output query ([`screen_size`]),
    /// by the largest monitor, because one picture from the pack goes to the
    /// whole desktop. The core understands "I do not know": then the choice
    /// goes by file name.
    fn apply_wallpaper_pack(&mut self, install_dir: &str, theme_name: &str) -> (bool, String) {
        let pack_dir = join(install_dir, theme_name);
        if !Path::new(&pack_dir).is_dir() {
            return (false, trf("Wallpaper pack folder not found: %1", &[&pack_dir]));
        }
        let snap = install_tree_snapshot(&pack_dir, &[]);
        let images = pack_images(&snap, &pack_dir);
        let (w, h) = screen_size();
        let Some(chosen) = pick_pack_image(&images, w, h) else {
            return (true, pack_no_images_message().to_string());
        };
        let line = trf("🖼️ Picture chosen from the pack: %1", &[&basename(&chosen)]);
        self.say(&line);
        self.apply_wallpaper_background(&chosen)
    }

    /// Port of `ThemeInstaller.install_and_apply` (install and enable in one
    /// action).
    pub fn install_and_apply(
        &mut self,
        item: &crate::models::ThemeItem,
        archive_path: &str,
    ) -> (bool, String) {
        let (ok, msg, names) = self.install(item, archive_path);
        if !ok {
            return (false, msg);
        }
        let Some(first) = names.first().cloned() else {
            return (true, msg);
        };
        let (applied_ok, apply_msg) = self.apply(item, &first);
        if applied_ok {
            (true, format!("✅ {msg}. {apply_msg}"))
        } else {
            (true, format!("{msg}. ⚠️ {apply_msg}"))
        }
    }

    // ── REMOVAL ─────────────────────────────────────────────────────────────

    /// Port of `ThemeInstaller.uninstall`: remove the files by the log and
    /// forget the entry.
    ///
    /// ⭐ Fixed during the port: the original answered "Removed" even when the
    /// log could not be written after removal: the theme disappeared from disk
    /// but stayed in the installed list forever. Now the divergence from the
    /// disk is named right in the answer.
    pub fn uninstall(&mut self, content_id: &str) -> (bool, String) {
        let log_path =
            crate::ocs_store::install_log_path_xdg(xdg_config_home().as_deref(), &self.home);
        let cats = crate::categories::categories(Path::new(&self.home));
        let home = self.home.clone();
        uninstall_from_log(&home, &log_path, content_id, &cats, &mut |cat, theme| {
            self.remove_system_theme(cat, theme)
        })
    }

    /// "Remove" for a system theme without an owning package and without a log
    /// entry (placed by hand or by someone else's installer), by the path from
    /// the scan. The same root helper as for a store theme: it checks once
    /// more that the directory does not belong to a package and the theme is
    /// not currently selected.
    pub fn remove_loose_system_theme(&mut self, path: &str) -> (bool, String) {
        let cats = crate::categories::categories(Path::new(&self.home));
        let Some((cat, theme)) = system_theme_of_path(&cats, path) else {
            return (false, trf("Couldn't remove “%1”: unexpected theme name.", &[&path]));
        };
        let (ok, why) = self.remove_system_theme(&cat, &theme);
        if !ok {
            return (false, why);
        }
        if path_present(path) {
            return (false, trf("Couldn't remove %1.", &[&path]));
        }
        (true, uninstall_message(1, &theme))
    }

    /// "Remove" for a login screen (SDDM) or boot splash (Plymouth) theme: the
    /// directory in `/usr/share/<kind>/themes` is removed by the root helper,
    /// with the same polkit action as installation and "Apply".
    fn remove_system_theme(&mut self, cat: &ThemeCategory, theme: &str) -> (bool, String) {
        let mut env = Self::live_root_env(&cat.apply_kind);
        // Plasma Login Manager prevents APPLYING an SDDM theme, but not removing it.
        env.is_plasmalogin = false;
        let active = crate::installed_scan::live_system_theme_active(&cat.apply_kind);
        if !active.iter().any(|a| a == theme) {
            let line = trf(
                "🔐 Removing “%1” (an administrator password may be requested)…",
                &[&theme],
            );
            self.say(&line);
        }
        system_theme_remove_with(
            &env,
            cat,
            theme,
            &crate::system_l10n::lang_args(env.system_helper_path),
            &active,
            &|argv, timeout| run_capture(argv, Some(timeout)),
        )
    }
}

/// A system theme by a path from the installed log: the section that needs
/// root (`requires_root`) and the theme name. Only a direct child of the
/// section's themes directory with a safe name; anything else is not a
/// system theme.
pub fn system_theme_of_path(cats: &[ThemeCategory], path: &str) -> Option<(ThemeCategory, String)> {
    let p = rstrip_slashes(path);
    let (parent, name) = p.rsplit_once('/')?;
    if !crate::ext_manager::is_safe_sys_name(name) {
        return None;
    }
    cats.iter()
        .find(|c| c.requires_root && !c.install_dir.is_empty() && rstrip_slashes(&c.install_dir) == parent)
        .map(|c| (c.clone(), name.to_string()))
}

/// Whether something exists at the path (the link is not dereferenced).
fn path_present(p: &str) -> bool {
    std::fs::symlink_metadata(p).is_ok()
}

/// This machine's home trash (`$XDG_DATA_HOME/Trash`).
fn home_trash_live() -> String {
    let xdg_data = std::env::var("XDG_DATA_HOME").ok();
    crate::utils::home_trash_dir(xdg_data.as_deref(), &std::env::var("HOME").unwrap_or_default())
}

/// Remove purchase `content_id` by log `log_path`: a port of
/// `ThemeInstaller.uninstall`, the decisions and the local removal.
///
/// Paths of system sections ([`system_theme_of_path`]) go to
/// `remove_system` (the root helper); the rest are removed here.
///
/// ⛔ Fix of 09.10.2026 (the "Removed (1 paths)" defect): previously a
/// removal error was swallowed, the path counted as removed, and the log
/// entry was always erased. For a Plymouth/SDDM theme from the store this
/// gave "Removed", an empty log and the theme remaining in `/usr/share`.
/// Now only what is no longer on disk counts as removed; if anything
/// remains, the log entry is not touched ("Remove" can be pressed again),
/// and the person sees the reason.
pub fn uninstall_from_log(
    home: &str,
    log_path: &str,
    content_id: &str,
    cats: &[ThemeCategory],
    remove_system: &mut dyn FnMut(&ThemeCategory, &str) -> (bool, String),
) -> (bool, String) {
    let mut data = crate::install_journal::store_log_view(log_path);
    let Some(rec) = data.get(content_id) else {
        return (false, tr("Theme not found in the installed-themes log"));
    };
    let paths: Vec<String> = rec["paths"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let name = rec["name"].as_str().unwrap_or(content_id).to_string();

    let mut removed = 0usize;
    let mut problems: Vec<String> = Vec::new();
    let mut local: Vec<String> = Vec::new();
    for p in &paths {
        // A Conky widget: first kill it and remove the autostart, then delete.
        crate::de_apply::conky_forget_widget(
            &crate::animations::SystemIo,
            &crate::de_apply::Ctx::live(),
            p,
        );
        match system_theme_of_path(cats, p) {
            Some((cat, theme)) => {
                if !path_present(p) {
                    continue; // already removed: nothing to remove
                }
                let (ok, msg) = remove_system(&cat, &theme);
                if ok && !path_present(p) {
                    removed += 1;
                } else if !ok {
                    problems.push(msg);
                }
            }
            None => local.push(p.clone()),
        }
    }
    // What to remove here comes from the common install log (step 3g of
    // merging the installers): what the purchase wrote and only its own,
    // together with the `~/.themes` link to a GTK theme. System themes were
    // already removed by the helper above, so their steps are not repeated.
    // No entry (the common log was not read): the old way, by disk.
    let journal = crate::install_journal::store_view(log_path);
    let entry = journal.items.get(&crate::install_journal::key_of(crate::install_journal::Source::Store, content_id));
    let by_journal = entry.is_some();
    let steps: Vec<ExtStep> = match entry {
        Some(e) => crate::install_journal::removal_plan(e, &crate::install_journal::probe_live)
            .steps
            .into_iter()
            .filter(|st| {
                crate::install_journal::removal_path(st).is_some_and(|p| system_theme_of_path(cats, &p).is_none())
            })
            .collect(),
        None => plan_uninstall(&local, &|p| Path::new(p).is_dir(), &|p| Path::new(p).is_file()),
    };
    // The removed purchase paths are counted in "Removed (N paths)"; the
    // service link `~/.themes` is not (cleanup removed it before as well,
    // without counting).
    let counted = |path: &str| {
        let path = rstrip_slashes(path);
        paths.iter().any(|p| rstrip_slashes(p) == path)
    };
    // A Conky widget goes to the trash rather than being erased, as with the
    // "Remove" of its tile (`theme_removal::commit`): a widget may carry the
    // person's edits.
    let conky_root = format!("{}/", crate::de_apply::conky_root(&crate::de_apply::Ctx::live()));
    let trash = home_trash_live();
    let date = crate::utils::trash_deletion_date_now();
    for step in &steps {
        let path = match step {
            ExtStep::RemoveTree { path, .. } | ExtStep::RemoveFile { path } => path,
            // There is no removal plan from other steps: skip silently.
            _ => continue,
        };
        if path.starts_with(&conky_root) {
            match crate::utils::move_to_trash(Path::new(path), Path::new(&trash), &date) {
                Ok(_) => removed += usize::from(counted(path)),
                // TRANSLATORS: %1 is a Conky widget name, %2 the system error.
                Err(e) => problems.push(trf(
                    "Couldn't move “%1” to the trash: %2. The widget stays where it was.",
                    &[&name, &e],
                )),
            }
            continue;
        }
        let res = match step {
            ExtStep::RemoveTree { .. } => std::fs::remove_dir_all(path),
            _ => std::fs::remove_file(path),
        };
        match res {
            Ok(()) => {
                removed += usize::from(counted(path));
                if by_journal {
                    // The ~/.themes link was removed by the log; spice leftovers
                    // (translations, settings) are not written to the log, so their
                    // cleanup stays.
                    crate::cinnamon_spices::cleanup_after_removal(
                        &crate::cinnamon_spices::SpiceEnv::for_home(home),
                        path,
                    );
                } else {
                    // The same cleanup as "Remove" on the tile (the ~/.themes link,
                    // spice leftovers): one behaviour for both removals.
                    crate::theme_removal::tidy_after_removal(home, path);
                }
            }
            // TRANSLATORS: %1 is a file or folder path, %2 is the system error.
            Err(e) => problems.push(trf("Couldn't remove %1: %2.", &[path, &e.to_string()])),
        }
    }

    let left: Vec<&String> = paths.iter().filter(|p| path_present(p)).collect();
    if !problems.is_empty() || !left.is_empty() {
        let mut message = if problems.is_empty() {
            trf("Couldn't remove %1.", &[&left.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")])
        } else {
            problems.join(" ")
        };
        message.push(' ');
        message.push_str(&trf(
            "“%1” stays in the list of installed themes.",
            &[&name],
        ));
        return (false, message);
    }

    crate::ocs_store::forget_installed(&mut data, content_id);
    // Parts that were placed as subfolders into the removed folder (window
    // frames in a GTK theme, `plan_nested_part`) went with it, and their
    // entries are forgotten too.
    let gone_inside: Vec<String> = data
        .iter()
        .filter(|(_, rec)| {
            let inner: Vec<&str> = rec["paths"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
            !inner.is_empty()
                && inner.iter().any(|q| paths.iter().any(|p| q.starts_with(&format!("{}/", rstrip_slashes(p)))))
                && !inner.iter().any(|q| path_present(q))
        })
        .map(|(id, _)| id.clone())
        .collect();
    for id in gone_inside {
        crate::ocs_store::forget_installed(&mut data, &id);
    }
    let mut message = uninstall_message(removed, &name);
    if let Err(e) = crate::install_journal::put_store_log(log_path, &data) {
        message.push(' ');
        message.push_str(&trf(
            "⚠️ The installed-themes log wasn't updated (%1) — the theme will stay \
             in the list until the next successful write.",
            &[&e],
        ));
    }
    (true, message)
}

/// The first `n` bytes of a file (for recognising a picture without a name).
fn read_head(path: &str, n: usize) -> Vec<u8> {
    use std::io::Read as _;
    let Ok(mut f) = std::fs::File::open(path) else {
        return Vec::new();
    };
    let mut buf = vec![0u8; n];
    match f.read(&mut buf) {
        Ok(read) => {
            buf.truncate(read);
            buf
        }
        Err(_) => Vec::new(),
    }
}

/// A picture as the desktop background via the slot executor: `(did it
/// take, executor text)`. We do not invent a wallpaper name for the bridge
/// (empty means "I do not know", see
/// [`crate::wallpaper_exec::WallpaperExec::apply`]).
fn apply_static_with_slot<S: crate::wallpaper_exec::WpSys + Default>(
    slot: &crate::wallpaper_exec::ExecSlot<S>,
    path: &str,
) -> (bool, String) {
    crate::wallpaper_exec::with_slot(slot, |exec| {
        let res = exec.apply(path, "static", "");
        (res.ok, res.message)
    })
}

/// The screen size a picture from a pack is measured against; `(0, 0)`
/// means there is nobody to ask.
///
/// The original asked Qt (the "Appearance" tab runs inside the
/// application); from the core the answer comes from the COMMON output
/// query ([`crate::wallpaper_exec::live_screen_outputs`]), the same one
/// that drives the markers in other engines' commands and the per-monitor
/// wallpaper layout. There is no `xrandr` parsing of its own here any more:
/// it took "the first resolution with an asterisk", and on a machine with
/// different monitors the pack was chosen by the first active mode, so the
/// large monitor got a picture stretched from the smaller one.
///
/// ⚠️ The measure is the LARGEST output. ONE picture from the pack goes to
/// the whole desktop (it is set by the common static wallpaper engine,
/// which covers the whole desktop; this path cannot do a picture per
/// monitor), and of the two evils "the large monitor shows its own size,
/// the small one scales down" is noticeably better than one stretched
/// from small to large.
///
/// ⚠️ Foolproofing: no query tool, the query failed, zero outputs, the
/// resolution did not parse: the answer is `(0, 0)`, and the core
/// understands it as "I do not know": the picture choice falls back to
/// sorting by name, exactly as before any enumeration of outputs.
fn screen_size() -> (u64, u64) {
    let session = crate::wallpaper_engine::detect_session_type(
        &env_str("XDG_SESSION_TYPE"),
        &env_str("WAYLAND_DISPLAY"),
        &env_str("DISPLAY"),
    );
    let outputs = crate::wallpaper_exec::live_screen_outputs(&session);
    crate::wallpaper_exec::largest_output(&outputs)
        .and_then(|o| o.size())
        .unwrap_or((0, 0))
}

// ============================================================================
// Parity checks of the live executor
// ============================================================================
//
// What is checked is the WORK on a real filesystem, not the plan texts
// (the texts are checked by `tests/appearance_installer_parity.rs`): was
// the directory created, did the file land, was the downloaded path
// substituted for an empty `src`, does the log survive a rewrite, does
// unpacking refuse an escape from the directory. Everything temporary is
// under `std::env::temp_dir()` and cleaned up afterwards.
//
// ⛔ The parity checks do NOT touch environment variables: `HOME` and
// `XDG_CONFIG_HOME` are read by neighbouring modules (and their own
// checks), and substituting them on a live machine sends the write into
// the user's REAL profile. So the log path in the check is set explicitly,
// not through `LiveInstaller::record`.
#[cfg(test)]
mod tests {
    use super::*;

    /// "Apply" for an SDDM theme with another login screen says the theme will
    /// take effect only with SDDM; with SDDM or an unknown one, the previous
    /// result.
    #[test]
    fn sddm_theme_note_names_other_login_manager() {
        let n = sddm_elsewhere_note("lightdm").unwrap();
        assert!(n.contains("LightDM") && n.contains("SDDM"), "{n}");
        assert!(sddm_elsewhere_note("gdm3").unwrap().contains("GDM"));
        for none in ["sddm", " sddm\n", "", "что-то-своё"] {
            assert_eq!(sddm_elsewhere_note(none), None, "{none:?}");
        }
    }

    /// A fake machine for the wallpaper executor: only the named programs
    /// exist, commands and launches are recorded, nothing is executed.
    #[derive(Default)]
    struct WpFake {
        tools: Vec<&'static str>,
        files: Vec<String>,
        log: Vec<String>,
        clock: f64,
    }

    impl crate::wallpaper_exec::WpSys for WpFake {
        fn run(&mut self, argv: &[String], _t: f64) -> Option<crate::session_splash::RunOut> {
            self.log.push(format!("run {}", argv.join(" ")));
            Some(crate::session_splash::RunOut { rc: 0, stdout: String::new(), stderr: String::new() })
        }
        fn spawn(&mut self, argv: &[String], _new_session: bool) -> Result<i64, String> {
            self.log.push(format!("spawn {}", argv.join(" ")));
            Ok(self.log.len() as i64)
        }
        fn poll(&mut self, _p: i64) -> Option<i64> {
            None
        }
        fn pid(&mut self, p: i64) -> Option<i64> {
            Some(1000 + p)
        }
        fn kill_process(&mut self, p: i64) {
            self.log.push(format!("kill {p}"));
        }
        fn has_tool(&mut self, tool: &str) -> bool {
            self.tools.contains(&tool)
        }
        fn package_hint(&mut self, _tool: &str) -> String {
            String::new()
        }
        fn exists(&mut self, path: &str) -> bool {
            self.files.iter().any(|f| f == path)
        }
        fn is_dir(&mut self, _path: &str) -> bool {
            false
        }
        fn listdir(&mut self, _path: &str) -> Vec<String> {
            Vec::new()
        }
        fn read_text(&mut self, _path: &str) -> Option<String> {
            None
        }
        fn write_text(&mut self, _path: &str, _content: &str) {}
        fn remove_file(&mut self, _path: &str) {}
        fn env(&mut self, name: &str) -> String {
            if name == "HOME" { "/fake-home".into() } else { String::new() }
        }
        fn monotonic(&mut self) -> f64 {
            self.clock += 1.0;
            self.clock
        }
        fn sleep(&mut self, seconds: f64) {
            self.clock += seconds;
        }
        fn bridge_notify(&mut self, _live: bool, _name: &str, _announce: bool) {}
    }

    fn wp_slot(de: &str, tools: &[&'static str], path: &str) -> crate::wallpaper_exec::ExecSlot<WpFake> {
        let sys = WpFake { tools: tools.to_vec(), files: vec![path.to_string()], ..WpFake::default() };
        std::sync::Arc::new(std::sync::Mutex::new(Some(crate::wallpaper_exec::WallpaperExec::with_env(sys, de, "wayland"))))
    }

    fn wp_log(slot: &crate::wallpaper_exec::ExecSlot<WpFake>) -> Vec<String> {
        crate::wallpaper_exec::with_slot(slot, |e| e.sys_mut().log.clone())
    }

    /// Store wallpapers on tiling compositors are set by the process's COMMON
    /// executor: the swww daemon is started, swaybg holds the background. The
    /// former own executor of "Appearance" rejected these steps, and on
    /// niri/river/Wayfire/Hyprland without hyprpaper the picture never took.
    #[test]
    fn store_wallpaper_on_tiling_goes_through_shared_executor() {
        let pic = "/h/.local/share/wallpapers/лес.jpg";
        for de in ["niri", "river", "wayfire", "hyprland"] {
            let slot = wp_slot(de, &["swaybg"], pic);
            let (ok, _) = apply_static_with_slot(&slot, pic);
            assert!(ok, "{de}");
            let log = wp_log(&slot);
            assert!(log.iter().any(|l| l == &format!("spawn swaybg -i {pic} -m fill")), "{de}: {log:?}");
            // A second picture replaces the previous background holder rather
            // than putting a second swaybg on top.
            let (ok, _) = apply_static_with_slot(&slot, pic);
            assert!(ok);
            assert!(wp_log(&slot).iter().any(|l| l.starts_with("kill ")), "{de}: держатель не снят");

            let slot = wp_slot(de, &["swww"], pic);
            assert!(apply_static_with_slot(&slot, pic).0, "{de}");
            let log = wp_log(&slot);
            assert!(log.iter().any(|l| l == "run swww query"), "{de}: демон swww не проверен: {log:?}");
            assert!(log.iter().any(|l| l == &format!("run swww img {pic}")), "{de}: {log:?}");
        }
        // Nothing to set it with: an honest refusal with a hint about what to install.
        let slot = wp_slot("niri", &[], pic);
        let (ok, msg) = apply_static_with_slot(&slot, pic);
        assert!(!ok);
        assert!(msg.contains("swww"), "{msg}");
        // Full environments use the same executor, with the native key.
        let slot = wp_slot("gnome", &[], pic);
        assert!(apply_static_with_slot(&slot, pic).0);
        assert!(wp_log(&slot).iter().any(|l| l.starts_with("run gsettings set org.gnome.desktop.background picture-uri")));
    }

    /// Deepin themes: a stop before any write in any session, `ok = false`, not
    /// a single write; the reason depends on the session: in Treeland "Control
    /// Center only", outside it "Rexon supports Deepin only on Treeland".
    #[test]
    fn deepin_theme_plan_stops_in_every_session() {
        let dir = tmp_dir("treeland");
        let path = dir.join("written").to_string_lossy().into_owned();
        let steps = vec![
            ApplyStep::DdeTreelandGate,
            ApplyStep::WriteSetsOk { dir: dir.to_string_lossy().into_owned(), path: path.clone(), content: "x".into() },
        ];
        for treeland in [true, false] {
            let r = exec_apply_plan_report_in(&steps, treeland);
            assert!(!r.ok);
            assert_eq!(r.reason, Some(crate::dde::refusal(treeland)));
            assert!(!Path::new(&path).exists(), "в Deepin ничего не пишем");
            assert_eq!(crate::themes::cursor_apply_message("deepin", &r), crate::dde::refusal(treeland));
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Lock screen wallpaper in Deepin on Treeland goes through the Control
    /// Center (`type=lock`); outside Treeland it is refused.
    #[test]
    fn deepin_lock_wallpaper_in_treeland_goes_to_control_center() {
        let wl = lock_wallpaper_plan_in("deepin", true, "/h/Обои", "a b.png", true).unwrap();
        assert_eq!(wl.image.argv, crate::dde::control_center_wallpaper_argv("/h/Обои/a b.png", true));
        let page = wl.image.argv.last().unwrap();
        assert!(page.ends_with("&type=lock"), "{page}");
        assert!(page.contains("url=file:///h/%D0%9E%D0%B1%D0%BE%D0%B8/a%20b.png"), "{page}");
        assert_eq!(lock_wallpaper_plan_in("deepin", false, "/h", "a.png", true).err(), Some(crate::dde::not_treeland_message()));
        assert_eq!(lock_wallpaper_plan_in("kde", true, "/h", "a.png", true).unwrap().image.argv[0], "kwriteconfig6");
    }

    /// Its own directory for the check in `std::env::temp_dir()`.
    fn tmp_dir(tag: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "rexon_ainst_{tag}_{}_{n}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("временный каталог создан");
        path
    }

    fn s(p: &Path) -> String {
        p.to_string_lossy().into_owned()
    }

    // ── "Remove" by the installed log (defect of 09.10.2026) ────────────

    /// Sections with system theme directories in the `root` sandbox.
    fn sandbox_cats(root: &Path) -> Vec<ThemeCategory> {
        crate::categories::categories(root)
            .into_iter()
            .map(|mut c| {
                if c.requires_root {
                    c.install_dir = s(&root.join(format!("usr/share/{}/themes", c.apply_kind)));
                }
                c
            })
            .collect()
    }

    fn write_log(root: &Path, paths: &[&Path]) -> String {
        let log = s(&root.join("installed_themes.json"));
        let list: Vec<String> = paths.iter().map(|p| format!("\"{}\"", s(p))).collect();
        std::fs::write(&log, format!("{{\"777\": {{\"name\": \"Neon\", \"paths\": [{}]}}}}", list.join(","))).unwrap();
        log
    }

    fn log_has_entry(log: &str) -> bool {
        crate::ocs_store::parse_installed_log(read_text_lossy(log).as_deref()).contains_key("777")
    }

    /// Before: "Removed (1 paths)", the entry erased, the directory in place.
    #[test]
    fn uninstall_keeps_log_when_folder_stays() {
        use std::os::unix::fs::PermissionsExt;
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let root = tmp_dir("uninst_stuck");
        let parent = root.join("locked");
        let theme = parent.join("neon");
        std::fs::create_dir_all(&theme).unwrap();
        std::fs::write(theme.join("a"), "x").unwrap();
        std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o555)).unwrap();
        std::fs::set_permissions(&theme, std::fs::Permissions::from_mode(0o555)).unwrap();
        let log = write_log(&root, &[&theme]);
        let cats = sandbox_cats(&root);
        let (ok, msg) = uninstall_from_log(&s(&root), &log, "777", &cats, &mut |_, _| panic!("не системная тема"));
        let stays = theme.is_dir();
        std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::set_permissions(&theme, std::fs::Permissions::from_mode(0o755)).unwrap();
        // Under root permissions do not get in the way, so there is nothing to check.
        if stays {
            assert!(!ok, "{msg}");
            assert!(log_has_entry(&log), "запись журнала стёрта при оставшейся теме");
            assert!(msg.contains("stays in the list"), "{msg}");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// D4: removal by the log also removes our `~/.themes/<theme>` link, like
    /// "Remove" on the tile; a foreign link with the same name is not touched.
    #[test]
    fn uninstall_from_log_drops_theme_link() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let root = tmp_dir("uninst_link");
        let home = s(&root);
        let theme = root.join(".local/share/themes/Neon");
        std::fs::create_dir_all(theme.join("gtk-3.0")).unwrap();
        std::fs::create_dir_all(root.join(".themes")).unwrap();
        let link = root.join(".themes/Neon");
        std::os::unix::fs::symlink("../.local/share/themes/Neon", &link).unwrap();
        // A foreign link under another name leads to something other than our theme.
        let other = root.join(".themes/Other");
        std::os::unix::fs::symlink("/usr/share/themes/Other", &other).unwrap();
        let log = write_log(&root, &[&theme]);
        let (ok, msg) = uninstall_from_log(&home, &log, "777", &sandbox_cats(&root), &mut |_, _| panic!());
        assert!(ok, "{msg}");
        assert!(!theme.exists(), "тема осталась");
        assert!(std::fs::symlink_metadata(&link).is_err(), "висячая ссылка ~/.themes/Neon осталась");
        assert!(std::fs::symlink_metadata(&other).is_ok(), "снята чужая ссылка");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn uninstall_local_theme_forgets_entry() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let root = tmp_dir("uninst_local");
        let theme = root.join("icons/neon");
        let file = root.join("colors/Neon.colors");
        std::fs::create_dir_all(&theme).unwrap();
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "x").unwrap();
        let gone = root.join("icons/gone");
        let log = write_log(&root, &[&theme, &file, &gone]);
        let (ok, msg) = uninstall_from_log(&s(&root), &log, "777", &sandbox_cats(&root), &mut |_, _| panic!());
        assert!(ok, "{msg}");
        assert_eq!(msg, uninstall_message(2, "Neon"));
        assert!(!theme.exists() && !file.exists());
        assert!(!log_has_entry(&log));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn uninstall_system_theme_goes_through_helper() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let root = tmp_dir("uninst_sys");
        let theme = root.join("usr/share/plymouth/themes/neon");
        std::fs::create_dir_all(&theme).unwrap();
        let log = write_log(&root, &[&theme]);
        let cats = sandbox_cats(&root);
        // The helper refused: the theme is in place, the entry intact, the reason visible.
        let mut calls = Vec::new();
        let (ok, msg) = uninstall_from_log(&s(&root), &log, "777", &cats, &mut |c, n| {
            calls.push((c.apply_kind.clone(), n.to_string()));
            (false, "REFUSED".to_string())
        });
        assert!(!ok && msg.contains("REFUSED") && log_has_entry(&log), "{msg}");
        assert_eq!(calls, vec![("plymouth".to_string(), "neon".to_string())]);
        // The helper answered "done", but the directory remained: that is not success.
        let (ok, _) = uninstall_from_log(&s(&root), &log, "777", &cats, &mut |_, _| (true, String::new()));
        assert!(!ok && log_has_entry(&log));
        // The helper removed the directory: the entry is forgotten.
        let (ok, msg) = uninstall_from_log(&s(&root), &log, "777", &cats, &mut |_, _| {
            std::fs::remove_dir_all(&theme).unwrap();
            (true, String::new())
        });
        assert!(ok, "{msg}");
        assert_eq!(msg, uninstall_message(1, "Neon"));
        assert!(!log_has_entry(&log));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn system_theme_path_is_direct_child_only() {
        let cats = crate::categories::categories(Path::new("/home/t"));
        let (c, n) = system_theme_of_path(&cats, "/usr/share/sddm/themes/sugar/").unwrap();
        assert_eq!((c.apply_kind.as_str(), n.as_str()), ("sddm", "sugar"));
        let (c, _) = system_theme_of_path(&cats, "/usr/share/plymouth/themes/neon").unwrap();
        assert_eq!(c.apply_kind, "plymouth");
        for p in [
            "/usr/share/plymouth/themes",
            "/usr/share/plymouth/themes/neon/sub",
            "/usr/share/plymouth/themes/..",
            "/usr/share/plymouth/themes/-R",
            "/home/t/.local/share/icons/neon",
            "neon",
        ] {
            assert!(system_theme_of_path(&cats, p).is_none(), "{p}");
        }
    }

    #[test]
    fn system_theme_remove_decisions() {
        use std::cell::RefCell;
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let cats = crate::categories::categories(Path::new("/home/t"));
        let ply = cats.iter().find(|c| c.apply_kind == "plymouth").unwrap();
        let sddm = cats.iter().find(|c| c.apply_kind == "sddm").unwrap();
        let env = RootEnv {
            is_plasmalogin: false,
            has_pkexec: true,
            system_helper_path: "/usr/lib/rexon-system-helper",
            system_helper_exists: true,
            is_steamos_or_immutable: false,
            is_steamos: false,
            family_is_debian: false,
        };
        let seen: RefCell<Vec<Vec<String>>> = RefCell::new(Vec::new());
        let reply = |rc: i32, err: &str| crate::utils::RunReport {
            outcome: RunOutcome::Ok,
            returncode: Some(rc),
            stderr: err.as_bytes().to_vec(),
            ..Default::default()
        };
        // An active one: refusal before the password window, the helper is not called.
        let (ok, msg) = system_theme_remove_with(&env, sddm, "sugar", &[], &["sugar".into()], &|a, _| {
            seen.borrow_mut().push(a.to_vec());
            reply(0, "")
        });
        assert!(!ok && msg.contains("current login screen theme"), "{msg}");
        let (ok, msg) = system_theme_remove_with(&env, ply, "neon", &[], &["neon".into()], &|_, _| reply(0, ""));
        assert!(!ok && msg.contains("current boot splash theme"), "{msg}");
        assert!(seen.borrow().is_empty());
        // The normal path: the helper call line.
        let (ok, _) = system_theme_remove_with(&env, ply, "neon", &["--lang=ru".into()], &["other".into()], &|a, _| {
            seen.borrow_mut().push(a.to_vec());
            reply(0, "")
        });
        assert!(ok);
        assert_eq!(
            seen.borrow()[0],
            vec!["pkexec", "/usr/lib/rexon-system-helper", "--lang=ru", "remove-plymouth", "neon", "-", "arch"]
        );
        // The password was not confirmed; the helper refused with a reason.
        let (ok, msg) = system_theme_remove_with(&env, ply, "neon", &[], &[], &|_, _| reply(126, ""));
        assert!(!ok && msg.contains("password"), "{msg}");
        let (ok, msg) = system_theme_remove_with(&env, ply, "neon", &[], &[], &|_, _| {
            reply(2, "rexon-helper: the theme neon belongs to a system package")
        });
        assert!(!ok && msg.contains("system package"), "{msg}");
        // No helper; an invalid name.
        let no_helper = RootEnv { system_helper_exists: false, ..env };
        let (ok, _) = system_theme_remove_with(&no_helper, ply, "neon", &[], &[], &|_, _| panic!());
        assert!(!ok);
        let (ok, _) = system_theme_remove_with(&env, ply, "../x", &[], &[], &|_, _| panic!());
        assert!(!ok);
    }

    /// The directory is created, the downloaded file is copied by the explicit
    /// source, links of the tree stay links.
    #[test]
    fn exec_steps_creates_dirs_copies_and_keeps_links() {
        let base = tmp_dir("exec");
        let archive = base.join("downloaded.png");
        std::fs::write(&archive, b"\x89PNG\r\n\x1a\n").expect("файл записан");
        let src_tree = base.join("tree");
        std::fs::create_dir_all(src_tree.join("inner")).expect("дерево создано");
        std::fs::write(src_tree.join("inner/a.txt"), "a").expect("файл записан");
        std::os::unix::fs::symlink("a.txt", src_tree.join("inner/link.txt"))
            .expect("ссылка создана");

        let dest_dir = base.join("dest");
        let steps = vec![
            ExtStep::MakeDirs { path: s(&dest_dir) },
            ExtStep::CopyFile {
                src: s(&archive),
                dest: s(&dest_dir.join("wallpaper.png")),
            },
            ExtStep::CopyTree {
                src: s(&src_tree),
                dest: s(&dest_dir.join("theme")),
            },
        ];
        exec_install_steps(&steps).expect("план исполнен");

        assert!(dest_dir.is_dir(), "каталог назначения создан");
        assert_eq!(
            std::fs::read(dest_dir.join("wallpaper.png")).expect("копия читается"),
            b"\x89PNG\r\n\x1a\n",
            "скачанный файл скопирован"
        );
        assert!(dest_dir.join("theme/inner/a.txt").is_file(), "дерево скопировано");
        let link = dest_dir.join("theme/inner/link.txt");
        assert!(
            link.symlink_metadata().expect("ссылка на месте").file_type().is_symlink(),
            "символьная ссылка осталась ссылкой, а не копией"
        );

        let _ = std::fs::remove_dir_all(&base);
    }

    /// `RemoveTree` of a nonexistent path does not break the plan: the
    /// original has `shutil.rmtree(..., ignore_errors=True)`, and the following
    /// steps must run.
    #[test]
    fn remove_tree_of_missing_path_does_not_stop_the_plan() {
        let base = tmp_dir("rmtree");
        let marker = base.join("after");
        let steps = vec![
            ExtStep::RemoveTree { path: s(&base.join("нет-такого")), safe: false, ignore_errors: true },
            ExtStep::MakeDirs { path: s(&marker) },
        ];
        exec_install_steps(&steps).expect("план исполнен целиком");
        assert!(marker.is_dir(), "шаг после снятия отсутствующего пути выполнен");
        let _ = std::fs::remove_dir_all(&base);
    }

    /// The installed log survives writing and rereading: by it the tab learns
    /// that a theme is installed, and by it the theme is later removed.
    #[test]
    fn install_log_survives_write_and_read() {
        let base = tmp_dir("log");
        let log_path = s(&base.join("rexon/ocs_installed.json"));

        let entry = crate::ocs_store::InstalledItem {
            content_id: "12345".to_string(),
            name: "Тема «Пример»".to_string(),
            rexon_type: kind_to_rexon_type("icons").to_string(),
            provider_id: "kde-look".to_string(),
            category_id: 132,
            ..crate::ocs_store::InstalledItem::default()
        };
        let paths = vec![s(&base.join("icons/Пример"))];
        let mut data = crate::ocs_store::parse_installed_log(None);
        crate::ocs_store::record_installed(&mut data, &entry, &paths, "2026-09-08T12:00:00");
        crate::install_journal::put_store_log(&log_path, &data).expect("журнал записан");

        let mut names: Vec<String> = std::fs::read_dir(base.join("rexon"))
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(
            names,
            vec!["install_journal.json", "ocs_installed.json"],
            "временный файл переименован, а не оставлен рядом"
        );
        // The mirror is the same text the former store log entry wrote.
        assert_eq!(
            read_text_lossy(&log_path).as_deref(),
            Some(crate::ocs_store::serialize_installed_log(&data).as_str())
        );
        let reread = crate::install_journal::store_log_view(&log_path);
        let rec = reread.get("12345").expect("запись найдена после перечитывания");
        assert_eq!(rec["name"].as_str(), Some("Тема «Пример»"));
        assert_eq!(rec["rexon_type"].as_str(), Some(kind_to_rexon_type("icons")));
        assert_eq!(rec["paths"][0].as_str(), Some(paths[0].as_str()));

        let _ = std::fs::remove_dir_all(&base);
    }

    /// A ZIP escaping the unpack directory is REFUSED and writes nothing
    /// outside: this is the only protection against a tampered archive from
    /// the Store.
    #[test]
    fn zip_with_escaping_entry_is_refused_and_writes_nothing() {
        use std::io::Write as _;

        let base = tmp_dir("zipslip");
        let archive = base.join("evil.zip");
        {
            let file = std::fs::File::create(&archive).expect("архив создан");
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("../побег.txt", options).expect("запись начата");
            zip.write_all("побег".as_bytes()).expect("запись удалась");
            zip.finish().expect("архив закрыт");
        }

        let dest = base.join("extracted");
        let err = live_safe_extract(&s(&archive), &s(&dest)).expect_err("побег отвергнут");
        // The check uses the catalog's source string, not the text: the language
        // of the check process depends on the machine, and the label is
        // translated.
        let unsafe_entry = crate::i18n::trf("unsafe entry in the archive: %1", &[&"../побег.txt"]);
        assert_eq!(
            err, unsafe_entry,
            "причина отказа названа словами, а не общим «не удалось»: {err}"
        );
        assert!(
            !base.join("побег.txt").exists(),
            "наружу каталога распаковки не записано ничего"
        );
        assert_eq!(
            std::fs::read_dir(&dest).expect("каталог распаковки создан").count(),
            0,
            "внутри каталога распаковки тоже пусто"
        );

        let _ = std::fs::remove_dir_all(&base);
    }

    /// A divergence guard: EVERY type from [`ACTUAL_TYPES_WITH_DIR`] has both
    /// an install directory and an apply kind. If the list diverged from the
    /// tables, the snapshot would not cover the directory where a reclassified
    /// theme goes: the previous copy of the theme would remain on disk next to
    /// the new one.
    #[test]
    fn actual_types_have_dirs_and_kinds() {
        let home = Path::new("/home/тест");
        for actual in ACTUAL_TYPES_WITH_DIR {
            let dir = actual_type_dir(home, actual)
                .unwrap_or_else(|| panic!("{actual}: нет каталога установки"));
            assert!(!dir.is_empty(), "{actual}: каталог установки пуст");
            assert_ne!(
                actual_to_kind(actual),
                "none",
                "{actual}: нет вида применения"
            );
        }
    }

    /// A snapshot of the unpacked tree: texts are read only for metadata, and
    /// children of the target directories are added NON-recursively: by them
    /// the planner recognises an already installed theme.
    #[test]
    fn tree_snapshot_reads_metadata_and_lists_dest_dirs() {
        let base = tmp_dir("snap");
        let root = base.join("extracted/Тема");
        std::fs::create_dir_all(root.join("contents")).expect("дерево создано");
        std::fs::write(root.join("metadata.json"), r#"{"KPlugin":{"Id":"Тема"}}"#)
            .expect("метаданные записаны");
        std::fs::write(root.join("contents/big.bin"), vec![0u8; 64]).expect("файл записан");

        let dest = base.join("installed");
        std::fs::create_dir_all(dest.join("Старая/глубоко/ещё")).expect("каталог создан");

        let snap = install_tree_snapshot(&s(&base.join("extracted")), &[s(&dest)]);
        assert_eq!(
            snap.read_text(&s(&root.join("metadata.json"))),
            Some(r#"{"KPlugin":{"Id":"Тема"}}"#),
            "текст метаданных прочитан"
        );
        assert_eq!(
            snap.read_text(&s(&root.join("contents/big.bin"))),
            Some(""),
            "содержимое обычного файла в снимок не тянется"
        );
        assert!(snap.is_dir(&s(&dest.join("Старая"))), "уже стоящая тема видна");
        // The first level inside a theme is visible (a frame subfolder in a GTK
        // theme, `plan_nested_part`), deeper levels are not.
        assert!(snap.is_dir(&s(&dest.join("Старая/глубоко"))));
        assert!(
            !snap.is_dir(&s(&dest.join("Старая/глубоко/ещё"))),
            "в целевой каталог снимок не спускается глубже детей темы"
        );
        assert_eq!(
            find_content_root(&snap, &s(&base.join("extracted"))),
            s(&root),
            "спуск в единственную верхнюю папку архива"
        );

        let _ = std::fs::remove_dir_all(&base);
    }

    /// File permissions on disk without the file type.
    fn mode_of(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        std::fs::symlink_metadata(path).expect("файл на месте").permissions().mode() & 0o7777
    }

    /// ZIP: the executable bit of a script from the archive reaches the disk
    /// (the Conky widget "Skyrim Conky Rustic Parchment", 15.11.2026:
    /// `scripts/*.sh` landed as `-rw-r--r--`, and a script calling its
    /// neighbours directly got "Permission denied"). setuid and write for
    /// group and others are stripped.
    #[test]
    fn zip_keeps_exec_bits_and_drops_setuid() {
        use std::io::Write as _;

        let base = tmp_dir("zipmode");
        let archive = base.join("widget.zip");
        {
            let file = std::fs::File::create(&archive).expect("архив создан");
            let mut zip = zip::ZipWriter::new(file);
            let plain = zip::write::SimpleFileOptions::default().unix_permissions(0o644);
            let exec = zip::write::SimpleFileOptions::default().unix_permissions(0o755);
            zip.add_directory("skyrim/scripts/", exec).expect("каталог");
            zip.start_file("skyrim/scripts/run.sh", exec).expect("запись начата");
            zip.write_all(b"#!/bin/sh\necho ok\n").expect("запись удалась");
            zip.start_file("skyrim/conky.conf", plain).expect("запись начата");
            zip.write_all(b"conky.config = {}\n").expect("запись удалась");
            zip.start_file("skyrim/scripts/suid.sh", exec).expect("запись начата");
            zip.write_all(b"#!/bin/sh\n").expect("запись удалась");
            zip.finish().expect("архив закрыт");
        }
        // The zip crate's writer cuts bits above 0o777 by itself, so setuid and
        // write for everyone are set directly in the archive's entry directory
        // (external_attr).
        let mut bytes = std::fs::read(&archive).expect("архив читается");
        let name = b"skyrim/scripts/suid.sh";
        let at = (0..bytes.len() - 46)
            .find(|&i| bytes[i..i + 4] == *b"PK\x01\x02" && bytes[i + 46..].starts_with(name))
            .expect("запись в каталоге архива");
        let attr: u32 = (0o104777u32) << 16;
        bytes[at + 38..at + 42].copy_from_slice(&attr.to_le_bytes());
        std::fs::write(&archive, &bytes).expect("архив записан");

        let dest = base.join("out");
        live_safe_extract(&s(&archive), &s(&dest)).expect("архив распакован");
        let run = mode_of(&dest.join("skyrim/scripts/run.sh"));
        assert_eq!(run & 0o111, 0o111, "скрипт исполняемый: {run:o}");
        assert_eq!(mode_of(&dest.join("skyrim/conky.conf")) & 0o111, 0, "настройки не исполняемые");
        let suid = mode_of(&dest.join("skyrim/scripts/suid.sh"));
        assert_eq!(suid & 0o7000, 0, "setuid снят: {suid:o}");
        assert_eq!(suid & 0o022, 0, "запись для группы и прочих снята: {suid:o}");
        assert_eq!(suid & 0o111, 0o111, "бит исполнения остался: {suid:o}");

        let _ = std::fs::remove_dir_all(&base);
    }

    /// TAR (the system `tar`): the executable bit stays, setuid, setgid and
    /// write for group and others are stripped.
    #[test]
    #[cfg_attr(feature = "isolated-checks", ignore = "нужен системный tar")]
    fn tar_keeps_exec_bits_and_drops_setuid() {
        use std::os::unix::fs::PermissionsExt;

        let base = tmp_dir("tarmode");
        let src = base.join("src/skyrim/scripts");
        std::fs::create_dir_all(&src).expect("каталог создан");
        std::fs::write(src.join("run.sh"), "#!/bin/sh\n").expect("файл записан");
        std::fs::set_permissions(src.join("run.sh"), std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(src.join("suid.sh"), "#!/bin/sh\n").expect("файл записан");
        std::fs::set_permissions(src.join("suid.sh"), std::fs::Permissions::from_mode(0o6777)).unwrap();
        let archive = base.join("widget.tar.gz");
        let argv = vec![
            "tar".to_string(),
            "-czf".to_string(),
            s(&archive),
            "-C".to_string(),
            s(&base.join("src")),
            "skyrim".to_string(),
        ];
        assert!(run_ok(&argv, Some(60)), "архив собран системным tar");

        let dest = base.join("out");
        live_safe_extract(&s(&archive), &s(&dest)).expect("архив распакован");
        let run = mode_of(&dest.join("skyrim/scripts/run.sh"));
        assert_eq!(run & 0o111, 0o111, "скрипт исполняемый: {run:o}");
        let suid = mode_of(&dest.join("skyrim/scripts/suid.sh"));
        assert_eq!(suid & 0o7000, 0, "setuid/setgid сняты: {suid:o}");
        assert_eq!(suid & 0o022, 0, "запись для группы и прочих снята: {suid:o}");
        assert_eq!(suid & 0o111, 0o111, "бит исполнения остался: {suid:o}");

        let _ = std::fs::remove_dir_all(&base);
    }

    /// Live TAR unpacking: the names come from the system `tar -tf`, and it
    /// extracts too. A real `tar` is needed: on a machine without it there is
    /// nothing to check.
    #[test]
    #[cfg_attr(feature = "isolated-checks", ignore = "нужен системный tar")]
    fn tar_archive_extracts_through_system_tar() {
        let base = tmp_dir("tar");
        let src = base.join("src");
        std::fs::create_dir_all(&src).expect("каталог создан");
        std::fs::write(src.join("тема.txt"), "содержимое").expect("файл записан");
        let archive = base.join("theme.tar.gz");
        let argv = vec![
            "tar".to_string(),
            "-czf".to_string(),
            s(&archive),
            "-C".to_string(),
            s(&src),
            "тема.txt".to_string(),
        ];
        assert!(run_ok(&argv, Some(60)), "архив собран системным tar");

        let dest = base.join("out");
        live_safe_extract(&s(&archive), &s(&dest)).expect("архив распакован");
        assert_eq!(
            std::fs::read_to_string(dest.join("тема.txt")).expect("файл на месте"),
            "содержимое"
        );

        let _ = std::fs::remove_dir_all(&base);
    }

    /// Links in 7z: parsing the 7-Zip/p7zip listing, a write through an
    /// archive link, where a link leads.
    #[test]
    fn seven_zip_links_are_judged() {
        let slt = "Path = T\nAttributes = D drwxr-xr-x\n\nPath = T/a\nAttributes = A lrwxrwxrwx\n\n\
                   Path = T/f\nAttributes = A -rw-r--r--\n\nPath = T/w\nAttributes = A\n";
        let e = parse_seven_zip_slt(slt, "x.7z");
        assert_eq!(e, [("T".into(), false), ("T/a".into(), true), ("T/f".into(), false), ("T/w".into(), false)]);
        assert_eq!(entry_under_archive_link(&e), None);
        let bad = vec![("T/d".to_string(), true), ("T/d/pwned".to_string(), false)];
        assert_eq!(entry_under_archive_link(&bad).as_deref(), Some("T/d/pwned"));
        let d = "/tmp/x";
        assert!(!link_escapes(d, "/tmp/x/T/gtk-3.0/link.css", "gtk.css"));
        assert!(!link_escapes(d, "/tmp/x/T-dark/gtk-2.0", "../T/gtk-2.0"));
        assert!(link_escapes(d, "/tmp/x/T/up", "../../../etc"));
        assert!(link_escapes(d, "/tmp/x/T/abs", "/etc/passwd"));
        assert!(link_escapes(d, "/tmp/x/T/abs", "/tmp/x/T/f"), "абсолютная — всегда наружу");
    }

    /// Live 7z unpacking with links by every installed program (bsdtar, 7-Zip
    /// `7z`/`7zz`, p7zip `7za`): a link inside the theme is kept; a link
    /// outside, an absolute one and a write through a link are refused.
    #[test]
    #[cfg_attr(feature = "isolated-checks", ignore = "нужен системный bsdtar или 7z")]
    fn seven_zip_links_extract_safely_with_every_tool() {
        let base = tmp_dir("7z-links");
        let bsdtar_ok = have("bsdtar");
        if !bsdtar_ok {
            return; // nothing to build the archive with
        }
        // The archive: from a directory (links as is) or from a tar stream with
        // its own entries, `bsdtar --format 7zip @tar`.
        let make = |name: &str, setup: &dyn Fn(&Path)| -> PathBuf {
            let src = base.join(format!("src-{name}"));
            std::fs::create_dir_all(&src).unwrap();
            setup(&src);
            let archive = base.join(format!("{name}.7z"));
            let argv: Vec<String> =
                vec!["bsdtar".into(), "--format".into(), "7zip".into(), "-cf".into(), s(&archive), "-C".into(), s(&src), "T".into()];
            assert!(run_ok(&argv, Some(60)), "архив {name} собран");
            archive
        };
        let inside = make("inside", &|src| {
            put(&src.join("T/gtk-3.0/gtk.css"), "css");
            std::os::unix::fs::symlink("gtk.css", src.join("T/gtk-3.0/link.css")).unwrap();
        });
        let up = make("up", &|src| {
            put(&src.join("T/gtk-3.0/gtk.css"), "css");
            std::os::unix::fs::symlink("../../../../../../etc", src.join("T/up")).unwrap();
        });
        let abs = make("abs", &|src| {
            put(&src.join("T/gtk-3.0/gtk.css"), "css");
            std::os::unix::fs::symlink("/etc/passwd", src.join("T/abs")).unwrap();
        });
        // A write through a link: `T/d -> /somewhere`, then `T/d/pwned`.
        let outside = base.join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        let through = {
            let tarball = base.join("through.tar");
            let src = base.join("src-through");
            std::fs::create_dir_all(src.join("T")).unwrap();
            std::os::unix::fs::symlink(&outside, src.join("T/d")).unwrap();
            let src2 = base.join("src-through2");
            put(&src2.join("T/d/pwned"), "x");
            assert!(run_ok(&["bsdtar".into(), "-cf".into(), s(&tarball), "-C".into(), s(&src), "T".into()], Some(60)));
            assert!(run_ok(&["bsdtar".into(), "-rf".into(), s(&tarball), "-C".into(), s(&src2), "T/d/pwned".into()], Some(60)));
            let archive = base.join("through.7z");
            assert!(run_ok(
                &["bsdtar".into(), "--format".into(), "7zip".into(), "-cf".into(), s(&archive), format!("@{}", s(&tarball))],
                Some(60)
            ));
            archive
        };
        let tools: Vec<&str> = ["bsdtar", "7z", "7za", "7zz"].into_iter().filter(|t| have(t)).collect();
        for tool in &tools {
            let run = |archive: &Path, tag: &str| {
                let dest = base.join(format!("out-{tool}-{tag}"));
                std::fs::create_dir_all(&dest).unwrap();
                let entries = seven_zip_entries_with(tool, &s(archive)).expect("список записей");
                let dest_abs = abs_dest(&s(&dest));
                (seven_zip_extract(tool, &entries, &s(archive), &s(&dest), &dest_abs), dest)
            };
            let (res, dest) = run(&inside, "inside");
            res.unwrap_or_else(|e| panic!("{tool}: ссылка внутри темы — не отказ: {e}"));
            let link = dest.join("T/gtk-3.0/link.css");
            assert!(link.symlink_metadata().unwrap().file_type().is_symlink(), "{tool}: ссылка сохранилась");
            assert_eq!(std::fs::read_to_string(&link).unwrap(), "css");
            for (archive, tag) in [(&up, "up"), (&abs, "abs"), (&through, "through")] {
                let (res, _) = run(archive, tag);
                assert!(res.as_ref().is_err_and(|e| e.contains("T/")), "{tool} {tag}: {res:?}");
            }
            assert!(!outside.join("pwned").exists(), "{tool}: запись сквозь ссылку ушла наружу");
        }
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Live 7z unpacking: `bsdtar` or 7-Zip (bench 17.10.2026: "Numix Holo" and
    /// "BlueSky" from the store arrive exactly like this). The archive is
    /// built by the same tool.
    #[test]
    #[cfg_attr(feature = "isolated-checks", ignore = "нужен системный bsdtar или 7z")]
    fn seven_zip_archive_extracts_through_system_tool() {
        let base = tmp_dir("7z");
        let src = base.join("src");
        put(&src.join("NumixHolo/xfwm4/themerc"), "button_offset=0");
        let archive = base.join("159304-NumixHolo.7z");
        let tool = seven_zip_tool().expect("на машине есть bsdtar или 7z");
        let argv: Vec<String> = if tool == "bsdtar" {
            vec![tool.into(), "--format".into(), "7zip".into(), "-cf".into(), s(&archive), "-C".into(), s(&src), "NumixHolo".into()]
        } else {
            vec![tool.into(), "a".into(), s(&archive), s(&src.join("NumixHolo"))]
        };
        assert!(run_ok(&argv, Some(60)), "архив собран: {tool}");
        assert!(is_archive_file(&s(&archive)), "7z опознан архивом");
        let dest = base.join("out");
        live_safe_extract(&s(&archive), &s(&dest)).expect("архив распакован");
        assert!(dest.join("NumixHolo/xfwm4/themerc").is_file());
        // Not 7z but text: the signature did not match, so it is not an archive.
        let text = base.join("scheme.colors");
        std::fs::write(&text, "[General]\n").unwrap();
        assert!(!is_archive_file(&s(&text)));
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Nothing to unpack 7z with: the refusal names the program and the
    /// family's packages, and bsdtar is installed before installing an item
    /// with a `.7z` file (packages checked against the Arch, Debian, Fedora and
    /// openSUSE repositories on 19.10.2026).
    #[test]
    fn seven_zip_without_tool_says_what_to_install() {
        let debian = seven_zip_missing("debian");
        assert!(debian.contains("bsdtar") && debian.contains("libarchive-tools") && debian.contains("7zip"), "{debian}");
        assert!(seven_zip_missing("arch").contains("libarchive"));
        assert_eq!(seven_zip_package("arch"), Some("libarchive"));
        assert_eq!(seven_zip_package("debian"), Some("libarchive-tools"));
        assert_eq!(seven_zip_package("fedora"), Some("bsdtar"));
        assert_eq!(seven_zip_package("suse"), Some("bsdtar"));
        assert_eq!(seven_zip_package("gentoo"), None, "незнакомое семейство — установку не предлагаем");

        // No unpacker: it is installed by itself, without a "Yes/No" question.
        assert_eq!(seven_zip_install_package("159304-NumixHolo.7z", false, Some("bsdtar")).as_deref(), Some("bsdtar"));
        assert!(seven_zip_install_package("A.7Z", false, Some("bsdtar")).is_some(), "расширение без регистра");
        assert!(seven_zip_install_package("a.7z", true, Some("bsdtar")).is_none(), "распаковщик есть");
        assert!(seven_zip_install_package("a.tar.xz", false, Some("bsdtar")).is_none());
        assert!(seven_zip_install_package("a.7z", false, None).is_none(), "поставить нечем");
        // It did not install: the item is not installed; password window closed: advice to retry.
        let canceled = seven_zip_install_failed(&password_not_confirmed());
        assert!(canceled.contains("bsdtar") && !canceled.contains(&password_not_confirmed()), "{canceled}");
        let failed = seven_zip_install_failed("pacman: error");
        assert!(failed.contains("bsdtar") && failed.contains("pacman: error"), "{failed}");

        // 7z by signature is an archive even when there is nothing to unpack it
        // with (not a "bare theme file").
        let base = tmp_dir("7z-magic");
        let archive = base.join("t.7z");
        let mut bytes = SEVEN_ZIP_MAGIC.to_vec();
        bytes.extend_from_slice(&[0; 26]);
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(&archive, &bytes).unwrap();
        assert!(is_archive_file(&s(&archive)));
        let _ = std::fs::remove_dir_all(&base);
    }

    // ── Environment sections: real layouts of pling archives ─────────────
    //
    // The fixtures repeat the structure of archives captured by the bench on
    // 17.10.2026 (ChrButtons, Win8-Metacity, Rosepine, Breeze-Lxqt, Semabe,
    // Extra) in miniature; the archives themselves are not put in the tree.

    /// Plan + execution of an archive installation the way the live installer
    /// calls it: from the unpack directory, with the name from the store.
    fn install_archive_tree(home: &Path, ex: &Path, kind: &str, download_name: &str) -> InstallOutcome {
        install_archive_tree_own(home, ex, kind, download_name, &[])
    }

    /// [`install_archive_tree`] with the paths this purchase installed before.
    fn install_archive_tree_own(home: &Path, ex: &Path, kind: &str, download_name: &str, own: &[String]) -> InstallOutcome {
        let cat = category_by_kind(home, kind).expect("раздел окружения");
        let mut dest_dirs = vec![cat.install_dir.clone()];
        if matches!(de_theme_marker(kind), Some(DeThemeMarker::Subdir(_))) {
            dest_dirs.push(crate::installed_scan::xdg_themes_dir(&s(home)));
        }
        let snap = install_tree_snapshot(&s(ex), &dest_dirs);
        let fallback = archive_theme_name(download_name, "Товар");
        let origin = ArchiveOrigin { extract_dir: &s(ex), fallback_name: &fallback };
        let out = plan_install_archive(home, &snap, &cat, origin, Some(own));
        if out.ok {
            exec_install_steps(&out.steps).expect("шаги исполнены");
        }
        out
    }

    /// A Conky widget from the store: the archive's fonts (any depth,
    /// including the `.fonts/` of Conky manager packages) go to their own
    /// folder `~/.local/share/fonts/rexon-conky-<name>/`; it is written to the
    /// log next to the widget ("Remove" will remove it), and the font cache is
    /// refreshed.
    #[test]
    fn conky_store_widget_installs_its_fonts() {
        let base = tmp_dir("conky_fonts");
        let home = base.join("home");
        let ex = base.join("ex");
        put(&ex.join("victorConky/ArchLarge"), "TEXT\n${font Zekton:size=13}x\n");
        put(&ex.join("fonts/Zekton.ttf"), "ttf");
        put(&ex.join("fonts/readme.txt"), "r");
        put(&ex.join(".fonts/Ubuntu.otf"), "otf");
        put(&ex.join("install.sh"), "cp -r fonts ~/.fonts\n");
        let cat = category_by_kind(&home, "conky").expect("раздел Conky");
        let plan = |ex: &Path, name: &str| {
            let snap = install_tree_snapshot(&s(ex), &[cat.install_dir.clone()]);
            let origin = ArchiveOrigin { extract_dir: &s(ex), fallback_name: name };
            plan_install_archive(&home, &snap, &cat, origin, Some(&[]))
        };
        let out = plan(&ex, "victorConky1.1");
        assert!(out.ok, "{}", out.message);
        let fonts = s(&home.join(".local/share/fonts/rexon-conky-victorConky1.1"));
        let widget = s(&home.join(".config/conky/victorConky1.1"));
        assert_eq!(out.record.as_ref().expect("запись").paths, [widget, fonts.clone()]);
        for f in ["fonts/Zekton.ttf", ".fonts/Ubuntu.otf"] {
            let name = f.rsplit('/').next().unwrap();
            let want = ExtStep::CopyFile { src: s(&ex.join(f)), dest: format!("{fonts}/{name}") };
            assert!(out.steps.contains(&want), "{f}: {:#?}", out.steps);
        }
        assert!(!out.steps.iter().any(|st| matches!(st, ExtStep::CopyFile { dest, .. } if dest.ends_with("readme.txt"))));
        assert!(out.steps.iter().any(|st| matches!(st, ExtStep::Run { argv, .. } if argv[0] == "fc-cache")));

        // Without fonts: no fonts folder in the log and no fc-cache.
        let bare = base.join("bare");
        put(&bare.join("Gotham/conky.conf"), "conky.text = [[x]]\n");
        let out = plan(&bare, "Gotham");
        assert_eq!(out.record.expect("запись").paths, [s(&home.join(".config/conky/Gotham"))]);
        assert!(!out.steps.iter().any(|st| matches!(st, ExtStep::Run { .. })));
        let _ = std::fs::remove_dir_all(&base);
    }

    /// A Conky theme package (`.cmtp.7z` and store archives with the same
    /// layout): every widget from `.conky/` is its own folder, a bare file from
    /// `.Conky/` is a folder with that file, fonts from `.fonts/` and
    /// `.conky/fonts/` go to one package folder; a name taken by someone
    /// else's widget is not overwritten.
    #[test]
    fn conky_theme_pack_installs_each_widget() {
        let base = tmp_dir("conky_pack");
        let home = base.join("home");
        let ex = base.join("ex");
        put(&ex.join(".conky/Gotham/Gotham"), "conky.text = [[x]]\n");
        put(&ex.join(".conky/Conky Seamod/conky_seamod"), "TEXT\nx\n");
        put(&ex.join(".conky/Conky Seamod/rings.lua"), "-- lua\n");
        put(&ex.join(".conky/Green Apple Desktop/Green Apple Desktop.cmtheme"), "~/.conky/Green Apple Desktop/Gotham\n");
        put(&ex.join(".conky/fonts/Ubuntu.ttf"), "ttf");
        put(&ex.join(".conky/README"), "r");
        put(&ex.join(".Conky/conkyx"), "TEXT\ny\n");
        put(&ex.join(".fonts/symbols.ttf"), "ttf");
        put(&home.join(".config/conky/Gotham/mine.conf"), "conky.text = [[mine]]\n");
        let cat = category_by_kind(&home, "conky").expect("раздел Conky");
        let snap = install_tree_snapshot(&s(&ex), &[cat.install_dir.clone()]);
        let origin = ArchiveOrigin { extract_dir: &s(&ex), fallback_name: "default-themes-2.1.cmtp" };
        let out = plan_install_archive(&home, &snap, &cat, origin, Some(&[]));
        assert!(out.ok, "{}", out.message);
        let conky = s(&home.join(".config/conky"));
        let fonts = s(&home.join(".local/share/fonts/rexon-conky-default-themes-2.1"));
        assert_eq!(out.names, ["Conky Seamod", "Green Apple Desktop", "conkyx"]);
        assert_eq!(
            out.record.as_ref().expect("запись").paths,
            [
                format!("{conky}/Conky Seamod"),
                format!("{conky}/Green Apple Desktop"),
                format!("{conky}/conkyx"),
                fonts.clone(),
            ]
        );
        // The person's own "Gotham" is not touched, and this is said.
        assert!(out.message.contains("Gotham"), "{}", out.message);
        assert!(!out.steps.iter().any(|st| matches!(st, ExtStep::RemoveTree { path, .. } if path.ends_with("/Gotham"))));
        let loose = ExtStep::CopyFile { src: s(&ex.join(".Conky/conkyx")), dest: format!("{conky}/conkyx/conkyx") };
        assert!(out.steps.contains(&loose), "{:#?}", out.steps);
        for f in [".conky/fonts/Ubuntu.ttf", ".fonts/symbols.ttf"] {
            let name = f.rsplit('/').next().unwrap();
            let want = ExtStep::CopyFile { src: s(&ex.join(f)), dest: format!("{fonts}/{name}") };
            assert!(out.steps.contains(&want), "{f}: {:#?}", out.steps);
        }
        exec_install_steps(&out.steps).expect("шаги исполнены");
        assert!(home.join(".config/conky/Conky Seamod/rings.lua").is_file());
        assert!(!home.join(".config/conky/fonts").exists() && !home.join(".config/conky/README").exists());

        // An archive without package folders is still one widget.
        let one = base.join("one");
        put(&one.join("Mimosa/Mimosa.conf"), "conky.text = [[x]]\n");
        let snap = install_tree_snapshot(&s(&one), &[cat.install_dir.clone()]);
        let origin = ArchiveOrigin { extract_dir: &s(&one), fallback_name: "Mimosa" };
        let out = plan_install_archive(&home, &snap, &cat, origin, Some(&[]));
        assert_eq!(out.names, ["Mimosa"]);
        let _ = std::fs::remove_dir_all(&base);
    }

    /// The fallback branch "wallpapers / fonts and others" by the owner's rule:
    /// a directory with the same name that this purchase did not install (the
    /// person, another purchase, a link) is not removed: a refusal with a
    /// reason, not a single step; one recorded in this purchase's log is
    /// removed and installed again.
    #[test]
    fn fallback_branch_keeps_foreign_dir() {
        let base = tmp_dir("fallback_owner");
        let home = base.join("home");
        for (kind, dir) in [("wallpapers", ".local/share/wallpapers"), ("fonts", ".local/share/fonts")] {
            let ex = base.join(format!("ex-{kind}"));
            put(&ex.join("Gold/contents/a.jpg"), "v2");
            let dest = home.join(dir).join("Gold");
            put(&dest.join("mine.txt"), "моё");
            let out = install_tree(&home, &ex, kind, Some(&[]));
            assert!(!out.ok, "{kind}: чужой каталог перезаписан: {}", out.message);
            assert_eq!(out.message, skipped_note("Gold", SkipReason::TakenByOther), "{kind}");
            assert!(out.steps.is_empty() && out.record.is_none(), "{kind}: {:#?}", out.steps);
            assert_eq!(std::fs::read_to_string(dest.join("mine.txt")).unwrap(), "моё", "{kind}");

            // A link in place is also foreign.
            std::fs::remove_dir_all(&dest).unwrap();
            std::os::unix::fs::symlink(base.join("outside"), &dest).unwrap();
            let out = install_tree(&home, &ex, kind, Some(&[]));
            assert!(!out.ok, "{kind}: ссылка заменена: {}", out.message);
            std::fs::remove_file(&dest).unwrap();

            // Our own previous purchase is removed and installed again.
            put(&dest.join("old.jpg"), "v1");
            let own = vec![s(&dest)];
            let out = install_tree(&home, &ex, kind, Some(&own));
            assert!(out.ok, "{kind}: своя прежняя не заменена: {}", out.message);
            assert_eq!(out.record.expect("запись").paths, own, "{kind}");
            assert!(!dest.join("old.jpg").exists(), "{kind}: прежняя копия не снята");
            assert_eq!(std::fs::read_to_string(dest.join("contents/a.jpg")).unwrap(), "v2", "{kind}");

            // Without a log (`None`, the Python reference): the old "is overwritten".
            let out = install_tree(&home, &ex, kind, None);
            assert!(out.ok, "{kind}: {}", out.message);
        }
        let _ = std::fs::remove_dir_all(&base);
    }

    /// The fonts folder of a Conky widget (single and package) with the same
    /// name from another purchase is not added to and not written to this
    /// purchase's log: the widget is installed, the fonts are skipped with a
    /// reason. Our own is replaced.
    #[test]
    fn conky_font_dir_of_other_purchase_is_kept() {
        let base = tmp_dir("conky_font_owner");
        let home = base.join("home");
        let cat = category_by_kind(&home, "conky").expect("раздел Conky");
        let fonts_base = s(&home.join(".local/share/fonts"));
        let plan = |ex: &Path, name: &str, own: &[String]| {
            let snap = install_tree_snapshot(&s(ex), &[cat.install_dir.clone(), fonts_base.clone()]);
            let origin = ArchiveOrigin { extract_dir: &s(ex), fallback_name: name };
            plan_install_archive(&home, &snap, &cat, origin, Some(own))
        };
        let one = base.join("one");
        put(&one.join("victor/conky.conf"), "conky.text = [[x]]\n");
        put(&one.join("fonts/Zekton.ttf"), "ttf");
        let pack = base.join("pack");
        put(&pack.join(".conky/Gotham/Gotham"), "conky.text = [[x]]\n");
        put(&pack.join(".fonts/symbols.ttf"), "ttf");
        for (ex, name, font_name) in [(&one, "victor", "rexon-conky-victor"), (&pack, "themes.cmtp", "rexon-conky-themes")] {
            let font_dir = home.join(".local/share/fonts").join(font_name);
            put(&font_dir.join("other.ttf"), "чужой");
            let out = plan(ex, name, &[]);
            assert!(out.ok, "{name}: {}", out.message);
            assert!(out.message.contains(&skipped_note(font_name, SkipReason::TakenByOther)), "{name}: {}", out.message);
            let paths = out.record.expect("запись").paths;
            assert!(!paths.contains(&s(&font_dir)), "{name}: чужая папка шрифтов в журнале: {paths:?}");
            assert!(
                !out.steps.iter().any(|st| match st {
                    ExtStep::RemoveTree { path, .. } | ExtStep::MakeDirs { path } => path == &s(&font_dir),
                    ExtStep::CopyFile { dest, .. } => dest.starts_with(&s(&font_dir)),
                    _ => false,
                }),
                "{name}: шаги трогают чужую папку шрифтов: {:#?}",
                out.steps
            );
            // Our own (recorded in the log) is removed and installed again.
            let out = plan(ex, name, &[s(&font_dir)]);
            assert!(out.ok, "{name}: {}", out.message);
            assert!(out.record.expect("запись").paths.contains(&s(&font_dir)), "{name}");
            assert!(out.steps.contains(&ExtStep::RemoveTree { path: s(&font_dir), safe: false, ignore_errors: true }), "{name}");
        }
        let _ = std::fs::remove_dir_all(&base);
    }

    /// A system theme (SDDM, Plymouth): a theme directory with the same name
    /// that this purchase did not install (a package's theme, the person's)
    /// means refusal before the password window: the helper copies over it, so
    /// the decision is made here.
    #[test]
    fn root_install_refuses_foreign_system_theme() {
        let base = tmp_dir("root_owner");
        let home = base.join("home");
        let ex = base.join("ex");
        put(&ex.join("MyTheme/metadata.desktop"), "[SddmGreeterTheme]\nName=My\n");
        put(&ex.join("MyTheme/Main.qml"), "Item {}\n");
        let cat = category_by_kind(&home, "sddm").expect("раздел SDDM");
        let env = RootEnv {
            is_steamos: false,
            is_steamos_or_immutable: false,
            is_plasmalogin: false,
            has_pkexec: true,
            system_helper_exists: true,
            system_helper_path: "/usr/lib/rexon/helper",
            family_is_debian: false,
        };
        let snap = install_tree_snapshot(&s(&ex), &[]);
        let root = s(&ex.join("MyTheme"));
        let dest = join(&cat.install_dir, "MyTheme");
        let err = plan_root_install_with(&env, &snap, &cat, &root, &|p| p == dest).unwrap_err();
        assert_eq!(err.message, skipped_note("MyTheme", SkipReason::TakenByOther));
        let ok = plan_root_install_with(&env, &snap, &cat, &root, &|_| false).expect("свободно — план");
        assert_eq!(ok.dest, dest);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn de_theme_markers_come_from_matrix() {
        assert_eq!(de_theme_marker("xfwm4"), Some(DeThemeMarker::Subdir("xfwm4")));
        assert_eq!(de_theme_marker("marco"), Some(DeThemeMarker::Subdir("metacity-1")));
        assert_eq!(de_theme_marker("gnomeshell"), Some(DeThemeMarker::Subdir("gnome-shell")));
        assert_eq!(de_theme_marker("cinnamontheme"), Some(DeThemeMarker::Subdir("cinnamon")));
        assert_eq!(de_theme_marker("lxqttheme"), Some(DeThemeMarker::File("lxqt-panel.qss")));
        for other in ["gtk", "gnomeext", "cinnamonapplets", "nemoactions", "conky", "tabbox"] {
            assert_eq!(de_theme_marker(other), None, "{other}");
        }
    }

    #[test]
    fn archive_theme_name_strips_extension_and_legacy_number() {
        assert_eq!(archive_theme_name("Rosepine-B-GS-Dark.tar.xz", "x"), "Rosepine-B-GS-Dark");
        assert_eq!(archive_theme_name("99823-Miu with xfwm.tar.gz", "x"), "Miu with xfwm");
        assert_eq!(archive_theme_name("Marble-shell.zip", "x"), "Marble-shell");
        assert_eq!(archive_theme_name("159304-NumixHolo.7z", "x"), "NumixHolo");
        assert_eq!(archive_theme_name("", "Win 8 / Metacity"), "Win 8 - Metacity");
        assert_eq!(archive_theme_name(".tar.gz", ""), "theme");
        // The file name is set by the item's author: a space before the dots
        // must not turn the theme name into ".." or "."; otherwise
        // `~/.themes/..` = `~`.
        for dl in [" ...zip", " ..zip", "1- ...tar.gz", " .hidden.zip"] {
            let n = archive_theme_name(dl, "Item");
            assert_eq!(n, "Item", "{dl:?} → {n:?}");
        }
    }

    /// A nameless archive root with a dangerous file name does not land in HOME.
    #[test]
    fn archive_without_folder_never_lands_above_the_theme_dir() {
        let base = tmp_dir("dotdot_name");
        let home = base.join("home");
        let ex = base.join("ex");
        put(&ex.join("gnome-shell/gnome-shell.css"), "x");
        put(&ex.join(".evilrc"), "x");
        let _ = install_archive_tree(&home, &ex, "gnomeshell", " ...tar.xz");
        assert!(!home.join(".evilrc").exists(), "файл архива лёг прямо в HOME");
    }

    /// The package name from metadata is also foreign data: "../.." in
    /// `KPlugin.Id` does not lead the installation out of the kind's directory.
    #[test]
    fn metadata_id_cannot_climb_out_of_the_kind_dir() {
        let snap = |file: &str, text: &str| {
            FsSnapshot::from_json(&serde_json::json!({
                "dirs": {"/x/Pkg": [file]},
                "files": {format!("/x/Pkg/{file}"): text},
            }))
        };
        let fs = snap("metadata.json", r#"{"KPlugin": {"Id": "../../../evil"}}"#);
        assert_eq!(theme_id_from(&fs, "/x/Pkg"), "Pkg");
        let fs = snap("metadata.desktop", "X-KDE-PluginInfo-Name=..\n");
        assert_eq!(theme_id_from(&fs, "/x/Pkg"), "Pkg");
        // An ordinary Id stays.
        let fs = snap("metadata.json", r#"{"KPlugin": {"Id": "org.kde.good"}}"#);
        assert_eq!(theme_id_from(&fs, "/x/Pkg"), "org.kde.good");
    }

    /// Window frames named like an installed GTK theme land as a subfolder in
    /// its folder (through our `~/.themes/<name>` link, into
    /// `~/.local/share/themes`), the link stays a link; reinstalling the same
    /// purchase replaces only the subfolder; a foreign subfolder is not
    /// overwritten; without a name, a separate folder as before.
    #[test]
    fn frame_with_gtk_theme_name_goes_inside_the_gtk_folder() {
        let base = tmp_dir("frame_nest");
        let home = base.join("home");
        let xdg = home.join(".local/share/themes");
        let legacy = home.join(".themes");
        put(&xdg.join("Arc/gtk-3.0/gtk.css"), "gtk");
        std::fs::create_dir_all(&legacy).unwrap();
        std::os::unix::fs::symlink("../.local/share/themes/Arc", legacy.join("Arc")).unwrap();
        put(&legacy.join("Plain/gtk-2.0/gtkrc"), "gtk2");

        let ex = base.join("marco");
        put(&ex.join("Arc/metacity-1/metacity-theme-3.xml"), "<metacity_theme/>");
        put(&ex.join("Arc/README"), "r");
        let out = install_archive_tree(&home, &ex, "marco", "Arc-metacity.tar.gz");
        assert!(out.ok, "{}", out.message);
        let part = s(&xdg.join("Arc/metacity-1"));
        assert_eq!(out.record.as_ref().expect("запись").paths, [part.clone()]);
        assert!(xdg.join("Arc/metacity-1/metacity-theme-3.xml").is_file());
        assert!(xdg.join("Arc/gtk-3.0/gtk.css").is_file(), "тема GTK цела");
        assert!(!xdg.join("Arc/README").exists(), "из архива — только подпапка рамки");
        assert!(std::fs::symlink_metadata(legacy.join("Arc")).unwrap().file_type().is_symlink());

        // The same purchase again: the subfolder is replaced, the GTK theme intact.
        put(&ex.join("Arc/metacity-1/metacity-theme-3.xml"), "<metacity_theme v2/>");
        let again = install_archive_tree_own(&home, &ex, "marco", "Arc-metacity.tar.gz", &[part.clone()]);
        assert!(again.ok, "{}", again.message);
        assert_eq!(std::fs::read_to_string(xdg.join("Arc/metacity-1/metacity-theme-3.xml")).unwrap(), "<metacity_theme v2/>");
        assert!(xdg.join("Arc/gtk-3.0/gtk.css").is_file());
        // Another purchase with the same name does not touch the foreign subfolder.
        let other = install_archive_tree(&home, &ex, "marco", "Arc-metacity.tar.gz");
        assert!(!other.ok, "{}", other.message);

        // xfwm4 into a real folder `~/.themes/Plain` (not a link).
        let ex = base.join("xfwm");
        put(&ex.join("Plain/xfwm4/themerc"), "button_offset=0");
        let out = install_archive_tree(&home, &ex, "xfwm4", "Plain.tar.gz");
        assert!(out.ok, "{}", out.message);
        assert_eq!(out.record.expect("запись").paths, [s(&legacy.join("Plain/xfwm4"))]);
        assert!(legacy.join("Plain/gtk-2.0/gtkrc").is_file());

        // No name: a separate folder in `~/.themes`.
        let ex = base.join("solo");
        put(&ex.join("Solo/xfwm4/themerc"), "x");
        let out = install_archive_tree(&home, &ex, "xfwm4", "Solo.tar.gz");
        assert_eq!(out.record.expect("запись").paths, [s(&legacy.join("Solo"))]);
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Several variants under a common wrapper: each goes to the top level of
    /// `~/.themes` (xfwm4: ChrButtons), and without a wrapper at all (Marco:
    /// Win8).
    #[test]
    fn de_frame_variants_land_on_top_level() {
        let base = tmp_dir("de_variants");
        let home = base.join("home");
        let themes = home.join(".themes");

        let ex = base.join("xfwm");
        put(&ex.join("ChrButtons/README"), "r");
        for v in ["ChrBlueButtons", "ChrDarkButtons"] {
            put(&ex.join(format!("ChrButtons/{v}/xfwm4/themerc")), "button_offset=2");
        }
        let out = install_archive_tree(&home, &ex, "xfwm4", "ChrButtons.tar.xz");
        assert!(out.ok, "{}", out.message);
        assert_eq!(out.names, ["ChrBlueButtons", "ChrDarkButtons"]);
        assert!(themes.join("ChrDarkButtons/xfwm4/themerc").is_file());
        assert!(!themes.join("ChrButtons").exists(), "обёртка не ставится");
        assert_eq!(out.record.expect("запись").rexon_type, "theme-xfwm4");

        let ex = base.join("marco");
        for v in ["Win8-05-Saffron", "Win8-16-Alabaster"] {
            put(&ex.join(format!("{v}/metacity-1/metacity-theme-3.xml")), "<metacity_theme/>");
        }
        let out = install_archive_tree(&home, &ex, "marco", "Win8-Metacity-v1.4.tar.gz");
        assert!(out.ok, "{}", out.message);
        assert_eq!(out.names, ["Win8-05-Saffron", "Win8-16-Alabaster"]);
        assert!(themes.join("Win8-16-Alabaster/metacity-1/metacity-theme-3.xml").is_file());
        assert!(!themes.join("extracted").exists() && !themes.join("marco").exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    /// An archive with `gnome-shell/` right at the root (Rosepine): the name
    /// comes from the store file, the theme is `~/.themes/<name>/gnome-shell`.
    #[test]
    fn shell_theme_without_own_folder_takes_archive_name() {
        let base = tmp_dir("de_noname");
        let home = base.join("home");
        let ex = base.join("ex");
        put(&ex.join("gnome-shell/gnome-shell.css"), "stage {}");
        put(&ex.join("gnome-shell/assets/a.svg"), "<svg/>");
        let out = install_archive_tree(&home, &ex, "gnomeshell", "Rosepine-B-GS-Dark.tar.xz");
        assert!(out.ok, "{}", out.message);
        assert_eq!(out.names, ["Rosepine-B-GS-Dark"]);
        let theme = home.join(".themes/Rosepine-B-GS-Dark");
        assert!(theme.join("gnome-shell/gnome-shell.css").is_file());
        assert!(theme.join("gnome-shell/assets/a.svg").is_file());
        assert!(!home.join(".themes/gnome-shell").exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    /// LXQt themes are recognised by `lxqt-panel.qss` at any depth
    /// (Breeze-Lxqt: `Breeze-Lxqt/themes/<name>/`); neighbouring Openbox themes
    /// are not installed. Cinnamon: variants three levels deep (Semabe), a
    /// combined GTK theme is installed as a whole.
    #[test]
    fn lxqt_and_cinnamon_themes_found_deep_inside() {
        let base = tmp_dir("de_deep");
        let home = base.join("home");

        let ex = base.join("lxqt");
        let w = ex.join("Breeze-Dark-Light-Lxqt-Ob");
        put(&w.join("Breeze-Dark-Ob/openbox-3/themerc"), "x");
        put(&w.join("Wallpaper-Dark.jpg"), "jpg");
        for v in ["Breeze Dark", "Breeze Light"] {
            put(&w.join(format!("Breeze-Lxqt/themes/{v}/lxqt-panel.qss")), "LXQtPanel {}");
        }
        let out = install_archive_tree(&home, &ex, "lxqttheme", "Breeze-Dark-Light-Lxqt-Ob.tar.gz");
        assert!(out.ok, "{}", out.message);
        assert_eq!(out.names, ["Breeze Dark", "Breeze Light"]);
        let lxqt = home.join(".local/share/lxqt/themes");
        assert_eq!(listing(&lxqt), ["Breeze Dark", "Breeze Light"]);
        assert!(lxqt.join("Breeze Light/lxqt-panel.qss").is_file());

        let ex = base.join("cinnamon");
        put(&ex.join("LED/gnome/large/Semabe Azure (LED)LG/cinnamon/cinnamon.css"), "");
        put(&ex.join("LED/left/small/Semabe Grey (LED)SL/cinnamon/cinnamon.css"), "");
        put(&ex.join("LED/left/small/Semabe Grey (LED)SL/gtk-3.0/gtk.css"), "");
        let out = install_archive_tree(&home, &ex, "cinnamontheme", "semabe-LED-3.6.2.tar.xz");
        assert!(out.ok, "{}", out.message);
        assert_eq!(out.names, ["Semabe Azure (LED)LG", "Semabe Grey (LED)SL"]);
        let grey = home.join(".themes/Semabe Grey (LED)SL");
        assert!(grey.join("cinnamon/cinnamon.css").is_file() && grey.join("gtk-3.0/gtk.css").is_file());
        assert!(!home.join(".themes/LED").exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    /// The archive has no theme of its own kind (fonts and wallpapers posing
    /// as a Cinnamon theme; an Openbox theme in the LXQt section): refusal,
    /// nothing on disk.
    #[test]
    fn de_archive_without_theme_of_its_kind_is_refused() {
        let base = tmp_dir("de_none");
        let home = base.join("home");
        let ex = base.join("cin");
        put(&ex.join("Extra 1.4/Roboto-Regular.ttf"), "ttf");
        put(&ex.join("Extra 1.4/Desert-wallpaper/1.jpg"), "jpg");
        let out = install_archive_tree(&home, &ex, "cinnamontheme", "Extra 1.4.tar.xz");
        assert!(!out.ok);
        assert_eq!(out.message, no_package_inside_message());
        let ex = base.join("lx");
        put(&ex.join("Lubuntu Big Sur/openbox-3/themerc"), "x");
        let out = install_archive_tree(&home, &ex, "lxqttheme", "Lubuntu Big Sur.zip");
        assert!(!out.ok);
        assert!(!home.join(".themes").exists() && !home.join(".local/share/lxqt").exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    /// A single theme with a wrapper is installed, as before, under its own
    /// name; the name from the store does not replace the theme folder name.
    #[test]
    fn single_de_theme_keeps_folder_name() {
        let base = tmp_dir("de_single");
        let home = base.join("home");
        let ex = base.join("ex");
        put(&ex.join("Miu/xfwm4/themerc"), "x");
        put(&ex.join("Miu/metacity-1/metacity-theme-1.xml"), "<m/>");
        let out = install_archive_tree(&home, &ex, "xfwm4", "99823-Miu with xfwm.tar.gz");
        assert!(out.ok, "{}", out.message);
        assert_eq!(out.names, ["Miu"]);
        assert!(home.join(".themes/Miu/metacity-1/metacity-theme-1.xml").is_file());
        let _ = std::fs::remove_dir_all(&base);
    }

    /// The same nameless root defect for GTK: a `gtk-3.0/` archive without a wrapper.
    #[test]
    fn gtk_theme_without_own_folder_takes_archive_name() {
        let base = tmp_dir("gtk_noname");
        let home = base.join("home");
        let ex = base.join("ex");
        put(&ex.join("gtk-3.0/gtk.css"), "");
        put(&ex.join("gtk-2.0/gtkrc"), "");
        let out = install_archive_tree(&home, &ex, "gtk", "Nordic-darker.tar.xz");
        assert!(out.ok, "{}", out.message);
        assert_eq!(out.names, ["Nordic-darker"]);
        assert!(home.join(".local/share/themes/Nordic-darker/gtk-3.0/gtk.css").is_file());
        let _ = std::fs::remove_dir_all(&base);
    }

    /// A picture from a pack is chosen by the LARGEST monitor.
    ///
    /// ONE picture from the pack goes to the desktop, so the measure is the
    /// large monitor: the small one will scale it down, and that is better than
    /// one stretched from a smaller size. The former `xrandr` parsing took "the
    /// first resolution with an asterisk": on such a pair of monitors it would
    /// have given a 1600x900 picture.
    #[test]
    fn the_pack_image_is_chosen_by_the_biggest_monitor() {
        use crate::wallpaper_exec::{largest_output, ScreenOutput};
        let out = |name: &str, res: &str| ScreenOutput {
            name: name.to_string(),
            resolution: res.to_string(),
            position: None,
        };
        let files: Vec<String> = ["1600x900.png", "1920x1080.png", "3840x2160.png"]
            .iter()
            .map(|n| format!("/сб/пачка/contents/images/{n}"))
            .collect();
        let measured = |outputs: &[ScreenOutput]| {
            let (w, h) = largest_output(outputs).and_then(|o| o.size()).unwrap_or((0, 0));
            pick_pack_image(&files, w, h).map(|f| basename(&f).to_string())
        };

        // One output: as before.
        assert_eq!(measured(&[out("eDP-1", "1920x1080")]).as_deref(), Some("1920x1080.png"));
        // Two of different sizes, the small one first in the list: take the large one.
        assert_eq!(
            measured(&[out("HDMI-1", "1600x900"), out("DP-1", "3840x2160")]).as_deref(),
            Some("3840x2160.png")
        );
        // ⚠️ The query failed (no outputs): the choice goes by name, as before
        // any enumeration of outputs.
        assert_eq!(measured(&[]).as_deref(), Some("1600x900.png"));
    }

    // ── theme roots in an archive and lifting nested ones (03.10.2026) ──────

    fn put(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().expect("есть родитель")).expect("каталог");
        std::fs::write(path, text).expect("файл записан");
    }

    const SOUND_INDEX: &str = "[Sound Theme]\nName=Zorin\nDirectories=stereo\n\n[stereo]\nOutputProfile=stereo\n";

    /// An archive like the live item "zorinos2": two sound themes two levels
    /// deeper, with wallpapers, xml and a ReadMe next to them.
    fn zorin_archive(ex: &Path) {
        let w = ex.join("zorinos2");
        put(&w.join("ReadMe."), "читай меня");
        put(&w.join("backgrounds/Waves.jpg"), "jpg");
        put(&w.join("gnome-background-properties/cosmos.xml"), "<xml/>");
        for t in ["Zorin", "Zorin2"] {
            put(&w.join(format!("sounds/{t}/index.theme")), SOUND_INDEX);
            put(&w.join(format!("sounds/{t}/stereo/bell.oga")), "oga");
        }
    }

    /// Plan + execution of installing the unpacked `ex` into category `kind`.
    fn install_tree(home: &Path, ex: &Path, kind: &str, own: Option<&[String]>) -> InstallOutcome {
        let cat = category_by_kind(home, kind).expect("категория есть");
        let snap = install_tree_snapshot(&s(ex), &[cat.install_dir.clone()]);
        let root = find_content_root(&snap, &s(ex));
        let out = plan_install_with(home, &snap, &cat, &root, own);
        if out.ok {
            exec_install_steps(&out.steps).expect("шаги исполнены");
        }
        out
    }

    fn listing(dir: &Path) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(dir)
            .map(|rd| rd.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect())
            .unwrap_or_default();
        v.sort();
        v
    }

    #[test]
    fn nested_sound_themes_land_on_top_level_without_extras() {
        let base = tmp_dir("snd_nested");
        let (home, ex) = (base.join("home"), base.join("ex"));
        zorin_archive(&ex);
        let out = install_tree(&home, &ex, "sounds", Some(&[]));
        assert!(out.ok, "{}", out.message);
        assert_eq!(out.names, ["Zorin", "Zorin2"]);
        let sounds = home.join(".local/share/sounds");
        assert_eq!(listing(&sounds), ["Zorin", "Zorin2"], "ни обёртки, ни обоев в sounds/");
        assert!(sounds.join("Zorin/index.theme").is_file());
        assert!(sounds.join("Zorin2/stereo/bell.oga").is_file());
        let rec = out.record.expect("запись журнала");
        assert_eq!(rec.paths, [s(&sounds.join("Zorin")), s(&sounds.join("Zorin2"))]);
    }

    /// An item of a KWin section from the store (item number = section number)
    /// lands as a KPackage: the directory is `KPlugin.Id`, no reclassification,
    /// the log type is the section type. C++ sources are not installed: a
    /// refusal with a reason.
    #[test]
    fn kwin_store_items_install_as_kpackage() {
        let base = tmp_dir("kwin_store");
        let home = base.join("home");
        for (cat_id, dir, rexon_type) in [
            (720, ".local/share/kwin/scripts", "kde-kwin-script"),
            (719, ".local/share/kwin/effects", "kde-kwin-effect"),
            (721, ".local/share/kwin/tabbox", "kde-window-switcher"),
        ] {
            let ex = base.join(format!("ex{cat_id}"));
            // Without a wrapper: metadata and contents/ at the archive root (.kwinscript).
            put(&ex.join("metadata.json"), r#"{"KPlugin":{"Id":"pkg-id","Name":"Pkg"}}"#);
            put(&ex.join("contents/code/main.js"), "//");
            let cat = crate::categories::category_by_id(&home, cat_id).expect("раздел");
            let snap = install_tree_snapshot(&s(&ex), &[cat.install_dir.clone()]);
            let root = find_content_root(&snap, &s(&ex));
            let out = plan_install_with(&home, &snap, &cat, &root, Some(&[]));
            assert!(out.ok, "{cat_id}: {}", out.message);
            assert_eq!(out.names, ["pkg-id"]);
            assert!(out.reclassified.is_none());
            let rec = out.record.expect("запись");
            assert_eq!(rec.paths, [s(&home.join(dir).join("pkg-id"))]);
            assert_eq!(rec.rexon_type, rexon_type);
            assert_eq!(rec.apply_kind, cat.key);

            // C++ sources of an effect: metadata exists only deeper.
            let src = base.join(format!("src{cat_id}"));
            put(&src.join("kwin-fx-main/CMakeLists.txt"), "project(x)");
            put(&src.join("kwin-fx-main/src/metadata.json"), "{}");
            let snap = install_tree_snapshot(&s(&src), &[cat.install_dir.clone()]);
            let root = find_content_root(&snap, &s(&src));
            let out = plan_install_with(&home, &snap, &cat, &root, Some(&[]));
            assert!(!out.ok, "{cat_id}: исходники не ставятся");
            assert_eq!(out.message, not_a_package_message());
            assert!(out.steps.is_empty());
        }
    }

    /// Widgets and wallpaper plugins are KPackages too: C++ sources, a pacman
    /// package and a non-package (a Dolphin menu in the widgets section) are
    /// not installed, and the refusal gives the reason and where to look next.
    /// The layouts come from live store items on 04.10.2026.
    #[test]
    fn widget_and_wallpaper_plugin_need_ready_kpackage() {
        let base = tmp_dir("kpkg_ready");
        let home = base.join("home");
        let plan = |ex: &std::path::Path, cat_id: i64| {
            let cat = crate::categories::category_by_id(&home, cat_id).expect("раздел");
            let snap = install_tree_snapshot(&s(ex), &[cat.install_dir.clone()]);
            let root = find_content_root(&snap, &s(ex));
            plan_install_with(&home, &snap, &cat, &root, Some(&[]))
        };

        // Widget sources: CMake at the top, QML packages deeper in package/.
        let src = base.join("awesome");
        put(&src.join("awesome-widgets/CMakeLists.txt"), "project(aw)");
        put(&src.join("awesome-widgets/awesomewidgets/plugin.cpp"), "//");
        put(&src.join("awesome-widgets/awesome-widget/package/metadata.json"), "{}");
        put(&src.join("awesome-widgets/awesome-widget/package/contents/ui/main.qml"), "Item {}");
        let out = plan(&src, 706);
        assert!(!out.ok);
        assert_eq!(out.message, not_a_package_message());
        assert!(out.steps.is_empty() && out.record.is_none());

        // A pacman package among wallpaper plugins: the content root is usr/.
        let pkg = base.join("pacman");
        put(&pkg.join(".PKGINFO"), "pkgname = x");
        put(&pkg.join(".MTREE"), "");
        put(&pkg.join("usr/share/plasma/wallpapers/com.example.we/metadata.json"), "{}");
        put(&pkg.join("usr/share/plasma/wallpapers/com.example.we/contents/ui/main.qml"), "Item {}");
        let out = plan(&pkg, 715);
        assert!(!out.ok);
        assert_eq!(out.message, not_a_package_message());

        // A Dolphin menu with a script in the widgets section: there is no package at all.
        let menu = base.join("menu");
        put(&menu.join("Print_Selected_image/bin/print-image.sh"), "#!/bin/sh");
        put(&menu.join("Print_Selected_image/.local/share/kio/servicemenus/PrintImage.desktop"), "");
        put(&menu.join("Print_Selected_image/Readme.txt"), "");
        let out = plan(&menu, 709);
        assert!(!out.ok);
        assert_eq!(out.message, no_package_inside_message());
        assert_ne!(no_package_inside_message(), not_a_package_message());

        // A ready wallpaper plugin is installed by KPlugin.Id; CMake next to a
        // ready package does not interfere (that is how com.t145.matrixrain is
        // published).
        let ready = base.join("ready");
        put(&ready.join("rain/metadata.json"), r#"{"KPlugin":{"Id":"org.example.rain"}}"#);
        put(&ready.join("rain/contents/ui/main.qml"), "Item {}");
        put(&ready.join("rain/CMakeLists.txt"), "project(rain)");
        let out = plan(&ready, 715);
        assert!(out.ok, "{}", out.message);
        assert_eq!(
            out.record.expect("запись").paths,
            [s(&home.join(".local/share/plasma/wallpapers/org.example.rain"))]
        );
    }

    /// A widget from a subcategory (clocks, 708) belongs to the "Widgets" section: a plasmoid.
    #[test]
    fn widget_from_subcategory_installs_as_plasmoid() {
        let base = tmp_dir("plasmoid_708");
        let (home, ex) = (base.join("home"), base.join("ex"));
        put(&ex.join("clock/metadata.json"), r#"{"KPlugin":{"Id":"org.example.clock"}}"#);
        put(&ex.join("clock/contents/ui/main.qml"), "Item {}");
        let cat = crate::categories::category_by_id(&home, 708).expect("раздел");
        assert_eq!(cat.apply_kind, "plasmoids");
        let snap = install_tree_snapshot(&s(&ex), &[cat.install_dir.clone()]);
        let root = find_content_root(&snap, &s(&ex));
        let out = plan_install_with(&home, &snap, &cat, &root, Some(&[]));
        assert!(out.ok, "{}", out.message);
        let rec = out.record.expect("запись");
        assert_eq!(
            rec.paths,
            [s(&home.join(".local/share/plasma/plasmoids/org.example.clock"))]
        );
        assert_eq!(rec.rexon_type, "theme-plasmoid");
    }

    #[test]
    fn archive_without_sound_theme_is_refused() {
        let base = tmp_dir("snd_none");
        let (home, ex) = (base.join("home"), base.join("ex"));
        put(&ex.join("pack/bell.wav"), "wav");
        put(&ex.join("pack/ReadMe"), "r");
        let out = install_tree(&home, &ex, "sounds", Some(&[]));
        assert!(!out.ok);
        assert_eq!(out.message, no_sound_theme_message());
        assert!(!home.join(".local/share/sounds/pack").exists());
    }

    /// A name taken by a foreign theme is not overwritten; our own previous one is replaced.
    #[test]
    fn taken_name_is_skipped_honestly_but_own_reinstall_replaces() {
        let base = tmp_dir("snd_conflict");
        let (home, ex) = (base.join("home"), base.join("ex"));
        zorin_archive(&ex);
        let foreign = home.join(".local/share/sounds/Zorin");
        put(&foreign.join("index.theme"), "[Sound Theme]\nName=Чужая\n");
        let out = install_tree(&home, &ex, "sounds", Some(&[]));
        assert!(out.ok);
        assert_eq!(out.names, ["Zorin2"]);
        assert!(out.message.contains("Zorin"), "{}", out.message);
        assert_eq!(
            std::fs::read_to_string(foreign.join("index.theme")).unwrap(),
            "[Sound Theme]\nName=Чужая\n",
            "чужая тема не тронута"
        );
        // The same purchase is installed again: its paths from the log are replaceable.
        let own = vec![s(&foreign), s(&home.join(".local/share/sounds/Zorin2"))];
        let again = install_tree(&home, &ex, "sounds", Some(&own));
        assert_eq!(again.names, ["Zorin", "Zorin2"]);
        assert_eq!(std::fs::read_to_string(foreign.join("index.theme")).unwrap(), SOUND_INDEX);
    }

    #[test]
    fn duplicate_names_inside_archive_are_reported() {
        let base = tmp_dir("snd_dup");
        let (home, ex) = (base.join("home"), base.join("ex"));
        put(&ex.join("w/a/Zorin/index.theme"), SOUND_INDEX);
        put(&ex.join("w/b/Zorin/index.theme"), SOUND_INDEX);
        let out = install_tree(&home, &ex, "sounds", Some(&[]));
        assert!(out.ok);
        assert_eq!(out.names, ["Zorin"]);
        assert_eq!(out.message.lines().count(), 2, "{}", out.message);
    }

    /// A theme at the very root of the archive is installed as before: the name comes from the directory.
    #[test]
    fn top_level_sound_theme_keeps_old_path() {
        let base = tmp_dir("snd_top");
        let (home, ex) = (base.join("home"), base.join("ex"));
        put(&ex.join("Ocean/index.theme"), SOUND_INDEX);
        let out = install_tree(&home, &ex, "sounds", None);
        assert!(out.ok);
        assert_eq!(out.names, ["Ocean"]);
        assert!(home.join(".local/share/sounds/Ocean/index.theme").is_file());
    }

    /// The shared `hicolor` set from an archive is not installed as a theme:
    /// neither next to a theme (roots), nor as the only content (the old path),
    /// nor under the old "taken gets overwritten" behaviour (`own = None`). The
    /// person's directory with program icons stays as it was (defect of
    /// 09.11.2026).
    #[test]
    fn shared_hicolor_from_archive_is_never_installed() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let base = tmp_dir("hicolor_guard");
        let home = base.join("home");
        let user_hicolor = home.join(".local/share/icons/hicolor");
        put(&user_hicolor.join("48x48/apps/app.png"), "png");
        const CUT: &str = "[Icon Theme]\nName=Hicolor\nDirectories=48x48/apps\n[48x48/apps]\nSize=48\n";

        let ex = base.join("pack");
        put(&ex.join("Pack/MyIcons/index.theme"), "[Icon Theme]\nName=My\nDirectories=apps\n");
        put(&ex.join("Pack/MyIcons/apps/a.svg"), "");
        put(&ex.join("Pack/hicolor/index.theme"), CUT);
        put(&ex.join("Pack/hicolor/48x48/apps/my.png"), "");
        for (n, own) in [None, Some(&[][..])].into_iter().enumerate() {
            // A home of its own per pass: otherwise the second sees the first's MyIcons as foreign.
            let home = base.join(format!("home-{n}"));
            let out = install_tree(&home, &ex, "icons", own);
            assert!(out.ok, "{}", out.message);
            assert_eq!(out.names, ["MyIcons"]);
            assert!(out.message.contains("“hicolor” skipped: this is the shared icon set"), "{}", out.message);
            assert!(!out.steps.iter().any(|st| format!("{st:?}").contains("icons/hicolor")), "{:?}", out.steps);
        }

        let ex = base.join("bare");
        put(&ex.join("hicolor/index.theme"), CUT);
        put(&ex.join("hicolor/48x48/apps/my.png"), "");
        for kind in ["icons", "cursor"] {
            let out = install_tree(&home, &ex, kind, None);
            assert!(!out.ok, "{kind}: {}", out.message);
            assert!(out.message.contains("shared icon set"), "{}", out.message);
        }
        assert!(!user_hicolor.join("index.theme").exists(), "index.theme в общий набор не лёг");
        assert_eq!(listing(&user_hicolor), ["48x48"]);
        assert!(user_hicolor.join("48x48/apps/app.png").is_file(), "значки программ целы");
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn gtk_variants_icons_cursors_aurorae_colors_from_nested_archives() {
        let base = tmp_dir("roots_kinds");
        let home = base.join("home");

        let ex = base.join("gtk");
        put(&ex.join("Orchis/README.md"), "r");
        put(&ex.join("Orchis/Orchis-Dark/gtk-3.0/gtk.css"), "");
        put(&ex.join("Orchis/Orchis-Light/gtk-3.0/gtk.css"), "");
        let out = install_tree(&home, &ex, "gtk", Some(&[]));
        assert_eq!(out.names, ["Orchis-Dark", "Orchis-Light"]);
        assert_eq!(listing(&home.join(".local/share/themes")), ["Orchis-Dark", "Orchis-Light"]);

        let ex = base.join("icons");
        put(&ex.join("Pack/MyIcons/index.theme"), "[Icon Theme]\nName=My\nDirectories=apps\n");
        put(&ex.join("Pack/MyIcons/apps/a.svg"), "");
        put(&ex.join("Pack/MyCursor/cursors/left_ptr"), "");
        put(&ex.join("Pack/wallpaper.png"), "");
        let out = install_tree(&home, &ex, "icons", Some(&[]));
        assert_eq!(out.names, ["MyCursor", "MyIcons"]);
        assert_eq!(listing(&home.join(".local/share/icons")), ["MyCursor", "MyIcons"]);

        let ex = base.join("aurorae");
        put(&ex.join("Pack/Deco/decoration.svg"), "");
        put(&ex.join("Pack/Deco/Decorc"), "");
        let out = install_tree(&home, &ex, "aurorae", Some(&[]));
        assert_eq!(out.names, ["Deco"]);
        assert!(home.join(".local/share/aurorae/themes/Deco/decoration.svg").is_file());

        let ex = base.join("colors");
        put(&ex.join("Pack/colors/Nord.colors"), "[General]\n");
        put(&ex.join("Pack/README"), "");
        let out = install_tree(&home, &ex, "colors", Some(&[]));
        assert!(out.ok, "{}", out.message);
        assert_eq!(out.names, ["Nord"]);
        assert!(home.join(".local/share/color-schemes/Nord.colors").is_file());
    }

    /// Where the owner's "Sounds" name came from: the archive of the item
    /// "ZorinOS 6 Sound Theme" is a top folder `Sounds` with a ReadMe and the
    /// theme `Zorin` inside. The old installation put the top folder as the
    /// theme and applied its name. Now the directory with `index.theme` becomes
    /// the theme, and the wrapper name does not count as a theme: it cannot be
    /// applied (`sound_theme_exists`).
    #[test]
    fn archive_wrapper_name_never_becomes_sound_theme_name() {
        let base = tmp_dir("snd_wrapper_name");
        let (home, ex) = (base.join("home"), base.join("ex"));
        put(&ex.join("Sounds/ReadMe"), "r");
        put(&ex.join("Sounds/Zorin/index.theme"), SOUND_INDEX);
        put(&ex.join("Sounds/Zorin/stereo/bell.oga"), "oga");
        let out = install_tree(&home, &ex, "sounds", Some(&[]));
        assert!(out.ok, "{}", out.message);
        assert_eq!(out.names, ["Zorin"]);
        let sounds = home.join(".local/share/sounds");
        assert_eq!(listing(&sounds), ["Zorin"]);
        let dirs = vec![s(&sounds)];
        assert!(crate::qt_settings::sound_theme_exists(&dirs, "Zorin"));
        assert!(!crate::qt_settings::sound_theme_exists(&dirs, "Sounds"));
    }

    /// A live case on 13.10.2026 on a fake HOME: the GTK files name the sound
    /// theme "Sounds", which does not exist (Zorin lies at the top). The
    /// sanitizer does NOT rewrite the file and says so in a remark; a broken
    /// icon theme in the same file is still repaired, while the sound stays as
    /// it was.
    #[test]
    fn gtk_sanitizer_keeps_missing_sound_theme_and_reports_it() {
        let base = tmp_dir("gtk-sound-kept");
        let home = base.join("home");
        let cfg = home.join(".config");
        let data = home.join(".local/share");
        put(&data.join("sounds/Zorin/index.theme"), SOUND_INDEX);
        let ini3 = cfg.join("gtk-3.0/settings.ini");
        let ini4 = cfg.join("gtk-4.0/settings.ini");
        let text = "[Settings]\ngtk-sound-theme-name=Sounds\ngtk-enable-event-sounds=1\n";
        put(&ini3, text);
        put(&ini4, text);

        assert!(gtk_ini_sanitize_at(&s(&home), &s(&cfg), &s(&data)).is_empty());
        assert_eq!(std::fs::read_to_string(&ini3).unwrap(), text);
        assert_eq!(std::fs::read_to_string(&ini4).unwrap(), text);
        let notes = gtk_notes_at(&s(&home), &s(&cfg), &s(&data));
        assert_eq!(notes.len(), 2, "{notes:?}");
        assert!(notes[0].contains("gtk-3.0/settings.ini: gtk-sound-theme-name"), "{}", notes[0]);
        assert!(notes[1].contains("gtk-4.0/settings.ini: gtk-sound-theme-name"), "{}", notes[1]);

        // An existing theme: no remarks.
        put(&ini3, "[Settings]\ngtk-sound-theme-name=Zorin\n");
        put(&ini4, "[Settings]\ngtk-sound-theme-name=Zorin\n");
        assert!(gtk_notes_at(&s(&home), &s(&cfg), &s(&data)).is_empty());

        // Broken icons are repaired, the sound next to them is not touched.
        put(&ini3, "[Settings]\ngtk-icon-theme-name=NoSuchIcons-rexon-test\ngtk-sound-theme-name=Sounds\n");
        let changes = gtk_ini_sanitize_at(&s(&home), &s(&cfg), &s(&data));
        assert_eq!(changes.len(), 1, "{changes:?}");
        let after = std::fs::read_to_string(&ini3).unwrap();
        assert!(after.contains("gtk-sound-theme-name=Sounds"), "{after}");
        assert!(!after.contains("NoSuchIcons-rexon-test"), "{after}");
    }

    /// Migration on a fake HOME: Rexon wrappers are taken apart, foreign things are not touched.
    #[test]
    fn migration_lifts_nested_themes_and_rewrites_journal() {
        let base = tmp_dir("migrate");
        let home = base.join("home");
        let sounds = home.join(".local/share/sounds");
        let data = home.join(".local/share");
        let log = home.join(".config/rexon/ocs_installed.json");

        // As on the owner's machine on 03.10: zorinos2 (two themes deep + wallpapers) and Sounds/Zorin.
        let z = sounds.join("zorinos2");
        put(&z.join(crate::installed_scan::REXON_PREVIEW_NAME), "png");
        put(&z.join("backgrounds/Waves.jpg"), "jpg");
        for t in ["Zorin", "Zorin2"] {
            put(&z.join(format!("sounds/{t}/index.theme")), SOUND_INDEX);
        }
        let w = sounds.join("Sounds");
        put(&w.join("ReadMe"), "r");
        put(&w.join("Zorin/index.theme"), SOUND_INDEX);
        // Only the log knows the Sounds wrapper (there is no marker).
        // A foreign one (no marker and not in the log): do not touch.
        let f = sounds.join("Foreign");
        put(&f.join("Inner/index.theme"), SOUND_INDEX);
        // A correct theme at the top: do not touch.
        put(&sounds.join("Ocean/index.theme"), SOUND_INDEX);
        put(
            &log,
            &format!(
                "{{\"1\": {{\"name\": \"a\", \"paths\": [\"{}\"]}}, \"2\": {{\"name\": \"b\", \"paths\": [\"{}\"]}}}}",
                s(&w),
                s(&z)
            ),
        );

        let lines = migrate_nested_themes_at(&s(&home), &s(&data), &s(&log));
        assert!(!lines.is_empty());
        // "Sounds" comes before "zorinos2": its Zorin takes the name first.
        assert_eq!(listing(&sounds), ["Foreign", "Ocean", "Zorin", "Zorin2", "zorinos2"]);
        assert!(sounds.join("Zorin2/index.theme").is_file());
        assert!(
            sounds.join("Zorin2").join(crate::installed_scan::REXON_PREVIEW_NAME).is_file(),
            "превью Rexon переехало к поднятой теме"
        );
        // Conflict: the second Zorin stayed in the wrapper, the wrapper is in place.
        assert!(z.join("sounds/Zorin/index.theme").is_file());
        assert!(f.join("Inner/index.theme").is_file(), "чужое не тронуто");
        // The rest of the Sounds wrapper goes to theme-leftovers; nothing is lost.
        let left = data.join("rexon/theme-leftovers/sounds/Sounds");
        assert!(left.join("ReadMe").is_file());
        // The installed log points to the new places.
        let j: Value = serde_json::from_str(&std::fs::read_to_string(&log).unwrap()).unwrap();
        let p1: Vec<&str> = j["1"]["paths"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
        assert_eq!(p1, [s(&sounds.join("Zorin")), s(&left)]);
        let p2: Vec<&str> = j["2"]["paths"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
        assert_eq!(p2, [s(&sounds.join("Zorin2")), s(&z)]);
        // The migration log is written.
        let mlog = std::fs::read_to_string(nested_migration_log_path(&s(&data))).unwrap();
        assert!(mlog.contains("конфликт"), "{mlog}");
    }

    /// The "theme with icons and a cursor" bench is NOT for a normal run: live
    /// environment commands (gsettings and the like). Run it only under
    /// `dbus-run-session` with `GSETTINGS_BACKEND=keyfile` and temporary
    /// `HOME`/`XDG_*` (the `HOME` path must contain `deps-stand`), without
    /// `DISPLAY`/`WAYLAND_DISPLAY`:
    /// `REXON_DEPS_STAND_THEME=<theme> REXON_DEPS_STAND_DE=mate cargo test -p
    /// rexon-core --lib theme_deps_stand -- --ignored --nocapture`.
    /// `REXON_DEPS_STAND_INSTALL=1` is consent to installing what is missing
    /// (the install commands on the bench must be substituted in `PATH`).
    #[test]
    #[ignore]
    fn theme_deps_stand() {
        let home = std::env::var("HOME").unwrap_or_default();
        let Ok(theme) = std::env::var("REXON_DEPS_STAND_THEME") else { return };
        assert!(home.contains("deps-stand"), "стенд только во временном HOME, а не в {home}");
        let de = std::env::var("REXON_DEPS_STAND_DE").unwrap_or_default();
        let mut installer = LiveInstaller::new(&de, Box::new(|line: &str| println!("журнал: {line}")));
        installer.allow_deps_install(std::env::var("REXON_DEPS_STAND_INSTALL").is_ok());
        let item = ThemeItem { category_id: 135, ..ThemeItem::default() };
        for i in installer.theme_deps_plan(&theme) {
            println!("просьба: {i:?}");
        }
        if std::env::var("REXON_DEPS_STAND_INSTALL").is_ok() {
            if let Err(why) = installer.prepare_theme_deps(&item, &theme) {
                println!("отказ: {why}");
                return;
            }
        }
        let (ok, msg) = installer.apply(&item, &theme);
        println!("итог ({ok}):\n{msg}");
    }

    /// B9: without the session helper, exactly the old command for installing
    /// appearance packages (the same argv for both kinds of the old path).
    #[test]
    fn repo_install_op_legacy_argv_is_todays() {
        use crate::privilege::op::Helpers;
        use crate::system::PkgFamily;
        let repo = vec!["conky".to_string(), "mint-y-icons".to_string()];
        for fam in [PkgFamily::Arch, PkgFamily::Debian, PkgFamily::Fedora, PkgFamily::Suse] {
            for pkg in [true, false] {
                let op = repo_install_op(fam, &repo, PkgSource::ThemeSets);
                let helpers = Helpers { pkg, ..Helpers::default() };
                assert_eq!(op.pkexec_argv(&helpers), crate::system_plan::install_argv(fam, &repo, pkg), "{fam:?}/{pkg}");
            }
        }
    }
}
