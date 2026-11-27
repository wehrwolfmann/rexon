// Rust wrappers over the WE tab's singleton shim (cpp/rexon_we_singletons.cpp):
// daemon events to the live Notify/App singletons, a transport error as a
// window toast, the daemon launcher, the QML network cache and the engine
// for language changes.
//
// Only the crate itself calls them (we_socket.rs, we_env.rs,
// status_queries.rs): there are no exported C symbols — the former Python
// host of the tab, for whose sake they existed, is in the archive. Why the
// shim itself is in C++ is explained in its header's preamble.
//
// This file is NOT a cxx-qt bridge: ordinary `extern "C"`, moc has no work
// here.

use std::ffi::{c_char, c_void};

/// The "launch the daemon" callback (for Rexon — `we_socket::launch_daemon_hook`).
///
/// The Rexon launcher knows more than the original: it looks for the binary
/// where the tab itself installs it, and on KDE it finishes the display
/// plugin. Without the callback the shim falls back to the original's branch
/// — `QProcess::startDetached("waywallen")`.
pub type LaunchHook = Option<extern "C" fn() -> bool>;

unsafe extern "C" {
    fn rexon_we_do_deliver_event(data: *const c_char, len: i32) -> i32;
    fn rexon_we_do_transport_disconnected() -> i32;
    fn rexon_we_do_client_error(message: *const c_char, len: i32) -> i32;
    fn rexon_we_do_renderers_replace_frame(frame: *const c_char, len: i32) -> i32;
    fn rexon_we_do_set_launch_hook(hook: LaunchHook);
    fn rexon_we_do_set_network_cache(dir: *const c_char, max_bytes: i64);
    fn rexon_we_install_network_cache(engine: *mut c_void);
    fn rexon_we_do_register_qml_engine(engine: *mut c_void);
}

/// Hand a raw `ServerFrame` frame from the daemon to the singletons.
///
/// Returns how many singletons the frame reached RIGHT NOW. Zero is not a
/// loss: a frame for a not yet built singleton goes into the replay cache and
/// will be delivered to it on registration (`cpp/rexon_we_singletons.cpp`).
pub fn deliver_singleton_event_bytes(frame: &[u8]) -> i32 {
    let Ok(len) = i32::try_from(frame.len()) else {
        return 0;
    };
    unsafe { rexon_we_do_deliver_event(frame.as_ptr().cast::<c_char>(), len) }
}

/// WebSocket disconnection: `Notify` goes back to the pessimistic state.
pub fn transport_disconnected() -> i32 {
    unsafe { rexon_we_do_transport_disconnected() }
}

/// Transport error → the `App.errorOccurred` signal (a toast of the tab
/// window).
///
/// Returns whether it got through: `false` means there is no `App`
/// singleton yet (the tab was not opened), and there is nobody to show the
/// error.
pub fn client_error(message: &str) -> bool {
    let Ok(len) = i32::try_from(message.len()) else {
        return false;
    };
    unsafe { rexon_we_do_client_error(message.as_ptr().cast::<c_char>(), len) != 0 }
}

/// A full replacement of the renderer list with the `renderer_list` answer.
///
/// `frame` is the RAW daemon frame, the same one the query parsed. The door
/// is internal (there is no C symbol outside): `RendererListQuery` lives in
/// another bridge of the same crate, and it can reach the live models of
/// `App` only through the shim — in the original this query and the manager
/// lie in one C++ object graph.
///
/// Returns whether the frame got through: `false` means there is no `App`
/// singleton yet.
pub fn renderers_replace_frame(frame: &[u8]) -> bool {
    if frame.is_empty() || i32::try_from(frame.len()).is_err() {
        return false;
    }
    let ok = unsafe {
        rexon_we_do_renderers_replace_frame(frame.as_ptr().cast::<c_char>(), frame.len() as i32)
    };
    ok != 0
}

/// Set the daemon launcher. `None` removes it.
pub fn set_launch_hook(hook: LaunchHook) {
    unsafe { rexon_we_do_set_launch_hook(hook) }
}

/// Set the directory and limit of the QML picture network cache (before
/// installing the factory).
///
/// # Safety
/// `dir` is a live null-terminated C string.
pub unsafe fn set_network_cache(dir: *const c_char, max_bytes: i64) {
    if dir.is_null() {
        return;
    }
    unsafe { rexon_we_do_set_network_cache(dir, max_bytes) }
}

/// Give the QML engine a network manager factory with a disk cache.
///
/// The cast to QQmlEngine inside the shim is checked, it will not accept
/// someone else's object.
///
/// # Safety
/// `engine` is a live QObject.
pub unsafe fn install_qml_network_cache(engine: *mut c_void) {
    if engine.is_null() {
        return;
    }
    unsafe { rexon_we_install_network_cache(engine) }
}

/// Record the tab's QML engine for changing the language on the fly.
///
/// The «Language» combo box on the vendored settings page changes the
/// translation catalogue, while the `qsTr()` bindings of an already shown
/// page are re-evaluated by the engine itself (`QQmlEngine::retranslate`).
/// Without this record the choice would be remembered, but the page would
/// stay in the previous language until a restart.
///
/// # Safety
/// `engine` is a live QObject.
pub unsafe fn register_qml_engine(engine: *mut c_void) {
    if engine.is_null() {
        return;
    }
    unsafe { rexon_we_do_register_qml_engine(engine) }
}
