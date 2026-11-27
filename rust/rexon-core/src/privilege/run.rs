//! The single entry point of a privileged operation (item B8 of the plan, see
//! [`super`]).
//!
//! [`run`] is what the window's executors call instead of `pkexec`: the
//! operation ([`PrivOp`]) goes to the privilege session helper, the session is
//! raised on the first request and then lives as one per window process (one
//! password). The client is a process singleton behind a mutex: the thread
//! waiting for the result does not hold the mutex, so "Revoke now"
//! ([`shutdown`]) from another thread does not wait for the operation to end.
//!
//! # When the old path is used (rule Z5)
//!
//! | Client result | [`RunOutcome`] | Old path |
//! |---|---|---|
//! | executed (`Done`, success or not) | [`RunOutcome::Done`] | no |
//! | no helper / unacceptable owner | [`RunOutcome::Legacy`] ([`LegacyReason::HelperUnavailable`]) | **yes** |
//! | the helper does not execute this operation (`not_supported_yet`, `not_for_helper`) | [`RunOutcome::Legacy`] ([`LegacyReason::NotSupported`]) | **yes** |
//! | check refusal (`Refused` with other keys) | [`RunOutcome::Failed`] | no |
//! | password not confirmed (126/127) | [`RunOutcome::Failed`] | no: a second password dialog after the person's refusal |
//! | `BuildMismatch` | [`RunOutcome::Failed`] | no: "Rexon was updated — restart it" |
//! | `Revoked`, `SessionLost`, `Closed`, `Busy`, `Spawn`, `HelperFailed` | [`RunOutcome::Failed`] | no |
//! | `Cancelled`, `InvalidRequest` | [`RunOutcome::Failed`] | no |
//!
//! [`run`] does NOT execute the old path itself: it returns
//! [`RunOutcome::Legacy`], and the caller goes through its own old executor
//! (`io.run`, `run_logged`, a script via `pkexec bash`) — with the same argv as
//! before "B". For places without their own argv there is [`legacy_command`]
//! (`PrivOp::pkexec_argv` / a script). There is no `pkexec` launch of its own
//! here.
//!
//! An operation the helper is known not to execute
//! ([`super::helper::executes`]) is handed by [`run`] to the old path without
//! raising the session: otherwise the person would enter the password for a
//! refusal and then once more.

use std::os::fd::OwnedFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock, TryLockError};

use super::client::{Client, ClientError, Outcome, Pending};
use super::helper;
use super::op::{Helpers, PrivOp};
use super::revoke::Reason;

// ============================================================================
// Seam to the client
// ============================================================================

/// Waiting for the result of a submitted operation.
pub trait Waiter: Send {
    fn wait(self: Box<Self>, on_progress: &mut dyn FnMut(&str)) -> Result<Outcome, ClientError>;
}

/// The privilege session client as [`run`] sees it. Live — [`Client`]; in tests
/// — a fake.
pub trait Backend: Send {
    /// Submit an operation (raises the session if there is none).
    fn submit(&mut self, op: &PrivOp, fds: Vec<OwnedFd>) -> Result<Box<dyn Waiter>, ClientError>;
    /// There is a live session.
    fn has_session(&self) -> bool;
    /// Close the session (window closed, "Revoke now").
    fn shutdown(&mut self);
}

impl Waiter for Pending {
    fn wait(self: Box<Self>, on_progress: &mut dyn FnMut(&str)) -> Result<Outcome, ClientError> {
        Pending::wait(*self, on_progress)
    }
}

impl Backend for Client {
    fn submit(&mut self, op: &PrivOp, fds: Vec<OwnedFd>) -> Result<Box<dyn Waiter>, ClientError> {
        Client::submit(self, op, fds).map(|p| Box::new(p) as Box<dyn Waiter>)
    }

    fn has_session(&self) -> bool {
        Client::has_session(self)
    }

    fn shutdown(&mut self) {
        Client::shutdown(self)
    }
}

// ============================================================================
// Result
// ============================================================================

/// Why the operation goes the old way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LegacyReason {
    /// There is no session helper or it is unacceptable (owner, permissions):
    /// [`ClientError::raw_fallback_allowed`].
    HelperUnavailable(ClientError),
    /// The helper does not execute such an operation: key
    /// [`helper::NOT_SUPPORTED_YET`] or [`helper::NOT_FOR_HELPER`].
    NotSupported(String),
}

/// Result of [`run`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunOutcome {
    /// The helper executed the operation (success — `ok`).
    Done(Outcome),
    /// Go the old way — through the caller's executor, as before "B".
    Legacy(LegacyReason),
    /// Not executed (or the outcome is unknown); there is NO old path (Z5).
    Failed(ClientError),
}

impl RunOutcome {
    /// The old path is allowed.
    pub fn is_legacy(&self) -> bool {
        matches!(self, RunOutcome::Legacy(_))
    }

    /// The result in the form of a command executor's answer — for places that then
    /// parse the code and output of `pkexec` ([`crate::system_exec::RunOut`]).
    /// `None` — [`RunOutcome::Legacy`]: let the executor run the old command itself.
    /// The helper's progress lines go to `stdout`.
    ///
    /// Executed — the program's code, `stderr` empty: the helper has already sent
    /// the explanation as a progress line in the window's language, and the "last
    /// line is the reason" parsing takes it from `stdout`. A password refusal is
    /// returned with a `pkexec` code (126/127): the callers already have an
    /// explanation for it that the person understands. Other refusals — code 1 and
    /// the error text in `stderr`.
    pub fn to_run_out(&self, stdout: String) -> Option<crate::system_exec::RunOut> {
        use crate::utils::RunOutcome as Rc;
        let (outcome, returncode, stderr) = match self {
            RunOutcome::Legacy(_) => return None,
            RunOutcome::Done(o) => (if o.ok { Rc::Ok } else { Rc::Failed }, o.code, String::new()),
            RunOutcome::Failed(ClientError::NotAuthorized { code }) => (Rc::Failed, Some(*code), self.failure_text()),
            RunOutcome::Failed(ClientError::Spawn(_)) => (Rc::Error, None, self.failure_text()),
            RunOutcome::Failed(_) => (Rc::Failed, Some(1), self.failure_text()),
        };
        Some(crate::system_exec::RunOut { outcome, returncode, stdout, stderr })
    }

    /// The refusal text for the person in the window's language
    /// ([`ClientError::human_text`], item B13). The technical line (`Display`) goes
    /// to the log: it would tell the person nothing.
    fn failure_text(&self) -> String {
        match self {
            RunOutcome::Failed(e) => {
                crate::log_warn!("privilege", "операция не исполнена: {e}");
                e.human_text()
            }
            _ => String::new(),
        }
    }
}

/// The old-path command, if the caller has none of its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LegacyCommand {
    /// argv (`pkexec <old helper> …` or plain `pkexec …`).
    Argv(Vec<String>),
    /// `pkexec bash <file>`: the temporary file prefix and the script text.
    Script { prefix: &'static str, text: String },
}

/// The operation's old path: [`PrivOp::pkexec_argv`], otherwise
/// [`PrivOp::fallback_script`]. `None` — there is no old path (Z5: `switch-dm`,
/// system themes without a helper) or it is not a single command.
pub fn legacy_command(op: &PrivOp, helpers: &Helpers) -> Option<LegacyCommand> {
    if let Some(argv) = op.pkexec_argv(helpers) {
        return Some(LegacyCommand::Argv(argv));
    }
    op.fallback_script().map(|(prefix, text)| LegacyCommand::Script { prefix, text })
}

/// Interpreting a client error per rule Z5.
fn classify(err: ClientError) -> RunOutcome {
    if err.raw_fallback_allowed() {
        return RunOutcome::Legacy(LegacyReason::HelperUnavailable(err));
    }
    match err {
        ClientError::Refused { message_key }
            if message_key == helper::NOT_SUPPORTED_YET || message_key == helper::NOT_FOR_HELPER =>
        {
            RunOutcome::Legacy(LegacyReason::NotSupported(message_key))
        }
        other => RunOutcome::Failed(other),
    }
}

// ============================================================================
// Executor
// ============================================================================

/// The privilege session client behind a mutex. A process needs one — [`run`];
/// a separate instance is for tests.
///
/// Warning: submission holds the mutex through the password dialog as well
/// (raising the session is `pkexec` and the handshake), i.e. arbitrarily long.
/// So [`Runner::has_session`] and [`Runner::shutdown`], which the window thread
/// calls (the "Settings" poll, the tray icon menu, the close button), do not
/// wait for the mutex: if it is busy, the answer is the last known state, and
/// "close" is postponed until submission releases it (B13).
pub struct Runner {
    backend: Mutex<Box<dyn Backend>>,
    /// The client's last `has_session` answer.
    session_seen: AtomicBool,
    /// Close the session as soon as the mutex is free.
    shutdown_pending: AtomicBool,
}

impl Runner {
    pub fn new(backend: Box<dyn Backend>) -> Self {
        Self { backend: Mutex::new(backend), session_seen: AtomicBool::new(false), shutdown_pending: AtomicBool::new(false) }
    }

    fn backend(&self) -> MutexGuard<'_, Box<dyn Backend>> {
        // A panic in another thread must not lock the privileges forever: the client
        // itself watches the session's liveness.
        let mut guard = self.backend.lock().unwrap_or_else(|e| e.into_inner());
        self.apply_pending(&mut guard);
        guard
    }

    /// The postponed "close" — under the acquired mutex.
    fn apply_pending(&self, backend: &mut Box<dyn Backend>) {
        if self.shutdown_pending.swap(false, Ordering::SeqCst) {
            backend.shutdown();
        }
        self.session_seen.store(backend.has_session(), Ordering::SeqCst);
    }

    /// Execute an operation (see [`run`]).
    pub fn run(&self, op: &PrivOp, fds: Vec<OwnedFd>, progress: &mut dyn FnMut(&str)) -> RunOutcome {
        if let Err(key) = helper::executes(op) {
            return RunOutcome::Legacy(LegacyReason::NotSupported(key.to_string()));
        }
        // The mutex is only for submission: waiting goes on without it. A "close"
        // that arrived during the password dialog is executed right after it.
        let pending = {
            let mut backend = self.backend();
            let pending = backend.submit(op, fds);
            self.apply_pending(&mut backend);
            pending
        };
        match pending.and_then(|p| p.wait(progress)) {
            Ok(outcome) => RunOutcome::Done(outcome),
            Err(e) => {
                // A helper from another build — the package was updated while the window
                // was open: the same "Rexon was updated" notification as the binary
                // guard's, right away rather than at its poll (B13).
                if matches!(e, ClientError::BuildMismatch { .. } | ClientError::HelperOtherBuild { .. }) {
                    crate::single_instance::nudge_self_update_watch();
                }
                classify(e)
            }
        }
    }

    /// There is a live session. Does not wait: while submission is in progress (the
    /// password dialog) — the last known state.
    pub fn has_session(&self) -> bool {
        match self.backend.try_lock() {
            Ok(mut guard) => {
                self.apply_pending(&mut guard);
                self.session_seen.load(Ordering::SeqCst)
            }
            Err(TryLockError::Poisoned(e)) => {
                let mut guard = e.into_inner();
                self.apply_pending(&mut guard);
                self.session_seen.load(Ordering::SeqCst)
            }
            Err(TryLockError::WouldBlock) => {
                !self.shutdown_pending.load(Ordering::SeqCst) && self.session_seen.load(Ordering::SeqCst)
            }
        }
    }

    /// Close the session. Does not wait: while submission is in progress, it will
    /// close right after it (the submitted operation then gets a refusal rather than
    /// being executed).
    pub fn shutdown(&self) {
        self.shutdown_pending.store(true, Ordering::SeqCst);
        self.session_seen.store(false, Ordering::SeqCst);
        match self.backend.try_lock() {
            Ok(mut guard) => self.apply_pending(&mut guard),
            Err(TryLockError::Poisoned(e)) => self.apply_pending(&mut e.into_inner()),
            Err(TryLockError::WouldBlock) => {}
        }
    }

    /// Close the session for reason `reason` (the window went to the tray, "Revoke
    /// now" in the icon menu). `Some(reason)` — there was a live privilege and it
    /// expired, and the person must be told that the password will be asked again;
    /// `None` — there was nothing to expire. Does not wait, like
    /// [`Runner::shutdown`].
    ///
    /// Only the window knows the reason: the helper sees only the end of the
    /// connection and takes it as the window closing, which there is no one to
    /// report to. So the reason text is shown by the window, not by the helper's
    /// answer.
    pub fn end_session(&self, reason: Reason) -> Option<Reason> {
        let had = self.has_session();
        self.shutdown();
        had.then_some(reason)
    }
}

static LIVE: OnceLock<Runner> = OnceLock::new();

/// The process executor on the live client ([`Client::live`]). The client itself
/// launches nothing until there is a request.
pub fn live() -> &'static Runner {
    LIVE.get_or_init(|| Runner::new(Box::new(Client::live())))
}

/// Execute a privileged operation through the window's privilege session.
///
/// `fds` — the operation's open files in the order of [`PrivOp::local_files`];
/// `progress` — progress lines. The result is [`RunOutcome`]; on
/// [`RunOutcome::Legacy`] the caller executes the old command itself.
pub fn run(op: &PrivOp, fds: Vec<OwnedFd>, progress: &mut dyn FnMut(&str)) -> RunOutcome {
    live().run(op, fds, progress)
}

/// Open an operation file on the window's side: a regular file as is, a
/// directory (a theme, a login screen layout) as an uncompressed ustar in memory
/// ([`super::unpack::pack_tree_memfd`]): the helper does not accept a directory
/// by descriptor (Z2).
pub fn open_local(path: &str) -> std::io::Result<OwnedFd> {
    let p = std::path::Path::new(path);
    if std::fs::metadata(p)?.is_dir() {
        super::unpack::pack_tree_memfd(p)
    } else {
        std::fs::File::open(p).map(OwnedFd::from)
    }
}

/// [`run`] for live executors: `files` — paths of the operation's files in the
/// order of [`PrivOp::local_files`] (they are opened here — [`open_local`] — and
/// sent as descriptors), progress lines are appended to the log `log_path` (if
/// any), passed to `line` and collected into the answer's `stdout`. `None` — the
/// old path ([`RunOutcome::Legacy`]): the executor runs its old command.
///
/// Files are opened only once it is clear that the helper executes the operation
/// ([`helper::executes`]): a directory is not packed for the sake of the old
/// path.
pub fn run_live(
    op: &PrivOp,
    files: &[String],
    log_path: Option<&str>,
    line: &mut dyn FnMut(&str),
) -> Option<crate::system_exec::RunOut> {
    use std::io::Write;
    if helper::executes(op).is_err() {
        return None;
    }
    let mut fds = Vec::with_capacity(files.len());
    for f in files {
        match open_local(f) {
            Ok(h) => fds.push(h),
            // There is no session helper — the old path decides, as before "B" (the
            // old helper accepted a directory with a symlink).
            Err(_) if !session_possible() => return None,
            // There is no file (or the directory cannot be packed) — the session helper
            // has nothing to receive, and there is no old path when it is present (Z5).
            Err(_) => return Some(crate::system_exec::RunOut::missing()),
        }
    }
    let mut log = log_path.and_then(|p| std::fs::OpenOptions::new().create(true).append(true).open(p).ok());
    let mut stdout = String::new();
    let outcome = run(op, fds, &mut |l: &str| {
        if let Some(f) = log.as_mut() {
            let _ = writeln!(f, "{l}");
        }
        stdout.push_str(l);
        stdout.push('\n');
        line(l);
    });
    outcome.to_run_out(stdout)
}

/// A privilege session exists or can be raised: the helper is found and has
/// passed the ownership check ([`super::client::locate_helper`]).
fn session_possible() -> bool {
    use super::client::{helper_candidates, locate_helper, Located, RealFs};
    has_session() || matches!(locate_helper(&RealFs, &helper_candidates()), Located::Ready(_))
}

/// There is a live privilege session (the next operation will not ask for the
/// password).
pub fn has_session() -> bool {
    LIVE.get().is_some_and(Runner::has_session)
}

/// Close the privilege session if there is one: the window is closed or
/// minimized to the tray, "Revoke now" (B13).
pub fn shutdown() {
    if let Some(r) = LIVE.get() {
        r.shutdown();
    }
}

/// Close the privilege session for reason `reason` ([`Runner::end_session`]).
/// `Some` — there was a privilege and it expired.
pub fn end_session(reason: Reason) -> Option<Reason> {
    LIVE.get().and_then(|r| r.end_session(reason))
}

/// What to tell the person when the close button hid the window in the tray:
/// `(title, text)` of a notification or `None` — stay silent.
///
/// `first_time` — the hint "Rexon stayed in the tray" has not been shown yet
/// (it is shown once per installation: every time would be intrusive). `ended` —
/// the result of [`end_session`]. We report an expired privilege EVERY time, not
/// once: otherwise a person who saw the hint yesterday would not learn why the
/// password is being asked again today (the "hint seen" mark used to silence
/// this too — the text of the `window-to-tray` reason was never shown).
pub fn tray_close_notice(first_time: bool, ended: Option<Reason>) -> Option<(String, String)> {
    let (title, hint) = crate::tray::hidden_notice();
    let body = match (first_time, ended) {
        (false, None) => return None,
        (true, None) => hint,
        (false, Some(r)) => r.message(),
        (true, Some(r)) => format!("{hint}\n{}", r.message()),
    };
    Some((title, body))
}

/// The "privilege expired" notification for reason `reason`, when the window
/// may not be on screen (the "Revoke now" item in the icon menu).
pub fn permission_ended_notice(reason: Reason) -> (String, String) {
    (crate::tray::APP_ID.to_string(), reason.message())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::privilege::client::{FileKind, FileMeta, HelperChild, HelperFs, Spawner};
    use crate::privilege::op::{LocalFile, PkgSource};
    use crate::privilege::proto::{self, ClientMsg, ErrorCode, Hello, HelperMsg};
    use crate::system::PkgFamily;
    use crate::system_plan as plan;
    use std::io;
    use std::os::fd::AsFd;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{mpsc, Arc};
    use std::thread;
    use std::time::Duration;

    const HELPER: &str = "/usr/lib/rexon-priv-helper";
    const BUILD: &str = "0.5.0+run-test";
    /// Limit on waiting for any result in tests: a hang is a test failure.
    const LIMIT: Duration = Duration::from_secs(20);

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn install_op() -> PrivOp {
        PrivOp::PkgInstall { family: PkgFamily::Arch, packages: names(&["sddm"]), source: PkgSource::DisplayManager }
    }

    /// File system: directories and (if `with_helper`) the helper — root 0755;
    /// otherwise the helper belongs to the user (`bad_owner`) or is missing.
    struct Fs {
        helper: Option<u32>,
    }

    impl HelperFs for Fs {
        fn lstat(&self, path: &Path) -> io::Result<FileMeta> {
            let dirs = ["/", "/usr", "/usr/lib", "/usr/local", "/usr/local/lib", "/var", "/var/lib"];
            if dirs.iter().any(|d| Path::new(d) == path) {
                return Ok(FileMeta { kind: FileKind::Dir, uid: 0, mode: 0o755 });
            }
            match (self.helper, path == Path::new(HELPER)) {
                (Some(uid), true) => Ok(FileMeta { kind: FileKind::Regular, uid, mode: 0o755 }),
                _ => Err(io::Error::from(io::ErrorKind::NotFound)),
            }
        }
        fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
            Ok(path.to_path_buf())
        }
    }

    /// What the fake helper answers to a request.
    #[derive(Clone)]
    enum Reply {
        Done,
        Refuse(&'static str),
    }

    type Script = dyn Fn(OwnedFd) -> i32 + Send + Sync;

    struct CountingSpawner {
        spawns: Arc<AtomicUsize>,
        script: Arc<Script>,
    }

    struct Child(Option<thread::JoinHandle<i32>>);

    impl HelperChild for Child {
        fn wait(&mut self) -> io::Result<Option<i32>> {
            Ok(self.0.take().map(|h| h.join().unwrap_or(-1)))
        }
    }

    impl Spawner for CountingSpawner {
        fn spawn(&mut self, _argv: &[String], helper_end: OwnedFd) -> io::Result<Box<dyn HelperChild>> {
            self.spawns.fetch_add(1, Ordering::SeqCst);
            let script = Arc::clone(&self.script);
            Ok(Box::new(Child(Some(thread::spawn(move || script(helper_end))))))
        }
    }

    /// A helper that answers the handshake and every request with `reply`.
    fn helper_script(build: &'static str, reply: Reply, seen: Arc<AtomicUsize>) -> Arc<Script> {
        Arc::new(move |sock: OwnedFd| {
            let Ok(Some(ClientMsg::Hello(Hello { .. }))) = proto::recv_msg::<ClientMsg<PrivOp>>(sock.as_fd()) else {
                return 1;
            };
            proto::send_msg(sock.as_fd(), &HelperMsg::Welcome { build_id: build.into() }).unwrap();
            loop {
                let Ok(Some((frame, _fds))) = proto::recv_frame_with_fds(sock.as_fd()) else {
                    return 0;
                };
                let Ok(ClientMsg::Request { id, .. }) = proto::decode::<ClientMsg<PrivOp>>(&frame) else {
                    return 1;
                };
                seen.fetch_add(1, Ordering::SeqCst);
                let msg = match &reply {
                    Reply::Done => {
                        proto::send_msg(sock.as_fd(), &HelperMsg::Progress { id, line: format!("ход {id}") }).unwrap();
                        HelperMsg::Done { id, ok: true, code: Some(0), message_key: None }
                    }
                    Reply::Refuse(key) => {
                        HelperMsg::Error { id: Some(id), code: ErrorCode::Refused, message_key: key.to_string() }
                    }
                };
                proto::send_msg(sock.as_fd(), &msg).unwrap();
            }
        })
    }

    /// Executor on a real [`Client`] with a faked file system and launching.
    fn runner(fs: Fs, script: Arc<Script>) -> (Arc<Runner>, Arc<AtomicUsize>) {
        let spawns = Arc::new(AtomicUsize::new(0));
        let spawner = CountingSpawner { spawns: Arc::clone(&spawns), script };
        let client = Client::new(Box::new(spawner), Box::new(fs), BUILD.into(), Some("2".into()))
            .with_candidates(vec![PathBuf::from(HELPER)]);
        (Arc::new(Runner::new(Box::new(client))), spawns)
    }

    /// `run` in a separate thread with a time limit.
    fn run_limited(r: &Arc<Runner>, op: PrivOp) -> (RunOutcome, Vec<String>) {
        let (tx, rx) = mpsc::channel();
        let r = Arc::clone(r);
        thread::spawn(move || {
            let mut lines = Vec::new();
            let out = r.run(&op, Vec::new(), &mut |l: &str| lines.push(l.to_string()));
            let _ = tx.send((out, lines));
        });
        rx.recv_timeout(LIMIT).expect("run завис")
    }

    #[test]
    fn many_operations_one_spawner() {
        let seen = Arc::new(AtomicUsize::new(0));
        let (r, spawns) = runner(Fs { helper: Some(0) }, helper_script(BUILD, Reply::Done, Arc::clone(&seen)));
        for i in 0..4 {
            let (out, lines) = run_limited(&r, install_op());
            assert!(matches!(&out, RunOutcome::Done(o) if o.ok), "{i}: {out:?}");
            assert_eq!(lines, [format!("ход {}", i + 1)]);
        }
        assert_eq!(spawns.load(Ordering::SeqCst), 1, "один pkexec на все операции");
        assert_eq!(seen.load(Ordering::SeqCst), 4);
        assert!(r.has_session());
        r.shutdown();
        assert!(!r.has_session());
    }

    /// Deepin on Treeland as a whole: build rebuilds, our own deepin-session build
    /// and the desktop set — one helper launch (one password). SteamOS: installing
    /// and removing the desktop image — also one.
    #[test]
    fn deepin_rebuild_and_set_and_sysext_one_spawner() {
        use crate::privilege::op::SysextOp;
        let seen = Arc::new(AtomicUsize::new(0));
        let (r, spawns) = runner(Fs { helper: Some(0) }, helper_script(BUILD, Reply::Done, Arc::clone(&seen)));
        let built = PrivOp::InstallBuilt {
            package: LocalFile { slot: 0, label: "/home/u/.cache/rexon/rebuild/deepin-session-treeland-1-1-x86_64.pkg.tar.zst".into() },
            build_only: names(&["cmake"]),
            replace: false,
        };
        let set = PrivOp::DeInstallSet {
            family: PkgFamily::Arch,
            de_id: Some("deepin".into()),
            packages: names(&["treeland", "ddm", "deepin-anything-dkms"]),
            deps_only: names(&["deepin-anything-dkms"]),
            replace: vec![],
            update_first: true,
        };
        let ops = [
            PrivOp::BuildDeps { names: names(&["cmake"]) },
            built,
            set,
            PrivOp::Sysext { op: SysextOp::Install, de_id: "xfce".into() },
            PrivOp::Sysext { op: SysextOp::Remove, de_id: "xfce".into() },
        ];
        for op in ops {
            let (tx, rx) = mpsc::channel();
            let r2 = Arc::clone(&r);
            let label = format!("{op:?}");
            thread::spawn(move || {
                let fds = op
                    .local_files()
                    .iter()
                    .map(|_| OwnedFd::from(std::fs::File::open("/dev/null").unwrap()))
                    .collect();
                let _ = tx.send(r2.run(&op, fds, &mut |_| {}));
            });
            let out = rx.recv_timeout(LIMIT).expect("run завис");
            assert!(matches!(&out, RunOutcome::Done(o) if o.ok), "{label}: {out:?}");
        }
        assert_eq!(spawns.load(Ordering::SeqCst), 1, "один pkexec на всё");
        assert_eq!(seen.load(Ordering::SeqCst), 5);
        r.shutdown();
    }

    #[test]
    fn no_helper_goes_legacy_with_todays_argv() {
        let (r, spawns) = runner(Fs { helper: None }, helper_script(BUILD, Reply::Done, Arc::default()));
        let (out, _) = run_limited(&r, install_op());
        assert_eq!(out, RunOutcome::Legacy(LegacyReason::HelperUnavailable(ClientError::HelperMissing)));
        assert_eq!(out.to_run_out(String::new()), None);
        assert_eq!(spawns.load(Ordering::SeqCst), 0);
        // The old command is the same one the install plan builds today.
        for pkg in [true, false] {
            let helpers = Helpers { pkg, ..Helpers::default() };
            let today = plan::install_plan(PkgFamily::Arch, &names(&["sddm"]), false, None, pkg).unwrap();
            let want = today
                .steps
                .iter()
                .find_map(|s| match s {
                    plan::PlanStep::Run { id: plan::RunStepId::PkgInstall, cmd, .. } => Some(cmd.argv.clone()),
                    _ => None,
                })
                .unwrap();
            assert_eq!(legacy_command(&install_op(), &helpers), Some(LegacyCommand::Argv(want)));
        }
        // A script where there is no single command.
        assert_eq!(
            legacy_command(&PrivOp::MateGschemaCleanup, &Helpers::default()),
            Some(LegacyCommand::Script { prefix: "rexon_gschema_", text: plan::mate_gschema_cleanup_script() })
        );
        // switch-dm without a helper — there is no old path.
        let switch = PrivOp::SwitchDm { family: PkgFamily::Arch, unit: "sddm".into(), greeter: None };
        assert_eq!(legacy_command(&switch, &Helpers::default()), None);
    }

    #[test]
    fn bad_owner_goes_legacy_without_spawn() {
        let (r, spawns) = runner(Fs { helper: Some(1000) }, helper_script(BUILD, Reply::Done, Arc::default()));
        let (out, _) = run_limited(&r, install_op());
        assert!(
            matches!(out, RunOutcome::Legacy(LegacyReason::HelperUnavailable(ClientError::HelperBadOwner(_)))),
            "{out:?}"
        );
        assert_eq!(spawns.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn refusal_of_check_has_no_raw_path() {
        let (r, _) = runner(Fs { helper: Some(0) }, helper_script(BUILD, Reply::Refuse("refused.not-in-table"), Arc::default()));
        let (out, _) = run_limited(&r, install_op());
        assert_eq!(out, RunOutcome::Failed(ClientError::Refused { message_key: "refused.not-in-table".into() }));
        assert!(!out.is_legacy());
        let ro = out.to_run_out(String::new()).unwrap();
        assert_eq!(ro.returncode, Some(1));
        // The person gets a phrase in the window's language, not the helper's key (B13).
        assert_eq!(ro.stderr, crate::privilege::client::key_text("refused.not-in-table"));
        assert!(!ro.stderr.contains("refused.") && !ro.stderr.contains("rexon-priv-helper"), "{ro:?}");
    }

    #[test]
    fn dismissed_password_and_build_mismatch_have_no_raw_path() {
        // The password dialog was closed: pkexec exited with 126 before the handshake.
        let dismissed: Arc<Script> = Arc::new(|sock: OwnedFd| {
            drop(sock);
            126
        });
        let (r, _) = runner(Fs { helper: Some(0) }, dismissed);
        let (out, _) = run_limited(&r, install_op());
        assert_eq!(out, RunOutcome::Failed(ClientError::NotAuthorized { code: 126 }));
        // The pkexec code is kept: the executors have an explanation for it.
        assert_eq!(out.to_run_out(String::new()).unwrap().returncode, Some(126));

        let (r, spawns) = runner(Fs { helper: Some(0) }, helper_script("0.4.0+other", Reply::Done, Arc::default()));
        let (out, _) = run_limited(&r, install_op());
        assert!(matches!(out, RunOutcome::Failed(ClientError::BuildMismatch { .. })), "{out:?}");
        // A repeat does not raise the session again and does not give the raw path either.
        let (again, _) = run_limited(&r, install_op());
        assert!(matches!(again, RunOutcome::Failed(ClientError::BuildMismatch { .. })), "{again:?}");
        assert_eq!(spawns.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn not_supported_reply_is_the_only_legacy_after_session() {
        for key in [helper::NOT_SUPPORTED_YET, helper::NOT_FOR_HELPER] {
            let (r, _) = runner(Fs { helper: Some(0) }, helper_script(BUILD, Reply::Refuse(key), Arc::default()));
            let (out, _) = run_limited(&r, install_op());
            assert_eq!(out, RunOutcome::Legacy(LegacyReason::NotSupported(key.into())));
            assert_eq!(out.to_run_out(String::new()), None);
        }
        // A similar but different key — refusal.
        assert!(matches!(
            classify(ClientError::Refused { message_key: "not_supported".into() }),
            RunOutcome::Failed(_)
        ));
    }

    #[test]
    fn known_unsupported_ops_skip_the_password() {
        let seen = Arc::new(AtomicUsize::new(0));
        let (r, spawns) = runner(Fs { helper: Some(0) }, helper_script(BUILD, Reply::Done, Arc::clone(&seen)));
        let aur = PrivOp::AurInstall { helper: "paru".into(), packages: names(&["x"]), replace_conflicts: false };
        let (out, _) = run_limited(&r, aur);
        assert_eq!(out, RunOutcome::Legacy(LegacyReason::NotSupported(helper::NOT_FOR_HELPER.into())));
        let (out, _) = run_limited(&r, PrivOp::MateGschemaCleanup);
        assert_eq!(out, RunOutcome::Legacy(LegacyReason::NotSupported(helper::NOT_SUPPORTED_YET.into())));
        assert_eq!(spawns.load(Ordering::SeqCst), 0, "ради заведомого отказа пароль не спрашивается");
        assert_eq!(seen.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn every_client_error_classified_by_z5() {
        let legacy = [
            ClientError::HelperMissing,
            ClientError::HelperOtherBuild { helper_build: "0.4.9+old".into() },
        ];
        for e in legacy {
            assert!(classify(e).is_legacy());
        }
        let failed = [
            ClientError::NotAuthorized { code: 127 },
            ClientError::BuildMismatch { helper_build: None },
            ClientError::Refused { message_key: "refused.vital".into() },
            ClientError::HelperFailed { message_key: "x".into() },
            ClientError::Cancelled,
            ClientError::Revoked { message_key: "idle".into() },
            ClientError::SessionLost { outcome_unknown: true, detail: "x".into() },
            ClientError::Closed,
            ClientError::Busy,
            ClientError::Spawn("x".into()),
            ClientError::InvalidRequest("x".into()),
        ];
        for e in failed {
            let out = classify(e.clone());
            assert_eq!(out, RunOutcome::Failed(e));
            assert!(out.to_run_out(String::new()).is_some());
        }
    }

    // ---------------------------------------------------------------- substitution via the trait

    /// Fake client: remembers operations, answers with what it is given.
    struct FakeBackend {
        calls: Arc<Mutex<Vec<(PrivOp, usize)>>>,
        answer: Result<Outcome, ClientError>,
    }

    struct Ready(Result<Outcome, ClientError>);

    impl Waiter for Ready {
        fn wait(self: Box<Self>, on_progress: &mut dyn FnMut(&str)) -> Result<Outcome, ClientError> {
            on_progress("строка");
            self.0
        }
    }

    impl Backend for FakeBackend {
        fn submit(&mut self, op: &PrivOp, fds: Vec<OwnedFd>) -> Result<Box<dyn Waiter>, ClientError> {
            self.calls.lock().unwrap().push((op.clone(), fds.len()));
            Ok(Box::new(Ready(self.answer.clone())))
        }
        fn has_session(&self) -> bool {
            true
        }
        fn shutdown(&mut self) {}
    }

    #[test]
    fn fake_backend_gets_op_files_and_progress() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let ok = Outcome { ok: false, code: Some(3), message_key: Some("pkg_failed".into()) };
        let r = Runner::new(Box::new(FakeBackend { calls: Arc::clone(&calls), answer: Ok(ok.clone()) }));
        let file = std::fs::File::open("/dev/null").unwrap();
        let op = PrivOp::InstallBuilt {
            package: LocalFile { slot: 0, label: "/home/u/.cache/rexon/rebuild/x.pkg.tar.zst".into() },
            build_only: vec![],
            replace: false,
        };
        let mut lines = Vec::new();
        let out = r.run(&op, vec![OwnedFd::from(file)], &mut |l: &str| lines.push(l.to_string()));
        assert_eq!(out, RunOutcome::Done(ok));
        assert_eq!(lines, ["строка"]);
        assert_eq!(calls.lock().unwrap().as_slice(), [(op, 1)]);
        // A program failure means executed, not a reason for the old path.
        let ro = out.to_run_out("строка\n".into()).unwrap();
        assert_eq!((ro.returncode, ro.stdout.as_str()), (Some(3), "строка\n"));
        assert_eq!(ro.outcome, crate::utils::RunOutcome::Failed);
    }

    /// A client whose submission hangs until released (the password dialog).
    struct StuckBackend {
        release: Arc<Mutex<mpsc::Receiver<()>>>,
        /// The "submission started" signal (the mutex is already taken and
        /// `apply_pending` before it has run).
        entered: mpsc::Sender<()>,
        alive: Arc<std::sync::atomic::AtomicBool>,
        shutdowns: Arc<AtomicUsize>,
    }

    impl Backend for StuckBackend {
        fn submit(&mut self, _op: &PrivOp, _fds: Vec<OwnedFd>) -> Result<Box<dyn Waiter>, ClientError> {
            let _ = self.entered.send(());
            let _ = self.release.lock().unwrap().recv_timeout(LIMIT);
            self.alive.store(true, Ordering::SeqCst);
            Err(ClientError::Closed)
        }
        fn has_session(&self) -> bool {
            self.alive.load(Ordering::SeqCst)
        }
        fn shutdown(&mut self) {
            self.alive.store(false, Ordering::SeqCst);
            self.shutdowns.fetch_add(1, Ordering::SeqCst);
        }
    }

    /// B13: while the password dialog is up, the window thread asks "is there a
    /// session" and presses "Revoke now" / hides the window in the tray — and does
    /// not block. The close is executed as soon as submission releases the client.
    #[test]
    fn window_thread_never_waits_for_password() {
        let (tx, rx) = mpsc::channel();
        let (entered_tx, entered_rx) = mpsc::channel();
        let alive = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let shutdowns = Arc::new(AtomicUsize::new(0));
        let r = Arc::new(Runner::new(Box::new(StuckBackend {
            release: Arc::new(Mutex::new(rx)),
            entered: entered_tx,
            alive: Arc::clone(&alive),
            shutdowns: Arc::clone(&shutdowns),
        })));
        let worker = {
            let r = Arc::clone(&r);
            thread::spawn(move || r.run(&install_op(), vec![], &mut |_| {}))
        };
        // Wait until the executor enters submission. Waiting for "mutex busy" alone
        // is not enough: it is also busy in `apply_pending` before it, and "close"
        // was then executed before submission — the test failed as a race.
        entered_rx.recv_timeout(LIMIT).expect("постановка не началась");
        assert!(r.backend.try_lock().is_err());
        let started = std::time::Instant::now();
        assert!(!r.has_session());
        r.shutdown();
        assert!(!r.has_session());
        assert!(started.elapsed() < Duration::from_secs(2), "поток окна ждал окна пароля");
        assert_eq!(shutdowns.load(Ordering::SeqCst), 0, "пока постановка идёт, клиента не трогаем");
        tx.send(()).unwrap();
        let out = worker.join().unwrap();
        assert_eq!(out, RunOutcome::Failed(ClientError::Closed));
        assert_eq!(shutdowns.load(Ordering::SeqCst), 1, "отложенное «закрыть» исполнено");
        assert!(!alive.load(Ordering::SeqCst));
        assert!(!r.has_session());
    }

    /// The close button hides the window in the tray: the live privilege expires,
    /// and the window knows the reason so it can tell the person; the second time
    /// there is nothing to expire.
    #[test]
    fn end_session_reports_reason_only_when_permission_was_alive() {
        let (_tx, rx) = mpsc::channel::<()>();
        let (entered, _) = mpsc::channel();
        let alive = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let shutdowns = Arc::new(AtomicUsize::new(0));
        let r = Runner::new(Box::new(StuckBackend {
            release: Arc::new(Mutex::new(rx)),
            entered,
            alive: Arc::clone(&alive),
            shutdowns: Arc::clone(&shutdowns),
        }));
        assert!(r.has_session());
        assert_eq!(r.end_session(Reason::WindowToTray), Some(Reason::WindowToTray));
        assert!(!r.has_session());
        assert_eq!(shutdowns.load(Ordering::SeqCst), 1);
        assert_eq!(r.end_session(Reason::WindowToTray), None, "права уже нет — молчим");
    }

    /// The text of the "minimized to tray" reason reaches the person every time the
    /// privilege expired, while the "stayed in the tray" hint — only the first time.
    #[test]
    fn tray_close_notice_tells_ended_permission_every_time() {
        let (title, hint) = crate::tray::hidden_notice();
        let why = Reason::WindowToTray.message();
        assert_eq!(tray_close_notice(false, None), None);
        assert_eq!(tray_close_notice(true, None), Some((title.clone(), hint.clone())));
        assert_eq!(tray_close_notice(false, Some(Reason::WindowToTray)), Some((title.clone(), why.clone())));
        let (t, body) = tray_close_notice(true, Some(Reason::WindowToTray)).unwrap();
        assert_eq!(t, title);
        assert!(body.starts_with(&hint) && body.ends_with(&why), "{body}");
        assert_eq!(
            permission_ended_notice(Reason::RevokedByUser),
            ("Rexon".to_string(), Reason::RevokedByUser.message())
        );
    }

    #[test]
    fn module_shutdown_without_live_client_is_noop() {
        // A live client is not created just to answer "is there a session".
        if LIVE.get().is_none() {
            assert!(!has_session());
            shutdown();
            assert!(LIVE.get().is_none());
        }
    }
}
