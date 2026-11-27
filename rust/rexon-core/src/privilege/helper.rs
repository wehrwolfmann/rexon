//! Root helper of the privilege session (item B4 of the plan, see [`super`]).
//!
//! The `rexon-priv-helper` binary is a thin `main` over [`main_live`]. The window
//! raises it with one `pkexec` and hands its end of a `socketpair` over on fd 0/1;
//! then the helper:
//! 1. makes sure it was started through pkexec (`PKEXEC_UID`, euid 0), cleans its
//!    environment, moves the socket off fd 0/1 (children will not get it);
//! 2. checks the peer (`SO_PEERCRED`: uid is `PKEXEC_UID`, pid is the parent,
//!    [`validate::check_peer`]);
//! 3. accepts `Hello` ([`proto::check_hello`]), then asks logind itself about
//!    the window's session: `User` is whoever entered the password, `Active=yes`.
//!    It does not trust the session number from the argument, only compares it;
//! 4. takes one operation at a time: [`Revoker::op_started`] → all checks of
//!    [`validate`] ([`check_op`]) → execution without a shell (explicit argv,
//!    [`RootIo`]) → `Done` → [`Revoker::op_finished`];
//! 5. goes away when the privilege is revoked ([`super::revoke`]): socket break,
//!    SIGTERM, idling, lock, sleep, session change. A running operation is finished.
//!
//! # What the helper does not trust
//!
//! The window may be replaced (U2p), so everything that decides "allowed or not"
//! the helper finds out itself: the package manager family from the os-release
//! of this machine, the current desktop from the logind session, SteamOS from
//! os-release. Only `PrivOp` is taken from the window, and it is checked in full
//! before the first action.
//!
//! # Execution (Z1)
//!
//! Neither a shell nor `eval`: each command is a `std::process::Command` with an
//! explicit argv from the core tables ([`plan::install_cmd`], [`plan::remove_cmd`]),
//! with a clean environment ([`child_env`]), working directory `/` and `stdin`
//! from `/dev/null`. Program output goes to the window as `Progress` lines no
//! longer than [`PROGRESS_MAX`].
//!
//! # What is executed now
//!
//! | Operation | Answer |
//! |---|---|
//! | `PkgInstall` (a name only from the named `PkgSource` table; the tables are exactly the allowlist of `rexon-pkg-helper`), `PkgRemove`, `EnableDm`, `SwitchDm` (with automatic rollback), `BuildDeps` | executed |
//! | `InstallBuilt`, `Stage`, `SystemTheme` | executed; a file only as an open fd, a root copy through [`super::unpack`] |
//! | `LoginSessionWayland` (the login screen's default session) | executed: the X11 session is from the core table, the pair file lies in the Wayland session directory; only the value of the SDDM and Plasma Login Manager session keys pointing to this X11 session is changed, in `/etc` and in SDDM's last session (not links); after the edit, by the same reading the guard uses, the login screen does not open it by itself; otherwise nothing is written |
//! | `DeInstallSet` (desktop as a set: DKMS `--asdeps`, replacement `--ask=4`, Deepin on Treeland) | executed; before the first action root repeats the core probe itself by the pacman of this machine ([`validate::check_de_set_preview`]) |
//! | `Sysext` (SteamOS) | executed; scripts and units are texts from its own binary ([`crate::steamos_sysext`]), written to `/var/lib/rexon` and `/etc`, `/usr` is not touched |
//! | `AurDeps`, `AurInstallBuilt`, `AurDropMake` (AUR shim, B10) | executed: dependencies are only names from the sync database of this machine; built packages come as an fd, the `.PKGINFO` of the copy is from the window's list and not the name of an official package; removal is only of orphan dependencies |
//! | `OrphanRemove` (an update guard orphan) | executed: each name is a member of an update guard set, installed and absent from the sync database of this machine; everything `pacman -Rns` would remove (probe `-p`) is not vital and not a package of the current desktop |
//! | `CacheRollback` (rolling back an update guard set from the pacman cache) | executed: the file only from the cache directory of this machine (`pacman-conf CacheDir`), opened by root itself without links (`openat2`), owner root, not writable by group/others; the name and version come from the `.PKGINFO` of the copy; the set is exactly the rollback of one guard finding by the pacman of this machine, versions strictly lower than the installed ones (`vercmp`); the `pacman -U --print` probe is exactly these packages; one transaction |
//! | `FontInstall`, `FontRemove` (fonts "for all users") | executed: only the core directory `/usr/local/share/fonts` (package fonts are not touched); install: the file as an open fd, a root copy, `fc-query` on the copy, the name is a font file name, an existing file or link with that name means refusal (no overwriting); removal: only a regular font file inside the directory, not through a link; then `fc-cache -f` of the directory |
//! | `SddmBackground`, `SddmClock`, `SddmLook` (SDDM login screen: theme background, «Show clock», carrying over the desktop look) | executed: the theme is a directory in `/usr/share/sddm/themes` (not a link) with a settings file from `metadata.desktop`; the clock is the `showClock` key in `<ConfigFile>.user`; the picture is an fd, a root copy, PNG/JPEG (WebP if the theme greeter's Qt has the WebP module) by signature, it goes in as `rexon-background.<ext>`, the name into `<ConfigFile>.user` (our previous one is removed, foreign files are not touched); carry-over: keys by the core rule `plan_sddm_look` with rollback, values checked, `-dpi` assembled by the helper; files come as fds, only names of the core table, UTF-8 up to 1 MiB, in the `sddm` home they are written as `sddm` (`setpriv`), not root; if the files fail, the keys are restored |
//! | `AurInstall`, `AeroShellRebuild` | `Error{Refused, "not_for_helper"}`: the build is done by paru/yay as the user, root gets only pacman calls through the shim (B10) |
//! | others | `Error{Refused, "not_supported_yet"}`: silently not executed |

use std::collections::{BTreeSet, HashSet};
use std::ffi::CString;
use std::io::{Read, Write};
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, OwnedFd};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use super::op::{self, PrivOp, SysextOp, SystemThemeOp};
use super::proto::{self, ClientMsg, ErrorCode, Hello, HelperMsg, ProtoError};
use super::revoke::{
    self, BootMinusMonotonic, Clock, Event, LogindWatch, MonotonicClock, Reason, Revoker,
    SessionAnswer, SessionQuery, SessionWatch,
};
use super::unpack::{self, UnpackError, WorkDir};
use super::validate::{self, Refusal, SystemThemeKind};
use crate::distro_compat::SteamOsBlock;
use crate::system::{self, DistroDetector, PkgFamily};
use crate::system_plan as plan;

/// The longest `Progress` line in bytes; longer ones are cut.
pub const PROGRESS_MAX: usize = 4096;

/// `PATH` of the helper and its children. Neither `~/.local/bin` nor `/usr/local`:
/// a program name is looked up only in system directories.
pub const CHILD_PATH: &str = "/usr/bin:/usr/sbin";

/// Program search directories: the same as in [`CHILD_PATH`].
const SEARCH_DIRS: &[&str] = &["/usr/bin", "/usr/sbin"];

/// Name in the journal (`SYSLOG_IDENTIFIER`).
pub const JOURNAL_ID: &str = "rexon-priv-helper";

/// journald socket for entries in the native format.
const JOURNAL_SOCKET: &str = "/run/systemd/journal/socket";

/// Helper exit codes. 126/127 are not used: they are pkexec's own codes
/// (password refused, no permission), by which the client decides there is no session.
pub const EXIT_OK: i32 = 0;
/// Wrong arguments.
pub const EXIT_USAGE: i32 = 2;
/// Not started through pkexec (no `PKEXEC_UID`, or euid is not 0).
pub const EXIT_NOT_PKEXEC: i32 = 3;
/// No window socket on fd 0, the parent died, the peer is foreign.
pub const EXIT_CHANNEL: i32 = 4;
/// The handshake or the logind session check failed.
pub const EXIT_HANDSHAKE: i32 = 5;

// ============================================================================
// Seam to the system
// ============================================================================

/// Exit code and standard output of a probe.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Captured {
    /// Exit code; `None` means it did not start or was killed by a signal.
    pub code: Option<i32>,
    pub stdout: String,
}

/// Extras to a command: text for `stdin` and variables beyond
/// [`child_env`] (only from the helper's code, not from the window).
#[derive(Debug, Clone, Default)]
pub struct RunExtra {
    pub stdin: Option<String>,
    pub env: Vec<(&'static str, String)>,
}

/// Everything the helper touches the system with. The live one is [`LiveRootIo`];
/// in tests it is a fake that records the calls and does nothing.
///
/// Mutating methods: [`RootIo::run`], [`RootIo::write_file`],
/// [`RootIo::copy_file`], [`RootIo::remove_file`], [`RootIo::remove_dir`],
/// [`RootIo::create_dir_all`]. The rest only read.
pub trait RootIo: Send + Sync {
    /// Execute argv (the first word is a program name from [`CHILD_PATH`] or an
    /// absolute path) and pass output lines (stdout and stderr together) to
    /// `out`. `Ok(code)`; `Ok(None)` means killed by a signal; `Err` means it did not start.
    fn run(&self, argv: &[String], extra: &RunExtra, out: &mut dyn FnMut(&str)) -> Result<Option<i32>, String>;
    /// A read-only probe: exit code and stdout.
    fn capture(&self, argv: &[String]) -> Captured;
    /// Path to a program (`name` without `/` is looked up in [`CHILD_PATH`]; with `/`
    /// it is the path itself, if it is an executable file).
    fn which(&self, name: &str) -> Option<String>;
    fn read_text(&self, path: &str) -> Option<String>;
    /// Whether the path exists (links are dereferenced).
    fn exists(&self, path: &str) -> bool;
    fn is_file(&self, path: &str) -> bool;
    fn is_dir(&self, path: &str) -> bool;
    /// The path itself is a symbolic link (live or dangling).
    fn is_symlink(&self, path: &str) -> bool;
    /// The real path (`realpath`); `None` means there is no path.
    fn canonicalize(&self, path: &str) -> Option<String>;
    /// Names in a directory; empty means there is no directory.
    fn list_dir(&self, path: &str) -> Vec<String>;
    /// Write a whole file with permissions `mode` (a link in place of the file is
    /// not dereferenced: refusal).
    fn write_file(&self, path: &str, content: &str, mode: u32) -> Result<(), String>;
    /// File permissions (without dereferencing a link); `None` means there is no file.
    fn mode_of(&self, path: &str) -> Option<u32> {
        use std::os::unix::fs::PermissionsExt;
        std::fs::symlink_metadata(path).ok().map(|m| m.permissions().mode() & 0o7777)
    }
    /// Copy a file with its permissions.
    fn copy_file(&self, src: &str, dst: &str) -> Result<(), String>;
    /// Rename within the same directory (replacing a file in one step: a reader
    /// sees either the old text or the new one; this is how a script that bash
    /// may be executing right now is changed).
    fn rename(&self, from: &str, to: &str) -> Result<(), String>;
    fn remove_file(&self, path: &str) -> Result<(), String>;
    /// Remove an EMPTY directory.
    fn remove_dir(&self, path: &str) -> Result<(), String>;
    fn create_dir_all(&self, path: &str, mode: u32) -> Result<(), String>;
    /// Copy the tree `src` (root's own working directory: only files and
    /// directories) to `dst`, replacing files instead of writing through links.
    fn copy_tree(&self, src: &str, dst: &str) -> Result<(), String>;
    /// Remove a tree (links inside are not dereferenced).
    fn remove_tree(&self, path: &str) -> Result<(), String>;
    /// Where to create its own working directory ([`WorkDir`]).
    fn work_base(&self) -> std::path::PathBuf;
    /// The user's home directory from passwd (not from the window environment).
    fn home_of(&self, uid: u32) -> Option<String>;
    /// A line to the journal (journald).
    fn log(&self, line: &str);
    /// Open root's file `name` in directory `dir` read-only: without links along
    /// the whole path, owner root, not writable by group/others
    /// ([`unpack::open_root_file`]). The fake test system substitutes it.
    fn open_root_file(&self, dir: &str, name: &str) -> Result<OwnedFd, UnpackError> {
        unpack::open_root_file(dir, name)
    }
}

/// The environment of every program started by the helper: only this, everything
/// else is wiped (`env_clear`). No `LD_*`, no `HOME`, no `XDG_*` of the window.
pub fn child_env() -> Vec<(&'static str, &'static str)> {
    vec![("PATH", CHILD_PATH), ("LC_ALL", "C"), ("LANG", "C")]
}

/// The live executor: real programs and the file system.
#[derive(Debug, Default, Clone, Copy)]
pub struct LiveRootIo;

impl LiveRootIo {
    fn command(&self, argv: &[String]) -> Result<Command, String> {
        let first = argv.first().ok_or_else(|| "пустая команда".to_string())?;
        let prog = self.which(first).ok_or_else(|| format!("нет программы {first}"))?;
        let mut cmd = Command::new(prog);
        cmd.args(&argv[1..]).env_clear().envs(child_env()).current_dir("/");
        Ok(cmd)
    }
}

fn is_exec_file(p: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0).unwrap_or(false)
}

impl RootIo for LiveRootIo {
    fn run(&self, argv: &[String], extra: &RunExtra, out: &mut dyn FnMut(&str)) -> Result<Option<i32>, String> {
        let mut cmd = self.command(argv)?;
        for (k, v) in &extra.env {
            cmd.env(k, v);
        }
        let (mut reader, writer) = std::io::pipe().map_err(|e| e.to_string())?;
        cmd.stdout(writer.try_clone().map_err(|e| e.to_string())?).stderr(writer);
        cmd.stdin(if extra.stdin.is_some() { Stdio::piped() } else { Stdio::null() });
        let mut child = cmd.spawn().map_err(|e| e.to_string())?;
        // Our copies of the write ends live in `cmd`: without this the reading would not see EOF.
        drop(cmd);
        if let (Some(text), Some(mut stdin)) = (&extra.stdin, child.stdin.take()) {
            let _ = stdin.write_all(text.as_bytes());
        }
        let mut cutter = LineCutter::default();
        let mut buf = [0u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => cutter.feed(&buf[..n], out),
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
        cutter.finish(out);
        let status = child.wait().map_err(|e| e.to_string())?;
        Ok(status.code())
    }

    fn capture(&self, argv: &[String]) -> Captured {
        let Ok(mut cmd) = self.command(argv) else { return Captured::default() };
        cmd.stdin(Stdio::null()).stderr(Stdio::null());
        match cmd.output() {
            Ok(o) => Captured { code: o.status.code(), stdout: String::from_utf8_lossy(&o.stdout).into_owned() },
            Err(_) => Captured::default(),
        }
    }

    fn which(&self, name: &str) -> Option<String> {
        if name.is_empty() {
            return None;
        }
        if name.contains('/') {
            return (name.starts_with('/') && is_exec_file(std::path::Path::new(name))).then(|| name.to_string());
        }
        SEARCH_DIRS
            .iter()
            .map(|d| format!("{d}/{name}"))
            .find(|p| is_exec_file(std::path::Path::new(p)))
    }

    fn read_text(&self, path: &str) -> Option<String> {
        std::fs::read_to_string(path).ok()
    }

    fn exists(&self, path: &str) -> bool {
        std::path::Path::new(path).exists()
    }

    fn is_file(&self, path: &str) -> bool {
        std::path::Path::new(path).is_file()
    }

    fn is_dir(&self, path: &str) -> bool {
        std::path::Path::new(path).is_dir()
    }

    fn is_symlink(&self, path: &str) -> bool {
        std::path::Path::new(path).is_symlink()
    }

    fn canonicalize(&self, path: &str) -> Option<String> {
        std::fs::canonicalize(path).ok().map(|p| p.to_string_lossy().into_owned())
    }

    fn list_dir(&self, path: &str) -> Vec<String> {
        std::fs::read_dir(path)
            .map(|rd| rd.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect())
            .unwrap_or_default()
    }

    fn write_file(&self, path: &str, content: &str, mode: u32) -> Result<(), String> {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(mode)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)
            .map_err(|e| format!("{path}: {e}"))?;
        f.write_all(content.as_bytes()).map_err(|e| format!("{path}: {e}"))?;
        f.set_permissions(std::fs::Permissions::from_mode(mode)).map_err(|e| format!("{path}: {e}"))
    }

    fn copy_file(&self, src: &str, dst: &str) -> Result<(), String> {
        std::fs::copy(src, dst).map(|_| ()).map_err(|e| format!("{src} → {dst}: {e}"))
    }

    fn rename(&self, from: &str, to: &str) -> Result<(), String> {
        std::fs::rename(from, to).map_err(|e| format!("{from} → {to}: {e}"))
    }

    fn remove_file(&self, path: &str) -> Result<(), String> {
        std::fs::remove_file(path).map_err(|e| format!("{path}: {e}"))
    }

    fn remove_dir(&self, path: &str) -> Result<(), String> {
        std::fs::remove_dir(path).map_err(|e| format!("{path}: {e}"))
    }

    fn create_dir_all(&self, path: &str, mode: u32) -> Result<(), String> {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new().recursive(true).mode(mode).create(path).map_err(|e| format!("{path}: {e}"))
    }

    fn copy_tree(&self, src: &str, dst: &str) -> Result<(), String> {
        copy_tree_live(std::path::Path::new(src), std::path::Path::new(dst))
    }

    fn remove_tree(&self, path: &str) -> Result<(), String> {
        std::fs::remove_dir_all(path).map_err(|e| format!("{path}: {e}"))
    }

    fn work_base(&self) -> std::path::PathBuf {
        std::path::PathBuf::from(unpack::WORK_BASE)
    }

    fn home_of(&self, uid: u32) -> Option<String> {
        let mut pw: libc::passwd = unsafe { std::mem::zeroed() };
        let mut buf = vec![0 as libc::c_char; 16 * 1024];
        let mut out: *mut libc::passwd = std::ptr::null_mut();
        // SAFETY: getpwuid_r writes into our pw and buf; the strings are read while buf is alive.
        let rc = unsafe { libc::getpwuid_r(uid, &mut pw, buf.as_mut_ptr(), buf.len(), &mut out) };
        if rc != 0 || out.is_null() || pw.pw_dir.is_null() {
            return None;
        }
        // SAFETY: pw_dir points into buf and ends with NUL.
        let dir = unsafe { std::ffi::CStr::from_ptr(pw.pw_dir) }.to_string_lossy().into_owned();
        dir.starts_with('/').then_some(dir)
    }

    fn log(&self, line: &str) {
        journal(line);
    }
}

/// Copy of its own tree into a system directory: directories 0755, files with
/// the copy's mode (`unpack` normalized it to 0644/0755); an existing file or
/// link in place of the destination is removed first (like `--remove-destination`).
fn copy_tree_live(src: &std::path::Path, dst: &std::path::Path) -> Result<(), String> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    let err = |p: &std::path::Path, e: std::io::Error| format!("{}: {e}", p.display());
    match std::fs::symlink_metadata(dst) {
        Ok(m) if m.is_dir() => {}
        Ok(_) => std::fs::remove_file(dst).map_err(|e| err(dst, e))?,
        Err(_) => std::fs::DirBuilder::new().recursive(true).mode(0o755).create(dst).map_err(|e| err(dst, e))?,
    }
    std::fs::set_permissions(dst, std::fs::Permissions::from_mode(0o755)).map_err(|e| err(dst, e))?;
    for entry in std::fs::read_dir(src).map_err(|e| err(src, e))? {
        let entry = entry.map_err(|e| err(src, e))?;
        let (from, to) = (entry.path(), dst.join(entry.file_name()));
        let ty = entry.file_type().map_err(|e| err(&from, e))?;
        if ty.is_dir() {
            copy_tree_live(&from, &to)?;
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

/// Writing to journald with the native protocol (without `logger` and a shell).
/// Without journald the line goes to stderr.
fn journal(line: &str) {
    let clean: String = line.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    let datagram = format!("SYSLOG_IDENTIFIER={JOURNAL_ID}\nPRIORITY=6\nMESSAGE={clean}\n");
    let sent = std::os::unix::net::UnixDatagram::unbound()
        .and_then(|s| s.send_to(datagram.as_bytes(), JOURNAL_SOCKET))
        .is_ok();
    if !sent {
        eprintln!("{JOURNAL_ID}: {clean}");
    }
}

// ============================================================================
// Program output → Progress lines
// ============================================================================

/// Cuts the output byte stream into lines (by `\n` and `\r`: pacman redraws its
/// progress line with a carriage return) no longer than [`PROGRESS_MAX`] bytes.
/// Empty lines are not emitted. A long line without a line break is cut at a
/// UTF-8 character boundary.
#[derive(Debug, Default)]
pub struct LineCutter {
    buf: Vec<u8>,
}

impl LineCutter {
    pub fn feed(&mut self, data: &[u8], out: &mut dyn FnMut(&str)) {
        for &b in data {
            if b == b'\n' || b == b'\r' {
                self.flush_all(out);
            } else {
                self.buf.push(b);
                if self.buf.len() >= PROGRESS_MAX {
                    self.flush_full(out);
                }
            }
        }
    }

    pub fn finish(&mut self, out: &mut dyn FnMut(&str)) {
        self.flush_all(out);
    }

    fn flush_all(&mut self, out: &mut dyn FnMut(&str)) {
        if !self.buf.is_empty() {
            out(&String::from_utf8_lossy(&self.buf));
            self.buf.clear();
        }
    }

    /// The buffer is full: emit whole characters, keep the unfinished UTF-8 tail.
    fn flush_full(&mut self, out: &mut dyn FnMut(&str)) {
        let keep = match std::str::from_utf8(&self.buf) {
            Ok(_) => 0,
            Err(e) if e.error_len().is_none() => self.buf.len() - e.valid_up_to(),
            Err(_) => 0,
        };
        let tail = self.buf.split_off(self.buf.len() - keep);
        self.flush_all(out);
        self.buf = tail;
    }
}

/// A line → pieces no longer than [`PROGRESS_MAX`] bytes at character boundaries.
pub fn cut_progress(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = line;
    while rest.len() > PROGRESS_MAX {
        let mut at = PROGRESS_MAX;
        while !rest.is_char_boundary(at) {
            at -= 1;
        }
        out.push(rest[..at].to_string());
        rest = &rest[at..];
    }
    if !rest.is_empty() {
        out.push(rest.to_string());
    }
    out
}

// ============================================================================
// Machine and session: from the system's words, not the window's
// ============================================================================

/// What the helper knows about the machine by itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Facts {
    /// Package manager family by os-release and the programs of this machine.
    pub family: PkgFamily,
    pub steamos: bool,
    /// The current desktop by the logind session (`Desktop`); empty means not
    /// recognized (then package removal is rejected, [`validate::check_remove_packages`]).
    pub current_de: String,
    /// Type of the window's session by logind (`Type`: `x11`, `wayland`, …); empty
    /// means not found out.
    pub session_type: String,
    /// Rebuild root of the user `PKEXEC_UID` (the home directory from passwd);
    /// `None` means there is no home directory.
    pub rebuild_root: Option<String>,
}

/// Collect [`Facts`]: the helper reads os-release, `desktop` and
/// `session_type` come from logind ([`verify_session`]).
pub fn machine_facts(io: &dyn RootIo, desktop: &str, session_type: &str, pkexec_uid: u32) -> Facts {
    let det = DistroDetector::from_os_release_files(
        io.read_text("/etc/os-release").as_deref(),
        io.read_text("/usr/lib/os-release").as_deref(),
    );
    let family = det.family(&|b| io.which(b).is_some());
    Facts {
        family,
        steamos: det.is_steamos(),
        current_de: system::detect_current_de(desktop, desktop, false).unwrap_or_default(),
        session_type: session_type.trim().to_ascii_lowercase(),
        rebuild_root: io.home_of(pkexec_uid).map(|h| validate::user_rebuild_root(&h)),
    }
}

/// The window's session by logind: `User` is whoever entered the password
/// (`PKEXEC_UID`), and the session is active. `Ok((Desktop, Type))` of the session; `Err` is the refusal key.
pub fn verify_session(io: &dyn RootIo, session: &str, pkexec_uid: u32) -> Result<(String, String), &'static str> {
    if !revoke::valid_session_id(session) {
        return Err("session_mismatch");
    }
    let argv: Vec<String> = [revoke::LOGINCTL, "show-session", session, "-p", "User", "-p", "Active", "-p", "Desktop", "-p", "Type"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let got = io.capture(&argv);
    if got.code != Some(0) {
        return Err("session_mismatch");
    }
    let mut user = None;
    let mut active = false;
    let mut desktop = String::new();
    let mut kind = String::new();
    for line in got.stdout.lines() {
        match line.split_once('=') {
            Some(("User", v)) => user = validate::parse_pkexec_uid(v.trim()),
            Some(("Active", v)) => active = v.trim() == "yes",
            Some(("Desktop", v)) => desktop = v.trim().to_string(),
            Some(("Type", v)) => kind = v.trim().to_string(),
            _ => {}
        }
    }
    if user == Some(pkexec_uid) && active {
        Ok((desktop, kind))
    } else {
        Err("session_mismatch")
    }
}

/// A logind query for [`LogindWatch`] through the same [`RootIo`] (clean
/// environment, absolute path of `loginctl`).
pub struct RootLoginctl(pub Arc<dyn RootIo>);

impl SessionQuery for RootLoginctl {
    fn query(&self, session: &str) -> SessionAnswer {
        let argv: Vec<String> = [revoke::LOGINCTL, "show-session", session, "-p", "Active", "-p", "LockedHint"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let got = self.0.capture(&argv);
        match got.code {
            None => SessionAnswer::Silent,
            Some(0) => revoke::parse_active_locked(&got.stdout),
            Some(_) => SessionAnswer::Gone,
        }
    }
}

// ============================================================================
// Checking an operation (everything before the first action)
// ============================================================================

/// A checked operation: what the helper will execute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plan {
    Install { family: PkgFamily, packages: Vec<String> },
    Remove { family: PkgFamily, packages: Vec<String> },
    EnableDm { unit: String },
    SwitchDm { family: PkgFamily, unit: String, greeter: Option<String> },
    /// Default session: the pair of an X11 session on Wayland (a table entry).
    LoginSessionWayland { session: &'static plan::SessionSuccessor },
    BuildDeps { deps: Vec<String> },
    /// A built package: the file is descriptor 0 of the request.
    InstallBuilt { label: String, build_only: Vec<String>, replace: bool, rebuild_root: String },
    /// Staging by category: the directory is descriptor 0 of the request (ustar).
    Stage { category: String, name: String },
    /// SDDM/Plymouth theme: install (descriptor 0 is ustar), selection, removal.
    ThemeInstall { kind: SystemThemeKind, theme: String },
    ThemeApply { kind: SystemThemeKind, theme: String },
    ThemeRemove { kind: SystemThemeKind, theme: String },
    /// Desktop as a set (pacman): the core rule by the root side's probe,
    /// before execution ([`de_set_verdict`]).
    DeInstallSet {
        family: PkgFamily,
        de_id: String,
        packages: Vec<String>,
        deps_only: Vec<String>,
        replace: Vec<String>,
        update_first: bool,
    },
    /// A desktop image on SteamOS.
    Sysext { op: SysextOp, de_id: String },
    /// AUR shim: dependencies from the official repositories.
    AurDeps { names: Vec<String> },
    /// AUR shim: built packages (request descriptors in the order of `labels`).
    AurInstallBuilt { labels: Vec<String>, allowed: Vec<String>, replace: bool },
    /// AUR shim: remove build packages (only orphan dependencies).
    AurDropMake { names: Vec<String> },
    /// An update guard orphan (the machine probe happens at execution).
    /// `current_de` is the desktop the helper found out from logind itself.
    OrphanRemove { names: Vec<String>, current_de: String },
    /// Rolling back an update guard set from the pacman cache (machine check at execution).
    CacheRollback { files: Vec<String> },
    /// Fonts for all: file names (request descriptors in order).
    FontInstall { names: Vec<String> },
    /// Fonts for all: paths inside the core directory.
    FontRemove { files: Vec<String> },
    /// SDDM theme background: `image` means the picture came as descriptor 0,
    /// otherwise restore the theme's background.
    SddmBackground { theme: String, image: bool },
    /// Carrying the look over to the SDDM login screen (`look: None` is «Reset»);
    /// files are request descriptors in the order of `names`.
    SddmLook { look: Option<plan::SddmLook>, names: Vec<String> },
    /// «Show clock» of an SDDM theme.
    SddmClock { theme: String, show: bool },
}

fn theme_kind(k: op::SystemThemeKind) -> SystemThemeKind {
    match k {
        op::SystemThemeKind::Sddm => SystemThemeKind::Sddm,
        op::SystemThemeKind::Plymouth => SystemThemeKind::Plymouth,
    }
}

/// What forbids a theme of this kind on SteamOS.
fn theme_block(k: SystemThemeKind) -> SteamOsBlock {
    match k {
        SystemThemeKind::Sddm => SteamOsBlock::LoginScreen,
        SystemThemeKind::Plymouth => SteamOsBlock::BootSplash,
    }
}

fn kind_dir(k: SystemThemeKind) -> &'static str {
    match k {
        SystemThemeKind::Sddm => "sddm",
        SystemThemeKind::Plymouth => "plymouth",
    }
}

/// A refusal before execution: the protocol code and the message key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deny {
    pub code: ErrorCode,
    pub key: String,
}

impl Deny {
    fn refused(key: &str) -> Self {
        Deny { code: ErrorCode::Refused, key: key.to_string() }
    }

    /// A refusal found already during execution (before touching the system).
    fn into_outcome(self) -> Outcome {
        Outcome::Refused(self.key)
    }
}

impl From<Refusal> for Deny {
    fn from(r: Refusal) -> Self {
        Deny::refused(&format!("refused.{}", r.code()))
    }
}

/// The family from the operation must match what the helper found out itself.
fn same_family(claimed: PkgFamily, facts: &Facts) -> Result<(), Refusal> {
    if claimed == facts.family {
        Ok(())
    } else {
        Err(Refusal::Family(claimed.as_str()))
    }
}

/// A build dependency string: a name by the core rule and possibly a version
/// constraint (`cmake>=3.20`), like `dep_ok` in `rexon-pkg-helper`.
fn check_build_dep(dep: &str) -> Result<(), Refusal> {
    if dep.len() > validate::MAX_NAME_LEN {
        return Err(Refusal::TooLong(dep.chars().take(validate::MAX_NAME_LEN).collect()));
    }
    let at = dep.find(['<', '>', '=']).unwrap_or(dep.len());
    let (name, ver) = dep.split_at(at);
    validate::check_package_name(PkgFamily::Arch, name)?;
    if ver.is_empty() {
        return Ok(());
    }
    let body = ["<=", ">=", "=", "<", ">"].iter().find_map(|op| ver.strip_prefix(op));
    let ok = body.is_some_and(|b| {
        b.chars().next().is_some_and(|c| c.is_ascii_alphanumeric())
            && b.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '+' | ':' | '~' | '-'))
    });
    if ok {
        Ok(())
    } else {
        Err(Refusal::BuildDep(dep.to_string()))
    }
}

/// Names of AUR shim operations: only the pacman of this machine, the system is
/// not read-only, the list is non-empty and no longer than
/// [`validate::MAX_PACKAGES`], each is a package name (without versions and
/// `repo/`). `no_vital` means vital packages are neither installed as built nor removed.
fn check_aur_names(facts: &Facts, names: &[String], no_vital: bool) -> Result<(), Refusal> {
    if facts.family != PkgFamily::Arch {
        return Err(Refusal::Family(facts.family.as_str()));
    }
    validate::check_mutable_system(SteamOsBlock::Desktops, facts.steamos)?;
    if names.is_empty() {
        return Err(Refusal::Empty);
    }
    if names.len() > validate::MAX_PACKAGES {
        return Err(Refusal::TooMany(names.len()));
    }
    for n in names {
        validate::check_package_name(PkgFamily::Arch, n)?;
        if no_vital {
            validate::check_not_vital(n)?;
        }
    }
    Ok(())
}

/// Fonts for all: on NixOS the system fonts are described by its configuration
/// (fontconfig does not scan `/usr/local` there), on SteamOS the system
/// partition is read-only.
fn check_font_machine(facts: &Facts) -> Result<(), Refusal> {
    if facts.family == PkgFamily::Nix {
        return Err(Refusal::Family(facts.family.as_str()));
    }
    validate::check_mutable_system(SteamOsBlock::SystemPart, facts.steamos)
}

/// Refusal key: the operation goes through the builder shim (AUR, AeroShell, B10),
/// the helper has no execution for it and never will.
pub const NOT_FOR_HELPER: &str = "not_for_helper";

/// Prefix of the refusal key "the window's session depends on the package" (+ package name).
pub const ORPHAN_IN_USE: &str = "refused.orphan-in-use.";

/// Prefix of the refusal key "the login screen itself opens a session on the package" (+ name).
pub const ORPHAN_LOGIN_DEFAULT: &str = "refused.orphan-login-default.";

/// Refusal key: the helper does not execute the operation yet.
pub const NOT_SUPPORTED_YET: &str = "not_supported_yet";

/// Whether the helper executes operations of this kind at all (before any data
/// checks). `Err` is the refusal key [`NOT_FOR_HELPER`] or [`NOT_SUPPORTED_YET`]:
/// the only refusals after which the window may take the old path
/// (`privilege::run`). The window asks this BEFORE raising the session, so as
/// not to ask for a password for a certain refusal.
pub fn executes(op: &PrivOp) -> Result<(), &'static str> {
    match op {
        PrivOp::AurInstall { .. } | PrivOp::AeroShellRebuild { .. } => Err(NOT_FOR_HELPER),
        PrivOp::InstallBuilt { .. }
        | PrivOp::Stage { .. }
        | PrivOp::LoginSessionWayland { .. }
        | PrivOp::SystemTheme(_)
        | PrivOp::PkgInstall { .. }
        | PrivOp::PkgRemove { .. }
        | PrivOp::EnableDm { .. }
        | PrivOp::SwitchDm { .. }
        | PrivOp::BuildDeps { .. }
        | PrivOp::DeInstallSet { .. }
        | PrivOp::Sysext { .. }
        | PrivOp::AurDeps { .. }
        | PrivOp::AurInstallBuilt { .. }
        | PrivOp::AurDropMake { .. }
        | PrivOp::OrphanRemove { .. }
        | PrivOp::CacheRollback { .. }
        | PrivOp::FontInstall { .. }
        | PrivOp::FontRemove { .. }
        | PrivOp::SddmBackground { .. }
        | PrivOp::SddmLook { .. }
        | PrivOp::SddmClock { .. } => Ok(()),
        PrivOp::DeInstall { .. }
        | PrivOp::GentooSync { .. }
        | PrivOp::GentooOverlay { .. }
        | PrivOp::MateGschemaCleanup
        | PrivOp::PlasmaForksReset { .. }
        | PrivOp::PlymouthReset { .. }
        | PrivOp::SddmReset { .. } => Err(NOT_SUPPORTED_YET),
    }
}

/// All root-side checks for one operation. `fds` is how many descriptors came
/// with the frame (it must match the operation's files).
///
/// `Err` means not a single action: no command, no file.
pub fn check_op(op: &PrivOp, facts: &Facts, fds: usize) -> Result<Plan, Deny> {
    executes(op).map_err(Deny::refused)?;
    // The operation's files are exactly the received descriptors, in order: slot = index.
    let files = op.local_files();
    if fds != files.len() || files.iter().enumerate().any(|(i, f)| usize::from(f.slot) != i) {
        return Err(Deny::refused("fds_mismatch"));
    }
    match op {
        PrivOp::PkgInstall { family, packages, source } => {
            same_family(*family, facts)?;
            validate::check_mutable_system(SteamOsBlock::Desktops, facts.steamos)?;
            // Only by the table the window named (the desktop to repair is the one the
            // helper found out from logind itself).
            validate::check_install(*family, packages, source.install_source(&facts.current_de))?;
            Ok(Plan::Install { family: *family, packages: packages.clone() })
        }
        PrivOp::PkgRemove { family, packages } => {
            same_family(*family, facts)?;
            validate::check_mutable_system(SteamOsBlock::Desktops, facts.steamos)?;
            validate::check_remove_packages(*family, packages, &facts.current_de)?;
            Ok(Plan::Remove { family: *family, packages: packages.clone() })
        }
        PrivOp::EnableDm { family, unit } => {
            same_family(*family, facts)?;
            validate::check_mutable_system(SteamOsBlock::LoginScreen, facts.steamos)?;
            validate::check_enable_dm_unit(unit)?;
            Ok(Plan::EnableDm { unit: unit.clone() })
        }
        PrivOp::SwitchDm { family, unit, greeter } => {
            same_family(*family, facts)?;
            validate::check_mutable_system(SteamOsBlock::LoginScreen, facts.steamos)?;
            validate::check_switch_dm_unit(unit)?;
            if let Some(g) = greeter {
                // A greeter is chosen only for LightDM.
                if unit != "lightdm" {
                    return Err(Refusal::DisplayManager(unit.clone()).into());
                }
                validate::check_item_name(g)?;
            }
            Ok(Plan::SwitchDm { family: *family, unit: unit.clone(), greeter: greeter.clone() })
        }
        PrivOp::LoginSessionWayland { x11 } => {
            validate::check_mutable_system(SteamOsBlock::LoginScreen, facts.steamos)?;
            // Only a session from the table, and exactly by name (not a path, not the
            // window's value with `.desktop`).
            let session = plan::X11_SESSION_SUCCESSORS
                .iter()
                .find(|s| s.x11 == x11)
                .ok_or_else(|| Deny::refused("refused.display-manager"))?;
            Ok(Plan::LoginSessionWayland { session })
        }
        PrivOp::BuildDeps { names } => {
            // Rebuild: pacman only; the family is taken from the machine.
            if facts.family != PkgFamily::Arch {
                return Err(Refusal::Family(facts.family.as_str()).into());
            }
            validate::check_mutable_system(SteamOsBlock::Desktops, facts.steamos)?;
            if names.is_empty() {
                return Err(Refusal::Empty.into());
            }
            if names.len() > validate::MAX_PACKAGES {
                return Err(Refusal::TooMany(names.len()).into());
            }
            names.iter().try_for_each(|d| check_build_dep(d))?;
            Ok(Plan::BuildDeps { deps: names.clone() })
        }
        PrivOp::InstallBuilt { package, build_only, replace } => {
            if facts.family != PkgFamily::Arch {
                return Err(Refusal::Family(facts.family.as_str()).into());
            }
            validate::check_mutable_system(SteamOsBlock::Desktops, facts.steamos)?;
            let Some(root) = facts.rebuild_root.as_deref() else {
                return Err(Refusal::OutsideRoot(package.label.clone()).into());
            };
            validate::check_built_package(&package.label, root)?;
            if build_only.len() > validate::MAX_PACKAGES {
                return Err(Refusal::TooMany(build_only.len()).into());
            }
            for n in build_only {
                validate::check_package_name(PkgFamily::Arch, n)?;
                validate::check_not_vital(n)?;
            }
            Ok(Plan::InstallBuilt {
                label: package.label.clone(),
                build_only: build_only.clone(),
                replace: *replace,
                rebuild_root: root.to_string(),
            })
        }
        PrivOp::Stage { category, name, .. } => {
            validate::check_mutable_system(SteamOsBlock::SystemPart, facts.steamos)?;
            validate::check_stage_category(category)?;
            validate::check_item_name(name)?;
            Ok(Plan::Stage { category: category.clone(), name: name.clone() })
        }
        PrivOp::SystemTheme(t) => {
            let (kind, theme) = match t {
                SystemThemeOp::Install { kind, theme, .. }
                | SystemThemeOp::Apply { kind, theme, .. }
                | SystemThemeOp::Remove { kind, theme, .. } => (theme_kind(*kind), theme.clone()),
            };
            validate::check_mutable_system(theme_block(kind), facts.steamos)?;
            validate::check_item_name(&theme)?;
            Ok(match t {
                SystemThemeOp::Install { .. } => Plan::ThemeInstall { kind, theme },
                SystemThemeOp::Apply { .. } => Plan::ThemeApply { kind, theme },
                SystemThemeOp::Remove { .. } => Plan::ThemeRemove { kind, theme },
            })
        }
        PrivOp::DeInstallSet { family, de_id, packages, deps_only, replace, update_first } => {
            same_family(*family, facts)?;
            validate::check_mutable_system(SteamOsBlock::Desktops, facts.steamos)?;
            let Some(de_id) = de_id else {
                return Err(Refusal::UnknownDesktop(String::new()).into());
            };
            validate::check_de_set(*family, de_id, packages, deps_only, replace, &facts.current_de)?;
            Ok(Plan::DeInstallSet {
                family: *family,
                de_id: de_id.clone(),
                packages: packages.clone(),
                deps_only: deps_only.clone(),
                replace: replace.clone(),
                update_first: *update_first,
            })
        }
        PrivOp::Sysext { op, de_id } => {
            validate::check_sysext(de_id, *op == SysextOp::Remove, facts.steamos, &facts.current_de)?;
            Ok(Plan::Sysext { op: *op, de_id: de_id.clone() })
        }
        PrivOp::AurDeps { names } => {
            check_aur_names(facts, names, false)?;
            Ok(Plan::AurDeps { names: names.clone() })
        }
        PrivOp::AurInstallBuilt { packages, allowed, replace } => {
            check_aur_names(facts, allowed, true)?;
            if packages.is_empty() {
                return Err(Refusal::Empty.into());
            }
            if packages.len() > validate::MAX_PACKAGES {
                return Err(Refusal::TooMany(packages.len()).into());
            }
            let mut labels = Vec::with_capacity(packages.len());
            let mut seen = HashSet::new();
            for p in packages {
                let name = validate::check_aur_package(&p.label)?;
                // Two copies under one name will not fit in the working directory.
                if !seen.insert(name.to_string()) {
                    return Err(Refusal::PackageFile(name.to_string()).into());
                }
                labels.push(p.label.clone());
            }
            Ok(Plan::AurInstallBuilt { labels, allowed: allowed.clone(), replace: *replace })
        }
        PrivOp::AurDropMake { names } => {
            check_aur_names(facts, names, true)?;
            Ok(Plan::AurDropMake { names: names.clone() })
        }
        PrivOp::OrphanRemove { names } => {
            check_aur_names(facts, names, true)?;
            // Permanent packages and packages of the current desktop (current desktop not found out: refusal).
            validate::check_remove_packages(PkgFamily::Arch, names, &facts.current_de)?;
            // The window's session depends on this package (kwin-x11 in Plasma X11).
            if let Some(s) = crate::update_guard::orphan_in_use(names, &facts.session_type, &facts.current_de) {
                return Err(Deny::refused(&format!("{ORPHAN_IN_USE}{}", s.package)));
            }
            Ok(Plan::OrphanRemove { names: names.clone(), current_de: facts.current_de.clone() })
        }
        PrivOp::CacheRollback { files } => {
            if facts.family != PkgFamily::Arch {
                return Err(Refusal::Family(facts.family.as_str()).into());
            }
            validate::check_mutable_system(SteamOsBlock::Desktops, facts.steamos)?;
            if files.is_empty() {
                return Err(Refusal::Empty.into());
            }
            if files.len() > crate::update_guard::ROLLBACK_BUTTON_MAX {
                return Err(Refusal::TooMany(files.len()).into());
            }
            let mut seen = HashSet::new();
            for f in files {
                let (_, name) = validate::check_cache_package(f)?;
                // Two copies under one name will not fit in the working directory.
                if !seen.insert(name.to_string()) {
                    return Err(Refusal::PackageFile(name.to_string()).into());
                }
            }
            Ok(Plan::CacheRollback { files: files.clone() })
        }
        PrivOp::FontInstall { fonts } => {
            check_font_machine(facts)?;
            let names: Vec<&str> = fonts.iter().map(|f| f.name.as_str()).collect();
            validate::check_font_install(&names)?;
            Ok(Plan::FontInstall { names: fonts.iter().map(|f| f.name.clone()).collect() })
        }
        PrivOp::FontRemove { files } => {
            check_font_machine(facts)?;
            validate::check_font_remove(files)?;
            Ok(Plan::FontRemove { files: files.clone() })
        }
        PrivOp::SddmBackground { theme, image } => {
            validate::check_mutable_system(SteamOsBlock::LoginScreen, facts.steamos)?;
            validate::check_item_name(theme)?;
            Ok(Plan::SddmBackground { theme: theme.clone(), image: image.is_some() })
        }
        PrivOp::SddmLook { look, files } => {
            validate::check_mutable_system(SteamOsBlock::LoginScreen, facts.steamos)?;
            let names: Vec<&str> = files.iter().map(|f| f.name.as_str()).collect();
            validate::check_sddm_look(look.as_ref(), &names)?;
            Ok(Plan::SddmLook { look: look.clone(), names: files.iter().map(|f| f.name.clone()).collect() })
        }
        PrivOp::SddmClock { theme, show } => {
            validate::check_mutable_system(SteamOsBlock::LoginScreen, facts.steamos)?;
            validate::check_item_name(theme)?;
            Ok(Plan::SddmClock { theme: theme.clone(), show: *show })
        }
        _ => Err(Deny::refused(NOT_SUPPORTED_YET)),
    }
}

// ============================================================================
// Execution
// ============================================================================

/// Result of execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The program finished (`ok` means success), `key` is the message key of the
    /// helpers catalog ([`crate::system_l10n::message`]).
    Done { ok: bool, code: Option<i32>, key: Option<String> },
    /// `Cancel` arrived before the system was touched.
    Cancelled,
    /// A file check refusal (on the root copy: `unpack`, `.PKGINFO`): the system
    /// is untouched.
    Refused(String),
    /// Helper failure (the program did not start).
    Internal(String),
}

impl From<UnpackError> for Outcome {
    fn from(e: UnpackError) -> Self {
        match e.error_code() {
            ErrorCode::Refused => Outcome::Refused(format!("refused.{}", e.code())),
            _ => Outcome::Internal(format!("unpack.{}", e.code())),
        }
    }
}

impl Outcome {
    fn fail(key: &str) -> Self {
        Outcome::Done { ok: false, code: Some(2), key: Some(key.to_string()) }
    }
    fn ok(key: Option<&str>) -> Self {
        Outcome::Done { ok: true, code: Some(0), key: key.map(str::to_string) }
    }
}

/// State of the running operation: cancellation and "the system has already been touched".
#[derive(Debug, Default)]
pub struct OpState {
    cancelled: bool,
    touched: bool,
}

/// Cancel if the system has not been touched yet. `true` means the cancellation is accepted.
fn request_cancel(state: &Mutex<OpState>) -> bool {
    let mut s = state.lock().unwrap_or_else(|e| e.into_inner());
    if s.touched {
        false
    } else {
        s.cancelled = true;
        true
    }
}

/// Environment of the executor of one operation.
pub struct Ctx<'a> {
    pub io: &'a dyn RootIo,
    pub progress: &'a (dyn Fn(&str) + Sync),
    pub state: &'a Mutex<OpState>,
    /// Language of messages for a person (`--lang`), `None` means English.
    pub lang: Option<&'a str>,
    /// What the last `BuildDeps` installed (for removing build packages after
    /// `InstallBuilt`; the record lives as long as the helper does).
    pub rebuilt: &'a Mutex<Vec<String>>,
}

impl Ctx<'_> {
    /// The first change to the system is about to happen. After this `Cancel`
    /// has no effect; before it, `Err(Cancelled)`.
    fn touch(&self) -> Result<(), Outcome> {
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if s.cancelled && !s.touched {
            return Err(Outcome::Cancelled);
        }
        s.touched = true;
        Ok(())
    }

    fn say_text(&self, key: &str, args: &[&str]) -> String {
        let msgid = crate::system_l10n::message(key).unwrap_or(key);
        let mut text = self
            .lang
            .and_then(|l| crate::system_l10n::translated(l, msgid))
            .unwrap_or_else(|| msgid.to_string());
        for (i, a) in args.iter().enumerate().rev() {
            text = text.replace(&format!("%{}", i + 1), a);
        }
        text
    }

    /// A helpers catalog message for the person (as a progress line).
    fn say(&self, key: &str, args: &[&str]) {
        (self.progress)(&self.say_text(key, args));
    }

    fn fail(&self, key: &str, args: &[&str]) -> Outcome {
        self.say(key, args);
        Outcome::fail(key)
    }

    fn run(&self, argv: &[String], extra: &RunExtra) -> Result<Option<i32>, String> {
        self.io.log(&format!("запуск: {}", argv.join(" ")));
        let rc = self.io.run(argv, extra, &mut |l| (self.progress)(l));
        self.io.log(&format!("итог: {rc:?}"));
        rc
    }

    /// Execute and collect the output into a string (for the error text), without progress.
    fn run_quiet(&self, argv: &[String], extra: &RunExtra) -> (Option<i32>, String) {
        self.io.log(&format!("запуск: {}", argv.join(" ")));
        let mut text = String::new();
        let rc = self.io.run(argv, extra, &mut |l| {
            if !text.is_empty() {
                text.push(' ');
            }
            text.push_str(l);
        });
        self.io.log(&format!("итог: {rc:?}"));
        (rc.ok().flatten(), text)
    }
}

fn strings(words: &[&str]) -> Vec<String> {
    words.iter().map(|s| s.to_string()).collect()
}

/// Program exit code → result.
fn finished(rc: Result<Option<i32>, String>) -> Outcome {
    match rc {
        Ok(code) => Outcome::Done { ok: code == Some(0), code, key: None },
        Err(_) => Outcome::Internal("spawn_failed".into()),
    }
}

/// Execute a checked operation. `files` are the request descriptors (already
/// matched against the operation in [`check_op`]).
pub fn execute(plan_: &Plan, ctx: &Ctx<'_>, files: Vec<OwnedFd>) -> Outcome {
    let file = files.first().map(|f| f.as_fd());
    let need_file = || Outcome::Refused("fds_mismatch".into());
    match plan_ {
        Plan::InstallBuilt { label, build_only, replace, rebuild_root } => match file {
            Some(fd) => install_built(ctx, fd, label, build_only, *replace, rebuild_root),
            None => need_file(),
        },
        Plan::Stage { category, name } => match file {
            Some(fd) => stage(ctx, fd, category, name),
            None => need_file(),
        },
        Plan::ThemeInstall { kind, theme } => match file {
            Some(fd) => theme_install(ctx, fd, *kind, theme),
            None => need_file(),
        },
        Plan::ThemeApply { kind, theme } => theme_apply(ctx, *kind, theme),
        Plan::ThemeRemove { kind, theme } => theme_remove(ctx, *kind, theme),
        Plan::Install { family, packages } => run_table(ctx, plan::install_cmd(*family, packages)),
        Plan::Remove { family, packages } => run_table(ctx, plan::remove_cmd(*family, packages)),
        Plan::EnableDm { unit } => enable_dm(ctx, unit),
        Plan::SwitchDm { family, unit, greeter } => switch_dm(ctx, *family, unit, greeter.as_deref()),
        Plan::LoginSessionWayland { session } => login_session_wayland(ctx, session),
        Plan::BuildDeps { deps } => build_deps(ctx, deps),
        Plan::DeInstallSet { family, de_id, packages, deps_only, replace, update_first } => {
            de_install_set(ctx, *family, de_id, packages, deps_only, replace, *update_first)
        }
        Plan::Sysext { op, de_id } => sysext(ctx, *op, de_id),
        Plan::AurDeps { names } => aur_deps(ctx, names),
        Plan::AurInstallBuilt { labels, allowed, replace } => {
            if files.len() != labels.len() {
                return need_file();
            }
            aur_install_built(ctx, &files, labels, allowed, *replace)
        }
        Plan::AurDropMake { names } => aur_drop_make(ctx, names),
        Plan::OrphanRemove { names, current_de } => orphan_remove(ctx, names, current_de),
        Plan::CacheRollback { files } => cache_rollback(ctx, files),
        Plan::FontInstall { names } => {
            if files.len() != names.len() {
                return need_file();
            }
            font_install(ctx, &files, names)
        }
        Plan::FontRemove { files } => font_remove(ctx, files),
        Plan::SddmBackground { theme, image } => match (image, file) {
            (true, Some(fd)) => sddm_background(ctx, theme, Some(fd)),
            (true, None) => need_file(),
            (false, _) => sddm_background(ctx, theme, None),
        },
        Plan::SddmLook { look, names } => {
            if files.len() != names.len() {
                return need_file();
            }
            sddm_look(ctx, look.as_ref(), &files, names)
        }
        Plan::SddmClock { theme, show } => sddm_clock(ctx, theme, *show),
    }
}

/// A command from the core table as is.
fn run_table(ctx: &Ctx<'_>, argv: Result<Vec<String>, String>) -> Outcome {
    let Ok(argv) = argv else { return Outcome::Internal("no_cmd".into()) };
    if ctx.io.which(&argv[0]).is_none() {
        return ctx.fail("no_pm", &[&argv[0]]);
    }
    if let Err(o) = ctx.touch() {
        return o;
    }
    finished(ctx.run(&argv, &RunExtra::default()))
}

/// `enable-dm`, as in `rexon-pkg-helper`: `systemctl enable [--force] <unit>`.
fn enable_dm(ctx: &Ctx<'_>, unit: &str) -> Outcome {
    if ctx.io.which("systemctl").is_none() {
        return ctx.fail("no_systemctl", &[]);
    }
    let mut argv = strings(&["systemctl", "enable"]);
    if plan::DM_FORCE_ENABLE.contains(&unit) {
        argv.push("--force".into());
    }
    argv.push(format!("{unit}.service"));
    if let Err(o) = ctx.touch() {
        return o;
    }
    finished(ctx.run(&argv, &RunExtra::default()))
}

/// `build-deps`, as in `rexon-pkg-helper`: `pacman -S --needed --asdeps`, and a
/// record of what exactly was installed (the difference of `pacman -Qq` before and after).
fn build_deps(ctx: &Ctx<'_>, deps: &[String]) -> Outcome {
    if ctx.io.which("pacman").is_none() {
        return ctx.fail("no_pm", &["pacman"]);
    }
    let qq = strings(&["pacman", "-Qq"]);
    let set = |c: Captured| -> BTreeSet<String> { c.stdout.lines().map(str::to_string).collect() };
    let before = set(ctx.io.capture(&qq));
    if let Err(o) = ctx.touch() {
        return o;
    }
    let mut argv = strings(&["pacman", "-S", "--needed", "--noconfirm", "--asdeps", "--"]);
    argv.extend(deps.iter().cloned());
    let rc = ctx.run(&argv, &RunExtra::default());
    let after = set(ctx.io.capture(&qq));
    let mut rec = ctx.rebuilt.lock().unwrap_or_else(|e| e.into_inner());
    *rec = if before.is_empty() || after.is_empty() {
        Vec::new()
    } else {
        after.difference(&before).cloned().collect()
    };
    finished(rc)
}

// ── Desktop as a set (`DeInstallSet`) ──────────────────────────────────

/// The core rule for a desktop set by the pacman answers of THIS machine (the
/// window is not trusted): members of the desktop table's groups (`pacman -Sg`)
/// and the probe [`plan::install_preview`], the same one the window made before
/// the password (DKMS modules and headers for the installed kernels, conflicts
/// with what is installed). Read-only.
fn de_set_verdict(
    io: &dyn RootIo,
    family: PkgFamily,
    de_id: &str,
    packages: &[String],
    deps_only: &[String],
    replace: &[String],
) -> Result<(), Refusal> {
    let run = |cmd: &plan::Cmd| -> plan::PreviewAnswer {
        let got = io.capture(&cmd.argv);
        got.code.map(|rc| (rc, got.stdout, String::new()))
    };
    let table = validate::desktop_packages(family, de_id);
    let mut allowed: HashSet<String> = table.iter().cloned().collect();
    if let Some((_, out, _)) = plan::sync_groups_cmd(family, &table).and_then(|cmd| run(&cmd)) {
        for (group, members) in plan::parse_sync_groups(&out) {
            if table.contains(&group) {
                allowed.extend(members);
            }
        }
    }
    let base: Vec<String> = packages.iter().filter(|p| allowed.contains(*p)).cloned().collect();
    if base.is_empty() {
        return Err(Refusal::NotInTable(packages.first().cloned().unwrap_or_default()));
    }
    let preview = plan::install_preview(family, &base, &run);
    validate::check_de_set_preview(packages, &base, deps_only, replace, preview.as_ref())
}

/// A desktop as a set: what the plan's `pkexec bash` wrapper did
/// ([`plan::install_batch_plan_with_deps`]), without a shell: refresh the
/// database (if the window decided so), `pacman -S` of the set (with `--ask=4`
/// only for an agreed replacement from the core table), then mark the DKMS
/// module and headers `--asdeps`. Each step only after the previous one succeeds (`set -e`).
fn de_install_set(
    ctx: &Ctx<'_>,
    family: PkgFamily,
    de_id: &str,
    packages: &[String],
    deps_only: &[String],
    replace: &[String],
    update_first: bool,
) -> Outcome {
    if ctx.io.which("pacman").is_none() {
        return ctx.fail("no_pm", &["pacman"]);
    }
    let verdict = |when: &str| {
        de_set_verdict(ctx.io, family, de_id, packages, deps_only, replace).map_err(|r| {
            ctx.io.log(&format!("набор {de_id} отвергнут {when}: {r:?}"));
            Outcome::Refused(format!("refused.{}", r.code()))
        })
    };
    if let Err(o) = verdict("до установки") {
        return o;
    }
    let Ok(mut install) = plan::install_cmd_replacing(family, packages, replace) else {
        return Outcome::Internal("no_cmd".into());
    };
    // As in the wrapper line: pacman progress without bars and color (it gets parsed).
    if let Some(at) = install.iter().position(|a| a == "--noconfirm") {
        install.splice(at + 1..at + 1, strings(&["--noprogressbar", "--color", "never"]));
    }
    if let Err(o) = ctx.touch() {
        return o;
    }
    if update_first {
        if let Some(update) = plan::update_db_cmd(family) {
            let rc = ctx.run(&update, &RunExtra::default());
            if rc != Ok(Some(0)) {
                return finished(rc);
            }
        }
        // The database was refreshed, so the probe is different. `--ask=4` answers
        // "yes" to ANY conflict, so the core rule runs once more on the new database.
        // Only the package database has been touched so far.
        if let Err(o) = verdict("после обновления базы") {
            return o;
        }
    }
    let rc = ctx.run(&install, &RunExtra::default());
    if rc != Ok(Some(0)) {
        return finished(rc);
    }
    // `deps_only` is a subset of the set (checked); marking is as in
    // [`plan::mark_deps_line`].
    if deps_only.is_empty() {
        return finished(rc);
    }
    let mut mark = strings(&["pacman", "-D", "--asdeps"]);
    mark.extend(deps_only.iter().cloned());
    finished(ctx.run(&mark, &RunExtra::default()))
}

// ── Desktop image on SteamOS (`Sysext`) ─────────────────────────────────

/// Replace a whole file with one rename (like `put` in `rexon-sysext-helper`):
/// bash reads a script as it executes it, and writing over a running one
/// would break the rebuild service.
fn put_file(io: &dyn RootIo, path: &str, text: &str, mode: u32) -> Result<(), String> {
    io.create_dir_all(parent_dir(path), 0o755)?;
    let tmp = format!("{path}.rexon-new");
    io.write_file(&tmp, text, mode)?;
    io.rename(&tmp, path)
}

/// Scripts, units, the policy and `rexon-sysext-helper` itself are texts from
/// its own binary ([`crate::steamos_sysext::layout_files`]), plus the language of
/// the script messages. The helper file is put in place too: the old path
/// (without a session) uses it, with its own polkit policy.
fn sysext_layout(ctx: &Ctx<'_>) -> Result<(), String> {
    use crate::steamos_sysext as sx;
    for f in sx::layout_files() {
        put_file(ctx.io, &f.path, &f.text, f.mode)?;
    }
    let lang = ctx
        .lang
        .filter(|l| crate::system_l10n::languages().contains(l))
        .unwrap_or(crate::i18n::SOURCE_LANGUAGE);
    put_file(ctx.io, sx::LANG_FILE, &format!("{lang}\n"), 0o644)?;
    put_file(ctx.io, sx::HELPER_PATH, &sx::helper_script(), 0o755)
}

/// Remove the desktop from the build list.
fn sysext_list_remove(ctx: &Ctx<'_>, de: &str) {
    use crate::steamos_sysext as sx;
    let Some(text) = ctx.io.read_text(sx::DESKTOPS_LIST) else { return };
    let kept: String = text.lines().filter(|l| *l != de).map(|l| format!("{l}\n")).collect();
    if let Err(e) = put_file(ctx.io, sx::DESKTOPS_LIST, &kept, 0o644) {
        ctx.io.log(&format!("список столов: {e}"));
    }
}

/// Forget the desktop images: the link in `/var/lib/extensions`, the images and
/// unfinished build directories (`rm --one-file-system`, as before).
fn sysext_forget(ctx: &Ctx<'_>, de: &str) {
    use crate::steamos_sysext as sx;
    let _ = ctx.io.remove_file(&format!("{}/rexon-{de}.raw", sx::EXTENSIONS_DIR));
    let prefix = format!("rexon-{de}-");
    for name in ctx.io.list_dir(sx::IMAGE_DIR) {
        if name.starts_with(&prefix) && name.ends_with(".raw") {
            let _ = ctx.io.remove_file(&format!("{}/{name}", sx::IMAGE_DIR));
        }
    }
    let builds: Vec<String> = ctx
        .io
        .list_dir(sx::BUILD_DIR)
        .into_iter()
        .filter(|n| n.starts_with(&format!("{de}-")))
        .map(|n| format!("{}/{n}", sx::BUILD_DIR))
        .collect();
    if !builds.is_empty() {
        let mut rm = strings(&["rm", "-rf", "--one-file-system", "--"]);
        rm.extend(builds);
        let _ = ctx.run_quiet(&rm, &RunExtra::default());
    }
}

/// A desktop image on SteamOS: what `rexon-sysext-helper` does, without
/// `bash -c`: laying out its own texts, services, the desktop list, build and
/// rollback (all or nothing), removal. Building and rebuilding are done by the
/// `rexon-sysext-*` scripts (they keep working without Rexon after an OS update,
/// see [`crate::steamos_sysext`]); the helper takes them from its own binary and
/// puts them into `/var/lib/rexon/bin` itself, only the operation and a desktop
/// from the table come from the window.
fn sysext(ctx: &Ctx<'_>, op: SysextOp, de: &str) -> Outcome {
    use crate::steamos_sysext as sx;
    // Its own state directories: a link in their place means refusal before touching anything.
    for dir in [sx::STATE_DIR, sx::BIN_DIR, parent_dir(sx::DESKTOPS_LIST), sx::IMAGE_DIR, sx::BUILD_DIR] {
        if ctx.io.is_symlink(dir) {
            return Outcome::Refused(format!("refused.{}", Refusal::Path(dir.to_string()).code()));
        }
    }
    if let Err(o) = ctx.touch() {
        return o;
    }
    let quiet = RunExtra::default();
    let sync = |only: &str| {
        vec![format!("{}/rexon-sysext-sync", sx::BIN_DIR), "--interactive".into(), "--only".into(), only.to_string()]
    };
    (ctx.progress)("@progress 1");
    if let Err(e) = sysext_layout(ctx) {
        ctx.io.log(&format!("раскладка: {e}"));
        return ctx.fail("sx_layout_failed", &[]);
    }
    let _ = ctx.run_quiet(&strings(&["systemctl", "daemon-reload"]), &quiet);
    match op {
        SysextOp::Install => {
            let units = strings(&["systemctl", "enable", "systemd-sysext.service", sx::GUARD_UNIT, sx::SYNC_UNIT]);
            if ctx.run_quiet(&units, &quiet).0 != Some(0) {
                ctx.say("sx_enable_failed", &[]);
            }
            let list = ctx.io.read_text(sx::DESKTOPS_LIST).unwrap_or_default();
            let fresh = !list.lines().any(|l| l == de);
            if fresh {
                let mut text = list;
                if !text.is_empty() && !text.ends_with('\n') {
                    text.push('\n');
                }
                text.push_str(&format!("{de}\n"));
                if let Err(e) = put_file(ctx.io, sx::DESKTOPS_LIST, &text, 0o644) {
                    ctx.io.log(&format!("список столов: {e}"));
                    return ctx.fail("sx_layout_failed", &[]);
                }
            }
            let rc = ctx.run(&sync(de), &quiet);
            if rc != Ok(Some(0)) && fresh {
                // All or nothing: a desktop that failed to build does not stay in the list.
                ctx.say("sx_rollback", &[de]);
                sysext_list_remove(ctx, de);
                sysext_forget(ctx, de);
                let _ = ctx.run_quiet(&sync("none"), &quiet);
            }
            (ctx.progress)("@progress 100");
            finished(rc)
        }
        SysextOp::Remove => {
            sysext_list_remove(ctx, de);
            sysext_forget(ctx, de);
            let rc = ctx.run(&sync("none"), &quiet);
            let _ = ctx.run(&[format!("{}/session-guard.sh", sx::BIN_DIR)], &quiet);
            let wanted = ctx.io.read_text(sx::WANTED_SESSION).unwrap_or_default();
            if wanted.lines().next() == Some(sx::session_file(de).as_str()) {
                let _ = ctx.io.remove_file(sx::WANTED_SESSION);
            }
            ctx.say("sx_removed", &[de]);
            (ctx.progress)("@progress 100");
            finished(rc)
        }
    }
}

// ── User files: only a root copy (`unpack`) ─────────────────────────────

/// Its own working directory (0700, root); removed together with the copy on exit.
fn work_dir(ctx: &Ctx<'_>) -> Result<WorkDir, Outcome> {
    WorkDir::create(&ctx.io.work_base()).map_err(|e| {
        ctx.io.log(&format!("рабочий каталог: {e}"));
        Outcome::Internal("work_dir".into())
    })
}

fn path_str(p: &std::path::Path) -> String {
    p.to_string_lossy().into_owned()
}

/// Package name from a makepkg file name: `<name>-<version>-<release>-<arch>.pkg.tar…`.
pub fn pkgname_from_file(file: &str) -> Option<String> {
    let base = basename(file);
    let stem = &base[..base.find(".pkg.tar")?];
    let mut parts = stem.rsplitn(4, '-');
    let (_arch, _rel, _ver) = (parts.next()?, parts.next()?, parts.next()?);
    parts.next().filter(|n| !n.is_empty()).map(str::to_string)
}

/// `pkgname` from the `.PKGINFO` text.
pub fn pkginfo_name(text: &str) -> Option<String> {
    pkginfo_field(text, "pkgname")
}

/// The first value of the field `key` from the `.PKGINFO` text (`pkgver` is the
/// full version with epoch and release).
pub fn pkginfo_field(text: &str, key: &str) -> Option<String> {
    text.lines()
        .filter_map(|l| l.split_once('='))
        .find(|(k, _)| k.trim() == key)
        .map(|(_, v)| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// `install-built`, as in `rexon-pkg-helper`, but the file comes as an open
/// descriptor: a root copy in its own directory, `.PKGINFO` is checked on the
/// COPY (`bsdtar`, root does not open the user's path), then `pacman -U`;
/// build packages are removed only if the previous `BuildDeps` of this session
/// installed them and they are marked as dependencies.
///
/// ⚠️ The boundary, as with the AUR: `pacman -U` of a file built by the user is
/// that user's code running as root (the package's install scripts). The helper
/// only guarantees that exactly the checked copy with the expected name is installed.
fn install_built(
    ctx: &Ctx<'_>,
    fd: BorrowedFd<'_>,
    label: &str,
    build_only: &[String],
    replace: bool,
    rebuild_root: &str,
) -> Outcome {
    let io = ctx.io;
    if io.which("pacman").is_none() {
        return ctx.fail("no_pm", &["pacman"]);
    }
    if io.which("bsdtar").is_none() {
        return ctx.fail("no_pm", &["bsdtar"]);
    }
    let work = match work_dir(ctx) {
        Ok(w) => w,
        Err(o) => return o,
    };
    let copy = match unpack::receive_built_package(fd, label, rebuild_root, &work) {
        Ok(p) => path_str(&p),
        Err(e) => return e.into(),
    };
    let info = io.capture(&strings(&["bsdtar", "-xOf", &copy, ".PKGINFO"]));
    let expected = pkgname_from_file(label);
    let got = (info.code == Some(0)).then(|| pkginfo_name(&info.stdout)).flatten();
    match (&expected, &got) {
        (Some(e), Some(g)) if e == g && validate::check_package_name(PkgFamily::Arch, g).is_ok() => {}
        _ => {
            io.log(&format!("install-built: .PKGINFO {got:?}, ожидалось {expected:?}"));
            return Outcome::Refused("refused.pkginfo".into());
        }
    }
    if let Err(o) = ctx.touch() {
        return o;
    }
    let mut argv = strings(&["pacman", "-U", "--noconfirm"]);
    if replace {
        argv.push("--ask=4".into());
    }
    argv.extend(["--".to_string(), copy]);
    let rc = ctx.run(&argv, &RunExtra::default());
    if !matches!(rc, Ok(Some(0))) {
        return finished(rc);
    }
    let ours: Vec<String> = std::mem::take(&mut *ctx.rebuilt.lock().unwrap_or_else(|e| e.into_inner()));
    if !build_only.is_empty() {
        let asdeps: HashSet<String> =
            io.capture(&strings(&["pacman", "-Qqd"])).stdout.lines().map(str::to_string).collect();
        let mut drop_list = Vec::new();
        for n in build_only {
            if ours.contains(n) && asdeps.contains(n) {
                drop_list.push(n.clone());
            } else {
                ctx.say("build_only_not_ours", &[n]);
            }
        }
        if !drop_list.is_empty() {
            let mut rm = strings(&["pacman", "-Rns", "--noconfirm", "--"]);
            rm.extend(drop_list);
            if !matches!(ctx.run(&rm, &RunExtra::default()), Ok(Some(0))) {
                ctx.say("build_only_kept", &[]);
            }
        }
    }
    Outcome::ok(None)
}

// ── AUR shim (item B10) ─────────────────────────────────────────────────

/// Package names of the official repositories of THIS machine (`pacman -Slq`).
/// Empty means the sync database could not be read (then no name passes).
fn sync_names(io: &dyn RootIo) -> HashSet<String> {
    let c = io.capture(&strings(&["pacman", "-Slq"]));
    if c.code != Some(0) {
        return HashSet::new();
    }
    c.stdout.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string).collect()
}

/// AUR build dependencies: only packages of the official repositories, and only
/// as dependencies: `pacman -S --needed --noconfirm --asdeps`.
fn aur_deps(ctx: &Ctx<'_>, names: &[String]) -> Outcome {
    if ctx.io.which("pacman").is_none() {
        return ctx.fail("no_pm", &["pacman"]);
    }
    let sync = sync_names(ctx.io);
    if let Some(n) = names.iter().find(|n| !sync.contains(n.as_str())) {
        ctx.io.log(&format!("aur-deps: {n} нет в sync-базе"));
        return Deny::from(Refusal::NotInTable(n.clone())).into_outcome();
    }
    if let Err(o) = ctx.touch() {
        return o;
    }
    let mut argv = strings(&["pacman", "-S", "--needed", "--noconfirm", "--asdeps", "--"]);
    argv.extend(names.iter().cloned());
    finished(ctx.run(&argv, &RunExtra::default()))
}

/// What paru/yay built: root copies, the `.PKGINFO` of each has a name from the
/// window's list, matches the file name and is not the name of an official
/// package (a built package does not replace a repository package). Everything
/// in one `pacman -U` transaction.
///
/// ⚠️ The boundary is the same as in [`install_built`]: the install scripts of a
/// built package are the user's code running as root. The helper only guarantees
/// that a checked copy with a name the window asked to build is installed.
fn aur_install_built(
    ctx: &Ctx<'_>,
    files: &[OwnedFd],
    labels: &[String],
    allowed: &[String],
    replace: bool,
) -> Outcome {
    let io = ctx.io;
    if io.which("pacman").is_none() {
        return ctx.fail("no_pm", &["pacman"]);
    }
    if io.which("bsdtar").is_none() {
        return ctx.fail("no_pm", &["bsdtar"]);
    }
    let work = match work_dir(ctx) {
        Ok(w) => w,
        Err(o) => return o,
    };
    let sync = sync_names(io);
    let mut copies = Vec::with_capacity(labels.len());
    for (fd, label) in files.iter().zip(labels) {
        let copy = match unpack::receive_aur_package(fd.as_fd(), label, &work) {
            Ok(p) => path_str(&p),
            Err(e) => return e.into(),
        };
        let info = io.capture(&strings(&["bsdtar", "-xOf", &copy, ".PKGINFO"]));
        let expected = pkgname_from_file(label);
        let got = (info.code == Some(0)).then(|| pkginfo_name(&info.stdout)).flatten();
        let ok = matches!((&expected, &got), (Some(e), Some(g)) if e == g && allowed.contains(g));
        if !ok {
            io.log(&format!("aur-install-built: .PKGINFO {got:?}, файл {expected:?}, окно просило {allowed:?}"));
            return Outcome::Refused("refused.pkginfo".into());
        }
        let name = got.unwrap_or_default();
        if sync.contains(&name) {
            io.log(&format!("aur-install-built: {name} — пакет официального репозитория"));
            return Deny::from(Refusal::Conflict(name)).into_outcome();
        }
        copies.push(copy);
    }
    if let Err(o) = ctx.touch() {
        return o;
    }
    let mut argv = strings(&["pacman", "-U", "--noconfirm"]);
    if replace {
        argv.push("--ask=4".into());
    }
    argv.push("--".into());
    argv.extend(copies);
    finished(ctx.run(&argv, &RunExtra::default()))
}

/// `--removemake`: only orphan dependencies are removed (`pacman -Qqdt`:
/// installed as a dependency and no longer needed by anyone), the rest stays,
/// with an explanation. `-Rs` also takes their dependencies that became orphans too.
fn aur_drop_make(ctx: &Ctx<'_>, names: &[String]) -> Outcome {
    if ctx.io.which("pacman").is_none() {
        return ctx.fail("no_pm", &["pacman"]);
    }
    let orphans: HashSet<String> =
        ctx.io.capture(&strings(&["pacman", "-Qqdt"])).stdout.lines().map(str::to_string).collect();
    let mut drop_list = Vec::new();
    for n in names {
        if orphans.contains(n) {
            drop_list.push(n.clone());
        } else {
            ctx.say("build_only_not_ours", &[n]);
        }
    }
    if drop_list.is_empty() {
        return Outcome::ok(None);
    }
    if let Err(o) = ctx.touch() {
        return o;
    }
    let mut rm = strings(&["pacman", "-Rs", "--noconfirm", "--"]);
    rm.extend(drop_list);
    let rc = ctx.run(&rm, &RunExtra::default());
    if !matches!(rc, Ok(Some(0))) {
        ctx.say("build_only_kept", &[]);
    }
    finished(rc)
}

// ── Update guard orphan ─────────────────────────────────────────────────

/// Remove update guard orphans. The window is not trusted: that each name is an
/// orphan we find out ourselves by the pacman of this machine, the same way the
/// guard judges ([`crate::update_guard`]):
/// 1. a member of a "single version" set, not an anchor
///    ([`crate::update_guard::orphan_candidate`], groups via `pacman -Qg`);
/// 2. installed (`pacman -Q`);
/// 3. present in no repository (`pacman -Slq` is the machine's sync database;
///    if it reads as empty, there is nothing to check with: refusal).
///
/// Then the `pacman -Rnsp` probe: everything the transaction removes together
/// with it (dependencies that became unneeded) is not vital and not a package
/// of the current desktop ([`validate::check_remove_packages`]). If the probe
/// fails (someone needs it, the database is locked): refusal, the system is untouched.
fn orphan_remove(ctx: &Ctx<'_>, names: &[String], current_de: &str) -> Outcome {
    let io = ctx.io;
    if io.which("pacman").is_none() {
        return ctx.fail("no_pm", &["pacman"]);
    }
    let not_orphan = |n: &str, why: &str| {
        io.log(&format!("orphan-remove: {n} — {why}"));
        Deny::from(Refusal::NotOrphan(n.to_string())).into_outcome()
    };
    let sync = sync_names(io);
    if sync.is_empty() {
        return not_orphan(&names.join(" "), "sync-база не прочиталась");
    }
    let mut qg = strings(&["pacman", "-Qg"]);
    qg.extend(crate::update_guard::groups_needed(crate::update_guard::PACKAGE_SETS).iter().map(|g| g.to_string()));
    let groups = crate::update_guard::parse_groups(&io.capture(&qg).stdout);
    for n in names {
        if !crate::update_guard::orphan_candidate(n, &groups) {
            return not_orphan(n, "не член набора сторожа");
        }
        if io.capture(&strings(&["pacman", "-Q", "--", n])).code != Some(0) {
            return not_orphan(n, "не установлен");
        }
        if sync.contains(n.as_str()) {
            return not_orphan(n, "есть в репозиториях");
        }
    }
    let mut preview = strings(&["pacman", "-Rnsp", "--print-format", "%n", "--"]);
    preview.extend(names.iter().cloned());
    let p = io.capture(&preview);
    let going: Vec<String> = p.stdout.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string).collect();
    if p.code != Some(0) || going.is_empty() {
        io.log(&format!("orphan-remove: проба не прошла ({:?})", p.code));
        return Outcome::Refused("refused.orphan-needed".into());
    }
    if let Err(r) = validate::check_remove_packages(PkgFamily::Arch, &going, current_de) {
        io.log(&format!("orphan-remove: снялось бы лишнее: {going:?}"));
        return Deny::from(r).into_outcome();
    }
    // The login screen itself opens a session that needs the package being removed.
    let defaults = crate::update_guard::login_default_sessions(&|p| io.read_text(p), &|d| io.list_dir(d));
    if let Some(s) = crate::update_guard::orphan_login_default(&going, &defaults) {
        io.log(&format!("orphan-remove: экран входа открывает сеанс {:?} ({defaults:?})", s.session_name));
        return Outcome::Refused(format!("{ORPHAN_LOGIN_DEFAULT}{}", s.package));
    }
    if let Err(o) = ctx.touch() {
        return o;
    }
    let mut rm = strings(&["pacman", "-Rns", "--noconfirm", "--"]);
    rm.extend(names.iter().cloned());
    finished(ctx.run(&rm, &RunExtra::default()))
}

// ── Rolling back an update guard set from the pacman cache ──────────────

/// Refusal key: the rollback transaction would touch something beyond the set
/// (a dependency from a repository, someone else's dependency on the new
/// version), or the pacman probe failed.
pub const ROLLBACK_EXTRA: &str = "refused.rollback-extra";

/// Refusal key: the cache file has a signature, but it does not open as a root
/// file or does not match the package: a sign of tampering.
pub const ROLLBACK_SIGNATURE: &str = "refused.rollback-signature";

/// Restore the previous versions of an update guard set. The window is trusted in nothing:
/// 1. the file's directory is exactly the cache directory of this machine
///    ([`PACKAGE_CACHE_DIR`] and `pacman-conf CacheDir` run as root,
///    [`crate::update_guard::parse_cache_dirs`]);
/// 2. root opens the file itself ([`RootIo::open_root_file`]: without links,
///    owner root, not writable by group/others), and from then on only a copy
///    in its own directory is used; a `.sig` signature next to it, if present,
///    is copied too and must match (`pacman-key --verify`), otherwise refusal;
/// 3. the name and version come from the `.PKGINFO` of the copy, not from the file name;
/// 4. its own check of the set by the pacman of this machine (`-Q`, `-Qg`,
///    `-Qmq`, without a pending update) with the same
///    [`crate::update_guard::evaluate`] as the window: the names are EXACTLY the
///    rollback of one finding (no fewer, so nothing partial, and no more), the
///    version of each fits the finding's target and is strictly lower than the
///    installed one (`vercmp`); permanent packages never;
/// 5. the `pacman -U --print` probe: exactly this is installed (no dependencies
///    from the repositories, no broken foreign dependencies: pacman would
///    refuse already at the probe);
/// 6. one `pacman -U` transaction, after which the versions are checked with `pacman -Q`.
///
/// The "package of the current desktop" check is deliberately not taken here:
/// rolling back Plasma or Qt is the current desktop; the protection is
/// "exactly the finding's set".
///
/// [`PACKAGE_CACHE_DIR`]: crate::update_guard::PACKAGE_CACHE_DIR
fn cache_rollback(ctx: &Ctx<'_>, files: &[String]) -> Outcome {
    use crate::update_guard as ug;
    let io = ctx.io;
    for prog in ["pacman", "bsdtar", "vercmp"] {
        if io.which(prog).is_none() {
            return ctx.fail("no_pm", &[prog]);
        }
    }
    let not_rollback = |what: &str, why: &str| {
        io.log(&format!("cache-rollback: {what} — {why}"));
        Deny::from(Refusal::NotRollback(what.to_string())).into_outcome()
    };
    let conf = if io.which("pacman-conf").is_some() {
        let c = io.capture(&strings(&["pacman-conf", "CacheDir"]));
        if c.code == Some(0) { c.stdout } else { String::new() }
    } else {
        String::new()
    };
    let dirs = ug::parse_cache_dirs(&conf);
    let work = match work_dir(ctx) {
        Ok(w) => w,
        Err(o) => return o,
    };
    // (name, version, copy) by the `.PKGINFO` of the copies.
    let mut got: Vec<(String, String, String)> = Vec::with_capacity(files.len());
    for path in files {
        let (dir, base) = match validate::check_cache_package(path) {
            Ok(x) => x,
            Err(r) => return Deny::from(r).into_outcome(),
        };
        if !dirs.iter().any(|d| d == dir) {
            io.log(&format!("cache-rollback: {path} — не каталог кэша ({dirs:?})"));
            return Deny::from(Refusal::Path(path.clone())).into_outcome();
        }
        let fd = match io.open_root_file(dir, base) {
            Ok(fd) => fd,
            Err(e) => {
                io.log(&format!("cache-rollback: {path}: {e}"));
                return e.into();
            }
        };
        let copy = match unpack::receive_file(fd.as_fd(), &work, base, unpack::MAX_PACKAGE_BYTES) {
            Ok(p) => path_str(&p),
            Err(e) => return e.into(),
        };
        // The signature goes next to the copy: with `LocalFileSigLevel = Required`
        // pacman refuses without it. No signature at all: pacman's SigLevel decides.
        // There IS a signature, but it does not open as a root file, does not copy or
        // does not match (`pacman-key --verify` on the copies): a sign of tampering,
        // refusal, we do not install without it.
        let sig = format!("{base}.sig");
        let sig_path = format!("{dir}/{sig}");
        if io.exists(&sig_path) || io.is_symlink(&sig_path) {
            let bad_sig = |why: String| {
                io.log(&format!("cache-rollback: подпись {sig_path} — {why}"));
                Outcome::Refused(ROLLBACK_SIGNATURE.into())
            };
            let fd = match io.open_root_file(dir, &sig) {
                Ok(fd) => fd,
                Err(e) => return bad_sig(e.to_string()),
            };
            let sig_copy = match unpack::receive_file(fd.as_fd(), &work, &sig, 64 * 1024) {
                Ok(p) => path_str(&p),
                Err(e) => return bad_sig(e.to_string()),
            };
            if io.which("pacman-key").is_none() {
                return bad_sig("нечем проверить (нет pacman-key)".into());
            }
            let v = io.capture(&strings(&["pacman-key", "--verify", &sig_copy, &copy]));
            if v.code != Some(0) {
                return bad_sig(format!("не сошлась ({:?})", v.code));
            }
        }
        let info = io.capture(&strings(&["bsdtar", "-xOf", &copy, ".PKGINFO"]));
        let field = |k: &str| (info.code == Some(0)).then(|| pkginfo_field(&info.stdout, k)).flatten();
        let (Some(name), Some(version)) = (field("pkgname"), field("pkgver")) else {
            io.log(&format!("cache-rollback: {path} — .PKGINFO без имени или версии"));
            return Outcome::Refused("refused.pkginfo".into());
        };
        if validate::check_package_name(PkgFamily::Arch, &name).is_err() || got.iter().any(|(n, _, _)| *n == name) {
            io.log(&format!("cache-rollback: {path} — .PKGINFO {name:?}"));
            return Outcome::Refused("refused.pkginfo".into());
        }
        if let Err(r) = validate::check_not_vital(&name) {
            return Deny::from(r).into_outcome();
        }
        got.push((name, version, copy));
    }
    let names: Vec<&str> = got.iter().map(|(n, _, _)| n.as_str()).collect();

    // Its own check of the set, with the same engine as the window, without a
    // pending update: a finding with a rollback then can only be "already diverged".
    let q = io.capture(&strings(&["pacman", "-Q"]));
    if q.code != Some(0) {
        return not_rollback(&names.join(" "), "pacman -Q не прочитался");
    }
    let installed = ug::parse_installed(&q.stdout);
    let mut qg = strings(&["pacman", "-Qg"]);
    qg.extend(ug::groups_needed(ug::PACKAGE_SETS).iter().map(|g| g.to_string()));
    let groups = ug::parse_groups(&io.capture(&qg).stdout);
    let fq = io.capture(&strings(&["pacman", "-Qmq"]));
    let foreign = match fq.code {
        Some(0) => Some(ug::parse_foreign(&fq.stdout)),
        Some(1) if fq.stdout.trim().is_empty() => Some(BTreeSet::new()),
        _ => None,
    };
    let survey = ug::Survey { installed: installed.clone(), groups, pending: Vec::new(), repo: None, foreign };
    let findings = ug::evaluate(&survey, ug::PACKAGE_SETS);
    let want: BTreeSet<&str> = names.iter().copied().collect();
    let Some(finding) = findings.iter().find(|f| {
        let r: BTreeSet<&str> = f.rollback.iter().map(|(n, _)| n.as_str()).collect();
        !r.is_empty() && r == want
    }) else {
        return not_rollback(&names.join(" "), &format!("не ровно откат одной находки ({findings:?})"));
    };
    for (name, version, _) in &got {
        let Some((_, target)) = finding.rollback.iter().find(|(n, _)| n == name) else {
            return not_rollback(name, "не в откате находки");
        };
        if !target.accepts(version) {
            return not_rollback(name, &format!("версия {version} не под цель {target:?}"));
        }
        let Some((_, have)) = installed.iter().find(|(n, _)| n == name) else {
            return not_rollback(name, "не установлен");
        };
        let c = io.capture(&strings(&["vercmp", version, have]));
        let older = c.code == Some(0) && c.stdout.trim().parse::<i32>().is_ok_and(|v| v < 0);
        if !older {
            return not_rollback(name, &format!("{version} не меньше установленной {have}"));
        }
    }

    // The probe: exactly this is installed.
    let copies: Vec<String> = got.iter().map(|(_, _, c)| c.clone()).collect();
    let mut probe = strings(&["pacman", "-U", "--print", "--print-format", "%n %v", "--"]);
    probe.extend(copies.iter().cloned());
    let p = io.capture(&probe);
    // A set of pairs: pacman does not necessarily print them in file order, and
    // missing dependencies from the repositories come as separate lines (`-d`
    // does not remove them). An extra pair or a line that is not "name version"
    // means refusal: the rollback is exactly the set, without pulling from the network.
    let lines: Vec<Vec<&str>> =
        p.stdout.lines().map(|l| l.split_whitespace().collect::<Vec<_>>()).filter(|w| !w.is_empty()).collect();
    let well_formed = lines.iter().all(|w| w.len() == 2);
    let printed: BTreeSet<(String, String)> =
        lines.iter().filter(|w| w.len() == 2).map(|w| (w[0].to_string(), w[1].to_string())).collect();
    let expected: BTreeSet<(String, String)> = got.iter().map(|(n, v, _)| (n.clone(), v.clone())).collect();
    if p.code != Some(0) || !well_formed || printed != expected {
        io.log(&format!("cache-rollback: проба {:?}: {printed:?}, ждали {expected:?}", p.code));
        return Outcome::Refused(ROLLBACK_EXTRA.into());
    }
    if let Err(o) = ctx.touch() {
        return o;
    }
    let mut argv = strings(&["pacman", "-U", "--noconfirm", "--"]);
    argv.extend(copies);
    let rc = ctx.run(&argv, &RunExtra::default());
    if !matches!(rc, Ok(Some(0))) {
        return finished(rc);
    }
    let mut after_q = strings(&["pacman", "-Q", "--"]);
    after_q.extend(names.iter().map(|n| n.to_string()));
    let after = ug::parse_installed(&io.capture(&after_q).stdout);
    let missed: Vec<&str> = got
        .iter()
        .filter(|(n, v, _)| !after.iter().any(|(an, av)| an == n && av == v))
        .map(|(n, _, _)| n.as_str())
        .collect();
    if !missed.is_empty() {
        io.log(&format!("cache-rollback: после pacman -U версии не те: {missed:?}"));
        return Outcome::Done { ok: false, code: Some(1), key: None };
    }
    Outcome::ok(None)
}

/// Top-level files of its own tree (root copy), by name.
fn top_files(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut v: Vec<std::path::PathBuf> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
                .map(|e| e.path())
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

/// Replace the `background=…` lines (like `sed -i s|^background=.*|…|`).
fn set_background_lines(text: &str, dest: &str) -> String {
    let mut out: String = text
        .lines()
        .map(|l| if l.starts_with("background=") { format!("background={dest}") } else { l.to_string() })
        .collect::<Vec<_>>()
        .join("\n");
    if text.ends_with('\n') {
        out.push('\n');
    }
    out
}

const BACKGROUNDS: &str = "/usr/share/backgrounds";
const SLICK_CONF: &str = "/etc/lightdm/slick-greeter.conf";
const GTK_GREETER_CONF: &str = "/etc/lightdm/lightdm-gtk-greeter.conf";
const GDM_DCONF_DIR: &str = "/etc/dconf/db/gdm.d";

/// Staging by category, a port of `rexon-stage-helper`: the destination is a
/// core constant, only the category, the name and the archive descriptor come from the window.
fn stage(ctx: &Ctx<'_>, fd: BorrowedFd<'_>, category: &str, name: &str) -> Outcome {
    let io = ctx.io;
    let work = match work_dir(ctx) {
        Ok(w) => w,
        Err(o) => return o,
    };
    let tree = match unpack::receive_stage(fd, &work, category) {
        Ok(t) => t,
        Err(e) => return e.into(),
    };
    let write = |r: Result<(), String>| -> Result<(), Outcome> {
        r.map_err(|e| {
            (ctx.progress)(&e);
            Outcome::Done { ok: false, code: Some(1), key: None }
        })
    };
    let result = (|| -> Result<Outcome, Outcome> {
        match category {
            "gdm-bg" | "lightdm-bg" | "cosmic-greeter" | "login-bg" => {
                let img = top_files(&tree).into_iter().find(|p| {
                    let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
                    matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "webp" | "bmp")
                });
                let Some(img) = img else { return Ok(ctx.fail("no_image", &[])) };
                let ext = img.extension().and_then(|e| e.to_str()).unwrap_or("png");
                let ext = if !ext.is_empty() && ext.chars().all(|c| c.is_ascii_alphanumeric()) { ext } else { "png" };
                let dest = format!("{BACKGROUNDS}/rexon-login.{ext}");
                ctx.touch()?;
                write(io.create_dir_all(BACKGROUNDS, 0o755))?;
                for old in io.list_dir(BACKGROUNDS) {
                    if old.starts_with("rexon-login.") {
                        let _ = io.remove_file(&format!("{BACKGROUNDS}/{old}"));
                    }
                }
                write(io.copy_file(&path_str(&img), &dest))?;
                match category {
                    "gdm-bg" => {
                        write(io.create_dir_all(GDM_DCONF_DIR, 0o755))?;
                        write(io.write_file(
                            &format!("{GDM_DCONF_DIR}/02-rexon-login"),
                            &format!(
                                "[org/gnome/desktop/background]\npicture-uri='file://{dest}'\n\
                                 picture-uri-dark='file://{dest}'\npicture-options='zoom'\n"
                            ),
                            0o644,
                        ))?;
                        if io.which("dconf").is_some() {
                            let _ = ctx.run_quiet(&strings(&["dconf", "update"]), &RunExtra::default());
                        }
                    }
                    "lightdm-bg" => {
                        if io.is_file(SLICK_CONF) {
                            let text = io.read_text(SLICK_CONF).unwrap_or_default();
                            write(io.write_file(SLICK_CONF, &set_background_lines(&text, &dest), 0o644))?;
                        } else if io.is_dir("/etc/lightdm") {
                            write(io.write_file(SLICK_CONF, &format!("[Greeter]\nbackground={dest}\n"), 0o644))?;
                        }
                        if io.is_file(GTK_GREETER_CONF) {
                            let text = io.read_text(GTK_GREETER_CONF).unwrap_or_default();
                            write(io.write_file(GTK_GREETER_CONF, &set_background_lines(&text, &dest), 0o644))?;
                        }
                    }
                    _ => {}
                }
                ctx.say("bg_installed", &[&dest]);
                Ok(Outcome::ok(Some("bg_installed")))
            }
            "lightdm-webkit" => {
                let roots = validate::stage_destination_roots(category);
                let Some(root) = roots.iter().find(|r| io.is_dir(r)) else {
                    return Ok(ctx.fail("no_greeter_dir", &[]));
                };
                let dest = format!("{root}/{name}");
                ctx.touch()?;
                write(io.copy_tree(&path_str(&tree), &dest))?;
                ctx.say("webkit_theme", &[name]);
                Ok(Outcome::ok(Some("webkit_theme")))
            }
            "mate-layouts" => {
                let Some(dst) = validate::stage_destination_roots(category).first().copied() else {
                    return Ok(Outcome::Internal("no_root".into()));
                };
                let files: Vec<std::path::PathBuf> = top_files(&tree)
                    .into_iter()
                    .filter(|p| matches!(p.extension().and_then(|e| e.to_str()), Some("layout" | "dock")))
                    .collect();
                if files.is_empty() {
                    return Ok(ctx.fail("no_layouts", &[]));
                }
                ctx.touch()?;
                write(io.create_dir_all(dst, 0o755))?;
                for f in files {
                    let name = f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    write(io.copy_file(&path_str(&f), &format!("{dst}/{name}")))?;
                }
                ctx.say("layouts_installed", &[]);
                Ok(Outcome::ok(Some("layouts_installed")))
            }
            _ => Ok(Outcome::Refused("refused.item-name".into())),
        }
    })();
    result.unwrap_or_else(|o| o)
}

// ── Fonts for all users ────────────────────────────────────────────────

/// The font file name extension (`.ttf`, `.pcf.gz`) in lower case, for the name
/// of the root copy: FreeType recognizes the format by content, but fontconfig
/// opens a compressed PCF only with `.gz`.
fn font_suffix(name: &str) -> String {
    let lower = name.to_lowercase();
    crate::qt_settings::FONT_INSTALL_EXTS
        .iter()
        .filter(|e| lower.ends_with(&format!(".{e}")))
        .max_by_key(|e| e.len())
        .map(|e| format!(".{e}"))
        .unwrap_or_default()
}

/// Rebuild the fontconfig cache of the directory for all (no `fc-cache` is no
/// problem: fontconfig notices a new file by the directory modification time itself).
fn refresh_font_cache(ctx: &Ctx<'_>) {
    if ctx.io.which("fc-cache").is_some() {
        let _ = ctx.run_quiet(&crate::qt_settings::fc_cache_argv(crate::qt_settings::SHARED_FONT_DIR), &RunExtra::default());
    }
}

/// Fonts for all: a root copy of each file, `fc-query` on it (what fontconfig
/// does not consider a font is refused before touching the system), a
/// destination probe (a file or link with this name already exists: refusal,
/// what is not ours is not overwritten), then copies into the core directory
/// with permissions 0644 and `fc-cache`. If one fails, those already placed are removed.
fn font_install(ctx: &Ctx<'_>, files: &[OwnedFd], names: &[String]) -> Outcome {
    let io = ctx.io;
    let root = crate::qt_settings::SHARED_FONT_DIR;
    if io.which("fc-query").is_none() {
        return Outcome::Refused("refused.no-fontconfig".into());
    }
    let work = match work_dir(ctx) {
        Ok(w) => w,
        Err(o) => return o,
    };
    let mut copies = Vec::with_capacity(names.len());
    for (i, (fd, name)) in files.iter().zip(names).enumerate() {
        let copy = match unpack::receive_file(fd.as_fd(), &work, &format!("font-{i}{}", font_suffix(name)), validate::MAX_FONT_BYTES) {
            Ok(c) => c,
            Err(e) => return e.into(),
        };
        let copy = path_str(&copy);
        let got = io.capture(&crate::qt_settings::fc_query_families_argv(&copy));
        let families = crate::qt_settings::parse_fc_query_families(&got.stdout);
        if got.code != Some(0) || families.is_empty() {
            io.log(&format!("шрифт: fc-query не узнал {name}"));
            return Deny::from(Refusal::NotAFont(name.clone())).into_outcome();
        }
        let dest = format!("{root}/{name}");
        if io.exists(&dest) || io.is_symlink(&dest) {
            return Deny::from(Refusal::FontExists(name.clone())).into_outcome();
        }
        // The root copy lies in its own 0700 directory with permissions 0600; in the
        // shared directory the font must be readable by everyone.
        {
            use std::os::unix::fs::PermissionsExt;
            if let Err(e) = std::fs::set_permissions(&copy, std::fs::Permissions::from_mode(0o644)) {
                io.log(&format!("шрифт: права копии {name}: {e}"));
                return Outcome::Internal("font_mode".into());
            }
        }
        copies.push((copy, dest));
    }
    if let Err(o) = ctx.touch() {
        return o;
    }
    let fail = |e: String, done: &[String]| {
        (ctx.progress)(&e);
        for d in done.iter().rev() {
            io.log(&format!("шрифт: откат {d}"));
            let _ = io.remove_file(d);
        }
        Outcome::Done { ok: false, code: Some(1), key: None }
    };
    if !io.is_dir(root) {
        if let Err(e) = io.create_dir_all(root, 0o755) {
            return fail(e, &[]);
        }
    }
    let mut done: Vec<String> = Vec::new();
    for (copy, dest) in &copies {
        io.log(&format!("шрифт: кладу {dest}"));
        if let Err(e) = io.copy_file(copy, dest) {
            return fail(e, &done);
        }
        done.push(dest.clone());
    }
    refresh_font_cache(ctx);
    Outcome::ok(None)
}

/// Fonts for all: remove only regular files inside the core directory that the
/// path reaches without links; emptied subdirectories (not the directory itself)
/// are removed; then `fc-cache`. The check comes before the first removal.
fn font_remove(ctx: &Ctx<'_>, files: &[String]) -> Outcome {
    let io = ctx.io;
    let root = crate::qt_settings::SHARED_FONT_DIR;
    let real_root = io.canonicalize(root).unwrap_or_else(|| root.to_string());
    for f in files {
        let Ok(rest) = validate::check_user_path(f, root) else {
            return Deny::from(Refusal::FontFile(f.clone())).into_outcome();
        };
        let honest = io.canonicalize(f).as_deref() == Some(format!("{real_root}/{rest}").as_str());
        if io.is_symlink(f) || !io.is_file(f) || !honest {
            io.log(&format!("шрифт: {f} — не обычный файл каталога шрифтов"));
            return Deny::from(Refusal::FontFile(f.clone())).into_outcome();
        }
    }
    if let Err(o) = ctx.touch() {
        return o;
    }
    let mut failed = false;
    let mut parents: BTreeSet<String> = BTreeSet::new();
    for f in files {
        io.log(&format!("шрифт: удаляю {f}"));
        if let Err(e) = io.remove_file(f) {
            (ctx.progress)(&e);
            failed = true;
            continue;
        }
        let mut dir = f.rsplit_once('/').map(|(d, _)| d.to_string()).unwrap_or_default();
        while dir.len() > root.len() && dir.starts_with(root) {
            parents.insert(dir.clone());
            dir = dir.rsplit_once('/').map(|(d, _)| d.to_string()).unwrap_or_default();
        }
    }
    // Deeper ones first: when a subdirectory empties, its parent is removed too.
    let mut parents: Vec<String> = parents.into_iter().collect();
    parents.sort_by_key(|d| std::cmp::Reverse(d.len()));
    for d in parents {
        if io.is_dir(&d) && !io.is_symlink(&d) && io.list_dir(&d).is_empty() {
            let _ = io.remove_dir(&d);
        }
    }
    refresh_font_cache(ctx);
    if failed {
        Outcome::Done { ok: false, code: Some(1), key: None }
    } else {
        Outcome::ok(None)
    }
}

// ── SDDM login screen: theme background and look carry-over ────────────

/// Restore the files as they were before the edit (`None` means there was no file).
fn restore_files(io: &dyn RootIo, before: Vec<(String, Option<String>, u32)>, tag: &str) {
    for (path, old, mode) in before.into_iter().rev() {
        io.log(&format!("{tag}: откат {path}"));
        let back = match old {
            Some(t) => io.write_file(&path, &t, mode),
            None => io.remove_file(&path),
        };
        if let Err(e) = back {
            io.log(&format!("{tag}: откат {path} не удался: {e}"));
        }
    }
}

/// SDDM theme background. A picture: a root copy, PNG/JPEG by signature, it goes
/// into the theme directory under its own name, the name into `<ConfigFile>.user`;
/// if the name was not written, the picture is removed. Our previous picture
/// (another extension) is removed after success. Without a picture, the
/// `background` and `type` keys leave `.user` (the theme shows its own
/// background), and our picture is removed.
fn sddm_background(ctx: &Ctx<'_>, theme: &str, fd: Option<BorrowedFd<'_>>) -> Outcome {
    let io = ctx.io;
    let deny = |r: Refusal| Deny::from(r).into_outcome();
    let (dir, bg, user) = match sddm_theme_user(io, theme) {
        Ok(t) => t,
        Err(o) => return o,
    };
    let existing = io.read_text(&user);
    let ours: Vec<String> = io.list_dir(&dir).into_iter().filter(|n| plan::sddm_background_is_ours(n)).collect();
    let fail = |e: String| {
        (ctx.progress)(&e);
        Outcome::Done { ok: false, code: Some(1), key: None }
    };
    let Some(fd) = fd else {
        let text = plan::sddm_background_user_text(existing.as_deref(), None);
        if text.is_none() && ours.is_empty() {
            return Outcome::ok(None);
        }
        if let Err(o) = ctx.touch() {
            return o;
        }
        if let Some(t) = text {
            if let Err(o) = write_edits(ctx, &[plan::SessionEdit { path: user, text: t }], "sddm-bg") {
                return o;
            }
        }
        for old in ours {
            let _ = io.remove_file(&format!("{dir}/{old}"));
        }
        return Outcome::ok(None);
    };
    let work = match work_dir(ctx) {
        Ok(w) => w,
        Err(o) => return o,
    };
    let copy = match unpack::receive_file(fd, &work, "login-background", plan::SDDM_BACKGROUND_MAX_BYTES) {
        Ok(c) => c,
        Err(e) => return e.into(),
    };
    let head: Vec<u8> = std::fs::File::open(&copy)
        .and_then(|f| {
            let mut buf = Vec::new();
            f.take(16).read_to_end(&mut buf).map(|_| buf)
        })
        .unwrap_or_default();
    // WebP only if the Qt of this theme's greeter has the WebP module.
    let webp = plan::qt_has_webp(bg.qt, &|p| io.exists(p));
    let Some(ext) = plan::login_image_ext_with(&head, webp) else {
        return deny(Refusal::LoginImage(theme.to_string()));
    };
    {
        use std::os::unix::fs::PermissionsExt;
        if std::fs::set_permissions(&copy, std::fs::Permissions::from_mode(0o644)).is_err() {
            return Outcome::Internal("background_mode".into());
        }
    }
    let name = format!("{}.{ext}", plan::SDDM_BACKGROUND_STEM);
    let dest = format!("{dir}/{name}");
    if io.is_symlink(&dest) {
        return deny(Refusal::SddmTheme(theme.to_string()));
    }
    let had = io.exists(&dest);
    let text = plan::sddm_background_user_text(existing.as_deref(), Some(&name));
    if let Err(o) = ctx.touch() {
        return o;
    }
    io.log(&format!("sddm-bg: кладу {dest}"));
    if let Err(e) = io.copy_file(&path_str(&copy), &dest) {
        return fail(e);
    }
    if let Some(t) = text {
        if let Err(o) = write_edits(ctx, &[plan::SessionEdit { path: user, text: t }], "sddm-bg") {
            if !had {
                let _ = io.remove_file(&dest);
            }
            return o;
        }
    }
    for old in ours.iter().filter(|o| **o != name) {
        let _ = io.remove_file(&format!("{dir}/{old}"));
    }
    Outcome::ok(None)
}

/// The SDDM theme `theme` from the system directory that accepts its own
/// settings: the directory, its background ([`plan::sddm_theme_background`]) and
/// the path `<ConfigFile>.user`. No theme, no settings file, or it (or `.user`)
/// is a link: refusal.
fn sddm_theme_user(io: &dyn RootIo, theme: &str) -> Result<(String, plan::SddmBackground, String), Outcome> {
    let deny = || Deny::from(Refusal::SddmTheme(theme.to_string())).into_outcome();
    let dir = theme_dest(SystemThemeKind::Sddm, theme);
    if !theme_present(io, &dir) {
        return Err(deny());
    }
    let bg = plan::sddm_theme_background(&dir, &|p| io.read_text(p));
    let user = format!("{}.user", bg.config);
    if !bg.supported || io.is_symlink(&bg.config) || io.is_symlink(&user) {
        return Err(deny());
    }
    Ok((dir, bg, user))
}

/// «Show clock» of an SDDM theme: `showClock` in `<ConfigFile>.user` (written
/// whole, atomically: a failure leaves the previous file).
fn sddm_clock(ctx: &Ctx<'_>, theme: &str, show: bool) -> Outcome {
    let io = ctx.io;
    let (_, _, user) = match sddm_theme_user(io, theme) {
        Ok(t) => t,
        Err(o) => return o,
    };
    let Some(text) = plan::sddm_clock_user_text(io.read_text(&user).as_deref(), show) else {
        return Outcome::ok(None);
    };
    if let Err(o) = ctx.touch() {
        return o;
    }
    if let Err(o) = write_edits(ctx, &[plan::SessionEdit { path: user, text }], "sddm-clock") {
        return o;
    }
    Outcome::ok(None)
}

/// The `sddm` account: uid, gid and home directory (via `getent`; the home must
/// be a directory with a normal absolute path).
fn sddm_account(io: &dyn RootIo) -> Option<(u32, u32, String)> {
    greeter_account(io, plan::SDDM_USER)
}

/// The greeter user's account: `(uid, gid, home)`; the home must be a real
/// directory, and the user must not be root.
fn greeter_account(io: &dyn RootIo, user: &str) -> Option<(u32, u32, String)> {
    let got = io.capture(&strings(&["getent", "passwd", user]));
    if got.code != Some(0) {
        return None;
    }
    let f: Vec<&str> = got.stdout.lines().next()?.split(':').collect();
    if f.len() < 7 || f[0] != user {
        return None;
    }
    let (uid, gid) = (f[2].parse::<u32>().ok()?, f[3].parse::<u32>().ok()?);
    let home = f[5].trim_end_matches('/');
    let sane = home.starts_with('/')
        && home.len() > 1
        && !home.split('/').any(|seg| seg == "." || seg == "..")
        && !home.contains("//")
        && !home.chars().any(|c| c.is_control());
    (sane && uid != 0 && io.is_dir(home)).then(|| (uid, gid, home.to_string()))
}

/// argv of a command run as the user `uid:gid` (without supplementary groups).
fn as_user_argv(uid: u32, gid: u32, cmd: &[String]) -> Vec<String> {
    let mut v = strings(&["setpriv"]);
    v.push(format!("--reuid={uid}"));
    v.push(format!("--regid={gid}"));
    v.extend(strings(&["--clear-groups", "--"]));
    v.extend(cmd.iter().cloned());
    v
}

/// Carrying the desktop look over to the SDDM login screen (`look: None` is
/// «Reset»). Order: all checks and file copies (UTF-8) before touching anything;
/// then the SDDM settings keys (rolled back if writing fails); then the files in
/// the `sddm` home, as that user. If the files fail, the keys are restored as they were.
fn sddm_look(ctx: &Ctx<'_>, look: Option<&plan::SddmLook>, files: &[OwnedFd], names: &[String]) -> Outcome {
    let io = ctx.io;
    let deny = |r: Refusal| Deny::from(r).into_outcome();
    let Some((uid, gid, home)) = sddm_account(io) else {
        return deny(Refusal::NoSddmUser);
    };
    if io.which("setpriv").is_none() {
        return deny(Refusal::NoSetpriv);
    }
    let mut contents: Vec<(String, String)> = Vec::new();
    if !files.is_empty() {
        let work = match work_dir(ctx) {
            Ok(w) => w,
            Err(o) => return o,
        };
        for (i, (fd, name)) in files.iter().zip(names).enumerate() {
            let copy = match unpack::receive_file(fd.as_fd(), &work, &format!("look-{i}"), plan::SDDM_LOOK_FILE_MAX_BYTES) {
                Ok(c) => c,
                Err(e) => return e.into(),
            };
            let Some(text) = std::fs::read(&copy).ok().and_then(|b| String::from_utf8(b).ok()) else {
                return deny(Refusal::LookFile(name.clone()));
            };
            contents.push((name.clone(), text));
        }
    }
    let edits = match plan::plan_sddm_look(look, &|p| io.read_text(p), &|d| io.list_dir(d)) {
        plan::SessionSwitch::Nothing => Vec::new(),
        plan::SessionSwitch::Elsewhere => return deny(Refusal::LookElsewhere),
        plan::SessionSwitch::Edits(e) => e,
    };
    if edits.iter().any(|e| io.is_symlink(&e.path)) {
        return deny(Refusal::LookElsewhere);
    }
    if let Err(o) = ctx.touch() {
        return o;
    }
    let before: Vec<(String, Option<String>, u32)> = edits
        .iter()
        .map(|e| (e.path.clone(), io.read_text(&e.path), io.mode_of(&e.path).unwrap_or(0o644)))
        .collect();
    if let Err(o) = write_edits(ctx, &edits, "sddm-look") {
        return o;
    }
    let as_sddm = |cmd: Vec<String>, stdin: Option<String>| -> bool {
        let (rc, text) = ctx.run_quiet(&as_user_argv(uid, gid, &cmd), &RunExtra { stdin, env: Vec::new() });
        if rc != Some(0) {
            io.log(&format!("sddm-look: от имени sddm не вышло ({rc:?}): {text}"));
            (ctx.progress)(&text);
        }
        rc == Some(0)
    };
    let cache = format!("{home}/.cache");
    let ok = match look {
        Some(_) => {
            as_sddm(strings(&["rm", "-rf", "--", &cache]), None)
                && as_sddm(strings(&["mkdir", "-p", "--", &format!("{home}/.config/fontconfig")]), None)
                && contents.into_iter().all(|(name, text)| {
                    as_sddm(
                        strings(&["sh", "-c", "umask 022 && cat > \"$1\"", "rexon-sddm-look", &format!("{home}/.config/{name}")]),
                        Some(text),
                    )
                })
        }
        None => {
            let mut cmd = strings(&["rm", "-rf", "--", &cache]);
            cmd.extend(plan::SDDM_LOOK_RESET_PATHS.iter().map(|p| format!("{home}/{p}")));
            as_sddm(cmd, None)
        }
    };
    if !ok {
        restore_files(io, before, "sddm-look");
        return Outcome::Done { ok: false, code: Some(1), key: None };
    }
    Outcome::ok(None)
}

fn theme_dest(kind: SystemThemeKind, theme: &str) -> String {
    format!("/usr/share/{}/themes/{theme}", kind_dir(kind))
}

/// SDDM/Plymouth theme: a root copy checked by `unpack` (links, `.so`,
/// `ModuleName`) is laid out into `/usr/share/<kind>/themes/<theme>`.
/// Installing does not select the theme; that is «Apply».
fn theme_install(ctx: &Ctx<'_>, fd: BorrowedFd<'_>, kind: SystemThemeKind, theme: &str) -> Outcome {
    let work = match work_dir(ctx) {
        Ok(w) => w,
        Err(o) => return o,
    };
    let tree = match unpack::receive_system_theme(fd, &work, kind, theme) {
        Ok(t) => t,
        Err(e) => return e.into(),
    };
    if let Err(o) = ctx.touch() {
        return o;
    }
    match ctx.io.copy_tree(&path_str(&tree), &theme_dest(kind, theme)) {
        Ok(()) => Outcome::ok(None),
        Err(e) => {
            (ctx.progress)(&e);
            Outcome::Done { ok: false, code: Some(1), key: None }
        }
    }
}

/// The theme sits in the system directory as a regular directory (not a link).
fn theme_present(io: &dyn RootIo, dest: &str) -> bool {
    io.is_dir(dest) && !io.is_symlink(dest)
}

/// Kernel image rebuilders, in order of preference, the first one found.
/// First is plymouth's own script (the same one `plymouth-set-default-theme -R`
/// calls): the distribution sets it up for its bootloader (CachyOS with Limine
/// uses `limine-mkinitcpio`, while a bare `mkinitcpio -P` there does not update
/// the image Limine boots from). Then the same programs directly, if there is
/// no script (Debian, Ubuntu).
const INITRAMFS_REBUILDERS: &[&[&str]] = &[
    &["/usr/lib/plymouth/plymouth-update-initrd"],
    &["/usr/libexec/plymouth/plymouth-update-initrd"],
    &["update-initramfs", "-u"],
    &["limine-mkinitcpio"],
    &["mkinitcpio", "-P"],
    &["limine-dracut"],
    &["dracut", "--regenerate-all", "--force"],
    &["/usr/lib/booster/regenerate_images"],
];

/// Rebuilding the kernel image after the splash theme changes. `None` means
/// nothing to rebuild with; otherwise the result of the run.
fn rebuild_initramfs(ctx: &Ctx<'_>) -> Option<Result<Option<i32>, String>> {
    let argv = INITRAMFS_REBUILDERS.iter().find(|a| ctx.io.which(a[0]).is_some())?;
    Some(ctx.run(&strings(argv), &RunExtra::default()))
}

/// Write settings edits one by one; if one fails, those already written are
/// restored as they were (new files are removed), and the system stays as it
/// was. `tag` is for the helper journal.
fn write_edits(ctx: &Ctx<'_>, edits: &[plan::SessionEdit], tag: &str) -> Result<(), Outcome> {
    let io = ctx.io;
    let mut done: Vec<(&str, Option<String>, u32)> = Vec::new();
    for e in edits {
        let old = io.read_text(&e.path);
        let mode = io.mode_of(&e.path).unwrap_or(0o644);
        io.log(&format!("{tag}: пишу {}", e.path));
        let parent = e.path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
        let r = if old.is_none() && !parent.is_empty() && !io.is_dir(parent) {
            io.create_dir_all(parent, 0o755)
        } else {
            Ok(())
        }
        .and_then(|_| io.write_file(&e.path, &e.text, mode));
        if let Err(err) = r {
            (ctx.progress)(&err);
            for (path, old, mode) in done.into_iter().rev() {
                io.log(&format!("{tag}: откат {path}"));
                let back = match old {
                    Some(t) => io.write_file(path, &t, mode),
                    None => io.remove_file(path),
                };
                if let Err(e) = back {
                    io.log(&format!("{tag}: откат {path} не удался: {e}"));
                }
            }
            return Err(Outcome::Done { ok: false, code: Some(1), key: None });
        }
        done.push((&e.path, old, mode));
    }
    Ok(())
}

/// Plymouth: put the previous theme back into the settings after a failed image
/// rebuild (the image itself is still the old one, the splash does not change) and refuse.
fn plymouth_rollback(ctx: &Ctx<'_>, back: Option<Vec<String>>) -> Outcome {
    if let Some(argv) = back {
        let _ = ctx.run(&argv, &RunExtra::default());
    }
    ctx.fail("initramfs_failed", &[])
}

/// «Apply», like `activate` of `rexon-system-helper`.
fn theme_apply(ctx: &Ctx<'_>, kind: SystemThemeKind, theme: &str) -> Outcome {
    let io = ctx.io;
    let dest = theme_dest(kind, theme);
    if !theme_present(io, &dest) {
        return ctx.fail("theme_not_installed", &[theme]);
    }
    let main = format!("{theme}.plymouth");
    if kind == SystemThemeKind::Plymouth && !io.is_file(&format!("{dest}/{main}")) {
        return ctx.fail("no_plymouth_file", &[&main]);
    }
    if kind == SystemThemeKind::Sddm {
        // The decision is the core's: all `/etc` files where the theme is set, plus
        // our own file if without it the result is wrong (`/etc/sddm.conf` overrides
        // `sddm.conf.d`).
        let edits = match plan::plan_sddm_theme(theme, &|p| io.read_text(p), &|d| io.list_dir(d)) {
            plan::SessionSwitch::Nothing => return Outcome::ok(None),
            plan::SessionSwitch::Elsewhere => return ctx.fail("theme_set_elsewhere", &[theme]),
            plan::SessionSwitch::Edits(e) => e,
        };
        if let Some(e) = edits.iter().find(|e| io.is_symlink(&e.path)) {
            io.log(&format!("sddm-theme: {} — ссылка, ничего не меняю", e.path));
            return ctx.fail("theme_set_elsewhere", &[theme]);
        }
        if let Err(o) = ctx.touch() {
            return o;
        }
        return match write_edits(ctx, &edits, "sddm-theme") {
            Ok(()) => Outcome::ok(None),
            Err(o) => o,
        };
    }
    // The method follows the actual tools, not the family (Ubuntu without
    // plymouth-set-default-theme, Debian without alternatives for plymouth).
    // The theme is selected without `-R`, and the image is rebuilt right after
    // by us: this way it is visible what exactly failed, and the previous theme can be restored.
    if io.which("plymouth-set-default-theme").is_some() {
        let prev = io.capture(&strings(&["plymouth-set-default-theme"]));
        let prev = prev.stdout.trim();
        let back = (prev != theme && validate::check_item_name(prev).is_ok())
            .then(|| strings(&["plymouth-set-default-theme", prev]));
        if let Err(o) = ctx.touch() {
            return o;
        }
        let rc = ctx.run(&strings(&["plymouth-set-default-theme", theme]), &RunExtra::default());
        if !matches!(rc, Ok(Some(0))) {
            return finished(rc);
        }
        return match rebuild_initramfs(ctx) {
            Some(Ok(Some(0))) => Outcome::ok(None),
            _ => plymouth_rollback(ctx, back),
        };
    }
    if io.which("update-alternatives").is_some() {
        let pl = format!("{dest}/{main}");
        let link = "/usr/share/plymouth/themes/default.plymouth";
        let back = io
            .canonicalize(link)
            .filter(|p| *p != pl)
            .map(|p| strings(&["update-alternatives", "--set", "default.plymouth", &p]));
        if let Err(o) = ctx.touch() {
            return o;
        }
        let rc = ctx.run(
            &strings(&["update-alternatives", "--install", link, "default.plymouth", &pl, "100"]),
            &RunExtra::default(),
        );
        if !matches!(rc, Ok(Some(0))) {
            return finished(rc);
        }
        let rc = ctx.run(&strings(&["update-alternatives", "--set", "default.plymouth", &pl]), &RunExtra::default());
        if !matches!(rc, Ok(Some(0))) {
            return finished(rc);
        }
        return match rebuild_initramfs(ctx) {
            Some(Ok(Some(0))) => Outcome::ok(None),
            _ => plymouth_rollback(ctx, back),
        };
    }
    ctx.fail("no_activation", &[])
}

/// A `key = value` line in the settings text (the key is case-insensitive).
fn conf_has(text: &str, key: &str, value: &str) -> bool {
    text.lines().any(|l| {
        l.trim_end_matches('\r')
            .split_once('=')
            .is_some_and(|(k, v)| k.trim().eq_ignore_ascii_case(key) && v.trim() == value)
    })
}

/// Whether the theme is selected now, with a margin, as in the old helper: an
/// extra refusal is better than a removed active theme.
fn theme_is_active(io: &dyn RootIo, kind: SystemThemeKind, theme: &str, dest: &str) -> bool {
    let confs = |dir: &str| -> Vec<String> {
        let mut v: Vec<String> =
            io.list_dir(dir).into_iter().filter(|n| n.ends_with(".conf")).map(|n| format!("{dir}/{n}")).collect();
        v.sort();
        v
    };
    let (files, key) = match kind {
        SystemThemeKind::Sddm => {
            let mut f = confs("/usr/lib/sddm/sddm.conf.d");
            f.extend(confs("/etc/sddm.conf.d"));
            f.push("/etc/sddm.conf".into());
            (f, "Current")
        }
        SystemThemeKind::Plymouth => (
            vec![
                "/etc/plymouth/plymouthd.conf".to_string(),
                "/usr/share/plymouth/plymouthd.defaults".into(),
                "/usr/share/plymouth/plymouthd.conf".into(),
            ],
            "Theme",
        ),
    };
    if files.iter().any(|f| io.read_text(f).is_some_and(|t| conf_has(&t, key, theme))) {
        return true;
    }
    kind == SystemThemeKind::Plymouth
        && io
            .canonicalize("/usr/share/plymouth/themes/default.plymouth")
            .is_some_and(|l| l.starts_with(&format!("{dest}/")))
}

/// The directory belongs to a system package: the package manager removes it.
fn theme_from_package(io: &dyn RootIo, dest: &str) -> bool {
    [
        ["pacman", "-Qoq", "--", dest],
        ["dpkg-query", "-S", "--", dest],
        ["rpm", "-qf", "--", dest],
    ]
    .iter()
    .any(|argv| io.which(argv[0]).is_some() && io.capture(&strings(argv)).code == Some(0))
}

/// «Remove»: only a theme that is not selected now and is not from a package.
fn theme_remove(ctx: &Ctx<'_>, kind: SystemThemeKind, theme: &str) -> Outcome {
    let io = ctx.io;
    let dest = theme_dest(kind, theme);
    if !theme_present(io, &dest) {
        return ctx.fail("theme_not_installed", &[theme]);
    }
    if theme_is_active(io, kind, theme, &dest) {
        return ctx.fail("theme_active", &[theme]);
    }
    if theme_from_package(io, &dest) {
        return ctx.fail("theme_from_package", &[theme]);
    }
    if let Err(o) = ctx.touch() {
        return o;
    }
    match io.remove_tree(&dest) {
        Ok(()) => Outcome::ok(None),
        Err(e) => {
            (ctx.progress)(&e);
            Outcome::Done { ok: false, code: Some(1), key: None }
        }
    }
}

// ── switch-dm: «Make default» for a login screen ────────────────────────
//
// A step-by-step port of the `switch-dm` operation from `rexon-pkg-helper`
// (`system_plan::PKG_HELPER_SCRIPT_TMPL`): the helper determines the previous
// login screen and the mechanism itself, repeats the readiness checks itself,
// verifies the result and rolls back on a mismatch. Only `enable`: it neither
// starts nor stops services in the live session. The safety net (the
// `OnFailure=` fallback service) uses the same core texts.

const UNIT_DIRS: &[&str] =
    &["/etc/systemd/system", "/usr/local/lib/systemd/system", "/usr/lib/systemd/system", "/lib/systemd/system"];
const X11_DEFAULT_DM: &str = "/etc/X11/default-display-manager";
const XGREETERS: &str = "/usr/share/xgreeters";
const POLICY_RC: &str = "/usr/sbin/policy-rc.d";
/// Names of text login programs: only for recognizing a greetd config,
/// nothing is started.
const TEXT_LOGINS: &[&str] = &["agreety", "login", "sh", "bash", "dash", "zsh", "fish"]; // сторож: имена, не запуск

fn basename(p: &str) -> &str {
    p.rsplit('/').next().unwrap_or(p)
}

fn parent_dir(p: &str) -> &str {
    p.rsplit_once('/').map(|(d, _)| d).filter(|d| !d.is_empty()).unwrap_or("/")
}

/// A unit or package name, like `name_ok` of the helper.
fn plain_name(s: &str) -> bool {
    s.chars().next().is_some_and(|c| c.is_ascii_alphanumeric())
        && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

fn first_line(text: Option<String>) -> String {
    text.and_then(|t| t.lines().next().map(str::to_string)).unwrap_or_default()
}

fn debian_pkg(unit: &str) -> String {
    plan::DM_DEBIAN_PKG_FOR_UNIT
        .iter()
        .find(|(u, _)| *u == unit)
        .map(|(_, p)| p.to_string())
        .unwrap_or_else(|| unit.to_string())
}

/// The unit's last `ExecStart=` line without systemd prefixes.
fn exec_start(unit_text: &str) -> String {
    unit_text
        .lines()
        .filter_map(|l| l.trim_start().strip_prefix("ExecStart="))
        .last()
        .unwrap_or("")
        .to_string()
}

/// The greetd config file from the start line (`--config X`, `-c X`, `--config=X`).
fn greetd_config(exec_line: &str) -> String {
    let words: Vec<&str> = exec_line.split_whitespace().collect();
    let mut found = None;
    for (i, w) in words.iter().enumerate() {
        if let Some(v) = w.strip_prefix("--config=").or_else(|| w.strip_prefix("-c=")) {
            found = Some(v.to_string());
        } else if (*w == "--config" || *w == "-c") && i + 1 < words.len() {
            found = Some(words[i + 1].to_string());
        }
    }
    found.unwrap_or_else(|| "/etc/greetd/config.toml".to_string())
}

/// The greeter program from greetd's `[default_session] command = …`.
fn greetd_greeter(config: &str) -> String {
    let mut in_section = false;
    let mut command = None;
    for line in config.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            in_section = t == "[default_session]";
            continue;
        }
        if in_section {
            if let Some((k, v)) = t.split_once('=') {
                if k.trim() == "command" {
                    command = Some(v.trim().trim_matches(['"', '\'']).to_string());
                    break;
                }
            }
        }
    }
    let command = command.unwrap_or_default();
    command
        .split_whitespace()
        .find(|w| *w != "env" && (w.starts_with('/') || !w.contains('=')))
        .unwrap_or("")
        .to_string()
}

/// The login screen's default session: the pair of an X11 session on Wayland.
/// The decision is the core's ([`plan::plan_wayland_default`]); here, machine checks and writing.
fn login_session_wayland(ctx: &Ctx<'_>, s: &plan::SessionSuccessor) -> Outcome {
    let io = ctx.io;
    let target = plan::wayland_session_file(s);
    if !io.is_file(&target) {
        return Outcome::Refused(format!("{}{}", plan::WAYLAND_SESSION_MISSING, s.x11));
    }
    let edits = match plan::plan_wayland_default(s, &|p| io.read_text(p), &|d| io.list_dir(d)) {
        plan::SessionSwitch::Nothing => return Outcome::ok(None),
        plan::SessionSwitch::Elsewhere => {
            io.log(&format!("login-session: {} задан в файле пакета — не меняю", s.x11));
            return Outcome::Refused(format!("{}{}", plan::SESSION_SET_ELSEWHERE, s.x11));
        }
        plan::SessionSwitch::Edits(e) => e,
    };
    // We write none of them if a link is in place of a file (and do not start at all).
    if let Some(e) = edits.iter().find(|e| io.is_symlink(&e.path)) {
        io.log(&format!("login-session: {} — ссылка, ничего не меняю", e.path));
        return Outcome::Refused(format!("{}{}", plan::SESSION_SET_ELSEWHERE, s.x11));
    }
    if let Err(o) = ctx.touch() {
        return o;
    }
    io.log(&format!("login-session: {} → {}", s.x11, s.wayland));
    match write_edits(ctx, &edits, "login-session") {
        Ok(()) => Outcome::ok(None),
        Err(o) => o,
    }
}

/// Carrying autologin over `old` → `new` by the core's decision
/// ([`crate::dm_autologin::plan_transfer`]); `None` means nothing to write.
/// The autologin user is checked against the system: if there is no such user
/// (or it is root), it is not carried over, and we say so.
fn autologin_transfer(ctx: &Ctx<'_>, old: &str, new: &str) -> Option<crate::dm_autologin::Transfer> {
    use crate::dm_autologin as al;
    let io = ctx.io;
    let pam = al::autologin_spec(new).and_then(|s| {
        [format!("/etc/pam.d/{}", s.pam), format!("/usr/lib/pam.d/{}", s.pam)].iter().find_map(|p| io.read_text(p))
    });
    let group_members = pam.as_deref().and_then(al::pam_autologin_group).and_then(|g| {
        let got = io.capture(&strings(&["getent", "group", &g]));
        if got.code != Some(0) || got.stdout.trim().is_empty() {
            return None;
        }
        let line = got.stdout.lines().next().unwrap_or("").to_string();
        let members = line.split(':').nth(3).unwrap_or("");
        Some(members.split(',').map(str::trim).filter(|m| !m.is_empty()).map(str::to_string).collect())
    });
    let facts = al::TargetFacts {
        pam_autologin: pam,
        group_members,
        greeter_user: plan::login_manager_spec(new).and_then(|s| s.user).map(str::to_string),
    };
    let t = al::plan_transfer(old, new, &|p| io.read_text(p), &|d| io.list_dir(d), &facts);
    for n in &t.notes {
        match n {
            al::Note::Moved { user } if !login_user_ok(io, user) => {
                ctx.say("dm_autologin_bad_user", &[user]);
                return None;
            }
            al::Note::LostCommand { user }
            | al::Note::LostUnknown { user }
            | al::Note::LostElsewhere { user }
            | al::Note::LostGreeterUser { user } => ctx.say("dm_autologin_not_moved", &[user, new]),
            _ => {}
        }
    }
    (!t.edits.is_empty() || t.group.is_some()).then_some(t)
}

/// The autologin user exists in the system and is not root.
fn login_user_ok(io: &dyn RootIo, user: &str) -> bool {
    if !plain_name(user) {
        return false;
    }
    let got = io.capture(&strings(&["getent", "passwd", user]));
    let line = got.stdout.lines().next().unwrap_or("").to_string();
    let f: Vec<&str> = line.split(':').collect();
    got.code == Some(0) && f.len() >= 7 && f[0] == user && f[2].parse::<u32>().is_ok_and(|uid| uid != 0)
}

/// Write autologin for the new login screen: the files (the previous texts go
/// into `saved` for a rollback), then the PAM group. An error is a text for a person.
fn apply_autologin(
    ctx: &Ctx<'_>,
    t: &crate::dm_autologin::Transfer,
    saved: &std::cell::RefCell<Vec<(String, Option<String>, u32)>>,
    group_done: &std::cell::RefCell<Option<(String, String, bool)>>,
) -> Result<(), String> {
    let io = ctx.io;
    // We do not write if a link is in place of a file (as for the default session).
    if let Some(e) = t.edits.iter().find(|e| io.is_symlink(&e.path)) {
        io.log(&format!("autologin: {} — ссылка, не пишу", e.path));
        return Err(ctx.say_text("dm_autologin_write_failed", &[]));
    }
    *saved.borrow_mut() =
        t.edits.iter().map(|e| (e.path.clone(), io.read_text(&e.path), io.mode_of(&e.path).unwrap_or(0o644))).collect();
    if write_edits(ctx, &t.edits, "autologin").is_err() {
        // write_edits has already restored what was partially written by itself.
        saved.borrow_mut().clear();
        return Err(ctx.say_text("dm_autologin_write_failed", &[]));
    }
    if let Some((user, group, create)) = &t.group {
        if *create {
            let (rc, out) = ctx.run_quiet(&strings(&["groupadd", "-r", group]), &RunExtra::default());
            if rc != Some(0) {
                return Err(out);
            }
            *group_done.borrow_mut() = Some((String::new(), group.clone(), true));
        }
        let (rc, out) = ctx.run_quiet(&strings(&["gpasswd", "-a", user, group]), &RunExtra::default());
        if rc != Some(0) {
            return Err(out);
        }
        *group_done.borrow_mut() = Some((user.clone(), group.clone(), *create));
    }
    Ok(())
}

/// Tell the person what was done with autologin.
fn autologin_said(ctx: &Ctx<'_>, t: &crate::dm_autologin::Transfer, new: &str) {
    use crate::dm_autologin::Note;
    for n in &t.notes {
        match n {
            Note::Moved { user } => ctx.say("dm_autologin_moved", &[user, new]),
            Note::TurnedOff { user } => ctx.say("dm_autologin_off", &[user, new]),
            Note::Group { user, group } => ctx.say("dm_autologin_group", &[user, group]),
            _ => {}
        }
    }
}

/// Login screen sound: the greeter user of the previous login screen has its
/// sound units masked (a workaround for "the volume drops to 40% after login"),
/// and the same masks are set for the greeter user of the new one. Written as
/// that user (`setpriv`): not a single root file in their home. A failure does
/// not cancel the switch (the login screen has already been changed): we say
/// so and go on.
fn move_greeter_sound_masks(ctx: &Ctx<'_>, old: &str, new: &str) {
    use crate::dm_autologin as al;
    let io = ctx.io;
    let user_of = |u: &str| plan::login_manager_spec(u).and_then(|s| s.user);
    let (Some(old_user), Some(new_user)) = (user_of(old), user_of(new)) else { return };
    if old_user == new_user {
        return;
    }
    let Some((_, _, old_home)) = greeter_account(io, old_user) else { return };
    let old_dir = al::greeter_user_unit_dir(&old_home);
    let masked: Vec<&str> = al::GREETER_AUDIO_UNITS
        .iter()
        .copied()
        .filter(|u| {
            let p = format!("{old_dir}/{u}");
            let link = if io.is_symlink(&p) { io.canonicalize(&p) } else { None };
            let text = if !io.is_symlink(&p) && io.is_file(&p) { io.read_text(&p) } else { None };
            al::unit_masked(link.as_deref(), text.as_deref())
        })
        .collect();
    if masked.is_empty() {
        return;
    }
    let Some((uid, gid, new_home)) = greeter_account(io, new_user) else {
        io.log(&format!("greeter-sound: у {new_user} нет дома — маски не ставлю"));
        return;
    };
    let new_dir = al::greeter_user_unit_dir(&new_home);
    let add = al::audio_masks_to_add(&masked, &|u| {
        let p = format!("{new_dir}/{u}");
        io.exists(&p) || io.is_symlink(&p)
    });
    if add.is_empty() {
        return;
    }
    if io.which("setpriv").is_none() {
        ctx.say("dm_greeter_sound_failed", &[new]);
        return;
    }
    let mut cmds = vec![strings(&["mkdir", "-p", &new_dir])];
    cmds.extend(add.iter().map(|u| strings(&["ln", "-s", "/dev/null", &format!("{new_dir}/{u}")])));
    for cmd in cmds {
        let (rc, _) = ctx.run_quiet(&as_user_argv(uid, gid, &cmd), &RunExtra::default());
        if rc != Some(0) {
            ctx.say("dm_greeter_sound_failed", &[new]);
            return;
        }
    }
    ctx.say("dm_greeter_sound_off", &[new, old]);
}

fn switch_dm(ctx: &Ctx<'_>, family: PkgFamily, new: &str, greeter: Option<&str>) -> Outcome {
    let io = ctx.io;
    if io.which("systemctl").is_none() {
        return ctx.fail("no_systemctl", &[]);
    }
    if !io.is_dir("/run/systemd/system") {
        return ctx.fail("no_systemd", &[]);
    }
    // The selection mechanism follows this machine (the same decision as dm_switch_backend).
    for p in [plan::DM_SYMLINK, "/usr/lib/systemd/system/display-manager.service"] {
        if io.is_file(p) && io.canonicalize(p).is_some_and(|r| basename(&r) == "display-manager.service") {
            return ctx.fail("dm_backend_unsupported", &[]);
        }
    }
    let debian = family == PkgFamily::Debian && io.is_file(X11_DEFAULT_DM);

    let Some(unit_file) = UNIT_DIRS
        .iter()
        .map(|d| format!("{d}/{new}.service"))
        .find(|p| io.exists(p) || io.is_symlink(p))
    else {
        return ctx.fail("dm_no_unit", &[new]);
    };
    let Some(unit_real) = io.canonicalize(&unit_file) else {
        return ctx.fail("dm_no_unit", &[new]);
    };
    if unit_real == "/dev/null" {
        return ctx.fail("dm_masked", &[new]);
    }

    let exec_line = exec_start(&io.read_text(&unit_real).unwrap_or_default());
    let exec_bin = exec_line
        .trim_start_matches(['-', '@', ':', '+', '!'])
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_string();
    if exec_bin.is_empty() {
        return ctx.fail("dm_no_program", &[new]);
    }
    let Some(bin_path) = io.which(&exec_bin) else {
        return ctx.fail("dm_no_program", &[&exec_bin]);
    };

    if let Some(spec) = plan::LOGIN_MANAGER_SPECS.iter().find(|s| s.unit == new) {
        let pam = spec.pam;
        if !pam.is_empty() && !io.is_file(&format!("/etc/pam.d/{pam}")) && !io.is_file(&format!("/usr/lib/pam.d/{pam}")) {
            return ctx.fail("dm_no_pam", &[pam]);
        }
    }

    // The unit's required services (`Requires=`; for DDM it is dde-seatd): without
    // them systemd will not start the login screen.
    let unit_text = io.read_text(&unit_real).unwrap_or_default();
    for need in plan::unit_values(&unit_text, "Unit", "Requires") {
        if !need.ends_with(".service") || need.contains('@') {
            continue;
        }
        let found = UNIT_DIRS.iter().map(|d| format!("{d}/{need}")).find(|p| io.exists(p) || io.is_symlink(p));
        let ok = found.and_then(|p| io.canonicalize(&p)).is_some_and(|r| r != "/dev/null");
        if !ok {
            return ctx.fail("dm_needs_unit", &[new, &need]);
        }
    }

    // greetd and everything through it: the greeter comes from ITS OWN config;
    // a text login does not fit.
    if basename(&exec_bin) == "greetd" {
        let cfg = greetd_config(&exec_line);
        let gprog = greetd_greeter(&io.read_text(&cfg).unwrap_or_default());
        let shown = if gprog.is_empty() { "agreety" } else { gprog.as_str() };
        if TEXT_LOGINS.contains(&basename(shown)) {
            return ctx.fail("dm_text_login", &[]);
        }
        if io.which(&gprog).is_none() {
            return ctx.fail("dm_no_greeter_prog", &[&gprog]);
        }
    }

    if let Some(g) = greeter {
        let gfile = format!("{XGREETERS}/{g}.desktop");
        if !io.is_file(&gfile) {
            return ctx.fail("dm_no_greeter_prog", &[g]);
        }
        let gexec = io
            .read_text(&gfile)
            .unwrap_or_default()
            .lines()
            .find_map(|l| l.strip_prefix("Exec=").map(|v| v.split_whitespace().next().unwrap_or("").to_string()))
            .unwrap_or_default();
        if gexec.is_empty() || io.which(&gexec).is_none() {
            return ctx.fail("dm_no_greeter_prog", &[g]);
        }
    }
    if new == "lightdm" {
        if !io.list_dir(XGREETERS).iter().any(|n| n.ends_with(".desktop")) {
            return ctx.fail("lightdm_no_greeter", &[]);
        }
        if io.which("Xorg").is_none() && !io.exists("/usr/lib/Xorg") {
            return ctx.fail("dm_no_xorg", &[]);
        }
    }

    // The previous login screen only by a live link.
    let (mut old, mut old_real) = (String::new(), String::new());
    if io.is_symlink(plan::DM_SYMLINK) && io.exists(plan::DM_SYMLINK) {
        old_real = io.canonicalize(plan::DM_SYMLINK).unwrap_or_default();
        old = basename(&old_real).trim_end_matches(".service").to_string();
        if !plain_name(&old) {
            old.clear();
            old_real.clear();
        }
    }
    if !old_real.is_empty() && old_real == unit_real {
        ctx.say("dm_already_default", &[new]);
        return Outcome::ok(Some("dm_already_default"));
    }
    let (mut old_x11, mut old_pkg) = (String::new(), String::new());
    if debian {
        old_x11 = first_line(io.read_text(X11_DEFAULT_DM));
        if !old.is_empty() {
            old_pkg = debian_pkg(&old);
        }
    }

    // The previous one's autologin goes to the new one (core decision, `dm_autologin`);
    // so far read-only.
    let autologin = if old.is_empty() { None } else { autologin_transfer(ctx, &old, new) };

    // ── from here on the system changes ─────────────────────────────
    if let Err(o) = ctx.touch() {
        return o;
    }
    let etc = plan::DM_FALLBACK_DIR;
    let greeter_prev = format!("{etc}/dm-fallback.greeter-prev");

    // The previous safety net is removed: it belonged to the previous switch.
    for entry in io.list_dir("/etc/systemd/system") {
        if !entry.ends_with(".service.d") {
            continue;
        }
        let dir = format!("/etc/systemd/system/{entry}");
        let f = format!("{dir}/{}", plan::DM_FALLBACK_DROPIN);
        if io.exists(&f) || io.is_symlink(&f) {
            let _ = io.remove_file(&f);
            let _ = io.remove_dir(&dir);
        }
    }
    for f in [format!("{etc}/dm-fallback.done"), format!("{etc}/dm-fallback.conf"), greeter_prev.clone()] {
        if io.exists(&f) || io.is_symlink(&f) {
            let _ = io.remove_file(&f);
        }
    }

    let debconf_set = |pkg: &str| {
        if io.which("debconf-communicate").is_some() {
            let extra = RunExtra { stdin: Some(format!("set shared/default-x-display-manager {pkg}\n")), env: Vec::new() };
            let _ = ctx.run_quiet(&strings(&["debconf-communicate"]), &extra);
        }
    };
    let greeter_written = std::cell::Cell::new(false);
    // The previous texts of the autologin files and what was done with the group, for a rollback.
    let autologin_saved: std::cell::RefCell<Vec<(String, Option<String>, u32)>> = Default::default();
    let group_done: std::cell::RefCell<Option<(String, String, bool)>> = Default::default();
    let rollback = || {
        for (path, text, mode) in autologin_saved.borrow().iter().rev() {
            io.log(&format!("autologin: откат {path}"));
            let _ = match text {
                Some(t) => io.write_file(path, t, *mode),
                None => io.remove_file(path),
            };
        }
        if let Some((user, group, created)) = group_done.borrow().as_ref() {
            if !user.is_empty() {
                let _ = ctx.run_quiet(&strings(&["gpasswd", "-d", user, group]), &RunExtra::default());
            }
            if *created {
                let _ = ctx.run_quiet(&strings(&["groupdel", group]), &RunExtra::default());
            }
        }
        if !old.is_empty() {
            let _ = ctx.run_quiet(&strings(&["systemctl", "enable", "--force", &format!("{old}.service")]), &RunExtra::default());
        }
        if debian && !old_x11.is_empty() {
            let _ = io.write_file(X11_DEFAULT_DM, &format!("{old_x11}\n"), 0o644);
            if !old_pkg.is_empty() {
                debconf_set(&old_pkg);
            }
        }
        if greeter_written.get() {
            if io.is_file(&greeter_prev) {
                let _ = io.copy_file(&greeter_prev, plan::LIGHTDM_GREETER_CONF);
            } else {
                let _ = io.remove_file(plan::LIGHTDM_GREETER_CONF);
            }
        }
    };
    let broke = |err: String| {
        rollback();
        ctx.fail("dm_switch_failed", &[new, &err])
    };

    if let Some(g) = greeter {
        let step = (|| -> Result<(), String> {
            io.create_dir_all(parent_dir(plan::LIGHTDM_GREETER_CONF), 0o755)?;
            io.create_dir_all(etc, 0o755)?;
            if io.is_file(plan::LIGHTDM_GREETER_CONF) {
                io.copy_file(plan::LIGHTDM_GREETER_CONF, &greeter_prev)?;
            }
            greeter_written.set(true);
            io.write_file(plan::LIGHTDM_GREETER_CONF, &format!("[Seat:*]\ngreeter-session={g}\n"), 0o644)
        })();
        if let Err(e) = step {
            return broke(e);
        }
    }

    if let Some(t) = &autologin {
        if let Err(e) = apply_autologin(ctx, t, &autologin_saved, &group_done) {
            return broke(e);
        }
    }

    if debian {
        let new_pkg = debian_pkg(new);
        debconf_set(&new_pkg);
        if let Err(e) = io.write_file(X11_DEFAULT_DM, &format!("{bin_path}\n"), 0o644) {
            return broke(e);
        }
        if io.which("dpkg-reconfigure").is_some() {
            // policy-rc.d: the package script must not start the service in the live session.
            let own_policy = !io.exists(POLICY_RC) && io.write_file(POLICY_RC, "#!/bin/sh\nexit 101\n", 0o755).is_ok();
            let extra = RunExtra { stdin: None, env: vec![("DEBIAN_FRONTEND", "noninteractive".into())] };
            let _ = ctx.run_quiet(&strings(&["dpkg-reconfigure", &new_pkg]), &extra);
            if own_policy {
                let _ = io.remove_file(POLICY_RC);
            }
        }
    }

    let (rc, out) = ctx.run_quiet(&strings(&["systemctl", "enable", "--force", &format!("{new}.service")]), &RunExtra::default());
    if rc != Some(0) {
        return broke(out);
    }

    // The "after" check: the link, the unit state, on Debian the selection file.
    let link_ok = io.canonicalize(plan::DM_SYMLINK).as_deref() == Some(unit_real.as_str());
    let state = io.capture(&strings(&["systemctl", "is-enabled", &format!("{new}.service")]));
    let enabled_ok = matches!(state.stdout.trim(), "enabled" | "enabled-runtime" | "alias");
    let x11_ok = !debian || first_line(io.read_text(X11_DEFAULT_DM)) == bin_path;
    if !(link_ok && enabled_ok && x11_ok) {
        rollback();
        return ctx.fail("dm_switch_mismatch", &[new]);
    }

    // Otherwise the previous one with WantedBy= (a home-made unit) would come up second.
    if !old.is_empty()
        && io.read_text(&old_real).unwrap_or_default().lines().any(|l| l.trim_start().starts_with("WantedBy="))
    {
        let _ = ctx.run_quiet(&strings(&["systemctl", "disable", &format!("{old}.service")]), &RunExtra::default());
    }

    if let Some(t) = &autologin {
        autologin_said(ctx, t, new);
    }
    if !old.is_empty() {
        move_greeter_sound_masks(ctx, &old, new);
    }

    if old.is_empty() {
        if io.exists(plan::PREVIOUS_DM_FILE) {
            let _ = io.remove_file(plan::PREVIOUS_DM_FILE);
        }
        ctx.say("dm_switched_first", &[new]);
        return Outcome::ok(Some("dm_switched_first"));
    }

    // Safety net: if the new login screen crashes at boot, the fallback service restores the previous one.
    let conf = format!(
        "old={old}\nold_real={old_real}\nnew={new}\nnew_real={unit_real}\nold_x11={old_x11}\nold_pkg={old_pkg}\ngreeter_written={}\n",
        u8::from(greeter_written.get())
    );
    let back = if debian {
        format!("sudo dpkg-reconfigure {old_pkg} && sudo systemctl enable --force {old}.service && sudo reboot")
    } else {
        format!("sudo systemctl enable --force {old}.service && sudo reboot")
    };
    let previous = format!(
        "{}\n{}\n    {back}\n{}\n",
        ctx.say_text("prev_dm_header", &[&old, new]),
        ctx.say_text("prev_dm_back", &[&old]),
        ctx.say_text("prev_dm_auto", &[new, &old]),
    );
    let dropin_dir = format!("/etc/systemd/system/{new}.service.d");
    let insured = (|| -> Result<(), String> {
        io.create_dir_all(etc, 0o755)?;
        io.write_file(&format!("{etc}/dm-fallback.conf"), &conf, 0o644)?;
        io.write_file(plan::DM_FALLBACK_SCRIPT_PATH, plan::DM_FALLBACK_SCRIPT, 0o755)?;
        io.write_file(&format!("/etc/systemd/system/{}", plan::DM_FALLBACK_SERVICE), &plan::dm_fallback_unit(), 0o644)?;
        io.create_dir_all(&dropin_dir, 0o755)?;
        io.write_file(
            &format!("{dropin_dir}/{}", plan::DM_FALLBACK_DROPIN),
            &format!("[Unit]\nOnFailure={}\n", plan::DM_FALLBACK_SERVICE),
            0o644,
        )?;
        io.write_file(plan::PREVIOUS_DM_FILE, &previous, 0o644)
    })();
    if let Err(e) = insured {
        // The login screen is already switched; no safety net is a failure, but not a
        // reason to roll back what was done (like `set -e` of the old helper).
        (ctx.progress)(&e);
        return Outcome::Done { ok: false, code: Some(1), key: None };
    }
    ctx.say("dm_switched", &[new, &old]);
    Outcome::ok(Some("dm_switched"))
}

// ============================================================================
// Session: socket, handshake, loop
// ============================================================================

/// Session settings.
#[derive(Debug, Clone)]
pub struct SessionConfig {
    /// The helper's [`proto::build_id`].
    pub build_id: String,
    /// Who entered the password (`PKEXEC_UID`).
    pub pkexec_uid: u32,
    /// Expected pid of the peer: the helper's parent (`getppid()`).
    pub parent_pid: i32,
    /// The window's logind session number (checked, not taken on trust).
    pub session_id: String,
    /// Language of messages for the person.
    pub lang: Option<String>,
    pub poll_interval: Duration,
    pub idle_limit: Duration,
}

/// How the session ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionEnd {
    /// Wrong `SO_PEERCRED` (foreign uid or pid): not a word in reply.
    PeerRejected,
    /// Handshake: a different build, or a message before `Hello`.
    HandshakeFailed(ErrorCode),
    /// logind: the session belongs to the wrong user or is not active.
    SessionRejected,
    /// The privilege is revoked, and no operation is running.
    Revoked(Reason),
}

/// The session result for the journal and tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionReport {
    pub end: SessionEnd,
    /// How many operations reached execution and finished.
    pub ops_executed: u32,
}

/// The helper got SIGTERM (the window's PDEATHSIG or `kill`). Only the signal
/// handler of the live helper sets it.
static SIGTERM_SEEN: AtomicBool = AtomicBool::new(false);

extern "C" fn on_sigterm(_: libc::c_int) {
    SIGTERM_SEEN.store(true, Ordering::SeqCst);
}

/// Sending to the socket from several threads; errors (the window is gone) are
/// swallowed: the running operation is finished even without a peer (Z4).
struct Out {
    fd: Arc<OwnedFd>,
    lock: Mutex<()>,
}

impl Out {
    fn send(&self, msg: &HelperMsg) {
        let _g = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let _ = proto::send_msg(self.fd.as_fd(), msg);
    }

    fn error(&self, id: Option<u64>, code: ErrorCode, key: &str) {
        self.send(&HelperMsg::Error { id, code, message_key: key.to_string() });
    }
}

enum Input {
    Msg(Result<Option<(ClientMsg<PrivOp>, Vec<OwnedFd>)>, ProtoError>),
    Watch(Vec<Event>),
    OpEnded(u64),
}

/// `SO_PEERCRED`: (uid, pid) of the peer.
pub(crate) fn peer_cred(fd: BorrowedFd<'_>) -> Option<(u32, i32)> {
    let mut cred = libc::ucred { pid: 0, uid: 0, gid: 0 };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: getsockopt writes no more than `len` bytes into `cred`.
    let rc = unsafe {
        libc::getsockopt(
            fd.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut cred as *mut libc::ucred).cast(),
            &mut len,
        )
    };
    (rc == 0).then_some((cred.uid, cred.pid))
}

struct Running {
    id: u64,
    state: Arc<Mutex<OpState>>,
    handle: JoinHandle<()>,
}

/// The whole session: from checking the peer to the exit. Returns when it is
/// time for the helper to exit; by then the socket is closed.
pub fn run_session(
    sock: OwnedFd,
    io: Arc<dyn RootIo>,
    cfg: SessionConfig,
    watch: Box<dyn SessionWatch + Send>,
    clock: Arc<dyn Clock + Send + Sync>,
) -> SessionReport {
    let sock = Arc::new(sock);
    match peer_cred(sock.as_fd()) {
        Some((uid, pid)) if validate::check_peer(uid, pid, cfg.pkexec_uid, cfg.parent_pid).is_ok() => {}
        other => {
            io.log(&format!("чужой собеседник на сокете: {other:?}"));
            return SessionReport { end: SessionEnd::PeerRejected, ops_executed: 0 };
        }
    }

    let (tx, rx) = mpsc::channel::<Input>();
    let reader = {
        let tx = tx.clone();
        let sock = Arc::clone(&sock);
        thread::spawn(move || loop {
            let got = proto::recv_frame_with_fds(sock.as_fd()).and_then(|f| match f {
                None => Ok(None),
                Some((frame, fds)) => proto::decode::<ClientMsg<PrivOp>>(&frame).map(|m| Some((m, fds))),
            });
            let last = !matches!(got, Ok(Some(_)));
            if tx.send(Input::Msg(got)).is_err() || last {
                break;
            }
        })
    };
    let (stop_tx, stop_rx) = mpsc::channel::<()>();
    let poller = {
        let tx = tx.clone();
        let interval = cfg.poll_interval;
        let mut watch = watch;
        thread::spawn(move || loop {
            match stop_rx.recv_timeout(interval) {
                Err(RecvTimeoutError::Timeout) => {}
                _ => break,
            }
            let mut events = watch.poll();
            if SIGTERM_SEEN.load(Ordering::SeqCst) {
                events.push(Event::WindowClosed);
            }
            if tx.send(Input::Watch(events)).is_err() {
                break;
            }
        })
    };

    let out = Arc::new(Out { fd: Arc::clone(&sock), lock: Mutex::new(()) });
    let report = session_loop(&io, &cfg, &out, &tx, &rx, clock.as_ref());

    // Cleanup: stop polling, close the socket on both sides (the reading thread
    // will see EOF), wait for the threads.
    drop(stop_tx);
    // SAFETY: shutdown only marks the socket closed, the fd stays ours.
    unsafe { libc::shutdown(sock.as_raw_fd(), libc::SHUT_RDWR) };
    let _ = poller.join();
    // The reading thread is NOT waited for: the helper's exit must not depend on
    // whether `shutdown` wakes a thread sitting in `recvmsg`. The window has
    // already got EOF; the thread exits by itself as soon as it wakes up (for the
    // live helper, together with the process).
    drop(reader);
    io.log(&format!("выход: {:?}, операций исполнено {}", report.end, report.ops_executed));
    report
}

fn session_loop(
    io: &Arc<dyn RootIo>,
    cfg: &SessionConfig,
    out: &Arc<Out>,
    tx: &Sender<Input>,
    rx: &Receiver<Input>,
    clock: &(dyn Clock + Send + Sync),
) -> SessionReport {
    let mut revoker = Revoker::with_idle_limit(clock.now(), cfg.idle_limit);
    let mut facts: Option<Facts> = None;
    let mut current: Option<Running> = None;
    let mut last_id: u64 = 0;
    let mut last_finished: Option<u64> = None;
    let mut announced = false;
    let mut executed = 0u32;
    let rebuilt: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let end = |end: SessionEnd, executed: u32| SessionReport { end, ops_executed: executed };

    loop {
        // The revocation is reported to the window once (except when the window closed: nobody to tell).
        if let Some(reason) = revoker.revoked() {
            if !announced {
                announced = true;
                io.log(&format!("право погашено: {}", reason.code()));
                if reason != Reason::WindowClosed {
                    out.error(None, ErrorCode::Revoked, &format!("revoked.{}", reason.code()));
                }
            }
            if revoker.may_exit() {
                return end(SessionEnd::Revoked(reason), executed);
            }
        }
        let Ok(input) = rx.recv() else {
            return end(SessionEnd::Revoked(Reason::WindowClosed), executed);
        };
        match input {
            Input::Watch(events) => {
                for e in events {
                    revoker.on_event(e, clock.now());
                }
                revoker.tick(clock.now());
            }
            Input::OpEnded(id) => {
                revoker.op_finished(clock.now());
                last_finished = Some(id);
                executed += 1;
                if let Some(r) = current.take() {
                    let _ = r.handle.join();
                }
            }
            Input::Msg(Ok(None)) => {
                io.log("окно закрыло сокет");
                revoker.on_event(Event::WindowClosed, clock.now());
            }
            Input::Msg(Err(e)) => {
                // A frame error = end of the session, without a reply (proto.rs).
                io.log(&format!("ошибка кадра: {e}"));
                revoker.on_event(Event::WindowClosed, clock.now());
            }
            Input::Msg(Ok(Some((msg, fds)))) => {
                let Some(facts_now) = facts.as_ref() else {
                    // Handshake.
                    let ClientMsg::Hello(hello) = msg else {
                        out.error(None, ErrorCode::Sequence, "hello_first");
                        return end(SessionEnd::HandshakeFailed(ErrorCode::Sequence), 0);
                    };
                    match handshake(io.as_ref(), cfg, &hello) {
                        Ok(f) => {
                            io.log(&format!("сеанс открыт: uid {}, сеанс {}, {:?}", cfg.pkexec_uid, cfg.session_id, f));
                            facts = Some(f);
                            out.send(&HelperMsg::Welcome { build_id: cfg.build_id.clone() });
                        }
                        Err((code, key, why)) => {
                            io.log(&format!("рукопожатие отвергнуто: {key}"));
                            out.error(None, code, key);
                            return end(why, 0);
                        }
                    }
                    continue;
                };
                match msg {
                    ClientMsg::Hello(_) => out.error(None, ErrorCode::Sequence, "hello_twice"),
                    ClientMsg::Cancel { id } => match &current {
                        Some(r) if r.id == id => {
                            let taken = request_cancel(&r.state);
                            io.log(&format!("отмена {id}: {}", if taken { "принята" } else { "поздно, доводим" }));
                        }
                        // A cancel that crossed paths with the end of the operation is not an error.
                        _ if last_finished == Some(id) => {}
                        _ => out.error(Some(id), ErrorCode::Sequence, "cancel_unknown"),
                    },
                    ClientMsg::Request { id, op } => {
                        if id <= last_id {
                            out.error(Some(id), ErrorCode::Sequence, "id_reused");
                            continue;
                        }
                        if current.is_some() {
                            out.send(&HelperMsg::Busy { id });
                            continue;
                        }
                        last_id = id;
                        if let Err(reason) = revoker.op_started(clock.now()) {
                            out.error(Some(id), ErrorCode::Revoked, &format!("revoked.{}", reason.code()));
                            continue;
                        }
                        let checked = check_op(&op, facts_now, fds.len());
                        let plan_ = match checked {
                            Ok(p) => p,
                            Err(deny) => {
                                io.log(&format!("запрос {id} отвергнут: {} ({op:?})", deny.key));
                                out.error(Some(id), deny.code, &deny.key);
                                revoker.op_finished(clock.now());
                                last_finished = Some(id);
                                continue;
                            }
                        };
                        io.log(&format!("запрос {id} принят: {plan_:?}"));
                        let state = Arc::new(Mutex::new(OpState::default()));
                        let handle = spawn_worker(id, plan_, fds, io, out, tx, &state, cfg.lang.clone(), &rebuilt);
                        current = Some(Running { id, state, handle });
                    }
                }
            }
        }
    }
}

/// `Hello` → build fingerprint → logind session → machine facts.
fn handshake(
    io: &dyn RootIo,
    cfg: &SessionConfig,
    hello: &Hello,
) -> Result<Facts, (ErrorCode, &'static str, SessionEnd)> {
    if let Err(code) = proto::check_hello(hello, &cfg.build_id) {
        return Err((code, "build_mismatch", SessionEnd::HandshakeFailed(code)));
    }
    let (desktop, kind) = verify_session(io, &cfg.session_id, cfg.pkexec_uid)
        .map_err(|key| (ErrorCode::Refused, key, SessionEnd::SessionRejected))?;
    Ok(machine_facts(io, &desktop, &kind, cfg.pkexec_uid))
}

#[allow(clippy::too_many_arguments)]
fn spawn_worker(
    id: u64,
    plan_: Plan,
    files: Vec<OwnedFd>,
    io: &Arc<dyn RootIo>,
    out: &Arc<Out>,
    tx: &Sender<Input>,
    state: &Arc<Mutex<OpState>>,
    lang: Option<String>,
    rebuilt: &Arc<Mutex<Vec<String>>>,
) -> JoinHandle<()> {
    let (io, out, tx, state, rebuilt) =
        (Arc::clone(io), Arc::clone(out), tx.clone(), Arc::clone(state), Arc::clone(rebuilt));
    thread::spawn(move || {
        let progress = |line: &str| {
            for chunk in cut_progress(line) {
                out.send(&HelperMsg::Progress { id, line: chunk });
            }
        };
        let ctx = Ctx { io: io.as_ref(), progress: &progress, state: &state, lang: lang.as_deref(), rebuilt: &rebuilt };
        let outcome = execute(&plan_, &ctx, files);
        io.log(&format!("запрос {id}: {outcome:?}"));
        // First tell the loop "the operation ended", then send the result to the
        // window. The other way round there was a race: the window got the result,
        // immediately sent the next request, while the loop did not know about the
        // end yet and answered `Busy` (caught under load by the bench
        // `замена_конфликтующего_только_из_таблицы_ядра`). On `OpEnded` the loop waits
        // for this thread (`join`), so the result still reaches the window before any
        // next reply of the loop (privilege revocation etc.).
        let _ = tx.send(Input::OpEnded(id));
        match outcome {
            Outcome::Done { ok, code, key } => out.send(&HelperMsg::Done { id, ok, code, message_key: key }),
            Outcome::Cancelled => out.error(Some(id), ErrorCode::Cancelled, "cancelled"),
            Outcome::Refused(key) => out.error(Some(id), ErrorCode::Refused, &key),
            Outcome::Internal(key) => out.error(Some(id), ErrorCode::Internal, &key),
        }
    })
}

// ============================================================================
// Live start (bin/rexon-priv-helper.rs)
// ============================================================================

/// Helper arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    /// The window's logind session number (`--session <id>`): only checked.
    /// Absent means the window did not know it; the helper takes the user's
    /// graphical session from logind itself ([`session_of_user`]).
    pub session: Option<String>,
    /// Message language (`--lang <code>`).
    pub lang: Option<String>,
}

/// The contract with the client (`client.rs`): `[--session <id>] [--lang <code>]`
/// (or `--key=value`); anything else, a repeat and an invalid value are refused.
pub fn parse_args(args: &[String]) -> Option<Args> {
    let mut out = Args { session: None, lang: None };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let (key, value) = match a.split_once('=') {
            Some((k, v)) => (k, v.to_string()),
            None => (a.as_str(), it.next()?.clone()),
        };
        let slot = match key {
            "--session" if revoke::valid_session_id(&value) => &mut out.session,
            "--lang" if crate::system_l10n::is_valid_code(&value) => &mut out.lang,
            _ => return None,
        };
        if slot.replace(value).is_some() {
            return None;
        }
    }
    Some(out)
}

/// The user's graphical session by logind (`show-user … -p Display`): a
/// fallback when the window did not pass a number. It is then checked the
/// same way as a passed one ([`verify_session`]).
pub fn session_of_user(io: &dyn RootIo, uid: u32) -> Option<String> {
    let argv: Vec<String> = [revoke::LOGINCTL, "show-user", &uid.to_string(), "-p", "Display", "--value"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let got = io.capture(&argv);
    let id = got.stdout.trim().to_string();
    (got.code == Some(0) && revoke::valid_session_id(&id)).then_some(id)
}

/// Wipe the process environment and set our own ([`child_env`]). Called at the
/// very beginning, while there is only one thread.
fn sanitize_process_env() {
    let names: Vec<std::ffi::OsString> = std::env::vars_os().map(|(k, _)| k).collect();
    for k in names {
        std::env::remove_var(k);
    }
    for (k, v) in child_env() {
        std::env::set_var(k, v);
    }
}

/// Move the window socket from fd 0 to our own fd with `CLOEXEC`, and put
/// `/dev/null` on 0 and 1: no program started by the helper gets the channel.
fn take_socket_from_stdio() -> Option<OwnedFd> {
    let mut ty: libc::c_int = 0;
    let mut len = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
    // SAFETY: getsockopt/fcntl/open/dup2 on our own fds; the results are checked.
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
        let null = CString::new("/dev/null").ok()?;
        let nfd = libc::open(null.as_ptr(), libc::O_RDWR | libc::O_CLOEXEC);
        if nfd < 0 || libc::dup2(nfd, 0) < 0 || libc::dup2(nfd, 1) < 0 {
            return None;
        }
        if nfd > 2 {
            libc::close(nfd);
        }
        Some(sock)
    }
}

/// The whole live helper. For the exit code see `EXIT_*`.
pub fn main_live() -> i32 {
    // SAFETY: prctl/signal/getppid/geteuid/umask/chdir are system calls without
    // shared Rust state; there is only one thread so far.
    let parent = unsafe {
        // pkexec setuid resets the PDEATHSIG set by the window, so we set our own.
        libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM as libc::c_ulong, 0, 0, 0);
        libc::signal(libc::SIGTERM, on_sigterm as extern "C" fn(libc::c_int) as libc::sighandler_t);
        libc::signal(libc::SIGHUP, on_sigterm as extern "C" fn(libc::c_int) as libc::sighandler_t);
        libc::getppid()
    };
    if parent <= 1 {
        return EXIT_CHANNEL;
    }
    let raw_args: Vec<String> = std::env::args().skip(1).collect();
    let Some(args) = parse_args(&raw_args) else {
        journal("неверные аргументы");
        return EXIT_USAGE;
    };
    let pkexec_uid = std::env::var("PKEXEC_UID").ok().and_then(|s| validate::parse_pkexec_uid(&s));
    // SAFETY: see above.
    let euid = unsafe { libc::geteuid() };
    let Some(pkexec_uid) = pkexec_uid.filter(|_| euid == 0) else {
        journal("запущен не через pkexec");
        return EXIT_NOT_PKEXEC;
    };
    sanitize_process_env();
    // SAFETY: see above.
    unsafe {
        libc::umask(0o022);
        libc::chdir(c"/".as_ptr());
    }
    let Some(sock) = take_socket_from_stdio() else {
        journal("на fd 0 нет сокета окна");
        return EXIT_CHANNEL;
    };
    let io: Arc<dyn RootIo> = Arc::new(LiveRootIo);
    let Some(session) = args.session.or_else(|| session_of_user(io.as_ref(), pkexec_uid)) else {
        io.log("сеанс окна не найден у logind");
        return EXIT_HANDSHAKE;
    };
    let Some(watch) = LogindWatch::new(RootLoginctl(Arc::clone(&io)), BootMinusMonotonic, &session) else {
        return EXIT_USAGE;
    };
    let cfg = SessionConfig {
        build_id: proto::build_id(),
        pkexec_uid,
        parent_pid: parent,
        session_id: session,
        lang: args.lang,
        poll_interval: revoke::POLL_INTERVAL,
        idle_limit: revoke::IDLE_LIMIT,
    };
    io.log(&format!("запуск: uid {pkexec_uid}, родитель {parent}, сборка {}", cfg.build_id));
    let report = run_session(sock, io, cfg, Box::new(watch), Arc::new(MonotonicClock));
    match report.end {
        SessionEnd::Revoked(_) => EXIT_OK,
        SessionEnd::PeerRejected => EXIT_CHANNEL,
        SessionEnd::HandshakeFailed(_) | SessionEnd::SessionRejected => EXIT_HANDSHAKE,
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::privilege::op::PkgSource;
    use std::collections::{BTreeMap, VecDeque};
    use std::sync::atomic::{AtomicU32, AtomicU64};
    use std::time::Instant;

    // ── fake system ─────────────────────────────────────────────────────

    #[derive(Default)]
    struct FakeState {
        files: BTreeMap<String, String>,
        dirs: BTreeSet<String>,
        links: BTreeMap<String, String>,
        exe: BTreeSet<String>,
        /// Commands that were run.
        runs: Vec<Vec<String>>,
        /// File changes: "write <path>", "remove <path>", …
        mutations: Vec<String>,
        log: Vec<String>,
        /// Output of each command.
        output: Vec<String>,
        /// `systemctl enable --force <new>` fails.
        enable_fails: bool,
        loginctl: String,
        /// The answer of `bsdtar -xOf <copy> .PKGINFO`.
        pkginfo: String,
        /// A specific `bsdtar` answer by the copy's file name (otherwise `pkginfo`).
        pkginfos: BTreeMap<String, String>,
        /// Root files for [`RootIo::open_root_file`]: path → contents.
        root_files: BTreeMap<String, Vec<u8>>,
        /// These paths are rejected by `open_root_file` (foreign owner).
        root_refuse: BTreeSet<String>,
        /// Probe answers: the first argv line prefix (space-joined) the line
        /// starts with → (code, stdout).
        answers: Vec<(String, i32, String)>,
        /// Commands starting with this end with code 1.
        fail_runs: Vec<String>,
        /// Writing these files fails.
        fail_writes: BTreeSet<String>,
        /// The input stream of each command (in the order of `runs`).
        stdins: Vec<Option<String>>,
    }

    #[derive(Default)]
    struct FakeIo {
        st: Mutex<FakeState>,
        /// `which("pacman")` waits for a signal (Cancel before touching).
        gate_which: Mutex<Option<Receiver<()>>>,
        /// `run` reports the start and waits for a signal (Cancel after, EOF).
        gate_run: Mutex<Option<(Sender<()>, Receiver<()>)>>,
        runs_finished: AtomicU32,
    }

    impl FakeIo {
        fn arch() -> Arc<Self> {
            let io = FakeIo::default();
            {
                let mut s = io.st.lock().unwrap();
                s.files.insert("/etc/os-release".into(), "ID=arch\nNAME=\"Arch Linux\"\n".into());
                for p in ["pacman", "systemctl", "loginctl"] {
                    s.exe.insert(format!("/usr/bin/{p}"));
                }
                s.loginctl = format!("User={}\nActive=yes\nDesktop=KDE\n", unsafe { libc::getuid() });
            }
            Arc::new(io)
        }
        fn st(&self) -> std::sync::MutexGuard<'_, FakeState> {
            self.st.lock().unwrap()
        }
        fn touched_nothing(&self) -> bool {
            let s = self.st();
            s.runs.is_empty() && s.mutations.is_empty()
        }
        fn resolve(&self, p: &str) -> String {
            let s = self.st();
            let mut cur = p.to_string();
            for _ in 0..40 {
                match s.links.get(&cur) {
                    Some(t) => cur = t.clone(),
                    None => break,
                }
            }
            cur
        }
        fn mutate(&self, what: String) {
            self.st().mutations.push(what);
        }
    }

    impl RootIo for FakeIo {
        fn run(&self, argv: &[String], extra: &RunExtra, out: &mut dyn FnMut(&str)) -> Result<Option<i32>, String> {
            self.st().stdins.push(extra.stdin.clone());
            let gate = self.gate_run.lock().unwrap().take();
            if let Some((started, go)) = gate {
                let _ = started.send(());
                let _ = go.recv_timeout(WAIT);
            }
            let lines = {
                let mut s = self.st();
                s.runs.push(argv.to_vec());
                s.output.clone()
            };
            for l in &lines {
                out(l);
            }
            let mut code = 0;
            let line = argv.join(" ");
            if self.st().fail_runs.iter().any(|f| line.starts_with(f.as_str())) {
                code = 1;
            }
            // systemctl enable --force X.service: the display-manager link → X.
            if argv.len() == 4 && argv[..3] == strings(&["systemctl", "enable", "--force"]) {
                let mut s = self.st();
                if s.enable_fails && argv[3] != "lightdm.service" {
                    code = 1;
                } else {
                    let target = format!("/usr/lib/systemd/system/{}", argv[3]);
                    s.links.insert(plan::DM_SYMLINK.into(), target);
                }
            }
            self.runs_finished.fetch_add(1, Ordering::SeqCst);
            Ok(Some(code))
        }
        fn capture(&self, argv: &[String]) -> Captured {
            if argv.first().map(String::as_str) == Some(revoke::LOGINCTL) {
                return Captured { code: Some(0), stdout: self.st().loginctl.clone() };
            }
            let line = argv.join(" ");
            if let Some((_, code, out)) = self.st().answers.iter().find(|(pre, _, _)| line.starts_with(pre.as_str())) {
                return Captured { code: Some(*code), stdout: out.clone() };
            }
            // The owner of the path is no package.
            if matches!(argv.get(1).map(String::as_str), Some("-Qoq" | "-S" | "-qf")) {
                return Captured { code: Some(1), stdout: String::new() };
            }
            if argv.first().map(String::as_str) == Some("bsdtar") {
                // The root copy already lies in the working directory: check that it is
                // the copy that gets checked, not the user's path.
                assert!(std::path::Path::new(&argv[2]).is_file(), "bsdtar не по копии: {argv:?}");
                let s = self.st();
                let own = s.pkginfos.get(basename(&argv[2])).cloned();
                return Captured { code: Some(0), stdout: own.unwrap_or_else(|| s.pkginfo.clone()) };
            }
            if argv.len() == 3 && argv[1] == "is-enabled" {
                let want = format!("/usr/lib/systemd/system/{}", argv[2]);
                let on = self.resolve(plan::DM_SYMLINK) == want;
                return Captured { code: Some(0), stdout: if on { "enabled\n" } else { "disabled\n" }.into() };
            }
            Captured { code: Some(0), stdout: String::new() }
        }
        fn which(&self, name: &str) -> Option<String> {
            if name == "pacman" {
                let gate = self.gate_which.lock().unwrap().take();
                if let Some(go) = gate {
                    let _ = go.recv_timeout(WAIT);
                }
            }
            let path = if name.starts_with('/') { name.to_string() } else { format!("/usr/bin/{name}") };
            self.st().exe.contains(&path).then_some(path)
        }
        fn read_text(&self, path: &str) -> Option<String> {
            let real = self.resolve(path);
            self.st().files.get(&real).cloned()
        }
        fn exists(&self, path: &str) -> bool {
            let real = self.resolve(path);
            let s = self.st();
            s.files.contains_key(&real) || s.dirs.contains(&real) || s.exe.contains(&real)
        }
        fn is_file(&self, path: &str) -> bool {
            let real = self.resolve(path);
            self.st().files.contains_key(&real)
        }
        fn is_dir(&self, path: &str) -> bool {
            let real = self.resolve(path);
            self.st().dirs.contains(&real)
        }
        fn is_symlink(&self, path: &str) -> bool {
            self.st().links.contains_key(path)
        }
        fn canonicalize(&self, path: &str) -> Option<String> {
            let real = self.resolve(path);
            (self.exists(&real) || real == "/dev/null").then_some(real)
        }
        fn list_dir(&self, path: &str) -> Vec<String> {
            let s = self.st();
            let prefix = format!("{path}/");
            s.files
                .keys()
                .chain(s.dirs.iter())
                .filter_map(|p| p.strip_prefix(&prefix))
                .filter(|rest| !rest.contains('/'))
                .map(str::to_string)
                .collect()
        }
        fn write_file(&self, path: &str, content: &str, _mode: u32) -> Result<(), String> {
            if self.st().fail_writes.contains(path) {
                return Err(format!("{path}: нет места"));
            }
            self.mutate(format!("write {path}"));
            self.st().files.insert(path.into(), content.into());
            Ok(())
        }
        fn mode_of(&self, path: &str) -> Option<u32> {
            // The fake does not ask the real system.
            self.st().files.contains_key(path).then_some(0o644)
        }
        fn copy_file(&self, src: &str, dst: &str) -> Result<(), String> {
            self.mutate(format!("copy {src} {dst}"));
            // The source is either a faked file or a root copy in its own directory.
            let text = self
                .read_text(src)
                .or_else(|| std::fs::read(src).ok().map(|b| String::from_utf8_lossy(&b).into_owned()))
                .ok_or("нет файла")?;
            self.st().files.insert(dst.into(), text);
            Ok(())
        }
        fn rename(&self, from: &str, to: &str) -> Result<(), String> {
            self.mutate(format!("rename {from} {to}"));
            let mut s = self.st();
            let text = s.files.remove(from).ok_or("нет файла")?;
            s.files.insert(to.into(), text);
            Ok(())
        }
        fn remove_file(&self, path: &str) -> Result<(), String> {
            self.mutate(format!("remove {path}"));
            self.st().files.remove(path);
            Ok(())
        }
        fn remove_dir(&self, path: &str) -> Result<(), String> {
            self.mutate(format!("rmdir {path}"));
            self.st().dirs.remove(path);
            Ok(())
        }
        fn create_dir_all(&self, path: &str, _mode: u32) -> Result<(), String> {
            self.mutate(format!("mkdir {path}"));
            self.st().dirs.insert(path.into());
            Ok(())
        }
        fn copy_tree(&self, src: &str, dst: &str) -> Result<(), String> {
            assert!(std::path::Path::new(src).is_dir(), "copy_tree не из своего каталога");
            self.mutate(format!("copy_tree {dst}"));
            Ok(())
        }
        fn remove_tree(&self, path: &str) -> Result<(), String> {
            self.mutate(format!("remove_tree {path}"));
            Ok(())
        }
        fn work_base(&self) -> std::path::PathBuf {
            std::env::temp_dir()
        }
        fn home_of(&self, _uid: u32) -> Option<String> {
            Some("/home/test".into())
        }
        fn log(&self, line: &str) {
            self.st().log.push(line.to_string());
        }
        fn open_root_file(&self, dir: &str, name: &str) -> Result<OwnedFd, UnpackError> {
            static N: AtomicU32 = AtomicU32::new(0);
            let path = format!("{dir}/{name}");
            if self.st().root_refuse.contains(&path) {
                return Err(UnpackError::NotRootOwned(path));
            }
            let bytes = self.st().root_files.get(&path).cloned();
            let bytes = bytes.ok_or_else(|| UnpackError::Malformed(format!("нет {path}")))?;
            Ok(user_file(&format!("root-{}-{name}", N.fetch_add(1, Ordering::SeqCst)), &bytes))
        }
    }

    // ── fake clock and observer ──────────────────────────────────────────

    #[derive(Default)]
    struct FakeClock(AtomicU64);
    impl FakeClock {
        fn advance(&self, d: Duration) {
            self.0.fetch_add(d.as_millis() as u64, Ordering::SeqCst);
        }
    }
    impl Clock for FakeClock {
        fn now(&self) -> Duration {
            Duration::from_millis(self.0.load(Ordering::SeqCst))
        }
    }

    #[derive(Clone, Default)]
    struct FakeWatch(Arc<Mutex<VecDeque<Event>>>);
    impl SessionWatch for FakeWatch {
        fn poll(&mut self) -> Vec<Event> {
            self.0.lock().unwrap().drain(..).collect()
        }
    }

    // ── bench: the helper in a thread, the window is the test ────────────

    const BUILD: &str = "0.0.0+test";

    /// The limit of any wait in tests: a hang is a failure, not an eternal
    /// `futex_wait` holding up the build queue.
    const WAIT: Duration = Duration::from_secs(10);

    /// Wait for a signal from a channel no longer than [`WAIT`].
    fn wait<T>(rx: &Receiver<T>, what: &str) -> T {
        rx.recv_timeout(WAIT).unwrap_or_else(|_| panic!("не дождались: {what}"))
    }

    struct Bench {
        client: OwnedFd,
        /// The session result via a channel, so it can be awaited with a timeout.
        done: Receiver<SessionReport>,
        clock: Arc<FakeClock>,
        watch: FakeWatch,
    }

    fn bench_with(io: Arc<FakeIo>, pkexec_uid: u32) -> Bench {
        let (client, helper_end) = proto::seqpacket_pair().unwrap();
        let tv = libc::timeval { tv_sec: WAIT.as_secs() as libc::time_t, tv_usec: 0 };
        // SAFETY: a read timeout, so the test fails instead of hanging.
        unsafe {
            libc::setsockopt(
                client.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_RCVTIMEO,
                (&tv as *const libc::timeval).cast(),
                std::mem::size_of::<libc::timeval>() as libc::socklen_t,
            );
        }
        let clock = Arc::new(FakeClock::default());
        let watch = FakeWatch::default();
        let cfg = SessionConfig {
            build_id: BUILD.into(),
            pkexec_uid,
            parent_pid: std::process::id() as i32,
            session_id: "2".into(),
            lang: None,
            poll_interval: Duration::from_millis(5),
            idle_limit: Duration::from_secs(900),
        };
        let (c, w) = (Arc::clone(&clock), watch.clone());
        let (done_tx, done) = mpsc::channel();
        thread::spawn(move || {
            let _ = done_tx.send(run_session(helper_end, io, cfg, Box::new(w), c));
        });
        Bench { client, done, clock, watch }
    }

    fn bench(io: Arc<FakeIo>) -> Bench {
        bench_with(io, unsafe { libc::getuid() })
    }

    impl Bench {
        fn send(&self, msg: ClientMsg<PrivOp>) {
            proto::send_msg(self.client.as_fd(), &msg).unwrap();
        }
        fn recv(&self) -> Option<HelperMsg> {
            proto::recv_msg(self.client.as_fd()).expect("ответ помощника")
        }
        /// The next non-Progress message.
        fn recv_final(&self) -> Option<HelperMsg> {
            loop {
                match self.recv() {
                    Some(HelperMsg::Progress { .. }) => continue,
                    other => return other,
                }
            }
        }
        fn hello(&self) {
            self.send(ClientMsg::Hello(Hello { build_id: BUILD.into() }));
            assert_eq!(self.recv(), Some(HelperMsg::Welcome { build_id: BUILD.into() }));
        }
        fn finish(self) -> SessionReport {
            drop(self.client);
            wait(&self.done, "выход помощника")
        }
    }

    fn install(id: u64, pkgs: &[&str]) -> ClientMsg<PrivOp> {
        ClientMsg::Request {
            id,
            op: PrivOp::PkgInstall { family: PkgFamily::Arch, packages: strings(pkgs), source: PkgSource::DisplayManager },
        }
    }

    fn refused(id: u64, key: &str) -> Option<HelperMsg> {
        Some(HelperMsg::Error { id: Some(id), code: ErrorCode::Refused, message_key: key.into() })
    }

    // ── handshake and peer ───────────────────────────────────────────────

    #[test]
    fn рукопожатие_своя_сборка_принята() {
        let io = FakeIo::arch();
        let b = bench(io.clone());
        b.hello();
        let r = b.finish();
        assert_eq!(r.end, SessionEnd::Revoked(Reason::WindowClosed));
        assert!(io.touched_nothing());
    }

    #[test]
    fn рукопожатие_чужая_сборка_отказ_и_выход() {
        let b = bench(FakeIo::arch());
        b.send(ClientMsg::Hello(Hello { build_id: "9.9.9+other".into() }));
        assert_eq!(
            b.recv(),
            Some(HelperMsg::Error { id: None, code: ErrorCode::BuildMismatch, message_key: "build_mismatch".into() })
        );
        assert_eq!(b.recv(), None, "после отказа помощник закрывает сокет");
        assert_eq!(b.finish().end, SessionEnd::HandshakeFailed(ErrorCode::BuildMismatch));
    }

    #[test]
    fn запрос_до_hello_отказ() {
        let io = FakeIo::arch();
        let b = bench(io.clone());
        b.send(install(1, &["sddm"]));
        assert_eq!(
            b.recv(),
            Some(HelperMsg::Error { id: None, code: ErrorCode::Sequence, message_key: "hello_first".into() })
        );
        assert_eq!(b.finish().end, SessionEnd::HandshakeFailed(ErrorCode::Sequence));
        assert!(io.touched_nothing());
    }

    #[test]
    fn сеанс_logind_чужой_или_неактивный_отказ() {
        for answer in [format!("User={}\nActive=yes\n", unsafe { libc::getuid() } + 1), format!(
            "User={}\nActive=no\n",
            unsafe { libc::getuid() }
        )] {
            let io = FakeIo::arch();
            io.st().loginctl = answer;
            let b = bench(io);
            b.send(ClientMsg::Hello(Hello { build_id: BUILD.into() }));
            assert_eq!(
                b.recv(),
                Some(HelperMsg::Error { id: None, code: ErrorCode::Refused, message_key: "session_mismatch".into() })
            );
            assert_eq!(b.finish().end, SessionEnd::SessionRejected);
        }
    }

    #[test]
    fn чужой_uid_пира_молча_выход() {
        let io = FakeIo::arch();
        let b = bench_with(io.clone(), unsafe { libc::getuid() } + 1);
        // The helper closes the socket without reading a single frame.
        assert_eq!(b.recv(), None, "чужому — ни Welcome, ни ответа");
        assert_eq!(b.finish().end, SessionEnd::PeerRejected);
        assert!(io.touched_nothing());
    }

    // ── checks before execution ──────────────────────────────────────────

    #[test]
    fn отказ_проверки_не_исполняет_ничего() {
        let io = FakeIo::arch();
        let b = bench(io.clone());
        b.hello();
        let rm = |id, p: &[&str]| ClientMsg::Request {
            id,
            op: PrivOp::PkgRemove { family: PkgFamily::Arch, packages: strings(p) },
        };
        b.send(rm(1, &["linux"]));
        assert_eq!(b.recv(), refused(1, "refused.vital"));
        b.send(rm(2, &["polkit"]));
        assert_eq!(b.recv(), refused(2, "refused.vital"));
        b.send(install(3, &["/tmp/x.deb"]));
        assert_eq!(b.recv(), refused(3, "refused.package-name"));
        b.send(install(4, &["-oX"]));
        assert_eq!(b.recv(), refused(4, "refused.operand"));
        b.send(install(5, &["some-random-tool"]));
        assert_eq!(b.recv(), refused(5, "refused.not-in-table"));
        // The family from the operation did not match the machine.
        b.send(ClientMsg::Request {
            id: 6,
            op: PrivOp::PkgInstall { family: PkgFamily::Debian, packages: strings(&["sddm"]), source: PkgSource::DisplayManager },
        });
        assert_eq!(b.recv(), refused(6, "refused.family"));
        b.send(ClientMsg::Request {
            id: 7,
            op: PrivOp::EnableDm { family: PkgFamily::Arch, unit: "evil".into() },
        });
        assert_eq!(b.recv(), refused(7, "refused.display-manager"));
        b.send(ClientMsg::Request {
            id: 8,
            op: PrivOp::SwitchDm { family: PkgFamily::Arch, unit: "sddm".into(), greeter: Some("x".into()) },
        });
        assert_eq!(b.recv(), refused(8, "refused.display-manager"));
        b.send(ClientMsg::Request { id: 9, op: PrivOp::BuildDeps { names: strings(&["cmake>=3;id"]) } });
        assert!(matches!(b.recv(), Some(HelperMsg::Error { id: Some(9), code: ErrorCode::Refused, .. })));
        // AUR is not for the helper; the rest is not executed yet; files wait for unpack.
        b.send(ClientMsg::Request {
            id: 10,
            op: PrivOp::AurInstall { helper: "paru".into(), packages: strings(&["x"]), replace_conflicts: false },
        });
        assert_eq!(b.recv(), refused(10, "not_for_helper"));
        b.send(ClientMsg::Request { id: 11, op: PrivOp::MateGschemaCleanup });
        assert_eq!(b.recv(), refused(11, "not_supported_yet"));
        b.send(ClientMsg::Request {
            id: 12,
            op: PrivOp::InstallBuilt {
                package: crate::privilege::op::LocalFile { slot: 0, label: "/x".into() },
                build_only: Vec::new(),
                replace: false,
            },
        });
        // An operation file without a descriptor: refusal.
        assert_eq!(b.recv(), refused(12, "fds_mismatch"));
        let r = b.finish();
        assert_eq!(r.ops_executed, 0);
        assert!(io.touched_nothing(), "после отказа система не тронута: {:?}", io.st().runs);
    }

    #[test]
    fn удаление_при_неизвестном_столе_отказ() {
        let io = FakeIo::arch();
        io.st().loginctl = format!("User={}\nActive=yes\nDesktop=\n", unsafe { libc::getuid() });
        let b = bench(io.clone());
        b.hello();
        b.send(ClientMsg::Request {
            id: 1,
            op: PrivOp::PkgRemove { family: PkgFamily::Arch, packages: strings(&["xfce4-panel"]) },
        });
        assert_eq!(b.recv(), refused(1, "refused.current-desktop-unknown"));
        b.finish();
        assert!(io.touched_nothing());
    }

    #[test]
    fn steamos_пакеты_не_трогаем() {
        let facts = Facts { steamos: true, ..facts() };
        let op = PrivOp::PkgInstall { family: PkgFamily::Arch, packages: strings(&["sddm"]), source: PkgSource::DisplayManager };
        assert_eq!(check_op(&op, &facts, 0).unwrap_err().key, "refused.read-only-system");
    }

    #[test]
    fn установка_только_из_названной_таблицы() {
        let facts = facts();
        let op = |pkgs: &[&str], source: PkgSource| PrivOp::PkgInstall {
            family: PkgFamily::Arch,
            packages: strings(pkgs),
            source,
        };
        let mate = system::get_packages("mate", PkgFamily::Arch);
        let xfce = system::get_packages("xfce", PkgFamily::Arch);
        let mate_only: Vec<&str> =
            mate.iter().filter(|p| !xfce.contains(p)).map(String::as_str).take(1).collect();
        assert!(!mate_only.is_empty());
        // Its own table: yes.
        assert!(check_op(&op(&mate_only, PkgSource::Desktop { de_id: "mate".into() }), &facts, 0).is_ok());
        assert!(check_op(&op(&["sddm"], PkgSource::DisplayManager), &facts, 0).is_ok());
        assert!(check_op(&op(&["yt-dlp"], PkgSource::YtDlp), &facts, 0).is_ok());
        // A name from another core table: no, even though it is "ours".
        for (pkgs, src) in [
            (mate_only.clone(), PkgSource::Desktop { de_id: "xfce".into() }),
            (vec!["sddm"], PkgSource::YtDlp),
            (vec!["yt-dlp"], PkgSource::DisplayManager),
            // Repair only of the current desktop (for the helper it is "kde").
            (mate_only.clone(), PkgSource::DesktopRepair),
        ] {
            let err = check_op(&op(&pkgs, src.clone()), &facts, 0).unwrap_err();
            assert_eq!(err.key, "refused.not-in-table", "{pkgs:?} {src:?}");
        }
        // Another desktop: a table refusal, not an install.
        let err = check_op(&op(&["a"], PkgSource::Desktop { de_id: "evil".into() }), &facts, 0).unwrap_err();
        assert_ne!(err.key, "not_supported_yet");
        assert!(err.key.starts_with("refused."), "{}", err.key);
    }

    #[test]
    fn лишний_дескриптор_отказ() {
        let facts = facts();
        let op = PrivOp::PkgInstall { family: PkgFamily::Arch, packages: strings(&["sddm"]), source: PkgSource::DisplayManager };
        assert_eq!(check_op(&op, &facts, 1).unwrap_err().key, "fds_mismatch");
        assert!(check_op(&op, &facts, 0).is_ok());
    }

    // ── execution ────────────────────────────────────────────────────────

    #[test]
    fn установка_из_таблицы_исполняется_без_оболочки() {
        let io = FakeIo::arch();
        io.st().output = vec!["resolving dependencies...".into()];
        let b = bench(io.clone());
        b.hello();
        b.send(install(1, &["sddm"]));
        assert_eq!(b.recv(), Some(HelperMsg::Progress { id: 1, line: "resolving dependencies...".into() }));
        assert_eq!(b.recv(), Some(HelperMsg::Done { id: 1, ok: true, code: Some(0), message_key: None }));
        assert_eq!(b.finish().ops_executed, 1);
        assert_eq!(io.st().runs, vec![strings(&["pacman", "-S", "--needed", "--noconfirm", "sddm"])]);
    }

    #[test]
    fn сборочные_зависимости_записываются() {
        let io = FakeIo::arch();
        let rebuilt = Mutex::new(Vec::new());
        let state = Mutex::new(OpState::default());
        let ctx = Ctx { io: io.as_ref(), progress: &|_: &str| {}, state: &state, lang: None, rebuilt: &rebuilt };
        let out = execute(&Plan::BuildDeps { deps: strings(&["cmake>=3.20"]) }, &ctx, Vec::new());
        assert_eq!(out, Outcome::Done { ok: true, code: Some(0), key: None });
        assert_eq!(
            io.st().runs,
            vec![strings(&["pacman", "-S", "--needed", "--noconfirm", "--asdeps", "--", "cmake>=3.20"])]
        );
    }

    #[test]
    fn вторая_операция_busy() {
        let io = FakeIo::arch();
        let (started_tx, started_rx) = mpsc::channel();
        let (go_tx, go_rx) = mpsc::channel();
        *io.gate_run.lock().unwrap() = Some((started_tx, go_rx));
        let b = bench(io.clone());
        b.hello();
        b.send(install(1, &["sddm"]));
        wait(&started_rx, "начало pacman");
        b.send(install(2, &["lightdm"]));
        assert_eq!(b.recv(), Some(HelperMsg::Busy { id: 2 }));
        go_tx.send(()).unwrap();
        assert!(matches!(b.recv_final(), Some(HelperMsg::Done { id: 1, ok: true, .. })));
        // After Busy the same id may be sent again.
        b.send(install(2, &["lightdm"]));
        assert!(matches!(b.recv_final(), Some(HelperMsg::Done { id: 2, ok: true, .. })));
        assert_eq!(b.finish().ops_executed, 2);
    }

    #[test]
    fn повтор_id_и_чужая_отмена_sequence() {
        let io = FakeIo::arch();
        let b = bench(io.clone());
        b.hello();
        b.send(install(5, &["sddm"]));
        assert!(matches!(b.recv_final(), Some(HelperMsg::Done { id: 5, .. })));
        b.send(install(5, &["sddm"]));
        assert_eq!(
            b.recv(),
            Some(HelperMsg::Error { id: Some(5), code: ErrorCode::Sequence, message_key: "id_reused".into() })
        );
        b.send(install(3, &["sddm"]));
        assert!(matches!(b.recv(), Some(HelperMsg::Error { id: Some(3), code: ErrorCode::Sequence, .. })));
        b.send(ClientMsg::Cancel { id: 42 });
        assert_eq!(
            b.recv(),
            Some(HelperMsg::Error { id: Some(42), code: ErrorCode::Sequence, message_key: "cancel_unknown".into() })
        );
        b.send(ClientMsg::Hello(Hello { build_id: BUILD.into() }));
        assert!(matches!(b.recv(), Some(HelperMsg::Error { id: None, code: ErrorCode::Sequence, .. })));
        // A cancel that crossed paths with the end of the operation: silently.
        b.send(ClientMsg::Cancel { id: 5 });
        b.send(install(6, &["sddm"]));
        assert!(matches!(b.recv_final(), Some(HelperMsg::Done { id: 6, .. })));
        assert_eq!(b.finish().ops_executed, 2);
        assert_eq!(io.st().runs.len(), 2);
    }

    #[test]
    fn отмена_до_касания_cancelled_и_ничего_не_исполнено() {
        let io = FakeIo::arch();
        let (go_tx, go_rx) = mpsc::channel();
        *io.gate_which.lock().unwrap() = Some(go_rx);
        let b = bench(io.clone());
        b.hello();
        b.send(install(1, &["sddm"]));
        b.send(ClientMsg::Cancel { id: 1 });
        // Busy on the second request means the loop has already processed the cancel.
        b.send(install(2, &["sddm"]));
        assert_eq!(b.recv(), Some(HelperMsg::Busy { id: 2 }));
        go_tx.send(()).unwrap();
        assert_eq!(
            b.recv(),
            Some(HelperMsg::Error { id: Some(1), code: ErrorCode::Cancelled, message_key: "cancelled".into() })
        );
        b.finish();
        assert!(io.touched_nothing());
    }

    #[test]
    fn отмена_после_касания_доводит_и_done() {
        let io = FakeIo::arch();
        let (started_tx, started_rx) = mpsc::channel();
        let (go_tx, go_rx) = mpsc::channel();
        *io.gate_run.lock().unwrap() = Some((started_tx, go_rx));
        let b = bench(io.clone());
        b.hello();
        b.send(install(1, &["sddm"]));
        wait(&started_rx, "начало pacman");
        b.send(ClientMsg::Cancel { id: 1 });
        b.send(install(2, &["sddm"]));
        assert_eq!(b.recv(), Some(HelperMsg::Busy { id: 2 }));
        go_tx.send(()).unwrap();
        assert!(matches!(b.recv_final(), Some(HelperMsg::Done { id: 1, ok: true, .. })));
        b.finish();
        assert_eq!(io.st().runs.len(), 1);
    }

    #[test]
    fn обрыв_сокета_посреди_операции_не_обрывает_её() {
        let io = FakeIo::arch();
        let (started_tx, started_rx) = mpsc::channel();
        let (go_tx, go_rx) = mpsc::channel();
        *io.gate_run.lock().unwrap() = Some((started_tx, go_rx));
        let b = bench(io.clone());
        b.hello();
        b.send(install(1, &["sddm"]));
        wait(&started_rx, "начало pacman");
        let Bench { client, done, .. } = b;
        drop(client);
        // The window is gone; the helper is still waiting for pacman to finish and does not exit.
        assert!(
            done.recv_timeout(Duration::from_millis(100)).is_err(),
            "помощник вышел посреди операции"
        );
        go_tx.send(()).unwrap();
        let r = wait(&done, "выход после операции");
        assert_eq!(r.end, SessionEnd::Revoked(Reason::WindowClosed));
        assert_eq!(r.ops_executed, 1);
        assert_eq!(io.runs_finished.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn отзыв_по_простою() {
        let io = FakeIo::arch();
        let b = bench(io.clone());
        b.hello();
        b.clock.advance(Duration::from_secs(901));
        assert_eq!(
            b.recv(),
            Some(HelperMsg::Error { id: None, code: ErrorCode::Revoked, message_key: "revoked.idle".into() })
        );
        assert_eq!(b.recv(), None, "после отзыва помощник выходит");
        assert_eq!(b.finish().end, SessionEnd::Revoked(Reason::Idle));
    }

    #[test]
    fn отзыв_по_блокировке_новые_отвергаются_идущая_доводится() {
        let io = FakeIo::arch();
        let (started_tx, started_rx) = mpsc::channel();
        let (go_tx, go_rx) = mpsc::channel();
        *io.gate_run.lock().unwrap() = Some((started_tx, go_rx));
        let b = bench(io.clone());
        b.hello();
        b.send(install(1, &["sddm"]));
        wait(&started_rx, "начало pacman");
        b.watch.0.lock().unwrap().push_back(Event::ScreenLocked);
        assert_eq!(
            b.recv(),
            Some(HelperMsg::Error { id: None, code: ErrorCode::Revoked, message_key: "revoked.screen-locked".into() })
        );
        // Revocation does not interrupt a running operation.
        go_tx.send(()).unwrap();
        assert!(matches!(b.recv_final(), Some(HelperMsg::Done { id: 1, ok: true, .. })));
        assert_eq!(b.recv(), None);
        let r = b.finish();
        assert_eq!(r.end, SessionEnd::Revoked(Reason::ScreenLocked));
        assert_eq!(r.ops_executed, 1);
    }

    #[test]
    fn после_отзыва_новая_операция_revoked() {
        let mut rev = Revoker::with_idle_limit(Duration::ZERO, Duration::from_secs(1));
        rev.op_started(Duration::ZERO).unwrap();
        rev.on_event(Event::Slept, Duration::ZERO);
        assert_eq!(rev.op_started(Duration::ZERO), Err(Reason::Slept));
        assert!(!rev.may_exit(), "идущая операция держит помощника");
    }

    // ── Progress ─────────────────────────────────────────────────────────

    #[test]
    fn progress_режется_по_4кб() {
        let long = "ж".repeat(5000); // 10,000 bytes
        let parts = cut_progress(&long);
        assert!(parts.iter().all(|p| p.len() <= PROGRESS_MAX));
        assert_eq!(parts.concat(), long);

        let mut cutter = LineCutter::default();
        let mut got = Vec::new();
        let mut data = "a\r\nb\n".as_bytes().to_vec();
        data.extend("я".repeat(3000).as_bytes());
        data.push(b'\n');
        for chunk in data.chunks(7) {
            cutter.feed(chunk, &mut |l| got.push(l.to_string()));
        }
        cutter.finish(&mut |l| got.push(l.to_string()));
        assert_eq!(&got[..2], &["a".to_string(), "b".to_string()]);
        assert!(got[2..].iter().all(|l| l.len() <= PROGRESS_MAX && !l.contains('\u{fffd}')));
        assert_eq!(got[2..].concat(), "я".repeat(3000));
    }

    #[test]
    fn progress_по_сокету_не_длиннее_предела() {
        let io = FakeIo::arch();
        io.st().output = vec!["x".repeat(10_000)];
        let b = bench(io);
        b.hello();
        b.send(install(1, &["sddm"]));
        let mut total = 0;
        loop {
            match b.recv() {
                Some(HelperMsg::Progress { line, .. }) => {
                    assert!(line.len() <= PROGRESS_MAX);
                    total += line.len();
                }
                Some(HelperMsg::Done { .. }) => break,
                other => panic!("{other:?}"),
            }
        }
        assert_eq!(total, 10_000);
        b.finish();
    }

    // ── environment ──────────────────────────────────────────────────────

    #[test]
    fn окружение_потомка_очищено() {
        // The real `env` program run as the user: the test environment has HOME,
        // USER and the rest, while a child of the helper must not have them.
        let got = LiveRootIo.capture(&strings(&["env"]));
        assert_eq!(got.code, Some(0));
        let mut lines: Vec<&str> = got.stdout.lines().collect();
        lines.sort_unstable();
        assert_eq!(lines, vec!["LANG=C", "LC_ALL=C", "PATH=/usr/bin:/usr/sbin"]);
        let mut seen = Vec::new();
        let rc = LiveRootIo.run(&strings(&["env"]), &RunExtra::default(), &mut |l| seen.push(l.to_string()));
        assert_eq!(rc, Ok(Some(0)));
        assert!(seen.iter().all(|l| !l.starts_with("LD_") && !l.starts_with("HOME=")));
        assert_eq!(seen.len(), 3);
    }

    #[test]
    fn аргументы_помощника() {
        let some = |s: &str| Some(s.to_string());
        // As the client calls it: `--session <id>`.
        assert_eq!(parse_args(&strings(&["--session", "2"])), Some(Args { session: some("2"), lang: None }));
        assert_eq!(
            parse_args(&strings(&["--session=c1", "--lang", "ru"])),
            Some(Args { session: some("c1"), lang: some("ru") })
        );
        // Without the flag it is allowed: the helper finds the session at logind itself.
        assert_eq!(parse_args(&[]), Some(Args { session: None, lang: None }));
        for bad in [
            &["--session"][..],
            &["--session", "-p"],
            &["--session=2", "--x"],
            &["--lang=../x"],
            &["--session", "2", "--session", "3"],
            &["2"],
        ] {
            assert_eq!(parse_args(&strings(bad)), None, "{bad:?}");
        }
    }

    // ── switch-dm ────────────────────────────────────────────────────────

    fn dm_machine() -> Arc<FakeIo> {
        let io = FakeIo::arch();
        {
            let mut s = io.st();
            s.dirs.insert("/run/systemd/system".into());
            s.files.insert(
                "/usr/lib/systemd/system/lightdm.service".into(),
                "[Service]\nExecStart=/usr/bin/lightdm\n".into(),
            );
            s.files.insert("/usr/lib/systemd/system/sddm.service".into(), "[Service]\nExecStart=/usr/bin/sddm\n".into());
            s.links.insert(plan::DM_SYMLINK.into(), "/usr/lib/systemd/system/lightdm.service".into());
            s.exe.insert("/usr/bin/sddm".into());
            s.files.insert("/etc/pam.d/sddm".into(), String::new());
        }
        io
    }

    fn run_switch(io: &FakeIo) -> (Outcome, Vec<String>) {
        run_switch_to(io, "sddm")
    }

    fn run_switch_to(io: &FakeIo, unit: &str) -> (Outcome, Vec<String>) {
        let rebuilt = Mutex::new(Vec::new());
        let state = Mutex::new(OpState::default());
        let lines = Mutex::new(Vec::new());
        let progress = |l: &str| lines.lock().unwrap().push(l.to_string());
        let ctx = Ctx { io, progress: &progress, state: &state, lang: None, rebuilt: &rebuilt };
        let plan_ = Plan::SwitchDm { family: PkgFamily::Arch, unit: unit.into(), greeter: None };
        let out = execute(&plan_, &ctx, Vec::new());
        let lines = lines.into_inner().unwrap();
        (out, lines)
    }

    #[test]
    fn switch_dm_переключает_и_ставит_страховку() {
        let io = dm_machine();
        let (out, lines) = run_switch(&io);
        assert_eq!(out, Outcome::ok(Some("dm_switched")), "{lines:?}");
        let s = io.st();
        assert_eq!(s.runs, vec![strings(&["systemctl", "enable", "--force", "sddm.service"])]);
        let conf = &s.files[&format!("{}/dm-fallback.conf", plan::DM_FALLBACK_DIR)];
        assert!(conf.contains("old=lightdm\n") && conf.contains("new=sddm\n"));
        assert_eq!(s.files[plan::DM_FALLBACK_SCRIPT_PATH], plan::DM_FALLBACK_SCRIPT);
        let dropin = format!("/etc/systemd/system/sddm.service.d/{}", plan::DM_FALLBACK_DROPIN);
        assert!(s.files[&dropin].contains(plan::DM_FALLBACK_SERVICE));
        assert!(s.files[plan::PREVIOUS_DM_FILE].contains("sudo systemctl enable --force lightdm.service"));
    }

    #[test]
    fn switch_dm_сбой_enable_откатывает() {
        let io = dm_machine();
        io.st().enable_fails = true;
        let (out, _) = run_switch(&io);
        assert_eq!(out, Outcome::fail("dm_switch_failed"));
        let s = io.st();
        assert_eq!(
            s.runs,
            vec![
                strings(&["systemctl", "enable", "--force", "sddm.service"]),
                strings(&["systemctl", "enable", "--force", "lightdm.service"]),
            ]
        );
        assert!(!s.files.contains_key(plan::PREVIOUS_DM_FILE));
    }

    #[test]
    fn switch_dm_нет_pam_отказ_до_касания() {
        let io = dm_machine();
        io.st().files.remove("/etc/pam.d/sddm");
        let (out, _) = run_switch(&io);
        assert_eq!(out, Outcome::fail("dm_no_pam"));
        assert!(io.touched_nothing());
    }

    #[test]
    fn switch_dm_уже_основной() {
        let io = dm_machine();
        io.st().links.insert(plan::DM_SYMLINK.into(), "/usr/lib/systemd/system/sddm.service".into());
        let (out, _) = run_switch(&io);
        assert_eq!(out, Outcome::ok(Some("dm_already_default")));
        assert!(io.touched_nothing());
    }

    /// Plasma Login with autologin for alice → SDDM (as on Wehrwolfmann's machine).
    fn plm_machine() -> Arc<FakeIo> {
        let io = dm_machine();
        {
            let mut s = io.st();
            s.files.insert(
                "/usr/lib/systemd/system/plasmalogin.service".into(),
                "[Service]\nExecStart=/usr/bin/plasmalogin\n".into(),
            );
            s.links.insert(plan::DM_SYMLINK.into(), "/usr/lib/systemd/system/plasmalogin.service".into());
            s.files.insert("/etc/plasmalogin.conf".into(), "[Autologin]\nRelogin=true\nSession=plasma\nUser=alice\n".into());
            s.files.insert("/etc/sddm.conf".into(), "[Autologin]\nSession=plasmax11\n".into());
            s.answers.push(("getent passwd alice".into(), 0, "alice:x:1000:1000::/home/alice:/usr/bin/fish\n".into()));
        }
        io
    }

    #[test]
    fn switch_dm_переносит_автовход() {
        let io = plm_machine();
        let (out, lines) = run_switch(&io);
        assert_eq!(out, Outcome::ok(Some("dm_switched")), "{lines:?}");
        let s = io.st();
        assert_eq!(s.files["/etc/sddm.conf"], "[Autologin]\nSession=plasma.desktop\n");
        let own = &s.files["/etc/sddm.conf.d/zz-rexon-autologin.conf"];
        assert!(own.contains("User=alice") && own.contains("Relogin=true"), "{own}");
        assert_eq!(s.files["/etc/plasmalogin.conf"], "[Autologin]\nRelogin=true\nSession=plasma\nUser=alice\n", "прежний не тронут");
        assert!(lines.iter().any(|l| l == "automatic sign-in for alice moved to sddm"), "{lines:?}");
        // Written BEFORE enabling: the rollback restores it.
        let write = s.mutations.iter().position(|m| m == "write /etc/sddm.conf").unwrap();
        assert!(s.mutations[..write].iter().all(|m| !m.contains("dm-fallback")));
    }

    #[test]
    fn switch_dm_сбой_enable_возвращает_автовход() {
        let io = plm_machine();
        io.st().enable_fails = true;
        let (out, _) = run_switch(&io);
        assert_eq!(out, Outcome::fail("dm_switch_failed"));
        let s = io.st();
        assert_eq!(s.files["/etc/sddm.conf"], "[Autologin]\nSession=plasmax11\n", "чужая строка вернулась");
        assert!(!s.files.contains_key("/etc/sddm.conf.d/zz-rexon-autologin.conf"), "свой файл снят");
        assert!(s.runs.contains(&strings(&["systemctl", "enable", "--force", "plasmalogin.service"])));
    }

    #[test]
    fn switch_dm_группа_pam_автовхода_и_её_откат() {
        let io = plm_machine();
        io.st().files.insert(
            "/etc/pam.d/sddm-autologin".into(),
            "auth [success=1 default=ignore] pam_succeed_if.so user ingroup autologin\n".into(),
        );
        let (out, lines) = run_switch(&io);
        assert_eq!(out, Outcome::ok(Some("dm_switched")), "{lines:?}");
        {
            let s = io.st();
            assert!(s.runs.contains(&strings(&["groupadd", "-r", "autologin"])));
            assert!(s.runs.contains(&strings(&["gpasswd", "-a", "alice", "autologin"])));
        }
        // Enabling failed: the group is removed together with the files.
        let io = plm_machine();
        io.st().files.insert(
            "/etc/pam.d/sddm-autologin".into(),
            "auth [success=1 default=ignore] pam_succeed_if.so user ingroup autologin\n".into(),
        );
        io.st().fail_runs.push("systemctl enable --force sddm".into());
        let (out, _) = run_switch(&io);
        assert_eq!(out, Outcome::fail("dm_switch_failed"));
        let s = io.st();
        assert!(s.runs.contains(&strings(&["gpasswd", "-d", "alice", "autologin"])));
        assert!(s.runs.contains(&strings(&["groupdel", "autologin"])));
    }

    #[test]
    fn switch_dm_неизвестный_пользователь_автовхода_не_переносится() {
        let io = plm_machine();
        io.st().answers.clear();
        let (out, lines) = run_switch(&io);
        assert_eq!(out, Outcome::ok(Some("dm_switched")));
        assert!(!io.st().files.contains_key("/etc/sddm.conf.d/zz-rexon-autologin.conf"));
        assert!(lines.iter().any(|l| l.contains("alice") && l.contains("isn't on this system")), "{lines:?}");
    }

    #[test]
    fn switch_dm_переносит_маски_звука_приветствия() {
        let io = plm_machine();
        {
            let mut s = io.st();
            s.exe.insert("/usr/bin/setpriv".into());
            s.dirs.insert("/var/lib/plasmalogin".into());
            s.dirs.insert("/var/lib/sddm".into());
            s.links.insert("/var/lib/plasmalogin/.config/systemd/user/wireplumber.service".into(), "/dev/null".into());
            s.answers.push(("getent passwd plasmalogin".into(), 0, "plasmalogin:x:957:957::/var/lib/plasmalogin:/usr/bin/nologin\n".into()));
            s.answers.push(("getent passwd sddm".into(), 0, "sddm:x:964:964::/var/lib/sddm:/usr/bin/nologin\n".into()));
        }
        let (out, lines) = run_switch(&io);
        assert_eq!(out, Outcome::ok(Some("dm_switched")), "{lines:?}");
        let s = io.st();
        let dir = "/var/lib/sddm/.config/systemd/user";
        let as_sddm = |cmd: &[&str]| {
            let mut v = strings(&["setpriv", "--reuid=964", "--regid=964", "--clear-groups", "--"]);
            v.extend(strings(cmd));
            v
        };
        assert!(s.runs.contains(&as_sddm(&["mkdir", "-p", dir])), "{:?}", s.runs);
        for u in crate::dm_autologin::GREETER_AUDIO_UNITS {
            assert!(s.runs.contains(&as_sddm(&["ln", "-s", "/dev/null", &format!("{dir}/{u}")])), "{u}");
        }
        assert!(lines.iter().any(|l| l == "login screen sound turned off for sddm too, as it was for plasmalogin"));
        // The previous one has no masks: nothing.
        let io = plm_machine();
        io.st().exe.insert("/usr/bin/setpriv".into());
        let _ = run_switch(&io);
        assert!(!io.st().runs.iter().any(|r| r.first().map(String::as_str) == Some("setpriv")));
    }

    /// Plasma Login (autologin for alice, greeter sound muted) → DDM: autologin
    /// goes into its own `ddm.conf.d` file without `Relogin=`, the sound masks go
    /// to the `dde` user (home `/var/lib/ddm`, as in ddm's sysusers).
    #[test]
    fn switch_dm_в_ddm_переносит_автовход_и_маски_звука() {
        let io = plm_machine();
        {
            let mut s = io.st();
            s.files.insert(
                "/usr/lib/systemd/system/ddm.service".into(),
                "[Unit]\nRequires=dde-seatd.service\n[Service]\nExecStart=/usr/bin/ddm\n".into(),
            );
            s.files.insert("/usr/lib/systemd/system/dde-seatd.service".into(), "[Service]\nExecStart=/usr/bin/dde-seatd\n".into());
            s.exe.insert("/usr/bin/ddm".into());
            s.exe.insert("/usr/bin/setpriv".into());
            s.files.insert("/usr/lib/pam.d/ddm".into(), String::new());
            s.dirs.insert("/var/lib/plasmalogin".into());
            s.dirs.insert("/var/lib/ddm".into());
            s.links.insert("/var/lib/plasmalogin/.config/systemd/user/wireplumber.service".into(), "/dev/null".into());
            s.answers.push(("getent passwd plasmalogin".into(), 0, "plasmalogin:x:957:957::/var/lib/plasmalogin:/usr/bin/nologin\n".into()));
            s.answers.push(("getent passwd dde".into(), 0, "dde:x:955:955:DDM Greeter Account:/var/lib/ddm:/usr/bin/nologin\n".into()));
        }
        let (out, lines) = run_switch_to(&io, "ddm");
        assert_eq!(out, Outcome::ok(Some("dm_switched")), "{lines:?}");
        let s = io.st();
        assert!(s.runs.contains(&strings(&["systemctl", "enable", "--force", "ddm.service"])), "{:?}", s.runs);
        let own = &s.files["/etc/ddm.conf.d/zz-rexon-autologin.conf"];
        assert!(own.contains("User=alice") && own.contains("Session=plasma.desktop"), "{own}");
        assert!(!own.contains("Relogin"), "у DDM нет Relogin: {own}");
        assert!(lines.iter().any(|l| l == "automatic sign-in for alice moved to ddm"), "{lines:?}");
        let dir = "/var/lib/ddm/.config/systemd/user";
        let as_dde = |cmd: &[&str]| {
            let mut v = strings(&["setpriv", "--reuid=955", "--regid=955", "--clear-groups", "--"]);
            v.extend(strings(cmd));
            v
        };
        for u in crate::dm_autologin::GREETER_AUDIO_UNITS {
            assert!(s.runs.contains(&as_dde(&["ln", "-s", "/dev/null", &format!("{dir}/{u}")])), "{u}: {:?}", s.runs);
        }
        assert!(lines.iter().any(|l| l == "login screen sound turned off for ddm too, as it was for plasmalogin"), "{lines:?}");
    }

    /// Without `dde-seatd` DDM does not start: refusal before any writing,
    /// autologin untouched.
    #[test]
    fn switch_dm_в_ddm_без_dde_seatd_отказ_до_касания() {
        let io = plm_machine();
        {
            let mut s = io.st();
            s.files.insert(
                "/usr/lib/systemd/system/ddm.service".into(),
                "[Unit]\nRequires=dde-seatd.service\n[Service]\nExecStart=/usr/bin/ddm\n".into(),
            );
            s.exe.insert("/usr/bin/ddm".into());
            s.files.insert("/usr/lib/pam.d/ddm".into(), String::new());
        }
        let (out, _) = run_switch_to(&io, "ddm");
        assert_eq!(out, Outcome::fail("dm_needs_unit"));
        assert!(io.touched_nothing());
    }

    #[test]
    fn greetd_текстовый_вход_распознан() {
        assert_eq!(greetd_config("/usr/bin/greetd --config /etc/g.toml"), "/etc/g.toml");
        assert_eq!(greetd_config("/usr/bin/greetd"), "/etc/greetd/config.toml");
        let cfg = "[terminal]\nvt = 1\n[default_session]\ncommand = \"env A=b tuigreet --cmd sway\"\nuser = \"greeter\"\n";
        assert_eq!(greetd_greeter(cfg), "tuigreet");
        assert_eq!(greetd_greeter("[default_session]\ncommand = \"agreety --cmd /bin/sh\"\n"), "agreety");
        assert_eq!(exec_start("[Service]\nExecStart=\nExecStart=-/usr/bin/x --y\n"), "-/usr/bin/x --y");
    }

    // ── files by descriptor ──────────────────────────────────────────────

    fn facts() -> Facts {
        Facts {
            family: PkgFamily::Arch,
            steamos: false,
            current_de: "kde".into(),
            session_type: "wayland".into(),
            rebuild_root: Some(validate::user_rebuild_root("/home/test")),
        }
    }

    const BUILT: &str = "/home/test/.cache/rexon/rebuild/deepin-session/deepin-session-treeland-1.2-3-x86_64.pkg.tar.zst";

    fn local(slot: u8, label: &str) -> crate::privilege::op::LocalFile {
        crate::privilege::op::LocalFile { slot, label: label.into() }
    }

    /// The user's file in their temporary directory, as an open descriptor.
    fn user_file(tag: &str, bytes: &[u8]) -> OwnedFd {
        let p = std::env::temp_dir().join(format!("rexon-b4-{}-{tag}", std::process::id()));
        std::fs::write(&p, bytes).unwrap();
        let f = std::fs::File::open(&p).unwrap();
        let _ = std::fs::remove_file(&p);
        OwnedFd::from(f)
    }

    fn run_plan(io: &FakeIo, plan_: &Plan, files: Vec<OwnedFd>, rebuilt: &Mutex<Vec<String>>) -> Outcome {
        let state = Mutex::new(OpState::default());
        let ctx = Ctx { io, progress: &|_: &str| {}, state: &state, lang: None, rebuilt };
        execute(plan_, &ctx, files)
    }

    #[test]
    fn дескрипторы_по_слотам() {
        let op = |slot| PrivOp::InstallBuilt { package: local(slot, BUILT), build_only: Vec::new(), replace: false };
        assert!(check_op(&op(0), &facts(), 1).is_ok());
        assert_eq!(check_op(&op(1), &facts(), 1).unwrap_err().key, "fds_mismatch");
        assert_eq!(check_op(&op(0), &facts(), 0).unwrap_err().key, "fds_mismatch");
        assert_eq!(check_op(&op(0), &facts(), 2).unwrap_err().key, "fds_mismatch");
        // A path outside the rebuild directory, a permanent package among those removed: refusal before the file.
        let outside = PrivOp::InstallBuilt { package: local(0, "/tmp/x.pkg.tar.zst"), build_only: Vec::new(), replace: false };
        assert_eq!(check_op(&outside, &facts(), 1).unwrap_err().key, "refused.outside-root");
        let vital = PrivOp::InstallBuilt { package: local(0, BUILT), build_only: strings(&["glibc"]), replace: false };
        assert_eq!(check_op(&vital, &facts(), 1).unwrap_err().key, "refused.vital");
    }

    #[test]
    fn имя_пакета_из_файла_и_pkginfo() {
        assert_eq!(pkgname_from_file(BUILT).as_deref(), Some("deepin-session-treeland"));
        assert_eq!(pkgname_from_file("/a/x-1-1-any.pkg.tar").as_deref(), Some("x"));
        assert_eq!(pkgname_from_file("/a/x.pkg.tar.zst"), None);
        assert_eq!(pkginfo_name("# gen\npkgname = foo\npkgver = 1\n").as_deref(), Some("foo"));
    }

    #[test]
    fn собранный_пакет_ставится_копией_после_сверки_pkginfo() {
        let io = FakeIo::arch();
        {
            let mut s = io.st();
            s.exe.insert("/usr/bin/bsdtar".into());
            s.pkginfo = "pkgname = deepin-session-treeland\n".into();
        }
        let rebuilt = Mutex::new(strings(&["cmake"]));
        let plan_ = Plan::InstallBuilt {
            label: BUILT.into(),
            build_only: strings(&["cmake", "ninja"]),
            replace: true,
            rebuild_root: validate::user_rebuild_root("/home/test"),
        };
        let out = run_plan(&io, &plan_, vec![user_file("ok", b"pkg")], &rebuilt);
        assert_eq!(out, Outcome::ok(None));
        let runs = io.st().runs.clone();
        assert_eq!(runs[0][..5], strings(&["pacman", "-U", "--noconfirm", "--ask=4", "--"]));
        assert!(!runs[0][5].starts_with("/home/"), "pacman получил путь пользователя: {:?}", runs[0]);
        assert!(runs[0][5].ends_with("deepin-session-treeland-1.2-3-x86_64.pkg.tar.zst"));
        // ninja was not installed by this build-deps, cmake is not marked as a
        // dependency (pacman -Qqd is empty): nothing is removed.
        assert_eq!(runs.len(), 1);
        assert!(rebuilt.lock().unwrap().is_empty(), "запись build-deps израсходована");
    }

    #[test]
    fn чужое_имя_в_pkginfo_отказ_без_установки() {
        let io = FakeIo::arch();
        {
            let mut s = io.st();
            s.exe.insert("/usr/bin/bsdtar".into());
            s.pkginfo = "pkgname = evil\n".into();
        }
        let plan_ = Plan::InstallBuilt {
            label: BUILT.into(),
            build_only: Vec::new(),
            replace: false,
            rebuild_root: validate::user_rebuild_root("/home/test"),
        };
        let out = run_plan(&io, &plan_, vec![user_file("evil", b"pkg")], &Mutex::new(Vec::new()));
        assert_eq!(out, Outcome::Refused("refused.pkginfo".into()));
        assert!(io.touched_nothing());
    }

    #[test]
    fn каталог_вместо_файла_отказ() {
        let io = FakeIo::arch();
        io.st().exe.insert("/usr/bin/bsdtar".into());
        let dir = OwnedFd::from(std::fs::File::open(std::env::temp_dir()).unwrap());
        let plan_ = Plan::InstallBuilt {
            label: BUILT.into(),
            build_only: Vec::new(),
            replace: false,
            rebuild_root: validate::user_rebuild_root("/home/test"),
        };
        let out = run_plan(&io, &plan_, vec![dir], &Mutex::new(Vec::new()));
        assert_eq!(out, Outcome::Refused("refused.not-regular".into()));
        assert!(io.touched_nothing());
    }

    // ── AUR shim (B10) ───────────────────────────────────────────────────

    const AUR_BUILT: &str = "/home/test/.cache/paru/clone/t/t-1.0-1-x86_64.pkg.tar.zst";

    #[test]
    fn шим_aur_проверки_до_действия() {
        let f = facts();
        let built = |label: &str, allowed: &[&str]| PrivOp::AurInstallBuilt {
            packages: vec![local(0, label)],
            allowed: strings(allowed),
            replace: false,
        };
        assert!(matches!(check_op(&built(AUR_BUILT, &["t"]), &f, 1), Ok(Plan::AurInstallBuilt { .. })));
        assert_eq!(check_op(&built(AUR_BUILT, &["t"]), &f, 0).unwrap_err().key, "fds_mismatch");
        assert_eq!(check_op(&built("/tmp/x.deb", &["t"]), &f, 1).unwrap_err().key, "refused.package-file");
        assert_eq!(check_op(&built("rel/t-1-1-any.pkg.tar", &["t"]), &f, 1).unwrap_err().key, "refused.path");
        assert_eq!(check_op(&built(AUR_BUILT, &["glibc"]), &f, 1).unwrap_err().key, "refused.vital");
        assert_eq!(check_op(&built(AUR_BUILT, &[]), &f, 1).unwrap_err().key, "refused.empty");
        let deps = |n: &[&str]| PrivOp::AurDeps { names: strings(n) };
        assert!(check_op(&deps(&["cmake"]), &f, 0).is_ok());
        assert_eq!(check_op(&deps(&["-oX"]), &f, 0).unwrap_err().key, "refused.operand");
        assert_eq!(check_op(&deps(&["extra/cmake"]), &f, 0).unwrap_err().key, "refused.package-name");
        assert!(check_op(&deps(&["cmake>=3"]), &f, 0).is_err(), "версия в имени");
        let drop = PrivOp::AurDropMake { names: strings(&["linux"]) };
        assert_eq!(check_op(&drop, &f, 0).unwrap_err().key, "refused.vital");
        let debian = Facts { family: PkgFamily::Debian, ..facts() };
        assert_eq!(check_op(&deps(&["cmake"]), &debian, 0).unwrap_err().key, "refused.family");
    }

    #[test]
    fn шим_aur_зависимости_только_из_sync_базы() {
        let io = FakeIo::arch();
        answers(&io, &[("pacman -Slq", "cmake\nninja\n")]);
        let out = run_plan(&io, &Plan::AurDeps { names: strings(&["cmake", "ninja"]) }, vec![], &Mutex::new(Vec::new()));
        assert_eq!(out, Outcome::ok(None));
        assert_eq!(io.st().runs, vec![strings(&["pacman", "-S", "--needed", "--noconfirm", "--asdeps", "--", "cmake", "ninja"])]);
        let io = FakeIo::arch();
        answers(&io, &[("pacman -Slq", "cmake\n")]);
        let out = run_plan(&io, &Plan::AurDeps { names: strings(&["cmake", "evil-aur"]) }, vec![], &Mutex::new(Vec::new()));
        assert_eq!(out, Outcome::Refused("refused.not-in-table".into()));
        assert!(io.touched_nothing());
    }

    #[test]
    fn шим_aur_собранное_копией_и_по_pkginfo() {
        let plan_ = |allowed: &[&str], replace| Plan::AurInstallBuilt {
            labels: strings(&[AUR_BUILT]),
            allowed: strings(allowed),
            replace,
        };
        let io = FakeIo::arch();
        {
            let mut s = io.st();
            s.exe.insert("/usr/bin/bsdtar".into());
            s.pkginfo = "pkgname = t\n".into();
        }
        answers(&io, &[("pacman -Slq", "cmake\n")]);
        let out = run_plan(&io, &plan_(&["t"], true), vec![user_file("aur-ok", b"pkg")], &Mutex::new(Vec::new()));
        assert_eq!(out, Outcome::ok(None));
        let runs = io.st().runs.clone();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0][..4], strings(&["pacman", "-U", "--noconfirm", "--ask=4"]));
        assert!(!runs[0][5].starts_with("/home/"), "pacman получил путь пользователя: {:?}", runs[0]);

        // .PKGINFO with a foreign name: refusal.
        let io = FakeIo::arch();
        {
            let mut s = io.st();
            s.exe.insert("/usr/bin/bsdtar".into());
            s.pkginfo = "pkgname = evil\n".into();
        }
        let out = run_plan(&io, &plan_(&["t", "evil"], false), vec![user_file("aur-evil", b"pkg")], &Mutex::new(Vec::new()));
        assert_eq!(out, Outcome::Refused("refused.pkginfo".into()));
        assert!(io.touched_nothing());

        // A name from the list, but not the package that was asked for (`allowed` without it).
        let io = FakeIo::arch();
        {
            let mut s = io.st();
            s.exe.insert("/usr/bin/bsdtar".into());
            s.pkginfo = "pkgname = t\n".into();
        }
        let out = run_plan(&io, &plan_(&["other"], false), vec![user_file("aur-other", b"pkg")], &Mutex::new(Vec::new()));
        assert_eq!(out, Outcome::Refused("refused.pkginfo".into()));
        assert!(io.touched_nothing());

        // An official package name is not installed as a built one.
        let io = FakeIo::arch();
        {
            let mut s = io.st();
            s.exe.insert("/usr/bin/bsdtar".into());
            s.pkginfo = "pkgname = t\n".into();
        }
        answers(&io, &[("pacman -Slq", "t\n")]);
        let out = run_plan(&io, &plan_(&["t"], false), vec![user_file("aur-repo", b"pkg")], &Mutex::new(Vec::new()));
        assert_eq!(out, Outcome::Refused("refused.conflict".into()));
        assert!(io.touched_nothing());
    }

    #[test]
    fn шим_aur_снимаются_только_сироты() {
        let io = FakeIo::arch();
        answers(&io, &[("pacman -Qqdt", "cmake\n")]);
        let out = run_plan(&io, &Plan::AurDropMake { names: strings(&["cmake", "qt6-base"]) }, vec![], &Mutex::new(Vec::new()));
        assert_eq!(out, Outcome::ok(None));
        assert_eq!(io.st().runs, vec![strings(&["pacman", "-Rs", "--noconfirm", "--", "cmake"])]);
        let io = FakeIo::arch();
        let out = run_plan(&io, &Plan::AurDropMake { names: strings(&["qt6-base"]) }, vec![], &Mutex::new(Vec::new()));
        assert_eq!(out, Outcome::ok(None));
        assert!(io.touched_nothing());
    }

    // ── update guard orphan ──────────────────────────────────────────────

    #[test]
    fn сирота_проверки_до_действия() {
        let f = facts();
        let op = |n: &[&str]| PrivOp::OrphanRemove { names: strings(n) };
        assert_eq!(
            check_op(&op(&["kwin-x11"]), &f, 0),
            Ok(Plan::OrphanRemove { names: strings(&["kwin-x11"]), current_de: "kde".into() })
        );
        assert_eq!(check_op(&op(&[]), &f, 0).unwrap_err().key, "refused.empty");
        assert_eq!(check_op(&op(&["glibc"]), &f, 0).unwrap_err().key, "refused.vital");
        assert_eq!(check_op(&op(&["-Rdd"]), &f, 0).unwrap_err().key, "refused.operand");
        // A package of the current desktop; the desktop not found out; not pacman; SteamOS.
        assert_eq!(check_op(&op(&["plasma-meta"]), &f, 0).unwrap_err().key, "refused.current-desktop-package");
        let unknown = Facts { current_de: String::new(), ..facts() };
        assert_eq!(check_op(&op(&["kwin-x11"]), &unknown, 0).unwrap_err().key, "refused.current-desktop-unknown");
        let debian = Facts { family: PkgFamily::Debian, ..facts() };
        assert_eq!(check_op(&op(&["kwin-x11"]), &debian, 0).unwrap_err().key, "refused.family");
        let steam = Facts { steamos: true, ..facts() };
        assert_eq!(check_op(&op(&["kwin-x11"]), &steam, 0).unwrap_err().key, "refused.read-only-system");
        assert!(executes(&op(&["kwin-x11"])).is_ok());
        // The window's session is Plasma (X11): it depends on kwin-x11, so refusal
        // with the package name; Wayland or another desktop on X11 is allowed.
        let x11 = Facts { session_type: "x11".into(), ..facts() };
        assert_eq!(check_op(&op(&["kwin-x11"]), &x11, 0).unwrap_err().key, "refused.orphan-in-use.kwin-x11");
        let xfce_x11 = Facts { session_type: "x11".into(), current_de: "xfce".into(), ..facts() };
        assert!(check_op(&op(&["kwin-x11"]), &xfce_x11, 0).is_ok());
        assert!(check_op(&op(&["qt6-svg"]), &x11, 0).is_ok());
    }

    #[test]
    fn сирота_сеанса_экрана_входа_по_умолчанию_не_снимается() {
        let plasma_qg = "plasma plasma-workspace\nplasma kwin\nplasma kwin-x11\n";
        for (file, text) in [
            ("/var/lib/sddm/state.conf", "[Last]\nSession=/usr/share/xsessions/plasmax11.desktop\n"),
            ("/etc/sddm.conf.d/autologin.conf", "[Autologin]\nUser=test\nSession=plasmax11.desktop\n"),
            ("/etc/plasmalogin.conf", "[Autologin]\nSession=plasmax11\n"),
            ("/etc/plasmalogin.conf", "[Greeter]\nPreselectedSession=plasmax11.desktop\n"),
        ] {
            let io = FakeIo::arch();
            answers(&io, &[("pacman -Slq", "kwin\n"), ("pacman -Qg", plasma_qg), ("pacman -Rnsp", "kwin-x11\n")]);
            io.st().files.insert(file.into(), text.into());
            let out = run_plan(&io, &orphan_plan(&["kwin-x11"]), vec![], &Mutex::new(Vec::new()));
            assert_eq!(out, Outcome::Refused("refused.orphan-login-default.kwin-x11".into()), "{file}");
            assert!(io.touched_nothing(), "{file}");
        }
        // With a Wayland session by default it is removed.
        let io = FakeIo::arch();
        answers(&io, &[("pacman -Slq", "kwin\n"), ("pacman -Qg", plasma_qg), ("pacman -Rnsp", "kwin-x11\n")]);
        io.st().files.insert("/etc/plasmalogin.conf".into(), "[Autologin]\nSession=plasma\n".into());
        io.st().files.insert("/var/lib/sddm/state.conf".into(), "[Last]\nSession=/usr/share/wayland-sessions/plasma.desktop\n".into());
        let out = run_plan(&io, &orphan_plan(&["kwin-x11"]), vec![], &Mutex::new(Vec::new()));
        assert_eq!(out, Outcome::ok(None));
    }

    #[test]
    fn тип_сеанса_берётся_у_logind() {
        let io = FakeIo::arch();
        let uid = unsafe { libc::getuid() };
        io.st().loginctl = format!("User={uid}\nActive=yes\nDesktop=KDE\nType=x11\n");
        assert_eq!(verify_session(&*io, "3", uid), Ok(("KDE".to_string(), "x11".to_string())));
        let f = machine_facts(&*io, "KDE", "X11", uid);
        assert_eq!((f.current_de.as_str(), f.session_type.as_str()), ("kde", "x11"));
    }

    /// "Make Plasma (Wayland) the default session": only the values of the session
    /// keys pointing to plasmax11; everything else byte for byte; afterwards the
    /// orphan is removed by the same guard.
    #[test]
    fn сеанс_по_умолчанию_переводится_на_wayland() {
        let op = |x11: &str| PrivOp::LoginSessionWayland { x11: x11.into() };
        let plasma = &plan::X11_SESSION_SUCCESSORS[0];
        assert_eq!(check_op(&op("plasmax11"), &facts(), 0), Ok(Plan::LoginSessionWayland { session: plasma }));
        for bad in ["plasma", "plasmax11.desktop", "/usr/share/xsessions/plasmax11.desktop", ""] {
            assert_eq!(check_op(&op(bad), &facts(), 0).unwrap_err().key, "refused.display-manager", "{bad}");
        }
        let steam = Facts { steamos: true, ..facts() };
        assert_eq!(check_op(&op("plasmax11"), &steam, 0).unwrap_err().key, "refused.read-only-system");

        let sddm = "[Autologin]\nRelogin=false\nSession=plasmax11.desktop\nUser=test\n\n[Theme]\nCurrent=breeze\n";
        let state = "[Last]\n# последний\nSession=/usr/share/xsessions/plasmax11.desktop\nUser=test\n";
        let plm = "[Autologin]\r\nSession = plasmax11\r\n\r\n[Greeter]\r\nPreselectedSession=plasmax11.desktop\r\nShowClock=true\r\n";
        let wayland = "/usr/share/wayland-sessions/plasma.desktop";
        let machine = || {
            let io = FakeIo::arch();
            {
                let mut st = io.st();
                st.files.insert(wayland.into(), "[Desktop Entry]\n".into());
                st.files.insert("/etc/sddm.conf.d/kde_settings.conf".into(), sddm.into());
                st.files.insert("/var/lib/sddm/state.conf".into(), state.into());
                st.files.insert("/etc/plasmalogin.conf".into(), plm.into());
            }
            io
        };
        let lsw = Plan::LoginSessionWayland { session: plasma };
        let io = machine();
        assert_eq!(run_plan(&io, &lsw, vec![], &Mutex::new(Vec::new())), Outcome::ok(None));
        let st = io.st();
        assert_eq!(
            st.files["/etc/sddm.conf.d/kde_settings.conf"],
            "[Autologin]\nRelogin=false\nSession=plasma.desktop\nUser=test\n\n[Theme]\nCurrent=breeze\n"
        );
        assert_eq!(st.files["/var/lib/sddm/state.conf"], "[Last]\n# последний\nSession=/usr/share/wayland-sessions/plasma.desktop\nUser=test\n");
        assert_eq!(
            st.files["/etc/plasmalogin.conf"],
            "[Autologin]\r\nSession = plasma\r\n\r\n[Greeter]\r\nPreselectedSession=plasma.desktop\r\nShowClock=true\r\n"
        );
        assert_eq!(st.mutations.len(), 3, "{:?}", st.mutations);
        assert!(st.runs.is_empty(), "{:?}", st.runs);
        drop(st);
        // Now the orphan is removed.
        let plasma_qg = "plasma plasma-workspace\nplasma kwin\nplasma kwin-x11\n";
        answers(&io, &[("pacman -Slq", "kwin\n"), ("pacman -Qg", plasma_qg), ("pacman -Rnsp", "kwin-x11\n")]);
        assert_eq!(run_plan(&io, &orphan_plan(&["kwin-x11"]), vec![], &Mutex::new(Vec::new())), Outcome::ok(None));

        // The second time there is nothing to change, so nothing is written.
        let io2 = machine();
        let _ = run_plan(&io2, &lsw, vec![], &Mutex::new(Vec::new()));
        io2.st().mutations.clear();
        assert_eq!(run_plan(&io2, &lsw, vec![], &Mutex::new(Vec::new())), Outcome::ok(None));
        assert!(io2.touched_nothing());

        // No pair: refusal, nothing touched.
        let io = machine();
        io.st().files.remove(wayland);
        assert_eq!(
            run_plan(&io, &lsw, vec![], &Mutex::new(Vec::new())),
            Outcome::Refused("refused.wayland-session-missing.plasmax11".into())
        );
        assert!(io.touched_nothing());

        // The X11 session is set in a package file, which Rexon does not edit:
        // refusal before the first write (and `/etc` is untouched too).
        let io = machine();
        io.st().files.insert("/usr/lib/sddm/sddm.conf.d/10-x.conf".into(), "[Autologin]\nSession=plasmax11\n".into());
        io.st().files.remove("/etc/sddm.conf.d/kde_settings.conf");
        assert_eq!(
            run_plan(&io, &lsw, vec![], &Mutex::new(Vec::new())),
            Outcome::Refused("refused.session-set-elsewhere.plasmax11".into())
        );
        assert!(io.touched_nothing());

        // A link in place of the settings file: also a refusal without writing.
        let io = machine();
        {
            let mut st = io.st();
            st.files.remove("/etc/plasmalogin.conf");
            st.files.insert("/srv/elsewhere.conf".into(), plm.into());
            st.links.insert("/etc/plasmalogin.conf".into(), "/srv/elsewhere.conf".into());
        }
        assert_eq!(
            run_plan(&io, &lsw, vec![], &Mutex::new(Vec::new())),
            Outcome::Refused("refused.session-set-elsewhere.plasmax11".into())
        );
        assert!(io.touched_nothing());
    }

    fn orphan_plan(names: &[&str]) -> Plan {
        Plan::OrphanRemove { names: strings(names), current_de: "kde".into() }
    }

    #[test]
    fn сирота_снимается_только_проверенная_машиной() {
        let plasma_qg = "plasma plasma-workspace\nplasma kwin\nplasma kwin-x11\n";
        // An orphan: a member of the plasma group, installed, absent from the sync
        // database; the probe removes only it.
        let io = FakeIo::arch();
        answers(&io, &[
            ("pacman -Slq", "kwin\nplasma-workspace\n"),
            ("pacman -Qg", plasma_qg),
            ("pacman -Rnsp", "kwin-x11\n"),
        ]);
        let out = run_plan(&io, &orphan_plan(&["kwin-x11"]), vec![], &Mutex::new(Vec::new()));
        assert_eq!(out, Outcome::ok(None));
        assert_eq!(io.st().runs, vec![strings(&["pacman", "-Rns", "--noconfirm", "--", "kwin-x11"])]);

        // Present in the repositories: not an orphan.
        let io = FakeIo::arch();
        answers(&io, &[("pacman -Slq", "kwin-x11\n"), ("pacman -Qg", plasma_qg), ("pacman -Rnsp", "kwin-x11\n")]);
        let out = run_plan(&io, &orphan_plan(&["kwin-x11"]), vec![], &Mutex::new(Vec::new()));
        assert_eq!(out, Outcome::Refused("refused.not-orphan".into()));
        assert!(io.touched_nothing());

        // Not a member of an update guard set (a third-party AUR package): not an orphan.
        let io = FakeIo::arch();
        answers(&io, &[("pacman -Slq", "kwin\n"), ("pacman -Qg", plasma_qg), ("pacman -Rnsp", "decky-loader\n")]);
        let out = run_plan(&io, &orphan_plan(&["decky-loader"]), vec![], &Mutex::new(Vec::new()));
        assert_eq!(out, Outcome::Refused("refused.not-orphan".into()));
        assert!(io.touched_nothing());

        // Not installed.
        let io = FakeIo::arch();
        answers(&io, &[
            ("pacman -Slq", "kwin\n"),
            ("pacman -Qg", plasma_qg),
            ("pacman -Q -- kwin-x11", ""),
        ]);
        io.st().answers[2].1 = 1;
        let out = run_plan(&io, &orphan_plan(&["kwin-x11"]), vec![], &Mutex::new(Vec::new()));
        assert_eq!(out, Outcome::Refused("refused.not-orphan".into()));
        assert!(io.touched_nothing());

        // The sync database could not be read: nothing to check with.
        let io = FakeIo::arch();
        answers(&io, &[("pacman -Qg", plasma_qg), ("pacman -Rnsp", "kwin-x11\n")]);
        let out = run_plan(&io, &orphan_plan(&["kwin-x11"]), vec![], &Mutex::new(Vec::new()));
        assert_eq!(out, Outcome::Refused("refused.not-orphan".into()));
        assert!(io.touched_nothing());

        // The transaction would take away something vital or a package of the current desktop.
        for (extra, key) in [("glibc", "refused.vital"), ("plasma-meta", "refused.current-desktop-package")] {
            let io = FakeIo::arch();
            let preview = format!("kwin-x11\n{extra}\n");
            answers(&io, &[("pacman -Slq", "kwin\n"), ("pacman -Qg", plasma_qg), ("pacman -Rnsp", preview.as_str())]);
            let out = run_plan(&io, &orphan_plan(&["kwin-x11"]), vec![], &Mutex::new(Vec::new()));
            assert_eq!(out, Outcome::Refused(key.into()), "{extra}");
            assert!(io.touched_nothing());
        }

        // The probe failed (needed by others, the database is locked): refusal, nothing touched.
        let io = FakeIo::arch();
        answers(&io, &[("pacman -Slq", "kwin\n"), ("pacman -Qg", plasma_qg), ("pacman -Rnsp", "")]);
        io.st().answers[2].1 = 1;
        let out = run_plan(&io, &orphan_plan(&["kwin-x11"]), vec![], &Mutex::new(Vec::new()));
        assert_eq!(out, Outcome::Refused("refused.orphan-needed".into()));
        assert!(io.touched_nothing());
    }

    // ── rolling back an update guard set from the pacman cache ───────────

    const QT_CACHE: &str = "/var/cache/pacman/pkg/qt6-base-6.11.2-1-x86_64.pkg.tar.zst";

    #[test]
    fn cache_rollback_checks_before_action() {
        let f = facts();
        let op = |files: &[&str]| PrivOp::CacheRollback { files: strings(files) };
        assert_eq!(check_op(&op(&[QT_CACHE]), &f, 0), Ok(Plan::CacheRollback { files: strings(&[QT_CACHE]) }));
        assert!(executes(&op(&[QT_CACHE])).is_ok());
        assert_eq!(check_op(&op(&[]), &f, 0).unwrap_err().key, "refused.empty");
        let many: Vec<String> =
            (0..65).map(|i| format!("/var/cache/pacman/pkg/p{i}-1-1-x86_64.pkg.tar.zst")).collect();
        let many_op = PrivOp::CacheRollback { files: many };
        assert_eq!(check_op(&many_op, &f, 0).unwrap_err().key, "refused.too-many");
        // The same file name twice (from two directories): the copies will not fit.
        let twice = op(&[QT_CACHE, "/mnt/c/qt6-base-6.11.2-1-x86_64.pkg.tar.zst"]);
        assert_eq!(check_op(&twice, &f, 0).unwrap_err().key, "refused.package-file");
        for bad in ["qt6-base-6.11.2-1-x86_64.pkg.tar.zst", "/var/../qt6-base-6.11.2-1-x86_64.pkg.tar.zst"] {
            assert_eq!(check_op(&op(&[bad]), &f, 0).unwrap_err().key, "refused.path", "{bad}");
        }
        assert_eq!(check_op(&op(&["/var/cache/pacman/pkg/passwd"]), &f, 0).unwrap_err().key, "refused.package-file");
        let debian = Facts { family: PkgFamily::Debian, ..facts() };
        assert_eq!(check_op(&op(&[QT_CACHE]), &debian, 0).unwrap_err().key, "refused.family");
        let steam = Facts { steamos: true, ..facts() };
        assert_eq!(check_op(&op(&[QT_CACHE]), &steam, 0).unwrap_err().key, "refused.read-only-system");
        // The current desktop not found out: it does not hinder the rollback (the protection is exactly the set).
        let unknown = Facts { current_de: String::new(), ..facts() };
        assert!(check_op(&op(&[QT_CACHE]), &unknown, 0).is_ok());
    }

    /// A machine with mismatched Qt after an update: qt6-base 6.12.0 with
    /// qt6-declarative 6.11.2, rolling qt6-base back to the 6.11.2 series.
    /// `extra` are specific answers (ahead of the shared ones); the whole
    /// `pacman -Q` goes into `installed` (otherwise the shared `pacman -Q` prefix
    /// would intercept `-Qg`/`-Qmq`).
    fn qt_mixed_machine(io: &FakeIo, extra: &[(&str, i32, &str)]) {
        qt_mixed_machine_with(io, extra, QT_MIXED);
    }

    const QT_MIXED: &str = "qt6-base 6.12.0-2\nqt6-declarative 6.11.2-1.1\nqt6-webengine 6.11.2-2\nfirefox 141.0-1\n";

    fn qt_mixed_machine_with(io: &FakeIo, extra: &[(&str, i32, &str)], installed: &str) {
        let mut s = io.st();
        for p in ["bsdtar", "vercmp", "pacman-conf", "pacman-key"] {
            s.exe.insert(format!("/usr/bin/{p}"));
        }
        s.root_files.insert(QT_CACHE.into(), b"zst".to_vec());
        s.pkginfos.insert(basename(QT_CACHE).into(), "pkgname = qt6-base\npkgver = 6.11.2-1\n".into());
        let mut answers: Vec<(String, i32, String)> = extra.iter().map(|(k, c, v)| (k.to_string(), *c, v.to_string())).collect();
        answers.extend(
            [
                ("pacman-conf CacheDir", 0, "/var/cache/pacman/pkg/\n"),
                ("pacman -Q --", 0, "qt6-base 6.11.2-1\n"),
                ("pacman -Qg", 0, ""),
                ("pacman -Qmq", 1, ""),
                ("pacman -U --print", 0, "qt6-base 6.11.2-1\n"),
                ("vercmp 6.11.2-1 6.12.0-2", 0, "-1\n"),
            ]
            .iter()
            .map(|(k, c, v)| (k.to_string(), *c, v.to_string())),
        );
        answers.push(("pacman -Q".into(), 0, installed.into()));
        s.answers = answers;
    }

    fn rollback_plan(files: &[&str]) -> Plan {
        Plan::CacheRollback { files: strings(files) }
    }

    #[test]
    fn cache_rollback_installs_only_checked_copies() {
        let io = FakeIo::arch();
        qt_mixed_machine(&io, &[]);
        let out = run_plan(&io, &rollback_plan(&[QT_CACHE]), vec![], &Mutex::new(Vec::new()));
        assert_eq!(out, Outcome::ok(None), "{:?}", io.st().log);
        let runs = io.st().runs.clone();
        assert_eq!(runs.len(), 1, "{runs:?}");
        assert_eq!(runs[0][..4], strings(&["pacman", "-U", "--noconfirm", "--"]));
        // The root copy is installed, not the file at the window's path.
        assert_eq!(runs[0].len(), 5);
        assert_ne!(runs[0][4], QT_CACHE);
        assert!(runs[0][4].ends_with("/qt6-base-6.11.2-1-x86_64.pkg.tar.zst"), "{runs:?}");

        // A signature is next to it and matches: checked on the copies, installed.
        let io = FakeIo::arch();
        qt_mixed_machine(&io, &[]);
        let sig = format!("{QT_CACHE}.sig");
        io.st().files.insert(sig.clone(), "sig".into());
        io.st().root_files.insert(sig, b"sig".to_vec());
        let out = run_plan(&io, &rollback_plan(&[QT_CACHE]), vec![], &Mutex::new(Vec::new()));
        assert_eq!(out, Outcome::ok(None), "{:?}", io.st().log);
        assert_eq!(io.st().runs.len(), 1);

        // A cache directory from pacman.conf is a cache too.
        let io = FakeIo::arch();
        let other = "/mnt/c/qt6-base-6.11.2-1-x86_64.pkg.tar.zst";
        qt_mixed_machine(&io, &[("pacman-conf CacheDir", 0, "/var/cache/pacman/pkg/\n/mnt/c/\n")]);
        io.st().root_files.insert(other.into(), b"zst".to_vec());
        let out = run_plan(&io, &rollback_plan(&[other]), vec![], &Mutex::new(Vec::new()));
        assert_eq!(out, Outcome::ok(None), "{:?}", io.st().log);
    }

    #[test]
    fn cache_rollback_refuses_without_touching() {
        let refused = |extra: &[(&str, i32, &str)], prep: &dyn Fn(&FakeIo), files: &[&str], key: &str| {
            let io = FakeIo::arch();
            qt_mixed_machine(&io, extra);
            prep(&io);
            let out = run_plan(&io, &rollback_plan(files), vec![], &Mutex::new(Vec::new()));
            assert_eq!(out, Outcome::Refused(key.into()), "{key}: {:?}", io.st().log);
            assert!(io.touched_nothing(), "{key}");
        };
        let none = |_: &FakeIo| {};
        // Not a cache directory (a file of the same kind at the user's).
        let home = "/home/test/qt6-base-6.11.2-1-x86_64.pkg.tar.zst";
        refused(&[], &|io| { io.st().root_files.insert(home.into(), b"zst".to_vec()); }, &[home], "refused.path");
        // The cache file is not root-only.
        refused(&[], &|io| { io.st().root_refuse.insert(QT_CACHE.into()); }, &[QT_CACHE], "refused.not-root-owned");
        // .PKGINFO: a foreign name, no version, a permanent package.
        let pkginfo = |text: &'static str| {
            move |io: &FakeIo| {
                io.st().pkginfos.insert(basename(QT_CACHE).into(), text.into());
            }
        };
        refused(&[], &pkginfo("pkgname = evil\npkgver = 6.11.2-1\n"), &[QT_CACHE], "refused.not-rollback");
        refused(&[], &pkginfo("pkgname = qt6-base\n"), &[QT_CACHE], "refused.pkginfo");
        refused(&[], &pkginfo("pkgname = glibc\npkgver = 2.40-1\n"), &[QT_CACHE], "refused.vital");
        // A version of the wrong series (the same as installed): that is not a rollback.
        refused(&[], &pkginfo("pkgname = qt6-base\npkgver = 6.12.0-2\n"), &[QT_CACHE], "refused.not-rollback");
        // vercmp does not confirm "lower".
        refused(&[("vercmp", 0, "1\n")], &none, &[QT_CACHE], "refused.not-rollback");
        // The set has already converged (nothing to roll back).
        let io = FakeIo::arch();
        qt_mixed_machine_with(&io, &[], "qt6-base 6.11.2-1\nqt6-declarative 6.11.2-1.1\n");
        let out = run_plan(&io, &rollback_plan(&[QT_CACHE]), vec![], &Mutex::new(Vec::new()));
        assert_eq!(out, Outcome::Refused("refused.not-rollback".into()));
        assert!(io.touched_nothing());
        // Beyond the set: qt6-declarative as well (it is not in the finding's rollback).
        let decl = "/var/cache/pacman/pkg/qt6-declarative-6.11.1-1-x86_64.pkg.tar.zst";
        refused(
            &[],
            &|io| {
                let mut s = io.st();
                s.root_files.insert(decl.into(), b"zst".to_vec());
                s.pkginfos.insert(basename(decl).into(), "pkgname = qt6-declarative\npkgver = 6.11.1-1\n".into());
            },
            &[QT_CACHE, decl],
            "refused.not-rollback",
        );
        // A signature IS next to it, but does not match or is not a root file: tampering.
        let sig = format!("{QT_CACHE}.sig");
        let with_sig = |io: &FakeIo| {
            let mut s = io.st();
            s.files.insert(sig.clone(), "sig".into());
            s.root_files.insert(sig.clone(), b"sig".to_vec());
        };
        refused(&[("pacman-key --verify", 1, "")], &with_sig, &[QT_CACHE], ROLLBACK_SIGNATURE);
        refused(&[], &|io| { with_sig(io); io.st().root_refuse.insert(sig.clone()); }, &[QT_CACHE], ROLLBACK_SIGNATURE);
        refused(&[], &|io| { with_sig(io); io.st().exe.remove("/usr/bin/pacman-key"); }, &[QT_CACHE], ROLLBACK_SIGNATURE);
        // The probe pulls in extra things or fails.
        refused(&[("pacman -U --print", 0, "qt6-base 6.11.2-1\nqt6-svg 6.11.2-1\n")], &none, &[QT_CACHE], ROLLBACK_EXTRA);
        refused(&[("pacman -U --print", 1, "")], &none, &[QT_CACHE], ROLLBACK_EXTRA);
        refused(&[("pacman -U --print", 0, "qt6-base 6.11.2-1\nwarning: downgrading package\n")], &none, &[QT_CACHE], ROLLBACK_EXTRA);
        // The previous version needs a dependency that is missing: pacman 7.1 puts it
        // into the probe from the repository (before the package itself); a rollback
        // is only exactly the set, nothing is pulled from the network.
        refused(
            &[("pacman -U --print", 0, "deno 2.9.8-1\nqt6-base 6.11.2-1\n")],
            &none,
            &[QT_CACHE],
            ROLLBACK_EXTRA,
        );
    }

    /// An incomplete rollback of a set: refusal. Plasma is mismatched, both
    /// plasma-workspace and kwin must be rolled back, but the window sent only one.
    #[test]
    fn cache_rollback_refuses_partial_set() {
        let pw = "/var/cache/pacman/pkg/plasma-workspace-6.7.5-3.1-x86_64.pkg.tar.zst";
        let kw = "/var/cache/pacman/pkg/kwin-6.7.5-3.1-x86_64.pkg.tar.zst";
        let machine = |io: &FakeIo| {
            qt_mixed_machine_with(
                io,
                &[
                    ("pacman -Q --", 0, "plasma-workspace 6.7.5-3.1\nkwin 6.7.5-3.1\n"),
                    ("pacman -Qg", 0, "plasma plasma-workspace\nplasma kwin\nplasma kwin-x11\n"),
                    ("pacman -U --print", 0, "plasma-workspace 6.7.5-3.1\nkwin 6.7.5-3.1\n"),
                    ("vercmp", 0, "-1\n"),
                ],
                "plasma-workspace 6.8.1-1.1\nkwin 6.8.1-2.1\nkwin-x11 6.7.5-3.1\n",
            );
            let mut s = io.st();
            for (p, n) in [(pw, "plasma-workspace"), (kw, "kwin")] {
                s.root_files.insert(p.into(), b"zst".to_vec());
                s.pkginfos.insert(basename(p).into(), format!("pkgname = {n}\npkgver = 6.7.5-3.1\n"));
            }
        };
        let io = FakeIo::arch();
        machine(&io);
        let out = run_plan(&io, &rollback_plan(&[pw]), vec![], &Mutex::new(Vec::new()));
        assert_eq!(out, Outcome::Refused("refused.not-rollback".into()));
        assert!(io.touched_nothing());
        // The whole set is one transaction; the probe is checked as a set of pairs:
        // the order of pacman's lines may differ from the order of the files.
        let io = FakeIo::arch();
        machine(&io);
        io.st().answers.insert(0, ("pacman -U --print".into(), 0, "kwin 6.7.5-3.1\nplasma-workspace 6.7.5-3.1\n".into()));
        let out = run_plan(&io, &rollback_plan(&[pw, kw]), vec![], &Mutex::new(Vec::new()));
        assert_eq!(out, Outcome::ok(None), "{:?}", io.st().log);
        let runs = io.st().runs.clone();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].len(), 6, "{runs:?}");
    }

    /// pacman finished, but the versions afterwards are wrong: a failure, not "done".
    #[test]
    fn cache_rollback_verifies_versions_after() {
        let io = FakeIo::arch();
        qt_mixed_machine(&io, &[("pacman -Q --", 0, "qt6-base 6.12.0-2\n")]);
        let out = run_plan(&io, &rollback_plan(&[QT_CACHE]), vec![], &Mutex::new(Vec::new()));
        assert_eq!(out, Outcome::Done { ok: false, code: Some(1), key: None });
        // pacman failed: its code.
        let io = FakeIo::arch();
        qt_mixed_machine(&io, &[]);
        io.st().fail_runs.push("pacman -U".into());
        let out = run_plan(&io, &rollback_plan(&[QT_CACHE]), vec![], &Mutex::new(Vec::new()));
        assert_eq!(out, Outcome::Done { ok: false, code: Some(1), key: None });
    }

    /// User directory → ustar in memory (as the window packs it).
    fn packed(tag: &str, files: &[(&str, &[u8])]) -> OwnedFd {
        let dir = std::env::temp_dir().join(format!("rexon-b4-tree-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for (name, bytes) in files {
            std::fs::write(dir.join(name), bytes).unwrap();
        }
        let fd = unpack::pack_tree_memfd(&dir).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        fd
    }

    #[test]
    fn фон_gdm_раскладывается_по_константам_ядра() {
        let io = FakeIo::arch();
        io.st().exe.insert("/usr/bin/dconf".into());
        let plan_ = Plan::Stage { category: "gdm-bg".into(), name: "bg".into() };
        let out = run_plan(&io, &plan_, vec![packed("gdm", &[("a.JPG", b"img")])], &Mutex::new(Vec::new()));
        assert_eq!(out, Outcome::ok(Some("bg_installed")));
        let s = io.st();
        assert_eq!(s.files["/usr/share/backgrounds/rexon-login.JPG"], "img");
        assert!(s.files["/etc/dconf/db/gdm.d/02-rexon-login"].contains("file:///usr/share/backgrounds/rexon-login.JPG"));
        assert_eq!(s.runs, vec![strings(&["dconf", "update"])]);
    }

    #[test]
    fn фон_без_картинки_отказ_до_касания() {
        let io = FakeIo::arch();
        let plan_ = Plan::Stage { category: "login-bg".into(), name: "bg".into() };
        let out = run_plan(&io, &plan_, vec![packed("noimg", &[("a.txt", b"x")])], &Mutex::new(Vec::new()));
        assert_eq!(out, Outcome::fail("no_image"));
        assert!(io.touched_nothing());
    }

    #[test]
    fn тема_plymouth_с_библиотекой_отказ() {
        let io = FakeIo::arch();
        let plan_ = Plan::ThemeInstall { kind: SystemThemeKind::Plymouth, theme: "t".into() };
        let fd = packed("ply", &[("t.plymouth", b"[Plymouth Theme]\nModuleName=script\n"), ("evil.so", b"\x7fELF")]);
        let out = run_plan(&io, &plan_, vec![fd], &Mutex::new(Vec::new()));
        assert!(matches!(out, Outcome::Refused(_)), "{out:?}");
        assert!(io.touched_nothing());
    }

    #[test]
    fn тема_sddm_ставится_копией_и_применяется() {
        let io = FakeIo::arch();
        let plan_ = Plan::ThemeInstall { kind: SystemThemeKind::Sddm, theme: "t".into() };
        let out = run_plan(&io, &plan_, vec![packed("sddm", &[("Main.qml", b"x"), ("metadata.desktop", b"y")])], &Mutex::new(Vec::new()));
        assert_eq!(out, Outcome::ok(None));
        assert_eq!(io.st().mutations, vec!["copy_tree /usr/share/sddm/themes/t".to_string()]);

        io.st().dirs.insert("/usr/share/sddm/themes/t".into());
        let out = run_plan(&io, &Plan::ThemeApply { kind: SystemThemeKind::Sddm, theme: "t".into() }, Vec::new(), &Mutex::new(Vec::new()));
        assert_eq!(out, Outcome::ok(None));
        assert_eq!(io.st().files["/etc/sddm.conf.d/zz-rexon-theme.conf"], "[Theme]\nCurrent=t\n");
    }

    fn apply(io: &FakeIo, kind: SystemThemeKind, theme: &str) -> Outcome {
        run_plan(io, &Plan::ThemeApply { kind, theme: theme.into() }, Vec::new(), &Mutex::new(Vec::new()))
    }

    #[test]
    fn тема_sddm_перекрытая_etc_sddm_conf_выбирается_и_там() {
        // As on CachyOS: `/etc/sddm.conf` is read last and holds its own theme.
        let io = FakeIo::arch();
        {
            let mut s = io.st();
            s.dirs.insert("/usr/share/sddm/themes/t".into());
            s.files.insert("/etc/sddm.conf.d/kde_settings.conf".into(), "[Theme]\nCurrent=breeze\n".into());
            s.files.insert("/etc/sddm.conf".into(), "[Autologin]\nSession=plasma\n\n[Theme]\nCurrent=breeze\nCursorSize=32\n".into());
        }
        assert_eq!(apply(&io, SystemThemeKind::Sddm, "t"), Outcome::ok(None));
        let s = io.st();
        assert_eq!(s.files["/etc/sddm.conf"], "[Autologin]\nSession=plasma\n\n[Theme]\nCurrent=t\nCursorSize=32\n");
        assert_eq!(s.files["/etc/sddm.conf.d/kde_settings.conf"], "[Theme]\nCurrent=t\n");
        assert!(!s.files.contains_key(plan::SDDM_REXON_THEME_CONF));
        assert!(s.runs.is_empty());
    }

    #[test]
    fn тема_sddm_не_записалась_прежние_файлы_возвращаются() {
        let io = FakeIo::arch();
        {
            let mut s = io.st();
            s.dirs.insert("/usr/share/sddm/themes/t".into());
            s.files.insert("/etc/sddm.conf.d/kde_settings.conf".into(), "[Theme]\nCurrent=breeze\n".into());
            s.files.insert("/etc/sddm.conf".into(), "[Theme]\nCurrent=breeze\n".into());
            s.fail_writes.insert("/etc/sddm.conf".into());
        }
        let out = apply(&io, SystemThemeKind::Sddm, "t");
        assert!(matches!(out, Outcome::Done { ok: false, .. }), "{out:?}");
        let s = io.st();
        assert_eq!(s.files["/etc/sddm.conf.d/kde_settings.conf"], "[Theme]\nCurrent=breeze\n");
        assert_eq!(
            s.mutations,
            ["write /etc/sddm.conf.d/kde_settings.conf", "write /etc/sddm.conf.d/kde_settings.conf"].map(String::from)
        );
    }

    #[test]
    fn тема_sddm_ссылкой_не_правится() {
        let io = FakeIo::arch();
        {
            let mut s = io.st();
            s.dirs.insert("/usr/share/sddm/themes/t".into());
            s.files.insert("/etc/real.conf".into(), "[Theme]\nCurrent=breeze\n".into());
            s.links.insert("/etc/sddm.conf".into(), "/etc/real.conf".into());
        }
        assert_eq!(apply(&io, SystemThemeKind::Sddm, "t"), Outcome::fail("theme_set_elsewhere"));
        assert!(io.touched_nothing());
    }

    /// A system with plymouth-set-default-theme and a plymouth rebuild script;
    /// `bgrt` is selected, the theme `t` lies in the system directory.
    fn plymouth_io() -> Arc<FakeIo> {
        let io = FakeIo::arch();
        {
            let mut s = io.st();
            s.dirs.insert("/usr/share/plymouth/themes/t".into());
            s.files.insert("/usr/share/plymouth/themes/t/t.plymouth".into(), "[Plymouth Theme]\nModuleName=script\n".into());
            s.exe.insert("/usr/bin/plymouth-set-default-theme".into());
            s.exe.insert("/usr/lib/plymouth/plymouth-update-initrd".into());
            s.exe.insert("/usr/bin/mkinitcpio".into());
            s.answers.push(("plymouth-set-default-theme".into(), 0, "bgrt\n".into()));
        }
        io
    }

    #[test]
    fn тема_plymouth_выбирается_и_образ_пересобирается_сценарием_plymouth() {
        let io = plymouth_io();
        assert_eq!(apply(&io, SystemThemeKind::Plymouth, "t"), Outcome::ok(None));
        // The plymouth script comes before a bare mkinitcpio (Limine on CachyOS).
        assert_eq!(
            io.st().runs,
            vec![strings(&["plymouth-set-default-theme", "t"]), strings(&["/usr/lib/plymouth/plymouth-update-initrd"])]
        );
    }

    #[test]
    fn тема_plymouth_образ_не_пересобрался_прежняя_тема_возвращается() {
        let io = plymouth_io();
        io.st().fail_runs.push("/usr/lib/plymouth/plymouth-update-initrd".into());
        assert_eq!(apply(&io, SystemThemeKind::Plymouth, "t"), Outcome::fail("initramfs_failed"));
        assert_eq!(io.st().runs.last().unwrap(), &strings(&["plymouth-set-default-theme", "bgrt"]));

        // Nothing to rebuild with: also a rollback, not "applied".
        let io = plymouth_io();
        {
            let mut s = io.st();
            s.exe.remove("/usr/lib/plymouth/plymouth-update-initrd");
            s.exe.remove("/usr/bin/mkinitcpio");
        }
        assert_eq!(apply(&io, SystemThemeKind::Plymouth, "t"), Outcome::fail("initramfs_failed"));
        assert_eq!(
            io.st().runs,
            vec![strings(&["plymouth-set-default-theme", "t"]), strings(&["plymouth-set-default-theme", "bgrt"])]
        );
    }

    #[test]
    fn тема_plymouth_не_выбралась_образ_не_трогается() {
        let io = plymouth_io();
        io.st().fail_runs.push("plymouth-set-default-theme t".into());
        let out = apply(&io, SystemThemeKind::Plymouth, "t");
        assert!(matches!(out, Outcome::Done { ok: false, .. }), "{out:?}");
        assert_eq!(io.st().runs, vec![strings(&["plymouth-set-default-theme", "t"])]);
    }

    #[test]
    fn тема_plymouth_через_alternatives_с_откатом() {
        let io = FakeIo::arch();
        {
            let mut s = io.st();
            s.dirs.insert("/usr/share/plymouth/themes/t".into());
            s.files.insert("/usr/share/plymouth/themes/t/t.plymouth".into(), "x".into());
            s.files.insert("/usr/share/plymouth/themes/old/old.plymouth".into(), "x".into());
            s.links.insert("/usr/share/plymouth/themes/default.plymouth".into(), "/usr/share/plymouth/themes/old/old.plymouth".into());
            s.exe.insert("/usr/bin/update-alternatives".into());
            s.exe.insert("/usr/bin/update-initramfs".into());
            s.fail_runs.push("update-initramfs".into());
        }
        assert_eq!(apply(&io, SystemThemeKind::Plymouth, "t"), Outcome::fail("initramfs_failed"));
        let runs = io.st().runs.clone();
        assert_eq!(runs[2], strings(&["update-initramfs", "-u"]));
        assert_eq!(
            runs.last().unwrap(),
            &strings(&["update-alternatives", "--set", "default.plymouth", "/usr/share/plymouth/themes/old/old.plymouth"])
        );
    }

    #[test]
    fn не_установленную_тему_не_выбрать() {
        let io = plymouth_io();
        assert_eq!(apply(&io, SystemThemeKind::Plymouth, "nope"), Outcome::fail("theme_not_installed"));
        assert_eq!(apply(&io, SystemThemeKind::Sddm, "t"), Outcome::fail("theme_not_installed"));
        assert!(io.touched_nothing());
    }

    #[test]
    fn выбранную_тему_не_удалить() {
        let io = FakeIo::arch();
        {
            let mut s = io.st();
            s.dirs.insert("/usr/share/sddm/themes/t".into());
            s.files.insert("/etc/sddm.conf.d/zz-rexon-theme.conf".into(), "[Theme]\nCurrent = t\n".into());
        }
        let out = run_plan(&io, &Plan::ThemeRemove { kind: SystemThemeKind::Sddm, theme: "t".into() }, Vec::new(), &Mutex::new(Vec::new()));
        assert_eq!(out, Outcome::fail("theme_active"));
        assert!(io.touched_nothing());
        io.st().files.clear();
        let out = run_plan(&io, &Plan::ThemeRemove { kind: SystemThemeKind::Sddm, theme: "t".into() }, Vec::new(), &Mutex::new(Vec::new()));
        assert_eq!(out, Outcome::ok(None));
        assert_eq!(io.st().mutations, vec!["remove_tree /usr/share/sddm/themes/t".to_string()]);
    }

    // ── desktop as a set (DeInstallSet) ──────────────────────────────────

    fn set_req(id: u64, de: &str, packages: &[String], deps_only: &[String], replace: &[&str], update_first: bool) -> ClientMsg<PrivOp> {
        ClientMsg::Request {
            id,
            op: PrivOp::DeInstallSet {
                family: PkgFamily::Arch,
                de_id: Some(de.into()),
                packages: packages.to_vec(),
                deps_only: deps_only.to_vec(),
                replace: strings(replace),
                update_first,
            },
        }
    }

    /// The core probe by the fake's answers: what the window does before the password.
    fn preview_by(io: &FakeIo, base: &[&str]) -> plan::InstallPreview {
        let run = |cmd: &plan::Cmd| {
            let got = io.capture(&cmd.argv);
            got.code.map(|rc| (rc, got.stdout, String::new()))
        };
        plan::install_preview(PkgFamily::Arch, &strings(base), &run).expect("проба")
    }

    fn answers(io: &FakeIo, list: &[(&str, &str)]) {
        io.st().answers = list.iter().map(|(k, v)| (k.to_string(), 0, v.to_string())).collect();
    }

    /// A machine with the CachyOS kernel and our own Deepin build: `deepin-anything`
    /// pulls a prebuilt module for the stock `linux`, and the core rule (91ecc0c3)
    /// takes the DKMS variant and the headers of the installed kernel.
    fn deepin_dkms_machine(io: &FakeIo) {
        answers(io, &[
            ("pacman -Sg", "deepin deepin-anything\ndeepin treeland\ndeepin deepin-session\n"),
            // The second round of the probe, already with the DKMS variant and headers.
            (
                "pacman -Sp --needed --noconfirm --print-format %n deepin-anything treeland deepin-anything-dkms",
                "deepin-anything\ntreeland\ndeepin-anything-dkms\nlinux-cachyos-headers\n",
            ),
            ("pacman -Sp", "deepin-anything\ntreeland\ndeepin-anything-arch\nlinux\n"),
            ("pacman -Qqo", "linux-cachyos\n"),
            ("pacman -Qq", "glibc\nlinux-cachyos\ndeepin-session-treeland\n"),
            ("find /usr/lib/modules", "/usr/lib/modules/6.17.1-cachyos/pkgbase\n"),
            (
                "pacman -Si deepin-anything treeland deepin-anything-arch linux",
                "Name            : deepin-anything-arch\nDepends On      : linux\n\n\
                 Name            : linux\nDepends On      : None\n",
            ),
            ("pacman -Si", "Name            : deepin-anything-dkms\n\nName            : linux-cachyos-headers\n"),
            ("pacman -Qi", ""),
        ]);
    }

    #[test]
    fn набор_deepin_с_dkms_одним_сеансом_без_оболочки() {
        let io = FakeIo::arch();
        deepin_dkms_machine(&io);
        let p = preview_by(&io, &["deepin-anything", "treeland"]);
        let extras = strings(&["deepin-anything-dkms", "linux-cachyos-headers"]);
        assert_eq!(p.deps_only, extras);
        let b = bench(io.clone());
        b.hello();
        // Rebuilding Deepin and the set in one session: one password.
        b.send(ClientMsg::Request { id: 1, op: PrivOp::BuildDeps { names: strings(&["cmake"]) } });
        assert!(matches!(b.recv_final(), Some(HelperMsg::Done { id: 1, ok: true, .. })));
        b.send(set_req(2, "deepin", &p.targets, &p.deps_only, &[], true));
        assert_eq!(b.recv_final(), Some(HelperMsg::Done { id: 2, ok: true, code: Some(0), message_key: None }));
        assert_eq!(b.finish().ops_executed, 2);
        let runs = io.st().runs.clone();
        let mut install = strings(&["pacman", "-S", "--needed", "--noconfirm", "--noprogressbar", "--color", "never"]);
        install.extend(p.targets.iter().cloned());
        let mut mark = strings(&["pacman", "-D", "--asdeps"]);
        mark.extend(extras);
        assert_eq!(runs[1..], [strings(&["pacman", "-Sy", "--noconfirm"]), install, mark], "{runs:?}");
        assert!(io.st().mutations.is_empty());
    }

    #[test]
    fn набор_постороннее_имя_и_не_тот_dkms_отказ_до_касания() {
        let io = FakeIo::arch();
        deepin_dkms_machine(&io);
        let p = preview_by(&io, &["deepin-anything", "treeland"]);
        let b = bench(io.clone());
        b.hello();
        let mut evil = p.targets.clone();
        evil.push("nvidia-dkms".into());
        b.send(set_req(1, "deepin", &evil, &[], &[], true));
        assert_eq!(b.recv(), refused(1, "refused.not-in-table"));
        // --asdeps not on a dependency of the core rule.
        b.send(set_req(2, "deepin", &p.targets, &strings(&["treeland"]), &[], true));
        assert_eq!(b.recv(), refused(2, "refused.set-rule"));
        // The recipe-built Deepin session and the whole group: never.
        for (id, bad) in [(3, "deepin-session"), (4, "deepin"), (5, "deepin-kwin")] {
            let mut set = p.targets.clone();
            set.push(bad.into());
            b.send(set_req(id, "deepin", &set, &[], &[], false));
            assert_eq!(b.recv(), refused(id, "refused.not-in-table"), "{bad}");
        }
        // A name of another desktop.
        b.send(set_req(6, "river", &p.targets, &[], &[], false));
        assert_eq!(b.recv(), refused(6, "refused.not-in-table"));
        b.finish();
        assert!(io.touched_nothing(), "{:?}", io.st().runs);
    }

    /// River 0.5 is installed, river-classic is being installed: a replacement from the core table.
    fn river_machine(io: &FakeIo, also_kwin: bool) {
        let si = if also_kwin {
            "Name            : river-classic\nConflicts With  : river  kwin\n"
        } else {
            "Name            : river-classic\nConflicts With  : river\n"
        };
        answers(io, &[
            ("pacman -Sg", ""),
            ("pacman -Sp", "river-classic\n"),
            ("pacman -Qq", "glibc\nriver\nkwin\n"),
            ("pacman -Si", si),
            (
                "pacman -Qi",
                "Name            : river\nVersion         : 0.5.0-1\nConflicts With  : river-classic\nRequired By     : None\n\n\
                 Name            : kwin\nVersion         : 6.5.0-1\nConflicts With  : None\nRequired By     : plasma-desktop\n",
            ),
        ]);
    }

    #[test]
    fn замена_конфликтующего_только_из_таблицы_ядра() {
        let io = FakeIo::arch();
        river_machine(&io, false);
        let b = bench(io.clone());
        b.hello();
        let pkgs = strings(&["river-classic"]);
        // Without consent the conflict is not removed.
        b.send(set_req(1, "river", &pkgs, &[], &[], false));
        assert_eq!(b.recv(), refused(1, "refused.conflict"));
        // An extra name for replacement.
        b.send(set_req(2, "river", &pkgs, &[], &["river", "foo"], false));
        assert_eq!(b.recv(), refused(2, "refused.conflict"));
        // A package of the current desktop (KDE) and a permanent one: refusal before asking pacman.
        b.send(set_req(3, "river", &pkgs, &[], &["river", "konsole"], false));
        assert_eq!(b.recv(), refused(3, "refused.current-desktop-package"));
        b.send(set_req(4, "river", &pkgs, &[], &["glibc"], false));
        assert_eq!(b.recv(), refused(4, "refused.vital"));
        assert!(io.touched_nothing());
        // An agreed replacement: one transaction with --ask=4.
        b.send(set_req(5, "river", &pkgs, &[], &["river"], false));
        assert_eq!(b.recv_final(), Some(HelperMsg::Done { id: 5, ok: true, code: Some(0), message_key: None }));
        b.finish();
        assert_eq!(
            io.st().runs,
            vec![strings(&[
                "pacman", "-S", "--needed", "--noconfirm", "--noprogressbar", "--color", "never", "--ask=4", "river-classic"
            ])]
        );
        // A foreign conflict next to it (kwin): the replacement will not help, --ask=4 would remove KDE.
        let io = FakeIo::arch();
        river_machine(&io, true);
        let b = bench(io.clone());
        b.hello();
        b.send(set_req(1, "river", &pkgs, &[], &["river"], false));
        assert_eq!(b.recv(), refused(1, "refused.conflict"));
        b.finish();
        assert!(io.touched_nothing());
    }

    #[test]
    fn набор_по_семейству_и_на_steamos_отказ() {
        let pkgs = strings(&["river-classic"]);
        let op = |family, de: Option<&str>| PrivOp::DeInstallSet {
            family,
            de_id: de.map(String::from),
            packages: pkgs.clone(),
            deps_only: vec![],
            replace: vec![],
            update_first: false,
        };
        assert_eq!(check_op(&op(PkgFamily::Debian, Some("river")), &facts(), 0).unwrap_err().key, "refused.family");
        assert_eq!(check_op(&op(PkgFamily::Arch, None), &facts(), 0).unwrap_err().key, "refused.unknown-desktop");
        let steam = Facts { steamos: true, ..facts() };
        assert_eq!(check_op(&op(PkgFamily::Arch, Some("river")), &steam, 0).unwrap_err().key, "refused.read-only-system");
        assert!(check_op(&op(PkgFamily::Arch, Some("river")), &facts(), 0).is_ok());
    }

    // ── desktop image on SteamOS (Sysext) ────────────────────────────────

    fn steamos_io(desktop: &str) -> Arc<FakeIo> {
        use crate::steamos_sysext as sx;
        let io = FakeIo::arch();
        {
            let mut s = io.st();
            s.files.insert("/etc/os-release".into(), "ID=steamos\nNAME=\"SteamOS\"\nVERSION_ID=3.8.14\n".into());
            s.loginctl = format!("User={}\nActive=yes\nDesktop={desktop}\n", unsafe { libc::getuid() });
            s.exe.insert(format!("{}/rexon-sysext-sync", sx::BIN_DIR));
        }
        io
    }

    fn sysext_req(id: u64, op: SysextOp, de: &str) -> ClientMsg<PrivOp> {
        ClientMsg::Request { id, op: PrivOp::Sysext { op, de_id: de.into() } }
    }

    /// Progress up to the result: lines and the result.
    fn progress_and_final(b: &Bench) -> (Vec<String>, Option<HelperMsg>) {
        let mut lines = Vec::new();
        loop {
            match b.recv() {
                Some(HelperMsg::Progress { line, .. }) => lines.push(line),
                other => return (lines, other),
            }
        }
    }

    #[test]
    fn steamos_образ_стола_ставится_без_оболочки() {
        use crate::steamos_sysext as sx;
        let io = steamos_io("KDE");
        let b = bench(io.clone());
        b.hello();
        b.send(sysext_req(1, SysextOp::Install, "xfce"));
        let (lines, last) = progress_and_final(&b);
        assert_eq!(last, Some(HelperMsg::Done { id: 1, ok: true, code: Some(0), message_key: None }));
        assert_eq!(lines.first().map(String::as_str), Some("@progress 1"));
        assert_eq!(lines.last().map(String::as_str), Some("@progress 100"));
        b.finish();
        let s = io.st();
        // Texts from its own binary, each file replaced with one rename.
        for f in sx::layout_files() {
            assert_eq!(s.files.get(&f.path), Some(&f.text), "{}", f.path);
            assert!(s.mutations.contains(&format!("rename {}.rexon-new {}", f.path, f.path)), "{}", f.path);
        }
        assert_eq!(s.files.get(sx::HELPER_PATH), Some(&sx::helper_script()));
        assert_eq!(s.files.get(sx::LANG_FILE).map(String::as_str), Some("en\n"));
        assert_eq!(s.files.get(sx::DESKTOPS_LIST).map(String::as_str), Some("xfce\n"));
        // /usr on SteamOS is read-only: not a single write there.
        assert!(!s.mutations.iter().any(|m| m.contains(" /usr/")), "{:?}", s.mutations);
        assert_eq!(
            s.runs,
            vec![
                strings(&["systemctl", "daemon-reload"]),
                strings(&["systemctl", "enable", "systemd-sysext.service", sx::GUARD_UNIT, sx::SYNC_UNIT]),
                vec![format!("{}/rexon-sysext-sync", sx::BIN_DIR), "--interactive".into(), "--only".into(), "xfce".into()],
            ]
        );
    }

    #[test]
    fn steamos_несобравшийся_стол_не_остаётся_в_списке() {
        use crate::steamos_sysext as sx;
        let io = steamos_io("KDE");
        io.st().fail_runs = vec![format!("{}/rexon-sysext-sync --interactive --only xfce", sx::BIN_DIR)];
        io.st().files.insert(sx::DESKTOPS_LIST.into(), "lxqt\n".into());
        let b = bench(io.clone());
        b.hello();
        b.send(sysext_req(1, SysextOp::Install, "xfce"));
        assert_eq!(b.recv_final(), Some(HelperMsg::Done { id: 1, ok: false, code: Some(1), message_key: None }));
        b.finish();
        let s = io.st();
        assert_eq!(s.files.get(sx::DESKTOPS_LIST).map(String::as_str), Some("lxqt\n"));
        let last = s.runs.last().unwrap().clone();
        assert_eq!(last[1..], strings(&["--interactive", "--only", "none"]));
    }

    #[test]
    fn steamos_снятие_стола() {
        use crate::steamos_sysext as sx;
        let io = steamos_io("KDE");
        {
            let mut s = io.st();
            s.files.insert(sx::DESKTOPS_LIST.into(), "xfce\nlxqt\n".into());
            s.files.insert(sx::WANTED_SESSION.into(), "rexon-xfce.desktop\n".into());
            s.files.insert(format!("{}/rexon-xfce.raw", sx::EXTENSIONS_DIR), String::new());
            s.files.insert(format!("{}/rexon-xfce-3.8.14.raw", sx::IMAGE_DIR), String::new());
            s.files.insert(format!("{}/rexon-lxqt-3.8.14.raw", sx::IMAGE_DIR), String::new());
            s.dirs.insert(format!("{}/xfce-3.8.14", sx::BUILD_DIR));
        }
        let b = bench(io.clone());
        b.hello();
        b.send(sysext_req(1, SysextOp::Remove, "xfce"));
        assert!(matches!(b.recv_final(), Some(HelperMsg::Done { id: 1, ok: true, .. })));
        b.finish();
        let s = io.st();
        assert_eq!(s.files.get(sx::DESKTOPS_LIST).map(String::as_str), Some("lxqt\n"));
        assert!(!s.files.contains_key(sx::WANTED_SESSION));
        assert!(!s.files.contains_key(&format!("{}/rexon-xfce.raw", sx::EXTENSIONS_DIR)));
        assert!(!s.files.contains_key(&format!("{}/rexon-xfce-3.8.14.raw", sx::IMAGE_DIR)));
        assert!(s.files.contains_key(&format!("{}/rexon-lxqt-3.8.14.raw", sx::IMAGE_DIR)));
        assert!(s.runs.contains(&strings(&["rm", "-rf", "--one-file-system", "--", "/home/.rexon/build/xfce-3.8.14"])));
        assert!(s.runs.contains(&vec![format!("{}/session-guard.sh", sx::BIN_DIR)]));
    }

    #[test]
    fn steamos_проверки_образа_стола() {
        // Not SteamOS: refusal; a desktop not from the table; removing the current one.
        let io = FakeIo::arch();
        let b = bench(io.clone());
        b.hello();
        b.send(sysext_req(1, SysextOp::Install, "xfce"));
        assert_eq!(b.recv(), refused(1, "refused.not-steamos"));
        b.finish();
        assert!(io.touched_nothing());
        let io = steamos_io("XFCE");
        let b = bench(io.clone());
        b.hello();
        b.send(sysext_req(1, SysextOp::Install, "mate"));
        assert_eq!(b.recv(), refused(1, "refused.unknown-desktop"));
        b.send(sysext_req(2, SysextOp::Remove, "xfce"));
        assert_eq!(b.recv(), refused(2, "refused.current-desktop"));
        // Packages on SteamOS are still not touched.
        b.send(install(3, &["sddm"]));
        assert_eq!(b.recv(), refused(3, "refused.read-only-system"));
        b.finish();
        assert!(io.touched_nothing());
    }

    // ── guard Z1 ─────────────────────────────────────────────────────────

    #[test]
    fn сторож_нет_оболочки_в_privilege() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/src/privilege");
        // The needles are assembled from parts so the guard does not find itself.
        let needles = [
            format!("{}{}", "sh ", "-c"),
            format!("\"{}\"", ["ba", "sh"].concat()),
            format!("/{}", ["ba", "sh"].concat()),
            format!("Command::new(\"{}\")", "sh"),
            format!("Command::new(\"/bin/{}\")", "sh"),
        ];
        // A line with this mark is data (names), not a launch.
        let allow = ["сторож:", " имена, не запуск"].concat();
        let eval_word = ["ev", "al"].concat();
        let mut hits = Vec::new();
        let mut files = 0;
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            files += 1;
            let text = std::fs::read_to_string(&path).unwrap();
            for (n, line) in text.lines().enumerate() {
                if line.contains(allow.as_str()) {
                    continue;
                }
                let code = line.split("//").next().unwrap_or("");
                let has_eval = code
                    .split(|c: char| !(c.is_alphanumeric() || c == '_'))
                    .any(|w| w == eval_word);
                if has_eval || needles.iter().any(|nd| code.contains(nd.as_str())) {
                    hits.push(format!("{}:{}: {}", path.display(), n + 1, line.trim()));
                }
            }
        }
        assert!(files >= 5, "каталог privilege не найден");
        assert!(hits.is_empty(), "оболочка в privilege/:\n{}", hits.join("\n"));
    }

    #[test]
    fn тайм_аут_стенда_разумный() {
        // The bench relies on SO_RCVTIMEO: check that an empty socket does not hang.
        let b = bench(FakeIo::arch());
        let t = Instant::now();
        b.hello();
        assert!(t.elapsed() < Duration::from_secs(5));
        b.finish();
    }

    // ── Fonts for all users ──────────────────────────────────────────────

    const FONTS: &str = "/usr/local/share/fonts";

    fn font_op(names: &[&str]) -> PrivOp {
        PrivOp::FontInstall {
            fonts: names
                .iter()
                .enumerate()
                .map(|(i, n)| crate::privilege::op::FontFile { source: local(i as u8, &format!("/home/test/{n}")), name: n.to_string() })
                .collect(),
        }
    }

    fn font_io() -> Arc<FakeIo> {
        let io = FakeIo::arch();
        {
            let mut s = io.st();
            s.exe.insert("/usr/bin/fc-query".into());
            s.exe.insert("/usr/bin/fc-cache".into());
            s.answers.push(("fc-query".into(), 0, "Stand Sans\n".into()));
        }
        io
    }

    /// The check before execution: only font file names, descriptors exactly per
    /// file, not on NixOS and not on SteamOS.
    #[test]
    fn шрифты_для_всех_проверка_операции() {
        let f = facts();
        assert_eq!(
            check_op(&font_op(&["a.ttf", "Мой шрифт.otf"]), &f, 2),
            Ok(Plan::FontInstall { names: strings(&["a.ttf", "Мой шрифт.otf"]) })
        );
        assert_eq!(check_op(&font_op(&["a.ttf"]), &f, 0).unwrap_err().key, "fds_mismatch");
        assert_eq!(check_op(&font_op(&["../a.ttf"]), &f, 1).unwrap_err().key, "refused.font-file");
        assert_eq!(check_op(&font_op(&["evil.sh"]), &f, 1).unwrap_err().key, "refused.font-file");
        assert_eq!(check_op(&font_op(&[]), &f, 0).unwrap_err().key, "refused.empty");
        let nix = Facts { family: PkgFamily::Nix, ..facts() };
        assert_eq!(check_op(&font_op(&["a.ttf"]), &nix, 1).unwrap_err().key, "refused.family");
        let deck = Facts { steamos: true, ..facts() };
        assert_eq!(check_op(&font_op(&["a.ttf"]), &deck, 1).unwrap_err().key, "refused.read-only-system");
        let rm = |files: &[&str]| PrivOp::FontRemove { files: strings(files) };
        assert_eq!(
            check_op(&rm(&["/usr/local/share/fonts/a.ttf"]), &f, 0),
            Ok(Plan::FontRemove { files: strings(&["/usr/local/share/fonts/a.ttf"]) })
        );
        assert_eq!(check_op(&rm(&["/usr/share/fonts/noto/a.ttf"]), &f, 0).unwrap_err().key, "refused.outside-root");
        assert_eq!(check_op(&rm(&["/usr/local/share/fonts/a.ttf"]), &f, 1).unwrap_err().key, "fds_mismatch");
        assert_eq!(executes(&font_op(&["a.ttf"])), Ok(()));
        assert_eq!(executes(&rm(&["/usr/local/share/fonts/a.ttf"])), Ok(()));
    }

    #[test]
    fn шрифт_для_всех_ложится_копией_и_кэш_пересобирается() {
        let io = font_io();
        let plan_ = Plan::FontInstall { names: strings(&["Мой шрифт.ttf"]) };
        let out = run_plan(&io, &plan_, vec![user_file("font-ok", b"font-bytes")], &Mutex::new(Vec::new()));
        assert_eq!(out, Outcome::ok(None));
        let s = io.st();
        assert_eq!(s.files[&format!("{FONTS}/Мой шрифт.ttf")], "font-bytes");
        assert!(s.mutations.iter().any(|m| m == &format!("mkdir {FONTS}")), "{:?}", s.mutations);
        assert!(s.mutations.iter().all(|m| m.starts_with("mkdir ") || m.ends_with(&format!(" {FONTS}/Мой шрифт.ttf"))), "{:?}", s.mutations);
        assert_eq!(s.runs, vec![strings(&["fc-cache", "-f", FONTS])]);
    }

    #[test]
    fn не_шрифт_отказ_до_касания() {
        let io = font_io();
        io.st().answers.clear();
        io.st().answers.push(("fc-query".into(), 1, String::new()));
        let plan_ = Plan::FontInstall { names: strings(&["a.ttf"]) };
        let out = run_plan(&io, &plan_, vec![user_file("font-bad", b"#!/bin/sh")], &Mutex::new(Vec::new()));
        assert_eq!(out, Outcome::Refused("refused.not-a-font".into()));
        assert!(io.touched_nothing());
        // fontconfig is silent (code 0, empty): not a font either.
        let io = font_io();
        io.st().answers.clear();
        let out = run_plan(&io, &plan_, vec![user_file("font-empty", b"x")], &Mutex::new(Vec::new()));
        assert_eq!(out, Outcome::Refused("refused.not-a-font".into()));
        assert!(io.touched_nothing());
        // No fontconfig at all: refusal, without copies.
        let io = font_io();
        io.st().exe.remove("/usr/bin/fc-query");
        let out = run_plan(&io, &plan_, vec![user_file("font-nofc", b"x")], &Mutex::new(Vec::new()));
        assert_eq!(out, Outcome::Refused("refused.no-fontconfig".into()));
        assert!(io.touched_nothing());
    }

    #[test]
    fn чужой_шрифт_с_тем_же_именем_не_перезаписывается() {
        let io = font_io();
        io.st().files.insert(format!("{FONTS}/b.ttf"), "чужой".into());
        // The second file is taken: the first is not placed either (the check comes before touching).
        let plan_ = Plan::FontInstall { names: strings(&["a.ttf", "b.ttf"]) };
        let fds = vec![user_file("font-a", b"a"), user_file("font-b", b"b")];
        let out = run_plan(&io, &plan_, fds, &Mutex::new(Vec::new()));
        assert_eq!(out, Outcome::Refused("refused.font-exists".into()));
        assert!(io.touched_nothing());
        assert_eq!(io.st().files[&format!("{FONTS}/b.ttf")], "чужой");
        // A link in place of the destination: also a refusal (we do not write through it).
        let io = font_io();
        io.st().links.insert(format!("{FONTS}/c.ttf"), "/etc/shadow".into());
        let out = run_plan(&io, &Plan::FontInstall { names: strings(&["c.ttf"]) }, vec![user_file("font-c", b"c")], &Mutex::new(Vec::new()));
        assert_eq!(out, Outcome::Refused("refused.font-exists".into()));
        assert!(io.touched_nothing());
    }

    #[test]
    fn удаление_шрифта_для_всех() {
        let io = font_io();
        {
            let mut s = io.st();
            s.dirs.insert(FONTS.into());
            s.dirs.insert(format!("{FONTS}/sub"));
            s.files.insert(format!("{FONTS}/a.ttf"), "a".into());
            s.files.insert(format!("{FONTS}/sub/b.otf"), "b".into());
        }
        let plan_ = Plan::FontRemove { files: vec![format!("{FONTS}/a.ttf"), format!("{FONTS}/sub/b.otf")] };
        assert_eq!(run_plan(&io, &plan_, Vec::new(), &Mutex::new(Vec::new())), Outcome::ok(None));
        let s = io.st();
        assert!(!s.files.contains_key(&format!("{FONTS}/a.ttf")) && !s.files.contains_key(&format!("{FONTS}/sub/b.otf")));
        assert!(!s.dirs.contains(&format!("{FONTS}/sub")), "опустевший подкаталог снят");
        assert!(s.dirs.contains(FONTS), "сам каталог остаётся");
        assert_eq!(s.runs, vec![strings(&["fc-cache", "-f", FONTS])]);
    }

    #[test]
    fn удаление_шрифта_через_ссылку_отказ() {
        // A file link and a subdirectory link pointing outside: refusal before the first removal.
        let io = font_io();
        {
            let mut s = io.st();
            s.dirs.insert(FONTS.into());
            s.files.insert(format!("{FONTS}/ok.ttf"), "ok".into());
            s.files.insert("/usr/share/fonts/noto/N.ttf".into(), "пакет".into());
            s.links.insert(format!("{FONTS}/link.ttf"), "/usr/share/fonts/noto/N.ttf".into());
            s.dirs.insert("/usr/share/fonts/noto".into());
            s.links.insert(format!("{FONTS}/pkg"), "/usr/share/fonts/noto".into());
        }
        for evil in [format!("{FONTS}/link.ttf"), format!("{FONTS}/pkg/N.ttf"), format!("{FONTS}/missing.ttf")] {
            let plan_ = Plan::FontRemove { files: vec![format!("{FONTS}/ok.ttf"), evil.clone()] };
            assert_eq!(
                run_plan(&io, &plan_, Vec::new(), &Mutex::new(Vec::new())),
                Outcome::Refused("refused.font-file".into()),
                "{evil}"
            );
            assert!(io.touched_nothing(), "{evil}");
        }
        assert_eq!(io.st().files["/usr/share/fonts/noto/N.ttf"], "пакет");
    }

    // ── SDDM login screen: theme background and look carry-over ──────────

    const BREEZE: &str = "/usr/share/sddm/themes/breeze";
    const PNG: &[u8] = b"\x89PNG\r\n\x1a\npicture";

    fn sddm_io() -> Arc<FakeIo> {
        let io = FakeIo::arch();
        {
            let mut s = io.st();
            s.dirs.insert(BREEZE.into());
            s.files.insert(format!("{BREEZE}/metadata.desktop"), "[SddmGreeterTheme]\nName=Breeze\nConfigFile=theme.conf\n".into());
            s.files.insert(format!("{BREEZE}/theme.conf"), "[General]\ntype=image\nbackground=/usr/share/wallpapers/Next.png\n".into());
            s.dirs.insert("/var/lib/sddm".into());
            s.exe.insert("/usr/bin/setpriv".into());
            s.answers.push(("getent passwd sddm".into(), 0, "sddm:x:955:955:SDDM Greeter Account:/var/lib/sddm:/usr/bin/nologin\n".into()));
            s.files.insert("/etc/sddm.conf.d/kde_settings.conf".into(), "[Theme]\nCurrent=breeze\nCursorTheme=breeze_cursors\n".into());
            s.files.insert("/usr/lib/sddm/sddm.conf.d/default.conf".into(), "[X11]\nServerArguments=-nolisten tcp\n".into());
        }
        io
    }

    fn look_op(files: &[&str]) -> PrivOp {
        PrivOp::SddmLook {
            look: Some(plan::SddmLook { cursor_theme: Some("Bibata".into()), dpi: Some(120), ..Default::default() }),
            files: files
                .iter()
                .enumerate()
                .map(|(i, n)| crate::privilege::op::LookFile { source: local(i as u8, &format!("/home/test/.config/{n}")), name: n.to_string() })
                .collect(),
        }
    }

    #[test]
    fn экран_входа_проверка_операции() {
        let f = facts();
        let bg = |theme: &str, image: bool| PrivOp::SddmBackground { theme: theme.into(), image: image.then(|| local(0, "/home/test/a.png")) };
        assert_eq!(check_op(&bg("breeze", true), &f, 1), Ok(Plan::SddmBackground { theme: "breeze".into(), image: true }));
        assert_eq!(check_op(&bg("breeze", false), &f, 0), Ok(Plan::SddmBackground { theme: "breeze".into(), image: false }));
        assert_eq!(check_op(&bg("breeze", true), &f, 0).unwrap_err().key, "fds_mismatch");
        assert_eq!(check_op(&bg("../../etc", false), &f, 0).unwrap_err().key, "refused.item-name");
        let deck = Facts { steamos: true, ..facts() };
        assert_eq!(check_op(&bg("breeze", false), &deck, 0).unwrap_err().key, "refused.read-only-system");
        let clock = |theme: &str| PrivOp::SddmClock { theme: theme.into(), show: false };
        assert_eq!(check_op(&clock("breeze"), &f, 0), Ok(Plan::SddmClock { theme: "breeze".into(), show: false }));
        assert_eq!(check_op(&clock("../../etc"), &f, 0).unwrap_err().key, "refused.item-name");
        assert_eq!(check_op(&clock("breeze"), &deck, 0).unwrap_err().key, "refused.read-only-system");
        assert!(check_op(&look_op(&["kdeglobals", "fontconfig/fonts.conf"]), &f, 2).is_ok());
        assert_eq!(check_op(&look_op(&["../.bashrc"]), &f, 1).unwrap_err().key, "refused.look-file");
        assert_eq!(check_op(&look_op(&["kdeglobals", "kdeglobals"]), &f, 2).unwrap_err().key, "refused.look-file");
        let bad = PrivOp::SddmLook { look: Some(plan::SddmLook { font: Some("A\n[X11]\nServerPath=/tmp/x".into()), ..Default::default() }), files: Vec::new() };
        assert_eq!(check_op(&bad, &f, 0).unwrap_err().key, "refused.look-value");
        let dpi = PrivOp::SddmLook { look: Some(plan::SddmLook { dpi: Some(0), ..Default::default() }), files: Vec::new() };
        assert_eq!(check_op(&dpi, &f, 0).unwrap_err().key, "refused.look-value");
        let reset_with_file = PrivOp::SddmLook {
            look: None,
            files: vec![crate::privilege::op::LookFile { source: local(0, "/h/k"), name: "kdeglobals".into() }],
        };
        assert_eq!(check_op(&reset_with_file, &f, 1).unwrap_err().key, "refused.look-file");
    }

    #[test]
    fn фон_экрана_входа_ложится_в_тему() {
        let io = sddm_io();
        let plan_ = Plan::SddmBackground { theme: "breeze".into(), image: true };
        io.st().files.insert(format!("{BREEZE}/rexon-background.jpg"), "прежний наш".into());
        let out = run_plan(&io, &plan_, vec![user_file("bg-png", PNG)], &Mutex::new(Vec::new()));
        assert_eq!(out, Outcome::ok(None));
        let s = io.st();
        assert!(s.files.contains_key(&format!("{BREEZE}/rexon-background.png")));
        assert!(!s.files.contains_key(&format!("{BREEZE}/rexon-background.jpg")), "прежний свой снят");
        assert_eq!(s.files[&format!("{BREEZE}/theme.conf.user")], "[General]\nbackground=rexon-background.png\ntype=image\n");
        assert!(s.runs.is_empty());
    }

    #[test]
    fn фон_не_картинка_или_чужая_тема_отказ_до_касания() {
        let io = sddm_io();
        let plan_ = Plan::SddmBackground { theme: "breeze".into(), image: true };
        let out = run_plan(&io, &plan_, vec![user_file("bg-sh", b"#!/bin/sh\nid\n")], &Mutex::new(Vec::new()));
        assert_eq!(out, Outcome::Refused("refused.login-image".into()));
        assert!(io.touched_nothing());
        // No theme, a theme without a settings file, `.user` is a link.
        let io = sddm_io();
        let none = Plan::SddmBackground { theme: "nope".into(), image: false };
        assert_eq!(run_plan(&io, &none, Vec::new(), &Mutex::new(Vec::new())), Outcome::Refused("refused.sddm-theme".into()));
        io.st().links.insert(format!("{BREEZE}/theme.conf.user"), "/etc/shadow".into());
        let reset = Plan::SddmBackground { theme: "breeze".into(), image: false };
        assert_eq!(run_plan(&io, &reset, Vec::new(), &Mutex::new(Vec::new())), Outcome::Refused("refused.sddm-theme".into()));
        assert!(io.touched_nothing());
    }

    #[test]
    fn фон_темы_возвращается() {
        let io = sddm_io();
        {
            let mut s = io.st();
            s.files.insert(format!("{BREEZE}/theme.conf.user"), "[General]\nshowClock=false\nbackground=rexon-background.png\ntype=image\n".into());
            s.files.insert(format!("{BREEZE}/rexon-background.png"), "x".into());
            s.files.insert(format!("{BREEZE}/kcm-picked.jpg"), "чужой".into());
        }
        let plan_ = Plan::SddmBackground { theme: "breeze".into(), image: false };
        assert_eq!(run_plan(&io, &plan_, Vec::new(), &Mutex::new(Vec::new())), Outcome::ok(None));
        let s = io.st();
        assert_eq!(s.files[&format!("{BREEZE}/theme.conf.user")], "[General]\nshowClock=false\n");
        assert!(!s.files.contains_key(&format!("{BREEZE}/rexon-background.png")));
        assert!(s.files.contains_key(&format!("{BREEZE}/kcm-picked.jpg")), "чужой файл не трогается");
    }

    #[test]
    fn перенос_оформления_от_имени_sddm() {
        let io = sddm_io();
        let plan_ = Plan::SddmLook {
            look: Some(plan::SddmLook { cursor_theme: Some("Bibata".into()), dpi: Some(120), ..Default::default() }),
            names: strings(&["kdeglobals"]),
        };
        let out = run_plan(&io, &plan_, vec![user_file("look-kg", b"[Colors:Window]\nBackgroundNormal=1,2,3\n")], &Mutex::new(Vec::new()));
        assert_eq!(out, Outcome::ok(None));
        let s = io.st();
        let kde = &s.files["/etc/sddm.conf.d/kde_settings.conf"];
        assert!(kde.contains("CursorTheme=Bibata") && kde.contains("ServerArguments=-nolisten tcp -dpi 120"), "{kde}");
        let sudo = strings(&["setpriv", "--reuid=955", "--regid=955", "--clear-groups", "--"]);
        assert!(s.runs.iter().all(|r| r[..5] == sudo[..]), "всё в доме sddm — от его имени: {:?}", s.runs);
        assert_eq!(s.runs[0][5..], strings(&["rm", "-rf", "--", "/var/lib/sddm/.cache"]));
        assert_eq!(s.runs[2][5..], strings(&["sh", "-c", "umask 022 && cat > \"$1\"", "rexon-sddm-look", "/var/lib/sddm/.config/kdeglobals"]));
        assert_eq!(s.stdins[2].as_deref(), Some("[Colors:Window]\nBackgroundNormal=1,2,3\n"));
    }

    #[test]
    fn перенос_не_вышел_ключи_возвращаются() {
        let io = sddm_io();
        io.st().fail_runs.push("setpriv --reuid=955 --regid=955 --clear-groups -- sh".into());
        let before = io.st().files["/etc/sddm.conf.d/kde_settings.conf"].clone();
        let plan_ = Plan::SddmLook { look: Some(plan::SddmLook { cursor_theme: Some("Bibata".into()), ..Default::default() }), names: strings(&["kdeglobals"]) };
        let out = run_plan(&io, &plan_, vec![user_file("look-fail", b"[General]\n")], &Mutex::new(Vec::new()));
        assert!(matches!(out, Outcome::Done { ok: false, .. }), "{out:?}");
        assert_eq!(io.st().files["/etc/sddm.conf.d/kde_settings.conf"], before);
        // Not text: refusal before touching.
        let io = sddm_io();
        let out = run_plan(&io, &plan_, vec![user_file("look-bin", b"\xff\xfe\x00bin")], &Mutex::new(Vec::new()));
        assert_eq!(out, Outcome::Refused("refused.look-file".into()));
        assert!(io.touched_nothing());
        // No sddm account / no setpriv: refusal before touching.
        let io = sddm_io();
        io.st().answers.clear();
        io.st().answers.push(("getent passwd sddm".into(), 2, String::new()));
        let reset = Plan::SddmLook { look: None, names: Vec::new() };
        assert_eq!(run_plan(&io, &reset, Vec::new(), &Mutex::new(Vec::new())), Outcome::Refused("refused.no-sddm-user".into()));
        let io = sddm_io();
        io.st().exe.remove("/usr/bin/setpriv");
        assert_eq!(run_plan(&io, &reset, Vec::new(), &Mutex::new(Vec::new())), Outcome::Refused("refused.no-setpriv".into()));
        assert!(io.touched_nothing());
    }

    #[test]
    fn сброс_оформления_экрана_входа() {
        let io = sddm_io();
        io.st().files.insert(
            "/etc/sddm.conf.d/kde_settings.conf".into(),
            "[Theme]\nCurrent=breeze\nCursorTheme=Bibata\nFont=Inter,11\n\n[X11]\nServerArguments=-nolisten tcp -dpi 120\n".into(),
        );
        let reset = Plan::SddmLook { look: None, names: Vec::new() };
        assert_eq!(run_plan(&io, &reset, Vec::new(), &Mutex::new(Vec::new())), Outcome::ok(None));
        let s = io.st();
        assert_eq!(s.files["/etc/sddm.conf.d/kde_settings.conf"], "[Theme]\nCurrent=breeze\n\n");
        assert_eq!(s.runs.len(), 1);
        assert!(s.runs[0].contains(&"/var/lib/sddm/.config/kdeglobals".to_string()) && s.runs[0].contains(&"/var/lib/sddm/.local/share/kscreen".to_string()));
    }
}
