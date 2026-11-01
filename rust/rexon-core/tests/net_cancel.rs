//! Отмена запроса, который уже ушёл в сеть (боевой транспорт `UreqNet`).
//!
//! Отдельный файл от `net_parity.rs` сознательно: там голден-сверка с Python
//! на ПОДДЕЛЬНОМ транспорте, здесь — живой сокет. Поднимаем свой медленный
//! HTTP-сервер на localhost (без TLS: проверяется механизм отмены, а не
//! шифрование) и смотрим три вещи:
//!   * обычный запрос к нему проходит и отдаёт тело;
//!   * взведённый в другом потоке токен рвёт ожидание ответа за доли секунды,
//!     а не ждёт медленный сервер;
//!   * сервер видит, что соединение реально закрылось — то есть канал
//!     освобождён, а не «ответ выброшен, а запрос доживает».
//!
//! Регресс к багу Wehrwolfmann 2026-07-31: при быстром переключении категорий
//! запросы покинутых категорий продолжали занимать канал, и время ответа
//! открытой категории росло с 0,95 с до 12,3 с.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use rexon_core::net::{CancelToken, NetError, NetIo, UreqNet, CANCELLED_DETAIL};

const UA: &str = "Rexon-test";

/// Итог со стороны сервера: успел ли клиент дождаться ответа и закрылось ли
/// соединение до того, как сервер собрался отвечать.
#[derive(Debug, PartialEq, Eq)]
enum ServerSaw {
    /// Клиент отвалился, пока сервер «думал».
    ClientGone,
    /// Клиент дождался ответа.
    Served,
}

/// Медленный сервер: принимает одно соединение, читает запрос, ждёт `delay`
/// и только потом отвечает. Возвращает порт и канал с тем, что он увидел.
fn slow_server(delay: Duration, body: &'static str) -> (u16, mpsc::Receiver<ServerSaw>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("слушатель");
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let Ok((mut sock, _)) = listener.accept() else {
            return;
        };
        // Прочитать строку запроса и заголовки (до пустой строки).
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
        // Ключевая проверка: ДО ответа смотрим, на месте ли ещё клиент.
        // Оборвавший соединение шлёт FIN, и чтение сразу возвращает 0 байт;
        // ждущий ответа — молчит (WouldBlock). Запись же успешна в обоих
        // случаях (ядро буферизует), поэтому по ней судить нельзя.
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
        Ok(0) => false,                                   // FIN — клиент ушёл
        Ok(_) => true,                                    // что-то прислал
        Err(e) => e.kind() == std::io::ErrorKind::WouldBlock, // молчит, но жив
    }
}

#[test]
fn обычный_запрос_проходит_через_отменяемый_транспорт() {
    let (port, saw) = slow_server(Duration::from_millis(0), "каталог");
    let mut net = UreqNet::new();
    let body = net
        .get(&format!("http://127.0.0.1:{port}/data"), UA, 25)
        .expect("тело ответа");
    assert_eq!(body, b"\xd0\xba\xd0\xb0\xd1\x82\xd0\xb0\xd0\xbb\xd0\xbe\xd0\xb3");
    assert_eq!(saw.recv_timeout(Duration::from_secs(5)), Ok(ServerSaw::Served));
}

#[test]
fn отмена_рвёт_ожидание_ответа_не_дожидаясь_сервера() {
    // Сервер «думает» 5 секунд — столько же ждал бы и клиент без отмены.
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
    // Сервер, досидев свои 5 секунд, должен обнаружить закрытое соединение.
    assert_eq!(
        saw.recv_timeout(Duration::from_secs(15)),
        Ok(ServerSaw::ClientGone),
        "соединение не закрылось — канал всё ещё занят"
    );
}

#[test]
fn запрос_с_уже_взведённым_токеном_в_сеть_не_идёт() {
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
    // Сервер либо вообще не увидел соединения (таймаут канала), либо увидел
    // и тут же его потерял — оба исхода означают «в сеть не пошли».
    match saw.recv_timeout(Duration::from_secs(8)) {
        Err(_) | Ok(ServerSaw::ClientGone) => {}
        Ok(ServerSaw::Served) => panic!("сервер успел обслужить отменённый запрос"),
    }
}

/// Живой HTTPS к боевому серверу каталога: проверяет, что дробление ожидания
/// на ломти не ломает TLS (ожидание прерывается прямо посреди приёма записи
/// шифрованного потока) и что системное хранилище корней подхватывается.
/// Требует интернет, поэтому по умолчанию не запускается:
/// `cargo test -p rexon-core --test net_cancel -- --ignored`.
#[test]
#[ignore = "нужен интернет"]
fn живой_https_запрос_каталога_проходит() {
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
fn токен_отменяет_только_свой_транспорт() {
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

/// Сервер, который отдаёт ШАПКУ ответа сразу, а тело — не целиком, и молчит.
/// Нужен, чтобы отмена застала клиента ВНУТРИ чтения тела, а не в ожидании
/// шапки: это другой участок кода и отдельный класс зависаний.
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
        // Шапка обещает больше, чем мы пошлём, а соединение держим открытым:
        // клиент остаётся внутри чтения тела.
        let resp = format!("HTTP/1.1 200 OK\r\nContent-Length: {promised}\r\n\r\n{sent}");
        let _ = sock.write_all(resp.as_bytes());
        let _ = sock.flush();
        let _ = tx.send(ServerSaw::Served);
        // Держим сокет открытым, пока клиент не уйдёт сам.
        std::thread::sleep(Duration::from_secs(20));
    });
    (port, rx)
}

/// ⚠️ РЕГРЕСС 2026-09-19: отмена ВО ВРЕМЯ ЧТЕНИЯ ТЕЛА не срабатывала вовсе —
/// поток уходил в вечный горячий круг и жёг ядро на 100 %.
///
/// Причина: отменённый транспорт отдавал `io::ErrorKind::Interrupted`, а
/// `Read::read_to_end` этот вид ошибки по договору стандартной библиотеки
/// ИГНОРИРУЕТ и повторяет чтение. Наружу это выходило так: человек закрывает
/// окно, а Rexon не закрывается — фоновая задача вкладки не кончается, и
/// выход ждёт её вечно.
#[test]
fn отмена_посреди_чтения_тела_возвращает_управление() {
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
