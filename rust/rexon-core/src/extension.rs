//! Класс `Extension` (одно расширение `.rxn`) — порт
//! `rexon/extensions/extension.py` (чистая часть).
//!
//! Портируется: разбор и валидация `manifest.json` (обязательные поля `id`/
//! `name`/`type`, проверка `type ∈ VALID_TYPES`), getters-свойства над
//! манифестом и сборка словаря `get_info()` для UI. Тексты ошибок
//! воспроизводятся байт-в-байт как `ValueError` Python, а `get_info()`
//! сверяется побайтово с `json.dumps(..., ensure_ascii=False, indent=1)`
//! (golden-тест `extension_parity.rs`).
//!
//! НЕ портируется (побочные эффекты, не ядро без Qt): `install.sh`/
//! `uninstall.sh` (`subprocess`), установка шрифтов/`.desktop`-записей
//! (`shutil`/`fc-cache`), `load_plugin`/`unload_plugin` (`importlib`),
//! `read_readme` (чтение файла для UI). Здесь — только разбор/представление.
//!
//! Отклонения от Python (осознанные):
//!   * `get_info` в Python зависит от файловой системы (`has_preview`/
//!     `preview_path` и т.п. проверяют файлы в каталоге расширения) — порт
//!     тоже смотрит на диск; в golden-тесте абсолютный префикс каталога
//!     (машинно-зависимый, одинаковый с обеих сторон) нормализуется в `DIR`,
//!     остальное сверяется байт-в-байт.
//!   * Если корень манифеста — не JSON-объект, Python `"id" not in manifest`
//!     для списка/скаляра даёт ошибку отсутствия поля `id`; здесь тот же
//!     результат. Патологический случай «манифест = JSON-строка» (Python
//!     сработал бы через подстроку) не воспроизводится — в реальных `.rxn`
//!     манифест всегда объект.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::constants::{
    self, is_supported_font, type_icon, type_label, wallpaper_format_icon,
    wallpaper_format_label,
};

/// Ошибка разбора манифеста (аналог `ValueError`/`FileNotFoundError` в
/// `Extension._load`). `Display` даёт тот же текст, что Python.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtensionError {
    /// `manifest.json` не найден в каталоге (`FileNotFoundError`).
    ManifestNotFound(String),
    /// `manifest.json` не является валидным JSON (`JSONDecodeError`).
    BadJson(String),
    /// Нет обязательного поля `id`/`name`/`type` (`ValueError`).
    MissingField(&'static str),
    /// Неизвестный `type` (`ValueError`).
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

/// Одно расширение Rexon (`.rxn`).
#[derive(Debug, Clone)]
pub struct Extension {
    /// Каталог расширения.
    pub dir: PathBuf,
    /// Разобранный `manifest.json` (объект).
    pub manifest: Map<String, Value>,
    /// Включено ли (по `enabled_state`).
    pub enabled: bool,
}

impl Extension {
    /// Собрать `Extension` из уже разобранного манифеста и каталога —
    /// чистое ядро `_load` (без чтения файла).
    ///
    /// `enabled_state` — карта `{id: bool}` (как `self.enabled = enabled_state
    /// .get(id, True)`; отсутствует → включено). Проверяет обязательные поля
    /// в порядке `id`, `name`, `type` и допустимость типа.
    pub fn from_manifest(
        manifest: Value,
        dir: impl Into<PathBuf>,
        enabled_state: &Map<String, Value>,
    ) -> Result<Self, ExtensionError> {
        // Корень манифеста должен быть объектом. Если нет — как `"id" not in
        // <не-dict>` в Python: обязательное поле id «отсутствует».
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

    /// Загрузить расширение из каталога: прочитать `manifest.json`, разобрать,
    /// провалидировать (порт `Extension.__init__`/`_load` целиком, с диском).
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

    // ── Свойства над манифестом ──────────────────────────────────────────

    /// `id` расширения (`manifest.get("id", "")`).
    pub fn ext_id(&self) -> &str {
        self.manifest.get("id").and_then(Value::as_str).unwrap_or("")
    }

    /// Тип расширения (`manifest.get("type", "plugin")`). После загрузки —
    /// всегда валидный тип из `VALID_TYPES`.
    pub fn ext_type(&self) -> &str {
        self.manifest
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("plugin")
    }

    /// Основной формат обоев для `wallpaper-pack` (иначе `None`) —
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

    /// Список обоев из манифеста (`manifest.get("wallpapers", [])`).
    fn wallpapers(&self) -> &[Value] {
        self.manifest
            .get("wallpapers")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// Фоновые звуки (`manifest.get("ambient_sounds", [])`).
    fn ambient_sounds(&self) -> &[Value] {
        self.manifest
            .get("ambient_sounds")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// Есть ли звук: непустой `ambient_sounds`, либо `has_audio` у любой обои,
    /// либо `manifest["has_audio"]` (порт `has_audio`, с Python-истинностью).
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

    /// Конфигурация сцены для формата `scene` (иначе `None`) —
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

    /// Названия семейств шрифтов (`manifest.get("font_families", [])`).
    fn font_families(&self) -> Value {
        self.manifest
            .get("font_families")
            .cloned()
            .unwrap_or_else(|| Value::Array(Vec::new()))
    }

    // ── Файлы (диск) ─────────────────────────────────────────────────────

    /// Путь к `preview.png`, если файл существует.
    pub fn preview_path(&self) -> Option<PathBuf> {
        file_if_exists(self.dir.join("preview.png"))
    }

    /// Путь к `icon.png`, если файл существует.
    pub fn icon_path(&self) -> Option<PathBuf> {
        file_if_exists(self.dir.join("icon.png"))
    }

    /// Путь к README (первый существующий из четырёх вариантов имени).
    pub fn readme_path(&self) -> Option<PathBuf> {
        for name in ["README.md", "readme.md", "README.txt", "readme.txt"] {
            if let Some(p) = file_if_exists(self.dir.join(name)) {
                return Some(p);
            }
        }
        None
    }

    /// Файлы шрифтов в `data/` (или в корне, если нет `data/`) для `font-pack`
    /// — рекурсивно, по расширению из `SUPPORTED_FONT_FORMATS`.
    ///
    /// Сопоставление формата — по суффиксу целиком (`is_supported_font`),
    /// поэтому составной `.pcf.gz` тоже матчится (см. починку бага там).
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

    // ── Info dict (для UI) ───────────────────────────────────────────────

    /// Словарь информации о расширении для UI — порт `get_info()`.
    ///
    /// Порядок ключей повторяет Python-dict (нужно для побайтовой сверки
    /// сериализации). Блоки для `wallpaper-pack` и `font-pack` добавляются в
    /// конце, как в оригинале.
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

    /// `manifest.get(key, default)` с сохранением JSON-типа значения.
    fn get_or(&self, key: &str, default: Value) -> Value {
        self.manifest.get(key).cloned().unwrap_or(default)
    }
}

// ── Хелперы ──────────────────────────────────────────────────────────────

/// `VALID_TYPES`, отсортированные лексикографически и склеенные `", "` — для
/// текста ошибки (как `', '.join(sorted(VALID_TYPES))`).
fn sorted_valid_types_joined() -> String {
    let mut types: Vec<&str> = constants::valid_types();
    types.sort_unstable();
    types.join(", ")
}

/// Истинность значения по правилам Python (`bool(v)`): пустые строка/массив/
/// объект и `0`/`false`/`null` — ложь, остальное — истина.
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

/// Путь, если это существующий файл (аналог `p if os.path.isfile(p) else None`).
fn file_if_exists(p: PathBuf) -> Option<PathBuf> {
    if p.is_file() {
        Some(p)
    } else {
        None
    }
}

/// Рекурсивно собрать файлы-шрифты (по суффиксу имени целиком) в `out`.
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

/// JSON-строка.
fn str_val(s: &str) -> Value {
    Value::String(s.to_string())
}

/// Пустой JSON-массив.
fn empty_array() -> Value {
    Value::Array(Vec::new())
}

/// Путь → JSON-строка, либо `null` (как `str | None` в Python).
fn path_val(p: Option<PathBuf>) -> Value {
    match p {
        Some(p) => Value::String(p.to_string_lossy().into_owned()),
        None => Value::Null,
    }
}
