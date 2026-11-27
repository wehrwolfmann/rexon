//! Receiving user files by descriptor and unpacking (item B5 of the plan, see
//! [`super`]).
//!
//! # The rule (Z2)
//!
//! The root helper does NOT open a single user path. A file arrives as an open
//! descriptor (`SCM_RIGHTS`, [`super::proto::recv_frame_with_fds`]) that the
//! window opened under its own identity, and the helper:
//! 1. checks the descriptor with `fstat`: only a regular file — not a directory,
//!    device, pipe or socket;
//! 2. copies the contents (`pread` from zero — the file position pointer shared
//!    with the window has no effect) into ITS OWN root 0700 directory
//!    ([`WorkDir`]) with a size limit;
//! 3. from then on works only with the copy: the window may change its file as
//!    much as it likes, what gets checked and installed is what lies with root.
//!
//! # Directories (themes, login screen layouts)
//!
//! A directory is not passed by descriptor: walking someone else's directory as
//! root is the same symlink-substitution hole that the "own 0700 copy" commit
//! closed. The window packs the directory it has already unpacked and checked
//! on its side into an UNCOMPRESSED `ustar` ([`pack_tree`], [`pack_tree_memfd`])
//! and passes a single file. Archives the person downloads (zip,
//! tar.gz/xz/zst, 7z) are still unpacked by the window, as the user
//! (`local_install`, `appearance_installer`): there is no decompression and no
//! parsing of three formats as root. Root reads exactly one simple format —
//! without external crates.
//!
//! Unpacking ([`unpack_tar_into`]) creates each entry via `openat2` with
//! `RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS | RESOLVE_NO_MAGICLINKS` relative to
//! the destination directory, and with `O_EXCL`: neither a symlink, nor
//! substitution of a path component, nor a repeated entry will take an entry
//! outside or overwrite what was already unpacked.
//!
//! # What answers what
//!
//! | What is in the archive | Answer |
//! |---|---|
//! | total size over the limit, size of one entry over the limit | refusal [`UnpackError::TooLarge`] — by the header, before writing to disk |
//! | more entries than the limit | refusal [`UnpackError::TooManyEntries`] |
//! | deeper than the limit | refusal [`UnpackError::TooDeep`] |
//! | symbolic link, hard link | refusal [`UnpackError::Link`] |
//! | device, pipe, other types (pax, GNU long name) | refusal [`UnpackError::Special`] / [`UnpackError::Unsupported`] |
//! | `..`, an empty or `.` segment | refusal [`UnpackError::DotDot`] / [`UnpackError::BadName`] |
//! | absolute path | refusal [`UnpackError::Absolute`] |
//! | setuid / setgid bit | **refusal** [`UnpackError::SetId`]: our packer does not write them, so the archive is not from the Rexon window |
//! | other mode bits (sticky, write for group/others) | stripped: file `0644`, executable `0755`, directory `0755`, owner root |
//! | repeated name | refusal [`UnpackError::Duplicate`] |
//! | a symlink on the path in the destination directory (a race) | refusal [`UnpackError::Escape`] (`openat2` returned `ELOOP`/`EXDEV`) |
//! | Plymouth: a `.so` in the theme | refusal during unpacking ([`validate::check_theme_file_not_code`]) |
//! | Plymouth: `ModuleName` not a stock one (including `../x`), NUL | refusal [`UnpackError::Theme`] ([`crate::installer::system_theme_problem`] on the copy) |
//! | Plymouth: no `<theme>.plymouth` at the root | refusal [`UnpackError::Theme`] |

use std::ffi::CString;
use std::fmt;
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use super::proto::ErrorCode;
use super::validate::{self, Refusal, SystemThemeKind};

/// Where root creates its working directories — the same place as the former
/// helpers (`mktemp -d /var/tmp/rexon-*-helper.XXXXXXXX`): not tmpfs, so a
/// gigabyte package will not eat memory.
pub const WORK_BASE: &str = "/var/tmp";

/// Limit of a built package for `pacman -U`.
pub const MAX_PACKAGE_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// Unpacking limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Sum of the sizes of the archive's files.
    pub max_total: u64,
    /// Number of entries (files + directories).
    pub max_entries: usize,
    /// Number of path segments of one entry.
    pub max_depth: usize,
}

impl Limits {
    /// SDDM/Plymouth theme, login screen background and layout, MATE layout. The
    /// heaviest themes are SDDM video backgrounds of tens of megabytes.
    pub const TREE: Limits = Limits {
        max_total: 512 * 1024 * 1024,
        max_entries: 20_000,
        max_depth: 24,
    };

    /// Limit of the archive file itself: data + header and padding per entry + the
    /// end of the archive.
    pub fn max_archive(&self) -> u64 {
        self.max_total + (self.max_entries as u64 + 4) * 2 * BLOCK as u64
    }
}

// ============================================================================
// Errors
// ============================================================================

/// Why the file was not accepted. Everything except [`UnpackError::Io`] is a
/// check refusal (the system is untouched); `Io` is a helper failure.
#[derive(Debug)]
pub enum UnpackError {
    /// The descriptor is not a regular file (kind: `directory`, `fifo`, …).
    NotRegular(&'static str),
    /// Over the limit (bytes — the limit).
    TooLarge(u64),
    /// More entries than the limit.
    TooManyEntries(usize),
    /// An entry deeper than the limit.
    TooDeep(String),
    /// A symbolic or hard link in the archive.
    Link(String),
    /// A device or pipe in the archive.
    Special(String),
    /// An entry type the Rexon packer does not write (pax, GNU long name…).
    Unsupported(String),
    /// A `..` segment.
    DotDot(String),
    /// An absolute path.
    Absolute(String),
    /// An empty or `.` segment, `//`, a control character, not UTF-8.
    BadName(String),
    /// setuid/setgid in an entry's mode.
    SetId(String),
    /// A repeated name (the file or directory already exists).
    Duplicate(String),
    /// The entry's path goes through a symlink or beyond the root (`openat2`).
    Escape(String),
    /// A corrupted archive: checksum, truncation, not ustar, no parent.
    Malformed(String),
    /// The theme did not pass the core check (a symlink, `.so`, `ModuleName`).
    Theme(crate::installer::SystemThemeProblem),
    /// A file or directory that must belong only to root: a foreign owner or
    /// writable by group/others (the pacman cache, [`open_root_file`]).
    NotRootOwned(String),
    /// Refusal of the name/category check ([`validate`]).
    Refused(Refusal),
    /// Input/output of the helper itself.
    Io(io::Error),
}

impl UnpackError {
    /// Short name for the log (not translated).
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

    /// Answer code to the window ([`super::proto::HelperMsg::Error`]).
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

/// A line for the log: no longer than 256 characters, control characters as `?`.
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
// Root's own 0700 directory
// ============================================================================

/// The helper's working directory: `mkdtemp` (0700, owned by the helper itself),
/// open as a descriptor; on destruction it is removed with all its contents.
#[derive(Debug)]
pub struct WorkDir {
    path: PathBuf,
    fd: OwnedFd,
}

impl WorkDir {
    /// Create a `<base>/rexon-priv.XXXXXX` directory. After creation it is checked
    /// via the descriptor: a directory, owned by our `euid`, mode exactly 0700
    /// (otherwise an error, and the directory is removed).
    pub fn create(base: &Path) -> io::Result<Self> {
        let mut template = base.as_os_str().as_bytes().to_vec();
        template.extend_from_slice(b"/rexon-priv.XXXXXX\0");
        // SAFETY: the template ends with NUL, mkdtemp writes into it in place.
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
        // SAFETY: geteuid has no preconditions.
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
        // remove_dir_all in std does not follow symlinks.
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn open_dir_nofollow(path: &Path) -> io::Result<OwnedFd> {
    let c = CString::new(path.as_os_str().as_bytes()).map_err(io::Error::other)?;
    // SAFETY: a NUL-terminated string.
    let fd = unsafe {
        libc::open(
            c.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: a new descriptor, we alone own it.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

fn fstat(fd: BorrowedFd<'_>) -> io::Result<libc::stat> {
    // SAFETY: stat is a plain C structure; fstat fills it entirely.
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: a live fd and a pointer to st.
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
/// Path resolution flags for everything root creates while unpacking.
const RESOLVE_STRICT: u64 = RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS | RESOLVE_NO_MAGICLINKS;

/// `struct open_how` from `linux/openat2.h` (in `libc` it is `non_exhaustive`).
#[repr(C)]
struct OpenHow {
    flags: u64,
    mode: u64,
    resolve: u64,
}

/// `openat2(dir, path, flags, mode, RESOLVE_STRICT)`. libc has no wrapper — a
/// direct system call. A kernel older than 5.6 (`ENOSYS`) is an `Io` error: the
/// helper will not unpack without `openat2`.
fn openat2(dir: BorrowedFd<'_>, path: &str, flags: libc::c_int, mode: u32) -> Res<OwnedFd> {
    let c = cstr(path)?;
    let how = OpenHow {
        flags: (flags | libc::O_CLOEXEC) as u64,
        mode: mode as u64,
        resolve: RESOLVE_STRICT,
    };
    loop {
        // SAFETY: a NUL-terminated string, how lives until the end of the call, the size is its own.
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
            // SAFETY: a new descriptor, we alone own it.
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
// Receiving one file
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

/// Copy the contents of `src` (a regular file, checked with `fstat`) into a new
/// file `name` of the working directory, no more than `max` bytes. Returns the
/// path of the copy. `name` is given by the helper itself or by the [`validate`]
/// check.
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
        // pread: the window's position in the shared file description does not bother
        // us and is not moved by the window.
        // SAFETY: a live buffer, the length is its own.
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
        // The file may have grown after fstat.
        if off > max {
            return Err(UnpackError::TooLarge(max));
        }
        out.write_all(&buf[..n as usize])?;
    }
    out.flush()?;
    Ok(work.path().join(name))
}

// ============================================================================
// A root file (the pacman cache)
// ============================================================================

/// A file or directory root can trust as its own: owned by root, the group and
/// others cannot write; a file is regular (`want_dir` — a directory). `what` —
/// for the log.
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

/// Open file `name` in directory `dir` (an absolute path) read-only — so that no
/// symlink can substitute either the directory's path or the file itself: the
/// directory via `openat2` from `/` without symlinks, the file via `openat2`
/// beneath it (`RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS`), `O_NONBLOCK` (a pipe
/// will not hang it). Both the directory and the file go through
/// [`check_root_stat`]; the file is no larger than [`MAX_PACKAGE_BYTES`]. Then
/// the helper works with a copy via this fd.
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

/// File `name` directly in the open directory `dir` — half of
/// [`open_root_file`] (the directory has already been checked).
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

/// A built package for `pacman -U` (`PrivOp::InstallBuilt`): `label` — the path
/// on the window's side, root does not open it; only the makepkg file name and
/// the location are checked by it (`<rebuild_root>/<recipe>/<file>`,
/// [`validate::check_built_package`]; the root side computes `rebuild_root`
/// itself from `PKEXEC_UID`). The copy is `<work>/<file name>`, 0600, root.
pub fn receive_built_package(
    src: BorrowedFd<'_>,
    label: &str,
    rebuild_root: &str,
    work: &WorkDir,
) -> Res<PathBuf> {
    let name = validate::check_built_package(label, rebuild_root)?;
    receive_file(src, work, name, MAX_PACKAGE_BYTES)
}

/// A package built by the AUR helper (`PrivOp::AurInstallBuilt`, item B10): like
/// [`receive_built_package`], but without the rebuild root — the file's location
/// is set by paru/yay ([`validate::check_aur_package`]). The copy is stored
/// under the makepkg file name; the helper checks `.PKGINFO` against it.
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

/// A parsed header.
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

/// `None` — a zero block (end of the archive).
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

/// Path segments of an entry; `None` — an entry of the root itself (`.`, `./`).
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

/// Open the parent directory of an entry under `dest` (`openat2`, without symlinks).
fn open_parent(dest: BorrowedFd<'_>, segs: &[&str]) -> Res<OwnedFd> {
    let parent = segs[..segs.len() - 1].join("/");
    let path = if parent.is_empty() { "." } else { parent.as_str() };
    openat2(dest, path, libc::O_PATH | libc::O_DIRECTORY, 0)
}

fn mkdir_beneath(dest: BorrowedFd<'_>, segs: &[&str], full: &str) -> Res<()> {
    let parent = open_parent(dest, segs)?;
    let last = cstr(segs[segs.len() - 1])?;
    // SAFETY: a NUL-terminated string, a live fd of the parent.
    if unsafe { libc::mkdirat(parent.as_raw_fd(), last.as_ptr(), 0o700) } != 0 {
        let err = io::Error::last_os_error();
        return Err(match err.raw_os_error() {
            Some(libc::EEXIST) => UnpackError::Duplicate(clip(full)),
            _ => err.into(),
        });
    }
    // The mode is taken from the descriptor of the directory just created, not from the path.
    let dir = openat2(parent.as_fd(), segs[segs.len() - 1], libc::O_RDONLY | libc::O_DIRECTORY, 0)?;
    // SAFETY: a live fd.
    if unsafe { libc::fchmod(dir.as_raw_fd(), 0o755) } != 0 {
        return Err(io::Error::last_os_error().into());
    }
    Ok(())
}

/// Unpack an uncompressed ustar from `archive` into directory `dest`.
///
/// `check_file` is called on each file's path BEFORE it is created (Plymouth —
/// [`validate::check_theme_file_not_code`]). The `limits` are checked by the
/// header, before writing data. Modes are normalized (see the module
/// description); setuid/setgid is a refusal. On an error what was unpacked stays
/// in `dest` — the directory's owner removes it ([`WorkDir`]).
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
            // End: a second zero block is mandatory (otherwise the archive is truncated).
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
        // SAFETY: a live fd.
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
// Receiving a directory
// ============================================================================

/// Receive a directory archive by descriptor: a copy into `work`, unpacking into
/// `<work>/tree`, the archive copy is deleted. Returns the `tree` path.
pub fn receive_tree(
    src: BorrowedFd<'_>,
    work: &WorkDir,
    limits: Limits,
    check_file: &dyn Fn(&str) -> Result<(), Refusal>,
) -> Res<PathBuf> {
    let archive = receive_file(src, work, "bundle.tar", limits.max_archive())?;
    let tree = cstr("tree")?;
    // SAFETY: a NUL-terminated string, a live fd of the working directory.
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

/// Login screen and MATE layout (`PrivOp::Stage`): the category comes from
/// [`crate::system_plan::STAGE_CATEGORIES`]. What must lie inside (one picture,
/// a webkit theme) is decided by the category's execution (B4).
pub fn receive_stage(src: BorrowedFd<'_>, work: &WorkDir, category: &str) -> Res<PathBuf> {
    validate::check_stage_category(category)?;
    receive_tree(src, work, Limits::TREE, &no_check)
}

/// A check "without special rules" for [`receive_tree`].
fn no_check(_: &str) -> Result<(), Refusal> {
    Ok(())
}

/// An SDDM/Plymouth system theme (`SystemThemeOp::Install`). Plymouth: a `.so`
/// is refused already during unpacking; after it — the same core check as for
/// the former helper and the window ([`crate::installer::system_theme_problem`]:
/// symlinks, `.so`, every `ModuleName` only a stock module, no NUL), but on
/// root's COPY; `<theme>.plymouth` at the theme root is mandatory.
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
// Packing (the window's side)
// ============================================================================

/// A ustar header. `name` — up to 255 bytes (100 of name + 155 of prefix split
/// at a `/`), otherwise an error.
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

/// Pack directory `dir` into an uncompressed ustar (the window's side, as the
/// user). Only regular files and directories — a symlink or a special file gives
/// an error (`InvalidData`): a theme need not contain them, and root would not
/// accept them anyway. Modes are normalized (`0644`/`0755`), owner 0, time 0,
/// order by name: the same directory gives the same bytes.
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

/// [`pack_tree`] into an anonymous in-memory file (`memfd`, CLOEXEC) — the
/// window passes its descriptor to the helper in `SCM_RIGHTS`.
pub fn pack_tree_memfd(dir: &Path) -> io::Result<OwnedFd> {
    // SAFETY: a string literal with NUL.
    let fd = unsafe { libc::memfd_create(c"rexon-bundle".as_ptr(), libc::MFD_CLOEXEC) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: a new descriptor, we alone own it.
    let fd = unsafe { OwnedFd::from_raw_fd(fd) };
    let mut f = io::BufWriter::new(File::from(fd.try_clone()?));
    pack_tree(dir, &mut f)?;
    drop(f);
    Ok(fd)
}

// ============================================================================
// Checks
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

    /// Build an archive from entries (name, type, mode, data, link).
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

    /// Unpack bytes into a new working directory.
    fn unpack(bytes: &[u8], limits: Limits) -> (WorkDir, Res<usize>) {
        let work = WorkDir::create(&base()).unwrap();
        let r = unpack_tar_into(&mut &bytes[..], work.as_fd(), limits, &|_| Ok(()));
        (work, r)
    }

    fn memfd_with(bytes: &[u8]) -> OwnedFd {
        // SAFETY: a literal with NUL.
        let fd = unsafe { libc::memfd_create(c"t".as_ptr(), libc::MFD_CLOEXEC) };
        assert!(fd >= 0);
        // SAFETY: a new fd.
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
        // The same directory — the same bytes.
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
        // One file "declares" more than the limit — refusal by the header.
        let mut v = ustar_header("big", b'0', 0o644, 1024 * 1024, "").unwrap().to_vec();
        v.extend_from_slice(&[0u8; 4 * BLOCK]);
        let (work, r) = unpack(&v, small());
        assert!(matches!(r, Err(UnpackError::TooLarge(_))), "{r:?}");
        assert!(!work.path().join("big").exists(), "ничего не записано");
        // The sum of small ones is over the limit.
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
        // sticky and write-for-all are not a refusal, they are stripped.
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

    /// A symlink that turned up in the destination directory (a race, a foreign
    /// directory): writing through it is refused, nothing appeared outside.
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
        // Truncated data.
        let mut v = ustar_header("a", b'0', 0o644, 100, "").unwrap().to_vec();
        v.extend_from_slice(b"short");
        let (_w, r) = unpack(&v, small());
        assert!(matches!(r, Err(UnpackError::Malformed(_))), "{r:?}");
        // No end of the archive.
        let mut v = tar(&[file_e("a", b"x")]);
        v.truncate(v.len() - 2 * BLOCK);
        assert!(matches!(unpack(&v, small()).1, Err(UnpackError::Malformed(_))));
        // A corrupted checksum.
        let mut v = tar(&[file_e("a", b"x")]);
        v[0] = b'b';
        assert!(matches!(unpack(&v, small()).1, Err(UnpackError::Malformed(_))));
        // A GNU header instead of POSIX.
        let mut h = ustar_header("a", b'0', 0o644, 0, "").unwrap();
        h[257..265].copy_from_slice(b"ustar  \0");
        let sum = checksum(&h);
        h[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
        let mut v = h.to_vec();
        v.extend_from_slice(&[0u8; 2 * BLOCK]);
        assert!(matches!(unpack(&v, small()).1, Err(UnpackError::Malformed(_))));
        // A file without a parent directory.
        assert!(matches!(unpack(&tar(&[file_e("no/x", b"1")]), small()).1, Err(UnpackError::Malformed(_))));
    }

    #[test]
    fn long_names_via_prefix() {
        // ustar does not store a segment longer than 100; a long path goes through the prefix.
        let dir = format!("{}/{}/", "d".repeat(80), "e".repeat(60));
        let long = format!("{dir}{}", "f".repeat(90));
        let top = format!("{}/", "d".repeat(80));
        let entries = [dir_e(&top), dir_e(&dir), file_e(&long, b"ok")];
        let (w, r) = unpack(&tar(&entries), small());
        r.unwrap();
        assert_eq!(std::fs::read(w.path().join(&long)).unwrap(), b"ok");
        // Cannot be split into a prefix and a name — the packer refuses.
        assert!(ustar_header(&"x".repeat(101), b'0', 0o644, 0, "").is_err());
    }

    #[test]
    fn receive_refuses_non_regular() {
        let work = WorkDir::create(&base()).unwrap();
        let dir = File::open(work.path()).unwrap();
        assert!(matches!(receive_file(dir.as_fd(), &work, "x", 10), Err(UnpackError::NotRegular("directory"))));
        let mut p = [0; 2];
        // SAFETY: an array of two ints.
        assert_eq!(unsafe { libc::pipe2(p.as_mut_ptr(), libc::O_CLOEXEC) }, 0);
        // SAFETY: new fds.
        let (r, _w) = unsafe { (OwnedFd::from_raw_fd(p[0]), OwnedFd::from_raw_fd(p[1])) };
        assert!(matches!(receive_file(r.as_fd(), &work, "x", 10), Err(UnpackError::NotRegular("fifo"))));
        let null = File::open("/dev/null").unwrap();
        assert!(matches!(receive_file(null.as_fd(), &work, "x", 10), Err(UnpackError::NotRegular("char-device"))));
        let (a, _b) = super::super::proto::seqpacket_pair().unwrap();
        assert!(matches!(receive_file(a.as_fd(), &work, "x", 10), Err(UnpackError::NotRegular("socket"))));
        // Nothing was created.
        assert_eq!(std::fs::read_dir(work.path()).unwrap().count(), 0);
    }

    #[test]
    fn receive_file_size_limit_and_offset() {
        let work = WorkDir::create(&base()).unwrap();
        let fd = memfd_with(&[5u8; 100]);
        assert!(matches!(receive_file(fd.as_fd(), &work, "big", 99), Err(UnpackError::TooLarge(99))));
        // The window's position is moved to the end — the copy is still complete (pread from 0).
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
        // The theme name is a name, not a path.
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

    /// Owner and permissions of a root file: only root, group and others do not write.
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

    /// The live file system (without root): a symlink in place of the file and in
    /// the directory path is refused even before the owner check; a user's file is
    /// refused by owner; a relative path and a name with `/` are refused.
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
        // A symlink in the directory path — before any other check.
        let r = open_root_file(&via, "a-1-1-x86_64.pkg.tar.zst");
        assert!(matches!(r, Err(UnpackError::Escape(_))), "{r:?}");
        // The test is not run as root: the user's directory is not accepted by owner.
        if unsafe { libc::geteuid() } != 0 {
            let r = open_root_file(&real, "a-1-1-x86_64.pkg.tar.zst");
            assert!(matches!(r, Err(UnpackError::NotRootOwned(_))), "{r:?}");
        }
        // A file in an already open directory: a symlink is refused, a user's file by
        // owner, a directory instead of a file as not regular.
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
