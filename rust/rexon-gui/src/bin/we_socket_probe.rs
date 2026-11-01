// Контрольный стенд транспорта вкладки Wallpaper Engine (партия 7).
//
// Гоняет НЕ макет, а сам продуктовый транспорт `rexon_gui::we_socket`:
// поиск демона по D-Bus, открытие сокета, отправку кадра `health`,
// маршрутизацию ответа по `request_id` изнутри `Response` и приём событий.
// Печатает одну машинно-читаемую строку `REXON-WS-PROBE ...` — в том же
// духе, что `REXON-SELFTEST ...` у стенда моделей.
//
// Выход 0 — демон ответил и номер запроса единственный; иначе 1 и причина
// в поле error=.
//
// ⚠️ Проверка на укус: `REXON_WS_PROBE_BITE=port` уводит сокет на соседний
// порт. Стенд, который и в этом случае печатает `health=healthy`, ничего
// не проверяет — значит, сломан сам стенд.

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

// Номер, под которым уходит `health`. Выдаёт его тот же счётчик, что и
// запросам вкладки, — проверка того, что счётчик один.
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

/// Ответ, которого не ждал ни один запрос вкладки, — то есть наш `health`.
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
    // QCoreApplication, а не QGuiApplication: транспорту не нужен ни экран,
    // ни QPA-платформа — это и есть проверка того, что слой отделим от UI.
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
    // Реконнект стенду не нужен: он проверяет один прогон, а не живучесть.
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
