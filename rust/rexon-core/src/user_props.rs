//! Разбор `project.json` обоев Wallpaper Engine и пользовательские свойства
//! (`userProperties`) — порт `rexon/wallpapers/user_props.py`.
//!
//! Терпимый парсер: неизвестные поля уходят в `extra`, `type`/`mode`
//! приводятся к нижнему регистру, `workshopid` всегда становится строкой
//! (в файлах Мастерской он бывает и числом, и строкой), отсутствующие
//! значения заменяются разумными умолчаниями.
//!
//! Реальные пограничные случаи Мастерской WE, ради которых парсер такой:
//!   * `type` с заглавной буквы (`"Web"`, `"Video"`) → нижний регистр;
//!   * `workshopid: 859267594` числом → строка;
//!   * `preview` бывает `.gif`/`.png`/`.jpg` — читаем как есть;
//!   * property без `type` — это `label` (HTML/BBCode-врезка);
//!   * `color.value` — `"0.6 0.6 0.6"` (три float через пробел);
//!   * заголовки с иероглифами/кириллицей — обязателен UTF-8.
//!
//! Ядро чистое: [`parse_project`] работает над уже разобранным JSON,
//! [`parse_project_text`]/[`parse_project_bytes`] — над содержимым файла, и
//! только тонкая обёртка [`parse_project_json`] читает файл с диска (та же
//! граница «чистое ядро + обёртка над путём», что в `favorites.rs`).
//!
//! ⚠ правило #16 — починенные при порте дефекты оригинала. Модуль объявлен
//! терпимым, но на деле ронял исключение или выдавал мусор:
//!
//!   1. **`null` превращался в строку `"None"`.** `str(raw.get("preview"))`
//!      на `"preview": null` давал файл с именем `None`, `"type": null` —
//!      тип обоев `none`, `"workshopid": null` — идентификатор `"None"`.
//!      То же для `file`/`title`/`description`/`contentrating`/`visibility`
//!      и для `text`/`condition`/`mode`/`fileType`/`label` свойства. Здесь
//!      `null` = «поля нет» → умолчание.
//!   2. **`int(...)` на `order`/`index` ронял ВЕСЬ разбор.**
//!      `"order": "abc"` → `ValueError`, `"order": "3.5"` → `ValueError`,
//!      `"order": [1]` → `TypeError`; из-за одного кривого свойства обои
//!      оставались вообще без пользовательских настроек. Здесь перевод
//!      терпимый: нечисловое → `0`, дробное — усечение к нулю (как `int()`).
//!   3. **Не-объект вместо словаря ронял разбор `AttributeError`'ом.**
//!      `"general": [1,2]`, `"general": {"properties": [1,2]}` и
//!      `project.json` с массивом/строкой на верхнем уровне. Здесь: верхний
//!      уровень не-объект — явная ошибка [`ParseError::NotObject`], а
//!      `general`/`properties` не-объект — просто пусто.
//!   4. **`tags` перебирались посимвольно.** `"tags": "abc"` давало
//!      `["a","b","c"]`, `"tags": {"a":1,"b":2}` — `["a","b"]` (ключи).
//!      Здесь теги берутся только из массива, элементы `null`/массив/объект
//!      пропускаются (раньше давали тег `"None"`/`"[1, 2]"`).
//!   5. **`supportsaudioprocessing: "false"` включал звук.**
//!      `bool("false")` в Python — истина. Здесь строка разбирается по
//!      смыслу (`false`/`0`/`no`/`off`/пусто — ложь).
//!   6. **`version` вопреки сигнатуре `Optional[int]` возвращалось как
//!      есть** (`3.9`, `"3"`). Здесь — терпимый перевод в целое.
//!   7. **`localization` вопреки сигнатуре могло оказаться чем угодно**
//!      (`[1,2]` уезжало в поле типа «язык → ключ → строка»). Здесь
//!      собирается только из вложенных объектов.
//!   8. **Битый байт в файле ронял разбор `UnicodeDecodeError`'ом.**
//!      Файлы Мастерской встречаются и не в UTF-8; теперь декодирование
//!      lossy — мусорный байт заменяется, остальные поля читаются.
//!
//! Сохранённые квирки оригинала (поведение не меняется):
//!   * `min`/`max`/`step`/`precision`/`fraction` хранятся СЫРЫМИ значениями
//!     JSON (в WE они бывают и числом, и строкой) — типизированный доступ
//!     даёт [`UserProperty::min_num`] и соседи;
//!   * не-объект вместо самого свойства (`"p": "строка"`) остаётся
//!     свойством типа `label` со значением-строкой;
//!   * property типа `group`/`label` не попадает в [`Project::defaults_for_js`];
//!   * свойство без значения (или с пустой строкой) даёт `{}`, а не
//!     `{"value": ""}` — на это опирается подстановка картинки по умолчанию
//!     у веб-обоев.

use std::path::Path;

use serde::Serialize;
use serde_json::{Map, Value};

/// Вариант выпадающего списка (`type: combo`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ComboOption {
    pub label: String,
    /// WE допускает и строку, и число. `Value::Null` — в файле был `null`,
    /// `""` — поля `value` не было вовсе (умолчание оригинала).
    pub value: Value,
}

/// Одно `user_property` из `project.json::general.properties`.
///
/// `r#type`: `bool` | `color` | `slider` | `combo` | `textinput` |
/// `directory` | `file` | `group` | `label`. Формат `color.value` —
/// `"R G B"` через пробел, float в диапазоне `[0..1]`.
/// ⚠️ Имена полей здесь — ФОРМА наружу (`r#type` уходит как `type`): она
/// объявлена у самого типа, а не переписана веткой на каждое поле в мосту.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UserProperty {
    pub name: String,
    /// `label` — property без `type` (врезка HTML/BBCode).
    pub r#type: String,
    /// Значения не было (или оно `null`) — `None`.
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
    /// Для `type: directory`.
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
    /// `min` числом (число либо числовая строка), иначе `None`.
    pub fn min_num(&self) -> Option<f64> {
        as_f64(self.min.as_ref())
    }
    /// `max` числом.
    pub fn max_num(&self) -> Option<f64> {
        as_f64(self.max.as_ref())
    }
    /// `step` числом.
    pub fn step_num(&self) -> Option<f64> {
        as_f64(self.step.as_ref())
    }
    /// `precision` целым.
    pub fn precision_int(&self) -> Option<i64> {
        as_f64(self.precision.as_ref()).map(|v| v.trunc() as i64)
    }
    /// `fraction` булевым.
    pub fn fraction_bool(&self) -> Option<bool> {
        self.fraction.as_ref().map(truthy)
    }
}

/// Разобранный `project.json`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Project {
    /// `web` | `scene` | `video` | `application`.
    pub r#type: String,
    /// Точка входа, путь относительно каталога обоев.
    pub file: String,
    pub title: String,
    pub description: String,
    /// Умолчание — `preview.jpg`.
    pub preview: String,
    pub tags: Vec<String>,
    pub content_rating: String,
    /// Умолчание — `public`.
    pub visibility: String,
    /// ВСЕГДА строка (в файлах бывает числом).
    pub workshop_id: Option<String>,
    pub workshop_url: Option<String>,
    pub version: Option<i64>,
    /// Свойства в порядке из файла (в Python — обычный `dict`).
    pub properties: Vec<(String, UserProperty)>,
    pub supports_audio: bool,
    /// `general.localization`: язык → ключ → перевод, в порядке из файла.
    pub localization: Vec<(String, Vec<(String, String)>)>,
    pub extra: Map<String, Value>,
}

impl Project {
    /// Свойство по имени.
    pub fn property(&self, name: &str) -> Option<&UserProperty> {
        self.properties.iter().find(|(k, _)| k == name).map(|(_, p)| p)
    }

    /// Формат WE `{ключ: {value: X}}` для `applyUserProperties` в JS.
    ///
    /// Пропускает `type: group` и `type: label` (это только оформление UI).
    /// Property без значения даёт `{}` — важно для подстановки картинки по
    /// умолчанию у веб-обоев.
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

/// Почему `project.json` не разобрался.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// Файл не прочитался (`OSError` оригинала).
    Io(String),
    /// Содержимое — не JSON (`json.JSONDecodeError`).
    NotJson(String),
    /// JSON разобрался, но на верхнем уровне не объект. Оригинал падал здесь
    /// `AttributeError`'ом («'list' object has no attribute 'get'»).
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

/// Название вида значения JSON — для текста ошибки.
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

// ── терпимые переводы значений ───────────────────────────────────────────

/// Строковое поле: `null` и составные значения = «поля нет» (⚠ правило #16 —
/// оригинал печатал их через `str()` и получал `"None"` / `"[1, 2]"`).
/// Скаляры переводятся как в Python: `True`/`False`, число — как записано.
fn str_field(raw: Option<&Value>, default: &str) -> String {
    match raw {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Bool(true)) => "True".into(),
        Some(Value::Bool(false)) => "False".into(),
        Some(Value::Number(n)) => n.to_string(),
        _ => default.to_string(),
    }
}

/// То же, но «поля не было» отличается от умолчания: `None` вместо строки.
fn opt_str_field(raw: Option<&Value>) -> Option<String> {
    match raw {
        Some(Value::String(s)) => Some(s.clone()),
        Some(Value::Bool(true)) => Some("True".into()),
        Some(Value::Bool(false)) => Some("False".into()),
        Some(Value::Number(n)) => Some(n.to_string()),
        _ => None,
    }
}

/// Число из значения JSON: число либо числовая строка (с пробелами по краям).
fn as_f64(raw: Option<&Value>) -> Option<f64> {
    match raw {
        Some(Value::Number(n)) => n.as_f64(),
        Some(Value::String(s)) => s.trim().parse::<f64>().ok(),
        _ => None,
    }
}

/// Целое поле (`order`/`index`) — терпимо (⚠ правило #16: `int()` оригинала
/// ронял разбор на нечисловой строке и на списке). `true` → 1, `false`/`null`/
/// пусто/мусор → 0, дробное усекается к нулю (как `int()` в Python).
fn int_field(raw: Option<&Value>) -> i64 {
    match raw {
        Some(Value::Bool(b)) => i64::from(*b),
        Some(Value::Number(n)) => n.as_f64().map_or(0, |v| v.trunc() as i64),
        Some(Value::String(s)) => s.trim().parse::<f64>().map_or(0, |v| v.trunc() as i64),
        _ => 0,
    }
}

/// Целое поле, которого может не быть (`version`).
fn opt_int_field(raw: Option<&Value>) -> Option<i64> {
    match raw {
        Some(Value::Bool(b)) => Some(i64::from(*b)),
        Some(Value::Number(n)) => n.as_f64().map(|v| v.trunc() as i64),
        Some(Value::String(s)) => s.trim().parse::<f64>().ok().map(|v| v.trunc() as i64),
        _ => None,
    }
}

/// Истинность значения. Строка читается по смыслу (⚠ правило #16: у Python
/// `bool("false")` — истина, из-за чего `supportsaudioprocessing: "false"`
/// включал захват звука; ложь — `false`/`0`/`no`/`off`/`none`/пусто). Для
/// остальных видов — правила Python: пустой
/// контейнер/ноль/пустая строка — ложь.
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

/// Значение поля, где `null` неотличим от отсутствия (как `dict.get`).
fn present(raw: Option<&Value>) -> Option<Value> {
    match raw {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.clone()),
    }
}

// ── разбор ───────────────────────────────────────────────────────────────

/// Поля свойства, которые разобраны явно (остальные уходят в `extra`).
const PROPERTY_CONSUMED: &[&str] = &[
    "type", "value", "text", "order", "index", "condition", "min", "max", "step",
    "precision", "fraction", "options", "mode", "fileType",
];

/// Поля проекта, которые разобраны явно (остальные уходят в `extra`).
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

/// Разобрать одно property. Терпимо к неизвестным полям.
pub fn parse_user_property(name: &str, raw: &Value) -> UserProperty {
    let Value::Object(obj) = raw else {
        // Пограничный случай: иногда `properties[name]` — просто строка или
        // число. Такое становится врезкой-`label` с этим значением.
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
                    // `o.get("value", "")` — поля нет → пустая строка,
                    // явный `null` → `null`.
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

/// Разобрать уже прочитанный JSON `project.json`.
///
/// Второй элемент результата — предупреждения для журнала (сейчас одно:
/// отсутствие поля `type`, как `log.warning` оригинала).
pub fn parse_project(raw: &Value) -> Result<(Project, Vec<String>), ParseError> {
    let Value::Object(obj) = raw else {
        return Err(ParseError::NotObject(json_kind(raw)));
    };
    let mut warnings = Vec::new();

    // `general` не-объект — считаем, что его нет (⚠ правило #16: оригинал
    // падал `AttributeError`'ом на `"general": [1,2]`).
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

    // Теги — только из массива, скаляры переводятся в строки (⚠ правило #16:
    // оригинал перебирал строку посимвольно, а словарь — по ключам).
    let tags = match obj.get("tags") {
        Some(Value::Array(items)) => items.iter().filter_map(|t| opt_str_field(Some(t))).collect(),
        _ => Vec::new(),
    };

    // Локализация — только вложенные объекты со строковыми значениями.
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

/// Разобрать текст `project.json`.
pub fn parse_project_text(text: &str) -> Result<(Project, Vec<String>), ParseError> {
    let raw: Value =
        serde_json::from_str(text).map_err(|e| ParseError::NotJson(e.to_string()))?;
    parse_project(&raw)
}

/// Разобрать байты `project.json`: декодирование lossy (⚠ правило #16 —
/// оригинал ронял разбор `UnicodeDecodeError`'ом на файлах Мастерской не в
/// UTF-8).
pub fn parse_project_bytes(bytes: &[u8]) -> Result<(Project, Vec<String>), ParseError> {
    parse_project_text(&String::from_utf8_lossy(bytes))
}

/// Прочитать и разобрать `project.json` с диска — единственное место с
/// вводом-выводом.
pub fn parse_project_json(path: &Path) -> Result<(Project, Vec<String>), ParseError> {
    let bytes = std::fs::read(path).map_err(|e| ParseError::Io(e.to_string()))?;
    parse_project_bytes(&bytes)
}

// ── Определение типа обои по содержимому ────────────────────────────────
//
// Порт `rexon/wallpapers/factory.py::_infer_type`. Часть официальных обоев
// Wallpaper Engine (techno, audiophile и другие) поля `type` в `project.json`
// не содержит вовсе — сам WE в таком случае смотрит на входной файл и на то,
// что лежит рядом. Не повторив это, мы теряем такие обои: они не выпадают с
// ошибкой, а просто не применяются.
//
// Из фабрики сюда переехало только это. Всё остальное в ней — выбор класса
// движка на Python, и оно уходит вместе с Python.

/// Расширения входного файла, по которым обои считаются видео.
///
/// ⚠️ ДОБАВЛЕН `.ogv` (правило #16). Список видео-хоста на Python знал его с
/// самого начала — QtMultimedia такие файлы играет, — а этот не знал, и
/// обои с входным `.ogv` без поля `type` в `project.json` молча не
/// применялись. Теперь список ОДИН на весь продукт, копии на Python нет.
///
/// `.wmv`, `.mpg`, `.mpeg` (ролики Windows DreamScene) хост играет сам, без
/// перекодирования: проверено запуском на WMV3+WMA2 в ASF и на MPEG-1/2.
pub const VIDEO_ENTRY_EXTS: &[&str] = &[
    ".mp4", ".webm", ".mkv", ".avi", ".mov", ".m4v", ".ogv", ".wmv", ".mpg", ".mpeg", ".gif",
];

/// Расширения входного файла, по которым обои считаются веб-страницей.
///
/// ⚠️ ДОБАВЛЕНО (правило #16). Оригинал знал только `.html`, и вход `.htm`
/// не опознавался НИКАК: такие обои молча не применялись. Соседний разбор в
/// `wallpaper.rs` `.htm` при этом знает — расхождение внутри одного продукта.
pub const WEB_ENTRY_EXTS: &[&str] = &[".html", ".htm"];

/// Имя файла упакованной сцены.
pub const SCENE_PKG: &str = "scene.pkg";
/// Имя файла разложенной сцены.
pub const SCENE_JSON: &str = "scene.json";
/// Имя входной страницы, которую ищут, когда входной файл не назван.
pub const INDEX_HTML: &str = "index.html";

/// Что нужно от каталога обои, чтобы определить её тип.
pub trait ProjectDirIo {
    /// `pathlib.Path(path).is_file()`.
    fn is_file(&mut self, path: &str) -> bool;
    /// `path.read_text(encoding="utf-8")`. `None` — файл не открылся или в
    /// нём не UTF-8; оригинал глушил и то, и другое.
    fn read_text(&mut self, path: &str) -> Option<String>;
}

fn join(dir: &str, name: &str) -> String {
    format!("{}/{}", dir.trim_end_matches('/'), name)
}

/// Есть ли в файле разложенная сцена: объект с массивом `objects`.
/// Пустой массив тоже считается сценой — так было в оригинале.
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

/// Определить тип обои по содержимому каталога, когда `project.json::type`
/// пуст. Пустая строка — не подошло ничего.
///
/// Порядок проверок значим и сохранён дословно: видео, затем веб, затем
/// приложение, затем сцена. Из-за этого порядка вход `.exe` рядом с
/// `index.html` даёт `web`, а не `application` — так вёл себя оригинал.
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
    // Входной файл проверяется первым, `scene.json` — запасным. Имя берётся
    // НЕ приведённым к нижнему регистру: к нижнему приводился только хвост
    // для сравнения расширения.
    if entry.ends_with(".json") && is_scene_json(&join(wp_dir, &project.file), io) {
        return "scene";
    }
    if is_scene_json(&join(wp_dir, SCENE_JSON), io) {
        return "scene";
    }
    ""
}

/// Настоящий каталог на диске.
///
/// Оба метода глушат ошибки ровно там же, где их глушил оригинал: файла нет,
/// прав нет, содержимое не в UTF-8 — всё это «не подошло», а не отказ.
struct FsDirIo;

impl ProjectDirIo for FsDirIo {
    fn is_file(&mut self, path: &str) -> bool {
        Path::new(path).is_file()
    }

    fn read_text(&mut self, path: &str) -> Option<String> {
        std::fs::read_to_string(path).ok()
    }
}

/// [`infer_type`] по настоящему каталогу на диске.
///
/// Зовётся, когда `project.json::type` пуст. Пустая строка в ответе — не
/// подошло ничего; решать, что делать дальше, должен вызывающий.
///
/// ⚠️ Здесь ЕДИНСТВЕННОЕ место, где продукт определяет тип обои по
/// содержимому. Своих списков расширений заводить рядом нельзя: именно так
/// разъехались `VIDEO_ENTRY_EXTS` и список видео-хоста на Python.
pub fn infer_type_fs(wp_dir: &Path, project: &Project) -> &'static str {
    infer_type(&wp_dir.to_string_lossy(), project, &mut FsDirIo)
}
