//! The Rexon log: ONE mechanism for the whole product.
//!
//! WHY. A real case on 08.10.2026: the owner switched the language, pressed
//! "Restart", and Rexon did not come back. The systemd journal had nothing,
//! and the file `~/.config/rexon/rexon.log` contained a single line without a
//! timestamp (a "symbol lookup error" from the new copy) in the middle of a
//! day's worth of other Qt messages. No time, no process ID, no restart step —
//! the cause cannot be reconstructed from such a log. Before this change the
//! "log" was a scattering of `eprintln!` calls with various prefixes, and the
//! file was descriptor 2 redirected into `~/.config/rexon/rexon.log` with no
//! size limit.
//!
//! WHAT IS HERE:
//!   * the file `$XDG_STATE_HOME/rexon/rexon.log` (by default
//!     `~/.local/state/rexon/rexon.log`) with size-based rotation:
//!     [`MAX_BYTES`] per file, [`KEEP`] previous ones (`rexon.log.1` … `.3`);
//!   * every record is one line: time to the millisecond, process ID, level,
//!     area and text;
//!   * the same line goes to the process's original error stream: the
//!     terminal when started from a console, the systemd journal when started
//!     from the menu (descriptor 2 then goes to the file for the Qt messages,
//!     but the original stream is preserved);
//!   * levels `debug`/`info`/`warn`/`error`; by default `info` and above are
//!     written, `REXON_LOG=debug` enables the detailed log;
//!   * a line about a fatal signal (SIGSEGV and its kin) — previously the
//!     process vanished without a trace.
//!
//! THE RULE. A product event (startup, language, restart, the single-instance
//! lock, exit, a privileged command, the AUR queue, a theme installation) is
//! written ONLY here, with the [`crate::log_info!`] macro and its siblings. A
//! new `eprintln!` for an event is a second log, which must not exist
//! (rule #12).
//!
//! Before [`init`] (tests, service binaries) records go only to the error
//! stream — just as `eprintln!` did — and no test writes into the profile.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI32, AtomicU8, Ordering};
use std::sync::Mutex;

/// The size limit of a single log file, in bytes.
pub const MAX_BYTES: u64 = 2 * 1024 * 1024;
/// How many previous files to keep (`rexon.log.1` is the most recent).
pub const KEEP: usize = 3;
/// The level variable: `debug`, `info` (the default), `warn`, `error`.
pub const LEVEL_ENV: &str = "REXON_LOG";

/// The record level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Debug = 10,
    Info = 20,
    Warn = 30,
    Error = 40,
}

impl Level {
    /// The level tag in a log line: exactly five characters, so the columns line up.
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

/// The value of [`LEVEL_ENV`] → the lowest level that is written. Unknown and
/// empty mean `info`: a typo in the variable must not silence the log.
pub fn parse_level(value: Option<&str>) -> Level {
    match value.map(|v| v.trim().to_ascii_lowercase()).as_deref() {
        Some("debug" | "trace" | "all" | "1" | "verbose") => Level::Debug,
        Some("warn" | "warning") => Level::Warn,
        Some("error") => Level::Error,
        _ => Level::Info,
    }
}

/// A level name in the style of Python's `logging` (`debug`, `info`,
/// `warning`, `error`, `critical`) → a log level. This is how the older core
/// receivers (`wallpaper_exec`, `splash_exec`) speak: their lines go here as
/// well rather than into their own `eprintln!`. Unknown means `debug`: unknown
/// noise is no reason to clutter the default log.
pub fn level_from_name(name: &str) -> Level {
    match name.trim().to_ascii_lowercase().as_str() {
        "info" => Level::Info,
        "warning" | "warn" => Level::Warn,
        "error" | "critical" | "fatal" => Level::Error,
        _ => Level::Debug,
    }
}

/// 0 means not yet read from the environment.
static MIN_LEVEL: AtomicU8 = AtomicU8::new(0);

/// The lowest level that is written (read from [`LEVEL_ENV`] once).
pub fn min_level() -> Level {
    let v = MIN_LEVEL.load(Ordering::Relaxed);
    if v != 0 {
        return Level::from_u8(v);
    }
    let level = parse_level(std::env::var(LEVEL_ENV).ok().as_deref());
    MIN_LEVEL.store(level as u8, Ordering::Relaxed);
    level
}

/// Whether `level` is written, for expensive messages (`if enabled(Debug) {…}`).
pub fn enabled(level: Level) -> bool {
    level >= min_level()
}

// ============================================================================
// The log line
// ============================================================================

/// The record timestamp: local time to the millisecond. The clock is the same
/// as for crash reports (`qt_app::Timestamp`): there is no second
/// `localtime_r` in the product.
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

/// Builds a log line. Multi-line text does not break the columns: from the
/// second line on it is indented, so `grep` at the line start finds only records.
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
// The file and rotation
// ============================================================================

struct Sink {
    path: PathBuf,
    file: File,
    size: u64,
    /// Descriptor 2 has been redirected into this file ([`redirect_stderr`]):
    /// after rotation it must be moved to the new file.
    stderr_in_file: bool,
}

static SINK: Mutex<Option<Sink>> = Mutex::new(None);

/// Where to duplicate the line: the original error stream. `-1` means nowhere
/// (the error stream is the log file itself, otherwise every line would land
/// twice).
static ECHO_FD: AtomicI32 = AtomicI32::new(2);
/// The saved copy of the original error stream after [`redirect_stderr`].
static ORIGINAL_STDERR: AtomicI32 = AtomicI32::new(-1);
/// The raw descriptor of the log file, for the fatal signal line, where
/// neither locks nor memory allocation are allowed.
static RAW_LOG_FD: AtomicI32 = AtomicI32::new(-1);

/// The name of previous file number `n` (`rexon.log.1`).
pub fn rotated_path(path: &Path, n: usize) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(format!(".{n}"));
    PathBuf::from(s)
}

/// Shifts the previous files: `.2→.3`, `.1→.2`, current → `.1`. The oldest is
/// dropped. Errors are silent: the log is no reason to crash the program.
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

/// Whether descriptor `fd` refers to the same file as `file` (by device and inode).
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

/// Sets up the process log. A repeated call does nothing. Returns the file
/// path; `None` means the file could not be opened (then records go only to
/// the error stream, and the program keeps running).
pub fn init() -> Option<PathBuf> {
    init_at(PathBuf::from(crate::utils::log_file_env()))
}

/// [`init`] with an explicit path, for test stands.
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
    // A process started by a restart of the previous version may have inherited
    // descriptor 2 already redirected into this file: duplicating there is not allowed.
    if same_file(2, &file) {
        ECHO_FD.store(-1, Ordering::SeqCst);
    }
    RAW_LOG_FD.store(file.as_raw_fd(), Ordering::SeqCst);
    *guard = Some(Sink { path: path.clone(), file, size, stderr_in_file: false });
    Some(path)
}

/// The log path of this process (or where it would go after [`init`]).
pub fn log_path() -> PathBuf {
    SINK.lock()
        .ok()
        .and_then(|g| g.as_ref().map(|s| s.path.clone()))
        .unwrap_or_else(|| PathBuf::from(crate::utils::log_file_env()))
}

/// Whether the log file has been set up ([`init`]). If not, the process is a
/// service process or a test stand (the window's self-check) and does not
/// need lines for humans.
pub fn is_active() -> bool {
    SINK.lock().map(|g| g.is_some()).unwrap_or(false)
}

/// Writes a line of level `level` in area `area`.
pub fn write(level: Level, area: &str, msg: &str) {
    write_opts(level, area, msg, true);
}

/// The same, but without a copy to the error stream (`echo = false`): for
/// lines the caller has already printed to stdout for the person itself.
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

/// Switches the file when the current one has reached the limit.
///
/// A neighboring Rexon process (a restart, the wallpaper host) writes to the
/// same file and may have already rotated it itself: then the file at the path
/// is small, and rotating once more would throw away fresh history. Such a
/// file is simply opened.
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

/// Redirects descriptor 2 into the log file, keeping the original error
/// stream for line copies (the systemd journal when started from the menu).
///
/// Why move descriptor 2 into the file at all: Qt, QML, FFmpeg and any library
/// in the process write there themselves, and without this their messages
/// would be lost among other lines in the systemd journal when started from
/// the menu. The decision "redirect or not" belongs to the caller
/// (`cli::install_qt_message_logger`).
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

/// The original error stream for a child process, as a copy of the descriptor.
///
/// Needed by the restart: otherwise the new Rexon would inherit descriptor 2
/// redirected into the file and decide it had been started with output to a
/// file — and the line copies to the systemd journal would disappear for all
/// subsequent starts. `None` means the stream was not redirected (started
/// from a terminal) and is inherited as is.
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

/// The last `max_bytes` of the log (with the tail of the previous file if the
/// current one is shorter), for the "Copy log" button. Trimmed at a line start.
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
// Fatal signal
// ============================================================================

const FATAL_SIGNALS: [libc::c_int; 5] =
    [libc::SIGSEGV, libc::SIGBUS, libc::SIGILL, libc::SIGFPE, libc::SIGABRT];

static mut OLD_ACTIONS: [Option<libc::sigaction>; 5] = [None; 5];

/// Installs the log line on a fatal signal. The previous handler (in Rust,
/// stack overflow detection) is not lost: ours writes the line, restores the
/// previous one and returns, and the signal recurs for it.
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

/// The decimal representation of a number into a buffer without memory allocation.
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

/// The handler: only async-signal-safe calls (`write`, `getpid`,
/// `clock_gettime`, `sigaction`). The time is in epoch seconds: local time
/// cannot be computed safely inside the handler.
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
    // Returning from the handler repeats the fault, and the signal then goes to
    // the previous handler (or the default action: a core dump and death).
    // glibc repeats SIGABRT from `abort()` by itself.
}

// ============================================================================
// Termination signal (SIGTERM, SIGINT, SIGHUP)
// ============================================================================
//
// WHY. A process that the desktop or a person told to "terminate" used to
// leave silently: the log just broke off, and it was impossible to tell
// whether Rexon crashed, hung or was closed. Now there is a line "terminated
// by signal X from process N (name)", and then everything is as before: the
// default action (exit) or the previous handler. SIGKILL cannot be caught —
// for it the action log and the window watchdog (`ui_watch`) leave the
// evidence.

const EXIT_SIGNALS: [libc::c_int; 3] = [libc::SIGTERM, libc::SIGINT, libc::SIGHUP];
static mut OLD_EXIT_ACTIONS: [Option<libc::sigaction>; 3] = [None; 3];
/// The local time offset from UTC, in seconds, computed at installation:
/// `localtime_r` must not be called inside the handler.
static UTC_OFFSET: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);

/// Installs the log line on termination signals. A signal the process was
/// told to ignore (`nohup` → SIGHUP) stays ignored.
pub fn install_exit_signal_logger() {
    unsafe {
        let now = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        if !libc::localtime_r(&now, &mut tm).is_null() {
            UTC_OFFSET.store(tm.tm_gmtoff as i64, Ordering::SeqCst);
        }
    }
    for (i, sig) in EXIT_SIGNALS.iter().enumerate() {
        unsafe {
            let mut old: libc::sigaction = std::mem::zeroed();
            if libc::sigaction(*sig, std::ptr::null(), &mut old) != 0 || old.sa_sigaction == libc::SIG_IGN {
                continue;
            }
            let mut act: libc::sigaction = std::mem::zeroed();
            act.sa_sigaction =
                on_exit_signal as extern "C" fn(libc::c_int, *mut libc::siginfo_t, *mut libc::c_void) as usize;
            act.sa_flags = libc::SA_SIGINFO;
            libc::sigemptyset(&mut act.sa_mask);
            if libc::sigaction(*sig, &act, &mut old) == 0 {
                (*std::ptr::addr_of_mut!(OLD_EXIT_ACTIONS))[i] = Some(old);
            }
        }
    }
}

fn exit_signal_name(sig: libc::c_int) -> &'static [u8] {
    match sig {
        libc::SIGTERM => b"SIGTERM",
        libc::SIGINT => b"SIGINT",
        libc::SIGHUP => b"SIGHUP",
        _ => b"?",
    }
}

/// A number zero-padded to `width` characters.
fn push_pad(buf: &mut [u8], at: &mut usize, v: u64, width: usize) {
    let mut digits = 1;
    let mut t = v;
    while t >= 10 {
        t /= 10;
        digits += 1;
    }
    for _ in digits..width {
        push_bytes(buf, at, b"0");
    }
    push_num(buf, at, v);
}

/// Days since 1970-01-01 → (year, month, day) in the Gregorian calendar.
fn civil_from_days(z: i64) -> (i64, u64, u64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u64;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u64;
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    (y, m, d)
}

/// The termination signal line, in the same form as all log lines. Without
/// memory allocation: it is built in the signal handler. `sender` 0 means the
/// sender is unknown (the kernel, a terminal); `comm` is the sender's name.
pub fn exit_signal_line(
    buf: &mut [u8],
    local_secs: i64,
    millis: u64,
    pid: u64,
    sig: libc::c_int,
    sender: u64,
    comm: &[u8],
) -> usize {
    let mut at = 0;
    let (y, mo, d) = civil_from_days(local_secs.div_euclid(86_400));
    let sod = local_secs.rem_euclid(86_400) as u64;
    push_pad(buf, &mut at, y.max(0) as u64, 4);
    push_bytes(buf, &mut at, b"-");
    push_pad(buf, &mut at, mo, 2);
    push_bytes(buf, &mut at, b"-");
    push_pad(buf, &mut at, d, 2);
    push_bytes(buf, &mut at, b" ");
    push_pad(buf, &mut at, sod / 3600, 2);
    push_bytes(buf, &mut at, b":");
    push_pad(buf, &mut at, sod / 60 % 60, 2);
    push_bytes(buf, &mut at, b":");
    push_pad(buf, &mut at, sod % 60, 2);
    push_bytes(buf, &mut at, b".");
    push_pad(buf, &mut at, millis, 3);
    push_bytes(buf, &mut at, b" [");
    push_num(buf, &mut at, pid);
    push_bytes(buf, &mut at, b"] WARN  exit: ");
    push_bytes(buf, &mut at, "завершение по сигналу ".as_bytes());
    push_bytes(buf, &mut at, exit_signal_name(sig));
    push_bytes(buf, &mut at, b" (");
    push_num(buf, &mut at, sig as u64);
    push_bytes(buf, &mut at, b")");
    if sender != 0 {
        push_bytes(buf, &mut at, " от процесса ".as_bytes());
        push_num(buf, &mut at, sender);
        if !comm.is_empty() {
            push_bytes(buf, &mut at, b" (");
            push_bytes(buf, &mut at, comm);
            push_bytes(buf, &mut at, b")");
        }
    }
    push_bytes(buf, &mut at, b"\n");
    at
}

/// The name of process `pid` from `/proc/<pid>/comm`, using only `open`/`read`/`close`.
fn read_comm(pid: u64, out: &mut [u8; 32]) -> usize {
    let mut path = [0u8; 40];
    let mut at = 0;
    push_bytes(&mut path, &mut at, b"/proc/");
    push_num(&mut path, &mut at, pid);
    push_bytes(&mut path, &mut at, b"/comm\0");
    let fd = unsafe { libc::open(path.as_ptr().cast(), libc::O_RDONLY | libc::O_CLOEXEC) };
    if fd < 0 {
        return 0;
    }
    let n = unsafe { libc::read(fd, out.as_mut_ptr().cast(), out.len()) };
    unsafe { libc::close(fd) };
    let mut n = n.max(0) as usize;
    while n > 0 && (out[n - 1] == b'\n' || out[n - 1] == 0) {
        n -= 1;
    }
    n
}

extern "C" fn on_exit_signal(sig: libc::c_int, info: *mut libc::siginfo_t, _ctx: *mut libc::c_void) {
    let mut ts: libc::timespec = unsafe { std::mem::zeroed() };
    unsafe { libc::clock_gettime(libc::CLOCK_REALTIME, &mut ts) };
    let sender = if info.is_null() { 0 } else { unsafe { (*info).si_pid() }.max(0) as u64 };
    let mut comm = [0u8; 32];
    let comm_len = if sender != 0 { read_comm(sender, &mut comm) } else { 0 };
    let mut buf = [0u8; 256];
    let n = exit_signal_line(
        &mut buf,
        ts.tv_sec as i64 + UTC_OFFSET.load(Ordering::Relaxed),
        (ts.tv_nsec / 1_000_000) as u64,
        unsafe { libc::getpid() } as u64,
        sig,
        sender,
        &comm[..comm_len],
    );
    let line = &buf[..n];
    let log = RAW_LOG_FD.load(Ordering::SeqCst);
    if log >= 0 {
        raw_write(log, line);
    }
    let echo = ECHO_FD.load(Ordering::SeqCst);
    if echo >= 0 && echo != log {
        raw_write(echo, line);
    }
    // From here on, as if we were not there: restore the previous action and
    // re-raise the signal. While the handler runs the signal is blocked: the
    // repeat is delivered right after returning — to the previous handler or the
    // default action.
    let idx = EXIT_SIGNALS.iter().position(|s| *s == sig);
    unsafe {
        match idx.and_then(|i| (*std::ptr::addr_of!(OLD_EXIT_ACTIONS))[i]) {
            Some(old) => {
                libc::sigaction(sig, &old, std::ptr::null_mut());
            }
            None => {
                libc::signal(sig, libc::SIG_DFL);
            }
        }
        libc::raise(sig);
    }
}

// ============================================================================
// Macros
// ============================================================================

/// A log line of level `debug`: `log_debug!("area", "text {}", x)`.
#[macro_export]
macro_rules! log_debug {
    ($area:expr, $($arg:tt)+) => {
        if $crate::applog::enabled($crate::applog::Level::Debug) {
            $crate::applog::write($crate::applog::Level::Debug, $area, &format!($($arg)+))
        }
    };
}

/// A log line of level `info`.
#[macro_export]
macro_rules! log_info {
    ($area:expr, $($arg:tt)+) => {
        $crate::applog::write($crate::applog::Level::Info, $area, &format!($($arg)+))
    };
}

/// A log line of level `warn`.
#[macro_export]
macro_rules! log_warn {
    ($area:expr, $($arg:tt)+) => {
        $crate::applog::write($crate::applog::Level::Warn, $area, &format!($($arg)+))
    };
}

/// A log line of level `error`.
#[macro_export]
macro_rules! log_error {
    ($area:expr, $($arg:tt)+) => {
        $crate::applog::write($crate::applog::Level::Error, $area, &format!($($arg)+))
    };
}

// ============================================================================
// The startup line
// ============================================================================

/// The Rexon version with the build commit: `0.5.0 (git 1a2b3c4d5e)`.
pub fn version_line() -> String {
    let v = crate::window_menu::version_string();
    match option_env!("REXON_GIT_REV") {
        Some(rev) => format!("{v} (git {rev})"),
        None => v,
    }
}

/// The desktop environment in a single line: what matters for analyzing failures.
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
// Privileged commands: what to show in the log
// ============================================================================

/// Wrappers after which a command runs as root.
const PRIVILEGE_WORDS: &[&str] = &["pkexec", "sudo", "doas", "run0"];

/// Whether argv contains a privilege elevation (a wrapper in front or the AUR
/// helper flag `--sudo pkexec`).
pub fn is_privileged(argv: &[String]) -> bool {
    argv.iter().any(|a| {
        let base = a.rsplit('/').next().unwrap_or(a);
        PRIVILEGE_WORDS.contains(&base)
    })
}

/// argv for the log: long arguments are truncated, and values that look like
/// secrets (`--password=…`, `token=…`, the argument after `--password`) are
/// replaced with asterisks. Rexon has no passwords in argv (pkexec asks
/// through the polkit agent), but logs get copied into other hands — this is
/// a safeguard.
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
    fn строка_сигнала_завершения_как_у_журнала() {
        let mut buf = [0u8; 256];
        // 2026-11-15 13:05:09 local time.
        let secs = 1_763_211_909 + 365 * 86_400;
        let n = exit_signal_line(&mut buf, secs, 7, 4242, libc::SIGTERM, 1234, b"cinnamon");
        assert_eq!(
            std::str::from_utf8(&buf[..n]).unwrap(),
            "2026-11-15 13:05:09.007 [4242] WARN  exit: завершение по сигналу SIGTERM (15) от процесса 1234 (cinnamon)\n"
        );
        let n = exit_signal_line(&mut buf, 0, 0, 1, libc::SIGINT, 0, b"");
        assert_eq!(
            std::str::from_utf8(&buf[..n]).unwrap(),
            "1970-01-01 00:00:00.000 [1] WARN  exit: завершение по сигналу SIGINT (2)\n"
        );
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
        assert_eq!(civil_from_days(19_782), (2024, 2, 29));
    }

    /// A live run: the same test binary, started as a child, sets up the log,
    /// installs the handler and receives SIGTERM. The line must land in the file,
    /// and the process must die from the same signal, as without the handler.
    #[test]
    fn sigterm_оставляет_строку_и_убивает_как_раньше() {
        const CHILD: &str = "REXON_APPLOG_SIGTERM_CHILD";
        if let Ok(path) = std::env::var(CHILD) {
            init_at(PathBuf::from(path));
            install_exit_signal_logger();
            unsafe { libc::raise(libc::SIGTERM) };
            // This point must not be reached: SIGTERM must terminate the process.
            std::process::exit(0);
        }
        let dir = std::env::temp_dir().join(format!("rexon-applog-sigterm-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("rexon.log");
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "applog::tests::sigterm_оставляет_строку_и_убивает_как_раньше", "--nocapture"])
            .env(CHILD, &log)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(status.signal(), Some(libc::SIGTERM), "{status:?}");
        let text = std::fs::read_to_string(&log).unwrap();
        assert!(text.contains("] WARN  exit: завершение по сигналу SIGTERM (15) от процесса "), "{text}");
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
