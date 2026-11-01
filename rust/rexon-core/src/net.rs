//! Сетевой транспорт каталогов оформления — порт HTTP-слоя из
//! `rexon/appearance/ocs_client.py` (`_http_get`/`_http_download`) и
//! `rexon/appearance/aur_client.py` (`_http_json`), где Python пользуется
//! stdlib `urllib`.
//!
//! Раньше HTTP сознательно оставался на Python: ядро собиралось без сетевых
//! зависимостей, а разбор ответов (`ocs.rs`, `aur.rs`) покрывался голденами на
//! записанных ответах. Решение пересмотрено — цель «убрать Python отовсюду,
//! кроме интерфейса» требует, чтобы запрос делало ядро.
//!
//! Устройство ровно как у остальных портов с побочными эффектами
//! (`ImpIo` в `importers.rs`, `ComplianceIo` в `compliance.rs`): вся ЛОГИКА —
//! какой URL дёрнуть, что считать успехом, когда уйти в кеш, когда удалить
//! недокачанный файл — живёт в ядре и тестируется, а сам вызов спрятан за
//! трейтом [`NetIo`]. В parity-тестах трейт отвечает ЗАПИСАННЫМИ ответами
//! (`tests/golden/*`), в бою — [`UreqNet`] поверх `ureq` + native-tls.
//!
//! Один открытый ответ и один открытый файл за раз — это не упрощение, а
//! точный слепок оригинала: `_http_download` целиком укладывается в один
//! `with urlopen(...) as resp:` и один `with open(dest,"wb") as f:`.

use std::collections::HashMap;
use std::io::{Read, Write};

/// Уровень сообщения журнала — `log.info` / `log.warning` / `log.error`
/// оригинала. Поведение от него не зависит, но тексты видны в `rexon.log`,
/// поэтому они часть паритета.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Info,
    Warning,
    Error,
}

impl LogLevel {
    /// Имя метода `logging`, которым сообщение уходит в журнал Python.
    pub fn logger_method(self) -> &'static str {
        match self {
            LogLevel::Info => "info",
            LogLevel::Warning => "warning",
            LogLevel::Error => "error",
        }
    }
}

/// Сетевой сбой — порт исключений, которые ловит (OCS) либо пробрасывает
/// наверх (AUR) оригинал.
///
/// Python ловит `urllib.error.HTTPError` (ответ вне 2xx после отработки
/// редиректов) и `urllib.error.URLError`/`OSError` (DNS, отказ соединения,
/// таймаут, TLS). [`Display`](std::fmt::Display) даёт ТУ ЖЕ форму строки, что
/// `str(e)` в Python, потому что у AUR-ветки этот текст доезжает до окна
/// покупателя (`_AurWorker.failed.emit(str(e))`), а у OCS попадает в журнал.
///
/// Форма зависит от того, ГДЕ порвалось, и оригинал здесь непоследователен:
/// не состоявшееся соединение — это `URLError`, у которого `str(e)` берёт
/// сообщение в скобки (`<urlopen error …>`), а обрыв уже идущего чтения — это
/// голый `OSError` без скобок. Поэтому оформление лежит на реализации
/// транспорта ([`UreqNet`] оборачивает в скобки именно сбой соединения), а
/// `detail` хранится и печатается как есть.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetError {
    /// Ответ со статусом вне 200..=299 (порт `HTTPError`).
    Status { code: u16, reason: String },
    /// Соединение не состоялось либо оборвалось (порт `URLError`/`OSError`).
    Transport { detail: String },
}

impl std::fmt::Display for NetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            // str(HTTPError) == "HTTP Error {code}: {reason}"
            NetError::Status { code, reason } => write!(f, "HTTP Error {code}: {reason}"),
            NetError::Transport { detail } => write!(f, "{detail}"),
        }
    }
}

impl std::error::Error for NetError {}

impl NetError {
    /// Человеческая формулировка для окна покупателя (правило #13: без
    /// жаргона и латиницы), на языке интерфейса. Оригинал показывал сырой `str(e)` от urllib —
    /// строку вида `<urlopen error [Errno -2] Name or service not known>`,
    /// по которой обычный человек ничего не понимает.
    ///
    /// Это ДОПОЛНЕНИЕ, а не замена: [`Display`](std::fmt::Display) остаётся
    /// побайтово совместимым с Python, чтобы паритет журналов и тестов не
    /// поехал, а UI-слой волен показывать `human()`.
    pub fn human(&self) -> String {
        match self {
            NetError::Status { code, reason } => {
                if reason.is_empty() {
                    // TRANSLATORS: %1 is an HTTP status code (404, 503…).
                    crate::i18n::trf("The catalog server returned error %1", &[code])
                } else {
                    // TRANSLATORS: %1 is an HTTP status code, %2 the server's reason text (in English).
                    crate::i18n::trf("The catalog server returned error %1 (%2)", &[code, reason])
                }
            }
            NetError::Transport { .. } => crate::i18n::tr(
                "Couldn't reach the catalog server — check your internet connection",
            ),
        }
    }
}

/// Транспорт сетевого слоя: каждый метод — ровно один побочный эффект
/// Python-оригинала, в порядке исполнения. В parity-тестах — реплей журнала,
/// в бою — [`UreqNet`].
///
/// Заголовок ровно один (`User-Agent`) — как в `urllib.request.Request(url,
/// headers={"User-Agent": USER_AGENT})`; таймаут в секундах — как аргумент
/// `urlopen(..., timeout=...)` (у urllib это таймаут сокета: и на соединение,
/// и на каждое чтение).
pub trait NetIo {
    /// `with urlopen(Request(url, headers), timeout) as resp: resp.read()` —
    /// тело ответа целиком.
    fn get(&mut self, url: &str, user_agent: &str, timeout_secs: u64) -> Result<Vec<u8>, NetError>;

    /// Вход в `with urlopen(...) as resp` для потокового скачивания.
    /// `Ok(total)` — `int(resp.headers.get("Content-Length", 0))`.
    ///
    /// ⚠️ Починка оригинала (правило #16): у Python нечисловой Content-Length
    /// давал `ValueError`, который `_http_download` НЕ ловит (там только
    /// `URLError`/`HTTPError`/`OSError`) — исключение улетало из воркера и
    /// роняло установку темы. Здесь неразобранный заголовок = `0`, как и его
    /// отсутствие: проверка размера просто не срабатывает.
    fn open(&mut self, url: &str, user_agent: &str, timeout_secs: u64) -> Result<i64, NetError>;

    /// `resp.read(n)` из открытого ответа; пустой вектор — тело кончилось.
    fn read(&mut self, n: usize) -> Result<Vec<u8>, NetError>;

    /// Выход из `with urlopen(...)` — закрыть текущий ответ.
    fn close(&mut self);

    /// `os.makedirs(path, exist_ok=True)`.
    fn makedirs(&mut self, path: &str);

    /// `open(path, "wb")`.
    fn create(&mut self, path: &str) -> Result<(), NetError>;

    /// `f.write(chunk)` в открытый файл.
    fn write(&mut self, chunk: &[u8]) -> Result<(), NetError>;

    /// `f.close()` / выход из `with open(...)`.
    fn close_file(&mut self);

    /// `os.path.exists(path)`.
    fn exists(&mut self, path: &str) -> bool;

    /// `os.remove(path)`; ошибка подавляется вызывающим, как в оригинале.
    fn remove(&mut self, path: &str);

    /// `on_progress(done, total)` — зовётся ТОЛЬКО когда колбэк передан.
    fn progress(&mut self, done: i64, total: i64);

    /// `log.warning` / `log.error` с уже собранным текстом.
    fn log(&mut self, level: LogLevel, message: &str);

    /// `time.sleep(ms / 1000)` — пауза перед повтором (см. [`with_retry`]).
    ///
    /// Побочный эффект, а не `std::thread::sleep` внутри ядра, ровно по той же
    /// причине, что и всё остальное в этом трейте: в сверке пауза должна быть
    /// ВИДНА и не отнимать время, а в бою — быть настоящей.
    fn sleep(&mut self, ms: u64);

    /// `resp.headers.get("Content-Type", "")` для ответа, открытого последним
    /// [`open`](NetIo::open); пусто — заголовка нет либо ответ не открыт.
    ///
    /// Отдельным методом, а не полем возврата `open`, потому что читатель
    /// ровно один — загрузчик снимков рабочих столов, которому тип нужен ДО
    /// чтения тела: страница ошибки (HTML вместо картинки) бывает в мегабайты,
    /// и качать её целиком, чтобы потом выбросить, незачем.
    fn content_type(&mut self) -> String;
}

// ============================================================================
// Повторы при временном сбое
// ============================================================================
//
// Зачем (Wehrwolfmann 2026-08-02): один и тот же класс отказа — сервер закрыл
// соединение на середине — был закрыт в ОДНОМ месте из трёх. У загрузчика
// превью каталога повтор появился (иначе `images.pling.com` гасил пачку
// карточек разом), у загрузчика снимков рабочих столов повтор был только на
// «слишком часто просите» (HTTP 429), а сам сетевой слой ядра — через который
// идут галерея деталки, скачивание тем и запросы AUR — не повторял ничего.
// Значит одна оборванная сессия молча стоила покупателю темы либо картинки.
//
// Правило #12: чиним общий механизм. Решение «стоит ли пробовать ещё раз» и
// «сколько ждать» живёт ЗДЕСЬ, одним набором правил на все запросы ядра, а
// пауза уходит в [`NetIo::sleep`] — то есть видна в сверке и не жжёт время
// тестов.

/// Сколько раз всего пробовать запрос: первая попытка плюс два повтора.
///
/// Три — потому что лечим ОДИНОЧНЫЙ обрыв (сервер закрыл соединение, зеркало
/// моргнуло). Если не помогло дважды подряд, дело не во «моргнуло», и дальше
/// повторы только держат покупателя перед крутящимся кружком.
pub const RETRY_ATTEMPTS: u32 = 3;

/// Пауза перед ПЕРВЫМ повтором обычного сбоя; дальше растёт кратно номеру
/// повтора (700 мс, 1400 мс). Меньше — не успевает закрыться оборванное
/// соединение, больше — заметно покупателю.
pub const RETRY_DELAY_MS: u64 = 700;

/// Пауза перед первым повтором при HTTP 429 «слишком часто просите» (3 с,
/// 6 с). Здесь сервер прямо просит подождать, и 700 мс он не примет —
/// значение унаследовано от загрузчика снимков, где эта ветка уже работала.
pub const RATE_LIMIT_DELAY_MS: u64 = 3_000;

/// Стоит ли пробовать ещё раз после такого сбоя.
///
/// Временное — обрыв соединения, таймаут, отказ DNS (всё это [`NetError::
/// Transport`]), а также ответы «я сейчас занят»: 408, 429 и весь 5xx.
/// Окончательное — 4xx (файла правда нет, доступ закрыт: повтор даст ровно
/// тот же ответ) и ОТМЕНА, которую взвёл сам интерфейс: повторять запрос,
/// который нам больше не нужен, — это ровно тот вред, ради устранения
/// которого отмену и делали.
pub fn is_temporary(e: &NetError) -> bool {
    match e {
        NetError::Transport { detail } => !detail.contains(CANCELLED_DETAIL),
        NetError::Status { code, .. } => {
            *code == 408 || *code == 429 || (500..600).contains(code)
        }
    }
}

/// Пауза перед повтором номер `retry` (первый повтор — единица).
pub fn retry_delay_ms(retry: u32, e: &NetError) -> u64 {
    let base = match e {
        NetError::Status { code: 429, .. } => RATE_LIMIT_DELAY_MS,
        _ => RETRY_DELAY_MS,
    };
    base * u64::from(retry.max(1))
}

/// Повторять ли этот запрос вообще.
///
/// Отдельным типом, а не булевым флагом, чтобы в месте вызова было написано
/// ПОЧЕМУ: повтор безопасен только для запроса, который ничего не меняет на
/// сервере. Сегодня ядро шлёт одни GET-ы, но выбор должен оставаться явным —
/// первый же POST, повторённый «заодно», означал бы вторую покупку, второй
/// голос, второй заказ.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Retry {
    /// Всего попыток, считая первую. Единица — без повторов.
    pub attempts: u32,
}

impl Retry {
    /// Запрос ничего не меняет на сервере (GET/HEAD) — повторять безопасно.
    pub const IDEMPOTENT: Retry = Retry { attempts: RETRY_ATTEMPTS };

    /// Повторов нет: запрос меняет состояние на той стороне либо цена
    /// лишнего вызова выше цены отказа.
    pub const NEVER: Retry = Retry { attempts: 1 };
}

/// Выполнить `op`, повторяя её при временном сбое.
///
/// `what` — как назвать запрос в журнале (URL и вид запроса); текст неудачной
/// попытки уходит предупреждением, окончательный отказ оформляет вызывающий
/// своим сообщением, как и раньше.
///
/// ⚠️ `op` обязана быть ПОЛНОЙ операцией, а не её хвостом: скачивание файла
/// повторяется с самого начала (ответ открывается заново, файл создаётся
/// заново и затирается), потому что дописать вторую попытку в конец огрызка
/// первой — это битый архив, а не починка.
pub fn with_retry<IO, T>(
    io: &mut IO,
    policy: Retry,
    what: &str,
    mut op: impl FnMut(&mut IO) -> Result<T, NetError>,
) -> Result<T, NetError>
where
    IO: NetIo + ?Sized,
{
    let total = policy.attempts.max(1);
    let mut attempt: u32 = 1;
    loop {
        let e = match op(io) {
            Ok(v) => return Ok(v),
            Err(e) => e,
        };
        if attempt >= total || !is_temporary(&e) {
            return Err(e);
        }
        let wait = retry_delay_ms(attempt, &e);
        io.log(
            LogLevel::Warning,
            &format!(
                "{what}: попытка {attempt} из {total} не удалась ({e}) — \
                 повтор через {wait} мс"
            ),
        );
        io.sleep(wait);
        attempt += 1;
    }
}

// ============================================================================
// Отмена запроса, который уже ушёл
// ============================================================================

/// Признак «этот запрос больше не нужен», общий для UI-потока и потока-воркера.
///
/// Зачем (Wehrwolfmann 2026-07-31): поколение ответов спасает интерфейс —
/// каталог покинутой категории не перерисует открытую, — но САМ запрос при
/// этом доживал до конца и всё это время занимал канал. Замер на живом
/// приложении: дюжина переключений категорий подряд давала до 13 запросов
/// одновременно, и тот же запрос, который в одиночку идёт 0,95 с, растягивался
/// до 12,3 с. Значит уходящий запрос надо не «забывать», а рвать.
///
/// Токен взводится из ЛЮБОГО потока (`cancel`), а замечает его транспорт
/// внутри соединения (см. [`CancelTransport`]): ожидание ответа дробится на
/// короткие ломти, между ними проверяется флаг. Заметив отмену, транспорт
/// возвращает ошибку, `ureq` бросает запрос, соединение закрывается.
#[derive(Clone, Debug, Default)]
pub struct CancelToken(std::sync::Arc<std::sync::atomic::AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    /// Пометить запрос ненужным. Действует и на уже идущий запрос.
    pub fn cancel(&self) {
        self.0.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(std::sync::atomic::Ordering::SeqCst)
    }
}

/// Как часто проверяется флаг отмены во время ожидания ответа. Задержка
/// реакции на отмену не превышает этот срок; чаще дёргать сокет незачем.
const CANCEL_POLL: std::time::Duration = std::time::Duration::from_millis(200);

/// Текст, по которому отменённый запрос отличается от сетевого сбоя.
pub const CANCELLED_DETAIL: &str = "запрос отменён";

/// ⛔ ВИД ОШИБКИ ЗДЕСЬ — НЕ ОФОРМЛЕНИЕ, А ПОВЕДЕНИЕ. Только не
/// `ErrorKind::Interrupted`: стандартная библиотека по договору ИГНОРИРУЕТ
/// этот вид и повторяет чтение (`Read::read_to_end`, `io::copy` и вся их
/// родня). Отменённый транспорт с таким видом ошибки получал вечный горячий
/// круг — поток жёг ядро на 100 % и не кончался никогда.
///
/// Наружу это выходило так: человек закрывает окно, а Rexon не закрывается.
/// Фоновая задача вкладки досиживала в этом круге, выход ждал её, и процесс
/// добивал сторож. Поймано 19.09.2026 на самопроверке главного окна: 9
/// прогонов из 12 не завершались сами.
///
/// `ConnectionAborted` — это и по смыслу то, что произошло: соединение
/// оборвали мы сами. Отмену от сетевого сбоя отличает не вид ошибки, а текст
/// [`CANCELLED_DETAIL`], и он сохранён.
fn cancelled_io() -> ureq::Error {
    ureq::Error::Io(std::io::Error::new(
        std::io::ErrorKind::ConnectionAborted,
        CANCELLED_DETAIL,
    ))
}

/// Звено цепочки соединений: заворачивает уже собранный транспорт (вместе с
/// TLS) в [`CancelTransport`]. Ставится ПОСЛЕ `DefaultConnector`, поэтому
/// проверка отмены работает и поверх шифрования.
#[derive(Debug)]
struct CancelConnector {
    token: CancelToken,
}

impl ureq::unversioned::transport::Connector<Box<dyn ureq::unversioned::transport::Transport>>
    for CancelConnector
{
    type Out = CancelTransport;

    fn connect(
        &self,
        _details: &ureq::unversioned::transport::ConnectionDetails,
        chained: Option<Box<dyn ureq::unversioned::transport::Transport>>,
    ) -> Result<Option<Self::Out>, ureq::Error> {
        let Some(inner) = chained else {
            return Ok(None);
        };
        if self.token.is_cancelled() {
            return Err(cancelled_io());
        }
        Ok(Some(CancelTransport {
            inner,
            token: self.token.clone(),
        }))
    }
}

/// Соединение, которое умеет бросить ожидание по внешнему сигналу.
#[derive(Debug)]
struct CancelTransport {
    inner: Box<dyn ureq::unversioned::transport::Transport>,
    token: CancelToken,
}

impl ureq::unversioned::transport::Transport for CancelTransport {
    fn buffers(&mut self) -> &mut dyn ureq::unversioned::transport::Buffers {
        self.inner.buffers()
    }

    fn transmit_output(
        &mut self,
        amount: usize,
        timeout: ureq::unversioned::transport::NextTimeout,
    ) -> Result<(), ureq::Error> {
        if self.token.is_cancelled() {
            return Err(cancelled_io());
        }
        self.inner.transmit_output(amount, timeout)
    }

    fn await_input(
        &mut self,
        timeout: ureq::unversioned::transport::NextTimeout,
    ) -> Result<bool, ureq::Error> {
        // Ждём ответ ломтями по CANCEL_POLL: истёкший ломоть — это ещё не
        // таймаут запроса, а лишь повод проверить флаг отмены. Настоящий
        // таймаут отмеряем сами по общему сроку, который дал ureq.
        let budget: std::time::Duration = *timeout.after;
        let deadline = std::time::Instant::now().checked_add(budget);
        loop {
            if self.token.is_cancelled() {
                return Err(cancelled_io());
            }
            let left = match deadline {
                Some(d) => d.saturating_duration_since(std::time::Instant::now()),
                None => budget,
            };
            if left.is_zero() {
                return Err(ureq::Error::Timeout(timeout.reason));
            }
            let slice = left.min(CANCEL_POLL);
            let sliced = ureq::unversioned::transport::NextTimeout {
                after: ureq::unversioned::transport::time::Duration::Exact(slice),
                reason: timeout.reason,
            };
            match self.inner.await_input(sliced) {
                Err(ureq::Error::Timeout(_)) if slice < left => continue,
                other => return other,
            }
        }
    }

    fn is_open(&mut self) -> bool {
        !self.token.is_cancelled() && self.inner.is_open()
    }

    fn is_tls(&self) -> bool {
        self.inner.is_tls()
    }
}

// ============================================================================
// Терпимость к кривой шапке ответа
// ============================================================================

/// Имя-заглушка той же длины, что `Content-Length` (14 байт) — им заменяется
/// имя заголовка, значение которого разобрать не удалось.
const BAD_LEN_NAME: &[u8] = b"X-Rexon-Badlen";

/// Максимум байт шапки, которые придерживаются ради проверки. Дальше ответ
/// пропускается как есть — про слишком длинную шапку `ureq` скажет сам
/// (`LargeResponseHeader`), подменять его диагноз незачем.
const HEAD_SCAN_LIMIT: usize = 64 * 1024;

/// Конец шапки ответа — позиция сразу ЗА пустой строкой.
fn head_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4)
}

/// Обезвредить `Content-Length`, который не разобрать: `Some(шапка)` —
/// исправленная копия, `None` — править нечего (обычный случай, копии не
/// делаем).
///
/// Правится ДВА случая, и оба — то, как ведёт себя `http.client` Python:
/// значение не число (`length = None` → тело до конца потока) и повторный
/// заголовок (берётся первый, остальные не смотрятся). Имя заменяется на
/// [`BAD_LEN_NAME`] той же длины, поэтому смещения байт не едут, а `ureq`
/// видит ответ без объявленной длины.
fn fix_bad_content_length(head: &[u8]) -> Option<Vec<u8>> {
    const NAME: &[u8] = b"content-length";
    let mut fixed: Option<Vec<u8>> = None;
    let mut line_start = 0usize;
    let mut first_line = true;
    let mut length_seen = false;
    while line_start < head.len() {
        let len = head[line_start..]
            .windows(2)
            .position(|w| w == b"\r\n")
            .unwrap_or(head.len() - line_start);
        let line = &head[line_start..line_start + len];
        if line.is_empty() {
            break; // пустая строка — шапка кончилась
        }
        if !first_line {
            if let Some(colon) = line.iter().position(|&b| b == b':') {
                let name = &line[..colon];
                if name.len() == NAME.len() && name.eq_ignore_ascii_case(NAME) {
                    let value = std::str::from_utf8(&line[colon + 1..])
                        .ok()
                        .map(str::trim);
                    let usable =
                        !length_seen && value.is_some_and(|v| v.parse::<u64>().is_ok());
                    if usable {
                        length_seen = true;
                    } else {
                        let buf = fixed.get_or_insert_with(|| head.to_vec());
                        buf[line_start..line_start + name.len()].copy_from_slice(BAD_LEN_NAME);
                    }
                }
            }
        }
        first_line = false;
        line_start += len + 2;
    }
    fixed
}

/// Звено цепочки соединений: заворачивает транспорт в [`HeadFixTransport`].
#[derive(Debug)]
struct HeadFixConnector;

impl ureq::unversioned::transport::Connector<CancelTransport> for HeadFixConnector {
    type Out = HeadFixTransport<CancelTransport>;

    fn connect(
        &self,
        details: &ureq::unversioned::transport::ConnectionDetails,
        chained: Option<CancelTransport>,
    ) -> Result<Option<Self::Out>, ureq::Error> {
        let Some(inner) = chained else {
            return Ok(None);
        };
        Ok(Some(HeadFixTransport::new(inner, details.config)))
    }
}

/// Соединение, которое показывает `ureq` шапку ответа только целиком — и
/// перед этим обезвреживает `Content-Length`, от которого разбор падает.
///
/// Зачем (Wehrwolfmann 2026-08-01): нечисловой `Content-Length` для `ureq`
/// смертелен — шапка отвергается ошибкой `BadContentLengthHeader` ЕЩЁ ДО
/// того, как ответ доходит до нашего кода, и установка темы падает целиком.
/// Настройки, чтобы это стерпеть, у `ureq` 3.3 нет (проверены все
/// `ConfigBuilder`, свежее версии нет). Python в этом месте терпим:
/// `http.client` при неразбираемой длине кладёт `length = None` и читает тело
/// до закрытия соединения — ровно то, что обещает контракт [`NetIo::open`]
/// («неразобранный заголовок = 0»). Кривую длину шлют зеркала и прокси, а
/// цена — не скачанная тема, поэтому терпимость восстановлена ОДНОЙ точкой на
/// весь сетевой слой (каталог OCS, AUR, превью, архивы), а не в одном вызове.
///
/// Устройство: пока читается шапка, байты копятся в буфере соединения и наверх
/// не отдаются; собранная целиком шапка проверяется
/// [`fix_bad_content_length`] и — только если было что чинить — заменяется
/// исправленной копией. Здоровый ответ идёт сырой перекачкой байт.
#[derive(Debug)]
struct HeadFixTransport<T: ureq::unversioned::transport::Transport> {
    inner: T,
    /// Буферы, которые видит `ureq` (у соединения — свои).
    buffers: ureq::unversioned::transport::LazyBuffers,
    /// Исправленная шапка, ещё не отданная наверх целиком.
    pending: Vec<u8>,
    /// Сколько байт `pending` уже отдано.
    sent: usize,
    /// Шапка ответа ещё не пройдена.
    scanning: bool,
}

impl<T: ureq::unversioned::transport::Transport> HeadFixTransport<T> {
    fn new(inner: T, config: &ureq::config::Config) -> Self {
        HeadFixTransport {
            inner,
            buffers: ureq::unversioned::transport::LazyBuffers::new(
                config.input_buffer_size(),
                config.output_buffer_size(),
            ),
            pending: Vec::new(),
            sent: 0,
            scanning: true,
        }
    }

    /// Отдать наверх сколько влезет из исправленной шапки.
    fn drain_pending(&mut self) -> usize {
        use ureq::unversioned::transport::Buffers;
        if self.pending.is_empty() {
            return 0;
        }
        let dst = self.buffers.input_append_buf();
        let n = (self.pending.len() - self.sent).min(dst.len());
        dst[..n].copy_from_slice(&self.pending[self.sent..self.sent + n]);
        self.buffers.input_appended(n);
        self.sent += n;
        if self.sent == self.pending.len() {
            self.pending.clear();
            self.sent = 0;
        }
        n
    }

    /// Сырая перекачка байт из буфера соединения наверх.
    fn copy_raw(&mut self) -> usize {
        use ureq::unversioned::transport::Buffers;
        let Self { inner, buffers, .. } = self;
        let n = {
            let src = inner.buffers().input();
            let dst = buffers.input_append_buf();
            let n = src.len().min(dst.len());
            dst[..n].copy_from_slice(&src[..n]);
            n
        };
        if n > 0 {
            buffers.input_appended(n);
            inner.buffers().input_consume(n);
        }
        n
    }

    /// Передать наверх то, что можно: хвост исправленной шапки, разобранную
    /// шапку целиком либо сырые байты тела. `0` — «шапка ещё не собралась,
    /// нужны данные».
    fn pump(&mut self) -> usize {
        let moved = self.drain_pending();
        if moved > 0 || !self.pending.is_empty() {
            return moved;
        }
        if self.scanning {
            match head_end(self.inner.buffers().input()) {
                Some(end) => {
                    let fixed = fix_bad_content_length(&self.inner.buffers().input()[..end]);
                    self.scanning = false;
                    if let Some(head) = fixed {
                        self.inner.buffers().input_consume(end);
                        self.pending = head;
                        self.sent = 0;
                        return self.drain_pending();
                    }
                }
                None => {
                    let grown = self.inner.buffers().input().len();
                    // Копить дальше некуда (буфер соединения полон) или шапка
                    // переросла разумный предел — отдаём как есть.
                    let stuck = self.inner.buffers().input_append_buf().is_empty();
                    if !stuck && grown <= HEAD_SCAN_LIMIT {
                        return 0;
                    }
                    self.scanning = false;
                }
            }
        }
        self.copy_raw()
    }
}

impl<T: ureq::unversioned::transport::Transport> ureq::unversioned::transport::Transport
    for HeadFixTransport<T>
{
    fn buffers(&mut self) -> &mut dyn ureq::unversioned::transport::Buffers {
        &mut self.buffers
    }

    fn transmit_output(
        &mut self,
        amount: usize,
        timeout: ureq::unversioned::transport::NextTimeout,
    ) -> Result<(), ureq::Error> {
        use ureq::unversioned::transport::Buffers;
        // Пошёл новый запрос — впереди новая шапка ответа.
        self.scanning = true;
        self.pending.clear();
        self.sent = 0;
        let mut done = 0usize;
        while done < amount {
            let n = {
                let Self { inner, buffers, .. } = self;
                let src = &buffers.output()[done..amount];
                let dst = inner.buffers().output();
                let n = src.len().min(dst.len());
                dst[..n].copy_from_slice(&src[..n]);
                n
            };
            if n == 0 {
                return Err(ureq::Error::Io(std::io::Error::other(
                    "буфер соединения не принимает запрос",
                )));
            }
            self.inner.transmit_output(n, timeout)?;
            done += n;
        }
        Ok(())
    }

    fn await_input(
        &mut self,
        timeout: ureq::unversioned::transport::NextTimeout,
    ) -> Result<bool, ureq::Error> {
        loop {
            if self.pump() > 0 {
                return Ok(true);
            }
            // Отдать нечего — ждём данных от соединения.
            if !self.inner.await_input(timeout)? {
                // Поток кончился на середине шапки: отдаём накопленное как
                // есть, дальше пусть решает `ureq`.
                self.scanning = false;
                return Ok(self.pump() > 0);
            }
        }
    }

    fn is_open(&mut self) -> bool {
        self.inner.is_open()
    }

    fn is_tls(&self) -> bool {
        self.inner.is_tls()
    }
}

// ============================================================================
// Живая реализация: ureq + native-tls
// ============================================================================

/// Боевой транспорт: синхронный `ureq` поверх rustls с корнями доверия из
/// системного хранилища.
///
/// Настройки подогнаны под urllib: таймаут применяется к соединению И к
/// каждому чтению/записи (у urllib это таймаут сокета, а не всего запроса),
/// редиректов не больше десяти (`HTTPRedirectHandler.max_redirections`),
/// успехом считается только 2xx (`urlopen` на прочих кодах бросает
/// `HTTPError`). Агенты кешируются по таймауту, чтобы соединения
/// переиспользовались между запросами одной пачки.
pub struct UreqNet {
    agents: HashMap<u64, ureq::Agent>,
    reader: Option<Box<dyn Read + Send + Sync>>,
    /// `Content-Type` открытого ответа — снимается в [`NetIo::open`], пока
    /// шапка ещё под рукой.
    ctype: String,
    file: Option<std::fs::File>,
    progress: Option<ProgressCb>,
    sink: Option<LogCb>,
    cancel: CancelToken,
    /// Заголовки сверх `User-Agent`, одинаковые для всех запросов этого
    /// транспорта. В трейте [`NetIo`] их нет намеренно: у urllib-оригиналов
    /// заголовок ровно один, и лишний параметр в каждом методе понадобился бы
    /// одному вызывающему из шести. Единственный сегодняшний потребитель —
    /// API GitHub, который просит `Accept: application/vnd.github+json`
    /// (состав пар задаёт `engine_update::api_headers`).
    extra_headers: Vec<(String, String)>,
}

/// Колбэк прогресса скачивания — `on_progress(done, total)` оригинала.
type ProgressCb = Box<dyn FnMut(i64, i64) + Send>;
/// Приёмник журнала — `log.warning`/`log.error` оригинала.
type LogCb = Box<dyn FnMut(LogLevel, &str) + Send>;

impl Default for UreqNet {
    fn default() -> Self {
        Self::new()
    }
}

impl UreqNet {
    pub fn new() -> Self {
        UreqNet {
            agents: HashMap::new(),
            reader: None,
            ctype: String::new(),
            file: None,
            progress: None,
            sink: None,
            cancel: CancelToken::new(),
            extra_headers: Vec::new(),
        }
    }

    /// Токен отмены этого транспорта: взводится снаружи (из UI-потока) и
    /// рвёт запрос, который уже ушёл. Каждый [`UreqNet`] держит СВОЙ токен —
    /// отмена одного запроса не задевает соседние.
    pub fn cancel_token(&self) -> CancelToken {
        self.cancel.clone()
    }

    /// Взять готовый токен (например общий на пачку запросов одной категории).
    /// Звать ДО первого запроса: агенты кешируются вместе с токеном.
    pub fn with_cancel(mut self, token: CancelToken) -> Self {
        self.cancel = token;
        self.agents.clear();
        self
    }

    /// Заголовки сверх `User-Agent` — по паре «имя, значение», как их назвало
    /// ядро. `User-Agent` сюда класть незачем: он приходит аргументом каждого
    /// запроса и будет выставлен поверх.
    pub fn with_headers(mut self, headers: Vec<(String, String)>) -> Self {
        self.extra_headers = headers;
        self
    }

    /// Колбэк прогресса скачивания (`on_progress` оригинала).
    pub fn with_progress(mut self, cb: impl FnMut(i64, i64) + Send + 'static) -> Self {
        self.progress = Some(Box::new(cb));
        self
    }

    /// Приёмник журнала (по умолчанию сообщения отбрасываются).
    pub fn with_log(mut self, cb: impl FnMut(LogLevel, &str) + Send + 'static) -> Self {
        self.sink = Some(Box::new(cb));
        self
    }

    /// Проверка сертификатов ТОЛЬКО по системному хранилищу доверия.
    ///
    /// Почему не rustls: у `ureq` 2.x фича `tls` жёстко тянет `webpki-roots`
    /// (`dep:webpki-roots` прямо в определении фичи) — вшитый в бинарь слепок
    /// списка корневых центров Mozilla под MPL-2.0. Правило #10 разрешает
    /// только permissive-лицензии, а вшитый список вдобавок стареет вместе с
    /// нашим релизом: отзыв центра дойдёт до покупателя лишь со следующей
    /// сборкой. `native-tls` на Linux берёт корни из системного хранилища,
    /// которое обновляет сам дистрибутив.
    ///
    /// ⚠️ `native-tls` НИКОГДА не подхватывается умолчанием (`ntls.rs` помечен
    /// «is never picked up as a default»), поэтому коннектор задаётся здесь
    /// явно — без него `ureq` без фичи `tls` отказывает на любом https.
    fn agent(&mut self, timeout_secs: u64) -> ureq::Agent {
        let token = self.cancel.clone();
        self.agents
            .entry(timeout_secs)
            .or_insert_with(|| {
                let t = std::time::Duration::from_secs(timeout_secs);
                // Таймауты подогнаны под urllib: у него это таймаут СОКЕТА —
                // и на соединение, и на каждое ожидание данных, а не на весь
                // запрос целиком. Поэтому глобальный timeout НЕ ставим.
                let config = ureq::Agent::config_builder()
                    .timeout_connect(Some(t))
                    .timeout_send_request(Some(t))
                    .timeout_recv_response(Some(t))
                    .timeout_recv_body(Some(t))
                    .max_redirects(10)
                    // Ответ вне 2xx разбираем сами (ниже), как и раньше:
                    // так один путь и для «ureq отдал ошибку статуса», и для
                    // «упёрлись в лимит редиректов и получили 3xx».
                    .http_status_as_error(false)
                    .tls_config(
                        ureq::tls::TlsConfig::builder()
                            .provider(ureq::tls::TlsProvider::NativeTls)
                            // Корни доверия — ТОЛЬКО системное хранилище
                            // (native-tls берёт их у OpenSSL дистрибутива).
                            // Вариант по умолчанию (WebPki) в ureq 3 не просто
                            // вшивает список Mozilla под MPL-2.0, но и вовсе
                            // недоступен: фича `native-tls-webpki-roots` у нас
                            // отключена, и такой конфиг падает в панику.
                            .root_certs(ureq::tls::RootCerts::PlatformVerifier)
                            .build(),
                    )
                    .build();
                // Свои звенья в цепочке соединений: одно делает запрос
                // отменяемым (CancelConnector), второе — терпимым к кривой
                // шапке ответа (HeadFixConnector).
                let connector = ureq::unversioned::transport::Connector::chain(
                    ureq::unversioned::transport::Connector::chain(
                        ureq::unversioned::transport::DefaultConnector::new(),
                        CancelConnector { token },
                    ),
                    HeadFixConnector,
                );
                ureq::Agent::with_parts(
                    config,
                    connector,
                    ureq::unversioned::resolver::DefaultResolver::default(),
                )
            })
            .clone()
    }

    fn call(
        &mut self,
        url: &str,
        user_agent: &str,
        timeout_secs: u64,
    ) -> Result<ureq::http::Response<ureq::Body>, NetError> {
        let agent = self.agent(timeout_secs);
        let mut req = agent.get(url).header("User-Agent", user_agent);
        for (name, value) in &self.extra_headers {
            req = req.header(name, value);
        }
        match req.call() {
            Ok(resp) => {
                // ureq отдаёт Ok и на 3xx, когда упёрся в лимит редиректов;
                // urlopen в этом месте бросает HTTPError — приводим к нему.
                let status = resp.status();
                let code = status.as_u16();
                if (200..300).contains(&code) {
                    Ok(resp)
                } else {
                    Err(NetError::Status {
                        code,
                        reason: status.canonical_reason().unwrap_or("").to_string(),
                    })
                }
            }
            Err(ureq::Error::StatusCode(code)) => Err(NetError::Status {
                code,
                reason: ureq::http::StatusCode::from_u16(code)
                    .ok()
                    .and_then(|s| s.canonical_reason())
                    .unwrap_or("")
                    .to_string(),
            }),
            // Соединение не состоялось — у Python это URLError, чей str()
            // берёт причину в скобки: `<urlopen error [Errno -2] …>`.
            Err(e) => Err(NetError::Transport {
                detail: format!("<urlopen error {e}>"),
            }),
        }
    }
}

/// Ошибка файловых операций приёмника и обрыва чтения в виде [`NetError`]:
/// оригинал ловит `OSError` от `open`/`write`/`resp.read` тем же `except`, что
/// и сетевые сбои, а печатает его БЕЗ скобок `<urlopen error …>` — поэтому
/// текст берётся как есть.
fn io_err(e: &std::io::Error) -> NetError {
    NetError::Transport {
        detail: e.to_string(),
    }
}

impl UreqNet {
    /// Ошибка чтения тела С УЧЁТОМ ОТМЕНЫ.
    ///
    /// ⚠️ Отменённое чтение `ureq` показывает СВОЕЙ ошибкой («Peer
    /// disconnected»): закрытым соединение делает наш же транспорт, и до
    /// читателя тела наш текст не доходит. Отдать её как есть значило бы
    /// две беды сразу: человеку на закрытии вкладки показывают сетевой
    /// сбой, которого не было, а [`is_temporary`] считает отменённый запрос
    /// временной бедой и ПОВТОРЯЕТ его — ровно то, ради чего отмену и
    /// делали. Отмена обязана оставаться отменой на всех участках, а не
    /// только в ожидании шапки.
    fn body_err(cancelled: bool, e: &std::io::Error) -> NetError {
        if cancelled {
            return NetError::Transport {
                detail: format!("<urlopen error {CANCELLED_DETAIL}>"),
            };
        }
        io_err(e)
    }
}

impl NetIo for UreqNet {
    fn get(&mut self, url: &str, user_agent: &str, timeout_secs: u64) -> Result<Vec<u8>, NetError> {
        let resp = self.call(url, user_agent, timeout_secs)?;
        let mut buf = Vec::new();
        let outcome = resp.into_body().into_reader().read_to_end(&mut buf);
        if let Err(e) = outcome {
            return Err(Self::body_err(self.cancel.is_cancelled(), &e));
        }
        Ok(buf)
    }

    fn open(&mut self, url: &str, user_agent: &str, timeout_secs: u64) -> Result<i64, NetError> {
        let resp = self.call(url, user_agent, timeout_secs)?;
        let total = resp
            .headers()
            .get(ureq::http::header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse::<i64>().ok())
            .unwrap_or(0);
        self.ctype = resp
            .headers()
            .get(ureq::http::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        self.reader = Some(Box::new(resp.into_body().into_reader()));
        Ok(total)
    }

    fn read(&mut self, n: usize) -> Result<Vec<u8>, NetError> {
        let Some(reader) = self.reader.as_mut() else {
            return Ok(Vec::new());
        };
        let mut buf = vec![0u8; n];
        // read() у сокета возвращает «сколько пришло», а `resp.read(n)` в
        // Python — ровно до n байт, но не меньше одного, пока тело не кончилось.
        let outcome = reader.read(&mut buf);
        let got = match outcome {
            Ok(got) => got,
            Err(e) => return Err(Self::body_err(self.cancel.is_cancelled(), &e)),
        };
        buf.truncate(got);
        Ok(buf)
    }

    fn close(&mut self) {
        self.reader = None;
        self.ctype.clear();
    }

    fn makedirs(&mut self, path: &str) {
        let _ = std::fs::create_dir_all(path);
    }

    fn create(&mut self, path: &str) -> Result<(), NetError> {
        let f = std::fs::File::create(path).map_err(|e| io_err(&e))?;
        self.file = Some(f);
        Ok(())
    }

    fn write(&mut self, chunk: &[u8]) -> Result<(), NetError> {
        let Some(f) = self.file.as_mut() else {
            return Ok(());
        };
        f.write_all(chunk).map_err(|e| io_err(&e))
    }

    fn close_file(&mut self) {
        self.file = None;
    }

    fn exists(&mut self, path: &str) -> bool {
        std::path::Path::new(path).exists()
    }

    fn remove(&mut self, path: &str) {
        let _ = std::fs::remove_file(path);
    }

    fn progress(&mut self, done: i64, total: i64) {
        if let Some(cb) = self.progress.as_mut() {
            cb(done, total);
        }
    }

    fn log(&mut self, level: LogLevel, message: &str) {
        if let Some(cb) = self.sink.as_mut() {
            cb(level, message);
        }
    }

    /// Пауза перед повтором. Ждать здесь безопасно: боевой транспорт зовут из
    /// рабочих потоков (QThread-воркеры вкладок), GIL на это время отпущен.
    /// Отменённый запрос сюда не попадает — [`is_temporary`] его отсекает.
    fn sleep(&mut self, ms: u64) {
        std::thread::sleep(std::time::Duration::from_millis(ms));
    }

    fn content_type(&mut self) -> String {
        self.ctype.clone()
    }
}

// ============================================================================
// Тесты терпимости к кривой шапке
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    const UA: &str = "Rexon-test";

    /// Сервер на один запрос: принимает соединение, дочитывает запрос и
    /// отдаёт `parts` кусками (между кусками — пауза, чтобы шапка гарантированно
    /// приехала не за одно чтение), после чего закрывает соединение.
    fn serve_once(parts: Vec<Vec<u8>>) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").expect("слушатель");
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let Ok((mut sock, _)) = listener.accept() else {
                return;
            };
            let mut head = Vec::new();
            let mut byte = [0u8; 1];
            while head.len() < 8192 {
                match sock.read(&mut byte) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => head.push(byte[0]),
                }
                if head.ends_with(b"\r\n\r\n") {
                    break;
                }
            }
            for (i, part) in parts.iter().enumerate() {
                if i > 0 {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                if sock.write_all(part).is_err() {
                    return;
                }
                let _ = sock.flush();
            }
        });
        port
    }

    /// Сервер с постоянным соединением: отвечает на КАЖДЫЙ пришедший запрос
    /// очередным ответом из `answers`, не закрывая сокет. Так проверяется, что
    /// шапка разбирается заново на каждом ответе, а не один раз на соединение.
    fn serve_keepalive(answers: Vec<Vec<u8>>) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").expect("слушатель");
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let mut left = answers.into_iter();
            while let Ok((mut sock, _)) = listener.accept() {
                loop {
                    let mut head = Vec::new();
                    let mut byte = [0u8; 1];
                    while head.len() < 8192 {
                        match sock.read(&mut byte) {
                            Ok(0) | Err(_) => break,
                            Ok(_) => head.push(byte[0]),
                        }
                        if head.ends_with(b"\r\n\r\n") {
                            break;
                        }
                    }
                    if !head.ends_with(b"\r\n\r\n") {
                        break; // клиент ушёл
                    }
                    let Some(answer) = left.next() else { return };
                    if sock.write_all(&answer).is_err() {
                        return;
                    }
                    let _ = sock.flush();
                    if left.len() == 0 {
                        return; // ответы кончились — закрываем соединение
                    }
                }
            }
        });
        port
    }

    /// Скачать тело целиком через открытый ответ (как это делает `ocs`).
    fn drain(net: &mut UreqNet) -> Vec<u8> {
        let mut body = Vec::new();
        loop {
            let chunk = net.read(8192).expect("чтение тела");
            if chunk.is_empty() {
                break;
            }
            body.extend_from_slice(&chunk);
        }
        net.close();
        body
    }

    #[test]
    fn нечисловая_длина_переименовывается() {
        let head = b"HTTP/1.1 200 OK\r\nContent-Type: x\r\nContent-Length: not-a-number\r\n\r\n";
        let fixed = fix_bad_content_length(head).expect("шапку надо было починить");
        assert_eq!(fixed.len(), head.len(), "смещения байт поехали");
        assert!(!fixed.windows(14).any(|w| w.eq_ignore_ascii_case(b"Content-Length")));
        assert!(fixed.windows(14).any(|w| w == BAD_LEN_NAME));
        assert!(fixed.ends_with(b"not-a-number\r\n\r\n"), "значение трогать незачем");
    }

    #[test]
    fn здоровая_шапка_не_переписывается() {
        assert!(fix_bad_content_length(b"HTTP/1.1 200 OK\r\nContent-Length: 42\r\n\r\n").is_none());
        assert!(fix_bad_content_length(b"HTTP/1.1 200 OK\r\nContent-Type: x\r\n\r\n").is_none());
        // Строка запроса с похожим текстом заголовком не считается.
        assert!(fix_bad_content_length(b"HTTP/1.1 200 Content-Length: no\r\n\r\n").is_none());
    }

    #[test]
    fn повторный_заголовок_длины_оставляет_первый() {
        let head = b"HTTP/1.1 200 OK\r\nContent-Length: 7\r\nContent-Length: 9\r\n\r\n";
        let fixed = fix_bad_content_length(head).expect("повтор надо было обезвредить");
        assert!(fixed.windows(18).any(|w| w == b"Content-Length: 7\r"));
        assert!(fixed.windows(18).any(|w| w == b"X-Rexon-Badlen: 9\r"));
    }

    #[test]
    fn пустое_значение_длины_обезвреживается() {
        let head = b"HTTP/1.1 200 OK\r\nContent-Length:\r\n\r\n";
        assert!(fix_bad_content_length(head).is_some());
    }

    #[test]
    fn кривой_content_length_не_роняет_скачивание() {
        let body = b"rexon-tema".repeat(500);
        let mut resp = b"HTTP/1.1 200 OK\r\nContent-Length: not-a-number\r\n\r\n".to_vec();
        resp.extend_from_slice(&body);
        let port = serve_once(vec![resp]);
        let mut net = UreqNet::new();
        let total = net
            .open(&format!("http://127.0.0.1:{port}/tema.tar.xz"), UA, 25)
            .expect("ответ с кривой длиной обязан открыться");
        assert_eq!(total, 0, "неразобранный заголовок = 0 (контракт NetIo::open)");
        assert_eq!(drain(&mut net), body, "тело должно приехать целиком");
    }

    #[test]
    fn кривая_шапка_склеивается_из_кусков() {
        let body = b"kusochki".repeat(100);
        let mut tail = b"a-number\r\n\r\n".to_vec();
        tail.extend_from_slice(&body);
        // Разрыв ровно посреди значения Content-Length.
        let port = serve_once(vec![
            b"HTTP/1.1 200 OK\r\nContent-Type: x\r\nContent-Length: not-".to_vec(),
            tail,
        ]);
        let mut net = UreqNet::new();
        let total = net
            .open(&format!("http://127.0.0.1:{port}/x"), UA, 25)
            .expect("шапка по кускам обязана собраться");
        assert_eq!(total, 0);
        assert_eq!(drain(&mut net), body);
    }

    #[test]
    fn на_постоянном_соединении_шапка_разбирается_каждый_раз() {
        let ok = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\nпервый", "первый".len())
            .into_bytes();
        let bad = "HTTP/1.1 200 OK\r\nContent-Length: nope\r\nConnection: close\r\n\r\nвторой"
            .to_string()
            .into_bytes();
        let port = serve_keepalive(vec![ok, bad]);
        let mut net = UreqNet::new();
        let url = format!("http://127.0.0.1:{port}/x");
        assert_eq!(net.get(&url, UA, 25).expect("первый ответ"), "первый".as_bytes());
        // Второй ответ по тому же соединению — с кривой длиной.
        assert_eq!(net.get(&url, UA, 25).expect("второй ответ"), "второй".as_bytes());
    }


    // ========================================================================
    // Повторы при временном сбое
    // ========================================================================

    /// Исполнитель-пустышка: считает попытки, отдаёт заготовленные исходы и
    /// записывает паузы вместо того, чтобы их выжидать.
    #[derive(Default)]
    struct Counter {
        /// Что вернуть на очередной попытке (сначала — первая).
        outcomes: Vec<Result<u8, NetError>>,
        tries: usize,
        slept: Vec<u64>,
        logs: Vec<String>,
    }

    impl Counter {
        fn run(&mut self, policy: Retry) -> Result<u8, NetError> {
            with_retry(self, policy, "проба", |io: &mut Counter| {
                // Исходов может быть меньше, чем попыток: последний повторяется.
                let out = io.outcomes[io.tries.min(io.outcomes.len() - 1)].clone();
                io.tries += 1;
                out
            })
        }
    }

    /// Только те методы, которые трогает [`with_retry`]; остальные не зовутся.
    impl NetIo for Counter {
        fn get(&mut self, _: &str, _: &str, _: u64) -> Result<Vec<u8>, NetError> {
            unreachable!()
        }
        fn open(&mut self, _: &str, _: &str, _: u64) -> Result<i64, NetError> {
            unreachable!()
        }
        fn read(&mut self, _: usize) -> Result<Vec<u8>, NetError> {
            unreachable!()
        }
        fn close(&mut self) {}
        fn makedirs(&mut self, _: &str) {}
        fn create(&mut self, _: &str) -> Result<(), NetError> {
            unreachable!()
        }
        fn write(&mut self, _: &[u8]) -> Result<(), NetError> {
            unreachable!()
        }
        fn close_file(&mut self) {}
        fn exists(&mut self, _: &str) -> bool {
            false
        }
        fn remove(&mut self, _: &str) {}
        fn progress(&mut self, _: i64, _: i64) {}
        fn log(&mut self, _: LogLevel, message: &str) {
            self.logs.push(message.to_string());
        }
        fn sleep(&mut self, ms: u64) {
            self.slept.push(ms);
        }
        fn content_type(&mut self) -> String {
            String::new()
        }
    }

    fn broken() -> NetError {
        NetError::Transport { detail: "<urlopen error соединение сброшено>".into() }
    }

    #[test]
    fn обрыв_соединения_повторяется_и_вторая_попытка_проходит() {
        let mut io = Counter {
            outcomes: vec![Err(broken()), Ok(7)],
            ..Default::default()
        };
        assert_eq!(io.run(Retry::IDEMPOTENT), Ok(7));
        assert_eq!(io.tries, 2, "должна быть ровно одна повторная попытка");
        assert_eq!(io.slept, vec![RETRY_DELAY_MS]);
        assert_eq!(io.logs.len(), 1, "неудачная попытка обязана быть в журнале");
    }

    #[test]
    fn после_исчерпания_попыток_возвращается_последняя_ошибка() {
        let mut io = Counter {
            outcomes: vec![Err(broken())],
            ..Default::default()
        };
        assert_eq!(io.run(Retry::IDEMPOTENT), Err(broken()));
        assert_eq!(io.tries, RETRY_ATTEMPTS as usize);
        // Паузы растут: 700 мс, 1400 мс — и их на одну меньше, чем попыток.
        assert_eq!(io.slept, vec![RETRY_DELAY_MS, RETRY_DELAY_MS * 2]);
    }

    #[test]
    fn отменённый_запрос_не_повторяется() {
        let cancelled = NetError::Transport {
            detail: format!("<urlopen error {CANCELLED_DETAIL}>"),
        };
        let mut io = Counter {
            outcomes: vec![Err(cancelled.clone())],
            ..Default::default()
        };
        assert_eq!(io.run(Retry::IDEMPOTENT), Err(cancelled));
        assert_eq!(io.tries, 1, "запрос, который нам больше не нужен, не повторяют");
        assert!(io.slept.is_empty());
    }

    #[test]
    fn отсутствующий_файл_не_повторяется_а_занятый_сервер_повторяется() {
        for (code, want_tries) in [
            (404u16, 1),                      // файла нет — второй раз тоже не будет
            (403, 1),                         // доступ закрыт
            (429, RETRY_ATTEMPTS as usize),   // «слишком часто просите»
            (503, RETRY_ATTEMPTS as usize),   // сервер занят
            (500, RETRY_ATTEMPTS as usize),
        ] {
            let mut io = Counter {
                outcomes: vec![Err(NetError::Status { code, reason: String::new() })],
                ..Default::default()
            };
            let _ = io.run(Retry::IDEMPOTENT);
            assert_eq!(io.tries, want_tries, "код {code}");
        }
    }

    #[test]
    fn лимит_запросов_ждёт_дольше_обычного_сбоя() {
        let mut io = Counter {
            outcomes: vec![Err(NetError::Status { code: 429, reason: String::new() })],
            ..Default::default()
        };
        let _ = io.run(Retry::IDEMPOTENT);
        assert_eq!(io.slept, vec![RATE_LIMIT_DELAY_MS, RATE_LIMIT_DELAY_MS * 2]);
        // Ровно те же 3 с и 6 с, что загрузчик снимков выдерживал и раньше.
        assert_eq!(io.slept, vec![3_000, 6_000]);
    }

    #[test]
    fn запрет_повторов_соблюдается() {
        let mut io = Counter {
            outcomes: vec![Err(broken())],
            ..Default::default()
        };
        assert_eq!(io.run(Retry::NEVER), Err(broken()));
        assert_eq!(io.tries, 1, "не-идемпотентный запрос повторять нельзя");
        assert!(io.logs.is_empty(), "повторов не было — извещать не о чем");
    }

    #[test]
    fn успех_с_первой_попытки_ничего_не_ждёт_и_не_пишет() {
        let mut io = Counter {
            outcomes: vec![Ok(1)],
            ..Default::default()
        };
        assert_eq!(io.run(Retry::IDEMPOTENT), Ok(1));
        assert_eq!(io.tries, 1);
        assert!(io.slept.is_empty() && io.logs.is_empty());
    }

    #[test]
    fn живой_транспорт_отдаёт_тип_ответа() {
        let body = b"kartinka".repeat(100);
        let mut resp = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\n\r\n",
            body.len()
        )
        .into_bytes();
        resp.extend_from_slice(&body);
        let port = serve_once(vec![resp]);
        let mut net = UreqNet::new();
        net.open(&format!("http://127.0.0.1:{port}/x.png"), UA, 25)
            .expect("ответ");
        assert_eq!(net.content_type(), "image/png");
        drain(&mut net);
        assert_eq!(net.content_type(), "", "закрытый ответ типа не помнит");
    }

    #[test]
    fn здоровый_ответ_проходит_как_прежде() {
        let body = b"0123456789".repeat(3000); // больше одного чтения сокета
        let mut resp = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", body.len())
            .into_bytes();
        resp.extend_from_slice(&body);
        let port = serve_once(vec![resp]);
        let mut net = UreqNet::new();
        let total = net
            .open(&format!("http://127.0.0.1:{port}/x"), UA, 25)
            .expect("обычный ответ");
        assert_eq!(total, body.len() as i64);
        assert_eq!(drain(&mut net), body);
    }
}
