// Мост «не гасить экран» — исполнитель решений ядра.
//
// ЧТО ГДЕ. Нужен ли мост в этом окружении, куда пробрасывать удержание, учёт
// куки и все ответы — `rexon_core::screensaver_bridge`, там же тесты. Здесь
// только исполнение: спросить шину, есть ли у имён хозяева, скормить план
// шиму (`cpp/rexon_screensaver_bridge.cpp`) и разносить по нему действия
// ядра. Весь учёт — под одним замком, действия исполняются ПОСЛЕ его снятия:
// шим может звать обратно в этом же потоке.
//
// Запрет простоя протоколом Wayland (там, где по шине держать нечем) — модуль
// `wayland_idle` ниже: своё подключение к композитору, отдельное от Qt,
// невидимая слой-поверхность 1×1 и `zwp_idle_inhibitor_v1` на ней. Отдельное
// подключение — намеренно: Qt в этом процессе может сидеть и на X11, и на
// offscreen, а запрет обязан жить ровно столько, сколько процесс Rexon:
// умер процесс — закрылось подключение — композитор снял запрет сам.

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
    CString::new(text.replace('\0', "")).expect("нули вычищены")
}

fn text(ptr: *const c_char) -> String {
    if ptr.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(ptr) }.to_string_lossy().into_owned()
}

/// Массив C-строк на время вызова шима.
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

/// Исполнить действия ядра (замок учёта уже снят).
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
                // Клиент мог уйти раньше, чем встала слежка, — тогда сигнала
                // об уходе уже не будет, и удержание повисло бы навсегда.
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
        // Учёта нет — квитанцию всё равно закрыть, клиент не должен ждать.
        let n = cstr(core_ssb::ERROR_FAILED);
        let t = cstr("мост не готов");
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

/// Имя забрал другой хозяин (попросил с заменой): снять у сеанса всё, что
/// держали, и сойти с шины.
extern "C" fn on_lost() {
    let actions = book()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_mut()
        .map(Book::release_all)
        .unwrap_or_default();
    run(actions);
    // Учёт остаётся: проброс, на который сеанс ещё не ответил, снимется по
    // его ответу (`on_call_done` видит, что записи уже нет). Обнулённый
    // учёт выбросил бы этот ответ, и куки сеанса висела бы за Rexon до
    // конца сеанса. Новых вызовов не будет — объекты с шины сняты.
    let service = cstr(core_ssb::SERVICE);
    unsafe { rexon_ssb_stop(service.as_ptr()) };
    log_info!(AREA, "{} забрал другой хозяин — мост сошёл", core_ssb::SERVICE);
}

/// Вывод `mate-screensaver-command --version`; `None` — команды нет или она
/// не ответила за несколько секунд (тогда её гасим по PID).
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

/// Решить и, если надо, поставить мост. Ждёт [`core_ssb::START_DELAY`] —
/// звать из фонового потока.
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
            log_info!(AREA, "мост {} не нужен: {why}", core_ssb::SERVICE);
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
            "{} занято Rexon: удержания идут в {}{}",
            core_ssb::SERVICE,
            plan.upstream.service,
            plan.companion
                .map(|c| format!(", хранитель экрана — {}", c.service))
                .unwrap_or_default()
        );
    } else {
        *book().lock().unwrap_or_else(|e| e.into_inner()) = None;
        log_warn!(AREA, "мост {} не встал (код {code})", core_ssb::SERVICE);
    }
}

/// Поставить мост «не гасить экран» в фоне — один раз за процесс.
///
/// Звать из процесса, который живёт весь сеанс (главное окно со значком в
/// лотке): удержания числятся за его подключением к шине.
pub fn spawn_once() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let spawned = std::thread::Builder::new()
            .name("screensaver-bridge".into())
            .spawn(start_after_delay);
        if let Err(e) = spawned {
            log_warn!(AREA, "мост {} не запущен: {e}", core_ssb::SERVICE);
        }
    });
}

/// Запрет простоя протоколом Wayland: решение «держать / отпустить» — ядро
/// ([`Action::HoldIdle`]), здесь только подключение и поверхность.
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

    /// Пространство имён слой-поверхности: по нему её видно в
    /// `swaymsg -t get_outputs`/`hyprctl layers` и в журнале композитора.
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

    /// Имя процесса на том конце сокета Wayland (`/proc/PID/comm`).
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

    /// Что за композитор и какие глобалы он объявил; `None` — сеанс не
    /// Wayland или подключиться не вышло.
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

    /// Поставить (`true`) или снять (`false`) запрет. Повтор — без действия.
    pub fn set(on: bool) {
        let mut guard = hold().lock().unwrap_or_else(|e| e.into_inner());
        if on {
            if guard.is_some() {
                return;
            }
            let raw = unsafe { libc::eventfd(0, libc::EFD_CLOEXEC | libc::EFD_NONBLOCK) };
            if raw < 0 {
                log_warn!(AREA, "запрет простоя Wayland не поставлен: eventfd {}", std::io::Error::last_os_error());
                return;
            }
            let stop = Arc::new(unsafe { OwnedFd::from_raw_fd(raw) });
            let thread_stop = Arc::clone(&stop);
            let spawned = std::thread::Builder::new().name("idle-inhibit".into()).spawn(move || {
                match run(&thread_stop) {
                    Ok(()) => log_info!(AREA, "запрет простоя Wayland снят"),
                    Err(e) => log_warn!(AREA, "запрет простоя Wayland оборвался: {e}"),
                }
            });
            match spawned {
                Ok(thread) => *guard = Some(Hold { stop, thread }),
                Err(e) => log_warn!(AREA, "запрет простоя Wayland не поставлен: {e}"),
            }
        } else if let Some(h) = guard.take() {
            let one: u64 = 1;
            unsafe { libc::write(h.stop.as_raw_fd(), (&one as *const u64).cast(), 8) };
            let _ = h.thread.join();
        }
    }

    /// Подключиться, поставить поверхность и запрет, держать до сигнала
    /// `stop`. Закрытие подключения при выходе снимает всё разом.
    fn run(stop: &OwnedFd) -> Result<(), String> {
        let conn = Connection::connect_to_env().map_err(|e| format!("подключение: {e}"))?;
        let (globals, mut queue) =
            registry_queue_init::<State>(&conn).map_err(|e| format!("реестр: {e}"))?;
        let qh = queue.handle();
        let bind_err = |what: &str, e: wayland_client::globals::BindError| format!("{what}: {e}");
        let compositor: wl_compositor::WlCompositor =
            globals.bind(&qh, 1..=4, ()).map_err(|e| bind_err("wl_compositor", e))?;
        let shm: wl_shm::WlShm = globals.bind(&qh, 1..=1, ()).map_err(|e| bind_err("wl_shm", e))?;
        let layer_shell: zwlr_layer_shell_v1::ZwlrLayerShellV1 =
            globals.bind(&qh, 1..=4, ()).map_err(|e| bind_err("zwlr_layer_shell_v1", e))?;
        let manager: zwp_idle_inhibit_manager_v1::ZwpIdleInhibitManagerV1 =
            globals.bind(&qh, 1..=1, ()).map_err(|e| bind_err("zwp_idle_inhibit_manager_v1", e))?;

        // Один прозрачный пиксель: поверхность без буфера не «отображена», и
        // sway такой запрет не считает.
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
            // Пустая область ввода: пиксель не перехватывает щелчков.
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
                // Запрет — только на отображённую поверхность: sway проверяет
                // «отображена ли» в миг появления запрета и при отображении
                // слоя не перепроверяет (sway/desktop/idle_inhibit_v1.c).
                if state.configured && inhibitor.is_none() {
                    inhibitor = Some(manager.create_inhibitor(&surface, &qh, ()));
                    log_info!(AREA, "запрет простоя Wayland поставлен");
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
            // Выход, на котором стояла поверхность, исчез — встать заново.
            log_info!(AREA, "слой-поверхность запрета закрыта композитором — ставлю заново");
        }
    }

    /// Крутить очередь до `stop` (`Ok(true)`) или закрытия поверхности
    /// (`Ok(false)`).
    fn pump(
        conn: &Connection,
        queue: &mut EventQueue<State>,
        state: &mut State,
        stop: &OwnedFd,
        mut after_dispatch: impl FnMut(&mut State),
    ) -> Result<bool, String> {
        loop {
            queue.dispatch_pending(state).map_err(|e| format!("очередь: {e}"))?;
            after_dispatch(state);
            if state.closed {
                return Ok(false);
            }
            conn.flush().map_err(|e| format!("отправка: {e}"))?;
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
                return Err("композитор закрыл подключение".into());
            }
            if fds[0].revents & libc::POLLIN != 0 {
                match guard.read() {
                    Ok(_) => {}
                    Err(wayland_client::backend::WaylandError::Io(e))
                        if e.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(e) => return Err(format!("чтение: {e}")),
                }
            }
        }
    }
}
