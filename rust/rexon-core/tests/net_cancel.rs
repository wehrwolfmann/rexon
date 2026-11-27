//! Cancelling a request that has already gone out to the network (the production transport `UreqNet`).
//!
//! A separate file from `net_parity.rs` on purpose: there it is a golden check against Python
//! on a FAKE transport, here it is a live socket. We bring up our own slow
//! HTTP server on localhost (without TLS: the cancellation mechanism is checked, not
//! encryption) and look at three things:
//!   * an ordinary request to it goes through and returns the body;
//!   * a token armed in another thread breaks the wait for the answer in fractions of a second,
//!     instead of waiting for the slow server;
//!   * the server sees that the connection really closed — i.e. the channel
//!     is freed, rather than "the answer is thrown away, but the request lives on".
//!
//! Regression of Wehrwolfmann's bug of 2026-07-31: when switching categories quickly,
//! the requests of abandoned categories kept occupying the channel, and the response time
//! of the open category grew from 0.95 s to 12.3 s.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use rexon_core::net::{CancelToken, NetError, NetIo, UreqNet, CANCELLED_DETAIL};

const UA: &str = "Rexon-test";

/// The outcome on the server side: did the client manage to wait for the answer, and did
/// the connection close before the server got around to answering.
#[derive(Debug, PartialEq, Eq)]
enum ServerSaw {
    /// The client dropped off while the server was "thinking".
    ClientGone,
    /// The client waited for the answer.
    Served,
}

/// A slow server: accepts one connection, reads the request, waits `delay`
/// and only then answers. Returns the port and a channel with what it saw.
fn slow_server(delay: Duration, body: &'static str) -> (u16, mpsc::Receiver<ServerSaw>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("слушатель");
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let Ok((mut sock, _)) = listener.accept() else {
            return;
        };
        // Read the request line and headers (up to the empty line).
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
        std::thread::sleep(delay);
        // The key check: BEFORE answering, see whether the client is still there.
        // One that dropped the connection sends FIN, and a read returns 0 bytes at once;
        // one waiting for the answer is silent (WouldBlock). A write succeeds in both
        // cases (the kernel buffers it), so it cannot be used to judge.
        if !client_still_there(&mut sock) {
            let _ = tx.send(ServerSaw::ClientGone);
            return;
        }
        let resp = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        let write_ok = sock.write_all(resp.as_bytes()).is_ok() && sock.flush().is_ok();
        let _ = tx.send(if write_ok {
            ServerSaw::Served
        } else {
            ServerSaw::ClientGone
        });
    });
    (port, rx)
}

fn client_still_there(sock: &mut TcpStream) -> bool {
    sock.set_read_timeout(Some(Duration::from_millis(200))).ok();
    let mut buf = [0u8; 1];
    match sock.read(&mut buf) {
        Ok(0) => false,                                   // FIN — the client left
        Ok(_) => true,                                    // sent something
        Err(e) => e.kind() == std::io::ErrorKind::WouldBlock, // silent, but alive
    }
}

#[test]
fn plain_request_passes_through_cancellable_transport() {
    let (port, saw) = slow_server(Duration::from_millis(0), "каталог");
    let mut net = UreqNet::new();
    let body = net
        .get(&format!("http://127.0.0.1:{port}/data"), UA, 25)
        .expect("тело ответа");
    assert_eq!(body, b"\xd0\xba\xd0\xb0\xd1\x82\xd0\xb0\xd0\xbb\xd0\xbe\xd0\xb3");
    assert_eq!(saw.recv_timeout(Duration::from_secs(5)), Ok(ServerSaw::Served));
}

#[test]
fn cancel_breaks_reply_wait_without_waiting_for_server() {
    // The server "thinks" for 5 seconds — as long as the client would wait without cancellation.
    let (port, saw) = slow_server(Duration::from_secs(5), "поздно");
    let mut net = UreqNet::new();
    let token = net.cancel_token();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        token.cancel();
    });

    let started = Instant::now();
    let err = net
        .get(&format!("http://127.0.0.1:{port}/data"), UA, 25)
        .expect_err("запрос обязан оборваться");
    let waited = started.elapsed();

    match &err {
        NetError::Transport { detail } => assert!(
            detail.contains(CANCELLED_DETAIL),
            "ошибка должна быть об отмене, а не о сети: {detail}"
        ),
        other => panic!("ожидался обрыв соединения, получено {other:?}"),
    }
    assert!(
        waited < Duration::from_secs(2),
        "отмена сработала за {waited:?} — ожидание сервера не прервано"
    );
    // The server, having sat out its 5 seconds, must discover a closed connection.
    assert_eq!(
        saw.recv_timeout(Duration::from_secs(15)),
        Ok(ServerSaw::ClientGone),
        "соединение не закрылось — канал всё ещё занят"
    );
}

#[test]
fn request_with_armed_token_does_not_hit_network() {
    let (port, saw) = slow_server(Duration::from_secs(5), "не должно понадобиться");
    let token = CancelToken::new();
    token.cancel();
    let mut net = UreqNet::new().with_cancel(token);

    let started = Instant::now();
    let err = net
        .get(&format!("http://127.0.0.1:{port}/data"), UA, 25)
        .expect_err("отменённый заранее запрос обязан отказать");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "отказ должен быть мгновенным"
    );
    match &err {
        NetError::Transport { detail } => assert!(detail.contains(CANCELLED_DETAIL)),
        other => panic!("ожидался обрыв, получено {other:?}"),
    }
    // The server either did not see the connection at all (channel timeout), or saw it
    // and lost it right away — both outcomes mean "did not go to the network".
    match saw.recv_timeout(Duration::from_secs(8)) {
        Err(_) | Ok(ServerSaw::ClientGone) => {}
        Ok(ServerSaw::Served) => panic!("сервер успел обслужить отменённый запрос"),
    }
}

/// Live HTTPS to the production catalog server: checks that splitting the wait
/// into slices does not break TLS (the wait is interrupted right in the middle of receiving a record
/// of the encrypted stream) and that the system root store is picked up.
/// Requires the internet, so it is not run by default:
/// `cargo test -p rexon-core --test net_cancel -- --ignored`.
#[test]
#[ignore = "needs the internet"]
fn live_https_catalog_request_passes() {
    let mut net = UreqNet::new();
    let started = Instant::now();
    let body = net
        .get(
            "https://api.opendesktop.org/ocs/v1/content/data\
             ?categories=114&page=0&pagesize=20&sortmode=new",
            "Rexon/5.0",
            25,
        )
        .expect("боевой каталог обязан ответить");
    println!(
        "живой ответ: {} байт за {:?}",
        body.len(),
        started.elapsed()
    );
    assert!(body.starts_with(b"<?xml"), "ожидался XML OCS");
    assert!(body.len() > 10_000, "ответ подозрительно короткий");
}

#[test]
fn token_cancels_only_own_transport() {
    let (port_a, _saw_a) = slow_server(Duration::from_millis(0), "первый");
    let (port_b, _saw_b) = slow_server(Duration::from_millis(0), "второй");
    let mut a = UreqNet::new();
    let mut b = UreqNet::new();
    a.cancel_token().cancel();
    assert!(a.get(&format!("http://127.0.0.1:{port_a}/x"), UA, 25).is_err());
    assert!(
        b.get(&format!("http://127.0.0.1:{port_b}/x"), UA, 25).is_ok(),
        "отмена одного запроса не должна задевать соседний"
    );
}

/// A server that sends the response HEAD right away, and the body not in full, and goes silent.
/// Needed so that cancellation catches the client INSIDE reading the body, not while waiting for
/// the head: this is a different section of code and a separate class of hangs.
fn stalling_body_server(promised: usize, sent: &'static str) -> (u16, mpsc::Receiver<ServerSaw>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("слушатель");
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = mpsc::channel();
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
        // The head promises more than we will send, and we keep the connection open:
        // the client stays inside reading the body.
        let resp = format!("HTTP/1.1 200 OK\r\nContent-Length: {promised}\r\n\r\n{sent}");
        let _ = sock.write_all(resp.as_bytes());
        let _ = sock.flush();
        let _ = tx.send(ServerSaw::Served);
        // Keep the socket open until the client leaves by itself.
        std::thread::sleep(Duration::from_secs(20));
    });
    (port, rx)
}

/// ⚠️ REGRESSION of 2026-09-19: cancellation DURING BODY READING did not work at all —
/// the thread went into an eternal hot loop and burned a core at 100 %.
///
/// The cause: the cancelled transport returned `io::ErrorKind::Interrupted`, and
/// `Read::read_to_end`, by the standard library's contract, IGNORES this kind of error
/// and retries the read. Outside it looked like this: a person closes
/// the window, but Rexon does not close — the tab's background task does not finish, and
/// the exit waits for it forever.
#[test]
fn cancel_mid_body_read_returns_control() {
    let (port, saw) = stalling_body_server(1_000_000, "начало тела");
    let mut net = UreqNet::new();
    let token = net.cancel_token();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(400));
        token.cancel();
    });

    let started = Instant::now();
    let err = net
        .get(&format!("http://127.0.0.1:{port}/big"), UA, 25)
        .expect_err("чтение тела обязано оборваться отменой");
    let waited = started.elapsed();

    assert_eq!(
        saw.recv_timeout(Duration::from_secs(5)),
        Ok(ServerSaw::Served),
        "сервер обязан был отдать шапку — иначе отмена застала не тот участок"
    );
    match &err {
        NetError::Transport { detail } => assert!(
            detail.contains(CANCELLED_DETAIL),
            "ошибка должна быть об отмене: {detail}"
        ),
        other => panic!("ожидался обрыв чтения тела, получено {other:?}"),
    }
    assert!(
        waited < Duration::from_secs(3),
        "чтение тела не прервалось за {waited:?} — вернулся вечный горячий круг"
    );
}
