// The Wallpaper Engine tab transport: the socket to the waywallen daemon
// and its surroundings —
// a port of `rexon/qt_wallpaper_engine/client.py` to Rust/cxx-qt.
//
// What is here and what is not:
//
//   * there IS the socket lifecycle (open/close/is_open), the map of
//     pending requests, parsing an incoming frame, splitting events,
//     finding and auto-starting the daemon, reconnection and the
//     synchronous wallpaper stop;
//   * there is NOT a single DECISION — all of them are already ported to
//     the core (`rexon_core::we_client`): the bus addresses, timeouts,
//     finding the binary, `ensure_running_decision`, `poll_decision`,
//     issuing `request_id`, assembling frames,
//     `stop_active_gate`/`stop_active_result`. Here there is only Qt
//     execution. There must not be a second copy of the rules in the crate.
//
// Warning: everything in this module lives in ONE thread — the one it was
// called in. There are no other threads in the transport at all (this is a
// deliberate decision of the original: «No QThread», client.py:16), so the
// state is kept in `thread_local` and needs no synchronization. The
// exception is the `request_id` counter: both the tab's queries and the
// synchronous wallpaper stop from the installer's thread ask for it, so it
// is under a mutex.
//
// Warning: the daemon's answer arrives IN THE MAIN THREAD and is delivered
// SYNCHRONOUSLY. A `CxxQtThread` queue must not be put here: it always
// defers the call to a tick of the event loop (mock-up measurement:
// `beforeLoop=0`), that is, it changes the observable order of the query's
// signals, and that order is recorded by the golden files. The addressee's
// liveness is held by a `QPointer` in `cpp/rexon_we_transport.cpp`.

/// cxx-qt bridge: a foreign C++ QObject (QWebSocket) and the transport shims.
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qbytearray.h");
        type QByteArray = cxx_qt_lib::QByteArray;
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qurl.h");
        type QUrl = cxx_qt_lib::QUrl;
        // Warning: qint64, not i64. cxx emits i64 as `::std::int64_t`, which on
        // LP64 is `long`, while Qt declares `qint64` as `long long` — the method
        // pointer does not convert, and the C++ does not compile.
        include!("cxx-qt-lib/qtypes.h");
        type qint64 = cxx_qt_lib::qint64;
    }

    // QWebSocket is declared AS IS: cxx-qt-lib 0.9.1 has no wrapper — the
    // crate has no network module at all (only core/gui/qml/quickcontrols).
    // `#[qobject]` is mandatory, otherwise `#[qsignal]` is not allowed in the
    // block.
    unsafe extern "C++Qt" {
        include!(<QtWebSockets/QWebSocket>);
        #[qobject]
        type QWebSocket;

        #[rust_name = "open_url"]
        fn open(self: Pin<&mut QWebSocket>, url: &QUrl);

        #[cxx_name = "sendBinaryMessage"]
        fn send_binary_message(self: Pin<&mut QWebSocket>, data: &QByteArray) -> qint64;

        // Warning: close() must NOT be declared here: it has two default
        // arguments, and cxx takes the method ADDRESS by the written signature.
        // It went into the shim — rexon_we_sock_close().

        #[qsignal]
        fn connected(self: Pin<&mut QWebSocket>);
        #[qsignal]
        fn disconnected(self: Pin<&mut QWebSocket>);
        #[qsignal]
        #[cxx_name = "binaryMessageReceived"]
        fn binary_message_received(self: Pin<&mut QWebSocket>, message: &QByteArray);
        // errorOccurred(QAbstractSocket::SocketError) cannot be declared for the
        // same reason as close(): the argument is a foreign enum that cxx-qt-lib
        // does not have. The shim makes the subscription and gives ready text.
    }

    // Shims: everything cxx-qt cannot do (see cpp/rexon_we_socket.h).
    unsafe extern "C++" {
        include!("rexon_we_socket.h");

        #[rust_name = "sock_daemon_property"]
        fn rexon_we_sock_daemon_property(
            service: &QString,
            path: &QString,
            iface: &QString,
            name: &QString,
            timeout_ms: i32,
            ok: &mut bool,
        ) -> QString;

        #[rust_name = "sock_new"]
        fn rexon_we_sock_new() -> *mut QWebSocket;

        /// # Safety
        /// `object` was created by the factory above and not deleted yet.
        #[allow(clippy::missing_safety_doc)]
        #[rust_name = "sock_destroy"]
        unsafe fn rexon_we_sock_destroy(object: *mut QObject);

        /// # Safety
        /// `socket` is a live QWebSocket or nullptr.
        #[allow(clippy::missing_safety_doc)]
        #[rust_name = "sock_close"]
        unsafe fn rexon_we_sock_close(socket: *mut QWebSocket);

        /// # Safety
        /// `socket` is a live QWebSocket or nullptr.
        #[allow(clippy::missing_safety_doc)]
        #[rust_name = "sock_shutdown"]
        unsafe fn rexon_we_sock_shutdown(socket: *mut QWebSocket);

        /// # Safety
        /// `socket` is a live QWebSocket or nullptr.
        #[allow(clippy::missing_safety_doc)]
        #[rust_name = "sock_quit_loop_on_error"]
        unsafe fn rexon_we_sock_quit_loop_on_error(socket: *mut QWebSocket);

        /// # Safety
        /// `socket` is a live QWebSocket or nullptr.
        #[allow(clippy::missing_safety_doc)]
        #[rust_name = "sock_is_open"]
        unsafe fn rexon_we_sock_is_open(socket: *mut QWebSocket) -> bool;

        /// # Safety
        /// `socket` is a live QWebSocket or nullptr.
        #[allow(clippy::missing_safety_doc)]
        #[rust_name = "sock_error_string"]
        unsafe fn rexon_we_sock_error_string(socket: *mut QWebSocket) -> QString;

        /// # Safety
        /// `socket` is a live QWebSocket.
        #[allow(clippy::missing_safety_doc)]
        #[rust_name = "sock_watch_error"]
        unsafe fn rexon_we_sock_watch_error(socket: *mut QWebSocket);

        #[rust_name = "sock_timer_start"]
        fn rexon_we_sock_timer_start(id: i32, interval_ms: i32, single_shot: bool);
        #[rust_name = "sock_timer_stop"]
        fn rexon_we_sock_timer_stop(id: i32);
        /// It can be called from any thread (see `rexon_we_socket.h`).
        #[rust_name = "sock_post_tick"]
        fn rexon_we_sock_post_tick(id: i32);

        #[rust_name = "sock_qt_available"]
        fn rexon_we_sock_qt_available() -> bool;

        #[rust_name = "sock_run_loop"]
        fn rexon_we_sock_run_loop(ms: i32) -> i32;
        #[rust_name = "sock_quit_loop"]
        fn rexon_we_sock_quit_loop();
    }
}

use core::pin::Pin;
use cxx_qt::QMetaObjectConnectionGuard;
use cxx_qt_lib::{QByteArray, QString, QUrl};
use prost::Message;
use qobject::QWebSocket;
use std::cell::{Cell, RefCell};
use std::ffi::c_char;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use rexon_core::proto::control as pb;
use rexon_core::waywallen_client::DbusValue;
use rexon_core::we_client;

// Function pointers cannot be expressed through cxx — these two shims are
// declared with an ordinary `extern "C"`, as already done in
// `we_transport.rs`.
unsafe extern "C" {
    fn rexon_we_sock_set_timer_tick(tick: Option<extern "C" fn(i32)>);
    fn rexon_we_sock_set_error_hook(hook: Option<extern "C" fn(*const c_char, i32)>);
}

// ── shim timer numbers ──────────────────────────────────────────────────────

/// Polling D-Bus after the daemon auto-start (`DaemonControl._poll_once`).
const TIMER_POLL: i32 = 1;
/// Reconnection after a disconnection (moved from `window.py:1136-1147`).
const TIMER_RECONNECT: i32 = 2;

/// The `health` answer watchdog (moved from `qml_window.py`: it was the
/// window's `QTimer`).
const TIMER_HEALTH: i32 = 3;
/// Deferred daemon start when the tab opens (the window's `QTimer.singleShot`).
const TIMER_START: i32 = 4;
/// Bring up the daemon that left by `Quit` for the sake of the plugin from
/// the package. Not a timer but a one-off tick from the background restart
/// thread (`sock_post_tick`).
const TIMER_PLUGIN_RELAUNCH: i32 = 5;
/// Held-back wallpaper apply frames on KDE: the "time to release" check (a
/// repeating timer plus a one-off tick from the plugin assignment thread).
const TIMER_KDE_HOLD: i32 = 6;
/// The desktop wallpaper rollback finished (a one-off tick from the rollback
/// thread) — tell the user how it ended.
const TIMER_KDE_REVERT: i32 = 7;
/// Preparing the X11 desktop icons finished (a one-off tick from its thread)
/// — tell the user what happened with the icons.
const TIMER_DESKTOP_ICONS: i32 = 8;
/// The step of checking the held-back frames.
const KDE_HOLD_TICK_MS: i32 = 250;

/// The reconnection pause — the same as stood in the window: 3000 ms,
/// without back-off and without an attempt limit. There is nothing to
/// improve here, and it is not the time.
const RECONNECT_MS: i32 = 3000;

// ── issuing request_id ──────────────────────────────────────────────────────

/// The `request_id` counter — ONE per process.
///
/// Previously there were two numbers: the Rust query's own token and the
/// Python client's own `request_id`, which for this parsed the ready frame
/// with `prost` and assembled it back. Now the number is one, and the
/// parsing is one.
fn ids() -> &'static Mutex<we_client::RequestIds> {
    static IDS: OnceLock<Mutex<we_client::RequestIds>> = OnceLock::new();
    IDS.get_or_init(|| Mutex::new(we_client::RequestIds::default()))
}

/// The next `request_id`.
pub fn alloc_request_id() -> i64 {
    let mut guard = ids().lock().expect("счётчик request_id не отравлен");
    guard.alloc() as i64
}

/// Start numbering anew (needed by the benches: the golden file numbers
/// frames from one).
pub fn reset_request_ids() {
    *ids().lock().expect("счётчик request_id не отравлен") = we_client::RequestIds::default();
}

// ── transport state ─────────────────────────────────────────────────────────

/// Callbacks of the transport probe (`set_hooks`). `extern "C"` because
/// `on_socket_error` with the same signature is also set into the C++
/// socket shim.
pub type VoidHook = Option<extern "C" fn()>;
pub type TextHook = Option<extern "C" fn(*const c_char, i32)>;
pub type FrameHook = Option<extern "C" fn(i64, *const c_char, i32)>;

thread_local! {
    /// The tab's live socket. A bare pointer rather than a RefCell with a
    /// struct: the answer handler synchronously calls the query, and it may
    /// immediately send a new frame — with a long borrow that would be a RefCell
    /// panic.
    static SOCKET: Cell<*mut QWebSocket> = const { Cell::new(std::ptr::null_mut()) };
    /// Guards of the connections to the socket's signals. They live exactly as
    /// long as the socket itself: they are removed BEFORE deletion, otherwise a
    /// guard would poke a dead object.
    static GUARDS: RefCell<Vec<QMetaObjectConnectionGuard>> = const { RefCell::new(Vec::new()) };
    /// The port of the last successful `open` — reconnection does not need it
    /// (the port is re-read over the bus), but the log and the bench do.
    static PORT: Cell<i32> = const { Cell::new(0) };
    /// Whether the connection has been established at least once. Without it
    /// reconnection does not start — exactly like `_was_connected` in the tab
    /// window.
    static WAS_CONNECTED: Cell<bool> = const { Cell::new(false) };
    /// The watcher of a running daemon launch (the decisions are the core's,
    /// `we_client::StartWatch`) and what the engine was launched with. `None`
    /// means there is no launch.
    static START: RefCell<Option<StartRun>> = const { RefCell::new(None) };
    /// The socket is being closed DELIBERATELY. Without this flag the
    /// disconnection handler would immediately start reconnecting and open the
    /// socket back — "close" would stop meaning "close".
    static CLOSING: Cell<bool> = const { Cell::new(false) };
    /// Whether to show the next transport error as a toast — the core's
    /// decision (`we_client::TransportErrorToast`): one toast per
    /// disconnection.
    static ERROR_TOAST: RefCell<we_client::TransportErrorToast> =
        RefCell::new(we_client::TransportErrorToast::default());

    static ON_CONNECTED: Cell<VoidHook> = const { Cell::new(None) };
    static ON_DISCONNECTED: Cell<VoidHook> = const { Cell::new(None) };
    static ON_ERROR: Cell<TextHook> = const { Cell::new(None) };
    static ON_RESPONSE: Cell<FrameHook> = const { Cell::new(None) };
    static ON_EVENT: Cell<TextHook> = const { Cell::new(None) };

    /// Subscribers in Rust (the page's action object, `we_actions.rs`).
    ///
    /// The `ON_*` cells above are the slot of ONE consumer (the transport
    /// probe), and whoever set it last takes it. There may be any number of
    /// subscribers in Rust, so the event goes to EACH: the slot and everyone in
    /// this list.
    static SUBSCRIBERS: RefCell<Vec<(u64, Rc<dyn Fn(&DaemonEvent)>)>> =
        const { RefCell::new(Vec::new()) };
    static NEXT_SUBSCRIBER: Cell<u64> = const { Cell::new(0) };
    /// `request_id` of the expected `health` answer; 0 — not waiting.
    static HEALTH_ID: Cell<u64> = const { Cell::new(0) };
    /// `request_id` of our own `plugin_list` (checking the plugins against the
    /// package); 0 — not waiting.
    static PLUGINS_ID: Cell<u64> = const { Cell::new(0) };
    /// The daemon's displays by its events (`display_snapshot/changed/removed`).
    /// Needed by one decision: whether there is anywhere to show an applied
    /// wallpaper.
    static DISPLAY_IDS: RefCell<std::collections::BTreeSet<u64>> =
        const { RefCell::new(std::collections::BTreeSet::new()) };
    /// Apply frames held back until the KDE plugin is assigned.
    static HELD: RefCell<Vec<Vec<u8>>> = const { RefCell::new(Vec::new()) };
    /// When the holding started.
    static HELD_SINCE: Cell<Option<std::time::Instant>> = const { Cell::new(None) };
    /// The watcher of a released apply (the decision is the core's,
    /// `ApplyWatch`) and the moment the frame was sent: if it failed, the
    /// desktops get their previous wallpapers back.
    static APPLY_WATCH: RefCell<Option<(rexon_core::waywallen_kde_plugin::ApplyWatch, std::time::Instant)>> =
        const { RefCell::new(None) };
}

fn call_text(hook: &'static std::thread::LocalKey<Cell<TextHook>>, text: &[u8]) {
    let Some(f) = hook.with(Cell::get) else { return };
    let len = i32::try_from(text.len()).unwrap_or(i32::MAX);
    f(text.as_ptr().cast::<c_char>(), len);
}

// ── parsing an incoming frame ───────────────────────────────────────────────

/// What arrived over the socket. `request_id` lies INSIDE `Response`, not in
/// the `ServerFrame` envelope — that is exactly the routing key.
enum Frame {
    Response(u64),
    Event(pb::Event),
}

fn read_frame(bytes: &[u8]) -> Result<Frame, prost::DecodeError> {
    let frame = pb::ServerFrame::decode(bytes)?;
    match frame.kind {
        Some(pb::server_frame::Kind::Response(resp)) => Ok(Frame::Response(resp.request_id)),
        Some(pb::server_frame::Kind::Event(event)) => Ok(Frame::Event(event)),
        None => Err(prost::DecodeError::new("пустой кадр сервера")),
    }
}

/// A `ServerFrame{Response{request_id, status, message}}` frame without a
/// payload — pending requests are closed with it on a disconnection.
///
/// Not a new mechanism but the same answer path: `we_query::take_response`
/// sees a non-OK status and moves the query into Error with this text. No
/// separate "the transport died" invokables had to be created.
fn error_frame(request_id: u64, message: &str) -> Vec<u8> {
    let resp = pb::Response {
        request_id,
        status: pb::Status::Internal as i32,
        message: message.to_owned(),
        ..Default::default()
    };
    pb::ServerFrame { kind: Some(pb::server_frame::Kind::Response(resp)) }.encode_to_vec()
}

/// The text the user will see instead of an eternal spinner.
const DISCONNECTED_TEXT: &str = rexon_core::i18n::n_("The connection to the wallpaper engine was lost");

/// Hand a daemon frame to its addressees.
///
/// The addressees:
///   * an answer → to our own service requests (`health`, `plugin_list`),
///     otherwise to the Rust query by the `QPointer` map (the shim),
///     otherwise to the probe slot;
///   * an event → to the long task progress, the `Notify`/`App` singletons
///     and the probe slot. The split is mandatory: the consumers differ.
pub fn dispatch_frame(bytes: &[u8]) -> bool {
    match read_frame(bytes) {
        Ok(Frame::Response(request_id)) => {
            // An answer to OUR OWN `health` (sent by `request_health`) — it is
            // addressed neither to a Rust query of the tab nor to the probe.
            if request_id != 0 && request_id == HEALTH_ID.with(Cell::get) {
                HEALTH_ID.with(|c| c.set(0));
                qobject::sock_timer_stop(TIMER_HEALTH);
                broadcast(&DaemonEvent::Health(health_state(bytes)));
                request_plugin_check();
                return true;
            }
            // An answer to OUR OWN `plugin_list` (checking against the package) —
            // nobody else's either.
            if request_id != 0 && request_id == PLUGINS_ID.with(Cell::get) {
                PLUGINS_ID.with(|c| c.set(0));
                on_plugin_list(bytes);
                return true;
            }
            watch_response(request_id, bytes);
            if crate::we_transport::deliver_response_bytes(request_id as i64, bytes) {
                return true;
            }
            let Some(f) = ON_RESPONSE.with(Cell::get) else { return false };
            let len = i32::try_from(bytes.len()).unwrap_or(i32::MAX);
            f(request_id as i64, bytes.as_ptr().cast::<c_char>(), len);
            true
        }
        Ok(Frame::Event(event)) => {
            let changed = DISPLAY_IDS.with(|ids| {
                rexon_core::waywallen_kde_plugin::track_displays(&mut ids.borrow_mut(), &event)
            });
            if changed {
                check_kde_hold();
            }
            // The watcher of a released apply: display links, renderers appearing
            // and being removed (a playlist does not name the renderer in the
            // answer — the core learns it by the displays, `ApplyWatch::on_event`).
            watch_verdict(|w| w.on_event(&event));
            crate::we_transport::deliver_event_bytes(bytes);
            crate::we_singletons::deliver_singleton_event_bytes(bytes);
            call_text(&ON_EVENT, bytes);
            true
        }
        Err(e) => {
            // A broken frame must give an error and be lost, not bring the process
            // down: the daemon may be newer than us.
            let text = rexon_core::we_client::frame_parse_error_message(&e.to_string());
            report_error(text.as_bytes());
            false
        }
    }
}

// ── socket lifecycle ────────────────────────────────────────────────────────

fn socket_ptr() -> *mut QWebSocket {
    SOCKET.with(Cell::get)
}

/// Create the socket and subscribe to its signals. Idempotent.
fn ensure_socket() -> *mut QWebSocket {
    let existing = socket_ptr();
    if !existing.is_null() {
        return existing;
    }
    let socket = qobject::sock_new();
    if socket.is_null() {
        return socket;
    }
    let mut sock = unsafe { Pin::new_unchecked(&mut *socket) };

    let on_connected = sock.as_mut().on_connected(|_sock| {
        WAS_CONNECTED.with(|c| c.set(true));
        ERROR_TOAST.with(|t| t.borrow_mut().on_connected());
        qobject::sock_timer_stop(TIMER_RECONNECT);
        // The tab's queries ask about the connection before sending a frame.
        // In Python each did it itself (`_client.is_open()`), here the transport
        // brings them the state — one place for all (rule #12).
        crate::discover_queries::set_connected(true);
        if let Some(f) = ON_CONNECTED.with(Cell::get) {
            f();
        }
        broadcast(&DaemonEvent::Connected);
    });
    let on_message = sock.as_mut().on_binary_message_received(|_sock, message| {
        let bytes: Vec<u8> = message.into();
        dispatch_frame(&bytes);
    });
    let on_disconnected = sock.as_mut().on_disconnected(|_sock| on_socket_closed());

    GUARDS.with(|g| {
        let mut list = g.borrow_mut();
        list.push(on_connected);
        list.push(on_message);
        list.push(on_disconnected);
    });
    unsafe { qobject::sock_watch_error(socket) };

    SOCKET.with(|c| c.set(socket));
    socket
}

/// Disconnection: the pending ones get an error, the subscribers a
/// notification, reconnection a start.
fn on_socket_closed() {
    crate::discover_queries::set_connected(false);
    // The displays will come again as a snapshot after reconnecting. Held-back
    // frames are thrown away: their queries will get a disconnection error
    // right now (`fail_pending` below), and sending them into the new
    // connection would be too late.
    DISPLAY_IDS.with(|ids| ids.borrow_mut().clear());
    drop_kde_hold();
    // Warning: a fix of the original's bug (rule #16): `_on_disconnected`
    // cleared the map silently — the callbacks were NEVER called, the query
    // stayed in the Querying status forever, and the user saw an eternal
    // spinner instead of an error. Now every pending one gets a frame with a
    // non-OK status.
    let frame = error_frame(0, &rexon_core::i18n::tr(DISCONNECTED_TEXT));
    crate::we_transport::fail_pending(&frame);

    if let Some(f) = ON_DISCONNECTED.with(Cell::get) {
        f();
    }
    // `Notify` must learn about the disconnection itself: previously the
    // Python window host told it, and with its departure nobody did — the
    // daemon phase stayed "ready" even without a daemon, and everything living
    // by it (the current wallpaper card in the rail, the vendored pages' hints)
    // showed yesterday's state.
    crate::we_singletons::transport_disconnected();
    // There will be no more `health` answer from a dropped socket. The watchdog
    // is removed rather than waiting for its term: in the Python window it
    // stayed armed, and a disconnection in the middle of a check declared the
    // engine dead 10 s later — although reconnection had already brought the
    // pages back by then.
    HEALTH_ID.with(|c| c.set(0));
    PLUGINS_ID.with(|c| c.set(0));
    qobject::sock_timer_stop(TIMER_HEALTH);
    broadcast(&DaemonEvent::Disconnected {
        was_connected: WAS_CONNECTED.with(Cell::get),
    });
    // Reconnection only after an established connection — the same caveat as
    // stood in the tab window (`_was_connected`) — and only if it broke not of
    // our own will.
    if WAS_CONNECTED.with(Cell::get) && !CLOSING.with(Cell::get) {
        qobject::sock_timer_start(TIMER_RECONNECT, RECONNECT_MS, false);
    }
    CLOSING.with(|c| c.set(false));
}

/// Open the socket on this port. The URL is hard-coded the same way as in
/// the original (`client.py:513`): the daemon listens only on the loopback.
pub fn open(port: i32) -> bool {
    let socket = ensure_socket();
    if socket.is_null() || port <= 0 {
        return false;
    }
    CLOSING.with(|c| c.set(false));
    PORT.with(|c| c.set(port));
    let mut sock = unsafe { Pin::new_unchecked(&mut *socket) };
    sock.as_mut()
        .open_url(&QUrl::from(format!("ws://127.0.0.1:{port}/").as_str()));
    true
}

pub fn close() {
    qobject::sock_timer_stop(TIMER_RECONNECT);
    let socket = socket_ptr();
    if socket.is_null() {
        return;
    }
    CLOSING.with(|c| c.set(true));
    unsafe { qobject::sock_close(socket) };
}

pub fn is_open() -> bool {
    let socket = socket_ptr();
    !socket.is_null() && unsafe { qobject::sock_is_open(socket) }
}

/// Send a request frame `field` taking into account who brings up the
/// display.
///
/// The only entry for the tab's request frames (`we_transport::send`). Where
/// the daemon's display is brought up by Rexon
/// (`wallpaper_backend::DisplaySetup`: KDE — the plugin, X11 — the output
/// client host), applying a wallpaper with zero displays is held back: the
/// daemon without a display would reject it (`NoDisplayRegistered`). The
/// display is brought up here — on the user's action, not when the tab
/// opens.
pub fn send_request_frame(field: &str, frame: &[u8]) {
    use rexon_core::waywallen_kde_plugin::{apply_gate, ApplyGate};
    if !is_open() {
        send_frame(frame);
        return;
    }
    let holding = HELD.with(|h| !h.borrow().is_empty());
    let displays = DISPLAY_IDS.with(|ids| ids.borrow().len());
    let gate = if holding && rexon_core::waywallen_kde_plugin::request_shows_wallpaper(field) {
        // We are already waiting for a display — a new apply frame joins the same
        // queue so that the order of applies does not get mixed up.
        ApplyGate::HoldAndSelect
    } else {
        apply_gate(display_setup().rexon_brings_display(), field, displays)
    };
    if rexon_core::waywallen_kde_plugin::request_shows_wallpaper(field) {
        // The user applied something else — there is no reason to watch the
        // previous apply: their new action matters more.
        watch_verdict(|w| w.on_new_apply());
        if display_setup() == rexon_core::wallpaper_backend::DisplaySetup::RexonHost {
            start_desktop_icons_prepare();
        }
        if rexon_core::wallpaper_backend::cinnamon_layer_shell_fact() {
            // Cinnamon with layer-shell: scene output is needed again (our own
            // wallpaper released it), and the scene surface gets the first frame
            // when the daemon brings up the renderer — lay the background service
            // under it later.
            rexon_core::wallpaper_backend::set_layer_shell_display_off_live(false);
            rexon_core::wallpaper_backend::restack_cinnamon_background_after("сцена Wallpaper Engine", CINNAMON_SCENE_RESTACK_MS);
        }
    }
    match gate {
        ApplyGate::Send => {
            send_frame(frame);
        }
        ApplyGate::HoldAndSelect => {
            HELD.with(|h| h.borrow_mut().push(frame.to_vec()));
            if holding {
                return;
            }
            HELD_SINCE.with(|c| c.set(Some(std::time::Instant::now())));
            if let Ok(mut slot) = KDE_PREVIOUS.lock() {
                *slot = None;
            }
            KDE_SELECTED_NOW.store(false, Ordering::SeqCst);
            KDE_REVERT_WANTED.store(false, Ordering::SeqCst);
            KDE_SELECT_STATE.store(KDE_SELECT_RUNNING, Ordering::SeqCst);
            qobject::sock_timer_start(TIMER_KDE_HOLD, KDE_HOLD_TICK_MS, false);
            if display_setup() == rexon_core::wallpaper_backend::DisplaySetup::RexonHost {
                rexon_core::log_info!("x11-display", "применение обоев: дисплеев нет — поднимаем вывод на рабочий стол X11, кадр «{field}» ждёт");
                start_rexon_display_host();
            } else if display_setup() == rexon_core::wallpaper_backend::DisplaySetup::GnomeExtension {
                rexon_core::log_info!("gnome-display", "применение обоев: дисплеев нет — включаем расширение вывода waywallen, кадр «{field}» ждёт");
                start_gnome_display_extension();
            } else {
                rexon_core::log_info!("kde-plugin", "применение обоев: дисплеев нет — назначаем плагин обоев KDE, кадр «{field}» ждёт");
                let daemon_version = KDE_DAEMON_VERSION.lock().ok().and_then(|v| v.clone());
                prepare_display_backend(daemon_version, find_daemon_binary(), false, KdePurpose::Apply);
            }
        }
    }
}

/// How long after applying a scene to restack the Cinnamon background.
const CINNAMON_SCENE_RESTACK_MS: u64 = 5000;

/// The plugin assignment state for the sake of held-back frames.
static KDE_SELECT_STATE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(KDE_SELECT_IDLE);
const KDE_SELECT_IDLE: u8 = 0;
const KDE_SELECT_RUNNING: u8 = 1;
const KDE_SELECT_OK: u8 = 2;
const KDE_SELECT_FAILED: u8 = 3;

/// The desktops' plugins before the assignment — a snapshot for the
/// rollback.
static KDE_PREVIOUS: Mutex<Option<Vec<rexon_core::waywallen_kde_plugin::DesktopPlugin>>> = Mutex::new(None);
/// The plugin was assigned for this apply (it was not on the desktops
/// before).
static KDE_SELECTED_NOW: AtomicBool = AtomicBool::new(false);
/// A rollback is needed, but the assignment has not reported yet — do it
/// at its end.
static KDE_REVERT_WANTED: AtomicBool = AtomicBool::new(false);
/// The rollback thread's outcome for the main thread.
static KDE_REVERT_RESULT: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(KDE_REVERT_NONE);
const KDE_REVERT_NONE: u8 = 0;
const KDE_REVERT_OK: u8 = 1;
const KDE_REVERT_FAILED: u8 = 2;

/// The output was brought up not by a plugin assignment (X11): there is
/// nothing to roll back, and further holding is run by the same core
/// (`release_held`).
fn kde_select_finished_without_plugin(ok: bool) {
    if let Ok(mut slot) = KDE_PREVIOUS.lock() {
        *slot = None;
    }
    KDE_SELECTED_NOW.store(false, Ordering::SeqCst);
    KDE_SELECT_STATE.store(if ok { KDE_SELECT_OK } else { KDE_SELECT_FAILED }, Ordering::SeqCst);
    qobject::sock_post_tick(TIMER_KDE_HOLD);
}

/// The assignment thread finished (any thread) — wake up the check in the
/// main one.
///
/// `previous` are the desktops' plugins before the assignment (present only
/// when the plugin was just assigned).
fn kde_select_finished(
    selected: bool,
    previous: Option<Vec<rexon_core::waywallen_kde_plugin::DesktopPlugin>>,
) {
    if let Ok(mut slot) = KDE_PREVIOUS.lock() {
        *slot = previous;
    }
    KDE_SELECTED_NOW.store(selected, Ordering::SeqCst);
    KDE_SELECT_STATE.store(if selected { KDE_SELECT_OK } else { KDE_SELECT_FAILED }, Ordering::SeqCst);
    qobject::sock_post_tick(TIMER_KDE_HOLD);
}

/// Release the held-back frames if it is time (the decision is the core's),
/// and run the watcher of an already released apply.
fn check_kde_hold() {
    if KDE_REVERT_WANTED.load(Ordering::SeqCst)
        && KDE_SELECT_STATE.load(Ordering::SeqCst) != KDE_SELECT_RUNNING
    {
        KDE_REVERT_WANTED.store(false, Ordering::SeqCst);
        start_kde_revert();
    }
    if HELD.with(|h| h.borrow().is_empty()) {
        if let Some(elapsed) = APPLY_WATCH.with(|w| w.borrow().as_ref().map(|(_, t)| t.elapsed())) {
            let ms = u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX);
            watch_verdict(|w| w.on_tick(ms));
        }
        let watching = APPLY_WATCH.with(|w| w.borrow().is_some());
        if !watching && !KDE_REVERT_WANTED.load(Ordering::SeqCst) {
            qobject::sock_timer_stop(TIMER_KDE_HOLD);
        }
        return;
    }
    let state = KDE_SELECT_STATE.load(Ordering::SeqCst);
    let done = matches!(state, KDE_SELECT_OK | KDE_SELECT_FAILED);
    let ok = state == KDE_SELECT_OK;
    let displays = DISPLAY_IDS.with(|ids| ids.borrow().len());
    let waited = HELD_SINCE
        .with(Cell::get)
        .map_or(u64::MAX, |t| u64::try_from(t.elapsed().as_millis()).unwrap_or(u64::MAX));
    if !rexon_core::waywallen_kde_plugin::release_held(done, ok, displays, waited) {
        return;
    }
    let frames = HELD.with(|h| std::mem::take(&mut *h.borrow_mut()));
    HELD_SINCE.with(|c| c.set(None));
    rexon_core::log_info!(
        "kde-plugin",
        "применение обоев: отпускаем {} кадр(ов) — дисплеев {displays}, вывод: {}, ждали {waited} мс",
        frames.len(),
        rexon_core::waywallen_kde_plugin::select_state_for_log(done, ok)
    );
    // What ends up on the desktop is decided by the LAST frame — we watch it.
    let last_id = frames.last().and_then(|f| pb::Request::decode(f.as_slice()).ok()).map(|r| r.request_id);
    for frame in &frames {
        send_frame(frame);
    }
    APPLY_WATCH.with(|w| {
        *w.borrow_mut() = last_id
            .map(|id| (rexon_core::waywallen_kde_plugin::ApplyWatch::new(id), std::time::Instant::now()));
    });
    // The timer keeps ticking — now for the watcher.
}

/// The daemon's answer to the watched apply frame — give it to the watcher.
fn watch_response(request_id: u64, bytes: &[u8]) {
    let watched = APPLY_WATCH.with(|w| w.borrow().as_ref().is_some_and(|(w, _)| w.watches(request_id)));
    if !watched {
        return;
    }
    let Ok(pb::ServerFrame { kind: Some(pb::server_frame::Kind::Response(rsp)) }) =
        pb::ServerFrame::decode(bytes)
    else {
        return;
    };
    let ok = rsp.status == pb::Status::Ok as i32;
    let renderer_id = match &rsp.payload {
        Some(pb::response::Payload::WallpaperApply(apply)) => apply.renderer_id.clone(),
        _ => String::new(),
    };
    if !ok {
        rexon_core::log_warn!("kde-plugin", "применение обоев не удалось: {}", rsp.message);
    }
    watch_verdict(|w| w.on_response(request_id, ok, &renderer_id));
}

/// A step of the apply watcher; the outcome is by the core's decision.
fn watch_verdict(step: impl FnOnce(&mut rexon_core::waywallen_kde_plugin::ApplyWatch) -> rexon_core::waywallen_kde_plugin::WatchVerdict) {
    use rexon_core::waywallen_kde_plugin::WatchVerdict;
    let verdict = APPLY_WATCH.with(|w| w.borrow_mut().as_mut().map(|(watch, _)| step(watch)));
    match verdict {
        None | Some(WatchVerdict::Keep) => {}
        Some(WatchVerdict::Done) => {
            APPLY_WATCH.with(|w| *w.borrow_mut() = None);
        }
        Some(WatchVerdict::Revert) => {
            APPLY_WATCH.with(|w| *w.borrow_mut() = None);
            if KDE_SELECT_STATE.load(Ordering::SeqCst) == KDE_SELECT_RUNNING {
                // There is no snapshot of the previous plugins yet — roll back at the end
                // of the assignment.
                KDE_REVERT_WANTED.store(true, Ordering::SeqCst);
                qobject::sock_timer_start(TIMER_KDE_HOLD, KDE_HOLD_TICK_MS, false);
            } else {
                start_kde_revert();
            }
        }
    }
}

/// Return to the desktops the plugin they had before the assignment (the
/// decision is the core's). The Plasma bus is slow — in a separate thread;
/// the outcome comes with a `TIMER_KDE_REVERT` tick.
fn start_kde_revert() {
    if display_setup() == rexon_core::wallpaper_backend::DisplaySetup::RexonHost {
        start_desktop_icons_restore();
    }
    let previous = KDE_PREVIOUS.lock().ok().and_then(|mut slot| slot.take());
    let selected_now = KDE_SELECTED_NOW.swap(false, Ordering::SeqCst);
    let Some(js) = rexon_core::waywallen_kde_plugin::revert_plugins_js(selected_now, previous.as_deref()) else {
        rexon_core::log_info!(
            "kde-plugin",
            "применение обоев не удалось; откатывать нечего (плагин стоял до применения или снимка нет)"
        );
        return;
    };
    rexon_core::log_info!("kde-plugin", "применение обоев не удалось — возвращаем столам прежние обои");
    let spawned = std::thread::Builder::new().name("rexon-kde-revert".into()).spawn(move || {
        let ok = {
            let _guard = KDE_ENSURE_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            plasma_evaluate(&KdeEnv::live(), &js, 15.0).is_some()
        };
        KDE_REVERT_RESULT.store(if ok { KDE_REVERT_OK } else { KDE_REVERT_FAILED }, Ordering::SeqCst);
        qobject::sock_post_tick(TIMER_KDE_REVERT);
    });
    if let Err(e) = spawned {
        rexon_core::log_warn!("kde-plugin", "откат обоев не запущен: {e}");
        KDE_REVERT_RESULT.store(KDE_REVERT_FAILED, Ordering::SeqCst);
        kde_revert_finished();
    }
}

/// The rollback thread finished — tell the user how it ended.
fn kde_revert_finished() {
    use rexon_core::waywallen_kde_plugin as wkp;
    let text = match KDE_REVERT_RESULT.swap(KDE_REVERT_NONE, Ordering::SeqCst) {
        KDE_REVERT_OK => {
            rexon_core::log_info!("kde-plugin", "прежние обои столов возвращены");
            wkp::revert_done_text()
        }
        KDE_REVERT_FAILED => {
            rexon_core::log_warn!("kde-plugin", "прежние обои столов вернуть не вышло: Plasma не ответила");
            // Here the user has something to DO themselves (restore the wallpaper
            // by hand) — this is an error toast, it stays until closed, as at the
            // vendor.
            crate::we_singletons::client_error(&wkp::revert_failed_text());
            return;
        }
        _ => return,
    };
    // The wallpaper is already restored, nothing needs to be done — the message
    // is informational and goes away by itself (defect of 30.10.2026: the
    // error toast hung for more than a minute and covered the panel buttons,
    // including the filter).
    broadcast(&DaemonEvent::Notice(text));
}

/// Remove the holding entirely (the frames are thrown away).
fn drop_kde_hold() {
    APPLY_WATCH.with(|w| *w.borrow_mut() = None);
    KDE_REVERT_WANTED.store(false, Ordering::SeqCst);
    HELD.with(|h| h.borrow_mut().clear());
    HELD_SINCE.with(|c| c.set(None));
    KDE_SELECT_STATE.store(KDE_SELECT_IDLE, Ordering::SeqCst);
    qobject::sock_timer_stop(TIMER_KDE_HOLD);
}

/// Send a ready `Request` frame. `false` means the socket is closed.
pub fn send_frame(frame: &[u8]) -> bool {
    let socket = socket_ptr();
    if socket.is_null() || !unsafe { qobject::sock_is_open(socket) } {
        return false;
    }
    let mut sock = unsafe { Pin::new_unchecked(&mut *socket) };
    let payload = QByteArray::from(frame);
    let sent = i64::from(sock.as_mut().send_binary_message(&payload));
    sent > 0
}

// ── DaemonControl: finding and auto-starting the daemon ─────────────────────

/// The live transport of the core's `WaywallenIo` trait over `Properties.Get`.
struct DbusProps;

impl rexon_core::waywallen_client::WaywallenIo for DbusProps {
    fn is_valid(&mut self) -> bool {
        true
    }

    fn get_property(&mut self, name: &str) -> DbusValue {
        let mut ok = false;
        let value = qobject::sock_daemon_property(
            &QString::from(we_client::DBUS_SERVICE),
            &QString::from(we_client::DBUS_PATH),
            &QString::from(we_client::DBUS_IFACE),
            &QString::from(name),
            i32::try_from(we_client::PROPERTY_TIMEOUT_MS).unwrap_or(1000),
            &mut ok,
        );
        if !ok {
            return DbusValue::None;
        }
        DbusValue::Str(value.to_string())
    }

    fn call(
        &mut self,
        _method: &str,
        _args: &[DbusValue],
    ) -> rexon_core::waywallen_client::DbusReply {
        // The transport calls no bus methods: all it needs is three properties.
        // The dead `ListTasks` was removed from the product on 18.08.
        rexon_core::waywallen_client::DbusReply {
            error_name: "org.freedesktop.DBus.Error.NotSupported".to_owned(),
            error_message: "транспорт не вызывает методов шины".to_owned(),
            arguments: Vec::new(),
        }
    }
}

pub fn probe() -> we_client::DaemonStatus {
    we_client::probe(&mut DbusProps)
}

/// Finding the daemon binary: the order of candidates is decided by the
/// core, the live FS checks are the crate-wide `we_env::FsProbe` (there is no
/// second copy, rule #12).
pub fn find_daemon_binary() -> Option<String> {
    crate::we_env::daemon_binary()
}

/// A port of `DaemonControl.ensure_running`: the daemon is alive → `ready`
/// at once; if not → auto-start and polling the bus. The decisions are
/// entirely the core's.
pub fn ensure_running() {
    // The only point every opening of the WE tab passes through — here we
    // also catch up on fixes of the autostart template for those who enabled it
    // long ago and have not touched the toggle since.
    crate::autostart_queries::refresh_autostart_unit();
    install_shim_hooks();

    let st = probe();
    let binary = find_daemon_binary();
    // The plugins from the package (Open Wallpaper Engine) are seen by a daemon
    // whose file lives under a different prefix only by the `--plugin` flag.
    let plugin_root = crate::we_env::engine_plugin_root_for(binary.as_deref());
    // Autostart is enabled — the daemon is brought up through the service, not
    // as a second process bypassing it: the daemon is one and under the
    // service's supervision.
    let service = !st.running && crate::autostart_queries::autostart_service_present();
    let decision = we_client::ensure_running_decision_service(
        &st,
        binary.as_deref(),
        plugin_root.as_deref(),
        service,
    );

    if decision.ensure_kde_plugin() {
        // Best-effort preparation of the display (on KDE — the Plasma wallpaper
        // plugin). The live daemon's version is needed by the plugin
        // compatibility watcher; if there is no daemon yet, the preparation will
        // ask `--version` of the binary that will be launched.
        let daemon_version = (st.running && !st.version.trim().is_empty())
            .then(|| st.version.trim().to_owned());
        // Never: only installation: opening the tab does NOT change the desktop
        // wallpaper (defect of 30.10.2026 — a black desktop with an empty
        // library). The plugin is assigned as the wallpaper on applying —
        // `send_request_frame`.
        prepare_display_backend(daemon_version, binary.clone(), false, KdePurpose::Prepare);
    }

    match decision {
        we_client::EnsureAction::Ready { status } => daemon_ready(&status),
        we_client::EnsureAction::NoBinary { message } => daemon_failed(&message),
        we_client::EnsureAction::Launch {
            program,
            args,
            polls_left: _,
            poll_interval_ms,
        } => {
            // A launch is already in progress (a repeated press, a second tab) — a
            // second process is not needed: the first one's watcher will see it
            // through.
            if START.with(|s| s.borrow().is_some()) {
                broadcast(&DaemonEvent::Starting { slow: false });
                return;
            }
            // In a separate session: the daemon must outlive Rexon being closed —
            // the wallpaper does not go out together with the window.
            let Some(child) = spawn_engine(&program, &args) else {
                daemon_failed(&we_client::daemon_launch_failed_message(&program));
                return;
            };
            // Under the service `systemctl` was launched, not the engine: we ask the
            // service about the engine. Otherwise we judge by the process's own PID.
            let kind = if service { StartKind::Service } else { StartKind::Process(child.id()) };
            let watch = we_client::StartWatch::new(matches!(kind, StartKind::Process(p) if p != 0));
            START.with(|s| {
                *s.borrow_mut() = Some(StartRun {
                    watch,
                    kind,
                    slow: false,
                    step_ms: poll_interval_ms,
                    child: Some(child),
                })
            });
            broadcast(&DaemonEvent::Starting { slow: false });
            qobject::sock_timer_start(
                TIMER_POLL,
                i32::try_from(poll_interval_ms).unwrap_or(500),
                false,
            );
        }
    }
}

/// What the engine was launched with — whom to ask for signs of life depends
/// on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StartKind {
    /// A separate process with this PID (0 — the PID is unknown).
    Process(u32),
    /// The autostart service (`systemctl --user start`).
    Service,
}

/// A running daemon launch.
struct StartRun {
    watch: we_client::StartWatch,
    kind: StartKind,
    /// We have already told the user that the launch is taking long.
    slow: bool,
    step_ms: u64,
    /// The launched process (the engine or `systemctl`): its exit code is
    /// learned through it. After the launch it is picked up by the reaper
    /// thread (`reap_engine`).
    child: Option<std::process::Child>,
}

/// Launch the engine as our own child in a separate session.
///
/// Previously this was `QProcess::startDetached`: the exit code of someone
/// else's child cannot be learned, and the daemon's stderr inherited the
/// window's descriptor 2 — rexon.log — and mixed with our lines. Now the
/// output goes to its own file next to the log
/// (`we_client::DAEMON_OUTPUT_LOG`, anew on every launch), and the exit code
/// is seen by the launch watcher. The daemon still survives the window
/// closing: its own session (`setsid`), it gets no signals when the parent
/// exits.
fn spawn_engine(program: &str, args: &[String]) -> Option<std::process::Child> {
    use std::os::unix::process::CommandExt;
    use std::process::Stdio;
    let out_path = engine_output_path();
    let (stdout, stderr) = match std::fs::File::create(&out_path) {
        Ok(f) => match f.try_clone() {
            Ok(c) => (Stdio::from(f), Stdio::from(c)),
            Err(_) => (Stdio::null(), Stdio::from(f)),
        },
        Err(e) => {
            rexon_core::log_warn!("we", "файл вывода движка {out_path} не создан: {e}");
            (Stdio::null(), Stdio::null())
        }
    };
    let mut cmd = std::process::Command::new(program);
    cmd.args(args).stdin(Stdio::null()).stdout(stdout).stderr(stderr);
    // SAFETY: in the child process before exec — only setsid (async-signal-safe).
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    match cmd.spawn() {
        Ok(child) => {
            rexon_core::log_info!("we", "движок запущен: pid {} ({program} {}), вывод — {out_path}", child.id(), args.join(" "));
            Some(child)
        }
        Err(e) => {
            rexon_core::log_warn!("we", "движок не запустился ({program}): {e}");
            None
        }
    }
}

/// The engine's output file — next to the Rexon log.
fn engine_output_path() -> String {
    let log = rexon_core::applog::log_path();
    let dir = log.parent().map(|d| d.to_string_lossy().into_owned()).unwrap_or_else(|| ".".into());
    let _ = std::fs::create_dir_all(&dir);
    we_client::daemon_output_log_path(&dir)
}

/// Pick up the process after the launch in a separate thread: without this a
/// dead daemon would hang as a zombie while the window is open. The exit
/// code goes as a line into the log.
fn reap_engine(mut child: std::process::Child) {
    let spawned = std::thread::Builder::new().name("we-reap".into()).spawn(move || {
        let pid = child.id();
        if let Ok(status) = child.wait() {
            rexon_core::log_info!("we", "процесс движка pid {pid} завершился: {status}");
        }
    });
    if let Err(e) = spawned {
        rexon_core::log_warn!("we", "поток-сборщик движка не запущен: {e}");
    }
}

/// Log lines about a launch refusal (`we_client::daemon_exit_report`): the
/// exit code, who holds the bus name, the tail of the daemon's output.
fn log_engine_exit(child: Option<&mut std::process::Child>, kind: StartKind) {
    use std::os::unix::process::ExitStatusExt;
    let StartKind::Process(pid) = kind else {
        // Under the service — `journalctl --user -u <unit>`; the path to it is
        // already named in the log by the refusal line.
        return;
    };
    let exit = match child.map(|c| c.try_wait()) {
        Some(Ok(Some(st))) => match (st.code(), st.signal()) {
            (Some(c), _) => we_client::EngineExit::Code(c),
            (None, Some(s)) => we_client::EngineExit::Signal(s),
            _ => we_client::EngineExit::Unknown,
        },
        _ => we_client::EngineExit::Unknown,
    };
    let owner = run(&we_client::daemon_name_owner_pid_argv(), we_client::BUSCTL_TIMEOUT_SECS as f64).map(|out| {
        match we_client::parse_name_owner_pid(&out.stdout) {
            Some(p) => {
                let cmdline = std::fs::read(format!("/proc/{p}/cmdline"))
                    .map(|b| String::from_utf8_lossy(&b).replace('\0', " ").trim().to_string())
                    .unwrap_or_default();
                we_client::BusNameOwner::Process { pid: p, cmdline }
            }
            None => we_client::BusNameOwner::Free,
        }
    });
    let path = engine_output_path();
    let text = std::fs::read(&path).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
    let tail = we_client::text_tail(&text, we_client::DAEMON_OUTPUT_TAIL_LINES);
    for line in we_client::daemon_exit_report(pid, exit, owner.as_ref(), &path, &tail) {
        rexon_core::log_warn!("we", "{line}");
    }
}

/// Signs of life of the engine being launched. There is no decision here —
/// only collecting a snapshot for the core.
fn engine_life(kind: StartKind) -> Option<we_client::EngineLife> {
    match kind {
        StartKind::Process(0) => None,
        StartKind::Process(pid) => we_client::read_process_life(pid),
        StartKind::Service => {
            let out = run(&we_client::engine_unit_show_argv(), we_client::BUSCTL_TIMEOUT_SECS as f64)?;
            let entries = we_client::read_proc_entries();
            we_client::service_life(&out.stdout, &entries)
        }
    }
}

/// The daemon launcher for the vendored pages (their launch button calls
/// `rexon_we_launch_daemon` of the singletons shim).
///
/// One for both processes. Under the Python host the launcher created a NEW
/// `DaemonControl`, and it took the flow's callbacks for itself — after a
/// press the tab window stopped learning "the daemon is ready / did not come
/// up". Here the callbacks are not reassigned: the events go to those
/// already subscribed.
pub extern "C" fn launch_daemon_hook() -> bool {
    if find_daemon_binary().is_none() {
        return false;
    }
    ensure_running();
    true
}

/// A tick of the launch watcher: the bus, the process's signs of life — the
/// core's decision.
fn poll_once() {
    let Some((kind, step_ms)) = START.with(|s| s.borrow().as_ref().map(|r| (r.kind, r.step_ms))) else {
        qobject::sock_timer_stop(TIMER_POLL);
        return;
    };
    let st = probe();
    // While there is no bus, we ask the process; once it is up, there is no
    // reason to ask.
    let life = if st.running && st.ws_port != 0 { None } else { engine_life(kind) };
    let step = START.with(|s| {
        s.borrow_mut().as_mut().map(|r| r.watch.tick(&st, life, step_ms))
    });
    match step {
        Some(we_client::StartStep::Ready { status }) => {
            finish_start();
            daemon_ready(&status);
        }
        Some(we_client::StartStep::Failed { message }) => {
            let waited = START.with(|s| s.borrow().as_ref().map_or(0, |r| r.watch.elapsed_ms()));
            rexon_core::log_warn!("we", "движок не запустился за {waited} мс: {message}");
            START.with(|s| {
                if let Some(run) = s.borrow_mut().as_mut() {
                    log_engine_exit(run.child.as_mut(), run.kind);
                }
            });
            finish_start();
            daemon_failed(&message);
        }
        Some(we_client::StartStep::Wait { slow }) => {
            let newly_slow = START.with(|s| {
                let mut s = s.borrow_mut();
                let run = s.as_mut()?;
                let newly = slow && !run.slow;
                run.slow |= slow;
                Some(newly)
            });
            if newly_slow == Some(true) {
                rexon_core::log_info!(
                    "we",
                    "движок запускается дольше обычного, процесс жив — ждём ({kind:?})"
                );
                broadcast(&DaemonEvent::Starting { slow: true });
            }
        }
        None => qobject::sock_timer_stop(TIMER_POLL),
    }
}

/// The launch ended (with success or a refusal): the watcher is removed, the
/// timer is stopped, the process is handed to the reaper.
fn finish_start() {
    qobject::sock_timer_stop(TIMER_POLL);
    if let Some(child) = START.with(|s| s.borrow_mut().take()).and_then(|r| r.child) {
        reap_engine(child);
    }
}

/// One reconnection attempt: the port is re-read over the bus every time —
/// after a restart the daemon listens on a DIFFERENT port.
fn reconnect_once() {
    if is_open() {
        qobject::sock_timer_stop(TIMER_RECONNECT);
        return;
    }
    // The daemon left at our request for the sake of the plugin from the
    // package — bring it up the ordinary way (the service or a launch with
    // `--plugin`) rather than waiting for someone else to bring it up.
    if PLUGIN_RELAUNCH.load(Ordering::SeqCst) {
        plugin_relaunch();
        return;
    }
    let st = probe();
    if st.running && st.ws_port != 0 {
        open(i32::try_from(st.ws_port).unwrap_or(0));
    }
}

/// A tick of the shim's timers. One entry for all timers: the number has no
/// meaning of its own in C++, Rust parses it.
extern "C" fn on_timer_tick(id: i32) {
    match id {
        TIMER_POLL => poll_once(),
        TIMER_RECONNECT => reconnect_once(),
        TIMER_HEALTH => health_timed_out(),
        TIMER_START => ensure_running(),
        TIMER_PLUGIN_RELAUNCH => plugin_relaunch(),
        TIMER_KDE_HOLD => check_kde_hold(),
        TIMER_KDE_REVERT => kde_revert_finished(),
        TIMER_DESKTOP_ICONS => desktop_icons_prepared(),
        _ => {}
    }
}

/// A socket error from the shim (the `errorOccurred` subscription).
extern "C" fn on_socket_error(text: *const c_char, len: i32) {
    if text.is_null() || len < 0 {
        return;
    }
    let bytes = unsafe { std::slice::from_raw_parts(text.cast::<u8>(), len as usize) };
    report_error(bytes);
}

/// A transport error — to the `ON_ERROR` slot (the probe) and as a toast of
/// the tab window through `App.errorOccurred`, as in the original.
///
/// The toast is not for every error: while there is no daemon, every
/// reconnection attempt gives the same socket error, and the decision
/// "whether to show" is made by the core (`we_client::TransportErrorToast`).
/// The probe gets everything.
fn report_error(text: &[u8]) {
    call_text(&ON_ERROR, text);
    let text = String::from_utf8_lossy(text);
    if ERROR_TOAST.with(|t| t.borrow_mut().on_error(&text)) {
        crate::we_singletons::client_error(&text);
    }
}

// ── subscribers in Rust ─────────────────────────────────────────────────────
//
// Six seams that the Python tab window used to hold (`qml_window.py`: the
// daemon is ready / did not come up, the socket is open / dropped, the
// `health` answer / its absence). There are no decisions in them: the
// verdict on `health` is given by the core (`we_window::health_verdict`),
// the screen words are the core's too. Here there is only broadcasting, and
// the page's action object receives it (`we_actions.rs`).

/// A daemon and socket event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DaemonEvent {
    /// The daemon is alive, the socket port is known.
    Ready(we_client::DaemonStatus),
    /// The daemon did not come up; the text is the raw reason from the core.
    Failed(String),
    /// The socket is open (this is not yet "the engine works" — see `Health`).
    Connected,
    /// The socket is dropped. `was_connected` — the connection existed at least
    /// once, that is, this is an engine restart, not a failed first visit.
    Disconnected { was_connected: bool },
    /// The `health` answer: the `state` field (empty — an old daemon does not
    /// send it).
    Health(String),
    /// The daemon connected and is silent on `health` longer than the core's
    /// term.
    HealthTimeout,
    /// A message for the user from the transport (already translated), shown as
    /// an INFORMATIONAL toast of the tab that goes away by itself: for example,
    /// «the live wallpaper did not start — the previous desktop wallpaper is
    /// restored».
    Notice(String),
    /// The engine is launched and starting (the launch watcher is still waiting
    /// for the bus). `slow` — the ordinary start term is over, but the process is
    /// alive and working: the waiting screen says that the first launch may take
    /// long.
    Starting { slow: bool },
}

/// Subscribe to daemon events. Returns a number for `unsubscribe`.
///
/// The callback is called SYNCHRONOUSLY in the transport thread (the main
/// one); what to do with the event next — whether to queue it into the
/// object — is decided by the subscriber.
pub fn subscribe(callback: impl Fn(&DaemonEvent) + 'static) -> u64 {
    let id = NEXT_SUBSCRIBER.with(|c| {
        let next = c.get() + 1;
        c.set(next);
        next
    });
    SUBSCRIBERS.with(|s| s.borrow_mut().push((id, Rc::new(callback))));
    id
}

/// Remove a subscription. Idempotent: `0` means there was no subscription.
pub fn unsubscribe(id: u64) {
    if id == 0 {
        return;
    }
    SUBSCRIBERS.with(|s| s.borrow_mut().retain(|(entry, _)| *entry != id));
}

/// Broadcast an event to all subscribers in Rust.
///
/// The list is copied BEFORE the calls: a subscriber may immediately
/// unsubscribe or add a new one, and with a live borrow that would be a
/// RefCell panic.
fn broadcast(event: &DaemonEvent) {
    let list: Vec<Rc<dyn Fn(&DaemonEvent)>> =
        SUBSCRIBERS.with(|s| s.borrow().iter().map(|(_, f)| Rc::clone(f)).collect());
    for callback in list {
        callback(event);
    }
}

fn daemon_ready(status: &we_client::DaemonStatus) {
    broadcast(&DaemonEvent::Ready(status.clone()));
}

fn daemon_failed(message: &str) {
    broadcast(&DaemonEvent::Failed(message.to_owned()));
}

/// The shim hands timer ticks and socket errors to Rust: they are handled by
/// the transport state, not by the tab host. They are set on any entry into
/// the transport, not only from the probe's `set_hooks`: without this the
/// window's daemon polling and reconnection would silently never tick.
fn install_shim_hooks() {
    unsafe {
        rexon_we_sock_set_timer_tick(Some(on_timer_tick));
        rexon_we_sock_set_error_hook(Some(on_socket_error));
    }
}

/// Bring up the daemon after `delay_ms` (0 — on the next tick of the event
/// loop).
///
/// Deferred, as in the Python window: detection and auto-start go to D-Bus
/// and the file system, and the tab must manage to show itself first.
pub fn ensure_running_later(delay_ms: i32) {
    install_shim_hooks();
    qobject::sock_timer_start(TIMER_START, delay_ms.max(0), true);
}

/// Ask the connected daemon for `health`; the watchdog is `timeout_ms`.
///
/// `false` — the socket is closed, there is nobody to ask. The answer comes
/// as the `Health` event, silence as `HealthTimeout`.
pub fn request_health(timeout_ms: i32) -> bool {
    install_shim_hooks();
    if !is_open() {
        return false;
    }
    let request_id = alloc_request_id() as u64;
    HEALTH_ID.with(|c| c.set(request_id));
    if !send_frame(&we_client::health(request_id)) {
        HEALTH_ID.with(|c| c.set(0));
        return false;
    }
    // A zero term does not arm the watchdog: otherwise it would fire on the
    // very first tick and declare a healthy engine dead.
    if timeout_ms > 0 {
        qobject::sock_timer_start(TIMER_HEALTH, timeout_ms, true);
    }
    true
}

// ── checking the live daemon's plugins against the package ──────────────────
//
// The daemon chooses the plugin copy (the package or the user's) ONCE — at
// start-up. Launched before a package update, it keeps the old copy until
// it is restarted (the analysis is in `we_client::plugin_refresh_decision`).
// The check goes by two paths, and the decision and the restart are shared:
//   * after every `health` answer of the tab — `plugin_list` over its socket;
//   * right after a package update — the tray watcher
//     (`spawn_package_watch`) hears a write into the engine directory and
//     asks for `plugin_list` with a one-off socket from its own thread.
// If it is behind — a restart, once per divergence.

/// The divergence key for which a restart was already done in this process.
static PLUGIN_REFRESH_TRIED: Mutex<Option<String>> = Mutex::new(None);
/// The daemon left by `Quit` for the sake of the plugin — it will be brought
/// up by the [`TIMER_PLUGIN_RELAUNCH`] tick (or by the tab's reconnection
/// tick, whichever comes first).
static PLUGIN_RELAUNCH: AtomicBool = AtomicBool::new(false);

/// Ask for `plugin_list` if there is something to compare with (Rexon from
/// the package).
fn request_plugin_check() {
    if crate::we_env::package_engine().is_none() || !is_open() {
        return;
    }
    let request_id = alloc_request_id() as u64;
    PLUGINS_ID.with(|c| c.set(request_id));
    if !send_frame(&we_client::plugin_list(request_id)) {
        PLUGINS_ID.with(|c| c.set(0));
    }
}

fn on_plugin_list(frame: &[u8]) {
    let Some(active) = we_client::active_plugins_from_frame(frame) else {
        return;
    };
    if let Some((key, stale)) = plugin_refresh_step(&active) {
        std::thread::spawn(move || {
            restart_for_plugins(key, stale);
        });
    }
}

/// The shared part of both paths: compare the active copies with the
/// package and decide. `Some` — restart (the key is already remembered);
/// `None` — not needed or already tried.
fn plugin_refresh_step(active: &[we_client::PluginCopy]) -> Option<(String, Vec<we_client::StalePlugin>)> {
    let bundled = crate::we_env::bundled_plugins();
    let tried = PLUGIN_REFRESH_TRIED.lock().map(|g| g.clone()).unwrap_or(None);
    match we_client::plugin_refresh_decision(&bundled, active, tried.as_deref()) {
        we_client::PluginRefresh::Fine => None,
        we_client::PluginRefresh::GiveUp { stale } => {
            rexon_core::log_warn!(
                "we",
                "демон после перезапуска всё равно не взял плагины из пакета ({}); \
                 его, вероятно, запускает не Rexon и без --plugin",
                we_client::stale_plugins_key(&stale)
            );
            None
        }
        we_client::PluginRefresh::Restart { key, stale } => {
            if let Ok(mut g) = PLUGIN_REFRESH_TRIED.lock() {
                *g = Some(key.clone());
            }
            Some((key, stale))
        }
    }
}

/// How the restart attempt ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RestartOutcome {
    Restarted,
    /// The daemon is busy with tasks — not touched, the attempt key is removed.
    Busy,
    Failed,
}

/// Restart the daemon for the sake of the package's plugins; if it worked —
/// a notification.
fn restart_for_plugins(key: String, stale: Vec<we_client::StalePlugin>) -> RestartOutcome {
    let outcome = restart_daemon(&key, &PLUGIN_REFRESH_TRIED, "ради плагинов из пакета");
    if outcome == RestartOutcome::Restarted {
        let kind = crate::we_env::package_engine().map_or(we_client::EngineKind::Bundled, |l| l.kind);
        rexon_core::utils::notify_live(
            &we_client::plugin_refresh_title(),
            &we_client::plugin_refresh_body_for(kind, &stale),
        );
    }
    outcome
}

/// The engine version divergence key for which a restart was already done.
static ENGINE_REFRESH_TRIED: Mutex<Option<String>> = Mutex::new(None);

/// Compare the live daemon (`live` is its `Version`) with the engine from the
/// package (built in or from the distribution package).
/// `Some((key, version on disk, whose engine))` — restart (the key is
/// already remembered).
fn engine_refresh_step(live: &str) -> Option<(String, String, we_client::EngineKind)> {
    use rexon_core::we_client::DaemonFs;
    let layout = crate::we_env::daemon_binary()
        .and_then(|b| we_client::layout_of(&we_client::engine_layouts(), &b).cloned());
    let runs_bundled = layout.is_some();
    let kind = layout.as_ref().map_or(we_client::EngineKind::Bundled, |l| l.kind);
    let on_disk = match &layout {
        Some(l) => crate::we_env::FsProbe.binary_version(&l.daemon).unwrap_or_default(),
        None => String::new(),
    };
    let tried = ENGINE_REFRESH_TRIED.lock().map(|g| g.clone()).unwrap_or(None);
    match we_client::engine_refresh_decision(runs_bundled, live, &on_disk, tried.as_deref()) {
        we_client::EngineRefresh::Fine => None,
        we_client::EngineRefresh::GiveUp => {
            rexon_core::log_warn!(
                "we",
                "демон после перезапуска всё равно рапортует {live}, а движок комплекта — {on_disk}; \
                 его, вероятно, запускает не Rexon"
            );
            None
        }
        we_client::EngineRefresh::Restart { key } => {
            if let Ok(mut g) = ENGINE_REFRESH_TRIED.lock() {
                *g = Some(key.clone());
            }
            Some((key, on_disk, kind))
        }
    }
}

/// Restart the daemon for the sake of a new engine from the package; if it
/// worked — a notification.
fn restart_for_engine(key: String, live: &str, on_disk: &str, kind: we_client::EngineKind) -> RestartOutcome {
    let outcome = restart_daemon(&key, &ENGINE_REFRESH_TRIED, "ради нового движка из пакета");
    if outcome == RestartOutcome::Restarted {
        rexon_core::utils::notify_live(
            &we_client::plugin_refresh_title(),
            &we_client::engine_refresh_body_for(kind, live, on_disk),
        );
    }
    outcome
}

/// Restart the daemon (`why` is for the log). A background thread:
/// `systemctl` and waiting for the daemon to leave do not hold the main
/// thread. Never: no `kill` — the service (`systemctl --user restart`) or
/// its own `Quit` over the bus. If the daemon is busy, the attempt key in
/// `tried` is removed so that the check is repeated.
fn restart_daemon(key: &str, tried: &Mutex<Option<String>>, why: &str) -> RestartOutcome {
    let secs = we_client::SYSTEMCTL_TIMEOUT_SECS as f64;
    // A download or a scan is in progress — do not break it; we will try on
    // the next connection (or with a repeat of the package watcher).
    let tasks = run(&we_client::busctl_tasks_argv(), we_client::BUSCTL_TIMEOUT_SECS as f64)
        .map(|out| we_client::parse_busctl_tasks(&out.stdout))
        .unwrap_or_default();
    if we_client::daemon_busy(&tasks) {
        if let Ok(mut g) = tried.lock() {
            *g = None;
        }
        rexon_core::log_info!("we", "демон занят задачами — перезапуск {why} отложен ({key})");
        return RestartOutcome::Busy;
    }
    let eu = rexon_core::engine_update::engine_unit_active_argv();
    let unit_active = run(&eu, secs).is_some_and(|out| out.rc == 0);
    let ok = if unit_active {
        let argv = we_client::systemctl_user_argv(&["restart", we_client::AUTOSTART_UNIT]);
        run(&argv, secs).is_some_and(|out| out.rc == 0)
    } else {
        let quit = rexon_core::engine_update::busctl_quit_argv();
        let asked = run(&quit, rexon_core::engine_update::BUSCTL_TIMEOUT_SECS as f64)
            .is_some_and(|out| out.rc == 0);
        let mut gone = false;
        if asked {
            for _ in 0..rexon_core::engine_update::QUIT_WAIT_STEPS {
                // The bus is asked with `busctl`, not with the transport's QtDBus:
                // that one lives in the main thread.
                let on_bus = run(
                    &rexon_core::engine_update::busctl_version_argv(),
                    rexon_core::engine_update::BUSCTL_TIMEOUT_SECS as f64,
                )
                .is_some_and(|out| {
                    !rexon_core::engine_update::parse_busctl_version(&out.stdout).is_empty()
                });
                if !on_bus {
                    gone = true;
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(
                    rexon_core::engine_update::QUIT_WAIT_STEP_MS,
                ));
            }
        }
        if gone {
            // The main thread brings it up (the launch and the bus polling live
            // there). The tick is posted at once rather than waiting for the tab's
            // reconnection: the tray without an open tab has no reconnection at all.
            PLUGIN_RELAUNCH.store(true, Ordering::SeqCst);
            qobject::sock_post_tick(TIMER_PLUGIN_RELAUNCH);
        }
        gone
    };
    if !ok {
        rexon_core::log_warn!("we", "не удалось перезапустить демон {why} ({key})");
        return RestartOutcome::Failed;
    }
    rexon_core::log_info!("we", "демон перезапущен {why} ({key})");
    RestartOutcome::Restarted
}

/// Bring up the daemon that left by `Quit` for the sake of the plugin, the
/// ordinary way (the service or a launch with `--plugin`). Once per
/// departure: the flag is cleared.
fn plugin_relaunch() {
    if PLUGIN_RELAUNCH.swap(false, Ordering::SeqCst) {
        qobject::sock_timer_stop(TIMER_RECONNECT);
        ensure_running();
    }
}

// ── engine package watcher (tray) ───────────────────────────────────────────

thread_local! {
    /// The watcher's one-off `plugin_list`: its own socket and its own event
    /// loop in its thread, like the synchronous wallpaper stop.
    static PLUGIN_QUERY: RefCell<PluginQuery> = RefCell::new(PluginQuery::default());
}

#[derive(Default)]
struct PluginQuery {
    request_id: u64,
    socket: *mut QWebSocket,
    answer: Option<Vec<we_client::PluginCopy>>,
}

/// The daemon's active plugins on port `port` — with a one-off socket,
/// synchronously, in the caller's thread (the tab's socket lives in the main
/// thread and must not be touched from here). `None` — did not connect, did
/// not answer or the answer is not the right one.
fn query_active_plugins(port: i64, timeout_ms: i32) -> Option<Vec<we_client::PluginCopy>> {
    if port <= 0 || !qobject::sock_qt_available() {
        return None;
    }
    let socket = qobject::sock_new();
    if socket.is_null() {
        return None;
    }
    PLUGIN_QUERY.with(|q| *q.borrow_mut() = PluginQuery { socket, ..PluginQuery::default() });
    let mut sock = unsafe { Pin::new_unchecked(&mut *socket) };
    let on_connected = sock.as_mut().on_connected(|_sock| {
        let request_id = alloc_request_id() as u64;
        let socket = PLUGIN_QUERY.with(|q| {
            let mut q = q.borrow_mut();
            q.request_id = request_id;
            q.socket
        });
        if !stop_send(socket, &we_client::plugin_list(request_id)) {
            qobject::sock_quit_loop();
        }
    });
    let on_message = sock.as_mut().on_binary_message_received(|_sock, message| {
        let bytes: Vec<u8> = message.into();
        let Ok(Frame::Response(request_id)) = read_frame(&bytes) else { return };
        if request_id == 0 || request_id != PLUGIN_QUERY.with(|q| q.borrow().request_id) {
            return;
        }
        PLUGIN_QUERY.with(|q| q.borrow_mut().answer = we_client::active_plugins_from_frame(&bytes));
        qobject::sock_quit_loop();
    });
    let on_disconnected = sock.as_mut().on_disconnected(|_sock| qobject::sock_quit_loop());
    unsafe { qobject::sock_quit_loop_on_error(socket) };
    sock.as_mut().open_url(&QUrl::from(format!("ws://127.0.0.1:{port}/").as_str()));
    qobject::sock_run_loop(timeout_ms);
    // The guards are removed BEFORE the socket, otherwise they would poke a
    // dead object.
    drop(on_connected);
    drop(on_message);
    drop(on_disconnected);
    unsafe {
        qobject::sock_shutdown(socket);
        qobject::sock_destroy(socket.cast());
    }
    PLUGIN_QUERY.with(|q| std::mem::take(&mut *q.borrow_mut()).answer)
}

/// Folder names in the package's `plugins/` — to watch their `plugin.toml`.
fn bundled_plugin_dirs(layout: &we_client::EngineLayout) -> Vec<String> {
    std::fs::read_dir(layout.plugins_dir())
        .map(|it| {
            it.flatten()
                .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default()
}

/// One watcher check after the package changed. `true` — the daemon is busy,
/// the check should be repeated later.
fn package_check() -> bool {
    let st = probe();
    if !st.running || st.ws_port == 0 {
        // There is no daemon — nothing to check: when launching, Rexon will pass it
        // `--plugin` itself, and someone else's launch will be caught by the tab's
        // check on connection.
        rexon_core::log_info!("we", "сверка с пакетом движка: демон не запущен — сверять не с чем");
        return false;
    }
    // The engine itself first: a restart for its sake will bring up the daemon
    // with the bundle's plugins too, and a separate plugin check is then not
    // needed.
    if let Some((key, on_disk, kind)) = engine_refresh_step(&st.version) {
        return restart_for_engine(key, &st.version, &on_disk, kind) == RestartOutcome::Busy;
    }
    let Some(active) = query_active_plugins(st.ws_port, we_client::ENGINE_WATCH_QUERY_MS) else {
        rexon_core::log_warn!("we", "сверка с пакетом движка: демон не ответил на plugin_list");
        return false;
    };
    match plugin_refresh_step(&active) {
        Some((key, stale)) => restart_for_plugins(key, stale) == RestartOutcome::Busy,
        None => false,
    }
}

/// The package watcher: lives as long as the tray does. Waits for writes into
/// the engine directory, the end of the batch, and checks; the watch is reset
/// after every batch (new plugin folders may have appeared or directories
/// may have been replaced entirely).
fn package_watch_loop() {
    // The event loop in this thread is set up BEFORE the first socket: a
    // QWebSocket opened in a thread without an event dispatcher will not create
    // its notifiers. A zero run of the loop creates a dispatcher for the whole
    // thread.
    qobject::sock_run_loop(0);
    // We watch ALL package layouts, not only the one installed now: the engine
    // may have come as a distribution package after the tray was started.
    let targets = || {
        we_client::engine_layouts()
            .iter()
            .flat_map(|l| we_client::engine_watch_targets(l, &bundled_plugin_dirs(l)))
            .collect::<Vec<_>>()
    };
    let Some(mut watch) = rexon_core::fs_watch::Watch::new(targets()) else {
        rexon_core::log_warn!("we", "сторож пакета движка не запущен: inotify недоступен");
        return;
    };
    // The first check is right at start-up, without a write into the package:
    // Rexon may have been restarted AFTER the update (with the «Restart Rexon»
    // button), while the daemon service still holds the previous engine.
    let mut first = true;
    loop {
        if !std::mem::take(&mut first) {
            watch.wait_burst(we_client::ENGINE_WATCH_QUIET_MS, we_client::ENGINE_WATCH_MAX_MS);
            // The watch is reset BEFORE the check: a write in the middle of the
            // check will not be lost.
            if let Some(fresh) = rexon_core::fs_watch::Watch::new(targets()) {
                watch = fresh;
            }
        }
        rexon_core::log_info!("we", "сверяю движок и плагины живого демона с пакетом");
        // The autostart service is the same engine being checked: a unit
        // pointing to the previous copy would bring up the old daemon at login
        // after the package update (and so would the service restart below).
        crate::autostart_queries::refresh_autostart_unit();
        let mut retries = 0;
        while package_check() && retries < we_client::ENGINE_WATCH_BUSY_RETRIES {
            retries += 1;
            // A new batch of writes while waiting — check after it.
            if watch.wait(we_client::ENGINE_WATCH_BUSY_RETRY_MS) {
                watch.settle(we_client::ENGINE_WATCH_QUIET_MS, we_client::ENGINE_WATCH_MAX_MS);
            }
        }
    }
}

/// Set up the engine package watcher once per process. Only for Rexon from a
/// package (the engine has `plugins/`): in the developer tree there is
/// nothing to compare with. Call from the main thread — the transport's tick
/// hooks are set here.
pub fn spawn_package_watch() {
    static STARTED: std::sync::Once = std::sync::Once::new();
    if crate::we_env::package_engine().is_none() {
        return;
    }
    install_shim_hooks();
    STARTED.call_once(|| {
        let spawned = std::thread::Builder::new()
            .name("engine-package-watch".into())
            .spawn(package_watch_loop);
        if let Err(e) = spawned {
            rexon_core::log_warn!("we", "сторож пакета движка не запущен: {e}");
        }
    });
}

fn health_timed_out() {
    if HEALTH_ID.with(|c| c.replace(0)) == 0 {
        return;
    }
    broadcast(&DaemonEvent::HealthTimeout);
}

/// The `state` field from the `health` answer. Parsing the frame is the
/// core's (`reply_fields`).
///
/// An empty string means both "an old daemon does not send the field" and
/// "the answer was not about health" — the core's verdict is the same in
/// both cases.
fn health_state(frame: &[u8]) -> String {
    match we_client::reply_fields(frame) {
        Ok(f) if f.payload == "health" => f
            .text
            .into_iter()
            .find(|(key, _)| key == "state")
            .map(|(_, value)| value)
            .unwrap_or_default(),
        _ => String::new(),
    }
}

// ── preparing the wallpaper display (KDE) ───────────────────────────────────
//
// A port of `DaemonControl._ensure_display_backend` and
// `rexon/waywallen_kde_plugin.ensure_waywallen_plugin`. All decisions are in
// the core (`wallpaper_backend::pick`, `waywallen_kde_plugin::*`: the
// kpackagetool6 pipeline, the plugin selection JS, the plasmashell restart
// commands, the final message); here there are only processes and files.
//
// ⭐ The difference from Python: the preparation runs in a BACKGROUND
// thread. In Python it ran right in the main one, and the first showing of
// the tab on KDE could stand for up to 30 s on kpackagetool6 and another
// 15 s on plasmashell. The order with the daemon does not depend on this: a
// daemon with `spawn=External` itself waits until the plugin is selected.

/// The preparation is already running — a second round (the «Retry» button
/// at that time) would add nothing except two kpackagetool6 runs over one
/// directory.
static KDE_PREPARE_BUSY: AtomicBool = AtomicBool::new(false);

/// What the tab shows about the KDE wallpaper plugin. The decision is the
/// core's (`waywallen_kde_plugin::plan_plugin`), here there is only delivery.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct KdePluginState {
    /// A message for the user; `None` — there is nothing to say.
    pub notice: Option<rexon_core::waywallen_kde_plugin::PluginNotice>,
    /// An installation by consent is in progress (the «Install Rexon's version»
    /// button).
    pub installing: bool,
    /// An installation by consent failed — the raw kpackagetool6 reason.
    pub failure: Option<String>,
}

type KdeSink = std::sync::Arc<dyn Fn(&KdePluginState) + Send + Sync>;

/// The last state — for a page opened after the check.
static KDE_STATE: Mutex<Option<KdePluginState>> = Mutex::new(None);
/// State subscribers. Unlike daemon events, they are called from the
/// BACKGROUND preparation thread — the subscriber queues the call into its
/// object itself.
static KDE_SINKS: Mutex<Vec<(u64, KdeSink)>> = Mutex::new(Vec::new());
static KDE_NEXT_SINK: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// The daemon version from the last check — needed by the installation by
/// consent.
static KDE_DAEMON_VERSION: Mutex<Option<String>> = Mutex::new(None);

/// Subscribe to the KDE plugin state. An already known state arrives at
/// once, in the caller's thread.
pub fn subscribe_kde_plugin(sink: impl Fn(&KdePluginState) + Send + Sync + 'static) -> u64 {
    let id = KDE_NEXT_SINK.fetch_add(1, Ordering::SeqCst) + 1;
    let sink: KdeSink = std::sync::Arc::new(sink);
    if let Ok(mut sinks) = KDE_SINKS.lock() {
        sinks.push((id, sink.clone()));
    }
    let current = KDE_STATE.lock().ok().and_then(|s| s.clone());
    if let Some(state) = current {
        sink(&state);
    }
    id
}

/// Remove a subscription; `0` means there was no subscription.
pub fn unsubscribe_kde_plugin(id: u64) {
    if id == 0 {
        return;
    }
    if let Ok(mut sinks) = KDE_SINKS.lock() {
        sinks.retain(|(entry, _)| *entry != id);
    }
}

fn publish_kde_plugin(state: KdePluginState) {
    if let Ok(mut slot) = KDE_STATE.lock() {
        *slot = Some(state.clone());
    }
    let sinks: Vec<KdeSink> =
        KDE_SINKS.lock().map(|s| s.iter().map(|(_, f)| f.clone()).collect()).unwrap_or_default();
    for sink in sinks {
        sink(&state);
    }
}

use rexon_core::waywallen_kde_plugin::EnsurePurpose as KdePurpose;

/// Preparing the plugin is done by one thread at a time: a wallpaper apply
/// that arrived in the middle of the installation on tab opening waits for
/// its end rather than being skipped.
static KDE_ENSURE_LOCK: Mutex<()> = Mutex::new(());

/// Who shows live wallpapers in this session (the core's decision).
pub fn session_backend() -> rexon_core::wallpaper_backend::WallpaperBackend {
    let env = |name: &str| std::env::var(name).unwrap_or_default();
    rexon_core::wallpaper_backend::pick(
        &env("XDG_CURRENT_DESKTOP"),
        &env("XDG_SESSION_TYPE"),
        &env("WAYLAND_DISPLAY"),
        &env("WAYFIRE_SOCKET"),
    )
}

/// In this session waywallen's wallpapers are shown by the Plasma plugin.
pub fn kde_plugin_backend() -> bool {
    session_backend().needs_rexon_setup()
}

/// How the daemon gets a display in this session (the core's decision).
pub fn display_setup() -> rexon_core::wallpaper_backend::DisplaySetup {
    rexon_core::wallpaper_backend::display_setup_in(
        session_backend(),
        &std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default(),
        rexon_core::wallpaper_backend::waywallen_gnome_ext_installed_live(),
        rexon_core::wallpaper_backend::cinnamon_layer_shell_fact(),
    )
}

/// GNOME: enable waywallen's output extension (Rexon disables it when setting
/// its own wallpaper — otherwise its black placeholder covers the desktop).
/// The shell bus — in a separate thread; further holding is run by the same
/// core (`release_held`).
fn start_gnome_display_extension() {
    let spawned = std::thread::Builder::new().name("rexon-gnome-display".into()).spawn(|| {
        use rexon_core::wallpaper_backend::{set_waywallen_gnome_display_live, GnomeDisplaySwitch as R};
        let ok = matches!(set_waywallen_gnome_display_live(true), R::Switched | R::Already);
        kde_select_finished_without_plugin(ok);
    });
    if let Err(e) = spawned {
        rexon_core::log_warn!("gnome-display", "включение расширения вывода не запущено: {e}");
        kde_select_finished_without_plugin(false);
    }
}

// ── X11: the Rexon wallpaper host as the daemon's output client ────────────
//
// Decisions and parsing are `rexon_core::wallpaper_backend` (the "display
// for the daemon" section). Here is execution: an output definition for
// future daemon starts and launching the host for the live daemon now.
// Restarting the daemon for the sake of the definition is not allowed: a
// socket disconnection would throw away the held-back frame, and «Apply»
// would be lost.

/// The wallpaper host: next to the Rexon window (the developer tree),
/// otherwise by `PATH`.
pub(crate) fn display_host_binary() -> Option<String> {
    use rexon_core::we_client::DaemonFs;
    let mut fs = crate::we_env::FsProbe;
    std::env::current_exe()
        .ok()
        .and_then(|exe| rexon_core::wallpaper_backend::rexon_display_host_sibling(&exe.to_string_lossy()))
        .filter(|cand| fs.is_exec_file(cand))
        .or_else(|| fs.which(rexon_core::wallpaper_engine::WALLPAPER_HOST_BINARY))
}

/// `$XDG_DATA_HOME` or `~/.local/share` — where the daemon looks for output
/// definitions.
fn xdg_data_home() -> String {
    match std::env::var("XDG_DATA_HOME") {
        Ok(v) if !v.is_empty() => v,
        _ => format!("{}/.local/share", crate::we_env::home()),
    }
}

/// Put an output definition for the daemon if there is none or it is stale.
/// `Err` is a text for the log.
fn ensure_display_manifest(host: &str) -> Result<bool, String> {
    use rexon_core::wallpaper_backend as wb;
    let path = wb::rexon_display_manifest_path(&xdg_data_home());
    let current = std::fs::read_to_string(&path).ok();
    if !wb::rexon_display_manifest_stale(current.as_deref(), host) {
        return Ok(false);
    }
    let dir = std::path::Path::new(&path).parent().ok_or("у пути определения нет каталога")?;
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let tmp = format!("{path}.tmp");
    std::fs::write(&tmp, wb::rexon_display_manifest(host)).map_err(|e| format!("{tmp}: {e}"))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("{path}: {e}"))?;
    Ok(true)
}

/// Whether another host instance already holds the output (its lock is
/// taken).
fn display_host_running() -> bool {
    let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_default();
    if runtime.is_empty() {
        return false;
    }
    let path = rexon_core::wallpaper_backend::rexon_host_lock_path(&runtime);
    let Ok(file) = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(&path) else {
        return false;
    };
    match file.try_lock() {
        Ok(()) => {
            let _ = file.unlock();
            false
        }
        Err(_) => true,
    }
}

/// Bring up the output on X11: a definition for the daemon and the host now.
/// The outcome is in the holding state (`kde_select_finished`): the display
/// will appear by itself when the host passes the handshake; if it failed,
/// the frame goes out at once, and the daemon honestly answers "no display".
fn start_rexon_display_host() {
    let spawned = std::thread::Builder::new().name("rexon-x11-display".into()).spawn(|| {
        let ok = match display_host_binary() {
            None => {
                rexon_core::log_warn!(
                    "x11-display",
                    "хоста обоев {} нет ни рядом с Rexon, ни в PATH — выводить сцены нечем",
                    rexon_core::wallpaper_engine::WALLPAPER_HOST_BINARY
                );
                false
            }
            Some(host) => {
                ensure_x11_output_element();
                match ensure_display_manifest(&host) {
                    Ok(true) => rexon_core::log_info!("x11-display", "движку обоев записано определение вывода (хост {host}) — со следующего старта движок поднимает вывод сам"),
                    Ok(false) => {}
                    Err(e) => rexon_core::log_warn!("x11-display", "определение вывода для движка не записано: {e}"),
                }
                if display_host_running() {
                    rexon_core::log_info!("x11-display", "хост вывода уже работает — ждём его дисплей");
                    true
                } else {
                    spawn_display_host(&host)
                }
            }
        };
        kde_select_finished_without_plugin(ok);
    });
    if let Err(e) = spawned {
        rexon_core::log_warn!("x11-display", "подъём вывода не запущен: {e}");
        kde_select_finished_without_plugin(false);
    }
}

/// Launch the host in a separate session (it survives the Rexon window
/// closing; further it is watched by the lock, and from the next start by
/// the daemon itself).
fn spawn_display_host(host: &str) -> bool {
    use std::os::unix::process::CommandExt;
    let argv = rexon_core::wallpaper_backend::rexon_display_host_argv(host, "");
    let mut cmd = std::process::Command::new(&argv[0]);
    cmd.args(&argv[1..])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    // SAFETY: in the child process before exec — only setsid (async-signal-safe).
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    match cmd.spawn() {
        Ok(mut child) => {
            rexon_core::log_info!("x11-display", "хост вывода запущен: pid {} ({})", child.id(), argv.join(" "));
            // Pick up the exit code so as not to leave a zombie.
            std::thread::spawn(move || {
                if let Ok(status) = child.wait() {
                    rexon_core::log_info!("x11-display", "хост вывода завершился: {status}");
                }
            });
            true
        }
        Err(e) => {
            rexon_core::log_warn!("x11-display", "хост вывода не запустился ({}): {e}", argv.join(" "));
            false
        }
    }
}

// ── X11: desktop icons under the output window ─────────────────────────
//
// Decisions are `rexon_core::wallpaper_backend` (the "desktop icons under
// live wallpapers" section): enable Caja's see-through background where it
// exists, otherwise honestly say that the icons are hidden while live
// wallpapers are on. The window order is kept by the output host itself
// (`keepX11Order`). gsettings and recreating Caja's desktop window take
// seconds — in a separate thread; the message comes with a tick in the main
// one.

/// A message about the icons waiting to be shown by the main thread.
static DESKTOP_ICONS_NOTICE: Mutex<Option<String>> = Mutex::new(None);
/// The «icons are hidden» warning was already shown in this launch — do not
/// repeat it on every apply.
static DESKTOP_ICONS_WARNED: AtomicBool = AtomicBool::new(false);
/// The preparation is running — a second apply in a row does not start a
/// second thread.
static DESKTOP_ICONS_BUSY: AtomicBool = AtomicBool::new(false);

/// Who draws the icons in this session (the core's decision by the
/// environment).
fn session_desktop_icons_owner() -> Option<rexon_core::wallpaper_backend::DesktopIconsOwner> {
    let env = |name: &str| std::env::var(name).unwrap_or_default();
    let session = rexon_core::wallpaper_engine::detect_session_type(
        &env("XDG_SESSION_TYPE"),
        &env("WAYLAND_DISPLAY"),
        &env("DISPLAY"),
    );
    let de = rexon_core::wallpaper_engine::detect_de(&env("XDG_CURRENT_DESKTOP"), &session);
    rexon_core::wallpaper_backend::desktop_icons_owner(&de, &session)
}

fn start_desktop_icons_prepare() {
    let Some(owner) = session_desktop_icons_owner() else {
        return;
    };
    if DESKTOP_ICONS_BUSY.swap(true, Ordering::SeqCst) {
        return;
    }
    let spawned = std::thread::Builder::new().name("rexon-desktop-icons".into()).spawn(move || {
        let mut sys = rexon_core::wallpaper_exec::LiveSys::new();
        let mut io = rexon_core::wallpaper_exec::DesktopIconsSys::new(&mut sys);
        let notice = rexon_core::wallpaper_backend::prepare_desktop_icons(&mut io, owner);
        DESKTOP_ICONS_BUSY.store(false, Ordering::SeqCst);
        if let Some(text) = notice {
            if let Ok(mut slot) = DESKTOP_ICONS_NOTICE.lock() {
                *slot = Some(text);
            }
            qobject::sock_post_tick(TIMER_DESKTOP_ICONS);
        }
    });
    if let Err(e) = spawned {
        DESKTOP_ICONS_BUSY.store(false, Ordering::SeqCst);
        rexon_core::log_warn!("desktop-icons", "подготовка значков стола не запущена: {e}");
    }
}

/// Main thread: show the message about the icons as an informational toast.
fn desktop_icons_prepared() {
    let Some(text) = DESKTOP_ICONS_NOTICE.lock().ok().and_then(|mut slot| slot.take()) else {
        return;
    };
    let owner = session_desktop_icons_owner();
    let hidden = owner.is_some_and(|o| text == rexon_core::wallpaper_backend::desktop_icons_hidden_text(o));
    if hidden && DESKTOP_ICONS_WARNED.swap(true, Ordering::SeqCst) {
        return;
    }
    broadcast(&DaemonEvent::Notice(text));
}

/// A failed apply is rolled back — restore the desktop as it was too.
fn start_desktop_icons_restore() {
    let spawned = std::thread::Builder::new().name("rexon-desktop-icons".into()).spawn(|| {
        let mut sys = rexon_core::wallpaper_exec::LiveSys::new();
        rexon_core::wallpaper_backend::restore_desktop_icons(&mut rexon_core::wallpaper_exec::DesktopIconsSys::new(&mut sys));
    });
    if let Err(e) = spawned {
        rexon_core::log_warn!("desktop-icons", "возврат стола не запущен: {e}");
    }
}

fn prepare_display_backend(
    daemon_version: Option<String>,
    binary: Option<String>,
    consent: bool,
    purpose: KdePurpose,
) {
    if !kde_plugin_backend() {
        if purpose.selects() {
            kde_select_finished(false, None);
        }
        return;
    }
    // The apply is not discarded because of a running preparation: it will
    // queue up on the lock. Only a second round of preparation is discarded.
    let gated = !purpose.selects();
    if gated && KDE_PREPARE_BUSY.swap(true, Ordering::SeqCst) {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("rexon-kde-plugin".into())
        .spawn(move || {
            // There is no daemon yet — the version is told by the binary that will
            // be launched.
            let daemon_version = daemon_version.or_else(|| {
                use rexon_core::we_client::DaemonFs;
                binary.as_deref().and_then(|bin| crate::we_env::FsProbe.binary_version(bin))
            });
            if let Ok(mut slot) = KDE_DAEMON_VERSION.lock() {
                if daemon_version.is_some() {
                    slot.clone_from(&daemon_version);
                }
            }
            let (ok, selected, previous, message) = {
                let _guard = KDE_ENSURE_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                ensure_kde_plugin(&KdeEnv::live(), daemon_version, consent, purpose)
            };
            if purpose.selects() {
                kde_select_finished(selected, previous);
            }
            if ok {
                rexon_core::log_info!("kde-plugin", "плагин обоев waywallen для KDE: {message}");
            } else {
                rexon_core::log_warn!("kde-plugin", "плагин обоев waywallen для KDE (не удалось): {message}");
            }
            if consent {
                finish_consent_install();
            }
            if gated {
                KDE_PREPARE_BUSY.store(false, Ordering::SeqCst);
            }
        });
    if let Err(e) = spawned {
        rexon_core::log_warn!("kde-plugin", "подготовка показа обоев не запущена: {e}");
        if gated {
            KDE_PREPARE_BUSY.store(false, Ordering::SeqCst);
        }
        if consent {
            finish_consent_install();
        }
        if purpose.selects() {
            kde_select_finished(false, None);
        }
    }
}

/// The installation by consent ended in any way — remove "installing" so
/// that the button does not stay disabled forever.
fn finish_consent_install() {
    let current = KDE_STATE.lock().ok().and_then(|s| s.clone());
    if let Some(state) = current.filter(|s| s.installing) {
        publish_kde_plugin(KdePluginState { installing: false, ..state });
    }
}

/// The «Install Rexon's version» button: the user agreed to replace the
/// plugin.
///
/// `false` — the preparation is already running (a second round would add
/// nothing).
pub fn install_kde_plugin_with_consent() -> bool {
    if KDE_PREPARE_BUSY.load(Ordering::SeqCst) {
        return false;
    }
    let daemon_version = KDE_DAEMON_VERSION.lock().ok().and_then(|v| v.clone());
    let previous = KDE_STATE.lock().ok().and_then(|s| s.clone()).unwrap_or_default();
    publish_kde_plugin(KdePluginState { installing: true, failure: None, ..previous });
    prepare_display_backend(daemon_version, find_daemon_binary(), true, KdePurpose::Prepare);
    // The preparation may not have started (not KDE, already running) — then
    // remove "installing".
    if !KDE_PREPARE_BUSY.load(Ordering::SeqCst) {
        finish_consent_install();
    }
    true
}

/// The plugin preparation environment. The live one comes from the process;
/// the reconciliation tests supply their own (fake
/// `kpackagetool6`/`dbus-send`/`systemctl` in PATH).
#[derive(Debug, Clone)]
struct KdeEnv {
    /// `XDG_CURRENT_DESKTOP`.
    desktop: String,
    home: String,
    /// The `rexon/` package directory (with `kpackage/` in it).
    pkg_dir: String,
    /// `PATH` for finding the tools.
    path: Option<String>,
    /// `XDG_DATA_DIRS`: where the packaged plugin is looked for.
    data_dirs: String,
    /// Installs a distribution package and reports the build stage for the
    /// person while it runs. The live one builds through the shared AUR queue;
    /// the stand tests substitute their own.
    install_package: PackageInstaller,
}

/// `(package, on_stage) → Ok | the reason for the person`.
type PackageInstaller = fn(&str, &mut dyn FnMut(&str)) -> Result<(), String>;

impl KdeEnv {
    fn live() -> KdeEnv {
        KdeEnv {
            desktop: std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default(),
            home: crate::we_env::home(),
            pkg_dir: crate::we_env::pkg_dir(),
            path: std::env::var("PATH").ok(),
            data_dirs: std::env::var("XDG_DATA_DIRS").unwrap_or_default(),
            install_package: install_package_live,
        }
    }
}

/// Install a distribution package the way Rexon installs every missing program:
/// at once, without a Yes/No question (the password window is the protection),
/// through the ONE AUR queue of the application (`appearance_actions`), so that
/// a theme build and this one never run side by side.
fn install_package_live(package: &str, on_stage: &mut dyn FnMut(&str)) -> Result<(), String> {
    use rexon_core::system::PkgFamily;
    let family = rexon_core::system::DistroDetector::detect_live().family_live();
    if family != PkgFamily::Arch {
        return Err(rexon_core::waywallen_kde_plugin::package_unsupported_reason());
    }
    crate::appearance_actions::aur_install_wait_with(package, on_stage)
}

/// Installing the plugin package failed in this process: it is not retried by
/// itself on every tab opening (each attempt is a password window and a long
/// build), only by the banner button. The reason is shown in the banner. Keyed
/// by the home directory: the live process has one, the stand tests run side by
/// side with their own.
static KDE_PACKAGE_FAILURE: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());

fn package_failure(env: &KdeEnv) -> Option<String> {
    KDE_PACKAGE_FAILURE.lock().ok()?.iter().find(|(home, _)| *home == env.home).map(|(_, r)| r.clone())
}

fn set_package_failure(env: &KdeEnv, reason: Option<String>) {
    if let Ok(mut list) = KDE_PACKAGE_FAILURE.lock() {
        list.retain(|(home, _)| *home != env.home);
        if let Some(reason) = reason {
            list.push((env.home.clone(), reason));
        }
    }
}

/// The packaged plugin: the first `$XDG_DATA_DIRS/plasma/wallpapers/org.waywallen.kde`
/// with `metadata.json`, and its owner by `pacman -Qo` (`LANG=C`). Other package
/// managers are not asked: the version is then unknown, and the version watcher
/// does not guess.
fn read_system_plugin(env: &KdeEnv) -> rexon_core::waywallen_kde_plugin::SystemPlugin {
    use rexon_core::waywallen_kde_plugin as wkp;
    let Some(dir) = wkp::system_install_dirs(&env.data_dirs, &env.home)
        .into_iter()
        .find(|d| std::path::Path::new(&wkp::metadata_json_path(d)).is_file())
    else {
        return wkp::SystemPlugin::default();
    };
    let owner = rexon_core::utils::resolve_tool("pacman", &[], env.path.as_deref()).and_then(|pacman| {
        let report = rexon_core::utils::run(
            &rexon_core::utils::RunSpec {
                argv: wkp::pacman_owner_argv(&pacman, &wkp::metadata_json_path(&dir)),
                output: rexon_core::utils::RunOutput::Capture,
                timeout_secs: Some(10),
                lang_c: true,
                env: Vec::new(),
                stderr_to_stdout: false,
            },
            env.path.as_deref(),
        );
        wkp::parse_pacman_owner(&String::from_utf8_lossy(&report.stdout))
    });
    let (package, version) = owner.map_or((None, None), |(p, v)| (Some(p), Some(v)));
    wkp::SystemPlugin { present: true, dir: Some(dir), version, package }
}

/// Move the copy in `~/.local` that hides the packaged plugin to a backup
/// (`waywallen_kde_plugin::user_copy_backup_dir`) — never deleted silently. The
/// "we installed it" marker lies inside the copy and moves with it. `Ok` — where
/// the copy went.
fn move_user_copy_away(env: &KdeEnv) -> Result<String, String> {
    use rexon_core::waywallen_kde_plugin as wkp;
    let install = wkp::install_dir(&env.home);
    let stamp = rexon_core::qt_app::Timestamp::now_local().map(|t| t.file_stamp()).unwrap_or_else(|| "0".to_owned());
    let dest = wkp::user_copy_backup_dir(&env.home, &stamp);
    if let Some(parent) = std::path::Path::new(&dest).parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    std::fs::rename(&install, &dest).map_err(|e| format!("{install} -> {dest}: {e}"))?;
    Ok(dest)
}

/// Install a package for live wallpapers with the banner of the tab showing the
/// build: "installing", the stage, and at the end either nothing (the caller
/// publishes what follows) or the reason it failed.
fn install_package_with_banner(install: PackageInstaller, package: &str, why: &str) -> Result<(), String> {
    use rexon_core::waywallen_kde_plugin::PluginNotice;
    let publish_stage = |stage: &str| {
        publish_kde_plugin(KdePluginState {
            notice: Some(PluginNotice::PackageInstalling { package: package.to_owned(), stage: stage.to_owned() }),
            installing: true,
            failure: None,
        });
    };
    publish_stage("");
    rexon_core::log_info!("kde-plugin", "installing the {package} package ({why})");
    let started = std::time::Instant::now();
    let result = install(package, &mut |stage| {
        rexon_core::log_info!("kde-plugin", "{package}: {stage}");
        publish_stage(stage);
    });
    let secs = started.elapsed().as_secs();
    match &result {
        Ok(()) => rexon_core::log_info!("kde-plugin", "{package} installed in {secs} s"),
        Err(reason) => {
            rexon_core::log_warn!("kde-plugin", "{package} not installed after {secs} s: {reason}");
            publish_kde_plugin(KdePluginState {
                notice: Some(PluginNotice::PackageMissing { package: package.to_owned(), reason: reason.clone() }),
                installing: false,
                failure: None,
            });
        }
    }
    result
}

/// Install the Plasma plugin package; a failure is remembered — no automatic
/// retry in this process.
fn install_plugin_package(env: &KdeEnv) -> Result<(), String> {
    use rexon_core::waywallen_kde_plugin::PACKAGE_NAME;
    let result = install_package_with_banner(env.install_package, PACKAGE_NAME, "no plugin and no bundled copy");
    set_package_failure(env, result.as_ref().err().cloned());
    result
}

/// Installing `waywallen-display` for the X11 output already failed in this
/// process — not retried on every apply.
static X11_MODULE_FAILED: AtomicBool = AtomicBool::new(false);

/// X11 (XFCE, MATE, Cinnamon on X11): the output host needs the output element —
/// the `Waywallen.Display` module of the `waywallen-display` package, or a
/// plasmoid. A distribution build of Rexon carries no plasmoid, so without the
/// package the output would silently disappear: Rexon installs it itself, like
/// any missing program (once per process; the reason stays in the banner).
fn ensure_x11_output_element() {
    use rexon_core::waywallen_kde_plugin as wkp;
    let env = |name: &str| std::env::var(name).unwrap_or_default();
    let is_file = |p: &str| std::path::Path::new(p).is_file();
    let available = || {
        let plasmoids = wkp::plasmoid_display_wrappers(Some(&crate::we_env::pkg_dir()), &crate::we_env::home());
        let dirs = wkp::qml_import_dirs(&env("QML_IMPORT_PATH"), &env("QML2_IMPORT_PATH"));
        // Only "is there an element at all": the host itself picks where the wrapper goes.
        wkp::pick_display_item(&dirs, Some("-"), &plasmoids, &is_file).is_some()
    };
    if available() || X11_MODULE_FAILED.load(Ordering::SeqCst) {
        return;
    }
    let result = install_package_with_banner(
        install_package_live,
        wkp::DISPLAY_MODULE_PACKAGE,
        "no output element for the X11 output host",
    );
    if result.is_err() {
        X11_MODULE_FAILED.store(true, Ordering::SeqCst);
    } else {
        publish_kde_plugin(KdePluginState::default());
    }
}

/// The plugin state on disk — everything the version watcher needs.
fn read_plugin_check(
    bundle: &str,
    install: &str,
    marker: &str,
    daemon_version: Option<String>,
    consent: bool,
) -> rexon_core::waywallen_kde_plugin::PluginCheck {
    use rexon_core::waywallen_kde_plugin as wkp;
    use std::path::Path;

    let sha = |p: &str| std::fs::read(p).ok().map(|bytes| wkp::file_sha(&bytes));
    let vendor_version =
        |base: &str| std::fs::read_to_string(wkp::vendor_toml_path(base)).ok().and_then(|t| wkp::parse_vendor_version(&t));
    let bundle_so = wkp::find_runtime_so(bundle, None);
    let bundle_rel = bundle_so.as_deref().and_then(|so| {
        Path::new(so).strip_prefix(bundle).ok().and_then(|r| r.to_str().map(str::to_owned))
    });
    let present = Path::new(&wkp::metadata_json_path(install)).is_file();
    let installed = if present {
        wkp::InstalledPlugin {
            present,
            vendor_version: vendor_version(install),
            marker: std::fs::read_to_string(marker).is_ok_and(|t| !t.trim().is_empty()),
            runtime_sha: wkp::find_runtime_so(install, bundle_rel.as_deref()).and_then(|so| sha(&so)),
        }
    } else {
        wkp::InstalledPlugin::default()
    };
    wkp::PluginCheck {
        bundle_version: vendor_version(bundle),
        bundle_sha: bundle_so.as_deref().and_then(sha),
        installed,
        daemon_version,
        consent,
        ..Default::default()
    }
}

/// A port of `ensure_waywallen_plugin` with the version watcher
/// (`waywallen_kde_plugin::plan_plugin`).
///
/// Never: the plugin is assigned as the desktops' wallpaper ONLY with
/// `purpose == Apply` (the user applied a wallpaper). Previously this was
/// done on every tab opening — with a plasmashell restart — and for a person
/// with an empty library the desktop went black (defect of 30.10.2026).
///
/// Returns (success, the plugin was just assigned as the wallpaper, a
/// message for the log).
fn ensure_kde_plugin(
    env: &KdeEnv,
    daemon_version: Option<String>,
    consent: bool,
    purpose: KdePurpose,
) -> (bool, bool, Option<Vec<rexon_core::waywallen_kde_plugin::DesktopPlugin>>, String) {
    use rexon_core::waywallen_kde_plugin as wkp;

    if !wkp::is_kde(&env.desktop) {
        return (true, false, None, "не KDE — пропускаем (no-op)".to_owned());
    }
    let home = env.home.clone();
    let bundle = wkp::bundle_dir(&env.pkg_dir);
    let install = wkp::install_dir(&home);
    let marker = wkp::version_marker(&home);
    let path_env = env.path.clone();
    let tool = rexon_core::utils::resolve_tool("kpackagetool6", &[], path_env.as_deref());

    let read_check = || {
        let mut check = read_plugin_check(&bundle, &install, &marker, daemon_version.clone(), consent);
        check.system = read_system_plugin(env);
        check.bundle_missing = !bundle_is_valid(&bundle);
        check.package_failed = package_failure(env).is_some();
        check
    };
    let mut check = read_check();
    let mut plan = wkp::plan_plugin(&check);
    if plan.action == wkp::PluginAction::InstallPackage {
        if let Err(reason) = install_plugin_package(env) {
            return (false, false, None, format!("{reason}; {}", plan.reason));
        }
        // The package brought the plugin: decide anew (usually "use the packaged
        // one", or "move our old copy away").
        check = read_check();
        plan = wkp::plan_plugin(&check);
    }
    if plan.action == wkp::PluginAction::NeedPackage {
        // The banner names the reason of the last failure.
        let reason = package_failure(env).unwrap_or_default();
        plan.notice = Some(wkp::PluginNotice::PackageMissing { package: wkp::PACKAGE_NAME.to_owned(), reason });
    }
    let summary = format!(
        "{} (бандл {}, установлен {}, демон {})",
        plan.reason,
        check.bundle_version.as_deref().unwrap_or("?"),
        plan.installed_version.as_deref().unwrap_or(if check.installed.present { "?" } else { "—" }),
        check.daemon_version.as_deref().unwrap_or("?"),
    );
    let summary = match (&check.system.dir, plan.action) {
        (Some(dir), _) => format!(
            "{summary}; packaged plugin {} ({}) in {dir}",
            check.system.version.as_deref().unwrap_or("?"),
            check.system.package.as_deref().unwrap_or("owner unknown"),
        ),
        _ => summary,
    };

    let outcome = if plan.action == wkp::PluginAction::RemoveUserCopy {
        match move_user_copy_away(env) {
            Ok(dest) => {
                publish_kde_plugin(KdePluginState { notice: plan.notice.clone(), installing: false, failure: None });
                // The plugin under the same Id changed (copy → package): a shell that
                // has the copy's `.so` in memory is restarted by the same rule as after
                // an update.
                wkp::InstallOutcome {
                    calls: Vec::new(),
                    ok: true,
                    message: format!("copy in ~/.local moved to {dest}"),
                    installed_now: true,
                    marker_to_write: None,
                }
            }
            Err(reason) => {
                if consent {
                    let previous = KDE_STATE.lock().ok().and_then(|s| s.clone()).unwrap_or_default();
                    publish_kde_plugin(KdePluginState { installing: false, failure: Some(reason.clone()), ..previous });
                }
                return (false, false, None, format!("could not move the copy away: {reason}; {summary}"));
            }
        }
    } else if plan.action.installs() {
        let bundle_valid = bundle_is_valid(&bundle);
        match run_kpackagetool(tool.as_deref(), &bundle, bundle_valid, &check, &marker) {
            Ok(outcome) => {
                publish_kde_plugin(KdePluginState::default());
                outcome
            }
            Err(reason) => {
                if consent {
                    let previous = KDE_STATE.lock().ok().and_then(|s| s.clone()).unwrap_or_default();
                    publish_kde_plugin(KdePluginState {
                        installing: false,
                        failure: Some(reason.clone()),
                        ..previous
                    });
                }
                return (false, false, None, format!("{reason}; {summary}"));
            }
        }
    } else {
        publish_kde_plugin(KdePluginState { notice: plan.notice.clone(), installing: false, failure: None });
        wkp::InstallOutcome {
            calls: Vec::new(),
            ok: true,
            message: summary.clone(),
            installed_now: false,
            marker_to_write: None,
        }
    };
    let changed = plan.action.installs() || plan.action == wkp::PluginAction::RemoveUserCopy;
    let base = if changed { format!("{}; {summary}", outcome.message) } else { summary };

    // What is set as the desktops' wallpaper now. `None` — Plasma did not
    // answer.
    let current = || {
        plasma_evaluate(env, &wkp::current_plugins_js(), 10.0).map(|out| wkp::parse_current_plugins(&out))
    };

    // A restart — only if we UPDATED a plugin that is already set as the
    // wallpaper: the old `.so` is in the shell's memory. The shell picks up a
    // fresh installation by itself when the plugin is assigned.
    // We ask Plasma only when the answer decides something.
    let in_use = outcome.installed_now
        && check.installed.present
        && current().is_some_and(|p| wkp::any_selected(&p));
    let restarted = wkp::restart_after_install(outcome.installed_now, check.installed.present, in_use)
        && restart_plasmashell(env);

    if !purpose.selects() {
        return (
            true,
            false,
            None,
            format!("{}; на десктопах не выбираем до применения обоев", wkp::ensure_message(&base, false, restarted)),
        );
    }
    if !plan.select {
        return (true, false, None, format!("{}; на десктопах не выбираем", outcome.message));
    }

    // The third value is "just assigned": only then is it worth waiting for
    // plasmashell to load the plugin and for it to register a display. If the
    // plugin was already everywhere and there is no display, waiting gives
    // nothing.
    let mut previous = None;
    let (selected, selected_now) = match current().map(|p| wkp::all_selected(&p)) {
        // Asking failed — and there is nothing to assign with: the answer and
        // the assignment go through one channel. We do not silently report
        // "success".
        None => {
            rexon_core::log_warn!("kde-plugin", "плагин обоев не назначен: Plasma недоступна");
            (false, false)
        }
        Some(all) if wkp::should_reselect(outcome.installed_now, all) => {
            // A snapshot of "what was there before us" — to restore it if the live
            // wallpaper does not start (`waywallen_kde_plugin::revert_plugins_js`).
            let before = plasma_evaluate(env, &wkp::plugins_by_desktop_js(), 10.0)
                .map(|out| wkp::parse_plugins_by_desktop(&out));
            let ok = plasma_evaluate(env, &wkp::select_plugin_js(), 15.0).is_some();
            if ok {
                previous = before;
            }
            (ok, ok)
        }
        Some(_) => (true, false),
    };
    (true, selected_now, previous, wkp::ensure_message(&base, selected, restarted))
}

/// The bundle is fit for kpackagetool6: a directory with metadata.json.
fn bundle_is_valid(bundle: &str) -> bool {
    use std::path::Path;
    Path::new(bundle).is_dir()
        && Path::new(&rexon_core::waywallen_kde_plugin::metadata_json_path(bundle)).is_file()
}

/// Install/update the plugin from the bundle with the core's pipeline
/// (`install_or_upgrade`) and write the "we installed it" marker. Shared by
/// applying a wallpaper and the quiet refresh. `Err` is the reason for the
/// log (raw, from kpackagetool6).
fn run_kpackagetool(
    tool: Option<&str>,
    bundle: &str,
    bundle_valid: bool,
    check: &rexon_core::waywallen_kde_plugin::PluginCheck,
    marker: &str,
) -> Result<rexon_core::waywallen_kde_plugin::InstallOutcome, String> {
    use rexon_core::waywallen_kde_plugin as wkp;
    // The "we installed it" marker: the bundle version, without it — the
    // runtime hash.
    let marker_value = check.bundle_version.clone().or_else(|| check.bundle_sha.clone());
    let mut launch_error: Option<String> = None;
    let outcome = wkp::install_or_upgrade(
        tool,
        bundle,
        bundle_valid,
        check.installed.present,
        // The "install" decision is already made by the watcher — the pipeline's
        // no-op is not needed.
        None,
        marker_value.as_deref(),
        &mut |argv| match run(argv, wkp::KPACKAGETOOL_TIMEOUT as f64) {
            Some(out) => out,
            None => {
                launch_error.get_or_insert_with(|| argv.join(" "));
                wkp::RunOutput { rc: -1, stdout: String::new(), stderr: String::new() }
            }
        },
    );
    if let Some(reason) = launch_error
        .map(|cmd| rexon_core::i18n::trf("kpackagetool6 failed to start: %1", &[&cmd]))
        .or_else(|| (!outcome.ok).then(|| outcome.message.clone()))
    {
        return Err(reason);
    }
    if let Some(version) = &outcome.marker_to_write {
        if let Err(e) = rexon_core::utils::atomic_write(marker, version) {
            rexon_core::log_warn!("kde-plugin", "не удалось записать метку версии плагина обоев: {e}");
        }
    }
    Ok(outcome)
}

/// Quietly refresh an ALREADY installed KDE wallpaper plugin if it diverged
/// from the bundle (the decision is `waywallen_kde_plugin::refresh_decision`).
/// Called by the Rexon start and by a successful engine update; works on ANY
/// desktop. A plugin that is not installed is not installed. It runs in a
/// background thread under the same lock as the preparation on applying a
/// wallpaper. The engine version is asked from the binary that will be
/// launched, and only when it cannot be decided without it.
pub fn refresh_installed_kde_plugin(trigger: &'static str) {
    let spawned = std::thread::Builder::new().name("rexon-kde-refresh".into()).spawn(move || {
        let message = {
            let _guard = KDE_ENSURE_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            refresh_kde_plugin(&KdeEnv::live())
        };
        match message {
            Ok(Some(text)) => rexon_core::log_info!("kde-plugin", "освежение плагина обоев KDE ({trigger}): {text}"),
            Ok(None) => {}
            Err(text) => rexon_core::log_warn!("kde-plugin", "освежение плагина обоев KDE ({trigger}) не удалось: {text}"),
        }
    });
    if let Err(e) = spawned {
        rexon_core::log_warn!("kde-plugin", "освежение плагина обоев KDE ({trigger}) не запущено: {e}");
    }
}

/// The executor of [`refresh_installed_kde_plugin`]. `Ok(None)` — there is
/// nothing to say (there is no plugin or it matches the bundle: an ordinary
/// start).
fn refresh_kde_plugin(env: &KdeEnv) -> Result<Option<String>, String> {
    use rexon_core::waywallen_kde_plugin::{self as wkp, RefreshDecision};

    let bundle = wkp::bundle_dir(&env.pkg_dir);
    let install = wkp::install_dir(&env.home);
    let marker = wkp::version_marker(&env.home);
    let tool = rexon_core::utils::resolve_tool("kpackagetool6", &[], env.path.as_deref());
    let bundle_valid = bundle_is_valid(&bundle);

    let mut check = read_plugin_check(&bundle, &install, &marker, None, false);
    // Only when there is a copy to judge: an ordinary start without one asks nothing.
    if check.installed.present {
        check.system = read_system_plugin(env);
    }
    check.bundle_missing = !bundle_valid;
    let mut decision = wkp::refresh_decision(&check, tool.is_some(), bundle_valid);
    // We ask for the daemon version only when it cannot be decided without
    // it: the bundle may not suit the daemon, and then we do not spoil a
    // working plugin.
    if decision.upgrades() {
        use rexon_core::we_client::DaemonFs;
        check.daemon_version =
            find_daemon_binary().as_deref().and_then(|bin| crate::we_env::FsProbe.binary_version(bin));
        decision = wkp::refresh_decision(&check, tool.is_some(), bundle_valid);
    }
    let versions = format!(
        "бандл {}, установлен {}, демон {}",
        check.bundle_version.as_deref().unwrap_or("?"),
        check.installed.vendor_version.as_deref().unwrap_or("?"),
        check.daemon_version.as_deref().unwrap_or("?"),
    );
    match decision {
        RefreshDecision::NotInstalled | RefreshDecision::UpToDate => return Ok(None),
        RefreshDecision::NoTool => {
            return Ok(Some(format!("kpackagetool6 не найден — пропускаем ({versions})")))
        }
        // A distribution build of Rexon has no bundle: the plugin comes as a package,
        // and there is nothing to refresh the copy with — not an error.
        RefreshDecision::NoBundle => {
            return Ok(Some(format!("no bundled plugin in this Rexon build, the copy is left as is ({versions})")))
        }
        RefreshDecision::Skip(action) => {
            return Ok(Some(format!("сторож версии против ({}) — не трогаем ({versions})", action.as_str())))
        }
        RefreshDecision::RemoveUserCopy => {
            let dest = move_user_copy_away(env).map_err(|reason| format!("could not move the copy away: {reason}"))?;
            publish_kde_plugin(KdePluginState::default());
            let on_kde = wkp::is_kde(&env.desktop);
            let in_use = on_kde
                && plasma_evaluate(env, &wkp::current_plugins_js(), 10.0)
                    .is_some_and(|out| wkp::any_selected(&wkp::parse_current_plugins(&out)));
            let restarted = wkp::refresh_restarts(on_kde, true, in_use) && restart_plasmashell(env);
            return Ok(Some(format!(
                "packaged plugin {} ({}), our copy in ~/.local moved to {dest}{}",
                check.system.version.as_deref().unwrap_or("?"),
                check.system.package.as_deref().unwrap_or("owner unknown"),
                if restarted { "; plasmashell restarted" } else { "" }
            )));
        }
        RefreshDecision::Upgrade => {}
    }

    let outcome = run_kpackagetool(tool.as_deref(), &bundle, bundle_valid, &check, &marker)
        .map_err(|reason| format!("{reason} ({versions})"))?;
    publish_kde_plugin(KdePluginState::default());

    // We ask Plasma only on KDE: outside it there is no shell, nothing to
    // restart — it will load the new plugin at the next login.
    let on_kde = wkp::is_kde(&env.desktop);
    let in_use = on_kde
        && outcome.installed_now
        && plasma_evaluate(env, &wkp::current_plugins_js(), 10.0)
            .is_some_and(|out| wkp::any_selected(&wkp::parse_current_plugins(&out)));
    let restarted = wkp::refresh_restarts(on_kde, outcome.installed_now, in_use) && restart_plasmashell(env);
    Ok(Some(format!(
        "{} ({versions}){}",
        outcome.message,
        if restarted { "; plasmashell перезапущен" } else { "" }
    )))
}

/// Running a command with a timeout; `None` — it did not start or did not
/// fit.
fn run(argv: &[String], timeout_secs: f64) -> Option<rexon_core::waywallen_kde_plugin::RunOutput> {
    rexon_core::splash_exec::run_capture(argv, timeout_secs).map(|out| {
        rexon_core::waywallen_kde_plugin::RunOutput {
            rc: i32::try_from(out.rc).unwrap_or(-1),
            stdout: out.stdout,
            stderr: out.stderr,
        }
    })
}

/// A port of `rexon.utils.plasma_evaluate_script`: JS into plasmashell over
/// the bus. `None` is a failure (no binary, no service, an error, a timeout),
/// not an empty success. The answer is brought to the "qdbus form" — as with
/// `dbus_call` in Python.
fn plasma_evaluate(env: &KdeEnv, js: &str, timeout_secs: f64) -> Option<String> {
    use rexon_core::utils;
    let path_env = env.path.clone();
    let (kind, exe) = utils::DBUS_TOOL_PREFERENCE.iter().find_map(|(tool, kind)| {
        utils::resolve_tool(tool, &[], path_env.as_deref()).map(|exe| (*kind, exe))
    })?;
    let (dest, path, method) = utils::PLASMA_EVALUATE_SCRIPT;
    let argv = utils::dbus_call_argv(kind, &exe, dest, path, method, &[utils::DbusArg::Str(js.to_owned())]);
    let out = run(&argv, timeout_secs)?;
    if out.rc != 0 {
        return None;
    }
    Some(if kind == "dbus-send" {
        utils::parse_dbus_send_reply(&out.stdout)
    } else {
        out.stdout.trim().to_owned()
    })
}

/// A port of `_restart_plasmashell`: the commands from the core in order,
/// success is the exit code of the last one.
fn restart_plasmashell(env: &KdeEnv) -> bool {
    let mut last = None;
    for mut call in rexon_core::waywallen_kde_plugin::restart_plasmashell_cmds() {
        // The tool is looked up by the same PATH as the others: otherwise
        // substituting the environment in the tests would not reach the shell
        // restart.
        if let Some(exe) = rexon_core::utils::resolve_tool(&call.argv[0], &[], env.path.as_deref()) {
            call.argv[0] = exe;
        }
        last = run(&call.argv, call.timeout_secs as f64);
    }
    last.is_some_and(|out| out.rc == 0)
}

// ── synchronous stop of active wallpapers ───────────────────────────────────

thread_local! {
    /// The state of one `stop_active_wallpapers` run. Separate from the main
    /// transport: the stop has its own socket and its own event loop, and it
    /// is called from the static wallpaper installer's thread.
    static STOP: RefCell<StopState> = RefCell::new(StopState::default());
}

#[derive(Default)]
struct StopState {
    /// Who has not been removed yet: the `request_id` of each `renderer_kill` in
    /// flight.
    awaited: usize,
    killed: i64,
    ok: bool,
    list_id: u64,
    /// The run's socket — the handlers need it to send `renderer_kill`.
    socket: *mut QWebSocket,
    /// The handshake is finished (`connected`) — for the reason in the log.
    connected: bool,
}

/// A port of `client.stop_active_wallpapers` — synchronously remove all
/// active wallpapers.
///
/// Returns the core's pair (`stop_active_result`): the status and the number
/// removed. It throws no exceptions in any state: no daemon / no Qt / a
/// disconnection is a status, not an error (the static wallpaper installer's
/// contract).
pub fn stop_active_wallpapers(timeout_ms: i32) -> (String, i64) {
    let st = probe();
    let gate = we_client::stop_active_gate(&st, qobject::sock_qt_available());
    if let Some((status, killed)) = gate {
        return (status.to_owned(), killed);
    }
    stop_active_on_port(st.ws_port, timeout_ms)
}

/// The WS part of [`stop_active_wallpapers`] on the daemon's known port.
///
/// Called from any thread: the socket, the event loop and the run state are
/// the thread's own. The frames go out only after `connected` (the WebSocket
/// handshake is finished); a socket error or a disconnection exits the loop
/// at once, `timeout_ms` is the ceiling of the whole run. Open for the bench
/// (`we_stop_selftest`), which brings up a fake server on its own port.
pub fn stop_active_on_port(port: i64, timeout_ms: i32) -> (String, i64) {
    let fail = || {
        let (status, killed) = we_client::stop_active_result(false, 0);
        (status.to_owned(), killed)
    };
    if port <= 0 || !qobject::sock_qt_available() {
        return fail();
    }
    // The thread's event dispatcher is set up by the factory itself
    // (`rexon_we_sock_new`).
    let socket = qobject::sock_new();
    if socket.is_null() {
        return fail();
    }
    STOP.with(|s| *s.borrow_mut() = StopState { socket, ..StopState::default() });

    let mut sock = unsafe { Pin::new_unchecked(&mut *socket) };
    let on_connected = sock.as_mut().on_connected(|_sock| stop_on_connected());
    let on_message = sock.as_mut().on_binary_message_received(|_sock, message| {
        let bytes: Vec<u8> = message.into();
        stop_on_frame(&bytes);
    });
    // A disconnection before completion — we exit: this is the original's
    // `("error", 0)` branch.
    let on_disconnected = sock.as_mut().on_disconnected(|_sock| qobject::sock_quit_loop());
    unsafe { qobject::sock_quit_loop_on_error(socket) };

    sock.as_mut().open_url(&QUrl::from(format!("ws://127.0.0.1:{port}/").as_str()));
    let by_guard = qobject::sock_run_loop(timeout_ms) != 0;

    let (ok, killed, connected) = STOP.with(|s| {
        let st = s.borrow();
        (st.ok, st.killed, st.connected)
    });
    if !ok {
        let why = unsafe { qobject::sock_error_string(socket) }.to_string();
        let stage = if connected { "ответ демона не дошёл" } else { "рукопожатие WebSocket не завершилось" };
        let cause = if by_guard { format!("за {timeout_ms} мс") } else { why };
        rexon_core::log_warn!("we", "снятие живых обоев: {stage} ({cause})");
    }

    // The guards are removed BEFORE the socket, otherwise they would poke a
    // dead object.
    drop(on_connected);
    drop(on_message);
    drop(on_disconnected);
    unsafe {
        qobject::sock_shutdown(socket);
        qobject::sock_destroy(socket.cast());
    }
    STOP.with(|s| *s.borrow_mut() = StopState::default());
    let (status, killed) = we_client::stop_active_result(ok, killed);
    (status.to_owned(), killed)
}

/// The core's hook `wallpaper_exec::set_we_stopper`: the wallpaper executor
/// calls it before its own wallpapers (from the apply worker thread — the
/// removal has its own socket and its own event loop, it does not touch the
/// tab's shared transport).
pub fn stop_we_wallpapers_hook(timeout_ms: u64) -> (String, i64) {
    stop_active_wallpapers(i32::try_from(timeout_ms).unwrap_or(i32::MAX))
}

fn stop_send(socket: *mut QWebSocket, frame: &[u8]) -> bool {
    if socket.is_null() {
        return false;
    }
    let mut sock = unsafe { Pin::new_unchecked(&mut *socket) };
    let payload = QByteArray::from(frame);
    i64::from(sock.as_mut().send_binary_message(&payload)) > 0
}

fn stop_on_connected() {
    let request_id = alloc_request_id() as u64;
    let socket = STOP.with(|s| {
        let mut st = s.borrow_mut();
        st.list_id = request_id;
        st.connected = true;
        st.socket
    });
    if !stop_send(socket, &we_client::renderer_list(request_id)) {
        qobject::sock_quit_loop();
    }
}

fn stop_on_frame(bytes: &[u8]) {
    let Ok(frame) = pb::ServerFrame::decode(bytes) else { return };
    let Some(pb::server_frame::Kind::Response(resp)) = frame.kind else {
        return;
    };
    let (list_id, socket) = STOP.with(|s| {
        let st = s.borrow();
        (st.list_id, st.socket)
    });

    if resp.request_id == list_id {
        // Parsing the frame is in the core (`renderer_ids_from_frame`). `None` =
        // "not our frame": there is nothing to remove, we behave as with an
        // empty list.
        let ids = we_client::renderer_ids_from_frame(bytes).unwrap_or_default();
        if ids.is_empty() {
            STOP.with(|s| s.borrow_mut().ok = true);
            qobject::sock_quit_loop();
            return;
        }
        STOP.with(|s| s.borrow_mut().awaited = ids.len());
        for rid in ids {
            let kill_id = alloc_request_id() as u64;
            stop_send(socket, &we_client::renderer_kill(kill_id, &rid));
        }
        return;
    }

    // An answer to renderer_kill: we count the removed ones, as
    // `_on_kill_reply` did.
    let done = STOP.with(|s| {
        let mut st = s.borrow_mut();
        if st.awaited == 0 {
            return false;
        }
        st.killed += 1;
        if st.killed as usize >= st.awaited {
            st.ok = true;
            return true;
        }
        false
    });
    if done {
        qobject::sock_quit_loop();
    }
}

// ══ probe callbacks ═════════════════════════════════════════════════════════

/// Set the socket callbacks. Any of them may be `None`.
///
/// Needed by one transport probe (`bin/we_socket_probe.rs`): it counts
/// connections, frames and errors on a live daemon without the tab window.
/// The tab itself listens to the transport with a subscription in Rust
/// (`subscribe`) and gets the answers through the shim's map
/// (`we_transport.rs`).
pub fn set_hooks(
    connected: VoidHook,
    disconnected: VoidHook,
    error: TextHook,
    response: FrameHook,
    event: TextHook,
) {
    ON_CONNECTED.with(|c| c.set(connected));
    ON_DISCONNECTED.with(|c| c.set(disconnected));
    ON_ERROR.with(|c| c.set(error));
    ON_RESPONSE.with(|c| c.set(response));
    ON_EVENT.with(|c| c.set(event));
    install_shim_hooks();
}

/// Preparing the KDE plugin on fake input: `kpackagetool6`, `dbus-send` and
/// `systemctl` are trap scripts writing their arguments into a log. The real
/// Plasma is not touched.
#[cfg(test)]
mod kde_plugin_tests {
    use super::{ensure_kde_plugin, KdeEnv, KdePurpose};
    use rexon_core::waywallen_kde_plugin as wkp;
    use std::path::{Path, PathBuf};

    struct Stand {
        root: PathBuf,
        env: KdeEnv,
    }

    impl Stand {
        /// `plugins` — what is "set as the wallpaper" on the desktops by
        /// Plasma's answer; `installed` — the version of the already installed
        /// plugin (`None` — none).
        fn new(name: &str, plugins: &str, installed: Option<&str>) -> Stand {
            use std::os::unix::fs::PermissionsExt;
            let root = std::env::temp_dir()
                .join(format!("rexon-kde-stand-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            let bin = root.join("bin");
            let home = root.join("home");
            let pkg = root.join("pkg");
            std::fs::create_dir_all(&bin).unwrap();
            std::fs::create_dir_all(&home).unwrap();
            let package = |base: &Path, version: &str, so: &[u8]| {
                let so_dir = base.join("contents/ui/Plugin/WaywallenDisplayEmbed");
                std::fs::create_dir_all(&so_dir).unwrap();
                std::fs::write(base.join("metadata.json"), "{}").unwrap();
                std::fs::write(base.join("vendor.toml"), format!("version = \"{version}\"\n")).unwrap();
                std::fs::write(so_dir.join(wkp::RUNTIME_SO_NAME), so).unwrap();
            };
            package(Path::new(&wkp::bundle_dir(pkg.to_str().unwrap())), "0.4.3", b"bundle");
            if let Some(version) = installed {
                // The same version — the same runtime: otherwise the version watcher
                // will decide to "refresh" and honestly reinstall the plugin.
                let so: &[u8] = if version == "0.4.3" { b"bundle" } else { b"old" };
                package(Path::new(&wkp::install_dir(home.to_str().unwrap())), version, so);
            }
            let log = root.join("calls.log");
            let script = |tool: &str, body: &str| {
                let path = bin.join(tool);
                std::fs::write(
                    &path,
                    format!("#!/bin/sh\necho \"{tool} $*\" >> '{}'\n{body}\nexit 0\n", log.display()),
                )
                .unwrap();
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            };
            script("kpackagetool6", "");
            script("systemctl", "");
            // The packaged plugin's owner, as pacman says it under LANG=C.
            script("pacman", "echo \"$2 is owned by plasma-wallpaper-waywallen 0.4.6-1\"");
            // Reading the desktops' plugins answers with a list, assigning — with
            // an empty answer, like the real Plasma.
            // A snapshot per desktop (`id=plugin`) — desktop numbers from one.
            let by_desktop: Vec<String> = plugins
                .split(',')
                .filter(|p| !p.is_empty())
                .enumerate()
                .map(|(i, p)| format!("{}={p}", i + 1))
                .collect();
            let by_desktop = by_desktop.join(";");
            script(
                "dbus-send",
                &format!(
                    "case \"$*\" in *out.push*) echo 'method return time=1 sender=:1.2'; \
                     echo '   string \"{plugins}\"';; \
                     *ds\\[i\\].id*) echo 'method return time=1 sender=:1.2'; \
                     echo '   string \"{by_desktop}\"';; esac"
                ),
            );
            let env = KdeEnv {
                desktop: "KDE".to_owned(),
                home: home.to_string_lossy().into_owned(),
                pkg_dir: pkg.to_string_lossy().into_owned(),
                path: Some(bin.to_string_lossy().into_owned()),
                data_dirs: root.join("share").to_string_lossy().into_owned(),
                install_package: fake_install_package,
            };
            STAND_ROOT.with(|r| *r.borrow_mut() = Some(root.clone()));
            Stand { root, env }
        }

        /// The distribution package `plasma-wallpaper-waywallen` is installed.
        fn packaged(self) -> Stand {
            packaged_plugin(&self.root);
            self
        }

        /// A distribution build of Rexon: no bundled plugin.
        fn without_bundle(self) -> Stand {
            std::fs::remove_dir_all(wkp::bundle_dir(&self.env.pkg_dir)).unwrap();
            self
        }

        /// The installed copy is foreign (KDE Store): no `vendor.toml`.
        fn foreign_copy(self) -> Stand {
            std::fs::remove_file(wkp::vendor_toml_path(&wkp::install_dir(&self.env.home))).unwrap();
            self
        }

        fn kpackagetool_installs(&self) -> bool {
            self.calls()
                .lines()
                .any(|l| l.starts_with("kpackagetool6") && (l.contains(" -i ") || l.contains(" -u ")))
        }

        fn user_copy_present(&self) -> bool {
            Path::new(&wkp::install_dir(&self.env.home)).exists()
        }

        /// Copies moved to the backup next to waywallen's data.
        fn backups(&self) -> Vec<PathBuf> {
            let dir = Path::new(&self.env.home).join(".local/share/waywallen");
            std::fs::read_dir(dir).map(|it| it.flatten().map(|e| e.path()).collect()).unwrap_or_default()
        }

        fn calls(&self) -> String {
            std::fs::read_to_string(self.root.join("calls.log")).unwrap_or_default()
        }

        /// Whether the plugin was assigned as the wallpaper (the `select_plugin`
        /// JS).
        fn selected(&self) -> bool {
            self.calls().lines().any(|l| l.starts_with("dbus-send") && l.contains("wallpaperPlugin ="))
        }

        fn restarted(&self) -> bool {
            self.calls().lines().any(|l| l.starts_with("systemctl") && l.contains("restart"))
        }
    }

    impl Drop for Stand {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    thread_local! {
        /// The stand of this test thread — for the package installer substitute
        /// (a plain `fn`, it cannot capture the stand).
        static STAND_ROOT: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
        /// The substitute's answer: `None` — the package "installs".
        static INSTALL_FAILS: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
    }

    fn packaged_plugin(root: &Path) {
        let dir = root.join("share/plasma/wallpapers").join(wkp::PLUGIN_ID);
        std::fs::create_dir_all(dir.join("contents/ui/Plugin")).unwrap();
        std::fs::write(dir.join("metadata.json"), "{}").unwrap();
    }

    /// Instead of the AUR build: records the call and puts the packaged plugin into
    /// the stand's `XDG_DATA_DIRS` (or fails with the given reason).
    fn fake_install_package(package: &str, on_stage: &mut dyn FnMut(&str)) -> Result<(), String> {
        let root = STAND_ROOT.with(|r| r.borrow().clone()).expect("stand of this thread");
        let mut log = std::fs::OpenOptions::new().append(true).create(true).open(root.join("calls.log")).unwrap();
        std::io::Write::write_all(&mut log, format!("install-package {package}\n").as_bytes()).unwrap();
        on_stage("Building");
        if let Some(reason) = INSTALL_FAILS.with(|f| f.borrow().clone()) {
            return Err(reason);
        }
        packaged_plugin(&root);
        Ok(())
    }

    /// The packaged plugin is used as is: no copy into `~/.local`, neither on
    /// opening the tab nor on applying (which still assigns it to the desktops).
    #[test]
    fn packaged_plugin_is_used_without_installing_a_copy() {
        let stand = Stand::new("packaged", "org.kde.image", None).packaged();
        let (ok, _, _, message) = ensure_kde_plugin(&stand.env, None, false, KdePurpose::Prepare);
        assert!(ok, "{message}");
        assert!(!stand.kpackagetool_installs(), "{}", stand.calls());
        assert!(message.contains("0.4.6 (plasma-wallpaper-waywallen)"), "{message}");
        let (ok, selected_now, _, message) = ensure_kde_plugin(&stand.env, None, true, KdePurpose::Apply);
        assert!(ok && selected_now, "{message}");
        assert!(!stand.kpackagetool_installs(), "even with consent: {}", stand.calls());
        assert!(!stand.user_copy_present());
    }

    /// Our copy in `~/.local` hides the package: moved to the backup, not deleted;
    /// the shell that has the copy in memory is restarted.
    #[test]
    fn our_copy_is_moved_away_from_the_package() {
        let stand = Stand::new("ourcopy", "org.waywallen.kde", Some("0.4.3")).packaged();
        let (ok, _, _, message) = ensure_kde_plugin(&stand.env, None, false, KdePurpose::Prepare);
        assert!(ok, "{message}");
        assert!(!stand.user_copy_present(), "the copy still hides the package");
        let backups = stand.backups();
        assert_eq!(backups.len(), 1, "{backups:?}");
        assert!(backups[0].join("vendor.toml").is_file(), "the copy is kept whole: {backups:?}");
        assert!(!stand.kpackagetool_installs(), "{}", stand.calls());
        assert!(stand.restarted(), "the copy's .so was in the shell: {}", stand.calls());
    }

    /// The same at Rexon start (any desktop): the quiet refresh never upgrades
    /// over a package, it moves our copy away.
    #[test]
    fn refresh_moves_our_copy_away_from_the_package() {
        let mut stand = Stand::new("refresh", "org.kde.image", Some("0.4.0")).packaged();
        stand.env.desktop = "MATE".to_owned();
        let message = super::refresh_kde_plugin(&stand.env).expect("refresh");
        assert!(message.is_some_and(|m| m.contains("moved to")));
        assert!(!stand.user_copy_present());
        assert!(!stand.kpackagetool_installs(), "{}", stand.calls());
        assert!(!stand.restarted(), "no shell outside Plasma: {}", stand.calls());
    }

    /// A foreign copy (KDE Store) is not touched without consent; with it — moved
    /// to the backup as well.
    #[test]
    fn foreign_copy_moves_only_by_consent() {
        let stand = Stand::new("foreign", "org.kde.image", Some("0.4.0")).packaged().foreign_copy();
        let (ok, _, _, message) = ensure_kde_plugin(&stand.env, None, false, KdePurpose::Prepare);
        assert!(ok, "{message}");
        assert!(stand.user_copy_present(), "moved without consent");
        let (ok, _, _, message) = ensure_kde_plugin(&stand.env, None, true, KdePurpose::Prepare);
        assert!(ok, "{message}");
        assert!(!stand.user_copy_present());
        assert_eq!(stand.backups().len(), 1);
        assert!(!stand.kpackagetool_installs(), "{}", stand.calls());
    }

    /// A distribution build without the bundle and without the package: Rexon
    /// installs the package itself (no question) and then uses it.
    #[test]
    fn missing_plugin_installs_the_package() {
        let stand = Stand::new("nopkg", "org.kde.image", None).without_bundle();
        let (ok, selected_now, _, message) = ensure_kde_plugin(&stand.env, None, false, KdePurpose::Apply);
        assert!(ok && selected_now, "{message}");
        assert!(stand.calls().contains("install-package plasma-wallpaper-waywallen"), "{}", stand.calls());
        assert!(message.contains("packaged plugin"), "{message}");
        assert!(!stand.kpackagetool_installs(), "{}", stand.calls());
    }

    /// A failed package install is not retried by itself (a password window on every
    /// opening), only by the button.
    #[test]
    fn failed_package_install_waits_for_the_button() {
        let stand = Stand::new("pkgfail", "org.kde.image", None).without_bundle();
        INSTALL_FAILS.with(|f| *f.borrow_mut() = Some("no AUR helper".to_owned()));
        let (ok, _, _, message) = ensure_kde_plugin(&stand.env, None, false, KdePurpose::Prepare);
        assert!(!ok && message.contains("no AUR helper"), "{message}");
        let attempts = || stand.calls().matches("install-package").count();
        let (ok, _, _, message) = ensure_kde_plugin(&stand.env, None, false, KdePurpose::Prepare);
        assert!(ok, "{message}");
        assert_eq!(attempts(), 1, "retried by itself: {}", stand.calls());
        INSTALL_FAILS.with(|f| *f.borrow_mut() = None);
        let (ok, _, _, message) = ensure_kde_plugin(&stand.env, None, true, KdePurpose::Prepare);
        assert!(ok, "{message}");
        assert_eq!(attempts(), 2, "the button retries");
        assert!(super::package_failure(&stand.env).is_none(), "success clears the failure");
    }

    /// Never: defect of 30.10.2026: the first tab opening with an empty
    /// library assigned the plugin as the wallpaper of all desktops and
    /// restarted plasmashell — the desktop went black. Now only installation.
    #[test]
    fn tab_open_with_empty_library_keeps_desktop_wallpaper() {
        let stand = Stand::new("open", "org.kde.image", None);
        let (ok, selected_now, previous, message) =
            ensure_kde_plugin(&stand.env, None, false, KdePurpose::Prepare);
        assert!(ok, "{message}");
        assert!(!selected_now);
        assert!(previous.is_none(), "ничего не назначали — и откатывать нечего");
        assert!(
            stand.calls().lines().any(|l| l.starts_with("kpackagetool6") && l.contains(" -i ")),
            "плагин ставится заранее: {}",
            stand.calls()
        );
        assert!(!stand.selected(), "обои стола не меняются: {}", stand.calls());
        assert!(!stand.restarted(), "plasmashell не перезапускается: {}", stand.calls());
        assert!(!message.contains("выбран на десктопах"), "{message}");
    }

    /// Applying a wallpaper from the library assigns the plugin — without a
    /// shell restart on a fresh installation.
    #[test]
    fn applying_wallpaper_selects_plugin() {
        let stand = Stand::new("apply", "org.kde.image", None);
        let (ok, selected_now, previous, message) =
            ensure_kde_plugin(&stand.env, None, false, KdePurpose::Apply);
        assert!(ok, "{message}");
        assert!(selected_now, "{message}");
        assert!(stand.selected(), "плагин назначен обоями: {}", stand.calls());
        // A snapshot of the previous wallpapers — for a rollback if the live
        // wallpaper does not start.
        assert_eq!(
            previous,
            Some(vec![rexon_core::waywallen_kde_plugin::DesktopPlugin {
                desktop_id: 1,
                plugin: "org.kde.image".to_owned()
            }])
        );
        assert!(!stand.restarted(), "свежая установка оболочку не трогает: {}", stand.calls());
    }

    /// The plugin is already everywhere — it is not assigned again, and there
    /// is no reason to wait for a display.
    #[test]
    fn applying_with_plugin_everywhere_does_not_reselect() {
        let stand = Stand::new("everywhere", "org.waywallen.kde,org.waywallen.kde", Some("0.4.3"));
        let (ok, selected_now, previous, message) =
            ensure_kde_plugin(&stand.env, None, false, KdePurpose::Apply);
        assert!(ok, "{message}");
        assert!(!selected_now);
        assert!(previous.is_none(), "плагин стоял до применения — откатывать нечего");
        assert!(!stand.selected(), "{}", stand.calls());
    }

    /// Updating a plugin that is already set as the wallpaper restarts the
    /// shell (the old `.so` is in memory) but assigns no wallpaper.
    #[test]
    fn upgrading_plugin_in_use_restarts_shell_without_selecting() {
        let stand = Stand::new("upgrade", "org.waywallen.kde,org.kde.image", Some("0.4.0"));
        let (ok, _, _, message) = ensure_kde_plugin(&stand.env, None, false, KdePurpose::Prepare);
        assert!(ok, "{message}");
        assert!(stand.restarted(), "{}", stand.calls());
        assert!(!stand.selected(), "{}", stand.calls());
    }

    /// Updating a plugin that is not selected does not touch the shell.
    #[test]
    fn upgrading_unused_plugin_does_not_restart_shell() {
        let stand = Stand::new("unused", "org.kde.image", Some("0.4.0"));
        let (ok, _, _, message) = ensure_kde_plugin(&stand.env, None, false, KdePurpose::Prepare);
        assert!(ok, "{message}");
        assert!(!stand.restarted(), "{}", stand.calls());
        assert!(!stand.selected(), "{}", stand.calls());
    }
}

#[cfg(test)]
mod tests {
    use rexon_core::we_query::{self, QueryState, STATUS_ERROR};

    /// A disconnection must CLOSE a pending request with an error rather than
    /// silently forget about it.
    ///
    /// The very frame that goes to all pending requests on `disconnected` is
    /// checked: it passes the ordinary answer parsing path and leaves the query
    /// in Error with a human-readable text. Previously the callbacks were never
    /// called, and the page showed an eternal spinner.
    #[test]
    fn obryv_zakryvaet_ozhidayushchij_zapros_oshibkoj() {
        let frame = super::error_frame(7, super::DISCONNECTED_TEXT);
        let mut st = QueryState::default();
        let signals = we_query::apply_ack(&mut st, &frame).expect("кадр разбирается");

        assert_eq!(st.status, STATUS_ERROR, "запрос обязан уйти в Error");
        assert_eq!(st.error, super::DISCONNECTED_TEXT);
        assert!(
            signals
                .iter()
                .any(|s| matches!(s, we_query::QuerySignal::ErrorOccurred(_))),
            "страница обязана получить errorOccurred, а не остаться в Querying"
        );
    }

    /// The request number in the disconnection frame does not matter for
    /// routing (the addressee is chosen by the QPointer map), but it must be a
    /// valid `Response`.
    #[test]
    fn kadr_obryva_eto_nastoyashchij_otvet_servera() {
        use prost::Message;
        let frame = super::error_frame(42, "тест");
        let decoded = rexon_core::proto::control::ServerFrame::decode(&frame[..])
            .expect("кадр сервера разбирается");
        match decoded.kind {
            Some(rexon_core::proto::control::server_frame::Kind::Response(r)) => {
                assert_eq!(r.request_id, 42);
                assert_ne!(r.status, rexon_core::proto::control::Status::Ok as i32);
            }
            other => panic!("ожидался Response, получено {other:?}"),
        }
    }

    /// A `health` answer frame with this engine state.
    fn health_frame(request_id: u64, state: &str) -> Vec<u8> {
        use prost::Message;
        use rexon_core::proto::control as pb;
        let resp = pb::Response {
            request_id,
            status: pb::Status::Ok as i32,
            payload: Some(pb::response::Payload::Health(pb::HealthResponse {
                state: state.to_owned(),
                ..Default::default()
            })),
            ..Default::default()
        };
        pb::ServerFrame { kind: Some(pb::server_frame::Kind::Response(resp)) }.encode_to_vec()
    }

    /// The `state` field arrives from the `health` answer, while someone else's
    /// answer gives emptiness, not someone else's value.
    #[test]
    fn health_state_reads_only_health_payload() {
        assert_eq!(super::health_state(&health_frame(3, "ready")), "ready");
        assert_eq!(super::health_state(&health_frame(3, "")), "");
        assert_eq!(super::health_state(&super::error_frame(3, "x")), "");
    }

    /// Our own `health` answer goes to the subscribers as an event and does NOT
    /// go further — neither into the map of Rust queries nor to Python. Someone
    /// else's number is not intercepted.
    #[test]
    fn own_health_reply_reaches_subscribers_once() {
        use std::cell::RefCell;
        use std::rc::Rc;
        let seen: Rc<RefCell<Vec<super::DaemonEvent>>> = Rc::default();
        let sink = Rc::clone(&seen);
        let id = super::subscribe(move |ev| sink.borrow_mut().push(ev.clone()));

        super::HEALTH_ID.with(|c| c.set(41));
        assert!(super::dispatch_frame(&health_frame(41, "ready")));
        assert_eq!(super::HEALTH_ID.with(std::cell::Cell::get), 0, "ожидание снято");
        // A repeat of the same number is no longer ours: there is nothing more
        // to wait for.
        super::dispatch_frame(&health_frame(41, "ready"));

        super::unsubscribe(id);
        super::dispatch_frame(&health_frame(41, "ready"));
        assert_eq!(*seen.borrow(), vec![super::DaemonEvent::Health("ready".into())]);
    }

    /// The broadcast reaches EACH subscriber, while unsubscribing removes
    /// exactly one.
    #[test]
    fn broadcast_reaches_every_subscriber() {
        use std::cell::Cell;
        use std::rc::Rc;
        let a = Rc::new(Cell::new(0));
        let b = Rc::new(Cell::new(0));
        let (a2, b2) = (Rc::clone(&a), Rc::clone(&b));
        let ia = super::subscribe(move |_| a2.set(a2.get() + 1));
        let ib = super::subscribe(move |_| b2.set(b2.get() + 1));
        super::broadcast(&super::DaemonEvent::Connected);
        super::unsubscribe(ia);
        super::broadcast(&super::DaemonEvent::HealthTimeout);
        super::unsubscribe(ib);
        super::unsubscribe(0);
        assert_eq!((a.get(), b.get()), (1, 2));
    }
}
