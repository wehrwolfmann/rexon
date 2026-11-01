//! Клиент сеанса прав (пункт Б7 плана, см. [`super`]): сторона окна.
//!
//! # Что делает
//!
//! 1. **Ищет помощника** [`HELPER_NAME`] среди [`helper_candidates`] и
//!    проверяет цепочку владения ([`check_ownership`]): файл и КАЖДЫЙ каталог
//!    пути принадлежат root, без записи для группы и всех, файл — обычный и
//!    исполняемый. Помощник, запущенный через `pkexec`, — это root для
//!    любого, кто успел подменить файл или каталог над ним; поэтому чужой
//!    владелец — сеанс не поднимается. Нет помощника или он недопустим — это
//!    единственные случаи, когда вызывающему можно идти путём «как сейчас»
//!    ([`ClientError::raw_fallback_allowed`], правило З5 в
//!    [`validate::raw_fallback_allowed`]).
//! 2. **Поднимает сеанс** одним `pkexec`: `socketpair` ([`proto::seqpacket_pair`],
//!    оба конца с `CLOEXEC`), конец помощника — на fd 0 и 1 дочки через
//!    [`Spawner`]; своя копия этого конца закрывается сразу после запуска.
//!    Ни один другой потомок окна сокета не наследует (З3): у оригиналов
//!    `CLOEXEC`, а на fd 0/1 сокет ставит только `dup2` внутри запускаемой
//!    дочки. Затем [`Hello`] → ждём `Welcome`. Пока человек вводит пароль,
//!    `Hello` лежит в сокете; закрыл окно пароля — `pkexec` выходит с 126,
//!    не авторизован — 127, и это [`ClientError::NotAuthorized`]: отказ
//!    человека, а не повод молча идти сырым путём.
//! 3. **Держит сеанс** в выделенном потоке «сеанс прав»: очередь запросов,
//!    по одной операции за раз (помощник берёт по одной), поток `Progress`
//!    вызывающему, [`Session::cancel`], [`Session::shutdown`] (закрытый сокет
//!    помощник понимает как «окно закрыто»: доводит идущее и выходит).
//!
//! # Договор с помощником (Б4)
//!
//! `pkexec <помощник> [--session <XDG_SESSION_ID>]`, сокет на fd 0 и 1.
//! Номер сеанса logind идёт аргументом, потому что `pkexec` чистит окружение,
//! а протокол ([`proto`]) после `Hello` ничего, кроме операций, не несёт.
//! Нет годного номера ([`revoke::valid_session_id`]) — флага нет вовсе.
//! Номер только СУЖАЕТ права помощника (по нему он следит за блокировкой и
//! сменой сеанса), расширить их им нельзя.
//!
//! # Ошибки — без сырого пути
//!
//! Обрыв, порченый кадр, нежданное сообщение — сеанс мёртв, исход идущей
//! операции неизвестен ([`ClientError::SessionLost`]). `BuildMismatch` —
//! «Rexon обновлён, перезапустите» (решение 28.10): клиент запоминает это и
//! больше сеанс не поднимает. `Refused` — отказ проверки. `Revoked` — право
//! отозвано; следующий запрос поднимет сеанс заново (новый пароль). Ни одна
//! из этих ошибок сырого `pkexec` не разрешает.

use std::collections::VecDeque;
use std::fmt;
use std::io;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, OwnedFd};
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{mpsc, Arc, Mutex, MutexGuard};
use std::thread;

use super::op::PrivOp;
use super::proto::{self, ClientMsg, ErrorCode, HelperMsg, Hello};
use super::revoke;
use super::validate::{self, HelperFailure};
use crate::system_plan as plan;
use crate::i18n::{tr, trf};

/// Имя бинаря помощника сеанса.
pub const HELPER_NAME: &str = "rexon-priv-helper";

/// Каталог помощников вне `/usr` (SteamOS, Steam-депо) — тот же, что у
/// [`crate::steamos_sysext::HELPER_PATH`]: `/var` переживает обновления.
pub const STEAMOS_HELPER_DIR: &str = "/var/lib/rexon/bin";

/// Чем поднимается помощник.
pub const PKEXEC: &str = "pkexec";

/// Флаг помощника перед номером сеанса logind (договор с Б4).
pub const SESSION_FLAG: &str = "--session";

/// Флаг языка сообщений помощника (договор с Б4).
pub const LANG_FLAG: &str = "--lang";

/// Код выхода `pkexec`: человек закрыл окно пароля.
pub const PKEXEC_DISMISSED: i32 = 126;

/// Код выхода `pkexec`: не авторизован (неверный пароль, нет агента polkit).
pub const PKEXEC_NOT_AUTHORIZED: i32 = 127;

/// Запись для группы и всех.
const GROUP_OTHER_WRITE: u32 = 0o022;

/// Все места помощника по убыванию предпочтения: пакет, ручная установка
/// ([`plan::HELPER_DIRS`]), затем каталог SteamOS.
pub fn helper_candidates() -> Vec<PathBuf> {
    plan::HELPER_DIRS
        .iter()
        .copied()
        .chain([STEAMOS_HELPER_DIR])
        .map(|dir| PathBuf::from(plan::helper_path_in(dir, HELPER_NAME)))
        .collect()
}

/// Аргумент языка помощника: `--lang=<код>` — та же форма, что у прежних
/// помощников ([`crate::system_l10n::lang_args`]). Английский (исходный),
/// код вне белого списка ([`crate::system_l10n::is_valid_code`]) или язык,
/// которого нет в каталоге помощников, — аргумента нет: помощник говорит
/// по-английски. Метку [`crate::system_l10n::LANG_MARKER`] не ищем: она
/// нужна только скриптам прежней версии, а `--lang` наш помощник понимал
/// с первой сборки (`helper::parse_args`).
pub fn lang_arg(lang: &str) -> Option<String> {
    if lang == crate::i18n::SOURCE_LANGUAGE || !crate::system_l10n::is_valid_code(lang) {
        return None;
    }
    if !crate::system_l10n::languages().contains(&lang) {
        return None;
    }
    Some(format!("{LANG_FLAG}={lang}"))
}

/// argv подъёма сеанса: `pkexec <помощник> [--session <id>] [--lang=<код>]`.
pub fn helper_argv(helper: &Path, session_id: Option<&str>, lang: Option<&str>) -> Vec<String> {
    let mut argv = vec![PKEXEC.to_string(), helper.to_string_lossy().into_owned()];
    if let Some(id) = session_id.filter(|id| revoke::valid_session_id(id)) {
        argv.push(SESSION_FLAG.to_string());
        argv.push(id.to_string());
    }
    argv.extend(lang.and_then(lang_arg));
    argv
}

// ============================================================================
// Проверка владения
// ============================================================================

/// Вид файла по `lstat` (ссылка не разворачивается).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Regular,
    Dir,
    Symlink,
    Other,
}

/// То, что проверке владения нужно знать о файле.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileMeta {
    pub kind: FileKind,
    pub uid: u32,
    /// Биты прав (`mode & 0o7777`).
    pub mode: u32,
}

/// Файловая система для поиска помощника (подменяется в тестах: создать
/// файл root без root нельзя).
pub trait HelperFs: Send {
    /// `lstat`: ссылка не разворачивается.
    fn lstat(&self, path: &Path) -> io::Result<FileMeta>;
    /// `realpath`.
    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf>;
}

/// Настоящая файловая система.
pub struct RealFs;

impl HelperFs for RealFs {
    fn lstat(&self, path: &Path) -> io::Result<FileMeta> {
        let meta = std::fs::symlink_metadata(path)?;
        let ft = meta.file_type();
        let kind = if ft.is_symlink() {
            FileKind::Symlink
        } else if ft.is_dir() {
            FileKind::Dir
        } else if ft.is_file() {
            FileKind::Regular
        } else {
            FileKind::Other
        };
        Ok(FileMeta {
            kind,
            uid: meta.uid(),
            mode: meta.mode() & 0o7777,
        })
    }

    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        std::fs::canonicalize(path)
    }
}

/// Почему помощник недопустим. Путь — то звено цепочки, что не прошло.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnerProblem {
    /// Путь не абсолютный или с `..`.
    NotAbsolute(PathBuf),
    /// Владелец не root.
    NotRoot(PathBuf),
    /// Запись для группы или всех.
    Writable(PathBuf),
    /// Сам помощник — не обычный файл (ссылка, каталог, устройство).
    NotRegular(PathBuf),
    /// Звено пути — не каталог и не ссылка.
    NotDirectory(PathBuf),
    /// У помощника нет бита исполнения.
    NotExecutable(PathBuf),
    /// Звено не прочесть (кроме «нет файла» у самого помощника).
    Unreadable(PathBuf, String),
}

impl fmt::Display for OwnerProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAbsolute(p) => write!(f, "путь не абсолютный: {}", p.display()),
            Self::NotRoot(p) => write!(f, "владелец не root: {}", p.display()),
            Self::Writable(p) => write!(f, "запись для группы или всех: {}", p.display()),
            Self::NotRegular(p) => write!(f, "не обычный файл: {}", p.display()),
            Self::NotDirectory(p) => write!(f, "не каталог: {}", p.display()),
            Self::NotExecutable(p) => write!(f, "не исполняемый: {}", p.display()),
            Self::Unreadable(p, e) => write!(f, "{}: {e}", p.display()),
        }
    }
}

/// Цепочка владения помощника: сам путь и его канонический вид (если путь
/// идёт через ссылку). У каждого звена владелец root; каталоги — без записи
/// для группы и всех; ссылка в середине допустима (её каталог проверен
/// предыдущим звеном, цель — каноническим путём); сам помощник — обычный
/// исполняемый файл без записи для группы и всех.
pub fn check_ownership(fs: &dyn HelperFs, path: &Path) -> Result<(), OwnerProblem> {
    check_chain(fs, path)?;
    let canon = fs
        .canonicalize(path)
        .map_err(|e| OwnerProblem::Unreadable(path.to_path_buf(), e.to_string()))?;
    if canon != path {
        check_chain(fs, &canon)?;
    }
    Ok(())
}

fn check_chain(fs: &dyn HelperFs, path: &Path) -> Result<(), OwnerProblem> {
    if !path.is_absolute() {
        return Err(OwnerProblem::NotAbsolute(path.to_path_buf()));
    }
    let comps: Vec<Component<'_>> = path
        .components()
        .filter(|c| !matches!(c, Component::CurDir))
        .collect();
    let mut prefix = PathBuf::new();
    for (i, comp) in comps.iter().enumerate() {
        match comp {
            Component::RootDir => prefix.push("/"),
            Component::Normal(name) => prefix.push(name),
            _ => return Err(OwnerProblem::NotAbsolute(path.to_path_buf())),
        }
        let meta = fs
            .lstat(&prefix)
            .map_err(|e| OwnerProblem::Unreadable(prefix.clone(), e.to_string()))?;
        if meta.uid != 0 {
            return Err(OwnerProblem::NotRoot(prefix));
        }
        let last = i + 1 == comps.len();
        if last {
            if meta.kind != FileKind::Regular {
                return Err(OwnerProblem::NotRegular(prefix));
            }
            if meta.mode & GROUP_OTHER_WRITE != 0 {
                return Err(OwnerProblem::Writable(prefix));
            }
            if meta.mode & 0o111 == 0 {
                return Err(OwnerProblem::NotExecutable(prefix));
            }
        } else {
            match meta.kind {
                FileKind::Symlink => {}
                FileKind::Dir => {
                    if meta.mode & GROUP_OTHER_WRITE != 0 {
                        return Err(OwnerProblem::Writable(prefix));
                    }
                }
                _ => return Err(OwnerProblem::NotDirectory(prefix)),
            }
        }
    }
    Ok(())
}

/// Итог поиска помощника.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Located {
    /// Найден и прошёл проверку владения.
    Ready(PathBuf),
    /// Ни одного кандидата нет.
    Missing,
    /// Первый найденный кандидат не прошёл проверку. Следующие не
    /// смотрим: подменённый файл на предпочтительном месте — уже повод
    /// не поднимать сеанс.
    BadOwner(OwnerProblem),
}

/// Найти помощника: первый существующий кандидат решает.
pub fn locate_helper(fs: &dyn HelperFs, candidates: &[PathBuf]) -> Located {
    for path in candidates {
        match fs.lstat(path) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => return Located::BadOwner(OwnerProblem::Unreadable(path.clone(), e.to_string())),
            Ok(_) => {
                return match check_ownership(fs, path) {
                    Ok(()) => Located::Ready(path.clone()),
                    Err(problem) => Located::BadOwner(problem),
                }
            }
        }
    }
    Located::Missing
}

// ============================================================================
// Запуск помощника
// ============================================================================

/// Запущенный помощник (процесс `pkexec`).
pub trait HelperChild: Send {
    /// Дождаться выхода. `Ok(None)` — убит сигналом.
    fn wait(&mut self) -> io::Result<Option<i32>>;
}

/// Как запускается помощник. Договор: `helper_end` встаёт на fd 0 и 1
/// дочки, своя копия закрывается не позже возврата из `spawn`, другим
/// потомкам конец не достаётся.
pub trait Spawner: Send {
    fn spawn(&mut self, argv: &[String], helper_end: OwnedFd) -> io::Result<Box<dyn HelperChild>>;

    /// Отпечаток сборки помощника — запуском `<helper> --build-id` от
    /// пользователя, без `pkexec`. `None` — не узнали (старый помощник без
    /// флага, сбой, тайм-аут): тогда сверка — как прежде, в рукопожатии.
    fn probe_build_id(&mut self, _helper: &Path) -> Option<String> {
        None
    }
}

/// Флаг помощника «напечатай отпечаток сборки и выйди» (без root).
pub const BUILD_ID_FLAG: &str = "--build-id";

/// Сколько ждать ответа на [`BUILD_ID_FLAG`].
const BUILD_ID_PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Живой запуск через [`Command`]: `argv[0]` — программа.
///
/// Конец помощника и его копия (`try_clone` = `F_DUPFD_CLOEXEC`) уходят в
/// `Stdio`; `dup2` на fd 0/1 делает сама дочка между `fork` и `exec`, у
/// копий в окне `CLOEXEC` остаётся. `Command` с этими `Stdio` уничтожается
/// сразу после запуска — копии окна закрыты.
pub struct CommandSpawner;

struct ProcessChild(std::process::Child);

impl HelperChild for ProcessChild {
    fn wait(&mut self) -> io::Result<Option<i32>> {
        self.0.wait().map(|st| st.code())
    }
}

impl Spawner for CommandSpawner {
    fn spawn(&mut self, argv: &[String], helper_end: OwnedFd) -> io::Result<Box<dyn HelperChild>> {
        let (program, args) = argv
            .split_first()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "пустой argv"))?;
        let out = helper_end.try_clone()?;
        let mut cmd = Command::new(program);
        cmd.args(args)
            .stdin(Stdio::from(helper_end))
            .stdout(Stdio::from(out))
            .stderr(Stdio::inherit());
        let child = cmd.spawn();
        drop(cmd);
        Ok(Box::new(ProcessChild(child?)))
    }

    fn probe_build_id(&mut self, helper: &Path) -> Option<String> {
        use std::io::Read;
        // Файл уже прошёл проверку владения: это программа root, и запуск её
        // от пользователя ничего не открывает. Окружение — пустое.
        let mut child = Command::new(helper)
            .arg(BUILD_ID_FLAG)
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let mut out = child.stdout.take()?;
        let (tx, rx) = mpsc::channel();
        let _ = thread::Builder::new().name("rexon-priv-probe".into()).spawn(move || {
            let mut text = String::new();
            let _ = out.by_ref().take(4096).read_to_string(&mut text);
            let _ = tx.send(text);
        });
        match rx.recv_timeout(BUILD_ID_PROBE_TIMEOUT) {
            Ok(text) => {
                let ok = child.wait().ok().is_some_and(|st| st.success());
                let id = text.trim();
                (ok && !id.is_empty() && !id.contains('\n')).then(|| id.to_string())
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                None
            }
        }
    }
}

/// Дождаться процесса в фоне: окно не ждёт помощника, который доводит
/// операцию после закрытия сокета.
fn reap_in_background(mut child: Box<dyn HelperChild>) {
    let _ = thread::Builder::new()
        .name("rexon-priv-reap".into())
        .spawn(move || {
            let _ = child.wait();
        });
}

// ============================================================================
// Итоги и ошибки
// ============================================================================

/// Операция исполнена помощником ([`HelperMsg::Done`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// Программа отработала успешно.
    pub ok: bool,
    /// Код выхода программы (`None` — убита сигналом).
    pub code: Option<i32>,
    /// Ключ перевода пояснения, если помощник его знает.
    pub message_key: Option<String>,
}

/// Почему операция не исполнена или исход неизвестен.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientError {
    /// Помощника нет — вызывающему можно путём «как сейчас» (З5).
    HelperMissing,
    /// Помощник недопустим (владелец, права) — путь «как сейчас» тоже
    /// разрешён: сеанс с подменённым файлом не поднимается вовсе.
    HelperBadOwner(OwnerProblem),
    /// Человек закрыл окно пароля (126) или не авторизован (127).
    NotAuthorized { code: i32 },
    /// Помощник от другой сборки: Rexon обновлён, нужен перезапуск окна.
    /// `helper_build` — отпечаток помощника, если он его назвал.
    BuildMismatch { helper_build: Option<String> },
    /// Помощник ДРУГОЙ сборки узнан ДО `pkexec` (окно из дерева разработки
    /// при пакете в системе; пакет обновили при открытом окне). Пароль не
    /// спрашивался, сеанс не поднимался — для правила З5 это то же, что
    /// «помощника нет»: путь «как сейчас» разрешён.
    HelperOtherBuild { helper_build: String },
    /// Помощник отказал по проверке аргументов.
    Refused { message_key: String },
    /// Сбой самого помощника; операция не исполнялась.
    HelperFailed { message_key: String },
    /// Операция отменена до касания системы.
    Cancelled,
    /// Право отозвано (простой, блокировка, сон…); следующий запрос
    /// поднимет сеанс заново, с новым паролем.
    Revoked { message_key: String },
    /// Сеанс оборвался. `outcome_unknown` — операция уже ушла помощнику, и
    /// исполнена ли она, неизвестно.
    SessionLost { outcome_unknown: bool, detail: String },
    /// Сеанс закрыло окно ([`Session::shutdown`]); операция не уходила.
    Closed,
    /// Помощник занят другой операцией (при очереди клиента не бывает).
    Busy,
    /// Не запустился `pkexec` или не создан сокет.
    Spawn(String),
    /// Запрос негоден (число дескрипторов, слишком большой кадр); помощнику
    /// не уходил.
    InvalidRequest(String),
}

impl ClientError {
    /// Чем ошибка является для правила З5.
    pub fn helper_failure(&self) -> Option<HelperFailure> {
        Some(match self {
            Self::HelperMissing | Self::HelperOtherBuild { .. } => HelperFailure::Missing,
            Self::HelperBadOwner(_) => HelperFailure::BadOwner,
            Self::NotAuthorized { .. } => HelperFailure::NotAuthorized,
            Self::Refused { .. } => HelperFailure::Refused,
            Self::BuildMismatch { .. }
            | Self::HelperFailed { .. }
            | Self::Revoked { .. }
            | Self::SessionLost { .. }
            | Self::Closed
            | Self::Busy
            | Self::Spawn(_) => HelperFailure::SessionLost,
            Self::Cancelled | Self::InvalidRequest(_) => return None,
        })
    }

    /// З5: можно ли вызывающему исполнить ту же операцию путём «как сейчас»
    /// (`PrivOp::pkexec_argv`). Только когда помощника нет или он недопустим.
    pub fn raw_fallback_allowed(&self) -> bool {
        self.helper_failure()
            .is_some_and(validate::raw_fallback_allowed)
    }
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HelperMissing => write!(f, "помощника сеанса прав нет"),
            Self::HelperBadOwner(p) => write!(f, "помощник сеанса прав недопустим: {p}"),
            Self::HelperOtherBuild { helper_build } => {
                write!(f, "помощник сеанса прав другой сборки ({helper_build}) — прежним путём")
            }
            Self::NotAuthorized { code } => write!(f, "пароль не подтверждён (pkexec {code})"),
            Self::BuildMismatch { helper_build: Some(b) } => {
                write!(f, "помощник другой сборки ({b}): Rexon обновлён, нужен перезапуск")
            }
            Self::BuildMismatch { helper_build: None } => {
                write!(f, "помощник другой сборки: Rexon обновлён, нужен перезапуск")
            }
            Self::Refused { message_key } => write!(f, "помощник отказал: {message_key}"),
            Self::HelperFailed { message_key } => write!(f, "сбой помощника: {message_key}"),
            Self::Cancelled => write!(f, "операция отменена"),
            Self::Revoked { message_key } => write!(f, "право отозвано: {message_key}"),
            Self::SessionLost { outcome_unknown: true, detail } => {
                write!(f, "сеанс прав оборвался, исход операции неизвестен: {detail}")
            }
            Self::SessionLost { outcome_unknown: false, detail } => {
                write!(f, "сеанс прав оборвался, операция не исполнялась: {detail}")
            }
            Self::Closed => write!(f, "сеанс прав закрыт окном"),
            Self::Busy => write!(f, "помощник занят другой операцией"),
            Self::Spawn(e) => write!(f, "не запустить помощника: {e}"),
            Self::InvalidRequest(e) => write!(f, "негодный запрос: {e}"),
        }
    }
}

impl std::error::Error for ClientError {}

impl ClientError {
    /// Текст человеку на языке окна (Б13): что случилось и что делать,
    /// без служебных слов. `Display` — строка для журнала.
    pub fn human_text(&self) -> String {
        match self {
            Self::HelperMissing => {
                // TRANSLATORS: Rexon can't change the system: one of its own files is missing.
                tr("Part of Rexon is missing, so it can't change the system. Reinstall Rexon.")
            }
            Self::HelperBadOwner(_) => {
                // TRANSLATORS: Rexon refused to use its own system file because someone else could have changed it.
                tr("Part of Rexon looks changed by someone else, so Rexon didn't use it. Reinstall Rexon.")
            }
            Self::HelperOtherBuild { .. } | Self::BuildMismatch { .. } => updated_text(),
            Self::NotAuthorized { .. } => {
                // TRANSLATORS: The password window was closed or the password was wrong.
                tr("Password not entered — no changes were made.")
            }
            Self::Refused { message_key } => key_text(message_key),
            Self::HelperFailed { .. } => {
                tr("Rexon couldn't start the change — nothing was changed. Try again.")
            }
            Self::Cancelled => tr("Cancelled — nothing was changed."),
            Self::Revoked { message_key } => key_text(message_key),
            Self::SessionLost { outcome_unknown: true, .. } => {
                tr("Rexon lost track of the change halfway. Check whether it was applied.")
            }
            Self::SessionLost { outcome_unknown: false, .. } => {
                tr("Rexon lost permission to change the system before starting — nothing was changed. Try again.")
            }
            Self::Closed => {
                // TRANSLATORS: The person pressed "Revoke now" (or closed the window) while a change was waiting.
                tr("Permission to change the system was turned off — nothing was changed.")
            }
            Self::Busy => tr("Rexon is already making another change. Wait for it to finish and try again."),
            Self::Spawn(_) => tr("Rexon couldn't ask for the password — nothing was changed."),
            Self::InvalidRequest(_) => tr("Rexon couldn't make this change — nothing was changed."),
        }
    }
}

/// «Rexon обновлён — перезапустите»: помощник другой сборки. Та же мысль,
/// что у уведомления о самообновлении (`single_instance::self_update_*`).
fn updated_text() -> String {
    // TRANSLATORS: Rexon was updated while its window was open; a system change needs a restart first.
    tr("Rexon was updated — restart it to continue.")
}

/// Текст человеку по ключу ответа помощника: `refused.<код>` (отказ
/// проверки), `revoked.<код>` (право отозвано), прочие ключи (сбой,
/// порядок кадров). Незнакомый ключ — общий отказ, сам ключ человеку не
/// показывается.
pub fn key_text(key: &str) -> String {
    if let Some(code) = key.strip_prefix("revoked.") {
        let reason = revoke::Reason::from_code(code).unwrap_or(revoke::Reason::SessionLost);
        // TRANSLATORS: %1 is why permission to change the system ended (a whole sentence).
        return trf("%1 Nothing was changed.", &[&reason.message()]);
    }
    let Some(code) = key.strip_prefix("refused.") else {
        return tr("Rexon couldn't make this change — nothing was changed.");
    };
    // Сирота, на которой держится сеанс: текст — из таблицы ядра по имени.
    if let Some(pkg) = code.strip_prefix("orphan-in-use.") {
        if let Some(s) = crate::update_guard::orphan_session_of(pkg) {
            return crate::update_guard::orphan_in_use_text(s);
        }
    }
    if let Some(pkg) = code.strip_prefix("orphan-login-default.") {
        if let Some(s) = crate::update_guard::orphan_session_of(pkg) {
            return crate::update_guard::orphan_login_default_text(s);
        }
    }
    // Сеанс по умолчанию у экрана входа: текст — из таблицы ядра по имени
    // X11-сеанса.
    let successor = |x11: &str| crate::system_plan::X11_SESSION_SUCCESSORS.iter().find(|s| s.x11 == x11);
    if let Some(s) = key.strip_prefix(crate::system_plan::WAYLAND_SESSION_MISSING).and_then(successor) {
        return crate::system_plan::wayland_session_missing_text(s);
    }
    if let Some(s) = key.strip_prefix(crate::system_plan::SESSION_SET_ELSEWHERE).and_then(successor) {
        return crate::system_plan::session_set_elsewhere_text(s);
    }
    match code {
        "vital" => tr("Rexon refused: this package is needed by the system."),
        "current-desktop" | "current-desktop-package" => {
            tr("Rexon refused: you are using this desktop right now. Sign in to another desktop first.")
        }
        "current-desktop-unknown" => {
            tr("Rexon refused: it couldn't tell which desktop you are using, so it didn't remove anything.")
        }
        "conflict" => tr("Rexon refused: this would remove other programs you have."),
        "read-only-system" => tr("Rexon refused: this system doesn't allow installing programs this way."),
        // TRANSLATORS: "SteamOS" is a product name.
        "not-steamos" => tr("Rexon refused: this works only on SteamOS."),
        // TRANSLATORS: Rexon was asked to remove a package that no longer gets updates, but the check showed otherwise.
        "not-orphan" => tr("Rexon didn't remove the package: it is still available for updates or isn't installed."),
        "orphan-needed" => tr(
            // TRANSLATORS: Rexon was asked to remove a package that no longer gets updates.
            "Rexon didn't remove the package: other installed programs may still need it, or the system is busy \
             installing something. Nothing was changed.",
        ),
        "not-rollback" => tr(
            // TRANSLATORS: Rexon was asked to put the previous versions of packages back, but the check showed otherwise.
            "Rexon didn't put the previous versions back: the packages on this computer have changed since the \
             check. Nothing was changed.",
        ),
        "rollback-extra" => tr(
            // TRANSLATORS: Rexon was asked to put the previous versions of packages back.
            "Rexon didn't put the previous versions back: they need other packages to be installed or changed too, \
             and Rexon doesn't do that by itself. Nothing was changed.",
        ),
        "rollback-signature" => tr(
            // TRANSLATORS: Rexon was asked to put the previous versions of packages back from files saved on this computer.
            "Rexon didn't put the previous versions back: the signature of a saved package doesn't match it, so the \
             file may have been tampered with. Nothing was changed.",
        ),
        "display-manager" => tr("Rexon refused: it can't set up this login screen."),
        // TRANSLATORS: Rexon was asked to install a font for all users.
        "not-a-font" | "font-file" => tr("Rexon refused: this file isn't a font it can install or remove for all users."),
        "font-exists" => tr(
            // TRANSLATORS: Rexon was asked to install a font for all users.
            "A different font file with this name is already installed for all users. Rexon didn't replace it — \
             nothing was changed.",
        ),
        "no-fontconfig" => tr(
            // TRANSLATORS: "fontconfig" is the name of the system's font library; keep it as is.
            "Rexon couldn't check the font: fontconfig isn't installed on this system. Nothing was changed.",
        ),
        // TRANSLATORS: "sddm" is the name of a system account; keep it as is.
        "no-sddm-user" => tr(
            "Rexon couldn't find the login screen's system account “sddm”. Check that SDDM is installed \
             properly. Nothing was changed.",
        ),
        "sddm-theme" => tr(
            "This login screen theme isn't installed in the system folder or doesn't let you change its \
             background. Nothing was changed.",
        ),
        "login-image" => tr(
            "The login screen can't show this file: it isn't a PNG or JPEG picture, or a WebP picture where \
             Qt's WebP image module is installed. Nothing was changed.",
        ),
        "look-file" | "look-value" => {
            tr("Rexon refused: these desktop settings can't be moved to the login screen. Nothing was changed.")
        }
        "look-elsewhere" => tr(
            "This login screen setting is fixed in a file Rexon doesn't change, so the login screen \
             wouldn't show it. Nothing was changed.",
        ),
        "no-setpriv" => tr(
            // TRANSLATORS: "setpriv" and "util-linux" are program and package names; keep them as is.
            "Rexon couldn't move the settings: the setpriv program (package util-linux) is missing. \
             Nothing was changed.",
        ),
        "family" => tr("Rexon refused: this action isn't meant for your system."),
        "not-in-table" | "unknown-desktop" | "set-rule" | "build-dep" | "package-name" => {
            tr("Rexon refused: it doesn't know this package, so nothing was installed.")
        }
        "package-file" | "pkginfo" => tr("Rexon refused: the package file is damaged or isn't the expected one."),
        "not-regular" | "too-large" | "too-many-entries" | "too-deep" | "link" | "special" | "unsupported"
        | "dot-dot" | "absolute" | "bad-name" | "set-id" | "duplicate" | "escape" | "malformed" | "theme"
        | "path" | "outside-root" | "item-name" | "boot-splash-code" | "not-root-owned" => {
            tr("Rexon refused: the file looks damaged or unsafe, so it wasn't installed.")
        }
        _ => tr("Rexon refused to make this change — nothing was changed."),
    }
}

// ============================================================================
// Сеанс: поток «сеанс прав»
// ============================================================================

/// Отчего сеанс кончился.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Death {
    Closed,
    Revoked(String),
    BuildMismatch(Option<String>),
    Lost(String),
}

impl Death {
    /// Ошибка операции, которая помощнику не уходила.
    fn queued_error(&self) -> ClientError {
        match self {
            Self::Closed => ClientError::Closed,
            Self::Revoked(key) => ClientError::Revoked { message_key: key.clone() },
            Self::BuildMismatch(b) => ClientError::BuildMismatch { helper_build: b.clone() },
            Self::Lost(d) => ClientError::SessionLost {
                outcome_unknown: false,
                detail: d.clone(),
            },
        }
    }

    /// Ошибка операции, что была у помощника, когда сеанс кончился.
    fn in_flight_error(&self) -> ClientError {
        let detail = match self {
            Self::Closed => "окно закрыло сеанс".to_string(),
            Self::Revoked(key) => format!("право отозвано ({key})"),
            Self::BuildMismatch(_) => "помощник другой сборки".to_string(),
            Self::Lost(d) => d.clone(),
        };
        ClientError::SessionLost {
            outcome_unknown: true,
            detail,
        }
    }
}

enum JobEvent {
    Progress(String),
    Finished(Result<Outcome, ClientError>),
}

struct Job {
    id: u64,
    frame: Vec<u8>,
    fds: Vec<OwnedFd>,
    reply: mpsc::Sender<JobEvent>,
}

struct InFlight {
    id: u64,
    reply: mpsc::Sender<JobEvent>,
}

impl InFlight {
    fn finish(self, result: Result<Outcome, ClientError>) {
        let _ = self.reply.send(JobEvent::Finished(result));
    }
}

#[derive(Default)]
struct State {
    queue: VecDeque<Job>,
    current: Option<InFlight>,
    dead: Option<Death>,
    closing: bool,
}

struct Shared {
    sock: OwnedFd,
    /// `eventfd`: будит поток при новом запросе и закрытии.
    wake: OwnedFd,
    state: Mutex<State>,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn wake(&self) {
        let one: u64 = 1;
        // SAFETY: пишем 8 байт из живой переменной в свой eventfd.
        unsafe {
            libc::write(self.wake.as_raw_fd(), (&one as *const u64).cast(), 8);
        }
    }

    fn drain_wake(&self) {
        let mut buf: u64 = 0;
        // SAFETY: читаем 8 байт в живую переменную; eventfd неблокирующий.
        unsafe {
            libc::read(self.wake.as_raw_fd(), (&mut buf as *mut u64).cast(), 8);
        }
    }

    fn shutdown_socket(&self) {
        // SAFETY: shutdown своего живого сокета.
        unsafe {
            libc::shutdown(self.sock.as_raw_fd(), libc::SHUT_RDWR);
        }
    }
}

fn new_eventfd() -> io::Result<OwnedFd> {
    // SAFETY: eventfd без указателей.
    let fd = unsafe { libc::eventfd(0, libc::EFD_CLOEXEC | libc::EFD_NONBLOCK) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: новый дескриптор, владеем им только мы.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

/// Живой сеанс прав. Клон — ещё одна ручка того же сеанса (для отмены из
/// другого потока); закрывает сеанс [`Session::shutdown`] или
/// уничтожение [`Client`].
#[derive(Clone)]
pub struct Session {
    shared: Arc<Shared>,
}

impl Session {
    fn start(sock: OwnedFd, wake: OwnedFd, child: Box<dyn HelperChild>) -> io::Result<Self> {
        let shared = Arc::new(Shared {
            sock,
            wake,
            state: Mutex::new(State::default()),
        });
        let worker = Arc::clone(&shared);
        let mut child = child;
        thread::Builder::new()
            .name("rexon-priv-session".into())
            .spawn(move || {
                let death = session_loop(&worker);
                die(&worker, death);
                let _ = child.wait();
            })?;
        Ok(Self { shared })
    }

    /// Сеанс жив: не закрыт окном и не кончился.
    pub fn is_alive(&self) -> bool {
        let st = self.shared.lock();
        st.dead.is_none() && !st.closing
    }

    fn death(&self) -> Option<Death> {
        self.shared.lock().dead.clone()
    }

    fn submit(&self, id: u64, frame: Vec<u8>, fds: Vec<OwnedFd>) -> Result<Pending, ClientError> {
        let (tx, rx) = mpsc::channel();
        {
            let mut st = self.shared.lock();
            if let Some(death) = &st.dead {
                return Err(death.queued_error());
            }
            if st.closing {
                return Err(ClientError::Closed);
            }
            st.queue.push_back(Job {
                id,
                frame,
                fds,
                reply: tx,
            });
        }
        self.shared.wake();
        Ok(Pending { id, rx })
    }

    /// Отменить операцию `id`. Ещё в очереди — снимается сразу (её
    /// ожидающий получит [`ClientError::Cancelled`]); уже у помощника —
    /// уходит `Cancel`, а ответит помощник (`Cancelled`, если не успел
    /// коснуться системы, иначе обычный итог). `false` — такой операции нет.
    pub fn cancel(&self, id: u64) -> bool {
        let mut st = self.shared.lock();
        if let Some(pos) = st.queue.iter().position(|j| j.id == id) {
            if let Some(job) = st.queue.remove(pos) {
                let _ = job.reply.send(JobEvent::Finished(Err(ClientError::Cancelled)));
            }
            return true;
        }
        if st.current.as_ref().is_some_and(|c| c.id == id) {
            return proto::send_msg(self.shared.sock.as_fd(), &ClientMsg::<PrivOp>::Cancel { id })
                .is_ok();
        }
        false
    }

    /// Закрыть сеанс: сокет закрывается, помощник видит «окно закрыто»,
    /// доводит идущую операцию и выходит. Не ждёт.
    pub fn shutdown(&self) {
        self.shared.lock().closing = true;
        self.shared.shutdown_socket();
        self.shared.wake();
    }
}

/// Отправленный запрос.
pub struct Pending {
    id: u64,
    rx: mpsc::Receiver<JobEvent>,
}

impl Pending {
    /// Номер операции (для [`Session::cancel`]).
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Дождаться итога, отдавая строки хода в `on_progress`.
    pub fn wait(self, on_progress: &mut dyn FnMut(&str)) -> Result<Outcome, ClientError> {
        loop {
            match self.rx.recv() {
                Ok(JobEvent::Progress(line)) => on_progress(&line),
                Ok(JobEvent::Finished(result)) => return result,
                Err(_) => {
                    return Err(ClientError::SessionLost {
                        outcome_unknown: true,
                        detail: "поток сеанса прав пропал".into(),
                    })
                }
            }
        }
    }
}

/// Ждать сокет или пробуждение. Возвращает (сокет готов, пробуждение).
fn wait_ready(shared: &Shared) -> io::Result<(bool, bool)> {
    let mut fds = [
        libc::pollfd {
            fd: shared.sock.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        },
        libc::pollfd {
            fd: shared.wake.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        },
    ];
    loop {
        // SAFETY: массив из двух живых pollfd.
        let rc = unsafe { libc::poll(fds.as_mut_ptr(), 2, -1) };
        if rc >= 0 {
            return Ok((fds[0].revents != 0, fds[1].revents != 0));
        }
        let err = io::Error::last_os_error();
        if err.kind() != io::ErrorKind::Interrupted {
            return Err(err);
        }
    }
}

/// Цикл потока «сеанс прав»: отдать следующую операцию, читать ответы.
fn session_loop(shared: &Shared) -> Death {
    loop {
        {
            let mut st = shared.lock();
            if st.closing {
                return Death::Closed;
            }
            if st.current.is_none() {
                if let Some(job) = st.queue.pop_front() {
                    let fds: Vec<BorrowedFd<'_>> = job.fds.iter().map(|f| f.as_fd()).collect();
                    if let Err(e) = proto::send_frame_with_fds(shared.sock.as_fd(), &job.frame, &fds) {
                        let detail = e.to_string();
                        // Сеанс мёртв ДО ответа ждущему: проснувшись, он не
                        // должен увидеть «живой» сеанс (см. [`handle_msg`]).
                        st.dead = Some(Death::Lost(detail.clone()));
                        let _ = job.reply.send(JobEvent::Finished(Err(ClientError::SessionLost {
                            outcome_unknown: false,
                            detail: detail.clone(),
                        })));
                        return Death::Lost(detail);
                    }
                    // Свои копии файлов закрываются здесь: у помощника — дубликаты.
                    st.current = Some(InFlight {
                        id: job.id,
                        reply: job.reply,
                    });
                }
            }
        }
        let (sock_ready, woken) = match wait_ready(shared) {
            Ok(r) => r,
            Err(e) => return Death::Lost(format!("poll: {e}")),
        };
        if woken {
            shared.drain_wake();
        }
        if !sock_ready {
            continue;
        }
        match proto::recv_msg::<HelperMsg>(shared.sock.as_fd()) {
            Ok(Some(msg)) => {
                if let Some(death) = handle_msg(shared, msg) {
                    return death;
                }
            }
            Ok(None) => {
                return if shared.lock().closing {
                    Death::Closed
                } else {
                    Death::Lost("помощник закрыл канал".into())
                };
            }
            Err(e) => {
                return if shared.lock().closing {
                    Death::Closed
                } else {
                    Death::Lost(e.to_string())
                };
            }
        }
    }
}

/// Разобрать сообщение помощника. `Some` — сеанс кончился.
fn handle_msg(shared: &Shared, msg: HelperMsg) -> Option<Death> {
    let mut st = shared.lock();
    let current = st.current.as_ref().map(|c| c.id);
    match msg {
        HelperMsg::Progress { id, line } if current == Some(id) => {
            if let Some(c) = &st.current {
                let _ = c.reply.send(JobEvent::Progress(line));
            }
            None
        }
        HelperMsg::Done {
            id,
            ok,
            code,
            message_key,
        } if current == Some(id) => {
            if let Some(c) = st.current.take() {
                c.finish(Ok(Outcome { ok, code, message_key }));
            }
            None
        }
        HelperMsg::Busy { id } if current == Some(id) => {
            if let Some(c) = st.current.take() {
                c.finish(Err(ClientError::Busy));
            }
            None
        }
        HelperMsg::Error {
            id: Some(id),
            code,
            message_key,
        } if current == Some(id) => {
            let c = st.current.take()?;
            match code {
                ErrorCode::Refused => {
                    c.finish(Err(ClientError::Refused { message_key }));
                    None
                }
                ErrorCode::Cancelled => {
                    c.finish(Err(ClientError::Cancelled));
                    None
                }
                ErrorCode::Internal => {
                    c.finish(Err(ClientError::HelperFailed { message_key }));
                    None
                }
                // Сеанс кончается: отметить смерть ДО ответа ждущему, под тем
                // же замком. Иначе ждущий просыпается раньше, чем поток сеанса
                // дойдёт до [`die`], и успевает увидеть «живой» сеанс
                // (`has_session()` после отзыва права — гонка, ловилась
                // тестом `revoked_answer_to_operation`).
                ErrorCode::Revoked => {
                    let death = Death::Revoked(message_key.clone());
                    st.dead = Some(death.clone());
                    c.finish(Err(ClientError::Revoked { message_key }));
                    Some(death)
                }
                ErrorCode::BuildMismatch => {
                    st.dead = Some(Death::BuildMismatch(None));
                    c.finish(Err(ClientError::BuildMismatch { helper_build: None }));
                    Some(Death::BuildMismatch(None))
                }
                ErrorCode::Sequence => {
                    let detail = format!("помощник: нарушен порядок ({message_key})");
                    st.dead = Some(Death::Lost(detail.clone()));
                    c.finish(Err(ClientError::SessionLost {
                        outcome_unknown: false,
                        detail: detail.clone(),
                    }));
                    Some(Death::Lost(detail))
                }
            }
        }
        HelperMsg::Error {
            id: None,
            code: ErrorCode::Revoked,
            message_key,
        } => Some(Death::Revoked(message_key)),
        HelperMsg::Error {
            id: None,
            code: ErrorCode::BuildMismatch,
            ..
        } => Some(Death::BuildMismatch(None)),
        other => Some(Death::Lost(format!("нежданное сообщение помощника: {other:?}"))),
    }
}

/// Сеанс кончился: всем ожидающим — ошибку, сокет — закрыть.
fn die(shared: &Shared, death: Death) {
    let (current, queued) = {
        let mut st = shared.lock();
        st.dead = Some(death.clone());
        (st.current.take(), std::mem::take(&mut st.queue))
    };
    if let Some(c) = current {
        c.finish(Err(death.in_flight_error()));
    }
    for job in queued {
        let _ = job.reply.send(JobEvent::Finished(Err(death.queued_error())));
    }
    shared.shutdown_socket();
}

// ============================================================================
// Клиент
// ============================================================================

enum HandshakeFail {
    /// Помощник закрыл канал, не ответив (окно пароля закрыто, нет прав).
    Eof,
    Error(ClientError),
}

/// `Hello` → `Welcome`. `Welcome` с чужим отпечатком тоже отвергается:
/// помощник проверяет сам, окно — второй раз.
fn handshake(sock: &OwnedFd, build_id: &str) -> Result<(), HandshakeFail> {
    let hello = ClientMsg::<PrivOp>::Hello(Hello {
        build_id: build_id.to_string(),
    });
    match proto::send_msg(sock.as_fd(), &hello) {
        Ok(()) => {}
        Err(proto::ProtoError::Io(e)) if e.raw_os_error() == Some(libc::EPIPE) => {
            return Err(HandshakeFail::Eof)
        }
        Err(e) => {
            return Err(HandshakeFail::Error(ClientError::SessionLost {
                outcome_unknown: false,
                detail: e.to_string(),
            }))
        }
    }
    let lost = |detail: String| {
        HandshakeFail::Error(ClientError::SessionLost {
            outcome_unknown: false,
            detail,
        })
    };
    match proto::recv_msg::<HelperMsg>(sock.as_fd()) {
        Ok(None) => Err(HandshakeFail::Eof),
        // Помощник (или сам pkexec) вышел, не прочитав `Hello`: непрочитанный
        // кадр в его очереди превращает закрытие в ECONNRESET, а не в EOF.
        // Живой pkexec с 126/127 выходит именно так.
        Err(proto::ProtoError::Io(e)) if e.raw_os_error() == Some(libc::ECONNRESET) => {
            Err(HandshakeFail::Eof)
        }
        Ok(Some(HelperMsg::Welcome { build_id: theirs })) => {
            if theirs == build_id {
                Ok(())
            } else {
                Err(HandshakeFail::Error(ClientError::BuildMismatch {
                    helper_build: Some(theirs),
                }))
            }
        }
        Ok(Some(HelperMsg::Error {
            code: ErrorCode::BuildMismatch,
            ..
        })) => Err(HandshakeFail::Error(ClientError::BuildMismatch { helper_build: None })),
        Ok(Some(HelperMsg::Error {
            code: ErrorCode::Revoked,
            message_key,
            ..
        })) => Err(HandshakeFail::Error(ClientError::Revoked { message_key })),
        Ok(Some(other)) => Err(lost(format!("рукопожатие: нежданное сообщение {other:?}"))),
        Err(e) => Err(lost(format!("рукопожатие: {e}"))),
    }
}

/// Клиент сеанса прав окна. Сеанс поднимается при первом запросе и
/// живёт, пока не кончится (отзыв, обрыв) или окно его не закроет;
/// следующий запрос после конца поднимает новый (новый пароль). После
/// `BuildMismatch` — больше никогда: окно должно перезапуститься.
pub struct Client {
    spawner: Box<dyn Spawner>,
    fs: Box<dyn HelperFs>,
    candidates: Vec<PathBuf>,
    build_id: String,
    session_id: Option<String>,
    /// Язык интерфейса окна: помощник переводит свои сообщения.
    lang: Option<String>,
    session: Option<Session>,
    stale_build: Option<Option<String>>,
    next_id: u64,
}

impl Client {
    pub fn new(
        spawner: Box<dyn Spawner>,
        fs: Box<dyn HelperFs>,
        build_id: String,
        session_id: Option<String>,
    ) -> Self {
        Self {
            spawner,
            fs,
            candidates: helper_candidates(),
            build_id,
            session_id,
            lang: None,
            session: None,
            stale_build: None,
            next_id: 1,
        }
    }

    /// Живой клиент: `pkexec`, настоящая ФС, отпечаток этой сборки, номер
    /// сеанса из `XDG_SESSION_ID`, язык интерфейса Rexon.
    pub fn live() -> Self {
        Self::new(
            Box::new(CommandSpawner),
            Box::new(RealFs),
            proto::build_id(),
            std::env::var("XDG_SESSION_ID").ok(),
        )
        .with_lang(&crate::i18n::current_language())
    }

    /// Язык сообщений помощника (код интерфейса, `ru`); в argv попадает
    /// только годный ([`lang_arg`]).
    pub fn with_lang(mut self, lang: &str) -> Self {
        self.lang = Some(lang.to_string());
        self
    }

    /// Свои места поиска помощника (тесты, будущий Б14).
    pub fn with_candidates(mut self, candidates: Vec<PathBuf>) -> Self {
        self.candidates = candidates;
        self
    }

    /// Живой сеанс, если есть (ручка для отмены из другого потока).
    pub fn session(&self) -> Option<Session> {
        self.session.as_ref().filter(|s| s.is_alive()).cloned()
    }

    /// Есть живой сеанс: следующий запрос пароля не спросит.
    pub fn has_session(&self) -> bool {
        self.session().is_some()
    }

    /// Сеанс есть — вернуть его; нет — поднять (окно пароля).
    pub fn ensure_session(&mut self) -> Result<Session, ClientError> {
        if let Some(b) = &self.stale_build {
            return Err(ClientError::BuildMismatch { helper_build: b.clone() });
        }
        if let Some(s) = &self.session {
            if s.is_alive() {
                return Ok(s.clone());
            }
            if let Some(Death::BuildMismatch(b)) = s.death() {
                self.stale_build = Some(b.clone());
                self.session = None;
                return Err(ClientError::BuildMismatch { helper_build: b });
            }
            self.session = None;
        }
        match self.connect() {
            Ok(s) => {
                self.session = Some(s.clone());
                Ok(s)
            }
            Err(e) => {
                if let ClientError::BuildMismatch { helper_build } = &e {
                    self.stale_build = Some(helper_build.clone());
                }
                Err(e)
            }
        }
    }

    fn connect(&mut self) -> Result<Session, ClientError> {
        let helper = match locate_helper(&*self.fs, &self.candidates) {
            Located::Ready(p) => p,
            Located::Missing => return Err(ClientError::HelperMissing),
            Located::BadOwner(problem) => return Err(ClientError::HelperBadOwner(problem)),
        };
        // Сверка сборки ДО окна пароля: иначе человек вводил бы пароль ради
        // одного «Rexon обновлён», а прежний путь после рукопожатия закрыт (З5).
        if let Some(theirs) = self.spawner.probe_build_id(&helper) {
            if theirs != self.build_id {
                return Err(ClientError::HelperOtherBuild { helper_build: theirs });
            }
        }
        let argv = helper_argv(&helper, self.session_id.as_deref(), self.lang.as_deref());
        let wake = new_eventfd().map_err(|e| ClientError::Spawn(format!("eventfd: {e}")))?;
        let (ours, theirs) =
            proto::seqpacket_pair().map_err(|e| ClientError::Spawn(format!("socketpair: {e}")))?;
        // `theirs` уходит в Spawner: он ставит его на fd 0/1 дочки и
        // закрывает свою копию до возврата.
        let mut child = self
            .spawner
            .spawn(&argv, theirs)
            .map_err(|e| ClientError::Spawn(e.to_string()))?;
        match handshake(&ours, &self.build_id) {
            Ok(()) => Session::start(ours, wake, child).map_err(|e| {
                ClientError::Spawn(format!("поток сеанса прав: {e}"))
            }),
            Err(HandshakeFail::Eof) => {
                drop(ours);
                match child.wait() {
                    Ok(Some(code)) if code == PKEXEC_DISMISSED || code == PKEXEC_NOT_AUTHORIZED => {
                        Err(ClientError::NotAuthorized { code })
                    }
                    Ok(code) => Err(ClientError::SessionLost {
                        outcome_unknown: false,
                        detail: format!("помощник вышел до рукопожатия (код {code:?})"),
                    }),
                    Err(e) => Err(ClientError::SessionLost {
                        outcome_unknown: false,
                        detail: format!("помощник до рукопожатия: {e}"),
                    }),
                }
            }
            Err(HandshakeFail::Error(e)) => {
                drop(ours);
                reap_in_background(child);
                Err(e)
            }
        }
    }

    /// Поставить операцию в очередь сеанса (поднимает сеанс, если нужно).
    /// `fds` — открытые файлы операции в порядке [`PrivOp::local_files`]
    /// (номер `slot` = индекс).
    pub fn submit(&mut self, op: &PrivOp, fds: Vec<OwnedFd>) -> Result<Pending, ClientError> {
        let files = op.local_files();
        if files.len() != fds.len() {
            return Err(ClientError::InvalidRequest(format!(
                "операции нужно {} файлов, приложено {}",
                files.len(),
                fds.len()
            )));
        }
        if files.iter().enumerate().any(|(i, f)| usize::from(f.slot) != i) {
            return Err(ClientError::InvalidRequest("номера файлов не по порядку".into()));
        }
        if fds.len() > proto::MAX_FDS {
            return Err(ClientError::InvalidRequest(format!(
                "{} файлов больше предела {}",
                fds.len(),
                proto::MAX_FDS
            )));
        }
        let id = self.next_id;
        self.next_id += 1;
        let frame = proto::encode(&ClientMsg::Request { id, op })
            .map_err(|e| ClientError::InvalidRequest(e.to_string()))?;
        let session = self.ensure_session()?;
        session.submit(id, frame, fds)
    }

    /// Выполнить операцию и дождаться итога; строки хода — в `on_progress`.
    pub fn request(
        &mut self,
        op: &PrivOp,
        fds: Vec<OwnedFd>,
        on_progress: &mut dyn FnMut(&str),
    ) -> Result<Outcome, ClientError> {
        self.submit(op, fds)?.wait(on_progress)
    }

    /// Закрыть сеанс (окно закрыто, свёрнуто в лоток, «Запретить сейчас»).
    pub fn shutdown(&mut self) {
        if let Some(s) = self.session.take() {
            s.shutdown();
        }
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::privilege::op::LocalFile;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    const BUILD: &str = "0.5.0+test";
    const HELPER: &str = "/usr/lib/rexon-priv-helper";

    // ---------------------------------------------------------------- ФС

    #[derive(Default)]
    struct FakeFs {
        files: HashMap<PathBuf, FileMeta>,
        links: HashMap<PathBuf, PathBuf>,
    }

    impl FakeFs {
        fn dir(mut self, p: &str, uid: u32, mode: u32) -> Self {
            self.files.insert(p.into(), FileMeta { kind: FileKind::Dir, uid, mode });
            self
        }
        fn file(mut self, p: &str, uid: u32, mode: u32) -> Self {
            self.files.insert(p.into(), FileMeta { kind: FileKind::Regular, uid, mode });
            self
        }
        fn link(mut self, p: &str, target: &str) -> Self {
            self.files.insert(p.into(), FileMeta { kind: FileKind::Symlink, uid: 0, mode: 0o777 });
            self.links.insert(p.into(), target.into());
            self
        }
        /// Корень и каталоги помощников — root 0755.
        fn base() -> Self {
            Self::default()
                .dir("/", 0, 0o755)
                .dir("/usr", 0, 0o755)
                .dir("/usr/lib", 0, 0o755)
                .dir("/usr/local", 0, 0o755)
                .dir("/usr/local/lib", 0, 0o755)
                .dir("/var", 0, 0o755)
                .dir("/var/lib", 0, 0o755)
                .dir("/var/lib/rexon", 0, 0o755)
                .dir("/var/lib/rexon/bin", 0, 0o755)
        }
        fn good() -> Self {
            Self::base().file(HELPER, 0, 0o755)
        }
    }

    impl HelperFs for FakeFs {
        fn lstat(&self, path: &Path) -> io::Result<FileMeta> {
            self.files
                .get(path)
                .copied()
                .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))
        }
        fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
            Ok(self.links.get(path).cloned().unwrap_or_else(|| path.to_path_buf()))
        }
    }

    // ---------------------------------------------------------------- помощник

    type Script = dyn Fn(OwnedFd) -> i32 + Send + Sync;

    struct FakeSpawner {
        calls: Arc<Mutex<Vec<Vec<String>>>>,
        script: Arc<Script>,
        /// Что помощник отвечает на `--build-id` (`None` — старый, без флага).
        probe: Option<String>,
    }

    struct FakeChild(Option<thread::JoinHandle<i32>>);

    impl HelperChild for FakeChild {
        fn wait(&mut self) -> io::Result<Option<i32>> {
            Ok(self.0.take().map(|h| h.join().unwrap_or(-1)))
        }
    }

    impl Spawner for FakeSpawner {
        fn spawn(&mut self, argv: &[String], helper_end: OwnedFd) -> io::Result<Box<dyn HelperChild>> {
            self.calls.lock().unwrap().push(argv.to_vec());
            let script = Arc::clone(&self.script);
            Ok(Box::new(FakeChild(Some(thread::spawn(move || script(helper_end))))))
        }

        fn probe_build_id(&mut self, _helper: &Path) -> Option<String> {
            self.probe.clone()
        }
    }

    fn client_with(
        fs: FakeFs,
        script: impl Fn(OwnedFd) -> i32 + Send + Sync + 'static,
    ) -> (Client, Arc<Mutex<Vec<Vec<String>>>>) {
        client_probing(fs, None, script)
    }

    fn client_probing(
        fs: FakeFs,
        probe: Option<&str>,
        script: impl Fn(OwnedFd) -> i32 + Send + Sync + 'static,
    ) -> (Client, Arc<Mutex<Vec<Vec<String>>>>) {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let spawner = FakeSpawner {
            calls: Arc::clone(&calls),
            script: Arc::new(script),
            probe: probe.map(str::to_string),
        };
        let client = Client::new(Box::new(spawner), Box::new(fs), BUILD.into(), Some("2".into()));
        (client, calls)
    }

    fn send(sock: &OwnedFd, msg: HelperMsg) {
        proto::send_msg(sock.as_fd(), &msg).unwrap();
    }

    /// Принять `Hello` и ответить `Welcome`.
    fn accept_hello(sock: &OwnedFd) {
        let hello: ClientMsg<PrivOp> = proto::recv_msg(sock.as_fd()).unwrap().unwrap();
        assert_eq!(hello, ClientMsg::Hello(Hello { build_id: BUILD.into() }));
        send(sock, HelperMsg::Welcome { build_id: BUILD.into() });
    }

    /// Помощник, исполняющий все операции успешно, с одной строкой хода.
    /// Номера пришедших операций и число файлов — в `seen`.
    fn obedient(seen: Arc<Mutex<Vec<(u64, usize)>>>) -> impl Fn(OwnedFd) -> i32 + Send + Sync {
        move |sock| {
            accept_hello(&sock);
            loop {
                let Ok(Some((frame, fds))) = proto::recv_frame_with_fds(sock.as_fd()) else {
                    return 0;
                };
                match proto::decode::<ClientMsg<PrivOp>>(&frame).unwrap() {
                    ClientMsg::Request { id, .. } => {
                        seen.lock().unwrap().push((id, fds.len()));
                        send(&sock, HelperMsg::Progress { id, line: format!("ход {id}") });
                        send(
                            &sock,
                            HelperMsg::Done { id, ok: true, code: Some(0), message_key: None },
                        );
                    }
                    other => panic!("нежданное: {other:?}"),
                }
            }
        }
    }

    fn op() -> PrivOp {
        PrivOp::BuildDeps { names: vec!["base-devel".into()] }
    }

    fn no_progress() -> impl FnMut(&str) {
        |_| {}
    }

    // ---------------------------------------------------------------- поиск

    #[test]
    fn candidates_cover_package_manual_and_steamos() {
        let c = helper_candidates();
        assert_eq!(
            c,
            vec![
                PathBuf::from("/usr/lib/rexon-priv-helper"),
                PathBuf::from("/usr/local/lib/rexon-priv-helper"),
                PathBuf::from("/var/lib/rexon/bin/rexon-priv-helper"),
            ]
        );
    }

    #[test]
    fn locate_good_and_missing() {
        let cands = helper_candidates();
        assert_eq!(locate_helper(&FakeFs::good(), &cands), Located::Ready(HELPER.into()));
        assert_eq!(locate_helper(&FakeFs::base(), &cands), Located::Missing);
        let steam = FakeFs::base().file("/var/lib/rexon/bin/rexon-priv-helper", 0, 0o755);
        assert_eq!(
            locate_helper(&steam, &cands),
            Located::Ready("/var/lib/rexon/bin/rexon-priv-helper".into())
        );
    }

    #[test]
    fn locate_refuses_foreign_owner_and_writable() {
        let cands = helper_candidates();
        let cases = [
            (FakeFs::base().file(HELPER, 1000, 0o755), OwnerProblem::NotRoot(HELPER.into())),
            (FakeFs::base().file(HELPER, 0, 0o775), OwnerProblem::Writable(HELPER.into())),
            (FakeFs::base().file(HELPER, 0, 0o757), OwnerProblem::Writable(HELPER.into())),
            (FakeFs::base().file(HELPER, 0, 0o644), OwnerProblem::NotExecutable(HELPER.into())),
            (
                FakeFs::good().dir("/usr/lib", 0, 0o775),
                OwnerProblem::Writable("/usr/lib".into()),
            ),
            (
                FakeFs::good().dir("/usr", 1000, 0o755),
                OwnerProblem::NotRoot("/usr".into()),
            ),
            (
                FakeFs::good().dir("/", 0, 0o777),
                OwnerProblem::Writable("/".into()),
            ),
        ];
        for (fs, want) in cases {
            assert_eq!(locate_helper(&fs, &cands), Located::BadOwner(want));
        }
        // Первый найденный решает: плохой в /usr/lib не обходится хорошим
        // в /usr/local/lib.
        let both = FakeFs::base()
            .file(HELPER, 1000, 0o755)
            .file("/usr/local/lib/rexon-priv-helper", 0, 0o755);
        assert!(matches!(locate_helper(&both, &cands), Located::BadOwner(_)));
    }

    #[test]
    fn helper_symlink_refused_and_dir_symlink_checked_by_target() {
        let cands = helper_candidates();
        // Сам помощник — ссылка: нельзя.
        let fs = FakeFs::base().link(HELPER, "/home/u/x");
        assert_eq!(
            locate_helper(&fs, &cands),
            Located::BadOwner(OwnerProblem::NotRegular(HELPER.into()))
        );
        // Каталог-ссылка в пользовательский каталог: цель проверяется.
        let fs = FakeFs::default()
            .dir("/", 0, 0o755)
            .dir("/usr", 0, 0o755)
            .link("/usr/lib", "/home/u/lib")
            .dir("/home", 0, 0o755)
            .dir("/home/u", 1000, 0o700)
            .dir("/home/u/lib", 1000, 0o755)
            .file(HELPER, 0, 0o755)
            .file("/home/u/lib/rexon-priv-helper", 0, 0o755);
        let mut fs = fs;
        fs.links.insert(HELPER.into(), "/home/u/lib/rexon-priv-helper".into());
        assert_eq!(
            check_ownership(&fs, Path::new(HELPER)),
            Err(OwnerProblem::NotRoot("/home/u".into()))
        );
    }

    #[test]
    fn relative_and_dotdot_paths_refused() {
        let fs = FakeFs::good();
        assert!(matches!(
            check_ownership(&fs, Path::new("usr/lib/rexon-priv-helper")),
            Err(OwnerProblem::NotAbsolute(_))
        ));
        assert!(matches!(
            check_ownership(&fs, Path::new("/usr/lib/../lib/rexon-priv-helper")),
            Err(OwnerProblem::NotAbsolute(_))
        ));
    }

    #[test]
    fn real_fs_root_binary_passes_user_file_fails() {
        // Настоящая ФС: системный бинарь root проходит, свой файл — нет.
        let sys = Path::new("/usr/bin/env");
        if RealFs.lstat(sys).map(|m| m.kind == FileKind::Regular && m.uid == 0).unwrap_or(false) {
            assert_eq!(check_ownership(&RealFs, sys), Ok(()));
        }
        // Файл в дереве пользователя: цепочка рвётся на его каталоге.
        let mine = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"));
        let res = check_ownership(&RealFs, mine);
        // Если тесты идут от root, владельца не проверить.
        // SAFETY: getuid без аргументов.
        if unsafe { libc::getuid() } != 0 {
            assert!(matches!(res, Err(OwnerProblem::NotRoot(_))), "{res:?}");
        }
        // /tmp (1777) — запись для всех: помощник оттуда не годится.
        assert_eq!(
            check_chain(&RealFs, Path::new("/tmp/rexon-priv-helper"))
                .err()
                .filter(|p| matches!(p, OwnerProblem::Writable(_))),
            Some(OwnerProblem::Writable("/tmp".into()))
        );
    }

    #[test]
    fn argv_carries_session_id_only_when_valid() {
        let p = Path::new(HELPER);
        assert_eq!(helper_argv(p, Some("2"), None), ["pkexec", HELPER, "--session", "2"]);
        assert_eq!(helper_argv(p, None, None), ["pkexec", HELPER]);
        assert_eq!(helper_argv(p, Some("-p"), None), ["pkexec", HELPER]);
        assert_eq!(helper_argv(p, Some(""), None), ["pkexec", HELPER]);
    }

    #[test]
    fn argv_carries_lang_like_old_helpers() {
        let p = Path::new(HELPER);
        // Тот же вид, что у прежних помощников (`system_l10n::lang_arg_for`).
        let old = crate::system_l10n::lang_arg_for("ru", Some(crate::system_l10n::LANG_MARKER));
        assert_eq!(old.as_deref(), Some("--lang=ru"));
        assert_eq!(
            helper_argv(p, Some("2"), Some("ru")),
            ["pkexec", HELPER, "--session", "2", "--lang=ru"]
        );
        assert_eq!(helper_argv(p, None, Some("de")), ["pkexec", HELPER, "--lang=de"]);
        // Английский, мусор и язык вне каталога — без аргумента.
        for bad in ["en", "", "../x", "RU", "ru;id", "xx"] {
            assert_eq!(helper_argv(p, None, Some(bad)), ["pkexec", HELPER], "{bad}");
        }
        // Помощник (Б4) понимает ровно этот вид.
        let argv = helper_argv(p, Some("c2"), Some("ru"));
        let args = crate::privilege::helper::parse_args(&argv[2..]).expect("помощник принял");
        assert_eq!(args.lang.as_deref(), Some("ru"));
        assert_eq!(args.session.as_deref(), Some("c2"));
    }

    #[test]
    fn live_style_client_passes_lang_to_spawner() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let (client, calls) = client_with(FakeFs::good(), obedient(Arc::clone(&seen)));
        let mut client = client.with_lang("ru");
        client.request(&op(), Vec::new(), &mut no_progress()).unwrap();
        assert_eq!(
            calls.lock().unwrap().as_slice(),
            [vec!["pkexec".to_string(), HELPER.into(), "--session".into(), "2".into(), "--lang=ru".into()]]
        );
    }

    // ---------------------------------------------------------------- сеанс

    #[test]
    fn missing_and_bad_owner_allow_raw_path_without_spawning() {
        let (mut c, calls) = client_with(FakeFs::base(), |_| 0);
        let e = c.request(&op(), vec![], &mut no_progress()).unwrap_err();
        assert_eq!(e, ClientError::HelperMissing);
        assert!(e.raw_fallback_allowed());

        let (mut c2, calls2) = client_with(FakeFs::base().file(HELPER, 1000, 0o755), |_| 0);
        let e = c2.request(&op(), vec![], &mut no_progress()).unwrap_err();
        assert!(matches!(e, ClientError::HelperBadOwner(OwnerProblem::NotRoot(_))));
        assert!(e.raw_fallback_allowed());
        assert!(calls.lock().unwrap().is_empty());
        assert!(calls2.lock().unwrap().is_empty());
    }

    #[test]
    fn hello_welcome_and_many_ops_one_password() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let (mut c, calls) = client_with(FakeFs::good(), obedient(Arc::clone(&seen)));
        let mut lines = Vec::new();
        for _ in 0..3 {
            let out = c.request(&op(), vec![], &mut |l: &str| lines.push(l.to_string())).unwrap();
            assert_eq!(out, Outcome { ok: true, code: Some(0), message_key: None });
        }
        assert!(c.has_session());
        let calls = calls.lock().unwrap();
        assert_eq!(calls.len(), 1, "одно окно пароля на три операции");
        assert_eq!(calls[0], ["pkexec", HELPER, "--session", "2"]);
        let ids: Vec<u64> = seen.lock().unwrap().iter().map(|(id, _)| *id).collect();
        assert_eq!(ids, [1, 2, 3], "номера монотонные");
        assert_eq!(lines, ["ход 1", "ход 2", "ход 3"]);
    }

    #[test]
    fn local_files_travel_as_fds() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let (mut c, _) = client_with(FakeFs::good(), obedient(Arc::clone(&seen)));
        let built = PrivOp::InstallBuilt {
            package: LocalFile { slot: 0, label: "/home/u/x.pkg.tar.zst".into() },
            build_only: vec![],
            replace: false,
        };
        // Без файла — негодный запрос, сеанс даже не поднимается.
        let e = c.request(&built, vec![], &mut no_progress()).unwrap_err();
        assert!(matches!(e, ClientError::InvalidRequest(_)));
        assert!(!c.has_session());
        let file = std::fs::File::open("/dev/null").unwrap();
        c.request(&built, vec![OwnedFd::from(file)], &mut no_progress()).unwrap();
        assert_eq!(seen.lock().unwrap().last().map(|(_, n)| *n), Some(1));
    }

    #[test]
    fn build_mismatch_is_sticky_and_not_raw() {
        let (mut c, calls) = client_with(FakeFs::good(), |sock| {
            let _: ClientMsg<PrivOp> = proto::recv_msg(sock.as_fd()).unwrap().unwrap();
            send(
                &sock,
                HelperMsg::Error {
                    id: None,
                    code: ErrorCode::BuildMismatch,
                    message_key: "build".into(),
                },
            );
            0
        });
        let e = c.request(&op(), vec![], &mut no_progress()).unwrap_err();
        assert_eq!(e, ClientError::BuildMismatch { helper_build: None });
        assert!(!e.raw_fallback_allowed());
        // Второй раз пароль не спрашивается: Rexon надо перезапустить.
        let e = c.request(&op(), vec![], &mut no_progress()).unwrap_err();
        assert!(matches!(e, ClientError::BuildMismatch { .. }));
        assert_eq!(calls.lock().unwrap().len(), 1);
    }

    /// Помощник другой сборки узнан до `pkexec`: окна пароля нет, путь
    /// «как сейчас» разрешён (окно из дерева разработки при пакете в системе).
    #[test]
    fn other_build_found_before_password_allows_old_path() {
        let (mut c, calls) = client_probing(FakeFs::good(), Some("0.4.9+old"), |_| 0);
        let e = c.request(&op(), vec![], &mut no_progress()).unwrap_err();
        assert_eq!(e, ClientError::HelperOtherBuild { helper_build: "0.4.9+old".into() });
        assert!(e.raw_fallback_allowed());
        assert!(calls.lock().unwrap().is_empty(), "пароль не спрашивался");
        // И в следующий раз — снова прежним путём, без «Rexon обновлён».
        let again = c.request(&op(), vec![], &mut no_progress()).unwrap_err();
        assert!(matches!(again, ClientError::HelperOtherBuild { .. }));
    }

    /// Своя сборка по `--build-id` — обычный сеанс, одно окно пароля.
    #[test]
    fn own_build_by_probe_starts_session() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let (mut c, calls) = client_probing(FakeFs::good(), Some(BUILD), obedient(Arc::clone(&seen)));
        c.request(&op(), vec![], &mut no_progress()).unwrap();
        assert_eq!(calls.lock().unwrap().len(), 1);
    }

    /// Живой `--build-id`: настоящая программа печатает отпечаток.
    #[test]
    fn live_probe_reads_first_line_and_rejects_failure() {
        let dir = std::env::temp_dir().join(format!("rexon-probe-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let good = dir.join("good");
        let bad = dir.join("bad");
        std::fs::write(&good, "#!/bin/sh\necho 1.2.3+abc\n").unwrap();
        std::fs::write(&bad, "#!/bin/sh\nexit 3\n").unwrap();
        for f in [&good, &bad] {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(f, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        assert_eq!(CommandSpawner.probe_build_id(&good).as_deref(), Some("1.2.3+abc"));
        assert_eq!(CommandSpawner.probe_build_id(&bad), None);
        assert_eq!(CommandSpawner.probe_build_id(&dir.join("absent")), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn welcome_with_foreign_build_is_mismatch() {
        let (mut c, _) = client_with(FakeFs::good(), |sock| {
            let _: ClientMsg<PrivOp> = proto::recv_msg(sock.as_fd()).unwrap().unwrap();
            send(&sock, HelperMsg::Welcome { build_id: "0.4.9+old".into() });
            0
        });
        let e = c.request(&op(), vec![], &mut no_progress()).unwrap_err();
        assert_eq!(e, ClientError::BuildMismatch { helper_build: Some("0.4.9+old".into()) });
    }

    #[test]
    fn pkexec_126_and_127_are_human_refusal() {
        for code in [PKEXEC_DISMISSED, PKEXEC_NOT_AUTHORIZED] {
            let (mut c, calls) = client_with(FakeFs::good(), move |sock| {
                drop(sock);
                code
            });
            let e = c.request(&op(), vec![], &mut no_progress()).unwrap_err();
            assert_eq!(e, ClientError::NotAuthorized { code });
            assert!(!e.raw_fallback_allowed(), "отказ человека — не повод для сырого pkexec");
            assert!(!c.has_session());
            // Следующий запрос снова спросит пароль.
            let _ = c.request(&op(), vec![], &mut no_progress());
            assert_eq!(calls.lock().unwrap().len(), 2);
        }
    }

    #[test]
    fn refused_has_no_raw_fallback_and_session_survives() {
        let (mut c, calls) = client_with(FakeFs::good(), |sock| {
            accept_hello(&sock);
            let mut n = 0;
            while let Ok(Some(ClientMsg::Request { id, .. })) =
                proto::recv_msg::<ClientMsg<PrivOp>>(sock.as_fd())
            {
                n += 1;
                let msg = if n == 1 {
                    HelperMsg::Error {
                        id: Some(id),
                        code: ErrorCode::Refused,
                        message_key: "refused.vital".into(),
                    }
                } else {
                    HelperMsg::Done { id, ok: false, code: Some(1), message_key: None }
                };
                send(&sock, msg);
            }
            0
        });
        let e = c.request(&op(), vec![], &mut no_progress()).unwrap_err();
        assert_eq!(e, ClientError::Refused { message_key: "refused.vital".into() });
        assert!(!e.raw_fallback_allowed());
        let out = c.request(&op(), vec![], &mut no_progress()).unwrap();
        assert_eq!(out, Outcome { ok: false, code: Some(1), message_key: None });
        assert_eq!(calls.lock().unwrap().len(), 1);
    }

    #[test]
    fn break_mid_operation_is_unknown_outcome() {
        let (mut c, _) = client_with(FakeFs::good(), |sock| {
            accept_hello(&sock);
            let msg: ClientMsg<PrivOp> = proto::recv_msg(sock.as_fd()).unwrap().unwrap();
            let ClientMsg::Request { id, .. } = msg else { panic!() };
            send(&sock, HelperMsg::Progress { id, line: "начато".into() });
            drop(sock);
            0
        });
        let mut lines = Vec::new();
        let e = c.request(&op(), vec![], &mut |l: &str| lines.push(l.to_string())).unwrap_err();
        assert!(matches!(e, ClientError::SessionLost { outcome_unknown: true, .. }), "{e:?}");
        assert!(!e.raw_fallback_allowed());
        assert_eq!(lines, ["начато"]);
        assert!(!c.has_session());
    }

    #[test]
    fn garbage_frame_kills_session() {
        let (mut c, _) = client_with(FakeFs::good(), |sock| {
            accept_hello(&sock);
            let _: ClientMsg<PrivOp> = proto::recv_msg(sock.as_fd()).unwrap().unwrap();
            proto::send_frame(sock.as_fd(), b"{\"v\":1,\"msg\":{\"what\":1}}").unwrap();
            // Ждём, пока окно закроет канал.
            let _ = proto::recv_frame(sock.as_fd());
            0
        });
        let e = c.request(&op(), vec![], &mut no_progress()).unwrap_err();
        assert!(matches!(e, ClientError::SessionLost { outcome_unknown: true, .. }), "{e:?}");
        assert!(!e.raw_fallback_allowed());
    }

    #[test]
    fn revoked_session_is_raised_again() {
        let round = Arc::new(AtomicUsize::new(0));
        let r = Arc::clone(&round);
        let (mut c, calls) = client_with(FakeFs::good(), move |sock| {
            let n = r.fetch_add(1, Ordering::SeqCst);
            accept_hello(&sock);
            if n == 0 {
                // Право отозвано простоем ещё до первой операции.
                send(
                    &sock,
                    HelperMsg::Error {
                        id: None,
                        code: ErrorCode::Revoked,
                        message_key: "idle".into(),
                    },
                );
                return 0;
            }
            while let Ok(Some(ClientMsg::Request { id, .. })) =
                proto::recv_msg::<ClientMsg<PrivOp>>(sock.as_fd())
            {
                send(&sock, HelperMsg::Done { id, ok: true, code: Some(0), message_key: None });
            }
            0
        });
        c.ensure_session().unwrap();
        // Дождаться, пока поток сеанса прочтёт отзыв.
        for _ in 0..200 {
            if !c.has_session() {
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }
        assert!(!c.has_session());
        c.request(&op(), vec![], &mut no_progress()).unwrap();
        assert_eq!(calls.lock().unwrap().len(), 2, "после отзыва — новый пароль");
    }

    #[test]
    fn revoked_answer_to_operation() {
        let (mut c, _) = client_with(FakeFs::good(), |sock| {
            accept_hello(&sock);
            let Ok(Some(ClientMsg::Request { id, .. })) =
                proto::recv_msg::<ClientMsg<PrivOp>>(sock.as_fd())
            else {
                return 1;
            };
            send(
                &sock,
                HelperMsg::Error { id: Some(id), code: ErrorCode::Revoked, message_key: "locked".into() },
            );
            0
        });
        let e = c.request(&op(), vec![], &mut no_progress()).unwrap_err();
        assert_eq!(e, ClientError::Revoked { message_key: "locked".into() });
        assert!(!e.raw_fallback_allowed());
        assert!(!c.has_session());
    }

    #[test]
    fn cancel_queued_and_in_flight() {
        let (mut c, _) = client_with(FakeFs::good(), |sock| {
            accept_hello(&sock);
            while let Ok(Some(msg)) = proto::recv_msg::<ClientMsg<PrivOp>>(sock.as_fd()) {
                match msg {
                    // Операцию не завершаем сами: ждём Cancel.
                    ClientMsg::Request { .. } => {}
                    ClientMsg::Cancel { id } => send(
                        &sock,
                        HelperMsg::Error {
                            id: Some(id),
                            code: ErrorCode::Cancelled,
                            message_key: "cancelled".into(),
                        },
                    ),
                    ClientMsg::Hello(_) => return 1,
                }
            }
            0
        });
        let first = c.submit(&op(), vec![]).unwrap();
        let second = c.submit(&op(), vec![]).unwrap();
        let session = c.session().unwrap();
        assert!(first.id() < second.id());
        // Второй ещё в очереди: снимается без помощника.
        assert!(session.cancel(second.id()));
        assert_eq!(second.wait(&mut no_progress()), Err(ClientError::Cancelled));
        // Первый у помощника (ждём, пока поток его отдаст).
        let mut sent = false;
        for _ in 0..200 {
            if session.shared.lock().current.is_some() {
                sent = true;
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }
        assert!(sent);
        assert!(session.cancel(first.id()));
        assert_eq!(first.wait(&mut no_progress()), Err(ClientError::Cancelled));
        assert!(!session.cancel(999));
        assert!(c.has_session());
    }

    #[test]
    fn shutdown_closes_socket_for_helper() {
        let (tx, rx) = mpsc::channel();
        let (mut c, _) = client_with(FakeFs::good(), move |sock| {
            accept_hello(&sock);
            // Окно закрыло сеанс — помощник видит EOF.
            let eof = matches!(proto::recv_frame(sock.as_fd()), Ok(None));
            tx.send(eof).unwrap();
            0
        });
        let session = c.ensure_session().unwrap();
        c.shutdown();
        assert!(rx.recv_timeout(Duration::from_secs(5)).unwrap());
        assert!(!session.is_alive());
        // Закрытая ручка новых операций не берёт.
        assert_eq!(session.submit(99, vec![1], vec![]).err(), Some(ClientError::Closed));
    }

    // ---------------------------------------------------------------- З3

    fn inode(fd: BorrowedFd<'_>) -> u64 {
        // SAFETY: fstat в нулевую структуру по живому fd.
        let mut st: libc::stat = unsafe { std::mem::zeroed() };
        assert_eq!(unsafe { libc::fstat(fd.as_raw_fd(), &mut st) }, 0);
        st.st_ino as u64
    }

    /// Номера сокетов, открытых у процесса: fd → inode.
    fn socket_fds(pid: u32) -> HashMap<i32, u64> {
        let mut out = HashMap::new();
        for entry in std::fs::read_dir(format!("/proc/{pid}/fd")).unwrap().flatten() {
            let Ok(target) = std::fs::read_link(entry.path()) else { continue };
            let t = target.to_string_lossy().into_owned();
            if let Some(ino) = t.strip_prefix("socket:[").and_then(|s| s.strip_suffix(']')) {
                let fd: i32 = entry.file_name().to_string_lossy().parse().unwrap();
                out.insert(fd, ino.parse().unwrap());
            }
        }
        out
    }

    #[test]
    fn other_children_do_not_inherit_socket() {
        let (ours, theirs) = proto::seqpacket_pair().unwrap();
        let ours_ino = inode(ours.as_fd());
        let theirs_ino = inode(theirs.as_fd());

        // «Помощник» — настоящий процесс через CommandSpawner: сокет на 0 и 1.
        let mut helper = CommandSpawner
            .spawn(&["sleep".into(), "30".into()], theirs)
            .unwrap();
        // Посторонний потомок окна при открытом сокете.
        let mut other = Command::new("sleep").arg("30").spawn().unwrap();
        // И настоящий `true`: запускается и выходит без сокета.
        assert!(Command::new("true").status().unwrap().success());

        // pid «помощника» не знаем через трейт — ищем среди своих детей.
        let other_socks = socket_fds(other.id());
        assert!(
            !other_socks.values().any(|&i| i == ours_ino || i == theirs_ino),
            "посторонний потомок видит сокет сеанса: {other_socks:?}"
        );
        let helper_pid = child_pids()
            .into_iter()
            .find(|&p| p != other.id() && socket_fds(p).values().any(|&i| i == theirs_ino))
            .expect("помощник с сокетом на fd 0/1");
        let hs = socket_fds(helper_pid);
        assert_eq!(hs.get(&0), Some(&theirs_ino));
        assert_eq!(hs.get(&1), Some(&theirs_ino));
        assert!(!hs.values().any(|&i| i == ours_ino), "конец окна утёк помощнику");
        assert_eq!(hs.values().filter(|&&i| i == theirs_ino).count(), 2, "лишние копии");

        // У окна своей копии конца помощника не осталось.
        let mine = socket_fds(std::process::id());
        assert!(!mine.values().any(|&i| i == theirs_ino), "окно держит конец помощника");

        other.kill().ok();
        other.wait().ok();
        // SAFETY: kill своего дочернего процесса.
        unsafe { libc::kill(helper_pid as i32, libc::SIGKILL) };
        helper.wait().ok();
        drop(ours);
    }

    /// Прямые потомки этого процесса.
    fn child_pids() -> Vec<u32> {
        let me = std::process::id();
        let mut out = Vec::new();
        for entry in std::fs::read_dir("/proc").unwrap().flatten() {
            let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else { continue };
            let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else { continue };
            // Поле 4 — ppid; имя процесса в скобках может содержать пробелы.
            let Some(rest) = stat.rsplit_once(')').map(|(_, r)| r) else { continue };
            if rest.split_whitespace().nth(1).and_then(|p| p.parse::<u32>().ok()) == Some(me) {
                out.push(pid);
            }
        }
        out
    }
}

/// Тексты человеку (Б13): у каждой ошибки и ключа помощника — своя фраза,
/// без служебных слов, с переводом в `po/ru.po` и `po/de.po`.
#[cfg(test)]
mod human_text_tests {
    use super::*;
    use crate::privilege::revoke::Reason;

    const REASONS: [Reason; 8] = [
        Reason::WindowClosed,
        Reason::WindowToTray,
        Reason::Idle,
        Reason::ScreenLocked,
        Reason::Slept,
        Reason::SessionInactive,
        Reason::SessionLost,
        Reason::RevokedByUser,
    ];

    /// Ключи отказов помощника: проверка аргументов, распаковка, `.PKGINFO`.
    const REFUSED: &[&str] = &[
        "wayland-session-missing.plasmax11", "session-set-elsewhere.plasmax11",
        "empty", "too-long", "too-many", "operand", "package-name", "item-name", "family",
        "unknown-desktop", "not-in-table", "build-dep", "vital", "current-desktop-package",
        "current-desktop", "current-desktop-unknown", "path", "outside-root", "package-file",
        "display-manager", "boot-splash-code", "peer", "plm-key", "read-only-system", "conflict",
        "set-rule", "not-steamos", "not-regular", "too-large", "too-many-entries", "too-deep",
        "link", "special", "unsupported", "dot-dot", "absolute", "bad-name", "set-id", "duplicate",
        "escape", "malformed", "theme", "pkginfo", "not-orphan", "orphan-needed",
        "not-rollback", "rollback-extra", "not-root-owned", "rollback-signature",
        "orphan-in-use.kwin-x11", "orphan-login-default.kwin-x11",
        "font-file", "not-a-font", "font-exists", "no-fontconfig",
        "no-sddm-user", "sddm-theme", "login-image", "look-file", "look-value", "look-elsewhere",
        "no-setpriv",
    ];

    /// Служебные слова, которых покупатель видеть не должен (нижний регистр).
    const JARGON: &[&str] = &[
        "pkexec", "polkit", "helper", "помощник", "сеанс прав", "root", "sudo", "build", "сборк",
        "refused.", "revoked.", "session", "socket", "сокет", "handshake", "рукопожат", "dbus",
        "administrator", "администратор", "/usr/", "_", "{", "rexon-priv",
    ];

    fn english() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
    }

    fn all_errors() -> Vec<ClientError> {
        let mut v = vec![
            ClientError::HelperMissing,
            ClientError::HelperBadOwner(OwnerProblem::NotRoot("/x".into())),
            ClientError::NotAuthorized { code: 126 },
            ClientError::BuildMismatch { helper_build: Some("0.5.0+x".into()) },
            ClientError::BuildMismatch { helper_build: None },
            ClientError::HelperOtherBuild { helper_build: "0.5.0+x".into() },
            ClientError::HelperFailed { message_key: "spawn_failed".into() },
            ClientError::Cancelled,
            ClientError::SessionLost { outcome_unknown: true, detail: "eof".into() },
            ClientError::SessionLost { outcome_unknown: false, detail: "eof".into() },
            ClientError::Closed,
            ClientError::Busy,
            ClientError::Spawn("enoent".into()),
            ClientError::InvalidRequest("fds".into()),
            ClientError::Refused { message_key: "fds_mismatch".into() },
            ClientError::Refused { message_key: "unknown-future-key".into() },
        ];
        v.extend(REFUSED.iter().map(|c| ClientError::Refused { message_key: format!("refused.{c}") }));
        v.extend(REASONS.iter().map(|r| ClientError::Revoked { message_key: format!("revoked.{}", r.code()) }));
        v
    }

    fn jargon(text: &str) -> Option<&'static str> {
        let lower = text.to_lowercase();
        JARGON.iter().copied().find(|j| lower.contains(j))
    }

    #[test]
    fn every_error_has_plain_text() {
        english();
        for e in all_errors() {
            let text = e.human_text();
            assert!(!text.is_empty(), "{e:?}");
            assert!(jargon(&text).is_none(), "служебное слово «{:?}» в тексте {e:?}: {text}", jargon(&text));
            assert!(!text.contains("0.5.0") && !text.contains("eof") && !text.contains("enoent"), "{text}");
        }
    }

    #[test]
    fn refusals_name_their_cause() {
        english();
        let t = |k: &str| key_text(&format!("refused.{k}"));
        assert_eq!(t("vital"), "Rexon refused: this package is needed by the system.");
        assert!(t("current-desktop").contains("using this desktop"));
        // Незнакомый ключ — общий отказ, сам ключ человеку не показан.
        assert_eq!(t("brand-new"), "Rexon refused to make this change — nothing was changed.");
        assert!(!key_text("revoked.idle").contains("idle"));
        assert!(key_text("revoked.idle").contains("15 minutes"));
    }

    #[test]
    fn build_mismatch_says_restart() {
        english();
        let text = ClientError::BuildMismatch { helper_build: None }.human_text();
        assert_eq!(text, "Rexon was updated — restart it to continue.");
        assert_eq!(ClientError::HelperOtherBuild { helper_build: "x".into() }.human_text(), text);
    }

    #[test]
    fn reason_codes_round_trip() {
        for r in REASONS {
            assert_eq!(Reason::from_code(r.code()), Some(r));
        }
        assert_eq!(Reason::from_code("nope"), None);
    }

    /// Каждая фраза (английский `msgid`) переведена в ru и de, перевод без
    /// служебных слов. Составная фраза отзыва проверяется по частям.
    #[test]
    fn texts_are_translated_without_jargon() {
        english();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../po");
        // Отказы «сеанс держится на пакете» — готовый текст с именами
        // сеансов; сверяются их шаблоны.
        let filled = |e: &ClientError| {
            matches!(e, ClientError::Refused { message_key } if message_key.starts_with("refused.orphan-in-use.")
                || message_key.starts_with("refused.orphan-login-default.")
                || message_key.starts_with(crate::system_plan::WAYLAND_SESSION_MISSING)
                || message_key.starts_with(crate::system_plan::SESSION_SET_ELSEWHERE))
        };
        let mut msgids: Vec<String> = all_errors()
            .iter()
            .filter(|e| !matches!(e, ClientError::Revoked { .. }) && !filled(e))
            .map(ClientError::human_text)
            .collect();
        msgids.extend(crate::update_guard::ORPHAN_SESSION_TEXTS.iter().map(|t| t.to_string()));
        msgids.extend(crate::system_plan::WAYLAND_DEFAULT_TEXTS.iter().map(|t| t.to_string()));
        msgids.extend(REASONS.iter().map(|r| r.message()));
        msgids.push("%1 Nothing was changed.".into());
        msgids.sort();
        msgids.dedup();
        for lang in ["ru", "de"] {
            let po = std::fs::read_to_string(root.join(format!("{lang}.po"))).expect("каталог читается");
            for id in &msgids {
                let Some(text) = crate::system_l10n::po_lookup(&po, id) else {
                    panic!("нет перевода {lang}: {id}");
                };
                assert!(jargon(&text).is_none(), "служебное слово в {lang}: {text}");
                if id.contains("%1") {
                    assert!(text.contains("%1"), "{lang}: {text}");
                }
            }
        }
    }
}
