//! Wallhaven (wallhaven.cc) — второй магазин раздела «Обои HD» рядом с
//! KDE Store.
//!
//! Свой клиент по официальной документации API v1
//! (<https://wallhaven.cc/help/api>): поиск и листинги
//! `GET /api/v1/search`, сведения о картинке `GET /api/v1/w/<id>`.
//! Решение владельца 06.11.2026.
//!
//! Что здесь живёт и почему так:
//!
//! * **Только SFW и без ключа.** В каждый запрос уходит `purity=100`; ключа
//!   API (`X-API-Key`) Rexon не шлёт вовсе — места для секретов в Rexon пока
//!   нет, а без ключа сервер NSFW и так не отдаёт.
//! * **Лимит 45 запросов в минуту** («API calls are currently limited to 45
//!   per minute», сверх — 429). Свой счётчик ([`Gate`]) не выпускает больше
//!   [`OWN_BUDGET_PER_MINUTE`] за скользящую минуту, после 429 сам молчит
//!   [`RATE_LIMIT_PAUSE_SECS`], а ответы держит в памяти сессии
//!   [`CACHE_TTL_SECS`]: вернулся в раздел или листнул назад — сеть не нужна.
//!   Миниатюры (`th.wallhaven.cc`) и сами файлы (`w.wallhaven.cc`) идут не
//!   через API и в счёт не входят.
//! * **Картинки — собственность авторов** («All images remain property of
//!   their original owners»). Rexon ничего не раздаёт и в пакет не кладёт:
//!   файл качает сам человек себе, в `~/.local/share/wallpapers`, тем же
//!   установщиком, что обои KDE Store, а применяет тем же общим
//!   исполнителем фона (`appearance_installer::apply_wallpaper_background`).
//!   У карточки — ссылка на страницу картинки на сайте ([`page_url`]).
//! * Отдельной ветки установки нет: карточка — обычный [`ThemeItem`]
//!   категории обоев (299) с прямой ссылкой на файл в `download_links`;
//!   журнал установленного, «Применить» и «Удалить» — общие.

use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, OnceLock};

use serde::Deserialize;
use serde_json::{json, Value};

use crate::i18n::{n_, tr, trc, trf};
use crate::models::ThemeItem;
use crate::net::NetError;

/// Адрес сайта.
pub const HOME_URL: &str = "https://wallhaven.cc";
/// Корень API v1.
pub const API_BASE: &str = "https://wallhaven.cc/api/v1";
/// Ключ источника в переключателе «Магазин:».
pub const SOURCE_KEY: &str = "wallhaven";
/// Префикс id карточки и записи журнала установленного: `wallhaven:<id>`.
pub const ID_PREFIX: &str = "wallhaven:";
/// Разделы, где Wallhaven — ещё один магазин.
pub const SECTIONS: &[&str] = &["wallpapers"];
/// Категория магазина, под которой ставится картинка (обои KDE Store): по
/// ней установщик берёт каталог и правило одиночного файла.
pub const CATEGORY_ID: i64 = 299;
/// Сколько картинок отдаёт сервер на страницу листинга (документация:
/// «Listings are limited to 24 results per page»).
pub const API_PAGE_SIZE: i64 = 24;
/// Лимит сервера на запросы к API в минуту.
pub const RATE_LIMIT_PER_MINUTE: usize = 45;
/// Свой потолок за скользящую минуту — с запасом под сайт, открытый в
/// браузере той же машины (лимит у сервера общий на адрес).
pub const OWN_BUDGET_PER_MINUTE: usize = 40;
/// Сколько молчать после ответа 429.
pub const RATE_LIMIT_PAUSE_SECS: u64 = 60;
/// Сколько ответ API живёт в памяти сессии.
pub const CACHE_TTL_SECS: u64 = 10 * 60;
/// Сколько ответов держать в памяти (старые вытесняются).
pub const CACHE_MAX_ENTRIES: usize = 64;
/// Таймаут запроса к API, секунды.
pub const TIMEOUT_SECS: u64 = 15;
/// Больше этого ответ API не читаем (страница листинга — ~20 КБ).
pub const MAX_RESPONSE_BYTES: usize = 2 << 20;
/// Полноразмерный файл попадает в галерею карточки, только если он не
/// больше этого: галерея качает кадры сразу при открытии.
pub const GALLERY_FULL_MAX_BYTES: i64 = 8 << 20;

/// Есть ли Wallhaven в разделе `section`.
pub fn has_section(section: &str) -> bool {
    SECTIONS.contains(&section)
}

/// Карточка отсюда?
pub fn is_item_id(id: &str) -> bool {
    id.starts_with(ID_PREFIX)
}

/// Подпись чипа «Магазин:».
pub fn chip_label() -> String {
    "Wallhaven".to_string()
}

/// Подсказка чипа.
pub fn chip_tooltip() -> String {
    tr("Wallpapers from wallhaven.cc, safe-for-work only. The pictures belong to their authors: \
        Rexon downloads the one you pick for you and links to its page on the site.")
}

/// Подсказка строки поиска.
pub fn search_placeholder() -> String {
    tr("🔍 Search Wallhaven…")
}

// ═══════════════════════════════════════════════════════════════════════
//  Сортировки
// ═══════════════════════════════════════════════════════════════════════

/// Сортировки Wallhaven в выпадающем списке: `(подпись, ключ)`. Ключи —
/// те же, что у KDE Store (`appearance_page::SORTS`), чтобы выбор
/// переживал переключение магазина: «Новые», «Популярные», «Лучшие».
pub const SORTS: &[(&str, &str)] = &[
    (n_("🆕 Newest"), "new"),
    (n_("🔥 Most Viewed"), "down"),
    (n_("🏆 Top of the Month"), "high"),
];

/// Ключ сортировки, который действует: неизвестный Wallhaven (у KDE Store
/// есть «По алфавиту») — «Новые».
pub fn effective_sort(sort: &str) -> &'static str {
    SORTS.iter().map(|(_, k)| *k).find(|k| *k == sort).unwrap_or("new")
}

/// Параметр `sorting` API для ключа Rexon (и `topRange` у «Лучших»).
pub fn api_sorting(sort: &str) -> (&'static str, Option<&'static str>) {
    match effective_sort(sort) {
        "down" => ("views", None),
        "high" => ("toplist", Some("1M")),
        _ => ("date_added", None),
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Адреса
// ═══════════════════════════════════════════════════════════════════════

/// Кодирование значения параметра запроса (пробел — `+`).
fn quote_plus(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Адрес страницы листинга. `api_page` — с единицы, как у сервера.
///
/// `categories=111` — все три категории сайта (общие, аниме, люди);
/// `purity=100` — только SFW, всегда.
pub fn search_url(query: &str, sort: &str, api_page: i64) -> String {
    let (sorting, range) = api_sorting(sort);
    let mut url = format!(
        "{API_BASE}/search?categories=111&purity=100&sorting={sorting}&order=desc&page={}",
        api_page.max(1)
    );
    if let Some(r) = range {
        url.push_str("&topRange=");
        url.push_str(r);
    }
    let q = query.trim();
    if !q.is_empty() {
        url.push_str("&q=");
        url.push_str(&quote_plus(q));
    }
    url
}

/// Похоже ли на id картинки Wallhaven (`94x38z`): латиница и цифры.
/// Из чужого ответа в адрес попадает только такое.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 16 && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
}

/// Адрес сведений о картинке.
pub fn detail_url(wid: &str) -> Option<String> {
    valid_id(wid).then(|| format!("{API_BASE}/w/{wid}"))
}

/// Страница картинки на сайте — собирается из id, а не берётся из ответа.
pub fn page_url(wid: &str) -> String {
    format!("{HOME_URL}/w/{wid}")
}

/// id карточки.
pub fn item_id(wid: &str) -> String {
    format!("{ID_PREFIX}{wid}")
}

/// id картинки из id карточки.
pub fn parse_item_id(id: &str) -> Option<&str> {
    id.strip_prefix(ID_PREFIX).filter(|w| valid_id(w))
}

/// Адрес только с хостов Wallhaven по https — иначе пусто: ссылка на файл и
/// миниатюры приходит из ответа сервера, и в загрузчик она уйдёт как есть.
fn wallhaven_https(url: &str) -> String {
    let ok = ["https://w.wallhaven.cc/", "https://th.wallhaven.cc/"]
        .iter()
        .any(|p| url.starts_with(p));
    if ok && !url.contains("..") {
        url.to_string()
    } else {
        String::new()
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Разбор ответов
// ═══════════════════════════════════════════════════════════════════════

/// Число из ответа: сервер отдаёт счётчики то числом, то строкой.
fn lenient_i64<'de, D: serde::Deserializer<'de>>(d: D) -> Result<i64, D::Error> {
    let v = Value::deserialize(d)?;
    Ok(match v {
        Value::Number(n) => n.as_i64().unwrap_or(0),
        Value::String(s) => s.trim().parse().unwrap_or(0),
        _ => 0,
    })
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Thumbs {
    pub large: String,
    pub original: String,
    pub small: String,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Uploader {
    pub username: String,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Tag {
    pub name: String,
}

/// Картинка из листинга или из сведений (у сведений есть автор загрузки и
/// метки).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Wallpaper {
    pub id: String,
    #[serde(deserialize_with = "lenient_i64")]
    pub views: i64,
    #[serde(deserialize_with = "lenient_i64")]
    pub favorites: i64,
    pub source: String,
    pub purity: String,
    pub category: String,
    #[serde(deserialize_with = "lenient_i64")]
    pub dimension_x: i64,
    #[serde(deserialize_with = "lenient_i64")]
    pub dimension_y: i64,
    #[serde(deserialize_with = "lenient_i64")]
    pub file_size: i64,
    pub file_type: String,
    pub created_at: String,
    pub path: String,
    pub thumbs: Thumbs,
    pub uploader: Option<Uploader>,
    pub tags: Vec<Tag>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
struct Meta {
    #[serde(deserialize_with = "lenient_i64")]
    current_page: i64,
    #[serde(deserialize_with = "lenient_i64")]
    last_page: i64,
    #[serde(deserialize_with = "lenient_i64")]
    total: i64,
}

#[derive(Deserialize)]
struct SearchBody {
    data: Vec<Value>,
    #[serde(default)]
    meta: Meta,
}

#[derive(Deserialize)]
struct DetailBody {
    data: Wallpaper,
}

/// Страница листинга.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SearchPage {
    pub items: Vec<Wallpaper>,
    pub current_page: i64,
    pub last_page: i64,
    pub total: i64,
}

/// Годится ли картинка для витрины: SFW (сервер без ключа другого не
/// отдаёт, но проверяем сами), с id и файлом на хосте Wallhaven.
fn usable(w: &Wallpaper) -> bool {
    valid_id(&w.id) && w.purity == "sfw" && !wallhaven_https(&w.path).is_empty()
}

/// Разобрать ответ `GET /search`. Негодные записи (не SFW, без файла,
/// с чужим адресом) молча выпадают — по одной, не роняя страницу.
pub fn parse_search(body: &[u8]) -> Result<SearchPage, String> {
    let parsed: SearchBody = serde_json::from_slice(body).map_err(|e| format!("Wallhaven: {e}"))?;
    let items = parsed
        .data
        .into_iter()
        .filter_map(|v| serde_json::from_value::<Wallpaper>(v).ok())
        .filter(usable)
        .collect();
    Ok(SearchPage {
        items,
        current_page: parsed.meta.current_page,
        last_page: parsed.meta.last_page,
        total: parsed.meta.total,
    })
}

/// Разобрать ответ `GET /w/<id>`.
pub fn parse_detail(body: &[u8]) -> Result<Wallpaper, String> {
    let parsed: DetailBody = serde_json::from_slice(body).map_err(|e| format!("Wallhaven: {e}"))?;
    if !usable(&parsed.data) {
        return Err("Wallhaven: unusable wallpaper".to_string());
    }
    Ok(parsed.data)
}

// ═══════════════════════════════════════════════════════════════════════
//  Карточка
// ═══════════════════════════════════════════════════════════════════════

/// Расширение файла по адресу (`jpg`, `png`) — только знакомые картинки.
fn file_ext(w: &Wallpaper) -> &'static str {
    let tail = w.path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match tail.as_str() {
        "png" => "png",
        "webp" => "webp",
        "gif" => "gif",
        _ => "jpg",
    }
}

/// Имя файла в `~/.local/share/wallpapers`: `wallhaven-<id>.<расш>` — как
/// его называет сам сайт.
pub fn file_name(w: &Wallpaper) -> String {
    format!("wallhaven-{}.{}", w.id, file_ext(w))
}

/// Категория сайта для человека.
pub fn category_label(category: &str) -> String {
    match category {
        "general" => trc("wallpaper category", "General"),
        "anime" => trc("wallpaper category", "Anime"),
        "people" => trc("wallpaper category", "People"),
        other => other.to_string(),
    }
}

/// `2026-10-10 04:08:04` → `2026-10-10T04:08:04` (ISO, как у магазина).
fn iso(created: &str) -> String {
    created.trim().replacen(' ', "T", 1)
}

/// Карточка картинки — обычный товар раздела обоев: установщик видит
/// одиночный файл категории 299 и кладёт его в `~/.local/share/wallpapers`.
pub fn theme_item(w: &Wallpaper) -> ThemeItem {
    let path = wallhaven_https(&w.path);
    let name = file_name(w);
    let kib = (w.file_size.max(0) + 1023) / 1024;
    let large = wallhaven_https(&w.thumbs.large);
    let original = wallhaven_https(&w.thumbs.original);
    let mut gallery: Vec<String> = Vec::new();
    if !original.is_empty() {
        gallery.push(original);
    }
    if !path.is_empty() && w.file_size > 0 && w.file_size <= GALLERY_FULL_MAX_BYTES {
        gallery.push(path.clone());
    }
    let preview = if large.is_empty() { gallery.first().cloned().unwrap_or_default() } else { large };
    let author = match &w.uploader {
        Some(u) if !u.username.trim().is_empty() => u.username.trim().to_string(),
        _ => "wallhaven.cc".to_string(),
    };
    ThemeItem {
        id: item_id(&w.id),
        name: format!("{} × {}", w.dimension_x, w.dimension_y),
        description: description(w),
        author,
        license: tr("Belongs to its author — see the page on wallhaven.cc"),
        preview_url: preview,
        preview_urls: gallery,
        download_url: path.clone(),
        download_name: name.clone(),
        download_size: kib,
        category_id: CATEGORY_ID,
        created: iso(&w.created_at),
        updated: iso(&w.created_at),
        download_links: vec![json!({ "url": path, "name": name, "size": kib })],
        ..ThemeItem::default()
    }
}

/// Описание карточки: категория, метки (они есть только в сведениях) и
/// первоисточник, если сайт его знает.
fn description(w: &Wallpaper) -> String {
    let mut lines = vec![trf("Category: %1", &[&category_label(&w.category)])];
    let tags: Vec<&str> = w.tags.iter().map(|t| t.name.trim()).filter(|t| !t.is_empty()).collect();
    if !tags.is_empty() {
        lines.push(trf("Tags: %1", &[&tags.join(", ")]));
    }
    let source = w.source.trim();
    if source.starts_with("https://") || source.starts_with("http://") {
        lines.push(trf("Original source: %1", &[&source]));
    }
    lines.push(trf("Page on Wallhaven: %1", &[&page_url(&w.id)]));
    lines.join("\n")
}

/// Строка оценки плитки: «♥ 328 · 👁 39.4K».
pub fn stats_line(w: &Wallpaper) -> String {
    format!(
        "♥ {} · 👁 {}",
        crate::appearance_page::human_count(w.favorites),
        crate::appearance_page::human_count(w.views)
    )
}

/// Подсказка строки оценки.
pub fn stats_tooltip() -> String {
    tr("Favorites and views on wallhaven.cc")
}

/// Строка ответа каталога: карточка плюс то, что знает только Wallhaven
/// (страница на сайте и своя строка оценки) — ключами `site*`, общими для
/// всех сторонних магазинов.
pub fn catalog_row(item: &ThemeItem, w: &Wallpaper) -> Value {
    let mut v = serde_json::to_value(item).unwrap_or(Value::Null);
    if let Some(m) = v.as_object_mut() {
        m.insert("sitePage".to_string(), Value::from(page_url(&w.id)));
        m.insert("siteStats".to_string(), Value::from(stats_line(w)));
        m.insert("siteStatsTooltip".to_string(), Value::from(stats_tooltip()));
    }
    v
}

// ═══════════════════════════════════════════════════════════════════════
//  Страница Rexon из страниц сервера
// ═══════════════════════════════════════════════════════════════════════

/// Какие страницы сервера (с единицы) покрывают страницу Rexon номер
/// `page` (с нуля) по `size` карточек — плюс одну карточку сверх, чтобы
/// честно знать, есть ли следующая. Сервер листает ровно по 24 и ничего не
/// прячет, поэтому окно считается сразу, без прохода с первой страницы.
pub fn api_pages_for(page: i64, size: i64) -> (i64, i64) {
    let size = size.max(1);
    let start = page.max(0) * size;
    let end = start + size + 1;
    (start / API_PAGE_SIZE + 1, (end - 1) / API_PAGE_SIZE + 1)
}

/// Страница Rexon: карточки, сколько всего и есть ли дальше.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Page {
    pub items: Vec<Wallpaper>,
    pub total: i64,
    pub has_more: bool,
}

/// Собрать страницу Rexon из страниц сервера; `fetch(n)` — страница
/// сервера `n` (с единицы). Ошибка первой нужной страницы — ошибка
/// страницы; ошибка второй — просто «дальше не знаем» (по `total`).
pub fn assemble_page(
    page: i64,
    size: i64,
    mut fetch: impl FnMut(i64) -> Result<SearchPage, FetchError>,
) -> Result<Page, FetchError> {
    let size = size.max(1);
    let (first, last) = api_pages_for(page, size);
    let skip = (page.max(0) * size - (first - 1) * API_PAGE_SIZE).max(0) as usize;
    let mut stream: Vec<Wallpaper> = Vec::new();
    let mut total = 0i64;
    let mut last_page = i64::MAX;
    let mut broken = false;
    for n in first..=last {
        if n > last_page {
            break;
        }
        match fetch(n) {
            Ok(p) => {
                total = total.max(p.total);
                if p.last_page > 0 {
                    last_page = p.last_page;
                }
                stream.extend(p.items);
            }
            Err(e) if n == first => return Err(e),
            Err(_) => {
                broken = true;
                break;
            }
        }
    }
    let mut items: Vec<Wallpaper> = stream.into_iter().skip(skip).collect();
    let has_more = if broken {
        (page.max(0) + 1) * size < total
    } else {
        items.len() > size as usize
    };
    items.truncate(size as usize);
    Ok(Page { items, total, has_more })
}

// ═══════════════════════════════════════════════════════════════════════
//  Лимит запросов и память ответов
// ═══════════════════════════════════════════════════════════════════════

/// Почему ответа нет.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchError {
    /// Свой счётчик или сервер (429) сказал «слишком часто»: подождать
    /// `wait_secs`.
    RateLimited { wait_secs: u64 },
    /// Сеть или сервер.
    Net(NetError),
    /// Ответ не разобрать.
    Parse(String),
}

impl FetchError {
    /// Текст для строки состояния.
    pub fn message(&self) -> String {
        match self {
            FetchError::RateLimited { wait_secs } => trf(
                // TRANSLATORS: %1 is a number of seconds.
                "Wallhaven allows only 45 requests per minute, and the limit is used up. \
                 Try again in %1 s.",
                &[&(*wait_secs).max(1)],
            ),
            FetchError::Net(NetError::Status { code: 401, .. }) => {
                tr("Wallhaven refused access to this picture (it is not safe-for-work).")
            }
            FetchError::Net(e) => crate::ui_base::humanize_network_error(&e.to_string(), "Wallhaven"),
            FetchError::Parse(_) => tr("Wallhaven sent an answer Rexon can't read. Try again later."),
        }
    }
}

/// Счётчик запросов за скользящую минуту, пауза после 429 и память
/// ответов сессии. Время — параметром (мс), чтобы проверялось без часов.
#[derive(Debug, Default)]
pub struct Gate {
    sent: VecDeque<u64>,
    paused_until: u64,
    cache: HashMap<String, (u64, Vec<u8>)>,
    order: VecDeque<String>,
}

impl Gate {
    pub fn new() -> Self {
        Self::default()
    }

    /// Свежий ответ из памяти.
    pub fn cached(&self, url: &str, now_ms: u64) -> Option<Vec<u8>> {
        let (at, body) = self.cache.get(url)?;
        (now_ms.saturating_sub(*at) < CACHE_TTL_SECS * 1000).then(|| body.clone())
    }

    /// Можно ли послать запрос сейчас; да — он сразу записан в счёт.
    /// Нет — сколько секунд подождать.
    pub fn admit(&mut self, now_ms: u64) -> Result<(), u64> {
        if now_ms < self.paused_until {
            return Err((self.paused_until - now_ms).div_ceil(1000));
        }
        while self.sent.front().is_some_and(|t| now_ms.saturating_sub(*t) >= 60_000) {
            self.sent.pop_front();
        }
        if self.sent.len() >= OWN_BUDGET_PER_MINUTE {
            let oldest = self.sent.front().copied().unwrap_or(now_ms);
            return Err((60_000 - now_ms.saturating_sub(oldest)).div_ceil(1000));
        }
        self.sent.push_back(now_ms);
        Ok(())
    }

    /// Сервер ответил 429: молчать минуту.
    pub fn throttled(&mut self, now_ms: u64) {
        self.paused_until = now_ms + RATE_LIMIT_PAUSE_SECS * 1000;
    }

    /// Запомнить ответ.
    pub fn store(&mut self, url: &str, body: Vec<u8>, now_ms: u64) {
        if self.cache.insert(url.to_string(), (now_ms, body)).is_none() {
            self.order.push_back(url.to_string());
        }
        while self.order.len() > CACHE_MAX_ENTRIES {
            if let Some(old) = self.order.pop_front() {
                self.cache.remove(&old);
            }
        }
    }
}

/// Один запрос к API через [`Gate`]: память (если не `force`), счёт, сеть.
/// `get` — транспорт (в бою [`crate::net::UreqNet`], в тестах — замыкание).
/// Мьютекс счётчика на время сети не держится.
pub fn fetch_with(
    gate: &Mutex<Gate>,
    url: &str,
    force: bool,
    now_ms: &dyn Fn() -> u64,
    get: &mut dyn FnMut(&str) -> Result<Vec<u8>, NetError>,
) -> Result<Vec<u8>, FetchError> {
    {
        let mut g = gate.lock().unwrap_or_else(|e| e.into_inner());
        if !force {
            if let Some(body) = g.cached(url, now_ms()) {
                return Ok(body);
            }
        }
        g.admit(now_ms()).map_err(|wait_secs| FetchError::RateLimited { wait_secs })?;
    }
    match get(url) {
        Ok(body) => {
            if body.len() > MAX_RESPONSE_BYTES {
                return Err(FetchError::Parse("Wallhaven: answer too large".to_string()));
            }
            gate.lock().unwrap_or_else(|e| e.into_inner()).store(url, body.clone(), now_ms());
            Ok(body)
        }
        Err(NetError::Status { code: 429, .. }) => {
            gate.lock().unwrap_or_else(|e| e.into_inner()).throttled(now_ms());
            Err(FetchError::RateLimited { wait_secs: RATE_LIMIT_PAUSE_SECS })
        }
        Err(e) => Err(FetchError::Net(e)),
    }
}

/// Счётчик процесса — один на все страницы «Оформления».
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

/// Страница Rexon из живого API.
pub fn live_page(
    query: &str,
    sort: &str,
    page: i64,
    size: i64,
    force: bool,
    token: &crate::net::CancelToken,
) -> Result<Page, FetchError> {
    let mut get = live_get(token);
    assemble_page(page, size, |n| {
        let url = search_url(query, sort, n);
        let body = fetch_with(live_gate(), &url, force, &now_ms, &mut get)?;
        parse_search(&body).map_err(FetchError::Parse)
    })
}

/// Сведения о картинке из живого API (автор загрузки, метки).
pub fn live_detail(wid: &str, token: &crate::net::CancelToken) -> Result<Wallpaper, FetchError> {
    let url = detail_url(wid).ok_or_else(|| FetchError::Parse("Wallhaven: bad id".to_string()))?;
    let mut get = live_get(token);
    let body = fetch_with(live_gate(), &url, false, &now_ms, &mut get)?;
    parse_detail(&body).map_err(FetchError::Parse)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEARCH: &[u8] = include_bytes!("../tests/data/wallhaven/search_toplist.json");
    const DETAIL: &[u8] = include_bytes!("../tests/data/wallhaven/detail.json");

    #[test]
    fn search_url_is_sfw_and_keyless() {
        let u = search_url("", "new", 1);
        assert_eq!(u, "https://wallhaven.cc/api/v1/search?categories=111&purity=100&sorting=date_added&order=desc&page=1");
        assert!(!u.contains("apikey"));
        let u = search_url(" sky & clouds ", "high", 3);
        assert!(u.contains("sorting=toplist") && u.contains("topRange=1M") && u.contains("page=3"));
        assert!(u.ends_with("&q=sky+%26+clouds"), "{u}");
        assert!(search_url("x", "down", 0).contains("sorting=views&order=desc&page=1"));
        // «По алфавиту» у Wallhaven нет — «Новые».
        assert_eq!(effective_sort("alpha"), "new");
        assert!(search_url("", "alpha", 1).contains("sorting=date_added"));
    }

    #[test]
    fn ids_and_urls() {
        assert!(valid_id("94x38z"));
        assert!(!valid_id("../x") && !valid_id("") && !valid_id("A1"));
        assert_eq!(detail_url("yqkl97").as_deref(), Some("https://wallhaven.cc/api/v1/w/yqkl97"));
        assert_eq!(detail_url("a/b"), None);
        assert_eq!(page_url("yqkl97"), "https://wallhaven.cc/w/yqkl97");
        assert_eq!(parse_item_id(&item_id("yqkl97")), Some("yqkl97"));
        assert_eq!(parse_item_id("wallhaven:../x"), None);
        assert_eq!(parse_item_id("spices:theme:x"), None);
        assert!(has_section("wallpapers") && !has_section("icons"));
    }

    #[test]
    fn parses_saved_search_sample() {
        let p = parse_search(SEARCH).expect("образец разбирается");
        assert_eq!((p.current_page, p.last_page, p.total), (1, 98, 2350));
        // В образце 24 записи; три испорчены нарочно (NSFW, чужой хост,
        // кривой id) — выпадают по одной.
        assert_eq!(p.items.len(), 21);
        assert!(p.items.iter().all(|w| w.purity == "sfw" && valid_id(&w.id)));
        let w = &p.items[0];
        assert_eq!(w.id, "yqkl97");
        assert_eq!((w.dimension_x, w.dimension_y, w.file_size), (2074, 1166, 976307));
        assert_eq!(w.path, "https://w.wallhaven.cc/full/yq/wallhaven-yqkl97.jpg");
        assert_eq!(w.thumbs.large, "https://th.wallhaven.cc/lg/yq/yqkl97.jpg");
        assert!(w.uploader.is_none() && w.tags.is_empty());
    }

    #[test]
    fn parses_saved_detail_sample() {
        let w = parse_detail(DETAIL).expect("образец разбирается");
        assert_eq!(w.id, "yqkl97");
        assert_eq!(w.uploader.as_ref().map(|u| u.username.as_str()), Some("Artix"));
        assert!(w.tags.iter().any(|t| t.name == "clouds"));
        let it = theme_item(&w);
        assert_eq!(it.author, "Artix");
        assert!(it.description.contains("clouds"));
        assert!(it.description.contains("https://wallhaven.cc/w/yqkl97"));
        assert!(it.description.contains("https://www.pixiv.net/en/artworks/150504495"));
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_search(b"<html>").is_err());
        assert!(parse_search(br#"{"error":"x"}"#).is_err());
        // Строковые счётчики — тоже числа.
        let p = parse_search(br#"{"data":[],"meta":{"current_page":"2","last_page":"5","per_page":"24","total":"100"}}"#).unwrap();
        assert_eq!((p.current_page, p.last_page, p.total), (2, 5, 100));
        assert!(parse_detail(br#"{"data":{"id":"abc","purity":"nsfw","path":"https://w.wallhaven.cc/full/ab/wallhaven-abc.jpg"}}"#).is_err());
    }

    #[test]
    fn card_is_plain_wallpaper_item() {
        let p = parse_search(SEARCH).unwrap();
        let w = &p.items[0];
        let it = theme_item(w);
        assert_eq!(it.id, "wallhaven:yqkl97");
        assert_eq!(it.category_id, CATEGORY_ID);
        assert_eq!(it.name, "2074 × 1166");
        assert_eq!(it.author, "wallhaven.cc");
        assert_eq!(it.download_name, "wallhaven-yqkl97.jpg");
        assert_eq!(it.download_size, 954);
        assert_eq!(it.created, "2026-10-10T04:08:04");
        assert_eq!(it.preview_url, "https://th.wallhaven.cc/lg/yq/yqkl97.jpg");
        // Галерея: миниатюра без обрезки и сам файл (он меньше 8 МиБ).
        assert_eq!(it.preview_urls, vec!["https://th.wallhaven.cc/orig/yq/yqkl97.jpg".to_string(), w.path.clone()]);
        // Ссылка на файл — та, что возьмёт общий загрузчик магазина.
        assert_eq!(it.download_links.len(), 1);
        assert_eq!(it.download_links[0]["url"], w.path.as_str());
        assert_eq!(crate::ocs::chosen_file_name(&it).as_deref(), Some("wallhaven-yqkl97.jpg"));
        // Большой файл в галерею не идёт.
        let big = p.items.iter().find(|w| w.file_size > GALLERY_FULL_MAX_BYTES).expect("в образце есть большой файл");
        assert_eq!(theme_item(big).preview_urls.len(), 1);
        let row = catalog_row(&it, w);
        assert_eq!(row["sitePage"], "https://wallhaven.cc/w/yqkl97");
        assert_eq!(row["siteStats"], "♥ 328 · 👁 39.4K");
        // Установщик видит одиночную картинку раздела обоев.
        let cat = crate::categories::category_by_id(std::path::Path::new("/home/u"), it.category_id).unwrap();
        assert_eq!(cat.key, "wallpapers");
        assert_eq!(cat.install_dir, "/home/u/.local/share/wallpapers");
    }

    #[test]
    fn page_window_math() {
        assert_eq!(api_pages_for(0, 20), (1, 1));
        assert_eq!(api_pages_for(0, 24), (1, 2)); // 25-я — «есть ли дальше»
        assert_eq!(api_pages_for(1, 20), (1, 2));
        assert_eq!(api_pages_for(5, 20), (5, 6)); // карточки 100..=120
    }

    fn fake_page(n: i64, last: i64) -> SearchPage {
        let items = (0..API_PAGE_SIZE)
            .map(|i| Wallpaper { id: format!("p{n}i{i}"), ..Wallpaper::default() })
            .collect();
        SearchPage { items, current_page: n, last_page: last, total: last * API_PAGE_SIZE }
    }

    #[test]
    fn assembles_rexon_pages_without_gaps() {
        let mut asked = Vec::new();
        let p = assemble_page(1, 20, |n| {
            asked.push(n);
            Ok(fake_page(n, 10))
        })
        .unwrap();
        assert_eq!(asked, vec![1, 2]);
        assert_eq!(p.items.len(), 20);
        assert_eq!(p.items[0].id, "p1i20");
        assert_eq!(p.items[19].id, "p2i15");
        assert!(p.has_more);
        assert_eq!(p.total, 240);
        // Конец листинга: последняя страница сервера — дальше не просим.
        let mut asked = Vec::new();
        let p = assemble_page(1, 20, |n| {
            asked.push(n);
            Ok(fake_page(n, 1))
        })
        .unwrap();
        assert_eq!(asked, vec![1]);
        assert_eq!(p.items.len(), 4);
        assert!(!p.has_more);
        // Первая нужная страница не пришла — ошибка страницы.
        let e = assemble_page(0, 20, |_| Err(FetchError::RateLimited { wait_secs: 7 })).unwrap_err();
        assert_eq!(e, FetchError::RateLimited { wait_secs: 7 });
    }

    #[test]
    fn gate_counts_a_sliding_minute() {
        let mut g = Gate::new();
        for i in 0..OWN_BUDGET_PER_MINUTE as u64 {
            assert!(g.admit(1_000 + i).is_ok());
        }
        let wait = g.admit(30_000).unwrap_err();
        assert_eq!(wait, 31); // самый старый (1 с) выйдет из окна в 61 с
        assert!(g.admit(61_000).is_ok());
        g.throttled(100_000);
        assert_eq!(g.admit(100_500).unwrap_err(), 60);
        assert!(g.admit(160_000).is_ok());
    }

    #[test]
    fn fetch_uses_memory_and_reports_429() {
        let gate = Mutex::new(Gate::new());
        let clock = std::cell::Cell::new(1_000u64);
        let now = || clock.get();
        let calls = std::cell::Cell::new(0);
        let mut ok = |_: &str| {
            calls.set(calls.get() + 1);
            Ok(b"{}".to_vec())
        };
        assert!(fetch_with(&gate, "u", false, &now, &mut ok).is_ok());
        assert!(fetch_with(&gate, "u", false, &now, &mut ok).is_ok());
        assert_eq!(calls.get(), 1, "второй раз — из памяти");
        assert!(fetch_with(&gate, "u", true, &now, &mut ok).is_ok());
        assert_eq!(calls.get(), 2, "«Обновить» идёт в сеть");
        clock.set(1_000 + CACHE_TTL_SECS * 1000);
        assert!(fetch_with(&gate, "u", false, &now, &mut ok).is_ok());
        assert_eq!(calls.get(), 3, "протухшее — заново");

        let mut too_many = |_: &str| Err(NetError::Status { code: 429, reason: "Too Many Requests".into() });
        let e = fetch_with(&gate, "v", false, &now, &mut too_many).unwrap_err();
        assert_eq!(e, FetchError::RateLimited { wait_secs: RATE_LIMIT_PAUSE_SECS });
        // После 429 в сеть не ходим вовсе, пока не выйдет пауза.
        let mut never = |_: &str| -> Result<Vec<u8>, NetError> { panic!("сеть после 429") };
        assert!(matches!(fetch_with(&gate, "w", false, &now, &mut never), Err(FetchError::RateLimited { .. })));
        assert!(e.message().contains("45"));
    }
}
