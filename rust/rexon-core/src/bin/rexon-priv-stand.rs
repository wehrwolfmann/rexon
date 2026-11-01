//! `rexon-priv-stand` — помощник сеанса прав для СТЕНДА (уровень 2 из
//! трёх, см. docs/testing.md). ⛔ Только тестовая сборка: бинарь собирается лишь с
//! feature `stand` (`required-features` в `Cargo.toml`), пакет и `build-app`
//! его не собирают. Код стенда живёт ТОЛЬКО в этом файле — библиотека ядра и
//! настоящий `rexon-priv-helper` от feature не меняются ни на байт (сторож —
//! `tests/priv_userns.rs`, `stand_code_stays_out_of_release`).
//!
//! Что это: тот же сеанс помощника ([`helper::run_session`] — проверка
//! собеседника, рукопожатие, отзыв, проверки операций, исполнение), но вместо
//! системы — «сухой» [`RootIo`] с корнем-префиксом:
//!
//! * программы НЕ запускаются: argv записывается в журнал стенда;
//! * файлы читаются и пишутся под `<стенд>/root` (ничего вне каталога стенда);
//! * logind — файл `<стенд>/session` (ответ `loginctl show-session`), сон —
//!   `<стенд>/slept_ms`, часы — `<стенд>/clock_ms`: тест меняет файл, и
//!   настоящий `LogindWatch`/`Revoker` помощника видят блокировку, сон, простой.
//!
//! Запуск — как у настоящего, через «шим pkexec» теста: сокет окна на fd 0/1,
//! `PKEXEC_UID`, аргументы `[--session <id>] [--lang <код>]`. Каталог стенда —
//! `REXON_STAND_DIR` (без него — отказ). Необязательно: `REXON_STAND_POLL_MS`
//! (опрос logind, по умолчанию 50 мс), `REXON_STAND_BUILD` (чужой отпечаток
//! сборки — для `BuildMismatch`).
//!
//! Журнал `<стенд>/journal` — по строке на событие:
//! `run <argv>`, `held <argv>`, `write <путь> <режим>`, `copy <куда>`,
//! `mkdir <путь>`, `rm <путь>`, `log <строка помощника>`, `exit <код>`,
//! `probe fc-query <копия>` (ответ — файл `<стенд>/fc-query`).
//! Файл `<стенд>/hold` задерживает каждую команду, пока его не удалят
//! (операция «посреди транзакции»).

use std::io::Write;
use std::os::fd::{FromRawFd, OwnedFd};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rexon_core::privilege::helper::{self, Captured, RootIo, RunExtra, SessionConfig, SessionEnd};
use rexon_core::privilege::revoke::{self, Clock, LogindWatch, SuspendGauge};
use rexon_core::privilege::{client, proto, validate};

/// Переменная с каталогом стенда.
const DIR_ENV: &str = "REXON_STAND_DIR";

struct Stand {
    dir: PathBuf,
    root: PathBuf,
    journal: Mutex<std::fs::File>,
}

impl Stand {
    fn note(&self, line: &str) {
        let clean: String = line.chars().map(|c| if c == '\n' { ' ' } else { c }).collect();
        let mut f = self.journal.lock().unwrap_or_else(|e| e.into_inner());
        let _ = writeln!(f, "{clean}");
        let _ = f.flush();
    }

    /// Системный путь → путь под корнем стенда. Путь, уже лежащий под
    /// корнем (рабочий каталог помощника — `work_base`), остаётся как есть.
    fn real(&self, path: &str) -> PathBuf {
        let p = Path::new(path);
        if p.starts_with(&self.root) {
            p.to_path_buf()
        } else {
            self.root.join(path.trim_start_matches('/'))
        }
    }

    fn read_ms(&self, name: &str) -> Duration {
        let ms = std::fs::read_to_string(self.dir.join(name))
            .ok()
            .and_then(|s| s.trim().parse::<u64>().ok())
            .unwrap_or(0);
        Duration::from_millis(ms)
    }
}

/// «Сухой» исполнитель стенда.
struct StandIo(Arc<Stand>);

fn exec_file(p: &Path) -> bool {
    std::fs::metadata(p).map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0).unwrap_or(false)
}

fn copy_tree(src: &Path, dst: &Path) -> Result<(), String> {
    let err = |p: &Path, e: std::io::Error| format!("{}: {e}", p.display());
    match std::fs::symlink_metadata(dst) {
        Ok(m) if m.is_dir() => {}
        Ok(_) => std::fs::remove_file(dst).map_err(|e| err(dst, e))?,
        Err(_) => std::fs::DirBuilder::new().recursive(true).mode(0o755).create(dst).map_err(|e| err(dst, e))?,
    }
    for entry in std::fs::read_dir(src).map_err(|e| err(src, e))? {
        let entry = entry.map_err(|e| err(src, e))?;
        let (from, to) = (entry.path(), dst.join(entry.file_name()));
        let ty = entry.file_type().map_err(|e| err(&from, e))?;
        if ty.is_dir() {
            copy_tree(&from, &to)?;
        } else if ty.is_file() {
            if let Ok(m) = std::fs::symlink_metadata(&to) {
                if m.is_dir() {
                    std::fs::remove_dir_all(&to).map_err(|e| err(&to, e))?;
                } else {
                    std::fs::remove_file(&to).map_err(|e| err(&to, e))?;
                }
            }
            std::fs::copy(&from, &to).map_err(|e| err(&from, e))?;
        } else {
            return Err(format!("{}: не файл и не каталог", from.display()));
        }
    }
    Ok(())
}

impl RootIo for StandIo {
    fn run(&self, argv: &[String], _extra: &RunExtra, out: &mut dyn FnMut(&str)) -> Result<Option<i32>, String> {
        let line = argv.join(" ");
        let hold = self.0.dir.join("hold");
        if hold.exists() {
            self.0.note(&format!("held {line}"));
            while hold.exists() {
                std::thread::sleep(Duration::from_millis(20));
            }
        }
        self.0.note(&format!("run {line}"));
        out(&format!("стенд: {line}"));
        let code = std::fs::read_to_string(self.0.dir.join("run_code"))
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0);
        Ok(Some(code))
    }

    fn capture(&self, argv: &[String]) -> Captured {
        let words: Vec<&str> = argv.iter().map(String::as_str).collect();
        match words.as_slice() {
            [revoke::LOGINCTL, "show-session", ..] => match std::fs::read_to_string(self.0.dir.join("session")) {
                Ok(text) => Captured { code: Some(0), stdout: text },
                Err(_) => Captured { code: Some(1), stdout: String::new() },
            },
            [revoke::LOGINCTL, ..] => Captured { code: Some(1), stdout: String::new() },
            // fontconfig стенда: ответ `fc-query` — файл `<стенд>/fc-query`
            // (нет файла — «не шрифт»), путь копии — в журнал.
            ["fc-query", .., path] => {
                self.0.note(&format!("probe fc-query {path}"));
                match std::fs::read_to_string(self.0.dir.join("fc-query")) {
                    Ok(text) => Captured { code: Some(0), stdout: text },
                    Err(_) => Captured { code: Some(1), stdout: String::new() },
                }
            }
            _ => Captured { code: Some(0), stdout: String::new() },
        }
    }

    fn which(&self, name: &str) -> Option<String> {
        if name.is_empty() {
            return None;
        }
        if name.contains('/') {
            return (name.starts_with('/') && exec_file(&self.0.real(name))).then(|| name.to_string());
        }
        ["/usr/bin", "/usr/sbin"]
            .iter()
            .map(|d| format!("{d}/{name}"))
            .find(|p| exec_file(&self.0.real(p)))
    }

    fn read_text(&self, path: &str) -> Option<String> {
        std::fs::read_to_string(self.0.real(path)).ok()
    }

    fn exists(&self, path: &str) -> bool {
        self.0.real(path).exists()
    }

    fn is_file(&self, path: &str) -> bool {
        self.0.real(path).is_file()
    }

    fn is_dir(&self, path: &str) -> bool {
        self.0.real(path).is_dir()
    }

    fn is_symlink(&self, path: &str) -> bool {
        self.0.real(path).is_symlink()
    }

    fn canonicalize(&self, path: &str) -> Option<String> {
        let canon = std::fs::canonicalize(self.0.real(path)).ok()?;
        let rel = canon.strip_prefix(&self.0.root).ok()?;
        Some(format!("/{}", rel.to_string_lossy()))
    }

    fn list_dir(&self, path: &str) -> Vec<String> {
        std::fs::read_dir(self.0.real(path))
            .map(|rd| rd.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect())
            .unwrap_or_default()
    }

    fn write_file(&self, path: &str, content: &str, mode: u32) -> Result<(), String> {
        self.0.note(&format!("write {path} {mode:o}"));
        let real = self.0.real(path);
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(mode)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&real)
            .map_err(|e| format!("{path}: {e}"))?;
        f.write_all(content.as_bytes()).map_err(|e| format!("{path}: {e}"))?;
        f.set_permissions(std::fs::Permissions::from_mode(mode)).map_err(|e| format!("{path}: {e}"))
    }

    fn copy_file(&self, src: &str, dst: &str) -> Result<(), String> {
        self.0.note(&format!("copy {dst}"));
        std::fs::copy(self.0.real(src), self.0.real(dst)).map(|_| ()).map_err(|e| format!("{src} → {dst}: {e}"))
    }

    fn rename(&self, from: &str, to: &str) -> Result<(), String> {
        self.0.note(&format!("rename {from} {to}"));
        std::fs::rename(self.0.real(from), self.0.real(to)).map_err(|e| format!("{from} → {to}: {e}"))
    }

    fn remove_file(&self, path: &str) -> Result<(), String> {
        self.0.note(&format!("rm {path}"));
        std::fs::remove_file(self.0.real(path)).map_err(|e| format!("{path}: {e}"))
    }

    fn remove_dir(&self, path: &str) -> Result<(), String> {
        self.0.note(&format!("rmdir {path}"));
        std::fs::remove_dir(self.0.real(path)).map_err(|e| format!("{path}: {e}"))
    }

    fn create_dir_all(&self, path: &str, mode: u32) -> Result<(), String> {
        self.0.note(&format!("mkdir {path}"));
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(mode)
            .create(self.0.real(path))
            .map_err(|e| format!("{path}: {e}"))
    }

    fn copy_tree(&self, src: &str, dst: &str) -> Result<(), String> {
        self.0.note(&format!("copy-tree {dst}"));
        copy_tree(&self.0.real(src), &self.0.real(dst))
    }

    fn remove_tree(&self, path: &str) -> Result<(), String> {
        self.0.note(&format!("rm-tree {path}"));
        std::fs::remove_dir_all(self.0.real(path)).map_err(|e| format!("{path}: {e}"))
    }

    fn work_base(&self) -> PathBuf {
        self.0.real("/var/tmp")
    }

    fn home_of(&self, _uid: u32) -> Option<String> {
        Some("/home/stand".into())
    }

    fn log(&self, line: &str) {
        self.0.note(&format!("log {line}"));
    }
}

/// Часы стенда: `<стенд>/clock_ms`.
struct StandClock(Arc<Stand>);

impl Clock for StandClock {
    fn now(&self) -> Duration {
        self.0.read_ms("clock_ms")
    }
}

/// Счётчик сна стенда: `<стенд>/slept_ms`.
struct StandGauge(Arc<Stand>);

impl SuspendGauge for StandGauge {
    fn slept_total(&self) -> Duration {
        self.0.read_ms("slept_ms")
    }
}

/// Сокет окна с fd 0 — в свой fd с `CLOEXEC`, на 0/1 — `/dev/null` (как у
/// настоящего помощника).
fn take_socket_from_stdio() -> Option<OwnedFd> {
    let mut ty: libc::c_int = 0;
    let mut len = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
    // SAFETY: getsockopt/fcntl/open/dup2 на своих fd; результаты проверяются.
    unsafe {
        if libc::getsockopt(0, libc::SOL_SOCKET, libc::SO_TYPE, (&mut ty as *mut libc::c_int).cast(), &mut len) != 0
            || ty != libc::SOCK_SEQPACKET
        {
            return None;
        }
        let own = libc::fcntl(0, libc::F_DUPFD_CLOEXEC, 3);
        if own < 0 {
            return None;
        }
        let sock = OwnedFd::from_raw_fd(own);
        let nfd = libc::open(c"/dev/null".as_ptr(), libc::O_RDWR | libc::O_CLOEXEC);
        if nfd < 0 || libc::dup2(nfd, 0) < 0 || libc::dup2(nfd, 1) < 0 {
            return None;
        }
        if nfd > 2 {
            libc::close(nfd);
        }
        Some(sock)
    }
}

fn own_build() -> String {
    std::env::var("REXON_STAND_BUILD").unwrap_or_else(|_| proto::build_id())
}

fn session(stand: &Arc<Stand>) -> i32 {
    // SAFETY: getppid без предусловий.
    let parent = unsafe { libc::getppid() };
    if parent <= 1 {
        return helper::EXIT_CHANNEL;
    }
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let Some(args) = helper::parse_args(&raw) else {
        stand.note("log неверные аргументы");
        return helper::EXIT_USAGE;
    };
    let Some(pkexec_uid) = std::env::var("PKEXEC_UID").ok().and_then(|s| validate::parse_pkexec_uid(&s)) else {
        stand.note("log запущен не через шим pkexec");
        return helper::EXIT_NOT_PKEXEC;
    };
    let Some(sock) = take_socket_from_stdio() else {
        stand.note("log на fd 0 нет сокета окна");
        return helper::EXIT_CHANNEL;
    };
    let io: Arc<dyn RootIo> = Arc::new(StandIo(Arc::clone(stand)));
    let Some(session_id) = args.session.or_else(|| helper::session_of_user(io.as_ref(), pkexec_uid)) else {
        io.log("сеанс окна не найден у logind");
        return helper::EXIT_HANDSHAKE;
    };
    let Some(watch) = LogindWatch::new(
        helper::RootLoginctl(Arc::clone(&io)),
        StandGauge(Arc::clone(stand)),
        &session_id,
    ) else {
        return helper::EXIT_USAGE;
    };
    let poll_ms = std::env::var("REXON_STAND_POLL_MS").ok().and_then(|s| s.parse().ok()).unwrap_or(50);
    let cfg = SessionConfig {
        build_id: own_build(),
        pkexec_uid,
        parent_pid: parent,
        session_id,
        lang: args.lang,
        poll_interval: Duration::from_millis(poll_ms),
        idle_limit: revoke::IDLE_LIMIT,
    };
    let report = helper::run_session(sock, io, cfg, Box::new(watch), Arc::new(StandClock(Arc::clone(stand))));
    stand.note(&format!("end {:?} ops {}", report.end, report.ops_executed));
    match report.end {
        SessionEnd::Revoked(_) => helper::EXIT_OK,
        SessionEnd::PeerRejected => helper::EXIT_CHANNEL,
        SessionEnd::HandshakeFailed(_) | SessionEnd::SessionRejected => helper::EXIT_HANDSHAKE,
    }
}

fn main() {
    if std::env::args().nth(1).as_deref() == Some(client::BUILD_ID_FLAG) {
        println!("{}", own_build());
        return;
    }
    let Some(dir) = std::env::var_os(DIR_ENV).map(PathBuf::from) else {
        eprintln!("rexon-priv-stand: нет {DIR_ENV} — это бинарь стенда, не помощник");
        std::process::exit(helper::EXIT_USAGE);
    };
    let journal = match std::fs::OpenOptions::new().create(true).append(true).open(dir.join("journal")) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("rexon-priv-stand: журнал стенда: {e}");
            std::process::exit(helper::EXIT_USAGE);
        }
    };
    let stand = Arc::new(Stand { root: dir.join("root"), dir, journal: Mutex::new(journal) });
    let code = session(&stand);
    stand.note(&format!("exit {code}"));
    std::process::exit(code);
}
