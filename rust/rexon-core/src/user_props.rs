//! Parsing of the `project.json` of Wallpaper Engine wallpapers and user properties
//! (`userProperties`) — port of `rexon/wallpapers/user_props.py`.
//!
//! A tolerant parser: unknown fields go to `extra`, `type`/`mode`
//! are lowercased, `workshopid` always becomes a string
//! (in Workshop files it is sometimes a number, sometimes a string), missing
//! values are replaced with sensible defaults.
//!
//! Real edge cases of the WE Workshop that make the parser this way:
//!   * `type` with a capital letter (`"Web"`, `"Video"`) → lowercase;
//!   * `workshopid: 859267594` as a number → a string;
//!   * `preview` can be `.gif`/`.png`/`.jpg` — read as is;
//!   * a property without `type` is a `label` (an HTML/BBCode insert);
//!   * `color.value` — `"0.6 0.6 0.6"` (three floats separated by spaces);
//!   * titles with CJK/Cyrillic characters — UTF-8 is mandatory.
//!
//! The core is pure: [`parse_project`] works on already parsed JSON,
//! [`parse_project_text`]/[`parse_project_bytes`] on the file contents, and
//! only the thin wrapper [`parse_project_json`] reads the file from disk (the same
//! "pure core + wrapper over a path" boundary as in `favorites.rs`).
//!
//! ⚠ rule #16 — defects of the original fixed during the port. The module claimed to be
//! tolerant, but actually raised an exception or produced garbage:
//!
//!   1. **`null` turned into the string `"None"`.** `str(raw.get("preview"))`
//!      on `"preview": null` gave a file named `None`, `"type": null` —
//!      wallpaper type `none`, `"workshopid": null` — the identifier `"None"`.
//!      The same for `file`/`title`/`description`/`contentrating`/`visibility`
//!      and for the property's `text`/`condition`/`mode`/`fileType`/`label`. Here
//!      `null` = "no field" → the default.
//!   2. **`int(...)` on `order`/`index` failed the WHOLE parse.**
//!      `"order": "abc"` → `ValueError`, `"order": "3.5"` → `ValueError`,
//!      `"order": [1]` → `TypeError`; because of one bad property the wallpaper
//!      was left without any user settings at all. Here the conversion is
//!      tolerant: non-numeric → `0`, fractional — truncation towards zero (as `int()`).
//!   3. **A non-object instead of a dict failed the parse with `AttributeError`.**
//!      `"general": [1,2]`, `"general": {"properties": [1,2]}` and
//!      `project.json` with an array/string at the top level. Here: a non-object
//!      top level is an explicit error [`ParseError::NotObject`], while a non-object
//!      `general`/`properties` is just empty.
//!   4. **`tags` were iterated character by character.** `"tags": "abc"` gave
//!      `["a","b","c"]`, `"tags": {"a":1,"b":2}` — `["a","b"]` (the keys).
//!      Here tags are taken only from an array, `null`/array/object elements
//!      are skipped (they used to give the tag `"None"`/`"[1, 2]"`).
//!   5. **`supportsaudioprocessing: "false"` enabled audio.**
//!      `bool("false")` in Python is true. Here the string is parsed by
//!      meaning (`false`/`0`/`no`/`off`/empty — false).
//!   6. **`version`, despite the `Optional[int]` signature, was returned as
//!      is** (`3.9`, `"3"`). Here — a tolerant conversion to an integer.
//!   7. **`localization`, despite the signature, could end up being anything**
//!      (`[1,2]` went into a field of type "language → key → string"). Here it
//!      is built only from nested objects.
//!   8. **A broken byte in the file failed the parse with `UnicodeDecodeError`.**
//!      Workshop files are sometimes not in UTF-8; decoding is now
//!      lossy — a garbage byte is replaced, the other fields are read.
//!
//! Quirks of the original kept as is (behaviour does not change):
//!   * `min`/`max`/`step`/`precision`/`fraction` are stored as RAW JSON
//!     values (in WE they are sometimes numbers, sometimes strings) — typed access
//!     is given by [`UserProperty::min_num`] and its neighbours;
//!   * a non-object instead of the property itself (`"p": "string"`) stays
//!     a `label` property with a string value;
//!   * a property of type `group`/`label` does not go into [`Project::defaults_for_js`];
//!   * a property without a value (or with an empty string) gives `{}`, not
//!     `{"value": ""}` — the default picture substitution of web wallpapers
//!     relies on this.

use std::path::Path;

use serde::Serialize;
use serde_json::{Map, Value};

/// A drop-down list option (`type: combo`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ComboOption {
    pub label: String,
    /// WE allows both a string and a number. `Value::Null` — the file had `null`,
    /// `""` — there was no `value` field at all (the original's default).
    pub value: Value,
}

/// One `user_property` from `project.json::general.properties`.
///
/// `r#type`: `bool` | `color` | `slider` | `combo` | `textinput` |
/// `directory` | `file` | `group` | `label`. The `color.value` format is
/// `"R G B"` separated by spaces, floats in the range `[0..1]`.
/// ⚠️ The field names here are the OUTWARD SHAPE (`r#type` goes out as `type`): it is
/// declared on the type itself, not rewritten per field by a branch in the bridge.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UserProperty {
    pub name: String,
    /// `label` — a property without `type` (an HTML/BBCode insert).
    pub r#type: String,
    /// There was no value (or it is `null`) — `None`.
    pub value: Option<Value>,
    pub text: String,
    pub order: i64,
    pub index: i64,
    pub condition: String,
    pub min: Option<Value>,
    pub max: Option<Value>,
    pub step: Option<Value>,
    pub precision: Option<Value>,
    pub fraction: Option<Value>,
    pub options: Vec<ComboOption>,
    /// For `type: directory`.
    pub mode: Option<String>,
    /// `video` | `image`.
    pub file_type: Option<String>,
    pub extra: Map<String, Value>,
}

impl Default for UserProperty {
    fn default() -> Self {
        Self {
            name: String::new(),
            r#type: "label".into(),
            value: None,
            text: String::new(),
            order: 0,
            index: 0,
            condition: String::new(),
            min: None,
            max: None,
            step: None,
            precision: None,
            fraction: None,
            options: Vec::new(),
            mode: None,
            file_type: None,
            extra: Map::new(),
        }
    }
}

impl UserProperty {
    /// `min` as a number (a number or a numeric string), otherwise `None`.
    pub fn min_num(&self) -> Option<f64> {
        as_f64(self.min.as_ref())
    }
    /// `max` as a number.
    pub fn max_num(&self) -> Option<f64> {
        as_f64(self.max.as_ref())
    }
    /// `step` as a number.
    pub fn step_num(&self) -> Option<f64> {
        as_f64(self.step.as_ref())
    }
    /// `precision` as an integer.
    pub fn precision_int(&self) -> Option<i64> {
        as_f64(self.precision.as_ref()).map(|v| v.trunc() as i64)
    }
    /// `fraction` as a boolean.
    pub fn fraction_bool(&self) -> Option<bool> {
        self.fraction.as_ref().map(truthy)
    }
}

/// A parsed `project.json`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Project {
    /// `web` | `scene` | `video` | `application`.
    pub r#type: String,
    /// Entry point, a path relative to the wallpaper directory.
    pub file: String,
    pub title: String,
    pub description: String,
    /// Default — `preview.jpg`.
    pub preview: String,
    pub tags: Vec<String>,
    pub content_rating: String,
    /// Default — `public`.
    pub visibility: String,
    /// ALWAYS a string (in files it is sometimes a number).
    pub workshop_id: Option<String>,
    pub workshop_url: Option<String>,
    pub version: Option<i64>,
    /// Properties in file order (in Python — a plain `dict`).
    pub properties: Vec<(String, UserProperty)>,
    pub supports_audio: bool,
    /// `general.localization`: language → key → translation, in file order.
    pub localization: Vec<(String, Vec<(String, String)>)>,
    pub extra: Map<String, Value>,
}

impl Project {
    /// A property by name.
    pub fn property(&self, name: &str) -> Option<&UserProperty> {
        self.properties.iter().find(|(k, _)| k == name).map(|(_, p)| p)
    }

    /// The WE format `{key: {value: X}}` for `applyUserProperties` in JS.
    ///
    /// Skips `type: group` and `type: label` (they are UI decoration only).
    /// A property without a value gives `{}` — important for the default picture
    /// substitution of web wallpapers.
    pub fn defaults_for_js(&self) -> Map<String, Value> {
        let mut out = Map::new();
        for (name, p) in &self.properties {
            if p.r#type == "group" || p.r#type == "label" {
                continue;
            }
            let empty = match &p.value {
                None => true,
                Some(Value::String(s)) => s.is_empty(),
                Some(_) => false,
            };
            if empty {
                out.insert(name.clone(), Value::Object(Map::new()));
            } else {
                let mut wrap = Map::new();
                wrap.insert("value".into(), p.value.clone().unwrap());
                out.insert(name.clone(), Value::Object(wrap));
            }
        }
        out
    }
}

/// Why `project.json` failed to parse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// The file could not be read (the original's `OSError`).
    Io(String),
    /// The contents are not JSON (`json.JSONDecodeError`).
    NotJson(String),
    /// The JSON parsed, but the top level is not an object. The original crashed here
    /// with `AttributeError` ("'list' object has no attribute 'get'").
    NotObject(&'static str),
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::Io(e) => write!(f, "не удалось прочитать project.json: {e}"),
            ParseError::NotJson(e) => write!(f, "project.json — не JSON: {e}"),
            ParseError::NotObject(kind) => {
                write!(f, "project.json: на верхнем уровне {kind}, а нужен объект")
            }
        }
    }
}

/// Name of the JSON value kind — for the error text.
fn json_kind(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "логическое значение",
        Value::Number(_) => "число",
        Value::String(_) => "строка",
        Value::Array(_) => "массив",
        Value::Object(_) => "объект",
    }
}

// ── tolerant value conversions ──────────────────────────────────────────

/// A string field: `null` and compound values = "no field" (⚠ rule #16 —
/// the original printed them via `str()` and got `"None"` / `"[1, 2]"`).
/// Scalars are converted as in Python: `True`/`False`, a number — as written.
fn str_field(raw: Option<&Value>, default: &str) -> String {
    match raw {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Bool(true)) => "True".into(),
        Some(Value::Bool(false)) => "False".into(),
        Some(Value::Number(n)) => n.to_string(),
        _ => default.to_string(),
    }
}

/// The same, but "there was no field" differs from the default: `None` instead of a string.
fn opt_str_field(raw: Option<&Value>) -> Option<String> {
    match raw {
        Some(Value::String(s)) => Some(s.clone()),
        Some(Value::Bool(true)) => Some("True".into()),
        Some(Value::Bool(false)) => Some("False".into()),
        Some(Value::Number(n)) => Some(n.to_string()),
        _ => None,
    }
}

/// A number from a JSON value: a number or a numeric string (with surrounding whitespace).
fn as_f64(raw: Option<&Value>) -> Option<f64> {
    match raw {
        Some(Value::Number(n)) => n.as_f64(),
        Some(Value::String(s)) => s.trim().parse::<f64>().ok(),
        _ => None,
    }
}

/// An integer field (`order`/`index`) — tolerant (⚠ rule #16: the original's `int()`
/// failed the parse on a non-numeric string and on a list). `true` → 1, `false`/`null`/
/// empty/garbage → 0, fractional is truncated towards zero (as `int()` in Python).
fn int_field(raw: Option<&Value>) -> i64 {
    match raw {
        Some(Value::Bool(b)) => i64::from(*b),
        Some(Value::Number(n)) => n.as_f64().map_or(0, |v| v.trunc() as i64),
        Some(Value::String(s)) => s.trim().parse::<f64>().map_or(0, |v| v.trunc() as i64),
        _ => 0,
    }
}

/// An integer field that may be missing (`version`).
fn opt_int_field(raw: Option<&Value>) -> Option<i64> {
    match raw {
        Some(Value::Bool(b)) => Some(i64::from(*b)),
        Some(Value::Number(n)) => n.as_f64().map(|v| v.trunc() as i64),
        Some(Value::String(s)) => s.trim().parse::<f64>().ok().map(|v| v.trunc() as i64),
        _ => None,
    }
}

/// Truthiness of a value. A string is read by meaning (⚠ rule #16: in Python
/// `bool("false")` is true, which made `supportsaudioprocessing: "false"`
/// enable audio capture; false — `false`/`0`/`no`/`off`/`none`/empty). For
/// the other kinds — Python rules: an empty
/// container/zero/empty string is false.
fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|x| x != 0.0),
        Value::String(s) => !matches!(
            s.trim().to_ascii_lowercase().as_str(),
            "" | "0" | "false" | "no" | "off" | "none"
        ),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// Field value where `null` is indistinguishable from absence (as `dict.get`).
fn present(raw: Option<&Value>) -> Option<Value> {
    match raw {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.clone()),
    }
}

// ── parsing ──────────────────────────────────────────────────────────────

/// Property fields that are parsed explicitly (the rest go to `extra`).
const PROPERTY_CONSUMED: &[&str] = &[
    "type", "value", "text", "order", "index", "condition", "min", "max", "step",
    "precision", "fraction", "options", "mode", "fileType",
];

/// Project fields that are parsed explicitly (the rest go to `extra`).
const PROJECT_CONSUMED: &[&str] = &[
    "type", "file", "title", "description", "preview", "tags", "contentrating",
    "visibility", "workshopid", "workshopurl", "version", "general",
];

fn extra_of(raw: &Map<String, Value>, consumed: &[&str]) -> Map<String, Value> {
    raw.iter()
        .filter(|(k, _)| !consumed.contains(&k.as_str()))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

/// Parse one property. Tolerant to unknown fields.
pub fn parse_user_property(name: &str, raw: &Value) -> UserProperty {
    let Value::Object(obj) = raw else {
        // Edge case: sometimes `properties[name]` is just a string or
        // a number. That becomes a `label` insert with this value.
        return UserProperty {
            name: name.to_string(),
            value: present(Some(raw)),
            ..UserProperty::default()
        };
    };

    let mut options = Vec::new();
    if let Some(Value::Array(items)) = obj.get("options") {
        for o in items {
            if let Value::Object(od) = o {
                options.push(ComboOption {
                    label: str_field(od.get("label"), ""),
                    // `o.get("value", "")` — no field → an empty string,
                    // an explicit `null` → `null`.
                    value: od.get("value").cloned().unwrap_or(Value::String(String::new())),
                });
            }
        }
    }

    UserProperty {
        name: name.to_string(),
        r#type: str_field(obj.get("type"), "label").to_lowercase(),
        value: present(obj.get("value")),
        text: str_field(obj.get("text"), ""),
        order: int_field(obj.get("order")),
        index: int_field(obj.get("index")),
        condition: str_field(obj.get("condition"), ""),
        min: present(obj.get("min")),
        max: present(obj.get("max")),
        step: present(obj.get("step")),
        precision: present(obj.get("precision")),
        fraction: present(obj.get("fraction")),
        options,
        mode: opt_str_field(obj.get("mode")).map(|s| s.to_lowercase()),
        file_type: opt_str_field(obj.get("fileType")).map(|s| s.to_lowercase()),
        extra: extra_of(obj, PROPERTY_CONSUMED),
    }
}

/// Parse an already read `project.json` JSON.
///
/// The second element of the result is warnings for the log (currently one:
/// the missing `type` field, as the original's `log.warning`).
pub fn parse_project(raw: &Value) -> Result<(Project, Vec<String>), ParseError> {
    let Value::Object(obj) = raw else {
        return Err(ParseError::NotObject(json_kind(raw)));
    };
    let mut warnings = Vec::new();

    // A non-object `general` — treat it as missing (⚠ rule #16: the original
    // crashed with `AttributeError` on `"general": [1,2]`).
    let empty = Map::new();
    let general = match obj.get("general") {
        Some(Value::Object(g)) => g,
        _ => &empty,
    };

    let mut properties = Vec::new();
    if let Some(Value::Object(props)) = general.get("properties") {
        for (name, prop_raw) in props {
            properties.push((name.clone(), parse_user_property(name, prop_raw)));
        }
    }

    let wp_type = str_field(obj.get("type"), "").to_lowercase();
    if wp_type.is_empty() {
        warnings.push("project.json: поле type отсутствует".to_string());
    }

    // Tags — only from an array, scalars are converted to strings (⚠ rule #16:
    // the original iterated a string character by character and a dict by keys).
    let tags = match obj.get("tags") {
        Some(Value::Array(items)) => items.iter().filter_map(|t| opt_str_field(Some(t))).collect(),
        _ => Vec::new(),
    };

    // Localization — only nested objects with string values.
    let mut localization = Vec::new();
    if let Some(Value::Object(langs)) = general.get("localization") {
        for (lang, table) in langs {
            if let Value::Object(entries) = table {
                let rows = entries
                    .iter()
                    .filter_map(|(k, v)| opt_str_field(Some(v)).map(|s| (k.clone(), s)))
                    .collect();
                localization.push((lang.clone(), rows));
            }
        }
    }

    let project = Project {
        r#type: wp_type,
        file: str_field(obj.get("file"), ""),
        title: str_field(obj.get("title"), ""),
        description: str_field(obj.get("description"), ""),
        preview: str_field(obj.get("preview"), "preview.jpg"),
        tags,
        content_rating: str_field(obj.get("contentrating"), ""),
        visibility: str_field(obj.get("visibility"), "public"),
        workshop_id: opt_str_field(obj.get("workshopid")),
        workshop_url: opt_str_field(obj.get("workshopurl")),
        version: opt_int_field(obj.get("version")),
        properties,
        supports_audio: general.get("supportsaudioprocessing").is_some_and(truthy),
        localization,
        extra: extra_of(obj, PROJECT_CONSUMED),
    };
    Ok((project, warnings))
}

/// Parse the text of `project.json`.
pub fn parse_project_text(text: &str) -> Result<(Project, Vec<String>), ParseError> {
    let raw: Value =
        serde_json::from_str(text).map_err(|e| ParseError::NotJson(e.to_string()))?;
    parse_project(&raw)
}

/// Parse the bytes of `project.json`: lossy decoding (⚠ rule #16 —
/// the original failed the parse with `UnicodeDecodeError` on Workshop files not in
/// UTF-8).
pub fn parse_project_bytes(bytes: &[u8]) -> Result<(Project, Vec<String>), ParseError> {
    parse_project_text(&String::from_utf8_lossy(bytes))
}

/// Read and parse `project.json` from disk — the only place with
/// input/output.
pub fn parse_project_json(path: &Path) -> Result<(Project, Vec<String>), ParseError> {
    let bytes = std::fs::read(path).map_err(|e| ParseError::Io(e.to_string()))?;
    parse_project_bytes(&bytes)
}

// ── Detecting the wallpaper type by its contents ────────────────────────
//
// Port of `rexon/wallpapers/factory.py::_infer_type`. Some official
// Wallpaper Engine wallpapers (techno, audiophile and others) have no `type` field
// in `project.json` at all — in that case WE itself looks at the entry file and at what
// lies next to it. Without repeating this we lose such wallpapers: they do not fail
// with an error, they are simply not applied.
//
// Only this moved here from the factory. Everything else in it is choosing the engine
// class in Python, and it goes away together with Python.

/// Entry file extensions by which a wallpaper is considered a video.
///
/// ⚠️ `.ogv` ADDED (rule #16). The list of the Python video host knew it from
/// the very beginning — QtMultimedia plays such files — but this one did not, and
/// wallpapers with an `.ogv` entry and no `type` field in `project.json` were silently
/// not applied. Now there is ONE list for the whole product, no Python copy.
///
/// `.wmv`, `.mpg`, `.mpeg` (Windows DreamScene clips) the host plays by itself, without
/// transcoding: verified by running WMV3+WMA2 in ASF and MPEG-1/2.
pub const VIDEO_ENTRY_EXTS: &[&str] = &[
    ".mp4", ".webm", ".mkv", ".avi", ".mov", ".m4v", ".ogv", ".wmv", ".mpg", ".mpeg", ".gif",
];

/// Entry file extensions by which a wallpaper is considered a web page.
///
/// ⚠️ ADDED (rule #16). The original knew only `.html`, and an `.htm` entry
/// was not recognised AT ALL: such wallpapers were silently not applied. The neighbouring parser in
/// `wallpaper.rs` does know `.htm` — a divergence within one product.
pub const WEB_ENTRY_EXTS: &[&str] = &[".html", ".htm"];

/// File name of a packed scene.
pub const SCENE_PKG: &str = "scene.pkg";
/// File name of an unpacked scene.
pub const SCENE_JSON: &str = "scene.json";
/// Name of the entry page searched for when the entry file is not named.
pub const INDEX_HTML: &str = "index.html";

/// What is needed from a wallpaper directory to determine its type.
pub trait ProjectDirIo {
    /// `pathlib.Path(path).is_file()`.
    fn is_file(&mut self, path: &str) -> bool;
    /// `path.read_text(encoding="utf-8")`. `None` — the file did not open or
    /// it is not UTF-8; the original swallowed both.
    fn read_text(&mut self, path: &str) -> Option<String>;
}

fn join(dir: &str, name: &str) -> String {
    format!("{}/{}", dir.trim_end_matches('/'), name)
}

/// Whether the file holds an unpacked scene: an object with an `objects` array.
/// An empty array also counts as a scene — as in the original.
fn is_scene_json<I: ProjectDirIo + ?Sized>(path: &str, io: &mut I) -> bool {
    if !io.is_file(path) {
        return false;
    }
    let Some(text) = io.read_text(path) else {
        return false;
    };
    let Ok(Value::Object(obj)) = serde_json::from_str::<Value>(&text) else {
        return false;
    };
    matches!(obj.get("objects"), Some(Value::Array(_)))
}

/// Determine the wallpaper type by the directory contents when `project.json::type`
/// is empty. An empty string — nothing matched.
///
/// The order of the checks matters and is kept verbatim: video, then web, then
/// application, then scene. Because of this order an `.exe` entry next to
/// `index.html` gives `web`, not `application` — as the original behaved.
pub fn infer_type<I: ProjectDirIo + ?Sized>(
    wp_dir: &str,
    project: &Project,
    io: &mut I,
) -> &'static str {
    let entry = project.file.to_lowercase();

    if VIDEO_ENTRY_EXTS.iter().any(|e| entry.ends_with(e)) {
        return "video";
    }
    if WEB_ENTRY_EXTS.iter().any(|e| entry.ends_with(e)) || io.is_file(&join(wp_dir, INDEX_HTML)) {
        return "web";
    }
    if entry.ends_with(".exe") {
        return "application";
    }

    if io.is_file(&join(wp_dir, SCENE_PKG)) {
        return "scene";
    }
    // The entry file is checked first, `scene.json` as a fallback. The name is taken
    // NOT lowercased: only the tail was lowercased
    // for comparing the extension.
    if entry.ends_with(".json") && is_scene_json(&join(wp_dir, &project.file), io) {
        return "scene";
    }
    if is_scene_json(&join(wp_dir, SCENE_JSON), io) {
        return "scene";
    }
    ""
}

/// A real directory on disk.
///
/// Both methods swallow errors exactly where the original swallowed them: no file,
/// no permissions, contents not in UTF-8 — all of this is "did not match", not a failure.
struct FsDirIo;

impl ProjectDirIo for FsDirIo {
    fn is_file(&mut self, path: &str) -> bool {
        Path::new(path).is_file()
    }

    fn read_text(&mut self, path: &str) -> Option<String> {
        std::fs::read_to_string(path).ok()
    }
}

/// [`infer_type`] on a real directory on disk.
///
/// Called when `project.json::type` is empty. An empty string in the result means
/// nothing matched; the caller decides what to do next.
///
/// ⚠️ This is the ONLY place where the product determines the wallpaper type by
/// contents. Do not create your own extension lists next to it: that is exactly how
/// `VIDEO_ENTRY_EXTS` and the list of the Python video host drifted apart.
pub fn infer_type_fs(wp_dir: &Path, project: &Project) -> &'static str {
    infer_type(&wp_dir.to_string_lossy(), project, &mut FsDirIo)
}
