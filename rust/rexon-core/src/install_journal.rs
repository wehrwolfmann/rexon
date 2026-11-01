//! Общий журнал установки: что положил план и что снимать при удалении.
//!
//! ЗАЧЕМ. До шага 3 слияния установщиков у Rexon было два журнала, и ни один
//! не говорил «что лежит на диске от этой установки»:
//!   * `.rxn` — `extensions_state.json` (только «включено ли») и копия пакета
//!     в каталоге расширений; что снимать, удаление выводило заново из плана
//!     по копии пакета (`ext_exec::uninstall`);
//!   * магазин «Оформления» — `ocs_installed.json`: карточка товара и голый
//!     список путей без вида и владельца (`ocs_store`,
//!     `appearance_installer::uninstall_from_log`).
//!
//! Здесь одна запись на установленный элемент ([`Entry`]): откуда он
//! ([`Source`]), положенные пути с видом и меткой владельца ([`Placed`]),
//! раздел, включён ли, карточка магазина целиком и — заделом под `.rxn` v5 —
//! элементы пакета с выбором нагрузки ([`Element`], [`ElementChoice`]).
//!
//! ПЕРЕХОДНЫЙ ПОРЯДОК (переезд на общий журнал `.rxn` v5). Шаги 3–4: старые
//! файлы были источником для своих читателей, общий журнал писался рядом
//! (двойная запись). Шаг 5б: источник правды — общий журнал. Писатели
//! (покупка магазина, «Удалить» на плитке, подъём вложенных тем, спайсы
//! Cinnamon, установка, удаление и переключение `.rxn`) пишут ТОЛЬКО его
//! ([`put_store_log`], [`record_rxn_at`], [`set_rxn_enabled_at`]), а старые
//! `ocs_installed.json` и `extensions_state.json` получают его зеркало
//! одной функцией (`mirror_legacy`) — тем же текстом, что писала прежняя
//! логика: вернувшийся на прежнюю версию Rexon (она читает только старые
//! файлы) не теряет ни одной установки. Старые файлы не удаляются.
//!
//! Читаются старые файлы только для переноса ([`reconcile`]): у установок,
//! сделанных до журнала, и у правок, сделанных прежней версией Rexon после
//! отката. Как это узнаётся — [`LegacySeen`]: журнал помнит отпечаток
//! каждого старого файла, каким его видел; совпал — файл писали только мы.
//! Пакеты `.rxn` сверяются с каталогом расширений всегда (его читают и
//! карточки «Расширений»). Точные пути `.rxn` журнал узнаёт от самой
//! установки ([`placed_from_steps`]); у пакетов, поставленных до журнала,
//! их нет ([`Entry::placed_known`] = `false`) — такие снимаются прежним
//! путём, по плану.
//!
//! Удаление по журналу ([`removal_plan`], [`remove`]) общее для обоих
//! источников и снимает ТОЛЬКО записанное и только своё: каталог с меткой
//! владельца — если метка всё ещё наша; ссылка — если там всё ещё ссылка;
//! каталог, бывший на месте до установки, не снимается вовсе.
//!
//! Шаг 3г: этим путём снимают файлы все удаления — `.rxn`
//! (`ext_exec::uninstall`, [`rxn_removal`]: выключение и перезагрузка
//! оболочки остаются шагами его плана вокруг файлового снятия), покупка
//! магазина (`appearance_installer::uninstall_from_log`), её тема с плитки
//! (`theme_removal::commit`, [`part_removal`]) и спайс Cinnamon
//! (`cinnamon_spices::uninstall_with`, [`merge_into_plan`]).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::ext_manager::{ExtStep, OWNER_MARK};

/// Имя файла журнала в каталоге конфигов Rexon (`~/.config/rexon`).
pub const JOURNAL_NAME: &str = "install_journal.json";

/// Версия формата журнала.
pub const JOURNAL_FORMAT: u32 = 1;

/// Старые журналы рядом с новым (их имена — у их владельцев).
const STORE_LOG_NAME: &str = "ocs_installed.json";
const EXT_STATE_NAME: &str = "extensions_state.json";
const EXT_DIR_NAME: &str = "extensions";

// ============================================================================
// Модель
// ============================================================================

/// Журнал целиком.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Journal {
    pub format: u32,
    /// Ключ — [`Entry::key`]: `rxn:<id>` или `store:<content_id>`.
    #[serde(default)]
    pub items: BTreeMap<String, Entry>,
    /// Порядок покупок магазина (`content_id`) — как в `ocs_installed.json`:
    /// читатели получают карточки в прежнем порядке ([`store_log_of`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub store_order: Vec<String>,
    /// Отпечатки старых журналов, какими общий журнал видел их согласованными
    /// с собой ([`LegacySeen`]).
    #[serde(default, skip_serializing_if = "LegacySeen::is_empty")]
    pub legacy_seen: LegacySeen,
    /// Последнее «Применить» по разделам: где лежит снимок «что было»
    /// ([`Applied`], шаг 5а). Ключ — раздел ([`Applied::section`]).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub applied: BTreeMap<String, Applied>,
    /// Прежние значения стола (`gsettings`/`dconf`, `xfconf`), которые
    /// переписало последнее «Применить» раздела, — в порядке записи
    /// ([`AppliedValue`]). Ключ — раздел, как у [`Self::applied`]; живут и
    /// снимаются вместе с его записью. Отдельно от [`Applied`], чтобы старые
    /// журналы и записи без значений читались как прежде.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub applied_values: BTreeMap<String, Vec<AppliedValue>>,
}

impl Default for Journal {
    fn default() -> Self {
        Journal {
            format: JOURNAL_FORMAT,
            items: BTreeMap::new(),
            store_order: Vec::new(),
            legacy_seen: LegacySeen::default(),
            applied: BTreeMap::new(),
            applied_values: BTreeMap::new(),
        }
    }
}

/// Отпечатки ([`fingerprint`]) старых журналов — `ocs_installed.json` и
/// `extensions_state.json` — в том виде, в каком общий журнал последний раз
/// видел их согласованными с собой (шаг 5б).
///
/// Отпечаток совпал с файлом — с тех пор файл никто, кроме нас, не писал, и
/// источник правды — общий журнал. Не совпал (или отпечатка нет) — файл писала
/// прежняя версия Rexon (откат версии и возврат) либо общего журнала тогда ещё
/// не было: записи этой части переносятся из старого файла ([`reconcile`]).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegacySeen {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub store: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
}

impl LegacySeen {
    fn is_empty(&self) -> bool {
        self.store.is_none() && self.state.is_none()
    }
}

/// Кто поставил элемент.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Source {
    /// Пакет `.rxn` (вкладка «Расширения», импорт тем Windows).
    Rxn,
    /// Магазин «Оформления» (KDE Store, спайсы Cinnamon).
    Store,
    /// Образ «Как в дистрибутиве» (`distro_looks`): набор `.rxn` v5,
    /// применённый целиком. Старых журналов у него нет — запись живёт
    /// только здесь и при сверке переносится как есть.
    Look,
}

impl Source {
    fn prefix(self) -> &'static str {
        match self {
            Source::Rxn => "rxn",
            Source::Store => "store",
            Source::Look => "look",
        }
    }
}

/// Одна установка.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub source: Source,
    /// id пакета `.rxn` или `content_id` товара магазина.
    pub id: String,
    pub name: String,
    /// Вид по старому пути: тип пакета `.rxn` (`theme-icons`) или
    /// `rexon_type` магазина.
    pub kind: String,
    /// Раздел, которому элемент принадлежит: `appearance`/`extensions` у
    /// `.rxn`, `rexon_type` у магазина.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
    /// Включён ли (`None` — у источника такого понятия нет: магазин).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installed_at: Option<String>,
    /// Копия пакета `.rxn` в каталоге расширений.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package_copy: Option<String>,
    /// Известны ли положенные пути. `false` — запись сверена из старого
    /// журнала `.rxn`, где путей не было: снимать прежним путём, по плану.
    #[serde(default = "yes")]
    pub placed_known: bool,
    /// Что положено, в порядке установки (снимается в обратном).
    #[serde(default)]
    pub placed: Vec<Placed>,
    /// Элементы пакета `.rxn` v5 (пусто у прежних форматов).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub elements: Vec<Element>,
    /// Карточка товара магазина — запись `ocs_installed.json` как есть
    /// (ничего не теряется при сверке).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub card: Option<Map<String, Value>>,
}

fn yes() -> bool {
    true
}

impl Entry {
    pub fn key(&self) -> String {
        key_of(self.source, &self.id)
    }
}

/// Ключ записи.
pub fn key_of(source: Source, id: &str) -> String {
    format!("{}:{id}", source.prefix())
}

/// Один положенный путь.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Placed {
    pub path: String,
    /// Что положено. `None` — путь из старого журнала магазина, которого при
    /// сверке на диске не было: вид неизвестен, удаление смотрит по диску.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub what: Option<What>,
    /// Текст метки владельца ([`OWNER_MARK`]) в корне каталога, если она
    /// есть: каталог снимается, только пока метка та же.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    /// Каталог уже был на месте до установки (план сложил файлы в чужой
    /// каталог): удаление его не трогает.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pre_existed: bool,
    /// Путь записи, которому этот служит: ссылка `~/.themes/<тема>` — теме
    /// в `~/.local/share/themes`. Снимается вместе с ним (и «Удалить» на
    /// плитке темы — тоже, [`part_removal`]); сверка пересобирает такие
    /// пути заново.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub of: Option<String>,
}

/// Вид положенного пути.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum What {
    File,
    Dir,
    Link,
    /// Каталог, заведённый планом по пути (база вроде
    /// `~/.local/share/fonts/rexon-<id>`): снимается, только если пуст.
    CreatedDir,
}

/// «Применить» в разделе (шаг 5а слияния установщиков): что нажатие
/// переписало и где лежит снимок того, что было до него.
///
/// Записывается после УСПЕШНОГО нажатия, исполненного общим планом
/// (`appearance_installer::LiveInstaller::apply`): откат нажатия
/// (`install_plan::Undo`) не стирает отложенные копии, а переносит их в
/// снимок ([`applied_snapshot_dir`]). Повторное нажатие в том же разделе
/// дописывает только новые пути — у прежних остаётся самое раннее «что
/// было». Вернуть — [`revert_applied_at`].
///
/// Курсор (и смена его размера) и цвета вне Plasma пишутся сюда тем же
/// путём. У цветов при этом остаётся и своя памятка слоя
/// (`shell_palette::read_layer_memo`, возврат `revert_scheme_layer`): сама
/// памятка — первый путь снимка, поэтому возврат по `applied` возвращает
/// её вместе с файлами, а возврат по памятке оставляет запись `applied`,
/// которая потом возвращает то же самое (сверка — `rxn_live_roundtrip_parity`).
///
/// Значения стола, которые хранит не файл, а служба (`gsettings`/`dconf`,
/// `xfconf-query`), снимаются до команды нажатия
/// (`install_plan::Undo::keeping_values`) и лежат рядом с записью —
/// [`Journal::applied_values`]; возврат их тоже возвращает.
///
/// Задел: возвраты, у которых путь свой (`shell_palette::revert_flow` —
/// палитра шелла, копии `.rexon-bak` анимаций и конфига композитора,
/// снимки `backup`), сюда пока не сведены. Файл KDE, который пишет
/// команда `kwriteconfig6`, снимается копией до неё — как прочие файлы.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Applied {
    /// Раздел «Оформления» (ключ категории: `gtk`, `icons`…).
    pub section: String,
    /// Применённая тема (последнее нажатие).
    pub theme: String,
    /// Запись общего журнала, которой тема принадлежит (`store:<id>`), если
    /// она там есть.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub applied_at: Option<String>,
    /// Каталог снимка «что было».
    pub snapshot: String,
    /// Тронутые пути в порядке записи (возвращаются в обратном).
    #[serde(default)]
    pub files: Vec<AppliedFile>,
}

/// Путь, который переписало нажатие «Применить».
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppliedFile {
    pub path: String,
    /// Копия прежнего содержимого в снимке. `None` — до нажатия пути не
    /// было (возврат его снимает).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<String>,
    /// Каталог, заведённый нажатием (снимается, только если пуст).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub dir: bool,
}

/// Прежнее значение ключа стола, которое переписало «Применить»
/// ([`Journal::applied_values`]): какой ключ и команда, которая вернёт
/// прежнее (ключ не был задан — сброс: `gsettings reset`, `dconf reset`,
/// `xfconf-query … -r`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppliedValue {
    /// `gsettings <схема> <ключ>`, `dconf <путь>`, `xfconf <канал> <свойство>`.
    pub key: String,
    pub restore: Vec<String>,
}

/// Невернувшееся из [`revert_applied_with`] — ключ настройки стола
/// ([`AppliedValue::key`]), а не путь файла.
pub fn is_value_key(failed: &str) -> bool {
    !failed.starts_with('/')
}

/// Ключ настройки стола для человека — без имени программы хранилища
/// (`gsettings org.a k` → `org.a k`).
pub fn value_key_label(key: &str) -> &str {
    key.split_once(' ').map_or(key, |(_, rest)| rest)
}

impl From<crate::install_plan::ValueBefore> for AppliedValue {
    fn from(v: crate::install_plan::ValueBefore) -> Self {
        AppliedValue { key: v.key, restore: v.restore }
    }
}

/// Каталог снимка «что было» раздела `section` в каталоге конфигов Rexon.
pub fn applied_snapshot_dir(rexon_dir: &Path, section: &str) -> PathBuf {
    let safe: String = section
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    rexon_dir.join("applied").join(if safe.is_empty() { "_".to_string() } else { safe })
}

/// Элемент пакета `.rxn` v5 (`rxn_manifest::Item`) — что именно встало.
/// Модель на будущее: установку v5 Rexon ещё не исполняет.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Element {
    /// Абстрактный вид (`window-decoration`, `icons`…).
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// Потребитель, для которого положено (`aurorae`, `xfwm4`, `gtk`…).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consumer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chosen: Option<ElementChoice>,
    /// Пути из [`Entry::placed`], которые положил этот элемент.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<String>,
}

/// Какой вариант элемента поставлен (`rxn_manifest::Choice` без отказов).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "choice", rename_all = "kebab-case")]
pub enum ElementChoice {
    Native,
    Compatible { from: String },
    Generate { model: String, accuracy: String },
    Substitute { role: String },
}

impl ElementChoice {
    /// Выбор из правил `.rxn` v5 и его потребитель; отказ и «выключено»
    /// ничего не ставят — записи нет.
    pub fn from_choice(choice: &crate::rxn_manifest::Choice) -> Option<(String, ElementChoice)> {
        use crate::rxn_manifest::{Accuracy, Choice};
        match choice {
            Choice::Native { consumer, .. } => Some((consumer.clone(), ElementChoice::Native)),
            Choice::Compatible { consumer, from, .. } => {
                Some((consumer.clone(), ElementChoice::Compatible { from: from.clone() }))
            }
            Choice::Generate { consumer, model, accuracy } => Some((
                consumer.clone(),
                ElementChoice::Generate {
                    model: model.clone(),
                    accuracy: match accuracy {
                        Accuracy::Exact => "exact",
                        Accuracy::Approximate => "approximate",
                    }
                    .to_string(),
                },
            )),
            Choice::Substitute { consumer, role, .. } => {
                Some((consumer.clone(), ElementChoice::Substitute { role: role.clone() }))
            }
            Choice::Refuse { .. } | Choice::Off => None,
        }
    }
}

// ============================================================================
// Текст файла
// ============================================================================

/// Разобрать журнал. `None` — нет текста или он не журнал (битый JSON,
/// чужая версия формата).
pub fn parse(text: Option<&str>) -> Option<Journal> {
    let j: Journal = serde_json::from_str(text?).ok()?;
    (j.format == JOURNAL_FORMAT).then_some(j)
}

/// Текст файла: JSON с отступами и переводом строки в конце.
pub fn serialize(journal: &Journal) -> String {
    let mut text = serde_json::to_string_pretty(journal).expect("журнал сериализуется");
    text.push('\n');
    text
}

// ============================================================================
// Что на диске
// ============================================================================

/// Что лежит по пути (ссылка не разыменовывается).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Probe {
    Missing,
    File,
    Link,
    /// Каталог и текст его метки владельца, если она есть.
    Dir { owner: Option<String> },
}

/// Живая проба пути.
pub fn probe_live(path: &str) -> Probe {
    let p = Path::new(path);
    match std::fs::symlink_metadata(p) {
        Err(_) => Probe::Missing,
        Ok(m) if m.file_type().is_symlink() => Probe::Link,
        Ok(m) if m.is_dir() => Probe::Dir {
            owner: std::fs::read_to_string(p.join(OWNER_MARK)).ok().map(|t| t.trim().to_string()),
        },
        Ok(_) => Probe::File,
    }
}

impl Probe {
    fn exists(&self) -> bool {
        !matches!(self, Probe::Missing)
    }
}

// ============================================================================
// Сверка «старые журналы → новый»
// ============================================================================

/// Пакет `.rxn`, лежащий в каталоге расширений.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyExt {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub enabled: bool,
    pub dir: String,
}

/// Старые журналы: текст `ocs_installed.json` и установленные пакеты `.rxn`
/// (каталог расширений, прочитанный с картой `extensions_state.json`).
#[derive(Debug, Clone, Default)]
pub struct Legacy {
    pub store_log: Option<String>,
    pub extensions: Vec<LegacyExt>,
    /// Наши ссылки `~/.themes/<имя>` на темы магазина из
    /// `~/.local/share/themes` (их ставит `installed_scan::sync_theme_links`):
    /// путь темы → путь ссылки ([`theme_links_of`]). Пишутся в запись покупки
    /// видом «ссылка» сразу после самой темы — удаление по журналу снимет и
    /// её.
    pub theme_links: BTreeMap<String, String>,
}

/// Наши ссылки `~/.themes` на темы из записей журнала магазина `store`:
/// путь темы в `~/.local/share/themes` → путь ссылки. Ссылка «наша», только
/// если ведёт ровно туда, куда её ставит `sync_theme_links`
/// (`installed_scan::is_theme_link`): занятое человеком имя не попадает.
pub fn theme_links_of(store: &Map<String, Value>) -> BTreeMap<String, String> {
    theme_links_of_paths(
        store
            .values()
            .filter_map(|r| r.get("paths")?.as_array())
            .flatten()
            .filter_map(Value::as_str),
    )
}

/// То же у записей `.rxn` журнала `journal`: их каталоги, легшие в
/// `~/.local/share/themes` (на 25.10.2026 ни один тип `.rxn` туда не кладёт —
/// тема GTK ложится в `~/.themes`, — но ссылку на такую тему
/// `sync_theme_links` поставит, и снять её обязано удаление пакета).
pub fn rxn_theme_links_of(journal: &Journal) -> BTreeMap<String, String> {
    theme_links_of_paths(
        journal
            .items
            .values()
            .filter(|e| e.source == Source::Rxn)
            .flat_map(|e| e.placed.iter())
            .filter(|p| p.what == Some(What::Dir) && p.of.is_none() && !p.pre_existed)
            .map(|p| p.path.as_str()),
    )
}

/// Наши ссылки `~/.themes` на темы из путей `paths` (см. [`theme_links_of`]).
pub fn theme_links_of_paths<'a>(paths: impl Iterator<Item = &'a str>) -> BTreeMap<String, String> {
    const XDG_THEMES: &str = "/.local/share/themes";
    let mut out = BTreeMap::new();
    for path in paths {
        let Some((dir, name)) = path.trim_end_matches('/').rsplit_once('/') else { continue };
        let Some(home) = dir.strip_suffix(XDG_THEMES) else { continue };
        if home.is_empty() || name.is_empty() || !crate::installed_scan::is_theme_link(home, name) {
            continue;
        }
        let link = format!("{}/{name}", crate::installed_scan::legacy_themes_dir(home));
        out.insert(path.to_string(), link);
    }
    out
}

fn section_of_rxn(kind: &str) -> Option<String> {
    if crate::constants::is_appearance(kind) {
        Some("appearance".into())
    } else if crate::constants::is_extension(kind) {
        Some("extensions".into())
    } else {
        None
    }
}

fn placed_of_probe(path: &str, probe: &Probe) -> Placed {
    let (what, owner) = match probe {
        Probe::Missing => (None, None),
        Probe::File => (Some(What::File), None),
        Probe::Link => (Some(What::Link), None),
        Probe::Dir { owner } => (Some(What::Dir), owner.clone()),
    };
    Placed { path: path.to_string(), what, owner, pre_existed: false, of: None }
}

/// Ссылка `~/.themes` на тему `theme` (если она наша, [`Legacy::theme_links`])
/// — в `placed` сразу вслед за темой: удаление снимает её раньше.
fn push_theme_link(placed: &mut Vec<Placed>, theme: &str, legacy: &Legacy) {
    if let Some(link) = legacy.theme_links.get(theme) {
        if !placed.iter().any(|q| &q.path == link) {
            placed.push(Placed {
                path: link.clone(),
                what: Some(What::Link),
                owner: None,
                pre_existed: false,
                of: Some(theme.to_string()),
            });
        }
    }
}

/// Сверить журнал со старыми: ничего из старых не теряется, ушедшее из
/// старых уходит и отсюда. `journal` — прочитанный новый журнал (`None` —
/// его ещё нет: первое чтение).
///
/// * Магазин: записи пересобираются из `ocs_installed.json` целиком
///   (карточка — как есть, пути — с видом и меткой по диску); элементы v5
///   прежней записи сохраняются.
/// * `.rxn`: имя, вид, «включено» и копия пакета — из каталога расширений;
///   положенные пути прежней записи сохраняются, у новой их нет
///   (`placed_known = false`).
pub fn reconcile(journal: Option<Journal>, legacy: &Legacy, probe: &dyn Fn(&str) -> Probe) -> Journal {
    let old = journal.unwrap_or_default();
    let mut items: BTreeMap<String, Entry> = BTreeMap::new();

    let mut store_order = Vec::new();
    let store = crate::ocs_store::parse_installed_log(legacy.store_log.as_deref());
    for (cid, rec) in &store {
        let Some(card) = rec.as_object() else { continue };
        let key = key_of(Source::Store, cid);
        let kind = card.get("rexon_type").and_then(Value::as_str).unwrap_or_default().to_string();
        let mut placed: Vec<Placed> = Vec::new();
        for p in card.get("paths").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
            if p.is_empty() {
                continue;
            }
            placed.push(placed_of_probe(p, &probe(p)));
            push_theme_link(&mut placed, p, legacy);
        }
        store_order.push(cid.clone());
        let elements = old.items.get(&key).map(|e| e.elements.clone()).unwrap_or_default();
        items.insert(
            key,
            Entry {
                source: Source::Store,
                id: cid.clone(),
                name: card.get("name").and_then(Value::as_str).unwrap_or(cid).to_string(),
                section: (!kind.is_empty()).then(|| kind.clone()),
                kind,
                enabled: None,
                installed_at: card.get("installed_at").and_then(Value::as_str).map(str::to_string),
                package_copy: None,
                placed_known: true,
                placed,
                elements,
                card: Some(card.clone()),
            },
        );
    }

    for ext in &legacy.extensions {
        let key = key_of(Source::Rxn, &ext.id);
        let mut entry = match old.items.get(&key) {
            Some(prev) if prev.source == Source::Rxn => prev.clone(),
            _ => Entry {
                source: Source::Rxn,
                id: ext.id.clone(),
                name: String::new(),
                kind: String::new(),
                section: None,
                enabled: None,
                installed_at: None,
                package_copy: None,
                placed_known: false,
                placed: Vec::new(),
                elements: Vec::new(),
                card: None,
            },
        };
        entry.name = ext.name.clone();
        entry.kind = ext.kind.clone();
        entry.section = section_of_rxn(&ext.kind);
        entry.enabled = Some(ext.enabled);
        entry.package_copy = Some(ext.dir.clone());
        // Ссылки `~/.themes` на темы пакета — заново по диску, как у магазина.
        let own: Vec<Placed> = entry.placed.drain(..).filter(|p| p.of.is_none()).collect();
        for p in own {
            let theme = (p.what == Some(What::Dir) && !p.pre_existed).then(|| p.path.clone());
            entry.placed.push(p);
            if let Some(theme) = theme {
                push_theme_link(&mut entry.placed, &theme, legacy);
            }
        }
        items.insert(key, entry);
    }

    // Образы старых журналов не имеют — переносятся как есть.
    for (key, entry) in &old.items {
        if entry.source == Source::Look {
            items.insert(key.clone(), entry.clone());
        }
    }

    // «Применить» к установкам отношения не имеет — переносится как есть.
    Journal {
        format: JOURNAL_FORMAT,
        items,
        store_order,
        legacy_seen: old.legacy_seen,
        applied: old.applied,
        applied_values: old.applied_values,
    }
}

/// Журнал магазина глазами общего журнала: `content_id` → карточка покупки
/// (запись `ocs_installed.json` как есть), в порядке [`Journal::store_order`].
/// Так читатели установленного (карточки, «установлено ли», решения по
/// удалению) берут данные из общего журнала, а видят то же, что раньше.
pub fn store_log_of(journal: &Journal) -> Map<String, Value> {
    let mut out = Map::new();
    let ordered = journal.store_order.iter().map(|cid| key_of(Source::Store, cid));
    let rest = journal.items.keys().cloned();
    for key in ordered.chain(rest) {
        let Some(e) = journal.items.get(&key) else { continue };
        if e.source != Source::Store || out.contains_key(&e.id) {
            continue;
        }
        if let Some(card) = &e.card {
            out.insert(e.id.clone(), Value::Object(card.clone()));
        }
    }
    out
}

// ============================================================================
// Что положил план
// ============================================================================

/// Положенные пути по файловому плану установки — до его исполнения.
///
/// `existed` — был ли путь на диске ДО плана. Учитываются шаги, кладущие
/// что-то в профиль: `CopyTree`/`ExtractZip` (каталог), `CopyFile`
/// (файл), `Symlink` (ссылка), `WriteFile` (файл; метка владельца —
/// владелец каталога, а не отдельный путь), `MakeDirs` и недостающие
/// родители (заведённые каталоги — снимаются, только если пусты). Раскладка
/// под `/tmp` (`Stage`), команды и заметки ничего в профиле не оставляют.
/// Снятое планом раньше (`RemoveTree` своей прежней копии) считается
/// отсутствующим.
pub fn placed_from_steps(steps: &[ExtStep], existed: &dyn Fn(&str) -> bool) -> Vec<Placed> {
    let mut out: Vec<Placed> = Vec::new();
    // Состояние пути по ходу плана поверх `existed`.
    let mut now: BTreeMap<String, bool> = BTreeMap::new();
    let present = |now: &BTreeMap<String, bool>, p: &str| -> bool {
        if let Some(v) = now.get(p) {
            return *v;
        }
        let mut cur = Path::new(p).parent();
        while let Some(a) = cur {
            if now.get(a.to_string_lossy().as_ref()) == Some(&false) {
                return false;
            }
            cur = a.parent();
        }
        existed(p)
    };

    fn put(out: &mut Vec<Placed>, now: &mut BTreeMap<String, bool>, path: &str, what: What, pre: bool) {
        now.insert(path.to_string(), true);
        if let Some(p) = out.iter_mut().find(|p| p.path == path) {
            // Повторный шаг по тому же пути: вид — последний, «был до
            // установки» — от первого.
            if what != What::CreatedDir {
                p.what = Some(what);
            }
            return;
        }
        out.push(Placed { path: path.to_string(), what: Some(what), owner: None, pre_existed: pre, of: None });
    }

    // Недостающие родители `path` (сверху вниз) — заведённые каталоги.
    let parents = |now: &BTreeMap<String, bool>, path: &str| -> Vec<String> {
        let mut missing = Vec::new();
        let mut cur = Path::new(path).parent();
        while let Some(a) = cur {
            let a_s = a.to_string_lossy().into_owned();
            if a_s.is_empty() || a_s == "/" || present(now, &a_s) {
                break;
            }
            missing.push(a_s);
            cur = a.parent();
        }
        missing.reverse();
        missing
    };

    for step in steps {
        let (path, what) = match step {
            ExtStep::CopyTree { dest, .. } | ExtStep::ExtractZip { dest, .. } => (dest, What::Dir),
            ExtStep::CopyFile { dest, .. } => (dest, What::File),
            ExtStep::Symlink { link, .. } => (link, What::Link),
            ExtStep::MakeDirs { path } => (path, What::CreatedDir),
            ExtStep::WriteFile { path, content } => {
                let p = Path::new(path);
                if p.file_name().is_some_and(|n| n == OWNER_MARK) {
                    let dir = p.parent().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default();
                    if let Some(placed) = out.iter_mut().find(|x| x.path == dir) {
                        placed.owner = Some(content.trim().to_string());
                        continue;
                    }
                }
                (path, What::File)
            }
            ExtStep::RemoveTree { path, .. } | ExtStep::RemoveFile { path } => {
                now.insert(path.clone(), false);
                continue;
            }
            ExtStep::Stage { .. }
            | ExtStep::PkexecScript { .. }
            | ExtStep::Run { .. }
            | ExtStep::Spawn { .. }
            | ExtStep::RunNoted { .. }
            | ExtStep::Notice { .. }
            | ExtStep::Refuse { .. } => continue,
        };
        let pre = present(&now, path);
        if what == What::CreatedDir && pre {
            continue; // база уже была — она не наша
        }
        if !pre {
            for parent in parents(&now, path) {
                put(&mut out, &mut now, &parent, What::CreatedDir, false);
            }
        }
        put(&mut out, &mut now, path, what, pre && what == What::Dir);
    }
    // Файл, положенный ВНУТРЬ своего каталога, снимается вместе с ним.
    let dirs: Vec<String> = out
        .iter()
        .filter(|p| p.what == Some(What::Dir) && !p.pre_existed)
        .map(|p| p.path.clone())
        .collect();
    out.retain(|p| !dirs.iter().any(|d| p.path.starts_with(&format!("{d}/"))));
    out
}

/// Запись установленного пакета `.rxn` с путями от его плана.
pub fn rxn_entry(ext: &LegacyExt, placed: Vec<Placed>, placed_known: bool, installed_at: &str) -> Entry {
    Entry {
        source: Source::Rxn,
        id: ext.id.clone(),
        name: ext.name.clone(),
        kind: ext.kind.clone(),
        section: section_of_rxn(&ext.kind),
        enabled: Some(ext.enabled),
        installed_at: Some(installed_at.to_string()),
        package_copy: Some(ext.dir.clone()),
        placed_known,
        placed,
        elements: Vec::new(),
        card: None,
    }
}

// ============================================================================
// Удаление по журналу
// ============================================================================

/// Почему записанный путь не снимается.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeptWhy {
    /// Метка владельца не наша (или пропала): каталог теперь чужой.
    NotOwner,
    /// На месте лежит не то, что клали (была ссылка — стал каталог…).
    Changed,
    /// Каталог был на месте до установки.
    PreExisted,
}

/// План снятия одной записи.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Removal {
    /// Файловые шаги для общего исполнителя ([`crate::install_plan::exec`]).
    pub steps: Vec<ExtStep>,
    /// Заведённые планом каталоги — снимаются после шагов, только пустые.
    pub prune: Vec<String>,
    /// Что из записанного оставлено и почему.
    pub kept: Vec<(String, KeptWhy)>,
}

/// Что снять у записи `entry` — только записанное и только своё.
///
/// Пути снимаются в обратном порядке установки, копия пакета `.rxn` — в
/// конце. У записи без известных путей (`placed_known = false`) файловых
/// шагов нет: такую снимает прежний путь, по плану.
pub fn removal_plan(entry: &Entry, probe: &dyn Fn(&str) -> Probe) -> Removal {
    let mut r = Removal::default();
    for p in entry.placed.iter().rev() {
        let now = probe(&p.path);
        if !now.exists() {
            continue;
        }
        let tree = |path: &str| ExtStep::RemoveTree { path: path.to_string(), safe: true, ignore_errors: false };
        let file = |path: &str| ExtStep::RemoveFile { path: path.to_string() };
        if let Some(owner) = &p.owner {
            match &now {
                Probe::Dir { owner: Some(o) } if o == owner => r.steps.push(tree(&p.path)),
                _ => r.kept.push((p.path.clone(), KeptWhy::NotOwner)),
            }
            continue;
        }
        match (p.what, &now) {
            (Some(What::Dir), Probe::Dir { .. }) if p.pre_existed => {
                r.kept.push((p.path.clone(), KeptWhy::PreExisted))
            }
            (Some(What::Dir), Probe::Dir { .. }) => r.steps.push(tree(&p.path)),
            (Some(What::File), Probe::File) | (Some(What::Link), Probe::Link) => r.steps.push(file(&p.path)),
            (Some(What::CreatedDir), Probe::Dir { .. }) => r.prune.push(p.path.clone()),
            // Вид неизвестен (путь старого журнала магазина): как прежде —
            // по тому, что на диске.
            (None, Probe::Dir { .. }) => r.steps.push(tree(&p.path)),
            (None, Probe::File | Probe::Link) => r.steps.push(file(&p.path)),
            _ => r.kept.push((p.path.clone(), KeptWhy::Changed)),
        }
    }
    if let Some(copy) = &entry.package_copy {
        if matches!(probe(copy), Probe::Dir { .. }) {
            r.steps.push(ExtStep::RemoveTree { path: copy.clone(), safe: true, ignore_errors: false });
        }
    }
    r
}

/// Записан ли путь `path` в записи `entry`: сам или внутри записанного
/// своего каталога (не бывшего до установки и не заведённого — у тех своё
/// только то, что записано поимённо). Записанное снимает журнал — и решает,
/// снимать ли (чужое и бывшее до установки остаются).
pub fn records(entry: &Entry, path: &str) -> bool {
    let path = path.trim_end_matches('/');
    entry.placed.iter().any(|p| {
        let own = p.path.trim_end_matches('/');
        own == path
            || (matches!(p.what, Some(What::Dir) | None)
                && !p.pre_existed
                && path.starts_with(own)
                && path[own.len()..].starts_with('/'))
    })
}

/// Вписать файловое снятие по журналу в план удаления `plan` старого пути.
///
/// Шаги плана, кроме файловых (`path_of` → `Some`), остаются как были и где
/// были: выключение (gsettings, `kpackagetool -r`, Conky…) — до файлов,
/// перезагрузка оболочки и кеши (`qdbus … reconfigure`, `fc-cache`) — после.
/// Файловый шаг по записанному пути ([`records`]) уходит: его снимает
/// журнал, своими шагами `ours`, на месте первого файлового шага плана.
/// Файловый шаг по НЕзаписанному пути остаётся — так снимается то, что
/// положила не сама установка (программа из плана), как раньше.
pub fn merge_into_plan<S: Clone>(
    plan: &[S],
    path_of: &dyn Fn(&S) -> Option<String>,
    entry: &Entry,
    ours: Vec<S>,
) -> Vec<S> {
    let mut out = Vec::with_capacity(plan.len() + ours.len());
    let mut ours = Some(ours);
    for step in plan {
        let Some(path) = path_of(step) else {
            out.push(step.clone());
            continue;
        };
        if let Some(first) = ours.take() {
            out.extend(first);
        }
        if !records(entry, &path) {
            out.push(step.clone());
        }
    }
    if let Some(rest) = ours {
        out.extend(rest);
    }
    out
}

/// Путь файлового шага снятия (`RemoveTree`/`RemoveFile`).
pub fn removal_path(step: &ExtStep) -> Option<String> {
    match step {
        ExtStep::RemoveTree { path, .. } | ExtStep::RemoveFile { path } => Some(path.clone()),
        _ => None,
    }
}

/// Удаление пакета `.rxn` по журналу внутри его плана удаления `plan`
/// ([`merge_into_plan`]); копию пакета снимает вызывающий, как прежде.
/// `None` — путей у записи нет (`placed_known = false`, пакет поставлен до
/// журнала): снимать прежним планом целиком.
pub fn rxn_removal(plan: &[ExtStep], entry: &Entry, probe: &dyn Fn(&str) -> Probe) -> Option<Removal> {
    if !entry.placed_known {
        return None;
    }
    let bare = Entry { package_copy: None, ..entry.clone() };
    let mut r = removal_plan(&bare, probe);
    r.steps = merge_into_plan(plan, &removal_path, &bare, std::mem::take(&mut r.steps));
    Some(r)
}

/// Снятие одного пути `path` записи `entry` и того, что ему служит (ссылка
/// `~/.themes` на тему, [`Placed::of`]) — «Удалить» на плитке темы. `None` —
/// путь в записи не записан.
pub fn part_removal(entry: &Entry, path: &str, probe: &dyn Fn(&str) -> Probe) -> Option<Removal> {
    let path = path.trim_end_matches('/');
    let mine = |p: &Placed| p.path.trim_end_matches('/') == path || p.of.as_deref().map(|o| o.trim_end_matches('/')) == Some(path);
    if !entry.placed.iter().any(|p| p.path.trim_end_matches('/') == path) {
        return None;
    }
    let part = Entry {
        package_copy: None,
        placed: entry.placed.iter().filter(|p| mine(p)).cloned().collect(),
        ..entry.clone()
    };
    Some(removal_plan(&part, probe))
}

/// Снять пустые заведённые планом каталоги снятия `removal`.
pub fn prune(removal: &Removal) {
    for dir in &removal.prune {
        let _ = std::fs::remove_dir(dir); // не пуст — значит, не только наше
    }
}

/// Снять запись `key`: план [`removal_plan`] — общим исполнителем с
/// контекстом `ctx`, затем пустые заведённые каталоги; запись забывается,
/// когда всё своё снято. Ошибка шага — запись остаётся («Удалить» можно
/// нажать ещё раз).
pub fn remove(
    journal: &mut Journal,
    key: &str,
    probe: &dyn Fn(&str) -> Probe,
    ctx: &crate::install_plan::Ctx,
) -> Result<Removal, crate::install_plan::Failure> {
    use crate::install_plan::{exec, Failure, Step};
    let Some(entry) = journal.items.get(key) else {
        return Err(Failure::Text(format!("{key}: нет в журнале")));
    };
    let removal = removal_plan(entry, probe);
    let steps: Vec<Step> = removal.steps.iter().cloned().map(Step::from).collect();
    exec(&steps, &mut Vec::new(), ctx)?;
    prune(&removal);
    journal.items.remove(key);
    Ok(removal)
}

// ============================================================================
// Файл на диске
// ============================================================================

/// Путь журнала в каталоге конфигов Rexon `rexon_dir`.
pub fn journal_path(rexon_dir: &Path) -> PathBuf {
    rexon_dir.join(JOURNAL_NAME)
}

/// Где лежат журналы: каталог конфигов Rexon и журнал магазина в нём (у
/// сверок он бывает и под своим именем — общий журнал тогда рядом с ним).
struct Files {
    dir: PathBuf,
    store_log: PathBuf,
}

impl Files {
    fn of_dir(dir: &Path) -> Self {
        Files { dir: dir.to_path_buf(), store_log: dir.join(STORE_LOG_NAME) }
    }

    fn of_store_log(store_log: &str) -> Option<Self> {
        let dir = rexon_dir_of_store_log(store_log)?;
        Some(Files { dir, store_log: PathBuf::from(store_log) })
    }

    fn state(&self) -> PathBuf {
        self.dir.join(EXT_STATE_NAME)
    }

    fn ext_dir(&self) -> PathBuf {
        self.dir.join(EXT_DIR_NAME)
    }
}

/// Тексты старых журналов на диске (`None` — файла нет или он не читается).
struct Disk {
    store: Option<String>,
    state: Option<String>,
}

fn disk_of(files: &Files) -> Disk {
    Disk {
        store: read_lossy(&files.store_log),
        state: std::fs::read_to_string(files.state()).ok(),
    }
}

/// Отпечаток текста старого журнала для [`LegacySeen`]: FNV-1a (64 бита) и
/// длина; у отсутствующего файла — `none`. Алгоритм закреплён здесь, а не
/// взят у `std` (у того хэш между выпусками Rust не обещан).
pub fn fingerprint(text: Option<&str>) -> String {
    let Some(text) = text else { return "none".to_string() };
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}-{}", text.len())
}

fn seen_of(disk: &Disk) -> LegacySeen {
    LegacySeen {
        store: Some(fingerprint(disk.store.as_deref())),
        state: Some(fingerprint(disk.state.as_deref())),
    }
}

/// Журнал магазина `disk` с тех пор, как его видел журнал `journal`, писали
/// только мы (см. [`LegacySeen`]).
fn trusts_store(journal: Option<&Journal>, disk: &Disk) -> bool {
    journal.and_then(|j| j.legacy_seen.store.as_deref()) == Some(fingerprint(disk.store.as_deref()).as_str())
}

/// То же у карты состояний `.rxn`.
fn trusts_state(journal: Option<&Journal>, disk: &Disk) -> bool {
    journal.and_then(|j| j.legacy_seen.state.as_deref()) == Some(fingerprint(disk.state.as_deref()).as_str())
}

/// Карта «включено ли» пакетов `.rxn` журнала — в формате
/// `extensions_state.json`.
fn state_map_of(journal: &Journal) -> Map<String, Value> {
    journal
        .items
        .values()
        .filter(|e| e.source == Source::Rxn)
        .filter_map(|e| Some((e.id.clone(), Value::Bool(e.enabled?))))
        .collect()
}

/// Текст журнала магазина, по которому сверяются записи магазина: свой
/// (общий журнал), если старый файл с тех пор писали только мы, иначе —
/// старый файл (перенос).
fn store_source(journal: Option<&Journal>, disk: &Disk) -> Option<String> {
    match journal {
        Some(j) if trusts_store(journal, disk) => {
            Some(crate::ocs_store::serialize_installed_log(&store_log_of(j)))
        }
        _ => disk.store.clone(),
    }
}

/// Источники сверки ([`Legacy`]) для журнала `journal` каталога `files`:
/// записи магазина и «включено» `.rxn` — из общего журнала либо, если старый
/// файл менялся без нас, из него ([`LegacySeen`]); пакеты `.rxn` — всегда
/// из каталога расширений (его карточки «Расширений» и читают).
fn sources(files: &Files, journal: Option<&Journal>, disk: &Disk) -> Legacy {
    let store_log = store_source(journal, disk);
    let theme_links = theme_links_of(&crate::ocs_store::parse_installed_log(store_log.as_deref()));
    let state = match journal {
        Some(j) if trusts_state(journal, disk) => state_map_of(j),
        _ => crate::ext_manager::load_state(disk.state.as_deref()),
    };
    let extensions = crate::qt_extensions::installed_in(&files.ext_dir(), &state).iter().map(legacy_ext).collect();
    Legacy { store_log, extensions, theme_links }
}

/// Старые журналы в `rexon_dir` как есть — те же файлы, что читают их
/// владельцы (без общего журнала).
pub fn legacy_at(rexon_dir: &Path) -> Legacy {
    let files = Files::of_dir(rexon_dir);
    sources(&files, None, &disk_of(&files))
}

/// Текст файла; не UTF-8 — с заменой (так журнал магазина читали его
/// читатели: битый байт не делает журнал пустым).
fn read_lossy(path: &Path) -> Option<String> {
    std::fs::read(path).ok().map(|raw| String::from_utf8_lossy(&raw).into_owned())
}

/// Общий журнал каталога конфигов `rexon_dir` для читателя: прочитан и
/// сверен ([`reconcile`]; старые файлы — только если их писали без нас), но
/// НЕ записан (см. [`store_log_view`]).
pub fn view_at(rexon_dir: &Path) -> Journal {
    let files = Files::of_dir(rexon_dir);
    let journal = read_lossy(&journal_path(rexon_dir)).and_then(|t| parse(Some(&t)));
    let disk = disk_of(&files);
    let legacy = legacy_with_rxn_links(sources(&files, journal.as_ref(), &disk), journal.as_ref());
    reconcile(journal, &legacy, &probe_live)
}

/// Старые журналы плюс ссылки `~/.themes` на темы пакетов `.rxn` журнала.
fn legacy_with_rxn_links(mut legacy: Legacy, journal: Option<&Journal>) -> Legacy {
    if let Some(j) = journal {
        legacy.theme_links.extend(rxn_theme_links_of(j));
    }
    legacy
}

/// Журнал магазина `log_path` для читателя — через общий журнал рядом
/// (тот же каталог конфигов), сверенный с ним ([`reconcile`],
/// [`store_log_of`]). Только чтение: ни общий, ни старый журнал не
/// пишутся — читатель на чужом или подставном HOME ничего не оставляет;
/// записывают журнал установка и удаление ([`put_store_log`]).
pub fn store_log_view(log_path: &str) -> Map<String, Value> {
    store_log_of(&store_view(log_path))
}

/// Записи магазина общего журнала рядом с журналом магазина `log_path`,
/// сверенные с ним ([`reconcile`]) — пути с видом по диску и ссылки
/// `~/.themes`. Только чтение (см. [`store_log_view`]); удаления магазина
/// берут отсюда, что снимать.
pub fn store_view(log_path: &str) -> Journal {
    let disk = Disk { store: read_lossy(Path::new(log_path)), state: None };
    let journal = rexon_dir_of_store_log(log_path)
        .and_then(|dir| read_lossy(&journal_path(&dir)))
        .and_then(|t| parse(Some(&t)));
    let store_log = store_source(journal.as_ref(), &disk);
    let theme_links = theme_links_of(&crate::ocs_store::parse_installed_log(store_log.as_deref()));
    let legacy = Legacy { store_log, extensions: Vec::new(), theme_links };
    reconcile(journal, &legacy, &probe_live)
}

/// Пакет из каталога расширений глазами журнала.
pub fn legacy_ext(ext: &crate::extension::Extension) -> LegacyExt {
    let id = ext.ext_id().to_string();
    LegacyExt {
        name: ext.manifest.get("name").and_then(Value::as_str).unwrap_or(&id).to_string(),
        kind: ext.ext_type().to_string(),
        enabled: ext.enabled,
        dir: ext.dir.to_string_lossy().into_owned(),
        id,
    }
}

/// Записать журнал атомарно: при сбое прежний файл цел и полуфайла нет.
pub fn save_at(rexon_dir: &Path, journal: &Journal) -> std::io::Result<()> {
    std::fs::create_dir_all(rexon_dir)?;
    crate::utils::atomic_write(&journal_path(rexon_dir).to_string_lossy(), &serialize(journal))
}

// ── Запись: только общий журнал, старые файлы — его зеркало ─────────────────

/// Прочитанный писателем журнал: текст файла, сверенный журнал и то, из
/// чего он сверен.
struct Loaded {
    files: Files,
    text: Option<String>,
    journal: Journal,
    legacy: Legacy,
    disk: Disk,
}

/// Прочитать журнал для записи. Нечитаемый журнал откладывается рядом
/// (`.broken`) и собирается заново из старых файлов.
fn load_files(files: Files) -> Loaded {
    let path = journal_path(&files.dir);
    let text = std::fs::read_to_string(&path).ok();
    let parsed = parse(text.as_deref());
    if text.is_some() && parsed.is_none() {
        let _ = std::fs::rename(&path, files.dir.join(format!("{JOURNAL_NAME}.broken")));
    }
    let disk = disk_of(&files);
    let legacy = legacy_with_rxn_links(sources(&files, parsed.as_ref(), &disk), parsed.as_ref());
    let journal = reconcile(parsed, &legacy, &probe_live);
    Loaded { files, text, journal, legacy, disk }
}

/// Какие старые журналы затронуты.
#[derive(Debug, Clone, Copy, Default)]
struct Parts {
    store: bool,
    state: bool,
}

/// Совпадают ли две карты вместе с порядком записей (порядок — часть текста
/// старого журнала).
fn same_entries(a: &Map<String, Value>, b: &Map<String, Value>) -> bool {
    a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| x == y)
}

/// Пары карты состояний `.rxn` по журналу — в том порядке, в каком их писал
/// прежний писатель (`ext_exec::save_state_with`): пакеты каталога
/// расширений по порядку чтения каталога, затем записанные, которых в
/// каталоге (ещё) не видно.
fn state_pairs_of(files: &Files, journal: &Journal) -> Vec<(String, bool)> {
    let state = state_map_of(journal);
    let mut pairs: Vec<(String, bool)> = crate::qt_extensions::installed_in(&files.ext_dir(), &state)
        .iter()
        .map(|e| (e.ext_id().to_string(), e.enabled))
        .collect();
    for e in journal.items.values().filter(|e| e.source == Source::Rxn) {
        if let Some(on) = e.enabled {
            if !pairs.iter().any(|(id, _)| id == &e.id) {
                pairs.push((e.id.clone(), on));
            }
        }
    }
    pairs
}

/// Зеркало: старые журналы из общего — тем же текстом, что писала прежняя
/// логика (`ocs_store::serialize_installed_log`,
/// `ext_manager::save_state_text`), атомарно. Пишутся только части `parts`.
/// Нужны они ровно тому, кто вернётся на прежнюю версию Rexon: она читает
/// только старые файлы. Возвращает, какие части записаны; сбой — в журнал
/// событий (общий журнал уже записан, следующая запись повторит зеркало).
fn mirror_legacy(files: &Files, journal: &Journal, parts: Parts) -> Parts {
    let mut wrote = Parts::default();
    let write = |path: &Path, text: &str| -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        crate::utils::atomic_write(&path.to_string_lossy(), text)
    };
    if parts.store {
        let text = crate::ocs_store::serialize_installed_log(&store_log_of(journal));
        match write(&files.store_log, &text) {
            Ok(()) => wrote.store = true,
            Err(e) => crate::log_warn!("install", "зеркало журнала магазина не записано ({}) — {e}", files.store_log.display()),
        }
    }
    if parts.state {
        let text = crate::ext_manager::save_state_text(&state_pairs_of(files, journal));
        match write(&files.state(), &text) {
            Ok(()) => wrote.state = true,
            Err(e) => crate::log_warn!("install", "зеркало карты состояний не записано ({}) — {e}", files.state().display()),
        }
    }
    wrote
}

/// Записать журнал `after` (прочитанный как `loaded`) и его зеркало.
///
/// Зеркалится часть, которую правил писатель (`touched`), если старый файл
/// с ней разошёлся (или его нет) — как всегда писал прежний писатель; и
/// часть, которую никто не правил, но чьё зеркало прошлой записью не легло
/// (файл тот же, что мы видели, а с журналом расходится). Старый файл,
/// который писали без нас, сам по себе не переписывается: он уже перенесён.
///
/// Порядок защищает от обрыва посредине: сначала журнал (с отпечатками
/// старых файлов, из которых он собран), затем зеркало, затем — отпечатки
/// записанного зеркала. Ошибка — только у записи самого журнала.
fn commit(loaded: Loaded, mut after: Journal, touched: Parts) -> std::io::Result<Journal> {
    let Loaded { files, text, journal: before, disk, .. } = loaded;
    let store_want = store_log_of(&after);
    let store_differs = disk.store.is_none()
        || !same_entries(&store_want, &crate::ocs_store::parse_installed_log(disk.store.as_deref()));
    let state_pairs = state_pairs_of(&files, &after);
    let state_want: Map<String, Value> = state_pairs.iter().map(|(id, on)| (id.clone(), Value::Bool(*on))).collect();
    let state_differs = disk.state.is_none()
        || !same_entries(&state_want, &crate::ext_manager::load_state(disk.state.as_deref()));
    let stale_store = trusts_store(Some(&before), &disk) && store_differs && !(disk.store.is_none() && store_want.is_empty());
    let stale_state = trusts_state(Some(&before), &disk) && state_differs && !(disk.state.is_none() && state_want.is_empty());
    let parts = Parts {
        store: (touched.store && store_differs) || stale_store,
        state: (touched.state && state_differs) || stale_state,
    };

    after.legacy_seen = seen_of(&disk);
    if text.as_deref() != Some(serialize(&after).as_str()) {
        save_at(&files.dir, &after)?;
    }
    let wrote = mirror_legacy(&files, &after, parts);
    if wrote.store || wrote.state {
        after.legacy_seen = seen_of(&disk_of(&files));
        if let Err(e) = save_at(&files.dir, &after) {
            crate::log_warn!("install", "отпечатки зеркала не записаны в общий журнал ({}) — {e}", files.dir.display());
        }
    }
    Ok(after)
}

/// Прочитать журнал, сверив его ([`reconcile`]), и записать, если он
/// изменился (на первом чтении — всегда); зеркало — только недописанное
/// прошлой записью ([`commit`]). Старые файлы, писанные без нас, читаются
/// один раз — для переноса — и не удаляются: это страховка отката версии.
pub fn load_at(rexon_dir: &Path) -> std::io::Result<Journal> {
    let loaded = load_files(Files::of_dir(rexon_dir));
    let journal = loaded.journal.clone();
    commit(loaded, journal, Parts::default())
}

/// Записать установку пакета `.rxn` (после её успеха).
pub fn record_rxn_at(rexon_dir: &Path, entry: Entry) -> std::io::Result<()> {
    let loaded = load_files(Files::of_dir(rexon_dir));
    let mut journal = loaded.journal.clone();
    journal.items.insert(entry.key(), entry);
    commit(loaded, journal, Parts { state: true, ..Parts::default() }).map(drop)
}

/// Записать применённый образ «Как в дистрибутиве» (после успеха).
pub fn record_look_at(rexon_dir: &Path, entry: Entry) -> std::io::Result<()> {
    let loaded = load_files(Files::of_dir(rexon_dir));
    let mut journal = loaded.journal.clone();
    journal.items.insert(entry.key(), entry);
    commit(loaded, journal, Parts::default()).map(drop)
}

/// Забыть запись `key` (образ снят «Вернуть как было»).
pub fn forget_at(rexon_dir: &Path, key: &str) -> std::io::Result<()> {
    let loaded = load_files(Files::of_dir(rexon_dir));
    let mut journal = loaded.journal.clone();
    journal.items.remove(key);
    commit(loaded, journal, Parts::default()).map(drop)
}

/// Отметить пакет `.rxn` `id` включённым или выключенным (`Some`) либо
/// забыть его (`None` — пакет снят). Пакет, которого в каталоге расширений
/// (ещё) не видно, всё равно записывается — как у прежней карты состояний.
pub fn set_rxn_enabled_at(rexon_dir: &Path, id: &str, enabled: Option<bool>) -> std::io::Result<()> {
    let loaded = load_files(Files::of_dir(rexon_dir));
    let mut journal = loaded.journal.clone();
    let key = key_of(Source::Rxn, id);
    match enabled {
        None => {
            journal.items.remove(&key);
        }
        Some(on) => {
            journal
                .items
                .entry(key)
                .or_insert_with(|| Entry {
                    source: Source::Rxn,
                    id: id.to_string(),
                    name: id.to_string(),
                    kind: String::new(),
                    section: None,
                    enabled: None,
                    installed_at: None,
                    package_copy: None,
                    placed_known: false,
                    placed: Vec::new(),
                    elements: Vec::new(),
                    card: None,
                })
                .enabled = Some(on);
        }
    }
    commit(loaded, journal, Parts { state: true, ..Parts::default() }).map(drop)
}

/// Записать покупки магазина: `data` — журнал магазина целиком в прежнем
/// формате (`content_id` → карточка), каким его сделал писатель из
/// [`store_log_view`]. Пишется общий журнал (записи магазина пересобраны,
/// `.rxn` — как были) и зеркало `log_path`.
pub fn put_store_log(log_path: &str, data: &Map<String, Value>) -> std::io::Result<()> {
    let files = Files::of_store_log(log_path)
        .ok_or_else(|| std::io::Error::other(format!("{log_path}: у пути нет каталога")))?;
    let loaded = load_files(files);
    let legacy = legacy_with_rxn_links(
        Legacy {
            store_log: Some(crate::ocs_store::serialize_installed_log(data)),
            extensions: loaded.legacy.extensions.clone(),
            theme_links: theme_links_of(data),
        },
        Some(&loaded.journal),
    );
    let journal = reconcile(Some(loaded.journal.clone()), &legacy, &probe_live);
    commit(loaded, journal, Parts { store: true, ..Parts::default() }).map(drop)
}

/// Записать «Применить» раздела (после успеха нажатия). Запись раздела уже
/// есть — дописываются только новые пути (их ранний снимок — правда о
/// том, что было до Rexon); лишние копии нового снимка стираются.
pub fn record_applied_at(rexon_dir: &Path, applied: Applied) -> std::io::Result<()> {
    record_applied_values_at(rexon_dir, applied, Vec::new())
}

/// [`record_applied_at`] вместе с прежними значениями стола `values`:
/// значение ключа, уже записанного раньше, не заменяется — раннее и есть
/// «что было до Rexon».
pub fn record_applied_values_at(rexon_dir: &Path, applied: Applied, values: Vec<AppliedValue>) -> std::io::Result<()> {
    let loaded = load_files(Files::of_dir(rexon_dir));
    let mut journal = loaded.journal.clone();
    // Значения без записи раздела — осиротевшие (их запись сняли не мы):
    // начинаем с чистого листа.
    if !journal.applied.contains_key(&applied.section) {
        journal.applied_values.remove(&applied.section);
    }
    if !values.is_empty() {
        let kept = journal.applied_values.entry(applied.section.clone()).or_default();
        for v in values {
            if !kept.iter().any(|k| k.key == v.key) {
                kept.push(v);
            }
        }
    }
    let merged = match journal.applied.remove(&applied.section) {
        None => applied,
        Some(mut prev) => {
            for f in applied.files {
                if prev.files.iter().any(|p| p.path == f.path) {
                    if let Some(copy) = &f.before {
                        let _ = std::fs::remove_file(copy);
                    }
                } else {
                    prev.files.push(f);
                }
            }
            prev.theme = applied.theme;
            prev.item = applied.item;
            prev.applied_at = applied.applied_at;
            prev
        }
    };
    journal.applied.insert(merged.section.clone(), merged);
    commit(loaded, journal, Parts::default()).map(drop)
}

/// «Вернуть как было» по записи [`Applied`] раздела `section`: прежнее
/// содержимое — на место, заведённое нажатием — снять (каталог — только
/// пустой), прежние значения стола — вернуть командами
/// ([`Journal::applied_values`]); запись и снимок стираются. `Ok(None)` —
/// записи нет; `Ok(Some(не вернулось))` — пути и ключи, которые вернуть не
/// вышло (их копии остаются в снимке, запись — тоже).
///
/// ⚠️ Команды возврата значений идут на ЖИВОЙ машине
/// ([`crate::install_plan::run_live`]); проверкам — [`revert_applied_with`]
/// с подменным запуском.
pub fn revert_applied_at(rexon_dir: &Path, section: &str) -> std::io::Result<Option<Vec<String>>> {
    revert_applied_with(rexon_dir, section, &|_| false, &crate::install_plan::run_live)
}

/// [`revert_applied_at`], но пути, для которых `handled` отвечает `true`,
/// не трогаются: их уже вернул свой путь (памятка слоя цветов,
/// `shell_palette::revert_scheme_layer`, который бережёт правки после
/// Rexon). Их копии уходят вместе со снимком.
pub fn revert_applied_except_at(
    rexon_dir: &Path,
    section: &str,
    handled: &dyn Fn(&str) -> bool,
) -> std::io::Result<Option<Vec<String>>> {
    revert_applied_with(rexon_dir, section, handled, &crate::install_plan::run_live)
}

/// [`revert_applied_except_at`] с запуском команд `run` (возврат значений
/// стола).
pub fn revert_applied_with(
    rexon_dir: &Path,
    section: &str,
    handled: &dyn Fn(&str) -> bool,
    run: &dyn Fn(&[String], Option<u64>) -> crate::utils::RunReport,
) -> std::io::Result<Option<Vec<String>>> {
    let loaded = load_files(Files::of_dir(rexon_dir));
    let mut journal = loaded.journal.clone();
    let Some(rec) = journal.applied.get(section).cloned() else {
        return Ok(None);
    };
    let mut failed = Vec::new();
    for f in rec.files.iter().rev() {
        if handled(&f.path) {
            continue;
        }
        let path = Path::new(&f.path);
        let ok = match (&f.before, f.dir) {
            (Some(copy), _) => restore_copy(Path::new(copy), path).is_ok(),
            // Не пуст — там уже не только наше, остаётся; нет — и не надо.
            (None, true) => {
                let _ = std::fs::remove_dir(path);
                true
            }
            (None, false) => match std::fs::symlink_metadata(path) {
                Ok(m) if m.is_dir() => false,
                Ok(_) => std::fs::remove_file(path).is_ok(),
                Err(_) => true,
            },
        };
        if !ok {
            failed.push(f.path.clone());
        }
    }
    let values: Vec<crate::install_plan::ValueBefore> = journal
        .applied_values
        .get(section)
        .into_iter()
        .flatten()
        .map(|v| crate::install_plan::ValueBefore { key: v.key.clone(), restore: v.restore.clone() })
        .collect();
    failed.extend(crate::install_plan::restore_values(&values, run));
    if failed.is_empty() {
        journal.applied.remove(section);
        journal.applied_values.remove(section);
        let snap = Path::new(&rec.snapshot);
        // Стираем только свой каталог снимка.
        if snap.starts_with(rexon_dir.join("applied")) {
            let _ = std::fs::remove_dir_all(snap);
        }
    }
    commit(loaded, journal, Parts::default())?;
    Ok(Some(failed))
}

/// Вернуть копию из снимка на место `path` (поверх того, что там сейчас).
fn restore_copy(copy: &Path, path: &Path) -> std::io::Result<()> {
    if let Ok(m) = std::fs::symlink_metadata(path) {
        if m.is_dir() {
            return Err(std::io::Error::other("на месте файла каталог"));
        }
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    // Копия, а не перенос: сбой посреди возврата не теряет снимок. Ссылку
    // на месте не снимаем — пишем сквозь неё, как писало нажатие.
    let data = std::fs::read(copy)?;
    crate::utils::atomic_write_bytes(&path.to_string_lossy(), &data)?;
    if let Ok(m) = std::fs::metadata(copy) {
        let _ = std::fs::set_permissions(path, m.permissions());
    }
    Ok(())
}

/// Каталог конфигов Rexon, в котором лежит журнал магазина `store_log`.
pub fn rexon_dir_of_store_log(store_log: &str) -> Option<PathBuf> {
    Path::new(store_log).parent().map(Path::to_path_buf)
}

// ============================================================================
// Проверки
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rexon-install-journal-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn s(p: &Path) -> String {
        p.to_string_lossy().into_owned()
    }

    fn no_run(_: &[String], _: Option<u64>) -> crate::utils::RunReport {
        crate::utils::RunReport::default()
    }
    fn nothing(_: &str) -> bool {
        false
    }
    fn no_spawn(_: &[String]) {}

    /// Подменный контекст: ни одной настоящей программы.
    fn ctx() -> crate::install_plan::Ctx<'static> {
        crate::install_plan::Ctx {
            install: false,
            steamos: false,
            privileged: None,
            run: &no_run,
            have: &nothing,
            spawn: &no_spawn,
            wayland: false,
            sync_env: &crate::install_plan::no_sync_env,
        }
    }

    /// Профиль с обоими старыми журналами: магазин (тема GTK — каталог, цвета
    /// — файл, путь, которого уже нет) и пакет `.rxn` (выключен в карте).
    fn legacy_profile(root: &Path) -> (PathBuf, String) {
        let rexon = root.join("config/rexon");
        let gtk = root.join("data/themes/Nordic");
        std::fs::create_dir_all(&gtk).unwrap();
        std::fs::write(gtk.join("index.theme"), "x").unwrap();
        let colors = root.join("data/color-schemes/Nordic.colors");
        std::fs::create_dir_all(colors.parent().unwrap()).unwrap();
        std::fs::write(&colors, "c").unwrap();
        let store = serde_json::json!({
            "12345": {
                "name": "Nordic", "rexon_type": "gtk",
                "paths": [s(&gtk), s(&colors), s(&root.join("gone")), ""],
                "installed_at": "2026-10-01T10:00:00", "author": "Автор",
                "score": 80, "tags": ["dark"], "provider_id": "kde-store", "category_id": 135
            }
        });
        let store_text = serde_json::to_string_pretty(&store).unwrap();
        std::fs::create_dir_all(&rexon).unwrap();
        std::fs::write(rexon.join(STORE_LOG_NAME), &store_text).unwrap();
        let pkg = rexon.join("extensions/my-icons");
        std::fs::create_dir_all(&pkg).unwrap();
        std::fs::write(
            pkg.join("manifest.json"),
            r#"{"id": "my-icons", "name": "Мои значки", "type": "theme-icons", "version": "1.0"}"#,
        )
        .unwrap();
        std::fs::write(rexon.join(EXT_STATE_NAME), "{\n  \"my-icons\": false\n}").unwrap();
        (rexon, store_text)
    }

    /// Первое чтение переносит ОБА старых формата, ничего не теряя, и
    /// старые файлы не трогает.
    #[test]
    fn first_read_migrates_both_legacy_formats() {
        let root = scratch("migrate");
        let (rexon, store_text) = legacy_profile(&root);
        let state_before = std::fs::read_to_string(rexon.join(EXT_STATE_NAME)).unwrap();

        let j = load_at(&rexon).expect("журнал собран");
        assert!(journal_path(&rexon).is_file(), "новый журнал не записан");

        let store = &j.items["store:12345"];
        assert_eq!(store.source, Source::Store);
        assert_eq!(store.name, "Nordic");
        assert_eq!(store.kind, "gtk");
        assert_eq!(store.installed_at.as_deref(), Some("2026-10-01T10:00:00"));
        let whats: Vec<Option<What>> = store.placed.iter().map(|p| p.what).collect();
        assert_eq!(whats, vec![Some(What::Dir), Some(What::File), None], "пустой путь отброшен, пропавший — без вида");
        // Карточка — запись старого журнала как есть.
        let old: Value = serde_json::from_str(&store_text).unwrap();
        assert_eq!(Value::Object(store.card.clone().unwrap()), old["12345"]);

        let rxn = &j.items["rxn:my-icons"];
        assert_eq!(rxn.name, "Мои значки");
        assert_eq!(rxn.kind, "theme-icons");
        assert_eq!(rxn.enabled, Some(false));
        assert!(!rxn.placed_known, "путей у старого .rxn не было — и выдумывать их нельзя");
        assert_eq!(rxn.package_copy.as_deref(), Some(s(&rexon.join("extensions/my-icons")).as_str()));

        assert_eq!(std::fs::read_to_string(rexon.join(STORE_LOG_NAME)).unwrap(), store_text);
        assert_eq!(std::fs::read_to_string(rexon.join(EXT_STATE_NAME)).unwrap(), state_before);

        // Второе чтение — тот же журнал, файл не переписывается.
        let again = load_at(&rexon).unwrap();
        assert_eq!(again, j);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Сверка: ушедшее из старых уходит, точные пути `.rxn` сохраняются,
    /// «включено» берётся из карты.
    #[test]
    fn reconcile_keeps_recorded_paths_and_follows_legacy() {
        let ext = LegacyExt { id: "a".into(), name: "A".into(), kind: "theme-icons".into(), enabled: true, dir: "/x/a".into() };
        let placed = vec![Placed { path: "/h/icons/A".into(), what: Some(What::Dir), owner: None, pre_existed: false, of: None }];
        let mut j = Journal::default();
        let e = rxn_entry(&ext, placed.clone(), true, "t");
        j.items.insert(e.key(), e);
        j.items.insert(
            "store:9".into(),
            Entry { source: Source::Store, id: "9".into(), ..rxn_entry(&ext, Vec::new(), true, "t") },
        );
        let legacy = Legacy { store_log: None, extensions: vec![LegacyExt { enabled: false, ..ext }], ..Default::default() };
        let out = reconcile(Some(j), &legacy, &|_| Probe::Missing);
        assert_eq!(out.items.keys().collect::<Vec<_>>(), vec!["rxn:a"], "снятое из магазина ушло");
        assert_eq!(out.items["rxn:a"].placed, placed);
        assert!(out.items["rxn:a"].placed_known);
        assert_eq!(out.items["rxn:a"].enabled, Some(false));
    }

    /// Пути из плана: каталог с меткой — с владельцем, база, которая уже
    /// была, — не наша, недостающая — заведённая, файл внутри своего
    /// каталога отдельно не пишется, своя прежняя копия — не «была до».
    #[test]
    fn placed_from_plan_steps() {
        let have = ["/h/.themes", "/h/.local/share/icons", "/h/.local/share/icons/Mine", "/h/.local/share"];
        let existed = |p: &str| have.contains(&p);
        let v = |x: &str| x.to_string();
        let steps = vec![
            ExtStep::MakeDirs { path: v("/h/.local/share") },
            ExtStep::RemoveTree { path: v("/h/.themes/T"), safe: true, ignore_errors: true },
            ExtStep::CopyTree { src: v("/e/data/T"), dest: v("/h/.themes/T") },
            ExtStep::WriteFile { path: v("/h/.themes/T/.rexon-owner"), content: v("pkg\n") },
            ExtStep::WriteFile { path: v("/h/.themes/T/extra.css"), content: v("") },
            ExtStep::CopyTree { src: v("/e/data/Mine"), dest: v("/h/.local/share/icons/Mine") },
            ExtStep::MakeDirs { path: v("/h/.local/share/fonts/rexon-pkg") },
            ExtStep::CopyFile { src: v("/e/a.ttf"), dest: v("/h/.local/share/fonts/rexon-pkg/a.ttf") },
            ExtStep::Symlink { target: v("/e/w"), link: v("/h/.local/share/rexon/wallpapers/pkg") },
            ExtStep::Run { argv: vec![v("fc-cache")], timeout_secs: None },
        ];
        let got: Vec<(String, Option<What>, Option<String>, bool)> = placed_from_steps(&steps, &existed)
            .into_iter()
            .map(|p| (p.path, p.what, p.owner, p.pre_existed))
            .collect();
        let d = |p: &str, w: What, o: Option<&str>, pre: bool| (v(p), Some(w), o.map(v), pre);
        assert_eq!(
            got,
            vec![
                d("/h/.themes/T", What::Dir, Some("pkg"), false),
                d("/h/.local/share/icons/Mine", What::Dir, None, true),
                d("/h/.local/share/fonts", What::CreatedDir, None, false),
                d("/h/.local/share/fonts/rexon-pkg", What::CreatedDir, None, false),
                d("/h/.local/share/fonts/rexon-pkg/a.ttf", What::File, None, false),
                d("/h/.local/share/rexon", What::CreatedDir, None, false),
                d("/h/.local/share/rexon/wallpapers", What::CreatedDir, None, false),
                d("/h/.local/share/rexon/wallpapers/pkg", What::Link, None, false),
            ]
        );
    }

    /// Удаление по журналу у `.rxn`: своё снято, заведённые каталоги — только
    /// пустые, чужая метка и бывший до установки каталог — на месте, копия
    /// пакета снята, запись забыта; программ не запускается.
    #[test]
    fn remove_rxn_entry_by_journal() {
        let root = scratch("rm-rxn");
        let p = |x: &str| s(&root.join(x));
        for d in ["themes/T", "themes/Foreign", "icons/Mine", "fonts/rexon-pkg", "pkgcopy"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        std::fs::write(root.join("themes/T/.rexon-owner"), "pkg\n").unwrap();
        std::fs::write(root.join("themes/Foreign/.rexon-owner"), "someone-else\n").unwrap();
        std::fs::write(root.join("icons/Mine/index.theme"), "человека").unwrap();
        std::fs::write(root.join("fonts/rexon-pkg/a.ttf"), "f").unwrap();
        std::fs::write(root.join("fonts/human.ttf"), "f").unwrap();
        std::os::unix::fs::symlink("/nonexistent", root.join("wall")).unwrap();
        let pl = |path: String, what: What, owner: Option<&str>, pre: bool| Placed {
            path,
            what: Some(what),
            owner: owner.map(str::to_string),
            pre_existed: pre,
            of: None,
        };
        let ext = LegacyExt { id: "pkg".into(), name: "P".into(), kind: "theme-gtk".into(), enabled: true, dir: p("pkgcopy") };
        let entry = rxn_entry(
            &ext,
            vec![
                pl(p("themes/T"), What::Dir, Some("pkg"), false),
                pl(p("themes/Foreign"), What::Dir, Some("pkg"), false),
                pl(p("icons/Mine"), What::Dir, None, true),
                pl(p("fonts"), What::CreatedDir, None, false),
                pl(p("fonts/rexon-pkg"), What::CreatedDir, None, false),
                pl(p("fonts/rexon-pkg/a.ttf"), What::File, None, false),
                pl(p("wall"), What::Link, None, false),
            ],
            true,
            "t",
        );
        let mut j = Journal::default();
        j.items.insert(entry.key(), entry);
        let removal = remove(&mut j, "rxn:pkg", &probe_live, &ctx()).expect("снято");
        assert!(!root.join("themes/T").exists());
        assert!(root.join("themes/Foreign/.rexon-owner").is_file(), "снят каталог с чужой меткой");
        assert!(root.join("icons/Mine/index.theme").is_file(), "снят каталог, бывший до установки");
        assert!(!root.join("fonts/rexon-pkg").exists(), "пустой заведённый каталог остался");
        assert!(root.join("fonts/human.ttf").is_file() && root.join("fonts").is_dir(), "непустой каталог снят");
        assert!(std::fs::symlink_metadata(root.join("wall")).is_err());
        assert!(!root.join("pkgcopy").exists(), "копия пакета осталась");
        assert!(j.items.is_empty());
        assert_eq!(
            removal.kept,
            vec![(p("icons/Mine"), KeptWhy::PreExisted), (p("themes/Foreign"), KeptWhy::NotOwner)]
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Удаление по журналу у магазина (запись из сверки старого журнала):
    /// каталог и файл сняты; путь, ставший ссылкой вместо каталога, — чужой.
    #[test]
    fn remove_store_entry_by_journal() {
        let root = scratch("rm-store");
        let (rexon, _) = legacy_profile(&root);
        let mut j = load_at(&rexon).unwrap();
        // После сверки на месте каталога темы появилась ссылка человека.
        let swapped = root.join("data/themes/Swapped");
        std::fs::create_dir_all(&swapped).unwrap();
        j.items.get_mut("store:12345").unwrap().placed.push(Placed {
            path: s(&swapped),
            what: Some(What::Dir),
            owner: None,
            pre_existed: false,
            of: None,
        });
        std::fs::remove_dir(&swapped).unwrap();
        std::os::unix::fs::symlink(root.join("data/themes/Nordic"), &swapped).unwrap();

        let removal = remove(&mut j, "store:12345", &probe_live, &ctx()).expect("снято");
        assert!(!root.join("data/themes/Nordic").exists());
        assert!(!root.join("data/color-schemes/Nordic.colors").exists());
        assert!(std::fs::symlink_metadata(&swapped).is_ok(), "снята чужая ссылка");
        assert_eq!(removal.kept, vec![(s(&swapped), KeptWhy::Changed)]);
        assert!(!j.items.contains_key("store:12345"));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Сбой записи журнала не оставляет полуфайла и не трогает старые.
    #[test]
    fn failed_write_leaves_no_partial_file() {
        let root = scratch("fail-write");
        let (rexon, store_text) = legacy_profile(&root);
        // На месте журнала — каталог: подмена имени не пройдёт.
        std::fs::create_dir_all(journal_path(&rexon)).unwrap();
        assert!(save_at(&rexon, &Journal::default()).is_err());
        assert!(load_at(&rexon).is_err());
        let names: Vec<String> = std::fs::read_dir(&rexon)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(sorted, vec!["extensions", EXT_STATE_NAME, JOURNAL_NAME, STORE_LOG_NAME], "полуфайл: {names:?}");
        assert!(journal_path(&rexon).is_dir());
        assert_eq!(std::fs::read_to_string(rexon.join(STORE_LOG_NAME)).unwrap(), store_text);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Шаг 5б: старые файлы читаются для переноса один раз. Дальше источник —
    /// общий журнал: его правка видна читателям, хотя старый файл о ней не
    /// знает. Правка старого файла без нас (прежняя версия Rexon после
    /// отката) — снова перенос, и сам старый файл при этом не переписывается.
    #[test]
    fn legacy_is_read_once_then_journal_is_the_source() {
        let root = scratch("once");
        let (rexon, store_text) = legacy_profile(&root);
        let log = s(&rexon.join(STORE_LOG_NAME));
        let mut j = load_at(&rexon).unwrap();
        assert_eq!(j.legacy_seen.store.as_deref(), Some(fingerprint(Some(&store_text)).as_str()));
        assert!(j.legacy_seen.state.is_some());

        // Правка только в общем журнале (старый файл тот же) — видна.
        j.items.get_mut("store:12345").unwrap().card.as_mut().unwrap().insert("name".into(), "Nordic (журнал)".into());
        save_at(&rexon, &j).unwrap();
        assert_eq!(store_log_view(&log)["12345"]["name"], "Nordic (журнал)", "старый файл перечитан вместо журнала");
        assert_eq!(view_at(&rexon).items["store:12345"].card.as_ref().unwrap()["name"], "Nordic (журнал)");
        assert_eq!(std::fs::read_to_string(&log).unwrap(), store_text, "читатель переписал старый файл");

        // Прежняя версия Rexon дописала покупку в старый файл — перенос.
        let mut old = crate::ocs_store::parse_installed_log(Some(&store_text));
        old.insert("777".into(), serde_json::json!({"name": "Neon", "rexon_type": "icons", "paths": []}));
        let rolled = crate::ocs_store::serialize_installed_log(&old);
        std::fs::write(&log, &rolled).unwrap();
        let view = store_log_view(&log);
        assert_eq!(view, old, "правка прежней версии потерялась");
        let j = load_at(&rexon).unwrap();
        assert!(j.items.contains_key("store:777"));
        assert_eq!(j.legacy_seen.store.as_deref(), Some(fingerprint(Some(&rolled)).as_str()));
        assert_eq!(std::fs::read_to_string(&log).unwrap(), rolled, "перенос переписал старый файл");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Писатели пишут общий журнал, а старые файлы — его зеркало ТЕМ ЖЕ
    /// текстом, что писала прежняя логика: журнал магазина —
    /// `serialize_installed_log` карты писателя, карта состояний —
    /// `save_state_text` в порядке каталога расширений. Нетронутый файл не
    /// переписывается.
    #[test]
    fn writers_write_journal_and_mirror_in_old_format() {
        let root = scratch("mirror");
        let (rexon, store_text) = legacy_profile(&root);
        let log = s(&rexon.join(STORE_LOG_NAME));
        let state = rexon.join(EXT_STATE_NAME);
        let state_before = std::fs::read_to_string(&state).unwrap();

        // Покупка магазина.
        let mut data = store_log_view(&log);
        let item = crate::ocs_store::InstalledItem {
            content_id: "777".into(),
            name: "Неон".into(),
            rexon_type: "icons".into(),
            ..Default::default()
        };
        crate::ocs_store::record_installed(&mut data, &item, &[s(&root.join("data/icons/Neon"))], "2026-10-25T10:00:00");
        put_store_log(&log, &data).unwrap();
        let mirror = std::fs::read_to_string(&log).unwrap();
        assert_eq!(mirror, crate::ocs_store::serialize_installed_log(&data), "зеркало — не прежний текст");
        assert_ne!(mirror, store_text);
        let j = parse(std::fs::read_to_string(journal_path(&rexon)).ok().as_deref()).unwrap();
        assert!(j.items.contains_key("store:777") && j.items.contains_key("store:12345"));
        assert_eq!(j.store_order, vec!["12345".to_string(), "777".to_string()]);
        assert_eq!(j.legacy_seen.store.as_deref(), Some(fingerprint(Some(&mirror)).as_str()));
        assert_eq!(std::fs::read_to_string(&state).unwrap(), state_before, "покупка магазина переписала карту состояний");

        // Переключение пакета `.rxn` и второй пакет: порядок — каталога.
        let pkg = rexon.join("extensions/a-first");
        std::fs::create_dir_all(&pkg).unwrap();
        std::fs::write(pkg.join("manifest.json"), r#"{"id": "a-first", "name": "A", "type": "theme-icons"}"#).unwrap();
        set_rxn_enabled_at(&rexon, "a-first", Some(false)).unwrap();
        set_rxn_enabled_at(&rexon, "my-icons", Some(true)).unwrap();
        let want = crate::ext_manager::save_state_text(&[("a-first".into(), false), ("my-icons".into(), true)]);
        assert_eq!(std::fs::read_to_string(&state).unwrap(), want);
        let j = view_at(&rexon);
        assert_eq!(j.items["rxn:my-icons"].enabled, Some(true));
        assert_eq!(j.items["rxn:a-first"].enabled, Some(false));
        assert_eq!(std::fs::read_to_string(&log).unwrap(), mirror, "переключение .rxn переписало журнал магазина");

        // Снятие пакета и покупки.
        std::fs::remove_dir_all(&pkg).unwrap();
        set_rxn_enabled_at(&rexon, "a-first", None).unwrap();
        assert_eq!(std::fs::read_to_string(&state).unwrap(), crate::ext_manager::save_state_text(&[("my-icons".into(), true)]));
        let mut data = store_log_view(&log);
        assert!(crate::ocs_store::forget_installed(&mut data, "777"));
        put_store_log(&log, &data).unwrap();
        assert_eq!(std::fs::read_to_string(&log).unwrap(), crate::ocs_store::serialize_installed_log(&data));
        let j = view_at(&rexon);
        assert!(!j.items.contains_key("store:777") && !j.items.contains_key("rxn:a-first"));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Зеркало не легло (каталог старого файла закрыт на запись): установка
    /// всё равно записана — в общем журнале, — полуфайла нигде нет, старый
    /// файл прежний, а следующая запись дописывает зеркало.
    #[test]
    fn failed_mirror_keeps_journal_and_leaves_no_partial_file() {
        use std::os::unix::fs::PermissionsExt;
        let root = scratch("mirror-fail");
        let (rexon, store_text) = legacy_profile(&root);
        let log = s(&rexon.join(STORE_LOG_NAME));
        let locked = root.join("locked");
        std::fs::create_dir_all(&locked).unwrap();
        std::fs::rename(&log, locked.join(STORE_LOG_NAME)).unwrap();
        std::os::unix::fs::symlink(locked.join(STORE_LOG_NAME), &log).unwrap();
        load_at(&rexon).unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555)).unwrap();

        let mut data = store_log_view(&log);
        crate::ocs_store::forget_installed(&mut data, "12345");
        let res = put_store_log(&log, &data);
        let after_fail = std::fs::read_to_string(&log).unwrap();
        let names = |d: &Path| {
            let mut v: Vec<String> = std::fs::read_dir(d).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
            v.sort();
            v
        };
        let locked_names = names(&locked);
        let rexon_names = names(&rexon);
        let view = store_log_view(&log);
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();

        assert!(res.is_ok(), "сбой зеркала уронил запись: {res:?}");
        assert_eq!(after_fail, store_text, "старый файл испорчен");
        assert_eq!(locked_names, vec![STORE_LOG_NAME], "полуфайл зеркала");
        assert_eq!(rexon_names, vec!["extensions", EXT_STATE_NAME, JOURNAL_NAME, STORE_LOG_NAME], "полуфайл журнала");
        assert!(view.is_empty(), "снятая покупка видна: источник — старый файл, а не журнал");

        // Следующая запись (или чтение писателем) дописывает зеркало.
        load_at(&rexon).unwrap();
        assert_eq!(std::fs::read_to_string(&log).unwrap(), "{}");
        assert!(std::fs::symlink_metadata(&log).unwrap().file_type().is_symlink(), "ссылка на файл заменена файлом");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Нечитаемый журнал откладывается и собирается заново из старых.
    #[test]
    fn broken_journal_is_rebuilt() {
        let root = scratch("broken");
        let (rexon, _) = legacy_profile(&root);
        std::fs::write(journal_path(&rexon), "{не json").unwrap();
        let j = load_at(&rexon).unwrap();
        assert!(j.items.contains_key("store:12345") && j.items.contains_key("rxn:my-icons"));
        assert_eq!(std::fs::read_to_string(rexon.join(format!("{JOURNAL_NAME}.broken"))).unwrap(), "{не json");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Элемент v5 переживает запись и чтение; выбор берётся из правил v5.
    #[test]
    fn v5_element_round_trips() {
        use crate::rxn_manifest::{Accuracy, Choice};
        let (consumer, chosen) = ElementChoice::from_choice(&Choice::Generate {
            consumer: "xfwm4".into(),
            model: "model/decoration.json".into(),
            accuracy: Accuracy::Approximate,
        })
        .unwrap();
        assert!(ElementChoice::from_choice(&Choice::Off).is_none());
        let ext = LegacyExt { id: "v5".into(), name: "V".into(), kind: "theme-gtk".into(), enabled: true, dir: "/c".into() };
        let mut e = rxn_entry(&ext, Vec::new(), true, "t");
        e.elements.push(Element {
            kind: "window-decoration".into(),
            role: None,
            consumer: Some(consumer),
            chosen: Some(chosen),
            paths: vec!["/h/.themes/V".into()],
        });
        let mut j = Journal::default();
        j.items.insert(e.key(), e);
        let text = serialize(&j);
        assert!(text.contains("\"choice\": \"generate\""), "{text}");
        assert_eq!(parse(Some(&text)), Some(j));
    }

    /// Читатель установленного на перенесённых старых данных видит то же,
    /// что раньше видел в `ocs_installed.json`: те же карточки как есть, в
    /// том же порядке (не по алфавиту ключей), с битым байтом — с заменой,
    /// как прежде. Есть старый общий журнал (шаг 3б, без порядка) — то же
    /// самое. Чтение ничего не пишет.
    #[test]
    fn store_view_shows_old_log_as_is_and_writes_nothing() {
        let root = scratch("view");
        let rexon = root.join("config/rexon");
        std::fs::create_dir_all(&rexon).unwrap();
        let log = rexon.join(STORE_LOG_NAME);
        let text = r#"{
  "9": {"name": "Девятая", "rexon_type": "icons", "paths": ["/nope/icons/Nine", ""], "installed_at": "2026-10-01", "tags": ["a"], "score": 5},
  "10": {"name": "Ten", "rexon_type": "gtk", "paths": [], "extra": {"k": [1, 2]}},
  "spices:applet:x@y": {"name": "Spice", "rexon_type": "cinnamon-applets", "paths": ["/nope/applets/x@y"]},
  "1": {"name": "One BAD", "rexon_type": "kvantum", "paths": ["/nope/kv/One"]}
}
"#;
        // Битый байт UTF-8 в имени.
        let mut raw = Vec::new();
        let (head, tail) = text.split_once("BAD").unwrap();
        raw.extend_from_slice(head.as_bytes());
        raw.push(0xff);
        raw.extend_from_slice(tail.as_bytes());
        std::fs::write(&log, &raw).unwrap();
        let old = crate::ocs_store::parse_installed_log(Some(&String::from_utf8_lossy(&raw)));
        assert_eq!(old.len(), 4);

        let view = store_log_view(&s(&log));
        assert_eq!(Value::Object(view.clone()), Value::Object(old.clone()), "карточки — как в журнале магазина");
        assert_eq!(view.keys().collect::<Vec<_>>(), old.keys().collect::<Vec<_>>(), "порядок прежний");
        assert!(!journal_path(&rexon).exists(), "читатель записал общий журнал");
        assert_eq!(store_log_of(&view_at(&rexon)), old, "вид всего каталога конфигов — тот же");
        assert!(!journal_path(&rexon).exists(), "view_at записал общий журнал");

        // Общий журнал шага 3б (без порядка, с прежней записью) — вид тот же.
        let prev = reconcile(None, &Legacy { store_log: Some(String::from_utf8_lossy(&raw).into_owned()), ..Default::default() }, &probe_live);
        let mut text = serialize(&Journal { store_order: Vec::new(), ..prev });
        assert!(!text.contains("store_order"));
        text = text.replace("\"Ten\"", "\"Ten (старое имя)\"");
        std::fs::write(journal_path(&rexon), &text).unwrap();
        assert_eq!(store_log_view(&s(&log)), old, "журнал магазина главнее записи общего");
        assert_eq!(std::fs::read_to_string(journal_path(&rexon)).unwrap(), text, "чтение переписало общий журнал");

        // Нет журнала магазина — пусто, как прежде.
        std::fs::remove_file(&log).unwrap();
        assert!(store_log_view(&s(&log)).is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Ссылка `~/.themes/<тема>`, которую ставит `sync_theme_links` у темы
    /// GTK магазина, записана в журнал видом «ссылка» вслед за темой, и
    /// удаление по журналу снимает её раньше темы. Имя, занятое своей темой
    /// или чужой ссылкой, в журнал не попадает.
    #[test]
    fn store_theme_link_in_dot_themes_is_recorded_as_link() {
        let root = scratch("links");
        let home = root.join("home");
        let xdg = home.join(".local/share/themes");
        for t in ["Nord", "Mine", "Other"] {
            std::fs::create_dir_all(xdg.join(t).join("gtk-3.0")).unwrap();
            std::fs::write(xdg.join(t).join("gtk-3.0/gtk.css"), "x").unwrap();
        }
        // «Mine» в ~/.themes — своя тема человека, «Other» — его же ссылка
        // в другое место.
        std::fs::create_dir_all(home.join(".themes/Mine")).unwrap();
        std::os::unix::fs::symlink("/elsewhere/Other", home.join(".themes/Other")).unwrap();
        let lines = crate::installed_scan::sync_theme_links(&s(&home));
        assert_eq!(lines.len(), 1, "{lines:?}");

        let rexon = home.join(".config/rexon");
        std::fs::create_dir_all(&rexon).unwrap();
        let card = |t: &str| serde_json::json!({"name": t, "rexon_type": "gtk", "paths": [s(&xdg.join(t))]});
        let log = serde_json::json!({"100": card("Nord"), "101": card("Mine"), "102": card("Other")});
        std::fs::write(rexon.join(STORE_LOG_NAME), serde_json::to_string_pretty(&log).unwrap()).unwrap();

        let j = load_at(&rexon).unwrap();
        let link = s(&home.join(".themes/Nord"));
        let nord = &j.items[&key_of(Source::Store, "100")];
        assert_eq!(
            nord.placed.iter().map(|p| (p.path.clone(), p.what)).collect::<Vec<_>>(),
            vec![(s(&xdg.join("Nord")), Some(What::Dir)), (link.clone(), Some(What::Link))]
        );
        for cid in ["101", "102"] {
            assert_eq!(j.items[&key_of(Source::Store, cid)].placed.len(), 1, "{cid}: чужое имя в ~/.themes записано");
        }
        // Записано в файле, а вид для читателей — без ссылки, как журнал магазина.
        let on_disk = parse(std::fs::read_to_string(journal_path(&rexon)).ok().as_deref()).unwrap();
        assert!(on_disk.items[&key_of(Source::Store, "100")].placed.iter().any(|p| p.path == link));
        assert_eq!(Value::Object(store_log_view(&s(&rexon.join(STORE_LOG_NAME)))), log);

        // Удаление по журналу: ссылка — раньше темы.
        let r = removal_plan(nord, &probe_live);
        assert_eq!(
            r.steps,
            vec![
                ExtStep::RemoveFile { path: link.clone() },
                ExtStep::RemoveTree { path: s(&xdg.join("Nord")), safe: true, ignore_errors: false },
            ]
        );
        // Ссылки больше нет (сняли руками) — запись её забывает.
        std::fs::remove_file(&link).unwrap();
        let j = load_at(&rexon).unwrap();
        assert_eq!(j.items[&key_of(Source::Store, "100")].placed.len(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Удаление `.rxn` внутри его плана: выключение — до файлов, перезагрузка
    /// оболочки — после, записанное снимает журнал (и решает: бывший до
    /// установки каталог остаётся), незаписанное — прежний шаг плана; копия
    /// пакета — не здесь. Пакет без путей — `None` (прежний план целиком).
    #[test]
    fn rxn_removal_keeps_plan_around_journal_steps() {
        let root = scratch("merge");
        let p = |x: &str| s(&root.join(x));
        for d in ["aurorae/T", "themes/T", "icons/Mine", "pkgcopy"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        std::fs::write(root.join("themes/T/.rexon-owner"), "pkg\n").unwrap();
        let ext = LegacyExt { id: "pkg".into(), name: "P".into(), kind: "theme-window-decoration".into(), enabled: true, dir: p("pkgcopy") };
        let pl = |path: String, pre: bool, owner: Option<&str>| Placed {
            path,
            what: Some(What::Dir),
            owner: owner.map(str::to_string),
            pre_existed: pre,
            of: None,
        };
        let entry = rxn_entry(
            &ext,
            vec![pl(p("aurorae/T"), false, None), pl(p("themes/T"), false, Some("pkg")), pl(p("icons/Mine"), true, None)],
            true,
            "t",
        );
        let v = |x: &str| x.to_string();
        let off = ExtStep::Run { argv: vec![v("kpackagetool6"), v("-r"), v("T")], timeout_secs: None };
        let reload = ExtStep::Run { argv: vec![v("qdbus6"), v("org.kde.KWin"), v("/KWin"), v("reconfigure")], timeout_secs: None };
        let tree = |path: String| ExtStep::RemoveTree { path, safe: true, ignore_errors: true };
        let plan = vec![
            off.clone(),
            tree(p("aurorae/T")),
            tree(p("icons/Mine/sub")), // внутри бывшего до установки — не записано
            tree(p("cache/T")),        // положила программа — не записано
            tree(p("themes/T")),
            reload.clone(),
        ];
        let r = rxn_removal(&plan, &entry, &probe_live).expect("пути известны");
        let journal_tree = |path: String| ExtStep::RemoveTree { path, safe: true, ignore_errors: false };
        assert_eq!(
            r.steps,
            vec![
                off,
                journal_tree(p("themes/T")),
                journal_tree(p("aurorae/T")),
                tree(p("icons/Mine/sub")),
                tree(p("cache/T")),
                reload,
            ]
        );
        assert_eq!(r.kept, vec![(p("icons/Mine"), KeptWhy::PreExisted)]);
        assert!(!r.steps.iter().any(|st| removal_path(st).as_deref() == Some(p("pkgcopy").as_str())), "копия пакета в плане");

        let old = Entry { placed_known: false, placed: Vec::new(), ..entry };
        assert!(rxn_removal(&plan, &old, &probe_live).is_none(), "пакет до журнала снят не прежним планом");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Ссылка `~/.themes` на тему пакета `.rxn`, легшую в
    /// `~/.local/share/themes`, записывается вслед за темой (как у магазина)
    /// и снимается раньше неё; пропала — уходит из записи.
    #[test]
    fn rxn_theme_link_in_dot_themes_is_recorded() {
        let root = scratch("rxn-link");
        let home = root.join("home");
        let theme = home.join(".local/share/themes/Pkg");
        std::fs::create_dir_all(theme.join("gtk-3.0")).unwrap();
        std::fs::write(theme.join("gtk-3.0/gtk.css"), "x").unwrap();
        assert_eq!(crate::installed_scan::sync_theme_links(&s(&home)).len(), 1);
        let rexon = home.join(".config/rexon");
        let pkg = rexon.join("extensions/pkg");
        std::fs::create_dir_all(&pkg).unwrap();
        std::fs::write(pkg.join("manifest.json"), r#"{"id": "pkg", "name": "P", "type": "theme-gtk", "version": "1"}"#).unwrap();
        let ext = LegacyExt { id: "pkg".into(), name: "P".into(), kind: "theme-gtk".into(), enabled: true, dir: s(&pkg) };
        let placed = vec![Placed { path: s(&theme), what: Some(What::Dir), owner: None, pre_existed: false, of: None }];
        record_rxn_at(&rexon, rxn_entry(&ext, placed, true, "t")).unwrap();

        let link = s(&home.join(".themes/Pkg"));
        let j = load_at(&rexon).unwrap();
        let e = &j.items["rxn:pkg"];
        assert_eq!(
            e.placed.iter().map(|p| (p.path.clone(), p.what, p.of.clone())).collect::<Vec<_>>(),
            vec![(s(&theme), Some(What::Dir), None), (link.clone(), Some(What::Link), Some(s(&theme)))]
        );
        assert_eq!(view_at(&rexon), j, "читатель видит ту же запись");
        let r = rxn_removal(&[], e, &probe_live).unwrap();
        assert_eq!(
            r.steps,
            vec![
                ExtStep::RemoveFile { path: link.clone() },
                ExtStep::RemoveTree { path: s(&theme), safe: true, ignore_errors: false },
            ]
        );
        std::fs::remove_file(&link).unwrap();
        assert_eq!(load_at(&rexon).unwrap().items["rxn:pkg"].placed.len(), 1, "пропавшая ссылка осталась в записи");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// «Удалить» на плитке темы покупки: снимается только эта тема и её
    /// ссылка `~/.themes`, прочие пути покупки — нет; незаписанный путь — `None`.
    #[test]
    fn part_removal_takes_theme_and_its_link_only() {
        let link = Placed { path: "/h/.themes/N".into(), what: Some(What::Link), owner: None, pre_existed: false, of: Some("/h/.local/share/themes/N".into()) };
        let theme = Placed { path: "/h/.local/share/themes/N".into(), what: Some(What::Dir), owner: None, pre_existed: false, of: None };
        let colors = Placed { path: "/h/.local/share/color-schemes/N.colors".into(), what: Some(What::File), owner: None, pre_existed: false, of: None };
        let ext = LegacyExt { id: "1".into(), name: "N".into(), kind: "gtk".into(), enabled: true, dir: "/c".into() };
        let e = Entry {
            source: Source::Store,
            package_copy: None,
            ..rxn_entry(&ext, vec![theme.clone(), link.clone(), colors.clone()], true, "t")
        };
        let probe = |p: &str| if p.ends_with(".colors") { Probe::File } else if p.contains("/.themes/") { Probe::Link } else { Probe::Dir { owner: None } };
        let r = part_removal(&e, "/h/.local/share/themes/N/", &probe).expect("записано");
        assert_eq!(
            r.steps,
            vec![
                ExtStep::RemoveFile { path: link.path.clone() },
                ExtStep::RemoveTree { path: theme.path.clone(), safe: true, ignore_errors: false },
            ]
        );
        assert!(part_removal(&e, "/h/.local/share/themes/Other", &probe).is_none());
        assert!(part_removal(&e, &link.path, &probe).is_some_and(|r| r.steps == vec![ExtStep::RemoveFile { path: link.path.clone() }]));
    }

    /// Всё под `root` побайтно (кроме `skip`): путь, вид, содержимое.
    fn bytes_under(root: &Path, skip: &Path) -> Vec<(String, String)> {
        let mut out = Vec::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).unwrap().flatten() {
                let p = e.path();
                if p.starts_with(skip) {
                    continue;
                }
                let md = std::fs::symlink_metadata(&p).unwrap();
                let what = if md.file_type().is_symlink() {
                    format!("link → {}", std::fs::read_link(&p).unwrap().display())
                } else if md.is_dir() {
                    stack.push(p.clone());
                    "dir".to_string()
                } else {
                    format!("file {:?}", std::fs::read(&p).unwrap())
                };
                out.push((p.strip_prefix(root).unwrap().to_string_lossy().into_owned(), what));
            }
        }
        out.sort();
        out
    }

    /// Шаг 5а: нажатие «Применить» общим планом → снимок «что было» в
    /// запись `applied` общего журнала → «вернуть как было» по ней: конфиги
    /// побайтно прежние, запись и снимок сняты. Второе нажатие в том же
    /// разделе не затирает раннее «что было».
    #[test]
    fn applied_press_snapshot_and_revert() {
        use crate::install_plan::{exec_undoable, Ctx, Step, Undo};
        use crate::themes::ApplyStep;
        let dir = scratch("applied");
        let home = dir.join("home");
        let cfg = home.join(".config");
        let rexon = cfg.join("rexon");
        std::fs::create_dir_all(cfg.join("gtk-3.0")).unwrap();
        std::fs::write(cfg.join("gtk-3.0/settings.ini"), "[Settings]\ngtk-icon-theme-name=Old\n").unwrap();
        std::fs::write(cfg.join("kdeglobals"), "[Icons]\nTheme=Old\n").unwrap();
        let before = bytes_under(&home, &rexon);

        let no_run = |_: &[String], _: Option<u64>| crate::utils::RunReport::default();
        let no = |_: &str| false;
        let no_spawn = |_: &[String]| {};
        let ctx = Ctx { install: true, steamos: false, privileged: None, run: &no_run, have: &no, spawn: &no_spawn, wayland: false, sync_env: &crate::install_plan::no_sync_env };
        let press = |theme: &str| {
            let c = |p: &str| s(&cfg.join(p));
            let steps: Vec<Step> = vec![
                ApplyStep::ForceOk,
                ApplyStep::GtkIni { path: c("gtk-3.0/settings.ini"), key: "gtk-icon-theme-name".into(), value: theme.into() },
                ApplyStep::GtkIni { path: c("gtk-4.0/settings.ini"), key: "gtk-icon-theme-name".into(), value: theme.into() },
                ApplyStep::IniSetIfOk { path: c("kdeglobals"), section: "[Icons]".into(), key: "Theme".into(), value: theme.into() },
            ]
            .into_iter()
            .map(Step::from)
            .collect();
            let mut undo = Undo::new();
            let report = exec_undoable(&steps, &mut Vec::new(), &ctx, &mut undo).expect("нажатие удалось");
            assert!(report.apply.ok);
            let snap = applied_snapshot_dir(&rexon, "icons");
            let first = std::fs::read_dir(&snap).map(|r| r.count() + 1).unwrap_or(1);
            let touched = undo.keep_into(&snap, first);
            record_applied_at(
                &rexon,
                Applied {
                    section: "icons".into(),
                    theme: theme.into(),
                    item: None,
                    applied_at: None,
                    snapshot: s(&snap),
                    files: touched
                        .iter()
                        .map(|t| AppliedFile { path: s(&t.path), before: t.before.as_deref().map(s), dir: t.dir })
                        .collect(),
                },
            )
            .unwrap();
        };
        press("First");
        press("Second");
        assert!(std::fs::read_to_string(cfg.join("kdeglobals")).unwrap().contains("Theme=Second"));
        let j = view_at(&rexon);
        let rec = &j.applied["icons"];
        assert_eq!(rec.theme, "Second");
        let paths: Vec<&str> = rec.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(
            paths,
            [s(&cfg.join("gtk-3.0/settings.ini")), s(&cfg.join("gtk-4.0")), s(&cfg.join("gtk-4.0/settings.ini")), s(&cfg.join("kdeglobals"))],
            "пути записаны не по разу или не в порядке записи"
        );
        // Ранний снимок: у settings.ini GTK 3 — содержимое до ПЕРВОГО нажатия.
        let copy = rec.files[0].before.clone().expect("копия прежнего");
        assert_eq!(std::fs::read_to_string(copy).unwrap(), "[Settings]\ngtk-icon-theme-name=Old\n");
        assert_eq!(std::fs::read_dir(applied_snapshot_dir(&rexon, "icons")).unwrap().count(), 2, "лишние копии второго нажатия остались");
        // Другие записи журнала «Применить» не трогает и сверка их не теряет.
        assert!(j.items.is_empty());

        assert_eq!(revert_applied_at(&rexon, "icons").unwrap(), Some(Vec::new()));
        assert_eq!(bytes_under(&home, &rexon), before, "«вернуть как было» оставило конфиги не прежними");
        assert!(!applied_snapshot_dir(&rexon, "icons").exists(), "снимок остался");
        assert!(view_at(&rexon).applied.is_empty(), "запись осталась");
        assert_eq!(revert_applied_at(&rexon, "icons").unwrap(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// «Применить» меняет значения стола (`gsettings`, `dconf`, `xfconf`) —
    /// прежние ложатся рядом с записью раздела, «вернуть как было»
    /// возвращает их: заданное — прежним значением, не заданное — сбросом.
    /// Второе нажатие раннее «что было» не затирает. Команды — подменный
    /// стол, настоящие программы не запускаются.
    #[test]
    fn applied_press_values_snapshot_and_revert() {
        use crate::install_plan::tests::{v, FakeDesk};
        use crate::install_plan::{exec_undoable, Ctx, Step, Undo};
        use crate::themes::{ApplyStep, Cmd};
        let dir = scratch("applied-values");
        let rexon = dir.join("rexon");
        let desk = FakeDesk {
            defaults: [
                ("org.a gtk-theme".to_string(), "'Adwaita'".to_string()),
                ("org.a icon-theme".to_string(), "'Adwaita'".to_string()),
            ]
            .into(),
            ..Default::default()
        };
        desk.gsettings.borrow_mut().insert("org.a gtk-theme".into(), "'Old'".into());
        desk.xfconf.borrow_mut().insert("xsettings /Net/ThemeName".into(), "Greybird".into());
        let before = desk.state();
        let run = |a: &[String], _: Option<u64>| desk.run(a);
        let no = |_: &str| false;
        let no_spawn = |_: &[String]| {};
        let ctx = Ctx { install: true, steamos: false, privileged: None, run: &run, have: &no, spawn: &no_spawn, wayland: false, sync_env: &crate::install_plan::no_sync_env };
        let cmd = |a: &[&str]| Cmd { argv: v(a), timeout: None };
        let press = |theme: &str| {
            let q = format!("'{theme}'");
            let steps: Vec<Step> = vec![
                ApplyStep::Primary(cmd(&["gsettings", "set", "org.a", "gtk-theme", &q])).into(),
                ApplyStep::BestEffort(cmd(&["gsettings", "set", "org.a", "icon-theme", &q])).into(),
                // Тот же ключ второй раз — снимок один, первый.
                ApplyStep::BestEffort(cmd(&["gsettings", "set", "org.a", "gtk-theme", &q])).into(),
                // Схемы нет — запись не удалась, снимать нечего.
                ApplyStep::BestEffort(cmd(&["gsettings", "set", "org.nope", "k", &q])).into(),
                ExtStep::Run { argv: v(&["dconf", "write", "/org/mate/marco/general/theme", &q]), timeout_secs: None }.into(),
                ApplyStep::IfOk(cmd(&["xfconf-query", "-c", "xsettings", "-p", "/Net/ThemeName", "-s", theme])).into(),
                ApplyStep::IfOk(cmd(&["xfconf-query", "-c", "xsettings", "-p", "/Net/IconThemeName", "-n", "-t", "string", "-s", theme])).into(),
            ];
            let mut undo = Undo::new().keeping_values();
            let report = exec_undoable(&steps, &mut Vec::new(), &ctx, &mut undo).expect("нажатие удалось");
            assert!(report.apply.ok);
            let values = undo.take_values();
            let snap = applied_snapshot_dir(&rexon, "gtk");
            let touched = undo.keep_into(&snap, 1);
            assert!(touched.is_empty());
            let rec = Applied { section: "gtk".into(), theme: theme.into(), item: None, applied_at: None, snapshot: s(&snap), files: Vec::new() };
            record_applied_values_at(&rexon, rec, values.into_iter().map(AppliedValue::from).collect()).unwrap();
        };
        press("First");
        press("Second");
        assert_eq!(desk.gsettings.borrow()["org.a gtk-theme"], "'Second'");
        let j = view_at(&rexon);
        assert_eq!(j.applied["gtk"].theme, "Second");
        let got: Vec<(String, Vec<String>)> = j.applied_values["gtk"].iter().map(|v| (v.key.clone(), v.restore.clone())).collect();
        assert_eq!(
            got,
            [
                ("gsettings org.a gtk-theme".to_string(), v(&["gsettings", "set", "org.a", "gtk-theme", "'Old'"])),
                ("gsettings org.a icon-theme".to_string(), v(&["gsettings", "reset", "org.a", "icon-theme"])),
                ("dconf /org/mate/marco/general/theme".to_string(), v(&["dconf", "reset", "/org/mate/marco/general/theme"])),
                ("xfconf xsettings /Net/ThemeName".to_string(), v(&["xfconf-query", "-c", "xsettings", "-p", "/Net/ThemeName", "-s", "Greybird"])),
                ("xfconf xsettings /Net/IconThemeName".to_string(), v(&["xfconf-query", "-c", "xsettings", "-p", "/Net/IconThemeName", "-r"])),
            ],
            "прежние значения не те или не по разу"
        );

        // Возврат, который не удался, — запись и значения остаются.
        let refuse = |_: &[String], _: Option<u64>| crate::utils::RunReport { returncode: Some(1), ..Default::default() };
        let left = revert_applied_with(&rexon, "gtk", &|_| false, &refuse).unwrap().unwrap();
        assert_eq!(left.len(), 5, "{left:?}");
        assert!(view_at(&rexon).applied_values.contains_key("gtk"));

        assert_eq!(revert_applied_with(&rexon, "gtk", &|_| false, &run).unwrap(), Some(Vec::new()));
        assert_eq!(desk.state(), before, "«вернуть как было» не вернуло значения стола");
        let j = view_at(&rexon);
        assert!(j.applied.is_empty() && j.applied_values.is_empty(), "запись осталась после возврата");
        assert_eq!(revert_applied_with(&rexon, "gtk", &|_| false, &run).unwrap(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Журнал прежнего формата (без значений стола) читается как прежде, а
    /// записи без значений не пишут лишнего поля.
    #[test]
    fn applied_values_keep_old_journals() {
        let old = r#"{"format":1,"applied":{"icons":{"section":"icons","theme":"T","snapshot":"/s","files":[{"path":"/p","before":"/s/0001"}]}}}"#;
        let j: Journal = serde_json::from_str(old).unwrap();
        assert_eq!(j.applied["icons"].files.len(), 1);
        assert!(j.applied_values.is_empty());
        assert!(!serde_json::to_string(&j).unwrap().contains("applied_values"));
    }
}
