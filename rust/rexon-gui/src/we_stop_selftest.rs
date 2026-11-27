//! Стенд синхронного снятия живых обоев (`we_socket::stop_active_on_port`)
//! против подставного демона waywallen.
//!
//! Зачем. 15.11.2026 вживую (Cinnamon 6.8 Wayland, обои Axon) снятие WE-обоев
//! перед своими кончалось «⚠ Не удалось остановить живые обои», хотя снимать
//! было нечего: сокет открывался в рабочем потоке std::thread, где у Qt ещё
//! не было диспетчера событий, — подключение никто не слышал, рукопожатие
//! WebSocket не уходило, а закрытие писало кадр в неподключённый сокет
//! (демон: «Handshake not finished»). Стенд гоняет снятие из такого же
//! потока и из главного против сервера на `std::net`, который сам делает
//! рукопожатие RFC 6455 и отвечает кадрами протокола демона.
//!
//! Случаи: снимать нечего; два рендерера; медленное рукопожатие (клиент не
//! смеет слать кадры до ответа сервера); сервер молчит (выход по таймауту,
//! а не зависание); порт закрыт (выход сразу, без таймаута).
//!
//!     QT_QPA_PLATFORM=offscreen cargo run -p rexon-gui --bin rexon-gui-selftest -- --we-stop-selftest

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};

use prost::Message;
use rexon_core::proto::control as pb;
use rexon_core::we_client;

pub const ARG: &str = "--we-stop-selftest";

/// Как ведёт себя подставной демон.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    /// Рукопожатие сразу, рендереров нет.
    Empty,
    /// Рукопожатие сразу, два рендерера — оба надо снять.
    Two,
    /// Ответ на рукопожатие через полсекунды, рендереров нет.
    SlowHandshake,
    /// Принял TCP и молчит.
    Mute,
}

/// Что увидел сервер.
#[derive(Debug, Default)]
struct Seen {
    /// Пришёл GET с `Upgrade: websocket` и ключом.
    handshake: bool,
    /// Байты от клиента ДО ответа сервера на рукопожатие (гонка).
    premature: usize,
    /// Снятые рендереры по порядку.
    kills: Vec<String>,
}

const GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

fn base64(bytes: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ABC[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn accept_key(key: &str) -> String {
    use sha1::{Digest, Sha1};
    let mut h = Sha1::new();
    h.update(key.as_bytes());
    h.update(GUID.as_bytes());
    base64(&h.finalize())
}

/// Прочитать HTTP-запрос рукопожатия. Возвращает ключ и лишние байты,
/// пришедшие следом в том же чтении.
fn read_request(stream: &mut TcpStream) -> Option<(String, usize)> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    loop {
        let n = stream.read(&mut chunk).ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buf[..end]).to_string();
            let extra = buf.len() - end - 4;
            let lower = head.to_lowercase();
            if !head.starts_with("GET ") || !lower.contains("upgrade: websocket") {
                return None;
            }
            let key = head
                .lines()
                .find_map(|l| {
                    let (k, v) = l.split_once(':')?;
                    k.trim().eq_ignore_ascii_case("sec-websocket-key").then(|| v.trim().to_string())
                })?;
            return Some((key, extra));
        }
    }
}

/// Один кадр клиента: (опкод, полезная нагрузка). Клиент обязан маскировать.
fn read_frame(stream: &mut TcpStream) -> Option<(u8, Vec<u8>)> {
    let mut head = [0u8; 2];
    stream.read_exact(&mut head).ok()?;
    let opcode = head[0] & 0x0f;
    let masked = head[1] & 0x80 != 0;
    let mut len = u64::from(head[1] & 0x7f);
    if len == 126 {
        let mut b = [0u8; 2];
        stream.read_exact(&mut b).ok()?;
        len = u64::from(u16::from_be_bytes(b));
    } else if len == 127 {
        let mut b = [0u8; 8];
        stream.read_exact(&mut b).ok()?;
        len = u64::from_be_bytes(b);
    }
    let mut mask = [0u8; 4];
    if masked {
        stream.read_exact(&mut mask).ok()?;
    }
    let mut payload = vec![0u8; usize::try_from(len).ok()?];
    stream.read_exact(&mut payload).ok()?;
    if masked {
        for (i, b) in payload.iter_mut().enumerate() {
            *b ^= mask[i % 4];
        }
    }
    Some((opcode, payload))
}

fn write_binary(stream: &mut TcpStream, payload: &[u8]) {
    let mut frame = vec![0x82u8];
    if payload.len() < 126 {
        frame.push(payload.len() as u8);
    } else {
        frame.push(126);
        frame.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    }
    frame.extend_from_slice(payload);
    let _ = stream.write_all(&frame);
}

fn response(request_id: u64, payload: pb::response::Payload) -> Vec<u8> {
    pb::ServerFrame {
        kind: Some(pb::server_frame::Kind::Response(pb::Response {
            request_id,
            status: pb::Status::Ok as i32,
            payload: Some(payload),
            ..Default::default()
        })),
    }
    .encode_to_vec()
}

fn serve(listener: TcpListener, mode: Mode) -> Seen {
    let mut seen = Seen::default();
    let Ok((mut stream, _)) = listener.accept() else { return seen };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(6)));
    let Some((key, extra)) = read_request(&mut stream) else { return seen };
    seen.handshake = true;
    seen.premature += extra;
    match mode {
        Mode::Mute => {
            // Держим связь, пока клиент не бросит сам.
            let mut sink = [0u8; 256];
            while matches!(stream.read(&mut sink), Ok(n) if n > 0) {}
            return seen;
        }
        Mode::SlowHandshake => {
            // Всё, что клиент пришлёт за эти полсекунды, — нарушение.
            let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
            let mut early = [0u8; 256];
            if let Ok(n) = stream.read(&mut early) {
                seen.premature += n;
            }
            let _ = stream.set_read_timeout(Some(Duration::from_secs(6)));
        }
        Mode::Empty | Mode::Two => {}
    }
    let reply = format!(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
         Sec-WebSocket-Accept: {}\r\n\r\n",
        accept_key(&key)
    );
    let _ = stream.write_all(reply.as_bytes());
    while let Some((opcode, payload)) = read_frame(&mut stream) {
        if opcode == 0x8 {
            break;
        }
        if opcode != 0x2 {
            continue;
        }
        let Ok(req) = pb::Request::decode(payload.as_slice()) else { continue };
        match req.payload {
            Some(pb::request::Payload::RendererList(_)) => {
                let ids: &[&str] = if mode == Mode::Two { &["r1", "r2"] } else { &[] };
                let instances = ids
                    .iter()
                    .map(|id| pb::RendererInstance { renderer_id: (*id).to_string(), ..Default::default() })
                    .collect();
                let list = pb::RendererListResponse { instances, ..Default::default() };
                write_binary(&mut stream, &response(req.request_id, pb::response::Payload::RendererList(list)));
            }
            Some(pb::request::Payload::RendererKill(kill)) => {
                seen.kills.push(kill.renderer_id);
                write_binary(&mut stream, &response(req.request_id, pb::response::Payload::RendererKill(pb::Empty {})));
            }
            _ => {}
        }
    }
    seen
}

/// Один прогон: сервер в своём потоке, снятие — в новом потоке std::thread
/// (как у «Применить» обоев) либо в главном.
fn run_case(mode: Mode, timeout_ms: i32, in_worker: bool) -> ((String, i64), Seen, Duration) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("порт для подставного демона");
    let port = i64::from(listener.local_addr().unwrap().port());
    let server = std::thread::spawn(move || serve(listener, mode));
    let started = Instant::now();
    let answer = if in_worker {
        std::thread::spawn(move || crate::we_socket::stop_active_on_port(port, timeout_ms))
            .join()
            .expect("поток снятия упал")
    } else {
        crate::we_socket::stop_active_on_port(port, timeout_ms)
    };
    let took = started.elapsed();
    let seen = server.join().expect("поток сервера упал");
    (answer, seen, took)
}

pub fn run() -> i32 {
    let _app = cxx_qt_lib::QCoreApplication::new();
    let mut failures: Vec<String> = Vec::new();
    let mut check = |ok: bool, what: String| {
        println!("{} {what}", if ok { "ok  " } else { "FAIL" });
        if !ok {
            failures.push(what);
        }
    };
    let stopped = we_client::STOP_STOPPED.to_string();
    let error = we_client::STOP_ERROR.to_string();

    // Прогрев без проверки: первый сокет Qt в процессе подгружает сетевые
    // библиотеки с диска, и на холодном кэше под чужими сборками это шло
    // 4,8 с (15.11.2026, итог при этом верный) — замеры времени ниже
    // меряли бы диск, а не снятие.
    let (_, _, warm) = run_case(Mode::Empty, 10_000, false);
    println!("прогрев: {warm:?}");

    for (in_worker, place) in [(true, "рабочий поток"), (false, "главный поток")] {
        // Снимать нечего — «сняли 0», а не ошибка (иначе предупреждение).
        let (answer, seen, took) = run_case(Mode::Empty, 4000, in_worker);
        check(
            answer == (stopped.clone(), 0) && seen.handshake && seen.premature == 0,
            format!("{place}: нечего снимать → {answer:?}, {seen:?}, {took:?}"),
        );
        check(took < Duration::from_secs(2), format!("{place}: пустой список без ожидания сторожа ({took:?})"));
    }

    let (answer, seen, took) = run_case(Mode::Two, 4000, true);
    check(
        answer == (stopped.clone(), 2) && seen.kills == ["r1", "r2"],
        format!("два рендерера сняты → {answer:?}, {seen:?}, {took:?}"),
    );

    let (answer, seen, took) = run_case(Mode::SlowHandshake, 4000, true);
    check(
        answer == (stopped.clone(), 0) && seen.premature == 0,
        format!("медленное рукопожатие: кадры только после него → {answer:?}, {seen:?}, {took:?}"),
    );

    let (answer, seen, took) = run_case(Mode::Mute, 700, true);
    check(
        answer == (error.clone(), 0) && took < Duration::from_millis(2500),
        format!("сервер молчит: выход по таймауту → {answer:?}, {seen:?}, {took:?}"),
    );

    // Закрытый порт: занять и сразу отпустить.
    let port = {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        i64::from(l.local_addr().unwrap().port())
    };
    let started = Instant::now();
    let answer = std::thread::spawn(move || crate::we_socket::stop_active_on_port(port, 4000))
        .join()
        .expect("поток снятия упал");
    let took = started.elapsed();
    check(
        answer == (error, 0) && took < Duration::from_millis(1500),
        format!("порт закрыт: отказ сразу, без сторожа → {answer:?}, {took:?}"),
    );

    if failures.is_empty() {
        println!("стенд снятия живых обоев: всё сошлось");
        0
    } else {
        eprintln!("стенд снятия живых обоев: провалов {}", failures.len());
        1
    }
}
