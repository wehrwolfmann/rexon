// Режим `gtk`: форма из настоящих виджетов GTK 3 в `GtkOffscreenWindow` —
// окно никогда не выходит на экран, кадр — его же рисунок
// (`gtk_widget_draw` в холст cairo с увеличением, запасной путь —
// `gtk_offscreen_window_get_pixbuf`). Тему задаёт окружение (`GTK_THEME` и
// свой `XDG_DATA_HOME` — решает ядро, `widget_shot::helper_env`).
//
// Почему GTK открывается `dlopen`, а не крейтом-обёрткой: обёртки GTK 3 для
// Rust (gtk-rs 0.18) больше не развиваются и тянут в сборку десятки крейтов и
// заголовки GTK; нам нужны три десятка функций с указателями и числами. Без
// библиотеки GTK 3 на машине помощник честно отвечает кодом 5, и плитка
// получает нарисованный образец. GTK — LGPL: загрузка во время работы,
// отдельный процесс.

use std::ffi::{c_char, c_int, c_uint, c_void, CStr, CString};

use rexon_core::widget_shot::HelperArgs;

type W = *mut c_void;

const NO_GTK: i32 = 5;
const SAVE_FAILED: i32 = 4;

/// Таблица функций GTK 3, взятых из библиотеки.
struct Gtk {
    init_check: unsafe extern "C" fn(*mut c_int, *mut *mut *mut c_char) -> c_int,
    offscreen_window_new: unsafe extern "C" fn() -> W,
    offscreen_window_get_pixbuf: unsafe extern "C" fn(W) -> W,
    widget_set_size_request: unsafe extern "C" fn(W, c_int, c_int),
    widget_set_hexpand: unsafe extern "C" fn(W, c_int),
    widget_set_vexpand: unsafe extern "C" fn(W, c_int),
    widget_set_sensitive: unsafe extern "C" fn(W, c_int),
    widget_get_style_context: unsafe extern "C" fn(W) -> W,
    style_context_add_class: unsafe extern "C" fn(W, *const c_char),
    widget_show_all: unsafe extern "C" fn(W),
    widget_queue_draw: unsafe extern "C" fn(W),
    container_add: unsafe extern "C" fn(W, W),
    container_set_border_width: unsafe extern "C" fn(W, c_uint),
    box_new: unsafe extern "C" fn(c_int, c_int) -> W,
    box_pack_start: unsafe extern "C" fn(W, W, c_int, c_int, c_uint),
    grid_new: unsafe extern "C" fn() -> W,
    grid_attach: unsafe extern "C" fn(W, W, c_int, c_int, c_int, c_int),
    grid_set_row_spacing: unsafe extern "C" fn(W, c_uint),
    grid_set_column_spacing: unsafe extern "C" fn(W, c_uint),
    grid_set_column_homogeneous: unsafe extern "C" fn(W, c_int),
    notebook_new: unsafe extern "C" fn() -> W,
    notebook_append_page: unsafe extern "C" fn(W, W, W) -> c_int,
    label_new: unsafe extern "C" fn(*const c_char) -> W,
    button_new_with_label: unsafe extern "C" fn(*const c_char) -> W,
    check_button_new_with_label: unsafe extern "C" fn(*const c_char) -> W,
    radio_button_new_with_label: unsafe extern "C" fn(*mut c_void, *const c_char) -> W,
    toggle_button_set_active: unsafe extern "C" fn(W, c_int),
    combo_box_text_new: unsafe extern "C" fn() -> W,
    combo_box_text_append_text: unsafe extern "C" fn(W, *const c_char),
    combo_box_set_active: unsafe extern "C" fn(W, c_int),
    entry_new: unsafe extern "C" fn() -> W,
    entry_set_text: unsafe extern "C" fn(W, *const c_char),
    spin_button_new_with_range: unsafe extern "C" fn(f64, f64, f64) -> W,
    spin_button_set_value: unsafe extern "C" fn(W, f64),
    scale_new_with_range: unsafe extern "C" fn(c_int, f64, f64, f64) -> W,
    scale_set_draw_value: unsafe extern "C" fn(W, c_int),
    range_set_value: unsafe extern "C" fn(W, f64),
    progress_bar_new: unsafe extern "C" fn() -> W,
    progress_bar_set_fraction: unsafe extern "C" fn(W, f64),
    switch_new: unsafe extern "C" fn() -> W,
    switch_set_active: unsafe extern "C" fn(W, c_int),
    adjustment_new: unsafe extern "C" fn(f64, f64, f64, f64, f64, f64) -> W,
    scrollbar_new: unsafe extern "C" fn(c_int, W) -> W,
    events_pending: unsafe extern "C" fn() -> c_int,
    main_iteration_do: unsafe extern "C" fn(c_int) -> c_int,
    pixbuf_get_width: unsafe extern "C" fn(W) -> c_int,
    pixbuf_get_height: unsafe extern "C" fn(W) -> c_int,
    pixbuf_new: unsafe extern "C" fn(c_int, c_int, c_int, c_int, c_int) -> W,
    pixbuf_fill: unsafe extern "C" fn(W, u32),
    pixbuf_scale: unsafe extern "C" fn(W, W, c_int, c_int, c_int, c_int, f64, f64, f64, f64, c_int),
    pixbuf_savev: unsafe extern "C" fn(W, *const c_char, *const c_char, *mut *mut c_char, *mut *mut c_char, *mut *mut c_void) -> c_int,
    object_unref: unsafe extern "C" fn(W),
    widget_get_allocated_width: unsafe extern "C" fn(W) -> c_int,
    widget_get_allocated_height: unsafe extern "C" fn(W) -> c_int,
    widget_draw: unsafe extern "C" fn(W, W),
    pixbuf_get_from_surface: unsafe extern "C" fn(W, c_int, c_int, c_int, c_int) -> W,
    cairo_image_surface_create: unsafe extern "C" fn(c_int, c_int, c_int) -> W,
    cairo_create: unsafe extern "C" fn(W) -> W,
    cairo_scale: unsafe extern "C" fn(W, f64, f64),
    cairo_destroy: unsafe extern "C" fn(W),
    cairo_surface_destroy: unsafe extern "C" fn(W),
}

const HORIZONTAL: c_int = 0;
const VERTICAL: c_int = 1;
/// `GDK_INTERP_HYPER` — лучшее сжатие.
const INTERP_HYPER: c_int = 3;
/// `CAIRO_FORMAT_ARGB32`.
const CAIRO_FORMAT_ARGB32: c_int = 0;

impl Gtk {
    /// Библиотека GTK 3 и все нужные функции; чего-то нет — `None`.
    fn load() -> Option<Gtk> {
        // SAFETY: dlopen/dlsym с нулём на конце строк; найденный адрес
        // приводится к сигнатуре, объявленной в документации GTK 3.
        unsafe {
            let lib = libc::dlopen(c"libgtk-3.so.0".as_ptr(), libc::RTLD_NOW | libc::RTLD_GLOBAL);
            if lib.is_null() {
                return None;
            }
            macro_rules! sym {
                ($name:literal) => {{
                    let p = libc::dlsym(lib, concat!($name, "\0").as_ptr() as *const c_char);
                    if p.is_null() {
                        return None;
                    }
                    std::mem::transmute::<*mut c_void, _>(p)
                }};
            }
            Some(Gtk {
                init_check: sym!("gtk_init_check"),
                offscreen_window_new: sym!("gtk_offscreen_window_new"),
                offscreen_window_get_pixbuf: sym!("gtk_offscreen_window_get_pixbuf"),
                widget_set_size_request: sym!("gtk_widget_set_size_request"),
                widget_set_hexpand: sym!("gtk_widget_set_hexpand"),
                widget_set_vexpand: sym!("gtk_widget_set_vexpand"),
                widget_set_sensitive: sym!("gtk_widget_set_sensitive"),
                widget_get_style_context: sym!("gtk_widget_get_style_context"),
                style_context_add_class: sym!("gtk_style_context_add_class"),
                widget_show_all: sym!("gtk_widget_show_all"),
                widget_queue_draw: sym!("gtk_widget_queue_draw"),
                container_add: sym!("gtk_container_add"),
                container_set_border_width: sym!("gtk_container_set_border_width"),
                box_new: sym!("gtk_box_new"),
                box_pack_start: sym!("gtk_box_pack_start"),
                grid_new: sym!("gtk_grid_new"),
                grid_attach: sym!("gtk_grid_attach"),
                grid_set_row_spacing: sym!("gtk_grid_set_row_spacing"),
                grid_set_column_spacing: sym!("gtk_grid_set_column_spacing"),
                grid_set_column_homogeneous: sym!("gtk_grid_set_column_homogeneous"),
                notebook_new: sym!("gtk_notebook_new"),
                notebook_append_page: sym!("gtk_notebook_append_page"),
                label_new: sym!("gtk_label_new"),
                button_new_with_label: sym!("gtk_button_new_with_label"),
                check_button_new_with_label: sym!("gtk_check_button_new_with_label"),
                radio_button_new_with_label: sym!("gtk_radio_button_new_with_label"),
                toggle_button_set_active: sym!("gtk_toggle_button_set_active"),
                combo_box_text_new: sym!("gtk_combo_box_text_new"),
                combo_box_text_append_text: sym!("gtk_combo_box_text_append_text"),
                combo_box_set_active: sym!("gtk_combo_box_set_active"),
                entry_new: sym!("gtk_entry_new"),
                entry_set_text: sym!("gtk_entry_set_text"),
                spin_button_new_with_range: sym!("gtk_spin_button_new_with_range"),
                spin_button_set_value: sym!("gtk_spin_button_set_value"),
                scale_new_with_range: sym!("gtk_scale_new_with_range"),
                scale_set_draw_value: sym!("gtk_scale_set_draw_value"),
                range_set_value: sym!("gtk_range_set_value"),
                progress_bar_new: sym!("gtk_progress_bar_new"),
                progress_bar_set_fraction: sym!("gtk_progress_bar_set_fraction"),
                switch_new: sym!("gtk_switch_new"),
                switch_set_active: sym!("gtk_switch_set_active"),
                adjustment_new: sym!("gtk_adjustment_new"),
                scrollbar_new: sym!("gtk_scrollbar_new"),
                events_pending: sym!("gtk_events_pending"),
                main_iteration_do: sym!("gtk_main_iteration_do"),
                pixbuf_get_width: sym!("gdk_pixbuf_get_width"),
                pixbuf_get_height: sym!("gdk_pixbuf_get_height"),
                pixbuf_new: sym!("gdk_pixbuf_new"),
                pixbuf_fill: sym!("gdk_pixbuf_fill"),
                pixbuf_scale: sym!("gdk_pixbuf_scale"),
                pixbuf_savev: sym!("gdk_pixbuf_savev"),
                object_unref: sym!("g_object_unref"),
                widget_get_allocated_width: sym!("gtk_widget_get_allocated_width"),
                widget_get_allocated_height: sym!("gtk_widget_get_allocated_height"),
                widget_draw: sym!("gtk_widget_draw"),
                pixbuf_get_from_surface: sym!("gdk_pixbuf_get_from_surface"),
                // cairo — зависимость GTK: dlsym по её описателю ищет и в ней.
                cairo_image_surface_create: sym!("cairo_image_surface_create"),
                cairo_create: sym!("cairo_create"),
                cairo_scale: sym!("cairo_scale"),
                cairo_destroy: sym!("cairo_destroy"),
                cairo_surface_destroy: sym!("cairo_surface_destroy"),
            })
        }
    }

    /// Нарисовать окно `window` в `zoom` раз крупнее (`gtk_widget_draw` в
    /// холст cairo с увеличением) и вернуть картинкой; не вышло — null.
    unsafe fn draw_zoomed(&self, window: W, zoom: c_int) -> W {
        let (w, h) = ((self.widget_get_allocated_width)(window), (self.widget_get_allocated_height)(window));
        if w <= 0 || h <= 0 {
            return std::ptr::null_mut();
        }
        let surface = (self.cairo_image_surface_create)(CAIRO_FORMAT_ARGB32, w * zoom, h * zoom);
        if surface.is_null() {
            return std::ptr::null_mut();
        }
        let cr = (self.cairo_create)(surface);
        (self.cairo_scale)(cr, zoom as f64, zoom as f64);
        (self.widget_draw)(window, cr);
        (self.cairo_destroy)(cr);
        let pixbuf = (self.pixbuf_get_from_surface)(surface, 0, 0, w * zoom, h * zoom);
        (self.cairo_surface_destroy)(surface);
        pixbuf
    }

    /// Прокрутить очередь событий GTK, пока она не опустеет (не больше
    /// `limit` шагов: тема с бесконечной анимацией очередь не опустошает).
    unsafe fn settle(&self, limit: usize) {
        for _ in 0..limit {
            if (self.events_pending)() == 0 {
                break;
            }
            (self.main_iteration_do)(0);
        }
    }
}

fn c(s: &str) -> CString {
    CString::new(s.replace('\0', "")).unwrap_or_default()
}

/// Снять форму и записать PNG; код возврата помощника.
pub fn shot(args: &HelperArgs) -> i32 {
    let Some(g) = Gtk::load() else { return NO_GTK };
    let t = |i: usize| c(args.texts.get(i).map(String::as_str).unwrap_or(""));
    // SAFETY: вызовы GTK из одного (главного) потока в порядке, который
    // требует GTK 3: init → виджеты → показ → кадр.
    unsafe {
        if (g.init_check)(std::ptr::null_mut(), std::ptr::null_mut()) == 0 {
            return NO_GTK;
        }
        let window = (g.offscreen_window_new)();
        (g.widget_set_size_request)(window, args.form.0, args.form.1);

        let outer = (g.box_new)(HORIZONTAL, 6);
        (g.container_set_border_width)(outer, 6);
        (g.container_add)(window, outer);

        let notebook = (g.notebook_new)();
        (g.widget_set_hexpand)(notebook, 1);
        (g.box_pack_start)(outer, notebook, 1, 1, 0);

        let grid = (g.grid_new)();
        (g.container_set_border_width)(grid, 8);
        (g.grid_set_row_spacing)(grid, 6);
        (g.grid_set_column_spacing)(grid, 8);
        (g.grid_set_column_homogeneous)(grid, 1);
        (g.notebook_append_page)(notebook, grid, (g.label_new)(t(0).as_ptr()));
        (g.notebook_append_page)(notebook, (g.box_new)(VERTICAL, 0), (g.label_new)(t(1).as_ptr()));

        let apply = (g.button_new_with_label)(t(2).as_ptr());
        (g.style_context_add_class)((g.widget_get_style_context)(apply), c"suggested-action".as_ptr());
        let combo = (g.combo_box_text_new)();
        (g.combo_box_text_append_text)(combo, t(6).as_ptr());
        (g.combo_box_set_active)(combo, 0);
        let check = (g.check_button_new_with_label)(t(4).as_ptr());
        (g.toggle_button_set_active)(check, 1);
        let radio = (g.radio_button_new_with_label)(std::ptr::null_mut(), t(5).as_ptr());
        (g.toggle_button_set_active)(radio, 1);
        let entry = (g.entry_new)();
        (g.entry_set_text)(entry, t(7).as_ptr());
        let spin = (g.spin_button_new_with_range)(0.0, 100.0, 1.0);
        (g.spin_button_set_value)(spin, 42.0);
        let scale = (g.scale_new_with_range)(HORIZONTAL, 0.0, 100.0, 1.0);
        (g.scale_set_draw_value)(scale, 0);
        (g.range_set_value)(scale, 60.0);
        let progress = (g.progress_bar_new)();
        (g.progress_bar_set_fraction)(progress, 0.6);
        let cancel = (g.button_new_with_label)(t(3).as_ptr());
        (g.widget_set_sensitive)(cancel, 0);
        let switch = (g.switch_new)();
        (g.switch_set_active)(switch, 1);
        let switch_row = (g.box_new)(HORIZONTAL, 0);
        (g.box_pack_start)(switch_row, switch, 0, 0, 0);

        let cells: [(W, c_int, c_int); 10] = [
            (apply, 0, 0),
            (combo, 1, 0),
            (check, 0, 1),
            (radio, 1, 1),
            (entry, 0, 2),
            (spin, 1, 2),
            (scale, 0, 3),
            (progress, 1, 3),
            (cancel, 0, 4),
            (switch_row, 1, 4),
        ];
        for (w, col, row) in cells {
            (g.grid_attach)(grid, w, col, row, 1, 1);
        }
        (g.widget_set_vexpand)(progress, 0);

        let adj = (g.adjustment_new)(20.0, 0.0, 100.0, 1.0, 10.0, 40.0);
        let scrollbar = (g.scrollbar_new)(VERTICAL, adj);
        (g.box_pack_start)(outer, scrollbar, 0, 0, 0);

        (g.widget_show_all)(window);
        g.settle(500);
        (g.widget_queue_draw)(window);
        g.settle(500);

        // Форма рисуется крупнее и потом только сжимается: растянутый кадр
        // выходил мыльным. Не вышло крупнее — картинка самого окна.
        let zoom = rexon_core::widget_shot::render_scale(args.form, args.size);
        let mut shot = if zoom > 1 { g.draw_zoomed(window, zoom) } else { std::ptr::null_mut() };
        if shot.is_null() {
            shot = (g.offscreen_window_get_pixbuf)(window);
        }
        if shot.is_null() {
            return SAVE_FAILED;
        }
        let (sw, sh) = ((g.pixbuf_get_width)(shot), (g.pixbuf_get_height)(shot));
        let (ow, oh) = args.size;
        if sw <= 0 || sh <= 0 {
            (g.object_unref)(shot);
            return SAVE_FAILED;
        }
        // Вписать с сохранением сторон в прозрачный кадр.
        let k = (ow as f64 / sw as f64).min(oh as f64 / sh as f64);
        let (dw, dh) = (((sw as f64 * k).round() as c_int).max(1), ((sh as f64 * k).round() as c_int).max(1));
        let (dx, dy) = ((ow - dw) / 2, (oh - dh) / 2);
        let frame = (g.pixbuf_new)(0, 1, 8, ow, oh);
        if frame.is_null() {
            (g.object_unref)(shot);
            return SAVE_FAILED;
        }
        (g.pixbuf_fill)(frame, 0);
        (g.pixbuf_scale)(shot, frame, dx, dy, dw, dh, dx as f64, dy as f64, k, k, INTERP_HYPER);
        let out = c(&args.out);
        let mut err: *mut c_void = std::ptr::null_mut();
        let saved = (g.pixbuf_savev)(
            frame,
            out.as_ptr(),
            c"png".as_ptr(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut err,
        );
        (g.object_unref)(shot);
        (g.object_unref)(frame);
        if saved == 0 {
            if !err.is_null() {
                // GError: { domain: u32, code: i32, message: *char }.
                let msg = *(err as *const u8).add(8).cast::<*const c_char>();
                if !msg.is_null() {
                    eprintln!("rexon-theme-shot: {}", CStr::from_ptr(msg).to_string_lossy());
                }
            }
            return SAVE_FAILED;
        }
        0
    }
}
