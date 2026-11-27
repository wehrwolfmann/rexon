//! The `Extension` class (a single `.rxn` extension): a port of
//! `rexon/extensions/extension.py` (the pure part).
//!
//! Ported: parsing and validating `manifest.json` (required fields `id`/
//! `name`/`type`, the `type ∈ VALID_TYPES` check), getter properties over the
//! manifest, and building the `get_info()` dictionary for the UI. Error texts
//! are reproduced byte for byte like Python's `ValueError`, and `get_info()`
//! is compared byte for byte with `json.dumps(..., ensure_ascii=False,
//! indent=1)` (golden test `extension_parity.rs`).
//!
//! NOT ported (side effects, not part of the Qt-free core): `install.sh`/
//! `uninstall.sh` (`subprocess`), installing fonts/`.desktop` entries
//! (`shutil`/`fc-cache`), `load_plugin`/`unload_plugin` (`importlib`),
//! `read_readme` (reading a file for the UI). Only parsing/representation
//! lives here.
//!
//! Deviations from Python (deliberate):
//!   * `get_info` in Python depends on the file system (`has_preview`/
//!     `preview_path` and so on check files in the extension directory); the
//!     port looks at the disk too. In the golden test the absolute directory
//!     prefix (machine-dependent, identical on both sides) is normalized to
//!     `DIR`, the rest is compared byte for byte.
//!   * If the manifest root is not a JSON object, Python's `"id" not in
//!     manifest` for a list/scalar yields a missing `id` field error; here
//!     the result is the same. The pathological case "manifest = JSON string"
//!     (Python would match a substring) is not reproduced: in real `.rxn`
//!     files the manifest is always an object.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::constants::{
    self, is_supported_font, type_icon, type_label, wallpaper_format_icon,
    wallpaper_format_label,
};

/// A manifest parsing error (the counterpart of `ValueError`/`FileNotFoundError`
/// in `Extension._load`). `Display` gives the same text as Python.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtensionError {
    /// `manifest.json` was not found in the directory (`FileNotFoundError`).
    ManifestNotFound(String),
    /// `manifest.json` is not valid JSON (`JSONDecodeError`).
    BadJson(String),
    /// A required field `id`/`name`/`type` is missing (`ValueError`).
    MissingField(&'static str),
    /// Unknown `type` (`ValueError`).
    UnknownType(String),
}

impl std::fmt::Display for ExtensionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExtensionError::ManifestNotFound(dir) => {
                f.write_str(&crate::i18n::trf("manifest.json not found in %1", &[dir]))
            }
            ExtensionError::BadJson(msg) => write!(f, "{msg}"),
            ExtensionError::MissingField(field) => {
                f.write_str(&crate::i18n::trf(
                    "manifest.json: the required field '%1' is missing",
                    &[field],
                ))
            }
            ExtensionError::UnknownType(t) => {
                f.write_str(&crate::i18n::trf(
                    "manifest.json: unknown type '%1'. Allowed: %2",
                    &[t, &sorted_valid_types_joined()],
                ))
            }
        }
    }
}

impl std::error::Error for ExtensionError {}

/// A single Rexon extension (`.rxn`).
#[derive(Debug, Clone)]
pub struct Extension {
    /// The extension directory.
    pub dir: PathBuf,
    /// The parsed `manifest.json` (an object).
    pub manifest: Map<String, Value>,
    /// Whether it is enabled (according to `enabled_state`).
    pub enabled: bool,
}

impl Extension {
    /// Builds an `Extension` from an already parsed manifest and a directory: the
    /// pure core of `_load` (without reading the file).
    ///
    /// `enabled_state` is a map `{id: bool}` (like `self.enabled = enabled_state
    /// .get(id, True)`; missing means enabled). Checks the required fields in the
    /// order `id`, `name`, `type`, and whether the type is allowed.
    pub fn from_manifest(
        manifest: Value,
        dir: impl Into<PathBuf>,
        enabled_state: &Map<String, Value>,
    ) -> Result<Self, ExtensionError> {
        // The manifest root must be an object. If not, behave like `"id" not in
        // <non-dict>` in Python: the required field id is "missing".
        let manifest = match manifest {
            Value::Object(m) => m,
            _ => return Err(ExtensionError::MissingField("id")),
        };

        for field in ["id", "name", "type"] {
            if !manifest.contains_key(field) {
                return Err(ExtensionError::MissingField(match field {
                    "id" => "id",
                    "name" => "name",
                    _ => "type",
                }));
            }
        }

        let ext_type = manifest
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if !constants::is_valid_type(ext_type) {
            return Err(ExtensionError::UnknownType(ext_type.to_string()));
        }

        let id = manifest.get("id").and_then(Value::as_str).unwrap_or_default();
        let enabled = enabled_state
            .get(id)
            .and_then(Value::as_bool)
            .unwrap_or(true);

        Ok(Extension {
            dir: dir.into(),
            manifest,
            enabled,
        })
    }

    /// Loads an extension from a directory: reads `manifest.json`, parses and
    /// validates it (a full port of `Extension.__init__`/`_load`, with the disk).
    pub fn load_from_dir(
        dir: impl Into<PathBuf>,
        enabled_state: &Map<String, Value>,
    ) -> Result<Self, ExtensionError> {
        let dir = dir.into();
        let manifest_path = dir.join("manifest.json");
        if !manifest_path.exists() {
            return Err(ExtensionError::ManifestNotFound(dir.display().to_string()));
        }
        let text = std::fs::read_to_string(&manifest_path)
            .map_err(|e| ExtensionError::BadJson(e.to_string()))?;
        let manifest: Value =
            serde_json::from_str(&text).map_err(|e| ExtensionError::BadJson(e.to_string()))?;
        Self::from_manifest(manifest, dir, enabled_state)
    }

    // ── Properties over the manifest ─────────────────────────────────────

    /// The extension `id` (`manifest.get("id", "")`).
    pub fn ext_id(&self) -> &str {
        self.manifest.get("id").and_then(Value::as_str).unwrap_or("")
    }

    /// The extension type (`manifest.get("type", "plugin")`). After loading it is
    /// always a valid type from `VALID_TYPES`.
    pub fn ext_type(&self) -> &str {
        self.manifest
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("plugin")
    }

    /// The main wallpaper format for `wallpaper-pack` (otherwise `None`):
    /// `manifest.get("wallpaper_format", "static")`.
    pub fn wallpaper_format(&self) -> Option<&str> {
        if self.ext_type() != "wallpaper-pack" {
            return None;
        }
        Some(
            self.manifest
                .get("wallpaper_format")
                .and_then(Value::as_str)
                .unwrap_or("static"),
        )
    }

    /// The list of wallpapers from the manifest (`manifest.get("wallpapers", [])`).
    fn wallpapers(&self) -> &[Value] {
        self.manifest
            .get("wallpapers")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// Ambient sounds (`manifest.get("ambient_sounds", [])`).
    fn ambient_sounds(&self) -> &[Value] {
        self.manifest
            .get("ambient_sounds")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// Whether there is sound: a non-empty `ambient_sounds`, or `has_audio` on any
    /// wallpaper, or `manifest["has_audio"]` (a port of `has_audio`, with Python
    /// truthiness).
    pub fn has_audio(&self) -> bool {
        if !self.ambient_sounds().is_empty() {
            return true;
        }
        for wp in self.wallpapers() {
            if is_truthy(wp.get("has_audio").unwrap_or(&Value::Bool(false))) {
                return true;
            }
        }
        is_truthy(self.manifest.get("has_audio").unwrap_or(&Value::Bool(false)))
    }

    /// The scene configuration for the `scene` format (otherwise `None`):
    /// `manifest.get("scene", {})`.
    pub fn scene_config(&self) -> Option<Value> {
        if self.wallpaper_format() != Some("scene") {
            return None;
        }
        Some(
            self.manifest
                .get("scene")
                .cloned()
                .unwrap_or_else(|| Value::Object(Map::new())),
        )
    }

    /// Font family names (`manifest.get("font_families", [])`).
    fn font_families(&self) -> Value {
        self.manifest
            .get("font_families")
            .cloned()
            .unwrap_or_else(|| Value::Array(Vec::new()))
    }

    // ── Files (disk) ─────────────────────────────────────────────────────

    /// The path to `preview.png`, if the file exists.
    pub fn preview_path(&self) -> Option<PathBuf> {
        file_if_exists(self.dir.join("preview.png"))
    }

    /// The path to `icon.png`, if the file exists.
    pub fn icon_path(&self) -> Option<PathBuf> {
        file_if_exists(self.dir.join("icon.png"))
    }

    /// The path to the README (the first existing one of four name variants).
    pub fn readme_path(&self) -> Option<PathBuf> {
        for name in ["README.md", "readme.md", "README.txt", "readme.txt"] {
            if let Some(p) = file_if_exists(self.dir.join(name)) {
                return Some(p);
            }
        }
        None
    }

    /// Font files in `data/` (or in the root if there is no `data/`) for
    /// `font-pack`, recursively, by extension from `SUPPORTED_FONT_FORMATS`.
    ///
    /// The format is matched by the whole suffix (`is_supported_font`), so the
    /// compound `.pcf.gz` matches too (see the bug fix there).
    fn font_files(&self) -> Vec<PathBuf> {
        if self.ext_type() != "font-pack" {
            return Vec::new();
        }
        let data_dir = self.dir.join("data");
        let root = if data_dir.is_dir() { data_dir } else { self.dir.clone() };
        let mut out = Vec::new();
        collect_fonts(&root, &mut out);
        out
    }

    // ── Info dict (for the UI) ───────────────────────────────────────────

    /// The extension information dictionary for the UI: a port of `get_info()`.
    ///
    /// The key order follows the Python dict (needed for the byte-for-byte
    /// comparison of the serialization). The blocks for `wallpaper-pack` and
    /// `font-pack` are appended at the end, as in the original.
    pub fn get_info(&self) -> Map<String, Value> {
        let mut info = Map::new();
        info.insert("id".into(), self.get_or("id", str_val("")));
        info.insert(
            "name".into(),
            self.get_or("name", str_val(&crate::i18n::trc("extension name", "Untitled"))),
        );
        info.insert("version".into(), self.get_or("version", str_val("0.0.0")));
        info.insert(
            "author".into(),
            self.get_or("author", str_val(&crate::i18n::trc("extension author", "Unknown"))),
        );
        info.insert("description".into(), self.get_or("description", str_val("")));

        let ext_type = self.ext_type().to_string();
        info.insert("type".into(), str_val(&ext_type));
        info.insert(
            "type_label".into(),
            // TRANSLATORS: type of an extension whose own type is unknown.
            str_val(&type_label(&ext_type).unwrap_or_else(|| crate::i18n::tr("Extension"))),
        );
        info.insert(
            "type_icon".into(),
            str_val(type_icon(&ext_type).unwrap_or("🧩")),
        );
        info.insert("de_support".into(), self.get_or("de_support", empty_array()));
        info.insert("tags".into(), self.get_or("tags", empty_array()));
        info.insert("enabled".into(), Value::Bool(self.enabled));
        info.insert("has_preview".into(), Value::Bool(self.preview_path().is_some()));
        info.insert("has_icon".into(), Value::Bool(self.icon_path().is_some()));
        info.insert("has_readme".into(), Value::Bool(self.readme_path().is_some()));
        info.insert("preview_path".into(), path_val(self.preview_path()));
        info.insert("icon_path".into(), path_val(self.icon_path()));
        info.insert("readme_path".into(), path_val(self.readme_path()));
        info.insert("license".into(), self.get_or("license", str_val("")));
        info.insert("homepage".into(), self.get_or("homepage", str_val("")));

        if ext_type == "wallpaper-pack" {
            let fmt = self.wallpaper_format().unwrap_or("static").to_string();
            info.insert("wallpaper_format".into(), str_val(&fmt));
            info.insert(
                "wallpaper_format_label".into(),
                str_val(&wallpaper_format_label(&fmt).unwrap_or_default()),
            );
            info.insert(
                "wallpaper_format_icon".into(),
                str_val(wallpaper_format_icon(&fmt).unwrap_or("")),
            );
            info.insert("has_audio".into(), Value::Bool(self.has_audio()));
            info.insert(
                "wallpaper_count".into(),
                Value::from(self.wallpapers().len()),
            );
            info.insert(
                "ambient_sound_count".into(),
                Value::from(self.ambient_sounds().len()),
            );
        }

        if ext_type == "font-pack" {
            info.insert("font_count".into(), Value::from(self.font_files().len()));
            info.insert("font_families".into(), self.font_families());
        }

        info
    }

    /// `manifest.get(key, default)`, keeping the JSON type of the value.
    fn get_or(&self, key: &str, default: Value) -> Value {
        self.manifest.get(key).cloned().unwrap_or(default)
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────

/// `VALID_TYPES`, sorted lexicographically and joined with `", "`, for the
/// error text (like `', '.join(sorted(VALID_TYPES))`).
fn sorted_valid_types_joined() -> String {
    let mut types: Vec<&str> = constants::valid_types();
    types.sort_unstable();
    types.join(", ")
}

/// The truthiness of a value by Python rules (`bool(v)`): an empty
/// string/array/object and `0`/`false`/`null` are false, everything else is true.
fn is_truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().map(|x| x != 0.0).unwrap_or(true),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// The path, if it is an existing file (the counterpart of `p if os.path.isfile(p) else None`).
fn file_if_exists(p: PathBuf) -> Option<PathBuf> {
    if p.is_file() {
        Some(p)
    } else {
        None
    }
}

/// Recursively collects font files (by the whole name suffix) into `out`.
fn collect_fonts(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_fonts(&path, out);
        } else if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if is_supported_font(name) {
                out.push(path);
            }
        }
    }
}

/// A JSON string.
fn str_val(s: &str) -> Value {
    Value::String(s.to_string())
}

/// An empty JSON array.
fn empty_array() -> Value {
    Value::Array(Vec::new())
}

/// Path → JSON string, or `null` (like `str | None` in Python).
fn path_val(p: Option<PathBuf>) -> Value {
    match p {
        Some(p) => Value::String(p.to_string_lossy().into_owned()),
        None => Value::Null,
    }
}
