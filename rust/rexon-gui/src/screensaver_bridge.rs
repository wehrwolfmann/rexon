// The "do not blank the screen" bridge — the executor of the core's
// decisions.
//
// WHAT IS WHERE. Whether the bridge is needed in this environment, where to
// forward the inhibition, cookie bookkeeping and all answers are in
// `rexon_core::screensaver_bridge`, together with the tests. Here there is
// only execution: ask the bus whether the names have owners, feed the plan
// to the shim (`cpp/rexon_screensaver_bridge.cpp`) and carry out the
// core's actions through it. All bookkeeping is under one lock, and the
// actions are executed AFTER it is released: the shim may call back in the
// same thread.
//
// Inhibiting idle through the Wayland protocol (where there is nothing to
// hold over the bus) is the `wayland_idle` module below: its own connection
// to the compositor, separate from Qt, an invisible 1×1 layer surface and
// `zwp_idle_inhibitor_v1` on it. A separate connection on purpose: Qt in
// this process may sit on X11 or on offscreen, while the inhibition must
// live exactly as long as the Rexon process: the process dies — the
// connection closes — the compositor lifts the inhibition by itself.

use std::ffi::{c_char, CStr, CString};
use std::sync::Mutex;

use rexon_core::screensaver_bridge::{self as core_ssb, Action, Book, Decision};
use rexon_core::{log_info, log_warn};

type SsbMessage = extern "C" fn(
    u64,
    *const c_char,
    *const c_char,
    *const c_char,
    *const c_char,
    *const c_char,
    *const *const c_char,
    i32,
);
type SsbCallDone = extern "C" fn(u64, bool, *const c_char);
type SsbVanished = extern "C" fn(*const c_char);
type SsbLost = extern "C" fn();

unsafe extern "C" {
    fn rexon_ssb_set_hooks(
        on_message: SsbMessage,
        on_call_done: SsbCallDone,
        on_vanished: SsbVanished,
        on_lost: SsbLost,
    );
    fn rexon_ssb_name_has_owner(name: *const c_char) -> i32;
    fn rexon_ssb_start(
        service: *const c_char,
        paths: *const *const c_char,
        count: i32,
        introspection_xml: *const c_char,
    ) -> i32;
    fn rexon_ssb_reply(token: u64, signature: *const c_char, values: *const *const c_char, count: i32);
    fn rexon_ssb_reply_error(token: u64, error_name: *const c_char, text: *const c_char);
    #[allow(clippy::too_many_arguments)]
    fn rexon_ssb_call(
        ctx: u64,
        service: *const c_char,
        path: *const c_char,
        iface: *const c_char,
        method: *const c_char,
        signature: *const c_char,
        values: *const *const c_char,
        count: i32,
        timeout_ms: i32,
    );
    fn rexon_ssb_watch(name: *const c_char);
    fn rexon_ssb_unwatch(name: *const c_char);
    fn rexon_ssb_stop(service: *const c_char);
}

const AREA: &str = "screensaver";

fn book() -> &'static Mutex<Option<Book>> {
    static BOOK: Mutex<Option<Book>> = Mutex::new(None);
    &BOOK
}

fn cstr(text: &str) -> CString {
    CString::new(text.replace('\0', "")).expect("NUL bytes were stripped")
}

fn text(ptr: *const c_char) -> String {
    if ptr.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(ptr) }.to_string_lossy().into_owned()
}

/// Array of C strings for the duration of a shim call.
struct CArgs {
    _owned: Vec<CString>,
    ptrs: Vec<*const c_char>,
}

impl CArgs {
    fn new(values: &[String]) -> Self {
        let owned: Vec<CString> = values.iter().map(|v| cstr(v)).collect();
        let ptrs = owned.iter().map(|c| c.as_ptr()).collect();
        Self { _owned: owned, ptrs }
    }
    fn ptr(&self) -> *const *const c_char {
        self.ptrs.as_ptr()
    }
    fn len(&self) -> i32 {
        self.ptrs.len() as i32
    }
}

fn has_owner(name: &str) -> Option<bool> {
    let n = cstr(name);
    match unsafe { rexon_ssb_name_has_owner(n.as_ptr()) } {
        1 => Some(true),
        0 => Some(false),
        _ => None,
    }
}

/// Execute the core's actions (the bookkeeping lock is already released).
fn run(actions: Vec<Action>) {
    for action in actions {
        match action {
            Action::Reply { token, signature, values } => {
                let sig = cstr(signature);
                let args = CArgs::new(&values);
                unsafe { rexon_ssb_reply(token, sig.as_ptr(), args.ptr(), args.len()) };
            }
            Action::ReplyError { token, name, text } => {
                let n = cstr(name);
                let t = cstr(&text);
                unsafe { rexon_ssb_reply_error(token, n.as_ptr(), t.as_ptr()) };
            }
            Action::Call { ctx, target, method, signature, values, timeout_ms } => {
                let service = cstr(target.service);
                let path = cstr(target.path);
                let iface = cstr(target.iface);
                let m = cstr(method);
                let sig = cstr(signature);
                let args = CArgs::new(&values);
                unsafe {
                    rexon_ssb_call(
                        ctx,
                        service.as_ptr(),
                        path.as_ptr(),
                        iface.as_ptr(),
                        m.as_ptr(),
                        sig.as_ptr(),
                        args.ptr(),
                        args.len(),
                        timeout_ms,
                    )
                };
            }
            Action::Watch(name) => {
                let n = cstr(&name);
                unsafe { rexon_ssb_watch(n.as_ptr()) };
                // The client may have left before the watch was set up — then there will
                // be no departure signal any more, and the inhibition would hang forever.
                if has_owner(&name) == Some(false) {
                    vanished_now(&name);
                }
            }
            Action::Unwatch(name) => {
                let n = cstr(&name);
                unsafe { rexon_ssb_unwatch(n.as_ptr()) };
            }
            Action::HoldIdle(on) => wayland_idle::set(on),
            Action::Note(line) => log_info!(AREA, "{line}"),
        }
    }
}

fn with_book(f: impl FnOnce(&mut Book) -> Vec<Action>) {
    let actions = {
        let mut guard = book().lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_mut() {
            Some(b) => f(b),
            None => Vec::new(),
        }
    };
    run(actions);
}

fn vanished_now(name: &str) {
    with_book(|b| b.on_vanished(name));
}

extern "C" fn on_message(
    token: u64,
    sender: *const c_char,
    _path: *const c_char,
    iface: *const c_char,
    member: *const c_char,
    signature: *const c_char,
    values: *const *const c_char,
    count: i32,
) {
    let args: Vec<String> = (0..count.max(0) as usize)
        .map(|i| text(unsafe { *values.add(i) }))
        .collect();
    let (sender, iface, member, signature) = (text(sender), text(iface), text(member), text(signature));
    let mut answered = false;
    with_book(|b| {
        answered = true;
        b.on_message(token, &sender, &iface, &member, &signature, &args)
    });
    if !answered {
        // There is no bookkeeping — close the receipt anyway, the client must not
        // wait.
        let n = cstr(core_ssb::ERROR_FAILED);
        let t = cstr("bridge not ready");
        unsafe { rexon_ssb_reply_error(token, n.as_ptr(), t.as_ptr()) };
    }
}

extern "C" fn on_call_done(ctx: u64, ok: bool, value: *const c_char) {
    let value = text(value);
    with_book(|b| b.on_call_done(ctx, ok, &value));
}

extern "C" fn on_vanished(name: *const c_char) {
    vanished_now(&text(name));
}

/// Another owner took the name (asked for it with replacement): release
/// everything held with the session and step down from the bus.
extern "C" fn on_lost() {
    let actions = book()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_mut()
        .map(Book::release_all)
        .unwrap_or_default();
    run(actions);
    // The bookkeeping stays: a forwarding the session has not answered yet will
    // be released on its answer (`on_call_done` sees that the entry is already
    // gone). Zeroed bookkeeping would throw that answer away, and the session's
    // cookie would hang on Rexon until the end of the session. There will be no
    // new calls — the objects are removed from the bus.
    let service = cstr(core_ssb::SERVICE);
    unsafe { rexon_ssb_stop(service.as_ptr()) };
    log_info!(AREA, "{} was taken by another owner — bridge stepped down", core_ssb::SERVICE);
}

/// Output of `mate-screensaver-command --version`; `None` means there is no
/// command or it did not answer within a few seconds (then it is killed by
/// PID).
fn mate_screensaver_version() -> Option<String> {
    let [prog, arg] = core_ssb::MATE_SCREENSAVER_VERSION_ARGV;
    let mut child = std::process::Command::new(prog)
        .arg(arg)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(50))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    let mut out = String::new();
    std::io::Read::read_to_string(&mut child.stdout.take()?, &mut out).ok()?;
    Some(out)
}

/// Decide and, if needed, set up the bridge. Waits for
/// [`core_ssb::START_DELAY`] — call from a background thread.
fn start_after_delay() {
    std::thread::sleep(core_ssb::START_DELAY);
    let xdg = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    let session = std::env::var("DESKTOP_SESSION").unwrap_or_default();
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
    let de = rexon_core::system::detect_current_de(&xdg, &session, wayland);
    let mate_version = core_ssb::wants_mate_screensaver_version(de.as_deref())
        .then(mate_screensaver_version)
        .flatten();
    let plan = match core_ssb::decide(de.as_deref(), mate_version.as_deref(), has_owner, wayland_idle::probe) {
        Decision::Start(plan) => plan,
        Decision::Skip(why) => {
            log_info!(AREA, "bridge {} not needed: {why}", core_ssb::SERVICE);
            return;
        }
    };
    *book().lock().unwrap_or_else(|e| e.into_inner()) = Some(Book::starting_at(plan, core_ssb::live_first_cookie()));
    unsafe { rexon_ssb_set_hooks(on_message, on_call_done, on_vanished, on_lost) };

    let service = cstr(core_ssb::SERVICE);
    let paths: Vec<String> = core_ssb::OBJECT_PATHS.iter().map(|p| p.to_string()).collect();
    let paths = CArgs::new(&paths);
    let xml = cstr(core_ssb::INTROSPECTION_XML);
    let code = unsafe { rexon_ssb_start(service.as_ptr(), paths.ptr(), paths.len(), xml.as_ptr()) };
    if code == 0 {
        log_info!(
            AREA,
            "{} owned by Rexon: inhibitions forwarded to {}{}",
            core_ssb::SERVICE,
            plan.upstream.service,
            plan.companion
                .map(|c| format!(", screensaver — {}", c.service))
                .unwrap_or_default()
        );
    } else {
        *book().lock().unwrap_or_else(|e| e.into_inner()) = None;
        log_warn!(AREA, "bridge {} failed to start (code {code})", core_ssb::SERVICE);
    }
}

/// Set up the "do not blank the screen" bridge in the background — once per
/// process.
///
/// Call from a process that lives for the whole session (the main window
/// with the tray icon): inhibitions are counted against its bus connection.
pub fn spawn_once() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let spawned = std::thread::Builder::new()
            .name("screensaver-bridge".into())
            .spawn(start_after_delay);
        if let Err(e) = spawned {
            log_warn!(AREA, "bridge {} not started: {e}", core_ssb::SERVICE);
        }
    });
}

/// Inhibiting idle through the Wayland protocol: the "hold / release"
/// decision is the core's ([`Action::HoldIdle`]); here there is only the
/// connection and the surface.
pub mod wayland_idle {
    use std::os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd};
    use std::sync::{Arc, Mutex};
    use std::thread::JoinHandle;

    use rexon_core::screensaver_bridge::WaylandFacts;
    use rexon_core::{log_info, log_warn};
    use wayland_client::globals::{registry_queue_init, GlobalListContents};
    use wayland_client::protocol::{
        wl_buffer, wl_compositor, wl_region, wl_registry, wl_shm, wl_shm_pool, wl_surface,
    };
    use wayland_client::{delegate_noop, Connection, Dispatch, EventQueue, QueueHandle};
    use wayland_protocols::wp::idle_inhibit::zv1::client::{
        zwp_idle_inhibit_manager_v1, zwp_idle_inhibitor_v1,
    };
    use wayland_protocols_wlr::layer_shell::v1::client::{zwlr_layer_shell_v1, zwlr_layer_surface_v1};

    use super::AREA;

    /// Namespace of the layer surface: it is visible by it in
    /// `swaymsg -t get_outputs`/`hyprctl layers` and in the compositor log.
    pub const LAYER_NAMESPACE: &str = "rexon-idle-inhibit";

    #[derive(Default)]
    struct State {
        configured: bool,
        needs_commit: bool,
        closed: bool,
    }

    impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
        fn event(
            _: &mut Self,
            _: &wl_registry::WlRegistry,
            _: wl_registry::Event,
            _: &GlobalListContents,
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
        }
    }

    impl Dispatch<zwlr_layer_surface_v1::ZwlrLayerSurfaceV1, ()> for State {
        fn event(
            state: &mut Self,
            layer: &zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
            event: zwlr_layer_surface_v1::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            match event {
                zwlr_layer_surface_v1::Event::Configure { serial, .. } => {
                    layer.ack_configure(serial);
                    state.configured = true;
                    state.needs_commit = true;
                }
                zwlr_layer_surface_v1::Event::Closed => state.closed = true,
                _ => {}
            }
        }
    }

    delegate_noop!(State: wl_compositor::WlCompositor);
    delegate_noop!(State: wl_region::WlRegion);
    delegate_noop!(State: wl_shm_pool::WlShmPool);
    delegate_noop!(State: ignore wl_shm::WlShm);
    delegate_noop!(State: ignore wl_buffer::WlBuffer);
    delegate_noop!(State: ignore wl_surface::WlSurface);
    delegate_noop!(State: zwlr_layer_shell_v1::ZwlrLayerShellV1);
    delegate_noop!(State: zwp_idle_inhibit_manager_v1::ZwpIdleInhibitManagerV1);
    delegate_noop!(State: zwp_idle_inhibitor_v1::ZwpIdleInhibitorV1);

    /// Name of the process on the other end of the Wayland socket
    /// (`/proc/PID/comm`).
    fn peer_comm(conn: &Connection) -> Option<String> {
        let backend = conn.backend();
        let fd = backend.poll_fd();
        let mut cred = libc::ucred { pid: 0, uid: 0, gid: 0 };
        let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
        let rc = unsafe {
            libc::getsockopt(
                fd.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                (&mut cred as *mut libc::ucred).cast(),
                &mut len,
            )
        };
        if rc != 0 || cred.pid <= 0 {
            return None;
        }
        let comm = std::fs::read_to_string(format!("/proc/{}/comm", cred.pid)).ok()?;
        Some(comm.trim().to_string())
    }

    /// Which compositor it is and which globals it announced; `None` means the
    /// session is not Wayland or connecting failed.
    pub fn probe() -> Option<WaylandFacts> {
        std::env::var_os("WAYLAND_DISPLAY")?;
        let conn = Connection::connect_to_env().ok()?;
        let (globals, _queue) = registry_queue_init::<State>(&conn).ok()?;
        let names = globals.contents().clone_list().into_iter().map(|g| g.interface).collect();
        Some(WaylandFacts { compositor: peer_comm(&conn).unwrap_or_default(), globals: names })
    }

    struct Hold {
        stop: Arc<OwnedFd>,
        thread: JoinHandle<()>,
    }

    fn hold() -> &'static Mutex<Option<Hold>> {
        static HOLD: Mutex<Option<Hold>> = Mutex::new(None);
        &HOLD
    }

    /// Set (`true`) or lift (`false`) the inhibition. A repeat does nothing.
    pub fn set(on: bool) {
        let mut guard = hold().lock().unwrap_or_else(|e| e.into_inner());
        if on {
            if guard.is_some() {
                return;
            }
            let raw = unsafe { libc::eventfd(0, libc::EFD_CLOEXEC | libc::EFD_NONBLOCK) };
            if raw < 0 {
                log_warn!(AREA, "Wayland idle inhibitor not set: eventfd {}", std::io::Error::last_os_error());
                return;
            }
            let stop = Arc::new(unsafe { OwnedFd::from_raw_fd(raw) });
            let thread_stop = Arc::clone(&stop);
            let spawned = std::thread::Builder::new().name("idle-inhibit".into()).spawn(move || {
                match run(&thread_stop) {
                    Ok(()) => log_info!(AREA, "Wayland idle inhibitor released"),
                    Err(e) => log_warn!(AREA, "Wayland idle inhibitor broke off: {e}"),
                }
            });
            match spawned {
                Ok(thread) => *guard = Some(Hold { stop, thread }),
                Err(e) => log_warn!(AREA, "Wayland idle inhibitor not set: {e}"),
            }
        } else if let Some(h) = guard.take() {
            let one: u64 = 1;
            unsafe { libc::write(h.stop.as_raw_fd(), (&one as *const u64).cast(), 8) };
            let _ = h.thread.join();
        }
    }

    /// Connect, set up the surface and the inhibition, hold until the `stop`
    /// signal. Closing the connection on exit removes everything at once.
    fn run(stop: &OwnedFd) -> Result<(), String> {
        let conn = Connection::connect_to_env().map_err(|e| format!("connect: {e}"))?;
        let (globals, mut queue) =
            registry_queue_init::<State>(&conn).map_err(|e| format!("registry: {e}"))?;
        let qh = queue.handle();
        let bind_err = |what: &str, e: wayland_client::globals::BindError| format!("{what}: {e}");
        let compositor: wl_compositor::WlCompositor =
            globals.bind(&qh, 1..=4, ()).map_err(|e| bind_err("wl_compositor", e))?;
        let shm: wl_shm::WlShm = globals.bind(&qh, 1..=1, ()).map_err(|e| bind_err("wl_shm", e))?;
        let layer_shell: zwlr_layer_shell_v1::ZwlrLayerShellV1 =
            globals.bind(&qh, 1..=4, ()).map_err(|e| bind_err("zwlr_layer_shell_v1", e))?;
        let manager: zwp_idle_inhibit_manager_v1::ZwpIdleInhibitManagerV1 =
            globals.bind(&qh, 1..=1, ()).map_err(|e| bind_err("zwp_idle_inhibit_manager_v1", e))?;

        // One transparent pixel: a surface without a buffer is not "mapped", and
        // sway does not count such an inhibition.
        let memfd = unsafe { libc::memfd_create(c"rexon-idle-inhibit".as_ptr(), libc::MFD_CLOEXEC) };
        if memfd < 0 {
            return Err(format!("memfd: {}", std::io::Error::last_os_error()));
        }
        let memfd = unsafe { OwnedFd::from_raw_fd(memfd) };
        if unsafe { libc::ftruncate(memfd.as_raw_fd(), 4) } != 0 {
            return Err(format!("memfd: {}", std::io::Error::last_os_error()));
        }
        let pool = shm.create_pool(memfd.as_fd(), 4, &qh, ());
        let buffer = pool.create_buffer(0, 1, 1, 4, wl_shm::Format::Argb8888, &qh, ());

        loop {
            let surface = compositor.create_surface(&qh, ());
            // An empty input region: the pixel does not intercept clicks.
            let region = compositor.create_region(&qh, ());
            surface.set_input_region(Some(&region));
            region.destroy();
            let layer = layer_shell.get_layer_surface(
                &surface,
                None,
                zwlr_layer_shell_v1::Layer::Background,
                LAYER_NAMESPACE.into(),
                &qh,
                (),
            );
            layer.set_size(1, 1);
            layer.set_anchor(zwlr_layer_surface_v1::Anchor::Top | zwlr_layer_surface_v1::Anchor::Left);
            surface.commit();
            let mut state = State::default();
            let mut inhibitor = None;
            let stopped = pump(&conn, &mut queue, &mut state, stop, |state| {
                if state.needs_commit {
                    state.needs_commit = false;
                    surface.attach(Some(&buffer), 0, 0);
                    surface.damage(0, 0, 1, 1);
                    surface.commit();
                }
                // Inhibition only on a mapped surface: sway checks "is it mapped" at the
                // moment the inhibitor appears and does not re-check when the layer is
                // mapped (sway/desktop/idle_inhibit_v1.c).
                if state.configured && inhibitor.is_none() {
                    inhibitor = Some(manager.create_inhibitor(&surface, &qh, ()));
                    log_info!(AREA, "Wayland idle inhibitor set");
                }
            })?;
            if let Some(i) = inhibitor {
                i.destroy();
            }
            layer.destroy();
            surface.destroy();
            if stopped {
                let _ = conn.flush();
                return Ok(());
            }
            // The output the surface was on has disappeared — set it up again.
            log_info!(AREA, "inhibitor layer surface closed by the compositor — setting it up again");
        }
    }

    /// Spin the queue until `stop` (`Ok(true)`) or until the surface is closed
    /// (`Ok(false)`).
    fn pump(
        conn: &Connection,
        queue: &mut EventQueue<State>,
        state: &mut State,
        stop: &OwnedFd,
        mut after_dispatch: impl FnMut(&mut State),
    ) -> Result<bool, String> {
        loop {
            queue.dispatch_pending(state).map_err(|e| format!("queue: {e}"))?;
            after_dispatch(state);
            if state.closed {
                return Ok(false);
            }
            conn.flush().map_err(|e| format!("flush: {e}"))?;
            let Some(guard) = queue.prepare_read() else {
                continue;
            };
            let mut fds = [
                libc::pollfd { fd: guard.connection_fd().as_raw_fd(), events: libc::POLLIN, revents: 0 },
                libc::pollfd { fd: stop.as_raw_fd(), events: libc::POLLIN, revents: 0 },
            ];
            let rc = unsafe { libc::poll(fds.as_mut_ptr(), 2, -1) };
            if rc < 0 {
                let err = std::io::Error::last_os_error();
                if err.kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(format!("poll: {err}"));
            }
            if fds[1].revents != 0 {
                return Ok(true);
            }
            if fds[0].revents & (libc::POLLERR | libc::POLLHUP) != 0 {
                return Err("the compositor closed the connection".into());
            }
            if fds[0].revents & libc::POLLIN != 0 {
                match guard.read() {
                    Ok(_) => {}
                    Err(wayland_client::backend::WaylandError::Io(e))
                        if e.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(e) => return Err(format!("read: {e}")),
                }
            }
        }
    }
}
