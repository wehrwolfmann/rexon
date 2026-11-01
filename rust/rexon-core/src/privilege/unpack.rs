//! Приём файлов пользователя по дескриптору и распаковка (пункт Б5 плана,
//! см. [`super`]).
//!
//! # Правило (З2)
//!
//! Root-помощник НЕ открывает ни одного пути пользователя. Файл приходит
//! открытым дескриптором (`SCM_RIGHTS`, [`super::proto::recv_frame_with_fds`]),
//! который окно открыло от своего имени, и помощник:
//! 1. проверяет `fstat` дескриптора: только обычный файл — не каталог,
//!    устройство, канал, сокет;
//! 2. копирует содержимое (`pread` с нуля — общий с окном указатель позиции
//!    не влияет) в СВОЙ каталог root 0700 ([`WorkDir`]) с пределом размера;
//! 3. дальше работает только с копией: окно может менять свой файл сколько
//!    угодно, проверено и поставлено будет то, что лежит у root.
//!
//! # Каталоги (темы, раскладки экрана входа)
//!
//! Каталог по дескриптору не передаётся: обход чужого каталога от root —
//! та же дыра с подменой ссылкой, что закрывал коммит «своя копия 0700».
//! Окно упаковывает уже распакованный и проверенный у себя каталог в
//! НЕСЖАТЫЙ `ustar` ([`pack_tree`], [`pack_tree_memfd`]) и передаёт один
//! файл. Архивы, которые человек скачивает (zip, tar.gz/xz/zst, 7z),
//! по-прежнему распаковывает окно, от пользователя (`local_install`,
//! `appearance_installer`): ни разжатия, ни разбора трёх форматов от root
//! нет. Root читает ровно один простой формат — без внешних крейтов.
//!
//! Распаковка ([`unpack_tar_into`]) создаёт каждую запись через `openat2`
//! с `RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS | RESOLVE_NO_MAGICLINKS`
//! относительно каталога назначения, и с `O_EXCL`: ни ссылка, ни подмена
//! части пути, ни повтор записи не выведут запись наружу и не перепишут
//! уже распакованное.
//!
//! # Что на что отвечает
//!
//! | Что в архиве | Ответ |
//! |---|---|
//! | общий размер больше предела, размер одной записи больше | отказ [`UnpackError::TooLarge`] — по заголовку, до записи на диск |
//! | записей больше предела | отказ [`UnpackError::TooManyEntries`] |
//! | глубже предела | отказ [`UnpackError::TooDeep`] |
//! | символьная ссылка, жёсткая ссылка | отказ [`UnpackError::Link`] |
//! | устройство, канал, прочие типы (pax, GNU long name) | отказ [`UnpackError::Special`] / [`UnpackError::Unsupported`] |
//! | `..`, пустой или `.` сегмент | отказ [`UnpackError::DotDot`] / [`UnpackError::BadName`] |
//! | абсолютный путь | отказ [`UnpackError::Absolute`] |
//! | setuid / setgid бит | **отказ** [`UnpackError::SetId`]: наш упаковщик их не пишет, значит архив не от окна Rexon |
//! | прочие биты режима (sticky, запись для группы/всех) | снимаются: файл `0644`, исполняемый `0755`, каталог `0755`, владелец root |
//! | повтор имени | отказ [`UnpackError::Duplicate`] |
//! | ссылка на пути в каталоге назначения (гонка) | отказ [`UnpackError::Escape`] (`openat2` вернул `ELOOP`/`EXDEV`) |
//! | Plymouth: `.so` в теме | отказ при распаковке ([`validate::check_theme_file_not_code`]) |
//! | Plymouth: `ModuleName` не штатный (в т. ч. `../x`), NUL | отказ [`UnpackError::Theme`] ([`crate::installer::system_theme_problem`] по копии) |
//! | Plymouth: нет `<тема>.plymouth` в корне | отказ [`UnpackError::Theme`] |

use std::ffi::CString;
use std::fmt;
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use super::proto::ErrorCode;
use super::validate::{self, Refusal, SystemThemeKind};

/// Где root заводит свои рабочие каталоги — там же, где прежние помощники
/// (`mktemp -d /var/tmp/rexon-*-helper.XXXXXXXX`): не tmpfs, пакет на
/// гигабайт память не съест.
pub const WORK_BASE: &str = "/var/tmp";

/// Предел собранного пакета для `pacman -U`.
pub const MAX_PACKAGE_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// Пределы распаковки.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Сумма размеров файлов архива.
    pub max_total: u64,
    /// Число записей (файлы + каталоги).
    pub max_entries: usize,
    /// Число сегментов пути одной записи.
    pub max_depth: usize,
}

impl Limits {
    /// Тема SDDM/Plymouth, фон и раскладка экрана входа, раскладка MATE.
    /// Самые тяжёлые темы — видео-фоны SDDM на десятки мегабайт.
    pub const TREE: Limits = Limits {
        max_total: 512 * 1024 * 1024,
        max_entries: 20_000,
        max_depth: 24,
    };

    /// Предел самого файла архива: данные + заголовок и добивка на запись
    /// + конец архива.
    pub fn max_archive(&self) -> u64 {
        self.max_total + (self.max_entries as u64 + 4) * 2 * BLOCK as u64
    }
}

// ============================================================================
// Ошибки
// ============================================================================

/// Почему файл не принят. Всё, кроме [`UnpackError::Io`], — отказ проверки
/// (система не тронута); `Io` — сбой помощника.
#[derive(Debug)]
pub enum UnpackError {
    /// Дескриптор — не обычный файл (вид: `directory`, `fifo`, …).
    NotRegular(&'static str),
    /// Больше предела (байт — предел).
    TooLarge(u64),
    /// Записей больше предела.
    TooManyEntries(usize),
    /// Запись глубже предела.
    TooDeep(String),
    /// Символьная или жёсткая ссылка в архиве.
    Link(String),
    /// Устройство или канал в архиве.
    Special(String),
    /// Тип записи, который упаковщик Rexon не пишет (pax, GNU long name…).
    Unsupported(String),
    /// Сегмент `..`.
    DotDot(String),
    /// Абсолютный путь.
    Absolute(String),
    /// Пустой, `.`-сегмент, `//`, управляющий символ, не UTF-8.
    BadName(String),
    /// setuid/setgid в режиме записи.
    SetId(String),
    /// Повтор имени (файл или каталог уже есть).
    Duplicate(String),
    /// Путь записи уходит через ссылку или за корень (`openat2`).
    Escape(String),
    /// Порченый архив: контрольная сумма, обрыв, не ustar, нет родителя.
    Malformed(String),
    /// Тема не прошла проверку ядра (ссылка, `.so`, `ModuleName`).
    Theme(crate::installer::SystemThemeProblem),
    /// Файл или каталог, который должен быть только root: чужой владелец
    /// либо пишется группой/всеми (кэш pacman, [`open_root_file`]).
    NotRootOwned(String),
    /// Отказ проверки имени/категории ([`validate`]).
    Refused(Refusal),
    /// Ввод-вывод самого помощника.
    Io(io::Error),
}

impl UnpackError {
    /// Короткое имя для журнала (не переводится).
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotRegular(_) => "not-regular",
            Self::TooLarge(_) => "too-large",
            Self::TooManyEntries(_) => "too-many-entries",
            Self::TooDeep(_) => "too-deep",
            Self::Link(_) => "link",
            Self::Special(_) => "special",
            Self::Unsupported(_) => "unsupported",
            Self::DotDot(_) => "dot-dot",
            Self::Absolute(_) => "absolute",
            Self::BadName(_) => "bad-name",
            Self::SetId(_) => "set-id",
            Self::Duplicate(_) => "duplicate",
            Self::Escape(_) => "escape",
            Self::Malformed(_) => "malformed",
            Self::Theme(_) => "theme",
            Self::NotRootOwned(_) => "not-root-owned",
            Self::Refused(r) => r.code(),
            Self::Io(_) => "io",
        }
    }

    /// Код ответа окну ([`super::proto::HelperMsg::Error`]).
    pub fn error_code(&self) -> ErrorCode {
        match self {
            Self::Io(_) => ErrorCode::Internal,
            _ => ErrorCode::Refused,
        }
    }
}

impl fmt::Display for UnpackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotRegular(k) => write!(f, "передан не обычный файл ({k})"),
            Self::TooLarge(n) => write!(f, "больше предела {n} байт"),
            Self::TooManyEntries(n) => write!(f, "записей больше предела {n}"),
            Self::TooDeep(p) => write!(f, "запись слишком глубоко: {p}"),
            Self::Link(p) => write!(f, "ссылка в архиве: {p}"),
            Self::Special(p) => write!(f, "особый файл в архиве: {p}"),
            Self::Unsupported(p) => write!(f, "неподдерживаемая запись: {p}"),
            Self::DotDot(p) => write!(f, "«..» в пути: {p}"),
            Self::Absolute(p) => write!(f, "абсолютный путь: {p}"),
            Self::BadName(p) => write!(f, "недопустимое имя: {p}"),
            Self::SetId(p) => write!(f, "setuid/setgid: {p}"),
            Self::Duplicate(p) => write!(f, "повтор записи: {p}"),
            Self::Escape(p) => write!(f, "путь уходит за корень или через ссылку: {p}"),
            Self::Malformed(e) => write!(f, "порченый архив: {e}"),
            Self::Theme(p) => write!(f, "тема не принята: {p:?}"),
            Self::NotRootOwned(p) => write!(f, "не только root: {p}"),
            Self::Refused(r) => write!(f, "отказ проверки: {r:?}"),
            Self::Io(e) => write!(f, "ввод-вывод: {e}"),
        }
    }
}

impl std::error::Error for UnpackError {}

impl From<io::Error> for UnpackError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<Refusal> for UnpackError {
    fn from(r: Refusal) -> Self {
        Self::Refused(r)
    }
}

type Res<T> = Result<T, UnpackError>;

/// Строка для журнала: не длиннее 256 символов, управляющие — `?`.
fn clip(s: &str) -> String {
    s.chars()
        .take(256)
        .map(|c| if c.is_control() { '?' } else { c })
        .collect()
}

fn cstr(s: &str) -> Res<CString> {
    CString::new(s).map_err(|_| UnpackError::BadName(clip(s)))
}

// ============================================================================
// Свой каталог root 0700
// ============================================================================

/// Рабочий каталог помощника: `mkdtemp` (0700, владелец — сам помощник),
/// открыт дескриптором; при уничтожении удаляется со всем содержимым.
#[derive(Debug)]
pub struct WorkDir {
    path: PathBuf,
    fd: OwnedFd,
}

impl WorkDir {
    /// Завести каталог `<base>/rexon-priv.XXXXXX`. После создания
    /// проверяется по дескриптору: каталог, владелец — наш `euid`, режим
    /// ровно 0700 (иначе — ошибка, каталог убирается).
    pub fn create(base: &Path) -> io::Result<Self> {
        let mut template = base.as_os_str().as_bytes().to_vec();
        template.extend_from_slice(b"/rexon-priv.XXXXXX\0");
        // SAFETY: шаблон оканчивается NUL, mkdtemp пишет в него на месте.
        let p = unsafe { libc::mkdtemp(template.as_mut_ptr().cast()) };
        if p.is_null() {
            return Err(io::Error::last_os_error());
        }
        template.pop();
        let path = PathBuf::from(std::ffi::OsStr::from_bytes(&template));
        let fd = match open_dir_nofollow(&path) {
            Ok(fd) => fd,
            Err(e) => {
                let _ = std::fs::remove_dir(&path);
                return Err(e);
            }
        };
        let st = fstat(fd.as_fd())?;
        // SAFETY: geteuid не имеет предусловий.
        let euid = unsafe { libc::geteuid() };
        if st.st_mode & libc::S_IFMT != libc::S_IFDIR || st.st_uid != euid || st.st_mode & 0o7777 != 0o700 {
            let _ = std::fs::remove_dir(&path);
            return Err(io::Error::other("рабочий каталог не 0700 или не наш"));
        }
        Ok(Self { path, fd })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn as_fd(&self) -> BorrowedFd<'_> {
        self.fd.as_fd()
    }
}

impl Drop for WorkDir {
    fn drop(&mut self) {
        // remove_dir_all в std ссылок не разыменовывает.
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn open_dir_nofollow(path: &Path) -> io::Result<OwnedFd> {
    let c = CString::new(path.as_os_str().as_bytes()).map_err(io::Error::other)?;
    // SAFETY: строка с NUL на конце.
    let fd = unsafe {
        libc::open(
            c.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: новый дескриптор, владеем им одни.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

fn fstat(fd: BorrowedFd<'_>) -> io::Result<libc::stat> {
    // SAFETY: stat — простая C-структура; fstat заполняет её целиком.
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: живой fd и указатель на st.
    if unsafe { libc::fstat(fd.as_raw_fd(), &mut st) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(st)
}

// ============================================================================
// openat2
// ============================================================================

const RESOLVE_NO_MAGICLINKS: u64 = 0x02;
const RESOLVE_NO_SYMLINKS: u64 = 0x04;
const RESOLVE_BENEATH: u64 = 0x08;
/// Флаги разрешения пути для всего, что root создаёт при распаковке.
const RESOLVE_STRICT: u64 = RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS | RESOLVE_NO_MAGICLINKS;

/// `struct open_how` из `linux/openat2.h` (у `libc` она `non_exhaustive`).
#[repr(C)]
struct OpenHow {
    flags: u64,
    mode: u64,
    resolve: u64,
}

/// `openat2(dir, path, flags, mode, RESOLVE_STRICT)`. Обёртки в libc нет —
/// прямой системный вызов. Ядро старше 5.6 (`ENOSYS`) — ошибка `Io`:
/// распаковывать без `openat2` помощник не будет.
fn openat2(dir: BorrowedFd<'_>, path: &str, flags: libc::c_int, mode: u32) -> Res<OwnedFd> {
    let c = cstr(path)?;
    let how = OpenHow {
        flags: (flags | libc::O_CLOEXEC) as u64,
        mode: mode as u64,
        resolve: RESOLVE_STRICT,
    };
    loop {
        // SAFETY: строка с NUL, how живёт до конца вызова, размер — его.
        let fd = unsafe {
            libc::syscall(
                libc::SYS_openat2,
                dir.as_raw_fd(),
                c.as_ptr(),
                &how as *const OpenHow,
                std::mem::size_of::<OpenHow>(),
            )
        };
        if fd >= 0 {
            // SAFETY: новый дескриптор, владеем им одни.
            return Ok(unsafe { OwnedFd::from_raw_fd(fd as libc::c_int) });
        }
        let err = io::Error::last_os_error();
        match err.raw_os_error() {
            Some(libc::EINTR) | Some(libc::EAGAIN) => continue,
            Some(libc::ELOOP) | Some(libc::EXDEV) => return Err(UnpackError::Escape(clip(path))),
            Some(libc::EEXIST) => return Err(UnpackError::Duplicate(clip(path))),
            Some(libc::ENOENT) | Some(libc::ENOTDIR) => {
                return Err(UnpackError::Malformed(format!("нет каталога для {}", clip(path))))
            }
            _ => return Err(err.into()),
        }
    }
}

// ============================================================================
// Приём одного файла
// ============================================================================

fn file_kind(mode: libc::mode_t) -> &'static str {
    match mode & libc::S_IFMT {
        libc::S_IFREG => "file",
        libc::S_IFDIR => "directory",
        libc::S_IFCHR => "char-device",
        libc::S_IFBLK => "block-device",
        libc::S_IFIFO => "fifo",
        libc::S_IFSOCK => "socket",
        libc::S_IFLNK => "symlink",
        _ => "unknown",
    }
}

/// Скопировать содержимое `src` (обычного файла, проверено `fstat`) в новый
/// файл `name` рабочего каталога, не больше `max` байт. Возвращает путь
/// копии. `name` даёт сам помощник или проверка [`validate`].
pub fn receive_file(src: BorrowedFd<'_>, work: &WorkDir, name: &str, max: u64) -> Res<PathBuf> {
    let st = fstat(src)?;
    if st.st_mode & libc::S_IFMT != libc::S_IFREG {
        return Err(UnpackError::NotRegular(file_kind(st.st_mode)));
    }
    if st.st_size as u64 > max {
        return Err(UnpackError::TooLarge(max));
    }
    if name.is_empty() || name.contains('/') || name == "." || name == ".." {
        return Err(UnpackError::BadName(clip(name)));
    }
    let out = openat2(work.as_fd(), name, libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL, 0o600)?;
    let mut out = File::from(out);
    let mut buf = vec![0u8; 256 * 1024];
    let mut off: u64 = 0;
    loop {
        // pread: позиция окна в общем описании файла нам не мешает и
        // окном не сдвигается.
        // SAFETY: живой буфер, длина — его.
        let n = unsafe {
            libc::pread(src.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len(), off as libc::off_t)
        };
        if n < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(err.into());
        }
        if n == 0 {
            break;
        }
        off += n as u64;
        // Файл мог вырасти после fstat.
        if off > max {
            return Err(UnpackError::TooLarge(max));
        }
        out.write_all(&buf[..n as usize])?;
    }
    out.flush()?;
    Ok(work.path().join(name))
}

// ============================================================================
// Файл root (кэш pacman)
// ============================================================================

/// Файл или каталог, которому root может верить как своему: владелец root,
/// группа и все прочие писать не могут; файл — обычный (`want_dir` —
/// каталог). `what` — для журнала.
pub fn check_root_stat(mode: libc::mode_t, uid: libc::uid_t, want_dir: bool, what: &str) -> Res<()> {
    let kind = mode & libc::S_IFMT;
    let want = if want_dir { libc::S_IFDIR } else { libc::S_IFREG };
    if kind != want {
        return Err(UnpackError::NotRegular(file_kind(mode)));
    }
    if uid != 0 || mode & 0o022 != 0 {
        return Err(UnpackError::NotRootOwned(clip(what)));
    }
    Ok(())
}

/// Открыть файл `name` в каталоге `dir` (абсолютный путь) только для
/// чтения — так, чтобы ни одна ссылка не подменила ни путь каталога, ни сам
/// файл: каталог — `openat2` от `/` без ссылок, файл — `openat2` под ним
/// (`RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS`), `O_NONBLOCK` (канал не
/// повесит). И каталог, и файл — [`check_root_stat`]; файл не больше
/// [`MAX_PACKAGE_BYTES`]. Дальше помощник работает с копией по этому fd.
pub fn open_root_file(dir: &str, name: &str) -> Res<OwnedFd> {
    let rel = dir.strip_prefix('/').ok_or_else(|| UnpackError::Absolute(clip(dir)))?;
    if rel.is_empty() || rel.split('/').any(|s| s.is_empty() || s == "." || s == "..") {
        return Err(UnpackError::BadName(clip(dir)));
    }
    if name.is_empty() || name.contains('/') || name == "." || name == ".." {
        return Err(UnpackError::BadName(clip(name)));
    }
    let root = open_dir_nofollow(Path::new("/"))?;
    let d = openat2(root.as_fd(), rel, libc::O_RDONLY | libc::O_DIRECTORY, 0)?;
    let st = fstat(d.as_fd())?;
    check_root_stat(st.st_mode, st.st_uid, true, dir)?;
    open_root_file_in(d.as_fd(), name)
}

/// Файл `name` прямо в открытом каталоге `dir` — половина
/// [`open_root_file`] (каталог уже проверен).
fn open_root_file_in(dir: BorrowedFd<'_>, name: &str) -> Res<OwnedFd> {
    if name.is_empty() || name.contains('/') || name == "." || name == ".." {
        return Err(UnpackError::BadName(clip(name)));
    }
    let f = openat2(dir, name, libc::O_RDONLY | libc::O_NONBLOCK, 0)?;
    let st = fstat(f.as_fd())?;
    check_root_stat(st.st_mode, st.st_uid, false, name)?;
    if st.st_size as u64 > MAX_PACKAGE_BYTES {
        return Err(UnpackError::TooLarge(MAX_PACKAGE_BYTES));
    }
    Ok(f)
}

/// Собранный пакет для `pacman -U` (`PrivOp::InstallBuilt`): `label` —
/// путь на стороне окна, root его не открывает; по нему только
/// проверяется имя файла makepkg и место (`<rebuild_root>/<рецепт>/<файл>`,
/// [`validate::check_built_package`]; `rebuild_root` root-сторона считает
/// сама по `PKEXEC_UID`). Копия — `<work>/<имя файла>`, 0600, root.
pub fn receive_built_package(
    src: BorrowedFd<'_>,
    label: &str,
    rebuild_root: &str,
    work: &WorkDir,
) -> Res<PathBuf> {
    let name = validate::check_built_package(label, rebuild_root)?;
    receive_file(src, work, name, MAX_PACKAGE_BYTES)
}

/// Собранный AUR-помощником пакет (`PrivOp::AurInstallBuilt`, пункт Б10):
/// как [`receive_built_package`], но без корня пересборок — место файла
/// задают paru/yay ([`validate::check_aur_package`]). Копия ложится под
/// именем файла makepkg; `.PKGINFO` сверяет помощник по ней.
pub fn receive_aur_package(src: BorrowedFd<'_>, label: &str, work: &WorkDir) -> Res<PathBuf> {
    let name = validate::check_aur_package(label)?;
    receive_file(src, work, name, MAX_PACKAGE_BYTES)
}

// ============================================================================
// ustar
// ============================================================================

const BLOCK: usize = 512;
const USTAR_MAGIC: &[u8] = b"ustar\0";
const USTAR_VERSION: &[u8] = b"00";

/// Разобранный заголовок.
#[derive(Debug)]
struct Header {
    name: String,
    typeflag: u8,
    mode: u32,
    size: u64,
}

fn field_str(b: &[u8]) -> &[u8] {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    &b[..end]
}

fn parse_octal(b: &[u8], what: &str) -> Res<u64> {
    let t: Vec<u8> = field_str(b).iter().copied().filter(|&c| c != b' ').collect();
    if t.is_empty() {
        return Ok(0);
    }
    if !t.iter().all(|c| (b'0'..=b'7').contains(c)) {
        return Err(UnpackError::Malformed(format!("поле {what} не восьмеричное")));
    }
    u64::from_str_radix(std::str::from_utf8(&t).unwrap_or("0"), 8)
        .map_err(|_| UnpackError::Malformed(format!("поле {what}")))
}

fn checksum(h: &[u8; BLOCK]) -> u64 {
    h.iter()
        .enumerate()
        .map(|(i, &c)| if (148..156).contains(&i) { b' ' as u64 } else { c as u64 })
        .sum()
}

/// `None` — нулевой блок (конец архива).
fn parse_header(h: &[u8; BLOCK]) -> Res<Option<Header>> {
    if h.iter().all(|&c| c == 0) {
        return Ok(None);
    }
    if parse_octal(&h[148..156], "chksum")? != checksum(h) {
        return Err(UnpackError::Malformed("контрольная сумма заголовка".into()));
    }
    if &h[257..263] != USTAR_MAGIC || &h[263..265] != USTAR_VERSION {
        return Err(UnpackError::Malformed("не POSIX ustar".into()));
    }
    let name = field_str(&h[0..100]);
    let prefix = field_str(&h[345..500]);
    let mut full = Vec::with_capacity(prefix.len() + 1 + name.len());
    if !prefix.is_empty() {
        full.extend_from_slice(prefix);
        full.push(b'/');
    }
    full.extend_from_slice(name);
    let name = String::from_utf8(full)
        .map_err(|e| UnpackError::BadName(String::from_utf8_lossy(e.as_bytes()).into_owned()))?;
    Ok(Some(Header {
        name,
        typeflag: h[156],
        mode: parse_octal(&h[100..108], "mode")? as u32,
        size: parse_octal(&h[124..136], "size")?,
    }))
}

/// Сегменты пути записи; `None` — запись самого корня (`.`, `./`).
fn entry_segments<'a>(name: &'a str, is_dir: bool, max_depth: usize) -> Res<Option<Vec<&'a str>>> {
    if name.is_empty() || name.chars().any(|c| c.is_control()) {
        return Err(UnpackError::BadName(clip(name)));
    }
    if name.starts_with('/') {
        return Err(UnpackError::Absolute(clip(name)));
    }
    let mut rel = name.strip_prefix("./").unwrap_or(name);
    if rel.is_empty() || rel == "." {
        return if is_dir { Ok(None) } else { Err(UnpackError::BadName(clip(name))) };
    }
    if let Some(r) = rel.strip_suffix('/') {
        if !is_dir {
            return Err(UnpackError::BadName(clip(name)));
        }
        rel = r;
    }
    let segs: Vec<&str> = rel.split('/').collect();
    if segs.contains(&"..") {
        return Err(UnpackError::DotDot(clip(name)));
    }
    if segs.iter().any(|s| s.is_empty() || *s == ".") {
        return Err(UnpackError::BadName(clip(name)));
    }
    if segs.len() > max_depth {
        return Err(UnpackError::TooDeep(clip(name)));
    }
    Ok(Some(segs))
}

/// Открыть каталог-родитель записи под `dest` (`openat2`, без ссылок).
fn open_parent(dest: BorrowedFd<'_>, segs: &[&str]) -> Res<OwnedFd> {
    let parent = segs[..segs.len() - 1].join("/");
    let path = if parent.is_empty() { "." } else { parent.as_str() };
    openat2(dest, path, libc::O_PATH | libc::O_DIRECTORY, 0)
}

fn mkdir_beneath(dest: BorrowedFd<'_>, segs: &[&str], full: &str) -> Res<()> {
    let parent = open_parent(dest, segs)?;
    let last = cstr(segs[segs.len() - 1])?;
    // SAFETY: строка с NUL, живой fd родителя.
    if unsafe { libc::mkdirat(parent.as_raw_fd(), last.as_ptr(), 0o700) } != 0 {
        let err = io::Error::last_os_error();
        return Err(match err.raw_os_error() {
            Some(libc::EEXIST) => UnpackError::Duplicate(clip(full)),
            _ => err.into(),
        });
    }
    // Режим — по дескриптору только что созданного каталога, не по пути.
    let dir = openat2(parent.as_fd(), segs[segs.len() - 1], libc::O_RDONLY | libc::O_DIRECTORY, 0)?;
    // SAFETY: живой fd.
    if unsafe { libc::fchmod(dir.as_raw_fd(), 0o755) } != 0 {
        return Err(io::Error::last_os_error().into());
    }
    Ok(())
}

/// Распаковать несжатый ustar из `archive` в каталог `dest`.
///
/// `check_file` зовётся на путь каждого файла ДО его создания (Plymouth —
/// [`validate::check_theme_file_not_code`]). Пределы `limits` проверяются по
/// заголовку, до записи данных. Режимы нормализуются (см. описание модуля);
/// setuid/setgid — отказ. При ошибке распакованное остаётся в `dest` —
/// убирает его владелец каталога ([`WorkDir`]).
pub fn unpack_tar_into(
    archive: &mut impl Read,
    dest: BorrowedFd<'_>,
    limits: Limits,
    check_file: &dyn Fn(&str) -> Result<(), Refusal>,
) -> Res<usize> {
    let mut entries = 0usize;
    let mut total = 0u64;
    let mut block = [0u8; BLOCK];
    loop {
        read_block(archive, &mut block)?;
        let Some(h) = parse_header(&block)? else {
            // Конец: второй нулевой блок обязателен (иначе архив обрезан).
            read_block(archive, &mut block)?;
            if block.iter().any(|&c| c != 0) {
                return Err(UnpackError::Malformed("за концом архива данные".into()));
            }
            return Ok(entries);
        };
        entries += 1;
        if entries > limits.max_entries {
            return Err(UnpackError::TooManyEntries(limits.max_entries));
        }
        let name = h.name.as_str();
        match h.typeflag {
            b'0' | 0 | b'5' => {}
            b'1' | b'2' => return Err(UnpackError::Link(clip(name))),
            b'3' | b'4' | b'6' => return Err(UnpackError::Special(clip(name))),
            t => {
                return Err(UnpackError::Unsupported(format!("{} (тип {:?})", clip(name), t as char)))
            }
        }
        if h.mode & 0o6000 != 0 {
            return Err(UnpackError::SetId(clip(name)));
        }
        let is_dir = h.typeflag == b'5';
        let segs = entry_segments(name, is_dir, limits.max_depth)?;
        if is_dir {
            if h.size != 0 {
                return Err(UnpackError::Malformed(format!("каталог с данными: {}", clip(name))));
            }
            if let Some(segs) = segs {
                mkdir_beneath(dest, &segs, name)?;
            }
            continue;
        }
        let segs = segs.expect("файл всегда с сегментами");
        let rel = segs.join("/");
        check_file(&rel)?;
        total = total.saturating_add(h.size);
        if h.size > limits.max_total || total > limits.max_total {
            return Err(UnpackError::TooLarge(limits.max_total));
        }
        let parent = open_parent(dest, &segs)?;
        let out = openat2(
            parent.as_fd(),
            segs[segs.len() - 1],
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
            0o600,
        )?;
        let mut out = File::from(out);
        let copied = io::copy(&mut archive.by_ref().take(h.size), &mut out)?;
        if copied != h.size {
            return Err(UnpackError::Malformed(format!("обрыв данных {}", clip(name))));
        }
        let mode = if h.mode & 0o111 != 0 { 0o755 } else { 0o644 };
        // SAFETY: живой fd.
        if unsafe { libc::fchmod(out.as_raw_fd(), mode) } != 0 {
            return Err(io::Error::last_os_error().into());
        }
        let pad = (BLOCK - (h.size as usize % BLOCK)) % BLOCK;
        if pad > 0 {
            let mut skip = [0u8; BLOCK];
            read_exact_or_malformed(archive, &mut skip[..pad])?;
        }
    }
}

fn read_block(r: &mut impl Read, b: &mut [u8; BLOCK]) -> Res<()> {
    read_exact_or_malformed(r, b)
}

fn read_exact_or_malformed(r: &mut impl Read, b: &mut [u8]) -> Res<()> {
    r.read_exact(b).map_err(|e| match e.kind() {
        io::ErrorKind::UnexpectedEof => UnpackError::Malformed("архив обрезан".into()),
        _ => UnpackError::Io(e),
    })
}

// ============================================================================
// Приём каталога
// ============================================================================

/// Принять архив-каталог по дескриптору: копия в `work`, распаковка в
/// `<work>/tree`, копия архива удаляется. Возвращает путь `tree`.
pub fn receive_tree(
    src: BorrowedFd<'_>,
    work: &WorkDir,
    limits: Limits,
    check_file: &dyn Fn(&str) -> Result<(), Refusal>,
) -> Res<PathBuf> {
    let archive = receive_file(src, work, "bundle.tar", limits.max_archive())?;
    let tree = cstr("tree")?;
    // SAFETY: строка с NUL, живой fd рабочего каталога.
    if unsafe { libc::mkdirat(work.as_fd().as_raw_fd(), tree.as_ptr(), 0o755) } != 0 {
        return Err(io::Error::last_os_error().into());
    }
    let dest = openat2(work.as_fd(), "tree", libc::O_RDONLY | libc::O_DIRECTORY, 0)?;
    let file = openat2(work.as_fd(), "bundle.tar", libc::O_RDONLY, 0)?;
    let mut reader = io::BufReader::new(File::from(file));
    let res = unpack_tar_into(&mut reader, dest.as_fd(), limits, check_file);
    let _ = std::fs::remove_file(&archive);
    res?;
    Ok(work.path().join("tree"))
}

/// Раскладка экрана входа и MATE (`PrivOp::Stage`): категория — из
/// [`crate::system_plan::STAGE_CATEGORIES`]. Что внутри должно лежать
/// (одна картинка, тема webkit) — решает исполнение категории (Б4).
pub fn receive_stage(src: BorrowedFd<'_>, work: &WorkDir, category: &str) -> Res<PathBuf> {
    validate::check_stage_category(category)?;
    receive_tree(src, work, Limits::TREE, &no_check)
}

/// Проверка «без особых правил» для [`receive_tree`].
fn no_check(_: &str) -> Result<(), Refusal> {
    Ok(())
}

/// Системная тема SDDM/Plymouth (`SystemThemeOp::Install`). Plymouth:
/// `.so` — отказ ещё при распаковке; после неё — та же проверка ядра, что
/// у прежнего помощника и окна ([`crate::installer::system_theme_problem`]:
/// ссылки, `.so`, каждое `ModuleName` — только штатный модуль, без NUL), но
/// по КОПИИ root; `<тема>.plymouth` в корне темы обязателен.
pub fn receive_system_theme(
    src: BorrowedFd<'_>,
    work: &WorkDir,
    kind: SystemThemeKind,
    theme: &str,
) -> Res<PathBuf> {
    validate::check_item_name(theme)?;
    let plymouth = kind == SystemThemeKind::Plymouth;
    let check: fn(&str) -> Result<(), Refusal> =
        if plymouth { validate::check_theme_file_not_code } else { no_check };
    let tree = receive_tree(src, work, Limits::TREE, &check)?;
    let kind_str = if plymouth { "plymouth" } else { "sddm" };
    if let Some(p) = crate::installer::system_theme_problem(kind_str, &tree, Some(theme)) {
        return Err(UnpackError::Theme(p));
    }
    if plymouth {
        let main = format!("{theme}.plymouth");
        let st = std::fs::symlink_metadata(tree.join(&main));
        if !st.map(|m| m.file_type().is_file()).unwrap_or(false) {
            return Err(UnpackError::Theme(crate::installer::SystemThemeProblem::BootSplashCode(main)));
        }
    }
    Ok(tree)
}

// ============================================================================
// Упаковка (сторона окна)
// ============================================================================

/// Заголовок ustar. `name` — до 255 байт (100 имени + 155 префикса по
/// границе `/`), иначе ошибка.
fn ustar_header(name: &str, typeflag: u8, mode: u32, size: u64, linkname: &str) -> io::Result<[u8; BLOCK]> {
    let mut h = [0u8; BLOCK];
    let nb = name.as_bytes();
    let (prefix, base): (&[u8], &[u8]) = if nb.len() <= 100 {
        (&[], nb)
    } else {
        let cut = nb
            .iter()
            .enumerate()
            .filter(|&(i, &c)| c == b'/' && i <= 155 && nb.len() - i - 1 <= 100 && nb.len() - i - 1 > 0)
            .map(|(i, _)| i)
            .next_back()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, format!("имя длиннее ustar: {name}")))?;
        (&nb[..cut], &nb[cut + 1..])
    };
    if linkname.len() > 100 {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "ссылка длиннее ustar"));
    }
    h[..base.len()].copy_from_slice(base);
    h[100..108].copy_from_slice(format!("{mode:07o}\0").as_bytes());
    h[108..116].copy_from_slice(b"0000000\0");
    h[116..124].copy_from_slice(b"0000000\0");
    if size >= 8u64.pow(11) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "файл больше предела ustar"));
    }
    h[124..136].copy_from_slice(format!("{size:011o}\0").as_bytes());
    h[136..148].copy_from_slice(b"00000000000\0");
    h[156] = typeflag;
    h[157..157 + linkname.len()].copy_from_slice(linkname.as_bytes());
    h[257..263].copy_from_slice(USTAR_MAGIC);
    h[263..265].copy_from_slice(USTAR_VERSION);
    h[345..345 + prefix.len()].copy_from_slice(prefix);
    let sum = checksum(&h);
    h[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
    Ok(h)
}

/// Упаковать каталог `dir` в несжатый ustar (сторона окна, от
/// пользователя). Только обычные файлы и каталоги — ссылка или особый файл
/// дают ошибку (`InvalidData`): тема не обязана их содержать, и root их всё
/// равно не примет. Режимы нормализуются (`0644`/`0755`), владелец 0,
/// время 0, порядок — по имени: одинаковый каталог даёт одинаковые байты.
pub fn pack_tree(dir: &Path, out: &mut impl Write) -> io::Result<()> {
    fn walk(root: &Path, rel: &str, out: &mut dyn Write) -> io::Result<()> {
        let abs = if rel.is_empty() { root.to_path_buf() } else { root.join(rel) };
        let mut names: Vec<String> = std::fs::read_dir(&abs)?
            .map(|e| {
                e.and_then(|e| {
                    e.file_name()
                        .into_string()
                        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "имя не UTF-8"))
                })
            })
            .collect::<io::Result<_>>()?;
        names.sort();
        for n in names {
            let r = if rel.is_empty() { n.clone() } else { format!("{rel}/{n}") };
            let md = std::fs::symlink_metadata(root.join(&r))?;
            let ft = md.file_type();
            if ft.is_dir() {
                out.write_all(&ustar_header(&format!("{r}/"), b'5', 0o755, 0, "")?)?;
                walk(root, &r, out)?;
            } else if ft.is_file() {
                use std::os::unix::fs::PermissionsExt;
                let mode = if md.permissions().mode() & 0o111 != 0 { 0o755 } else { 0o644 };
                let mut f = File::open(root.join(&r))?;
                let size = md.len();
                out.write_all(&ustar_header(&r, b'0', mode, size, "")?)?;
                let copied = io::copy(&mut (&mut f).take(size), out)?;
                if copied != size {
                    return Err(io::Error::new(io::ErrorKind::UnexpectedEof, format!("файл изменился: {r}")));
                }
                let pad = (BLOCK - (size as usize % BLOCK)) % BLOCK;
                out.write_all(&[0u8; BLOCK][..pad])?;
            } else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("ссылка или особый файл: {r}"),
                ));
            }
        }
        Ok(())
    }
    walk(dir, "", out)?;
    out.write_all(&[0u8; 2 * BLOCK])?;
    out.flush()
}

/// [`pack_tree`] в анонимный файл в памяти (`memfd`, CLOEXEC) — его
/// дескриптор окно и отдаёт помощнику в `SCM_RIGHTS`.
pub fn pack_tree_memfd(dir: &Path) -> io::Result<OwnedFd> {
    // SAFETY: строка-литерал с NUL.
    let fd = unsafe { libc::memfd_create(c"rexon-bundle".as_ptr(), libc::MFD_CLOEXEC) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: новый дескриптор, владеем им одни.
    let fd = unsafe { OwnedFd::from_raw_fd(fd) };
    let mut f = io::BufWriter::new(File::from(fd.try_clone()?));
    pack_tree(dir, &mut f)?;
    drop(f);
    Ok(fd)
}

// ============================================================================
// Сверки
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn base() -> PathBuf {
        std::env::temp_dir()
    }

    fn small() -> Limits {
        Limits { max_total: 64 * 1024, max_entries: 16, max_depth: 4 }
    }

    /// Собрать архив из записей (имя, тип, режим, данные, ссылка).
    fn tar(entries: &[(&str, u8, u32, &[u8], &str)]) -> Vec<u8> {
        let mut v = Vec::new();
        for (name, t, mode, data, link) in entries {
            v.extend_from_slice(&ustar_header(name, *t, *mode, data.len() as u64, link).unwrap());
            v.extend_from_slice(data);
            v.resize(v.len().div_ceil(BLOCK) * BLOCK, 0);
        }
        v.extend_from_slice(&[0u8; 2 * BLOCK]);
        v
    }

    fn file_e<'a>(name: &'a str, data: &'a [u8]) -> (&'a str, u8, u32, &'a [u8], &'a str) {
        (name, b'0', 0o644, data, "")
    }

    fn dir_e(name: &str) -> (&str, u8, u32, &[u8], &str) {
        (name, b'5', 0o755, b"", "")
    }

    /// Распаковать байты в новый рабочий каталог.
    fn unpack(bytes: &[u8], limits: Limits) -> (WorkDir, Res<usize>) {
        let work = WorkDir::create(&base()).unwrap();
        let r = unpack_tar_into(&mut &bytes[..], work.as_fd(), limits, &|_| Ok(()));
        (work, r)
    }

    fn memfd_with(bytes: &[u8]) -> OwnedFd {
        // SAFETY: литерал с NUL.
        let fd = unsafe { libc::memfd_create(c"t".as_ptr(), libc::MFD_CLOEXEC) };
        assert!(fd >= 0);
        // SAFETY: новый fd.
        let fd = unsafe { OwnedFd::from_raw_fd(fd) };
        File::from(fd.try_clone().unwrap()).write_all(bytes).unwrap();
        fd
    }

    #[test]
    fn workdir_is_0700_and_removed() {
        let path;
        {
            let w = WorkDir::create(&base()).unwrap();
            path = w.path().to_path_buf();
            let md = std::fs::symlink_metadata(&path).unwrap();
            assert!(md.is_dir());
            assert_eq!(md.permissions().mode() & 0o7777, 0o700);
            std::fs::write(path.join("x"), b"1").unwrap();
        }
        assert!(!path.exists());
    }

    #[test]
    fn pack_and_unpack_round_trip_normalizes_modes() {
        let src = WorkDir::create(&base()).unwrap();
        let s = src.path();
        std::fs::create_dir_all(s.join("images/sub")).unwrap();
        std::fs::write(s.join("neon.plymouth"), b"[Plymouth Theme]\nModuleName=script\n").unwrap();
        std::fs::write(s.join("images/sub/a.png"), vec![7u8; 1500]).unwrap();
        std::fs::write(s.join("run.sh"), b"#!/bin/sh\n").unwrap();
        std::fs::set_permissions(s.join("run.sh"), std::fs::Permissions::from_mode(0o775)).unwrap();
        std::fs::set_permissions(s.join("neon.plymouth"), std::fs::Permissions::from_mode(0o666)).unwrap();
        let fd = pack_tree_memfd(s).unwrap();
        let work = WorkDir::create(&base()).unwrap();
        let tree = receive_tree(fd.as_fd(), &work, small(), &|_| Ok(())).unwrap();
        assert_eq!(std::fs::read(tree.join("images/sub/a.png")).unwrap(), vec![7u8; 1500]);
        let mode = |p: &str| std::fs::metadata(tree.join(p)).unwrap().permissions().mode() & 0o7777;
        assert_eq!(mode("run.sh"), 0o755);
        assert_eq!(mode("neon.plymouth"), 0o644, "запись для всех снимается");
        assert_eq!(mode("images"), 0o755);
        assert!(!work.path().join("bundle.tar").exists(), "копия архива убрана");
        // Тот же каталог — те же байты.
        let (mut a, mut b) = (Vec::new(), Vec::new());
        pack_tree(s, &mut a).unwrap();
        pack_tree(s, &mut b).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn pack_refuses_symlink_in_source() {
        let src = WorkDir::create(&base()).unwrap();
        std::os::unix::fs::symlink("/etc/passwd", src.path().join("l")).unwrap();
        let e = pack_tree(src.path(), &mut Vec::new()).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn bomb_total_size_refused_before_write() {
        // Один файл «объявляет» больше предела — отказ по заголовку.
        let mut v = ustar_header("big", b'0', 0o644, 1024 * 1024, "").unwrap().to_vec();
        v.extend_from_slice(&[0u8; 4 * BLOCK]);
        let (work, r) = unpack(&v, small());
        assert!(matches!(r, Err(UnpackError::TooLarge(_))), "{r:?}");
        assert!(!work.path().join("big").exists(), "ничего не записано");
        // Сумма мелких больше предела.
        let chunk = vec![1u8; 20 * 1024];
        let names: Vec<String> = (0..5).map(|i| format!("f{i}")).collect();
        let entries: Vec<_> = names.iter().map(|n| file_e(n, &chunk)).collect();
        let (_w, r) = unpack(&tar(&entries), small());
        assert!(matches!(r, Err(UnpackError::TooLarge(_))), "{r:?}");
    }

    #[test]
    fn bomb_entry_count_refused() {
        let names: Vec<String> = (0..17).map(|i| format!("d{i}/")).collect();
        let entries: Vec<_> = names.iter().map(|n| dir_e(n)).collect();
        let (_w, r) = unpack(&tar(&entries), small());
        assert!(matches!(r, Err(UnpackError::TooManyEntries(16))), "{r:?}");
        let ok: Vec<_> = names[..16].iter().map(|n| dir_e(n)).collect();
        assert_eq!(unpack(&tar(&ok), small()).1.unwrap(), 16);
    }

    #[test]
    fn depth_refused() {
        let (_w, r) = unpack(&tar(&[file_e("a/b/c/d/e", b"x")]), small());
        assert!(matches!(r, Err(UnpackError::TooDeep(_))), "{r:?}");
    }

    #[test]
    fn links_refused() {
        let (_w, r) = unpack(&tar(&[("evil", b'2', 0o777, b"", "/etc/passwd")]), small());
        assert!(matches!(r, Err(UnpackError::Link(_))), "симлинк: {r:?}");
        let (_w, r) = unpack(&tar(&[file_e("a", b"x"), ("b", b'1', 0o644, b"", "a")]), small());
        assert!(matches!(r, Err(UnpackError::Link(_))), "жёсткая ссылка: {r:?}");
    }

    #[test]
    fn dotdot_and_absolute_refused() {
        for n in ["../x", "a/../../x", "a/..", "./../x"] {
            let (_w, r) = unpack(&tar(&[dir_e("a/"), file_e(n, b"x")]), small());
            assert!(matches!(r, Err(UnpackError::DotDot(_))), "{n}: {r:?}");
        }
        for n in ["/etc/x", "/tmp/rexon-evil"] {
            let (_w, r) = unpack(&tar(&[file_e(n, b"x")]), small());
            assert!(matches!(r, Err(UnpackError::Absolute(_))), "{n}: {r:?}");
        }
        for n in ["a//b", "a/./b", ".", "x\ny"] {
            let (_w, r) = unpack(&tar(&[dir_e("a/"), file_e(n, b"x")]), small());
            assert!(matches!(r, Err(UnpackError::BadName(_))), "{n}: {r:?}");
        }
    }

    #[test]
    fn devices_fifo_and_unknown_types_refused() {
        for t in [b'3', b'4', b'6'] {
            let (_w, r) = unpack(&tar(&[("dev", t, 0o644, b"", "")]), small());
            assert!(matches!(r, Err(UnpackError::Special(_))), "{}: {r:?}", t as char);
        }
        for t in [b'x', b'g', b'L', b'K', b'7'] {
            let (_w, r) = unpack(&tar(&[("p", t, 0o644, b"", "")]), small());
            assert!(matches!(r, Err(UnpackError::Unsupported(_))), "{}: {r:?}", t as char);
        }
    }

    #[test]
    fn setid_refused() {
        for mode in [0o4755, 0o2755, 0o6644] {
            let (_w, r) = unpack(&tar(&[("x", b'0', mode, b"x", "")]), small());
            assert!(matches!(r, Err(UnpackError::SetId(_))), "{mode:o}: {r:?}");
        }
        // sticky и запись для всех — не отказ, снимаются.
        let (w, r) = unpack(&tar(&[("x", b'0', 0o1777, b"x", "")]), small());
        r.unwrap();
        assert_eq!(std::fs::metadata(w.path().join("x")).unwrap().permissions().mode() & 0o7777, 0o755);
    }

    #[test]
    fn duplicates_refused() {
        let (_w, r) = unpack(&tar(&[file_e("a", b"1"), file_e("a", b"2")]), small());
        assert!(matches!(r, Err(UnpackError::Duplicate(_))), "{r:?}");
        let (_w, r) = unpack(&tar(&[dir_e("d/"), dir_e("d/")]), small());
        assert!(matches!(r, Err(UnpackError::Duplicate(_))), "{r:?}");
    }

    /// Ссылка, оказавшаяся в каталоге назначения (гонка, чужой каталог):
    /// запись через неё — отказ, снаружи ничего не появилось.
    #[test]
    fn symlink_in_destination_does_not_escape() {
        let outside = WorkDir::create(&base()).unwrap();
        let work = WorkDir::create(&base()).unwrap();
        std::os::unix::fs::symlink(outside.path(), work.path().join("link")).unwrap();
        for n in ["link/x", "link"] {
            let r = unpack_tar_into(&mut &tar(&[file_e(n, b"evil")])[..], work.as_fd(), small(), &|_| Ok(()));
            assert!(matches!(r, Err(UnpackError::Escape(_)) | Err(UnpackError::Duplicate(_))), "{n}: {r:?}");
        }
        let r = unpack_tar_into(&mut &tar(&[dir_e("link/sub/")])[..], work.as_fd(), small(), &|_| Ok(()));
        assert!(matches!(r, Err(UnpackError::Escape(_))), "{r:?}");
        assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
    }

    #[test]
    fn malformed_archives_refused() {
        // Обрыв данных.
        let mut v = ustar_header("a", b'0', 0o644, 100, "").unwrap().to_vec();
        v.extend_from_slice(b"short");
        let (_w, r) = unpack(&v, small());
        assert!(matches!(r, Err(UnpackError::Malformed(_))), "{r:?}");
        // Нет конца архива.
        let mut v = tar(&[file_e("a", b"x")]);
        v.truncate(v.len() - 2 * BLOCK);
        assert!(matches!(unpack(&v, small()).1, Err(UnpackError::Malformed(_))));
        // Порченая сумма.
        let mut v = tar(&[file_e("a", b"x")]);
        v[0] = b'b';
        assert!(matches!(unpack(&v, small()).1, Err(UnpackError::Malformed(_))));
        // GNU-заголовок вместо POSIX.
        let mut h = ustar_header("a", b'0', 0o644, 0, "").unwrap();
        h[257..265].copy_from_slice(b"ustar  \0");
        let sum = checksum(&h);
        h[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
        let mut v = h.to_vec();
        v.extend_from_slice(&[0u8; 2 * BLOCK]);
        assert!(matches!(unpack(&v, small()).1, Err(UnpackError::Malformed(_))));
        // Файл без каталога-родителя.
        assert!(matches!(unpack(&tar(&[file_e("no/x", b"1")]), small()).1, Err(UnpackError::Malformed(_))));
    }

    #[test]
    fn long_names_via_prefix() {
        // Сегмент длиннее 100 ustar не хранит; длинный путь — через префикс.
        let dir = format!("{}/{}/", "d".repeat(80), "e".repeat(60));
        let long = format!("{dir}{}", "f".repeat(90));
        let top = format!("{}/", "d".repeat(80));
        let entries = [dir_e(&top), dir_e(&dir), file_e(&long, b"ok")];
        let (w, r) = unpack(&tar(&entries), small());
        r.unwrap();
        assert_eq!(std::fs::read(w.path().join(&long)).unwrap(), b"ok");
        // Не делится на префикс и имя — упаковщик отказывает.
        assert!(ustar_header(&"x".repeat(101), b'0', 0o644, 0, "").is_err());
    }

    #[test]
    fn receive_refuses_non_regular() {
        let work = WorkDir::create(&base()).unwrap();
        let dir = File::open(work.path()).unwrap();
        assert!(matches!(receive_file(dir.as_fd(), &work, "x", 10), Err(UnpackError::NotRegular("directory"))));
        let mut p = [0; 2];
        // SAFETY: массив из двух int.
        assert_eq!(unsafe { libc::pipe2(p.as_mut_ptr(), libc::O_CLOEXEC) }, 0);
        // SAFETY: новые fd.
        let (r, _w) = unsafe { (OwnedFd::from_raw_fd(p[0]), OwnedFd::from_raw_fd(p[1])) };
        assert!(matches!(receive_file(r.as_fd(), &work, "x", 10), Err(UnpackError::NotRegular("fifo"))));
        let null = File::open("/dev/null").unwrap();
        assert!(matches!(receive_file(null.as_fd(), &work, "x", 10), Err(UnpackError::NotRegular("char-device"))));
        let (a, _b) = super::super::proto::seqpacket_pair().unwrap();
        assert!(matches!(receive_file(a.as_fd(), &work, "x", 10), Err(UnpackError::NotRegular("socket"))));
        // Ничего не создано.
        assert_eq!(std::fs::read_dir(work.path()).unwrap().count(), 0);
    }

    #[test]
    fn receive_file_size_limit_and_offset() {
        let work = WorkDir::create(&base()).unwrap();
        let fd = memfd_with(&[5u8; 100]);
        assert!(matches!(receive_file(fd.as_fd(), &work, "big", 99), Err(UnpackError::TooLarge(99))));
        // Позиция окна сдвинута в конец — копия всё равно целиком (pread с 0).
        let fd = memfd_with(&[5u8; 100]);
        let p = receive_file(fd.as_fd(), &work, "ok", 100).unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), vec![5u8; 100]);
        assert_eq!(std::fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o600);
        assert!(matches!(receive_file(fd.as_fd(), &work, "ok", 100), Err(UnpackError::Duplicate(_))));
        for bad in ["", "a/b", "..", "."] {
            assert!(matches!(receive_file(fd.as_fd(), &work, bad, 100), Err(UnpackError::BadName(_))));
        }
    }

    #[test]
    fn built_package_copied_by_label_checks() {
        let work = WorkDir::create(&base()).unwrap();
        let root = "/home/u/.cache/rexon/rebuild";
        let fd = memfd_with(b"zstd-bytes");
        let p = receive_built_package(
            fd.as_fd(),
            "/home/u/.cache/rexon/rebuild/deepin/deepin-session-treeland-1.0-1-x86_64.pkg.tar.zst",
            root,
            &work,
        )
        .unwrap();
        assert_eq!(p, work.path().join("deepin-session-treeland-1.0-1-x86_64.pkg.tar.zst"));
        assert_eq!(std::fs::read(&p).unwrap(), b"zstd-bytes");
        for bad in [
            "/tmp/x.deb",
            "/home/u/.cache/rexon/rebuild/deepin/x.deb",
            "/home/u/.cache/rexon/rebuild/x.pkg.tar.zst",
            "/home/u/.cache/rexon/rebuild/a/../b/x.pkg.tar.zst",
            "/var/cache/pacman/pkg/linux-6.0-1-x86_64.pkg.tar.zst",
        ] {
            let r = receive_built_package(fd.as_fd(), bad, root, &work);
            assert!(matches!(r, Err(UnpackError::Refused(_))), "{bad}: {r:?}");
        }
    }

    fn theme_fd(files: &[(&str, &[u8])]) -> OwnedFd {
        let entries: Vec<_> = files.iter().map(|(n, d)| file_e(n, d)).collect();
        memfd_with(&tar(&entries))
    }

    #[test]
    fn plymouth_theme_checks() {
        let good = b"[Plymouth Theme]\nName=Neon\nModuleName=script\n" as &[u8];
        let work = WorkDir::create(&base()).unwrap();
        let tree = receive_system_theme(theme_fd(&[("neon.plymouth", good)]).as_fd(), &work, SystemThemeKind::Plymouth, "neon").unwrap();
        assert!(tree.join("neon.plymouth").is_file());

        let cases: &[&[(&str, &[u8])]] = &[
            &[("neon.plymouth", b"[Plymouth Theme]\nModuleName=../x\n")],
            &[("neon.plymouth", b"[Plymouth Theme]\nModuleName=../../../share/plymouth/themes/neon/evil\n")],
            &[("neon.plymouth", b"[Plymouth Theme]\nModuleName\n=../x\n")],
            &[("neon.plymouth", b"[Plymouth Theme]\nModuleName=script\0\n")],
            &[("neon.plymouth", good), ("other.plymouth", b"modulename=../x\n")],
            &[("neon.plymouth", b"[Plymouth Theme]\nName=Neon\n")],
            &[("other.plymouth", good)],
        ];
        for files in cases {
            let work = WorkDir::create(&base()).unwrap();
            let r = receive_system_theme(theme_fd(files).as_fd(), &work, SystemThemeKind::Plymouth, "neon");
            assert!(matches!(r, Err(UnpackError::Theme(_))), "{files:?}: {r:?}");
        }
        for so in ["evil.so", "lib/evil.so.1"] {
            let work = WorkDir::create(&base()).unwrap();
            let entries = [dir_e("lib/"), file_e("neon.plymouth", good), file_e(so, b"\x7fELF")];
            let r = receive_system_theme(memfd_with(&tar(&entries)).as_fd(), &work, SystemThemeKind::Plymouth, "neon");
            assert!(matches!(r, Err(UnpackError::Refused(Refusal::BootSplashCode(_)))), "{so}: {r:?}");
            assert!(!work.path().join("tree").join(so).exists(), ".so не записан");
        }
        // Имя темы — имя, а не путь.
        let work = WorkDir::create(&base()).unwrap();
        let r = receive_system_theme(theme_fd(&[("neon.plymouth", good)]).as_fd(), &work, SystemThemeKind::Plymouth, "../neon");
        assert!(matches!(r, Err(UnpackError::Refused(_))), "{r:?}");
    }

    #[test]
    fn sddm_theme_and_stage() {
        let work = WorkDir::create(&base()).unwrap();
        let fd = theme_fd(&[("Main.qml", b"Item {}"), ("metadata.desktop", b"[SddmGreeterTheme]\n")]);
        receive_system_theme(fd.as_fd(), &work, SystemThemeKind::Sddm, "rexon").unwrap();
        let work = WorkDir::create(&base()).unwrap();
        let tree = receive_stage(theme_fd(&[("bg.png", b"\x89PNG")]).as_fd(), &work, "gdm-bg").unwrap();
        assert!(tree.join("bg.png").is_file());
        let work = WorkDir::create(&base()).unwrap();
        let r = receive_stage(theme_fd(&[("bg.png", b"x")]).as_fd(), &work, "../etc");
        assert!(matches!(r, Err(UnpackError::Refused(_))), "{r:?}");
    }

    #[test]
    fn error_codes() {
        assert_eq!(UnpackError::Link("x".into()).error_code(), ErrorCode::Refused);
        assert_eq!(UnpackError::Io(io::Error::other("x")).error_code(), ErrorCode::Internal);
        assert_eq!(UnpackError::Refused(Refusal::Empty).code(), "empty");
        assert_eq!(UnpackError::NotRootOwned("x".into()).code(), "not-root-owned");
    }

    /// Владелец и права файла root: только root, группа и прочие не пишут.
    #[test]
    fn root_stat_only_root_not_writable_by_others() {
        let reg = libc::S_IFREG;
        assert!(check_root_stat(reg | 0o644, 0, false, "f").is_ok());
        assert!(check_root_stat(reg | 0o600, 0, false, "f").is_ok());
        assert!(check_root_stat(libc::S_IFDIR | 0o755, 0, true, "d").is_ok());
        for mode in [0o664, 0o646, 0o666, 0o777] {
            assert!(matches!(check_root_stat(reg | mode, 0, false, "f"), Err(UnpackError::NotRootOwned(_))), "{mode:o}");
        }
        assert!(matches!(check_root_stat(reg | 0o644, 1000, false, "f"), Err(UnpackError::NotRootOwned(_))));
        assert!(matches!(check_root_stat(libc::S_IFDIR | 0o775, 0, true, "d"), Err(UnpackError::NotRootOwned(_))));
        assert!(matches!(check_root_stat(libc::S_IFIFO | 0o644, 0, false, "f"), Err(UnpackError::NotRegular("fifo"))));
        assert!(matches!(check_root_stat(libc::S_IFDIR | 0o755, 0, false, "f"), Err(UnpackError::NotRegular(_))));
        assert!(matches!(check_root_stat(reg | 0o644, 0, true, "d"), Err(UnpackError::NotRegular(_))));
    }

    /// Живая файловая система (без root): ссылка на месте файла и в пути
    /// каталога — отказ ещё до проверки владельца; файл пользователя — отказ
    /// по владельцу; относительный путь и имя с `/` — отказ.
    #[test]
    fn root_file_open_refuses_links_and_foreign_owner() {
        use std::os::unix::fs::symlink;
        let dir = base().join(format!("rexon-root-file-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("real")).unwrap();
        std::fs::write(dir.join("real/a-1-1-x86_64.pkg.tar.zst"), b"x").unwrap();
        symlink(dir.join("real/a-1-1-x86_64.pkg.tar.zst"), dir.join("real/link-1-1-x86_64.pkg.tar.zst")).unwrap();
        symlink(dir.join("real"), dir.join("via")).unwrap();
        let real = dir.join("real").to_string_lossy().into_owned();
        let via = dir.join("via").to_string_lossy().into_owned();
        // Ссылка в пути каталога — до любой другой проверки.
        let r = open_root_file(&via, "a-1-1-x86_64.pkg.tar.zst");
        assert!(matches!(r, Err(UnpackError::Escape(_))), "{r:?}");
        // Тест не от root: каталог пользователя не принят по владельцу.
        if unsafe { libc::geteuid() } != 0 {
            let r = open_root_file(&real, "a-1-1-x86_64.pkg.tar.zst");
            assert!(matches!(r, Err(UnpackError::NotRootOwned(_))), "{r:?}");
        }
        // Файл в уже открытом каталоге: ссылка — отказ, файл пользователя —
        // по владельцу, каталог вместо файла — не обычный.
        let d = open_dir_nofollow(&dir.join("real")).unwrap();
        let r = open_root_file_in(d.as_fd(), "link-1-1-x86_64.pkg.tar.zst");
        assert!(matches!(r, Err(UnpackError::Escape(_))), "{r:?}");
        if unsafe { libc::geteuid() } != 0 {
            let r = open_root_file_in(d.as_fd(), "a-1-1-x86_64.pkg.tar.zst");
            assert!(matches!(r, Err(UnpackError::NotRootOwned(_))), "{r:?}");
        }
        let top = open_dir_nofollow(&dir).unwrap();
        let r = open_root_file_in(top.as_fd(), "real");
        assert!(matches!(r, Err(UnpackError::NotRegular("directory"))), "{r:?}");
        assert!(matches!(open_root_file("var/cache", "a"), Err(UnpackError::Absolute(_))));
        assert!(matches!(open_root_file(&real, "../a"), Err(UnpackError::BadName(_))));
        assert!(matches!(open_root_file("/var/../etc", "a"), Err(UnpackError::BadName(_))));
        assert!(open_root_file(&real, "none").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
