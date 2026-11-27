//! Schema of the adaptive video manifest `.rxn` v4 — port of
//! `rexon/extensions/manifest_v4.py`.
//!
//! Validation of the `manifest.json` structure (`validate_v4_manifest`) and
//! reading the manifest from the archive without unpacking it
//! (`parse_v4_manifest`). Error texts are reproduced byte for byte as in Python
//! (including the `{x!r}` repr) so that UI messages do not drift — checked by
//! the golden test `manifest_v4_parity.rs`.
//!
//! Deliberate deviations from Python:
//!   * the soft warning "default_locale is missing from audio_tracks" is only
//!     logged in Python and does not affect the result — it is omitted here
//!     (the core has no logger).
//!   * `isinstance(x, int)` in Python accepts `bool` (True == 1). Here a bool
//!     value in numeric fields does not count as a number — in real manifests
//!     these fields are numeric, so the edge case does not occur.
//!   * Python's `repr` for float is not reproduced (in manifests these fields
//!     are integers/strings); for int/str/None/bool the repr matches.

use std::io::Read;
use std::path::Path;

use serde_json::{Map, Value};

use crate::ext_utils::zip_error_text;
use crate::i18n::{tr, trf};

/// `format_version` of a v4 manifest.
pub const V4_FORMAT_VERSION: i64 = 4;
/// `type` of a v4 manifest.
pub const V4_TYPE: &str = "wallpaper-video-adaptive";

/// Resolutions supported by QML `pickVideo()` (see main.qml).
/// The order in error messages is Python's `sorted()` (lexicographic).
const ALLOWED_RESOLUTIONS: [&str; 4] = ["720p", "1080p", "1440p", "2160p"];
/// repr of the Python list `sorted(ALLOWED_RESOLUTIONS)` for the error text.
const ALLOWED_RESOLUTIONS_REPR: &str = "['1080p', '1440p', '2160p', '720p']";

/// Python `repr` of a value for error texts (`{x!r}`). Covers the types that
/// occur in manifests: None/bool/int/string.
fn py_repr(v: &Value) -> String {
    match v {
        Value::Null => "None".to_string(),
        Value::Bool(true) => "True".to_string(),
        Value::Bool(false) => "False".to_string(),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i.to_string()
            } else if let Some(u) = n.as_u64() {
                u.to_string()
            } else {
                // float — does not occur in manifests; best effort.
                n.to_string()
            }
        }
        Value::String(s) => format!("'{s}'"),
        // list/dict are not used in messages — a debugging fallback.
        other => other.to_string(),
    }
}

/// `isinstance(v, str) and bool(v.strip())` — a non-empty string (after strip).
fn is_nonempty_str(v: &Value) -> bool {
    matches!(v, Value::String(s) if !s.trim().is_empty())
}

/// `_is_locale_code`: exactly 2 characters, ASCII, lower-case letters (ISO 639-1).
fn is_locale_code(v: &Value) -> bool {
    let Value::String(s) = v else { return false };
    s.chars().count() == 2 && s.chars().all(|c| c.is_ascii_lowercase())
}

/// Field value (missing → `Null`, like `dict.get` without a default → None).
fn field<'a>(meta: &'a Map<String, Value>, key: &str) -> &'a Value {
    meta.get(key).unwrap_or(&Value::Null)
}

/// Soft warnings of a v4 manifest — things that do NOT block installation but
/// that the pack's author should know about. The core has no logger, so the
/// lines are simply returned: the caller writes them to the log (the
/// `manifest_v4.py` shim).
///
/// For now there is a single warning: `default_locale` has no track of its own
/// in a non-empty `audio_tracks` (QML will fall back, but the audio will be the
/// wrong one).
pub fn v4_soft_warnings(meta: &Value) -> Vec<String> {
    let mut out = Vec::new();
    let Some(obj) = meta.as_object() else {
        return out;
    };
    let Some(audio) = field(obj, "audio_tracks").as_object() else {
        return out;
    };
    if audio.is_empty() {
        return out;
    }
    let dl = field(obj, "default_locale");
    let dl_str = dl.as_str().unwrap_or("");
    if !audio.contains_key(dl_str) {
        let mut keys: Vec<&str> = audio.keys().map(String::as_str).collect();
        keys.sort_unstable();
        let listed = keys
            .iter()
            .map(|k| format!("'{k}'"))
            .collect::<Vec<_>>()
            .join(", ");
        out.push(format!(
            "manifest v4: default_locale={} отсутствует в audio_tracks=[{}]",
            py_repr(dl),
            listed
        ));
    }
    out
}

/// Check the structure of a v4 manifest. Returns `(ok, error_msg)`; when ok,
/// the message is empty — like Python's `validate_v4_manifest`.
pub fn validate_v4_manifest(meta: &Value) -> (bool, String) {
    let Value::Object(meta) = meta else {
        return (false, tr("manifest must be a JSON object (dict)"));
    };

    // format_version
    let fv = field(meta, "format_version");
    let fv_ok = fv.as_i64() == Some(V4_FORMAT_VERSION) || fv.as_f64() == Some(V4_FORMAT_VERSION as f64);
    if !fv_ok {
        return (false, trf("format_version=%1, expected %2", &[&py_repr(fv), &V4_FORMAT_VERSION]));
    }

    // type
    let t = field(meta, "type");
    if t.as_str() != Some(V4_TYPE) {
        return (false, trf("type=%1, expected '%2'", &[&py_repr(t), &V4_TYPE]));
    }

    // title
    if !is_nonempty_str(field(meta, "title")) {
        return (false, tr("title must be a non-empty string"));
    }

    // videos
    let videos = field(meta, "videos");
    let videos = match videos {
        Value::Object(m) if !m.is_empty() => m,
        _ => return (false, tr("videos must be a non-empty dictionary")),
    };
    for (res, path) in videos {
        if !ALLOWED_RESOLUTIONS.contains(&res.as_str()) {
            return (
                false,
                trf(
                    "videos: unsupported resolution %1 (allowed: %2)",
                    &[&py_repr(&Value::String(res.clone())), &ALLOWED_RESOLUTIONS_REPR],
                ),
            );
        }
        if !is_nonempty_str(path) {
            return (false, trf("videos[%1] must be a non-empty path string", &[&py_repr(&Value::String(res.clone()))]));
        }
    }

    // audio_tracks (optional): missing/None → an empty dictionary.
    let empty = Map::new();
    let audio = match meta.get("audio_tracks") {
        None | Some(Value::Null) => &empty,
        Some(Value::Object(m)) => m,
        Some(_) => return (false, tr("audio_tracks must be a dictionary (or be absent)")),
    };
    for (lang, path) in audio {
        if !is_locale_code(&Value::String(lang.clone())) {
            return (
                false,
                trf(
                    "audio_tracks: invalid language code %1 (expected ISO 639-1, lower case, 2 characters)",
                    &[&py_repr(&Value::String(lang.clone()))],
                ),
            );
        }
        if !is_nonempty_str(path) {
            return (false, trf("audio_tracks[%1] must be a non-empty path string", &[&py_repr(&Value::String(lang.clone()))]));
        }
    }

    // fallback_video
    if !is_nonempty_str(field(meta, "fallback_video")) {
        return (false, tr("fallback_video must be a non-empty path string"));
    }

    // default_locale
    let dl = field(meta, "default_locale");
    if !is_locale_code(dl) {
        return (false, trf("default_locale=%1, expected ISO 639-1 (lower case, 2 characters)", &[&py_repr(dl)]));
    }

    // Optional fields — type checks if they are present.
    if let Some(vol) = meta.get("audio_volume_default") {
        let ok = vol.as_i64().is_some_and(|n| (0..=100).contains(&n));
        if !ok {
            return (false, trf("audio_volume_default=%1, expected an int 0..100", &[&py_repr(vol)]));
        }
    }
    if let Some(l) = meta.get("loop") {
        if !l.is_boolean() {
            return (false, tr("loop must be a bool"));
        }
    }
    if let Some(tags) = meta.get("tags") {
        let ok = matches!(tags, Value::Array(a) if a.iter().all(Value::is_string));
        if !ok {
            return (false, tr("tags must be a list of strings"));
        }
    }

    (true, String::new())
}

/// Errors reading the manifest from the archive (the analogue of Python exceptions).
#[derive(Debug)]
pub enum ParseError {
    /// The archive does not exist (FileNotFoundError).
    NotFound(String),
    /// Corrupted archive / read error (BadZipFile, OSError).
    BadZip(String),
    /// There is no `manifest.json` inside (KeyError).
    NoManifest(String),
    /// Invalid JSON (JSONDecodeError).
    BadJson(String),
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::NotFound(m) | ParseError::BadZip(m) | ParseError::NoManifest(m) | ParseError::BadJson(m) => {
                write!(f, "{m}")
            }
        }
    }
}

impl std::error::Error for ParseError {}

/// Read `manifest.json` from an `.rxn` (ZIP) without unpacking — port of
/// `parse_v4_manifest`. Looks for `manifest.json` at the root (case-sensitive).
///
/// The bytes are decoded as UTF-8 with invalid sequences replaced (like
/// `data.decode("utf-8", errors="replace")` in Python).
pub fn parse_v4_manifest(zip_path: &Path) -> Result<Value, ParseError> {
    let text = read_manifest_text(zip_path)?;
    serde_json::from_str(&text).map_err(|e| ParseError::BadJson(e.to_string()))
}

/// Read the TEXT of `manifest.json` from an `.rxn` — everything
/// `parse_v4_manifest` does before `json.loads`: checking the file, opening the
/// ZIP, finding the entry at the root, decoding UTF-8 with replacement
/// (`errors="replace"`).
///
/// Kept separate from `parse_v4_manifest` for the sake of the former Python
/// bridge: JSON parsing stayed with Python, otherwise `json.JSONDecodeError`
/// with its `msg`/`pos` could not be reproduced.
pub fn read_manifest_text(zip_path: &Path) -> Result<String, ParseError> {
    if !zip_path.is_file() {
        return Err(ParseError::NotFound(trf("Archive not found: %1", &[&zip_path.display()])));
    }
    let file = std::fs::File::open(zip_path).map_err(|e| ParseError::BadZip(e.to_string()))?;
    // Texts from the `zip` crate ("invalid Zip archive: Could not find EOCD") reach
    // the buyer's window as is — the .rxn installer appends str(e) to its own
    // prefix. We translate them into zipfile's wording, otherwise the Rust path and
    // the Python fallback would say different things about the same archive.
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| ParseError::BadZip(zip_error_text(&e)))?;

    let name = zip_path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut entry = match archive.by_name("manifest.json") {
        Ok(e) => e,
        Err(_) => return Err(ParseError::NoManifest(trf("manifest.json is missing from the archive %1", &[&name]))),
    };
    let mut bytes = Vec::new();
    entry.read_to_end(&mut bytes).map_err(|e| {
        // A broken payload with an intact entry directory — "Bad CRC-32…".
        if e.kind() == std::io::ErrorKind::InvalidData && e.to_string().contains("Invalid checksum") {
            ParseError::BadZip("Bad CRC-32 for file 'manifest.json'".to_string())
        } else {
            ParseError::BadZip(e.to_string())
        }
    })?;

    // errors="replace": invalid sequences → U+FFFD.
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}
