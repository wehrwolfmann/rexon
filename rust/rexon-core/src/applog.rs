//! Журнал Rexon — ОДИН механизм на весь продукт.
//!
//! ЗАЧЕМ. Живой случай 08.10.2026: владелец переключил язык, нажал
//! «Перезапустить» — и Rexon не вернулся. В журнале systemd не было ничего,
//! а в файле `~/.config/rexon/rexon.log` нашлась единственная строка без
//! времени («symbol lookup error» новой копии) посреди суток чужих
//! сообщений Qt. Ни времени, ни номера процесса, ни шага перезапуска — по
//! такому журналу причину не восстановить. До этой правки «журнал» был
//! россыпью `eprintln!` с разными приставками, а файлом служил дескриптор 2,
//! перенаправленный в `~/.config/rexon/rexon.log` без предела роста.
//!
//! ЧТО ЗДЕСЬ:
//!   * файл `$XDG_STATE_HOME/rexon/rexon.log` (по умолчанию
//!     `~/.local/state/rexon/rexon.log`) с ротацией по размеру:
//!     [`MAX_BYTES`] на файл, [`KEEP`] прежних (`rexon.log.1` … `.3`);
//!   * каждая запись — одна строка: время до миллисекунд, номер процесса,
//!     уровень, область и текст;
//!   * та же строка — в исходный поток ошибок процесса: терминал при запуске
//!     из консоли, журнал systemd при запуске из меню (дескриптор 2 при этом
//!     уходит в файл ради сообщений Qt, но исходный поток сохраняется);
//!   * уровни `debug`/`info`/`warn`/`error`; по умолчанию пишется `info` и
//!     выше, `REXON_LOG=debug` включает подробный журнал;
//!   * строка о смертельном сигнале (SIGSEGV и родня) — прежде процесс
//!     исчезал вовсе без следа.
//!
//! ПРАВИЛО. Событие продукта (запуск, язык, перезапуск, замок одного
//! экземпляра, выход, привилегированная команда, очередь AUR, установка
//! темы) пишется ТОЛЬКО сюда, макросами [`crate::log_info!`] и соседями.
//! Новый `eprintln!` для события — это второй журнал, которого быть не
//! должно (правило #12).
//!
//! До [`init`] (тесты, служебные бинари) записи идут только в поток ошибок
//! — так же, как шли `eprintln!`, и ни один тест не пишет в профиль.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI32, AtomicU8, Ordering};
use std::sync::Mutex;

/// Предел одного файла журнала, байт.
pub const MAX_BYTES: u64 = 2 * 1024 * 1024;
/// Сколько прежних файлов хранить (`rexon.log.1` — самый свежий).
pub const KEEP: usize = 3;
/// Переменная уровня: `debug`, `info` (по умолчанию), `warn`, `error`.
pub const LEVEL_ENV: &str = "REXON_LOG";

/// Уровень записи.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Debug = 10,
    Info = 20,
    Warn = 30,
    Error = 40,
}

impl Level {
    /// Метка уровня в строке журнала — ровно пять знаков, столбцы ровные.
    pub fn tag(self) -> &'static str {
        match self {
            Level::Debug => "DEBUG",
            Level::Info => "INFO ",
            Level::Warn => "WARN ",
            Level::Error => "ERROR",
        }
    }

    fn from_u8(v: u8) -> Level {
        match v {
            10 => Level::Debug,
            30 => Level::Warn,
            40 => Level::Error,
            _ => Level::Info,
        }
    }
}

/// Значение [`LEVEL_ENV`] → наименьший пишущийся уровень. Незнакомое и
/// пустое — `info`: опечатка в переменной не должна глушить журнал.
pub fn parse_level(value: Option<&str>) -> Level {
    match value.map(|v| v.trim().to_ascii_lowercase()).as_deref() {
        Some("debug" | "trace" | "all" | "1" | "verbose") => Level::Debug,
        Some("warn" | "warning") => Level::Warn,
        Some("error") => Level::Error,
        _ => Level::Info,
    }
}

/// Имя уровня в духе `logging` Python (`debug`, `info`, `warning`, `error`,
/// `critical`) → уровень журнала. Так говорят прежние приёмники ядра
/// (`wallpaper_exec`, `splash_exec`): их строки уходят сюда же, а не в
/// свой `eprintln!`. Незнакомое — `debug`: неизвестный шум не повод
/// засорять журнал по умолчанию.
pub fn level_from_name(name: &str) -> Level {
    match name.trim().to_ascii_lowercase().as_str() {
        "info" => Level::Info,
        "warning" | "warn" => Level::Warn,
        "error" | "critical" | "fatal" => Level::Error,
        _ => Level::Debug,
    }
}

/// 0 — ещё не прочитан из окружения.
static MIN_LEVEL: AtomicU8 = AtomicU8::new(0);

/// Наименьший пишущийся уровень (читается из [`LEVEL_ENV`] один раз).
pub fn min_level() -> Level {
    let v = MIN_LEVEL.load(Ordering::Relaxed);
    if v != 0 {
        return Level::from_u8(v);
    }
    let level = parse_level(std::env::var(LEVEL_ENV).ok().as_deref());
    MIN_LEVEL.store(level as u8, Ordering::Relaxed);
    level
}

/// Пишется ли `level` — для дорогих сообщений (`if enabled(Debug) {…}`).
pub fn enabled(level: Level) -> bool {
    level >= min_level()
}

// ============================================================================
// Строка журнала
// ============================================================================

/// Отметка времени записи: местное время до миллисекунд. Часы — те же, что
/// у отчётов о крахе (`qt_app::Timestamp`): второго `localtime_r` в
/// продукте нет.
fn timestamp() -> String {
    match crate::qt_app::Timestamp::now_local() {
        Some(t) => format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}",
            t.year,
            t.month,
            t.day,
            t.hour,
            t.minute,
            t.second,
            t.microsecond / 1000
        ),
        None => "????-??-?? ??:??:??.???".to_string(),
    }
}

/// Собрать строку журнала. Многострочный текст не рвёт столбцы: со второй
/// строки — отступ, и `grep` по началу строки находит только записи.
pub fn format_line(stamp: &str, pid: u32, level: Level, area: &str, msg: &str) -> String {
    let mut out = format!("{stamp} [{pid}] {} {area}: ", level.tag());
    let mut lines = msg.trim_end_matches('\n').split('\n');
    out.push_str(lines.next().unwrap_or(""));
    for l in lines {
        out.push_str("\n    ");
        out.push_str(l);
    }
    out.push('\n');
    out
}

// ============================================================================
// Файл и ротация
// ============================================================================

struct Sink {
    path: PathBuf,
    file: File,
    size: u64,
    /// Дескриптор 2 заведён в этот файл ([`redirect_stderr`]): после
    /// ротации его надо перевести на новый файл.
    stderr_in_file: bool,
}

static SINK: Mutex<Option<Sink>> = Mutex::new(None);

/// Куда дублировать строку: исходный поток ошибок. `-1` — никуда (поток
/// ошибок и есть файл журнала — иначе каждая строка легла бы дважды).
static ECHO_FD: AtomicI32 = AtomicI32::new(2);
/// Сохранённая копия исходного потока ошибок после [`redirect_stderr`].
static ORIGINAL_STDERR: AtomicI32 = AtomicI32::new(-1);
/// Сырой дескриптор файла журнала — для строки о смертельном сигнале, где
/// ни замки, ни выделение памяти недопустимы.
static RAW_LOG_FD: AtomicI32 = AtomicI32::new(-1);

/// Имя прежнего файла номер `n` (`rexon.log.1`).
pub fn rotated_path(path: &Path, n: usize) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(format!(".{n}"));
    PathBuf::from(s)
}

/// Сдвинуть прежние файлы: `.2→.3`, `.1→.2`, текущий → `.1`. Самый старый
/// пропадает. Ошибки молча: журнал не повод ронять программу.
pub fn rotate_files(path: &Path, keep: usize) {
    if keep == 0 {
        let _ = std::fs::remove_file(path);
        return;
    }
    let _ = std::fs::remove_file(rotated_path(path, keep));
    for n in (1..keep).rev() {
        let _ = std::fs::rename(rotated_path(path, n), rotated_path(path, n + 1));
    }
    let _ = std::fs::rename(path, rotated_path(path, 1));
}

fn open_append(path: &Path) -> std::io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    OpenOptions::new().create(true).append(true).mode(0o600).open(path)
}

/// Тот же ли файл за дескриптором `fd`, что и `file` (по устройству и inode).
fn same_file(fd: i32, file: &File) -> bool {
    let mut a: libc::stat = unsafe { std::mem::zeroed() };
    let mut b: libc::stat = unsafe { std::mem::zeroed() };
    unsafe {
        libc::fstat(fd, &mut a) == 0
            && libc::fstat(file.as_raw_fd(), &mut b) == 0
            && a.st_dev == b.st_dev
            && a.st_ino == b.st_ino
    }
}

/// Завести журнал процесса. Повторный вызов ничего не делает. Отвечает
/// путём файла; `None` — файл не открылся (тогда записи идут только в поток
/// ошибок, программа работает дальше).
pub fn init() -> Option<PathBuf> {
    init_at(PathBuf::from(crate::utils::log_file_env()))
}

/// [`init`] с явным путём — для стендов.
pub fn init_at(path: PathBuf) -> Option<PathBuf> {
    let mut guard = SINK.lock().ok()?;
    if let Some(sink) = guard.as_ref() {
        return Some(sink.path.clone());
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if std::fs::metadata(&path).map(|m| m.len() >= MAX_BYTES).unwrap_or(false) {
        rotate_files(&path, KEEP);
    }
    let file = open_append(&path).ok()?;
    let size = file.metadata().map(|m| m.len()).unwrap_or(0);
    // Процесс, запущенный перезапуском прежней версии, мог унаследовать
    // дескриптор 2, уже заведённый в этот файл: дублировать туда же нельзя.
    if same_file(2, &file) {
        ECHO_FD.store(-1, Ordering::SeqCst);
    }
    RAW_LOG_FD.store(file.as_raw_fd(), Ordering::SeqCst);
    *guard = Some(Sink { path: path.clone(), file, size, stderr_in_file: false });
    Some(path)
}

/// Путь журнала этого процесса (или тот, куда он лёг бы после [`init`]).
pub fn log_path() -> PathBuf {
    SINK.lock()
        .ok()
        .and_then(|g| g.as_ref().map(|s| s.path.clone()))
        .unwrap_or_else(|| PathBuf::from(crate::utils::log_file_env()))
}

/// Записать строку уровня `level` в области `area`.
pub fn write(level: Level, area: &str, msg: &str) {
    write_opts(level, area, msg, true);
}

/// То же, но без копии в поток ошибок (`echo = false`): для строк, которые
/// вызывающий уже напечатал человеку в stdout сам.
pub fn write_opts(level: Level, area: &str, msg: &str, echo: bool) {
    if !enabled(level) {
        return;
    }
    let line = format_line(&timestamp(), std::process::id(), level, area, msg);
    let mut in_file = false;
    if let Ok(mut guard) = SINK.lock() {
        if let Some(sink) = guard.as_mut() {
            in_file = true;
            if sink.size + line.len() as u64 > MAX_BYTES {
                rotate_sink(sink);
            }
            if sink.file.write_all(line.as_bytes()).is_ok() {
                sink.size += line.len() as u64;
            }
        }
    }
    if !echo && in_file {
        return;
    }
    let fd = if in_file { ECHO_FD.load(Ordering::SeqCst) } else { 2 };
    if fd >= 0 {
        raw_write(fd, line.as_bytes());
    }
}

/// Сменить файл, когда текущий дорос до предела.
///
/// Соседний процесс Rexon (перезапуск, хост обоев) пишет в тот же файл и
/// мог уже сдвинуть его сам: тогда файл по пути маленький, и сдвигать ещё
/// раз — значит выбросить свежую историю. Такой файл просто открываем.
fn rotate_sink(sink: &mut Sink) {
    let fresh = std::fs::metadata(&sink.path).map(|m| m.len()).unwrap_or(0);
    if fresh >= MAX_BYTES || same_inode(&sink.path, &sink.file) {
        rotate_files(&sink.path, KEEP);
    }
    let Ok(file) = open_append(&sink.path) else {
        return;
    };
    if sink.stderr_in_file {
        unsafe { libc::dup2(file.as_raw_fd(), 2) };
    }
    RAW_LOG_FD.store(file.as_raw_fd(), Ordering::SeqCst);
    sink.size = file.metadata().map(|m| m.len()).unwrap_or(0);
    sink.file = file;
}

fn same_inode(path: &Path, file: &File) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (std::fs::metadata(path), file.metadata()) {
        (Ok(a), Ok(b)) => a.dev() == b.dev() && a.ino() == b.ino() && a.len() >= MAX_BYTES,
        _ => false,
    }
}

fn raw_write(fd: i32, mut bytes: &[u8]) {
    while !bytes.is_empty() {
        let n = unsafe { libc::write(fd, bytes.as_ptr().cast(), bytes.len()) };
        if n <= 0 {
            return;
        }
        bytes = &bytes[n as usize..];
    }
}

/// Завести дескриптор 2 в файл журнала, сохранив исходный поток ошибок для
/// копий строк (журнал systemd при запуске из меню).
///
/// Зачем дескриптор 2 вообще уводить в файл: Qt, QML, FFmpeg и любая
/// библиотека процесса пишут туда сами, и без этого их сообщения при
/// запуске из меню терялись бы в журнале systemd среди чужих строк. Решение
/// «уводить или нет» — у вызывающего (`cli::install_qt_message_logger`).
pub fn redirect_stderr() -> bool {
    let Ok(mut guard) = SINK.lock() else {
        return false;
    };
    let Some(sink) = guard.as_mut() else {
        return false;
    };
    if sink.stderr_in_file {
        return true;
    }
    if ORIGINAL_STDERR.load(Ordering::SeqCst) < 0 {
        let saved = unsafe { libc::fcntl(2, libc::F_DUPFD_CLOEXEC, 3) };
        if saved >= 0 {
            ORIGINAL_STDERR.store(saved, Ordering::SeqCst);
        }
    }
    if unsafe { libc::dup2(sink.file.as_raw_fd(), 2) } == -1 {
        return false;
    }
    sink.stderr_in_file = true;
    ECHO_FD.store(ORIGINAL_STDERR.load(Ordering::SeqCst), Ordering::SeqCst);
    true
}

/// Исходный поток ошибок для дочернего процесса — копией дескриптора.
///
/// Нужен перезапуску: новый Rexon иначе унаследовал бы дескриптор 2,
/// заведённый в файл, и решил бы, что его запустили с выводом в файл, — а
/// копии строк в журнал systemd пропали бы у всех последующих запусков.
/// `None` — поток не уводился (запуск из терминала), наследуется как есть.
pub fn original_stderr_for_child() -> Option<std::process::Stdio> {
    let fd = ORIGINAL_STDERR.load(Ordering::SeqCst);
    if fd < 0 {
        return None;
    }
    let dup = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 3) };
    if dup < 0 {
        return None;
    }
    use std::os::fd::FromRawFd;
    Some(std::process::Stdio::from(unsafe { std::os::fd::OwnedFd::from_raw_fd(dup) }))
}

/// Последние `max_bytes` журнала (с хвостом прежнего файла, если текущий
/// короче) — для кнопки «Скопировать журнал». Обрезка — по началу строки.
pub fn tail(max_bytes: u64) -> String {
    let path = log_path();
    let mut text = read_tail(&path, max_bytes);
    if (text.len() as u64) < max_bytes {
        let older = read_tail(&rotated_path(&path, 1), max_bytes - text.len() as u64);
        text = older + &text;
    }
    text
}

fn read_tail(path: &Path, max_bytes: u64) -> String {
    let Ok(mut f) = File::open(path) else {
        return String::new();
    };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let start = len.saturating_sub(max_bytes);
    if f.seek(SeekFrom::Start(start)).is_err() {
        return String::new();
    }
    let mut buf = Vec::new();
    let _ = f.read_to_end(&mut buf);
    let mut text = String::from_utf8_lossy(&buf).into_owned();
    if start > 0 {
        if let Some(nl) = text.find('\n') {
            text.drain(..=nl);
        }
    }
    text
}

// ============================================================================
// Смертельный сигнал
// ============================================================================

const FATAL_SIGNALS: [libc::c_int; 5] =
    [libc::SIGSEGV, libc::SIGBUS, libc::SIGILL, libc::SIGFPE, libc::SIGABRT];

static mut OLD_ACTIONS: [Option<libc::sigaction>; 5] = [None; 5];

/// Ставить строку журнала на смертельный сигнал. Прежний обработчик
/// (у Rust — распознавание переполнения стека) не теряется: наш пишет
/// строку, возвращает прежний и выходит, сигнал повторяется уже для него.
pub fn install_fatal_signal_logger() {
    for (i, sig) in FATAL_SIGNALS.iter().enumerate() {
        unsafe {
            let mut act: libc::sigaction = std::mem::zeroed();
            act.sa_sigaction = on_fatal_signal as extern "C" fn(libc::c_int) as usize;
            act.sa_flags = libc::SA_ONSTACK;
            libc::sigemptyset(&mut act.sa_mask);
            let mut old: libc::sigaction = std::mem::zeroed();
            if libc::sigaction(*sig, &act, &mut old) == 0 {
                (*std::ptr::addr_of_mut!(OLD_ACTIONS))[i] = Some(old);
            }
        }
    }
}

fn signal_name(sig: libc::c_int) -> &'static [u8] {
    match sig {
        libc::SIGSEGV => b"SIGSEGV",
        libc::SIGBUS => b"SIGBUS",
        libc::SIGILL => b"SIGILL",
        libc::SIGFPE => b"SIGFPE",
        libc::SIGABRT => b"SIGABRT",
        _ => b"?",
    }
}

/// Десятичная запись числа в буфер без выделения памяти.
fn push_num(buf: &mut [u8], at: &mut usize, mut v: u64) {
    let mut tmp = [0u8; 20];
    let mut n = 0;
    loop {
        tmp[n] = b'0' + (v % 10) as u8;
        n += 1;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    while n > 0 && *at < buf.len() {
        n -= 1;
        buf[*at] = tmp[n];
        *at += 1;
    }
}

fn push_bytes(buf: &mut [u8], at: &mut usize, s: &[u8]) {
    for b in s {
        if *at >= buf.len() {
            return;
        }
        buf[*at] = *b;
        *at += 1;
    }
}

/// Обработчик: только безопасные для сигнала вызовы (`write`, `getpid`,
/// `clock_gettime`, `sigaction`). Время — секунды эпохи: местное время
/// внутри обработчика не посчитать безопасно.
extern "C" fn on_fatal_signal(sig: libc::c_int) {
    let mut buf = [0u8; 160];
    let mut at = 0;
    let mut ts: libc::timespec = unsafe { std::mem::zeroed() };
    unsafe { libc::clock_gettime(libc::CLOCK_REALTIME, &mut ts) };
    push_bytes(&mut buf, &mut at, b"@");
    push_num(&mut buf, &mut at, ts.tv_sec as u64);
    push_bytes(&mut buf, &mut at, b" [");
    push_num(&mut buf, &mut at, unsafe { libc::getpid() } as u64);
    push_bytes(&mut buf, &mut at, b"] ERROR crash: ");
    push_bytes(&mut buf, &mut at, "смертельный сигнал ".as_bytes());
    push_bytes(&mut buf, &mut at, signal_name(sig));
    push_bytes(&mut buf, &mut at, b" (");
    push_num(&mut buf, &mut at, sig as u64);
    push_bytes(&mut buf, &mut at, b")\n");
    let line = &buf[..at];
    let log = RAW_LOG_FD.load(Ordering::SeqCst);
    if log >= 0 {
        raw_write(log, line);
    }
    let echo = ECHO_FD.load(Ordering::SeqCst);
    if echo >= 0 && echo != log {
        raw_write(echo, line);
    }
    let idx = FATAL_SIGNALS.iter().position(|s| *s == sig);
    unsafe {
        match idx.and_then(|i| (*std::ptr::addr_of!(OLD_ACTIONS))[i]) {
            Some(old) => {
                libc::sigaction(sig, &old, std::ptr::null_mut());
            }
            None => {
                libc::signal(sig, libc::SIG_DFL);
            }
        }
    }
    // Возврат из обработчика повторяет ошибку, и сигнал получает уже прежний
    // обработчик (или действие по умолчанию — снимок памяти и смерть).
    // SIGABRT от `abort()` glibc повторяет сама.
}

// ============================================================================
// Макросы
// ============================================================================

/// Строка журнала уровня `debug`: `log_debug!("область", "текст {}", x)`.
#[macro_export]
macro_rules! log_debug {
    ($area:expr, $($arg:tt)+) => {
        if $crate::applog::enabled($crate::applog::Level::Debug) {
            $crate::applog::write($crate::applog::Level::Debug, $area, &format!($($arg)+))
        }
    };
}

/// Строка журнала уровня `info`.
#[macro_export]
macro_rules! log_info {
    ($area:expr, $($arg:tt)+) => {
        $crate::applog::write($crate::applog::Level::Info, $area, &format!($($arg)+))
    };
}

/// Строка журнала уровня `warn`.
#[macro_export]
macro_rules! log_warn {
    ($area:expr, $($arg:tt)+) => {
        $crate::applog::write($crate::applog::Level::Warn, $area, &format!($($arg)+))
    };
}

/// Строка журнала уровня `error`.
#[macro_export]
macro_rules! log_error {
    ($area:expr, $($arg:tt)+) => {
        $crate::applog::write($crate::applog::Level::Error, $area, &format!($($arg)+))
    };
}

// ============================================================================
// Строка запуска
// ============================================================================

/// Версия Rexon с коммитом сборки: `0.5.0 (git 1a2b3c4d5e)`.
pub fn version_line() -> String {
    let v = crate::window_menu::version_string();
    match option_env!("REXON_GIT_REV") {
        Some(rev) => format!("{v} (git {rev})"),
        None => v,
    }
}

/// Окружение рабочего стола одной строкой — что важно для разбора сбоев.
pub fn desktop_env_line() -> String {
    let keys = [
        "XDG_CURRENT_DESKTOP",
        "XDG_SESSION_TYPE",
        "XDG_SESSION_DESKTOP",
        "WAYLAND_DISPLAY",
        "DISPLAY",
        "QT_QPA_PLATFORM",
        crate::installer::PACKAGED_ENV,
    ];
    keys.iter()
        .filter_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()).map(|v| format!("{k}={v}")))
        .collect::<Vec<_>>()
        .join(" ")
}

// ============================================================================
// Привилегированные команды: что показывать в журнале
// ============================================================================

/// Обёртки, после которых команда выполняется от root.
const PRIVILEGE_WORDS: &[&str] = &["pkexec", "sudo", "doas", "run0"];

/// Есть ли в argv повышение прав (обёрткой впереди или флагом помощника
/// AUR `--sudo pkexec`).
pub fn is_privileged(argv: &[String]) -> bool {
    argv.iter().any(|a| {
        let base = a.rsplit('/').next().unwrap_or(a);
        PRIVILEGE_WORDS.contains(&base)
    })
}

/// argv для журнала: длинные аргументы обрезаны, а значения, похожие на
/// секрет (`--password=…`, `token=…`, аргумент после `--password`),
/// заменены звёздочками. Паролей в argv у Rexon нет (pkexec спрашивает
/// через агент polkit), но журнал копируют в чужие руки — страховка.
pub fn redact_argv(argv: &[String]) -> String {
    const SECRET: &[&str] = &["pass", "token", "secret", "apikey", "api_key", "auth"];
    let looks_secret = |s: &str| {
        let low = s.to_ascii_lowercase();
        SECRET.iter().any(|w| low.contains(w))
    };
    let mut out: Vec<String> = Vec::with_capacity(argv.len());
    let mut hide_next = false;
    for a in argv {
        if hide_next {
            out.push("***".to_string());
            hide_next = false;
            continue;
        }
        if let Some((key, _)) = a.split_once('=') {
            if looks_secret(key) {
                out.push(format!("{key}=***"));
                continue;
            }
        } else if a.starts_with('-') && looks_secret(a) {
            out.push(a.clone());
            hide_next = true;
            continue;
        }
        let shown: String = a.chars().take(160).collect();
        let shown = if shown.len() < a.len() { format!("{shown}…") } else { shown };
        if shown.contains(' ') || shown.is_empty() {
            out.push(format!("'{shown}'"));
        } else {
            out.push(shown);
        }
    }
    out.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn уровень_из_переменной() {
        assert_eq!(parse_level(None), Level::Info);
        assert_eq!(parse_level(Some("")), Level::Info);
        assert_eq!(parse_level(Some("DEBUG")), Level::Debug);
        assert_eq!(parse_level(Some(" warn ")), Level::Warn);
        assert_eq!(parse_level(Some("error")), Level::Error);
        assert_eq!(parse_level(Some("чепуха")), Level::Info);
    }

    #[test]
    fn level_names_of_old_sinks() {
        assert_eq!(level_from_name("info"), Level::Info);
        assert_eq!(level_from_name("warning"), Level::Warn);
        assert_eq!(level_from_name("critical"), Level::Error);
        assert_eq!(level_from_name("error"), Level::Error);
        assert_eq!(level_from_name("debug"), Level::Debug);
        assert_eq!(level_from_name("нечто"), Level::Debug);
    }

    #[test]
    fn строка_с_временем_процессом_и_отступом() {
        let line = format_line("2026-10-08 12:00:00.001", 42, Level::Warn, "restart", "раз\nдва\n");
        assert_eq!(line, "2026-10-08 12:00:00.001 [42] WARN  restart: раз\n    два\n");
    }

    #[test]
    fn ротация_сдвигает_и_выбрасывает_самый_старый() {
        let dir = std::env::temp_dir().join(format!("rexon-applog-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("rexon.log");
        for gen in 0..5 {
            std::fs::write(&p, format!("{gen}")).unwrap();
            rotate_files(&p, 3);
        }
        assert!(!p.exists());
        assert_eq!(std::fs::read_to_string(rotated_path(&p, 1)).unwrap(), "4");
        assert_eq!(std::fs::read_to_string(rotated_path(&p, 3)).unwrap(), "2");
        assert!(!rotated_path(&p, 4).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn хвост_режется_по_строке() {
        let dir = std::env::temp_dir().join(format!("rexon-applog-tail-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("x.log");
        std::fs::write(&p, "первая\nвторая\nтретья\n").unwrap();
        let t = read_tail(&p, 15);
        assert_eq!(t, "третья\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn секреты_не_попадают_в_журнал() {
        let argv: Vec<String> = ["pkexec", "/usr/lib/rexon-system-helper", "--password", "x1", "token=abc", "a b"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let s = redact_argv(&argv);
        assert_eq!(s, "pkexec /usr/lib/rexon-system-helper --password *** token=*** 'a b'");
        assert!(is_privileged(&argv));
        let aur: Vec<String> = ["paru", "-S", "--sudo", "pkexec", "x"].iter().map(|s| s.to_string()).collect();
        assert!(is_privileged(&aur));
        let plain: Vec<String> = ["pacman", "-Q"].iter().map(|s| s.to_string()).collect();
        assert!(!is_privileged(&plain));
    }
}
