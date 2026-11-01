//! Живой исполнитель видеозаставки входа — реализация трейта
//! [`crate::session_splash::SplashIo`] на Rust.
//!
//! Ядро (`rexon-core::session_splash`) ведёт ВЕСЬ поток заставки: скан
//! заставок Steam, кадр витрины, конфиг/избранное/ротацию, гибрид KSplash,
//! правило KWin, автозапуск и все три пути прогона вместе со всеми
//! ожиданиями и их порогами. Здесь — только исполнение того, что ядро
//! решило: файловая система, подпроцессы (mpv/ffmpeg/systemctl/
//! kwriteconfig6), unix-сокеты mpv-IPC и sd_notify, flock замка «один
//! прогон на сессию», собственный X11-слой (override-redirect окно во весь
//! экран, постер фоном, событийный raise-цикл в своём потоке) и журнал.
//!
//! До 2026-08-23 этот слой жил в `rexon/session_splash.py` (ctypes+libX11,
//! `threading`, `subprocess`, `socket`, `fcntl`, Pillow). Перенесён по
//! правилу #16 целиком; в Python остался тонкий фасад, который создаёт
//! исполнителя (`rexon_py.SplashIo`) и передаёт его в потоки ядра.
//!
//! Что изменилось при переносе (правило #16 — баги не консервируем):
//!   * остановка raise-цикла БУДИТ поток трубой пробуждения, а не ждёт
//!     истечения страховочного шага: раньше `join(timeout=1.0)` мог
//!     вернуться раньше, чем поток дошёл до проверки признака, и слой
//!     закрывался у него под руками (XRaiseWindow по уничтоженному окну);
//!   * сроки, которые в шиме были своими числами (2 c на сокетах, 1 c на
//!     ожидании потока), переехали в ядро — `IPC_SOCK_TIMEOUT_SEC` и
//!     `RAISE_JOIN_WAIT_SEC`; параллельных копий значений не осталось;
//!   * отсоединённые процессы (тест из вкладки) собираются в общий список
//!     и подбираются при следующем запуске — раньше в долгоживущем
//!     UI-процессе после каждого теста оставался зомби.
//!
//! Допущения исполнителя (как у `LiveImpIo` импортёров): порядок `listdir`
//! — отсортированный (у Python порядок ФС не определён); отсутствие бинаря
//! и таймаут команды — `None`, то есть тот же безопасный дефолт, что давал
//! Python на `OSError`/`TimeoutExpired`.
//!
//! Сверка: `rust/rexon-core/tests/session_splash_parity.rs` (реплей журнала
//! исполнителя) + `rust/rexon-py/tests/session_splash_roundtrip.py` (живой
//! модуль на записанном эталоне, side-effects подменяются обёрткой поверх
//! ЭТОГО исполнителя).
//!
//! ⚠️ ПОЧЕМУ ИСПОЛНИТЕЛЬ ЖИВЁТ В ЯДРЕ, А НЕ В МОСТЕ. До 2026-09-08 он лежал в
//! `rexon-py/src/splash_io.rs`, то есть внутри крейта-`cdylib` для Python. Из
//! Rust такой крейт не подключается вовсе, и вкладке «Steam» на Rust
//! (`rexon-gui::steam_actions`) заставка была недоступна: её кнопки пришлось
//! бы либо оставить мёртвыми, либо завести ВТОРУЮ копию исполнителя (правило
//! #12 — нельзя). Модуль перенесён сюда целиком, ни строки логики не изменив;
//! в мосте остался только `#[pyclass]`-переходник `rexon_py.SplashIo` поверх
//! этого же типа.
//!
//! Единственное, что при переезде пришлось развязать, — журнал: писать в
//! `logging` Python ядро не может и не должно. Уровень с сообщением уходят в
//! СМЕНЯЕМЫЙ приёмник ([`set_log_sink`]); мост ставит свой и продолжает писать
//! в `rexon.session_splash`, а Rust-процессы получают умолчание — стандартный
//! поток ошибок.

use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::session_splash as core_ss;
use crate::session_splash::SplashIo as _;
use crate::utils as core_utils;

// ============================================================================
// Общие мелочи: время, PATH, пути, запуск команд
// ============================================================================

/// Точка отсчёта `monotonic()` — одна на процесс, как у `time.monotonic`.
fn mono_epoch() -> Instant {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    *EPOCH.get_or_init(Instant::now)
}

/// Приёмник журнала заставки: `(уровень, сообщение)`.
///
/// Ставится один раз на процесс. Мост в Python подставляет сюда запись в
/// `logging.getLogger("rexon.session_splash")` — тот же логгер, что был у
/// фасада; Rust-процессы (вкладка «Steam», бинарь заставки) обходятся
/// умолчанием.
type LogSink = Box<dyn Fn(&str, &str) + Send + Sync>;

static LOG_SINK: OnceLock<LogSink> = OnceLock::new();

/// Задать приёмник журнала. Второй вызов ничего не меняет: приёмник у процесса
/// один, и «последний победил» прятал бы строки заставки от того, кто поставил
/// свой раньше.
pub fn set_log_sink(sink: impl Fn(&str, &str) + Send + Sync + 'static) {
    let _ = LOG_SINK.set(Box::new(sink));
}

/// Строка в журнал заставки.
///
/// Уровень неизвестен → `debug` (как `getattr(log, level, log.debug)`).
///
/// Без своего приёмника строка уходит в общий журнал Rexon (`applog`,
/// область `splash`). ⚠️ `debug` там пишется только при `REXON_LOG=debug`:
/// заставка пишет отладку на каждый шаг ожидания, и запуск ролика из
/// вкладки залил бы журнал сотнями строк.
fn log_line(level: &str, message: &str) {
    let level = match level {
        "warning" | "info" | "error" | "critical" | "debug" => level,
        _ => "debug",
    };
    if let Some(sink) = LOG_SINK.get() {
        sink(level, message);
        return;
    }
    // Отладка уходит в журнал только при `REXON_LOG=debug` — уровень
    // отсекает её сам, поток ошибок больше не заливается.
    crate::applog::write(crate::applog::level_from_name(level), "splash", message);
}

/// Исполняемый ли файл (`os.access(path, os.X_OK)`).
fn is_exec(path: &Path) -> bool {
    path.metadata()
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

/// Порт `shutil.which`: имя со слэшем — проверка на месте, иначе перебор
/// `PATH`. Общий резолвер `utils::resolve_tool` здесь НЕ годится — он знает
/// синонимы (`convert`→`magick`) и лишние каталоги, а оригинал заставки
/// смотрел ровно `PATH`.
fn which_path(name: &str) -> String {
    if name.is_empty() {
        return String::new();
    }
    if name.contains('/') {
        let p = Path::new(name);
        return if is_exec(p) { name.to_string() } else { String::new() };
    }
    let path = std::env::var("PATH").unwrap_or_default();
    for dir in path.split(':') {
        if dir.is_empty() {
            continue;
        }
        let cand = Path::new(dir).join(name);
        if is_exec(&cand) {
            return cand.to_string_lossy().into_owned();
        }
    }
    String::new()
}

/// Порт `os.path.abspath`: приклеить рабочий каталог и свернуть `.`/`..`
/// БЕЗ обращения к файловой системе.
fn abspath(path: &str) -> String {
    let p = Path::new(path);
    let joined = if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("/"))
            .join(p)
    };
    let mut out = PathBuf::from("/");
    for c in joined.components() {
        match c {
            Component::Prefix(_) | Component::RootDir | Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            Component::Normal(s) => out.push(s),
        }
    }
    out.to_string_lossy().into_owned()
}

/// Порт `os.path.realpath`: символические ссылки разворачиваются, а
/// несуществующие звенья остаются как есть (`canonicalize` на них падает, и
/// поиск каталога Steam по несуществующим кандидатам сломался бы).
fn realpath(path: &str) -> String {
    let start = abspath(path);
    let mut out = PathBuf::from("/");
    let mut hops = 0u32;
    for comp in Path::new(&start).components() {
        let Component::Normal(name) = comp else { continue };
        out.push(name);
        // Разворачиваем цепочку ссылок этого звена (потолок — как у ядра ОС).
        while hops < 40 {
            let Ok(meta) = std::fs::symlink_metadata(&out) else { break };
            if !meta.file_type().is_symlink() {
                break;
            }
            let Ok(target) = std::fs::read_link(&out) else { break };
            hops += 1;
            if target.is_absolute() {
                out = target;
            } else {
                out.pop();
                for c in target.components() {
                    match c {
                        Component::ParentDir => {
                            out.pop();
                        }
                        Component::Normal(s) => out.push(s),
                        _ => {}
                    }
                }
            }
        }
    }
    out.to_string_lossy().into_owned()
}

/// Порт `subprocess.run(capture_output=True, text=True, timeout=…)`:
/// `(rc, stdout, stderr)` либо `None` на любой неудаче (нет бинаря,
/// таймаут) — тот же безопасный дефолт, что давал Python на `OSError`.
pub fn run_capture(argv: &[String], timeout: f64) -> Option<core_ss::RunOut> {
    let (bin, rest) = argv.split_first()?;
    let mut child = Command::new(bin)
        .args(rest)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    let mut op = child.stdout.take();
    let mut ep = child.stderr.take();
    // Пайпы вычитываются в своих потоках: команда, напечатавшая больше
    // ёмкости пайпа, иначе встала бы навсегда (и таймаут её не спас бы —
    // он ждёт выхода, а выйти она не может).
    let to = std::thread::spawn(move || {
        let mut b = Vec::new();
        if let Some(p) = op.as_mut() {
            let _ = p.read_to_end(&mut b);
        }
        b
    });
    let te = std::thread::spawn(move || {
        let mut b = Vec::new();
        if let Some(p) = ep.as_mut() {
            let _ = p.read_to_end(&mut b);
        }
        b
    });
    let deadline = Instant::now() + Duration::from_secs_f64(timeout.max(0.0));
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break Some(st),
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(_) => break None,
        }
    };
    let stdout = to.join().unwrap_or_default();
    let stderr = te.join().unwrap_or_default();
    let st = status?;
    Some(core_ss::RunOut {
        rc: st.code().unwrap_or(-1) as i64,
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
    })
}

// ============================================================================
// unix-сокеты: mpv-IPC и sd_notify
// ============================================================================

/// Соединение с unix-сокетом по пути ИЛИ в abstract namespace (адрес,
/// начинающийся с NUL, — так systemd отдаёт `NOTIFY_SOCKET=@…`; ни
/// `UnixStream::connect`, ни `UnixDatagram::connect` такого не умеют).
///
/// `dgram` — датаграмма (sd_notify) вместо потока (mpv-IPC).
fn connect_unix(addr: &str, dgram: bool, timeout: f64) -> Option<UnixSock> {
    let bytes = addr.as_bytes();
    let mut sa: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    sa.sun_family = libc::AF_UNIX as libc::sa_family_t;
    if bytes.len() >= std::mem::size_of_val(&sa.sun_path) {
        return None;
    }
    for (i, b) in bytes.iter().enumerate() {
        sa.sun_path[i] = *b as libc::c_char;
    }
    let len = (std::mem::size_of::<libc::sa_family_t>() + bytes.len()) as libc::socklen_t;
    let kind = if dgram { libc::SOCK_DGRAM } else { libc::SOCK_STREAM };
    let fd = unsafe { libc::socket(libc::AF_UNIX, kind | libc::SOCK_CLOEXEC, 0) };
    if fd < 0 {
        return None;
    }
    let sock = UnixSock(fd);
    let tv = libc::timeval {
        tv_sec: timeout as libc::time_t,
        tv_usec: ((timeout - timeout.trunc()) * 1e6) as libc::suseconds_t,
    };
    for opt in [libc::SO_RCVTIMEO, libc::SO_SNDTIMEO] {
        unsafe {
            libc::setsockopt(
                fd,
                libc::SOL_SOCKET,
                opt,
                std::ptr::addr_of!(tv).cast(),
                std::mem::size_of_val(&tv) as libc::socklen_t,
            );
        }
    }
    let rc = unsafe {
        libc::connect(
            fd,
            std::ptr::addr_of!(sa).cast::<libc::sockaddr>(),
            len,
        )
    };
    if rc < 0 {
        return None;
    }
    Some(sock)
}

/// Дескриптор сокета, закрывающийся сам (`with socket.socket(...)`).
struct UnixSock(libc::c_int);

impl UnixSock {
    fn send_all(&self, data: &[u8]) -> bool {
        let mut sent = 0usize;
        while sent < data.len() {
            let n = unsafe {
                libc::send(
                    self.0,
                    data[sent..].as_ptr().cast(),
                    data.len() - sent,
                    libc::MSG_NOSIGNAL,
                )
            };
            if n <= 0 {
                return false;
            }
            sent += n as usize;
        }
        true
    }

    fn recv(&self, buf: &mut [u8]) -> isize {
        unsafe { libc::recv(self.0, buf.as_mut_ptr().cast(), buf.len(), 0) }
    }
}

impl Drop for UnixSock {
    fn drop(&mut self) {
        unsafe { libc::close(self.0) };
    }
}

// ============================================================================
// X11: собственный override-redirect слой
// ============================================================================

/// Живой X11-клиент заставки: чёрное окно во весь (primary-)экран, которым
/// НЕ управляет оконный менеджер.
///
/// WM в его судьбе не участвует ⇒ слой работает одинаково на любом X11
/// DE/WM (Plasma, XFCE, MATE, Cinnamon-X11, Budgie…) и даже ДО старта WM.
/// mpv рисует внутрь через `--wid`. Время жизни окна = время жизни нашего
/// соединения с X: упал процесс — X-сервер сам всё уничтожил, экран не
/// остаётся перекрытым.
mod x11 {
    use std::ffi::CString;
    use std::os::raw::{c_int, c_uint, c_ulong};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{mpsc, Arc, Mutex};
    use std::time::Duration;

    use crate::session_splash as core_ss;
    use x11_dl::xlib;

    /// libX11, поднятая через dlopen. Указатели на функции разделяются с
    /// потоком raise-цикла (перед первым `XOpenDisplay` зовём
    /// `XInitThreads`, поэтому libX11 сама разводит потоки).
    struct Lib(xlib::Xlib);
    unsafe impl Send for Lib {}
    unsafe impl Sync for Lib {}

    /// Соединение с X-сервером. Отдельное на поток; общее только там, где
    /// без него не обойтись (страховочный поллинг фолбэка).
    #[derive(Clone, Copy)]
    struct Dpy(*mut xlib::Display);
    unsafe impl Send for Dpy {}
    unsafe impl Sync for Dpy {}

    /// Труба пробуждения: остановка raise-цикла не ждёт истечения шага.
    struct WakePipe {
        read: c_int,
        write: c_int,
    }

    impl WakePipe {
        fn new() -> Option<WakePipe> {
            let mut fds = [0 as c_int; 2];
            let rc = unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) };
            if rc != 0 {
                return None;
            }
            Some(WakePipe {
                read: fds[0],
                write: fds[1],
            })
        }

        fn wake(&self) {
            let b = [1u8];
            unsafe { libc::write(self.write, b.as_ptr().cast(), 1) };
        }
    }

    impl Drop for WakePipe {
        fn drop(&mut self) {
            unsafe {
                libc::close(self.read);
                libc::close(self.write);
            }
        }
    }

    /// Работающий raise-цикл: признак остановки, труба пробуждения и
    /// уведомление о том, что поток действительно ушёл.
    struct RaiseLoop {
        stop: Arc<AtomicBool>,
        pipe: Arc<WakePipe>,
        /// Приёмник под замком: сам по себе он не `Sync`, а слой лежит
        /// внутри объекта, который Python вправе трогать из любого потока.
        done: Mutex<mpsc::Receiver<()>>,
    }

    pub struct Overlay {
        lib: Arc<Lib>,
        dpy: Option<Dpy>,
        screen: c_int,
        pub window: c_ulong,
        pub width: u32,
        pub height: u32,
        raiser: Option<RaiseLoop>,
    }

    impl Overlay {
        /// Создать слой. `geometry` — прямоугольник primary-монитора
        /// (`None` — весь экран). Любая неудача (нет libX11, нет
        /// X-сервера) — `None`: вызывающий откатывается на оконный mpv.
        pub fn create(geometry: Option<(i64, i64, i64, i64)>) -> Option<Overlay> {
            let lib = match xlib::Xlib::open() {
                Ok(l) => Arc::new(Lib(l)),
                Err(e) => {
                    super::log_line("warning", &format!("x11 overlay: libX11 недоступна: {e}"));
                    return None;
                }
            };
            unsafe {
                // ДО XOpenDisplay: XRaiseWindow дёргается из фонового потока.
                (lib.0.XInitThreads)();
                let dpy = (lib.0.XOpenDisplay)(std::ptr::null());
                if dpy.is_null() {
                    super::log_line("warning", "x11 overlay: XOpenDisplay не удался");
                    return None;
                }
                let screen = (lib.0.XDefaultScreen)(dpy);
                let root = (lib.0.XRootWindow)(dpy, screen);
                let (x, y, w, h) = geometry.unwrap_or_else(|| {
                    (
                        0,
                        0,
                        (lib.0.XDisplayWidth)(dpy, screen) as i64,
                        (lib.0.XDisplayHeight)(dpy, screen) as i64,
                    )
                });
                if w <= 0 || h <= 0 {
                    (lib.0.XCloseDisplay)(dpy);
                    return None;
                }
                let mut attrs: xlib::XSetWindowAttributes = std::mem::zeroed();
                attrs.background_pixel = (lib.0.XBlackPixel)(dpy, screen);
                attrs.override_redirect = xlib::True;
                let win = (lib.0.XCreateWindow)(
                    dpy,
                    root,
                    x as c_int,
                    y as c_int,
                    w as c_uint,
                    h as c_uint,
                    0,
                    xlib::CopyFromParent,      // depth
                    xlib::InputOutput as c_uint,
                    std::ptr::null_mut(),      // visual = CopyFromParent
                    xlib::CWBackPixel | xlib::CWOverrideRedirect,
                    &mut attrs,
                );
                if win == 0 {
                    (lib.0.XCloseDisplay)(dpy);
                    return None;
                }
                if let Ok(name) = CString::new("rexon-session-splash") {
                    (lib.0.XStoreName)(dpy, win, name.as_ptr());
                }
                // НЕ маппим здесь: на тёплом входе постер ставится фоном ДО
                // маппинга, чтобы окно появилось сразу с картинкой.
                Some(Overlay {
                    lib,
                    dpy: Some(Dpy(dpy)),
                    screen,
                    window: win,
                    width: w as u32,
                    height: h as u32,
                    raiser: None,
                })
            }
        }

        fn dpy(&self) -> Option<*mut xlib::Display> {
            self.dpy.map(|d| d.0)
        }

        /// Показать слой (поверх всего). Звать после подготовки фона.
        pub fn map(&self) {
            let Some(dpy) = self.dpy() else { return };
            unsafe {
                (self.lib.0.XMapRaised)(dpy, self.window);
                (self.lib.0.XFlush)(dpy);
            }
        }

        /// Композитор уже работает? Мгновенная проверка `_NET_WM_CM_S<n>`.
        pub fn compositor_present(&self) -> bool {
            let Some(dpy) = self.dpy() else { return false };
            let Ok(name) = CString::new(format!("_NET_WM_CM_S{}", self.screen)) else {
                return false;
            };
            unsafe {
                let atom = (self.lib.0.XInternAtom)(dpy, name.as_ptr(), xlib::False);
                atom != 0 && (self.lib.0.XGetSelectionOwner)(dpy, atom) != 0
            }
        }

        /// Нарисовать постер (первый кадр ролика) фоном слоя вместо чёрного.
        ///
        /// Закрывает чёрную паузу между гашением постера ksplashqml
        /// (~0,4 c от входа) и первым кадром mpv (~1,5 c инициализации vo):
        /// слой показывает тот же кадр, который затем нарисует mpv.
        /// Letterbox — как у mpv (вписать с сохранением пропорций, поля
        /// чёрные). Любая неудача не фатальна: остаётся чёрный фон.
        pub fn set_poster(&self, png: &str) -> bool {
            let Some(dpy) = self.dpy() else { return false };
            let x = &self.lib.0;
            let depth = unsafe { (x.XDefaultDepth)(dpy, self.screen) };
            if !core_ss::X11_POSTER_DEPTHS.contains(&(depth as i64)) {
                // 30-битный (10bpc) visual: наши BGRX-байты легли бы криво.
                super::log_line("debug", &format!("set_poster: depth={depth} не поддержан"));
                return false;
            }
            let (w, h) = (self.width, self.height);
            let src = match image::ImageReader::open(png)
                .ok()
                .and_then(|r| r.with_guessed_format().ok())
                .and_then(|r| r.decode().ok())
            {
                Some(im) => im.to_rgb8(),
                None => return false,
            };
            // Рамку letterbox считает ядро: формула та же, что у mpv
            // --fullscreen, и живёт она в одном месте.
            let (ox, oy, sw, sh) = core_ss::letterbox_rect(
                src.width() as i64,
                src.height() as i64,
                w as i64,
                h as i64,
            );
            if sw <= 0 || sh <= 0 {
                return false;
            }
            let scaled = image::imageops::resize(
                &src,
                sw as u32,
                sh as u32,
                image::imageops::FilterType::Lanczos3,
            );
            // Холст BGRX во весь слой: поля остаются чёрными.
            let mut buf = vec![0u8; (w as usize) * (h as usize) * 4];
            for row in 0..scaled.height() {
                let dy = oy as u32 + row;
                if dy >= h {
                    break;
                }
                for col in 0..scaled.width() {
                    let dx = ox as u32 + col;
                    if dx >= w {
                        break;
                    }
                    let p = scaled.get_pixel(col, row).0;
                    let off = ((dy as usize) * (w as usize) + dx as usize) * 4;
                    buf[off] = p[2];
                    buf[off + 1] = p[1];
                    buf[off + 2] = p[0];
                }
            }
            unsafe {
                let pixmap = (x.XCreatePixmap)(dpy, self.window, w, h, depth as c_uint);
                if pixmap == 0 {
                    return false;
                }
                let gc = (x.XCreateGC)(dpy, pixmap, 0, std::ptr::null_mut());
                let visual = (x.XDefaultVisual)(dpy, self.screen);
                let img = (x.XCreateImage)(
                    dpy,
                    visual,
                    depth as c_uint,
                    xlib::ZPixmap,
                    0,
                    buf.as_mut_ptr().cast(),
                    w,
                    h,
                    32,
                    (w as c_int) * 4,
                );
                if img.is_null() {
                    (x.XFreeGC)(dpy, gc);
                    (x.XFreePixmap)(dpy, pixmap);
                    return false;
                }
                (x.XPutImage)(dpy, pixmap, gc, img, 0, 0, 0, 0, w, h);
                // НЕ XDestroyImage: данные — наш буфер, освобождаем только
                // структуру.
                (*img).data = std::ptr::null_mut();
                (x.XFree)(img.cast());
                (x.XFreeGC)(dpy, gc);
                (x.XSetWindowBackgroundPixmap)(dpy, self.window, pixmap);
                (x.XClearWindow)(dpy, self.window);
                // фон скопирован ссылкой на сервере — pixmap можно отпустить
                (x.XFreePixmap)(dpy, pixmap);
                (x.XFlush)(dpy);
            }
            true
        }

        /// Запустить фоновый цикл «держать слой выше чужих поздних попапов».
        ///
        /// Событийно, не поллингом: подписка `SubstructureNotify` на корень —
        /// map/restack ЛЮБОГО чужого окна (напр. панелька перевода Firefox
        /// при восстановлении сессии) поднимает слой в ту же миллисекунду,
        /// до следующего кадра композитора, т.е. попап не успевает попасть
        /// на экран. Поллинг раз в 0,08 c такие блики оставлял до ~80 мс.
        /// Своё соединение с X: не делить очередь событий с главным потоком.
        pub fn raise_start(&mut self) {
            if self.raiser.is_some() {
                return;
            }
            let Some(pipe) = WakePipe::new().map(Arc::new) else {
                super::log_line("debug", "raise_loop: труба пробуждения не создалась");
                return;
            };
            let stop = Arc::new(AtomicBool::new(false));
            let (tx, done) = mpsc::channel();
            let lib = self.lib.clone();
            let main_dpy = self.dpy;
            let (win, screen) = (self.window, self.screen);
            let (t_stop, t_pipe) = (stop.clone(), pipe.clone());
            std::thread::Builder::new()
                .name("rexon-splash-raise".into())
                .spawn(move || {
                    raise_loop(&lib, main_dpy, screen, win, &t_stop, &t_pipe);
                    let _ = tx.send(());
                })
                .ok();
            self.raiser = Some(RaiseLoop {
                stop,
                pipe,
                done: Mutex::new(done),
            });
        }

        /// Остановить цикл и ДОЖДАТЬСЯ потока: закрывать окно, пока он ещё
        /// зовёт по нему XRaiseWindow, нельзя.
        pub fn raise_stop(&mut self) {
            let Some(r) = self.raiser.take() else { return };
            r.stop.store(true, Ordering::SeqCst);
            r.pipe.wake();
            let waited = r
                .done
                .lock()
                .ok()
                .map(|rx| rx.recv_timeout(Duration::from_secs_f64(core_ss::RAISE_JOIN_WAIT_SEC)));
            if !matches!(waited, Some(Ok(()))) {
                super::log_line("debug", "raise_loop: поток не завершился в срок");
            }
        }

        /// Уничтожить слой и закрыть соединение с X.
        pub fn close(&mut self) {
            self.raise_stop();
            let Some(dpy) = self.dpy.take().map(|d| d.0) else { return };
            unsafe {
                (self.lib.0.XDestroyWindow)(dpy, self.window);
                (self.lib.0.XCloseDisplay)(dpy);
            }
        }
    }

    impl Drop for Overlay {
        fn drop(&mut self) {
            self.close();
        }
    }

    /// Тело raise-цикла (в своём потоке, со своим соединением с X).
    fn raise_loop(
        lib: &Arc<Lib>,
        main_dpy: Option<Dpy>,
        screen: c_int,
        win: c_ulong,
        stop: &AtomicBool,
        pipe: &WakePipe,
    ) {
        let x = &lib.0;
        let dpy = unsafe { (x.XOpenDisplay)(std::ptr::null()) };
        if dpy.is_null() {
            // Своё соединение не поднялось — страховочный поллинг по
            // общему (главное соединение живо, пока слой не закрыт).
            let Some(main) = main_dpy else { return };
            while !stop.load(Ordering::SeqCst) {
                if wait_fds(&[pipe.read], core_ss::RAISE_POLL_FALLBACK_SEC) == Some(0) {
                    break; // разбудили — уходим
                }
                unsafe {
                    (x.XRaiseWindow)(main.0, win);
                    (x.XFlush)(main.0);
                }
            }
            return;
        }
        unsafe {
            let root = (x.XRootWindow)(dpy, screen);
            (x.XSelectInput)(dpy, root, xlib::SubstructureNotifyMask);
            (x.XRaiseWindow)(dpy, win);
            (x.XFlush)(dpy);
            let fd = (x.XConnectionNumber)(dpy);
            let mut ev: xlib::XEvent = std::mem::zeroed();
            while !stop.load(Ordering::SeqCst) {
                let ready = wait_fds(&[pipe.read, fd], core_ss::RAISE_INTERVAL_SEC);
                if ready == Some(0) {
                    break; // разбудили на остановку
                }
                // таймаут → страховочный raise
                let mut foreign = ready.is_none();
                while (x.XPending)(dpy) > 0 {
                    (x.XNextEvent)(dpy, &mut ev);
                    let (kind, other) = match ev.get_type() {
                        xlib::MapNotify => (true, ev.map.window),
                        xlib::ConfigureNotify => (true, ev.configure.window),
                        xlib::CirculateNotify => (true, ev.circulate.window),
                        _ => (false, 0),
                    };
                    // свои события фильтруем: наш raise тоже родит
                    // ConfigureNotify — иначе цикл кормил бы сам себя
                    if kind && other != win {
                        foreign = true;
                    }
                }
                if foreign {
                    (x.XRaiseWindow)(dpy, win);
                    (x.XFlush)(dpy);
                }
            }
            (x.XCloseDisplay)(dpy);
        }
    }

    /// Ждать готовности дескрипторов: `Some(индекс первого готового)` либо
    /// `None` по таймауту (порт `select.select` с тайм-аутом).
    fn wait_fds(fds: &[c_int], timeout: f64) -> Option<usize> {
        let mut pfd: Vec<libc::pollfd> = fds
            .iter()
            .map(|f| libc::pollfd {
                fd: *f,
                events: libc::POLLIN,
                revents: 0,
            })
            .collect();
        let ms = (timeout * 1000.0).round().max(0.0) as libc::c_int;
        let rc = unsafe { libc::poll(pfd.as_mut_ptr(), pfd.len() as libc::nfds_t, ms) };
        if rc <= 0 {
            return None;
        }
        pfd.iter().position(|p| p.revents != 0)
    }
}

// ============================================================================
// Живой исполнитель трейта SplashIo
// ============================================================================

/// Замок «один прогон заставки на сессию»: дескриптор живёт до конца
/// процесса вместе с flock (порт модульной переменной `_LOCK_FD`).
static LOCK_FD: Mutex<Option<File>> = Mutex::new(None);

/// Отсоединённые процессы (тест заставки из вкладки): держим, чтобы
/// подобрать их при следующем запуске и не копить зомби в UI-процессе.
static DETACHED: Mutex<Vec<Child>> = Mutex::new(Vec::new());

/// Реальный исполнитель [`core_ss::SplashIo`].
///
/// Один экземпляр — на один вызов фасада: в нём лежат дескрипторы
/// запущенных процессов и X11-слоёв, поэтому гасим только СВОИ (правило
/// «никаких pkill/killall по имени»).
pub struct LiveSplashIo {
    /// `os.path.expanduser("~")` — наблюдение, которое даёт Python.
    home: String,
    /// `sys.executable` — нужен составным шагам установки автозапуска.
    python_exe: String,
    procs: HashMap<i64, Child>,
    overlays: HashMap<i64, x11::Overlay>,
    next: i64,
    /// Зерно жребия ротации (xorshift): свой, чтобы не тянуть крейт ради
    /// одного `random.choice`.
    seed: u64,
}

impl LiveSplashIo {
    pub fn new(home: &str, python_exe: &str) -> LiveSplashIo {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9E37_79B9_7F4A_7C15);
        LiveSplashIo {
            home: home.to_string(),
            python_exe: python_exe.to_string(),
            procs: HashMap::new(),
            overlays: HashMap::new(),
            next: 1,
            seed: (nanos ^ ((std::process::id() as u64) << 32)) | 1,
        }
    }

    fn handle(&mut self) -> i64 {
        let h = self.next;
        self.next += 1;
        h
    }

    fn rnd(&mut self) -> u64 {
        // xorshift64*: жребий ротации, к качеству требований нет.
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        self.seed
    }

    /// Прямоугольник primary-монитора по выводу xrandr (argv и разбор — ядро).
    fn primary_geometry(&mut self) -> Option<(i64, i64, i64, i64)> {
        let xr = self.which("xrandr");
        if xr.is_empty() {
            return None;
        }
        let out = self.run(&core_ss::xrandr_query_argv(&xr), core_ss::XRANDR_TIMEOUT_SEC)?;
        core_ss::parse_xrandr_geometry(&out.stdout)
    }
}

impl core_ss::SplashIo for LiveSplashIo {
    fn env(&mut self, name: &str) -> String {
        std::env::var(name).unwrap_or_default()
    }

    fn which(&mut self, name: &str) -> String {
        which_path(name)
    }

    fn launch_entry(&mut self) -> crate::installer::LaunchEntry {
        crate::installer::launch_entry_live()
    }

    fn is_file(&mut self, path: &str) -> bool {
        Path::new(path).is_file()
    }

    fn is_dir(&mut self, path: &str) -> bool {
        Path::new(path).is_dir()
    }

    fn exists(&mut self, path: &str) -> bool {
        Path::new(path).exists()
    }

    fn is_link(&mut self, path: &str) -> bool {
        std::fs::symlink_metadata(path)
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false)
    }

    fn is_executable(&mut self, path: &str) -> bool {
        unsafe {
            match std::ffi::CString::new(path) {
                Ok(c) => libc::access(c.as_ptr(), libc::X_OK) == 0,
                Err(_) => false,
            }
        }
    }

    fn realpath(&mut self, path: &str) -> String {
        realpath(path)
    }

    fn abspath(&mut self, path: &str) -> String {
        abspath(path)
    }

    fn listdir(&mut self, path: &str) -> Vec<String> {
        let Ok(rd) = std::fs::read_dir(path) else {
            return Vec::new();
        };
        let mut out: Vec<String> = rd
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        // Порядок ФС не определён и у Python — сортируем, чтобы витрина и
        // журнал исполнителя были воспроизводимы.
        out.sort();
        out
    }

    fn read_text(&mut self, path: &str) -> Option<String> {
        let bytes = std::fs::read(path).ok()?;
        Some(String::from_utf8_lossy(&bytes).into_owned())
    }

    fn read_bytes(&mut self, path: &str) -> Option<Vec<u8>> {
        std::fs::read(path).ok()
    }

    fn mtime(&mut self, path: &str) -> Option<i64> {
        let m = std::fs::metadata(path).ok()?;
        let t = m.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
        Some(t.as_secs() as i64)
    }

    fn file_size(&mut self, path: &str) -> i64 {
        std::fs::metadata(path).map(|m| m.len() as i64).unwrap_or(0)
    }

    fn makedirs(&mut self, path: &str) -> Option<String> {
        std::fs::create_dir_all(path).err().map(|e| e.to_string())
    }

    fn write_text(&mut self, path: &str, content: &str) -> Option<String> {
        std::fs::write(path, content).err().map(|e| e.to_string())
    }

    fn write_atomic(&mut self, path: &str, content: &str) -> Option<String> {
        // Недописанный конфиг/юнит на месте настоящего — худшее, что может
        // случиться при входе в сессию. Отсюда общая дверь ядра: своя пара
        // «запись + переименование» здесь была без `fsync`, а заставка входа
        // как раз переживает выключение машины чаще всего прочего.
        crate::utils::atomic_write(path, content).err().map(|e| e.to_string())
    }

    fn replace(&mut self, src: &str, dst: &str) -> Option<String> {
        std::fs::rename(src, dst).err().map(|e| e.to_string())
    }

    fn remove_file(&mut self, path: &str) -> Option<String> {
        std::fs::remove_file(path).err().map(|e| e.to_string())
    }

    fn remove_tree(&mut self, path: &str) -> Option<String> {
        std::fs::remove_dir_all(path).err().map(|e| e.to_string())
    }

    fn copy_file(&mut self, src: &str, dst: &str) -> Option<String> {
        std::fs::copy(src, dst).err().map(|e| e.to_string())
    }

    fn symlink(&mut self, target: &str, link: &str) -> Option<String> {
        std::os::unix::fs::symlink(target, link)
            .err()
            .map(|e| e.to_string())
    }

    fn run(&mut self, argv: &[String], timeout: f64) -> Option<core_ss::RunOut> {
        run_capture(argv, timeout)
    }

    fn spawn(&mut self, argv: &[String], detached: bool) -> Result<i64, String> {
        // Подобрать отсоединённых предшественников: в долгоживущем
        // UI-процессе иначе копятся зомби.
        if let Ok(mut kept) = DETACHED.lock() {
            kept.retain_mut(|c| !matches!(c.try_wait(), Ok(Some(_))));
        }
        let Some((bin, rest)) = argv.split_first() else {
            return Err(crate::i18n::tr("empty command"));
        };
        let mut cmd = Command::new(bin);
        cmd.args(rest);
        if detached {
            // Порт `start_new_session=True`: свой сеанс, чтобы процесс
            // пережил выход вызывающего и не ловил его сигналы.
            unsafe {
                cmd.pre_exec(|| {
                    libc::setsid();
                    Ok(())
                });
            }
        }
        match cmd.spawn() {
            Ok(child) => {
                if detached {
                    if let Ok(mut kept) = DETACHED.lock() {
                        kept.push(child);
                    }
                    // Дескриптор вызывающему не нужен: отсоединённый прогон
                    // ядро больше не трогает.
                    let h = self.handle();
                    Ok(h)
                } else {
                    let h = self.handle();
                    self.procs.insert(h, child);
                    Ok(h)
                }
            }
            Err(e) => Err(e.to_string()),
        }
    }

    fn poll(&mut self, proc: i64) -> Option<i64> {
        let c = self.procs.get_mut(&proc)?;
        match c.try_wait() {
            Ok(Some(st)) => Some(st.code().unwrap_or(-1) as i64),
            _ => None,
        }
    }

    fn wait(&mut self, proc: i64, timeout: f64) -> Option<i64> {
        let deadline = Instant::now() + Duration::from_secs_f64(timeout.max(0.0));
        loop {
            {
                let c = self.procs.get_mut(&proc)?;
                match c.try_wait() {
                    Ok(Some(st)) => return Some(st.code().unwrap_or(-1) as i64),
                    Ok(None) => {}
                    Err(_) => return None,
                }
            }
            if Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn terminate(&mut self, proc: i64) {
        // Только СВОЙ дескриптор, никогда не по имени процесса.
        if let Some(c) = self.procs.get_mut(&proc) {
            let pid = c.id() as libc::pid_t;
            unsafe { libc::kill(pid, libc::SIGTERM) };
        }
    }

    fn kill(&mut self, proc: i64) {
        if let Some(c) = self.procs.get_mut(&proc) {
            let _ = c.kill();
            let _ = c.wait();
        }
    }

    fn monotonic(&mut self) -> f64 {
        mono_epoch().elapsed().as_secs_f64()
    }

    fn sleep(&mut self, seconds: f64) {
        if seconds > 0.0 {
            std::thread::sleep(Duration::from_secs_f64(seconds));
        }
    }

    fn choice(&mut self, items: &[String]) -> Option<String> {
        if items.is_empty() {
            return None;
        }
        let i = (self.rnd() % items.len() as u64) as usize;
        Some(items[i].clone())
    }

    fn ipc_send(&mut self, sock: &str, payload: &str) -> bool {
        match connect_unix(sock, false, core_ss::IPC_SOCK_TIMEOUT_SEC) {
            Some(s) => s.send_all(payload.as_bytes()),
            None => false,
        }
    }

    fn ipc_request(&mut self, sock: &str, payload: &str, reply_marker: &str) -> Option<String> {
        // В сокет вперемешку сыплются события, поэтому читаем, пока не
        // появится завершённая строка с маркером (его даёт ядро) либо пока
        // соединение не закроется / не упрёмся в предел. Разбор — в ядре.
        let s = connect_unix(sock, false, core_ss::IPC_SOCK_TIMEOUT_SEC)?;
        if !s.send_all(payload.as_bytes()) {
            return None;
        }
        let marker = reply_marker.as_bytes();
        let mut buf: Vec<u8> = Vec::new();
        let mut chunk = [0u8; 4096];
        while buf.len() < core_ss::MPV_IPC_READ_LIMIT {
            let n = s.recv(&mut chunk);
            if n <= 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n as usize]);
            if let Some(nl) = buf.iter().rposition(|b| *b == b'\n') {
                if buf[..nl].windows(marker.len()).any(|w| w == marker) {
                    break;
                }
            }
        }
        Some(String::from_utf8_lossy(&buf).into_owned())
    }

    fn notify(&mut self, addr: &str, message: &str) {
        let sent = connect_unix(addr, true, core_ss::IPC_SOCK_TIMEOUT_SEC)
            .map(|s| s.send_all(message.as_bytes()))
            .unwrap_or(false);
        if !sent {
            log_line("debug", &format!("sd_notify не ушёл: {addr}"));
        }
    }

    fn acquire_lock(&mut self, path: &str) -> bool {
        // Один сплеш на сессию (юнит + autostart, двойной клик…).
        let Ok(f) = File::create(path) else { return false };
        let rc = unsafe {
            libc::flock(
                std::os::unix::io::AsRawFd::as_raw_fd(&f),
                libc::LOCK_EX | libc::LOCK_NB,
            )
        };
        if rc != 0 {
            return false;
        }
        // Дескриптор живёт до конца процесса вместе с замком.
        if let Ok(mut slot) = LOCK_FD.lock() {
            *slot = Some(f);
        }
        true
    }

    fn kwin_reconfigure_dbus(&mut self, timeout: f64) -> bool {
        // Основной путь reconfigure — сессионная шина: `dbus-send` есть
        // везде, где есть сама шина. Порядок предпочтения инструментов и
        // сборка argv — общие (`utils`), своей копии здесь нет.
        let path_env = std::env::var("PATH").ok();
        for (tool, kind) in core_utils::DBUS_TOOL_PREFERENCE {
            let Some(exe) = core_utils::resolve_tool(tool, &[], path_env.as_deref()) else {
                continue;
            };
            let (dest, obj, method) = core_utils::KWIN_RECONFIGURE;
            let argv = core_utils::dbus_call_argv(kind, &exe, dest, obj, method, &[]);
            return matches!(run_capture(&argv, timeout), Some(r) if r.rc == 0);
        }
        false
    }

    fn x11_create(&mut self) -> Option<i64> {
        let geo = self.primary_geometry();
        let overlay = x11::Overlay::create(geo)?;
        let h = self.handle();
        self.overlays.insert(h, overlay);
        Some(h)
    }

    fn x11_window_id(&mut self, overlay: i64) -> i64 {
        self.overlays.get(&overlay).map(|o| o.window as i64).unwrap_or(0)
    }

    fn x11_set_poster(&mut self, overlay: i64, png: &str) -> bool {
        self.overlays
            .get(&overlay)
            .map(|o| o.set_poster(png))
            .unwrap_or(false)
    }

    fn x11_map(&mut self, overlay: i64) {
        if let Some(o) = self.overlays.get(&overlay) {
            o.map();
        }
    }

    fn x11_compositor_present(&mut self, overlay: i64) -> bool {
        self.overlays
            .get(&overlay)
            .map(|o| o.compositor_present())
            .unwrap_or(false)
    }

    fn x11_raise_start(&mut self, overlay: i64) {
        if let Some(o) = self.overlays.get_mut(&overlay) {
            o.raise_start();
        }
    }

    fn x11_raise_stop(&mut self, overlay: i64) {
        if let Some(o) = self.overlays.get_mut(&overlay) {
            o.raise_stop();
        }
    }

    fn x11_close(&mut self, overlay: i64) {
        if let Some(mut o) = self.overlays.remove(&overlay) {
            o.close();
        }
    }

    // ── составные шаги маршрута автозапуска ────────────────────────────
    // Их внутренности тоже в ядре; маршрут `install_autostart` зовёт их
    // ЧЕРЕЗ исполнителя, чтобы сверка маршрутов могла подменить шаг.

    fn step_install_hybrid(&mut self, video: &str) -> (bool, String) {
        let (home, exe) = (self.home.clone(), self.python_exe.clone());
        core_ss::Splash::new(self, &home).install_hybrid(video, &exe)
    }

    fn step_uninstall_hybrid(&mut self) -> (bool, String) {
        let home = self.home.clone();
        core_ss::Splash::new(self, &home).uninstall_hybrid()
    }

    fn step_install_desktop_autostart(&mut self) -> (bool, String) {
        let (home, exe) = (self.home.clone(), self.python_exe.clone());
        core_ss::Splash::new(self, &home).install_desktop_autostart(&exe)
    }

    fn step_remove_desktop_autostart(&mut self) {
        let home = self.home.clone();
        core_ss::Splash::new(self, &home).remove_desktop_autostart();
    }

    fn step_hybrid_supported(&mut self) -> bool {
        let home = self.home.clone();
        core_ss::Splash::new(self, &home).hybrid_supported()
    }

    fn login_lines(&mut self, desktop: Option<&str>) -> (Option<String>, Option<String>) {
        use crate::compositor_autostart as ca;
        match desktop {
            Some(d) => {
                let (text, refusal) = ca::mirror_desktop_live(ca::ID_SPLASH, d);
                (Some(text), refusal)
            }
            None => {
                ca::remove_live(ca::ID_SPLASH);
                (None, None)
            }
        }
    }

    fn login_works(&mut self, desktop_path: &str) -> bool {
        use crate::compositor_autostart as ca;
        let text = std::fs::read_to_string(desktop_path).ok();
        ca::works_live(ca::ID_SPLASH, text.as_deref())
    }

    fn log(&mut self, level: &str, message: &str) {
        log_line(level, message);
    }
}
