//! Порт `rexon/ocs_store.py` — детект фактического типа KDE-темы и журнал
//! установленного.
//!
//! Живой установщик тем спрашивает у этого модуля две вещи: какой на самом
//! деле тип у распакованной темы (авторы на KDE Store сплошь и рядом грузят
//! Aurorae или Kvantum в категорию «Global Theme», и применение через
//! `plasma-apply-lookandfeel` тогда не работает) и что уже установлено —
//! чтобы карточка в режиме «только установленные» выглядела как в каталоге,
//! не обращаясь к сети.
//!
//! Обе задачи здесь чисто вычислительные: обход каталога и чтение файлов
//! делает исполнитель и подаёт наблюдения структурой, запись журнала —
//! тоже он.

use serde_json::{Map, Value};

// ============================================================================
// Детект фактического типа KDE-темы
// ============================================================================

/// Наблюдения о распакованном каталоге темы — ровно то, что оригинал
/// добывал вызовами `listdir`/`isdir`/`isfile`/чтением метаданных.
#[derive(Debug, Clone, Default)]
pub struct KdeContentScan {
    /// Существует ли сам каталог (`os.path.isdir(content_root)`).
    pub exists: bool,
    /// Содержимое каталога; пусто, если листинг не удался.
    pub files: Vec<String>,
    /// Те элементы `files`, что являются каталогами.
    pub subdirs: Vec<String>,
    /// Есть ли подкаталог `contents`.
    pub has_contents_dir: bool,
    /// Есть ли файл `contents/defaults`.
    pub has_contents_defaults: bool,
    /// Содержимое `contents`; пусто, если листинг не удался.
    pub contents_subdirs: Vec<String>,
    /// Текст `metadata.desktop`, если файл есть и читается.
    pub metadata_text: Option<String>,
}

/// Папки-признаки «полного пака» из нескольких компонентов KDE.
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

/// Файлы-признаки декорации окон Aurorae в корне темы.
const AURORAE_MARKERS: &[&str] = &[
    "decoration.svg",
    "decoration.svgz",
    "close.svg",
    "minimize.svg",
    "maximize.svg",
];

/// Подкаталоги, характерные для темы иконок.
const ICON_SUBDIRS: &[&str] = &[
    "apps",
    "places",
    "mimetypes",
    "actions",
    "categories",
    "devices",
    "status",
];

/// Подкаталоги `contents`, характерные для Plasma Style.
const PLASMA_STYLE_SUBDIRS: &[&str] =
    &["widgets", "dialogs", "frames", "icons", "translucent"];

/// Фактический тип распакованной KDE-темы либо `None`, если определить не
/// удалось.
///
/// ⚠️ ПОРЯДОК ПРОВЕРОК ЗНАЧИМ и повторяет оригинал: сначала «полный пак»
/// (две и более компонентных папки), потом однозначная цветовая схема,
/// затем Aurorae, Kvantum, иконки, и лишь в конце — `contents` и
/// `metadata.desktop`. Поменять порядок значит поменять решение на темах,
/// подходящих сразу под несколько признаков.
pub fn detect_actual_kde_type(scan: &KdeContentScan) -> Option<&'static str> {
    if !scan.exists {
        return None;
    }

    let has_subdir = |name: &str| scan.subdirs.iter().any(|s| s == name);
    let has_file = |name: &str| scan.files.iter().any(|f| f == name);

    // Полный пак: несколько компонентов KDE отдельными папками.
    if BUNDLE_MARKERS.iter().filter(|m| has_subdir(m)).count() >= 2 {
        return Some("bundle");
    }

    // Цветовая схема — однозначный признак.
    if scan.files.iter().any(|f| f.ends_with(".colors")) {
        return Some("color-scheme");
    }

    // Декорация окон Aurorae.
    if AURORAE_MARKERS.iter().any(|m| has_file(m)) {
        return Some("aurorae");
    }

    // Тема Kvantum.
    if scan.files.iter().any(|f| f.ends_with(".kvconfig")) {
        return Some("kvantum");
    }

    // Тема иконок: index.theme И хотя бы одна характерная подпапка. Без
    // подпапки это не иконки — index.theme носят и другие типы тем.
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

    // Последний рубеж — заявленный тип службы в метаданных.
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
// Журнал установленных тем
// ============================================================================

/// Путь журнала установленного.
pub fn install_log_path(home: &str) -> String {
    install_log_path_xdg(None, home)
}

/// То же с учётом `$XDG_CONFIG_HOME`.
pub fn install_log_path_xdg(xdg_config_home: Option<&str>, home: &str) -> String {
    format!(
        "{}/rexon/ocs_installed.json",
        crate::utils::config_home(xdg_config_home, home)
    )
}

/// Путь временного файла атомарной записи журнала.
pub fn install_log_tmp_path(log_path: &str) -> String {
    format!("{log_path}.tmp")
}

/// Разобрать журнал. Любая проблема — отсутствие файла, битый JSON, валидный
/// JSON не-объект на верхнем уровне — даёт пустой журнал: испорченный чужой
/// файл не должен ронять карточки установленного.
pub fn parse_installed_log(text: Option<&str>) -> Map<String, Value> {
    let Some(text) = text else {
        return Map::new();
    };
    match serde_json::from_str::<Value>(text) {
        Ok(Value::Object(map)) => map,
        _ => Map::new(),
    }
}

/// Поля карточки, которые журнал хранит вместе с записью. Оригинал читал их
/// с объекта по утиной типизации; здесь состав закреплён типом.
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

/// Запись журнала для установленной темы.
///
/// ⚠️ ПОРЯДОК КЛЮЧЕЙ значим: журнал пишется как JSON с отступами и
/// сравнивается побайтно, а Python сохраняет порядок вставки. Пустые пути
/// отфильтровываются — они попадали в список от неудавшихся шагов установки.
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

/// Добавить (или заменить) запись в журнале.
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

/// Убрать запись; `false` — записи не было (файл тогда не переписывается).
pub fn forget_installed(log: &mut Map<String, Value>, content_id: &str) -> bool {
    log.remove(content_id).is_some()
}

/// Текст файла журнала — как `json.dump(..., ensure_ascii=False, indent=2)`:
/// кириллица не экранируется, перевода строки в конце нет.
pub fn serialize_installed_log(log: &Map<String, Value>) -> String {
    serde_json::to_string_pretty(&Value::Object(log.clone()))
        .unwrap_or_else(|_| "{}".to_string())
}
