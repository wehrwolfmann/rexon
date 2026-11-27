//! extensions.gnome.org (EGO) — магазин раздела «Расширения GNOME».
//!
//! Свой клиент по официальной схеме API v1 сайта (OpenAPI 3:
//! <https://extensions.gnome.org/api/schema/?format=json>, страница
//! <https://extensions.gnome.org/api/docs/>) и собственным замерам ответов
//! (08.11 и 09.11.2026). Маршруты:
//!
//! * листинг `GET /api/v1/extensions/` — `ordering`, `page`, `page_size`,
//!   `status`, `shell_version`, `uuid` (можно много раз);
//! * поиск `GET /api/v1/extensions/search/{строка}/` — те же, кроме
//!   `status` и `uuid` (у маршрута их нет);
//! * карточка `GET /api/v1/extensions/{uuid}/`;
//! * выпуски `GET /api/v1/extensions/{uuid}/versions/` (от старых к новым,
//!   страницами), один выпуск — `…/versions/{N}/?format=json|zip`.
//!
//! Что здесь живёт и почему так:
//!
//! * **Ставит не Rexon, а GNOME.** Карточка — обычный [`ThemeItem`]
//!   категории 914 с ПУСТЫМИ `download_links`: установку просит оболочка
//!   (`InstallRemoteExtension`), она сама берёт с сайта сборку под себя.
//!   Адрес zip выпуска ([`release_zip_url`]) — только для запасного пути,
//!   когда оболочки нет.
//! * **Только одобренное.** Листинг уходит с `status=3`; у выпусков
//!   «версия для вас» выбирается только среди активных ([`STATUS_ACTIVE`]).
//! * **Совместимость считает сервер.** С фильтром «только для вашей версии»
//!   в запрос добавляется `shell_version` в виде сайта
//!   ([`site_shell_version`]); без фильтра несовместимые помечаются вторым
//!   запросом — листингом по пачке uuid ([`compat_batch_urls`], до
//!   [`UUID_BATCH`] за раз).
//! * **Вежливость.** Опубликованного лимита у сайта нет, но одна страница —
//!   один запрос, ответы держатся в памяти сессии ([`Gate`]: листинги
//!   [`LIST_TTL_SECS`], карточки и выпуски [`DETAIL_TTL_SECS`]), после ответа
//!   5xx сайт не трогаем [`FAIL_BACKOFF_SECS`] (кроме «Обновить»).
//! * **Картинки — только с самого сайта** по https ([`ego_https`]):
//!   скриншот и значок приходят адресом из ответа и уходят в загрузчик
//!   картинок как есть. Описания и скриншоты принадлежат авторам — Rexon их
//!   только показывает.

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::sync::{Mutex, OnceLock};

use serde::Deserialize;
use serde_json::Value;

use crate::i18n::{n_, tr, trf};
use crate::models::ThemeItem;
use crate::net::NetError;

/// Адрес сайта.
pub const HOME_URL: &str = "https://extensions.gnome.org";
/// Корень API v1.
pub const API_BASE: &str = "https://extensions.gnome.org/api/v1";
/// Ключ источника в переключателе «Магазин:».
pub const SOURCE_KEY: &str = "ego";
/// Префикс id карточки: `ego:<uuid>`.
pub const ID_PREFIX: &str = "ego:";
/// Поставщик в журнале установленного.
pub const PROVIDER_ID: &str = "ego";
/// Разделы, где EGO — магазин.
pub const SECTIONS: &[&str] = &["gnomeext"];
/// Категория раздела «Расширения GNOME» (`categories::CATEGORIES`).
pub const CATEGORY_ID: i64 = 914;
/// Карточек на страницу витрины — общий размер страницы вкладки. Сервер
/// принимает любой `page_size`, поэтому страница Rexon = страница сервера.
pub const PAGE_SIZE: i64 = crate::appearance_page::PAGE_SIZE as i64;
/// Выпусков на страницу при листании версий (сервер отдаёт до 100).
pub const VERSIONS_PAGE_SIZE: i64 = 100;
/// Больше стольких страниц выпусков не листаем (тысяча выпусков — с
/// огромным запасом: у самых старых расширений их около ста).
pub const VERSIONS_MAX_PAGES: usize = 10;
/// Сколько uuid спрашивать одним листингом при сверке совместимости.
pub const UUID_BATCH: usize = 50;
/// `status` одобренного (активного) выпуска: 0 — не проверен, 1 —
/// отклонён, 2 — неактивен, 3 — активен, 4 — ждёт автора.
pub const STATUS_ACTIVE: i64 = 3;
/// Сколько живёт в памяти ответ листинга или поиска.
pub const LIST_TTL_SECS: u64 = 10 * 60;
/// Сколько живут в памяти карточка и выпуски.
pub const DETAIL_TTL_SECS: u64 = 60 * 60;
/// Сколько не трогать сайт после ответа 5xx (кроме «Обновить»).
pub const FAIL_BACKOFF_SECS: u64 = 2 * 60;
/// Сколько ответов держать в памяти (старые вытесняются).
pub const CACHE_MAX_ENTRIES: usize = 96;
/// Таймаут запроса, секунды.
pub const TIMEOUT_SECS: u64 = 15;
/// Больше этого ответ не читаем (страница из 100 выпусков — ~40 КБ).
pub const MAX_RESPONSE_BYTES: usize = 2 << 20;

/// Есть ли EGO в разделе `section`.
pub fn has_section(section: &str) -> bool {
    SECTIONS.contains(&section)
}

/// Карточка отсюда?
pub fn is_item_id(id: &str) -> bool {
    id.starts_with(ID_PREFIX)
}

/// Подпись чипа «Магазин:».
pub fn chip_label() -> String {
    "extensions.gnome.org".to_string()
}

/// Подсказка чипа.
pub fn chip_tooltip() -> String {
    tr("The official GNOME extensions site. GNOME itself installs the extension you pick \
        and asks you to confirm.")
}

/// Подсказка строки поиска.
pub fn search_placeholder() -> String {
    tr("🔍 Search GNOME extensions…")
}

// ═══════════════════════════════════════════════════════════════════════
//  Сортировки
// ═══════════════════════════════════════════════════════════════════════

/// Сортировки EGO: `(подпись, ключ)`. «Новые» и «Загрузки» — те же ключи,
/// что у KDE Store (`appearance_page::SORTS`), чтобы выбор переживал смену
/// магазина.
pub const SORTS: &[(&str, &str)] = &[
    (n_("🔥 Popular"), "popular"),
    (n_("⬇️ Most Downloaded"), "down"),
    (n_("🔄 Recently Updated"), "updated"),
    (n_("🆕 Newest"), "new"),
];

/// Ключ сортировки, который действует: незнакомый EGO («Лучшие», «По
/// алфавиту» у KDE Store) — «Популярные», как витрина самого сайта.
pub fn effective_sort(sort: &str) -> &'static str {
    SORTS.iter().map(|(_, k)| *k).find(|k| *k == sort).unwrap_or("popular")
}

/// Параметр `ordering` для ключа Rexon.
pub fn api_ordering(sort: &str) -> &'static str {
    match effective_sort(sort) {
        "down" => "-downloads",
        "updated" => "-updated",
        "new" => "-created",
        _ => "-popularity",
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Версия оболочки
// ═══════════════════════════════════════════════════════════════════════

/// Версия GNOME Shell так, как её считает сайт: с GNOME 40 — главный номер
/// (`51.1` → 51), у 3.x — пара (`3.38.4` → 3.38). Предвыпуск (`51.beta`,
/// `51.alpha.2`, `51.rc`) помнится словом: сайт принимает `51.beta`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellVersion {
    pub major: i64,
    /// Только у 3.x.
    pub minor: Option<i64>,
    /// `alpha` / `beta` / `rc`, если оболочка — предвыпуск.
    pub pre: Option<String>,
}

/// Разобрать `ShellVersion` (свойство D-Bus или `gnome-shell --version`:
/// «GNOME Shell 51.1» тоже годится). Мусор — `None`.
pub fn parse_shell_version(raw: &str) -> Option<ShellVersion> {
    let s = raw.trim();
    let s = s.rsplit(' ').next().unwrap_or(s);
    let mut parts = s.split('.');
    let major: i64 = parts.next()?.parse().ok()?;
    if major <= 0 {
        return None;
    }
    let rest: Vec<&str> = parts.collect();
    let pre_of = |p: &str| {
        let low = p.to_ascii_lowercase();
        ["alpha", "beta", "rc"].contains(&low.as_str()).then_some(low)
    };
    if major == 3 {
        let minor: i64 = rest.first()?.parse().ok()?;
        return Some(ShellVersion { major, minor: Some(minor), pre: None });
    }
    if major < 40 {
        return None;
    }
    let pre = rest.first().and_then(|p| pre_of(p));
    if pre.is_none() {
        if let Some(p) = rest.first() {
            p.parse::<i64>().ok()?;
        }
    }
    Some(ShellVersion { major, minor: None, pre })
}

/// Значение параметра `shell_version` для сайта: `51`, `51.beta`, `3.38`.
pub fn site_shell_version(v: &ShellVersion) -> String {
    match (&v.minor, &v.pre) {
        (Some(minor), _) => format!("{}.{minor}", v.major),
        (None, Some(pre)) => format!("{}.{pre}", v.major),
        (None, None) => v.major.to_string(),
    }
}

/// Подпись версии для человека: «51», «3.38» (предвыпуск — без слова: в
/// подписях «Не поддерживает GNOME 51» оно только мешает).
pub fn shell_label(v: &ShellVersion) -> String {
    match v.minor {
        Some(minor) => format!("{}.{minor}", v.major),
        None => v.major.to_string(),
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Адреса
// ═══════════════════════════════════════════════════════════════════════

/// Кодирование значения параметра запроса и куска пути.
fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Похоже ли на uuid расширения (`dash-to-dock@micxgx.gmail.com`). Из чужого
/// ответа и из id карточки в адрес попадает только такое.
pub fn valid_uuid(uuid: &str) -> bool {
    !uuid.is_empty()
        && uuid.len() <= 200
        && !uuid.starts_with('.')
        && !uuid.contains("..")
        && uuid.bytes().all(|b| b.is_ascii_alphanumeric() || b"._@+-".contains(&b))
}

/// Адрес страницы витрины. `page` — с нуля (как у вкладки), `shell` —
/// значение [`site_shell_version`] или `None` («показать все»). Пустой
/// запрос — листинг одобренного, иначе маршрут поиска.
pub fn listing_url(query: &str, sort: &str, page: i64, size: i64, shell: Option<&str>) -> String {
    // Косая черта в куске пути сервер путает с разделителем — заменяем
    // пробелом: искать по ней смысла нет.
    let q = query.replace('/', " ");
    let q = q.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut url = if q.is_empty() {
        format!("{API_BASE}/extensions/?status={STATUS_ACTIVE}&")
    } else {
        format!("{API_BASE}/extensions/search/{}/?", quote(&q))
    };
    url.push_str(&format!(
        "ordering={}&page={}&page_size={}",
        api_ordering(sort),
        page.max(0) + 1,
        size.clamp(1, 100)
    ));
    if let Some(s) = shell.filter(|s| !s.is_empty()) {
        url.push_str("&shell_version=");
        url.push_str(&quote(s));
    }
    url
}

/// Адреса сверки совместимости: листинги по пачкам uuid (не больше
/// [`UUID_BATCH`] в каждом) с тем же `shell_version`. Ответ каждого —
/// те из пачки, что подходят. Негодные uuid и повторы выпадают.
pub fn compat_batch_urls(uuids: &[&str], shell: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    let clean: Vec<&str> = uuids.iter().copied().filter(|u| valid_uuid(u) && seen.insert(*u)).collect();
    clean
        .chunks(UUID_BATCH)
        .map(|chunk| {
            let mut url = format!("{API_BASE}/extensions/?page_size={UUID_BATCH}&shell_version={}", quote(shell));
            for u in chunk {
                url.push_str("&uuid=");
                url.push_str(&quote(u));
            }
            url
        })
        .collect()
}

/// Адрес карточки.
pub fn detail_url(uuid: &str) -> Option<String> {
    valid_uuid(uuid).then(|| format!("{API_BASE}/extensions/{}/", quote(uuid)))
}

/// Адрес страницы выпусков (`page` — с единицы, как у сервера).
pub fn versions_url(uuid: &str, page: i64) -> Option<String> {
    valid_uuid(uuid).then(|| {
        format!(
            "{API_BASE}/extensions/{}/versions/?page={}&page_size={VERSIONS_PAGE_SIZE}",
            quote(uuid),
            page.max(1)
        )
    })
}

/// Адрес архива выпуска — только для запасного пути без оболочки.
pub fn release_zip_url(uuid: &str, version: i64) -> Option<String> {
    (valid_uuid(uuid) && version > 0)
        .then(|| format!("{API_BASE}/extensions/{}/versions/{version}/?format=zip", quote(uuid)))
}

/// id карточки.
pub fn item_id(uuid: &str) -> String {
    format!("{ID_PREFIX}{uuid}")
}

/// uuid из id карточки.
pub fn parse_item_id(id: &str) -> Option<&str> {
    id.strip_prefix(ID_PREFIX).filter(|u| valid_uuid(u))
}

/// Адрес только с самого сайта по https — иначе пусто.
pub fn ego_https(url: &str) -> String {
    let url = url.trim();
    if url.starts_with("https://extensions.gnome.org/") && !url.contains("..") && !url.contains(char::is_whitespace)
    {
        url.to_string()
    } else {
        String::new()
    }
}

/// Внешняя ссылка (домашняя страница, пожертвования): только http(s).
fn web_link(url: &str) -> String {
    let url = url.trim();
    if (url.starts_with("https://") || url.starts_with("http://")) && !url.contains(char::is_whitespace) {
        url.to_string()
    } else {
        String::new()
    }
}

/// Страница расширения на сайте: из `link` ответа (`/extension/9419/x/`),
/// если он похож на путь сайта, иначе из id.
pub fn page_url(e: &Extension) -> String {
    let link = e.link.trim();
    let safe = link.starts_with("/extension/")
        && !link.contains("..")
        && link.bytes().all(|b| b.is_ascii_alphanumeric() || b"/-_.".contains(&b));
    if safe {
        format!("{HOME_URL}{link}")
    } else if e.id > 0 {
        format!("{HOME_URL}/extension/{}/", e.id)
    } else {
        HOME_URL.to_string()
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Разбор ответов
// ═══════════════════════════════════════════════════════════════════════

/// `null` и отсутствие поля — одно и то же: значение по умолчанию.
fn nullable<'de, D, T>(d: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(d)?.unwrap_or_default())
}

/// Целое из ответа: число (в том числе дробное) или строка; прочее — 0.
fn lenient_i64<'de, D: serde::Deserializer<'de>>(d: D) -> Result<i64, D::Error> {
    Ok(match Value::deserialize(d)? {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)).unwrap_or(0),
        Value::String(s) => s.trim().parse().unwrap_or(0),
        _ => 0,
    })
}

/// Дробное из ответа; прочее — 0.
fn lenient_f64<'de, D: serde::Deserializer<'de>>(d: D) -> Result<f64, D::Error> {
    Ok(match Value::deserialize(d)? {
        Value::Number(n) => n.as_f64().unwrap_or(0.0),
        Value::String(s) => s.trim().parse().unwrap_or(0.0),
        _ => 0.0,
    })
}

/// Список строк; `null`, не-строки внутри и не-список — терпим.
fn lenient_strings<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    Ok(match Value::deserialize(d)? {
        Value::Array(a) => a.into_iter().filter_map(|v| v.as_str().map(str::to_string)).collect(),
        _ => Vec::new(),
    })
}

/// Автор расширения.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Creator {
    #[serde(deserialize_with = "lenient_i64")]
    pub id: i64,
    #[serde(deserialize_with = "nullable")]
    pub username: String,
    #[serde(deserialize_with = "nullable")]
    pub display_name: String,
}

/// Расширение из листинга, поиска или карточки (поля у всех трёх одни).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Extension {
    #[serde(deserialize_with = "lenient_i64")]
    pub id: i64,
    #[serde(deserialize_with = "nullable")]
    pub uuid: String,
    #[serde(deserialize_with = "nullable")]
    pub name: String,
    #[serde(deserialize_with = "nullable")]
    pub creator: Creator,
    #[serde(deserialize_with = "nullable")]
    pub description: String,
    #[serde(deserialize_with = "nullable")]
    pub created: String,
    #[serde(deserialize_with = "nullable")]
    pub updated: String,
    #[serde(deserialize_with = "lenient_i64")]
    pub downloads: i64,
    #[serde(deserialize_with = "lenient_i64")]
    pub popularity: i64,
    #[serde(deserialize_with = "nullable")]
    pub screenshot: String,
    #[serde(deserialize_with = "nullable")]
    pub icon: String,
    /// Средняя оценка 0…5.
    #[serde(deserialize_with = "lenient_f64")]
    pub rating: f64,
    /// Число голосов.
    #[serde(deserialize_with = "lenient_i64")]
    pub rated: i64,
    /// Домашняя страница автора.
    #[serde(deserialize_with = "nullable")]
    pub url: String,
    #[serde(deserialize_with = "lenient_strings")]
    pub donation_urls: Vec<String>,
    /// Путь страницы на сайте.
    #[serde(deserialize_with = "nullable")]
    pub link: String,
}

/// Версия GNOME в выпуске: `minor`/`point` = −1 — «любая».
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct ReleaseShell {
    #[serde(deserialize_with = "lenient_i64")]
    pub major: i64,
    #[serde(deserialize_with = "lenient_i64")]
    pub minor: i64,
    #[serde(deserialize_with = "lenient_i64")]
    pub point: i64,
}

impl ReleaseShell {
    /// Подходит ли оболочке: с 40 — по главному номеру, у 3.x — по паре.
    pub fn matches(&self, v: &ShellVersion) -> bool {
        if self.major != v.major {
            return false;
        }
        match v.minor {
            Some(minor) => self.minor == minor || self.minor < 0,
            None => true,
        }
    }
}

/// Выпуск расширения.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Release {
    #[serde(deserialize_with = "lenient_i64")]
    pub version: i64,
    #[serde(deserialize_with = "nullable")]
    pub version_name: String,
    #[serde(deserialize_with = "lenient_i64")]
    pub status: i64,
    #[serde(deserialize_with = "nullable")]
    pub shell_versions: Vec<ReleaseShell>,
    #[serde(deserialize_with = "nullable")]
    pub created: String,
    #[serde(deserialize_with = "lenient_strings")]
    pub session_modes: Vec<String>,
}

impl Release {
    /// Подпись выпуска: имя автора («5.1.0»), иначе номер сайта.
    pub fn label(&self) -> String {
        let name = self.version_name.trim();
        if name.is_empty() {
            self.version.to_string()
        } else {
            name.to_string()
        }
    }

    /// Подходит ли оболочке.
    pub fn supports(&self, v: &ShellVersion) -> bool {
        self.shell_versions.iter().any(|s| s.matches(v))
    }
}

/// Страница ответа сервера: `{count, next, previous, results}`.
#[derive(Deserialize)]
struct PageBody {
    #[serde(default, deserialize_with = "lenient_i64")]
    count: i64,
    #[serde(default)]
    next: Option<String>,
    results: Vec<Value>,
}

/// Страница расширений.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExtPage {
    pub items: Vec<Extension>,
    /// Сколько всего по запросу.
    pub total: i64,
    /// Есть ли следующая страница.
    pub has_more: bool,
}

/// Страница выпусков.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ReleasePage {
    pub items: Vec<Release>,
    pub total: i64,
    pub has_more: bool,
}

fn parse_page(body: &[u8]) -> Result<PageBody, String> {
    serde_json::from_slice(body).map_err(|e| format!("EGO: {e}"))
}

/// Разобрать листинг или поиск. Записи без годного uuid выпадают по одной,
/// не роняя страницу.
pub fn parse_listing(body: &[u8]) -> Result<ExtPage, String> {
    let p = parse_page(body)?;
    let items = p
        .results
        .into_iter()
        .filter_map(|v| serde_json::from_value::<Extension>(v).ok())
        .filter(|e| valid_uuid(&e.uuid))
        .collect();
    Ok(ExtPage { items, total: p.count, has_more: p.next.is_some_and(|n| !n.is_empty()) })
}

/// Разобрать карточку.
pub fn parse_detail(body: &[u8]) -> Result<Extension, String> {
    let e: Extension = serde_json::from_slice(body).map_err(|e| format!("EGO: {e}"))?;
    if !valid_uuid(&e.uuid) {
        return Err("EGO: extension without uuid".to_string());
    }
    Ok(e)
}

/// Разобрать страницу выпусков. Выпуски без номера выпадают.
pub fn parse_versions(body: &[u8]) -> Result<ReleasePage, String> {
    let p = parse_page(body)?;
    let items = p
        .results
        .into_iter()
        .filter_map(|v| serde_json::from_value::<Release>(v).ok())
        .filter(|r| r.version > 0)
        .collect();
    Ok(ReleasePage { items, total: p.count, has_more: p.next.is_some_and(|n| !n.is_empty()) })
}

// ═══════════════════════════════════════════════════════════════════════
//  Совместимость и версии
// ═══════════════════════════════════════════════════════════════════════

/// «Версия для вас»: самый новый АКТИВНЫЙ выпуск, где заявлена текущая
/// оболочка. Нет такого — `None` (ставить можно, но с предупреждением).
pub fn release_for<'a>(releases: &'a [Release], v: &ShellVersion) -> Option<&'a Release> {
    releases
        .iter()
        .filter(|r| r.status == STATUS_ACTIVE && r.supports(v))
        .max_by_key(|r| r.version)
}

/// Место версии GNOME в общем ряду выпусков: …3.36, 3.38, 40, 41… идут
/// подряд. Нечётные 3.x (разработческие) соседей не имеют.
fn shell_position(s: &ReleaseShell) -> Option<i64> {
    match s.major {
        3 if s.minor >= 0 && s.minor % 2 == 0 => Some(s.minor / 2),
        m if m >= 40 => Some(m - 20),
        _ => None,
    }
}

fn shell_text(s: &ReleaseShell) -> String {
    if s.major == 3 && s.minor >= 0 {
        format!("3.{}", s.minor)
    } else {
        s.major.to_string()
    }
}

/// Список версий GNOME, заявленных активными выпусками (нет активных —
/// всеми), сжатый в промежутки: «3.36–41, 45, 47–50».
pub fn supported_summary(releases: &[Release]) -> String {
    let active: Vec<&Release> = releases.iter().filter(|r| r.status == STATUS_ACTIVE).collect();
    let pool: Vec<&Release> = if active.is_empty() { releases.iter().collect() } else { active };
    // (порядок, позиция в ряду, текст): 3.x раньше 40+.
    let mut set: BTreeSet<(i64, i64, Option<i64>, String)> = BTreeSet::new();
    for r in pool {
        for s in &r.shell_versions {
            if s.major < 3 || (s.major > 3 && s.major < 40) {
                continue;
            }
            let minor_key = if s.major == 3 { s.minor.max(0) } else { 0 };
            set.insert((s.major, minor_key, shell_position(s), shell_text(s)));
        }
    }
    let mut parts: Vec<String> = Vec::new();
    let mut run: Option<(String, String, i64)> = None; // (начало, конец, позиция конца)
    for (_, _, pos, text) in set {
        match (&mut run, pos) {
            (Some((_, end, last)), Some(p)) if p == *last + 1 => {
                *end = text;
                *last = p;
            }
            _ => {
                if let Some((a, b, _)) = run.take() {
                    parts.push(if a == b { a } else { format!("{a}–{b}") });
                }
                match pos {
                    Some(p) => run = Some((text.clone(), text, p)),
                    None => parts.push(text),
                }
            }
        }
    }
    if let Some((a, b, _)) = run {
        parts.push(if a == b { a } else { format!("{a}–{b}") });
    }
    parts.join(", ")
}

/// Строки диалога «Поддерживаемые версии GNOME»: от новых к старым, у
/// каждой — подпись выпуска, его версии GNOME и отметки «для вас» и
/// «установлено» (`installed` — номер сайта из сведений оболочки).
pub fn release_rows(releases: &[Release], v: Option<&ShellVersion>, installed: Option<i64>) -> Vec<Value> {
    let chosen = v.and_then(|v| release_for(releases, v)).map(|r| r.version);
    let mut rows: Vec<&Release> = releases.iter().filter(|r| r.status == STATUS_ACTIVE).collect();
    rows.sort_by_key(|r| std::cmp::Reverse(r.version));
    rows.into_iter()
        .map(|r| {
            serde_json::json!({
                "version": r.version,
                "label": r.label(),
                "shells": supported_summary(std::slice::from_ref(r)),
                "forYou": Some(r.version) == chosen,
                "installed": Some(r.version) == installed,
            })
        })
        .collect()
}

/// Какие из показанных uuid НЕ подходят: те, кого нет в ответах сверки.
pub fn unsupported_uuids(shown: &[&str], compatible: &HashSet<String>) -> HashSet<String> {
    shown.iter().filter(|u| !compatible.contains(**u)).map(|u| u.to_string()).collect()
}

// ═══════════════════════════════════════════════════════════════════════
//  Подписи
// ═══════════════════════════════════════════════════════════════════════

/// Переключатель фильтра совместимости.
pub fn only_compatible_label() -> String {
    tr("Only for your GNOME version")
}

/// Положение «показать всё».
pub fn show_all_label() -> String {
    tr("Show all")
}

/// Пометка несовместимого.
pub fn unsupported_label(shell: &str) -> String {
    // TRANSLATORS: %1 is a GNOME version: 51, 3.38.
    trf("Doesn't support GNOME %1", &[&shell])
}

/// Сводка заявленных версий.
pub fn supports_label(summary: &str) -> String {
    // TRANSLATORS: %1 is a list of GNOME versions: 47–50, 3.38.
    trf("Supports GNOME %1", &[&summary])
}

/// «Версия для вашего GNOME: 12».
pub fn release_for_you_label(release: &str) -> String {
    // TRANSLATORS: %1 is the extension's release: 12, 5.1.0.
    trf("Version for your GNOME: %1", &[&release])
}

/// Режим сеанса `unlock-dialog`.
pub fn session_modes_label(modes: &[String]) -> String {
    if modes.iter().any(|m| m == "unlock-dialog") {
        tr("Also works on the lock screen")
    } else {
        String::new()
    }
}

/// Сайт не отвечает (5xx).
pub fn site_down_message() -> String {
    tr("The GNOME extensions site isn't responding right now. Outages are shown at status.gnome.org — \
        try again later. Installed extensions keep working.")
}

/// Лицензия карточки.
pub fn license_label() -> String {
    tr("GPL-compatible, as extensions.gnome.org requires")
}

// ═══════════════════════════════════════════════════════════════════════
//  Карточка
// ═══════════════════════════════════════════════════════════════════════

/// Имя автора: отображаемое, иначе учётное, иначе сайт.
pub fn author_name(e: &Extension) -> String {
    [&e.creator.display_name, &e.creator.username]
        .into_iter()
        .map(|s| s.trim())
        .find(|s| !s.is_empty())
        .unwrap_or("extensions.gnome.org")
        .to_string()
}

/// Оценка 0…5 в шкалу магазина 0…100.
pub fn rating_percent(rating: f64) -> i64 {
    if rating.is_finite() {
        (rating.clamp(0.0, 5.0) * 20.0).round() as i64
    } else {
        0
    }
}

/// Дата сайта (`2026-02-25T17:12:27.692638`) без долей секунды.
fn iso(date: &str) -> String {
    let d = date.trim();
    match d.find('.') {
        Some(i) if d[..i].contains('T') => d[..i].to_string(),
        _ => d.to_string(),
    }
}

/// Карточка расширения. Ссылок на скачивание нет: ставит оболочка.
/// Галерея — только настоящий скриншот (значок туда не идёт: человек ждёт
/// в галерее снимков, а не логотипа).
pub fn theme_item(e: &Extension) -> ThemeItem {
    let shot = ego_https(&e.screenshot);
    let preview = card_picture(e);
    let created = iso(&e.created);
    let updated = if e.updated.trim().is_empty() { created.clone() } else { iso(&e.updated) };
    ThemeItem {
        id: item_id(&e.uuid),
        name: if e.name.trim().is_empty() { e.uuid.clone() } else { e.name.trim().to_string() },
        description: e.description.trim().to_string(),
        author: author_name(e),
        license: license_label(),
        rating: rating_percent(e.rating),
        downloads: e.downloads.max(0),
        preview_url: preview,
        preview_urls: if shot.is_empty() { Vec::new() } else { vec![shot] },
        category_id: CATEGORY_ID,
        created,
        updated,
        download_links: Vec::new(),
        ..ThemeItem::default()
    }
}

/// Картинка плитки витрины: снимок, а без него — значок расширения.
pub fn card_picture(e: &Extension) -> String {
    let shot = ego_https(&e.screenshot);
    if shot.is_empty() {
        ego_https(&e.icon)
    } else {
        shot
    }
}

/// Картинки витрины, которые уже пришли со страницами и карточками EGO:
/// uuid → адрес картинки плитки ([`card_picture`]). По ней установленное
/// расширение показывает тот же снимок, что его карточка в витрине, — без
/// своего запроса на сайт (живая проверка 15.11: в витрине снимок, среди
/// установленного — заглушка).
#[derive(Debug, Default)]
pub struct Pictures {
    by_uuid: HashMap<String, String>,
}

impl Pictures {
    /// Сколько адресов держать: память процесса, а не кеш на диске.
    const LIMIT: usize = 4096;

    /// Запомнить картинки страницы (пустые — пропустить).
    pub fn remember(&mut self, items: &[Extension]) {
        if self.by_uuid.len() > Self::LIMIT {
            self.by_uuid.clear();
        }
        for e in items {
            let url = card_picture(e);
            if !e.uuid.is_empty() && !url.is_empty() {
                self.by_uuid.insert(e.uuid.clone(), url);
            }
        }
    }

    /// Адрес картинки расширения `uuid`; `None` — витрина его не показывала.
    pub fn get(&self, uuid: &str) -> Option<&str> {
        self.by_uuid.get(uuid).map(String::as_str)
    }
}

/// Картинки витрины этого процесса ([`Pictures`]).
pub fn live_pictures() -> &'static Mutex<Pictures> {
    static PICTURES: OnceLock<Mutex<Pictures>> = OnceLock::new();
    PICTURES.get_or_init(|| Mutex::new(Pictures::default()))
}

fn remember_pictures(items: &[Extension]) {
    if let Ok(mut p) = live_pictures().lock() {
        p.remember(items);
    }
}

/// Строка оценки плитки: «★ 3.5 (17) · ⬇ 149.9K».
pub fn stats_line(e: &Extension) -> String {
    let dl = crate::appearance_page::human_count(e.downloads.max(0));
    if e.rated > 0 {
        format!(
            "★ {} ({}) · ⬇ {dl}",
            crate::i18n::format_decimal(e.rating.clamp(0.0, 5.0), 1),
            crate::appearance_page::human_count(e.rated)
        )
    } else {
        format!("⬇ {dl}")
    }
}

/// Подсказка строки оценки.
pub fn stats_tooltip() -> String {
    tr("Rating, number of votes and downloads on extensions.gnome.org")
}

/// Строка ответа каталога: карточка плюс то, что знает только EGO.
/// `unsupported` — подпись версии оболочки, которую расширение НЕ
/// поддерживает (по сверке), или `None`. Общие ключи сторонних магазинов
/// (`sitePage`, `siteStats`) — как у Wallhaven.
pub fn catalog_row(item: &ThemeItem, e: &Extension, unsupported: Option<&str>) -> Value {
    let mut v = serde_json::to_value(item).unwrap_or(Value::Null);
    if let Some(m) = v.as_object_mut() {
        let page = page_url(e);
        let donations: Vec<Value> =
            e.donation_urls.iter().map(|u| web_link(u)).filter(|u| !u.is_empty()).map(Value::from).collect();
        m.insert("egoUuid".into(), Value::from(e.uuid.clone()));
        m.insert("egoRating".into(), Value::from(if e.rating.is_finite() { e.rating.clamp(0.0, 5.0) } else { 0.0 }));
        m.insert("egoRated".into(), Value::from(e.rated.max(0)));
        m.insert("egoDownloads".into(), Value::from(e.downloads.max(0)));
        m.insert("egoUnsupported".into(), Value::from(unsupported.is_some()));
        m.insert(
            "egoUnsupportedText".into(),
            Value::from(unsupported.map(unsupported_label).unwrap_or_default()),
        );
        m.insert("egoPage".into(), Value::from(page.clone()));
        m.insert("egoHomepage".into(), Value::from(web_link(&e.url)));
        m.insert("egoIcon".into(), Value::from(ego_https(&e.icon)));
        m.insert("egoDonations".into(), Value::Array(donations));
        // Режимы сеанса знает только выпуск — заполняются подробностями
        // ([`with_release`]).
        m.insert("egoSessionModes".into(), Value::Array(Vec::new()));
        m.insert("sitePage".into(), Value::from(page));
        m.insert("siteStats".into(), Value::from(stats_line(e)));
        m.insert("siteStatsTooltip".into(), Value::from(stats_tooltip()));
    }
    v
}

/// Дополнить строку каталога сведениями о выпусках: «версия для вас»,
/// сводка версий GNOME, режимы сеанса выбранного выпуска.
pub fn with_release(row: &mut Value, releases: &[Release], v: Option<&ShellVersion>) {
    let Some(m) = row.as_object_mut() else { return };
    let chosen = v.and_then(|v| release_for(releases, v));
    let summary = supported_summary(releases);
    m.insert("egoSupports".into(), Value::from(if summary.is_empty() { String::new() } else { supports_label(&summary) }));
    m.insert("egoReleaseForYou".into(), Value::from(chosen.map(|r| r.version).unwrap_or(0)));
    m.insert(
        "egoReleaseForYouText".into(),
        Value::from(chosen.map(|r| release_for_you_label(&r.label())).unwrap_or_default()),
    );
    let modes: Vec<String> = chosen.map(|r| r.session_modes.clone()).unwrap_or_default();
    m.insert("egoSessionModesText".into(), Value::from(session_modes_label(&modes)));
    m.insert("egoSessionModes".into(), Value::Array(modes.into_iter().map(Value::from).collect()));
    if let Some(r) = chosen {
        m.insert("version".into(), Value::from(r.label()));
    }
    if let (Some(v), None) = (v, chosen) {
        m.insert("egoUnsupported".into(), Value::from(true));
        m.insert("egoUnsupportedText".into(), Value::from(unsupported_label(&shell_label(v))));
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Память ответов и отступ после сбоев
// ═══════════════════════════════════════════════════════════════════════

/// Почему ответа нет.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchError {
    /// Недавно был ответ 5xx: сайт не трогаем ещё `wait_secs`.
    BackingOff { wait_secs: u64 },
    /// Сеть или сервер.
    Net(NetError),
    /// Ответ не разобрать.
    Parse(String),
}

impl FetchError {
    /// Сайт сказал «такого нет» (404) — у установленного это не ошибка:
    /// показываются местные сведения.
    pub fn is_not_found(&self) -> bool {
        matches!(self, FetchError::Net(NetError::Status { code: 404, .. }))
    }

    /// Текст для строки состояния.
    pub fn message(&self) -> String {
        match self {
            FetchError::BackingOff { .. } => site_down_message(),
            FetchError::Net(NetError::Status { code, .. }) if *code >= 500 => site_down_message(),
            FetchError::Net(e) => crate::ui_base::humanize_network_error(&e.to_string(), "extensions.gnome.org"),
            FetchError::Parse(_) => tr("The GNOME extensions site sent an answer Rexon can't read. Try again later."),
        }
    }
}

/// Память ответов сессии и отступ после 5xx. Время — параметром (мс),
/// чтобы проверялось без часов.
#[derive(Debug, Default)]
pub struct Gate {
    failed_until: u64,
    cache: HashMap<String, (u64, u64, Vec<u8>)>, // адрес → (когда, срок мс, тело)
    order: VecDeque<String>,
}

/// Срок жизни ответа по адресу: листинги и поиск — коротко, остальное — час.
pub fn ttl_secs(url: &str) -> u64 {
    let path = url.split('?').next().unwrap_or(url);
    if path.ends_with("/extensions/") || path.contains("/extensions/search/") {
        LIST_TTL_SECS
    } else {
        DETAIL_TTL_SECS
    }
}

impl Gate {
    pub fn new() -> Self {
        Self::default()
    }

    /// Свежий ответ из памяти.
    pub fn cached(&self, url: &str, now_ms: u64) -> Option<Vec<u8>> {
        let (at, ttl, body) = self.cache.get(url)?;
        (now_ms.saturating_sub(*at) < *ttl).then(|| body.clone())
    }

    /// Можно ли идти в сеть; нет — сколько секунд подождать.
    pub fn admit(&self, now_ms: u64) -> Result<(), u64> {
        if now_ms < self.failed_until {
            Err((self.failed_until - now_ms).div_ceil(1000))
        } else {
            Ok(())
        }
    }

    /// Сайт ответил 5xx.
    pub fn failed(&mut self, now_ms: u64) {
        self.failed_until = now_ms + FAIL_BACKOFF_SECS * 1000;
    }

    /// Запомнить ответ.
    pub fn store(&mut self, url: &str, body: Vec<u8>, now_ms: u64) {
        let ttl = ttl_secs(url) * 1000;
        if self.cache.insert(url.to_string(), (now_ms, ttl, body)).is_none() {
            self.order.push_back(url.to_string());
        }
        while self.order.len() > CACHE_MAX_ENTRIES {
            if let Some(old) = self.order.pop_front() {
                self.cache.remove(&old);
            }
        }
        self.failed_until = 0;
    }
}

/// Один запрос через [`Gate`]: память (если не `force`), отступ (если не
/// `force`), сеть. `get` — транспорт (в бою [`crate::net::UreqNet`], в
/// тестах — замыкание). Мьютекс на время сети не держится.
pub fn fetch_with(
    gate: &Mutex<Gate>,
    url: &str,
    force: bool,
    now_ms: &dyn Fn() -> u64,
    get: &mut dyn FnMut(&str) -> Result<Vec<u8>, NetError>,
) -> Result<Vec<u8>, FetchError> {
    if !force {
        let g = gate.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(body) = g.cached(url, now_ms()) {
            return Ok(body);
        }
        g.admit(now_ms()).map_err(|wait_secs| FetchError::BackingOff { wait_secs })?;
    }
    match get(url) {
        Ok(body) => {
            if body.len() > MAX_RESPONSE_BYTES {
                return Err(FetchError::Parse("EGO: answer too large".to_string()));
            }
            gate.lock().unwrap_or_else(|e| e.into_inner()).store(url, body.clone(), now_ms());
            Ok(body)
        }
        Err(e) => {
            if matches!(e, NetError::Status { code, .. } if code >= 500) {
                gate.lock().unwrap_or_else(|e| e.into_inner()).failed(now_ms());
            }
            Err(FetchError::Net(e))
        }
    }
}

/// Все выпуски расширения: листает страницы, пока сервер говорит «ещё»
/// (не больше [`VERSIONS_MAX_PAGES`]). Ошибка первой страницы — ошибка;
/// поздней — то, что успели.
pub fn collect_releases(
    uuid: &str,
    mut fetch: impl FnMut(&str) -> Result<Vec<u8>, FetchError>,
) -> Result<Vec<Release>, FetchError> {
    let mut all = Vec::new();
    for n in 1..=VERSIONS_MAX_PAGES as i64 {
        let url = versions_url(uuid, n).ok_or_else(|| FetchError::Parse("EGO: bad uuid".to_string()))?;
        let page = match fetch(&url).and_then(|b| parse_versions(&b).map_err(FetchError::Parse)) {
            Ok(p) => p,
            Err(e) if n == 1 => return Err(e),
            Err(_) => break,
        };
        all.extend(page.items);
        if !page.has_more {
            break;
        }
    }
    Ok(all)
}

/// Память процесса — одна на все страницы «Оформления».
pub fn live_gate() -> &'static Mutex<Gate> {
    static GATE: OnceLock<Mutex<Gate>> = OnceLock::new();
    GATE.get_or_init(|| Mutex::new(Gate::new()))
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn live_get(token: &crate::net::CancelToken) -> impl FnMut(&str) -> Result<Vec<u8>, NetError> {
    let mut net = crate::net::UreqNet::new().with_cancel(token.clone());
    move |url: &str| crate::net::NetIo::get(&mut net, url, crate::ocs::USER_AGENT, TIMEOUT_SECS)
}

/// Страница витрины из живого API.
pub fn live_page(
    query: &str,
    sort: &str,
    page: i64,
    size: i64,
    shell: Option<&str>,
    force: bool,
    token: &crate::net::CancelToken,
) -> Result<ExtPage, FetchError> {
    let url = listing_url(query, sort, page, size, shell);
    let body = fetch_with(live_gate(), &url, force, &now_ms, &mut live_get(token))?;
    let page = parse_listing(&body).map_err(FetchError::Parse)?;
    remember_pictures(&page.items);
    Ok(page)
}

/// Какие из `uuids` подходят оболочке `shell` (сверка пачками).
pub fn live_compatible(
    uuids: &[&str],
    shell: &str,
    token: &crate::net::CancelToken,
) -> Result<HashSet<String>, FetchError> {
    let mut get = live_get(token);
    let mut ok = HashSet::new();
    for url in compat_batch_urls(uuids, shell) {
        let body = fetch_with(live_gate(), &url, false, &now_ms, &mut get)?;
        ok.extend(parse_listing(&body).map_err(FetchError::Parse)?.items.into_iter().map(|e| e.uuid));
    }
    Ok(ok)
}

/// Карточка из живого API.
pub fn live_detail(uuid: &str, token: &crate::net::CancelToken) -> Result<Extension, FetchError> {
    let url = detail_url(uuid).ok_or_else(|| FetchError::Parse("EGO: bad uuid".to_string()))?;
    let body = fetch_with(live_gate(), &url, false, &now_ms, &mut live_get(token))?;
    let e = parse_detail(&body).map_err(FetchError::Parse)?;
    remember_pictures(std::slice::from_ref(&e));
    Ok(e)
}

/// Адрес сведений о расширении по номеру со страницы сайта
/// (`/extension/<pk>/…`): API v1 ищет только по uuid, а ссылки в чужом тексте
/// несут номер.
pub fn info_by_pk_url(pk: u64) -> String {
    format!("{HOME_URL}/extension-info/?pk={pk}")
}

/// uuid из ответа `extension-info`.
pub fn parse_info_uuid(body: &[u8]) -> Option<String> {
    let v: serde_json::Value = serde_json::from_slice(body).ok()?;
    let uuid = v.get("uuid")?.as_str()?;
    valid_uuid(uuid).then(|| uuid.to_string())
}

/// uuid расширения по номеру со страницы сайта.
pub fn live_uuid_by_pk(pk: u64, token: &crate::net::CancelToken) -> Result<String, FetchError> {
    let body = fetch_with(live_gate(), &info_by_pk_url(pk), false, &now_ms, &mut live_get(token))?;
    parse_info_uuid(&body).ok_or_else(|| FetchError::Parse("EGO: нет uuid в extension-info".to_string()))
}

/// Выпуски из живого API.
pub fn live_releases(uuid: &str, token: &crate::net::CancelToken) -> Result<Vec<Release>, FetchError> {
    let mut get = live_get(token);
    collect_releases(uuid, |url| fetch_with(live_gate(), url, false, &now_ms, &mut get))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuid_by_site_number() {
        assert_eq!(info_by_pk_url(615), "https://extensions.gnome.org/extension-info/?pk=615");
        // Ответ сайта (замер 16.11.2026), укорочен.
        let body = br#"{"uuid": "appindicatorsupport@rgcjonas.gmail.com", "name": "AppIndicator", "pk": 615}"#;
        assert_eq!(parse_info_uuid(body).as_deref(), Some("appindicatorsupport@rgcjonas.gmail.com"));
        assert_eq!(parse_info_uuid(br#"{"uuid": "../x"}"#), None);
        assert_eq!(parse_info_uuid(b"<html>"), None);
    }

    fn v(raw: &str) -> ShellVersion {
        parse_shell_version(raw).expect(raw)
    }

    fn rel(version: i64, status: i64, shells: &[(i64, i64)]) -> Release {
        Release {
            version,
            status,
            shell_versions: shells.iter().map(|&(major, minor)| ReleaseShell { major, minor, point: -1 }).collect(),
            ..Release::default()
        }
    }

    #[test]
    fn shell_version_to_site_form() {
        assert_eq!(site_shell_version(&v("51.1")), "51");
        assert_eq!(site_shell_version(&v("51")), "51");
        assert_eq!(site_shell_version(&v("51.beta")), "51.beta");
        assert_eq!(site_shell_version(&v("51.alpha.2")), "51.alpha");
        assert_eq!(site_shell_version(&v("52.RC")), "52.rc");
        assert_eq!(site_shell_version(&v("3.38.4")), "3.38");
        assert_eq!(site_shell_version(&v("GNOME Shell 47.2")), "47");
        assert_eq!(shell_label(&v("51.beta")), "51");
        for bad in ["", "abc", "3", "39.1", "0", "-1", "51.x"] {
            assert_eq!(parse_shell_version(bad), None, "{bad}");
        }
    }

    #[test]
    fn listing_and_search_urls() {
        assert_eq!(
            listing_url("", "popular", 0, 20, Some("51")),
            "https://extensions.gnome.org/api/v1/extensions/?status=3&ordering=-popularity&page=1&page_size=20&shell_version=51"
        );
        let u = listing_url("  dash  to/dock & co ", "down", 2, 20, None);
        assert_eq!(
            u,
            "https://extensions.gnome.org/api/v1/extensions/search/dash%20to%20dock%20%26%20co/?ordering=-downloads&page=3&page_size=20"
        );
        assert!(!u.contains("status="), "у поиска нет параметра status");
        assert!(listing_url("", "updated", 0, 500, None).contains("ordering=-updated&page=1&page_size=100"));
        assert!(listing_url("", "new", -3, 0, Some("51.beta")).contains("ordering=-created&page=1&page_size=1&shell_version=51.beta"));
        // Чужие ключи сортировки — «Популярные».
        assert_eq!(api_ordering("alpha"), "-popularity");
        assert_eq!(api_ordering("high"), "-popularity");
    }

    #[test]
    fn detail_versions_zip_and_ids() {
        assert!(valid_uuid("dash-to-dock@micxgx.gmail.com"));
        assert!(valid_uuid("a+b@x"));
        for bad in ["", "../x", ".hidden@x", "a/b@x", "a b@x", "a..b@x", "a?b"] {
            assert!(!valid_uuid(bad), "{bad}");
        }
        assert_eq!(
            detail_url("demo@rexon.example").as_deref(),
            Some("https://extensions.gnome.org/api/v1/extensions/demo%40rexon.example/")
        );
        assert_eq!(detail_url("a/b"), None);
        assert_eq!(
            versions_url("demo@rexon.example", 2).as_deref(),
            Some("https://extensions.gnome.org/api/v1/extensions/demo%40rexon.example/versions/?page=2&page_size=100")
        );
        assert_eq!(
            release_zip_url("demo@rexon.example", 7).as_deref(),
            Some("https://extensions.gnome.org/api/v1/extensions/demo%40rexon.example/versions/7/?format=zip")
        );
        assert_eq!(release_zip_url("demo@rexon.example", 0), None);
        assert_eq!(parse_item_id(&item_id("demo@rexon.example")), Some("demo@rexon.example"));
        assert_eq!(parse_item_id("ego:../x"), None);
        assert_eq!(parse_item_id("wallhaven:abc"), None);
        assert!(has_section("gnomeext") && !has_section("wallpapers"));
    }

    #[test]
    fn batches_of_fifty() {
        let names: Vec<String> = (0..120).map(|i| format!("ext{i}@rexon.example")).collect();
        let mut refs: Vec<&str> = names.iter().map(String::as_str).collect();
        refs.push("ext0@rexon.example"); // повтор
        refs.push("../bad");
        let urls = compat_batch_urls(&refs, "51");
        assert_eq!(urls.len(), 3);
        let counts: Vec<usize> = urls.iter().map(|u| u.matches("&uuid=").count()).collect();
        assert_eq!(counts, vec![50, 50, 20]);
        assert!(urls[0].starts_with("https://extensions.gnome.org/api/v1/extensions/?page_size=50&shell_version=51&uuid=ext0%40rexon.example"));
        assert!(!urls.iter().any(|u| u.contains("bad")));
        assert!(compat_batch_urls(&[], "51").is_empty());
        let ok: HashSet<String> = ["a@x".to_string()].into();
        assert_eq!(unsupported_uuids(&["a@x", "b@x"], &ok), ["b@x".to_string()].into());
    }

    #[test]
    fn picks_newest_active_release_for_shell() {
        let rs = vec![
            rel(1, STATUS_ACTIVE, &[(45, -1), (46, -1)]),
            rel(2, STATUS_ACTIVE, &[(50, -1), (51, -1)]),
            rel(3, 1, &[(51, -1)]),                    // отклонён
            rel(4, STATUS_ACTIVE, &[(52, -1)]),
        ];
        assert_eq!(release_for(&rs, &v("51.1")).map(|r| r.version), Some(2));
        assert_eq!(release_for(&rs, &v("46")).map(|r| r.version), Some(1));
        assert_eq!(release_for(&rs, &v("47")), None);
        let old = vec![rel(9, STATUS_ACTIVE, &[(3, 36), (3, 38)]), rel(10, STATUS_ACTIVE, &[(3, -1)])];
        assert_eq!(release_for(&old, &v("3.38.1")).map(|r| r.version), Some(10));
        assert_eq!(release_for(&old[..1], &v("3.34")), None);
    }

    #[test]
    fn summary_compresses_runs() {
        let rs = vec![
            rel(1, STATUS_ACTIVE, &[(3, 36), (3, 38), (40, -1), (41, -1)]),
            rel(2, STATUS_ACTIVE, &[(45, -1)]),
            rel(3, STATUS_ACTIVE, &[(47, -1), (48, -1), (49, -1), (50, -1)]),
            rel(4, 1, &[(51, -1)]), // отклонённый в сводку не идёт
        ];
        assert_eq!(supported_summary(&rs), "3.36–41, 45, 47–50");
        assert_eq!(supported_summary(&[rel(1, STATUS_ACTIVE, &[(3, 3), (3, 4), (51, -1)])]), "3.3, 3.4, 51");
        // Нет ни одного активного — по всем.
        assert_eq!(supported_summary(&[rel(1, 0, &[(50, -1), (51, -1)])]), "50–51");
        assert_eq!(supported_summary(&[]), "");
    }

    #[test]
    fn release_rows_mark_chosen_and_installed() {
        let rs = vec![rel(1, STATUS_ACTIVE, &[(50, -1)]), rel(2, STATUS_ACTIVE, &[(51, -1)]), rel(3, 2, &[(51, -1)])];
        let rows = release_rows(&rs, Some(&v("51")), Some(1));
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["version"], 2);
        assert_eq!(rows[0]["forYou"], true);
        assert_eq!(rows[0]["shells"], "51");
        assert_eq!(rows[1]["installed"], true);
        assert_eq!(rows[1]["forYou"], false);
    }

    #[test]
    fn nulls_and_garbage_are_tolerated() {
        let body = br#"{"count":"3","next":null,"previous":null,"results":[
            {"id":1,"uuid":"a@rexon.example","name":null,"creator":null,"description":null,
             "created":null,"updated":null,"downloads":null,"popularity":"5","screenshot":null,
             "icon":null,"rating":null,"rated":null,"url":null,"donation_urls":null,"link":null},
            {"id":2,"uuid":"../evil"},
            "not an object",
            {"uuid":"b@rexon.example","donation_urls":["https://x.example/d", 5, null],"rating":"4.5","unknown_new_field":{"a":1}}
        ]}"#;
        let p = parse_listing(body).unwrap();
        assert_eq!((p.total, p.has_more, p.items.len()), (3, false, 2));
        let a = &p.items[0];
        assert_eq!((a.popularity, a.rating, a.creator.id), (5, 0.0, 0));
        let it = theme_item(a);
        assert_eq!(it.name, "a@rexon.example");
        assert_eq!(it.author, "extensions.gnome.org");
        assert!(it.preview_url.is_empty() && it.preview_urls.is_empty());
        assert_eq!(p.items[1].donation_urls, vec!["https://x.example/d".to_string()]);
        assert_eq!(p.items[1].rating, 4.5);
        assert!(parse_listing(b"<html>").is_err());
        assert!(parse_listing(br#"{"detail":"No Extension matches the given query."}"#).is_err());
        assert!(parse_detail(br#"{"detail":"No Extension matches the given query."}"#).is_err());
        let vs = parse_versions(br#"{"count":2,"next":"https://x","results":[
            {"version":3,"version_name":null,"status":3,"shell_versions":null,"session_modes":null},
            {"version":null}]}"#)
        .unwrap();
        assert_eq!((vs.items.len(), vs.has_more), (1, true));
        assert_eq!(vs.items[0].label(), "3");
    }

    #[test]
    fn image_hosts_and_links() {
        assert_eq!(ego_https("https://extensions.gnome.org/extension-data/s.webp"), "https://extensions.gnome.org/extension-data/s.webp");
        for bad in [
            "http://extensions.gnome.org/x.png",
            "https://extensions.gnome.org.evil.example/x.png",
            "https://evil.example/https://extensions.gnome.org/x.png",
            "https://extensions.gnome.org/../x.png",
            "/extension-data/x.png",
        ] {
            assert_eq!(ego_https(bad), "", "{bad}");
        }
        let mut e = Extension { id: 42, link: "/extension/42/demo/".into(), ..Extension::default() };
        assert_eq!(page_url(&e), "https://extensions.gnome.org/extension/42/demo/");
        e.link = "https://evil.example/".into();
        assert_eq!(page_url(&e), "https://extensions.gnome.org/extension/42/");
        assert_eq!(web_link("javascript:alert(1)"), "");
    }

    #[test]
    fn card_and_catalog_row() {
        let e = Extension {
            id: 7,
            uuid: "demo@rexon.example".into(),
            name: "Demo".into(),
            creator: Creator { id: 1, username: "demo_user".into(), display_name: " ".into() },
            description: "Line one\nLine two".into(),
            created: "2026-01-02T03:04:05.123456".into(),
            downloads: 149_933,
            screenshot: "https://extensions.gnome.org/extension-data/screenshots/demo.gif".into(),
            icon: "https://extensions.gnome.org/extension-data/icons/demo.png".into(),
            rating: 3.47,
            rated: 17,
            url: "https://example.org/demo".into(),
            donation_urls: vec!["https://example.org/give".into(), "ftp://nope".into()],
            link: "/extension/7/demo/".into(),
            ..Extension::default()
        };
        let it = theme_item(&e);
        assert_eq!(it.id, "ego:demo@rexon.example");
        assert_eq!(it.category_id, CATEGORY_ID);
        assert_eq!(it.author, "demo_user");
        assert_eq!(it.rating, 69);
        assert_eq!(it.created, "2026-01-02T03:04:05");
        assert_eq!(it.updated, it.created, "updated = null — берётся дата создания");
        assert!(it.download_links.is_empty(), "ставит оболочка");
        assert_eq!(it.preview_urls, vec![e.screenshot.clone()]);
        let mut row = catalog_row(&it, &e, Some("51"));
        assert_eq!(row["egoRated"], 17);
        assert_eq!(row["egoDownloads"], 149_933);
        assert_eq!(row["egoUnsupported"], true);
        assert_eq!(row["egoUnsupportedText"], "Doesn't support GNOME 51");
        assert_eq!(row["egoPage"], "https://extensions.gnome.org/extension/7/demo/");
        assert_eq!(row["egoDonations"], serde_json::json!(["https://example.org/give"]));
        // Разделитель дроби — по языку (`format_decimal`).
        let expect = format!("★ {} (17) · ⬇ {}", crate::i18n::format_decimal(3.47, 1), crate::appearance_page::human_count(149_933));
        assert_eq!(row["siteStats"], expect.as_str());
        assert_eq!(catalog_row(&it, &e, None)["egoUnsupported"], false);

        let mut r = rel(5, STATUS_ACTIVE, &[(50, -1), (51, -1)]);
        r.version_name = "5.1.0".into();
        r.session_modes = vec!["unlock-dialog".into()];
        let releases = vec![rel(4, STATUS_ACTIVE, &[(49, -1)]), r];
        with_release(&mut row, &releases, Some(&v("51.1")));
        assert_eq!(row["egoReleaseForYou"], 5);
        assert_eq!(row["egoReleaseForYouText"], "Version for your GNOME: 5.1.0");
        assert_eq!(row["egoSupports"], "Supports GNOME 49–51");
        assert_eq!(row["egoSessionModes"], serde_json::json!(["unlock-dialog"]));
        assert_eq!(row["egoSessionModesText"], "Also works on the lock screen");
        assert_eq!(row["version"], "5.1.0");
        // Под 52 подходящего выпуска нет — пометка.
        let mut row2 = catalog_row(&it, &e, None);
        with_release(&mut row2, &releases, Some(&v("52")));
        assert_eq!(row2["egoUnsupportedText"], "Doesn't support GNOME 52");
        assert_eq!(row2["egoReleaseForYou"], 0);
    }

    /// Память картинок витрины: снимок, без него — значок; без картинки и
    /// без uuid — не запоминается; адрес тот же, что у плитки витрины.
    #[test]
    fn pictures_remember_showcase_tiles() {
        let shot = Extension {
            uuid: "a@x".into(),
            screenshot: "https://extensions.gnome.org/extension-data/screenshots/a.png".into(),
            icon: "https://extensions.gnome.org/extension-data/icons/a.png".into(),
            ..Extension::default()
        };
        let icon_only = Extension { uuid: "b@x".into(), icon: "https://extensions.gnome.org/extension-data/icons/b.png".into(), ..Extension::default() };
        let bare = Extension { uuid: "c@x".into(), ..Extension::default() };
        let nameless = Extension { screenshot: "https://extensions.gnome.org/s.png".into(), ..Extension::default() };
        let mut p = Pictures::default();
        p.remember(&[shot.clone(), icon_only.clone(), bare, nameless]);
        assert_eq!(p.get("a@x"), Some(theme_item(&shot).preview_url.as_str()));
        assert!(p.get("a@x").unwrap().contains("screenshots"));
        assert_eq!(p.get("b@x"), Some(theme_item(&icon_only).preview_url.as_str()));
        assert_eq!(p.get("c@x"), None);
        // Чужой сайт картинкой не становится (как у плитки витрины).
        p.remember(&[Extension { uuid: "d@x".into(), screenshot: "https://evil.example/x.png".into(), ..Extension::default() }]);
        assert_eq!(p.get("d@x"), None);
        assert_eq!(p.by_uuid.len(), 2);
    }

    #[test]
    fn cache_ttls_and_backoff_after_5xx() {
        let gate = Mutex::new(Gate::new());
        let clock = std::cell::Cell::new(1_000u64);
        let now = || clock.get();
        let calls = std::cell::Cell::new(0);
        let mut ok = |_: &str| {
            calls.set(calls.get() + 1);
            Ok(br#"{"results":[]}"#.to_vec())
        };
        let list = listing_url("", "popular", 0, 20, None);
        let card = detail_url("demo@rexon.example").unwrap();
        assert_eq!(ttl_secs(&list), LIST_TTL_SECS);
        assert_eq!(ttl_secs(&listing_url("dock", "popular", 0, 20, None)), LIST_TTL_SECS);
        assert_eq!(ttl_secs(&card), DETAIL_TTL_SECS);
        assert_eq!(ttl_secs(&versions_url("demo@rexon.example", 1).unwrap()), DETAIL_TTL_SECS);
        fetch_with(&gate, &list, false, &now, &mut ok).unwrap();
        fetch_with(&gate, &card, false, &now, &mut ok).unwrap();
        fetch_with(&gate, &list, false, &now, &mut ok).unwrap();
        assert_eq!(calls.get(), 2, "повтор — из памяти");
        clock.set(1_000 + LIST_TTL_SECS * 1000);
        fetch_with(&gate, &list, false, &now, &mut ok).unwrap();
        fetch_with(&gate, &card, false, &now, &mut ok).unwrap();
        assert_eq!(calls.get(), 3, "листинг протух, карточка ещё жива");
        fetch_with(&gate, &card, true, &now, &mut ok).unwrap();
        assert_eq!(calls.get(), 4, "«Обновить» идёт в сеть");

        let mut down = |_: &str| Err(NetError::Status { code: 503, reason: "Service Unavailable".into() });
        let e = fetch_with(&gate, "u1", false, &now, &mut down).unwrap_err();
        assert_eq!(e.message(), site_down_message());
        let mut never = |_: &str| -> Result<Vec<u8>, NetError> { panic!("сеть во время отступа") };
        assert_eq!(
            fetch_with(&gate, "u2", false, &now, &mut never).unwrap_err(),
            FetchError::BackingOff { wait_secs: FAIL_BACKOFF_SECS }
        );
        // Память во время отступа работает.
        assert!(fetch_with(&gate, &card, false, &now, &mut never).is_ok());
        // «Обновить» пробует сразу.
        assert!(fetch_with(&gate, "u3", true, &now, &mut ok).is_ok());
        // Удачный ответ снимает отступ.
        assert!(fetch_with(&gate, "u4", false, &now, &mut ok).is_ok());

        let mut missing = |_: &str| Err(NetError::Status { code: 404, reason: "Not Found".into() });
        let e = fetch_with(&gate, "u5", false, &now, &mut missing).unwrap_err();
        assert!(e.is_not_found());
        assert!(fetch_with(&gate, "u6", false, &now, &mut ok).is_ok(), "404 отступа не включает");
    }

    #[test]
    fn collects_release_pages() {
        let mut asked = Vec::new();
        let rs = collect_releases("demo@rexon.example", |url| {
            asked.push(url.to_string());
            let n = if url.contains("page=1&") { 1 } else { 2 };
            let next = if n == 1 { "\"https://next\"" } else { "null" };
            Ok(format!(r#"{{"count":2,"next":{next},"results":[{{"version":{n},"status":3}}]}}"#).into_bytes())
        })
        .unwrap();
        assert_eq!(asked.len(), 2);
        assert_eq!(rs.iter().map(|r| r.version).collect::<Vec<_>>(), vec![1, 2]);
        let e = collect_releases("demo@rexon.example", |_| Err(FetchError::Parse("x".into()))).unwrap_err();
        assert_eq!(e, FetchError::Parse("x".into()));
        assert!(collect_releases("../x", |_| unreachable!()).is_err());
    }
}
