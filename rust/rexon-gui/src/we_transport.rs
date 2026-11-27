// Sending the WE tab's request frames and returning the daemon's answers to
// the addressee.
//
// A pair to cpp/rexon_we_transport.cpp: there lie the "request number →
// QPointer to the request itself" map and calling its `deliverResponse`
// invokable by name through the metaobject — why this is C++ is laid out in
// the header's preamble. Here there are only thin safe wrappers: a slice
// into a pointer+length.
//
// Warning: this module makes no decisions: what to send is computed by the
// query, the wire is held by the socket (src/we_socket.rs), whom to return
// the answer to is remembered by the shim.

use std::ffi::{c_char, c_void};

unsafe extern "C" {
    fn rexon_we_remember(token: i64, query: *mut c_void);
    fn rexon_we_do_deliver_response(token: i64, data: *const c_char, len: i32) -> i32;
    fn rexon_we_do_pending_count() -> i32;
    fn rexon_we_do_fail_pending(data: *const c_char, len: i32) -> i32;
}

/// Remember the owner of the number before sending the frame.
///
/// # Safety
/// `query` is a live QObject query (a pointer to itself).
pub unsafe fn remember(token: i64, query: *mut c_void) {
    unsafe { rexon_we_remember(token, query) }
}

/// Hand the request frame to the daemon socket. If there is no socket (the
/// model bench, the daemon is not running) — silence: the query will wait for
/// a disconnection answer or a reconnection.
///
/// `token` is not needed: the number already lies in the frame
/// (`request_id`). `field` is needed by the socket: on KDE it holds back
/// applying a wallpaper until the wallpaper plugin is assigned
/// (`we_socket::send_request_frame`).
pub fn send(_token: i64, field: &str, request: &[u8]) {
    crate::we_socket::send_request_frame(field, request);
}

/// Give the daemon's answer to the query with this number. `false` means
/// nobody is waiting.
pub fn deliver_response_bytes(request_id: i64, frame: &[u8]) -> bool {
    let Ok(len) = i32::try_from(frame.len()) else {
        return false;
    };
    unsafe { rexon_we_do_deliver_response(request_id, frame.as_ptr().cast::<c_char>(), len) != 0 }
}

/// Hand a long task's progress snapshot to the waiting queries.
///
/// Needed by exactly one consumer — the progress of long tasks
/// (`task_progress`), to which every `ProgressQuery` is subscribed.
pub fn deliver_event_bytes(frame: &[u8]) -> usize {
    let Some((query_id, snapshot)) = rexon_core::we_manage::take_task_progress(frame) else {
        return 0;
    };
    crate::query_base::progress::dispatch(&query_id, &snapshot)
}

/// Close ALL pending queries with this frame (a disconnection).
///
/// Returns the number of queries the frame reached.
pub fn fail_pending(frame: &[u8]) -> i32 {
    let Ok(len) = i32::try_from(frame.len()) else {
        return 0;
    };
    unsafe { rexon_we_do_fail_pending(frame.as_ptr().cast::<c_char>(), len) }
}

/// How many queries are waiting for an answer. Needed by checks: an unclosed
/// count after a run means a lost answer.
pub fn pending_count() -> i32 {
    unsafe { rexon_we_do_pending_count() }
}
