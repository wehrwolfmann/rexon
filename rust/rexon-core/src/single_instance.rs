//! One Rexon per session: the second-launch lock and the «raise the window» listener.
//!
//! WHY A SEPARATE MODULE. Before this change the mechanism was torn into three
//! pieces, and none was in the core in full:
//!   * the lock and socket addresses lived in `app.rs` (pure functions);
//!   * the `flock` itself — in the Python binding `rust/rexon-py/src/lib.rs`, i.e.
//!     it was fundamentally unavailable to the product's Rust entry point: the binding
//!     exists only inside the Python process;
//!   * the «raise the window» listener (`QLocalServer`) and knocking on it — in Python
//!     (`rexon/qt_main/app.py::_serve_activation`, `rexon/app.py::
//!     _activate_running_instance`).
//! While the lock lives in the binding, the Rust binary cannot become the product's
//! entry point: it either brings up a second window next to the running Rexon, or
//! acquires a SECOND copy of the lock — and two locks for one product mean
//! two different answers to the single question «am I first?» (rule #12).
//!
//! Here the whole mechanism is gathered in one place: addresses, lock, listener and
//! knock. The binding and both entry points (Python and Rust) call the core.
//!
//! ── HISTORY OF THE DEFECTS THIS CLOSES ─────────────────────────────────
//!
//! 2026-09-02, «five cacti in the tray». The owner's KDE system tray
//! held five or six identical Rexon icons. `qdbus6` +
//! `GetConnectionUnixProcessID` + `/proc/<pid>/cmdline` showed five LIVE
//! `rexon` processes (not dead StatusNotifierWatcher records), started
//! at different times over three days: some by the launcher shortcut, some by hand from
//! the development directory. No launch checked that it was not the first.
//!
//! 2026-09-06, «the program is running, but there is no door into it». A lock ALONE IS NOT ENOUGH.
//! By itself it behaves correctly and quietly exits — but the user sees
//! nothing of it: the warning goes to a console the shortcut does not
//! have. The first Rexon's window had been hidden to the tray with the close button (with a live
//! icon the close button hides rather than quits), the person clicked the shortcut twice, both
//! launches hit the lock, and EXACTLY NOTHING happened on the screen.
//! The right answer of a single instance to a second launch is not «leave»,
//! but RAISE the window of the one already running.
//!
//! 2026-09-15, «the door does not fit into the address». `sun_path` holds 107 bytes,
//! and with a long `XDG_RUNTIME_DIR` the listener did not come up at all, while the second
//! launch said «already running — not creating a second icon» instead of a raised
//! window. Now one function, [`socket_path`], decides the address BEFORE `bind` and
//! `connect` (a short fallback directory `/tmp/rexon-<uid>`), both sides
//! check the peer's uid, and when the listener is silent the core tells the person
//! the truth ([`said_not_responding`]).

use std::fs::File;
use std::io::{Read, Write};
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::os::unix::io::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::i18n::{tr, trf};

// ============================================================================
// Addresses: directory, lock, socket
// ============================================================================

/// Directory of the lock and socket: `$XDG_RUNTIME_DIR`, or `/tmp` when it is missing.
///
/// Port of the expression `os.environ.get("XDG_RUNTIME_DIR") or "/tmp"`, which in
/// Python was repeated at two ends of the mechanism. An empty string in the variable
/// equals its absence — exactly like Python's `or`, where `""` is falsy.
pub fn runtime_dir(xdg_runtime_dir: Option<&str>) -> String {
    match xdg_runtime_dir {
        Some(dir) if !dir.is_empty() => dir.to_string(),
        _ => "/tmp".to_string(),
    }
}

/// The same from the live environment — the only place where the variable is read.
pub fn runtime_dir_from_env() -> String {
    runtime_dir(std::env::var("XDG_RUNTIME_DIR").ok().as_deref())
}

/// The «one Rexon per session» flock file.
pub fn lock_path(runtime_dir: &str) -> String {
    format!("{runtime_dir}/rexon.lock")
}

/// The longest socket address the kernel accepts: `sockaddr_un.sun_path` —
/// 108 bytes including the terminating zero.
pub const SUN_PATH_MAX: usize = 107;

/// Socket name in the runtime directory.
const SOCKET_FILE: &str = "rexon-activate.sock";

/// Root of the fallback directory. Exactly `/tmp`, not `std::env::temp_dir()`:
/// `TMPDIR` can be arbitrarily long, and the fallback address would exceed
/// the limit for the same reason as the main one.
const FALLBACK_ROOT: &str = "/tmp";

/// The effective uid of this process — the owner of the fallback directory and
/// the only one whose knock the listener accepts.
pub fn current_uid() -> u32 {
    unsafe { libc::geteuid() }
}

/// The main socket address — next to the flock file, in the same directory.
pub fn preferred_socket_path(runtime_dir: &str) -> String {
    format!("{runtime_dir}/{SOCKET_FILE}")
}

/// Fallback directory of user `uid`: `/tmp/rexon-<uid>` (mode 0700).
pub fn fallback_socket_dir(uid: u32) -> String {
    format!("{FALLBACK_ROOT}/rexon-{uid}")
}

/// Fallback address for a runtime directory: `/tmp/rexon-<uid>/activate-<hash>.sock`.
///
/// The directory hash in the name is mandatory: one user can have several
/// sessions with different (and both long) `XDG_RUNTIME_DIR`s, their locks are
/// different, and without the hash the second session would knock on the first one's window.
///
/// ⚠️ The hash is our own FNV-1a, not `DefaultHasher`: std does not promise to keep
/// its algorithm between releases, and the listener and the knock can be different binaries
/// (the Python wheel and `rexon-gui`) built by different Rust versions.
pub fn fallback_socket_path(runtime_dir: &str, uid: u32) -> String {
    let key = match runtime_dir.trim_end_matches('/') {
        "" => runtime_dir,
        trimmed => trimmed,
    };
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in key.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{}/activate-{hash:016x}.sock", fallback_socket_dir(uid))
}

/// The «raise the window» socket for user `uid` — see [`socket_path`].
pub fn socket_path_for(runtime_dir: &str, uid: u32) -> String {
    let preferred = preferred_socket_path(runtime_dir);
    if preferred.len() <= SUN_PATH_MAX {
        preferred
    } else {
        fallback_socket_path(runtime_dir, uid)
    }
}

/// The «raise the window» socket — the ONLY address function: the first Rexon
/// listens on it, the second knocks (both entries, Rust and Python via the bridge).
///
/// ⚠️ Usually — next to the lock, in the same directory: the socket lives and dies
/// together with the session. But `sun_path` holds 107 bytes, and with a long
/// `XDG_RUNTIME_DIR` (measurement 15.09: the Rust window's listener did not come up with
/// «path must be shorter than SUN_LEN», the second launch falsely said «already running»
/// instead of a raised window) the address moves to the short fallback directory
/// [`fallback_socket_path`]. The decision is made HERE, before `bind` and
/// `connect`, so both sides arrive at the same address.
///
/// Why the fallback address is a file in `/tmp/rexon-<uid>/` and not a Linux abstract
/// name: the classic window's listener is a `QLocalServer`, it receives the
/// address as a string via the bridge and does not understand an abstract name without
/// changing Python. An abstract address in Rust and a file one in Python are two
/// addresses for one mechanism, and a Rust launch would not reach the
/// Python host. A foreign user is cut off by the directory permissions (0700,
/// the owner is checked, [`ensure_private_dir`]) and by checking the peer's uid
/// at BOTH ends of the connection (`SO_PEERCRED`).
pub fn socket_path(runtime_dir: &str) -> String {
    socket_path_for(runtime_dir, current_uid())
}

/// A log line if the address went to the fallback directory; `None` — the main one.
pub fn socket_address_note(runtime_dir: &str) -> Option<String> {
    let preferred = preferred_socket_path(runtime_dir);
    if preferred.len() <= SUN_PATH_MAX {
        return None;
    }
    Some(format!(
        "адрес «подними окно» {preferred} — {} байт, а в sun_path помещается {SUN_PATH_MAX}; \
         взят запасной {}",
        preferred.len(),
        fallback_socket_path(runtime_dir, current_uid())
    ))
}

/// Prepare the private socket directory: create it with mode 0700 or
/// make sure the existing one is a real directory (not a link) of this
/// user. Our own directory with extra permissions is fixed to 0700.
///
/// ⚠️ `/tmp` is shared by the whole machine: a foreign user can create
/// `/tmp/rexon-<uid>` in advance as their own and plant their socket there. Such a directory is NOT
/// used — we must not come up there, and the reason goes to the log.
pub fn ensure_private_dir(dir: &str, uid: u32) -> Result<(), BindError> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    let refuse = |reason: String| BindError::UnsafeDir {
        dir: dir.to_string(),
        reason,
    };
    let meta = match std::fs::symlink_metadata(dir) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            match std::fs::DirBuilder::new().mode(0o700).create(dir) {
                // A race with a neighbouring launch that created the directory first
                // is not an error: it is checked below anyway.
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(refuse(format!("не создаётся: {e}"))),
            }
            std::fs::symlink_metadata(dir).map_err(|e| refuse(format!("не читается: {e}")))?
        }
        Err(e) => return Err(refuse(format!("не читается: {e}"))),
    };
    if meta.file_type().is_symlink() {
        return Err(refuse("это символическая ссылка, а не каталог".to_string()));
    }
    if !meta.is_dir() {
        return Err(refuse("это не каталог".to_string()));
    }
    if meta.uid() != uid {
        return Err(refuse(format!(
            "владелец — uid {}, а не {uid}",
            meta.uid()
        )));
    }
    if meta.mode() & 0o077 != 0 {
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| refuse(format!("права {:o} не исправляются: {e}", meta.mode() & 0o777)))?;
    }
    Ok(())
}

/// uid of the process at the other end of the connection (`SO_PEERCRED`); `None` — the kernel did not
/// answer.
pub fn peer_uid(stream: &UnixStream) -> Option<u32> {
    let mut cred = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let rc = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut cred as *mut libc::ucred).cast(),
            &mut len,
        )
    };
    (rc == 0 && len as usize == std::mem::size_of::<libc::ucred>()).then_some(cred.uid)
}

// ============================================================================
// The knock protocol
// ============================================================================

/// The second launch's request: «raise your window».
pub const SHOW_REQUEST: &[u8] = b"show\n";
/// The reply «raised».
pub const REPLY_RAISED: &[u8] = b"ok\n";
/// The reply «could not raise» — the second launch must tell the person the truth
/// about the tray instead of pretending the window is on screen.
pub const REPLY_FAILED: &[u8] = b"no\n";

/// How long to wait for the reply. A second is more than enough: at the other end is a live event
/// loop, not work. Waiting longer means keeping the person in front of an empty
/// screen instead of telling them about the tray.
pub const KNOCK_TIMEOUT: Duration = Duration::from_secs(2);

/// There is nothing larger to accept: a request is «show» with a section and subsection,
/// a reply is a short word. The former 16-byte limit would already cut
/// «show desktops compare».
const WIRE_BUF: usize = 128;

/// The first word of the «raise the window» request.
const SHOW_WORD: &str = "show";

/// Trim the newline and whitespace at both ends — port of `bytes.strip()`.
fn trimmed(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(bytes.len());
    let end = bytes
        .iter()
        .rposition(|b| !b.is_ascii_whitespace())
        .map_or(start, |i| i + 1);
    &bytes[start..end]
}

/// Whether this is a «raise the window» request — bare or with a section.
pub fn is_show_request(bytes: &[u8]) -> bool {
    parse_show_request(bytes).is_some()
}

/// A parsed «raise the window» request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShowRequest {
    /// (section, subsection) to open; `None` — open as is.
    pub section: Option<(String, String)>,
}

/// A verified target: a section from [`crate::qt_main::SECTIONS`] and a subsection
/// that this section really has (otherwise the subsection is empty).
///
/// There is no list of its own here: subsections come from the same two tables
/// used to choose the section at startup (`app::PAGE_TARGETS` and
/// `cli::SINGLE_TAB_SECTIONS`). An unknown section is `None`: a foreign or
/// garbage name raises the window but does not switch anywhere.
pub fn valid_target(section: &str, subpage: &str) -> Option<(String, String)> {
    if !crate::qt_main::SECTIONS.contains(&section) {
        return None;
    }
    let known_subpage = !subpage.is_empty()
        && crate::app::PAGE_TARGETS
            .iter()
            .map(|(_, s, p)| (*s, *p))
            .chain(crate::cli::SINGLE_TAB_SECTIONS.iter().map(|(_, s, p)| (*s, *p)))
            .any(|(s, p)| s == section && p == subpage);
    let subpage = if known_subpage { subpage } else { "" };
    Some((section.to_string(), subpage.to_string()))
}

/// Parse a request: `show`, `show <section>` or `show <section> <subsection>`.
///
/// `None` — this is not «show» at all (do not touch the window). A bare `show` is the old
/// protocol and works as before. The section is checked by [`valid_target`];
/// an unknown section or extra words give a request WITHOUT a section: the window
/// is raised but does not switch to garbage.
pub fn parse_show_request(bytes: &[u8]) -> Option<ShowRequest> {
    let text = std::str::from_utf8(trimmed(bytes)).ok()?;
    let mut words = text.split_ascii_whitespace();
    if words.next()? != SHOW_WORD {
        return None;
    }
    let rest: Vec<&str> = words.collect();
    let section = match rest.as_slice() {
        [section] => valid_target(section, ""),
        [section, subpage] => valid_target(section, subpage),
        _ => None,
    };
    Some(ShowRequest { section })
}

/// The knock word for a target: `show\n` or `show <section>[ <subsection>]\n`.
/// An unverified target does not go on the wire — a bare `show` does.
pub fn show_request(target: Option<(&str, &str)>) -> Vec<u8> {
    match target.and_then(|(s, p)| valid_target(s, p)) {
        Some((section, subpage)) if subpage.is_empty() => {
            format!("{SHOW_WORD} {section}\n").into_bytes()
        }
        Some((section, subpage)) => format!("{SHOW_WORD} {section} {subpage}\n").into_bytes(),
        None => SHOW_REQUEST.to_vec(),
    }
}

/// Which section to carry in the knock, from the second launch's command line.
///
/// The same translation used to choose the section at startup
/// (`cli::start_section`: `--section=`, the single-tab flag, `--page=`),
/// plus the [`valid_target`] check. Otherwise a shortcut quick action
/// («Manage backups») with Rexon running raised the window
/// on the section where it was left — the section got lost in the knock.
pub fn knock_target(args: &[String]) -> Option<(String, String)> {
    let (section, subpage) = crate::cli::start_section(args)?;
    valid_target(&section, &subpage)
}

/// The listener's reply based on the outcome: whether the window came up or not.
pub fn reply_for(raised: bool) -> &'static [u8] {
    if raised {
        REPLY_RAISED
    } else {
        REPLY_FAILED
    }
}

/// Whether the first instance accepted the request and raised the window.
///
/// Port of `reply.strip() == b"ok"`: any other reply (including empty and
/// garbage) counts as a refusal — lying to the user «raised» is worse than
/// honestly sending them to the tray.
pub fn knock_accepted(reply: &[u8]) -> bool {
    trimmed(reply) == trimmed(REPLY_RAISED)
}

// ============================================================================
// Lock
// ============================================================================

/// The lock descriptor lives until the end of THIS process together with the `flock`.
///
/// Port of the module variable `_LOCK_FD` from the binding. There is no such thing as a stale
/// lock: the kernel releases the `flock` itself when the descriptor is closed or
/// the holder dies, so a lock file left over from a previous session does not
/// fool a second launch.
///
/// Next to the descriptor is the lock path: by it [`holds_lock`] answers whether
/// THIS process has already taken the lock.
static LOCK_FD: Mutex<Option<(String, File)>> = Mutex::new(None);

/// `flock(LOCK_EX|LOCK_NB)` on `path`; `false` — the lock is already held by a live
/// process, and the caller must not bring up a second window but knock on
/// the first one ([`activate_running_instance`]).
pub fn acquire_lock(path: &str) -> bool {
    let Ok(file) = File::create(path) else {
        return false;
    };
    let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if rc != 0 {
        return false;
    }
    if let Ok(mut slot) = LOCK_FD.lock() {
        *slot = Some((path.to_string(), file));
    }
    true
}

/// Whether THIS process holds the lock on `path`.
///
/// Needed by an entry point that can be reached both after taking the lock and
/// bypassing it: `rexon/qt_main/app.py::main` is called from `rexon/app.py`, where
/// the lock is already taken, but also directly (`python3 -m rexon.qt_main.app`), where nobody
/// took it. A repeated `flock` from the same process on a new
/// descriptor blocks itself — without this question the first instance
/// would take itself for the second.
pub fn holds_lock(path: &str) -> bool {
    LOCK_FD
        .lock()
        .map(|slot| matches!(slot.as_ref(), Some((held, _)) if held == path))
        .unwrap_or(false)
}

/// The classic window flag (Python entry): the shortcut item «Rexon (classic
/// window)». With the lock taken, such a launch asks the holder to give way.
pub const CLASSIC_FLAG: &str = "--classic";

/// What a launch that hit a taken lock should do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnBusyLock {
    /// Knock on the first instance and raise its window.
    Knock,
    /// Exit silently with code 0 without touching anything: nobody asked for a window.
    ExitQuietly,
    /// Classic window (`--classic`): ask the holder to hand over the lock
    /// ([`request_handover`]). Otherwise, with a Rust host, the «classic
    /// window» shortcut would only raise the Rust window — the classic one could not be opened.
    RequestHandover,
}

/// Decision for a launch that hit a taken lock (`args` — without the program
/// name). One for both entry points: Python (`rexon/app.py`) and Rust
/// (`rexon-gui/src/bin/main_window.rs`).
///
/// ⚠️ A hidden launch (`--tray`, which Rexon uses at login) does NOT
/// knock. It used to — and pulled an already open window onto the screen:
/// a launch that by definition needs no window showed it to the person
/// over the desktop. If Rexon is already running, a hidden launch has nothing
/// to do at all: the tray icon is already there.
pub fn on_busy_lock(args: &[String]) -> OnBusyLock {
    if crate::tray::starts_hidden(args) {
        OnBusyLock::ExitQuietly
    } else if args.iter().any(|a| a == CLASSIC_FLAG) {
        OnBusyLock::RequestHandover
    } else {
        OnBusyLock::Knock
    }
}

// ============================================================================
// Knock: «raise your window»
// ============================================================================

/// Ask an already running Rexon to raise its window and WAIT for the reply.
///
/// `false` — there is no listener (Rexon is older than this change), the socket is dead or
/// the window could not be raised. Without a reply we do not know whether the window came up, and
/// lying to the user «raised» is worse than honestly sending them to the tray.
pub fn activate_running_instance(socket_path: &str) -> bool {
    activate_running_instance_to(socket_path, None)
}

/// The same, but with a section to open in the raised window ([`show_request`]).
pub fn activate_running_instance_to(socket_path: &str, target: Option<(&str, &str)>) -> bool {
    knock(socket_path, target) == KnockOutcome::Raised
}

/// How the knock ended — what to tell the person depends on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KnockOutcome {
    /// The listener raised the window.
    Raised,
    /// The listener replied, but the window did not come up.
    Refused,
    /// There was no reply: the listener did not come up, the socket is dead, a foreign owner, or
    /// the timeout expired. The string is the real reason for the log.
    NoAnswer(String),
}

/// Knock and learn the outcome in detail (for the second launch's text).
pub fn knock(socket_path: &str, target: Option<(&str, &str)>) -> KnockOutcome {
    knock_expecting_owner(socket_path, target, current_uid())
}

/// The same, but the listener must belong to user `owner_uid`.
pub fn knock_expecting_owner(
    socket_path: &str,
    target: Option<(&str, &str)>,
    owner_uid: u32,
) -> KnockOutcome {
    match exchange(socket_path, &show_request(target), owner_uid) {
        Ok(reply) if knock_accepted(&reply) => KnockOutcome::Raised,
        Ok(reply) if trimmed(&reply).is_empty() => {
            KnockOutcome::NoAnswer(format!("слушатель на {socket_path} закрыл соединение без ответа"))
        }
        Ok(_) => KnockOutcome::Refused,
        Err(reason) => KnockOutcome::NoAnswer(reason),
    }
}

/// Say a word to the listener and wait for the reply; `Err` — the real reason
/// why there is no reply (no listener, dead socket, foreign owner, timeout).
fn exchange(socket_path: &str, word: &[u8], owner_uid: u32) -> Result<Vec<u8>, String> {
    if socket_path.len() > SUN_PATH_MAX {
        return Err(too_long(socket_path));
    }
    let mut sock = UnixStream::connect(socket_path)
        .map_err(|e| format!("к {socket_path} не подключиться: {e}"))?;
    // The word goes only to our own: the other end may be a process
    // of another user that planted a socket at our address.
    match peer_uid(&sock) {
        Some(uid) if uid == owner_uid => {}
        other => {
            return Err(format!(
                "на {socket_path} слушает не наш процесс (uid {}, ожидался {owner_uid}) — \
                 стук не отправлен",
                other.map_or_else(|| "не известен".to_string(), |u| u.to_string())
            ));
        }
    }
    // Both timeouts are mandatory: without them a process dead at the other end
    // (the socket is intact, the event loop is stuck) would hang the launch forever.
    let io = |e: std::io::Error| format!("обмен с {socket_path} оборвался: {e}");
    sock.set_read_timeout(Some(KNOCK_TIMEOUT)).map_err(io)?;
    sock.set_write_timeout(Some(KNOCK_TIMEOUT)).map_err(io)?;
    sock.write_all(word).map_err(io)?;
    sock.flush().map_err(io)?;
    let mut reply = [0u8; WIRE_BUF];
    let n = sock.read(&mut reply).map_err(io)?;
    Ok(reply[..n].to_vec())
}

/// The «address does not fit» reason — with the path, length and limit.
fn too_long(path: &str) -> String {
    format!(
        "адрес {path} — {} байт, а в sun_path помещается {SUN_PATH_MAX}",
        path.len()
    )
}

// ============================================================================
// Handing over the host: the classic window asks the holder to give up the lock
// ============================================================================
//
// The session host can be the Rust window (with the tray and autostart it always
// will be), while the shortcut item «Rexon (classic window)» starts the Python entry.
// A normal knock would raise the Rust window — the classic one could not be opened.
// So the classic launch says a special word, and the holder decides for
// itself ([`listener_action`]): a Rust holder closes the window and the icon and
// exits (the lock is released together with the process), a Python holder simply
// raises its window — it is the classic one, there is nothing to hand over.

/// The classic window's request «give up the lock» — one protocol word.
pub const HANDOVER_REQUEST: &[u8] = b"handover\n";
/// The holder's reply «leaving, the lock will be free now».
pub const REPLY_LEAVING: &[u8] = b"bye\n";
/// How long the new process waits for the lock to be released after «bye»: the holder
/// needs to close the window and the icon and exit.
pub const HANDOVER_WAIT: Duration = Duration::from_secs(5);

/// A parsed word on the socket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// «Raise the window» (with or without a section).
    Show(ShowRequest),
    /// «Give up the lock» from the classic window.
    Handover,
}

/// Parse a word: `show …` ([`parse_show_request`]) or `handover`.
pub fn parse_request(bytes: &[u8]) -> Option<Request> {
    if let Some(show) = parse_show_request(bytes) {
        return Some(Request::Show(show));
    }
    let word = std::str::from_utf8(trimmed(bytes)).ok()?;
    (word == trimmed_str(HANDOVER_REQUEST)).then_some(Request::Handover)
}

fn trimmed_str(bytes: &'static [u8]) -> &'static str {
    std::str::from_utf8(trimmed(bytes)).unwrap_or("")
}

/// Who holds the lock — the reply to `handover` depends on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Holder {
    /// The classic window in Python.
    Python,
    /// The window in Rust.
    Rust,
}

/// What the listener should do on a word.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListenerAction {
    /// Raise the window and, if given, open the section.
    Raise(Option<(String, String)>),
    /// Give up the lock: close the window and the icon, exit.
    HandOver,
}

/// Decision of the listener of holder `holder` on a word; `None` — a foreign word,
/// do not touch the window. One for both listeners.
pub fn listener_action(holder: Holder, bytes: &[u8]) -> Option<ListenerAction> {
    Some(match parse_request(bytes)? {
        Request::Show(show) => ListenerAction::Raise(show.section),
        // The holder is the classic window itself: nothing to hand over.
        Request::Handover if holder == Holder::Python => ListenerAction::Raise(None),
        Request::Handover => ListenerAction::HandOver,
    })
}

/// Take the lock, waiting for it to be released no longer than `limit`.
///
/// Needed after «bye»: the holder does not leave instantly (it closes the window and
/// the icon), and guessing timings from outside would mean sometimes missing, sometimes
/// waiting in vain.
pub fn wait_for_lock(path: &str, limit: Duration) -> bool {
    let deadline = Instant::now() + limit;
    loop {
        if acquire_lock(path) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

// ============================================================================
// Restart: the new process waits for the old one to leave
// ============================================================================
//
// Live case 08.10.2026: «Restart» after changing the language — and Rexon did not
// come back. The old scheme waited for the old process to exit from outside (`sh` with
// `kill -0` for up to 30 s) and started the new one «blindly»: if the old one did not leave in time,
// the new one hit the lock, knocked on the leaving window and quietly exited, and then
// the old one left too. Now the new process knows it is a restart
// ([`RESTART_OF_ENV`] = pid of the previous one) and waits for the lock to be released itself
// ([`wait_for_previous`]); each step is a line in the log.

/// The variable «I am a restart of process <pid>» — set by the restarting side.
pub const RESTART_OF_ENV: &str = "REXON_RESTART_OF";

/// How long the new process waits for the previous one to leave. The previous one's exit is bounded
/// by the exit watchdog (`main_window`: `EXIT_WATCHDOG`), so there is no point waiting longer
/// than that, and less means leaving before the old one again.
pub const RESTART_LOCK_WAIT: Duration = Duration::from_secs(45);

/// Whether process `pid` is really alive (a zombie is no longer alive: it released its
/// locks, yet `kill -0` on it answers «exists»).
pub fn process_alive(pid: u32) -> bool {
    if pid == 0 || unsafe { libc::kill(pid as libc::pid_t, 0) } != 0 {
        // EPERM — the process exists but belongs to someone else; for our case it is «alive».
        return std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM);
    }
    match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        // The state is the first field after the closing parenthesis of the name.
        Ok(stat) => stat
            .rsplit_once(") ")
            .and_then(|(_, rest)| rest.chars().next())
            .is_none_or(|state| state != 'Z' && state != 'X'),
        Err(_) => false,
    }
}

/// Who holds the `flock` on file `path` — from `/proc/locks`. For the log:
/// «lock taken» without the holder's name explains nothing.
pub fn lock_holder_pid(path: &str) -> Option<u32> {
    let meta = std::fs::metadata(path).ok()?;
    let dev = meta.dev();
    let (major, minor) = (libc::major(dev), libc::minor(dev));
    let ino = meta.ino();
    let locks = std::fs::read_to_string("/proc/locks").ok()?;
    lock_holder_in(&locks, major, minor, ino)
}

/// Parse `/proc/locks` lines: `1: FLOCK ADVISORY WRITE <pid> <maj>:<min>:<ino> 0 EOF`.
pub fn lock_holder_in(locks: &str, major: u32, minor: u32, ino: u64) -> Option<u32> {
    locks.lines().find_map(|line| {
        let f: Vec<&str> = line.split_whitespace().collect();
        // Lines of waiters (` -> FLOCK …`) are shifted by one field — skip them.
        if f.len() < 6 || f[1] != "FLOCK" {
            return None;
        }
        let mut id = f[5].split(':');
        let maj = u32::from_str_radix(id.next()?, 16).ok()?;
        let min = u32::from_str_radix(id.next()?, 16).ok()?;
        let i: u64 = id.next()?.parse().ok()?;
        (maj == major && min == minor && i == ino).then(|| f[4].parse().ok()).flatten()
    })
}

/// How waiting for the previous process ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreviousWait {
    /// The lock is ours — the previous one left.
    Acquired(Duration),
    /// The previous one never left within the allotted time.
    PreviousStillRunning,
    /// The previous one left, but someone else holds the lock (pid, if known):
    /// for example, Rexon was started by hand while the restart was in progress.
    OtherHolder(Option<u32>),
}

/// Wait for lock `path` to be released while the previous process `previous` is alive,
/// but no longer than `limit`.
pub fn wait_for_previous(path: &str, previous: u32, limit: Duration) -> PreviousWait {
    let started = Instant::now();
    let deadline = started + limit;
    // After the previous one dies the lock is released immediately; half a second of margin
    // in case the kernel has not finished cleaning up the descriptors.
    let mut gone_since: Option<Instant> = None;
    loop {
        if acquire_lock(path) {
            return PreviousWait::Acquired(started.elapsed());
        }
        let now = Instant::now();
        if !process_alive(previous) {
            let since = *gone_since.get_or_insert(now);
            if now.duration_since(since) > Duration::from_millis(500) {
                return PreviousWait::OtherHolder(lock_holder_pid(path));
            }
        }
        if now >= deadline {
            return PreviousWait::PreviousStillRunning;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// How the request to give up the lock ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandoverOutcome {
    /// The holder is the classic window: it raised itself, this launch should leave.
    Raised,
    /// The holder left, the lock is ours now: the launch continues as the first.
    Acquired,
    /// Did not work out: no listener, it refused, or the lock was not released in time.
    Failed,
}

/// Ask the holder of lock `lock_path` to make way for the classic window.
pub fn request_handover(socket_path: &str, lock_path: &str, wait: Duration) -> HandoverOutcome {
    let Ok(reply) = exchange(socket_path, HANDOVER_REQUEST, current_uid()) else {
        return HandoverOutcome::Failed;
    };
    if knock_accepted(&reply) {
        return HandoverOutcome::Raised;
    }
    if trimmed(&reply) == trimmed(REPLY_LEAVING) && wait_for_lock(lock_path, wait) {
        return HandoverOutcome::Acquired;
    }
    HandoverOutcome::Failed
}

/// Whether a listener is alive at the address — WITHOUT asking it to raise the window.
///
/// Deliberately separate from the knock: the liveness check is needed by whoever IS ABOUT
/// to listen, and it must not poke another instance's window as a side
/// effect of its own launch.
pub fn socket_alive(path: &str) -> bool {
    UnixStream::connect(path).is_ok()
}

// ============================================================================
// Listener
// ============================================================================

/// Why we could not come up as the listener.
#[derive(Debug)]
pub enum BindError {
    /// Someone is already listening at the address: we are not the first.
    Busy,
    /// The address is taken by something that is NOT a socket — a foreign file must not be removed.
    NotASocket,
    /// A file system or kernel error.
    Io(std::io::Error),
    /// The address does not fit into `sun_path` (path, length — in the text).
    TooLong(String),
    /// The fallback directory is unfit: a foreign owner, a link, not a directory.
    UnsafeDir { dir: String, reason: String },
}

impl std::fmt::Display for BindError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BindError::Busy => write!(f, "на адресе уже слушает живой Rexon"),
            BindError::NotASocket => write!(f, "адрес занят не сокетом"),
            BindError::Io(e) => write!(f, "{e}"),
            BindError::TooLong(path) => write!(f, "{}", too_long(path)),
            BindError::UnsafeDir { dir, reason } => {
                write!(f, "запасной каталог {dir} непригоден: {reason}")
            }
        }
    }
}

/// Free the address for the listener: remove the file only if it is a DEAD
/// socket. `Ok(())` — the address is free (there was no file, or the dead one was removed).
///
/// One for both listeners: Rust ([`ActivationListener::bind`]) and Python
/// (`QLocalServer` in `rexon/qt_main/app.py`). The Python listener used to call
/// `QLocalServer::removeServer`, which removes the file UNCONDITIONALLY — and running
/// the module directly, bypassing the lock, took the live shared socket away from the running
/// Rexon, leaving it without a door for second launches.
///
/// It also prepares the fallback directory ([`socket_path`] with a long
/// `XDG_RUNTIME_DIR`): the Python listener calls it before `QLocalServer.listen`,
/// and the 0700 directory appears for both listeners from one piece of code.
pub fn clear_stale_socket(path: &str) -> Result<(), BindError> {
    if path.len() > SUN_PATH_MAX {
        return Err(BindError::TooLong(path.to_string()));
    }
    let uid = current_uid();
    let fallback_dir = fallback_socket_dir(uid);
    if std::path::Path::new(path).parent() == Some(std::path::Path::new(&fallback_dir)) {
        ensure_private_dir(&fallback_dir, uid)?;
    }
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return Ok(());
    };
    if socket_alive(path) {
        return Err(BindError::Busy);
    }
    if !meta.file_type().is_socket() {
        return Err(BindError::NotASocket);
    }
    std::fs::remove_file(path).map_err(BindError::Io)
}

/// Listener of «raise the window» requests from second launches.
///
/// Port of `rexon/qt_main/app.py::_serve_activation` without Qt: a `QLocalServer` on
/// Unix is the same `AF_UNIX` socket, and the core does not need Qt for it.
pub struct ActivationListener {
    listener: UnixListener,
    path: String,
    /// Socket inode at the moment of capture — see [`Drop`].
    inode: u64,
    /// Whose knock is accepted: by default — our own uid ([`current_uid`]).
    owner_uid: u32,
}

impl ActivationListener {
    /// Come up as the listener on `path`.
    ///
    /// ⚠️⚠️ WHY NOT `QLocalServer::removeServer`, AS IN PYTHON. That one removes
    /// the file at the address UNCONDITIONALLY — and that is exactly the trap that already
    /// cost the project a session: `$XDG_RUNTIME_DIR` is shared by the whole session, and
    /// a run that removed someone else's socket took a live program's address away.
    /// So here the order is reversed: first we ask whether a LIVE Rexon is listening at
    /// the address (`socket_alive`), and if so we leave with
    /// [`BindError::Busy`] without removing anything. We remove only a dead file, and
    /// only if it really is a socket.
    pub fn bind(path: &str) -> Result<ActivationListener, BindError> {
        clear_stale_socket(path)?;
        let listener = UnixListener::bind(path).map_err(BindError::Io)?;
        let inode = std::fs::symlink_metadata(path)
            .map(|m| m.ino())
            .map_err(BindError::Io)?;
        Ok(ActivationListener {
            listener,
            path: path.to_string(),
            inode,
            owner_uid: current_uid(),
        })
    }

    /// Accept knocks only from user `uid` (by default — our own).
    ///
    /// ⚠️ A foreign user's knock is refused without reading the word: otherwise a process
    /// of another user could poke the window or, worse, close Rexon with the word
    /// `handover` and take its place.
    pub fn with_owner_uid(mut self, uid: u32) -> ActivationListener {
        self.owner_uid = uid;
        self
    }

    /// The address we are listening on.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Serve ONE request: accept a connection, read the word, raise the
    /// window and reply based on the outcome.
    ///
    /// `Some(raised)` — the request was served; `None` — the connection did not
    /// happen or something other than «show» arrived on it (then we do not touch the window and
    /// reply with a refusal: a foreign knock at our address is no reason to poke the screen).
    pub fn serve_one<F: FnMut() -> bool>(&self, raise: &mut F) -> Option<bool> {
        self.serve_one_to(&mut |_section| raise())
    }

    /// The same, but `raise` gets the section from the request (`None` — no section).
    /// Such a listener does not hand over the host: on `handover` — the refusal «no».
    pub fn serve_one_to<F: FnMut(Option<(String, String)>) -> bool>(
        &self,
        raise: &mut F,
    ) -> Option<bool> {
        self.serve_one_with(None, &mut |action| match action {
            ListenerAction::Raise(section) => raise(section),
            ListenerAction::HandOver => false,
        })
    }

    /// Listener of holder `holder` that can hand over the host
    /// ([`listener_action`]).
    ///
    /// ⚠️ On [`ListenerAction::HandOver`] the handler must NOT exit by itself,
    /// but schedule an exit (in the GUI thread: close the window and the icon, exit —
    /// the lock goes away with the process) and return `true`: the reply «bye» is sent
    /// AFTER it returns. `false` — staying, the launch gets a refusal.
    pub fn serve_one_for<F: FnMut(ListenerAction) -> bool>(
        &self,
        holder: Holder,
        handler: &mut F,
    ) -> Option<bool> {
        self.serve_one_with(Some(holder), handler)
    }

    fn serve_one_with<F: FnMut(ListenerAction) -> bool>(
        &self,
        holder: Option<Holder>,
        handler: &mut F,
    ) -> Option<bool> {
        let (mut conn, _) = self.listener.accept().ok()?;
        if peer_uid(&conn) != Some(self.owner_uid) {
            let _ = conn.set_write_timeout(Some(KNOCK_TIMEOUT));
            let _ = conn.write_all(REPLY_FAILED);
            let _ = conn.shutdown(std::net::Shutdown::Both);
            return None;
        }
        let _ = conn.set_read_timeout(Some(KNOCK_TIMEOUT));
        let _ = conn.set_write_timeout(Some(KNOCK_TIMEOUT));
        let mut buf = [0u8; WIRE_BUF];
        let read = conn.read(&mut buf).unwrap_or(0);
        let word = &buf[..read];
        let action = match holder {
            Some(holder) => listener_action(holder, word),
            // A listener without host handover understands only «show».
            None => parse_show_request(word).map(|show| ListenerAction::Raise(show.section)),
        };
        let Some(action) = action else {
            let _ = conn.write_all(REPLY_FAILED);
            let _ = conn.flush();
            return None;
        };
        let leaving = action == ListenerAction::HandOver;
        let raised = handler(action);
        let reply = match (leaving, raised) {
            (true, true) => REPLY_LEAVING,
            (true, false) => REPLY_FAILED,
            (false, raised) => reply_for(raised),
        };
        let _ = conn.write_all(reply);
        let _ = conn.flush();
        // Close right after the reply: the second launch waits for one word and
        // leaves, there is nothing to keep the connection open with and no reason to.
        let _ = conn.shutdown(std::net::Shutdown::Both);
        Some(raised)
    }

    /// Serve requests until the end of the process.
    pub fn serve_forever<F: FnMut() -> bool>(&self, mut raise: F) {
        self.serve_forever_to(move |_section| raise())
    }

    /// The same, but `raise` gets the section from the request.
    pub fn serve_forever_to<F: FnMut(Option<(String, String)>) -> bool>(&self, mut raise: F) {
        loop {
            // A broken connection is no reason to stop listening: otherwise
            // one malformed knock would deprive the user of the door into the program until
            // a restart.
            let _ = self.serve_one_to(&mut raise);
        }
    }

    /// Come up as the listener and serve requests in its own thread.
    ///
    /// ⚠️ `raise` is called FROM THIS THREAD. The caller is responsible for making sure
    /// that window work inside `raise` lands in the GUI thread (Qt does not allow raising
    /// a window from another thread).
    pub fn spawn<F>(path: &str, raise: F) -> Result<(), BindError>
    where
        F: FnMut() -> bool + Send + 'static,
    {
        let mut raise = raise;
        ActivationListener::spawn_to(path, move |_section| raise())
    }

    /// The same, but `raise` gets the section from the request (a shortcut quick action
    /// with Rexon running). ⚠️ Called from the listener thread — as with `spawn`.
    pub fn spawn_to<F>(path: &str, raise: F) -> Result<(), BindError>
    where
        F: FnMut(Option<(String, String)>) -> bool + Send + 'static,
    {
        let server = ActivationListener::bind(path)?;
        std::thread::Builder::new()
            .name("rexon-activate".to_string())
            .spawn(move || server.serve_forever_to(raise))
            .map_err(BindError::Io)?;
        Ok(())
    }

    /// Serve requests of holder `holder` until the end of the process.
    pub fn serve_forever_for<F: FnMut(ListenerAction) -> bool>(&self, holder: Holder, mut handler: F) {
        loop {
            let _ = self.serve_one_for(holder, &mut handler);
        }
    }

    /// Come up as the listener of holder `holder` in its own thread — with host
    /// handover ([`Self::serve_one_for`]). ⚠️ `handler` is called from the listener
    /// thread, as with `spawn`.
    pub fn spawn_for<F>(path: &str, holder: Holder, handler: F) -> Result<(), BindError>
    where
        F: FnMut(ListenerAction) -> bool + Send + 'static,
    {
        let server = ActivationListener::bind(path)?;
        std::thread::Builder::new()
            .name("rexon-activate".to_string())
            .spawn(move || server.serve_forever_for(holder, handler))
            .map_err(BindError::Io)?;
        Ok(())
    }
}

impl Drop for ActivationListener {
    fn drop(&mut self) {
        // Remove ONLY our own socket: compare the inode with the one we captured.
        // Otherwise a listener that came up on our address after us would lose it
        // because of our own shutdown — the very same theft of the shared socket, only
        // in reverse.
        let ours = std::fs::symlink_metadata(&self.path)
            .map(|m| m.file_type().is_socket() && m.ino() == self.inode)
            .unwrap_or(false);
        if ours {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

// ============================================================================
// Rexon was updated, but the old copy is running
// ============================================================================
//
// The package manager does not touch a running process: it puts the new file
// next to it and renames it into the old one's place. The window and the tray live
// on as the old copy — until logout, sometimes for weeks — and the person
// does not understand why the «updated» Rexon behaves the old way. Signs of
// the replacement are visible right from the process: the kernel appends « (deleted)» to the
// `/proc/self/exe` link if the file the process was started from was removed from disk, and
// the old path holds a file with a different inode/modification time.
// The decision is here (pure functions under tests on a fake path), polling and
// the notification with the «Restart» button are in the tray (`rexon-gui::tray`).

/// The tail the kernel appends to a link to a file removed from disk.
pub const DELETED_EXE_SUFFIX: &str = " (deleted)";

/// The process's link to its own executable.
pub const PROC_SELF_EXE: &str = "/proc/self/exe";

/// How often the tray compares its binary with the disk, s. Polling, not inotify:
/// it is a `readlink` and two `stat`s every half minute, while watching the package
/// directory would wake the tray on every file of any system update.
pub const SELF_UPDATE_POLL_SECS: u64 = 30;

/// How long to wait for the notification button to be pressed, s: a day — the person may have stepped away.
pub const SELF_UPDATE_NOTICE_WAIT_SECS: u64 = 24 * 3600;

/// File fingerprint: device, inode and modification time. If any of the
/// three changed, there is a different file on disk now (or the same one rewritten in place).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FileStamp {
    pub dev: u64,
    pub ino: u64,
    pub mtime_sec: i64,
    pub mtime_nsec: i64,
}

impl FileStamp {
    /// File fingerprint by path (links are resolved). `None` — no file.
    pub fn of(path: &std::path::Path) -> Option<Self> {
        let meta = std::fs::metadata(path).ok()?;
        Some(Self { dev: meta.dev(), ino: meta.ino(), mtime_sec: meta.mtime(), mtime_nsec: meta.mtime_nsec() })
    }
}

/// What happened to the running process's executable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExeChange {
    /// What is running is what lies on disk.
    Same,
    /// A different file lies at the old path — a new version. `stamp` is its
    /// fingerprint: by it the notification is shown once per update.
    Replaced { path: String, stamp: FileStamp },
    /// There is no file at the old path at all (the package was removed or the replacement is still
    /// in progress) — nothing to restart into, nothing to offer.
    Gone,
}

/// Decision from observations: `link_target` — contents of the
/// `/proc/self/exe` link, `running` — fingerprint of the running file, `on_disk` —
/// fingerprint of the file at the path from the link (without the « (deleted)» tail).
pub fn exe_change(link_target: &str, running: Option<FileStamp>, on_disk: Option<FileStamp>) -> ExeChange {
    let (path, deleted) = match link_target.strip_suffix(DELETED_EXE_SUFFIX) {
        Some(p) => (p, true),
        None => (link_target, false),
    };
    let Some(stamp) = on_disk else {
        return if deleted { ExeChange::Gone } else { ExeChange::Same };
    };
    let replaced = deleted || running.is_some_and(|r| r != stamp);
    if replaced {
        ExeChange::Replaced { path: path.to_string(), stamp }
    } else {
        ExeChange::Same
    }
}

/// Live check via link `link` (in the product — [`PROC_SELF_EXE`], in tests
/// — a fake link). `running` — fingerprint of the running file; for
/// `/proc/self/exe` it is `FileStamp::of(link)`: through it the kernel returns the
/// running file itself, even one removed from disk.
pub fn exe_change_at(link: &std::path::Path, running: Option<FileStamp>) -> ExeChange {
    let Ok(target) = std::fs::read_link(link) else {
        return ExeChange::Same;
    };
    let target = target.to_string_lossy().into_owned();
    let path = target.strip_suffix(DELETED_EXE_SUFFIX).unwrap_or(&target);
    exe_change(&target, running, FileStamp::of(std::path::Path::new(path)))
}

/// Memory of the «Rexon updated» notification: show it ONCE per update and
/// not before the replacement has settled.
///
/// Settled means the same new file is seen on two polls
/// in a row: in the middle of a package manager transaction the binary is already new while
/// the libraries next to it are still old, and a restart at that second would only crash.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SelfUpdateNotice {
    /// The fingerprint already announced.
    told: Option<FileStamp>,
    /// A fingerprint noticed by the previous poll and not yet confirmed.
    pending: Option<FileStamp>,
}

impl SelfUpdateNotice {
    /// A polling step. `true` — show the notification now.
    pub fn step(&mut self, change: &ExeChange) -> bool {
        let ExeChange::Replaced { stamp, .. } = change else {
            self.pending = None;
            return false;
        };
        if self.told == Some(*stamp) {
            return false;
        }
        if self.pending == Some(*stamp) {
            self.pending = None;
            self.told = Some(*stamp);
            return true;
        }
        self.pending = Some(*stamp);
        false
    }

    /// A polling step on a nudge ([`nudge_self_update_watch`]): the update has already been
    /// confirmed by the privileged session helper of another build, a second «has it settled»
    /// poll is not needed. `true` — show the notification now (we do not speak about the same
    /// file a second time).
    pub fn confirm(&mut self, change: &ExeChange) -> bool {
        let ExeChange::Replaced { stamp, .. } = change else {
            return false;
        };
        self.pending = None;
        if self.told == Some(*stamp) {
            return false;
        }
        self.told = Some(*stamp);
        true
    }
}

/// The nudge to the «Rexon updated» watcher and waiting for it.
static SELF_UPDATE_NUDGE: (Mutex<bool>, std::sync::Condvar) = (Mutex::new(false), std::sync::Condvar::new());

/// Wake the «Rexon updated» watcher now without waiting for the poll: the privileged session
/// replied «helper of another build» (the package was updated with the window open,
/// B13). The notification is the same and single — there is no second window about the update.
pub fn nudge_self_update_watch() {
    let (flag, cv) = &SELF_UPDATE_NUDGE;
    *flag.lock().unwrap_or_else(|e| e.into_inner()) = true;
    cv.notify_all();
}

/// Wait for the watcher's next poll: `timeout` or a nudge. `true` — nudged.
pub fn wait_self_update_poll(timeout: Duration) -> bool {
    let (flag, cv) = &SELF_UPDATE_NUDGE;
    let guard = flag.lock().unwrap_or_else(|e| e.into_inner());
    let (mut guard, _) = cv
        .wait_timeout_while(guard, timeout, |nudged| !*nudged)
        .unwrap_or_else(|e| e.into_inner());
    std::mem::take(&mut *guard)
}

/// Arguments of the new copy when restarting for an update: the same as the
/// previous one's, but without the hidden start, the self-check and the shortcut section.
/// The person pressed «Restart» themselves — the new copy's window is the answer that
/// the restart happened; silently returning to the tray would leave them
/// guessing.
pub fn update_restart_args(args: &[String]) -> Vec<String> {
    args.iter()
        .filter(|a| {
            *a != crate::tray::START_HIDDEN_FLAG
                && *a != "--selftest"
                && !a.starts_with(crate::cli::SECTION_FLAG)
                && !a.starts_with(crate::app::PAGE_FLAG)
        })
        .cloned()
        .collect()
}

/// Key of the «Restart» button in the notification.
pub const SELF_UPDATE_RESTART_ACTION: &str = "restart";

pub fn self_update_title() -> String {
    // TRANSLATORS: Title of a desktop notification: the Rexon package was updated while Rexon kept running.
    tr("Rexon was updated")
}

pub fn self_update_body() -> String {
    // TRANSLATORS: Body of the "Rexon was updated" notification; the button below it says "Restart".
    tr("A new version of Rexon is installed, but the old one is still running. Restart Rexon to start using the new version.")
}

/// Notification buttons.
pub fn self_update_actions() -> Vec<(String, String)> {
    // TRANSLATORS: Button in the "Rexon was updated" notification.
    vec![(SELF_UPDATE_RESTART_ACTION.to_string(), tr("Restart Rexon"))]
}

/// Notification title if the restart failed (the text is the reason).
pub fn self_update_restart_failed_title() -> String {
    // TRANSLATORS: Title of a desktop notification when Rexon failed to come back after a restart.
    tr("Rexon didn't restart")
}

/// What to do yourself if the notification service does not support buttons.
pub fn self_update_no_buttons() -> String {
    // TRANSLATORS: Added to the "Rexon was updated" notification when the system can't show buttons.
    tr("To restart, choose “Quit” in the menu of the Rexon tray icon and open Rexon again.")
}

// ============================================================================
// What to tell the person
// ============================================================================
//
// No entry point may keep its own copy of these strings: the user
// sees them, so a difference between the Python and Rust entries would be
// a difference in the product.

/// The second launch raised the first one's window.
pub fn said_raised() -> String {
    tr("↑ Rexon is already running — brought its window to the front.")
}
/// The second launch hit the lock and could not raise the window.
pub fn said_already_running() -> String {
    tr("⚠️  Rexon is already running — not adding a second tray icon.")
}
/// A line with the lock address (for a second launch that left empty-handed).
pub fn said_lock_path(lock_path: &str) -> String {
    format!("    {}", trf("Lock: %1", &[&lock_path]))
}
/// Where to look when the window could not be raised.
pub fn said_look_in_tray() -> String {
    format!(
        "    {}",
        tr("Couldn't bring up the window — look for the Rexon icon in the system tray.")
    )
}
/// Rexon is running (the lock is taken), but did not answer the request to show the window:
/// the listener did not come up, the address is unfit or the program is not responding. The former
/// lines («not creating a second icon» and the lock path) were misleading
/// here: the person asked for a window, not an icon.
pub fn said_not_responding() -> String {
    tr("⚠️  Rexon is already open but doesn't respond to the request to show its window. \
        Open it from the icon in the system tray or restart Rexon.")
}

/// What to tell a launch that hit a taken lock: `knock` — the knock's outcome,
/// `None` — there was no knock (a hidden launch). One decision for all entries.
pub fn said_on_busy_lock(lock_path: &str, knock: Option<&KnockOutcome>) -> Vec<String> {
    let already = || vec![said_already_running(), said_lock_path(lock_path)];
    match knock {
        Some(KnockOutcome::Raised) => vec![said_raised()],
        Some(KnockOutcome::NoAnswer(_)) => vec![said_not_responding()],
        Some(KnockOutcome::Refused) => {
            let mut lines = already();
            lines.push(said_look_in_tray());
            lines
        }
        None => already(),
    }
}

// ============================================================================
// Launch notification (X11, freedesktop startup-notification)
// ============================================================================
//
// 2026-11-01, «the window is open, but the cursor keeps thinking». MATE on X11: Rexon is already
// running, the person clicks the menu item — the second launch hits the lock,
// knocks on the first (the window is raised) and exits. The panel, however, started it with
// `DESKTOP_STARTUP_ID` and waits for the reply «remove: ID=…» in `_NET_STARTUP_INFO`:
// until then the cursor «thinks», and the panel shows «Starting Rexon». The reply
// is usually sent by Qt itself (`QXcbScreen::windowShown` when the first
// top-level window is shown), but the second launch never reaches `QApplication` — and
// nobody replied until the panel gave up on a timeout. The owner's log:
// every such launch from the menu — «lock taken» → «knock: Raised» → exit.
//
// Now a launch that leaves without its own window completes the notification itself.

/// The variable in which the launcher (panel, menu) passes the launch ID.
pub const STARTUP_ID_ENV: &str = "DESKTOP_STARTUP_ID";

/// Launch ID from the environment; an empty value — there was no launch with a notification.
pub fn startup_id_from(value: Option<&str>) -> Option<String> {
    value.map(str::trim).filter(|v| !v.is_empty()).map(str::to_string)
}

/// The «launch finished» message per the startup-notification spec:
/// the value is quoted if it contains a space, a quote or a backslash.
pub fn startup_remove_message(id: &str) -> String {
    let needs_quotes = id.chars().any(|c| c == ' ' || c == '"' || c == '\\');
    if needs_quotes {
        let escaped: String = id
            .chars()
            .flat_map(|c| match c {
                '"' | '\\' => vec!['\\', c],
                _ => vec![c],
            })
            .collect();
        format!("remove: ID=\"{escaped}\"")
    } else {
        format!("remove: ID={id}")
    }
}

/// Slice the message into 20-byte X client messages, with a terminating
/// zero (as in libstartup-notification and in Qt): the first piece goes with type
/// `_NET_STARTUP_INFO_BEGIN`, the rest with `_NET_STARTUP_INFO`.
pub fn startup_message_chunks(message: &str) -> Vec<[u8; 20]> {
    let mut bytes = message.as_bytes().to_vec();
    bytes.push(0);
    bytes
        .chunks(20)
        .map(|c| {
            let mut out = [0u8; 20];
            out[..c.len()].copy_from_slice(c);
            out
        })
        .collect()
}

/// Send «remove: ID=…» to the root window of display `display` (`None` —
/// the process's `DISPLAY`). libX11 is loaded at call time: without it (pure
/// Wayland) — an honest error, not a crash.
pub fn send_startup_remove(display: Option<&str>, id: &str) -> Result<(), String> {
    use std::ffi::CString;
    use x11_dl::xlib;

    let lib = xlib::Xlib::open().map_err(|e| format!("libX11 не загрузилась: {e}"))?;
    let name = display
        .map(|d| CString::new(d).map_err(|_| "имя дисплея с нулевым байтом".to_string()))
        .transpose()?;
    let chunks = startup_message_chunks(&startup_remove_message(id));
    // SAFETY: our own display, opened and closed right here; the event is filled
    // completely (zeroed), the pointers live until the end of the block.
    unsafe {
        let dpy = (lib.XOpenDisplay)(name.as_ref().map_or(std::ptr::null(), |n| n.as_ptr()));
        if dpy.is_null() {
            return Err(format!("дисплей {} не открылся", display.unwrap_or("$DISPLAY")));
        }
        let root = (lib.XDefaultRootWindow)(dpy);
        let begin = (lib.XInternAtom)(dpy, c"_NET_STARTUP_INFO_BEGIN".as_ptr(), xlib::False);
        let more = (lib.XInternAtom)(dpy, c"_NET_STARTUP_INFO".as_ptr(), xlib::False);
        for (i, chunk) in chunks.iter().enumerate() {
            let mut ev: xlib::XClientMessageEvent = std::mem::zeroed();
            ev.type_ = xlib::ClientMessage;
            ev.display = dpy;
            ev.window = root;
            ev.message_type = if i == 0 { begin } else { more };
            ev.format = 8;
            for (k, b) in chunk.iter().enumerate() {
                ev.data.as_bytes_mut()[k] = *b as std::os::raw::c_char;
            }
            let mut xev = xlib::XEvent::from(ev);
            (lib.XSendEvent)(dpy, root, xlib::False, xlib::PropertyChangeMask, &mut xev);
        }
        (lib.XSync)(dpy, xlib::False);
        (lib.XCloseDisplay)(dpy);
    }
    Ok(())
}

/// The launch leaves without its own window (the lock is taken): complete the launch
/// notification if there was one. `None` — there was no notification; otherwise — the ID and the outcome
/// of sending. We do not unset the variable: a repeated «remove» of the same ID (if a Qt
/// child inherits it) is harmless, while unsetting it with live threads is not.
pub fn complete_startup_notification() -> Option<(String, Result<(), String>)> {
    let id = startup_id_from(std::env::var(STARTUP_ID_ENV).ok().as_deref())?;
    let has_x = std::env::var_os("DISPLAY").is_some_and(|d| !d.is_empty());
    let sent = if has_x {
        send_startup_remove(None, &id)
    } else {
        Err("нет DISPLAY — уведомление X11 некому завершать".to_string())
    };
    Some((id, sent))
}
