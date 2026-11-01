// Отправка кадров запросов вкладки WE и возврат ответов демона адресату.
//
// Пара к cpp/rexon_we_transport.cpp: там карта «номер запроса → QPointer на
// сам запрос» и вызов его инвокабла `deliverResponse` по имени через
// метаобъект — почему это C++, расписано в шапке заголовка. Здесь только
// тонкие безопасные обёртки: срез в указатель+длину.
//
// ⚠️ Решений этот модуль не принимает: что отправлять — считает запрос,
// провод держит сокет (src/we_socket.rs), кому вернуть ответ — помнит шим.

use std::ffi::{c_char, c_void};

unsafe extern "C" {
    fn rexon_we_remember(token: i64, query: *mut c_void);
    fn rexon_we_do_deliver_response(token: i64, data: *const c_char, len: i32) -> i32;
    fn rexon_we_do_pending_count() -> i32;
    fn rexon_we_do_fail_pending(data: *const c_char, len: i32) -> i32;
}

/// Запомнить владельца номера перед отправкой кадра.
///
/// # Safety
/// `query` — живой QObject-запрос (указатель на самого себя).
pub unsafe fn remember(token: i64, query: *mut c_void) {
    unsafe { rexon_we_remember(token, query) }
}

/// Отдать кадр запроса в сокет демона. Сокета нет (стенд моделей, демон не
/// запущен) — тишина: запрос дождётся ответа об обрыве или переподключения.
///
/// `token` не нужен: номер уже лежит в кадре (`request_id`). `field`
/// нужен сокету: применение обоев на KDE он придерживает, пока не назначен
/// плагин обоев (`we_socket::send_request_frame`).
pub fn send(_token: i64, field: &str, request: &[u8]) {
    crate::we_socket::send_request_frame(field, request);
}

/// Отдать ответ демона запросу с этим номером. `false` — ждущего нет.
pub fn deliver_response_bytes(request_id: i64, frame: &[u8]) -> bool {
    let Ok(len) = i32::try_from(frame.len()) else {
        return false;
    };
    unsafe { rexon_we_do_deliver_response(request_id, frame.as_ptr().cast::<c_char>(), len) != 0 }
}

/// Раздать снимок прогресса длинной задачи ждущим запросам.
///
/// Нужен ровно одному потребителю — прогрессу длинных задач
/// (`task_progress`), на который подписан каждый `ProgressQuery`.
pub fn deliver_event_bytes(frame: &[u8]) -> usize {
    let Some((query_id, snapshot)) = rexon_core::we_manage::take_task_progress(frame) else {
        return 0;
    };
    crate::query_base::progress::dispatch(&query_id, &snapshot)
}

/// Закрыть ВСЕ ожидающие запросы этим кадром (обрыв соединения).
///
/// Возвращает число запросов, до которых кадр доехал.
pub fn fail_pending(frame: &[u8]) -> i32 {
    let Ok(len) = i32::try_from(frame.len()) else {
        return 0;
    };
    unsafe { rexon_we_do_fail_pending(frame.as_ptr().cast::<c_char>(), len) }
}

/// Сколько запросов ждут ответа. Нужно проверкам: незакрытый счёт после
/// прогона означает потерянный ответ.
pub fn pending_count() -> i32 {
    unsafe { rexon_we_do_pending_count() }
}
