//! Live executor of the login video splash — the Rust implementation of the trait
//! [`crate::session_splash::SplashIo`].
//!
//! The core (`rexon-core::session_splash`) drives the WHOLE splash flow: scanning
//! Steam splashes, the showcase frame, config/favorites/rotation, the KSplash hybrid,
//! the KWin rule, autostart and all three run paths together with all the
//! waits and their thresholds. This module only executes what the core
//! decided: the file system, subprocesses (mpv/ffmpeg/systemctl/
//! kwriteconfig6), mpv-IPC unix sockets and sd_notify, the flock of the «one
//! run per session» lock, its own X11 layer (a fullscreen override-redirect window,
//! the poster as background, an event-driven raise loop in its own thread) and the log.
//!
//! Until 2026-08-23 this layer lived in `rexon/session_splash.py` (ctypes+libX11,
//! `threading`, `subprocess`, `socket`, `fcntl`, Pillow). It was moved wholesale per
//! rule #16; Python kept a thin facade that creates the
//! executor (`rexon_py.SplashIo`) and hands it to the core flows.
//!
//! What changed in the move (rule #16 — bugs are not preserved):
//!   * stopping the raise loop WAKES the thread via a wake-up pipe instead of waiting
//!     for the safety step to expire: previously `join(timeout=1.0)` could
//!     return before the thread reached the flag check, and the layer
//!     was closed right under it (XRaiseWindow on a destroyed window);
//!   * timeouts that were local numbers in the shim (2 s on sockets, 1 s on
//!     waiting for the thread) moved to the core — `IPC_SOCK_TIMEOUT_SEC` and
//!     `RAISE_JOIN_WAIT_SEC`; no parallel copies of the values remain;
//!   * detached processes (a test from the tab) are collected in a shared list
//!     and reaped on the next launch — previously the long-lived
//!     UI process was left with a zombie after every test.
//!
//! Executor assumptions (as in the importers' `LiveImpIo`): `listdir` order
//! is sorted (in Python the FS order is undefined); a missing binary
//! and a command timeout are `None`, i.e. the same safe default that
//! Python gave on `OSError`/`TimeoutExpired`.
//!
//! Verification: `rust/rexon-core/tests/session_splash_parity.rs` (replay of the
//! executor log) + `rust/rexon-py/tests/session_splash_roundtrip.py` (the live
//! module on a recorded reference, side effects are swapped by a wrapper over
//! THIS executor).
//!
//! ⚠️ WHY THE EXECUTOR LIVES IN THE CORE AND NOT IN THE BRIDGE. Until 2026-09-08 it lived in
//! `rexon-py/src/splash_io.rs`, i.e. inside the `cdylib` crate for Python. Such a
//! crate cannot be used from Rust at all, and the «Steam» tab in Rust
//! (`rexon-gui::steam_actions`) had no access to the splash: its buttons would have
//! had to stay dead or get a SECOND copy of the executor (rule
//! #12 — not allowed). The module was moved here wholesale without changing a line of logic;
//! the bridge kept only the `#[pyclass]` adapter `rexon_py.SplashIo` over
//! this same type.
//!
//! The only thing that had to be untangled in the move is the log: the core cannot
//! and must not write into Python `logging`. Level and message go into a
//! REPLACEABLE sink ([`set_log_sink`]); the bridge installs its own and keeps writing
//! into `rexon.session_splash`, while Rust processes get the default — the standard
//! error stream.

use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::session_splash as core_ss;
use crate::session_splash::SplashIo as _;
use crate::utils as core_utils;

// ============================================================================
// Common bits: time, PATH, paths, running commands
// ============================================================================

/// The reference point of `monotonic()` — one per process, like `time.monotonic`.
fn mono_epoch() -> Instant {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    *EPOCH.get_or_init(Instant::now)
}

/// Splash log sink: `(level, message)`.
///
/// Installed once per process. The Python bridge puts a write into
/// `logging.getLogger("rexon.session_splash")` here — the same logger the
/// facade had; Rust processes (the «Steam» tab, the splash binary) make do
/// with the default.
type LogSink = Box<dyn Fn(&str, &str) + Send + Sync>;

static LOG_SINK: OnceLock<LogSink> = OnceLock::new();

/// Set the log sink. A second call changes nothing: a process has one sink,
/// and «last one wins» would hide splash lines from whoever installed
/// theirs earlier.
pub fn set_log_sink(sink: impl Fn(&str, &str) + Send + Sync + 'static) {
    let _ = LOG_SINK.set(Box::new(sink));
}

/// A line into the splash log.
///
/// Unknown level → `debug` (like `getattr(log, level, log.debug)`).
///
/// Without its own sink the line goes into the shared Rexon log (`applog`,
/// area `splash`). ⚠️ `debug` is written there only with `REXON_LOG=debug`:
/// the splash logs debug output on every wait step, and running a clip from
/// the tab would flood the log with hundreds of lines.
fn log_line(level: &str, message: &str) {
    let level = match level {
        "warning" | "info" | "error" | "critical" | "debug" => level,
        _ => "debug",
    };
    if let Some(sink) = LOG_SINK.get() {
        sink(level, message);
        return;
    }
    // Debug output goes into the log only with `REXON_LOG=debug` — the level
    // filters it by itself, the error stream is no longer flooded.
    crate::applog::write(crate::applog::level_from_name(level), "splash", message);
}

/// Whether the file is executable (`os.access(path, os.X_OK)`).
fn is_exec(path: &Path) -> bool {
    path.metadata()
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

/// Port of `shutil.which`: a name with a slash is checked in place, otherwise
/// `PATH` is searched. The shared resolver `utils::resolve_tool` does NOT fit here — it knows
/// synonyms (`convert`→`magick`) and extra directories, while the original splash
/// looked at exactly `PATH`.
fn which_path(name: &str) -> String {
    if name.is_empty() {
        return String::new();
    }
    if name.contains('/') {
        let p = Path::new(name);
        return if is_exec(p) { name.to_string() } else { String::new() };
    }
    let path = std::env::var("PATH").unwrap_or_default();
    for dir in path.split(':') {
        if dir.is_empty() {
            continue;
        }
        let cand = Path::new(dir).join(name);
        if is_exec(&cand) {
            return cand.to_string_lossy().into_owned();
        }
    }
    String::new()
}

/// Port of `os.path.abspath`: prepend the working directory and collapse `.`/`..`
/// WITHOUT touching the file system.
fn abspath(path: &str) -> String {
    let p = Path::new(path);
    let joined = if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("/"))
            .join(p)
    };
    let mut out = PathBuf::from("/");
    for c in joined.components() {
        match c {
            Component::Prefix(_) | Component::RootDir | Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            Component::Normal(s) => out.push(s),
        }
    }
    out.to_string_lossy().into_owned()
}

/// Port of `os.path.realpath`: symbolic links are resolved, while
/// nonexistent components stay as they are (`canonicalize` fails on them, and
/// the search for the Steam directory over nonexistent candidates would break).
fn realpath(path: &str) -> String {
    let start = abspath(path);
    let mut out = PathBuf::from("/");
    let mut hops = 0u32;
    for comp in Path::new(&start).components() {
        let Component::Normal(name) = comp else { continue };
        out.push(name);
        // Resolve this component's link chain (the limit is the same as the OS kernel's).
        while hops < 40 {
            let Ok(meta) = std::fs::symlink_metadata(&out) else { break };
            if !meta.file_type().is_symlink() {
                break;
            }
            let Ok(target) = std::fs::read_link(&out) else { break };
            hops += 1;
            if target.is_absolute() {
                out = target;
            } else {
                out.pop();
                for c in target.components() {
                    match c {
                        Component::ParentDir => {
                            out.pop();
                        }
                        Component::Normal(s) => out.push(s),
                        _ => {}
                    }
                }
            }
        }
    }
    out.to_string_lossy().into_owned()
}

/// Port of `subprocess.run(capture_output=True, text=True, timeout=…)`:
/// `(rc, stdout, stderr)` or `None` on any failure (no binary,
/// timeout) — the same safe default that Python gave on `OSError`.
pub fn run_capture(argv: &[String], timeout: f64) -> Option<core_ss::RunOut> {
    let (bin, rest) = argv.split_first()?;
    let mut child = Command::new(bin)
        .args(rest)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    let mut op = child.stdout.take();
    let mut ep = child.stderr.take();
    // Pipes are drained in their own threads: otherwise a command that printed more
    // than the pipe capacity would hang forever (and the timeout would not save it —
    // it waits for exit, and the command cannot exit).
    let to = std::thread::spawn(move || {
        let mut b = Vec::new();
        if let Some(p) = op.as_mut() {
            let _ = p.read_to_end(&mut b);
        }
        b
    });
    let te = std::thread::spawn(move || {
        let mut b = Vec::new();
        if let Some(p) = ep.as_mut() {
            let _ = p.read_to_end(&mut b);
        }
        b
    });
    let deadline = Instant::now() + Duration::from_secs_f64(timeout.max(0.0));
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break Some(st),
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(_) => break None,
        }
    };
    let stdout = to.join().unwrap_or_default();
    let stderr = te.join().unwrap_or_default();
    let st = status?;
    Some(core_ss::RunOut {
        rc: st.code().unwrap_or(-1) as i64,
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
    })
}

// ============================================================================
// unix sockets: mpv-IPC and sd_notify
// ============================================================================

/// Connect to a unix socket by path OR in the abstract namespace (an address
/// starting with NUL — that is how systemd hands out `NOTIFY_SOCKET=@…`; neither
/// `UnixStream::connect` nor `UnixDatagram::connect` can do that).
///
/// `dgram` — a datagram (sd_notify) instead of a stream (mpv-IPC).
fn connect_unix(addr: &str, dgram: bool, timeout: f64) -> Option<UnixSock> {
    let bytes = addr.as_bytes();
    let mut sa: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    sa.sun_family = libc::AF_UNIX as libc::sa_family_t;
    if bytes.len() >= std::mem::size_of_val(&sa.sun_path) {
        return None;
    }
    for (i, b) in bytes.iter().enumerate() {
        sa.sun_path[i] = *b as libc::c_char;
    }
    let len = (std::mem::size_of::<libc::sa_family_t>() + bytes.len()) as libc::socklen_t;
    let kind = if dgram { libc::SOCK_DGRAM } else { libc::SOCK_STREAM };
    let fd = unsafe { libc::socket(libc::AF_UNIX, kind | libc::SOCK_CLOEXEC, 0) };
    if fd < 0 {
        return None;
    }
    let sock = UnixSock(fd);
    let tv = libc::timeval {
        tv_sec: timeout as libc::time_t,
        tv_usec: ((timeout - timeout.trunc()) * 1e6) as libc::suseconds_t,
    };
    for opt in [libc::SO_RCVTIMEO, libc::SO_SNDTIMEO] {
        unsafe {
            libc::setsockopt(
                fd,
                libc::SOL_SOCKET,
                opt,
                std::ptr::addr_of!(tv).cast(),
                std::mem::size_of_val(&tv) as libc::socklen_t,
            );
        }
    }
    let rc = unsafe {
        libc::connect(
            fd,
            std::ptr::addr_of!(sa).cast::<libc::sockaddr>(),
            len,
        )
    };
    if rc < 0 {
        return None;
    }
    Some(sock)
}

/// A socket descriptor that closes itself (`with socket.socket(...)`).
struct UnixSock(libc::c_int);

impl UnixSock {
    fn send_all(&self, data: &[u8]) -> bool {
        let mut sent = 0usize;
        while sent < data.len() {
            let n = unsafe {
                libc::send(
                    self.0,
                    data[sent..].as_ptr().cast(),
                    data.len() - sent,
                    libc::MSG_NOSIGNAL,
                )
            };
            if n <= 0 {
                return false;
            }
            sent += n as usize;
        }
        true
    }

    fn recv(&self, buf: &mut [u8]) -> isize {
        unsafe { libc::recv(self.0, buf.as_mut_ptr().cast(), buf.len(), 0) }
    }
}

impl Drop for UnixSock {
    fn drop(&mut self) {
        unsafe { libc::close(self.0) };
    }
}

// ============================================================================
// X11: our own override-redirect layer
// ============================================================================

/// Live X11 client of the splash: a black window over the whole (primary) screen that
/// the window manager does NOT manage.
///
/// The WM plays no part in its fate ⇒ the layer works the same on any X11
/// DE/WM (Plasma, XFCE, MATE, Cinnamon-X11, Budgie…) and even BEFORE the WM starts.
/// mpv draws into it via `--wid`. The window lives as long as our
/// connection to X: if the process dies, the X server destroys everything itself, and the screen
/// does not stay covered.
mod x11 {
    use std::ffi::CString;
    use std::os::raw::{c_int, c_uint, c_ulong};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{mpsc, Arc, Mutex};
    use std::time::Duration;

    use crate::session_splash as core_ss;
    use x11_dl::xlib;

    /// libX11 loaded via dlopen. Function pointers are shared with the
    /// raise-loop thread (`XInitThreads` is called before the first `XOpenDisplay`,
    /// so libX11 handles threads itself).
    struct Lib(xlib::Xlib);
    unsafe impl Send for Lib {}
    unsafe impl Sync for Lib {}

    /// Connection to the X server. Separate per thread; shared only where
    /// it cannot be avoided (the fallback safety polling).
    #[derive(Clone, Copy)]
    struct Dpy(*mut xlib::Display);
    unsafe impl Send for Dpy {}
    unsafe impl Sync for Dpy {}

    /// Wake-up pipe: stopping the raise loop does not wait for the step to expire.
    struct WakePipe {
        read: c_int,
        write: c_int,
    }

    impl WakePipe {
        fn new() -> Option<WakePipe> {
            let mut fds = [0 as c_int; 2];
            let rc = unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) };
            if rc != 0 {
                return None;
            }
            Some(WakePipe {
                read: fds[0],
                write: fds[1],
            })
        }

        fn wake(&self) {
            let b = [1u8];
            unsafe { libc::write(self.write, b.as_ptr().cast(), 1) };
        }
    }

    impl Drop for WakePipe {
        fn drop(&mut self) {
            unsafe {
                libc::close(self.read);
                libc::close(self.write);
            }
        }
    }

    /// A running raise loop: the stop flag, the wake-up pipe and
    /// the notification that the thread has really gone.
    struct RaiseLoop {
        stop: Arc<AtomicBool>,
        pipe: Arc<WakePipe>,
        /// The receiver is behind a lock: by itself it is not `Sync`, and the layer lives
        /// inside an object that Python may touch from any thread.
        done: Mutex<mpsc::Receiver<()>>,
    }

    pub struct Overlay {
        lib: Arc<Lib>,
        dpy: Option<Dpy>,
        screen: c_int,
        pub window: c_ulong,
        pub width: u32,
        pub height: u32,
        raiser: Option<RaiseLoop>,
    }

    impl Overlay {
        /// Create the layer. `geometry` — the rectangle of the primary monitor
        /// (`None` — the whole screen). Any failure (no libX11, no
        /// X server) — `None`: the caller falls back to windowed mpv.
        pub fn create(geometry: Option<(i64, i64, i64, i64)>) -> Option<Overlay> {
            let lib = match xlib::Xlib::open() {
                Ok(l) => Arc::new(Lib(l)),
                Err(e) => {
                    super::log_line("warning", &format!("x11 overlay: libX11 недоступна: {e}"));
                    return None;
                }
            };
            unsafe {
                // BEFORE XOpenDisplay: XRaiseWindow is called from a background thread.
                (lib.0.XInitThreads)();
                let dpy = (lib.0.XOpenDisplay)(std::ptr::null());
                if dpy.is_null() {
                    super::log_line("warning", "x11 overlay: XOpenDisplay не удался");
                    return None;
                }
                let screen = (lib.0.XDefaultScreen)(dpy);
                let root = (lib.0.XRootWindow)(dpy, screen);
                let (x, y, w, h) = geometry.unwrap_or_else(|| {
                    (
                        0,
                        0,
                        (lib.0.XDisplayWidth)(dpy, screen) as i64,
                        (lib.0.XDisplayHeight)(dpy, screen) as i64,
                    )
                });
                if w <= 0 || h <= 0 {
                    (lib.0.XCloseDisplay)(dpy);
                    return None;
                }
                let mut attrs: xlib::XSetWindowAttributes = std::mem::zeroed();
                attrs.background_pixel = (lib.0.XBlackPixel)(dpy, screen);
                attrs.override_redirect = xlib::True;
                let win = (lib.0.XCreateWindow)(
                    dpy,
                    root,
                    x as c_int,
                    y as c_int,
                    w as c_uint,
                    h as c_uint,
                    0,
                    xlib::CopyFromParent,      // depth
                    xlib::InputOutput as c_uint,
                    std::ptr::null_mut(),      // visual = CopyFromParent
                    xlib::CWBackPixel | xlib::CWOverrideRedirect,
                    &mut attrs,
                );
                if win == 0 {
                    (lib.0.XCloseDisplay)(dpy);
                    return None;
                }
                if let Ok(name) = CString::new("rexon-session-splash") {
                    (lib.0.XStoreName)(dpy, win, name.as_ptr());
                }
                // Do NOT map here: on a warm login the poster is set as background BEFORE
                // mapping, so the window appears with the picture right away.
                Some(Overlay {
                    lib,
                    dpy: Some(Dpy(dpy)),
                    screen,
                    window: win,
                    width: w as u32,
                    height: h as u32,
                    raiser: None,
                })
            }
        }

        fn dpy(&self) -> Option<*mut xlib::Display> {
            self.dpy.map(|d| d.0)
        }

        /// Show the layer (above everything). Call after preparing the background.
        pub fn map(&self) {
            let Some(dpy) = self.dpy() else { return };
            unsafe {
                (self.lib.0.XMapRaised)(dpy, self.window);
                (self.lib.0.XFlush)(dpy);
            }
        }

        /// Is a compositor already running? An instant check of `_NET_WM_CM_S<n>`.
        pub fn compositor_present(&self) -> bool {
            let Some(dpy) = self.dpy() else { return false };
            let Ok(name) = CString::new(format!("_NET_WM_CM_S{}", self.screen)) else {
                return false;
            };
            unsafe {
                let atom = (self.lib.0.XInternAtom)(dpy, name.as_ptr(), xlib::False);
                atom != 0 && (self.lib.0.XGetSelectionOwner)(dpy, atom) != 0
            }
        }

        /// Draw the poster (the clip's first frame) as the layer background instead of black.
        ///
        /// Covers the black pause between ksplashqml hiding its poster
        /// (~0.4 s after login) and mpv's first frame (~1.5 s of vo initialization):
        /// the layer shows the same frame that mpv will then draw.
        /// Letterbox — like mpv (fit preserving the aspect ratio, black
        /// bars). Any failure is not fatal: the black background stays.
        pub fn set_poster(&self, png: &str) -> bool {
            let Some(dpy) = self.dpy() else { return false };
            let x = &self.lib.0;
            let depth = unsafe { (x.XDefaultDepth)(dpy, self.screen) };
            if !core_ss::X11_POSTER_DEPTHS.contains(&(depth as i64)) {
                // A 30-bit (10bpc) visual: our BGRX bytes would land wrong.
                super::log_line("debug", &format!("set_poster: depth={depth} не поддержан"));
                return false;
            }
            let (w, h) = (self.width, self.height);
            let src = match image::ImageReader::open(png)
                .ok()
                .and_then(|r| r.with_guessed_format().ok())
                .and_then(|r| r.decode().ok())
            {
                Some(im) => im.to_rgb8(),
                None => return false,
            };
            // The core computes the letterbox frame: the formula is the same as mpv's
            // --fullscreen, and it lives in one place.
            let (ox, oy, sw, sh) = core_ss::letterbox_rect(
                src.width() as i64,
                src.height() as i64,
                w as i64,
                h as i64,
            );
            if sw <= 0 || sh <= 0 {
                return false;
            }
            let scaled = image::imageops::resize(
                &src,
                sw as u32,
                sh as u32,
                image::imageops::FilterType::Lanczos3,
            );
            // A BGRX canvas over the whole layer: the bars stay black.
            let mut buf = vec![0u8; (w as usize) * (h as usize) * 4];
            for row in 0..scaled.height() {
                let dy = oy as u32 + row;
                if dy >= h {
                    break;
                }
                for col in 0..scaled.width() {
                    let dx = ox as u32 + col;
                    if dx >= w {
                        break;
                    }
                    let p = scaled.get_pixel(col, row).0;
                    let off = ((dy as usize) * (w as usize) + dx as usize) * 4;
                    buf[off] = p[2];
                    buf[off + 1] = p[1];
                    buf[off + 2] = p[0];
                }
            }
            unsafe {
                let pixmap = (x.XCreatePixmap)(dpy, self.window, w, h, depth as c_uint);
                if pixmap == 0 {
                    return false;
                }
                let gc = (x.XCreateGC)(dpy, pixmap, 0, std::ptr::null_mut());
                let visual = (x.XDefaultVisual)(dpy, self.screen);
                let img = (x.XCreateImage)(
                    dpy,
                    visual,
                    depth as c_uint,
                    xlib::ZPixmap,
                    0,
                    buf.as_mut_ptr().cast(),
                    w,
                    h,
                    32,
                    (w as c_int) * 4,
                );
                if img.is_null() {
                    (x.XFreeGC)(dpy, gc);
                    (x.XFreePixmap)(dpy, pixmap);
                    return false;
                }
                (x.XPutImage)(dpy, pixmap, gc, img, 0, 0, 0, 0, w, h);
                // NOT XDestroyImage: the data is our buffer, we free only the
                // structure.
                (*img).data = std::ptr::null_mut();
                (x.XFree)(img.cast());
                (x.XFreeGC)(dpy, gc);
                (x.XSetWindowBackgroundPixmap)(dpy, self.window, pixmap);
                (x.XClearWindow)(dpy, self.window);
                // the background was copied by reference on the server — the pixmap can be released
                (x.XFreePixmap)(dpy, pixmap);
                (x.XFlush)(dpy);
            }
            true
        }

        /// Start the background loop «keep the layer above other late popups».
        ///
        /// Event-driven, not polling: a `SubstructureNotify` subscription on the root —
        /// map/restack of ANY foreign window (e.g. Firefox's translation bar
        /// on session restore) raises the layer in the same millisecond,
        /// before the compositor's next frame, i.e. the popup has no time to reach
        /// the screen. Polling every 0.08 s left such flashes for up to ~80 ms.
        /// Its own X connection: do not share the event queue with the main thread.
        pub fn raise_start(&mut self) {
            if self.raiser.is_some() {
                return;
            }
            let Some(pipe) = WakePipe::new().map(Arc::new) else {
                super::log_line("debug", "raise_loop: труба пробуждения не создалась");
                return;
            };
            let stop = Arc::new(AtomicBool::new(false));
            let (tx, done) = mpsc::channel();
            let lib = self.lib.clone();
            let main_dpy = self.dpy;
            let (win, screen) = (self.window, self.screen);
            let (t_stop, t_pipe) = (stop.clone(), pipe.clone());
            std::thread::Builder::new()
                .name("rexon-splash-raise".into())
                .spawn(move || {
                    raise_loop(&lib, main_dpy, screen, win, &t_stop, &t_pipe);
                    let _ = tx.send(());
                })
                .ok();
            self.raiser = Some(RaiseLoop {
                stop,
                pipe,
                done: Mutex::new(done),
            });
        }

        /// Stop the loop and WAIT for the thread: the window must not be closed while it still
        /// calls XRaiseWindow on it.
        pub fn raise_stop(&mut self) {
            let Some(r) = self.raiser.take() else { return };
            r.stop.store(true, Ordering::SeqCst);
            r.pipe.wake();
            let waited = r
                .done
                .lock()
                .ok()
                .map(|rx| rx.recv_timeout(Duration::from_secs_f64(core_ss::RAISE_JOIN_WAIT_SEC)));
            if !matches!(waited, Some(Ok(()))) {
                super::log_line("debug", "raise_loop: поток не завершился в срок");
            }
        }

        /// Destroy the layer and close the X connection.
        pub fn close(&mut self) {
            self.raise_stop();
            let Some(dpy) = self.dpy.take().map(|d| d.0) else { return };
            unsafe {
                (self.lib.0.XDestroyWindow)(dpy, self.window);
                (self.lib.0.XCloseDisplay)(dpy);
            }
        }
    }

    impl Drop for Overlay {
        fn drop(&mut self) {
            self.close();
        }
    }

    /// Body of the raise loop (in its own thread, with its own X connection).
    fn raise_loop(
        lib: &Arc<Lib>,
        main_dpy: Option<Dpy>,
        screen: c_int,
        win: c_ulong,
        stop: &AtomicBool,
        pipe: &WakePipe,
    ) {
        let x = &lib.0;
        let dpy = unsafe { (x.XOpenDisplay)(std::ptr::null()) };
        if dpy.is_null() {
            // Our own connection did not come up — safety polling over the
            // shared one (the main connection is alive until the layer is closed).
            let Some(main) = main_dpy else { return };
            while !stop.load(Ordering::SeqCst) {
                if wait_fds(&[pipe.read], core_ss::RAISE_POLL_FALLBACK_SEC) == Some(0) {
                    break; // woken up — leave
                }
                unsafe {
                    (x.XRaiseWindow)(main.0, win);
                    (x.XFlush)(main.0);
                }
            }
            return;
        }
        unsafe {
            let root = (x.XRootWindow)(dpy, screen);
            (x.XSelectInput)(dpy, root, xlib::SubstructureNotifyMask);
            (x.XRaiseWindow)(dpy, win);
            (x.XFlush)(dpy);
            let fd = (x.XConnectionNumber)(dpy);
            let mut ev: xlib::XEvent = std::mem::zeroed();
            while !stop.load(Ordering::SeqCst) {
                let ready = wait_fds(&[pipe.read, fd], core_ss::RAISE_INTERVAL_SEC);
                if ready == Some(0) {
                    break; // woken up to stop
                }
                // timeout → safety raise
                let mut foreign = ready.is_none();
                while (x.XPending)(dpy) > 0 {
                    (x.XNextEvent)(dpy, &mut ev);
                    let (kind, other) = match ev.get_type() {
                        xlib::MapNotify => (true, ev.map.window),
                        xlib::ConfigureNotify => (true, ev.configure.window),
                        xlib::CirculateNotify => (true, ev.circulate.window),
                        _ => (false, 0),
                    };
                    // filter our own events: our raise also produces a
                    // ConfigureNotify — otherwise the loop would feed itself
                    if kind && other != win {
                        foreign = true;
                    }
                }
                if foreign {
                    (x.XRaiseWindow)(dpy, win);
                    (x.XFlush)(dpy);
                }
            }
            (x.XCloseDisplay)(dpy);
        }
    }

    /// Wait for descriptors to become ready: `Some(index of the first ready one)` or
    /// `None` on timeout (port of `select.select` with a timeout).
    fn wait_fds(fds: &[c_int], timeout: f64) -> Option<usize> {
        let mut pfd: Vec<libc::pollfd> = fds
            .iter()
            .map(|f| libc::pollfd {
                fd: *f,
                events: libc::POLLIN,
                revents: 0,
            })
            .collect();
        let ms = (timeout * 1000.0).round().max(0.0) as libc::c_int;
        let rc = unsafe { libc::poll(pfd.as_mut_ptr(), pfd.len() as libc::nfds_t, ms) };
        if rc <= 0 {
            return None;
        }
        pfd.iter().position(|p| p.revents != 0)
    }
}

// ============================================================================
// Live executor of the SplashIo trait
// ============================================================================

/// The «one splash run per session» lock: the descriptor lives until the end of
/// the process together with the flock (port of the module variable `_LOCK_FD`).
static LOCK_FD: Mutex<Option<File>> = Mutex::new(None);

/// Detached processes (a splash test from the tab): we keep them to
/// reap them on the next launch and not accumulate zombies in the UI process.
static DETACHED: Mutex<Vec<Child>> = Mutex::new(Vec::new());

/// The real executor of [`core_ss::SplashIo`].
///
/// One instance per facade call: it holds the descriptors of
/// started processes and X11 layers, so we only stop OUR OWN (the
/// «no pkill/killall by name» rule).
pub struct LiveSplashIo {
    /// `os.path.expanduser("~")` — the observation Python provides.
    home: String,
    /// `sys.executable` — needed by the compound steps of autostart installation.
    python_exe: String,
    procs: HashMap<i64, Child>,
    overlays: HashMap<i64, x11::Overlay>,
    next: i64,
    /// Rotation draw seed (xorshift): our own, so as not to pull in a crate for
    /// a single `random.choice`.
    seed: u64,
}

impl LiveSplashIo {
    pub fn new(home: &str, python_exe: &str) -> LiveSplashIo {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9E37_79B9_7F4A_7C15);
        LiveSplashIo {
            home: home.to_string(),
            python_exe: python_exe.to_string(),
            procs: HashMap::new(),
            overlays: HashMap::new(),
            next: 1,
            seed: (nanos ^ ((std::process::id() as u64) << 32)) | 1,
        }
    }

    fn handle(&mut self) -> i64 {
        let h = self.next;
        self.next += 1;
        h
    }

    fn rnd(&mut self) -> u64 {
        // xorshift64*: the rotation draw, no quality requirements.
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        self.seed
    }

    /// Rectangle of the primary monitor from xrandr output (argv and parsing are in the core).
    fn primary_geometry(&mut self) -> Option<(i64, i64, i64, i64)> {
        let xr = self.which("xrandr");
        if xr.is_empty() {
            return None;
        }
        let out = self.run(&core_ss::xrandr_query_argv(&xr), core_ss::XRANDR_TIMEOUT_SEC)?;
        core_ss::parse_xrandr_geometry(&out.stdout)
    }
}

impl core_ss::SplashIo for LiveSplashIo {
    fn env(&mut self, name: &str) -> String {
        std::env::var(name).unwrap_or_default()
    }

    fn which(&mut self, name: &str) -> String {
        which_path(name)
    }

    fn launch_entry(&mut self) -> crate::installer::LaunchEntry {
        crate::installer::launch_entry_live()
    }

    fn is_file(&mut self, path: &str) -> bool {
        Path::new(path).is_file()
    }

    fn is_dir(&mut self, path: &str) -> bool {
        Path::new(path).is_dir()
    }

    fn exists(&mut self, path: &str) -> bool {
        Path::new(path).exists()
    }

    fn is_link(&mut self, path: &str) -> bool {
        std::fs::symlink_metadata(path)
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false)
    }

    fn is_executable(&mut self, path: &str) -> bool {
        unsafe {
            match std::ffi::CString::new(path) {
                Ok(c) => libc::access(c.as_ptr(), libc::X_OK) == 0,
                Err(_) => false,
            }
        }
    }

    fn realpath(&mut self, path: &str) -> String {
        realpath(path)
    }

    fn abspath(&mut self, path: &str) -> String {
        abspath(path)
    }

    fn listdir(&mut self, path: &str) -> Vec<String> {
        let Ok(rd) = std::fs::read_dir(path) else {
            return Vec::new();
        };
        let mut out: Vec<String> = rd
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        // FS order is undefined in Python too — we sort so that the showcase and
        // the executor log are reproducible.
        out.sort();
        out
    }

    fn read_text(&mut self, path: &str) -> Option<String> {
        let bytes = std::fs::read(path).ok()?;
        Some(String::from_utf8_lossy(&bytes).into_owned())
    }

    fn read_bytes(&mut self, path: &str) -> Option<Vec<u8>> {
        std::fs::read(path).ok()
    }

    fn mtime(&mut self, path: &str) -> Option<i64> {
        let m = std::fs::metadata(path).ok()?;
        let t = m.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
        Some(t.as_secs() as i64)
    }

    fn file_size(&mut self, path: &str) -> i64 {
        std::fs::metadata(path).map(|m| m.len() as i64).unwrap_or(0)
    }

    fn makedirs(&mut self, path: &str) -> Option<String> {
        std::fs::create_dir_all(path).err().map(|e| e.to_string())
    }

    fn write_text(&mut self, path: &str, content: &str) -> Option<String> {
        std::fs::write(path, content).err().map(|e| e.to_string())
    }

    fn write_atomic(&mut self, path: &str, content: &str) -> Option<String> {
        // A half-written config/unit in place of the real one is the worst thing that can
        // happen at session login. Hence the core's shared door: the local
        // «write + rename» pair here had no `fsync`, and the login splash is
        // exactly what most often lives through a machine power-off.
        crate::utils::atomic_write(path, content).err().map(|e| e.to_string())
    }

    fn replace(&mut self, src: &str, dst: &str) -> Option<String> {
        std::fs::rename(src, dst).err().map(|e| e.to_string())
    }

    fn remove_file(&mut self, path: &str) -> Option<String> {
        std::fs::remove_file(path).err().map(|e| e.to_string())
    }

    fn remove_tree(&mut self, path: &str) -> Option<String> {
        std::fs::remove_dir_all(path).err().map(|e| e.to_string())
    }

    fn copy_file(&mut self, src: &str, dst: &str) -> Option<String> {
        std::fs::copy(src, dst).err().map(|e| e.to_string())
    }

    fn symlink(&mut self, target: &str, link: &str) -> Option<String> {
        std::os::unix::fs::symlink(target, link)
            .err()
            .map(|e| e.to_string())
    }

    fn run(&mut self, argv: &[String], timeout: f64) -> Option<core_ss::RunOut> {
        run_capture(argv, timeout)
    }

    fn spawn(&mut self, argv: &[String], detached: bool) -> Result<i64, String> {
        // Reap detached predecessors: otherwise zombies accumulate in the long-lived
        // UI process.
        if let Ok(mut kept) = DETACHED.lock() {
            kept.retain_mut(|c| !matches!(c.try_wait(), Ok(Some(_))));
        }
        let Some((bin, rest)) = argv.split_first() else {
            return Err(crate::i18n::tr("empty command"));
        };
        let mut cmd = Command::new(bin);
        cmd.args(rest);
        if detached {
            // Port of `start_new_session=True`: its own session, so the process
            // outlives the caller's exit and does not catch its signals.
            unsafe {
                cmd.pre_exec(|| {
                    libc::setsid();
                    Ok(())
                });
            }
        }
        match cmd.spawn() {
            Ok(child) => {
                if detached {
                    if let Ok(mut kept) = DETACHED.lock() {
                        kept.push(child);
                    }
                    // The caller does not need the descriptor: the core no longer touches
                    // a detached run.
                    let h = self.handle();
                    Ok(h)
                } else {
                    let h = self.handle();
                    self.procs.insert(h, child);
                    Ok(h)
                }
            }
            Err(e) => Err(e.to_string()),
        }
    }

    fn poll(&mut self, proc: i64) -> Option<i64> {
        let c = self.procs.get_mut(&proc)?;
        match c.try_wait() {
            Ok(Some(st)) => Some(st.code().unwrap_or(-1) as i64),
            _ => None,
        }
    }

    fn wait(&mut self, proc: i64, timeout: f64) -> Option<i64> {
        let deadline = Instant::now() + Duration::from_secs_f64(timeout.max(0.0));
        loop {
            {
                let c = self.procs.get_mut(&proc)?;
                match c.try_wait() {
                    Ok(Some(st)) => return Some(st.code().unwrap_or(-1) as i64),
                    Ok(None) => {}
                    Err(_) => return None,
                }
            }
            if Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn terminate(&mut self, proc: i64) {
        // Only OUR OWN descriptor, never by process name.
        if let Some(c) = self.procs.get_mut(&proc) {
            let pid = c.id() as libc::pid_t;
            unsafe { libc::kill(pid, libc::SIGTERM) };
        }
    }

    fn kill(&mut self, proc: i64) {
        if let Some(c) = self.procs.get_mut(&proc) {
            let _ = c.kill();
            let _ = c.wait();
        }
    }

    fn monotonic(&mut self) -> f64 {
        mono_epoch().elapsed().as_secs_f64()
    }

    fn sleep(&mut self, seconds: f64) {
        if seconds > 0.0 {
            std::thread::sleep(Duration::from_secs_f64(seconds));
        }
    }

    fn choice(&mut self, items: &[String]) -> Option<String> {
        if items.is_empty() {
            return None;
        }
        let i = (self.rnd() % items.len() as u64) as usize;
        Some(items[i].clone())
    }

    fn ipc_send(&mut self, sock: &str, payload: &str) -> bool {
        match connect_unix(sock, false, core_ss::IPC_SOCK_TIMEOUT_SEC) {
            Some(s) => s.send_all(payload.as_bytes()),
            None => false,
        }
    }

    fn ipc_request(&mut self, sock: &str, payload: &str, reply_marker: &str) -> Option<String> {
        // Events pour into the socket intermixed, so we read until a complete line with
        // the marker appears (the core provides it), or until the
        // connection closes / we hit the limit. Parsing is in the core.
        let s = connect_unix(sock, false, core_ss::IPC_SOCK_TIMEOUT_SEC)?;
        if !s.send_all(payload.as_bytes()) {
            return None;
        }
        let marker = reply_marker.as_bytes();
        let mut buf: Vec<u8> = Vec::new();
        let mut chunk = [0u8; 4096];
        while buf.len() < core_ss::MPV_IPC_READ_LIMIT {
            let n = s.recv(&mut chunk);
            if n <= 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n as usize]);
            if let Some(nl) = buf.iter().rposition(|b| *b == b'\n') {
                if buf[..nl].windows(marker.len()).any(|w| w == marker) {
                    break;
                }
            }
        }
        Some(String::from_utf8_lossy(&buf).into_owned())
    }

    fn notify(&mut self, addr: &str, message: &str) {
        let sent = connect_unix(addr, true, core_ss::IPC_SOCK_TIMEOUT_SEC)
            .map(|s| s.send_all(message.as_bytes()))
            .unwrap_or(false);
        if !sent {
            log_line("debug", &format!("sd_notify не ушёл: {addr}"));
        }
    }

    fn acquire_lock(&mut self, path: &str) -> bool {
        // One splash per session (unit + autostart, double click…).
        let Ok(f) = File::create(path) else { return false };
        let rc = unsafe {
            libc::flock(
                std::os::unix::io::AsRawFd::as_raw_fd(&f),
                libc::LOCK_EX | libc::LOCK_NB,
            )
        };
        if rc != 0 {
            return false;
        }
        // The descriptor lives until the end of the process together with the lock.
        if let Ok(mut slot) = LOCK_FD.lock() {
            *slot = Some(f);
        }
        true
    }

    fn kwin_reconfigure_dbus(&mut self, timeout: f64) -> bool {
        // The main reconfigure path is the session bus: `dbus-send` exists
        // wherever the bus itself exists. Tool preference order and
        // argv assembly are shared (`utils`), there is no local copy here.
        let path_env = std::env::var("PATH").ok();
        for (tool, kind) in core_utils::DBUS_TOOL_PREFERENCE {
            let Some(exe) = core_utils::resolve_tool(tool, &[], path_env.as_deref()) else {
                continue;
            };
            let (dest, obj, method) = core_utils::KWIN_RECONFIGURE;
            let argv = core_utils::dbus_call_argv(kind, &exe, dest, obj, method, &[]);
            return matches!(run_capture(&argv, timeout), Some(r) if r.rc == 0);
        }
        false
    }

    fn x11_create(&mut self) -> Option<i64> {
        let geo = self.primary_geometry();
        let overlay = x11::Overlay::create(geo)?;
        let h = self.handle();
        self.overlays.insert(h, overlay);
        Some(h)
    }

    fn x11_window_id(&mut self, overlay: i64) -> i64 {
        self.overlays.get(&overlay).map(|o| o.window as i64).unwrap_or(0)
    }

    fn x11_set_poster(&mut self, overlay: i64, png: &str) -> bool {
        self.overlays
            .get(&overlay)
            .map(|o| o.set_poster(png))
            .unwrap_or(false)
    }

    fn x11_map(&mut self, overlay: i64) {
        if let Some(o) = self.overlays.get(&overlay) {
            o.map();
        }
    }

    fn x11_compositor_present(&mut self, overlay: i64) -> bool {
        self.overlays
            .get(&overlay)
            .map(|o| o.compositor_present())
            .unwrap_or(false)
    }

    fn x11_raise_start(&mut self, overlay: i64) {
        if let Some(o) = self.overlays.get_mut(&overlay) {
            o.raise_start();
        }
    }

    fn x11_raise_stop(&mut self, overlay: i64) {
        if let Some(o) = self.overlays.get_mut(&overlay) {
            o.raise_stop();
        }
    }

    fn x11_close(&mut self, overlay: i64) {
        if let Some(mut o) = self.overlays.remove(&overlay) {
            o.close();
        }
    }

    // ── compound steps of the autostart route ──────────────────────────
    // Their internals are in the core too; the `install_autostart` route calls them
    // THROUGH the executor so that route verification can swap a step.

    fn step_install_hybrid(&mut self, video: &str) -> (bool, String) {
        let (home, exe) = (self.home.clone(), self.python_exe.clone());
        core_ss::Splash::new(self, &home).install_hybrid(video, &exe)
    }

    fn step_uninstall_hybrid(&mut self) -> (bool, String) {
        let home = self.home.clone();
        core_ss::Splash::new(self, &home).uninstall_hybrid()
    }

    fn step_install_desktop_autostart(&mut self) -> (bool, String) {
        let (home, exe) = (self.home.clone(), self.python_exe.clone());
        core_ss::Splash::new(self, &home).install_desktop_autostart(&exe)
    }

    fn step_remove_desktop_autostart(&mut self) {
        let home = self.home.clone();
        core_ss::Splash::new(self, &home).remove_desktop_autostart();
    }

    fn step_hybrid_supported(&mut self) -> bool {
        let home = self.home.clone();
        core_ss::Splash::new(self, &home).hybrid_supported()
    }

    fn login_lines(&mut self, desktop: Option<&str>) -> (Option<String>, Option<String>) {
        use crate::compositor_autostart as ca;
        match desktop {
            Some(d) => {
                let (text, refusal) = ca::mirror_desktop_live(ca::ID_SPLASH, d);
                (Some(text), refusal)
            }
            None => {
                ca::remove_live(ca::ID_SPLASH);
                (None, None)
            }
        }
    }

    fn login_works(&mut self, desktop_path: &str) -> bool {
        use crate::compositor_autostart as ca;
        let text = std::fs::read_to_string(desktop_path).ok();
        ca::works_live(ca::ID_SPLASH, text.as_deref())
    }

    fn log(&mut self, level: &str, message: &str) {
        log_line(level, message);
    }
}
