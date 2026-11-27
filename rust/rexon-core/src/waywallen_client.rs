//! Port of `rexon/waywallen_client.py` — a thin D-Bus client for the waywallen daemon.
//!
//! Rexon (frontend, workshop browser) tells the waywallen daemon (backend,
//! Rust Vulkan renderer) to "apply scene by ID" and reads its status. In Python
//! the transport is PySide6.QtDBus (`QDBusInterface` on the session bus); here ALL
//! of the client logic (property value coercions, parsing of `_call` replies,
//! the error format, the CLI semantics of `python3 -m tools.py_shims.waywallen_client`)
//! is separated from live I/O by the [`WaywallenIo`] trait:
//!
//! * the core does NOT pull in a D-Bus dependency (zbus etc.) — per the rexon-core
//!   minimal-dependency policy, the live transport will appear as a trait
//!   implementation on the bridge side when it is wired up;
//! * in parity tests the trait replays a log captured from live Python
//!   with QtDBus fully mocked (`waywallen_client_cases.json`).
//!
//! The client is strictly SYNCHRONOUS (every call is a blocking D-Bus round trip),
//! there are no async callbacks — the owner guards for delivering late replies in
//! `qt_wallpaper_engine/client.py` (the fix for the crash on dead QML objects)
//! belong to the WS client of the WE tab and do not apply to this module.
//!
//! Bugs of the original fixed during the port (auto-fix rule; every place is
//! marked "intentional divergence" next to the code):
//! * `int(prop or 0)` / `int(call or 0)` in Python crashed with an unhandled
//!   `ValueError` on a non-numeric string from the daemon and a `TypeError` on
//!   a multi-arg `Rescan` reply — here an unconvertible value gives 0;
//! * typing: string properties always return `String` (Python returned
//!   "whatever came in", e.g. an int despite the `-> str` annotation).

// ============================================================================
// D-Bus constants (BUS_NAME / OBJECT_PATH / IFACE)
// ============================================================================

/// Service name of the daemon on the session bus.
pub const BUS_NAME: &str = "org.waywallen.waywallen.Daemon";
/// Object path of the daemon.
pub const OBJECT_PATH: &str = "/org/waywallen/waywallen/Daemon";
/// Interface of the daemon.
pub const IFACE: &str = "org.waywallen.waywallen.Daemon1";

// ============================================================================
// D-Bus values
// ============================================================================

/// A value as Python saw it after the QVariant conversion: None / bool /
/// int / str. The daemon interface does not return larger types (double/arrays);
/// if they appear, extend the enum together with the golden data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DbusValue {
    None,
    Bool(bool),
    Int(i64),
    Str(String),
}

impl DbusValue {
    /// Python `bool(x)`.
    pub fn is_truthy(&self) -> bool {
        match self {
            DbusValue::None => false,
            DbusValue::Bool(b) => *b,
            DbusValue::Int(i) => *i != 0,
            DbusValue::Str(s) => !s.is_empty(),
        }
    }

    /// Python `str(x)` — how the value looks in the CLI f-string.
    pub fn py_str(&self) -> String {
        match self {
            DbusValue::None => "None".to_string(),
            DbusValue::Bool(true) => "True".to_string(),
            DbusValue::Bool(false) => "False".to_string(),
            DbusValue::Int(i) => i.to_string(),
            DbusValue::Str(s) => s.clone(),
        }
    }

    /// Python `repr(x)` — list elements in `str(list)`. Strings are quoted
    /// by the CPython rule: single quotes, but double if the string contains `'` and no
    /// `"`. Escaping of control characters is not implemented (daemon replies
    /// are identifiers/messages without them).
    fn py_repr(&self) -> String {
        match self {
            DbusValue::Str(s) => {
                if s.contains('\'') && !s.contains('"') {
                    format!("\"{s}\"")
                } else {
                    format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'"))
                }
            }
            other => other.py_str(),
        }
    }

    /// Python `int(x)` for truthy values: bool → 0/1, int → as is,
    /// string — literal parsing (surrounding whitespace, sign, `_` between digits).
    /// `None` = Python would have raised ValueError.
    pub fn py_int(&self) -> Option<i64> {
        match self {
            DbusValue::None => None,
            DbusValue::Bool(b) => Some(i64::from(*b)),
            DbusValue::Int(i) => Some(*i),
            DbusValue::Str(s) => py_parse_int(s),
        }
    }
}

/// Python `int(str)`: trims whitespace, allows a sign and single `_`
/// between digits.
fn py_parse_int(s: &str) -> Option<i64> {
    let t = s.trim();
    let digits = t.strip_prefix(['+', '-']).unwrap_or(t);
    if digits.is_empty() {
        return None;
    }
    // `_` is allowed only between digits (as in Python 3 literals).
    let bytes = digits.as_bytes();
    for (i, b) in bytes.iter().enumerate() {
        match b {
            b'0'..=b'9' => {}
            b'_' => {
                let prev_digit = i > 0 && bytes[i - 1].is_ascii_digit();
                let next_digit =
                    i + 1 < bytes.len() && bytes[i + 1].is_ascii_digit();
                if !prev_digit || !next_digit {
                    return None;
                }
            }
            _ => return None,
        }
    }
    t.replace('_', "").parse().ok()
}

/// A D-Bus call reply as Python saw it: `reply.errorName()`,
/// `reply.errorMessage()`, `reply.arguments()`. An empty `error_name` =
/// the call succeeded.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DbusReply {
    pub error_name: String,
    pub error_message: String,
    pub arguments: Vec<DbusValue>,
}

/// `_call`: `argv[0] if len(argv) == 1 else argv` — a single value or
/// a list (possibly empty).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplyValue {
    Single(DbusValue),
    Multi(Vec<DbusValue>),
}

impl ReplyValue {
    fn from_arguments(mut args: Vec<DbusValue>) -> Self {
        if args.len() == 1 {
            ReplyValue::Single(args.remove(0))
        } else {
            ReplyValue::Multi(args)
        }
    }

    /// Python `bool(x)`: a list is truthy if non-empty.
    fn is_truthy(&self) -> bool {
        match self {
            ReplyValue::Single(v) => v.is_truthy(),
            ReplyValue::Multi(list) => !list.is_empty(),
        }
    }

    /// Python `str(x)` — how the CLI prints the value (`print(c.next())`).
    pub fn py_str(&self) -> String {
        match self {
            ReplyValue::Single(v) => v.py_str(),
            ReplyValue::Multi(list) => {
                let items: Vec<String> =
                    list.iter().map(DbusValue::py_repr).collect();
                format!("[{}]", items.join(", "))
            }
        }
    }
}

/// D-Bus call error — port of the `RuntimeError` from `_call`. `message` is the same
/// text: `"{method}: {errorName} — {errorMessage}"`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallError {
    pub message: String,
}

impl std::fmt::Display for CallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CallError {}

// ============================================================================
// Transport trait
// ============================================================================

/// The `QDBusInterface` transport: each method is exactly one D-Bus side effect
/// of the Python original. In parity tests — a log replay; in the live bridge —
/// the real session bus (the implementation will appear when wired up).
pub trait WaywallenIo {
    /// `self._iface.isValid()` — the daemon is visible on the session bus.
    fn is_valid(&mut self) -> bool;
    /// `self._iface.property(name)`; [`DbusValue::None`] — no such property.
    fn get_property(&mut self, name: &str) -> DbusValue;
    /// `self._iface.call(method, *args)` → the raw reply.
    fn call(&mut self, method: &str, args: &[DbusValue]) -> DbusReply;
}

// ============================================================================
// Client
// ============================================================================

/// Thin D-Bus wrapper — port of the `WaywallenClient` class. The constructor does not
/// block — check [`available`](Self::available).
pub struct WaywallenClient<IO: WaywallenIo> {
    io: IO,
}

impl<IO: WaywallenIo> WaywallenClient<IO> {
    pub fn new(io: IO) -> Self {
        WaywallenClient { io }
    }

    /// Take the transport back (for parity tests — for the final checks).
    pub fn into_io(self) -> IO {
        self.io
    }

    /// True if the daemon is visible on the session bus (running and activated).
    pub fn available(&mut self) -> bool {
        self.io.is_valid()
    }

    /// `_call`: method call; a D-Bus error → [`CallError`] with the same text
    /// as the original's `RuntimeError`.
    fn call(
        &mut self,
        method: &str,
        args: &[DbusValue],
    ) -> Result<ReplyValue, CallError> {
        let reply = self.io.call(method, args);
        if !reply.error_name.is_empty() {
            return Err(CallError {
                message: format!(
                    "{method}: {} — {}",
                    reply.error_name, reply.error_message
                ),
            });
        }
        Ok(ReplyValue::from_arguments(reply.arguments))
    }

    /// `self._prop(name) or ""` — a string property. The Python intent is
    /// `-> str`; intentional divergence: a truthy value of a NON-string type
    /// is converted with `str()` (Python returned it as is despite the annotation;
    /// the observable CLI output via f-string is identical).
    fn str_prop(&mut self, name: &str) -> String {
        let v = self.io.get_property(name);
        if v.is_truthy() { v.py_str() } else { String::new() }
    }

    /// `int(self._prop(name) or 0)` — an integer property. Intentional
    /// divergence: a non-numeric string from the daemon gives 0 (Python crashed
    /// with an unhandled ValueError).
    fn int_prop(&mut self, name: &str) -> i64 {
        let v = self.io.get_property(name);
        if !v.is_truthy() {
            return 0;
        }
        v.py_int().unwrap_or(0)
    }

    /// The `CurrentWallpaperId` property.
    pub fn current_wallpaper_id(&mut self) -> String {
        self.str_prop("CurrentWallpaperId")
    }

    /// The `Version` property.
    pub fn version(&mut self) -> String {
        self.str_prop("Version")
    }

    /// The `WsPort` property.
    pub fn ws_port(&mut self) -> i64 {
        self.int_prop("WsPort")
    }

    /// The `QueueMode` property.
    pub fn queue_mode(&mut self) -> String {
        self.str_prop("QueueMode")
    }

    /// The `RotationSecs` property.
    pub fn rotation_secs(&mut self) -> i64 {
        self.int_prop("RotationSecs")
    }

    /// Apply a scene wallpaper; returns a task id (or an error message).
    /// The **main** integration method — the Apply button of config.qml calls it.
    /// Python did `str(wallpaper_id)` — here the coercion is fixed by the argument
    /// type (the caller converts a non-string id to a string).
    pub fn apply_by_id(
        &mut self,
        wallpaper_id: &str,
    ) -> Result<ReplyValue, CallError> {
        self.call("ApplyById", &[DbusValue::Str(wallpaper_id.to_string())])
    }

    /// Next wallpaper in the queue. Named like the D-Bus method `Next` and Python
    /// `next()` — this is not an iterator, name parity matters more than the lint.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Result<ReplyValue, CallError> {
        self.call("Next", &[])
    }

    /// Previous wallpaper.
    pub fn previous(&mut self) -> Result<ReplyValue, CallError> {
        self.call("Previous", &[])
    }

    /// Pause rendering.
    pub fn pause(&mut self) -> Result<(), CallError> {
        self.call("Pause", &[]).map(|_| ())
    }

    /// Resume rendering.
    pub fn resume(&mut self) -> Result<(), CallError> {
        self.call("Resume", &[]).map(|_| ())
    }

    /// `int(self._call("Rescan") or 0)` — id of the rescan task. Intentional
    /// divergence: an unconvertible reply (non-numeric string — ValueError;
    /// multi-arg list — TypeError in Python) gives 0.
    pub fn rescan(&mut self) -> Result<i64, CallError> {
        let rv = self.call("Rescan", &[])?;
        if !rv.is_truthy() {
            return Ok(0);
        }
        Ok(match rv {
            ReplyValue::Single(v) => v.py_int().unwrap_or(0),
            ReplyValue::Multi(_) => 0,
        })
    }

    /// Open the daemon's own UI.
    pub fn open_ui(&mut self) -> Result<(), CallError> {
        self.call("OpenUi", &[]).map(|_| ())
    }

    /// Shut down the daemon.
    pub fn quit(&mut self) -> Result<(), CallError> {
        self.call("Quit", &[]).map(|_| ())
    }

    /// Rotation interval in seconds (Python coerced `int(secs)` — fixed by the
    /// type).
    pub fn set_rotation_interval(
        &mut self,
        secs: i64,
    ) -> Result<(), CallError> {
        self.call("SetRotationInterval", &[DbusValue::Int(secs)])
            .map(|_| ())
    }

    /// Shuffle mode (Python coerced `bool(on)` — fixed by the type).
    pub fn set_shuffle(&mut self, on: bool) -> Result<(), CallError> {
        self.call("SetShuffle", &[DbusValue::Bool(on)]).map(|_| ())
    }
}

// ============================================================================
// CLI (port of `_cli` — a smoke test: status / apply / next / pause / …)
// ============================================================================

/// Result of a CLI run: exit code + captured streams (printing is
/// up to the caller).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliResult {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl CliResult {
    fn ok(stdout: String) -> Self {
        CliResult { code: 0, stdout, stderr: String::new() }
    }

    fn err(code: i32, stderr: String) -> Self {
        CliResult { code, stdout: String::new(), stderr }
    }
}

/// Port of `_cli()`: `prog` — `sys.argv[0]`, `args` — the arguments after it.
/// Texts/exit codes are byte-for-byte identical to Python (except the fixed
/// `rescan` crash, see [`WaywallenClient::rescan`]).
pub fn cli<IO: WaywallenIo>(
    client: &mut WaywallenClient<IO>,
    prog: &str,
    args: &[&str],
) -> CliResult {
    if !client.available() {
        return CliResult::err(
            1,
            "error: waywallen daemon not running on session-bus\n\
             Hint: запустите ~/.local/share/waywallen/waywallen.AppImage\n"
                .to_string(),
        );
    }

    if args.is_empty() {
        // Status
        let version = client.version();
        let current = client.current_wallpaper_id();
        let current = if current.is_empty() {
            "(none)".to_string()
        } else {
            current
        };
        let stdout = format!(
            "Version    : {version}\n\
             Current ID : {current}\n\
             WS port    : {}\n\
             Queue mode : {}\n\
             Rotation   : {}s\n",
            client.ws_port(),
            client.queue_mode(),
            client.rotation_secs(),
        );
        return CliResult::ok(stdout);
    }

    let action = args[0];
    let result: Result<String, CallError> = match action {
        "apply" if args.len() >= 2 => client
            .apply_by_id(args[1])
            .map(|r| format!("applied: {}\n", r.py_str())),
        "next" => client.next().map(|r| format!("{}\n", r.py_str())),
        "previous" => client.previous().map(|r| format!("{}\n", r.py_str())),
        "pause" => client.pause().map(|_| "paused\n".to_string()),
        "resume" => client.resume().map(|_| "resumed\n".to_string()),
        "rescan" => {
            client.rescan().map(|id| format!("rescan task id: {id}\n"))
        }
        "open-ui" => client.open_ui().map(|_| "UI opened\n".to_string()),
        _ => {
            return CliResult::err(
                2,
                format!(
                    "usage: {prog} [apply <id>|next|previous|pause|resume|\
                     rescan|open-ui]\n"
                ),
            );
        }
    };
    match result {
        Ok(stdout) => CliResult::ok(stdout),
        Err(e) => CliResult::err(1, format!("error: {e}\n")),
    }
}
