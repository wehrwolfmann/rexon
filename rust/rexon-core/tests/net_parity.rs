//! Golden-сверка СЕТЕВОГО слоя каталогов тем с оригиналами
//! `rexon/appearance/ocs_client.py` и `rexon/appearance/aur_client.py`.
//!
//! Эталон `golden/net_cases.json` снят с ЖИВОГО Python
//! одноразовым генератором: монипатчатся ровно границы — `urlopen`,
//! кеш (`get_cache`), `PREVIEW_DIR`, журнал, — а решения принимает сам
//! оригинал. Ответы подставляются из записанных фикстур (`ocs_list.xml`,
//! `ocs_comments.xml`, `aur_search_raw.json`), поэтому эталон снимается без
//! сети: `aur.archlinux.org` из среды разработки недоступен в принципе.
//!
//! Сверяется ВЕСЬ наблюдаемый след: какие URL запрошены, с каким
//! User-Agent и таймаутом, что записано в журнал, какие методы кеша вызваны
//! и в каком порядке, что осталось на диске и что вернулось наверх.
//!
//! Четыре места, где Rust НАМЕРЕННО расходится с эталоном (проверяются
//! отдельными утверждениями, а не побайтово):
//!   * текст ошибки разбора XML — у Python внутри сообщения диагностика
//!     `ElementTree` («unclosed token: line 1, column 0»), которой у нас нет;
//!     сверяется префикс и уровень;
//!   * текст ошибки разбора JSON — то же самое для `json.loads`;
//!   * имя заголовка — эталон пишет `User-agent` (так его нормализует
//!     `urllib`), мы передаём каноническое `User-Agent`; сверяется значение;
//!   * ⭐ ПОВТОРЫ при временном сбое — Python пробовал ровно один раз и на
//!     первом же обрыве соединения отдавал пустую страницу каталога либо
//!     несостоявшуюся установку темы. Ядро повторяет
//!     ([`rexon_core::net::with_retry`]), поэтому эталонный след запросов
//!     размножается на число попыток ([`assert_requests_retried`]), а
//!     извещения о повторах снимаются с журнала отдельно
//!     ([`assert_retries`]) — окончательный отказ по-прежнему оформляется
//!     тем же текстом и уровнем, что у оригинала.

use rexon_core::aur;

// Тексты ошибок AUR переводятся; эталон Python — русский, сверка идёт на
// каталоге `po/ru.po`.
mod i18n_pin;
use rexon_core::models::ThemeItem;
use rexon_core::net::{LogLevel, NetError, NetIo};
use rexon_core::ocs::{self, OcsCacheIo, OcsClient};
use serde_json::{json, Value};

fn cases() -> Value {
    serde_json::from_str(include_str!("golden/net_cases.json")).unwrap()
}

const LIST_XML: &[u8] = include_bytes!("golden/ocs_list.xml");
const COMMENTS_XML: &[u8] = include_bytes!("golden/ocs_comments.xml");
const AUR_RAW: &[u8] = include_bytes!("golden/aur_search_raw.json");

/// Тело, которым генератор кормил скачивание: 102 400 байт = два куска по
/// 65 536 и остаток.
fn body() -> Vec<u8> {
    (0..256u16).map(|b| b as u8).cycle().take(102_400).collect()
}

// ============================================================================
// Фальшивый транспорт: отдаёт записанные ответы, ведёт журнал вызовов
// ============================================================================

/// Заготовленный ответ на очередной запрос.
enum Reply {
    /// Тело + значение заголовка Content-Length (`None` — заголовка нет).
    Ok(Vec<u8>, Option<i64>),
    /// Соединение не состоялось (`URLError` оригинала).
    Fail(NetError),
    /// Соединение открылось, но чтение оборвалось (голый `OSError`).
    BrokenRead(Vec<u8>, Option<i64>, NetError),
}

#[derive(Default)]
struct FakeNet {
    replies: Vec<Reply>,
    /// (url, user_agent, timeout)
    requests: Vec<(String, String, u64)>,
    logs: Vec<(LogLevel, String)>,
    progress: Vec<(i64, i64)>,
    /// след файловых операций: `mkdir:…`, `create:…`, `write:N`, `close`,
    /// `remove:…`
    fs: Vec<String>,
    /// «диск»: путь → сколько байт записано
    files: std::collections::HashMap<String, usize>,
    stream: Option<(Vec<u8>, usize, Option<NetError>)>,
    /// `Content-Type` открытого ответа (у голден-случаев каталога не смотрится).
    ctype: String,
    open_file: Option<String>,
    /// Паузы перед повторами в миллисекундах — по одной на повтор.
    /// Настоящего ожидания в сверке нет: важен факт и длительность.
    slept: Vec<u64>,
    // кеш
    lists: Vec<Option<(Vec<ThemeItem>, i64)>>,
    items: Vec<Option<ThemeItem>>,
    /// Ответы `get_item_detail` (свежие полные детали).
    details: Vec<Option<ThemeItem>>,
    previews: Vec<Option<String>>,
    cache: Vec<Value>,
}

impl FakeNet {
    fn with(replies: Vec<Reply>) -> Self {
        FakeNet { replies: replies.into_iter().rev().collect(), ..Default::default() }
    }

    /// Один и тот же временный сбой на ВСЕ попытки ядра: так проверяется
    /// окончательный отказ (эталонный след Python повторяется целиком).
    fn always_failing(detail: &str) -> Self {
        Self::with(
            (0..ATTEMPTS)
                .map(|_| Reply::Fail(NetError::Transport { detail: detail.into() }))
                .collect(),
        )
    }

    fn next(&mut self, url: &str, ua: &str, timeout: u64) -> Reply {
        self.requests.push((url.to_string(), ua.to_string(), timeout));
        self.replies.pop().expect("лишний сетевой запрос")
    }

    /// Журнал БЕЗ извещений о повторах — то, что сверяется с эталоном Python.
    fn final_log_texts(&self) -> Vec<&str> {
        self.logs
            .iter()
            .map(|l| l.1.as_str())
            .filter(|m| !is_retry_notice(m))
            .collect()
    }
}

impl NetIo for FakeNet {
    fn get(&mut self, url: &str, ua: &str, timeout: u64) -> Result<Vec<u8>, NetError> {
        match self.next(url, ua, timeout) {
            Reply::Ok(body, _) => Ok(body),
            Reply::Fail(e) | Reply::BrokenRead(_, _, e) => Err(e),
        }
    }

    fn open(&mut self, url: &str, ua: &str, timeout: u64) -> Result<i64, NetError> {
        match self.next(url, ua, timeout) {
            Reply::Ok(b, total) => {
                let t = total.unwrap_or(0);
                self.stream = Some((b, 0, None));
                Ok(t)
            }
            Reply::BrokenRead(b, total, e) => {
                let t = total.unwrap_or(0);
                self.stream = Some((b, 0, Some(e)));
                Ok(t)
            }
            Reply::Fail(e) => Err(e),
        }
    }

    fn read(&mut self, n: usize) -> Result<Vec<u8>, NetError> {
        let Some((buf, pos, err)) = self.stream.as_mut() else {
            return Ok(Vec::new());
        };
        if let Some(e) = err.clone() {
            return Err(e);
        }
        let end = (*pos + n).min(buf.len());
        let out = buf[*pos..end].to_vec();
        *pos = end;
        Ok(out)
    }

    fn close(&mut self) {
        self.stream = None;
    }

    fn makedirs(&mut self, path: &str) {
        self.fs.push(format!("mkdir:{path}"));
    }

    fn create(&mut self, path: &str) -> Result<(), NetError> {
        self.fs.push(format!("create:{path}"));
        self.files.insert(path.to_string(), 0);
        self.open_file = Some(path.to_string());
        Ok(())
    }

    fn write(&mut self, chunk: &[u8]) -> Result<(), NetError> {
        self.fs.push(format!("write:{}", chunk.len()));
        if let Some(p) = self.open_file.clone() {
            *self.files.entry(p).or_insert(0) += chunk.len();
        }
        Ok(())
    }

    fn close_file(&mut self) {
        self.fs.push("close".to_string());
        self.open_file = None;
    }

    fn exists(&mut self, path: &str) -> bool {
        self.files.contains_key(path)
    }

    fn remove(&mut self, path: &str) {
        self.fs.push(format!("remove:{path}"));
        self.files.remove(path);
    }

    fn progress(&mut self, done: i64, total: i64) {
        self.progress.push((done, total));
    }

    fn log(&mut self, level: LogLevel, message: &str) {
        self.logs.push((level, message.to_string()));
    }

    fn sleep(&mut self, ms: u64) {
        self.slept.push(ms);
    }

    fn content_type(&mut self) -> String {
        self.ctype.clone()
    }
}

impl OcsCacheIo for FakeNet {
    fn cache_get_list(&mut self, key: &str) -> Option<(Vec<ThemeItem>, i64)> {
        let out = if self.lists.is_empty() { None } else { self.lists.remove(0) };
        self.cache
            .push(json!({"call": "get_list", "key": key, "hit": out.is_some()}));
        out
    }

    fn cache_put_list(&mut self, key: &str, items: &[ThemeItem], total: i64) {
        let ids: Vec<&str> = items.iter().map(|i| i.id.as_str()).collect();
        self.cache
            .push(json!({"call": "put_list", "key": key, "total": total, "ids": ids}));
    }

    fn cache_get_item(&mut self, content_id: &str) -> Option<ThemeItem> {
        let out = if self.items.is_empty() { None } else { self.items.remove(0) };
        self.cache
            .push(json!({"call": "get_item", "id": content_id, "hit": out.is_some()}));
        out
    }

    fn cache_put_item(&mut self, item: &ThemeItem) {
        self.cache.push(json!({"call": "put_item", "id": item.id}));
    }

    fn cache_get_item_detail(&mut self, content_id: &str, max_age_secs: u64) -> Option<ThemeItem> {
        let out = if self.details.is_empty() { None } else { self.details.remove(0) };
        self.cache.push(json!({
            "call": "get_item_detail", "id": content_id,
            "max_age": max_age_secs, "hit": out.is_some()
        }));
        out
    }

    fn cache_get_preview(&mut self, url: &str) -> Option<String> {
        let out = if self.previews.is_empty() { None } else { self.previews.remove(0) };
        self.cache.push(json!({
            "call": "get_preview", "url": url,
            "hit": out.as_deref().map(|s| !s.is_empty()).unwrap_or(false)
        }));
        out
    }

    fn cache_put_preview(&mut self, url: &str, local_path: &str) {
        let name = local_path.rsplit('/').next().unwrap_or(local_path);
        self.cache
            .push(json!({"call": "put_preview", "url": url, "name": name}));
    }
}

// ============================================================================
// Помощники сверки
// ============================================================================

/// Сколько ВСЕГО попыток делает ядро на временный сбой (первая + повторы).
const ATTEMPTS: usize = rexon_core::net::RETRY_ATTEMPTS as usize;

/// Паузы, которые ядро выдерживает между попытками обычного (не 429) сбоя.
const WAITS: &[u64] = &[700, 1400];

/// Строка журнала о повторе — НАШЕ добавление, у эталона её нет.
fn is_retry_notice(msg: &str) -> bool {
    msg.contains("— повтор через")
}

/// Один запрос совпал с эталоном: URL, User-Agent и таймаут.
/// Имя заголовка эталон пишет как `User-agent` (нормализация `urllib`) —
/// сверяется ЗНАЧЕНИЕ, регистр имени на проводе не значим.
fn assert_requests(io: &FakeNet, want: &Value, ctx: &str) {
    assert_requests_retried(io, want, ctx, 1);
}

/// То же, но эталонный след повторён `attempts` раз: временный сбой ядро
/// пробует заново, Python пробовал один раз (осознанное расхождение).
fn assert_requests_retried(io: &FakeNet, want: &Value, ctx: &str, attempts: usize) {
    let want = want["requests"].as_array().unwrap();
    assert_eq!(
        io.requests.len(),
        want.len() * attempts,
        "{ctx}: число запросов ({attempts} попыт.)"
    );
    for (i, got) in io.requests.iter().enumerate() {
        let w = &want[i % want.len()];
        assert_eq!(got.0, w["url"].as_str().unwrap(), "{ctx}: URL");
        assert_eq!(
            got.1,
            w["headers"]["User-agent"].as_str().unwrap(),
            "{ctx}: User-Agent"
        );
        assert_eq!(got.2 as i64, w["timeout"].as_i64().unwrap(), "{ctx}: таймаут");
    }
}

/// Повторы отработали ровно так, как задумано: столько-то извещений в журнал,
/// столько-то пауз и ровно такой длины.
fn assert_retries(io: &FakeNet, ctx: &str, retries: usize) {
    let notices = io.logs.iter().filter(|l| is_retry_notice(&l.1)).count();
    assert_eq!(notices, retries, "{ctx}: извещений о повторе");
    assert_eq!(io.slept, WAITS[..retries].to_vec(), "{ctx}: паузы между попытками");
}

fn assert_cache(io: &FakeNet, want: &Value, ctx: &str) {
    assert_eq!(
        Value::Array(io.cache.clone()),
        want["cache"],
        "{ctx}: журнал кеша"
    );
}

/// Журнал БЕЗ извещений о повторах совпадает с эталоном: окончательный отказ
/// оформляется тем же текстом и уровнем, что у Python.
fn assert_logs(io: &FakeNet, want: &Value, ctx: &str) {
    let want = want["logs"].as_array().unwrap();
    let logs: Vec<&(LogLevel, String)> =
        io.logs.iter().filter(|l| !is_retry_notice(&l.1)).collect();
    assert_eq!(logs.len(), want.len(), "{ctx}: число записей журнала");
    for ((level, msg), w) in logs.into_iter().zip(want) {
        let want_level = match w["level"].as_str().unwrap() {
            "info" => LogLevel::Info,
            "warning" => LogLevel::Warning,
            "error" => LogLevel::Error,
            other => panic!("{ctx}: неизвестный уровень {other}"),
        };
        assert_eq!(*level, want_level, "{ctx}: уровень");
        assert_eq!(msg, w["message"].as_str().unwrap(), "{ctx}: текст");
    }
}

/// Полная сверка следа (запросы + журнал + кеш).
fn assert_trace(io: &FakeNet, want: &Value, ctx: &str) {
    assert_requests(io, want, ctx);
    assert_logs(io, want, ctx);
    assert_cache(io, want, ctx);
}

fn ids(items: &[ocs::OcsContent]) -> Vec<&str> {
    items.iter().map(|c| c.item.id.as_str()).collect()
}

fn want_ids(want: &Value) -> Vec<String> {
    want["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["id"].as_str().unwrap().to_string())
        .collect()
}

fn client(io: FakeNet) -> OcsClient<FakeNet> {
    OcsClient::new(io, "/tmp/previews")
}

fn cached_item(id: &str, name: &str, category_id: i64) -> ThemeItem {
    ThemeItem {
        id: id.to_string(),
        name: name.to_string(),
        category_id,
        ..ThemeItem::default()
    }
}

// ============================================================================
// OCS: search
// ============================================================================

/// Сеть отвечает РЕАЛЬНЫМ ответом каталога: тот же URL, тот же ключ кеша,
/// те же элементы и totalitems, страница легла в кеш.
#[test]
fn search_network_ok_matches_python() {
    let want = cases()["search_network_ok"].clone();
    let io = FakeNet::with(vec![Reply::Ok(LIST_XML.to_vec(), None)]);
    let mut c = client(io);
    let (items, total) = c.search("104", 0, 20, "new", "", true);

    assert_eq!(total, want["total"].as_i64().unwrap(), "totalitems");
    assert_eq!(ids(&items), want_ids(&want), "элементы");
    for (got, w) in items.iter().zip(want["items"].as_array().unwrap()) {
        assert_eq!(got.item.name, w["name"].as_str().unwrap(), "имя");
        assert_eq!(
            got.item.category_id,
            w["category_id"].as_i64().unwrap(),
            "category_id"
        );
        assert_eq!(
            got.comment_count,
            w["comment_count"].as_i64().unwrap(),
            "comment_count"
        );
    }
    assert_trace(&c.into_io(), &want, "search_network_ok");
}

/// Свежий кеш — сети не касаемся вовсе.
#[test]
fn search_cache_hit_matches_python() {
    let want = cases()["search_cache_hit"].clone();
    let mut io = FakeNet::with(vec![]);
    io.lists = vec![Some((vec![cached_item("777", "из кеша", 104)], 42))];
    let mut c = client(io);
    let (items, total) = c.search("104", 0, 20, "new", "", true);

    assert_eq!(total, 42);
    assert_eq!(ids(&items), want_ids(&want));
    // comment_count через SQLite не проходит: у Python это отсутствующий
    // атрибут (null в эталоне), у нас — ноль, ровно как видит интерфейс.
    assert!(want["items"][0]["comment_count"].is_null());
    assert_eq!(items[0].comment_count, 0);
    assert_trace(&c.into_io(), &want, "search_cache_hit");
}

/// Сеть упала — в дело идёт УСТАРЕВШИЙ кеш (второй `get_list` тем же ключом),
/// в журнал уходит предупреждение с URL и текстом ошибки.
#[test]
fn search_net_fail_falls_back_to_stale_cache() {
    let want = cases()["search_net_fail_stale"].clone();
    let mut io = FakeNet::always_failing("<urlopen error boom>");
    io.lists = vec![None, Some((vec![cached_item("777", "из кеша", 104)], 42))];
    let mut c = client(io);
    let (items, total) = c.search("104", 1, 20, "down", " Тёмная ", true);

    assert_eq!(total, 42);
    assert_eq!(ids(&items), want_ids(&want));
    let io = c.into_io();
    assert_requests_retried(&io, &want, "search_net_fail_stale", ATTEMPTS);
    assert_retries(&io, "search_net_fail_stale", ATTEMPTS - 1);
    assert_logs(&io, &want, "search_net_fail_stale");
    assert_cache(&io, &want, "search_net_fail_stale");
}

/// Сеть упала и кеша нет — пусто, страница в кеш НЕ кладётся.
#[test]
fn search_net_fail_without_cache_is_empty() {
    let want = cases()["search_net_fail_empty"].clone();
    let mut io = FakeNet::always_failing("<urlopen error boom>");
    io.lists = vec![None, None];
    let mut c = client(io);
    let (items, total) = c.search("104", 0, 20, "new", "", true);

    assert!(items.is_empty());
    assert_eq!(total, 0);
    let io = c.into_io();
    assert_requests_retried(&io, &want, "search_net_fail_empty", ATTEMPTS);
    assert_retries(&io, "search_net_fail_empty", ATTEMPTS - 1);
    assert_logs(&io, &want, "search_net_fail_empty");
    assert_cache(&io, &want, "search_net_fail_empty");
}

/// `status != ok`: пустой результат и предупреждение с кодом — как у
/// оригинала. ⚠️ Расхождение с эталоном намеренное: отказ магазина НЕ ложится
/// в кеш пустой страницей (Python клал — и час после сбоя раздел стоял
/// пустым, хотя магазин уже ожил); вместо записи — второй взгляд в кеш.
#[test]
fn search_status_not_ok_matches_python() {
    let want = cases()["search_status_not_ok"].clone();
    let xml = br#"<?xml version="1.0"?><ocs><meta><status>failed</status><statuscode>999</statuscode><message>nope</message></meta><data/></ocs>"#;
    let mut io = FakeNet::with(vec![Reply::Ok(xml.to_vec(), None)]);
    io.lists = vec![None, None];
    let mut c = client(io);
    let (items, total) = c.search("104", 0, 20, "new", "", true);

    assert!(items.is_empty());
    assert_eq!(total, 0);
    let io = c.into_io();
    assert_requests(&io, &want, "search_status_not_ok");
    assert_logs(&io, &want, "search_status_not_ok");
    assert_no_list_written(&io, "search_status_not_ok");
}

/// Битый XML: пустой результат и сообщение уровня error. Текст расходится
/// намеренно — внутри Python-сообщения диагностика `ElementTree`.
#[test]
fn search_broken_xml_matches_python() {
    let want = cases()["search_broken_xml"].clone();
    let mut io = FakeNet::with(vec![Reply::Ok(b"<not xml".to_vec(), None)]);
    io.lists = vec![None, None];
    let mut c = client(io);
    let (items, total) = c.search("104", 0, 20, "new", "", true);

    assert!(items.is_empty());
    assert_eq!(total, 0);
    let io = c.into_io();
    assert_requests(&io, &want, "search_broken_xml");
    // Нечитаемый ответ — не «товаров нет»: в кеш не пишется (см. выше).
    assert_no_list_written(&io, "search_broken_xml");
    assert_eq!(io.logs.len(), 1);
    assert_eq!(io.logs[0].0, LogLevel::Error);
    let prefix = "OCS XML parse error: ";
    assert!(want["logs"][0]["message"].as_str().unwrap().starts_with(prefix));
    assert!(io.logs[0].1.starts_with(prefix), "{}", io.logs[0].1);
}

/// Категория-зонт «716,488»: URL и ключ кеша совпадают с оригиналом, а вот
/// пустой typeid ПОЧИНЕН — Python клал в целочисленное поле строку.
#[test]
fn search_umbrella_category_fixes_string_in_int_field() {
    let want = cases()["search_umbrella_category"].clone();
    let xml = br#"<?xml version="1.0"?><ocs><meta><status>ok</status><statuscode>100</statuscode><totalitems>1</totalitems></meta><data><content><id>5</id><name>Splash</name><typeid></typeid></content></data></ocs>"#;
    let mut io = FakeNet::with(vec![Reply::Ok(xml.to_vec(), None)]);
    io.lists = vec![None];
    let mut c = client(io);
    let (items, total) = c.search("716,488", 0, 20, "high", "", true);

    assert_eq!(total, 1);
    assert_eq!(ids(&items), want_ids(&want));
    // эталон: строка «716,488» в поле типа int
    assert_eq!(want["items"][0]["category_id_type"], "str");
    assert_eq!(want["items"][0]["category_id"], "716,488");
    // починка: неразбираемый зонт оставляет 0
    assert_eq!(items[0].item.category_id, 0);
    assert_trace(&c.into_io(), &want, "search_umbrella_category");
}

/// `cached_search` — только кеш, без единого запроса.
#[test]
fn cached_search_never_touches_network() {
    let mut io = FakeNet::with(vec![]);
    io.lists = vec![Some((vec![cached_item("777", "из кеша", 104)], 42))];
    let mut c = client(io);
    let got = c.cached_search("104", 0, "new", "");
    assert_eq!(got.as_ref().map(|(i, t)| (ids(i), *t)), Some((vec!["777"], 42)));
    let io = c.into_io();
    assert!(io.requests.is_empty(), "cached_search не ходит в сеть");
    assert_eq!(io.cache.len(), 1);
}

// ============================================================================
// OCS: get_item / comments
// ============================================================================

#[test]
fn get_item_ok_matches_python() {
    let want = cases()["get_item_ok"].clone();
    let mut c = client(FakeNet::with(vec![Reply::Ok(LIST_XML.to_vec(), None)]));
    let got = c.get_item("2358391").expect("элемент");

    assert_eq!(got.item.id, want["item"]["id"].as_str().unwrap());
    assert_eq!(got.item.name, want["item"]["name"].as_str().unwrap());
    assert_eq!(got.comment_count, want["item"]["comment_count"].as_i64().unwrap());
    assert_trace(&c.into_io(), &want, "get_item_ok");
}

#[test]
fn get_item_net_fail_falls_back_to_cache() {
    let want = cases()["get_item_net_fail"].clone();
    let mut io = FakeNet::always_failing("<urlopen error boom>");
    io.items = vec![Some(cached_item("2358391", "из кеша", 0))];
    let mut c = client(io);
    let got = c.get_item("2358391").expect("элемент из кеша");

    assert_eq!(got.item.name, "из кеша");
    let io = c.into_io();
    assert_requests_retried(&io, &want, "get_item_net_fail", ATTEMPTS);
    assert_retries(&io, "get_item_net_fail", ATTEMPTS - 1);
    assert_cache(&io, &want, "get_item_net_fail");
    // предупреждение _http_get здесь тоже пишется — сверх извещений о повторах
    assert_eq!(io.final_log_texts().len(), 1);
}

/// Детали для показа: свежие из кеша — БЕЗ сети; промах — обычный запрос.
#[test]
fn get_item_cached_uses_fresh_detail_and_skips_network() {
    use rexon_core::ocs::DETAIL_TTL_SECS;

    let mut io = FakeNet::with(Vec::new());
    io.details = vec![Some(cached_item("2358391", "свежие детали", 0))];
    let mut c = client(io);
    let got = c.get_item_cached("2358391", DETAIL_TTL_SECS).expect("из кеша");
    assert_eq!(got.item.name, "свежие детали");
    let io = c.into_io();
    assert!(io.requests.is_empty(), "свежие детали не ходят в сеть");
    assert_eq!(
        io.cache,
        vec![json!({"call": "get_item_detail", "id": "2358391",
                    "max_age": 86400, "hit": true})]
    );

    let mut io = FakeNet::with(vec![Reply::Ok(LIST_XML.to_vec(), None)]);
    io.details = vec![None];
    let mut c = client(io);
    let got = c.get_item_cached("2358391", DETAIL_TTL_SECS).expect("из сети");
    // Фикстура — ответ-список: разбор деталей берёт первый товар ответа.
    assert_eq!(got.item.id, "1352754");
    let io = c.into_io();
    assert_eq!(io.requests.len(), 1, "промах кеша — один сетевой запрос");
    assert_eq!(io.cache[0]["call"], "get_item_detail");
    assert_eq!(io.cache.last().unwrap()["call"], "put_item");
}

/// Ответ не разобрать (нет `<data>`) → кеш; кеш пуст → None.
#[test]
fn get_item_malformed_falls_back_to_cache() {
    let want = cases()["get_item_malformed"].clone();
    let mut io = FakeNet::with(vec![Reply::Ok(b"<ocs><meta/></ocs>".to_vec(), None)]);
    io.items = vec![None];
    let mut c = client(io);
    assert!(c.get_item("2358391").is_none());
    assert_trace(&c.into_io(), &want, "get_item_malformed");
}

/// Разобрано, но `id` пуст → None БЕЗ обращения к кешу.
#[test]
fn get_item_empty_id_skips_cache() {
    let want = cases()["get_item_empty_id"].clone();
    let xml = br#"<?xml version="1.0"?><ocs><meta><status>ok</status></meta><data><content><id></id><name>no id</name></content></data></ocs>"#;
    let mut c = client(FakeNet::with(vec![Reply::Ok(xml.to_vec(), None)]));
    assert!(c.get_item("0").is_none());
    assert_trace(&c.into_io(), &want, "get_item_empty_id");
}

#[test]
fn comments_ok_matches_python() {
    let want = cases()["comments_ok"].clone();
    let mut c = client(FakeNet::with(vec![Reply::Ok(COMMENTS_XML.to_vec(), None)]));
    let got = c.comments("1294729", 0, 10).expect("комментарии");

    assert_eq!(got.len() as i64, want["count"].as_i64().unwrap());
    let first = &want["first"];
    assert_eq!(got[0].author, first["author"].as_str().unwrap());
    assert_eq!(got[0].date, first["date"].as_str().unwrap());
    assert_eq!(got[0].text, first["text"].as_str().unwrap());
    assert_eq!(got[0].depth, first["depth"].as_i64().unwrap());
    assert_trace(&c.into_io(), &want, "comments_ok");
}

#[test]
fn comments_net_fail_is_none() {
    let want = cases()["comments_net_fail"].clone();
    let mut c = client(FakeNet::always_failing("<urlopen error boom>"));
    assert!(c.comments("1294729", 0, 10).is_none());
    assert!(want["result"].is_null());
    let io = c.into_io();
    assert_requests_retried(&io, &want, "comments_net_fail", ATTEMPTS);
    assert_retries(&io, "comments_net_fail", ATTEMPTS - 1);
    assert_cache(&io, &want, "comments_net_fail");
}

/// Код 101 — «комментариев нет», а не ошибка: пустой список и ни слова в журнал.
#[test]
fn comments_101_is_empty_list() {
    let want = cases()["comments_101"].clone();
    let xml = br#"<?xml version="1.0"?><ocs><meta><status>failed</status><statuscode>101</statuscode></meta><data/></ocs>"#;
    let mut c = client(FakeNet::with(vec![Reply::Ok(xml.to_vec(), None)]));
    assert_eq!(c.comments("2321785", 2, 10), Some(Vec::new()));
    assert_eq!(want["result"], json!([]));
    assert_trace(&c.into_io(), &want, "comments_101");
}

/// Прочий неуспешный код — None и предупреждение.
#[test]
fn comments_bad_status_is_none() {
    let want = cases()["comments_status_bad"].clone();
    let xml = br#"<?xml version="1.0"?><ocs><meta><status>failed</status><statuscode>403</statuscode></meta><data/></ocs>"#;
    let mut c = client(FakeNet::with(vec![Reply::Ok(xml.to_vec(), None)]));
    assert!(c.comments("1", 0, 10).is_none());
    assert!(want["result"].is_null());
    assert_trace(&c.into_io(), &want, "comments_status_bad");
}

// ============================================================================
// OCS: превью и скачивание
// ============================================================================

const PREVIEW_URL: &str = "https://images.pling.com/cache/770x540-4/img/00/00/1.jpg";

#[test]
fn preview_cache_hit_skips_download() {
    let want = cases()["preview_cache_hit"].clone();
    let mut io = FakeNet::with(vec![]);
    io.previews = vec![Some("/уже/скачано.jpg".into())];
    let mut c = client(io);
    assert_eq!(
        c.get_preview_image(PREVIEW_URL).as_deref(),
        want["path"].as_str()
    );
    assert_trace(&c.into_io(), &want, "preview_cache_hit");
}

/// Промах кеша → скачивание с ТЕМ ЖЕ именем файла (sha1 URL + расширение),
/// таймаутом 20 с и записью в кеш.
#[test]
fn preview_download_matches_python() {
    let want = cases()["preview_download_ok"].clone();
    let b = body();
    let mut io = FakeNet::with(vec![Reply::Ok(b.clone(), Some(b.len() as i64))]);
    io.previews = vec![None];
    let mut c = client(io);
    let path = c.get_preview_image(PREVIEW_URL).expect("путь превью");

    let name = want["name"].as_str().unwrap();
    assert_eq!(path, format!("/tmp/previews/{name}"));
    let io = c.into_io();
    assert_trace(&io, &want, "preview_download_ok");
    assert_eq!(
        io.files.get(&path).copied(),
        Some(want["fs"]["size"].as_u64().unwrap() as usize),
        "размер записанного файла"
    );
    // прогресс у превью не запрашивается (on_progress=None у оригинала)
    assert!(io.progress.is_empty());
}

#[test]
fn preview_download_fail_returns_none() {
    let want = cases()["preview_download_fail"].clone();
    let mut io = FakeNet::always_failing("<urlopen error нет сети>");
    io.previews = vec![None];
    let mut c = client(io);
    assert!(c.get_preview_image(PREVIEW_URL).is_none());
    assert!(want["path"].is_null());
    let io = c.into_io();
    assert_requests_retried(&io, &want, "preview_download_fail", ATTEMPTS);
    assert_retries(&io, "preview_download_fail", ATTEMPTS - 1);
    assert_logs(&io, &want, "preview_download_fail");
    assert_cache(&io, &want, "preview_download_fail");
}

#[test]
fn preview_empty_url_is_none() {
    let want = cases()["preview_empty_url"].clone();
    let mut c = client(FakeNet::with(vec![]));
    assert!(c.get_preview_image("").is_none());
    assert!(want["path"].is_null());
    assert_trace(&c.into_io(), &want, "preview_empty_url");
}

fn theme_with_links() -> ThemeItem {
    ThemeItem {
        id: "1".into(),
        name: "Bibata".into(),
        download_url: "https://pling.com/p/1/fallback.tar.gz".into(),
        download_links: vec![
            json!({"url": "https://pling.com/p/1/Bibata-Windows.zip",
                   "name": "Bibata-Windows.zip", "size": 10}),
            json!({"url": "https://pling.com/p/1/Bibata Modern Ice.tar.xz",
                   "name": "Bibata Modern Ice.tar.xz", "size": 20}),
        ],
        ..ThemeItem::default()
    }
}

/// Автовыбор линка: Windows-сборка отбрасывается, берётся `.tar.xz`, пробел в
/// имени файла экранируется, прогресс идёт кусками по 65 536.
#[test]
fn download_picks_best_link_and_reports_progress() {
    let want = cases()["download_best_link"].clone();
    let b = body();
    let io = FakeNet::with(vec![Reply::Ok(b.clone(), Some(b.len() as i64))]);
    let mut c = client(io);
    let dest = "/tmp/rexon-test/sub/theme.tar.xz";
    assert!(c.download(&theme_with_links(), dest, -1, true));

    let io = c.into_io();
    assert_trace(&io, &want, "download_best_link");
    let want_progress: Vec<(i64, i64)> = want["progress"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| (p[0].as_i64().unwrap(), p[1].as_i64().unwrap()))
        .collect();
    assert_eq!(io.progress, want_progress, "прогресс");
    assert_eq!(io.files.get(dest).copied(), Some(b.len()));
    assert!(io.fs.contains(&"mkdir:/tmp/rexon-test/sub".to_string()));
}

/// Явный индекс перебивает автовыбор.
#[test]
fn download_explicit_link_matches_python() {
    let want = cases()["download_explicit_link"].clone();
    let b = body();
    let io = FakeNet::with(vec![Reply::Ok(b.clone(), Some(b.len() as i64))]);
    let mut c = client(io);
    assert!(c.download(&theme_with_links(), "/tmp/rexon-test/sub/theme2.zip", 0, false));
    let io = c.into_io();
    assert_trace(&io, &want, "download_explicit_link");
    assert!(io.progress.is_empty(), "без колбэка прогресс не зовётся");
}

/// Файл, выбранный руками в панели деталей, доходит до скачивания: качается
/// он, а не автоматический (здесь — даже Windows-сборка, раз её выбрали).
#[test]
fn download_takes_the_file_chosen_by_name() {
    let item = theme_with_links();
    let b = body();
    let io = FakeNet::with(vec![Reply::Ok(b.clone(), Some(b.len() as i64))]);
    let mut c = client(io);
    let idx = ocs::file_index_for(&item, "Bibata-Windows.zip");
    assert!(c.download(&item, "/tmp/rexon-test/sub/chosen.zip", idx, false));
    let io = c.into_io();
    assert_eq!(io.requests.len(), 1);
    assert_eq!(io.requests[0].0, "https://pling.com/p/1/Bibata-Windows.zip");

    // Имени нет среди файлов — автоматический выбор, как у плитки.
    let io = FakeNet::with(vec![Reply::Ok(b.clone(), Some(b.len() as i64))]);
    let mut c = client(io);
    let idx = ocs::file_index_for(&item, "");
    assert!(c.download(&item, "/tmp/rexon-test/sub/auto.tar.xz", idx, false));
    assert_eq!(c.into_io().requests[0].0, "https://pling.com/p/1/Bibata%20Modern%20Ice.tar.xz");
}

/// Линков нет — берётся download_url.
#[test]
fn download_falls_back_to_download_url() {
    let want = cases()["download_url_fallback"].clone();
    let b = body();
    let io = FakeNet::with(vec![Reply::Ok(b.clone(), Some(b.len() as i64))]);
    let item = ThemeItem {
        id: "3".into(),
        name: "Y".into(),
        download_url: "https://pling.com/p/3/only.tar.gz".into(),
        ..ThemeItem::default()
    };
    let mut c = client(io);
    assert!(c.download(&item, "/tmp/rexon-test/fallback.tar.gz", -1, false));
    assert_trace(&c.into_io(), &want, "download_url_fallback");
}

/// Ни линков, ни URL — false без единого запроса.
#[test]
fn download_without_url_is_false() {
    let want = cases()["download_no_url"].clone();
    let item = ThemeItem { id: "2".into(), name: "X".into(), ..ThemeItem::default() };
    let mut c = client(FakeNet::with(vec![]));
    assert!(!c.download(&item, "/tmp/rexon-test/none.zip", -1, false));
    assert!(!want["ok"].as_bool().unwrap());
    assert_trace(&c.into_io(), &want, "download_no_url");
}

/// Content-Length больше потолка — файл даже не создаётся.
#[test]
fn download_too_big_by_header_matches_python() {
    let want = cases()["download_too_big_header"].clone();
    let io = FakeNet::with(vec![Reply::Ok(body(), Some(700 * 1024 * 1024))]);
    let item = ThemeItem {
        id: "3".into(),
        download_url: "https://pling.com/p/3/only.tar.gz".into(),
        ..ThemeItem::default()
    };
    let mut c = client(io);
    let dest = "/tmp/rexon-test/big.tar.gz";
    assert!(!c.download(&item, dest, -1, false));
    let io = c.into_io();
    assert_trace(&io, &want, "download_too_big_header");
    assert!(!io.files.contains_key(dest), "файл не создавался");
    assert!(io.fs.is_empty(), "ни одной файловой операции");
}

/// Заголовка нет, а тело переросло потолок — недокачанный файл удаляется,
/// в журнал при этом НИЧЕГО не пишется (страховочная ветка оригинала).
#[test]
fn download_overrun_body_removes_file() {
    let want = cases()["download_overrun_body"].clone();
    let mut io = FakeNet::with(vec![Reply::Ok(body(), None)]);
    let dest = "/tmp/rexon-test/overrun.bin";
    let ok = ocs::http_download(
        &mut io,
        "https://pling.com/p/1/x.tar.gz",
        dest,
        false,
        120,
        100,
    );
    assert!(!ok);
    assert_requests(&io, &want, "download_overrun_body");
    assert_logs(&io, &want, "download_overrun_body");
    assert!(!io.files.contains_key(dest), "файл удалён");
    assert!(io.fs.contains(&format!("remove:{dest}")));
}

/// Обрыв чтения посередине — сообщение об ошибке и удаление огрызка.
#[test]
fn download_broken_stream_removes_file() {
    let want = cases()["download_broken_stream"].clone();
    let mut io = FakeNet::with(
        (0..ATTEMPTS)
            .map(|_| {
                Reply::BrokenRead(
                    Vec::new(),
                    Some(10),
                    NetError::Transport { detail: "соединение разорвано".into() },
                )
            })
            .collect(),
    );
    let dest = "/tmp/rexon-test/broken.bin";
    let ok = ocs::http_download(&mut io, "https://pling.com/p/1/x.tar.gz", dest, false, 120, 0);
    assert!(!ok);
    assert_requests_retried(&io, &want, "download_broken_stream", ATTEMPTS);
    assert_retries(&io, "download_broken_stream", ATTEMPTS - 1);
    assert_logs(&io, &want, "download_broken_stream");
    assert!(!io.files.contains_key(dest), "огрызок удалён");
}

/// Обрыв на середине архива → вторая попытка скачивает его ЦЕЛИКОМ, а не
/// дописывает хвост к огрызку: файл создаётся заново и на диске ровно столько
/// байт, сколько в теле. Регресс к «тема не установилась из-за одного обрыва».
#[test]
fn download_retries_whole_file_after_broken_stream() {
    let b = body();
    let mut io = FakeNet::with(vec![
        // Первая попытка: ответ открылся, а чтение тела оборвалось.
        Reply::BrokenRead(
            b[..b.len() / 2].to_vec(),
            Some(b.len() as i64),
            NetError::Transport { detail: "соединение разорвано".into() },
        ),
        // Вторая: сервер отдаёт всё.
        Reply::Ok(b.clone(), Some(b.len() as i64)),
    ]);
    let dest = "/tmp/rexon-test/retry.tar.xz";
    assert!(ocs::http_download(
        &mut io,
        "https://pling.com/p/1/x.tar.xz",
        dest,
        false,
        120,
        0
    ));
    assert_eq!(io.requests.len(), 2, "ответ обязан открыться заново");
    assert_eq!(
        io.files.get(dest).copied(),
        Some(b.len()),
        "файл должен быть ровно один целый, а не полтора"
    );
    // Огрызок затирается созданием файла заново, а не дописывается.
    assert_eq!(
        io.fs.iter().filter(|s| s.starts_with("create:")).count(),
        2,
        "каждая попытка создаёт файл заново"
    );
    assert_retries(&io, "download_retry", 1);
    // Окончательного «OCS download failed» быть не должно — тема доехала.
    assert!(io.final_log_texts().is_empty(), "успех не пишет об ошибке");
}

/// Отмена (покупатель ушёл с категории) повтором НЕ считается: один запрос,
/// ни одной паузы. Иначе отмена, ради которой освобождали канал, тут же
/// занимала бы его снова.
#[test]
fn cancelled_request_is_not_retried() {
    let mut io = FakeNet::with(vec![Reply::Fail(NetError::Transport {
        detail: format!("<urlopen error {}>", rexon_core::net::CANCELLED_DETAIL),
    })]);
    assert!(ocs::http_get(&mut io, "https://api.opendesktop.org/x", 25).is_none());
    assert_eq!(io.requests.len(), 1, "отменённый запрос повторять нельзя");
    assert!(io.slept.is_empty(), "и ждать перед несделанным повтором тоже");
}

/// Форма текста, доезжающего до покупателя: скобки `<urlopen error …>` ставит
/// транспорт (сбой соединения), а код ответа печатается как у `HTTPError`.
#[test]
fn error_texts_match_python() {
    let want = cases()["exception_texts"].clone();
    let transport = NetError::Transport { detail: "<urlopen error boom>".into() };
    assert_eq!(transport.to_string(), want["url_error"].as_str().unwrap());
    let status = NetError::Status { code: 404, reason: "Not Found".into() };
    assert_eq!(status.to_string(), want["http_error"].as_str().unwrap());
}

/// Человеческие формулировки — наше дополнение; по-русски дословно как
/// было до перевода.
#[test]
fn russian_human_errors_read_as_before() {
    let _lang = i18n_pin::ru();
    let status = NetError::Status { code: 404, reason: "Not Found".into() };
    assert_eq!(status.human(), "Сервер каталога ответил ошибкой 404 (Not Found)");
    let bare = NetError::Status { code: 503, reason: String::new() };
    assert_eq!(bare.human(), "Сервер каталога ответил ошибкой 503");
    let transport = NetError::Transport { detail: "<urlopen error boom>".into() };
    assert_eq!(
        transport.human(),
        "Не удалось связаться с сервером каталога — проверьте подключение к интернету"
    );
}

/// Те же формулировки по-английски — без единой русской буквы.
#[test]
fn english_human_errors_have_no_russian() {
    let _lang = i18n_pin::en();
    let status = NetError::Status { code: 404, reason: "Not Found".into() };
    assert_eq!(status.human(), "The catalog server returned error 404 (Not Found)");
    let bare = NetError::Status { code: 503, reason: String::new() };
    assert_eq!(bare.human(), "The catalog server returned error 503");
    let transport = NetError::Transport { detail: "<urlopen error boom>".into() };
    let text = transport.human();
    assert_eq!(text, "Couldn't reach the catalog server — check your internet connection");
    for t in [status.human(), bare.human(), text] {
        assert!(!t.chars().any(|c| ('\u{0400}'..='\u{04FF}').contains(&c)), "{t}");
    }
}

// ============================================================================
// AUR
// ============================================================================

fn aur_io(n: usize) -> FakeNet {
    FakeNet::with((0..n).map(|_| Reply::Ok(AUR_RAW.to_vec(), None)).collect())
}

fn names(pkgs: &[aur::AurPackage]) -> Vec<&str> {
    pkgs.iter().map(|p| p.name.as_str()).collect()
}

fn want_names(want: &Value) -> Vec<&str> {
    want["names"].as_array().unwrap().iter().map(|n| n.as_str().unwrap()).collect()
}

#[test]
fn aur_search_matches_python() {
    let want = cases()["aur_search_ok"].clone();
    let mut io = aur_io(1);
    let got = aur::search_aur(&mut io, "aerothemeplasma", None).unwrap();
    assert_eq!(names(&got), want_names(&want));
    assert_requests(&io, &want, "aur_search_ok");
}

/// Многословный запрос: в RPC уходит самое длинное слово, остальные — местный
/// AND-фильтр; `limit` режет хвост.
#[test]
fn aur_search_multiword_limit_matches_python() {
    let want = cases()["aur_search_multiword_limit"].clone();
    let mut io = aur_io(1);
    let got = aur::search_aur(&mut io, "aerothemeplasma desktop", Some(3)).unwrap();
    assert_eq!(names(&got), want_names(&want));
    assert_requests(&io, &want, "aur_search_multiword_limit");
}

#[test]
fn aur_search_empty_query_skips_network() {
    let want = cases()["aur_search_empty_query"].clone();
    let mut io = aur_io(0);
    assert!(aur::search_aur(&mut io, "   ", None).unwrap().is_empty());
    assert_requests(&io, &want, "aur_search_empty_query");
}

/// Категория: три запроса по именному соглашению, дедуп по имени, у всех
/// пакетов проставлен ключ категории.
#[test]
fn aur_category_matches_python() {
    let want = cases()["aur_category_cursor"].clone();
    let mut io = aur_io(3);
    let got = aur::fetch_for_category(&mut io, "cursor").unwrap();
    assert_eq!(names(&got), want_names(&want));
    let cats: Vec<&str> = want["categories"].as_array().unwrap().iter().map(|c| c.as_str().unwrap()).collect();
    assert!(got.iter().all(|p| cats.contains(&p.category.as_str())));
    assert_requests(&io, &want, "aur_category_cursor");
}

#[test]
fn aur_unknown_category_skips_network() {
    let want = cases()["aur_category_unknown"].clone();
    let mut io = aur_io(0);
    assert!(aur::fetch_for_category(&mut io, "нет-такой").unwrap().is_empty());
    assert_requests(&io, &want, "aur_category_unknown");
}

/// Раздел без запроса: выдача категории и сверху её курируемые пакеты.
/// Подборка просится ОДНИМ `/info` и только именами своего раздела; не
/// ответил `/info` — раздел показывается своей выдачей, без ошибки.
#[test]
fn aur_category_pins_curated_of_its_section() {
    let search = |names: &[&str]| -> Vec<u8> {
        let results: Vec<Value> = names
            .iter()
            .map(|n| serde_json::json!({"Name": n, "Description": "Desktop sound theme", "NumVotes": 1}))
            .collect();
        serde_json::to_vec(&serde_json::json!({"type": "search", "results": results})).unwrap()
    };
    let info = serde_json::to_vec(&serde_json::json!({"type": "multiinfo", "results": [
        {"Name": "aerothemeplasma-sounds-git", "Description": "Windows 7 sounds", "NumVotes": 3}
    ]}))
    .unwrap();
    let mut io = FakeNet::with(vec![
        Reply::Ok(search(&["ocean-sound-theme"]), None),
        Reply::Ok(search(&[]), None),
        Reply::Ok(search(&[]), None),
        Reply::Ok(info.clone(), None),
    ]);
    let got = aur::fetch_category_with_curated(&mut io, "sounds").unwrap();
    assert_eq!(names(&got), ["aerothemeplasma-sounds-git", "ocean-sound-theme"]);
    let last = &io.requests.last().unwrap().0;
    assert!(last.contains("/info?") && last.contains("aerothemeplasma-sounds-git"), "{last}");
    assert!(!last.contains("icons"), "в /info только пакеты своего раздела: {last}");

    // /info упал — выдача раздела без подборки.
    let mut io = FakeNet::with(
        std::iter::repeat_with(|| Reply::Ok(search(&["ocean-sound-theme"]), None))
            .take(3)
            .chain((0..ATTEMPTS).map(|_| Reply::Fail(NetError::Transport { detail: "x".into() })))
            .collect(),
    );
    let got = aur::fetch_category_with_curated(&mut io, "sounds").unwrap();
    assert_eq!(names(&got), ["ocean-sound-theme"]);

    // Раздел без подборки в /info не ходит.
    let mut io = FakeNet::with((0..2).map(|_| Reply::Ok(search(&[]), None)).collect());
    assert!(aur::fetch_category_with_curated(&mut io, "colors").unwrap().is_empty());
    assert_eq!(io.requests.len(), 2);
}

/// Виртуальные popular/new: агрегация пяти категорий (12 запросов) в том же
/// порядке, дедуп и разные сортировки.
#[test]
fn aur_virtual_categories_match_python() {
    for (key, case) in [("popular", "aur_category_popular"), ("new", "aur_category_new")] {
        let want = cases()[case].clone();
        let mut io = aur_io(want["requests"].as_array().unwrap().len());
        let got = aur::fetch_for_category(&mut io, key).unwrap();
        assert_eq!(names(&got), want_names(&want), "{key}");
        assert_requests(&io, &want, case);
    }
}

#[test]
fn aur_curated_matches_python() {
    let want = cases()["aur_curated"].clone();
    let mut io = aur_io(1);
    let got = aur::fetch_curated(&mut io).unwrap();
    assert_eq!(names(&got), want_names(&want));
    assert_requests(&io, &want, "aur_curated");
}

/// Не-JSON: тот же префикс сообщения (диагностика разборщика своя).
#[test]
fn aur_not_json_message_matches_python() {
    let _lang = i18n_pin::ru();
    let want = cases()["aur_not_json"].clone();
    let mut io = FakeNet::with(vec![Reply::Ok(b"<html>503</html>".to_vec(), None)]);
    let err = aur::search_aur(&mut io, "plasma", None).unwrap_err();
    let prefix = "AUR RPC вернул не JSON: ";
    assert!(want["error"].as_str().unwrap().starts_with(prefix));
    assert!(err.to_string().starts_with(prefix), "{err}");
    assert_requests(&io, &want, "aur_not_json");
}

/// Не-объект: имя типа по-питоновски, текст побайтово как у оригинала.
#[test]
fn aur_not_object_messages_match_python() {
    let _lang = i18n_pin::ru();
    for (case, body) in [
        ("aur_not_object_list", "[1,2]"),
        ("aur_not_object_str", "\"oops\""),
        ("aur_not_object_int", "5"),
        ("aur_not_object_float", "5.5"),
        ("aur_not_object_none", "null"),
        ("aur_not_object_bool", "true"),
    ] {
        let want = cases()[case].clone();
        let mut io = FakeNet::with(vec![Reply::Ok(body.as_bytes().to_vec(), None)]);
        let err = aur::search_aur(&mut io, "plasma", None).unwrap_err();
        assert_eq!(err.to_string(), want["error"].as_str().unwrap(), "{case}");
    }
}

/// Те же ошибки на исходном языке.
#[test]
fn aur_shape_errors_in_english() {
    let _lang = i18n_pin::en();
    let mut io = FakeNet::with(vec![Reply::Ok(b"[1,2]".to_vec(), None)]);
    let err = aur::search_aur(&mut io, "plasma", None).unwrap_err();
    assert_eq!(err.to_string(), "AUR RPC returned a list instead of an object");
    let mut io = FakeNet::with(vec![Reply::Ok(b"<html>503</html>".to_vec(), None)]);
    let err = aur::search_aur(&mut io, "plasma", None).unwrap_err();
    assert!(err.to_string().starts_with("AUR RPC returned something other than JSON: "), "{err}");
}

/// Ошибка уровня RPC пробрасывается наверх с текстом сервера (или заглушкой).
#[test]
fn aur_rpc_error_messages_match_python() {
    for (case, body) in [
        ("aur_rpc_error", r#"{"type":"error","error":"too many results"}"#),
        ("aur_rpc_error_no_text", r#"{"type":"error"}"#),
    ] {
        let want = cases()[case].clone();
        let mut io = FakeNet::with(vec![Reply::Ok(body.as_bytes().to_vec(), None)]);
        let err = aur::search_aur(&mut io, "plasma", None).unwrap_err();
        assert_eq!(err.to_string(), want["error"].as_str().unwrap(), "{case}");
    }
}

/// Сетевой сбой у AUR НЕ глотается (в отличие от OCS) — уходит наверх с тем
/// же текстом, который увидит покупатель.
#[test]
fn aur_net_fail_propagates() {
    let want = cases()["aur_net_fail"].clone();
    let mut io = FakeNet::always_failing("<urlopen error нет сети>");
    let err = aur::search_aur(&mut io, "plasma", None).unwrap_err();
    assert_eq!(err.to_string(), want["error"].as_str().unwrap());
    assert!(
        io.final_log_texts().is_empty(),
        "AUR-ветка своих сообщений в журнал не пишет — только извещения о повторах"
    );
    assert_requests_retried(&io, &want, "aur_net_fail", ATTEMPTS);
    assert_retries(&io, "aur_net_fail", ATTEMPTS - 1);
}

// ============================================================================
// OCS: отказ магазина ≠ «товаров нет» (20.10.2026)
// ============================================================================
//
// 20.10.2026 api.kde-look.org/api.opendesktop.org на любой запрос каталога
// отвечали HTTP 200 с телом «status failed, statuscode 999, unknown request»,
// а раздел писал «ничего не найдено», будто товаров нет. Ниже — ЗАПИСАННЫЕ
// ответы того дня (XML — то, что приходит на наш запрос; JSON — то же при
// `format=json`).

const FAILED_999_XML: &[u8] = b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ocs><meta><status>failed</status><statuscode>999</statuscode><message>unknown request</message></meta></ocs>\n";
const FAILED_999_JSON: &[u8] = br#"{"status":"failed","statuscode":999,"message":"unknown request"}"#;

/// Ни одной записи страницы в кеш — ответом-ошибкой хороший кеш не затирается.
fn assert_no_list_written(io: &FakeNet, ctx: &str) {
    assert!(
        !io.cache.iter().any(|c| c["call"] == "put_list"),
        "{ctx}: отказ магазина записан в кеш: {:?}",
        io.cache
    );
}

#[test]
fn store_refusal_999_is_an_error_not_an_empty_page() {
    for (what, body) in [("xml", FAILED_999_XML), ("json", FAILED_999_JSON)] {
        let mut io = FakeNet::with(vec![Reply::Ok(body.to_vec(), None)]);
        io.lists = vec![None, None];
        let mut c = client(io);
        let got = c.search_checked("104", 0, 20, "new", "Nordic", true);
        assert_eq!(
            got.unwrap_err(),
            ocs::StoreFailure::Refused { code: "999".into(), message: "unknown request".into() },
            "{what}"
        );
        assert_no_list_written(&c.into_io(), what);
    }
}

/// Отказ магазина при живом кеше той же страницы: показываем кеш, не ошибку.
#[test]
fn store_refusal_keeps_cached_page() {
    let mut io = FakeNet::with(vec![Reply::Ok(FAILED_999_XML.to_vec(), None)]);
    io.lists = vec![Some((vec![cached_item("777", "из кеша", 104)], 42))];
    let mut c = client(io);
    let (items, total) = c.search_checked("104", 0, 20, "new", "", false).unwrap();
    assert_eq!(total, 42);
    assert_eq!(ids(&items), vec!["777"]);
    assert_no_list_written(&c.into_io(), "кеш при отказе");
}

/// Сеть упала и кеша нет — это сбой связи, а не пустой раздел.
#[test]
fn store_network_failure_is_reported() {
    let mut io = FakeNet::always_failing("<urlopen error boom>");
    io.lists = vec![None, None];
    let mut c = client(io);
    match c.search_checked("104", 0, 20, "new", "", true) {
        Err(ocs::StoreFailure::Network(text)) => assert!(text.contains("boom"), "{text}"),
        other => panic!("ждали сбой сети, получили {other:?}"),
    }
}

/// Настоящий пустой ответ (status ok, товаров 0) — по-прежнему пусто, не ошибка.
#[test]
fn store_ok_empty_is_not_an_error() {
    let xml = br#"<?xml version="1.0"?><ocs><meta><status>ok</status><statuscode>100</statuscode><totalitems>0</totalitems></meta><data/></ocs>"#;
    let mut io = FakeNet::with(vec![Reply::Ok(xml.to_vec(), None)]);
    io.lists = vec![None];
    let mut c = client(io);
    let (items, total) = c.search_checked("104", 0, 20, "new", "zzz", true).unwrap();
    assert!(items.is_empty());
    assert_eq!(total, 0);
}

/// Подпись для человека — по-русски дословно, на «вы».
#[test]
fn store_refusal_text_in_russian() {
    let _lang = i18n_pin::ru();
    let f = ocs::StoreFailure::Refused { code: "999".into(), message: "unknown request".into() };
    assert_eq!(
        rexon_core::appearance_page::store_failure_text(&f),
        "Магазин KDE сейчас не отвечает (ошибка 999: unknown request). Попробуйте позже."
    );
}
