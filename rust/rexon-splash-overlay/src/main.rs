// rexon-splash-overlay — видео на самом верхнем слое (wlr-layer-shell OVERLAY)
// в сессии KWin Wayland (Plasma 6).
//
// CLI: rexon-splash-overlay --video <PATH> [--ipc <SOCK>] [--paused] [--mute]
//
// Заводит по layer-поверхности (layer=Overlay, все якоря → на весь вывод,
// exclusive_zone=-1, клавиатура=None) на КАЖДЫЙ подключённый вывод: при двух
// мониторах заставку видят оба, а не один. Поднимает EGL (GLES2) на
// wl_egl_window каждой поверхности — контекст один на процесс, поверхностей
// столько, сколько экранов, — и проигрывает видео встроенной libmpv через
// render API (OpenGL): один кадр рисуется в каждую поверхность по очереди.
//
// Звук звучит РОВНО ОДИН РАЗ: проигрыватель в процессе один на все выходы,
// и звуковую дорожку ведёт он — размножаются только картинки. Основным
// считается вывод в позиции (0,0) — тот же порядок, что был до появления
// нескольких поверхностей.
//
// Управление — по обычному JSON IPC mpv (--ipc). Держит последний кадр
// (keep-open=yes) до SIGTERM или команды quit по IPC / события
// MPV_EVENT_SHUTDOWN. Уход и возврат монитора на ходу поверхности только
// добавляет и убирает, ПРОЦЕСС ЖИВ: условие окончания заставки — конец
// файла/quit/сигнал, а не число экранов; при нуле выходов кадры всё равно
// забираются у mpv (SKIP_RENDERING), иначе воспроизведение встало бы и до
// конца файла оркестратор не дождался бы никогда.
//
// На выходе: сначала mpv_render_context_free, затем mpv_terminate_destroy.

mod mpv;

use std::ffi::{CStr, CString};
use std::os::fd::{AsFd, AsRawFd};
use std::os::raw::{c_char, c_int, c_void};
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::OnceLock;

use khronos_egl as egl;

use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_layer, delegate_output, delegate_registry,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    shell::{
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
        WaylandSurface,
    },
};
use smithay_client_toolkit::reexports::client as wayland_client;
use wayland_client::{
    globals::registry_queue_init,
    protocol::{wl_output, wl_surface},
    Connection, Proxy, QueueHandle,
};
use wayland_egl::WlEglSurface;

// --- Глобальное состояние для C-колбэков (единственный поток владеет всем) ---

// EGL-инстанс: нужен из extern "C" колбэка get_proc_address для mpv.
static EGL: OnceLock<egl::Instance<egl::Static>> = OnceLock::new();
// eventfd для пробуждения poll из колбэков mpv и обработчика сигнала.
static EVENTFD: AtomicI32 = AtomicI32::new(-1);
// Флаг завершения по сигналу (SIGTERM/SIGINT).
static TERMINATE: AtomicBool = AtomicBool::new(false);

// Размер вывода, когда композитор не сказал ни логического размера, ни режима.
const FALLBACK_W: u32 = 1920;
const FALLBACK_H: u32 = 1080;

// Переменная окружения для отладочных строк на stderr (число выходов и
// поверхностей). В боевом прогоне молчим: stderr хелпера читает оркестратор.
const DEBUG_ENV: &str = "REXON_SPLASH_DEBUG";

// Сколько кадров подряд получает только что появившаяся поверхность.
const FIRST_FRAMES: u8 = 12;

// --- Колбэки ---

// get_proc_address для mpv: резолвим GL-функции через EGL.
extern "C" fn get_proc_address(_ctx: *mut c_void, name: *const c_char) -> *mut c_void {
    if name.is_null() {
        return ptr::null_mut();
    }
    let cstr = unsafe { CStr::from_ptr(name) };
    let s = match cstr.to_str() {
        Ok(s) => s,
        Err(_) => return ptr::null_mut(),
    };
    match EGL.get().and_then(|e| e.get_proc_address(s)) {
        Some(f) => f as usize as *mut c_void,
        None => ptr::null_mut(),
    }
}

// Будим poll: пишем 1 в eventfd. Async-signal-safe (write) — годится и для
// обработчика сигнала, и для колбэков mpv.
fn wake_eventfd() {
    let fd = EVENTFD.load(Ordering::Relaxed);
    if fd >= 0 {
        let one: u64 = 1;
        unsafe {
            libc::write(fd, &one as *const u64 as *const c_void, 8);
        }
    }
}

extern "C" fn mpv_wakeup_cb(_d: *mut c_void) {
    wake_eventfd();
}

extern "C" fn mpv_render_update_cb(_d: *mut c_void) {
    wake_eventfd();
}

extern "C" fn handle_term(_sig: c_int) {
    TERMINATE.store(true, Ordering::SeqCst);
    wake_eventfd();
}

fn debug_on() -> bool {
    std::env::var_os(DEBUG_ENV).is_some()
}

// --- Минимум GL: очистка свежей поверхности ---
//
// Страховка на первый кадр только что созданной поверхности: её задний буфер
// не определён, и если бы кадр лёг не на всю площадь, по краям оказался бы
// мусор. Замером 16.09 видно, что mpv перекрашивает всю цель сам (заливка
// красным до отрисовки на снимках не проступила ни разу), так что цена
// страховки — одна заливка на поверхность.

type GlClearColorFn = extern "C" fn(f32, f32, f32, f32);
type GlClearFn = extern "C" fn(u32);
const GL_COLOR_BUFFER_BIT: u32 = 0x4000;

static GL_CLEAR: OnceLock<Option<(GlClearColorFn, GlClearFn)>> = OnceLock::new();

fn gl_clear_fns() -> Option<(GlClearColorFn, GlClearFn)> {
    *GL_CLEAR.get_or_init(|| {
        let egl = EGL.get()?;
        let color = egl.get_proc_address("glClearColor")?;
        let clear = egl.get_proc_address("glClear")?;
        Some(unsafe {
            (
                std::mem::transmute::<extern "system" fn(), GlClearColorFn>(color),
                std::mem::transmute::<extern "system" fn(), GlClearFn>(clear),
            )
        })
    })
}

// Заливает текущую поверхность непрозрачным чёрным.
fn clear_current_surface() {
    if let Some((clear_color, clear)) = gl_clear_fns() {
        clear_color(0.0, 0.0, 0.0, 1.0);
        clear(GL_COLOR_BUFFER_BIT);
    }
}

// --- Разбор аргументов ---

struct Args {
    video: String,
    ipc: Option<String>,
    paused: bool,
    mute: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut video: Option<String> = None;
    let mut ipc: Option<String> = None;
    let mut paused = false;
    let mut mute = false;

    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--video" => {
                video = Some(it.next().ok_or("--video требует путь")?);
            }
            "--ipc" => {
                ipc = Some(it.next().ok_or("--ipc требует путь к сокету")?);
            }
            "--paused" => paused = true,
            "--mute" => mute = true,
            other => return Err(format!("неизвестный аргумент: {other}")),
        }
    }

    Ok(Args {
        video: video.ok_or("не задан --video <PATH>")?,
        ipc,
        paused,
        mute,
    })
}

// --- Поверхность одного вывода ---

// Один экран = одна layer-поверхность + своя EGL-поверхность. Контекст EGL
// общий на процесс: делать по контексту на экран незачем, а обмен текстурами
// между контекстами mpv не поддерживает.
struct OutputView {
    output: wl_output::WlOutput,
    // Держим LayerSurface живой: её Drop сносит роль и wl_surface.
    layer: LayerSurface,
    // Держим WlEglSurface живым: он владеет wl_egl_window.
    egl_window: Option<WlEglSurface>,
    egl_surface: Option<egl::Surface>,
    // Логический размер (то, что прислал композитор) и масштаб экрана.
    width: u32,
    height: u32,
    scale: i32,
    configured: bool,
    // Экрану нужен кадр, даже если новых кадров у mpv нет (пришёл монитор на
    // паузе гибридного режима — иначе он остался бы чёрным до снятия паузы).
    needs_redraw: bool,
    // Задний буфер ещё не залит: у новой поверхности он не определён.
    needs_clear: bool,
    // Сколько кадров подряд ещё должна получить свежая поверхность.
    //
    // Замер 16.09 (sway headless, заставка на паузе гибридного режима):
    // вернувшийся монитор оставался чёрным несколько секунд, хотя кадры в него
    // уходили с первой секунды и swap_buffers их принимал. На паузе mpv отдаёт
    // перерисовку примерно раз в секунду — свежей поверхности отдаём кадры
    // подряд, не дожидаясь этой неторопливой выдачи. Задержку это сократило,
    // но не убрало совсем: остаток — за композитором стенда, на живом
    // проигрывании (25 кадров в секунду) вернувшийся монитор показывает
    // заставку сразу.
    first_frames: u8,
}

impl OutputView {
    // Размер буфера в пикселях: логический размер × масштаб экрана.
    fn buffer_size(&self) -> (i32, i32) {
        let s = self.scale.max(1);
        ((self.width as i32) * s, (self.height as i32) * s)
    }
}

// --- Состояние приложения (sctk) ---

struct State {
    registry_state: RegistryState,
    output_state: OutputState,
    compositor: CompositorState,
    layer_shell: LayerShell,

    exit: bool,

    // EGL: заполняется до цикла событий.
    egl_display: egl::Display,
    egl_config: egl::Config,
    egl_context: egl::Context,

    // По поверхности на каждый подключённый вывод.
    views: Vec<OutputView>,

    // Нужно сходить в mpv за кадром.
    render_pending: bool,
}

impl State {
    // Отвязываем контекст от любой поверхности: обязательный шаг перед тем,
    // как рушить EGL-поверхность ушедшего монитора.
    fn release_current(&self) {
        let egl = EGL.get().expect("EGL не инициализирован");
        let _ = egl.make_current(self.egl_display, None, None, Some(self.egl_context));
    }

    // Делает EGL-контекст текущим на поверхности вывода.
    fn make_current_view(&self, idx: usize) -> bool {
        let egl = EGL.get().expect("EGL не инициализирован");
        match self.views.get(idx).and_then(|v| v.egl_surface) {
            Some(surf) => egl
                .make_current(
                    self.egl_display,
                    Some(surf),
                    Some(surf),
                    Some(self.egl_context),
                )
                .is_ok(),
            None => false,
        }
    }

    // Основной вывод — тот, что в позиции (0,0); иначе первый заведённый.
    // Порядок ровно тот же, каким одиночная поверхность выбирала себе экран:
    // на (0,0) у Plasma стоит главный монитор, и заставка начиналась с него.
    fn primary_index(&self) -> Option<usize> {
        for (i, v) in self.views.iter().enumerate() {
            if let Some(info) = self.output_state.info(&v.output) {
                if let Some((x, y)) = info.logical_position {
                    if x == 0 && y == 0 {
                        return Some(i);
                    }
                }
            }
        }
        if self.views.is_empty() {
            None
        } else {
            Some(0)
        }
    }

    // Запасной размер вывода: логический размер, иначе текущий режим,
    // иначе FullHD. Нужен, когда configure пришёл с 0×0.
    fn output_fallback_size(&self, output: &wl_output::WlOutput) -> (u32, u32) {
        if let Some(info) = self.output_state.info(output) {
            if let Some((w, h)) = info.logical_size {
                if w > 0 && h > 0 {
                    return (w as u32, h as u32);
                }
            }
            if let Some(m) = info.modes.iter().find(|m| m.current) {
                let (w, h) = m.dimensions;
                if w > 0 && h > 0 {
                    return (w as u32, h as u32);
                }
            }
        }
        (FALLBACK_W, FALLBACK_H)
    }

    // Заводит поверхность на выводе, если её ещё нет.
    fn add_view(&mut self, qh: &QueueHandle<State>, output: wl_output::WlOutput) {
        if self.views.iter().any(|v| v.output == output) {
            return;
        }
        let (w, h) = self.output_fallback_size(&output);
        let scale = self
            .output_state
            .info(&output)
            .map(|i| i.scale_factor)
            .unwrap_or(1)
            .max(1);

        let surface = self.compositor.create_surface(qh);
        // Целимся в конкретный вывод явно: без этого композитор вправе
        // положить все поверхности на один монитор.
        let layer = self.layer_shell.create_layer_surface(
            qh,
            surface,
            Layer::Overlay,
            Some("rexon-splash".to_string()),
            Some(&output),
        );
        layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
        layer.set_exclusive_zone(-1);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer.set_size(0, 0);
        layer.commit();

        self.views.push(OutputView {
            output,
            layer,
            egl_window: None,
            egl_surface: None,
            width: w,
            height: h,
            scale,
            configured: false,
            needs_redraw: false,
            needs_clear: true,
            first_frames: 0,
        });
    }

    // Снимает поверхность: сначала EGL, потом layer-поверхность. Порядок
    // важен — рисовать в поверхность снятого монитора нельзя, а первый же
    // кадр в неё композитор считает протокольной ошибкой и рвёт соединение.
    fn drop_view(&mut self, idx: usize) {
        if idx >= self.views.len() {
            return;
        }
        self.release_current();
        let egl = EGL.get().expect("EGL не инициализирован");
        let mut view = self.views.remove(idx);
        if let Some(surf) = view.egl_surface.take() {
            let _ = egl.destroy_surface(self.egl_display, surf);
        }
        view.egl_window = None;
        // view (а с ним LayerSurface) рушится здесь.
    }

    fn remove_view_by_output(&mut self, output: &wl_output::WlOutput) {
        if let Some(i) = self.views.iter().position(|v| &v.output == output) {
            self.drop_view(i);
        }
    }

    fn remove_view_by_layer(&mut self, layer: &LayerSurface) {
        if let Some(i) = self.views.iter().position(|v| &v.layer == layer) {
            self.drop_view(i);
        }
    }

    fn view_index_by_surface(&self, surface: &wl_surface::WlSurface) -> Option<usize> {
        self.views
            .iter()
            .position(|v| v.layer.wl_surface() == surface)
    }

    fn debug_report(&self, tag: &str) {
        if !debug_on() {
            return;
        }
        let outputs = self.output_state.outputs().count();
        let primary = self
            .primary_index()
            .and_then(|i| self.output_state.info(&self.views[i].output))
            .and_then(|i| i.name)
            .unwrap_or_else(|| "—".to_string());
        let sizes: Vec<String> = self
            .views
            .iter()
            .map(|v| {
                let name = self
                    .output_state
                    .info(&v.output)
                    .and_then(|i| i.name)
                    .unwrap_or_else(|| "?".to_string());
                format!("{name}:{}x{}@{}", v.width, v.height, v.scale)
            })
            .collect();
        rexon_core::log_info!(
            "splash-overlay",
            "SCREENS[{tag}] выходов={outputs} поверхностей={} звук=«{primary}» {}",
            self.views.len(),
            sizes.join(" ")
        );
    }
}

// --- Обработчики протоколов ---

impl CompositorHandler for State {
    fn scale_factor_changed(
        &mut self,
        _c: &Connection,
        _q: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        factor: i32,
    ) {
        // Разный DPI у мониторов: буфер каждой поверхности растёт во столько
        // же раз, во сколько масштабируется её экран, иначе на HiDPI-мониторе
        // кадр поднимали бы растяжением из логического размера.
        let idx = match self.view_index_by_surface(surface) {
            Some(i) => i,
            None => return,
        };
        let factor = factor.max(1);
        if self.views[idx].scale == factor {
            return;
        }
        self.views[idx].scale = factor;
        let (bw, bh) = self.views[idx].buffer_size();
        surface.set_buffer_scale(factor);
        if let Some(win) = &self.views[idx].egl_window {
            win.resize(bw, bh, 0, 0);
        }
        self.views[idx].needs_redraw = true;
        self.views[idx].needs_clear = true;
        self.render_pending = true;
        self.debug_report("масштаб");
    }
    fn transform_changed(
        &mut self,
        _c: &Connection,
        _q: &QueueHandle<Self>,
        _s: &wl_surface::WlSurface,
        _t: wl_output::Transform,
    ) {
    }
    fn frame(
        &mut self,
        _c: &Connection,
        _q: &QueueHandle<Self>,
        _s: &wl_surface::WlSurface,
        _t: u32,
    ) {
    }
    fn surface_enter(
        &mut self,
        _c: &Connection,
        _q: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        output: &wl_output::WlOutput,
    ) {
        if !debug_on() {
            return;
        }
        // Куда поверхность легла НА САМОМ ДЕЛЕ — слово композитора, а не наше
        // пожелание: «заставка поднялась» и «заставка легла на все мониторы»
        // это разные утверждения.
        if let Some(i) = self.view_index_by_surface(surface) {
            let name = self
                .output_state
                .info(output)
                .and_then(|info| info.name)
                .unwrap_or_else(|| "?".to_string());
            rexon_core::log_info!(
                "splash-overlay",
                "SCREENS[вход] поверхность №{i} на выходе «{name}»"
            );
        }
    }
    fn surface_leave(
        &mut self,
        _c: &Connection,
        _q: &QueueHandle<Self>,
        _s: &wl_surface::WlSurface,
        _o: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for State {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(
        &mut self,
        _c: &Connection,
        qh: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        self.add_view(qh, output);
        self.debug_report("+выход");
    }
    fn update_output(
        &mut self,
        _c: &Connection,
        _q: &QueueHandle<Self>,
        _o: wl_output::WlOutput,
    ) {
    }
    fn output_destroyed(
        &mut self,
        _c: &Connection,
        _q: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        // Монитор сняли: поверхность убираем немедленно, ДО следующего кадра.
        self.remove_view_by_output(&output);
        self.debug_report("−выход");
    }
}

impl LayerShellHandler for State {
    fn closed(&mut self, _c: &Connection, _q: &QueueHandle<Self>, layer: &LayerSurface) {
        // Закрыли поверхность одного экрана — это не конец заставки: она
        // обязана доиграть до своего условия окончания. Гасить процесс здесь
        // значило бы, что снятый монитор оставляет человека без заставки, а
        // при неудачном стечении — и без входа в сеанс.
        self.remove_view_by_layer(layer);
        self.debug_report("закрыт слой");
    }

    fn configure(
        &mut self,
        _c: &Connection,
        _q: &QueueHandle<Self>,
        layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
        let idx = match self.views.iter().position(|v| &v.layer == layer) {
            Some(i) => i,
            None => return,
        };

        // Размер от композитора; при 0 берём заранее вычисленный fallback.
        let (mut w, mut h) = configure.new_size;
        if w == 0 {
            w = self.views[idx].width;
        }
        if h == 0 {
            h = self.views[idx].height;
        }
        self.views[idx].width = w;
        self.views[idx].height = h;
        let (bw, bh) = self.views[idx].buffer_size();
        let scale = self.views[idx].scale;

        let egl = EGL.get().expect("EGL не инициализирован");

        if self.views[idx].egl_window.is_none() {
            // Первый configure: создаём wl_egl_window и EGL-оконную поверхность.
            let wl_surf = self.views[idx].layer.wl_surface().clone();
            let win = match WlEglSurface::new(wl_surf.id(), bw, bh) {
                Ok(win) => win,
                Err(e) => {
                    rexon_core::log_error!("splash-overlay", "не создать WlEglSurface: {e:?}");
                    // Экран остаётся без картинки, остальные продолжают.
                    self.drop_view(idx);
                    return;
                }
            };
            let surf = unsafe {
                egl.create_window_surface(
                    self.egl_display,
                    self.egl_config,
                    win.ptr() as egl::NativeWindowType,
                    None,
                )
            };
            let surf = match surf {
                Ok(s) => s,
                Err(e) => {
                    rexon_core::log_error!(
                        "splash-overlay",
                        "create_window_surface провалился: {e:?}"
                    );
                    self.drop_view(idx);
                    return;
                }
            };
            self.views[idx].egl_window = Some(win);
            self.views[idx].egl_surface = Some(surf);
            wl_surf.set_buffer_scale(scale);

            if self.make_current_view(idx) {
                // Ждать вертикальной синхронизации НЕ просим ни на одной
                // поверхности: погашенному монитору композитор перестаёт
                // слать frame-события, и блокирующий swap_buffers на нём
                // подвесил бы весь процесс — заставка не ответила бы ни на
                // quit, ни на SIGTERM и осталась бы висеть поверх сеанса.
                // Темп задаёт сам mpv: кадр — колбэк — отрисовка.
                let _ = egl.swap_interval(self.egl_display, 0);
            }
        } else if let Some(win) = &self.views[idx].egl_window {
            // Переконфигурация: подгоняем размер окна.
            win.resize(bw, bh, 0, 0);
        }

        if !self.views[idx].configured {
            self.views[idx].first_frames = FIRST_FRAMES;
        }
        self.views[idx].configured = true;
        self.views[idx].needs_redraw = true;
        self.views[idx].needs_clear = true;
        self.render_pending = true;
    }
}

delegate_compositor!(State);
delegate_output!(State);
delegate_layer!(State);
delegate_registry!(State);

impl ProvidesRegistryState for State {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState];
}

// --- Настройка libmpv ---

fn set_opt(mpv: *mut mpv::mpv_handle, name: &str, val: &str) {
    let cn = CString::new(name).unwrap();
    let cv = CString::new(val).unwrap();
    unsafe {
        mpv::mpv_set_option_string(mpv, cn.as_ptr(), cv.as_ptr());
    }
}

fn main() {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("rexon-splash-overlay: {e}");
            eprintln!(
                "использование: rexon-splash-overlay --video <PATH> [--ipc <SOCK>] [--paused] [--mute]"
            );
            std::process::exit(2);
        }
    };

    // Журнал — общий файл Rexon (`rexon_core::applog`), область
    // `splash-overlay`: заставку запускает маршрут `--session-splash`
    // главного бинаря, и её строки встают в одну хронологию с его строками
    // (номер процесса в каждой). Справка и неверные ключи выше — вывод
    // команды, не журнал.
    rexon_core::applog::init();
    if let Err(e) = run(args) {
        rexon_core::log_error!("splash-overlay", "{e}");
        std::process::exit(1);
    }
}

fn run(args: Args) -> Result<(), String> {
    // --- Wayland ---
    let conn = Connection::connect_to_env()
        .map_err(|e| format!("нет подключения к Wayland: {e}"))?;
    let (globals, mut event_queue) =
        registry_queue_init::<State>(&conn).map_err(|e| format!("registry init: {e}"))?;
    let qh = event_queue.handle();

    let compositor = CompositorState::bind(&globals, &qh)
        .map_err(|e| format!("нет wl_compositor: {e}"))?;
    let layer_shell = LayerShell::bind(&globals, &qh)
        .map_err(|e| format!("нет wlr-layer-shell (нужен KWin/wlroots): {e}"))?;
    let output_state = OutputState::new(&globals, &qh);
    let registry_state = RegistryState::new(&globals);

    // Сырой указатель wl_display для EGL и для mpv (WL_DISPLAY).
    let display_ptr = conn.backend().display_ptr() as *mut c_void;

    // --- EGL: display/config/context (без поверхности пока) ---
    let egl_instance = egl::Instance::new(egl::Static);
    let egl_display = unsafe { egl_instance.get_display(display_ptr) }
        .ok_or("EGL: get_display вернул None")?;
    let (maj, min) = egl_instance
        .initialize(egl_display)
        .map_err(|e| format!("EGL initialize: {e:?}"))?;
    let _ = (maj, min);

    egl_instance
        .bind_api(egl::OPENGL_ES_API)
        .map_err(|e| format!("EGL bind_api(GLES): {e:?}"))?;

    let (egl_config, egl_context) = create_egl_context(&egl_instance, egl_display)?;

    // Делаем контекст текущим без поверхности, чтобы создать render-контекст mpv.
    egl_instance
        .make_current(egl_display, None, None, Some(egl_context))
        .map_err(|e| format!("EGL make_current(no-surface): {e:?}"))?;

    // Публикуем EGL-инстанс для колбэка get_proc_address.
    EGL.set(egl_instance)
        .map_err(|_| "EGL уже установлен".to_string())?;

    // --- libmpv ---
    let mpv = unsafe { mpv::mpv_create() };
    if mpv.is_null() {
        return Err("mpv_create вернул NULL".into());
    }

    // Опции до инициализации.
    set_opt(mpv, "config", "no");
    set_opt(mpv, "terminal", "no");
    set_opt(mpv, "msg-level", "all=no");
    set_opt(mpv, "osd-level", "0");
    set_opt(mpv, "input-default-bindings", "no");
    set_opt(mpv, "input-vo-keyboard", "no");
    set_opt(mpv, "loop-file", "no");
    set_opt(mpv, "keep-open", "yes");
    set_opt(mpv, "hwdec", "auto");
    set_opt(mpv, "vo", "libmpv");
    if let Some(ipc) = &args.ipc {
        set_opt(mpv, "input-ipc-server", ipc);
    }
    if args.paused {
        set_opt(mpv, "pause", "yes");
    }
    if args.mute {
        set_opt(mpv, "mute", "yes");
    }

    if unsafe { mpv::mpv_initialize(mpv) } < 0 {
        return Err("mpv_initialize провалился".into());
    }

    // --- render-контекст mpv (OpenGL) ---
    let api_type = CString::new("opengl").unwrap();
    let mut init_params = mpv::mpv_opengl_init_params {
        get_proc_address: Some(get_proc_address),
        get_proc_address_ctx: ptr::null_mut(),
    };
    let mut rparams = [
        mpv::mpv_render_param {
            type_: mpv::MPV_RENDER_PARAM_API_TYPE,
            data: api_type.as_ptr() as *mut c_void,
        },
        mpv::mpv_render_param {
            type_: mpv::MPV_RENDER_PARAM_OPENGL_INIT_PARAMS,
            data: &mut init_params as *mut _ as *mut c_void,
        },
        mpv::mpv_render_param {
            type_: mpv::MPV_RENDER_PARAM_WL_DISPLAY,
            data: display_ptr,
        },
        mpv::mpv_render_param {
            type_: mpv::MPV_RENDER_PARAM_INVALID,
            data: ptr::null_mut(),
        },
    ];

    let mut mpv_gl: *mut mpv::mpv_render_context = ptr::null_mut();
    let rc = unsafe { mpv::mpv_render_context_create(&mut mpv_gl, mpv, rparams.as_mut_ptr()) };
    if rc < 0 || mpv_gl.is_null() {
        unsafe { mpv::mpv_terminate_destroy(mpv) };
        return Err(format!("mpv_render_context_create провалился (код {rc})"));
    }

    // Колбэки пробуждения.
    unsafe {
        mpv::mpv_set_wakeup_callback(mpv, Some(mpv_wakeup_cb), ptr::null_mut());
        mpv::mpv_render_context_set_update_callback(
            mpv_gl,
            Some(mpv_render_update_cb),
            ptr::null_mut(),
        );
    }

    // Загружаем видео.
    let cpath = CString::new(args.video.clone())
        .map_err(|_| "путь к видео содержит NUL".to_string())?;
    // Имя команды — литерал C-строки (`c"…"`): завершающий NUL ставит сам
    // компилятор, вручную дописывать его к байтовой строке незачем.
    let load_cmd: [*const c_char; 3] = [c"loadfile".as_ptr(), cpath.as_ptr(), ptr::null()];
    if unsafe { mpv::mpv_command(mpv, load_cmd.as_ptr()) } < 0 {
        cleanup(mpv_gl, mpv);
        return Err("loadfile провалился".into());
    }

    // --- eventfd + сигналы ---
    let efd = unsafe { libc::eventfd(0, libc::EFD_NONBLOCK | libc::EFD_CLOEXEC) };
    if efd < 0 {
        cleanup(mpv_gl, mpv);
        return Err("eventfd провалился".into());
    }
    EVENTFD.store(efd, Ordering::SeqCst);
    unsafe {
        libc::signal(libc::SIGTERM, handle_term as *const () as usize);
        libc::signal(libc::SIGINT, handle_term as *const () as usize);
    }

    // --- Поверхности: по одной на каждый подключённый вывод ---
    let mut state = State {
        registry_state,
        output_state,
        compositor,
        layer_shell,
        exit: false,
        egl_display,
        egl_config,
        egl_context,
        views: Vec::new(),
        render_pending: false,
    };

    // Один обход очереди: он и доносит геометрию выводов (положение, размер,
    // масштаб), и заводит поверхности — new_output зовётся на каждый уже
    // существующий вывод. Без обхода положение (0,0) ещё неизвестно, и
    // «основной экран» выродился бы в «первый попавшийся».
    if event_queue.roundtrip(&mut state).is_err() {
        cleanup(mpv_gl, mpv);
        return Err("Wayland: первый обход очереди не удался".into());
    }
    // Страховка на случай, если вывод не прошёл через new_output.
    let known: Vec<wl_output::WlOutput> = state.output_state.outputs().collect();
    for output in known {
        state.add_view(&qh, output);
    }
    state.debug_report("старт");

    // --- Главный цикл ---
    main_loop(&conn, &mut event_queue, &mut state, mpv, mpv_gl, efd);

    // Поверхности сносим до контекста mpv: рисовать больше нечем и некуда.
    while !state.views.is_empty() {
        state.drop_view(0);
    }
    let _ = event_queue.flush();

    // Чистое завершение: сначала render-контекст, потом ядро mpv.
    cleanup(mpv_gl, mpv);
    Ok(())
}

fn cleanup(mpv_gl: *mut mpv::mpv_render_context, mpv: *mut mpv::mpv_handle) {
    unsafe {
        if !mpv_gl.is_null() {
            mpv::mpv_render_context_free(mpv_gl);
        }
        if !mpv.is_null() {
            mpv::mpv_terminate_destroy(mpv);
        }
    }
}

// Пытаемся создать GLES2-контекст, при неудаче — GLES3.
fn create_egl_context(
    egl: &egl::Instance<egl::Static>,
    display: egl::Display,
) -> Result<(egl::Config, egl::Context), String> {
    // Атрибуты конфигурации: окно, GLES2-рендеринг, RGB8, без альфы.
    let cfg_attribs = [
        egl::SURFACE_TYPE,
        egl::WINDOW_BIT,
        egl::RENDERABLE_TYPE,
        egl::OPENGL_ES2_BIT,
        egl::RED_SIZE,
        8,
        egl::GREEN_SIZE,
        8,
        egl::BLUE_SIZE,
        8,
        egl::ALPHA_SIZE,
        0,
        egl::NONE,
    ];
    let config = egl
        .choose_first_config(display, &cfg_attribs)
        .map_err(|e| format!("EGL choose_first_config: {e:?}"))?
        .ok_or("EGL: подходящей конфигурации не найдено")?;

    // Пробуем версии клиента GLES: 2, затем 3.
    for ver in [2, 3] {
        let ctx_attribs = [egl::CONTEXT_MAJOR_VERSION, ver, egl::NONE];
        match egl.create_context(display, config, None, &ctx_attribs) {
            Ok(ctx) => return Ok((config, ctx)),
            Err(e) => {
                rexon_core::log_warn!(
                    "splash-overlay",
                    "create_context GLES{ver} не удался: {e:?}"
                );
            }
        }
    }
    Err("EGL: не создать контекст ни GLES2, ни GLES3".into())
}

// Рисует текущий кадр mpv в поверхность одного вывода.
fn render_view(
    state: &mut State,
    mpv_gl: *mut mpv::mpv_render_context,
    idx: usize,
) {
    let egl = EGL.get().expect("EGL");
    if !state.make_current_view(idx) {
        return;
    }
    let (bw, bh) = state.views[idx].buffer_size();
    if state.views[idx].needs_clear {
        clear_current_surface();
    }
    let mut fbo = mpv::mpv_opengl_fbo {
        fbo: 0,
        w: bw as c_int,
        h: bh as c_int,
        internal_format: 0,
    };
    let mut flip: c_int = 1;
    let mut params = [
        mpv::mpv_render_param {
            type_: mpv::MPV_RENDER_PARAM_OPENGL_FBO,
            data: &mut fbo as *mut _ as *mut c_void,
        },
        mpv::mpv_render_param {
            type_: mpv::MPV_RENDER_PARAM_FLIP_Y,
            data: &mut flip as *mut _ as *mut c_void,
        },
        mpv::mpv_render_param {
            type_: mpv::MPV_RENDER_PARAM_INVALID,
            data: ptr::null_mut(),
        },
    ];
    unsafe {
        mpv::mpv_render_context_render(mpv_gl, params.as_mut_ptr());
    }
    if let Some(surf) = state.views[idx].egl_surface {
        let _ = egl.swap_buffers(state.egl_display, surf);
    }
    state.views[idx].needs_redraw = false;
    state.views[idx].needs_clear = false;
    state.views[idx].first_frames = state.views[idx].first_frames.saturating_sub(1);
}

// Забирает кадр у mpv, никуда его не рисуя: единственный способ не уронить
// воспроизведение, когда экранов не осталось вовсе. Без этого очередь кадров
// встала бы, файл никогда не кончился бы, а оркестратор ждал бы конца
// заставки до своего таймаута.
fn drain_frame(state: &State, mpv_gl: *mut mpv::mpv_render_context) {
    state.release_current();
    let mut skip: c_int = 1;
    let mut params = [
        mpv::mpv_render_param {
            type_: mpv::MPV_RENDER_PARAM_SKIP_RENDERING,
            data: &mut skip as *mut _ as *mut c_void,
        },
        mpv::mpv_render_param {
            type_: mpv::MPV_RENDER_PARAM_INVALID,
            data: ptr::null_mut(),
        },
    ];
    unsafe {
        mpv::mpv_render_context_render(mpv_gl, params.as_mut_ptr());
    }
}

fn main_loop(
    conn: &Connection,
    event_queue: &mut wayland_client::EventQueue<State>,
    state: &mut State,
    mpv: *mut mpv::mpv_handle,
    mpv_gl: *mut mpv::mpv_render_context,
    efd: i32,
) {
    loop {
        // 1. Отправляем запросы и разбираем уже полученные события.
        let _ = event_queue.flush();
        if event_queue.dispatch_pending(state).is_err() {
            break;
        }
        if state.exit || TERMINATE.load(Ordering::SeqCst) {
            break;
        }

        // 2. Кадр: новый — во все готовые поверхности, иначе только тем,
        //    кто просил перерисовки (пришёл монитор, сменился масштаб).
        if state.render_pending {
            let flags = unsafe { mpv::mpv_render_context_update(mpv_gl) };
            let new_frame = flags & mpv::MPV_RENDER_UPDATE_FRAME != 0;
            let targets: Vec<usize> = state
                .views
                .iter()
                .enumerate()
                .filter(|(_, v)| {
                    v.configured
                        && v.egl_surface.is_some()
                        && (new_frame || v.needs_redraw || v.first_frames > 0)
                })
                .map(|(i, _)| i)
                .collect();

            if new_frame && targets.is_empty() {
                drain_frame(state, mpv_gl);
            } else {
                for idx in targets {
                    render_view(state, mpv_gl, idx);
                }
            }
            if new_frame {
                unsafe {
                    mpv::mpv_render_context_report_swap(mpv_gl);
                }
            }
            // Пока свежей поверхности не хватает кадров — идём на новый круг
            // сразу, не засыпая на poll.
            state.render_pending = state.views.iter().any(|v| v.first_frames > 0);
        }

        // 3. Разбираем события mpv.
        loop {
            let ev = unsafe { mpv::mpv_wait_event(mpv, 0.0) };
            let id = unsafe { (*ev).event_id };
            if id == mpv::MPV_EVENT_NONE {
                break;
            }
            if id == mpv::MPV_EVENT_SHUTDOWN {
                state.exit = true;
            }
            // MPV_EVENT_END_FILE игнорируем: keep-open=yes держит последний кадр.
        }
        if state.exit || TERMINATE.load(Ordering::SeqCst) {
            break;
        }

        // 4. Готовим чтение wayland и ждём на poll(wl_fd, eventfd).
        match event_queue.prepare_read() {
            Some(guard) => {
                let wl_fd = conn.as_fd().as_raw_fd();
                let mut fds = [
                    libc::pollfd {
                        fd: wl_fd,
                        events: libc::POLLIN,
                        revents: 0,
                    },
                    libc::pollfd {
                        fd: efd,
                        events: libc::POLLIN,
                        revents: 0,
                    },
                ];
                let r = unsafe { libc::poll(fds.as_mut_ptr(), 2, -1) };
                if r < 0 {
                    // Прервано сигналом — проверим флаги в начале цикла.
                    drop(guard);
                    continue;
                }
                if fds[0].revents & libc::POLLIN != 0 {
                    let _ = guard.read();
                } else {
                    drop(guard);
                }
                if fds[1].revents & libc::POLLIN != 0 {
                    // Осушаем eventfd и планируем кадр.
                    let mut buf: u64 = 0;
                    unsafe {
                        libc::read(efd, &mut buf as *mut u64 as *mut c_void, 8);
                    }
                    state.render_pending = true;
                }
            }
            None => {
                // Есть неразобранные события — вернёмся к dispatch.
            }
        }
    }
}
