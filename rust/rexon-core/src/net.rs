//! Network transport of the appearance catalogs — port of the HTTP layer from
//! `rexon/appearance/ocs_client.py` (`_http_get`/`_http_download`) and
//! `rexon/appearance/aur_client.py` (`_http_json`), where Python uses the stdlib
//! `urllib`.
//!
//! HTTP used to stay in Python on purpose: the core was built without network
//! dependencies, and response parsing (`ocs.rs`, `aur.rs`) was covered by golden
//! tests on recorded answers. The decision has been revised — the goal "remove
//! Python from everywhere except the interface" requires the core to make the
//! request.
//!
//! The structure is exactly as in the other ports with side effects (`ImpIo` in
//! `importers.rs`, `ComplianceIo` in `compliance.rs`): all the LOGIC — which URL
//! to hit, what counts as success, when to fall back to the cache, when to
//! delete a partially downloaded file — lives in the core and is tested, while
//! the call itself is hidden behind the [`NetIo`] trait. In parity tests the
//! trait answers with RECORDED responses (`tests/golden/*`), in production it is
//! [`UreqNet`] on top of `ureq` + native-tls.
//!
//! One open response and one open file at a time is not a simplification but an
//! exact copy of the original: `_http_download` fits entirely into one
//! `with urlopen(...) as resp:` and one `with open(dest,"wb") as f:`.

use std::collections::HashMap;
use std::io::{Read, Write};

/// Log message level — the original's `log.info` / `log.warning` / `log.error`.
/// Behavior does not depend on it, but the texts are visible in `rexon.log`, so
/// they are part of the parity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Info,
    Warning,
    Error,
}

impl LogLevel {
    /// Name of the `logging` method the message goes to the Python log with.
    pub fn logger_method(self) -> &'static str {
        match self {
            LogLevel::Info => "info",
            LogLevel::Warning => "warning",
            LogLevel::Error => "error",
        }
    }
}

/// A network failure — port of the exceptions the original catches (OCS) or
/// re-raises (AUR).
///
/// Python catches `urllib.error.HTTPError` (an answer outside 2xx after
/// redirects are followed) and `urllib.error.URLError`/`OSError` (DNS, connection
/// refused, timeout, TLS). [`Display`](std::fmt::Display) gives THE SAME string
/// form as `str(e)` in Python, because in the AUR branch this text reaches the
/// buyer's window (`_AurWorker.failed.emit(str(e))`), and in OCS it goes into
/// the log.
///
/// The form depends on WHERE it broke, and the original is inconsistent here: a
/// connection that never happened is a `URLError`, whose `str(e)` puts the
/// message in brackets (`<urlopen error …>`), while a break of an ongoing read
/// is a bare `OSError` without brackets. So the formatting is the transport
/// implementation's job ([`UreqNet`] wraps exactly a connection failure in
/// brackets), and `detail` is stored and printed as is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetError {
    /// An answer with a status outside 200..=299 (port of `HTTPError`).
    Status { code: u16, reason: String },
    /// The connection did not happen or broke (port of `URLError`/`OSError`).
    Transport { detail: String },
}

impl std::fmt::Display for NetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            // str(HTTPError) == "HTTP Error {code}: {reason}"
            NetError::Status { code, reason } => write!(f, "HTTP Error {code}: {reason}"),
            NetError::Transport { detail } => write!(f, "{detail}"),
        }
    }
}

impl std::error::Error for NetError {}

impl NetError {
    /// A human wording for the buyer's window (rule #13: no jargon and no Latin
    /// terms), in the interface language. The original showed urllib's raw `str(e)`
    /// — a string like `<urlopen error [Errno -2] Name or service not known>`, from
    /// which an ordinary person understands nothing.
    ///
    /// This is an ADDITION, not a replacement: [`Display`](std::fmt::Display) stays
    /// byte-compatible with Python so that the parity of logs and tests does not
    /// drift, and the UI layer is free to show `human()`.
    pub fn human(&self) -> String {
        match self {
            NetError::Status { code, reason } => {
                if reason.is_empty() {
                    // TRANSLATORS: %1 is an HTTP status code (404, 503…).
                    crate::i18n::trf("The catalog server returned error %1", &[code])
                } else {
                    // TRANSLATORS: %1 is an HTTP status code, %2 the server's reason text (in English).
                    crate::i18n::trf("The catalog server returned error %1 (%2)", &[code, reason])
                }
            }
            NetError::Transport { .. } => crate::i18n::tr(
                "Couldn't reach the catalog server — check your internet connection",
            ),
        }
    }
}

/// Transport of the network layer: each method is exactly one side effect of the
/// Python original, in execution order. In parity tests — a replay of a journal,
/// in production — [`UreqNet`].
///
/// There is exactly one header (`User-Agent`) — as in
/// `urllib.request.Request(url, headers={"User-Agent": USER_AGENT})`; the timeout
/// in seconds — as the argument of `urlopen(..., timeout=...)` (in urllib this
/// is a socket timeout: both for connecting and for every read).
pub trait NetIo {
    /// `with urlopen(Request(url, headers), timeout) as resp: resp.read()` — the
    /// whole response body.
    fn get(&mut self, url: &str, user_agent: &str, timeout_secs: u64) -> Result<Vec<u8>, NetError>;

    /// Entering `with urlopen(...) as resp` for a streaming download.
    /// `Ok(total)` — `int(resp.headers.get("Content-Length", 0))`.
    ///
    /// Warning — a fix to the original (rule #16): in Python a non-numeric
    /// Content-Length gave a `ValueError` that `_http_download` does NOT catch (it
    /// catches only `URLError`/`HTTPError`/`OSError`) — the exception flew out of
    /// the worker and broke the theme installation. Here an unparsed header = `0`,
    /// the same as its absence: the size check simply does not trigger.
    fn open(&mut self, url: &str, user_agent: &str, timeout_secs: u64) -> Result<i64, NetError>;

    /// `resp.read(n)` from the open response; an empty vector — the body is over.
    fn read(&mut self, n: usize) -> Result<Vec<u8>, NetError>;

    /// Leaving `with urlopen(...)` — close the current response.
    fn close(&mut self);

    /// `os.makedirs(path, exist_ok=True)`.
    fn makedirs(&mut self, path: &str);

    /// `open(path, "wb")`.
    fn create(&mut self, path: &str) -> Result<(), NetError>;

    /// `f.write(chunk)` into the open file.
    fn write(&mut self, chunk: &[u8]) -> Result<(), NetError>;

    /// `f.close()` / leaving `with open(...)`.
    fn close_file(&mut self);

    /// `os.path.exists(path)`.
    fn exists(&mut self, path: &str) -> bool;

    /// `os.remove(path)`; the caller suppresses the error, as in the original.
    fn remove(&mut self, path: &str);

    /// `on_progress(done, total)` — called ONLY when a callback was passed.
    fn progress(&mut self, done: i64, total: i64);

    /// `log.warning` / `log.error` with an already assembled text.
    fn log(&mut self, level: LogLevel, message: &str);

    /// `time.sleep(ms / 1000)` — a pause before a retry (see [`with_retry`]).
    ///
    /// A side effect rather than `std::thread::sleep` inside the core, for exactly
    /// the same reason as everything else in this trait: in a check the pause must be
    /// VISIBLE and take no time, while in production it must be real.
    fn sleep(&mut self, ms: u64);

    /// `resp.headers.get("Content-Type", "")` for the response opened last by
    /// [`open`](NetIo::open); empty — there is no header or no response is open.
    ///
    /// A separate method rather than a field of `open`'s return value, because there
    /// is exactly one reader — the desktop screenshot loader, which needs the type
    /// BEFORE reading the body: an error page (HTML instead of a picture) can be
    /// megabytes, and there is no point in downloading it entirely only to throw it
    /// away.
    fn content_type(&mut self) -> String;
}

// ============================================================================
// Retries on a temporary failure
// ============================================================================
//
// Why (Wehrwolfmann 2026-08-02): one and the same class of failure — the server
// closed the connection midway — was handled in ONE place out of three. The
// catalog preview loader got a retry (otherwise `images.pling.com` killed a
// batch of cards at once), the desktop screenshot loader retried only on "you
// are asking too often" (HTTP 429), and the core's network layer itself —
// through which the detail gallery, theme downloads and AUR requests go —
// retried nothing. So a single broken session silently cost the buyer a theme
// or a picture.
//
// Rule #12: we fix the shared mechanism. The decision "is it worth trying
// again" and "how long to wait" lives HERE, as one set of rules for all of the
// core's requests, and the pause goes to [`NetIo::sleep`] — i.e. it is visible
// in checks and does not burn test time.

/// How many times in total to try a request: the first attempt plus two retries.
///
/// Three — because we cure a SINGLE break (the server closed the connection, a
/// mirror blinked). If it did not help twice in a row, the problem is not a
/// "blink", and further retries only keep the buyer in front of a spinner.
pub const RETRY_ATTEMPTS: u32 = 3;

/// Pause before the FIRST retry of an ordinary failure; after that it grows as a
/// multiple of the retry number (700 ms, 1400 ms). Less — the broken connection
/// does not have time to close; more — the buyer notices.
pub const RETRY_DELAY_MS: u64 = 700;

/// Pause before the first retry on HTTP 429 "you are asking too often" (3 s,
/// 6 s). Here the server explicitly asks to wait, and it will not accept 700 ms
/// — the value is inherited from the screenshot loader, where this branch
/// already worked.
pub const RATE_LIMIT_DELAY_MS: u64 = 3_000;

/// Whether it is worth trying again after such a failure.
///
/// Temporary — a connection break, a timeout, a DNS failure (all of these are
/// [`NetError::Transport`]), as well as "I am busy right now" answers: 408, 429
/// and the whole 5xx range. Final — 4xx (the file really does not exist, access
/// is denied: a retry gives exactly the same answer) and a CANCELLATION triggered
/// by the interface itself: repeating a request we no longer need is exactly the
/// harm cancellation was made to eliminate.
pub fn is_temporary(e: &NetError) -> bool {
    match e {
        NetError::Transport { detail } => !detail.contains(CANCELLED_DETAIL),
        NetError::Status { code, .. } => {
            *code == 408 || *code == 429 || (500..600).contains(code)
        }
    }
}

/// Pause before retry number `retry` (the first retry is one).
pub fn retry_delay_ms(retry: u32, e: &NetError) -> u64 {
    let base = match e {
        NetError::Status { code: 429, .. } => RATE_LIMIT_DELAY_MS,
        _ => RETRY_DELAY_MS,
    };
    base * u64::from(retry.max(1))
}

/// Whether to retry this request at all.
///
/// A separate type rather than a boolean flag, so that the call site says WHY: a
/// retry is safe only for a request that changes nothing on the server. Today
/// the core sends only GETs, but the choice must stay explicit — the very first
/// POST repeated "while we are at it" would mean a second purchase, a second
/// vote, a second order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Retry {
    /// Total attempts, counting the first. One — no retries.
    pub attempts: u32,
}

impl Retry {
    /// The request changes nothing on the server (GET/HEAD) — retrying is safe.
    pub const IDEMPOTENT: Retry = Retry { attempts: RETRY_ATTEMPTS };

    /// No retries: the request changes state on the other side, or the cost of an
    /// extra call is higher than the cost of a refusal.
    pub const NEVER: Retry = Retry { attempts: 1 };
}

/// Execute `op`, repeating it on a temporary failure.
///
/// `what` — how to name the request in the log (URL and request kind); the text
/// of a failed attempt goes out as a warning, and the final refusal is worded by
/// the caller with its own message, as before.
///
/// Warning: `op` must be a COMPLETE operation, not its tail: a file download is
/// repeated from the very beginning (the response is opened anew, the file is
/// created anew and overwritten), because appending the second attempt to the
/// end of the first one's stub is a broken archive, not a fix.
pub fn with_retry<IO, T>(
    io: &mut IO,
    policy: Retry,
    what: &str,
    mut op: impl FnMut(&mut IO) -> Result<T, NetError>,
) -> Result<T, NetError>
where
    IO: NetIo + ?Sized,
{
    let total = policy.attempts.max(1);
    let mut attempt: u32 = 1;
    loop {
        let e = match op(io) {
            Ok(v) => return Ok(v),
            Err(e) => e,
        };
        if attempt >= total || !is_temporary(&e) {
            return Err(e);
        }
        let wait = retry_delay_ms(attempt, &e);
        io.log(
            LogLevel::Warning,
            &format!(
                "{what}: попытка {attempt} из {total} не удалась ({e}) — \
                 повтор через {wait} мс"
            ),
        );
        io.sleep(wait);
        attempt += 1;
    }
}

// ============================================================================
// Cancelling a request that has already gone out
// ============================================================================

/// A "this request is no longer needed" flag shared by the UI thread and the
/// worker thread.
///
/// Why (Wehrwolfmann 2026-07-31): a generation of answers saves the interface —
/// the catalog of a left category will not redraw the open one — but the request
/// ITSELF lived on until the end and occupied the channel all that time.
/// Measured on the live application: a dozen category switches in a row gave up
/// to 13 simultaneous requests, and the same request that takes 0.95 s alone
/// stretched to 12.3 s. So a departing request must not be "forgotten" but cut.
///
/// The token is triggered from ANY thread (`cancel`), and the transport notices
/// it inside the connection (see [`CancelTransport`]): waiting for the answer is
/// split into short slices, and the flag is checked between them. Having noticed
/// the cancellation, the transport returns an error, `ureq` drops the request,
/// the connection is closed.
#[derive(Clone, Debug, Default)]
pub struct CancelToken(std::sync::Arc<std::sync::atomic::AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    /// Mark the request as unneeded. Also affects a request already in progress.
    pub fn cancel(&self) {
        self.0.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(std::sync::atomic::Ordering::SeqCst)
    }
}

/// How often the cancellation flag is checked while waiting for an answer. The
/// delay of reacting to a cancellation does not exceed this period; there is no
/// point in poking the socket more often.
const CANCEL_POLL: std::time::Duration = std::time::Duration::from_millis(200);

/// The text by which a cancelled request is told apart from a network failure.
pub const CANCELLED_DETAIL: &str = "запрос отменён";

/// Never: THE ERROR KIND HERE IS NOT FORMATTING BUT BEHAVIOR. Never
/// `ErrorKind::Interrupted`: the standard library by contract IGNORES this kind
/// and retries the read (`Read::read_to_end`, `io::copy` and all their
/// relatives). A cancelled transport with this error kind got an eternal hot
/// loop — the thread burned a CPU core at 100 % and never ended.
///
/// Outside this showed up like this: a person closes the window, and Rexon does
/// not close. The tab's background task sat in this loop, the exit waited for
/// it, and the watchdog finished the process off. Caught on 19.09.2026 during a
/// self-check of the main window: 9 runs out of 12 did not finish by
/// themselves.
///
/// `ConnectionAborted` is also what happened in meaning: we cut the connection
/// ourselves. Cancellation is told apart from a network failure not by the error
/// kind but by the [`CANCELLED_DETAIL`] text, and that is preserved.
fn cancelled_io() -> ureq::Error {
    ureq::Error::Io(std::io::Error::new(
        std::io::ErrorKind::ConnectionAborted,
        CANCELLED_DETAIL,
    ))
}

/// A link of the connection chain: wraps the already assembled transport
/// (together with TLS) into [`CancelTransport`]. It stands BETWEEN TCP and TLS
/// (see `UreqNet::agent`): the waiting slice expires at the socket, and the
/// encryption does not see it.
#[derive(Debug)]
struct CancelConnector {
    token: CancelToken,
}

impl<In: ureq::unversioned::transport::Transport> ureq::unversioned::transport::Connector<In>
    for CancelConnector
{
    type Out = CancelTransport;

    fn connect(
        &self,
        _details: &ureq::unversioned::transport::ConnectionDetails,
        chained: Option<In>,
    ) -> Result<Option<Self::Out>, ureq::Error> {
        let Some(inner) = chained else {
            return Ok(None);
        };
        if self.token.is_cancelled() {
            return Err(cancelled_io());
        }
        Ok(Some(CancelTransport {
            inner: inner.boxed(),
            token: self.token.clone(),
        }))
    }
}

/// A connection that can drop waiting on an external signal.
#[derive(Debug)]
struct CancelTransport {
    inner: Box<dyn ureq::unversioned::transport::Transport>,
    token: CancelToken,
}

impl ureq::unversioned::transport::Transport for CancelTransport {
    fn buffers(&mut self) -> &mut dyn ureq::unversioned::transport::Buffers {
        self.inner.buffers()
    }

    fn transmit_output(
        &mut self,
        amount: usize,
        timeout: ureq::unversioned::transport::NextTimeout,
    ) -> Result<(), ureq::Error> {
        if self.token.is_cancelled() {
            return Err(cancelled_io());
        }
        self.inner.transmit_output(amount, timeout)
    }

    fn await_input(
        &mut self,
        timeout: ureq::unversioned::transport::NextTimeout,
    ) -> Result<bool, ureq::Error> {
        // We wait for the answer in slices of CANCEL_POLL: an expired slice is
        // not yet the request's timeout, merely a reason to check the cancellation
        // flag. We measure the real timeout ourselves against the overall
        // deadline given by ureq.
        let budget: std::time::Duration = *timeout.after;
        let deadline = std::time::Instant::now().checked_add(budget);
        loop {
            if self.token.is_cancelled() {
                return Err(cancelled_io());
            }
            let left = match deadline {
                Some(d) => d.saturating_duration_since(std::time::Instant::now()),
                None => budget,
            };
            if left.is_zero() {
                return Err(ureq::Error::Timeout(timeout.reason));
            }
            let slice = left.min(CANCEL_POLL);
            let sliced = ureq::unversioned::transport::NextTimeout {
                after: ureq::unversioned::transport::time::Duration::Exact(slice),
                reason: timeout.reason,
            };
            match self.inner.await_input(sliced) {
                Err(ureq::Error::Timeout(_)) if slice < left => continue,
                other => return other,
            }
        }
    }

    fn is_open(&mut self) -> bool {
        !self.token.is_cancelled() && self.inner.is_open()
    }

    fn is_tls(&self) -> bool {
        self.inner.is_tls()
    }
}

// ============================================================================
// Tolerance of a crooked response header
// ============================================================================

/// A placeholder name of the same length as `Content-Length` (14 bytes) — it
/// replaces the name of a header whose value could not be parsed.
const BAD_LEN_NAME: &[u8] = b"X-Rexon-Badlen";

/// Maximum number of header bytes held back for the check. Beyond that the
/// response is passed as is — `ureq` will report a header that is too long itself
/// (`LargeResponseHeader`), there is no point in substituting its diagnosis.
const HEAD_SCAN_LIMIT: usize = 64 * 1024;

/// End of the response header — the position right AFTER the empty line.
fn head_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4)
}

/// Defuse a `Content-Length` that cannot be parsed: `Some(header)` — a fixed
/// copy, `None` — nothing to fix (the usual case, no copy is made).
///
/// TWO cases are fixed, and both mimic how Python's `http.client` behaves: a
/// value that is not a number (`length = None` → the body until the end of the
/// stream) and a repeated header (the first one is taken, the rest are not
/// looked at). The name is replaced with [`BAD_LEN_NAME`] of the same length, so
/// byte offsets do not shift, and `ureq` sees a response without a declared
/// length.
fn fix_bad_content_length(head: &[u8]) -> Option<Vec<u8>> {
    const NAME: &[u8] = b"content-length";
    let mut fixed: Option<Vec<u8>> = None;
    let mut line_start = 0usize;
    let mut first_line = true;
    let mut length_seen = false;
    while line_start < head.len() {
        let len = head[line_start..]
            .windows(2)
            .position(|w| w == b"\r\n")
            .unwrap_or(head.len() - line_start);
        let line = &head[line_start..line_start + len];
        if line.is_empty() {
            break; // an empty line — the header is over
        }
        if !first_line {
            if let Some(colon) = line.iter().position(|&b| b == b':') {
                let name = &line[..colon];
                if name.len() == NAME.len() && name.eq_ignore_ascii_case(NAME) {
                    let value = std::str::from_utf8(&line[colon + 1..])
                        .ok()
                        .map(str::trim);
                    let usable =
                        !length_seen && value.is_some_and(|v| v.parse::<u64>().is_ok());
                    if usable {
                        length_seen = true;
                    } else {
                        let buf = fixed.get_or_insert_with(|| head.to_vec());
                        buf[line_start..line_start + name.len()].copy_from_slice(BAD_LEN_NAME);
                    }
                }
            }
        }
        first_line = false;
        line_start += len + 2;
    }
    fixed
}

/// A link of the connection chain: wraps the transport into [`HeadFixTransport`].
#[derive(Debug)]
struct HeadFixConnector;

impl<In: ureq::unversioned::transport::Transport> ureq::unversioned::transport::Connector<In>
    for HeadFixConnector
{
    type Out = HeadFixTransport<In>;

    fn connect(
        &self,
        details: &ureq::unversioned::transport::ConnectionDetails,
        chained: Option<In>,
    ) -> Result<Option<Self::Out>, ureq::Error> {
        let Some(inner) = chained else {
            return Ok(None);
        };
        Ok(Some(HeadFixTransport::new(inner, details.config)))
    }
}

/// A connection that shows `ureq` the response header only as a whole — and
/// before that defuses a `Content-Length` that would make parsing fail.
///
/// Why (Wehrwolfmann 2026-08-01): a non-numeric `Content-Length` is fatal for
/// `ureq` — the header is rejected with a `BadContentLengthHeader` error EVEN
/// BEFORE the response reaches our code, and the theme installation fails
/// entirely. `ureq` 3.3 has no setting to tolerate this (all of `ConfigBuilder`
/// was checked, there is no newer version). Python is tolerant here:
/// `http.client` with an unparsable length sets `length = None` and reads the
/// body until the connection closes — exactly what the [`NetIo::open`] contract
/// promises ("an unparsed header = 0"). Mirrors and proxies send crooked
/// lengths, and the price is a theme that does not download, so tolerance is
/// restored at ONE point for the whole network layer (the OCS catalog, AUR,
/// previews, archives), not in one call.
///
/// Structure: while the header is being read, bytes accumulate in the
/// connection buffer and are not passed up; the header collected as a whole is
/// checked by [`fix_bad_content_length`] and — only if there was something to
/// fix — replaced with a fixed copy. A healthy response goes as a raw byte
/// transfer.
#[derive(Debug)]
struct HeadFixTransport<T: ureq::unversioned::transport::Transport> {
    inner: T,
    /// Buffers `ureq` sees (the connection has its own).
    buffers: ureq::unversioned::transport::LazyBuffers,
    /// The fixed header, not yet passed up entirely.
    pending: Vec<u8>,
    /// How many bytes of `pending` have already been passed.
    sent: usize,
    /// The response header has not been passed yet.
    scanning: bool,
}

impl<T: ureq::unversioned::transport::Transport> HeadFixTransport<T> {
    fn new(inner: T, config: &ureq::config::Config) -> Self {
        HeadFixTransport {
            inner,
            buffers: ureq::unversioned::transport::LazyBuffers::new(
                config.input_buffer_size(),
                config.output_buffer_size(),
            ),
            pending: Vec::new(),
            sent: 0,
            scanning: true,
        }
    }

    /// Pass up as much of the fixed header as fits.
    fn drain_pending(&mut self) -> usize {
        use ureq::unversioned::transport::Buffers;
        if self.pending.is_empty() {
            return 0;
        }
        let dst = self.buffers.input_append_buf();
        let n = (self.pending.len() - self.sent).min(dst.len());
        dst[..n].copy_from_slice(&self.pending[self.sent..self.sent + n]);
        self.buffers.input_appended(n);
        self.sent += n;
        if self.sent == self.pending.len() {
            self.pending.clear();
            self.sent = 0;
        }
        n
    }

    /// Raw transfer of bytes from the connection buffer upwards.
    fn copy_raw(&mut self) -> usize {
        use ureq::unversioned::transport::Buffers;
        let Self { inner, buffers, .. } = self;
        let n = {
            let src = inner.buffers().input();
            let dst = buffers.input_append_buf();
            let n = src.len().min(dst.len());
            dst[..n].copy_from_slice(&src[..n]);
            n
        };
        if n > 0 {
            buffers.input_appended(n);
            inner.buffers().input_consume(n);
        }
        n
    }

    /// Pass up what can be passed: the tail of the fixed header, the whole parsed
    /// header or raw body bytes. `0` — "the header has not been collected yet, data
    /// is needed".
    fn pump(&mut self) -> usize {
        let moved = self.drain_pending();
        if moved > 0 || !self.pending.is_empty() {
            return moved;
        }
        if self.scanning {
            match head_end(self.inner.buffers().input()) {
                Some(end) => {
                    let fixed = fix_bad_content_length(&self.inner.buffers().input()[..end]);
                    self.scanning = false;
                    if let Some(head) = fixed {
                        self.inner.buffers().input_consume(end);
                        self.pending = head;
                        self.sent = 0;
                        return self.drain_pending();
                    }
                }
                None => {
                    let grown = self.inner.buffers().input().len();
                    // There is nowhere further to accumulate (the connection buffer is
                    // full) or the header outgrew a reasonable limit — we pass it as is.
                    let stuck = self.inner.buffers().input_append_buf().is_empty();
                    if !stuck && grown <= HEAD_SCAN_LIMIT {
                        return 0;
                    }
                    self.scanning = false;
                }
            }
        }
        self.copy_raw()
    }
}

impl<T: ureq::unversioned::transport::Transport> ureq::unversioned::transport::Transport
    for HeadFixTransport<T>
{
    fn buffers(&mut self) -> &mut dyn ureq::unversioned::transport::Buffers {
        &mut self.buffers
    }

    fn transmit_output(
        &mut self,
        amount: usize,
        timeout: ureq::unversioned::transport::NextTimeout,
    ) -> Result<(), ureq::Error> {
        use ureq::unversioned::transport::Buffers;
        // A new request has started — a new response header lies ahead.
        self.scanning = true;
        self.pending.clear();
        self.sent = 0;
        let mut done = 0usize;
        while done < amount {
            let n = {
                let Self { inner, buffers, .. } = self;
                let src = &buffers.output()[done..amount];
                let dst = inner.buffers().output();
                let n = src.len().min(dst.len());
                dst[..n].copy_from_slice(&src[..n]);
                n
            };
            if n == 0 {
                return Err(ureq::Error::Io(std::io::Error::other(
                    "буфер соединения не принимает запрос",
                )));
            }
            self.inner.transmit_output(n, timeout)?;
            done += n;
        }
        Ok(())
    }

    fn await_input(
        &mut self,
        timeout: ureq::unversioned::transport::NextTimeout,
    ) -> Result<bool, ureq::Error> {
        loop {
            if self.pump() > 0 {
                return Ok(true);
            }
            // Nothing to pass — waiting for data from the connection.
            if !self.inner.await_input(timeout)? {
                // The stream ended in the middle of the header: we pass what was
                // accumulated as is, and let `ureq` decide from there.
                self.scanning = false;
                return Ok(self.pump() > 0);
            }
        }
    }

    fn is_open(&mut self) -> bool {
        self.inner.is_open()
    }

    fn is_tls(&self) -> bool {
        self.inner.is_tls()
    }
}

// ============================================================================
// Live implementation: ureq + native-tls
// ============================================================================

/// The production transport: synchronous `ureq` on top of rustls with trust
/// roots from the system store.
///
/// The settings are tuned to match urllib: the timeout applies to the connection
/// AND to every read/write (in urllib this is a socket timeout, not one for the
/// whole request), no more than ten redirects
/// (`HTTPRedirectHandler.max_redirections`), only 2xx counts as success
/// (`urlopen` raises `HTTPError` on other codes). Agents are cached by timeout so
/// that connections are reused between the requests of one batch.
pub struct UreqNet {
    agents: HashMap<u64, ureq::Agent>,
    reader: Option<Box<dyn Read + Send + Sync>>,
    /// `Content-Type` of the open response — captured in [`NetIo::open`] while the
    /// header is still at hand.
    ctype: String,
    file: Option<std::fs::File>,
    progress: Option<ProgressCb>,
    sink: Option<LogCb>,
    cancel: CancelToken,
    /// Headers besides `User-Agent`, the same for all requests of this transport.
    /// They are deliberately not in the [`NetIo`] trait: the urllib originals have
    /// exactly one header, and an extra parameter in every method would be needed
    /// by one caller out of six. The only consumer today is the GitHub API, which
    /// asks for `Accept: application/vnd.github+json` (the set of pairs is given by
    /// `engine_update::api_headers`).
    extra_headers: Vec<(String, String)>,
}

/// Download progress callback — the original's `on_progress(done, total)`.
type ProgressCb = Box<dyn FnMut(i64, i64) + Send>;
/// Log sink — the original's `log.warning`/`log.error`.
type LogCb = Box<dyn FnMut(LogLevel, &str) + Send>;

impl Default for UreqNet {
    fn default() -> Self {
        Self::new()
    }
}

impl UreqNet {
    pub fn new() -> Self {
        UreqNet {
            agents: HashMap::new(),
            reader: None,
            ctype: String::new(),
            file: None,
            progress: None,
            sink: None,
            cancel: CancelToken::new(),
            extra_headers: Vec::new(),
        }
    }

    /// Cancellation token of this transport: triggered from outside (from the UI
    /// thread) and cuts a request that has already gone out. Each [`UreqNet`] holds
    /// ITS OWN token — cancelling one request does not affect its neighbors.
    pub fn cancel_token(&self) -> CancelToken {
        self.cancel.clone()
    }

    /// Take a ready token (for example one shared by a batch of requests of one
    /// category). Call BEFORE the first request: agents are cached together with
    /// the token.
    pub fn with_cancel(mut self, token: CancelToken) -> Self {
        self.cancel = token;
        self.agents.clear();
        self
    }

    /// Headers besides `User-Agent` — as "name, value" pairs, as the core named
    /// them. There is no point in putting `User-Agent` here: it comes as an argument
    /// of every request and will be set on top.
    pub fn with_headers(mut self, headers: Vec<(String, String)>) -> Self {
        self.extra_headers = headers;
        self
    }

    /// Download progress callback (the original's `on_progress`).
    pub fn with_progress(mut self, cb: impl FnMut(i64, i64) + Send + 'static) -> Self {
        self.progress = Some(Box::new(cb));
        self
    }

    /// Log sink (by default messages are discarded).
    pub fn with_log(mut self, cb: impl FnMut(LogLevel, &str) + Send + 'static) -> Self {
        self.sink = Some(Box::new(cb));
        self
    }

    /// Certificates are verified ONLY against the system trust store.
    ///
    /// Why not rustls: in `ureq` 2.x the `tls` feature hard-pulls `webpki-roots`
    /// (`dep:webpki-roots` right in the feature definition) — a snapshot of
    /// Mozilla's list of root authorities embedded in the binary, under MPL-2.0.
    /// Rule #10 allows only permissive licenses, and an embedded list also ages
    /// together with our release: revocation of an authority would reach the buyer
    /// only with the next build. `native-tls` on Linux takes roots from the system
    /// store, which the distribution itself updates.
    ///
    /// Warning: `native-tls` is NEVER picked up as the default (`ntls.rs` is marked
    /// "is never picked up as a default"), so the connector is set explicitly here —
    /// without it `ureq` without the `tls` feature refuses any https.
    fn agent(&mut self, timeout_secs: u64) -> ureq::Agent {
        let token = self.cancel.clone();
        self.agents
            .entry(timeout_secs)
            .or_insert_with(|| {
                let t = std::time::Duration::from_secs(timeout_secs);
                // The timeouts are tuned to urllib: there it is a SOCKET timeout —
                // both for connecting and for every wait for data, not for the whole
                // request. So we do NOT set a global timeout.
                let config = ureq::Agent::config_builder()
                    .timeout_connect(Some(t))
                    .timeout_send_request(Some(t))
                    .timeout_recv_response(Some(t))
                    .timeout_recv_body(Some(t))
                    .max_redirects(10)
                    // We parse an answer outside 2xx ourselves (below), as before: that
                    // way there is one path both for "ureq returned a status error" and
                    // for "hit the redirect limit and got a 3xx".
                    .http_status_as_error(false)
                    .tls_config(
                        ureq::tls::TlsConfig::builder()
                            .provider(ureq::tls::TlsProvider::NativeTls)
                            // Trust roots — ONLY the system store (native-tls takes them from
                            // the distribution's OpenSSL). The default variant (WebPki) in
                            // ureq 3 not only embeds Mozilla's list under MPL-2.0 but is not
                            // available at all: our `native-tls-webpki-roots` feature is
                            // disabled, and such a config panics.
                            .root_certs(ureq::tls::RootCerts::PlatformVerifier)
                            .build(),
                    )
                    .build();
                // Our own links in the connection chain: one makes the request
                // cancellable (CancelConnector), the other tolerant of a crooked
                // response header (HeadFixConnector).
                //
                // Never: cancellation goes UNDER the encryption, on the bare socket,
                // not on top of TLS. Slicing the wait into CANCEL_POLL slices on top
                // of native-tls broke the connection: OpenSSL considers a slice that
                // expires in the middle of a read a fatal error, and the next read
                // gives "OpenSSL error". Sites answering faster than 200 ms did not
                // notice this, while Wallhaven behind Cloudflare (~0.4–1 s) never
                // opened once (16.11.2026; probe: our client 0 of 3, ureq without
                // the link 3 of 3). Under TLS the slice expires at the socket, and
                // the encryption does not see it. The order of links is as in
                // ureq::DefaultConnector (without socks: the feature is off), with
                // our link between TCP and TLS.
                use ureq::unversioned::transport::Connector as _;
                let connector = ()
                    .chain(ureq::unversioned::transport::ConnectProxyConnector::default())
                    .chain(ureq::unversioned::transport::TcpConnector::default())
                    .chain(CancelConnector { token })
                    .chain(ureq::unversioned::transport::NativeTlsConnector::default())
                    .chain(HeadFixConnector);
                ureq::Agent::with_parts(
                    config,
                    connector,
                    ureq::unversioned::resolver::DefaultResolver::default(),
                )
            })
            .clone()
    }

    fn call(
        &mut self,
        url: &str,
        user_agent: &str,
        timeout_secs: u64,
    ) -> Result<ureq::http::Response<ureq::Body>, NetError> {
        let agent = self.agent(timeout_secs);
        let mut req = agent.get(url).header("User-Agent", user_agent);
        for (name, value) in &self.extra_headers {
            req = req.header(name, value);
        }
        match req.call() {
            Ok(resp) => {
                // ureq returns Ok on a 3xx too when it hits the redirect limit;
                // urlopen raises HTTPError at this point — we bring it to that.
                let status = resp.status();
                let code = status.as_u16();
                if (200..300).contains(&code) {
                    Ok(resp)
                } else {
                    Err(NetError::Status {
                        code,
                        reason: status.canonical_reason().unwrap_or("").to_string(),
                    })
                }
            }
            Err(ureq::Error::StatusCode(code)) => Err(NetError::Status {
                code,
                reason: ureq::http::StatusCode::from_u16(code)
                    .ok()
                    .and_then(|s| s.canonical_reason())
                    .unwrap_or("")
                    .to_string(),
            }),
            // The connection did not happen — in Python this is URLError, whose
            // str() puts the reason in brackets: `<urlopen error [Errno -2] …>`.
            Err(e) => Err(NetError::Transport {
                detail: format!("<urlopen error {e}>"),
            }),
        }
    }
}

/// An error of the sink's file operations and of a read break, as a
/// [`NetError`]: the original catches `OSError` from `open`/`write`/`resp.read`
/// with the same `except` as network failures, but prints it WITHOUT the
/// `<urlopen error …>` brackets — so the text is taken as is.
fn io_err(e: &std::io::Error) -> NetError {
    NetError::Transport {
        detail: e.to_string(),
    }
}

impl UreqNet {
    /// A body read error TAKING CANCELLATION INTO ACCOUNT.
    ///
    /// Warning: `ureq` shows a cancelled read with ITS OWN error ("Peer
    /// disconnected"): it is our own transport that closes the connection, and our
    /// text does not reach the body reader. Passing it on as is would mean two
    /// problems at once: the person closing a tab is shown a network failure that
    /// did not happen, and [`is_temporary`] considers the cancelled request a
    /// temporary problem and RETRIES it — exactly what cancellation was made to
    /// prevent. A cancellation must stay a cancellation on every stretch, not only
    /// while waiting for the header.
    fn body_err(cancelled: bool, e: &std::io::Error) -> NetError {
        if cancelled {
            return NetError::Transport {
                detail: format!("<urlopen error {CANCELLED_DETAIL}>"),
            };
        }
        io_err(e)
    }
}

impl NetIo for UreqNet {
    fn get(&mut self, url: &str, user_agent: &str, timeout_secs: u64) -> Result<Vec<u8>, NetError> {
        let resp = self.call(url, user_agent, timeout_secs)?;
        let mut buf = Vec::new();
        let outcome = resp.into_body().into_reader().read_to_end(&mut buf);
        if let Err(e) = outcome {
            return Err(Self::body_err(self.cancel.is_cancelled(), &e));
        }
        Ok(buf)
    }

    fn open(&mut self, url: &str, user_agent: &str, timeout_secs: u64) -> Result<i64, NetError> {
        let resp = self.call(url, user_agent, timeout_secs)?;
        let total = resp
            .headers()
            .get(ureq::http::header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse::<i64>().ok())
            .unwrap_or(0);
        self.ctype = resp
            .headers()
            .get(ureq::http::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        self.reader = Some(Box::new(resp.into_body().into_reader()));
        Ok(total)
    }

    fn read(&mut self, n: usize) -> Result<Vec<u8>, NetError> {
        let Some(reader) = self.reader.as_mut() else {
            return Ok(Vec::new());
        };
        let mut buf = vec![0u8; n];
        // The socket's read() returns "how much arrived", while `resp.read(n)`
        // in Python returns up to n bytes, but no fewer than one, until the body
        // is over.
        let outcome = reader.read(&mut buf);
        let got = match outcome {
            Ok(got) => got,
            Err(e) => return Err(Self::body_err(self.cancel.is_cancelled(), &e)),
        };
        buf.truncate(got);
        Ok(buf)
    }

    fn close(&mut self) {
        self.reader = None;
        self.ctype.clear();
    }

    fn makedirs(&mut self, path: &str) {
        let _ = std::fs::create_dir_all(path);
    }

    fn create(&mut self, path: &str) -> Result<(), NetError> {
        let f = std::fs::File::create(path).map_err(|e| io_err(&e))?;
        self.file = Some(f);
        Ok(())
    }

    fn write(&mut self, chunk: &[u8]) -> Result<(), NetError> {
        let Some(f) = self.file.as_mut() else {
            return Ok(());
        };
        f.write_all(chunk).map_err(|e| io_err(&e))
    }

    fn close_file(&mut self) {
        self.file = None;
    }

    fn exists(&mut self, path: &str) -> bool {
        std::path::Path::new(path).exists()
    }

    fn remove(&mut self, path: &str) {
        let _ = std::fs::remove_file(path);
    }

    fn progress(&mut self, done: i64, total: i64) {
        if let Some(cb) = self.progress.as_mut() {
            cb(done, total);
        }
    }

    fn log(&mut self, level: LogLevel, message: &str) {
        if let Some(cb) = self.sink.as_mut() {
            cb(level, message);
        }
        // The shared Rexon log — ALWAYS, with and without a sink: the sink
        // shows it to the person, the log keeps it for analysis. Without this
        // line the reason of a failure was lost: "Could not download the
        // archive" without a single line about what the network said
        // (16.11.2026, KDE Store wallpapers of 135 MiB).
        match level {
            LogLevel::Info => crate::log_info!("net", "{message}"),
            LogLevel::Warning => crate::log_warn!("net", "{message}"),
            LogLevel::Error => crate::log_error!("net", "{message}"),
        }
    }

    /// Pause before a retry. Waiting here is safe: the production transport is
    /// called from worker threads (the tabs' QThread workers), the GIL is released
    /// for that time. A cancelled request does not get here — [`is_temporary`] cuts
    /// it off.
    fn sleep(&mut self, ms: u64) {
        std::thread::sleep(std::time::Duration::from_millis(ms));
    }

    fn content_type(&mut self) -> String {
        self.ctype.clone()
    }
}

// ============================================================================
// Tests of tolerance of a crooked header
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    const UA: &str = "Rexon-test";

    /// A server for one request: accepts a connection, reads the request to the end
    /// and sends `parts` in pieces (with a pause between pieces, so that the header
    /// is guaranteed not to arrive in a single read), after which it closes the
    /// connection.
    fn serve_once(parts: Vec<Vec<u8>>) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").expect("слушатель");
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let Ok((mut sock, _)) = listener.accept() else {
                return;
            };
            let mut head = Vec::new();
            let mut byte = [0u8; 1];
            while head.len() < 8192 {
                match sock.read(&mut byte) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => head.push(byte[0]),
                }
                if head.ends_with(b"\r\n\r\n") {
                    break;
                }
            }
            for (i, part) in parts.iter().enumerate() {
                if i > 0 {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                if sock.write_all(part).is_err() {
                    return;
                }
                let _ = sock.flush();
            }
        });
        port
    }

    /// A server with a persistent connection: answers EVERY incoming request with
    /// the next answer from `answers`, without closing the socket. This checks that
    /// the header is parsed anew on every response rather than once per connection.
    fn serve_keepalive(answers: Vec<Vec<u8>>) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").expect("слушатель");
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let mut left = answers.into_iter();
            while let Ok((mut sock, _)) = listener.accept() {
                loop {
                    let mut head = Vec::new();
                    let mut byte = [0u8; 1];
                    while head.len() < 8192 {
                        match sock.read(&mut byte) {
                            Ok(0) | Err(_) => break,
                            Ok(_) => head.push(byte[0]),
                        }
                        if head.ends_with(b"\r\n\r\n") {
                            break;
                        }
                    }
                    if !head.ends_with(b"\r\n\r\n") {
                        break; // the client went away
                    }
                    let Some(answer) = left.next() else { return };
                    if sock.write_all(&answer).is_err() {
                        return;
                    }
                    let _ = sock.flush();
                    if left.len() == 0 {
                        return; // the answers are over — close the connection
                    }
                }
            }
        });
        port
    }

    /// Download the whole body through the open response (as `ocs` does).
    fn drain(net: &mut UreqNet) -> Vec<u8> {
        let mut body = Vec::new();
        loop {
            let chunk = net.read(8192).expect("чтение тела");
            if chunk.is_empty() {
                break;
            }
            body.extend_from_slice(&chunk);
        }
        net.close();
        body
    }

    #[test]
    fn non_numeric_length_renamed() {
        let head = b"HTTP/1.1 200 OK\r\nContent-Type: x\r\nContent-Length: not-a-number\r\n\r\n";
        let fixed = fix_bad_content_length(head).expect("шапку надо было починить");
        assert_eq!(fixed.len(), head.len(), "смещения байт поехали");
        assert!(!fixed.windows(14).any(|w| w.eq_ignore_ascii_case(b"Content-Length")));
        assert!(fixed.windows(14).any(|w| w == BAD_LEN_NAME));
        assert!(fixed.ends_with(b"not-a-number\r\n\r\n"), "значение трогать незачем");
    }

    #[test]
    fn healthy_header_not_rewritten() {
        assert!(fix_bad_content_length(b"HTTP/1.1 200 OK\r\nContent-Length: 42\r\n\r\n").is_none());
        assert!(fix_bad_content_length(b"HTTP/1.1 200 OK\r\nContent-Type: x\r\n\r\n").is_none());
        // A request line with similar text does not count as a header.
        assert!(fix_bad_content_length(b"HTTP/1.1 200 Content-Length: no\r\n\r\n").is_none());
    }

    #[test]
    fn repeated_length_header_keeps_first() {
        let head = b"HTTP/1.1 200 OK\r\nContent-Length: 7\r\nContent-Length: 9\r\n\r\n";
        let fixed = fix_bad_content_length(head).expect("повтор надо было обезвредить");
        assert!(fixed.windows(18).any(|w| w == b"Content-Length: 7\r"));
        assert!(fixed.windows(18).any(|w| w == b"X-Rexon-Badlen: 9\r"));
    }

    #[test]
    fn empty_length_value_neutralized() {
        let head = b"HTTP/1.1 200 OK\r\nContent-Length:\r\n\r\n";
        assert!(fix_bad_content_length(head).is_some());
    }

    #[test]
    fn broken_content_length_does_not_break_download() {
        let body = b"rexon-tema".repeat(500);
        let mut resp = b"HTTP/1.1 200 OK\r\nContent-Length: not-a-number\r\n\r\n".to_vec();
        resp.extend_from_slice(&body);
        let port = serve_once(vec![resp]);
        let mut net = UreqNet::new();
        let total = net
            .open(&format!("http://127.0.0.1:{port}/tema.tar.xz"), UA, 25)
            .expect("ответ с кривой длиной обязан открыться");
        assert_eq!(total, 0, "неразобранный заголовок = 0 (контракт NetIo::open)");
        assert_eq!(drain(&mut net), body, "тело должно приехать целиком");
    }

    #[test]
    fn broken_header_assembled_from_chunks() {
        let body = b"kusochki".repeat(100);
        let mut tail = b"a-number\r\n\r\n".to_vec();
        tail.extend_from_slice(&body);
        // A break exactly in the middle of the Content-Length value.
        let port = serve_once(vec![
            b"HTTP/1.1 200 OK\r\nContent-Type: x\r\nContent-Length: not-".to_vec(),
            tail,
        ]);
        let mut net = UreqNet::new();
        let total = net
            .open(&format!("http://127.0.0.1:{port}/x"), UA, 25)
            .expect("шапка по кускам обязана собраться");
        assert_eq!(total, 0);
        assert_eq!(drain(&mut net), body);
    }

    #[test]
    fn keep_alive_header_parsed_every_time() {
        let ok = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\nпервый", "первый".len())
            .into_bytes();
        let bad = "HTTP/1.1 200 OK\r\nContent-Length: nope\r\nConnection: close\r\n\r\nвторой"
            .to_string()
            .into_bytes();
        let port = serve_keepalive(vec![ok, bad]);
        let mut net = UreqNet::new();
        let url = format!("http://127.0.0.1:{port}/x");
        assert_eq!(net.get(&url, UA, 25).expect("первый ответ"), "первый".as_bytes());
        // The second response on the same connection — with a crooked length.
        assert_eq!(net.get(&url, UA, 25).expect("второй ответ"), "второй".as_bytes());
    }


    // ========================================================================
    // Retries on a temporary failure
    // ========================================================================

    /// A dummy executor: counts attempts, returns prepared outcomes and records
    /// pauses instead of waiting them out.
    #[derive(Default)]
    struct Counter {
        /// What to return on the next attempt (the first one first).
        outcomes: Vec<Result<u8, NetError>>,
        tries: usize,
        slept: Vec<u64>,
        logs: Vec<String>,
    }

    impl Counter {
        fn run(&mut self, policy: Retry) -> Result<u8, NetError> {
            with_retry(self, policy, "проба", |io: &mut Counter| {
                // There may be fewer outcomes than attempts: the last one repeats.
                let out = io.outcomes[io.tries.min(io.outcomes.len() - 1)].clone();
                io.tries += 1;
                out
            })
        }
    }

    /// Only the methods [`with_retry`] touches; the others are not called.
    impl NetIo for Counter {
        fn get(&mut self, _: &str, _: &str, _: u64) -> Result<Vec<u8>, NetError> {
            unreachable!()
        }
        fn open(&mut self, _: &str, _: &str, _: u64) -> Result<i64, NetError> {
            unreachable!()
        }
        fn read(&mut self, _: usize) -> Result<Vec<u8>, NetError> {
            unreachable!()
        }
        fn close(&mut self) {}
        fn makedirs(&mut self, _: &str) {}
        fn create(&mut self, _: &str) -> Result<(), NetError> {
            unreachable!()
        }
        fn write(&mut self, _: &[u8]) -> Result<(), NetError> {
            unreachable!()
        }
        fn close_file(&mut self) {}
        fn exists(&mut self, _: &str) -> bool {
            false
        }
        fn remove(&mut self, _: &str) {}
        fn progress(&mut self, _: i64, _: i64) {}
        fn log(&mut self, _: LogLevel, message: &str) {
            self.logs.push(message.to_string());
        }
        fn sleep(&mut self, ms: u64) {
            self.slept.push(ms);
        }
        fn content_type(&mut self) -> String {
            String::new()
        }
    }

    fn broken() -> NetError {
        NetError::Transport { detail: "<urlopen error соединение сброшено>".into() }
    }

    #[test]
    fn connection_drop_retried_and_second_attempt_passes() {
        let mut io = Counter {
            outcomes: vec![Err(broken()), Ok(7)],
            ..Default::default()
        };
        assert_eq!(io.run(Retry::IDEMPOTENT), Ok(7));
        assert_eq!(io.tries, 2, "должна быть ровно одна повторная попытка");
        assert_eq!(io.slept, vec![RETRY_DELAY_MS]);
        assert_eq!(io.logs.len(), 1, "неудачная попытка обязана быть в журнале");
    }

    #[test]
    fn after_retries_exhausted_last_error_returned() {
        let mut io = Counter {
            outcomes: vec![Err(broken())],
            ..Default::default()
        };
        assert_eq!(io.run(Retry::IDEMPOTENT), Err(broken()));
        assert_eq!(io.tries, RETRY_ATTEMPTS as usize);
        // Pauses grow: 700 ms, 1400 ms — and there is one fewer of them than attempts.
        assert_eq!(io.slept, vec![RETRY_DELAY_MS, RETRY_DELAY_MS * 2]);
    }

    #[test]
    fn cancelled_request_not_retried() {
        let cancelled = NetError::Transport {
            detail: format!("<urlopen error {CANCELLED_DETAIL}>"),
        };
        let mut io = Counter {
            outcomes: vec![Err(cancelled.clone())],
            ..Default::default()
        };
        assert_eq!(io.run(Retry::IDEMPOTENT), Err(cancelled));
        assert_eq!(io.tries, 1, "запрос, который нам больше не нужен, не повторяют");
        assert!(io.slept.is_empty());
    }

    #[test]
    fn missing_file_not_retried_busy_server_retried() {
        for (code, want_tries) in [
            (404u16, 1),                      // no file — there will not be one the second time either
            (403, 1),                         // access denied
            (429, RETRY_ATTEMPTS as usize),   // "you are asking too often"
            (503, RETRY_ATTEMPTS as usize),   // the server is busy
            (500, RETRY_ATTEMPTS as usize),
        ] {
            let mut io = Counter {
                outcomes: vec![Err(NetError::Status { code, reason: String::new() })],
                ..Default::default()
            };
            let _ = io.run(Retry::IDEMPOTENT);
            assert_eq!(io.tries, want_tries, "код {code}");
        }
    }

    #[test]
    fn rate_limit_waits_longer_than_normal_failure() {
        let mut io = Counter {
            outcomes: vec![Err(NetError::Status { code: 429, reason: String::new() })],
            ..Default::default()
        };
        let _ = io.run(Retry::IDEMPOTENT);
        assert_eq!(io.slept, vec![RATE_LIMIT_DELAY_MS, RATE_LIMIT_DELAY_MS * 2]);
        // Exactly the same 3 s and 6 s that the screenshot loader waited before.
        assert_eq!(io.slept, vec![3_000, 6_000]);
    }

    #[test]
    fn no_retry_respected() {
        let mut io = Counter {
            outcomes: vec![Err(broken())],
            ..Default::default()
        };
        assert_eq!(io.run(Retry::NEVER), Err(broken()));
        assert_eq!(io.tries, 1, "не-идемпотентный запрос повторять нельзя");
        assert!(io.logs.is_empty(), "повторов не было — извещать не о чем");
    }

    #[test]
    fn first_try_success_waits_and_logs_nothing() {
        let mut io = Counter {
            outcomes: vec![Ok(1)],
            ..Default::default()
        };
        assert_eq!(io.run(Retry::IDEMPOTENT), Ok(1));
        assert_eq!(io.tries, 1);
        assert!(io.slept.is_empty() && io.logs.is_empty());
    }

    #[test]
    fn live_transport_returns_content_type() {
        let body = b"kartinka".repeat(100);
        let mut resp = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\n\r\n",
            body.len()
        )
        .into_bytes();
        resp.extend_from_slice(&body);
        let port = serve_once(vec![resp]);
        let mut net = UreqNet::new();
        net.open(&format!("http://127.0.0.1:{port}/x.png"), UA, 25)
            .expect("ответ");
        assert_eq!(net.content_type(), "image/png");
        drain(&mut net);
        assert_eq!(net.content_type(), "", "закрытый ответ типа не помнит");
    }

    #[test]
    fn healthy_response_passes_as_before() {
        let body = b"0123456789".repeat(3000); // more than one socket read
        let mut resp = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", body.len())
            .into_bytes();
        resp.extend_from_slice(&body);
        let port = serve_once(vec![resp]);
        let mut net = UreqNet::new();
        let total = net
            .open(&format!("http://127.0.0.1:{port}/x"), UA, 25)
            .expect("обычный ответ");
        assert_eq!(total, body.len() as i64);
        assert_eq!(drain(&mut net), body);
    }
}
