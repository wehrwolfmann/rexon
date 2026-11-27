//! Port of `rexon/ocs_store.py` — detection of the actual type of a KDE theme
//! and the journal of installed items.
//!
//! The live theme installer asks this module two things: what type the
//! unpacked theme really is (authors on the KDE Store routinely upload Aurorae
//! or Kvantum themes into the "Global Theme" category, and applying them via
//! `plasma-apply-lookandfeel` then does not work) and what is already installed
//! — so that a card in "installed only" mode looks the same as in the catalog
//! without going to the network.
//!
//! Both tasks are purely computational here: the executor walks the directory
//! and reads the files and hands over its observations as a structure; it also
//! writes the journal.

use serde_json::{Map, Value};

// ============================================================================
// Detecting the actual type of a KDE theme
// ============================================================================

/// Observations about the unpacked theme directory — exactly what the original
/// obtained with `listdir`/`isdir`/`isfile` calls and by reading the metadata.
#[derive(Debug, Clone, Default)]
pub struct KdeContentScan {
    /// Whether the directory itself exists (`os.path.isdir(content_root)`).
    pub exists: bool,
    /// Directory contents; empty if listing failed.
    pub files: Vec<String>,
    /// The `files` entries that are directories.
    pub subdirs: Vec<String>,
    /// Whether there is a `contents` subdirectory.
    pub has_contents_dir: bool,
    /// Whether there is a `contents/defaults` file.
    pub has_contents_defaults: bool,
    /// Contents of `contents`; empty if listing failed.
    pub contents_subdirs: Vec<String>,
    /// Text of `metadata.desktop`, if the file exists and is readable.
    pub metadata_text: Option<String>,
}

/// Folders that mark a "full pack" of several KDE components.
const BUNDLE_MARKERS: &[&str] = &[
    "aurorae",
    "color-schemes",
    "kvantum",
    "Kvantum",
    "plasma",
    "icons",
    "wallpaper",
    "wallpapers",
    "sddm",
];

/// Files at the theme root that mark an Aurorae window decoration.
const AURORAE_MARKERS: &[&str] = &[
    "decoration.svg",
    "decoration.svgz",
    "close.svg",
    "minimize.svg",
    "maximize.svg",
];

/// Subdirectories typical of an icon theme.
const ICON_SUBDIRS: &[&str] = &[
    "apps",
    "places",
    "mimetypes",
    "actions",
    "categories",
    "devices",
    "status",
];

/// Subdirectories of `contents` typical of a Plasma Style.
const PLASMA_STYLE_SUBDIRS: &[&str] =
    &["widgets", "dialogs", "frames", "icons", "translucent"];

/// The actual type of the unpacked KDE theme, or `None` if it could not be
/// determined.
///
/// Warning: THE ORDER OF CHECKS MATTERS and follows the original: first the
/// "full pack" (two or more component folders), then an unambiguous color
/// scheme, then Aurorae, Kvantum, icons, and only at the end `contents` and
/// `metadata.desktop`. Changing the order changes the decision for themes that
/// match several signs at once.
pub fn detect_actual_kde_type(scan: &KdeContentScan) -> Option<&'static str> {
    if !scan.exists {
        return None;
    }

    let has_subdir = |name: &str| scan.subdirs.iter().any(|s| s == name);
    let has_file = |name: &str| scan.files.iter().any(|f| f == name);

    // Full pack: several KDE components in separate folders.
    if BUNDLE_MARKERS.iter().filter(|m| has_subdir(m)).count() >= 2 {
        return Some("bundle");
    }

    // A color scheme — an unambiguous sign.
    if scan.files.iter().any(|f| f.ends_with(".colors")) {
        return Some("color-scheme");
    }

    // Aurorae window decoration.
    if AURORAE_MARKERS.iter().any(|m| has_file(m)) {
        return Some("aurorae");
    }

    // Kvantum theme.
    if scan.files.iter().any(|f| f.ends_with(".kvconfig")) {
        return Some("kvantum");
    }

    // Icon theme: index.theme AND at least one typical subfolder. Without a
    // subfolder it is not icons — other theme types carry an index.theme too.
    if has_file("index.theme") && ICON_SUBDIRS.iter().any(|s| has_subdir(s)) {
        return Some("icons");
    }

    if scan.has_contents_dir {
        if scan.has_contents_defaults {
            return Some("look-and-feel");
        }
        if PLASMA_STYLE_SUBDIRS
            .iter()
            .any(|s| scan.contents_subdirs.iter().any(|c| c == s))
        {
            return Some("plasma-theme");
        }
    }

    // Last resort — the service type declared in the metadata.
    if let Some(text) = &scan.metadata_text {
        if text.contains("Plasma/LookAndFeel") {
            return Some("look-and-feel");
        }
        if text.contains("Plasma/Theme") {
            return Some("plasma-theme");
        }
    }

    None
}

// ============================================================================
// Journal of installed themes
// ============================================================================

/// Path of the installed-items journal.
pub fn install_log_path(home: &str) -> String {
    install_log_path_xdg(None, home)
}

/// The same, honoring `$XDG_CONFIG_HOME`.
pub fn install_log_path_xdg(xdg_config_home: Option<&str>, home: &str) -> String {
    format!(
        "{}/rexon/ocs_installed.json",
        crate::utils::config_home(xdg_config_home, home)
    )
}

/// Path of the temporary file for the journal's atomic write.
pub fn install_log_tmp_path(log_path: &str) -> String {
    format!("{log_path}.tmp")
}

/// Parse the journal. Any problem — a missing file, broken JSON, valid JSON
/// that is not an object at the top level — gives an empty journal: someone
/// else's corrupted file must not break the installed-item cards.
pub fn parse_installed_log(text: Option<&str>) -> Map<String, Value> {
    let Some(text) = text else {
        return Map::new();
    };
    match serde_json::from_str::<Value>(text) {
        Ok(Value::Object(map)) => map,
        _ => Map::new(),
    }
}

/// Card fields that the journal stores together with the entry. The original
/// read them from the object by duck typing; here the set is fixed by a type.
#[derive(Debug, Clone, Default)]
pub struct InstalledItem {
    pub content_id: String,
    pub name: String,
    pub rexon_type: String,
    pub author: String,
    pub description: String,
    pub summary: String,
    pub version: String,
    pub license: String,
    pub preview_url: String,
    pub score: i64,
    pub downloads: i64,
    pub tags: Vec<String>,
    pub web_url: String,
    pub provider_id: String,
    pub category_id: i64,
}

/// Journal entry for an installed theme.
///
/// Warning: KEY ORDER matters: the journal is written as indented JSON and
/// compared byte for byte, and Python preserves insertion order. Empty paths
/// are filtered out — they got into the list from failed installation steps.
pub fn installed_entry(
    item: &InstalledItem,
    paths: &[String],
    installed_at: &str,
) -> Value {
    let mut m = Map::new();
    m.insert("name".into(), Value::from(item.name.clone()));
    m.insert("rexon_type".into(), Value::from(item.rexon_type.clone()));
    m.insert(
        "paths".into(),
        Value::Array(
            paths
                .iter()
                .filter(|p| !p.is_empty())
                .map(|p| Value::from(p.clone()))
                .collect(),
        ),
    );
    m.insert("installed_at".into(), Value::from(installed_at));
    m.insert("author".into(), Value::from(item.author.clone()));
    m.insert("description".into(), Value::from(item.description.clone()));
    m.insert("summary".into(), Value::from(item.summary.clone()));
    m.insert("version".into(), Value::from(item.version.clone()));
    m.insert("license".into(), Value::from(item.license.clone()));
    m.insert("preview_url".into(), Value::from(item.preview_url.clone()));
    m.insert("score".into(), Value::from(item.score));
    m.insert("downloads".into(), Value::from(item.downloads));
    m.insert(
        "tags".into(),
        Value::Array(item.tags.iter().map(|t| Value::from(t.clone())).collect()),
    );
    m.insert("web_url".into(), Value::from(item.web_url.clone()));
    m.insert("provider_id".into(), Value::from(item.provider_id.clone()));
    m.insert("category_id".into(), Value::from(item.category_id));
    Value::Object(m)
}

/// Add (or replace) an entry in the journal.
pub fn record_installed(
    log: &mut Map<String, Value>,
    item: &InstalledItem,
    paths: &[String],
    installed_at: &str,
) {
    log.insert(
        item.content_id.clone(),
        installed_entry(item, paths, installed_at),
    );
}

/// Remove an entry; `false` — there was no entry (the file is then not rewritten).
pub fn forget_installed(log: &mut Map<String, Value>, content_id: &str) -> bool {
    log.remove(content_id).is_some()
}

/// Text of the journal file — as `json.dump(..., ensure_ascii=False, indent=2)`:
/// Cyrillic is not escaped, there is no trailing newline.
pub fn serialize_installed_log(log: &Map<String, Value>) -> String {
    serde_json::to_string_pretty(&Value::Object(log.clone()))
        .unwrap_or_else(|_| "{}".to_string())
}
