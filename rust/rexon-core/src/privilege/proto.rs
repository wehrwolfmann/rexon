//! Protocol between the window and the root helper (item B2 of the plan, see
//! [`super`]).
//!
//! # Transport
//!
//! `AF_UNIX` + `SOCK_SEQPACKET` from `socketpair` (the window hands its end to
//! the helper on fd 0/1; the channel has no address — Z3). One write to the
//! socket = one frame = one message: the kernel keeps the boundaries, we do not
//! encode the length.
//!
//! # Frame
//!
//! JSON of the form `{"v":1,"msg":{…}}`, no longer than [`MAX_FRAME`] bytes:
//! * `v` — the protocol version ([`PROTO_VERSION`]); a foreign version → error;
//! * `msg` — the message ([`ClientMsg`] from the window, [`HelperMsg`] from the
//!   helper);
//! * extra fields at any level → error (`deny_unknown_fields`).
//!
//! # A frame error = end of the session
//!
//! A corrupted, too large, extra-field or foreign-version frame is a
//! [`ProtoError`]. Recovering in the middle of the stream is not allowed: the
//! frame may have come from someone other than the party we agreed with. So
//! **the side that received the frame** closes:
//! * the helper: drops its end of the socket and behaves as on EOF (Z4) — it
//!   finishes the running transaction, takes no new ones, exits. No answer is
//!   sent to a corrupted frame: there is nothing to say to a peer that sends
//!   garbage;
//! * the window: drops its end (the helper sees EOF and exits), the session is
//!   considered dead, the running operation has an unknown outcome. A raw
//!   `pkexec` is **not** launched instead of it (Z5): the person sees a message.
//!
//! # Refusal ≠ failure
//!
//! [`Done`](HelperMsg::Done) — the operation was executed (successfully or not,
//! with the program's exit code). [`Error`](HelperMsg::Error) — execution was
//! never reached or the session was interrupted; its [`ErrorCode::class`] tells a
//! "check refusal" from a "helper failure". No code of this protocol allows the
//! window to fall back to a raw `pkexec` (Z5): the raw path is only for when
//! there is no helper at all, and the client decides that BEFORE the handshake.
//!
//! # Operations
//!
//! The operation type is the `Op` parameter (it will appear in B1 as
//! `privilege::op::PrivOp`). The protocol knows nothing about the content of an
//! operation and only carries it.

use std::fmt;
use std::io;
use std::os::fd::{AsRawFd, BorrowedFd, FromRawFd, OwnedFd};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

/// Protocol version. Changes with any incompatible change to the messages.
pub const PROTO_VERSION: u32 = 1;

/// Limit of one frame in bytes (the whole JSON, with the envelope).
pub const MAX_FRAME: usize = 64 * 1024;

/// How many descriptors (`SCM_RIGHTS`) one frame may carry. The "thickest"
/// operation needs one file; the margin is for the future, but no more: every
/// extra fd is one more thing root keeps open.
pub const MAX_FDS: usize = 4;

/// Rexon build fingerprint for the handshake: version and commit,
/// `0.5.0+1a2b3c4d5e`.
///
/// The commit is `REXON_GIT_REV` from `build.rs` (the same as in the log's
/// startup line, [`crate::applog::version_line`]). The window and the helper
/// are binaries of one build of one crate, so their fingerprints match, while a
/// helper from another Rexon installation (the package was updated while the
/// window was open, a foreign copy) is rejected. A build without git (from an
/// archive) gives only the version.
pub fn build_id() -> String {
    let v = crate::window_menu::version_string();
    match option_env!("REXON_GIT_REV") {
        Some(rev) => format!("{v}+{rev}"),
        None => v,
    }
}

// ============================================================================
// Messages
// ============================================================================

/// The window's first message. Until the helper has accepted it, it takes nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hello {
    /// The window's [`build_id`].
    pub build_id: String,
}

/// Window → helper.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ClientMsg<Op> {
    /// Handshake; exactly once and first.
    Hello(Hello),
    /// Execute an operation. The window issues `id`, the helper repeats it in its answers.
    Request { id: u64, op: Op },
    /// Cancel operation `id`. Effective only before the system is touched; after
    /// that the helper finishes the operation and answers with an ordinary `Done`.
    Cancel { id: u64 },
}

/// Helper → window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum HelperMsg {
    /// Handshake accepted; the helper's `build_id` is for the window's log.
    Welcome { build_id: String },
    /// A progress line of the operation (instead of a log in `/tmp`, Z2). The helper
    /// cuts long lines itself: a frame larger than [`MAX_FRAME`] will not be sent.
    Progress { id: u64, line: String },
    /// The operation was executed. `ok == false` — the program ran with an error:
    /// `code` — its exit code (no code — killed by a signal), `message_key` — the
    /// translation key of the explanation, if the helper knows it.
    Done {
        id: u64,
        ok: bool,
        code: Option<i32>,
        message_key: Option<String>,
    },
    /// Another operation is already running: the helper takes one at a time. `id` —
    /// of the rejected one.
    Busy { id: u64 },
    /// Execution was never reached or the session was interrupted. `id` — of the
    /// operation, if the error is about it (`None` — about the session: handshake,
    /// revocation of privileges).
    Error {
        id: Option<u64>,
        code: ErrorCode,
        message_key: String,
    },
}

/// Codes of [`HelperMsg::Error`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// The argument check did not pass (name, allow-list, a vital package, the
    /// current desktop — Z1, Z2, Z6, Z7, Z8).
    Refused,
    /// Privileges revoked: inactivity, lock, session change, "Revoke now". The
    /// session is closed; a new one — only with a new password.
    Revoked,
    /// Handshake: a different Rexon build. The helper exits after answering.
    BuildMismatch,
    /// A request before `Hello`, a repeated `Hello`, `Cancel` of a foreign `id`.
    Sequence,
    /// `Cancel` accepted before touching the system: the operation was not executed.
    Cancelled,
    /// A failure of the helper itself (a program did not start, an I/O error).
    Internal,
}

/// What an [`ErrorCode`] essentially is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorClass {
    /// The helper deliberately did not do it. It may not be bypassed with a raw `pkexec` (Z5).
    Refusal,
    /// The person cancelled the operation themselves.
    Cancelled,
    /// The helper could not do it. A raw `pkexec` is not launched either (Z5): the
    /// helper was there, and since it failed, a second path around it would be the
    /// same hole.
    Failure,
}

impl ErrorCode {
    pub fn class(self) -> ErrorClass {
        match self {
            Self::Refused | Self::Revoked | Self::BuildMismatch | Self::Sequence => {
                ErrorClass::Refusal
            }
            Self::Cancelled => ErrorClass::Cancelled,
            Self::Internal => ErrorClass::Failure,
        }
    }
}

/// Handshake check on the helper's side: `Ok` — continue, `Err` — answer with
/// [`HelperMsg::Error`] with this code and exit.
pub fn check_hello(hello: &Hello, own_build_id: &str) -> Result<(), ErrorCode> {
    if hello.build_id == own_build_id {
        Ok(())
    } else {
        Err(ErrorCode::BuildMismatch)
    }
}

// ============================================================================
// Codec
// ============================================================================

/// A frame error. Any of them ends the session (see the module description).
#[derive(Debug)]
pub enum ProtoError {
    /// Socket error.
    Io(io::Error),
    /// The frame is longer than [`MAX_FRAME`] (the real length, not the truncated one).
    TooLarge(usize),
    /// Not JSON, truncation, extra or missing fields, an unknown message.
    Malformed(String),
    /// A foreign protocol version (`None` — there is no `v` field or it is not a number).
    Version(Option<u64>),
    /// The frame's descriptors: more than [`MAX_FDS`], truncated by the kernel
    /// (`MSG_CTRUNC`), a foreign kind of ancillary data. All fds received with the
    /// frame are already closed.
    Fds(String),
}

impl fmt::Display for ProtoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "privilege session socket: {e}"),
            Self::TooLarge(n) => write!(f, "frame of {n} bytes exceeds the limit {MAX_FRAME}"),
            Self::Malformed(e) => write!(f, "malformed frame: {e}"),
            Self::Version(Some(v)) => {
                write!(f, "protocol version {v}, expected {PROTO_VERSION}")
            }
            Self::Version(None) => write!(f, "the frame has no protocol version"),
            Self::Fds(e) => write!(f, "frame descriptors: {e}"),
        }
    }
}

impl std::error::Error for ProtoError {}

impl From<io::Error> for ProtoError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

#[derive(Serialize)]
struct EnvelopeRef<'a, M> {
    v: u32,
    msg: &'a M,
}

/// Message → frame bytes. More than [`MAX_FRAME`] — [`ProtoError::TooLarge`].
pub fn encode<M: Serialize>(msg: &M) -> Result<Vec<u8>, ProtoError> {
    let bytes = serde_json::to_vec(&EnvelopeRef {
        v: PROTO_VERSION,
        msg,
    })
    .map_err(|e| ProtoError::Malformed(e.to_string()))?;
    if bytes.len() > MAX_FRAME {
        return Err(ProtoError::TooLarge(bytes.len()));
    }
    Ok(bytes)
}

/// Frame bytes → message.
///
/// Parsing in two steps: first the envelope (exactly the `v` and `msg` fields,
/// the version), then the message itself. That way a frame of a newer version
/// gives a clear [`ProtoError::Version`] rather than an "unknown variant" from
/// deep inside `msg`.
pub fn decode<M: DeserializeOwned>(bytes: &[u8]) -> Result<M, ProtoError> {
    if bytes.len() > MAX_FRAME {
        return Err(ProtoError::TooLarge(bytes.len()));
    }
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|e| ProtoError::Malformed(e.to_string()))?;
    let serde_json::Value::Object(mut obj) = value else {
        return Err(ProtoError::Malformed("the frame is not an object".into()));
    };
    let version = obj.get("v").and_then(serde_json::Value::as_u64);
    if version != Some(u64::from(PROTO_VERSION)) {
        return Err(ProtoError::Version(version));
    }
    if let Some(extra) = obj.keys().find(|k| *k != "v" && *k != "msg") {
        return Err(ProtoError::Malformed(format!(
            "unexpected envelope field `{extra}`"
        )));
    }
    let msg = obj
        .remove("msg")
        .ok_or_else(|| ProtoError::Malformed("no `msg` field".into()))?;
    serde_json::from_value(msg).map_err(|e| ProtoError::Malformed(e.to_string()))
}

// ============================================================================
// Socket
// ============================================================================

/// A `SOCK_SEQPACKET` pair with `SOCK_CLOEXEC`: neither end will survive `exec`
/// in the window's children (Z3). The client itself puts the helper's end on fd
/// 0/1 of the child via `dup2` — the copy has CLOEXEC cleared, the original
/// keeps it.
pub fn seqpacket_pair() -> io::Result<(OwnedFd, OwnedFd)> {
    let mut fds = [0; 2];
    // SAFETY: `fds` is an array of two ints, exactly what socketpair expects.
    let rc = unsafe {
        libc::socketpair(
            libc::AF_UNIX,
            libc::SOCK_SEQPACKET | libc::SOCK_CLOEXEC,
            0,
            fds.as_mut_ptr(),
        )
    };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: socketpair returned two new descriptors, only we own them.
    Ok(unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) })
}

/// Write one frame. `MSG_NOSIGNAL`: a closed peer gives an `EPIPE` error rather
/// than a SIGPIPE that kills the process.
pub fn send_frame(fd: BorrowedFd<'_>, frame: &[u8]) -> Result<(), ProtoError> {
    if frame.len() > MAX_FRAME {
        return Err(ProtoError::TooLarge(frame.len()));
    }
    loop {
        // SAFETY: the pointer and length come from a live slice.
        let n = unsafe {
            libc::send(
                fd.as_raw_fd(),
                frame.as_ptr().cast(),
                frame.len(),
                libc::MSG_NOSIGNAL,
            )
        };
        if n >= 0 {
            // SEQPACKET writes the frame entirely or not at all.
            return if n as usize == frame.len() {
                Ok(())
            } else {
                Err(ProtoError::Io(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "the frame was not written completely",
                )))
            };
        }
        let err = io::Error::last_os_error();
        if err.kind() != io::ErrorKind::Interrupted {
            return Err(err.into());
        }
    }
}

/// Read one frame. `Ok(None)` — the peer closed its end (EOF).
///
/// `MSG_TRUNC` makes the kernel return the real frame length even if it did not
/// fit into the buffer: a too-large frame is recognized rather than silently
/// cut down to "plausible" JSON. The protocol never sends empty frames, so a
/// length of 0 is always read as EOF.
pub fn recv_frame(fd: BorrowedFd<'_>) -> Result<Option<Vec<u8>>, ProtoError> {
    let mut buf = vec![0u8; MAX_FRAME];
    loop {
        // SAFETY: the pointer and length come from a live buffer.
        let n = unsafe {
            libc::recv(
                fd.as_raw_fd(),
                buf.as_mut_ptr().cast(),
                buf.len(),
                libc::MSG_TRUNC,
            )
        };
        if n >= 0 {
            let n = n as usize;
            if n == 0 {
                return Ok(None);
            }
            if n > MAX_FRAME {
                return Err(ProtoError::TooLarge(n));
            }
            buf.truncate(n);
            return Ok(Some(buf));
        }
        let err = io::Error::last_os_error();
        if err.kind() != io::ErrorKind::Interrupted {
            return Err(err.into());
        }
    }
}

/// [`encode`] + [`send_frame`].
pub fn send_msg<M: Serialize>(fd: BorrowedFd<'_>, msg: &M) -> Result<(), ProtoError> {
    send_frame(fd, &encode(msg)?)
}

/// [`recv_frame`] + [`decode`]. `Ok(None)` — EOF.
pub fn recv_msg<M: DeserializeOwned>(fd: BorrowedFd<'_>) -> Result<Option<M>, ProtoError> {
    match recv_frame(fd)? {
        Some(frame) => decode(&frame).map(Some),
        None => Ok(None),
    }
}

// ============================================================================
// Frame with descriptors (SCM_RIGHTS)
// ============================================================================
//
// The helper receives the user's files ONLY as open descriptors (Z2): the
// window opens the file itself, under its own identity, and passes the fd
// together with the operation frame; the helper never opens the path. The fd
// number in an operation is the index in this frame's list
// (`op::LocalFile::slot`).

/// Room for the ancillary data for `n` descriptors (`CMSG_SPACE`), in `u64`
/// words: the cmsg buffer must be aligned like `cmsghdr`.
fn cmsg_space_words(n: usize) -> usize {
    // SAFETY: CMSG_SPACE is pure arithmetic over a length.
    let bytes = unsafe { libc::CMSG_SPACE((n * std::mem::size_of::<libc::c_int>()) as u32) };
    (bytes as usize).div_ceil(std::mem::size_of::<u64>())
}

/// Write a frame together with descriptors `fds` (no more than [`MAX_FDS`]). An
/// empty `fds` is the same as [`send_frame`]. The sender keeps its own copies of
/// the fds and closes them itself: the kernel passes duplicates to the peer.
pub fn send_frame_with_fds(
    fd: BorrowedFd<'_>,
    frame: &[u8],
    fds: &[BorrowedFd<'_>],
) -> Result<(), ProtoError> {
    if fds.is_empty() {
        return send_frame(fd, frame);
    }
    if fds.len() > MAX_FDS {
        return Err(ProtoError::Fds(format!("{} descriptors exceed the limit {MAX_FDS}", fds.len())));
    }
    if frame.is_empty() {
        // The peer would read an empty frame as EOF, and the fds would be lost with it.
        return Err(ProtoError::Malformed("an empty frame with descriptors".into()));
    }
    if frame.len() > MAX_FRAME {
        return Err(ProtoError::TooLarge(frame.len()));
    }
    let raw: Vec<libc::c_int> = fds.iter().map(|f| f.as_raw_fd()).collect();
    let mut control = vec![0u64; cmsg_space_words(raw.len())];
    let mut iov = libc::iovec {
        iov_base: frame.as_ptr() as *mut libc::c_void,
        iov_len: frame.len(),
    };
    // SAFETY: msghdr is a plain C structure, zeros are valid in it.
    let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
    msg.msg_iov = &mut iov;
    msg.msg_iovlen = 1;
    msg.msg_control = control.as_mut_ptr().cast();
    // SAFETY: CMSG_SPACE is arithmetic.
    msg.msg_controllen =
        unsafe { libc::CMSG_SPACE(std::mem::size_of_val(raw.as_slice()) as u32) } as _;
    // SAFETY: the control buffer is allocated for CMSG_SPACE(n) and aligned; the
    // first header exists, and data is written exactly within its bounds.
    unsafe {
        let cmsg = libc::CMSG_FIRSTHDR(&msg);
        (*cmsg).cmsg_level = libc::SOL_SOCKET;
        (*cmsg).cmsg_type = libc::SCM_RIGHTS;
        (*cmsg).cmsg_len = libc::CMSG_LEN(std::mem::size_of_val(raw.as_slice()) as u32) as _;
        std::ptr::copy_nonoverlapping(
            raw.as_ptr().cast::<u8>(),
            libc::CMSG_DATA(cmsg),
            std::mem::size_of_val(raw.as_slice()),
        );
    }
    loop {
        // SAFETY: msg points to live iov and control.
        let n = unsafe { libc::sendmsg(fd.as_raw_fd(), &msg, libc::MSG_NOSIGNAL) };
        if n >= 0 {
            return if n as usize == frame.len() {
                Ok(())
            } else {
                Err(ProtoError::Io(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "the frame was not written completely",
                )))
            };
        }
        let err = io::Error::last_os_error();
        if err.kind() != io::ErrorKind::Interrupted {
            return Err(err.into());
        }
    }
}

/// Read a frame together with descriptors. `Ok(None)` — EOF.
///
/// * `MSG_CMSG_CLOEXEC`: the received fds will not survive `exec` of the
///   programs the helper starts (pacman, cp) — none of them may inherit a file
///   of the window;
/// * every received fd IMMEDIATELY becomes an [`OwnedFd`] — before any check, so
///   on any error below (truncation, extras, a foreign cmsg kind, a frame over
///   the limit, EOF) all of them close by themselves;
/// * `MSG_CTRUNC` (the ancillary data did not fit: the peer sent more than
///   [`MAX_FDS`]) — an error: the kernel has already dropped some fds, the frame
///   is incomplete;
/// * ancillary data other than `SCM_RIGHTS` (for example, `SCM_CREDENTIALS`) —
///   an error.
pub fn recv_frame_with_fds(
    fd: BorrowedFd<'_>,
) -> Result<Option<(Vec<u8>, Vec<OwnedFd>)>, ProtoError> {
    let mut buf = vec![0u8; MAX_FRAME];
    let mut control = vec![0u64; cmsg_space_words(MAX_FDS)];
    loop {
        let mut iov = libc::iovec {
            iov_base: buf.as_mut_ptr().cast(),
            iov_len: buf.len(),
        };
        // SAFETY: msghdr is a plain C structure, zeros are valid in it.
        let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
        msg.msg_iov = &mut iov;
        msg.msg_iovlen = 1;
        msg.msg_control = control.as_mut_ptr().cast();
        msg.msg_controllen = std::mem::size_of_val(control.as_slice()) as _;
        // SAFETY: msg points to live buf and control.
        let n = unsafe {
            libc::recvmsg(
                fd.as_raw_fd(),
                &mut msg,
                libc::MSG_TRUNC | libc::MSG_CMSG_CLOEXEC,
            )
        };
        if n < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(err.into());
        }
        // First take ownership of everything that arrived — then check.
        let mut owned: Vec<OwnedFd> = Vec::new();
        let mut foreign = false;
        // SAFETY: walking the headers with the kernel macros within msg_controllen,
        // which recvmsg set according to the facts; SCM_RIGHTS data is an int array.
        unsafe {
            let mut cmsg = libc::CMSG_FIRSTHDR(&msg);
            while !cmsg.is_null() {
                if (*cmsg).cmsg_level == libc::SOL_SOCKET && (*cmsg).cmsg_type == libc::SCM_RIGHTS {
                    let data = libc::CMSG_DATA(cmsg);
                    let len = (*cmsg).cmsg_len as usize - (data as usize - cmsg as usize);
                    let count = len / std::mem::size_of::<libc::c_int>();
                    for i in 0..count {
                        let raw = std::ptr::read_unaligned(
                            data.cast::<libc::c_int>().add(i),
                        );
                        if raw >= 0 {
                            owned.push(OwnedFd::from_raw_fd(raw));
                        }
                    }
                } else {
                    foreign = true;
                }
                cmsg = libc::CMSG_NXTHDR(&msg, cmsg);
            }
        }
        if msg.msg_flags & libc::MSG_CTRUNC != 0 {
            return Err(ProtoError::Fds("control data truncated (MSG_CTRUNC)".into()));
        }
        if foreign {
            return Err(ProtoError::Fds("control data is not SCM_RIGHTS".into()));
        }
        if owned.len() > MAX_FDS {
            return Err(ProtoError::Fds(format!(
                "{} descriptors exceed the limit {MAX_FDS}",
                owned.len()
            )));
        }
        let n = n as usize;
        if n == 0 {
            return Ok(None);
        }
        if n > MAX_FRAME {
            return Err(ProtoError::TooLarge(n));
        }
        buf.truncate(n);
        return Ok(Some((buf, owned)));
    }
}

// ============================================================================
// Checks
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::fd::AsFd;

    /// A stub operation until B1 (`privilege::op`).
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    enum TestOp {
        Install { packages: Vec<String> },
        Reboot,
    }

    type Client = ClientMsg<TestOp>;

    fn all_client() -> Vec<Client> {
        vec![
            ClientMsg::Hello(Hello {
                build_id: build_id(),
            }),
            ClientMsg::Request {
                id: 1,
                op: TestOp::Install {
                    packages: vec!["plasma-desktop".into()],
                },
            },
            ClientMsg::Request {
                id: 2,
                op: TestOp::Reboot,
            },
            ClientMsg::Cancel { id: 1 },
        ]
    }

    fn all_helper() -> Vec<HelperMsg> {
        let mut v = vec![
            HelperMsg::Welcome {
                build_id: build_id(),
            },
            HelperMsg::Progress {
                id: 1,
                line: "(1/3) установка «пакета»…".into(), // i18n-exempt: non-ASCII progress text must survive the frame
            },
            HelperMsg::Done {
                id: 1,
                ok: true,
                code: Some(0),
                message_key: None,
            },
            HelperMsg::Done {
                id: 2,
                ok: false,
                code: None,
                message_key: Some("priv.killed".into()),
            },
            HelperMsg::Busy { id: 3 },
        ];
        for code in [
            ErrorCode::Refused,
            ErrorCode::Revoked,
            ErrorCode::BuildMismatch,
            ErrorCode::Sequence,
            ErrorCode::Cancelled,
            ErrorCode::Internal,
        ] {
            v.push(HelperMsg::Error {
                id: Some(4),
                code,
                message_key: "k".into(),
            });
            v.push(HelperMsg::Error {
                id: None,
                code,
                message_key: "k".into(),
            });
        }
        v
    }

    #[test]
    fn round_trip_all_messages() {
        for m in all_client() {
            let back: Client = decode(&encode(&m).unwrap()).unwrap();
            assert_eq!(back, m);
        }
        for m in all_helper() {
            let back: HelperMsg = decode(&encode(&m).unwrap()).unwrap();
            assert_eq!(back, m);
        }
    }

    #[test]
    fn frame_shape_is_stable() {
        let bytes = encode(&Client::Cancel { id: 7 }).unwrap();
        assert_eq!(
            std::str::from_utf8(&bytes).unwrap(),
            r#"{"v":1,"msg":{"cancel":{"id":7}}}"#
        );
        let bytes = encode(&HelperMsg::Error {
            id: None,
            code: ErrorCode::BuildMismatch,
            message_key: "k".into(),
        })
        .unwrap();
        assert_eq!(
            std::str::from_utf8(&bytes).unwrap(),
            r#"{"v":1,"msg":{"error":{"id":null,"code":"build_mismatch","message_key":"k"}}}"#
        );
    }

    #[test]
    fn too_large_rejected_both_ways() {
        let big = HelperMsg::Progress {
            id: 1,
            line: "x".repeat(MAX_FRAME),
        };
        assert!(matches!(encode(&big), Err(ProtoError::TooLarge(_))));
        let raw = vec![b' '; MAX_FRAME + 1];
        assert!(matches!(
            decode::<HelperMsg>(&raw),
            Err(ProtoError::TooLarge(_))
        ));
        assert!(matches!(
            send_frame_dummy(&raw),
            Err(ProtoError::TooLarge(_))
        ));
    }

    fn send_frame_dummy(frame: &[u8]) -> Result<(), ProtoError> {
        let (a, _b) = seqpacket_pair().unwrap();
        send_frame(a.as_fd(), frame)
    }

    #[test]
    fn unknown_fields_rejected() {
        let cases = [
            // in the envelope
            r#"{"v":1,"msg":{"cancel":{"id":1}},"x":1}"#,
            // in the message
            r#"{"v":1,"msg":{"cancel":{"id":1,"argv":["sh"]}}}"#,
            // in Hello (a separate structure)
            r#"{"v":1,"msg":{"hello":{"build_id":"a","uid":0}}}"#,
            // in the operation
            r#"{"v":1,"msg":{"request":{"id":1,"op":{"install":{"packages":[],"flags":"-oX"}}}}}"#,
            // in the request next to the operation
            r#"{"v":1,"msg":{"request":{"id":1,"op":"reboot","shell":"x"}}}"#,
        ];
        for c in cases {
            let r = decode::<Client>(c.as_bytes());
            assert!(matches!(r, Err(ProtoError::Malformed(_))), "{c}: {r:?}");
        }
        let r = decode::<HelperMsg>(br#"{"v":1,"msg":{"busy":{"id":1,"pid":5}}}"#);
        assert!(matches!(r, Err(ProtoError::Malformed(_))));
    }

    #[test]
    fn missing_fields_and_unknown_variants_rejected() {
        let cases = [
            r#"{"v":1}"#,
            r#"{"v":1,"msg":{"cancel":{}}}"#,
            r#"{"v":1,"msg":{"exec":{"argv":["id"]}}}"#,
            r#"{"v":1,"msg":{"request":{"id":1,"op":"rm_rf"}}}"#,
            r#"{"v":1,"msg":{"request":{"id":-1,"op":"reboot"}}}"#,
        ];
        for c in cases {
            let r = decode::<Client>(c.as_bytes());
            assert!(matches!(r, Err(ProtoError::Malformed(_))), "{c}: {r:?}");
        }
        let r = decode::<HelperMsg>(
            br#"{"v":1,"msg":{"error":{"id":null,"code":"oops","message_key":"k"}}}"#,
        );
        assert!(matches!(r, Err(ProtoError::Malformed(_))));
    }

    #[test]
    fn garbage_and_cut_frames_rejected() {
        let good = encode(&Client::Request {
            id: 9,
            op: TestOp::Reboot,
        })
        .unwrap();
        for cut in 1..good.len() {
            let r = decode::<Client>(&good[..cut]);
            assert!(r.is_err(), "a cut at byte {cut} was accepted: {r:?}");
        }
        for junk in [
            &b""[..],
            b"\0\0\0",
            b"\xff\xfe{",
            b"[1,2]",
            b"\"hello\"",
            b"null",
        ] {
            assert!(decode::<Client>(junk).is_err(), "{junk:?}");
        }
    }

    #[test]
    fn version_checked() {
        for (c, want) in [
            (r#"{"v":2,"msg":{"cancel":{"id":1}}}"#, Some(2)),
            (r#"{"v":0,"msg":{"cancel":{"id":1}}}"#, Some(0)),
            (r#"{"msg":{"cancel":{"id":1}}}"#, None),
            (r#"{"v":"1","msg":{"cancel":{"id":1}}}"#, None),
            // a newer version with an unknown message is still "version"
            (r#"{"v":2,"msg":{"something_new":{}}}"#, Some(2)),
        ] {
            match decode::<Client>(c.as_bytes()) {
                Err(ProtoError::Version(v)) => assert_eq!(v, want, "{c}"),
                other => panic!("{c}: {other:?}"),
            }
        }
    }

    #[test]
    fn hello_check_and_error_classes() {
        let own = build_id();
        assert!(!own.is_empty());
        assert_eq!(
            check_hello(
                &Hello {
                    build_id: own.clone()
                },
                &own
            ),
            Ok(())
        );
        assert_eq!(
            check_hello(
                &Hello {
                    build_id: "0.0.1+deadbeef".into()
                },
                &own
            ),
            Err(ErrorCode::BuildMismatch)
        );
        // Z5: a check refusal and a failure are different classes, cancellation is its own.
        assert_eq!(ErrorCode::Refused.class(), ErrorClass::Refusal);
        assert_eq!(ErrorCode::Revoked.class(), ErrorClass::Refusal);
        assert_eq!(ErrorCode::BuildMismatch.class(), ErrorClass::Refusal);
        assert_eq!(ErrorCode::Sequence.class(), ErrorClass::Refusal);
        assert_eq!(ErrorCode::Cancelled.class(), ErrorClass::Cancelled);
        assert_eq!(ErrorCode::Internal.class(), ErrorClass::Failure);
    }

    #[test]
    fn socketpair_is_cloexec_seqpacket() {
        let (a, b) = seqpacket_pair().unwrap();
        for fd in [&a, &b] {
            // SAFETY: fcntl/getsockopt on live descriptors.
            let flags = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFD) };
            assert!(flags & libc::FD_CLOEXEC != 0);
            let mut ty: libc::c_int = 0;
            let mut len = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
            let rc = unsafe {
                libc::getsockopt(
                    fd.as_raw_fd(),
                    libc::SOL_SOCKET,
                    libc::SO_TYPE,
                    (&mut ty as *mut libc::c_int).cast(),
                    &mut len,
                )
            };
            assert_eq!(rc, 0);
            assert_eq!(ty, libc::SOCK_SEQPACKET);
        }
    }

    /// A live exchange over socketpair in two threads: handshake, request,
    /// progress, result, then EOF at the helper when the window drops its end.
    #[test]
    fn socketpair_dialogue_two_threads() {
        let (win, helper) = seqpacket_pair().unwrap();
        let h = std::thread::spawn(move || {
            let fd = helper.as_fd();
            let Some(Client::Hello(hello)) = recv_msg::<Client>(fd).unwrap() else {
                panic!("Hello must come first");
            };
            check_hello(&hello, &build_id()).unwrap();
            send_msg(
                fd,
                &HelperMsg::Welcome {
                    build_id: build_id(),
                },
            )
            .unwrap();
            let mut seen = Vec::new();
            while let Some(m) = recv_msg::<Client>(fd).unwrap() {
                if let Client::Request { id, .. } = &m {
                    send_msg(
                        fd,
                        &HelperMsg::Progress {
                            id: *id,
                            line: "step".into(),
                        },
                    )
                    .unwrap();
                    send_msg(
                        fd,
                        &HelperMsg::Done {
                            id: *id,
                            ok: true,
                            code: Some(0),
                            message_key: None,
                        },
                    )
                    .unwrap();
                }
                seen.push(m);
            }
            seen // EOF: the window closed its end
        });
        let fd = win.as_fd();
        send_msg(
            fd,
            &Client::Hello(Hello {
                build_id: build_id(),
            }),
        )
        .unwrap();
        assert_eq!(
            recv_msg::<HelperMsg>(fd).unwrap(),
            Some(HelperMsg::Welcome {
                build_id: build_id()
            })
        );
        let req = Client::Request {
            id: 5,
            op: TestOp::Install {
                packages: vec!["a".into()],
            },
        };
        send_msg(fd, &req).unwrap();
        assert_eq!(
            recv_msg::<HelperMsg>(fd).unwrap(),
            Some(HelperMsg::Progress {
                id: 5,
                line: "step".into()
            })
        );
        assert_eq!(
            recv_msg::<HelperMsg>(fd).unwrap(),
            Some(HelperMsg::Done {
                id: 5,
                ok: true,
                code: Some(0),
                message_key: None
            })
        );
        drop(win);
        assert_eq!(h.join().unwrap(), vec![req]);
    }

    /// Too large and corrupted frames arriving over a real socket: the length is
    /// recognized by `MSG_TRUNC`, not by the truncated part; after a frame of
    /// 64 KB + 1 the next normal frame is read separately (boundaries intact).
    #[test]
    fn socketpair_oversize_and_garbage() {
        let (a, b) = seqpacket_pair().unwrap();
        let writer = std::thread::spawn(move || {
            let fd = a.as_fd();
            let huge = vec![b'{'; MAX_FRAME + 1];
            // SAFETY: bypassing send_frame — we imitate a foreign writer.
            let n = unsafe {
                libc::send(
                    fd.as_raw_fd(),
                    huge.as_ptr().cast(),
                    huge.len(),
                    libc::MSG_NOSIGNAL,
                )
            };
            assert_eq!(n as usize, huge.len());
            send_frame(fd, b"{\"v\":1,\"msg\":{\"cancel\":{\"id\":1}").unwrap(); // truncation
            send_frame(fd, br#"{"v":1,"msg":{"cancel":{"id":1}},"evil":true}"#).unwrap();
            send_msg(fd, &Client::Cancel { id: 2 }).unwrap();
            a // keep the end open until reading is done
        });
        let fd = b.as_fd();
        match recv_msg::<Client>(fd) {
            Err(ProtoError::TooLarge(n)) => assert_eq!(n, MAX_FRAME + 1),
            other => panic!("{other:?}"),
        }
        assert!(matches!(
            recv_msg::<Client>(fd),
            Err(ProtoError::Malformed(_))
        ));
        assert!(matches!(
            recv_msg::<Client>(fd),
            Err(ProtoError::Malformed(_))
        ));
        assert_eq!(
            recv_msg::<Client>(fd).unwrap(),
            Some(Client::Cancel { id: 2 })
        );
        let a = writer.join().unwrap();
        drop(a);
        assert_eq!(recv_msg::<Client>(fd).unwrap(), None);
    }

    /// Writing to a socket whose peer has gone is an error, not a SIGPIPE.
    #[test]
    fn send_to_closed_peer_is_error() {
        let (a, b) = seqpacket_pair().unwrap();
        drop(b);
        assert!(matches!(
            send_msg(a.as_fd(), &Client::Cancel { id: 1 }),
            Err(ProtoError::Io(_))
        ));
    }

    // ---- frame with descriptors ----

    fn pipe_cloexec() -> (OwnedFd, OwnedFd) {
        let mut fds = [0; 2];
        // SAFETY: an array of two ints for pipe2.
        assert_eq!(unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) }, 0);
        // SAFETY: pipe2 returned two new fds.
        unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) }
    }

    fn is_cloexec(fd: BorrowedFd<'_>) -> bool {
        // SAFETY: F_GETFD on a live fd.
        let flags = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFD) };
        flags >= 0 && flags & libc::FD_CLOEXEC != 0
    }

    /// Wait for EOF on the reading end of the pipe: it comes only when ALL
    /// copies of the writing end are closed — this checks that the helper kept
    /// none of them.
    fn pipe_sees_eof(r: BorrowedFd<'_>) -> bool {
        let mut pfd = libc::pollfd { fd: r.as_raw_fd(), events: libc::POLLIN, revents: 0 };
        // SAFETY: a single pollfd.
        let rc = unsafe { libc::poll(&mut pfd, 1, 5000) };
        if rc <= 0 {
            return false;
        }
        let mut b = [0u8; 1];
        // SAFETY: a one-byte buffer.
        unsafe { libc::read(r.as_raw_fd(), b.as_mut_ptr().cast(), 1) == 0 }
    }

    /// Raw sendmsg with any number of fds — to send more than MAX_FDS bypassing
    /// the sender's check (as a foreign peer would).
    fn raw_send_fds(sock: BorrowedFd<'_>, frame: &[u8], fds: &[libc::c_int]) {
        let mut control = vec![0u64; cmsg_space_words(fds.len())];
        let mut iov = libc::iovec { iov_base: frame.as_ptr() as *mut _, iov_len: frame.len() };
        // SAFETY: as in send_frame_with_fds.
        unsafe {
            let mut msg: libc::msghdr = std::mem::zeroed();
            msg.msg_iov = &mut iov;
            msg.msg_iovlen = 1;
            msg.msg_control = control.as_mut_ptr().cast();
            msg.msg_controllen = libc::CMSG_SPACE(std::mem::size_of_val(fds) as u32) as _;
            let cmsg = libc::CMSG_FIRSTHDR(&msg);
            (*cmsg).cmsg_level = libc::SOL_SOCKET;
            (*cmsg).cmsg_type = libc::SCM_RIGHTS;
            (*cmsg).cmsg_len = libc::CMSG_LEN(std::mem::size_of_val(fds) as u32) as _;
            std::ptr::copy_nonoverlapping(fds.as_ptr().cast::<u8>(), libc::CMSG_DATA(cmsg), std::mem::size_of_val(fds));
            assert_eq!(libc::sendmsg(sock.as_raw_fd(), &msg, libc::MSG_NOSIGNAL), frame.len() as isize);
        }
    }

    /// The window in one thread sends a frame with a file and the end of a pipe,
    /// the helper in another receives it: the frame is intact, the fds work and
    /// have CLOEXEC, the order is preserved.
    #[test]
    fn fds_two_threads_cloexec_and_order() {
        let (a, b) = seqpacket_pair().unwrap();
        let (pr, pw) = pipe_cloexec();
        let dir = std::env::temp_dir().join(format!("rexon-proto-fds-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("f");
        std::fs::write(&path, b"theme-bytes").unwrap();
        let file = std::fs::File::open(&path).unwrap();
        let frame = encode(&Client::Cancel { id: 7 }).unwrap();
        let writer = std::thread::spawn(move || {
            send_frame_with_fds(a.as_fd(), &frame, &[file.as_fd(), pw.as_fd()]).unwrap();
            // The sender closes its copies — the helper keeps its own.
            drop(file);
            drop(pw);
            a
        });
        let (got, fds) = recv_frame_with_fds(b.as_fd()).unwrap().unwrap();
        assert_eq!(decode::<Client>(&got).unwrap(), Client::Cancel { id: 7 });
        assert_eq!(fds.len(), 2);
        assert!(fds.iter().all(|f| is_cloexec(f.as_fd())), "received fds must be CLOEXEC");
        let mut f = std::fs::File::from(fds.into_iter().next().unwrap());
        let mut s = String::new();
        std::io::Read::read_to_string(&mut f, &mut s).unwrap();
        assert_eq!(s, "theme-bytes");
        let _a = writer.join().unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        // The second received fd (pw) is already closed together with the vector — EOF.
        assert!(pipe_sees_eof(pr.as_fd()));
    }

    /// Old and new functions understand each other: a frame without fds is read
    /// by recv_frame_with_fds with an empty list, an empty list is an ordinary send.
    #[test]
    fn fds_compatible_with_plain_frames() {
        let (a, b) = seqpacket_pair().unwrap();
        send_msg(a.as_fd(), &Client::Cancel { id: 1 }).unwrap();
        let (frame, fds) = recv_frame_with_fds(b.as_fd()).unwrap().unwrap();
        assert!(fds.is_empty());
        assert_eq!(decode::<Client>(&frame).unwrap(), Client::Cancel { id: 1 });
        send_frame_with_fds(a.as_fd(), &encode(&Client::Cancel { id: 2 }).unwrap(), &[]).unwrap();
        assert_eq!(recv_msg::<Client>(b.as_fd()).unwrap(), Some(Client::Cancel { id: 2 }));
        drop(a);
        assert!(recv_frame_with_fds(b.as_fd()).unwrap().is_none());
    }

    /// The sender does not send more than MAX_FDS or an empty frame with fds.
    #[test]
    fn fds_sender_limits() {
        let (a, _b) = seqpacket_pair().unwrap();
        let (_r, w) = pipe_cloexec();
        let many: Vec<BorrowedFd<'_>> = (0..=MAX_FDS).map(|_| w.as_fd()).collect();
        assert!(matches!(send_frame_with_fds(a.as_fd(), b"{}", &many), Err(ProtoError::Fds(_))));
        assert!(matches!(send_frame_with_fds(a.as_fd(), b"", &[w.as_fd()]), Err(ProtoError::Malformed(_))));
        let big = vec![b' '; MAX_FRAME + 1];
        assert!(matches!(send_frame_with_fds(a.as_fd(), &big, &[w.as_fd()]), Err(ProtoError::TooLarge(_))));
    }

    /// A foreign peer sends MAX_FDS+2 descriptors: the kernel truncates the
    /// ancillary data (MSG_CTRUNC), receiving is an error, and NOT A SINGLE fd
    /// remains at the receiver (the pipe sees EOF once the sender has closed its
    /// copies).
    #[test]
    fn fds_ctrunc_rejected_and_all_closed() {
        let (a, b) = seqpacket_pair().unwrap();
        let (pr, pw) = pipe_cloexec();
        let writer = std::thread::spawn(move || {
            let raws = vec![pw.as_raw_fd(); MAX_FDS + 2];
            raw_send_fds(a.as_fd(), b"{\"v\":1}", &raws);
            drop(pw);
            a
        });
        let r = recv_frame_with_fds(b.as_fd());
        assert!(matches!(r, Err(ProtoError::Fds(ref e)) if e.contains("MSG_CTRUNC")), "{r:?}");
        let _a = writer.join().unwrap();
        assert!(pipe_sees_eof(pr.as_fd()), "received fds must be closed");
    }

    /// A frame over the limit together with fds is an error, the fds are closed.
    #[test]
    fn fds_with_oversize_frame_closed() {
        let (a, b) = seqpacket_pair().unwrap();
        // The default send buffer may be smaller; enlarge it.
        let sz: libc::c_int = 4 * MAX_FRAME as libc::c_int;
        // SAFETY: setsockopt with an int.
        unsafe {
            libc::setsockopt(a.as_raw_fd(), libc::SOL_SOCKET, libc::SO_SNDBUF, (&sz as *const libc::c_int).cast(), 4);
        }
        let (pr, pw) = pipe_cloexec();
        let big = vec![b' '; MAX_FRAME + 10];
        raw_send_fds(a.as_fd(), &big, &[pw.as_raw_fd()]);
        drop(pw);
        assert!(matches!(recv_frame_with_fds(b.as_fd()), Err(ProtoError::TooLarge(n)) if n == MAX_FRAME + 10));
        assert!(pipe_sees_eof(pr.as_fd()));
    }
}
