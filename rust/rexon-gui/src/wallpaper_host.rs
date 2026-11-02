// Хост видео-обоев на Rust: замена `python -m rexon.video_wallpaper`.
//
// Что это за кусок и почему он второй по очереди. LayerShellQt держит два
// флагманских сценария продукта — живые обои и видеозаставку входа, — и пока
// подступа к нему из Rust не было, ни один Rust-путь обоев не мог поднять
// окно. Сам подступ (шим `cpp/rexon_layer_shell.*` и QObject `WallpaperLayer`
// в `src/layer_shell.rs`) уже есть; здесь на нём собран настоящий хост,
// которому Python больше не нужен.
//
// Разделение обязанностей ровно то же, что во всём остальном ядре:
//
//   * ЧТО показывать и с какими значениями — `rexon-core::wallpaper`
//     (`video_qml_file`, `video_context_properties`, `video_host_reject_reason`,
//     `mmap_size`) и `rexon-core::wallpaper_mmap` (частота кадров, план X11,
//     раскладка кадра, признак сеанса Wayland). Своих чисел здесь нет;
//   * КАК положить окно на задний план — три способа, и все три сходятся в
//     одном QML-файле (`qml/WallpaperVideo.qml`) вместо трёх почти одинаковых:
//     слой ставит Rust, а не присоединённое свойство QML, и разница из
//     разметки уходит (правило #12).
//
// ⚠️ Починка при переносе (правило #16). Ядро запускало хост командой
// `… -m rexon.video_wallpaper <файл> --audio --volume 0.7`
// (`wallpaper_engine::video_native_plan`), а разбор аргументов на Python
// таких ключей не знал вовсе и выходил с ошибкой. То есть видео-обои СО
// ЗВУКОМ не запускались ни разу — ни на Wayland, ни на X11. Здесь ключи
// приняты и заводятся туда же, куда попадают свойства из `project.json`, а
// не отдельной веткой: громкость и признак звука решает один и тот же
// код ядра.

use rexon_core::wallpaper_engine::HostMode;
use serde_json::{Map, Value};

/// Мост cxx-qt: то, до чего из Rust не дотянуться, — идентификатор окна X11
/// и поток кадров окна в mmap-файл (см. `cpp/rexon_wallpaper_host.*`).
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    unsafe extern "C++" {
        include!("rexon_wallpaper_host.h");

        /// `winId()` окна: создаёт окно платформы, если его ещё нет, но не
        /// показывает его.
        ///
        /// # Safety
        /// `window` — живой QObject (обычно QWindow).
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_wp_window_id(window: *mut QObject) -> i64;

        /// # Safety
        /// `window` — живой QObject корня QML.
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_wp_mmap_stream_start(
            window: *mut QObject,
            video_output: *mut QObject,
            path: &QString,
            generation_path: &QString,
            width: i32,
            height: i32,
            interval_ms: i32,
            file_mode: u32,
            may_show_window: bool,
        ) -> bool;

        /// # Safety
        /// `window` — живой QObject корня QML.
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_wp_mmap_capture(window: *mut QObject) -> QString;

        /// # Safety
        /// `window` — живой QObject корня QML.
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_wp_mmap_nonblack_permille(window: *mut QObject) -> i64;

        /// # Safety
        /// `window` — живой QObject корня QML.
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_wp_mmap_frames(window: *mut QObject) -> i64;

        /// # Safety
        /// `window` — живой QObject корня QML.
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_wp_mmap_center_rgb(window: *mut QObject) -> i64;

        /// Сколько экранов сейчас у сеанса (0 — мониторов нет вовсе).
        fn rexon_wp_screen_count() -> i32;

        /// Закрывать ли приложение вместе с последним окном.
        fn rexon_wp_set_quit_on_last_window_closed(enabled: bool);

        // Flash-режим: голое окно под wgpu (обёртки — модуль `swf_window`).
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_wp_swf_window_new(
            width: i32,
            height: i32,
            screen_index: i32,
        ) -> *mut QObject;
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_wp_swf_window_show(window: *mut QObject);
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_wp_swf_window_close(window: *mut QObject);
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_wp_swf_window_exposed(window: *mut QObject) -> bool;
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_wp_swf_window_width(window: *mut QObject) -> i32;
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_wp_swf_window_height(window: *mut QObject) -> i32;
        fn rexon_wp_process_events();
        fn rexon_wp_wl_display() -> usize;
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_wp_wl_surface(window: *mut QObject) -> usize;
        fn rexon_wp_xcb_connection() -> usize;
    }

    extern "RustQt" {
        /// Способы пристроить окно на задний план, кроме слоя: сам слой
        /// ставит `WallpaperLayer` из `src/layer_shell.rs`. Объект не владеет
        /// окном — только настраивает переданное.
        #[qobject]
        #[qml_element]
        type WallpaperPlacement = super::WallpaperPlacementRust;
    }

    extern "RustQt" {
        /// X11: подготовить окно к роли рабочего стола — тип
        /// `_NET_WM_WINDOW_TYPE_DESKTOP`, состояние и «на всех столах».
        /// Звать ДО показа окна (`visible = true`): тип оконный менеджер
        /// читает при показе. Окно платформы создаётся здесь же.
        ///
        /// # Safety
        /// `window` — живой, ещё не показанный QWindow.
        #[qinvokable]
        #[cxx_name = "prepareX11Desktop"]
        unsafe fn prepare_x11_desktop(self: &WallpaperPlacement, window: *mut QObject) -> bool;

        /// X11: довести показанное окно до роли рабочего стола просьбами
        /// EWMH корневому окну. Звать ПОСЛЕ показа; повтор безвреден.
        ///
        /// # Safety
        /// `window` — живой показанный QWindow.
        #[qinvokable]
        #[cxx_name = "applyX11Desktop"]
        unsafe fn apply_x11_desktop(self: &WallpaperPlacement, window: *mut QObject) -> bool;

        /// X11: поставить показанное окно обоев выше или ниже окна значков
        /// рабочего стола — по тому, сквозной ли у значков фон (решение —
        /// `rexon_core::wallpaper_mmap::x11_restack_goal`). Звать после
        /// показа и потом периодически: окно значков пересоздаётся
        /// (перезапуск Caja, смена её настроек) и встаёт сверху. Стоит где
        /// надо — ничего не делает. Менеджер окон просьбу не исполнил
        /// несколько кругов подряд — одна запись в журнал и больше не
        /// просит, пока нижний слой не изменится. Истина — порядок
        /// проверен (или просьба отправлена).
        ///
        /// # Safety
        /// `window` — живой показанный QWindow.
        #[qinvokable]
        #[cxx_name = "keepX11Order"]
        unsafe fn keep_x11_order(self: &WallpaperPlacement, window: *mut QObject) -> bool;

        /// Запустить поток кадров окна в общий mmap-файл (GNOME/Cinnamon).
        ///
        /// `video_output` — VideoOutput, из приёмника которого брать кадр
        /// (видео-обои), либо `null` — сцену окна рисует QQuickRenderControl
        /// без окна на экране, на видеокарте (веб-обои). Видео иначе нельзя:
        /// на платформе `offscreen` VideoOutput не рисует, и снимок окна
        /// выходит чёрным. Окно QML в режиме потока не показывать.
        ///
        /// # Safety
        /// `window` — живой QObject корня QML; `video_output` — живой
        /// VideoOutput либо null.
        #[qinvokable]
        #[cxx_name = "startMmapStream"]
        unsafe fn start_mmap_stream(
            self: &WallpaperPlacement,
            window: *mut QObject,
            video_output: *mut QObject,
            path: &QString,
            width: i32,
            height: i32,
            fps: i32,
        ) -> bool;

        /// Центральный пиксель последнего кадра в файле, 0xRRGGBB; -1 —
        /// кадров не было. Для самопроверки: чёрные кадры прежде считались
        /// успехом.
        ///
        /// # Safety
        /// `window` — живой QObject корня QML.
        #[qinvokable]
        #[cxx_name = "mmapCenterRgb"]
        unsafe fn mmap_center_rgb(self: &WallpaperPlacement, window: *mut QObject) -> f64;

        /// Сколько кадров ушло в файл; -1 — потока нет.
        ///
        /// ⚠️ Возврат `f64`, а не `i64`: 64-битное целое наружу в QML не
        /// выпускаем (разбор и границы точности — `src/qvalue.rs`). Счётчик
        /// кадров до потолка `double` (2^53) не дойдёт и за век работы.
        ///
        /// # Safety
        /// `window` — живой QObject корня QML.
        #[qinvokable]
        #[cxx_name = "mmapFrames"]
        unsafe fn mmap_frames(self: &WallpaperPlacement, window: *mut QObject) -> f64;

        /// Чем снимается кадр потока (для отчёта самопроверки): видеокарта
        /// через QQuickRenderControl, программный рендер, приёмник видео или
        /// снимок окна. Пусто — потока нет.
        ///
        /// # Safety
        /// `window` — живой QObject корня QML.
        #[qinvokable]
        #[cxx_name = "mmapCapture"]
        unsafe fn mmap_capture(self: &WallpaperPlacement, window: *mut QObject) -> QString;

        /// Доля не-чёрных пикселей последнего кадра, ‰; -1 — кадров не было.
        /// Самопроверка по ней не принимает чёрные кадры: центр бывает
        /// чёрным и у живой страницы.
        ///
        /// # Safety
        /// `window` — живой QObject корня QML.
        #[qinvokable]
        #[cxx_name = "mmapNonBlackPermille"]
        unsafe fn mmap_nonblack_permille(self: &WallpaperPlacement, window: *mut QObject) -> f64;
    }
}

/// Состояния у объекта нет: он только исполняет над переданным окном.
#[derive(Default)]
pub struct WallpaperPlacementRust;

/// Номер окна X11; `winId()` создаёт окно платформы, если его ещё нет (без
/// показа). `None` — окна нет или номер не выдан (причина в журнале).
///
/// # Safety
/// `window` — живой QObject либо null.
unsafe fn x11_window_id_of(window: *mut qobject::QObject) -> Option<i64> {
    if window.is_null() {
        return None;
    }
    let id = unsafe { qobject::rexon_wp_window_id(window) };
    if id == 0 {
        rexon_core::log_warn!("wallpaper-host", "REXON-WPH-X11 окно без идентификатора X11");
        return None;
    }
    Some(id)
}

impl qobject::WallpaperPlacement {
    unsafe fn prepare_x11_desktop(&self, window: *mut qobject::QObject) -> bool {
        // SAFETY: контракт invokable — живой QWindow.
        match unsafe { x11_window_id_of(window) } {
            Some(id) => prepare_x11_desktop_layer(id),
            None => false,
        }
    }

    unsafe fn apply_x11_desktop(&self, window: *mut qobject::QObject) -> bool {
        // SAFETY: контракт invokable — живой QWindow.
        match unsafe { x11_window_id_of(window) } {
            Some(id) => apply_x11_desktop_layer(id),
            None => false,
        }
    }

    unsafe fn keep_x11_order(&self, window: *mut qobject::QObject) -> bool {
        // SAFETY: контракт invokable — живой QWindow.
        match unsafe { x11_window_id_of(window) } {
            Some(id) => x11_desktop::keep_order(id),
            None => false,
        }
    }

    unsafe fn start_mmap_stream(
        &self,
        window: *mut qobject::QObject,
        video_output: *mut qobject::QObject,
        path: &cxx_qt_lib::QString,
        width: i32,
        height: i32,
        fps: i32,
    ) -> bool {
        if window.is_null() {
            return false;
        }
        // Раскладку кадра, интервал тика и путь счётчика поколения считает
        // ядро — своих чисел здесь нет намеренно: кривой размер кадра уже
        // однажды оставлял человека без обоев, и искать, какая из двух копий
        // победила, было негде.
        let plan = rexon_core::wallpaper_mmap::prime_plan(i64::from(width), i64::from(height));
        let interval = rexon_core::wallpaper_mmap::headless_tick_ms(i64::from(fps));
        let generation = cxx_qt_lib::QString::from(
            &rexon_core::wallpaper_mmap::frame_generation_path(&path.to_string()),
        );
        // Показать окно ради снимка можно только на `offscreen`; платформу к
        // этому месту выставил `prepare_qpa_platform` до создания приложения.
        let may_show_window = rexon_core::wallpaper_mmap::stream_window_may_show(
            &std::env::var("QT_QPA_PLATFORM").unwrap_or_default(),
        );
        unsafe {
            qobject::rexon_wp_mmap_stream_start(
                window,
                video_output,
                path,
                &generation,
                plan.width as i32,
                plan.height as i32,
                interval as i32,
                plan.mode,
                may_show_window,
            )
        }
    }

    unsafe fn mmap_capture(&self, window: *mut qobject::QObject) -> cxx_qt_lib::QString {
        if window.is_null() {
            return cxx_qt_lib::QString::default();
        }
        unsafe { qobject::rexon_wp_mmap_capture(window) }
    }

    unsafe fn mmap_nonblack_permille(&self, window: *mut qobject::QObject) -> f64 {
        if window.is_null() {
            return -1.0;
        }
        crate::qvalue::i64_to_qml(unsafe { qobject::rexon_wp_mmap_nonblack_permille(window) })
    }

    unsafe fn mmap_frames(&self, window: *mut qobject::QObject) -> f64 {
        if window.is_null() {
            return -1.0;
        }
        crate::qvalue::i64_to_qml(unsafe { qobject::rexon_wp_mmap_frames(window) })
    }

    unsafe fn mmap_center_rgb(&self, window: *mut qobject::QObject) -> f64 {
        if window.is_null() {
            return -1.0;
        }
        crate::qvalue::i64_to_qml(unsafe { qobject::rexon_wp_mmap_center_rgb(window) })
    }
}

/// Подготовить окно X11 к роли рабочего стола — ДО его показа: тип
/// `_NET_WM_WINDOW_TYPE_DESKTOP`, состояние (без панели задач и
/// переключателя, на всех столах) и `_NET_WM_DESKTOP` = все столы.
///
/// Окно к этому времени уже создано платформой (`winId()`), но не показано:
/// EWMH разрешает клиенту ставить такие свойства только неотображённому окну,
/// а тип оконный менеджер читает при показе. Qt при показе своё
/// `_NET_WM_STATE` с уже записанным сливает, а не затирает.
pub fn prepare_x11_desktop_layer(window_id: i64) -> bool {
    x11_desktop::before_map(window_id)
}

/// Довести уже показанное окно X11 до роли рабочего стола: просьбы EWMH
/// корневому окну (`_NET_WM_STATE` с `_NET_WM_STATE_ADD`, `_NET_WM_DESKTOP`)
/// и повтор типа окна. Повторный вызов безвреден.
///
/// План (что и в каком порядке) строит ядро —
/// [`rexon_core::wallpaper_mmap::x11_desktop_plan`]; здесь только запросы
/// к серверу. Один исполнитель на видео-, веб- и Flash-обои (правило #12).
pub fn apply_x11_desktop_layer(window_id: i64) -> bool {
    x11_desktop::after_map(window_id)
}

/// Фон X11 у ещё не показанного окна — сплошной цвет `rgb` (0xRRGGBB).
///
/// Зачем. Qt создаёт окна X11 без фона (`BackPixmap None`): до первого кадра
/// сцены сервер ничего в окне не рисует, и показанное окно выглядит
/// прозрачным — сквозь него виден стол, под композитором чёрное или
/// мусор. С цветом фона сервер сам заливает окно в момент показа, ещё до
/// первого кадра Qt, и до кадра окно стоит в краске скина. Тот же цвет
/// сервер кладёт и на открывшуюся при растяжении полосу — она не мигает.
///
/// Звать после `winId()` и до показа. `false` — не X11 (Wayland, offscreen):
/// там у окна свой путь показа и делать нечего, в журнал не пишем.
pub fn paint_x11_background(window_id: i64, rgb: u32) -> bool {
    if qobject::rexon_wp_xcb_connection() == 0 {
        return false;
    }
    x11_desktop::background(window_id, rgb)
}

/// Исполнитель плана X11 на соединении xcb самого Qt.
///
/// ⚠️ Почему соединение Qt, а не своё. Запросы одного соединения сервер
/// исполняет по порядку: свойства, записанные здесь до показа, гарантированно
/// лягут раньше `MapWindow`, который Qt пошлёт по тому же соединению. Своё
/// соединение (или `xprop`) так не умеет — окно, созданное Qt, могло ещё не
/// дойти до сервера, а показ — уже случиться.
///
/// libxcb не линкуется: функции берутся у той копии, что уже загрузил
/// платформенный модуль Qt (`dlopen` с `RTLD_NOLOAD`). Не X11 — ничего не
/// делаем и честно говорим об этом.
mod x11_desktop {
    use std::ffi::{c_char, c_int, c_uint, c_void, CString};
    use std::sync::OnceLock;

    use rexon_core::wallpaper_mmap as wm;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Cookie {
        sequence: c_uint,
    }

    #[repr(C)]
    struct InternAtomReply {
        response_type: u8,
        pad0: u8,
        sequence: u16,
        length: u32,
        atom: u32,
    }

    #[repr(C)]
    struct QueryTreeReply {
        response_type: u8,
        pad0: u8,
        sequence: u16,
        length: u32,
        root: u32,
        parent: u32,
        children_len: u16,
        pad1: [u8; 14],
    }

    #[repr(C)]
    struct GenericError {
        response_type: u8,
        error_code: u8,
        sequence: u16,
    }

    #[repr(C)]
    struct GetPropertyReply {
        response_type: u8,
        format: u8,
        sequence: u16,
        length: u32,
        kind: u32,
        bytes_after: u32,
        value_len: u32,
        pad0: [u8; 12],
    }

    #[repr(C)]
    struct GetGeometryReply {
        response_type: u8,
        depth: u8,
        sequence: u16,
        length: u32,
        root: u32,
        x: i16,
        y: i16,
        width: u16,
        height: u16,
        border_width: u16,
        pad0: [u8; 2],
    }

    #[repr(C)]
    struct GetSelectionOwnerReply {
        response_type: u8,
        pad0: u8,
        sequence: u16,
        length: u32,
        owner: u32,
    }

    /// Предопределённые атомы X11 (`xproto.h`).
    const XA_STRING: u32 = 31;
    const XA_WINDOW: u32 = 33;
    const XA_WM_CLASS: u32 = 67;
    /// `XCB_CW_EVENT_MASK`, `XCB_EVENT_MASK_PROPERTY_CHANGE`.
    const CW_EVENT_MASK: u32 = 1 << 11;
    const PROPERTY_CHANGE_MASK: u32 = 1 << 22;
    /// `XCB_PROP_MODE_APPEND`.
    const PROP_MODE_APPEND: u8 = 2;
    /// Код события PropertyNotify.
    const PROPERTY_NOTIFY: u8 = 28;
    /// `XCB_CONFIG_WINDOW_STACK_MODE`.
    const CONFIG_WINDOW_STACK_MODE: u16 = 1 << 6;

    type Conn = *mut c_void;
    type Err = *mut *mut GenericError;

    struct Xcb {
        intern_atom: unsafe extern "C" fn(Conn, u8, u16, *const c_char) -> Cookie,
        intern_atom_reply: unsafe extern "C" fn(Conn, Cookie, Err) -> *mut InternAtomReply,
        change_property_checked:
            unsafe extern "C" fn(Conn, u8, u32, u32, u32, u8, u32, *const c_void) -> Cookie,
        send_event_checked: unsafe extern "C" fn(Conn, u8, u32, u32, *const c_char) -> Cookie,
        query_tree: unsafe extern "C" fn(Conn, u32) -> Cookie,
        query_tree_reply: unsafe extern "C" fn(Conn, Cookie, Err) -> *mut QueryTreeReply,
        request_check: unsafe extern "C" fn(Conn, Cookie) -> *mut GenericError,
        flush: unsafe extern "C" fn(Conn) -> c_int,
        connection_has_error: unsafe extern "C" fn(Conn) -> c_int,
        get_property: unsafe extern "C" fn(Conn, u8, u32, u32, u32, u32, u32) -> Cookie,
        get_property_reply: unsafe extern "C" fn(Conn, Cookie, Err) -> *mut GetPropertyReply,
        get_property_value: unsafe extern "C" fn(*const GetPropertyReply) -> *mut c_void,
        get_property_value_length: unsafe extern "C" fn(*const GetPropertyReply) -> c_int,
        get_geometry: unsafe extern "C" fn(Conn, u32) -> Cookie,
        get_geometry_reply: unsafe extern "C" fn(Conn, Cookie, Err) -> *mut GetGeometryReply,
        get_selection_owner: unsafe extern "C" fn(Conn, u32) -> Cookie,
        get_selection_owner_reply: unsafe extern "C" fn(Conn, Cookie, Err) -> *mut GetSelectionOwnerReply,
        configure_window_checked: unsafe extern "C" fn(Conn, u32, u16, *const c_void) -> Cookie,
        change_property: unsafe extern "C" fn(Conn, u8, u32, u32, u32, u8, u32, *const c_void) -> Cookie,
        change_window_attributes: unsafe extern "C" fn(Conn, u32, u32, *const c_void) -> Cookie,
        connect: unsafe extern "C" fn(*const c_char, *mut c_int) -> Conn,
        disconnect: unsafe extern "C" fn(Conn),
        wait_for_event: unsafe extern "C" fn(Conn) -> *mut c_void,
    }

    // SAFETY: только указатели на функции библиотеки, которая не выгружается.
    unsafe impl Send for Xcb {}
    unsafe impl Sync for Xcb {}

    fn xcb() -> Option<&'static Xcb> {
        static XCB: OnceLock<Option<Xcb>> = OnceLock::new();
        XCB.get_or_init(|| {
            // SAFETY: dlopen/dlsym с корректными C-строками; NOLOAD не грузит
            // ничего нового, ссылка на библиотеку держится до конца процесса.
            unsafe {
                let lib = libc::dlopen(c"libxcb.so.1".as_ptr(), libc::RTLD_NOW | libc::RTLD_NOLOAD);
                if lib.is_null() {
                    return None;
                }
                macro_rules! sym {
                    ($name:literal) => {{
                        let p = libc::dlsym(lib, $name.as_ptr());
                        if p.is_null() {
                            return None;
                        }
                        std::mem::transmute::<*mut c_void, _>(p)
                    }};
                }
                Some(Xcb {
                    intern_atom: sym!(c"xcb_intern_atom"),
                    intern_atom_reply: sym!(c"xcb_intern_atom_reply"),
                    change_property_checked: sym!(c"xcb_change_property_checked"),
                    send_event_checked: sym!(c"xcb_send_event_checked"),
                    query_tree: sym!(c"xcb_query_tree"),
                    query_tree_reply: sym!(c"xcb_query_tree_reply"),
                    request_check: sym!(c"xcb_request_check"),
                    flush: sym!(c"xcb_flush"),
                    connection_has_error: sym!(c"xcb_connection_has_error"),
                    get_property: sym!(c"xcb_get_property"),
                    get_property_reply: sym!(c"xcb_get_property_reply"),
                    get_property_value: sym!(c"xcb_get_property_value"),
                    get_property_value_length: sym!(c"xcb_get_property_value_length"),
                    get_geometry: sym!(c"xcb_get_geometry"),
                    get_geometry_reply: sym!(c"xcb_get_geometry_reply"),
                    get_selection_owner: sym!(c"xcb_get_selection_owner"),
                    get_selection_owner_reply: sym!(c"xcb_get_selection_owner_reply"),
                    configure_window_checked: sym!(c"xcb_configure_window_checked"),
                    change_property: sym!(c"xcb_change_property"),
                    change_window_attributes: sym!(c"xcb_change_window_attributes"),
                    connect: sym!(c"xcb_connect"),
                    disconnect: sym!(c"xcb_disconnect"),
                    wait_for_event: sym!(c"xcb_wait_for_event"),
                })
            }
        })
        .as_ref()
    }

    /// Соединение Qt и номер окна; `None` — не X11 (причина уже в журнале).
    fn session(window_id: i64) -> Option<(&'static Xcb, Conn, u32)> {
        let Ok(window) = u32::try_from(window_id) else {
            rexon_core::log_warn!("wallpaper-host", "REXON-WPH-X11 номер окна вне X11: {window_id}");
            return None;
        };
        if window == 0 {
            return None;
        }
        let conn = super::qobject::rexon_wp_xcb_connection() as Conn;
        if conn.is_null() {
            rexon_core::log_warn!(
                "wallpaper-host",
                "REXON-WPH-X11 у приложения нет соединения X11 — окно рабочим столом не станет"
            );
            return None;
        }
        let Some(x) = xcb() else {
            rexon_core::log_warn!(
                "wallpaper-host",
                "REXON-WPH-X11 libxcb не найдена в процессе — окно рабочим столом не станет"
            );
            return None;
        };
        // SAFETY: живое соединение Qt.
        if unsafe { (x.connection_has_error)(conn) } != 0 {
            rexon_core::log_warn!("wallpaper-host", "REXON-WPH-X11 соединение X11 сломано");
            return None;
        }
        Some((x, conn, window))
    }

    /// Номера атомов плана: все запросы разом, потом ответы.
    fn intern_all(x: &Xcb, conn: Conn, names: &[&'static str]) -> Option<Vec<(&'static str, u32)>> {
        let cnames: Vec<CString> = names.iter().map(|n| CString::new(*n).unwrap()).collect();
        // SAFETY: строки живут до конца функции; длина имени < u16::MAX.
        let cookies: Vec<Cookie> = cnames
            .iter()
            .map(|c| unsafe { (x.intern_atom)(conn, 0, c.as_bytes().len() as u16, c.as_ptr()) })
            .collect();
        let mut out = Vec::with_capacity(names.len());
        let mut ok = true;
        for (name, cookie) in names.iter().zip(cookies) {
            let mut err: *mut GenericError = std::ptr::null_mut();
            // SAFETY: ответ и ошибку выделяет libxcb, освобождаем free().
            let reply = unsafe { (x.intern_atom_reply)(conn, cookie, &mut err) };
            if reply.is_null() {
                ok = false;
                rexon_core::log_warn!("wallpaper-host", "REXON-WPH-X11 сервер не дал атом {name}");
            } else {
                out.push((*name, unsafe { (*reply).atom }));
                unsafe { libc::free(reply.cast()) };
            }
            if !err.is_null() {
                unsafe { libc::free(err.cast()) };
            }
        }
        ok.then_some(out)
    }

    fn atom_of(atoms: &[(&'static str, u32)], name: &str) -> Option<u32> {
        atoms.iter().find(|(n, _)| *n == name).map(|(_, a)| *a)
    }

    /// Дождаться ответа сервера на проверяемый запрос; `false` — отказ.
    fn checked(x: &Xcb, conn: Conn, cookie: Cookie, what: &str) -> bool {
        // SAFETY: cookie от *_checked этого же соединения.
        let err = unsafe { (x.request_check)(conn, cookie) };
        if err.is_null() {
            return true;
        }
        let code = unsafe { (*err).error_code };
        unsafe { libc::free(err.cast()) };
        rexon_core::log_warn!("wallpaper-host", "REXON-WPH-X11 {what}: сервер отказал, ошибка X11 {code}");
        false
    }

    fn set_properties(
        x: &Xcb,
        conn: Conn,
        window: u32,
        atoms: &[(&'static str, u32)],
        props: &[wm::X11Property],
    ) -> bool {
        let mut ok = true;
        for p in props {
            let (Some(name), Some(values)) = (
                atom_of(atoms, p.name),
                wm::x11_resolve_words(&p.values, |a| atom_of(atoms, a)),
            ) else {
                ok = false;
                continue;
            };
            // SAFETY: values живёт до ответа; формат 32 — слова u32.
            let cookie = unsafe {
                (x.change_property_checked)(
                    conn,
                    0, // XCB_PROP_MODE_REPLACE
                    window,
                    name,
                    p.kind,
                    32,
                    values.len() as u32,
                    values.as_ptr().cast(),
                )
            };
            ok &= checked(x, conn, cookie, p.name);
        }
        ok
    }

    pub(super) fn before_map(window_id: i64) -> bool {
        let Some((x, conn, window)) = session(window_id) else {
            return false;
        };
        let plan = wm::x11_desktop_plan();
        let Some(atoms) = intern_all(x, conn, &wm::x11_desktop_plan_atoms(&plan)) else {
            return false;
        };
        let ok = set_properties(x, conn, window, &atoms, &plan.before_map);
        // SAFETY: живое соединение.
        unsafe { (x.flush)(conn) };
        ok
    }

    /// `XCB_CW_BACK_PIXEL`.
    const CW_BACK_PIXEL: u32 = 1 << 1;

    pub(super) fn background(window_id: i64, rgb: u32) -> bool {
        let Some((x, conn, window)) = session(window_id) else {
            return false;
        };
        // Пиксель TrueColor: у окна глубины 32 (с альфой) старший байт —
        // непрозрачность, без него фон под композитором был бы прозрачным.
        let pixel = if depth(x, conn, window) == 32 { 0xff00_0000 | rgb } else { rgb & 0x00ff_ffff };
        // SAFETY: живое соединение; значение — один u32 под маску.
        unsafe {
            (x.change_window_attributes)(conn, window, CW_BACK_PIXEL, (&pixel as *const u32).cast());
            (x.flush)(conn);
        }
        true
    }

    pub(super) fn after_map(window_id: i64) -> bool {
        let Some((x, conn, window)) = session(window_id) else {
            return false;
        };
        let plan = wm::x11_desktop_plan();
        let Some(atoms) = intern_all(x, conn, &wm::x11_desktop_plan_atoms(&plan)) else {
            return false;
        };
        let mut ok = set_properties(x, conn, window, &atoms, &plan.after_map_properties);

        // Корневое окно — того экрана X, на котором живёт окно.
        let mut err: *mut GenericError = std::ptr::null_mut();
        // SAFETY: ответ и ошибку выделяет libxcb, освобождаем free().
        let tree = unsafe { (x.query_tree_reply)(conn, (x.query_tree)(conn, window), &mut err) };
        if !err.is_null() {
            unsafe { libc::free(err.cast()) };
        }
        if tree.is_null() {
            rexon_core::log_warn!("wallpaper-host", "REXON-WPH-X11 окно {window} не найдено на сервере");
            return false;
        }
        let root = unsafe { (*tree).root };
        unsafe { libc::free(tree.cast()) };

        for m in &plan.after_map_messages {
            let (Some(kind), Some(words)) = (
                atom_of(&atoms, m.message_type),
                wm::x11_resolve_words(&m.data, |a| atom_of(&atoms, a)),
            ) else {
                ok = false;
                continue;
            };
            let data: [u32; 5] = [words[0], words[1], words[2], words[3], words[4]];
            let event = wm::x11_client_message_bytes(window, kind, data);
            // SAFETY: событие — 32 байта, как требует xcb_send_event.
            let cookie = unsafe {
                (x.send_event_checked)(conn, 0, root, wm::ROOT_MESSAGE_EVENT_MASK, event.as_ptr().cast())
            };
            ok &= checked(x, conn, cookie, m.message_type);
        }
        // SAFETY: живое соединение.
        unsafe { (x.flush)(conn) };
        // Показанное окно встаёт над всеми окнами стола — в том числе над
        // сквозным окном значков, под которым ему место.
        keep_order(window_id);
        ok
    }

    // ── порядок окна обоев и окна значков ─────────────────────────────────

    /// Атомы сверки порядка. Номер экрана нужен для выбора композитора.
    fn order_atom_names(screen: u32) -> Vec<&'static str> {
        static CM: OnceLock<&'static str> = OnceLock::new();
        let cm = *CM.get_or_init(|| Box::leak(wm::x11_compositor_selection(screen).into_boxed_str()));
        let mut names = vec![
            "_NET_CLIENT_LIST_STACKING",
            "_NET_SUPPORTED",
            "_NET_RESTACK_WINDOW",
            "_NET_WM_WINDOW_TYPE",
            wm::X11_DESKTOP_WINDOW_TYPE,
            "_NET_WM_USER_TIME",
            "_NET_WM_USER_TIME_WINDOW",
            cm,
        ];
        names.extend(wm::X11_ICONS_WINDOW_ROOT_PROPERTIES);
        names
    }

    /// Атомы на соединении Qt: номера живут, пока жив сервер, — узнаются
    /// один раз.
    fn order_atoms(x: &Xcb, conn: Conn) -> Option<&'static [(&'static str, u32)]> {
        static ATOMS: OnceLock<Option<Vec<(&'static str, u32)>>> = OnceLock::new();
        ATOMS
            .get_or_init(|| {
                let screen = wm::x11_screen_number(&std::env::var("DISPLAY").unwrap_or_default());
                intern_all(x, conn, &order_atom_names(screen))
            })
            .as_deref()
    }

    /// Свойство окна как 32-битные слова (`kind` — тип свойства). Нет
    /// свойства или ошибка — пусто.
    fn property32(x: &Xcb, conn: Conn, window: u32, property: u32, kind: u32) -> Vec<u32> {
        let mut err: *mut GenericError = std::ptr::null_mut();
        // SAFETY: ответ и ошибку выделяет libxcb, освобождаем free().
        let reply = unsafe {
            (x.get_property_reply)(conn, (x.get_property)(conn, 0, window, property, kind, 0, 8192), &mut err)
        };
        if !err.is_null() {
            unsafe { libc::free(err.cast()) };
        }
        if reply.is_null() {
            return Vec::new();
        }
        // SAFETY: значение живёт вместе с ответом; длина — в байтах.
        let out = unsafe {
            let len = (x.get_property_value_length)(reply).max(0) as usize / 4;
            let data = (x.get_property_value)(reply) as *const u32;
            if (*reply).format == 32 && !data.is_null() {
                std::slice::from_raw_parts(data, len).to_vec()
            } else {
                Vec::new()
            }
        };
        unsafe { libc::free(reply.cast()) };
        out
    }

    /// `WM_CLASS` окна: экземпляр и класс.
    fn wm_class(x: &Xcb, conn: Conn, window: u32) -> (String, String) {
        let mut err: *mut GenericError = std::ptr::null_mut();
        // SAFETY: как в `property32`; формат 8 — байты.
        let reply = unsafe {
            (x.get_property_reply)(
                conn,
                (x.get_property)(conn, 0, window, XA_WM_CLASS, XA_STRING, 0, 256),
                &mut err,
            )
        };
        if !err.is_null() {
            unsafe { libc::free(err.cast()) };
        }
        if reply.is_null() {
            return Default::default();
        }
        let bytes = unsafe {
            let len = (x.get_property_value_length)(reply).max(0) as usize;
            let data = (x.get_property_value)(reply) as *const u8;
            if data.is_null() { Vec::new() } else { std::slice::from_raw_parts(data, len).to_vec() }
        };
        unsafe { libc::free(reply.cast()) };
        let mut parts = bytes.split(|b| *b == 0);
        let instance = String::from_utf8_lossy(parts.next().unwrap_or_default()).into_owned();
        let class = String::from_utf8_lossy(parts.next().unwrap_or_default()).into_owned();
        (instance, class)
    }

    /// Глубина окна; 0 — не узнали.
    fn depth(x: &Xcb, conn: Conn, window: u32) -> u8 {
        let mut err: *mut GenericError = std::ptr::null_mut();
        // SAFETY: ответ и ошибку выделяет libxcb, освобождаем free().
        let reply = unsafe { (x.get_geometry_reply)(conn, (x.get_geometry)(conn, window), &mut err) };
        if !err.is_null() {
            unsafe { libc::free(err.cast()) };
        }
        if reply.is_null() {
            return 0;
        }
        let depth = unsafe { (*reply).depth };
        unsafe { libc::free(reply.cast()) };
        depth
    }

    /// Есть ли у выбора владелец (композитор держит `_NET_WM_CM_Sn`).
    fn selection_owned(x: &Xcb, conn: Conn, selection: u32) -> bool {
        let mut err: *mut GenericError = std::ptr::null_mut();
        // SAFETY: ответ и ошибку выделяет libxcb, освобождаем free().
        let reply =
            unsafe { (x.get_selection_owner_reply)(conn, (x.get_selection_owner)(conn, selection), &mut err) };
        if !err.is_null() {
            unsafe { libc::free(err.cast()) };
        }
        if reply.is_null() {
            return false;
        }
        let owner = unsafe { (*reply).owner };
        unsafe { libc::free(reply.cast()) };
        owner != 0
    }

    /// Корневое окно экрана, на котором живёт окно.
    fn root_of(x: &Xcb, conn: Conn, window: u32) -> Option<u32> {
        let mut err: *mut GenericError = std::ptr::null_mut();
        // SAFETY: ответ и ошибку выделяет libxcb, освобождаем free().
        let tree = unsafe { (x.query_tree_reply)(conn, (x.query_tree)(conn, window), &mut err) };
        if !err.is_null() {
            unsafe { libc::free(err.cast()) };
        }
        if tree.is_null() {
            return None;
        }
        let root = unsafe { (*tree).root };
        unsafe { libc::free(tree.cast()) };
        Some(root)
    }

    /// Свежее время сервера X. Marco (как и Metacity) не исполняет просьбу о
    /// порядке от окна, чьё `_NET_WM_USER_TIME` старше, чем у активного
    /// окна чужого приложения (`meta_window_configure_request`); время
    /// сервера узнаётся только из события, а события соединения Qt разбирает
    /// Qt. Поэтому — своё короткое соединение: пустая дописка к свойству
    /// корневого окна и время из пришедшего PropertyNotify.
    fn server_time(x: &Xcb, root: u32) -> Option<u32> {
        // SAFETY: своё соединение, закрывается здесь же; события — free().
        unsafe {
            let own = (x.connect)(std::ptr::null(), std::ptr::null_mut());
            if own.is_null() {
                return None;
            }
            if (x.connection_has_error)(own) != 0 {
                (x.disconnect)(own);
                return None;
            }
            let time = (|| {
                let atoms = intern_all(x, own, &["_REXON_SERVER_TIME"])?;
                let mask: u32 = PROPERTY_CHANGE_MASK;
                (x.change_window_attributes)(own, root, CW_EVENT_MASK, (&mask as *const u32).cast());
                (x.change_property)(own, PROP_MODE_APPEND, root, atoms[0].1, XA_STRING, 8, 0, std::ptr::null());
                (x.flush)(own);
                for _ in 0..64 {
                    let ev = (x.wait_for_event)(own);
                    if ev.is_null() {
                        return None;
                    }
                    let bytes = std::slice::from_raw_parts(ev as *const u8, 16);
                    let is_property_notify = bytes[0] & 0x7f == PROPERTY_NOTIFY;
                    let time = u32::from_ne_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]);
                    libc::free(ev.cast());
                    if is_property_notify {
                        return Some(time);
                    }
                }
                None
            })();
            (x.disconnect)(own);
            time
        }
    }

    thread_local! {
        /// Наблюдения о чужих окнах по номеру — у окна они не меняются.
        static FACTS: std::cell::RefCell<std::collections::HashMap<u32, wm::X11ClientFacts>> =
            std::cell::RefCell::new(std::collections::HashMap::new());
        /// Последний записанный в журнал порядок (чтобы не писать одно и то же).
        static LAST_LOGGED: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
        /// Учёт просьб о порядке по окну обоев (окно на монитор).
        static TRACKERS: std::cell::RefCell<std::collections::HashMap<u32, wm::X11RestackTracker>> =
            std::cell::RefCell::new(std::collections::HashMap::new());
    }

    /// Менеджер окон не опустил обои под сквозные значки (xfwm4): один раз
    /// пересоздать окно значков переключением «значки выкл → вкл» — новое
    /// окно xfwm4 кладёт поверх обоев. В отдельном потоке: между «выкл» и
    /// «вкл» пауза, а сверка идёт в главном потоке Qt.
    fn recreate_icons_window(own: u32, goal: wm::X11Restack) {
        let (sibling, _) = goal.sibling_and_mode();
        let Some((schema, key)) = FACTS.with(|f| {
            f.borrow()
                .get(&sibling)
                .and_then(|f| rexon_core::wallpaper_backend::icons_window_toggle(&f.instance, &f.class))
        }) else {
            return;
        };
        if !TRACKERS.with(|t| t.borrow_mut().get_mut(&own).is_some_and(|t| t.take_icons_recreate(goal))) {
            return;
        }
        rexon_core::log_info!(
            "wallpaper-host",
            "REXON-WPH-X11 пересоздаю окно значков 0x{sibling:x} ({schema} {key}: выкл → вкл), чтобы оно встало над обоями"
        );
        let spawned = std::thread::Builder::new().name("rexon-icons-recreate".into()).spawn(move || {
            let mut sys = rexon_core::wallpaper_exec::LiveSys::new();
            let mut io = rexon_core::wallpaper_exec::DesktopIconsSys::new(&mut sys);
            if !rexon_core::wallpaper_backend::recreate_icons_window(&mut io, schema, key) {
                rexon_core::log_warn!("wallpaper-host", "REXON-WPH-X11 окно значков не пересоздано — значки останутся под обоями");
            }
        });
        if let Err(e) = spawned {
            rexon_core::log_warn!("wallpaper-host", "REXON-WPH-X11 поток пересоздания окна значков не запустился: {e}");
        }
    }

    pub(super) fn keep_order(window_id: i64) -> bool {
        let Some((x, conn, own)) = session(window_id) else {
            return false;
        };
        let Some(atoms) = order_atoms(x, conn) else {
            return false;
        };
        let atom = |name: &str| atom_of(atoms, name).unwrap_or(0);
        let Some(root) = root_of(x, conn, own) else {
            return false;
        };
        let stacking = property32(x, conn, root, atom("_NET_CLIENT_LIST_STACKING"), XA_WINDOW);
        if stacking.is_empty() {
            return false;
        }
        let announced: Vec<u32> = wm::X11_ICONS_WINDOW_ROOT_PROPERTIES
            .iter()
            .flat_map(|p| property32(x, conn, root, atom(p), XA_WINDOW))
            .collect();
        let screen = wm::x11_screen_number(&std::env::var("DISPLAY").unwrap_or_default());
        let cm = wm::x11_compositor_selection(screen);
        let compositing = selection_owned(x, conn, atom(&cm));
        let desktop_type = atom(wm::X11_DESKTOP_WINDOW_TYPE);

        let stack: Vec<(u32, wm::X11StackRole)> = FACTS.with(|facts| {
            let mut facts = facts.borrow_mut();
            facts.retain(|w, _| stacking.contains(w));
            stacking
                .iter()
                .map(|&w| {
                    if w == own {
                        return (w, wm::X11StackRole::Own);
                    }
                    let f = facts.entry(w).or_insert_with(|| {
                        let (instance, class) = wm_class(x, conn, w);
                        wm::X11ClientFacts {
                            desktop_type: property32(x, conn, w, atom("_NET_WM_WINDOW_TYPE"), wm::XA_ATOM)
                                .contains(&desktop_type),
                            announced_icons: false,
                            instance,
                            class,
                            depth: depth(x, conn, w),
                        }
                    });
                    f.announced_icons = announced.contains(&w);
                    (w, wm::x11_stack_role(f, compositing))
                })
                .collect()
        });

        let described: String = stack
            .iter()
            .filter(|(_, role)| *role != wm::X11StackRole::Other)
            .map(|(w, role)| format!("0x{w:x}={role:?}"))
            .collect::<Vec<_>>()
            .join(" ");
        let changed = LAST_LOGGED.with(|last| {
            let mut last = last.borrow_mut();
            let changed = *last != described;
            if changed {
                last.clone_from(&described);
            }
            changed
        });
        if changed {
            rexon_core::log_info!(
                "wallpaper-host",
                "REXON-WPH-X11 нижний слой (снизу вверх, композитор: {}): {described}",
                if compositing { "есть" } else { "нет" }
            );
        }

        let step = TRACKERS.with(|t| t.borrow_mut().entry(own).or_default().step(&stack, own));
        let goal = match step {
            None => return true,
            Some(wm::X11RestackStep::Ask(goal)) => goal,
            Some(wm::X11RestackStep::GiveUp(goal)) => {
                let (sibling, mode) = goal.sibling_and_mode();
                let where_ = if mode == wm::X11_STACK_BELOW { "под окно значков" } else { "над окном значков" };
                rexon_core::log_warn!(
                    "wallpaper-host",
                    "REXON-WPH-X11 оконный менеджер не поставил окно 0x{own:x} {where_} 0x{sibling:x} после {} просьб — больше не прошу, пока нижний слой не изменится",
                    wm::X11_RESTACK_TRIES
                );
                recreate_icons_window(own, goal);
                return false;
            }
            Some(wm::X11RestackStep::Quiet) => return false,
        };
        let (sibling, mode) = goal.sibling_and_mode();
        let where_ = if mode == wm::X11_STACK_BELOW { "под окно значков" } else { "над окном значков" };
        let supported = property32(x, conn, root, atom("_NET_SUPPORTED"), wm::XA_ATOM);
        let restack = atom("_NET_RESTACK_WINDOW");
        let ok = if restack != 0 && supported.contains(&restack) {
            rexon_core::log_info!("wallpaper-host", "REXON-WPH-X11 окно 0x{own:x} {where_} 0x{sibling:x} (_NET_RESTACK_WINDOW)");
            let event = wm::x11_client_message_bytes(own, restack, [wm::EWMH_SOURCE_PAGER, sibling, mode, 0, 0]);
            // SAFETY: событие — 32 байта, как требует xcb_send_event.
            let cookie = unsafe {
                (x.send_event_checked)(conn, 0, root, wm::ROOT_MESSAGE_EVENT_MASK, event.as_ptr().cast())
            };
            checked(x, conn, cookie, "_NET_RESTACK_WINDOW")
        } else {
            // Без `_NET_RESTACK_WINDOW` (Marco) — ConfigureWindow без соседа:
            // Marco соседа не смотрит вовсе и поднимает окно на верх его
            // слоя либо опускает на низ (`meta_window_raise`/`_lower`), а
            // окно значков — в том же слое. Сосед в запросе только навлёк бы
            // BadMatch, будь окно значков в рамке.
            rexon_core::log_info!("wallpaper-host", "REXON-WPH-X11 окно 0x{own:x} {where_} 0x{sibling:x} (ConfigureWindow)");
            let user_time_window = property32(x, conn, own, atom("_NET_WM_USER_TIME_WINDOW"), XA_WINDOW)
                .first()
                .copied()
                .unwrap_or(own);
            if let Some(time) = server_time(x, root) {
                // SAFETY: одно слово формата 32.
                let cookie = unsafe {
                    (x.change_property_checked)(
                        conn,
                        0,
                        user_time_window,
                        atom("_NET_WM_USER_TIME"),
                        wm::XA_CARDINAL,
                        32,
                        1,
                        (&time as *const u32).cast(),
                    )
                };
                checked(x, conn, cookie, "_NET_WM_USER_TIME");
            } else {
                rexon_core::log_warn!("wallpaper-host", "REXON-WPH-X11 время сервера не узнали — Marco может отказать в порядке");
            }
            // SAFETY: одно значение — stack_mode.
            let cookie = unsafe { (x.configure_window_checked)(conn, own, CONFIG_WINDOW_STACK_MODE, (&mode as *const u32).cast()) };
            checked(x, conn, cookie, "ConfigureWindow")
        };
        // SAFETY: живое соединение.
        unsafe { (x.flush)(conn) };
        ok
    }
}

/// Не закрывать приложение вместе с последним окном.
///
/// ⚠️ Обоям это обязательно, и вот почему. Окон у хоста столько, сколько
/// мониторов: отключили монитор — окно уничтожается. При умолчании Qt
/// («последнее окно закрыли — выходим») уход последнего монитора убил бы
/// весь процесс обоев, и вернувшийся через секунду монитор остался бы
/// пустым навсегда — перезапускать хост некому. Выход у обоев ровно один:
/// сигнал от исполнителя (`rexon-core::wallpaper_exec`), а у прогонов —
/// `Qt.exit` из QML.
///
/// Звать ПОСЛЕ создания `QGuiApplication`.
pub fn keep_alive_without_windows() {
    qobject::rexon_wp_set_quit_on_last_window_closed(false);
}

// ── Выход хоста ────────────────────────────────────────────────────────────

/// Сколько даём Qt на разбор, прежде чем выйти силой.
///
/// Три секунды: честный разбор укладывается в десятки миллисекунд (замер на
/// стенде), и даже разбор Chromium у веб-обоев — в доли секунды. Больше брать
/// нельзя: исполнитель обоев считает хост живым, если тот не умер за секунду
/// (`wallpaper_engine::SPAWN_ALIVE_GRACE_SEC`), и долгий отказ выглядел бы у
/// человека как «обои применились».
pub const QT_SHUTDOWN_GRACE: std::time::Duration = std::time::Duration::from_secs(3);

/// Код возврата, когда хост сломался изнутри (паника). 101 — то же число,
/// которым закрывается всякая паникующая программа на Rust, и оно намеренно
/// не пересекается с таблицей `wallpaper_engine::host_exit`.
pub const PANIC_EXIT: i32 = 101;

/// Приговор вынесен: назвать код и не дать Qt задержать выход навсегда.
///
/// ⚠️ Зачем это есть. Замер 16.09.2026, стенд X11 с двумя экранами,
/// сведёнными через `+xinerama`: хост обоев не заканчивался ВООБЩЕ — ни за
/// 120 секунд. Стек живого процесса показал, что главный поток стоит в
/// `QThread::wait()` внутри разрушения `QXcbEventQueue`, а сам поток очереди
/// событий висит в `xcb_wait_for_event()` и разбудить его уже нечем. То есть
/// зависал не показ, а РАЗБОР Qt на выходе. Навсегда висящий хост обоев —
/// худший исход из возможных: обоев нет, процесс жив, снять его некому.
/// Поэтому у разбора есть срок, а по его истечении — честная жалоба и выход
/// с тем же приговором.
pub fn verdict(code: i32) -> i32 {
    std::thread::spawn(move || {
        std::thread::sleep(QT_SHUTDOWN_GRACE);
        rexon_core::log_warn!(
            "wallpaper-host",
            "REXON-WPH-WARN Qt не отпускает хост обоев дольше {} с — закрываемся силой; \
             приговор прежний, код {code}",
            QT_SHUTDOWN_GRACE.as_secs()
        );
        // Буферы `_exit` не сбрасывает — делаем это сами, иначе жалоба
        // потеряется ровно там, где она нужнее всего.
        use std::io::Write;
        let _ = std::io::stderr().flush();
        // ⚠️ Именно `_exit`, а не `abort`: SIGABRT — это дамп и окно «программа
        // упала» перед человеком, а мы закрываемся по своему решению.
        unsafe { libc::_exit(code) };
    });
    code
}

/// Паника хоста обоев закрывает процесс сразу, не раскручивая стек.
///
/// ⚠️ Раскрутка стека прошла бы через разрушение `QGuiApplication`, а оно на
/// X11 ждёт поток очереди событий — и именно на этом ожидании хост зависал
/// навсегда (разбор — у [`verdict`]). Ни один вид обоев не выигрывает от
/// раскрутки: показывать после паники нечего, а исполнитель обоев
/// (`rexon-core::wallpaper_exec`) поднимет хост заново сам.
///
/// Звать первой строкой `main`, до всякого Qt.
pub fn install_panic_guard() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        previous(info);
        rexon_core::log_error!(
            "wallpaper-host",
            "REXON-WPH-ERROR хост обоев сломался и показывать дальше нечего — закрываемся. \
             Обои можно включить заново в Rexon."
        );
        use std::io::Write;
        let _ = std::io::stderr().flush();
        unsafe { libc::_exit(PANIC_EXIT) };
    }));
}

/// Завести журнал хоста обоев — ОБЩИЙ файл Rexon (`rexon_core::applog`,
/// `~/.local/state/rexon/rexon.log`), а не свой.
///
/// Почему общий: хост — продолжение действия человека в окне Rexon
/// («включить обои»), и разбирающему нужна одна хронология: что окно
/// попросило и что хост на это сделал, в одном файле, по времени. Строки
/// процессов различает номер процесса в каждой строке и область
/// `wallpaper-host`; кнопка «Скопировать журнал» берёт один файл. Ротация та
/// же — у файла она общая, соседний процесс её учитывает
/// (`applog::rotate_sink`).
///
/// Зачем вообще: исполнитель запускает хост с потоком ошибок в `/dev/null`
/// (`wallpaper_exec::LiveSys::spawn` — трубу на живых обоях никто не
/// читает), и до этой правки всё, что хост говорил о себе (`REXON-WPH-…`
/// из Rust и из QML, сообщения Qt и FFmpeg), пропадало без следа. Теперь
/// строки Rust — в файл, а дескриптор 2 при `/dev/null` уводится туда же
/// тем же путём, что у окна (`cli::install_qt_message_logger`), поэтому
/// доходят и строки QML. Из терминала поток ошибок не перехватывается.
///
/// ⚠️ Самопроверка (`--selftest`) в профиль не пишет — как и у окна.
pub fn install_host_log(selftest: bool) {
    if selftest {
        return;
    }
    rexon_core::applog::init();
    rexon_core::applog::install_fatal_signal_logger();
    rexon_core::cli::install_qt_message_logger();
}

/// Голое окно Flash-режима (`--mode swf`): создание, показ, размер и
/// native-хэндлы для wgpu. Шим — `cpp/rexon_wallpaper_host.*`; решения —
/// у вызывающего (`src/bin/wallpaper_host/swf.rs`).
pub mod swf_window {
    use super::qobject::{self, QObject};

    /// Окно без QML и без буфера Qt на экране `screen_index` (< 0 —
    /// основной); `width`/`height` ≤ 0 — размер и угол этого экрана. Нужен
    /// живой `QGuiApplication`.
    ///
    /// # Safety
    /// Зовётся в главном потоке после создания приложения Qt.
    pub unsafe fn create(width: i32, height: i32, screen_index: i32) -> *mut QObject {
        unsafe { qobject::rexon_wp_swf_window_new(width, height, screen_index) }
    }

    /// Сколько экранов сейчас у сеанса; 0 — мониторов нет вовсе.
    pub fn screen_count() -> i32 {
        qobject::rexon_wp_screen_count()
    }

    /// # Safety
    /// `window` — окно из [`create`].
    pub unsafe fn show(window: *mut QObject) {
        unsafe { qobject::rexon_wp_swf_window_show(window) }
    }

    /// Убрать окно совсем — видеокарта отказалась в него рисовать.
    ///
    /// # Safety
    /// `window` — окно из [`create`], и больше к нему не обращаются.
    pub unsafe fn close(window: *mut QObject) {
        unsafe { qobject::rexon_wp_swf_window_close(window) }
    }

    /// # Safety
    /// `window` — окно из [`create`].
    pub unsafe fn exposed(window: *mut QObject) -> bool {
        unsafe { qobject::rexon_wp_swf_window_exposed(window) }
    }

    /// Размер в физических пикселях.
    ///
    /// # Safety
    /// `window` — окно из [`create`].
    pub unsafe fn size(window: *mut QObject) -> (i32, i32) {
        unsafe { (qobject::rexon_wp_swf_window_width(window), qobject::rexon_wp_swf_window_height(window)) }
    }

    /// Идентификатор окна X11 (0 — не X11). Окно платформы создаётся при
    /// первом вызове, показа для этого не нужно.
    ///
    /// # Safety
    /// `window` — окно из [`create`].
    pub unsafe fn x11_window_id(window: *mut QObject) -> i64 {
        unsafe { qobject::rexon_wp_window_id(window) }
    }

    pub fn process_events() {
        qobject::rexon_wp_process_events();
    }

    /// `wl_display*` приложения; 0 — не Wayland.
    pub fn wayland_display() -> usize {
        qobject::rexon_wp_wl_display()
    }

    /// `wl_surface*` окна; 0 — не Wayland или окна нет.
    ///
    /// # Safety
    /// `window` — окно из [`create`], уже созданное платформой (показанное).
    pub unsafe fn wayland_surface(window: *mut QObject) -> usize {
        unsafe { qobject::rexon_wp_wl_surface(window) }
    }

    /// `xcb_connection_t*` приложения; 0 — не X11.
    pub fn xcb_connection() -> usize {
        qobject::rexon_wp_xcb_connection()
    }
}

// ── Разбор аргументов ──────────────────────────────────────────────────────

/// Ключи командной строки хоста.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Cli {
    pub path: String,
    pub windowed: bool,
    pub verbose: bool,
    /// `--audio` / `--no-audio`; `None` — как записано в `project.json`.
    pub audio: Option<bool>,
    /// `--volume` в долях единицы; `None` — как записано в `project.json`.
    pub volume: Option<f64>,
    /// Самопроверка: через сколько миллисекунд окно отчитается и закроется.
    pub selftest_ms: i64,
}

/// Текст `--help` — ОДИН на хост обоев и все его режимы. По-человечески и
/// по-русски (правило #13).
pub const USAGE: &str = "\
Живые обои Rexon: видео, веб-страницы, Flash и сцены движка обоев.

  rexon-wallpaper-host [--mode video] <файл видео | каталог обоев> [ключи]
  rexon-wallpaper-host --mode web <ссылка | файл.html | каталог обоев> [ключи]
  rexon-wallpaper-host --mode swf <файл.swf> [ключи]
  rexon-wallpaper-host --mode waywallen [--socket <сокет вывода>] [-w]

Режим:
      --mode <video|web|swf|waywallen>  что показывать; без ключа — видео;
                        waywallen — вывести на рабочий стол X11 сцены, которые
                        рисует движок обоев (его запускает сам движок)

Ключи всех режимов:
  -w, --windowed        окно 1280×720 вместо обоев (для проверки)
  -v, --verbose         подробный вывод
      --selftest[=мс]   отчитаться о состоянии и закрыться
                        (умолчание: видео 3000, веб 6000, Flash 3000)
  -h, --help            эта справка

Видео и Flash:
      --audio           включить звук, что бы ни было записано в обоях
      --no-audio        выключить звук
      --volume <0..1>   громкость (принимается и 0..100); заданная здесь,
                        она закрепляется и не следует за настройками

У видео и у Flash громкость, «без звука» и приглушение при полноэкранном
окне или чужом звуке берутся из ОДНИХ настроек Rexon и применяются на ходу,
без перезапуска: одно положение ползунка звучит у обоих одинаково громко.

Коды возврата: 0 — всё хорошо, 1 — нечего показывать, 2 — QML не поднялся,
3 — поток кадров не пошёл; видео: 4 — звук просили, а звуковой дорожки
в файле нет; веб: 5 — страница не загрузилась; Flash: 7 — нет видеокарты
с Vulkan, способной рисовать обои, 8 — ролик не загрузился.";

/// Снять с командной строки ключ режима (`--mode video|web` или
/// `--mode=…`) и вернуть режим с остальными аргументами.
///
/// Без ключа — [`HostMode::DEFAULT`] (видео): прежний контракт бинаря, и
/// такую строку уже хранит расширение оболочки в gsettings покупателя.
/// Значения режима разбирает ядро — второго списка режимов здесь нет.
pub fn take_mode(argv: &[String]) -> Result<(HostMode, Vec<String>), String> {
    let flag = rexon_core::wallpaper_engine::HOST_MODE_FLAG;
    let mut mode = None;
    let mut rest = Vec::with_capacity(argv.len());
    let mut it = argv.iter();
    while let Some(arg) = it.next() {
        let value: &str = if arg == flag {
            it.next()
                .ok_or_else(|| format!("после {flag} нужен режим: {}", mode_names()))?
        } else if let Some(v) = arg.strip_prefix(flag).and_then(|t| t.strip_prefix('=')) {
            v
        } else {
            rest.push(arg.clone());
            continue;
        };
        let parsed = HostMode::from_arg(value).ok_or_else(|| {
            format!("неизвестный режим «{value}»: бывает {}", mode_names())
        })?;
        if mode.replace(parsed).is_some() {
            return Err(format!("режим задан дважды: {flag} нужен один"));
        }
    }
    Ok((mode.unwrap_or(HostMode::DEFAULT), rest))
}

/// Имена режимов для сообщений — из ядра, через «|».
fn mode_names() -> String {
    HostMode::ALL
        .iter()
        .map(|m| m.as_arg())
        .collect::<Vec<_>>()
        .join("|")
}

/// Разбор аргументов режима видео (ключ `--mode` уже снят [`take_mode`]).
/// Ошибка — готовая строка для человека.
///
/// ⚠️ `--audio` и `--volume` принимаются именно здесь: ими зовёт хост
/// движок обоев (`rexon-core::wallpaper_engine::video_native_plan`), а
/// прежний разбор на Python их не знал и падал — видео-обои со звуком не
/// стартовали вовсе.
pub fn parse_args(argv: &[String]) -> Result<Cli, String> {
    let mut cli = Cli::default();
    let mut it = argv.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-w" | "--windowed" => cli.windowed = true,
            "-v" | "--verbose" => cli.verbose = true,
            "--audio" => cli.audio = Some(true),
            "--no-audio" => cli.audio = Some(false),
            "--volume" => {
                let raw = it.next().ok_or("после --volume нужна громкость")?;
                cli.volume = Some(parse_volume(raw)?);
            }
            "-h" | "--help" => return Err(USAGE.to_string()),
            other if other.starts_with("--volume=") => {
                cli.volume = Some(parse_volume(&other["--volume=".len()..])?);
            }
            other if other == "--selftest" => cli.selftest_ms = DEFAULT_SELFTEST_MS,
            other if other.starts_with("--selftest=") => {
                cli.selftest_ms = other["--selftest=".len()..]
                    .parse()
                    .map_err(|_| "после --selftest= нужны миллисекунды".to_string())?;
            }
            other if other.starts_with('-') && other.len() > 1 => {
                return Err(format!("неизвестный ключ: {other}"));
            }
            other => {
                if !cli.path.is_empty() {
                    return Err(format!("лишний путь: {other}"));
                }
                cli.path = other.to_string();
            }
        }
    }
    if cli.path.is_empty() {
        return Err("не указан файл видео или каталог обоев".to_string());
    }
    Ok(cli)
}

/// Умолчание паузы самопроверки: столько нужно, чтобы дошёл `configure`
/// композитора и проигрыватель успел отдать первый кадр.
pub const DEFAULT_SELFTEST_MS: i64 = 3000;

fn parse_volume(raw: &str) -> Result<f64, String> {
    let v: f64 = raw
        .trim()
        .parse()
        .map_err(|_| format!("громкость «{raw}» — не число"))?;
    // Приведение 0..100 → 0..1 и зажим в границы делает ядро: то же
    // правило, что для громкости из project.json.
    Ok(rexon_core::wallpaper::as_volume01(
        Some(&Value::from(v)),
        rexon_core::wallpaper::DEFAULT_VOLUME,
    ))
}

/// Наложить ключи командной строки на свойства обоев.
///
/// Именно наложить, а не завести отдельную ветку: дальше и то, и другое
/// разбирает один и тот же `video_context_properties` ядра, поэтому
/// «звук из ключа» и «звук из project.json» не могут разойтись.
pub fn apply_cli_overrides(props: &mut Map<String, Value>, cli: &Cli) {
    if let Some(audio) = cli.audio {
        props.insert("audioenabled".to_string(), Value::Bool(audio));
    }
    if let Some(volume) = cli.volume {
        props.insert("volume".to_string(), Value::from(volume));
    }
}

/// Значение свойства QML в вариант Qt — ОДНА функция на все хосты обоев.
///
/// Развилка здесь ровно одна, и она неочевидная: локальный путь надо
/// заворачивать `QUrl::from_local_file`, а готовую ссылку — нет. Второй
/// копией это правило однажды уже разъехалось бы: веб-обои принимают и то, и
/// другое, и `from_local_file("https://…")` даёт «file:///https:/…», то есть
/// молча пустую страницу вместо честного отказа (правило #12).
pub fn qml_value_to_variant(value: &rexon_core::wallpaper::QmlValue) -> cxx_qt_lib::QVariant {
    use cxx_qt_lib::{QString, QUrl, QVariant};
    use rexon_core::wallpaper::QmlValue;
    match value {
        QmlValue::LocalFileUrl(path) => {
            QVariant::from(&QUrl::from_local_file(&QString::from(path)))
        }
        QmlValue::Url(url) => QVariant::from(&QUrl::from(&QString::from(url))),
        QmlValue::Bool(v) => QVariant::from(v),
        QmlValue::Real(v) => QVariant::from(v),
        QmlValue::Int(v) => QVariant::from(&(*v as i32)),
    }
}

/// Как окно попадает на задний план.
pub const PLACEMENT_LAYER: &str = "layer";
pub const PLACEMENT_X11: &str = "x11";
pub const PLACEMENT_OFFSCREEN: &str = "offscreen";

/// Способ показа по решению ядра. Ядро отвечает именем QML-файла прежней
/// раскладки (три файла), здесь оно переводится в один из трёх режимов
/// единственного файла.
pub fn placement(has_mmap_output: bool, wayland: bool) -> &'static str {
    match rexon_core::wallpaper::video_qml_file(has_mmap_output, wayland) {
        rexon_core::wallpaper::QML_OFFSCREEN => PLACEMENT_OFFSCREEN,
        rexon_core::wallpaper::QML_WAYLAND => PLACEMENT_LAYER,
        _ => PLACEMENT_X11,
    }
}

/// Способ показа для хоста обоев с учётом `--windowed` — ОДИН на видео и
/// веб. Отладочное окно — это окно на экране, поэтому поток кадров оно
/// отменяет: иначе на платформе `offscreen` его просто не было бы видно.
/// (Прежде так решал только веб-хост, видео-хост уводил `--windowed` при
/// заданном `REXON_WP_OUTPUT` в офскрин.)
pub fn host_placement(has_mmap_output: bool, windowed: bool, wayland: bool) -> &'static str {
    placement(has_mmap_output && !windowed, wayland)
}

/// Выставить платформу Qt под способ показа и режим хоста. Звать ДО создания
/// QGuiApplication — платформу выбирает его конструктор. Решение — ядра
/// (`wallpaper_mmap::host_qpa_platform`: видео и Flash в потоке — `offscreen`,
/// веб — платформа сеанса под `QQuickRenderControl`), здесь только наблюдение
/// окружения и запись переменной.
pub fn prepare_qpa_platform(placement: &str, mode: rexon_core::wallpaper_engine::HostMode) {
    let env = |name: &str| std::env::var(name).unwrap_or_default();
    let current = std::env::var("QT_QPA_PLATFORM").ok();
    if let Some(platform) = rexon_core::wallpaper_mmap::host_qpa_platform(
        placement == PLACEMENT_OFFSCREEN,
        rexon_core::wallpaper_mmap::host_stream_capture(mode),
        current.as_deref(),
        &env("WAYLAND_DISPLAY"),
        &env("DISPLAY"),
    ) {
        if let Some(previous) = current.filter(|v| !v.is_empty()) {
            rexon_core::log_info!(
                "wallpaper-host",
                "REXON-WPH-QPA QT_QPA_PLATFORM={previous} заменена на {platform}: \
                 в режиме потока кадров окна на экране быть не должно"
            );
        }
        // SAFETY: зовётся из main до создания приложения Qt и до любых
        // потоков — читателей окружения ещё нет.
        unsafe { std::env::set_var("QT_QPA_PLATFORM", platform) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ключи_звука_принимаются() {
        // Именно та строка, которой зовёт движок обоев. Прежний хост на ней
        // выходил с ошибкой — видео-обои со звуком не запускались.
        let argv = [
            "/v.mp4".to_string(),
            "--audio".to_string(),
            "--volume".to_string(),
            "0.7".to_string(),
        ];
        let cli = parse_args(&argv).expect("разбор обязан пройти");
        assert_eq!(cli.path, "/v.mp4");
        assert_eq!(cli.audio, Some(true));
        assert_eq!(cli.volume, Some(0.7));
    }

    #[test]
    fn громкость_в_сотнях_приводится_как_в_ядре() {
        let cli = parse_args(&["/v.mp4".into(), "--volume=70".into()]).unwrap();
        assert_eq!(cli.volume, Some(0.7));
        // Мусор — не повод остаться без обоев, но и не повод молча играть:
        // ключ разбирается строго, а вот значение из project.json ядро
        // откатывает к умолчанию. Это разные источники.
        assert!(parse_args(&["/v.mp4".into(), "--volume=трах".into()]).is_err());
    }

    #[test]
    fn путь_обязателен_и_единственный() {
        assert!(parse_args(&[]).is_err());
        assert!(parse_args(&["/a.mp4".into(), "/b.mp4".into()]).is_err());
        assert!(parse_args(&["--нетакого".into(), "/a.mp4".into()]).is_err());
    }

    #[test]
    fn ключи_накладываются_на_свойства_обоев() {
        let mut props = Map::new();
        props.insert("audioenabled".into(), Value::Bool(false));
        props.insert("volume".into(), Value::from(10));
        let cli = Cli {
            audio: Some(true),
            volume: Some(0.25),
            ..Cli::default()
        };
        apply_cli_overrides(&mut props, &cli);
        let ctx = rexon_core::wallpaper::video_context_properties("/v.mp4", false, &props, 1, 1);
        let by_name = |n: &str| {
            ctx.iter()
                .find(|p| p.name == n)
                .map(|p| p.value.clone())
                .unwrap()
        };
        assert_eq!(
            by_name("audioEnabled"),
            rexon_core::wallpaper::QmlValue::Bool(true)
        );
        assert_eq!(
            by_name("audioVolume"),
            rexon_core::wallpaper::QmlValue::Real(0.25)
        );
    }

    #[test]
    fn свойства_обоев_остаются_при_пустых_ключах() {
        let mut props = Map::new();
        props.insert("audioenabled".into(), Value::Bool(false));
        apply_cli_overrides(&mut props, &Cli::default());
        assert_eq!(props.get("audioenabled"), Some(&Value::Bool(false)));
        assert!(!props.contains_key("volume"));
    }

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    /// Строки, которыми ядро зовёт хост, разбираются в свой режим, а ключ
    /// режима не доезжает до разбора режима.
    #[test]
    fn режим_снимается_с_командной_строки() {
        let (mode, rest) = take_mode(&args(&["--mode", "web", "https://a"])).unwrap();
        assert_eq!(mode, HostMode::Web);
        assert_eq!(rest, args(&["https://a"]));
        let (mode, rest) =
            take_mode(&args(&["--mode=video", "/v.mp4", "--no-audio"])).unwrap();
        assert_eq!(mode, HostMode::Video);
        assert_eq!(rest, args(&["/v.mp4", "--no-audio"]));
        // Строка ядра целиком (без имени бинаря) проходит оба разбора.
        let argv = rexon_core::wallpaper_engine::video_host_argv("/h", "/v.mp4", false);
        let (mode, rest) = take_mode(&argv[1..]).unwrap();
        assert_eq!(mode, HostMode::Video);
        assert_eq!(parse_args(&rest).unwrap().audio, Some(false));
        let argv = rexon_core::wallpaper_engine::wallpaper_host_argv("/h", HostMode::Web, "/i.html");
        let (mode, rest) = take_mode(&argv[1..]).unwrap();
        assert_eq!(mode, HostMode::Web);
        assert_eq!(crate::web_host::parse_args(&rest).unwrap().target, "/i.html");
        // Flash: строка ядра `--mode swf <файл>` и ключи звука — тот же разбор, что у видео.
        let argv = rexon_core::wallpaper_engine::wallpaper_host_argv("/h", HostMode::Swf, "/r.swf");
        let (mode, rest) = take_mode(&argv[1..]).unwrap();
        assert_eq!(mode, HostMode::Swf);
        assert_eq!(parse_args(&rest).unwrap().path, "/r.swf");
        let cli = parse_args(&args(&["/r.swf", "--audio", "--volume", "0.3"])).unwrap();
        assert_eq!((cli.audio, cli.volume), (Some(true), Some(0.3)));
    }

    /// Без ключа — видео: прежний контракт бинаря, такую строку хранит
    /// расширение оболочки. Мусор и повтор — ошибка, а не молчаливое видео.
    #[test]
    fn без_режима_видео_а_мусор_отказ() {
        let (mode, rest) = take_mode(&args(&["/v.mp4"])).unwrap();
        assert_eq!(mode, HostMode::Video);
        assert_eq!(rest, args(&["/v.mp4"]));
        let (mode, rest) = take_mode(&args(&["--mode", "swf", "/a.swf"])).unwrap();
        assert_eq!(mode, HostMode::Swf);
        assert_eq!(rest, args(&["/a.swf"]));
        assert!(take_mode(&args(&["--mode", "flash", "/a.swf"])).unwrap_err().contains("video|web|swf"));
        assert!(take_mode(&args(&["/v.mp4", "--mode"])).is_err());
        assert!(take_mode(&args(&["--mode=web", "--mode", "video", "/v"])).is_err());
    }

    /// Справка одна и называет все режимы и все коды возврата таблицы ядра.
    #[test]
    fn справка_одна_на_все_режимы() {
        use rexon_core::wallpaper_engine::host_exit;
        for mode in HostMode::ALL {
            assert!(USAGE.contains(mode.as_arg()), "{mode:?}");
        }
        for code in [
            host_exit::NOTHING_TO_SHOW,
            host_exit::QML_FAILED,
            host_exit::FRAMES_FAILED,
            host_exit::VIDEO_NO_AUDIO,
            host_exit::WEB_PAGE_FAILED,
            host_exit::SWF_NO_GPU,
            host_exit::SWF_MOVIE_FAILED,
        ] {
            assert!(USAGE.contains(&format!("{code} —")), "в справке нет кода {code}");
        }
        assert!(USAGE.contains("--mode swf"));
        assert!(!USAGE.contains("rexon-web-host"));
        assert_eq!(crate::web_host::parse_args(&args(&["--help"])), Err(USAGE.to_string()));
        assert_eq!(parse_args(&args(&["--help"])), Err(USAGE.to_string()));
    }

    #[test]
    fn режим_показа_повторяет_решение_ядра() {
        assert_eq!(placement(true, true), PLACEMENT_OFFSCREEN);
        assert_eq!(placement(true, false), PLACEMENT_OFFSCREEN);
        assert_eq!(placement(false, true), PLACEMENT_LAYER);
        assert_eq!(placement(false, false), PLACEMENT_X11);
    }

    #[test]
    fn отладочное_окно_отменяет_поток_кадров_у_обоих_хостов() {
        // Поток кадров без --windowed — офскрин (окна на экране нет).
        assert_eq!(host_placement(true, false, true), PLACEMENT_OFFSCREEN);
        // --windowed при заданном REXON_WP_OUTPUT — окно на экране, иначе на
        // платформе offscreen его просто не было бы видно.
        assert_eq!(host_placement(true, true, true), PLACEMENT_LAYER);
        assert_eq!(host_placement(true, true, false), PLACEMENT_X11);
        assert_eq!(host_placement(false, false, true), PLACEMENT_LAYER);
    }
}
