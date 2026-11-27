//! The privilege session client (item B7 of the plan, see [`super`]): the
//! window's side.
//!
//! # What it does
//!
//! 1. **Finds the helper** [`HELPER_NAME`] among [`helper_candidates`] and
//!    checks the ownership chain ([`check_ownership`]): the file and EVERY
//!    directory of the path belong to root, not writable by group and others,
//!    the file is regular and executable. A helper started via `pkexec` is root
//!    for anyone who managed to substitute the file or a directory above it; so
//!    with a foreign owner the session is not raised. No helper, or an
//!    unacceptable one, are the only cases when the caller may take the "as
//!    now" path ([`ClientError::raw_fallback_allowed`], rule Z5 in
//!    [`validate::raw_fallback_allowed`]).
//! 2. **Raises the session** with a single `pkexec`: a `socketpair`
//!    ([`proto::seqpacket_pair`], both ends with `CLOEXEC`), the helper's end on
//!    fd 0 and 1 of the child via the [`Spawner`]; our own copy of that end is
//!    closed right after the launch. No other descendant of the window inherits
//!    the socket (Z3): the originals have `CLOEXEC`, and only `dup2` inside the
//!    child being launched puts the socket on fd 0/1. Then [`Hello`] → we wait
//!    for `Welcome`. While the person is entering the password, `Hello` lies in
//!    the socket; if they closed the password dialog, `pkexec` exits with 126, if
//!    not authorized — 127, and that is [`ClientError::NotAuthorized`]: the
//!    person's refusal, not a reason to silently take the raw path.
//! 3. **Holds the session** in a dedicated "privilege session" thread: a request
//!    queue, one operation at a time (the helper takes one at a time), a stream
//!    of `Progress` to the caller, [`Session::cancel`], [`Session::shutdown`]
//!    (the helper understands a closed socket as "the window was closed": it
//!    finishes what is running and exits).
//!
//! # Contract with the helper (B4)
//!
//! `pkexec <helper> [--session <XDG_SESSION_ID>]`, the socket on fd 0 and 1.
//! The logind session number goes as an argument because `pkexec` clears the
//! environment, and the protocol ([`proto`]) carries nothing but operations
//! after `Hello`. If there is no valid number ([`revoke::valid_session_id`]),
//! there is no flag at all. The number only NARROWS the helper's privileges (by
//! it the helper watches for lock and session changes); it cannot widen them.
//!
//! # Errors — without a raw path
//!
//! A broken connection, a corrupted frame, an unexpected message — the session
//! is dead, the outcome of the running operation is unknown
//! ([`ClientError::SessionLost`]). `BuildMismatch` — "Rexon was updated, restart
//! it" (decision of 28.10): the client remembers this and no longer raises the
//! session. `Refused` — a check refusal. `Revoked` — the privilege was revoked;
//! the next request will raise the session again (a new password). None of
//! these errors allows a raw `pkexec`.

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

/// Name of the session helper binary.
pub const HELPER_NAME: &str = "rexon-priv-helper";

/// Directory of helpers outside `/usr` (SteamOS, Steam depot) — the same as
/// [`crate::steamos_sysext::HELPER_PATH`]: `/var` survives updates.
pub const STEAMOS_HELPER_DIR: &str = "/var/lib/rexon/bin";

/// What starts the helper.
pub const PKEXEC: &str = "pkexec";

/// The helper's flag before the logind session number (contract with B4).
pub const SESSION_FLAG: &str = "--session";

/// The helper's message language flag (contract with B4).
pub const LANG_FLAG: &str = "--lang";

/// `pkexec` exit code: the person closed the password dialog.
pub const PKEXEC_DISMISSED: i32 = 126;

/// `pkexec` exit code: not authorized (wrong password, no polkit agent).
pub const PKEXEC_NOT_AUTHORIZED: i32 = 127;

/// Write for group and others.
const GROUP_OTHER_WRITE: u32 = 0o022;

/// All helper locations in decreasing order of preference: the package, a
/// manual installation ([`plan::HELPER_DIRS`]), then the SteamOS directory.
pub fn helper_candidates() -> Vec<PathBuf> {
    plan::HELPER_DIRS
        .iter()
        .copied()
        .chain([STEAMOS_HELPER_DIR])
        .map(|dir| PathBuf::from(plan::helper_path_in(dir, HELPER_NAME)))
        .collect()
}

/// The helper's language argument: `--lang=<code>` — the same form as for the
/// former helpers ([`crate::system_l10n::lang_args`]). English (the source
/// language), a code outside the allow-list ([`crate::system_l10n::is_valid_code`])
/// or a language missing from the helper catalog — no argument: the helper
/// speaks English. We do not look for the [`crate::system_l10n::LANG_MARKER`]
/// marker: only the scripts of the previous version need it, while our helper
/// has understood `--lang` since its first build (`helper::parse_args`).
pub fn lang_arg(lang: &str) -> Option<String> {
    if lang == crate::i18n::SOURCE_LANGUAGE || !crate::system_l10n::is_valid_code(lang) {
        return None;
    }
    if !crate::system_l10n::languages().contains(&lang) {
        return None;
    }
    Some(format!("{LANG_FLAG}={lang}"))
}

/// argv for raising the session: `pkexec <helper> [--session <id>] [--lang=<code>]`.
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
// Ownership check
// ============================================================================

/// File kind by `lstat` (a symlink is not followed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Regular,
    Dir,
    Symlink,
    Other,
}

/// What the ownership check needs to know about a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileMeta {
    pub kind: FileKind,
    pub uid: u32,
    /// Permission bits (`mode & 0o7777`).
    pub mode: u32,
}

/// The file system for finding the helper (substituted in tests: a root file
/// cannot be created without root).
pub trait HelperFs: Send {
    /// `lstat`: a symlink is not followed.
    fn lstat(&self, path: &Path) -> io::Result<FileMeta>;
    /// `realpath`.
    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf>;
}

/// The real file system.
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

/// Why the helper is unacceptable. The path is the link of the chain that failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnerProblem {
    /// The path is not absolute or contains `..`.
    NotAbsolute(PathBuf),
    /// The owner is not root.
    NotRoot(PathBuf),
    /// Writable by group or others.
    Writable(PathBuf),
    /// The helper itself is not a regular file (a symlink, a directory, a device).
    NotRegular(PathBuf),
    /// A path link is neither a directory nor a symlink.
    NotDirectory(PathBuf),
    /// The helper has no execute bit.
    NotExecutable(PathBuf),
    /// A link cannot be read (except "no file" for the helper itself).
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

/// The helper's ownership chain: the path itself and its canonical form (if the
/// path goes through a symlink). Every link is owned by root; directories are
/// not writable by group and others; a symlink in the middle is acceptable (its
/// directory was checked by the previous link, its target by the canonical
/// path); the helper itself is a regular executable file not writable by group
/// and others.
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

/// Result of the helper search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Located {
    /// Found and passed the ownership check.
    Ready(PathBuf),
    /// Not a single candidate exists.
    Missing,
    /// The first candidate found did not pass the check. We do not look at the
    /// following ones: a substituted file in the preferred location is already a
    /// reason not to raise the session.
    BadOwner(OwnerProblem),
}

/// Find the helper: the first existing candidate decides.
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
// Launching the helper
// ============================================================================

/// A launched helper (the `pkexec` process).
pub trait HelperChild: Send {
    /// Wait for exit. `Ok(None)` — killed by a signal.
    fn wait(&mut self) -> io::Result<Option<i32>>;
}

/// How the helper is launched. The contract: `helper_end` goes onto fd 0 and 1
/// of the child, our own copy is closed no later than the return from `spawn`,
/// other descendants do not get the end.
pub trait Spawner: Send {
    fn spawn(&mut self, argv: &[String], helper_end: OwnedFd) -> io::Result<Box<dyn HelperChild>>;

    /// The helper's build fingerprint — by running `<helper> --build-id` as the
    /// user, without `pkexec`. `None` — not learned (an old helper without the flag,
    /// a failure, a timeout): then the check happens as before, in the handshake.
    fn probe_build_id(&mut self, _helper: &Path) -> Option<String> {
        None
    }
}

/// The helper's flag "print the build fingerprint and exit" (without root).
pub const BUILD_ID_FLAG: &str = "--build-id";

/// How long to wait for the answer to [`BUILD_ID_FLAG`].
const BUILD_ID_PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Live launch via [`Command`]: `argv[0]` is the program.
///
/// The helper's end and its copy (`try_clone` = `F_DUPFD_CLOEXEC`) go into
/// `Stdio`; the child itself does `dup2` onto fd 0/1 between `fork` and `exec`,
/// while the copies in the window keep `CLOEXEC`. The `Command` with these
/// `Stdio` is destroyed right after the launch — the window's copies are closed.
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
        // The file has already passed the ownership check: it is a root program, and
        // running it as the user opens nothing. The environment is empty.
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

/// Wait for the process in the background: the window does not wait for a
/// helper that finishes an operation after the socket is closed.
fn reap_in_background(mut child: Box<dyn HelperChild>) {
    let _ = thread::Builder::new()
        .name("rexon-priv-reap".into())
        .spawn(move || {
            let _ = child.wait();
        });
}

// ============================================================================
// Results and errors
// ============================================================================

/// The operation was executed by the helper ([`HelperMsg::Done`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// The program ran successfully.
    pub ok: bool,
    /// The program's exit code (`None` — killed by a signal).
    pub code: Option<i32>,
    /// Translation key of the explanation, if the helper knows it.
    pub message_key: Option<String>,
}

/// Why the operation was not executed or its outcome is unknown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientError {
    /// There is no helper — the caller may take the "as now" path (Z5).
    HelperMissing,
    /// The helper is unacceptable (owner, permissions) — the "as now" path is also
    /// allowed: a session with a substituted file is not raised at all.
    HelperBadOwner(OwnerProblem),
    /// The person closed the password dialog (126) or is not authorized (127).
    NotAuthorized { code: i32 },
    /// A helper from another build: Rexon was updated, the window must be restarted.
    /// `helper_build` — the helper's fingerprint, if it named it.
    BuildMismatch { helper_build: Option<String> },
    /// A helper of ANOTHER build was recognized BEFORE `pkexec` (a window from the
    /// development tree with the package installed in the system; the package was
    /// updated while the window was open). The password was not asked, the session
    /// was not raised — for rule Z5 this is the same as "no helper": the "as now"
    /// path is allowed.
    HelperOtherBuild { helper_build: String },
    /// The helper refused on the argument check.
    Refused { message_key: String },
    /// A failure of the helper itself; the operation was not executed.
    HelperFailed { message_key: String },
    /// The operation was cancelled before touching the system.
    Cancelled,
    /// The privilege was revoked (inactivity, lock, sleep…); the next request will
    /// raise the session again, with a new password.
    Revoked { message_key: String },
    /// The session broke off. `outcome_unknown` — the operation had already gone to
    /// the helper, and whether it was executed is unknown.
    SessionLost { outcome_unknown: bool, detail: String },
    /// The window closed the session ([`Session::shutdown`]); the operation did not go out.
    Closed,
    /// The helper is busy with another operation (does not happen with the client's queue).
    Busy,
    /// `pkexec` did not start or the socket was not created.
    Spawn(String),
    /// The request is invalid (number of descriptors, a frame too large); it did not
    /// go to the helper.
    InvalidRequest(String),
}

impl ClientError {
    /// What the error is for rule Z5.
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

    /// Z5: whether the caller may execute the same operation via the "as now" path
    /// (`PrivOp::pkexec_argv`). Only when there is no helper or it is unacceptable.
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
    /// Text for the person in the window's language (B13): what happened and what
    /// to do, without technical words. `Display` is the line for the log.
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

/// "Rexon was updated — restart it": a helper from another build. The same idea
/// as the self-update notification (`single_instance::self_update_*`).
fn updated_text() -> String {
    // TRANSLATORS: Rexon was updated while its window was open; a system change needs a restart first.
    tr("Rexon was updated — restart it to continue.")
}

/// Text for the person by the key of the helper's answer: `refused.<code>` (a
/// check refusal), `revoked.<code>` (the privilege was revoked), other keys
/// (failure, frame order). An unknown key gives a general refusal; the key
/// itself is not shown to the person.
pub fn key_text(key: &str) -> String {
    if let Some(code) = key.strip_prefix("revoked.") {
        let reason = revoke::Reason::from_code(code).unwrap_or(revoke::Reason::SessionLost);
        // TRANSLATORS: %1 is why permission to change the system ended (a whole sentence).
        return trf("%1 Nothing was changed.", &[&reason.message()]);
    }
    let Some(code) = key.strip_prefix("refused.") else {
        return tr("Rexon couldn't make this change — nothing was changed.");
    };
    // An orphan the session relies on: the text comes from the core table by name.
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
    // The default session of the login screen: the text comes from the core table
    // by the name of the X11 session.
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
// Session: the "privilege session" thread
// ============================================================================

/// Why the session ended.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Death {
    Closed,
    Revoked(String),
    BuildMismatch(Option<String>),
    Lost(String),
}

impl Death {
    /// Error of an operation that did not go to the helper.
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

    /// Error of the operation that was with the helper when the session ended.
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
    /// `eventfd`: wakes the thread on a new request and on closing.
    wake: OwnedFd,
    state: Mutex<State>,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn wake(&self) {
        let one: u64 = 1;
        // SAFETY: we write 8 bytes from a live variable into our own eventfd.
        unsafe {
            libc::write(self.wake.as_raw_fd(), (&one as *const u64).cast(), 8);
        }
    }

    fn drain_wake(&self) {
        let mut buf: u64 = 0;
        // SAFETY: we read 8 bytes into a live variable; the eventfd is non-blocking.
        unsafe {
            libc::read(self.wake.as_raw_fd(), (&mut buf as *mut u64).cast(), 8);
        }
    }

    fn shutdown_socket(&self) {
        // SAFETY: shutdown of our own live socket.
        unsafe {
            libc::shutdown(self.sock.as_raw_fd(), libc::SHUT_RDWR);
        }
    }
}

fn new_eventfd() -> io::Result<OwnedFd> {
    // SAFETY: eventfd without pointers.
    let fd = unsafe { libc::eventfd(0, libc::EFD_CLOEXEC | libc::EFD_NONBLOCK) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: a new descriptor, only we own it.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

/// A live privilege session. A clone is one more handle of the same session
/// (for cancelling from another thread); the session is closed by
/// [`Session::shutdown`] or by destroying the [`Client`].
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

    /// The session is alive: not closed by the window and not ended.
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

    /// Cancel operation `id`. If it is still in the queue, it is removed right away
    /// (its waiter gets [`ClientError::Cancelled`]); if it is already with the
    /// helper, a `Cancel` goes out and the helper answers (`Cancelled` if it did not
    /// manage to touch the system, otherwise the ordinary result). `false` — there
    /// is no such operation.
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

    /// Close the session: the socket is closed, the helper sees "the window was
    /// closed", finishes the running operation and exits. Does not wait.
    pub fn shutdown(&self) {
        self.shared.lock().closing = true;
        self.shared.shutdown_socket();
        self.shared.wake();
    }
}

/// A submitted request.
pub struct Pending {
    id: u64,
    rx: mpsc::Receiver<JobEvent>,
}

impl Pending {
    /// Operation number (for [`Session::cancel`]).
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Wait for the result, passing progress lines to `on_progress`.
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

/// Wait for the socket or a wake-up. Returns (socket ready, woken up).
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
        // SAFETY: an array of two live pollfds.
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

/// Loop of the "privilege session" thread: hand out the next operation, read
/// the answers.
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
                        // The session is dead BEFORE the waiter gets its answer: on waking up
                        // it must not see a "live" session (see [`handle_msg`]).
                        st.dead = Some(Death::Lost(detail.clone()));
                        let _ = job.reply.send(JobEvent::Finished(Err(ClientError::SessionLost {
                            outcome_unknown: false,
                            detail: detail.clone(),
                        })));
                        return Death::Lost(detail);
                    }
                    // Our own copies of the files are closed here: the helper has duplicates.
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

/// Parse a helper message. `Some` — the session has ended.
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
                // The session is ending: mark the death BEFORE answering the waiter,
                // under the same lock. Otherwise the waiter wakes up before the session
                // thread reaches [`die`] and manages to see a "live" session
                // (`has_session()` after the privilege was revoked — a race, caught by
                // the `revoked_answer_to_operation` test).
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

/// The session ended: an error for all waiters, the socket is closed.
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
// Client
// ============================================================================

enum HandshakeFail {
    /// The helper closed the channel without answering (the password dialog was
    /// closed, no privileges).
    Eof,
    Error(ClientError),
}

/// `Hello` → `Welcome`. A `Welcome` with a foreign fingerprint is also rejected:
/// the helper checks it itself, the window a second time.
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
        // The helper (or pkexec itself) exited without reading `Hello`: the
        // unread frame in its queue turns the close into ECONNRESET rather than
        // EOF. A live pkexec with 126/127 exits exactly this way.
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

/// The window's privilege session client. The session is raised on the first
/// request and lives until it ends (revocation, break) or the window closes it;
/// the next request after the end raises a new one (a new password). After
/// `BuildMismatch` — never again: the window must restart.
pub struct Client {
    spawner: Box<dyn Spawner>,
    fs: Box<dyn HelperFs>,
    candidates: Vec<PathBuf>,
    build_id: String,
    session_id: Option<String>,
    /// The window's interface language: the helper translates its messages.
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

    /// Live client: `pkexec`, the real file system, the fingerprint of this build,
    /// the session number from `XDG_SESSION_ID`, the Rexon interface language.
    pub fn live() -> Self {
        Self::new(
            Box::new(CommandSpawner),
            Box::new(RealFs),
            proto::build_id(),
            std::env::var("XDG_SESSION_ID").ok(),
        )
        .with_lang(&crate::i18n::current_language())
    }

    /// The helper's message language (the interface code, `ru`); only a valid one
    /// gets into argv ([`lang_arg`]).
    pub fn with_lang(mut self, lang: &str) -> Self {
        self.lang = Some(lang.to_string());
        self
    }

    /// Custom helper search locations (tests, the future B14).
    pub fn with_candidates(mut self, candidates: Vec<PathBuf>) -> Self {
        self.candidates = candidates;
        self
    }

    /// The live session, if any (a handle for cancelling from another thread).
    pub fn session(&self) -> Option<Session> {
        self.session.as_ref().filter(|s| s.is_alive()).cloned()
    }

    /// There is a live session: the next request will not ask for the password.
    pub fn has_session(&self) -> bool {
        self.session().is_some()
    }

    /// If there is a session — return it; if not — raise one (the password dialog).
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
        // Checking the build BEFORE the password dialog: otherwise the person
        // would enter the password for a mere "Rexon was updated", and the old
        // path is closed after the handshake (Z5).
        if let Some(theirs) = self.spawner.probe_build_id(&helper) {
            if theirs != self.build_id {
                return Err(ClientError::HelperOtherBuild { helper_build: theirs });
            }
        }
        let argv = helper_argv(&helper, self.session_id.as_deref(), self.lang.as_deref());
        let wake = new_eventfd().map_err(|e| ClientError::Spawn(format!("eventfd: {e}")))?;
        let (ours, theirs) =
            proto::seqpacket_pair().map_err(|e| ClientError::Spawn(format!("socketpair: {e}")))?;
        // `theirs` goes to the Spawner: it puts it on fd 0/1 of the child and
        // closes its own copy before returning.
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

    /// Put an operation into the session queue (raises the session if needed).
    /// `fds` — the operation's open files in the order of [`PrivOp::local_files`]
    /// (the `slot` number = the index).
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

    /// Execute an operation and wait for the result; progress lines go to `on_progress`.
    pub fn request(
        &mut self,
        op: &PrivOp,
        fds: Vec<OwnedFd>,
        on_progress: &mut dyn FnMut(&str),
    ) -> Result<Outcome, ClientError> {
        self.submit(op, fds)?.wait(on_progress)
    }

    /// Close the session (window closed, minimized to the tray, "Revoke now").
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

    // ---------------------------------------------------------------- file system

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
        /// The root and the helper directories — root 0755.
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

    // ---------------------------------------------------------------- helper

    type Script = dyn Fn(OwnedFd) -> i32 + Send + Sync;

    struct FakeSpawner {
        calls: Arc<Mutex<Vec<Vec<String>>>>,
        script: Arc<Script>,
        /// What the helper answers to `--build-id` (`None` — an old one, without the flag).
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

    /// Accept `Hello` and answer `Welcome`.
    fn accept_hello(sock: &OwnedFd) {
        let hello: ClientMsg<PrivOp> = proto::recv_msg(sock.as_fd()).unwrap().unwrap();
        assert_eq!(hello, ClientMsg::Hello(Hello { build_id: BUILD.into() }));
        send(sock, HelperMsg::Welcome { build_id: BUILD.into() });
    }

    /// A helper that executes all operations successfully, with one progress line.
    /// The numbers of the arrived operations and the number of files go to `seen`.
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

    // ---------------------------------------------------------------- search

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
        // The first one found decides: a bad one in /usr/lib is not bypassed by a
        // good one in /usr/local/lib.
        let both = FakeFs::base()
            .file(HELPER, 1000, 0o755)
            .file("/usr/local/lib/rexon-priv-helper", 0, 0o755);
        assert!(matches!(locate_helper(&both, &cands), Located::BadOwner(_)));
    }

    #[test]
    fn helper_symlink_refused_and_dir_symlink_checked_by_target() {
        let cands = helper_candidates();
        // The helper itself is a symlink: not allowed.
        let fs = FakeFs::base().link(HELPER, "/home/u/x");
        assert_eq!(
            locate_helper(&fs, &cands),
            Located::BadOwner(OwnerProblem::NotRegular(HELPER.into()))
        );
        // A directory symlink into the user's directory: the target is checked.
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
        // Real file system: a system binary owned by root passes, our own file does not.
        let sys = Path::new("/usr/bin/env");
        if RealFs.lstat(sys).map(|m| m.kind == FileKind::Regular && m.uid == 0).unwrap_or(false) {
            assert_eq!(check_ownership(&RealFs, sys), Ok(()));
        }
        // A file in the user's tree: the chain breaks at its directory.
        let mine = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"));
        let res = check_ownership(&RealFs, mine);
        // If the tests run as root, the owner cannot be checked.
        // SAFETY: getuid without arguments.
        if unsafe { libc::getuid() } != 0 {
            assert!(matches!(res, Err(OwnerProblem::NotRoot(_))), "{res:?}");
        }
        // /tmp (1777) is writable by all: a helper from there is no good.
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
        // The same form as for the former helpers (`system_l10n::lang_arg_for`).
        let old = crate::system_l10n::lang_arg_for("ru", Some(crate::system_l10n::LANG_MARKER));
        assert_eq!(old.as_deref(), Some("--lang=ru"));
        assert_eq!(
            helper_argv(p, Some("2"), Some("ru")),
            ["pkexec", HELPER, "--session", "2", "--lang=ru"]
        );
        assert_eq!(helper_argv(p, None, Some("de")), ["pkexec", HELPER, "--lang=de"]);
        // English, garbage and a language outside the catalog — no argument.
        for bad in ["en", "", "../x", "RU", "ru;id", "xx"] {
            assert_eq!(helper_argv(p, None, Some(bad)), ["pkexec", HELPER], "{bad}");
        }
        // The helper (B4) understands exactly this form.
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

    // ---------------------------------------------------------------- session

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
        // Without a file — an invalid request, the session is not even raised.
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
        // The password is not asked a second time: Rexon must be restarted.
        let e = c.request(&op(), vec![], &mut no_progress()).unwrap_err();
        assert!(matches!(e, ClientError::BuildMismatch { .. }));
        assert_eq!(calls.lock().unwrap().len(), 1);
    }

    /// A helper of another build was recognized before `pkexec`: there is no
    /// password dialog, the "as now" path is allowed (a window from the development
    /// tree with the package installed in the system).
    #[test]
    fn other_build_found_before_password_allows_old_path() {
        let (mut c, calls) = client_probing(FakeFs::good(), Some("0.4.9+old"), |_| 0);
        let e = c.request(&op(), vec![], &mut no_progress()).unwrap_err();
        assert_eq!(e, ClientError::HelperOtherBuild { helper_build: "0.4.9+old".into() });
        assert!(e.raw_fallback_allowed());
        assert!(calls.lock().unwrap().is_empty(), "пароль не спрашивался");
        // And next time — again via the old path, without "Rexon was updated".
        let again = c.request(&op(), vec![], &mut no_progress()).unwrap_err();
        assert!(matches!(again, ClientError::HelperOtherBuild { .. }));
    }

    /// Our own build per `--build-id` — an ordinary session, one password dialog.
    #[test]
    fn own_build_by_probe_starts_session() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let (mut c, calls) = client_probing(FakeFs::good(), Some(BUILD), obedient(Arc::clone(&seen)));
        c.request(&op(), vec![], &mut no_progress()).unwrap();
        assert_eq!(calls.lock().unwrap().len(), 1);
    }

    /// Live `--build-id`: the real program prints the fingerprint.
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
            // The next request will ask for the password again.
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
            // We wait until the window closes the channel.
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
                // The privilege was revoked by inactivity even before the first operation.
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
        // Wait until the session thread reads the revocation.
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
                    // We do not finish the operation ourselves: we wait for Cancel.
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
        // The second one is still in the queue: it is removed without the helper.
        assert!(session.cancel(second.id()));
        assert_eq!(second.wait(&mut no_progress()), Err(ClientError::Cancelled));
        // The first one is with the helper (we wait until the thread hands it out).
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
            // The window closed the session — the helper sees EOF.
            let eof = matches!(proto::recv_frame(sock.as_fd()), Ok(None));
            tx.send(eof).unwrap();
            0
        });
        let session = c.ensure_session().unwrap();
        c.shutdown();
        assert!(rx.recv_timeout(Duration::from_secs(5)).unwrap());
        assert!(!session.is_alive());
        // A closed handle does not take new operations.
        assert_eq!(session.submit(99, vec![1], vec![]).err(), Some(ClientError::Closed));
    }

    // ---------------------------------------------------------------- Z3

    fn inode(fd: BorrowedFd<'_>) -> u64 {
        // SAFETY: fstat into a zeroed structure on a live fd.
        let mut st: libc::stat = unsafe { std::mem::zeroed() };
        assert_eq!(unsafe { libc::fstat(fd.as_raw_fd(), &mut st) }, 0);
        st.st_ino as u64
    }

    /// Numbers of the sockets open in the process: fd → inode.
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

        // The "helper" is a real process via CommandSpawner: the socket on 0 and 1.
        let mut helper = CommandSpawner
            .spawn(&["sleep".into(), "30".into()], theirs)
            .unwrap();
        // An unrelated descendant of the window while the socket is open.
        let mut other = Command::new("sleep").arg("30").spawn().unwrap();
        // And a real `true`: it starts and exits without the socket.
        assert!(Command::new("true").status().unwrap().success());

        // We do not know the "helper's" pid through the trait — we look for it among
        // our children.
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

        // The window has no copy of the helper's end left.
        let mine = socket_fds(std::process::id());
        assert!(!mine.values().any(|&i| i == theirs_ino), "окно держит конец помощника");

        other.kill().ok();
        other.wait().ok();
        // SAFETY: kill of our own child process.
        unsafe { libc::kill(helper_pid as i32, libc::SIGKILL) };
        helper.wait().ok();
        drop(ours);
    }

    /// Direct descendants of this process.
    fn child_pids() -> Vec<u32> {
        let me = std::process::id();
        let mut out = Vec::new();
        for entry in std::fs::read_dir("/proc").unwrap().flatten() {
            let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else { continue };
            let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else { continue };
            // Field 4 is the ppid; the process name in parentheses may contain spaces.
            let Some(rest) = stat.rsplit_once(')').map(|(_, r)| r) else { continue };
            if rest.split_whitespace().nth(1).and_then(|p| p.parse::<u32>().ok()) == Some(me) {
                out.push(pid);
            }
        }
        out
    }
}

/// Texts for the person (B13): every error and helper key has its own phrase,
/// without technical words, with a translation in `po/ru.po` and `po/de.po`.
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

    /// Keys of helper refusals: argument check, unpacking, `.PKGINFO`.
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

    /// Technical words the buyer must not see (lower case).
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
        // An unknown key gives a general refusal; the key itself is not shown to the person.
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

    /// Every phrase (the English `msgid`) is translated into ru and de, and the
    /// translation has no technical words. A compound revocation phrase is checked
    /// in parts.
    #[test]
    fn texts_are_translated_without_jargon() {
        english();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../po");
        // Refusals "the session relies on a package" — a ready text with session
        // names; their templates are checked.
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
