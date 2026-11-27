// The control bench of the Wallpaper Engine tab transport (batch 7).
//
// It runs NOT a mock-up but the product transport `rexon_gui::we_socket`
// itself: finding the daemon over D-Bus, opening the socket, sending the
// `health` frame, routing the answer by the `request_id` inside `Response`
// and receiving events. It prints one machine-readable line
// `REXON-WS-PROBE ...` — in the same spirit as `REXON-SELFTEST ...` of the
// model bench.
//
// Exit 0 — the daemon answered and the request number is unique; otherwise
// 1 and the reason in the error= field.
//
// Warning: a bite check: `REXON_WS_PROBE_BITE=port` takes the socket to a
// neighbouring port. A bench that prints `health=healthy` even in this case
// checks nothing — it means the bench itself is broken.

use std::cell::{Cell, RefCell};
use std::ffi::c_char;

use cxx_qt_lib::QCoreApplication;
use prost::Message;
use rexon_core::proto::control as pb;
use rexon_gui::we_socket;

thread_local! {
    static CONNECTED: Cell<bool> = const { Cell::new(false) };
    static FRAMES: Cell<i32> = const { Cell::new(0) };
    static EVENTS: Cell<i32> = const { Cell::new(0) };
    static REQUEST_ID: Cell<i64> = const { Cell::new(0) };
    static SENT: Cell<bool> = const { Cell::new(false) };
    static HEALTH: RefCell<String> = RefCell::new(String::new());
    static ERROR: RefCell<String> = RefCell::new(String::new());
}

// The number `health` goes out under. It is issued by the same counter as
// for the tab's queries — a check that the counter is one.
thread_local! {
    static HEALTH_ID: Cell<u64> = const { Cell::new(0) };
}

extern "C" fn on_connected() {
    CONNECTED.with(|c| c.set(true));
    let request_id = we_socket::alloc_request_id() as u64;
    HEALTH_ID.with(|c| c.set(request_id));
    let sent = we_socket::send_frame(&rexon_core::we_client::health(request_id));
    SENT.with(|c| c.set(sent));
}

extern "C" fn on_disconnected() {
    ERROR.with(|e| {
        let mut text = e.borrow_mut();
        if text.is_empty() && HEALTH.with(|h| h.borrow().is_empty()) {
            *text = "сокет закрылся до ответа".to_owned();
        }
    });
}

extern "C" fn on_error(data: *const c_char, len: i32) {
    let text = text_of(data, len);
    ERROR.with(|e| {
        let mut slot = e.borrow_mut();
        if slot.is_empty() {
            *slot = text;
        }
    });
}

/// An answer that no tab query was waiting for — that is, our `health`.
extern "C" fn on_response(request_id: i64, data: *const c_char, len: i32) {
    FRAMES.with(|c| c.set(c.get() + 1));
    REQUEST_ID.with(|c| c.set(request_id));
    let bytes = bytes_of(data, len);
    let Ok(frame) = pb::ServerFrame::decode(&bytes[..]) else {
        return;
    };
    let Some(pb::server_frame::Kind::Response(resp)) = frame.kind else {
        return;
    };
    if let Some(pb::response::Payload::Health(h)) = resp.payload {
        HEALTH.with(|slot| *slot.borrow_mut() = h.state);
    }
}

extern "C" fn on_event(data: *const c_char, len: i32) {
    let _ = (data, len);
    FRAMES.with(|c| c.set(c.get() + 1));
    EVENTS.with(|c| c.set(c.get() + 1));
}

fn bytes_of(data: *const c_char, len: i32) -> Vec<u8> {
    if data.is_null() || len <= 0 {
        return Vec::new();
    }
    unsafe { std::slice::from_raw_parts(data.cast::<u8>(), len as usize) }.to_vec()
}

fn text_of(data: *const c_char, len: i32) -> String {
    String::from_utf8_lossy(&bytes_of(data, len)).into_owned()
}

fn main() {
    // QCoreApplication, not QGuiApplication: the transport needs neither a
    // screen nor a QPA platform — this is exactly the check that the layer is
    // separable from the UI.
    let _app = QCoreApplication::new();

    let status = we_socket::probe();
    let bite = std::env::var("REXON_WS_PROBE_BITE").unwrap_or_default();
    let port = i32::try_from(status.ws_port).unwrap_or(0) + i32::from(bite == "port");

    if port == 0 {
        println!("REXON-WS-PROBE port=0 error=демон не отвечает по D-Bus (WsPort=0)");
        std::process::exit(1);
    }

    we_socket::set_hooks(
        Some(on_connected),
        Some(on_disconnected),
        Some(on_error),
        Some(on_response),
        Some(on_event),
    );
    we_socket::open(port);
    // The bench does not need reconnection: it checks one run, not robustness.
    rexon_gui::we_socket::qobject::sock_run_loop(4000);

    let health = HEALTH.with(|h| h.borrow().clone());
    let error = ERROR.with(|e| e.borrow().clone());
    let request_id = REQUEST_ID.with(Cell::get);
    let health_id = HEALTH_ID.with(Cell::get);

    let ok = health == "healthy"
        && request_id == health_id as i64
        && health_id == 1
        && SENT.with(Cell::get);

    println!(
        "REXON-WS-PROBE port={} version={} connected={} sent={} frames={} \
events={} requestId={} health={} pending={} error={}",
        port,
        if status.version.is_empty() { "-" } else { &status.version },
        CONNECTED.with(Cell::get),
        SENT.with(Cell::get),
        FRAMES.with(Cell::get),
        EVENTS.with(Cell::get),
        request_id,
        if health.is_empty() { "-" } else { &health },
        rexon_gui::we_transport::pending_count(),
        if error.is_empty() { "-" } else { &error },
    );

    we_socket::close();
    std::process::exit(i32::from(!ok));
}
