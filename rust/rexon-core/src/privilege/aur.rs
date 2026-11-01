//! Шим `sudo` для сборщиков AUR (пункт Б10 плана, см. [`super`]).
//!
//! paru и yay собирают пакет от пользователя, а root им нужен только для
//! вызовов pacman: поставить зависимости из репозиториев, поставить
//! собранное, пометить причину установки, снять сборочные. Каждый такой
//! вызов они делают через свой «sudo» — программу из `--sudo` (оба
//! сборщика, см. `man paru`, `man yay`: «--sudo <command>», «--sudoflags
//! <flags> … passed to every instance where sudo is called»). Раньше это
//! был `pkexec` — отдельное окно пароля мимо сеанса прав на каждый вызов.
//!
//! Теперь в `--sudo` стоит `rexon-sudo-shim`, а в `--sudoflags` — метка
//! операции `--rexon-op=<pid окна>.<токен>`. Шим ничего не исполняет сам:
//! он передаёт строку pacman окну, и окно решает.
//!
//! # Пара «окно — шим»
//!
//! * **Где окно слушает.** `$XDG_RUNTIME_DIR/rexon-sudo-shim/<pid>.sock`,
//!   каталог 0700 пользователя (проверяется владелец и права, ссылка —
//!   отказ). Сокет и принятые соединения — с `CLOEXEC`: потомки окна их не
//!   наследуют.
//! * **Кто может спросить** ([`Broker::decide`]):
//!   1. `SO_PEERCRED`: uid — тот же, что у окна;
//!   2. токен — 128 бит из `getrandom`, выдан окном на ОДНУ операцию
//!      ([`Broker::register`]): имена пакетов, которые окно просило собрать,
//!      и согласие на замену конфликтующего;
//!   3. первый запрос ПРИВЯЗЫВАЕТ операцию к процессу-сборщику: родитель
//!      шима должен быть потомком окна и нести метку операции в своей
//!      командной строке (это ровно тот paru/yay, которого запустило окно).
//!      Запоминаются pid и время старта (защита от повторного pid);
//!   4. дальше — только потомки привязанного сборщика.
//! * **Конец операции.** Сборщик вышел (нет процесса с тем pid и временем
//!   старта) — операция закрыта навсегда, токен в списке использованных;
//!   любой следующий запрос с ним — отказ. Непривязанная операция живёт
//!   [`UNBOUND_TTL`], привязанная — не дольше [`BOUND_MAX`].
//!
//! # Что пропускается (всё прочее — отказ, [`parse_pacman`])
//!
//! | Строка сборщика | Что делает окно |
//! |---|---|
//! | `pacman -S … -- [repo/]имя…` | [`PrivOp::AurDeps`]: помощник ставит `--asdeps --needed`, только имена из sync-базы своей машины |
//! | `pacman -U … -- /путь/файл.pkg.tar…` | [`PrivOp::AurInstallBuilt`]: имя из имени файла — из списка операции (иначе отказ ДО пароля), файл открывает окно (`O_NOFOLLOW`) и отдаёт дескриптором; помощник сверяет `.PKGINFO` копии |
//! | `pacman -D --asdeps/--asexplicit -- имя…` | ничего от root: `-S` уже ставит зависимостью, `-U` — явно; имена обязаны быть из операции |
//! | `pacman -R[s][u] … -- имя…` | [`PrivOp::AurDropMake`]: только имена, которые эта операция ставила через `-S`; помощник снимает лишь сироты |
//!
//! Флаги вне белого списка (`--overwrite`, `--dbpath`, `--root`, `-d`,
//! чужой `--config`, `--ask` без согласия на замену…) — отказ.
//!
//! # Исполнение
//!
//! Через [`super::run`]: первый запрос поднимает сеанс прав (одно окно
//! пароля), остальные идут в тот же сеанс. Помощника сеанса нет вовсе
//! ([`LegacyReason::HelperUnavailable`]) — шим получает «прежний путь» и
//! сам запускает `pkexec` той же строки pacman, как было до Б10 (З5: при
//! отказе проверки прежнего пути нет). Без помощника на машине окно шим и не
//! ставит ([`sudo_for`]): `--sudo pkexec`, как раньше.
//!
//! Пересборка AeroShell в чистом окружении (`paru --chroot`) зовёт через
//! sudo `mkarchroot`/`arch-nspawn` — это не pacman, шим такое не пропустит,
//! поэтому она идёт прежним путём (`--sudo pkexec`).

use std::collections::{BTreeSet, HashMap, HashSet};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::op::{LocalFile, PrivOp};
use super::run::{LegacyReason, RunOutcome};
use crate::i18n::tr;

/// Имя бинаря шима.
pub const SHIM_NAME: &str = "rexon-sudo-shim";

/// Метка операции в `--sudoflags`: `--rexon-op=<pid окна>.<токен>`.
pub const OP_FLAG: &str = "--rexon-op=";

/// Подкаталог `$XDG_RUNTIME_DIR` с сокетами окон.
pub const SOCKET_DIR: &str = "rexon-sudo-shim";

/// Сколько живёт операция, к которой ещё не пришёл ни один запрос: команда
/// AUR может стоять в плане после долгой установки стола.
pub const UNBOUND_TTL: Duration = Duration::from_secs(12 * 3600);

/// Предел жизни привязанной операции (сборка крупного комплекта — часы).
pub const BOUND_MAX: Duration = Duration::from_secs(24 * 3600);

/// Сколько окно ждёт строку запроса от шима.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// Самый длинный запрос шима, байт.
const REQUEST_MAX: u64 = 256 * 1024;

/// Сколько операций окно держит разом (неиспользованные старые вытесняются).
const MAX_OPS: usize = 64;

/// Пути pacman, которые шим пропускает (имя — как его даёт сборщик).
const PACMAN_BINS: &[&str] = &["pacman", "/usr/bin/pacman"];

/// Единственный конфиг pacman, который может прийти в `--config`.
const PACMAN_CONF: &str = "/etc/pacman.conf";

// ============================================================================
// Протокол шим ↔ окно (строки JSON)
// ============================================================================

/// Запрос шима: токен операции и строка, которую сборщик отдал «sudo».
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShimRequest {
    pub token: String,
    pub argv: Vec<String>,
}

/// Ответ окна (поток строк; последняя — `Exit`, `Fallback` или `Refused`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ShimReply {
    /// Строка хода (вывод pacman у помощника).
    Line(String),
    /// Исполнено, код pacman.
    Exit(i32),
    /// Помощника сеанса нет: шим сам запускает `pkexec` той же строки.
    Fallback,
    /// Отказ: текст для человека.
    Refused(String),
}

// ============================================================================
// Разбор строки pacman
// ============================================================================

/// Что просит сборщик (строка pacman после белого списка флагов).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PacmanCall {
    /// `-S`: имена без `repo/`.
    Sync(Vec<String>),
    /// `-U`: абсолютные пути файлов.
    Upgrade(Vec<String>),
    /// `-D --asdeps` (`explicit = false`) или `--asexplicit`.
    Database { explicit: bool, names: Vec<String> },
    /// `-R` (с `-s`/`-u`/`-n`).
    Remove(Vec<String>),
}

/// Вид операции pacman.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PacOp {
    Sync,
    Upgrade,
    Database,
    Remove,
}

/// Разобрать строку pacman от сборщика. `replace` — человек согласился на
/// замену конфликтующего (`--ask=4`). `Err` — короткая причина для журнала.
///
/// Флаги — до `--`, цели — только после него (так строят и paru, и yay).
pub fn parse_pacman(argv: &[String], replace: bool) -> Result<PacmanCall, String> {
    let Some((bin, rest)) = argv.split_first() else {
        return Err("пустая команда".into());
    };
    if !PACMAN_BINS.contains(&bin.as_str()) {
        return Err(format!("не pacman: {bin}"));
    }
    let Some(sep) = rest.iter().position(|a| a == "--") else {
        return Err("нет `--` перед целями".into());
    };
    let (flags, targets) = (&rest[..sep], &rest[sep + 1..]);
    let mut op: Option<PacOp> = None;
    let (mut asdeps, mut asexplicit, mut remove_mods) = (false, false, false);
    let set_op = |o: PacOp, op: &mut Option<PacOp>| -> Result<(), String> {
        if op.is_some_and(|x| x != o) {
            return Err("две операции pacman".into());
        }
        *op = Some(o);
        Ok(())
    };
    let mut i = 0;
    while i < flags.len() {
        let f = flags[i].as_str();
        // Флаг со значением: `--x=v` или `--x v`.
        let mut value = |name: &str| -> Option<Result<String, String>> {
            if let Some(v) = f.strip_prefix(name).and_then(|r| r.strip_prefix('=')) {
                return Some(Ok(v.to_string()));
            }
            if f == name {
                i += 1;
                return Some(flags.get(i).cloned().ok_or_else(|| format!("{name} без значения")));
            }
            None
        };
        if let Some(v) = value("--ask") {
            let v = v?;
            if !(replace && v == "4") {
                return Err(format!("--ask {v} без согласия на замену"));
            }
        } else if let Some(v) = value("--config") {
            let v = v?;
            if v != PACMAN_CONF {
                return Err(format!("чужой --config {v}"));
            }
        } else if let Some(v) = value("--color") {
            let v = v?;
            if !matches!(v.as_str(), "auto" | "always" | "never") {
                return Err(format!("--color {v}"));
            }
        } else {
            match f {
                "-S" | "--sync" => set_op(PacOp::Sync, &mut op)?,
                "-U" | "--upgrade" => set_op(PacOp::Upgrade, &mut op)?,
                "-D" | "--database" => set_op(PacOp::Database, &mut op)?,
                "-R" | "--remove" => set_op(PacOp::Remove, &mut op)?,
                "--noconfirm" | "--needed" => {}
                "--asdeps" | "--asdep" => asdeps = true,
                "--asexplicit" | "--asexp" => asexplicit = true,
                "-s" | "--recursive" | "-u" | "--unneeded" | "-n" | "--nosave" => remove_mods = true,
                _ => return Err(format!("флаг вне списка: {f}")),
            }
        }
        i += 1;
    }
    let Some(op) = op else {
        return Err("нет операции pacman".into());
    };
    if remove_mods && op != PacOp::Remove {
        return Err("-s/-u/-n не у -R".into());
    }
    if targets.is_empty() {
        return Err("нет целей".into());
    }
    if targets.iter().any(|t| t.is_empty() || t.starts_with('-') || t.chars().any(char::is_control)) {
        return Err("цель похожа на флаг".into());
    }
    let names = |strip_repo: bool| -> Result<Vec<String>, String> {
        targets
            .iter()
            .map(|t| {
                let name = match t.split_once('/') {
                    Some((repo, n)) if strip_repo && is_repo_name(repo) => n,
                    _ => t.as_str(),
                };
                if crate::system_plan::is_safe_pkg_arg(crate::system::PkgFamily::Arch, name) {
                    Ok(name.to_string())
                } else {
                    Err(format!("не имя пакета: {t}"))
                }
            })
            .collect()
    };
    match op {
        PacOp::Sync => {
            if asexplicit {
                return Err("-S --asexplicit".into());
            }
            Ok(PacmanCall::Sync(names(true)?))
        }
        PacOp::Upgrade => {
            if asdeps || asexplicit {
                return Err("-U с причиной установки".into());
            }
            if let Some(t) = targets.iter().find(|t| super::validate::check_aur_package(t).is_err()) {
                return Err(format!("не файл makepkg: {t}"));
            }
            Ok(PacmanCall::Upgrade(targets.to_vec()))
        }
        PacOp::Database => {
            if asdeps == asexplicit {
                return Err("-D без одной причины установки".into());
            }
            Ok(PacmanCall::Database { explicit: asexplicit, names: names(false)? })
        }
        PacOp::Remove => {
            if asdeps || asexplicit {
                return Err("-R с причиной установки".into());
            }
            Ok(PacmanCall::Remove(names(false)?))
        }
    }
}

/// Имя репозитория в цели `repo/имя` (`core`, `extra`, `cachyos-v3`…).
fn is_repo_name(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
}

// ============================================================================
// Процессы (/proc)
// ============================================================================

/// Процесс с временем старта (pid может достаться другому).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Proc {
    pid: i32,
    start: u64,
}

/// `(ppid, время старта)` из `/proc/<pid>/stat`.
fn proc_stat(pid: i32) -> Option<(i32, u64)> {
    let text = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // Имя процесса в скобках может содержать пробелы и скобки — режем по
    // последней `)`. Дальше: состояние (3), ppid (4) … starttime (22).
    let rest = &text[text.rfind(')')? + 1..];
    let f: Vec<&str> = rest.split_whitespace().collect();
    Some((f.get(1)?.parse().ok()?, f.get(19)?.parse().ok()?))
}

fn proc_of(pid: i32) -> Option<Proc> {
    proc_stat(pid).map(|(_, start)| Proc { pid, start })
}

fn alive(p: Proc) -> bool {
    proc_of(p.pid) == Some(p)
}

/// `pid` — потомок `ancestor` (не сам он).
fn is_descendant(pid: i32, ancestor: i32) -> bool {
    let mut cur = pid;
    for _ in 0..256 {
        let Some((ppid, _)) = proc_stat(cur) else { return false };
        if ppid == ancestor {
            return true;
        }
        if ppid <= 1 {
            return false;
        }
        cur = ppid;
    }
    false
}

fn cmdline(pid: i32) -> Vec<String> {
    std::fs::read(format!("/proc/{pid}/cmdline"))
        .map(|b| {
            b.split(|&c| c == 0)
                .filter(|s| !s.is_empty())
                .map(|s| String::from_utf8_lossy(s).into_owned())
                .collect()
        })
        .unwrap_or_default()
}

// ============================================================================
// Операции и решение
// ============================================================================

/// Одна сборка AUR, как её задумало окно.
#[derive(Debug)]
struct AurOp {
    /// Что окно просило собрать (имя собранного — только отсюда).
    allowed: Vec<String>,
    /// Согласие на замену конфликтующего (`--ask=4`).
    replace: bool,
    /// Что эта операция ставила через `-S` (для `-D --asdeps` и `-R`).
    deps: BTreeSet<String>,
    created: Instant,
    /// Сборщик, к которому привязана операция.
    bound: Option<(Proc, Instant)>,
}

/// Решение по запросу шима.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Отказ, причина для журнала.
    Refuse(String),
    /// Ответить успехом без root (`-D`).
    NoOp,
    /// Исполнить через сеанс прав; `files` — пути для `-U` (в порядке слотов).
    Run { op: PrivOp, files: Vec<String> },
}

/// Исполнитель операции: в окне — [`super::run::run`].
pub type ExecFn = dyn Fn(&PrivOp, Vec<OwnedFd>, &mut dyn FnMut(&str)) -> RunOutcome + Send + Sync;

/// Посредник окна: операции, сокет, решения.
pub struct Broker {
    dir: PathBuf,
    window_pid: i32,
    ops: Mutex<HashMap<String, AurOp>>,
    /// Токены закончившихся операций: повтор — отказ с этой причиной.
    used: Mutex<HashSet<String>>,
    exec: Box<ExecFn>,
}

/// Метка операции для `--sudoflags`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registration {
    pub token: String,
    /// `--rexon-op=<pid>.<токен>`.
    pub flag: String,
}

fn random_token() -> String {
    let mut buf = [0u8; 16];
    let mut got = 0;
    while got < buf.len() {
        // SAFETY: пишем в свой буфер не больше его остатка.
        let n = unsafe { libc::getrandom(buf[got..].as_mut_ptr().cast(), buf.len() - got, 0) };
        if n > 0 {
            got += n as usize;
        } else if std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
            // getrandom есть с Linux 3.17; без него — /dev/urandom.
            let mut f = std::fs::File::open("/dev/urandom").expect("нет источника случайности");
            f.read_exact(&mut buf).expect("нет источника случайности");
            got = buf.len();
        }
    }
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

fn is_token(s: &str) -> bool {
    s.len() == 32 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

/// Подготовить каталог сокетов: создать 0700 или принять существующий,
/// только если он наш, 0700 и не ссылка.
fn prepare_dir(dir: &Path) -> std::io::Result<()> {
    match std::fs::DirBuilder::new().mode(0o700).create(dir) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e),
    }
    let m = std::fs::symlink_metadata(dir)?;
    // SAFETY: getuid не ошибается.
    let uid = unsafe { libc::getuid() };
    if !m.is_dir() || m.uid() != uid || m.permissions().mode() & 0o777 != 0o700 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!("{}: не каталог 0700 пользователя", dir.display()),
        ));
    }
    Ok(())
}

impl Broker {
    /// Посредник окна `window_pid` с сокетом в `dir` (каталог 0700). Поток
    /// приёма живёт, пока жив процесс.
    pub fn start(dir: &Path, window_pid: i32, exec: Box<ExecFn>) -> std::io::Result<Arc<Self>> {
        prepare_dir(dir)?;
        let path = dir.join(format!("{window_pid}.sock"));
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path)?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        let broker = Arc::new(Self {
            dir: dir.to_path_buf(),
            window_pid,
            ops: Mutex::new(HashMap::new()),
            used: Mutex::new(HashSet::new()),
            exec,
        });
        let b = Arc::clone(&broker);
        std::thread::Builder::new().name("rexon-sudo-shim".into()).spawn(move || {
            for conn in listener.incoming().flatten() {
                let b = Arc::clone(&b);
                let _ = std::thread::Builder::new().name("rexon-sudo-shim-req".into()).spawn(move || b.serve(conn));
            }
        })?;
        Ok(broker)
    }

    /// Путь сокета.
    pub fn socket_path(&self) -> PathBuf {
        self.dir.join(format!("{}.sock", self.window_pid))
    }

    /// Завести операцию: имена, которые окно просит собрать, и согласие на
    /// замену конфликтующего.
    pub fn register(&self, allowed: &[String], replace: bool) -> Registration {
        let token = random_token();
        let mut ops = self.lock_ops();
        self.sweep(&mut ops);
        if ops.len() >= MAX_OPS {
            // Вытесняем самую старую непривязанную (такие — неиспользованные планы).
            if let Some(old) = ops
                .iter()
                .filter(|(_, o)| o.bound.is_none())
                .min_by_key(|(_, o)| o.created)
                .map(|(t, _)| t.clone())
            {
                ops.remove(&old);
                self.lock_used().insert(old);
            }
        }
        ops.insert(
            token.clone(),
            AurOp { allowed: allowed.to_vec(), replace, deps: BTreeSet::new(), created: Instant::now(), bound: None },
        );
        Registration { flag: format!("{OP_FLAG}{}.{token}", self.window_pid), token }
    }

    fn lock_ops(&self) -> std::sync::MutexGuard<'_, HashMap<String, AurOp>> {
        self.ops.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn lock_used(&self) -> std::sync::MutexGuard<'_, HashSet<String>> {
        self.used.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Закрыть всё, что кончилось: сборщик вышел, срок истёк.
    fn sweep(&self, ops: &mut HashMap<String, AurOp>) {
        let over: Vec<String> = ops.iter().filter(|(_, o)| op_over(o)).map(|(t, _)| t.clone()).collect();
        let mut used = self.lock_used();
        for t in over {
            ops.remove(&t);
            used.insert(t);
        }
    }

    /// Решение по запросу шима от процесса `peer_pid` (uid уже сверен).
    pub fn decide(&self, token: &str, peer_pid: i32, argv: &[String]) -> Decision {
        let refuse = |why: &str| Decision::Refuse(why.to_string());
        if !is_token(token) {
            return refuse("токен не по форме");
        }
        let mut ops = self.lock_ops();
        self.sweep(&mut ops);
        if self.lock_used().contains(token) {
            return refuse("операция этого токена закончилась");
        }
        let Some(op) = ops.get_mut(token) else {
            return refuse("неизвестный токен");
        };
        match op.bound {
            Some((p, _)) => {
                if !is_descendant(peer_pid, p.pid) {
                    return refuse("запрос не от сборщика этой операции");
                }
            }
            None => {
                // Первый запрос: родитель шима — сборщик, запущенный окном
                // с меткой ЭТОЙ операции.
                let Some((parent, _)) = proc_stat(peer_pid) else {
                    return refuse("нет процесса шима");
                };
                let flag = format!("{OP_FLAG}{}.{token}", self.window_pid);
                if !is_descendant(parent, self.window_pid) || !cmdline(parent).iter().any(|a| *a == flag) {
                    return refuse("родитель шима — не сборщик этой операции");
                }
                let Some(p) = proc_of(parent) else {
                    return refuse("сборщик уже вышел");
                };
                op.bound = Some((p, Instant::now()));
            }
        }
        let call = match parse_pacman(argv, op.replace) {
            Ok(c) => c,
            Err(why) => return Decision::Refuse(why),
        };
        let in_op = |names: &[String], set: &dyn Fn(&str) -> bool| names.iter().find(|n| !set(n)).cloned();
        match call {
            PacmanCall::Sync(names) => Decision::Run { op: PrivOp::AurDeps { names }, files: Vec::new() },
            PacmanCall::Upgrade(files) => {
                // Чужое имя — отказ сразу, без подъёма сеанса.
                for f in &files {
                    let name = super::helper::pkgname_from_file(f).unwrap_or_default();
                    if !op.allowed.contains(&name) {
                        return Decision::Refuse(format!("{name}: окно не просило собирать этот пакет"));
                    }
                }
                let packages =
                    (0..files.len()).map(|i| LocalFile { slot: i as u8, label: files[i].clone() }).collect();
                if files.len() > u8::MAX as usize {
                    return refuse("слишком много файлов");
                }
                Decision::Run {
                    op: PrivOp::AurInstallBuilt { packages, allowed: op.allowed.clone(), replace: op.replace },
                    files,
                }
            }
            PacmanCall::Database { explicit, names } => {
                let miss = if explicit {
                    in_op(&names, &|n| op.allowed.iter().any(|a| a == n))
                } else {
                    in_op(&names, &|n| op.deps.contains(n) || op.allowed.iter().any(|a| a == n))
                };
                match miss {
                    Some(n) => Decision::Refuse(format!("{n}: не из этой операции")),
                    None => Decision::NoOp,
                }
            }
            PacmanCall::Remove(names) => match in_op(&names, &|n| op.deps.contains(n)) {
                Some(n) => Decision::Refuse(format!("{n}: эта операция его не ставила")),
                None => Decision::Run { op: PrivOp::AurDropMake { names }, files: Vec::new() },
            },
        }
    }

    /// После успешного `-S` — запомнить поставленные имена операции.
    fn note_deps(&self, token: &str, names: &[String]) {
        if let Some(op) = self.lock_ops().get_mut(token) {
            op.deps.extend(names.iter().cloned());
        }
    }

    /// Обслужить одно соединение шима.
    fn serve(&self, conn: UnixStream) {
        let mut out = match conn.try_clone() {
            Ok(c) => c,
            Err(_) => return,
        };
        let mut send = |r: &ShimReply| {
            if let Ok(mut line) = serde_json::to_string(r) {
                line.push('\n');
                let _ = out.write_all(line.as_bytes());
            }
        };
        // SAFETY: getuid не ошибается.
        let uid = unsafe { libc::getuid() };
        let peer = super::helper::peer_cred(conn.as_fd());
        let Some((_, peer_pid)) = peer.filter(|(u, p)| *u == uid && *p > 1) else {
            crate::log_warn!("privilege", "шим AUR: чужой собеседник {peer:?}");
            return;
        };
        let _ = conn.set_read_timeout(Some(REQUEST_TIMEOUT));
        let mut line = String::new();
        if BufReader::new((&conn).take(REQUEST_MAX)).read_line(&mut line).is_err() {
            return;
        }
        let Ok(req) = serde_json::from_str::<ShimRequest>(line.trim_end()) else {
            send(&ShimReply::Refused(refused_text()));
            return;
        };
        match self.decide(&req.token, peer_pid, &req.argv) {
            Decision::Refuse(why) => {
                crate::log_warn!("privilege", "шим AUR: отказ ({why}): {}", req.argv.join(" "));
                send(&ShimReply::Refused(format!("{} ({why})", refused_text())));
            }
            Decision::NoOp => send(&ShimReply::Exit(0)),
            Decision::Run { op, files } => {
                let mut fds = Vec::with_capacity(files.len());
                for f in &files {
                    match std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW).open(f) {
                        Ok(h) => fds.push(OwnedFd::from(h)),
                        Err(e) => {
                            send(&ShimReply::Refused(format!("{} ({f}: {e})", refused_text())));
                            return;
                        }
                    }
                }
                let outcome = (self.exec)(&op, fds, &mut |l: &str| send(&ShimReply::Line(l.to_string())));
                match outcome {
                    RunOutcome::Done(o) => {
                        if o.ok {
                            if let PrivOp::AurDeps { names } = &op {
                                self.note_deps(&req.token, names);
                            }
                        }
                        send(&ShimReply::Exit(o.code.unwrap_or(1)));
                    }
                    RunOutcome::Legacy(LegacyReason::HelperUnavailable(_)) => {
                        // Прежний путь — только для того, что окно уже
                        // проверило; `-S` запомним заранее (pkexec исполнит).
                        if let PrivOp::AurDeps { names } = &op {
                            self.note_deps(&req.token, names);
                        }
                        send(&ShimReply::Fallback);
                    }
                    RunOutcome::Legacy(LegacyReason::NotSupported(k)) => {
                        send(&ShimReply::Refused(format!("{} ({k})", refused_text())));
                    }
                    RunOutcome::Failed(e) => {
                        crate::log_warn!("privilege", "шим AUR: операция не исполнена: {e}");
                        send(&ShimReply::Refused(e.human_text()));
                    }
                }
            }
        }
    }
}

/// Операция кончилась: сборщик вышел или срок истёк.
fn op_over(o: &AurOp) -> bool {
    match o.bound {
        Some((p, since)) => !alive(p) || since.elapsed() > BOUND_MAX,
        None => o.created.elapsed() > UNBOUND_TTL,
    }
}

/// Отказ шима для человека (строка уходит в вывод сборщика и окно хода).
pub fn refused_text() -> String {
    // TRANSLATORS: An AUR package builder asked for administrator rights for something Rexon did not ask it to do.
    tr("Rexon refused this request from the AUR builder: it only installs what you chose to build")
}

// ============================================================================
// Окно: включение и метка для сборщика
// ============================================================================

static ENABLED: AtomicBool = AtomicBool::new(false);
static LIVE: OnceLock<Option<Arc<Broker>>> = OnceLock::new();

/// Включить шим в этом процессе (окно Rexon). Без вызова сборщики AUR
/// получают `--sudo pkexec`, как до Б10 (консольные инструменты, тесты).
pub fn enable() {
    ENABLED.store(true, Ordering::SeqCst);
}

/// Установленный шим: рядом с помощником сеанса ([`crate::system_plan::HELPER_DIRS`],
/// каталог SteamOS).
pub fn shim_path() -> Option<String> {
    crate::system_plan::HELPER_DIRS
        .iter()
        .copied()
        .chain([super::client::STEAMOS_HELPER_DIR])
        .map(|d| crate::system_plan::helper_path_in(d, SHIM_NAME))
        .find(|p| std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0))
}

fn live_broker() -> Option<&'static Arc<Broker>> {
    LIVE.get_or_init(|| {
        let runtime = std::env::var_os("XDG_RUNTIME_DIR")?;
        let dir = Path::new(&runtime).join(SOCKET_DIR);
        let exec: Box<ExecFn> = Box::new(|op, fds, progress| super::run::run(op, fds, progress));
        match Broker::start(&dir, std::process::id() as i32, exec) {
            Ok(b) => Some(b),
            Err(e) => {
                crate::log_warn!("privilege", "шим AUR не поднят: {e}");
                None
            }
        }
    })
    .as_ref()
}

/// «sudo» сборщика AUR и его флаги.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SudoArgs {
    pub program: String,
    pub flags: String,
}

/// `--sudo`/`--sudoflags` для сборки `packages`: шим и метка новой операции,
/// если шим включён ([`enable`]), установлен и помощник сеанса на месте;
/// иначе `None` — прежний `--sudo pkexec`.
pub fn sudo_for(packages: &[String], replace: bool) -> Option<SudoArgs> {
    if !ENABLED.load(Ordering::SeqCst) || packages.is_empty() {
        return None;
    }
    let shim = shim_path()?;
    use super::client::{helper_candidates, locate_helper, Located, RealFs};
    if !matches!(locate_helper(&RealFs, &helper_candidates()), Located::Ready(_)) {
        return None;
    }
    let reg = live_broker()?.register(packages, replace);
    Some(SudoArgs { program: shim, flags: reg.flag })
}

// ============================================================================
// Сам шим (бинарь `rexon-sudo-shim`)
// ============================================================================

/// Разобрать метку `--rexon-op=<pid>.<токен>`.
pub fn parse_op_flag(arg: &str) -> Option<(u32, String)> {
    let (pid, token) = arg.strip_prefix(OP_FLAG)?.split_once('.')?;
    let pid: u32 = pid.parse().ok().filter(|p| *p > 1)?;
    is_token(token).then(|| (pid, token.to_string()))
}

/// `main` шима: `rexon-sudo-shim --rexon-op=<pid>.<токен> pacman …`.
/// `-v` (продление sudo у yay с `sudoloop`) — успех без окна: ничего не
/// продлевается и не исполняется.
pub fn shim_main(args: &[String]) -> i32 {
    let say = |text: &str| eprintln!("{SHIM_NAME}: {text}");
    let rest = args.get(1..).unwrap_or_default();
    if matches!(rest, [v] if v == "-v" || v == "--validate") {
        return 0;
    }
    let Some((pid, token)) = rest.first().and_then(|a| parse_op_flag(a)) else {
        say(&refused_text());
        return 1;
    };
    let argv = rest[1..].to_vec();
    let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR") else {
        say("XDG_RUNTIME_DIR");
        return 1;
    };
    let path = Path::new(&runtime).join(SOCKET_DIR).join(format!("{pid}.sock"));
    let mut conn = match UnixStream::connect(&path) {
        Ok(c) => c,
        Err(e) => {
            say(&format!("{}: {e}", path.display()));
            return 1;
        }
    };
    let req = ShimRequest { token, argv: argv.clone() };
    let Ok(mut line) = serde_json::to_string(&req) else { return 1 };
    line.push('\n');
    if conn.write_all(line.as_bytes()).is_err() {
        return 1;
    }
    let stdout = std::io::stdout();
    for l in BufReader::new(conn).lines() {
        let Ok(l) = l else { break };
        match serde_json::from_str::<ShimReply>(&l) {
            Ok(ShimReply::Line(text)) => {
                let mut o = stdout.lock();
                let _ = writeln!(o, "{text}");
                let _ = o.flush();
            }
            Ok(ShimReply::Exit(code)) => return code,
            Ok(ShimReply::Refused(text)) => {
                say(&text);
                return 1;
            }
            Ok(ShimReply::Fallback) => {
                // Помощника сеанса нет — как до Б10: окно пароля на вызов.
                use std::os::unix::process::CommandExt;
                let err = std::process::Command::new(super::client::PKEXEC).args(&argv).exec();
                say(&err.to_string());
                return 127;
            }
            Err(_) => break,
        }
    }
    say(&tr("Rexon closed the connection before the change finished"));
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn paru_и_yay_разбираются() {
        // paru: repo_install, do_install, set_install_reason, removemake.
        assert_eq!(
            parse_pacman(&s(&["pacman", "--sync", "--needed", "--noconfirm", "--", "extra/cmake", "core/gcc"]), false),
            Ok(PacmanCall::Sync(s(&["cmake", "gcc"])))
        );
        assert_eq!(
            parse_pacman(&s(&["pacman", "--upgrade", "--ask=4", "--", "/h/.cache/paru/clone/t/t-1-1-any.pkg.tar.zst"]), true),
            Ok(PacmanCall::Upgrade(s(&["/h/.cache/paru/clone/t/t-1-1-any.pkg.tar.zst"])))
        );
        assert_eq!(
            parse_pacman(&s(&["pacman", "--database", "--noconfirm", "--asdeps", "--", "cmake"]), false),
            Ok(PacmanCall::Database { explicit: false, names: s(&["cmake"]) })
        );
        assert_eq!(
            parse_pacman(&s(&["pacman", "--remove", "--noconfirm", "--noconfirm", "--", "cmake"]), false),
            Ok(PacmanCall::Remove(s(&["cmake"])))
        );
        // yay: флаги в любом порядке, `--ask 4` отдельным словом, свой --config.
        assert_eq!(
            parse_pacman(
                &s(&["pacman", "-U", "--ask", "4", "--needed", "--noconfirm", "--config", "/etc/pacman.conf", "--", "/a/t-1-1-x86_64.pkg.tar.zst"]),
                true
            ),
            Ok(PacmanCall::Upgrade(s(&["/a/t-1-1-x86_64.pkg.tar.zst"])))
        );
        assert_eq!(
            parse_pacman(&s(&["pacman", "-R", "-s", "-u", "--noconfirm", "--config", "/etc/pacman.conf", "--", "cmake"]), false),
            Ok(PacmanCall::Remove(s(&["cmake"])))
        );
    }

    #[test]
    fn прочее_отказ() {
        for (argv, replace) in [
            (s(&["bash", "-c", "id"]), false), // сторож: имена, не запуск
            (s(&["pacman", "-S", "x"]), false),                      // цели без `--`
            (s(&["pacman", "-Syu", "--"]), false),                   // склейка, нет целей
            (s(&["pacman", "-S", "--", "x", "-y"]), false),          // флаг среди целей
            (s(&["pacman", "-S", "-U", "--", "x"]), false),          // две операции
            (s(&["pacman", "-U", "--overwrite", "*", "--", "/a/t-1-1-any.pkg.tar.zst"]), false),
            (s(&["pacman", "-U", "--dbpath", "/tmp", "--", "/a/t-1-1-any.pkg.tar.zst"]), false),
            (s(&["pacman", "-U", "--root=/tmp", "--", "/a/t-1-1-any.pkg.tar.zst"]), false),
            (s(&["pacman", "-U", "-d", "--", "/a/t-1-1-any.pkg.tar.zst"]), false),
            (s(&["pacman", "-U", "--ask=4", "--", "/a/t-1-1-any.pkg.tar.zst"]), false), // замена без согласия
            (s(&["pacman", "-U", "--ask=255", "--", "/a/t-1-1-any.pkg.tar.zst"]), true),
            (s(&["pacman", "-U", "--config=/tmp/p.conf", "--", "/a/t-1-1-any.pkg.tar.zst"]), false),
            (s(&["pacman", "-U", "--", "/tmp/x.deb"]), false),
            (s(&["pacman", "-U", "--", "t-1-1-any.pkg.tar.zst"]), false), // не абсолютный
            (s(&["pacman", "-U", "--", "/a/../etc/t-1-1-any.pkg.tar.zst"]), false),
            (s(&["pacman", "-D", "--", "x"]), false),                 // без причины
            (s(&["pacman", "-S", "--asexplicit", "--", "x"]), false),
            (s(&["pacman", "-S", "-s", "--", "x"]), false),
            (s(&["pacman", "-S", "--", "../x"]), false),
            (s(&["/tmp/pacman", "-S", "--", "x"]), false),
            (s(&["pacman", "-Q", "--", "x"]), false),
        ] {
            assert!(parse_pacman(&argv, replace).is_err(), "{argv:?}");
        }
    }

    #[test]
    fn метка_операции() {
        let t = "0123456789abcdef0123456789abcdef";
        assert_eq!(parse_op_flag(&format!("--rexon-op=42.{t}")), Some((42, t.to_string())));
        assert_eq!(parse_op_flag(&format!("--rexon-op=1.{t}")), None);
        assert_eq!(parse_op_flag("--rexon-op=42.ABC"), None);
        assert_eq!(parse_op_flag(&format!("--rexon-op=x.{t}")), None);
        assert!(is_token(&random_token()));
        assert_ne!(random_token(), random_token());
    }
}
