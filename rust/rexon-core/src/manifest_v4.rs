//! Схема адаптивного видео-манифеста `.rxn` v4 — порт
//! `rexon/extensions/manifest_v4.py`.
//!
//! Валидация структуры `manifest.json` (`validate_v4_manifest`) и чтение
//! манифеста из архива без распаковки (`parse_v4_manifest`). Тексты ошибок
//! воспроизводятся байт-в-байт как у Python (включая `{x!r}`-repr), чтобы
//! сообщения в UI не поехали — сверено golden-тестом `manifest_v4_parity.rs`.
//!
//! Отклонения от Python (осознанные):
//!   * soft-warning «default_locale отсутствует в audio_tracks» в Python лишь
//!     логируется и не влияет на результат — здесь опущен (в ядре нет логгера).
//!   * `isinstance(x, int)` в Python принимает `bool` (True == 1). Здесь
//!     bool-значение в числовых полях не считается числом — в реальных
//!     манифестах эти поля числовые, край не встречается.
//!   * Python `repr` для float не воспроизводится (в манифестах эти поля —
//!     целые/строки); для int/str/None/bool repr совпадает.

use std::io::Read;
use std::path::Path;

use serde_json::{Map, Value};

use crate::ext_utils::zip_error_text;
use crate::i18n::{tr, trf};

/// `format_version` манифеста v4.
pub const V4_FORMAT_VERSION: i64 = 4;
/// `type` манифеста v4.
pub const V4_TYPE: &str = "wallpaper-video-adaptive";

/// Разрешения, поддерживаемые QML `pickVideo()` (см. main.qml).
/// Порядок для сообщений об ошибке — `sorted()` Python (лексикографический).
const ALLOWED_RESOLUTIONS: [&str; 4] = ["720p", "1080p", "1440p", "2160p"];
/// repr Python-списка `sorted(ALLOWED_RESOLUTIONS)` для текста ошибки.
const ALLOWED_RESOLUTIONS_REPR: &str = "['1080p', '1440p', '2160p', '720p']";

/// Python-`repr` значения для текстов ошибок (`{x!r}`). Покрывает типы,
/// встречающиеся в манифестах: None/bool/int/строка.
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
                // float — в манифестах не встречается; best-effort.
                n.to_string()
            }
        }
        Value::String(s) => format!("'{s}'"),
        // list/dict в сообщениях не используются — отладочный fallback.
        other => other.to_string(),
    }
}

/// `isinstance(v, str) and bool(v.strip())` — непустая строка (после strip).
fn is_nonempty_str(v: &Value) -> bool {
    matches!(v, Value::String(s) if !s.trim().is_empty())
}

/// `_is_locale_code`: ровно 2 символа, ASCII, lower-case, буквы (ISO 639-1).
fn is_locale_code(v: &Value) -> bool {
    let Value::String(s) = v else { return false };
    s.chars().count() == 2 && s.chars().all(|c| c.is_ascii_lowercase())
}

/// Значение поля (отсутствует → `Null`, как `dict.get` без дефолта → None).
fn field<'a>(meta: &'a Map<String, Value>, key: &str) -> &'a Value {
    meta.get(key).unwrap_or(&Value::Null)
}

/// Мягкие предупреждения манифеста v4 — то, что НЕ блокирует установку, но
/// автору набора стоит знать. Логгера у ядра нет, поэтому строки просто
/// возвращаются: пишет их в журнал вызывающий (шим `manifest_v4.py`).
///
/// Сейчас предупреждение одно: `default_locale` не имеет своего трека в
/// непустом `audio_tracks` (QML сделает fallback, но звук будет не тот).
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

/// Проверить структуру manifest v4. Возвращает `(ok, error_msg)`; при ok
/// сообщение пустое — как Python `validate_v4_manifest`.
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

    // audio_tracks (optional): отсутствует/None → пустой словарь.
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

    // Optional поля — проверка типов, если присутствуют.
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

/// Ошибки чтения манифеста из архива (аналог исключений Python).
#[derive(Debug)]
pub enum ParseError {
    /// Архив не существует (FileNotFoundError).
    NotFound(String),
    /// Повреждённый архив / ошибка чтения (BadZipFile, OSError).
    BadZip(String),
    /// Внутри нет `manifest.json` (KeyError).
    NoManifest(String),
    /// Невалидный JSON (JSONDecodeError).
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

/// Прочитать `manifest.json` из `.rxn` (ZIP) без распаковки — порт
/// `parse_v4_manifest`. Ищет `manifest.json` в корне (case-sensitive).
///
/// Байты декодируются UTF-8 с заменой некорректных (как
/// `data.decode("utf-8", errors="replace")` в Python).
pub fn parse_v4_manifest(zip_path: &Path) -> Result<Value, ParseError> {
    let text = read_manifest_text(zip_path)?;
    serde_json::from_str(&text).map_err(|e| ParseError::BadJson(e.to_string()))
}

/// Прочитать ТЕКСТ `manifest.json` из `.rxn` — всё, что делает
/// `parse_v4_manifest` до `json.loads`: проверка файла, открытие ZIP, поиск
/// записи в корне, декодирование UTF-8 с заменой (`errors="replace"`).
///
/// Отдельно от `parse_v4_manifest` ради живого моста: разбор JSON остаётся за
/// Python, иначе не воспроизвести `json.JSONDecodeError` с его `msg`/`pos` —
/// а вызывающий (`rxn_v4_installer`) ловит именно этот тип.
pub fn read_manifest_text(zip_path: &Path) -> Result<String, ParseError> {
    if !zip_path.is_file() {
        return Err(ParseError::NotFound(trf("Archive not found: %1", &[&zip_path.display()])));
    }
    let file = std::fs::File::open(zip_path).map_err(|e| ParseError::BadZip(e.to_string()))?;
    // Тексты крейта `zip` («invalid Zip archive: Could not find EOCD») доезжают
    // до окна покупателя как есть — установщик .rxn подклеивает str(e) к своему
    // префиксу. Переводим их в слова zipfile, иначе Rust-путь и Python-фолбэк
    // говорят про один и тот же архив разное.
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| ParseError::BadZip(zip_error_text(&e)))?;

    let name = zip_path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut entry = match archive.by_name("manifest.json") {
        Ok(e) => e,
        Err(_) => return Err(ParseError::NoManifest(trf("manifest.json is missing from the archive %1", &[&name]))),
    };
    let mut bytes = Vec::new();
    entry.read_to_end(&mut bytes).map_err(|e| {
        // Битая полезная нагрузка при живом каталоге записей — «Bad CRC-32…».
        if e.kind() == std::io::ErrorKind::InvalidData && e.to_string().contains("Invalid checksum") {
            ParseError::BadZip("Bad CRC-32 for file 'manifest.json'".to_string())
        } else {
            ParseError::BadZip(e.to_string())
        }
    })?;

    // errors="replace": невалидные последовательности → U+FFFD.
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}
