//! Порт `rexon/waywallen_client.py` — тонкий D-Bus клиент демона waywallen.
//!
//! Rexon (frontend, workshop-browser) говорит демону waywallen (backend,
//! Rust Vulkan-рендер) «применить scene по ID» и читает его статус. В Python
//! транспорт — PySide6.QtDBus (`QDBusInterface` на session-bus); здесь ВСЯ
//! логика клиента (коэрции значений свойств, разбор ответов `_call`,
//! формат ошибок, CLI-семантика `python3 -m tools.py_shims.waywallen_client`)
//! отделена от живого I/O трейтом [`WaywallenIo`]:
//!
//! * ядро НЕ тащит D-Bus зависимость (zbus и т.п.) — по политике минимума
//!   зависимостей rexon-core живой транспорт появится реализацией трейта на
//!   стороне моста при подключении;
//! * в parity-тестах трейт реплеит журнал, снятый с живого Python
//!   с полностью замоканным QtDBus (`waywallen_client_cases.json`).
//!
//! Клиент строго СИНХРОННЫЙ (каждый вызов — блокирующий D-Bus round-trip),
//! асинхронных колбэков нет — owner-guard'ы доставки поздних ответов из
//! `qt_wallpaper_engine/client.py` (фикс краша по умершим QML-объектам)
//! относятся к WS-клиенту вкладки WE и на этот модуль не распространяются.
//!
//! Починенные при порте баги оригинала (правило авто-починки; каждое место
//! помечено «намеренное расхождение» рядом с кодом):
//! * `int(prop or 0)` / `int(call or 0)` в Python падали необработанным
//!   `ValueError` на нечисловой строке от демона и `TypeError` на
//!   multi-arg ответе `Rescan` — здесь неконвертируемое значение даёт 0;
//! * типизация: строковые свойства всегда возвращают `String` (Python
//!   возвращал «что пришло», напр. int вопреки аннотации `-> str`).

// ============================================================================
// Константы D-Bus (BUS_NAME / OBJECT_PATH / IFACE)
// ============================================================================

/// Имя сервиса демона на session-bus.
pub const BUS_NAME: &str = "org.waywallen.waywallen.Daemon";
/// Путь объекта демона.
pub const OBJECT_PATH: &str = "/org/waywallen/waywallen/Daemon";
/// Интерфейс демона.
pub const IFACE: &str = "org.waywallen.waywallen.Daemon1";

// ============================================================================
// Значения D-Bus
// ============================================================================

/// Значение, каким его видел Python после конверсии QVariant: None / bool /
/// int / str. Больших типов (double/массивы) интерфейс демона не отдаёт;
/// появятся — расширим enum вместе с golden.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DbusValue {
    None,
    Bool(bool),
    Int(i64),
    Str(String),
}

impl DbusValue {
    /// Python `bool(x)`.
    pub fn is_truthy(&self) -> bool {
        match self {
            DbusValue::None => false,
            DbusValue::Bool(b) => *b,
            DbusValue::Int(i) => *i != 0,
            DbusValue::Str(s) => !s.is_empty(),
        }
    }

    /// Python `str(x)` — так значение выглядит в f-string CLI.
    pub fn py_str(&self) -> String {
        match self {
            DbusValue::None => "None".to_string(),
            DbusValue::Bool(true) => "True".to_string(),
            DbusValue::Bool(false) => "False".to_string(),
            DbusValue::Int(i) => i.to_string(),
            DbusValue::Str(s) => s.clone(),
        }
    }

    /// Python `repr(x)` — элементы списка в `str(list)`. Строки — в кавычках
    /// по правилу CPython: одинарные, но двойные, если внутри есть `'` и нет
    /// `"`. Экранирование управляющих символов не реализовано (ответы демона
    /// — идентификаторы/сообщения без них).
    fn py_repr(&self) -> String {
        match self {
            DbusValue::Str(s) => {
                if s.contains('\'') && !s.contains('"') {
                    format!("\"{s}\"")
                } else {
                    format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'"))
                }
            }
            other => other.py_str(),
        }
    }

    /// Python `int(x)` для truthy-значений: bool → 0/1, int → как есть,
    /// строка — парсинг литерала (пробелы по краям, знак, `_` между цифрами).
    /// `None` = Python поднял бы ValueError.
    pub fn py_int(&self) -> Option<i64> {
        match self {
            DbusValue::None => None,
            DbusValue::Bool(b) => Some(i64::from(*b)),
            DbusValue::Int(i) => Some(*i),
            DbusValue::Str(s) => py_parse_int(s),
        }
    }
}

/// Python `int(str)`: обрезает пробелы, допускает знак и одиночные `_`
/// между цифрами.
fn py_parse_int(s: &str) -> Option<i64> {
    let t = s.trim();
    let digits = t.strip_prefix(['+', '-']).unwrap_or(t);
    if digits.is_empty() {
        return None;
    }
    // `_` допустим только между цифрами (как в литералах Python 3).
    let bytes = digits.as_bytes();
    for (i, b) in bytes.iter().enumerate() {
        match b {
            b'0'..=b'9' => {}
            b'_' => {
                let prev_digit = i > 0 && bytes[i - 1].is_ascii_digit();
                let next_digit =
                    i + 1 < bytes.len() && bytes[i + 1].is_ascii_digit();
                if !prev_digit || !next_digit {
                    return None;
                }
            }
            _ => return None,
        }
    }
    t.replace('_', "").parse().ok()
}

/// Ответ D-Bus вызова, каким его видел Python: `reply.errorName()`,
/// `reply.errorMessage()`, `reply.arguments()`. Пустой `error_name` =
/// вызов успешен.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DbusReply {
    pub error_name: String,
    pub error_message: String,
    pub arguments: Vec<DbusValue>,
}

/// `_call`: `argv[0] if len(argv) == 1 else argv` — одиночное значение либо
/// список (в т.ч. пустой).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplyValue {
    Single(DbusValue),
    Multi(Vec<DbusValue>),
}

impl ReplyValue {
    fn from_arguments(mut args: Vec<DbusValue>) -> Self {
        if args.len() == 1 {
            ReplyValue::Single(args.remove(0))
        } else {
            ReplyValue::Multi(args)
        }
    }

    /// Python `bool(x)`: список truthy, если непуст.
    fn is_truthy(&self) -> bool {
        match self {
            ReplyValue::Single(v) => v.is_truthy(),
            ReplyValue::Multi(list) => !list.is_empty(),
        }
    }

    /// Python `str(x)` — как значение печатает CLI (`print(c.next())`).
    pub fn py_str(&self) -> String {
        match self {
            ReplyValue::Single(v) => v.py_str(),
            ReplyValue::Multi(list) => {
                let items: Vec<String> =
                    list.iter().map(DbusValue::py_repr).collect();
                format!("[{}]", items.join(", "))
            }
        }
    }
}

/// Ошибка D-Bus вызова — порт `RuntimeError` из `_call`. `message` — тот же
/// текст: `"{method}: {errorName} — {errorMessage}"`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallError {
    pub message: String,
}

impl std::fmt::Display for CallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CallError {}

// ============================================================================
// Трейт транспорта
// ============================================================================

/// Транспорт `QDBusInterface`: каждый метод — ровно один D-Bus side-effect
/// Python-оригинала. В parity-тестах — реплей журнала; в живом мосте —
/// реальный session-bus (реализация появится при подключении).
pub trait WaywallenIo {
    /// `self._iface.isValid()` — демон виден на session-bus.
    fn is_valid(&mut self) -> bool;
    /// `self._iface.property(name)`; [`DbusValue::None`] — свойства нет.
    fn get_property(&mut self, name: &str) -> DbusValue;
    /// `self._iface.call(method, *args)` → сырой ответ.
    fn call(&mut self, method: &str, args: &[DbusValue]) -> DbusReply;
}

// ============================================================================
// Клиент
// ============================================================================

/// Тонкий D-Bus wrapper — порт класса `WaywallenClient`. Конструктор не
/// блокирует — проверять [`available`](Self::available).
pub struct WaywallenClient<IO: WaywallenIo> {
    io: IO,
}

impl<IO: WaywallenIo> WaywallenClient<IO> {
    pub fn new(io: IO) -> Self {
        WaywallenClient { io }
    }

    /// Забрать транспорт обратно (parity-тестам — для финальных проверок).
    pub fn into_io(self) -> IO {
        self.io
    }

    /// True, если daemon виден на session-bus (запущен и активирован).
    pub fn available(&mut self) -> bool {
        self.io.is_valid()
    }

    /// `_call`: вызов метода; ошибка D-Bus → [`CallError`] с тем же текстом,
    /// что у `RuntimeError` оригинала.
    fn call(
        &mut self,
        method: &str,
        args: &[DbusValue],
    ) -> Result<ReplyValue, CallError> {
        let reply = self.io.call(method, args);
        if !reply.error_name.is_empty() {
            return Err(CallError {
                message: format!(
                    "{method}: {} — {}",
                    reply.error_name, reply.error_message
                ),
            });
        }
        Ok(ReplyValue::from_arguments(reply.arguments))
    }

    /// `self._prop(name) or ""` — строковое свойство. Намерение Python —
    /// `-> str`; намеренное расхождение: truthy-значение НЕ-строкового типа
    /// приводится `str()` (Python возвращал его как есть вопреки аннотации;
    /// наблюдаемый CLI-вывод через f-string идентичен).
    fn str_prop(&mut self, name: &str) -> String {
        let v = self.io.get_property(name);
        if v.is_truthy() { v.py_str() } else { String::new() }
    }

    /// `int(self._prop(name) or 0)` — целочисленное свойство. Намеренное
    /// расхождение: нечисловая строка от демона даёт 0 (Python падал
    /// необработанным ValueError).
    fn int_prop(&mut self, name: &str) -> i64 {
        let v = self.io.get_property(name);
        if !v.is_truthy() {
            return 0;
        }
        v.py_int().unwrap_or(0)
    }

    /// Свойство `CurrentWallpaperId`.
    pub fn current_wallpaper_id(&mut self) -> String {
        self.str_prop("CurrentWallpaperId")
    }

    /// Свойство `Version`.
    pub fn version(&mut self) -> String {
        self.str_prop("Version")
    }

    /// Свойство `WsPort`.
    pub fn ws_port(&mut self) -> i64 {
        self.int_prop("WsPort")
    }

    /// Свойство `QueueMode`.
    pub fn queue_mode(&mut self) -> String {
        self.str_prop("QueueMode")
    }

    /// Свойство `RotationSecs`.
    pub fn rotation_secs(&mut self) -> i64 {
        self.int_prop("RotationSecs")
    }

    /// Применить scene-обои; возвращает task-id (или сообщение об ошибке).
    /// **Главный** метод интеграции — Apply-кнопка config.qml зовёт его.
    /// Python делал `str(wallpaper_id)` — здесь коэрция закреплена типом
    /// аргумента (не-строковый id приводит к строке вызывающий).
    pub fn apply_by_id(
        &mut self,
        wallpaper_id: &str,
    ) -> Result<ReplyValue, CallError> {
        self.call("ApplyById", &[DbusValue::Str(wallpaper_id.to_string())])
    }

    /// Следующие обои в очереди. Имя как у D-Bus метода `Next` и Python
    /// `next()` — это не итератор, паритет имён важнее lint'а.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Result<ReplyValue, CallError> {
        self.call("Next", &[])
    }

    /// Предыдущие обои.
    pub fn previous(&mut self) -> Result<ReplyValue, CallError> {
        self.call("Previous", &[])
    }

    /// Пауза рендера.
    pub fn pause(&mut self) -> Result<(), CallError> {
        self.call("Pause", &[]).map(|_| ())
    }

    /// Возобновление рендера.
    pub fn resume(&mut self) -> Result<(), CallError> {
        self.call("Resume", &[]).map(|_| ())
    }

    /// `int(self._call("Rescan") or 0)` — id задачи рескана. Намеренное
    /// расхождение: неконвертируемый ответ (нечисловая строка — ValueError;
    /// multi-arg список — TypeError в Python) даёт 0.
    pub fn rescan(&mut self) -> Result<i64, CallError> {
        let rv = self.call("Rescan", &[])?;
        if !rv.is_truthy() {
            return Ok(0);
        }
        Ok(match rv {
            ReplyValue::Single(v) => v.py_int().unwrap_or(0),
            ReplyValue::Multi(_) => 0,
        })
    }

    /// Открыть родной UI демона.
    pub fn open_ui(&mut self) -> Result<(), CallError> {
        self.call("OpenUi", &[]).map(|_| ())
    }

    /// Завершить демон.
    pub fn quit(&mut self) -> Result<(), CallError> {
        self.call("Quit", &[]).map(|_| ())
    }

    /// Интервал ротации в секундах (Python коэрцил `int(secs)` — закреплено
    /// типом).
    pub fn set_rotation_interval(
        &mut self,
        secs: i64,
    ) -> Result<(), CallError> {
        self.call("SetRotationInterval", &[DbusValue::Int(secs)])
            .map(|_| ())
    }

    /// Режим перемешивания (Python коэрцил `bool(on)` — закреплено типом).
    pub fn set_shuffle(&mut self, on: bool) -> Result<(), CallError> {
        self.call("SetShuffle", &[DbusValue::Bool(on)]).map(|_| ())
    }
}

// ============================================================================
// CLI (порт `_cli` — smoke-тест: статус / apply / next / pause / …)
// ============================================================================

/// Результат CLI-прогона: код возврата + захваченные потоки (печать —
/// на вызывающем).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliResult {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl CliResult {
    fn ok(stdout: String) -> Self {
        CliResult { code: 0, stdout, stderr: String::new() }
    }

    fn err(code: i32, stderr: String) -> Self {
        CliResult { code, stdout: String::new(), stderr }
    }
}

/// Порт `_cli()`: `prog` — `sys.argv[0]`, `args` — аргументы после него.
/// Тексты/коды возврата — байт-в-байт с Python (кроме починенного краша
/// `rescan`, см. [`WaywallenClient::rescan`]).
pub fn cli<IO: WaywallenIo>(
    client: &mut WaywallenClient<IO>,
    prog: &str,
    args: &[&str],
) -> CliResult {
    if !client.available() {
        return CliResult::err(
            1,
            "error: waywallen daemon not running on session-bus\n\
             Hint: запустите ~/.local/share/waywallen/waywallen.AppImage\n"
                .to_string(),
        );
    }

    if args.is_empty() {
        // Статус
        let version = client.version();
        let current = client.current_wallpaper_id();
        let current = if current.is_empty() {
            "(none)".to_string()
        } else {
            current
        };
        let stdout = format!(
            "Version    : {version}\n\
             Current ID : {current}\n\
             WS port    : {}\n\
             Queue mode : {}\n\
             Rotation   : {}s\n",
            client.ws_port(),
            client.queue_mode(),
            client.rotation_secs(),
        );
        return CliResult::ok(stdout);
    }

    let action = args[0];
    let result: Result<String, CallError> = match action {
        "apply" if args.len() >= 2 => client
            .apply_by_id(args[1])
            .map(|r| format!("applied: {}\n", r.py_str())),
        "next" => client.next().map(|r| format!("{}\n", r.py_str())),
        "previous" => client.previous().map(|r| format!("{}\n", r.py_str())),
        "pause" => client.pause().map(|_| "paused\n".to_string()),
        "resume" => client.resume().map(|_| "resumed\n".to_string()),
        "rescan" => {
            client.rescan().map(|id| format!("rescan task id: {id}\n"))
        }
        "open-ui" => client.open_ui().map(|_| "UI opened\n".to_string()),
        _ => {
            return CliResult::err(
                2,
                format!(
                    "usage: {prog} [apply <id>|next|previous|pause|resume|\
                     rescan|open-ui]\n"
                ),
            );
        }
    };
    match result {
        Ok(stdout) => CliResult::ok(stdout),
        Err(e) => CliResult::err(1, format!("error: {e}\n")),
    }
}
