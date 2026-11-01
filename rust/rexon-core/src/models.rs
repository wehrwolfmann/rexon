//! Модели данных каталога оформления — порт `rexon/appearance/models.py`.
//!
//! `ThemeItem`     — один элемент контента OCS (тема/иконки/курсор/…).
//! `ThemeCategory` — категория каталога (cat_id, путь установки, apply-команда).
//!
//! Порядок полей `ThemeItem` совпадает с объявлением dataclass — от него
//! зависит порядок ключей в `favorites.json` (`dataclasses.asdict`), поэтому
//! он load-bearing и сверяется побайтово (golden-тест `models_parity.rs`).

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Один элемент контента из OCS API.
///
/// Все поля с дефолтами (`#[serde(default)]`): при десериализации отсутствующие
/// заполняются как в Python-dataclass, а неизвестные ключи serde молча
/// отбрасывает — точный аналог `ThemeItem(**{k: v ... if k in _FIELDS})`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ThemeItem {
    /// content_id на store.kde.org
    pub id: String,
    pub name: String,
    pub description: String,
    /// перевод (argos, после MVP)
    pub description_translated: String,
    pub author: String,
    pub license: String,
    pub version: String,
    /// 0-100 (OCS score)
    pub rating: i64,
    pub downloads: i64,
    /// URL большого превью
    pub preview_url: String,
    /// ВСЕ скриншоты previewpic1..12 (галерея детального вида)
    pub preview_urls: Vec<String>,
    /// путь к скачанному превью (кеш)
    pub preview_local: String,
    /// URL для скачивания архива
    pub download_url: String,
    /// имя файла архива
    pub download_name: String,
    /// размер архива в КиБ — как отдаёт магазин (`downloadsize1`)
    pub download_size: i64,
    /// OCS typeid (cat_id)
    pub category_id: i64,
    /// дата создания (ISO)
    pub created: String,
    /// дата обновления (ISO)
    pub updated: String,
    /// установлен локально?
    pub installed: bool,
    /// Применён ли ПРЯМО СЕЙЧАС (совпадает с активной темой своего вида).
    /// Установка и применение — РАЗНЫЕ состояния: тему можно скачать кнопкой
    /// «Только скачать» и не включать.
    pub applied: bool,
    /// путь установки (если installed)
    pub install_path: String,
    /// Все скачиваемые файлы (для выбора при мульти-архивах)
    pub download_links: Vec<Value>,

    /// Рантайм-атрибут `_local_kind` из Python: не сериализуется (аналог
    /// подчёркнутого поля, которого нет в `dataclasses.fields`). Заполняется
    /// в `favorites::list_favorite_items` для локальных тем.
    #[serde(skip)]
    pub local_kind: Option<String>,

    /// Имя темы с диска БЕЗ перевода (`Name=` без языка): «High Contrast»
    /// у карточки «Высококонтрастная». Нужно только поиску — человек ищет и
    /// по тому, что видит, и по тому, как тему зовут в оригинале. Пусто,
    /// когда совпадает с показанным именем или темы на диске нет. В файлы не
    /// пишется (порядок полей `favorites.json` load-bearing), между потоками
    /// вкладки едет отдельным ключом `sourceName`.
    #[serde(skip)]
    pub source_name: String,
}

impl ThemeItem {
    /// Описание для показа: перевод если есть, иначе оригинал.
    pub fn display_description(&self) -> &str {
        if self.description_translated.is_empty() {
            &self.description
        } else {
            &self.description_translated
        }
    }

    /// Рейтинг 0-100 → 0.0-5.0 звёзд, как Python `round(rating / 20.0, 1)`.
    ///
    /// Python round округляет к ближайшему с привязкой к чётному по ИСТИННОМУ
    /// значению double (напр. rating=25 → 1.25 → 1.2, а rating=1 → 0.05-как-
    /// double чуть больше 0.05 → 0.1). Rust `format!("{:.1}", …)` использует ту
    /// же семантику (round-half-to-even на истинном значении), поэтому
    /// форматирование с обратным парсингом даёт байт-в-байт те же числа —
    /// сверено на всех 101 значениях (golden-тест).
    pub fn rating_stars(&self) -> f64 {
        format!("{:.1}", self.rating as f64 / 20.0)
            .parse()
            .expect("формат {:.1} всегда парсится обратно в f64")
    }
}

/// Категория каталога оформления.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThemeCategory {
    /// OCS cat_id (0 = виртуальная)
    pub id: i64,
    /// внутренний ключ (popular/new/global/…)
    pub key: String,
    /// человекочитаемое имя (RU)
    pub name: String,
    /// emoji для сайдбара
    pub icon: String,
    /// путь установки (~ раскрывается)
    pub install_dir: String,
    /// тип apply (global/plasma/colors/…)
    pub apply_kind: String,
    /// нужен ли pkexec (SDDM/Plymouth)
    pub requires_root: bool,
    /// Виртуальные категории сайдбара (popular/new/active/downloaded/system)
    /// не имеют своего cat_id и не ходят в сеть.
    ///
    /// В Python поле называется `virtual` (зарезервировано в Rust) — при
    /// сериализации переименовываем обратно, чтобы `asdict`-вывод совпадал
    /// побайтово (golden-тест `categories_parity.rs`).
    #[serde(rename = "virtual")]
    pub virtual_: bool,
    /// Откуда раздел берёт онлайн-каталог ([`CatalogSource`]). Номер
    /// раздела (`id`) об этом больше ничего не говорит: у раздела «Рамки
    /// окон (XFCE)» номер 906 (на нём держатся журнал и избранное), а
    /// каталог — категория магазина 138.
    pub store: CatalogSource,
    /// Для popular/new — сортировка по умолчанию: new|down|high|alpha
    pub default_sort: String,
}

impl ThemeCategory {
    /// Создать категорию с обязательными полями; остальные — дефолты
    /// dataclass (пустые пути/флаги, `default_sort = "new"`).
    pub fn new(id: i64, key: &str, name: &str, icon: &str) -> Self {
        Self {
            id,
            key: key.to_string(),
            name: name.to_string(),
            icon: icon.to_string(),
            install_dir: String::new(),
            apply_kind: String::new(),
            requires_root: false,
            virtual_: false,
            store: CatalogSource::Ocs(vec![id]),
            default_sort: "new".to_string(),
        }
    }

    /// Раздел без онлайн-каталога: показывает только то, что лежит на диске.
    pub fn local_only(&self) -> bool {
        self.store == CatalogSource::None
    }

    /// Номера категорий магазина OCS, из которых собран каталог раздела
    /// (несколько — «зонт»: OCS принимает «id1,id2»). Пусто — каталог не
    /// из OCS или его нет.
    pub fn ocs_cats(&self) -> &[i64] {
        match &self.store {
            CatalogSource::Ocs(cats) => cats,
            _ => &[],
        }
    }
}

/// Источник онлайн-каталога раздела «Оформления».
///
/// AUR — не источник раздела, а дополнительная полка ВНУТРИ
/// раздела с каталогом (переключатель «Магазин:»,
/// `appearance_page::source_keys_for`).
///
/// Cinnamon Spices есть у пяти разделов Cinnamon (`crate::cinnamon_spices`).
/// ⚠️ extensions.gnome.org и cosmic-themes.org пока только названы:
/// клиента у них в странице ещё нет, и ни один раздел их не получает.
/// Раздел с таким источником вкладка сочла бы сетевым — заводить его можно
/// только вместе с клиентом.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogSource {
    /// Каталога нет — только диск.
    None,
    /// Магазин OCS (KDE Store, pling): номера категорий.
    Ocs(Vec<i64>),
    /// Сборная витрина «Популярные»/«Новые»: ходовые категории OCS этого
    /// окружения (`categories::aggregate_cats`).
    OcsShowcase,
    /// Cinnamon Spices (`cinnamon-spices.linuxmint.com`): вид каталога —
    /// `themes`, `applets`, `desklets`, `extensions`, `actions`.
    Spices(String),
    /// extensions.gnome.org.
    GnomeExtensions,
    /// cosmic-themes.org.
    CosmicThemes,
}

/// Сериализация в JSON с отступом в 1 пробел — как Python
/// `json.dumps(obj, ensure_ascii=False, indent=1)` (формат `favorites.json`).
///
/// serde_json `to_string_pretty` жёстко зашивает 2 пробела, а стор избранного
/// пишется с `indent=1`, поэтому нужен `PrettyFormatter::with_indent(b" ")`.
/// Разделитель ключ-значение `": "` и элементов `,\n` у serde совпадают с
/// Python при заданном indent (кириллица не экранируется — `ensure_ascii=False`).
pub fn to_json_indent1(value: &Value) -> String {
    let mut buf = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(b" ");
    let mut ser = serde_json::Serializer::with_formatter(&mut buf, formatter);
    value
        .serialize(&mut ser)
        .expect("serde_json::Value всегда сериализуется");
    String::from_utf8(buf).expect("serde_json пишет валидный UTF-8")
}
