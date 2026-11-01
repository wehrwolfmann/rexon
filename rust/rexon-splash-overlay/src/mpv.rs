// Ручные FFI-привязки к libmpv (client API + render API, OpenGL).
//
// Проверено по /usr/include/mpv/{client.h,render.h,render_gl.h}, libmpv.so.2,
// API 2.5.0. Обёртки-крейты намеренно не используются — встраиваем libmpv
// напрямую (-lmpv), без бинаря mpv и без --wid (на Wayland не работает).

#![allow(non_camel_case_types)]
// Часть биндингов — полная поверхность FFI, задействована не вся.
#![allow(dead_code)]

use std::os::raw::{c_char, c_int, c_void};

// Непрозрачные типы-хэндлы.
pub enum mpv_handle {}
pub enum mpv_render_context {}

// Идентификаторы событий (нам нужен только event_id — первое поле структуры).
pub const MPV_EVENT_NONE: c_int = 0;
pub const MPV_EVENT_SHUTDOWN: c_int = 1;
pub const MPV_EVENT_END_FILE: c_int = 7;

// Типы render-параметров (mpv_render_param_type).
pub const MPV_RENDER_PARAM_INVALID: c_int = 0;
pub const MPV_RENDER_PARAM_API_TYPE: c_int = 1;
pub const MPV_RENDER_PARAM_OPENGL_INIT_PARAMS: c_int = 2;
pub const MPV_RENDER_PARAM_OPENGL_FBO: c_int = 3;
pub const MPV_RENDER_PARAM_FLIP_Y: c_int = 4;
pub const MPV_RENDER_PARAM_WL_DISPLAY: c_int = 9;
pub const MPV_RENDER_PARAM_ADVANCED_CONTROL: c_int = 10;
// «Кадр забрать, но не рисовать»: цель отрисовки не нужна вовсе, а очередь
// кадров продвигается как обычно. Нужен, когда экранов не осталось.
pub const MPV_RENDER_PARAM_SKIP_RENDERING: c_int = 13;

// Бит «есть новый кадр» в возврате mpv_render_context_update().
pub const MPV_RENDER_UPDATE_FRAME: u64 = 1;

// C-строка типа OpenGL-рендерера ("opengl").
pub const MPV_RENDER_API_TYPE_OPENGL: &[u8] = b"opengl\0";

#[repr(C)]
pub struct mpv_render_param {
    // В C поле называется `type`.
    pub type_: c_int,
    pub data: *mut c_void,
}

#[repr(C)]
pub struct mpv_opengl_init_params {
    pub get_proc_address:
        Option<extern "C" fn(*mut c_void, *const c_char) -> *mut c_void>,
    pub get_proc_address_ctx: *mut c_void,
}

#[repr(C)]
pub struct mpv_opengl_fbo {
    pub fbo: c_int,
    pub w: c_int,
    pub h: c_int,
    pub internal_format: c_int,
}

// Читаем только первое поле event_id; остальную часть настоящей структуры
// (её владелец — mpv) не трогаем.
#[repr(C)]
pub struct mpv_event {
    pub event_id: c_int,
}

#[link(name = "mpv")]
extern "C" {
    pub fn mpv_create() -> *mut mpv_handle;
    pub fn mpv_initialize(ctx: *mut mpv_handle) -> c_int;
    pub fn mpv_set_option_string(
        ctx: *mut mpv_handle,
        name: *const c_char,
        data: *const c_char,
    ) -> c_int;
    pub fn mpv_command(ctx: *mut mpv_handle, args: *const *const c_char) -> c_int;
    pub fn mpv_command_string(ctx: *mut mpv_handle, args: *const c_char) -> c_int;
    pub fn mpv_terminate_destroy(ctx: *mut mpv_handle);
    pub fn mpv_set_wakeup_callback(
        ctx: *mut mpv_handle,
        cb: Option<extern "C" fn(*mut c_void)>,
        d: *mut c_void,
    );
    pub fn mpv_wait_event(ctx: *mut mpv_handle, timeout: f64) -> *mut mpv_event;

    pub fn mpv_render_context_create(
        res: *mut *mut mpv_render_context,
        mpv: *mut mpv_handle,
        params: *mut mpv_render_param,
    ) -> c_int;
    pub fn mpv_render_context_render(
        ctx: *mut mpv_render_context,
        params: *mut mpv_render_param,
    ) -> c_int;
    pub fn mpv_render_context_set_update_callback(
        ctx: *mut mpv_render_context,
        cb: Option<extern "C" fn(*mut c_void)>,
        cb_ctx: *mut c_void,
    );
    pub fn mpv_render_context_update(ctx: *mut mpv_render_context) -> u64;
    pub fn mpv_render_context_report_swap(ctx: *mut mpv_render_context);
    pub fn mpv_render_context_free(ctx: *mut mpv_render_context);
}
