// The video wallpaper host in Rust: a replacement for
// `python -m rexon.video_wallpaper`.
//
// What this piece is and why it is second in line. LayerShellQt holds two
// flagship scenarios of the product — live wallpapers and the login video
// splash — and while there was no way to it from Rust, no Rust wallpaper
// path could bring up a window. The way itself (the
// `cpp/rexon_layer_shell.*` shim and the `WallpaperLayer` QObject in
// `src/layer_shell.rs`) already exists; here a real host is built on it,
// which no longer needs Python.
//
// The split of duties is exactly the same as in the rest of the core:
//
//   * WHAT to show and with which values — `rexon-core::wallpaper`
//     (`video_qml_file`, `video_context_properties`,
//     `video_host_reject_reason`, `mmap_size`) and
//     `rexon-core::wallpaper_mmap` (frame rate, the X11 plan, the frame
//     layout, the Wayland session flag). There are no numbers of our own
//     here;
//   * HOW to put the window into the background — three ways, and all three
//     converge in one QML file (`qml/WallpaperVideo.qml`) instead of three
//     almost identical ones: the layer is set by Rust, not by an attached
//     QML property, and the difference leaves the markup (rule #12).
//
// Warning: a fix during the port (rule #16). The core launched the host
// with the command `… -m rexon.video_wallpaper <file> --audio --volume 0.7`
// (`wallpaper_engine::video_native_plan`), while the Python argument parser
// did not know such keys at all and exited with an error. That is, video
// wallpapers WITH SOUND never started once — neither on Wayland nor on
// X11. Here the keys are accepted and go to the same place the properties
// from `project.json` go, not as a separate branch: the volume and the
// sound flag are decided by the same core code.

use rexon_core::wallpaper_engine::HostMode;
use serde_json::{Map, Value};

/// cxx-qt bridge: what cannot be reached from Rust — the X11 window
/// identifier and the window's frame stream into an mmap file (see
/// `cpp/rexon_wallpaper_host.*`).
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    unsafe extern "C++" {
        include!("rexon_wallpaper_host.h");

        /// The window's `winId()`: creates the platform window if there is none
        /// yet, but does not show it.
        ///
        /// # Safety
        /// `window` is a live QObject (usually a QWindow).
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_wp_window_id(window: *mut QObject) -> i64;

        /// # Safety
        /// `window` is a live QObject of the QML root.
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
        /// `window` is a live QObject of the QML root.
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_wp_mmap_capture(window: *mut QObject) -> QString;

        /// # Safety
        /// `window` is a live QObject of the QML root.
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_wp_mmap_nonblack_permille(window: *mut QObject) -> i64;

        /// # Safety
        /// `window` is a live QObject of the QML root.
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_wp_mmap_frames(window: *mut QObject) -> i64;

        /// # Safety
        /// `window` is a live QObject of the QML root.
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_wp_mmap_center_rgb(window: *mut QObject) -> i64;

        /// How many screens the session has right now (0 — no monitors at all).
        fn rexon_wp_screen_count() -> i32;

        /// Whether to quit the application together with the last window.
        fn rexon_wp_set_quit_on_last_window_closed(enabled: bool);

        // Flash mode: a bare window for wgpu (the wrappers are in the `swf_window`
        // module).
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
        /// Ways to put the window into the background other than the layer: the
        /// layer itself is set by `WallpaperLayer` from `src/layer_shell.rs`. The
        /// object does not own the window — it only configures the one passed to
        /// it.
        #[qobject]
        #[qml_element]
        type WallpaperPlacement = super::WallpaperPlacementRust;
    }

    extern "RustQt" {
        /// X11: prepare the window for the desktop role — the
        /// `_NET_WM_WINDOW_TYPE_DESKTOP` type, the state and "on all desktops".
        /// Call BEFORE showing the window (`visible = true`): the window manager
        /// reads the type on show. The platform window is created right here.
        ///
        /// # Safety
        /// `window` is a live QWindow not yet shown.
        #[qinvokable]
        #[cxx_name = "prepareX11Desktop"]
        unsafe fn prepare_x11_desktop(self: &WallpaperPlacement, window: *mut QObject) -> bool;

        /// X11: bring the shown window to the desktop role with EWMH requests to
        /// the root window. Call AFTER showing; a repeat is harmless.
        ///
        /// # Safety
        /// `window` is a live shown QWindow.
        #[qinvokable]
        #[cxx_name = "applyX11Desktop"]
        unsafe fn apply_x11_desktop(self: &WallpaperPlacement, window: *mut QObject) -> bool;

        /// X11: put the shown wallpaper window above or below the desktop icons
        /// window — depending on whether the icons have a see-through background
        /// (the decision is `rexon_core::wallpaper_mmap::x11_restack_goal`). Call
        /// after showing and then periodically: the icons window is recreated
        /// (restarting Caja, changing its settings) and lands on top. If it stands
        /// where it should, nothing is done. If the window manager has not fulfilled
        /// the request for several rounds in a row — one log entry and no more
        /// requests until the lower layer changes. True means the order was checked
        /// (or the request was sent).
        ///
        /// # Safety
        /// `window` is a live shown QWindow.
        #[qinvokable]
        #[cxx_name = "keepX11Order"]
        unsafe fn keep_x11_order(self: &WallpaperPlacement, window: *mut QObject) -> bool;

        /// Start the window's frame stream into the shared mmap file
        /// (GNOME/Cinnamon).
        ///
        /// `video_output` is the VideoOutput whose sink to take the frame from
        /// (video wallpapers), or `null` — the window's scene is drawn by
        /// QQuickRenderControl without a window on screen, on the GPU (web
        /// wallpapers). Video cannot be done otherwise: on the `offscreen` platform
        /// VideoOutput does not draw, and the window snapshot comes out black. Do
        /// not show the QML window in stream mode.
        ///
        /// # Safety
        /// `window` is a live QObject of the QML root; `video_output` is a live
        /// VideoOutput or null.
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

        /// The central pixel of the last frame in the file, 0xRRGGBB; -1 means
        /// there were no frames. For the self-test: black frames used to count as
        /// success.
        ///
        /// # Safety
        /// `window` is a live QObject of the QML root.
        #[qinvokable]
        #[cxx_name = "mmapCenterRgb"]
        unsafe fn mmap_center_rgb(self: &WallpaperPlacement, window: *mut QObject) -> f64;

        /// How many frames went into the file; -1 means there is no stream.
        ///
        /// Warning: the return is `f64`, not `i64`: we do not expose a 64-bit
        /// integer to QML (the analysis and the precision limits are in
        /// `src/qvalue.rs`). The frame counter will not reach the `double` ceiling
        /// (2^53) even in a century of work.
        ///
        /// # Safety
        /// `window` is a live QObject of the QML root.
        #[qinvokable]
        #[cxx_name = "mmapFrames"]
        unsafe fn mmap_frames(self: &WallpaperPlacement, window: *mut QObject) -> f64;

        /// What the stream frame is captured with (for the self-test report): the
        /// GPU through QQuickRenderControl, software rendering, the video sink or a
        /// window snapshot. Empty means there is no stream.
        ///
        /// # Safety
        /// `window` is a live QObject of the QML root.
        #[qinvokable]
        #[cxx_name = "mmapCapture"]
        unsafe fn mmap_capture(self: &WallpaperPlacement, window: *mut QObject) -> QString;

        /// The share of non-black pixels of the last frame, ‰; -1 means there were
        /// no frames. The self-test does not accept black frames by it: the centre
        /// can be black on a live page too.
        ///
        /// # Safety
        /// `window` is a live QObject of the QML root.
        #[qinvokable]
        #[cxx_name = "mmapNonBlackPermille"]
        unsafe fn mmap_nonblack_permille(self: &WallpaperPlacement, window: *mut QObject) -> f64;

        /// A heartbeat mark for the live wallpaper watchdog
        /// (`rexon_core::renderer_watch`). Call from a QML timer AFTER requesting a
        /// frame from the windows: if rendering hangs, the interface thread stops
        /// too, and the mark stops being updated.
        #[qinvokable]
        fn heartbeat(self: &WallpaperPlacement);

        /// How often to call [`heartbeat`], milliseconds — the core's number.
        #[qinvokable]
        #[cxx_name = "heartbeatIntervalMs"]
        fn heartbeat_interval_ms(self: &WallpaperPlacement) -> i32;

        /// The first wallpaper frame on screen (`what` is the kind code: `video`,
        /// `web`): a measurement line into the log and restacking the Cinnamon
        /// background ([`super::first_frame_shown`]). Call from the window's
        /// `frameSwapped` handler when the frame already has real content.
        #[qinvokable]
        #[cxx_name = "firstFrameShown"]
        fn first_frame_shown(self: &WallpaperPlacement, what: &QString);
    }
}

/// The object has no state: it only acts on the window passed to it.
#[derive(Default)]
pub struct WallpaperPlacementRust;

/// The X11 window number; `winId()` creates the platform window if there is
/// none yet (without showing). `None` means there is no window or the
/// number was not issued (the reason is in the log).
///
/// # Safety
/// `window` is a live QObject or null.
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
        // SAFETY: the invokable's contract — a live QWindow.
        match unsafe { x11_window_id_of(window) } {
            Some(id) => prepare_x11_desktop_layer(id),
            None => false,
        }
    }

    unsafe fn apply_x11_desktop(&self, window: *mut qobject::QObject) -> bool {
        // SAFETY: the invokable's contract — a live QWindow.
        match unsafe { x11_window_id_of(window) } {
            Some(id) => apply_x11_desktop_layer(id),
            None => false,
        }
    }

    unsafe fn keep_x11_order(&self, window: *mut qobject::QObject) -> bool {
        // SAFETY: the invokable's contract — a live QWindow.
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
        // The frame layout, the tick interval and the generation counter path are
        // computed by the core — there are deliberately no numbers of our own here:
        // a wrong frame size once already left a person without a wallpaper, and
        // there was nowhere to look for which of the two copies won.
        let plan = rexon_core::wallpaper_mmap::prime_plan(i64::from(width), i64::from(height));
        let interval = rexon_core::wallpaper_mmap::headless_tick_ms(i64::from(fps));
        let generation = cxx_qt_lib::QString::from(
            &rexon_core::wallpaper_mmap::frame_generation_path(&path.to_string()),
        );
        // Showing the window for the sake of a snapshot is possible only on
        // `offscreen`; the platform was set for this place by
        // `prepare_qpa_platform` before the application was created.
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

    fn heartbeat(&self) {
        rexon_core::renderer_watch::write_heartbeat_live();
    }

    fn heartbeat_interval_ms(&self) -> i32 {
        rexon_core::renderer_watch::HEARTBEAT_INTERVAL_MS as i32
    }

    fn first_frame_shown(&self, what: &cxx_qt_lib::QString) {
        // The kind code comes from QML in Latin letters, and goes to the log in
        // Russian.
        let what = what.to_string();
        let label = match what.as_str() {
            "video" => "видео",
            "web" => "веб-страница",
            other => other,
        };
        first_frame_shown(label);
    }

    unsafe fn mmap_center_rgb(&self, window: *mut qobject::QObject) -> f64 {
        if window.is_null() {
            return -1.0;
        }
        crate::qvalue::i64_to_qml(unsafe { qobject::rexon_wp_mmap_center_rgb(window) })
    }
}

/// Prepare an X11 window for the desktop role — BEFORE showing it: the
/// `_NET_WM_WINDOW_TYPE_DESKTOP` type, the state (without the task bar and
/// the switcher, on all desktops) and `_NET_WM_DESKTOP` = all desktops.
///
/// By this time the window is already created by the platform (`winId()`)
/// but not shown: EWMH allows a client to set such properties only on an
/// unmapped window, and the window manager reads the type on show. On show
/// Qt merges its `_NET_WM_STATE` with the already written one rather than
/// overwriting it.
pub fn prepare_x11_desktop_layer(window_id: i64) -> bool {
    x11_desktop::before_map(window_id)
}

/// Bring an already shown X11 window to the desktop role: EWMH requests to
/// the root window (`_NET_WM_STATE` with `_NET_WM_STATE_ADD`,
/// `_NET_WM_DESKTOP`) and repeating the window type. A repeated call is
/// harmless.
///
/// The plan (what and in which order) is built by the core —
/// [`rexon_core::wallpaper_mmap::x11_desktop_plan`]; here there are only the
/// requests to the server. One executor for video, web and Flash wallpapers
/// (rule #12).
pub fn apply_x11_desktop_layer(window_id: i64) -> bool {
    x11_desktop::after_map(window_id)
}

/// X11 background of a not yet shown window — the solid colour `rgb`
/// (0xRRGGBB).
///
/// Why. Qt creates X11 windows without a background (`BackPixmap None`):
/// until the scene's first frame the server draws nothing in the window,
/// and the shown window looks transparent — the desktop is visible through
/// it, under a compositor there is black or garbage. With a background
/// colour the server itself fills the window at the moment of showing, even
/// before Qt's first frame, and until the frame the window stands in the
/// skin colour. The server puts the same colour on the strip that opens up
/// on stretching — it does not flicker.
///
/// Call after `winId()` and before showing. `false` means not X11 (Wayland,
/// offscreen): there the window has its own way of showing and there is
/// nothing to do; we do not write to the log.
pub fn paint_x11_background(window_id: i64, rgb: u32) -> bool {
    if qobject::rexon_wp_xcb_connection() == 0 {
        return false;
    }
    x11_desktop::background(window_id, rgb)
}

/// Executor of the X11 plan on Qt's own xcb connection.
///
/// Warning: why Qt's connection and not our own. The server executes the
/// requests of one connection in order: the properties written here before
/// showing are guaranteed to land before the `MapWindow` that Qt will send
/// over the same connection. Our own connection (or `xprop`) cannot do that
/// — the window created by Qt might not have reached the server yet, while
/// the show has already happened.
///
/// libxcb is not linked: the functions are taken from the copy already
/// loaded by Qt's platform module (`dlopen` with `RTLD_NOLOAD`). Not X11 —
/// we do nothing and honestly say so.
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

    /// Predefined X11 atoms (`xproto.h`).
    const XA_STRING: u32 = 31;
    const XA_WINDOW: u32 = 33;
    const XA_WM_CLASS: u32 = 67;
    /// `XCB_CW_EVENT_MASK`, `XCB_EVENT_MASK_PROPERTY_CHANGE`.
    const CW_EVENT_MASK: u32 = 1 << 11;
    const PROPERTY_CHANGE_MASK: u32 = 1 << 22;
    /// `XCB_PROP_MODE_APPEND`.
    const PROP_MODE_APPEND: u8 = 2;
    /// The PropertyNotify event code.
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

    // SAFETY: only pointers to functions of a library that is not unloaded.
    unsafe impl Send for Xcb {}
    unsafe impl Sync for Xcb {}

    fn xcb() -> Option<&'static Xcb> {
        static XCB: OnceLock<Option<Xcb>> = OnceLock::new();
        XCB.get_or_init(|| {
            // SAFETY: dlopen/dlsym with correct C strings; NOLOAD loads nothing new,
            // and the library reference is held until the end of the process.
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

    /// Qt's connection and the window number; `None` means not X11 (the reason
    /// is already in the log).
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
        // SAFETY: a live Qt connection.
        if unsafe { (x.connection_has_error)(conn) } != 0 {
            rexon_core::log_warn!("wallpaper-host", "REXON-WPH-X11 соединение X11 сломано");
            return None;
        }
        Some((x, conn, window))
    }

    /// Atom numbers of the plan: all requests at once, then the replies.
    fn intern_all(x: &Xcb, conn: Conn, names: &[&'static str]) -> Option<Vec<(&'static str, u32)>> {
        let cnames: Vec<CString> = names.iter().map(|n| CString::new(*n).unwrap()).collect();
        // SAFETY: the strings live until the end of the function; the name length
        // < u16::MAX.
        let cookies: Vec<Cookie> = cnames
            .iter()
            .map(|c| unsafe { (x.intern_atom)(conn, 0, c.as_bytes().len() as u16, c.as_ptr()) })
            .collect();
        let mut out = Vec::with_capacity(names.len());
        let mut ok = true;
        for (name, cookie) in names.iter().zip(cookies) {
            let mut err: *mut GenericError = std::ptr::null_mut();
            // SAFETY: the reply and the error are allocated by libxcb, we release them
            // with free().
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

    /// Wait for the server's answer to a checked request; `false` means a
    /// refusal.
    fn checked(x: &Xcb, conn: Conn, cookie: Cookie, what: &str) -> bool {
        // SAFETY: the cookie is from *_checked of this same connection.
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
            // SAFETY: values lives until the reply; format 32 means u32 words.
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
        // SAFETY: a live connection.
        unsafe { (x.flush)(conn) };
        ok
    }

    /// `XCB_CW_BACK_PIXEL`.
    const CW_BACK_PIXEL: u32 = 1 << 1;

    pub(super) fn background(window_id: i64, rgb: u32) -> bool {
        let Some((x, conn, window)) = session(window_id) else {
            return false;
        };
        // A TrueColor pixel: for a window of depth 32 (with alpha) the high byte is
        // the opacity; without it the background under a compositor would be
        // transparent.
        let pixel = if depth(x, conn, window) == 32 { 0xff00_0000 | rgb } else { rgb & 0x00ff_ffff };
        // SAFETY: a live connection; the value is one u32 for the mask.
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

        // The root window of the X screen on which the window lives.
        let mut err: *mut GenericError = std::ptr::null_mut();
        // SAFETY: the reply and the error are allocated by libxcb, we release them
        // with free().
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
            // SAFETY: the event is 32 bytes, as xcb_send_event requires.
            let cookie = unsafe {
                (x.send_event_checked)(conn, 0, root, wm::ROOT_MESSAGE_EVENT_MASK, event.as_ptr().cast())
            };
            ok &= checked(x, conn, cookie, m.message_type);
        }
        // SAFETY: a live connection.
        unsafe { (x.flush)(conn) };
        // A shown window rises above all desktop windows — including the
        // see-through icons window under which it belongs.
        keep_order(window_id);
        ok
    }

    // ── order of the wallpaper window and the icons window ────────────────

    /// Atoms of the order check. The screen number is needed to choose the
    /// compositor.
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

    /// Atoms on Qt's connection: the numbers live as long as the server does —
    /// they are looked up once.
    fn order_atoms(x: &Xcb, conn: Conn) -> Option<&'static [(&'static str, u32)]> {
        static ATOMS: OnceLock<Option<Vec<(&'static str, u32)>>> = OnceLock::new();
        ATOMS
            .get_or_init(|| {
                let screen = wm::x11_screen_number(&std::env::var("DISPLAY").unwrap_or_default());
                intern_all(x, conn, &order_atom_names(screen))
            })
            .as_deref()
    }

    /// A window property as 32-bit words (`kind` is the property type). No
    /// property or an error — empty.
    fn property32(x: &Xcb, conn: Conn, window: u32, property: u32, kind: u32) -> Vec<u32> {
        let mut err: *mut GenericError = std::ptr::null_mut();
        // SAFETY: the reply and the error are allocated by libxcb, we release them
        // with free().
        let reply = unsafe {
            (x.get_property_reply)(conn, (x.get_property)(conn, 0, window, property, kind, 0, 8192), &mut err)
        };
        if !err.is_null() {
            unsafe { libc::free(err.cast()) };
        }
        if reply.is_null() {
            return Vec::new();
        }
        // SAFETY: the value lives together with the reply; the length is in bytes.
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

    /// The window's `WM_CLASS`: the instance and the class.
    fn wm_class(x: &Xcb, conn: Conn, window: u32) -> (String, String) {
        let mut err: *mut GenericError = std::ptr::null_mut();
        // SAFETY: as in `property32`; format 8 means bytes.
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

    /// The window depth; 0 means not found out.
    fn depth(x: &Xcb, conn: Conn, window: u32) -> u8 {
        let mut err: *mut GenericError = std::ptr::null_mut();
        // SAFETY: the reply and the error are allocated by libxcb, we release them
        // with free().
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

    /// Whether the selection has an owner (the compositor holds `_NET_WM_CM_Sn`).
    fn selection_owned(x: &Xcb, conn: Conn, selection: u32) -> bool {
        let mut err: *mut GenericError = std::ptr::null_mut();
        // SAFETY: the reply and the error are allocated by libxcb, we release them
        // with free().
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

    /// The root window of the screen on which the window lives.
    fn root_of(x: &Xcb, conn: Conn, window: u32) -> Option<u32> {
        let mut err: *mut GenericError = std::ptr::null_mut();
        // SAFETY: the reply and the error are allocated by libxcb, we release them
        // with free().
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

    /// A fresh X server time. Marco (like Metacity) does not fulfil an order
    /// request from a window whose `_NET_WM_USER_TIME` is older than that of
    /// another application's active window (`meta_window_configure_request`);
    /// the server time can be learned only from an event, and the events of
    /// Qt's connection are handled by Qt. Hence our own short connection: an
    /// empty append to a property of the root window and the time from the
    /// PropertyNotify that arrives.
    fn server_time(x: &Xcb, root: u32) -> Option<u32> {
        // SAFETY: our own connection, closed right here; events — free().
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
        /// Observations about other windows by number — they do not change for a
        /// window.
        static FACTS: std::cell::RefCell<std::collections::HashMap<u32, wm::X11ClientFacts>> =
            std::cell::RefCell::new(std::collections::HashMap::new());
        /// The last order written to the log (so as not to write the same thing).
        static LAST_LOGGED: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
        /// Accounting of order requests per wallpaper window (one window per
        /// monitor).
        static TRACKERS: std::cell::RefCell<std::collections::HashMap<u32, wm::X11RestackTracker>> =
            std::cell::RefCell::new(std::collections::HashMap::new());
    }

    /// The window manager did not lower the wallpaper under see-through icons
    /// (xfwm4): once recreate the icons window by toggling "icons off → on" —
    /// xfwm4 puts the new window above the wallpaper. In a separate thread:
    /// there is a pause between "off" and "on", and the check runs in Qt's main
    /// thread.
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
            // SAFETY: the event is 32 bytes, as xcb_send_event requires.
            let cookie = unsafe {
                (x.send_event_checked)(conn, 0, root, wm::ROOT_MESSAGE_EVENT_MASK, event.as_ptr().cast())
            };
            checked(x, conn, cookie, "_NET_RESTACK_WINDOW")
        } else {
            // Without `_NET_RESTACK_WINDOW` (Marco) — ConfigureWindow without a
            // sibling: Marco does not look at the sibling at all and raises the window
            // to the top of its layer or lowers it to the bottom
            // (`meta_window_raise`/`_lower`), and the icons window is in the same
            // layer. A sibling in the request would only bring a BadMatch if the icons
            // window were in a frame.
            rexon_core::log_info!("wallpaper-host", "REXON-WPH-X11 окно 0x{own:x} {where_} 0x{sibling:x} (ConfigureWindow)");
            let user_time_window = property32(x, conn, own, atom("_NET_WM_USER_TIME_WINDOW"), XA_WINDOW)
                .first()
                .copied()
                .unwrap_or(own);
            if let Some(time) = server_time(x, root) {
                // SAFETY: one word of format 32.
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
            // SAFETY: one value — stack_mode.
            let cookie = unsafe { (x.configure_window_checked)(conn, own, CONFIG_WINDOW_STACK_MODE, (&mode as *const u32).cast()) };
            checked(x, conn, cookie, "ConfigureWindow")
        };
        // SAFETY: a live connection.
        unsafe { (x.flush)(conn) };
        ok
    }
}

/// The moment the host process started — the reference point for measuring
/// the first frame.
static HOST_STARTED: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

/// Mark the host start. Call as the very first thing in the host's `main`.
pub fn mark_host_started() {
    HOST_STARTED.get_or_init(std::time::Instant::now);
}

/// The first wallpaper frame on screen (for video — a clip frame, not a
/// black window): one measurement line into the log and, on Cinnamon with
/// layer-shell, restacking the background service now rather than by the
/// fallback timeout. Once per process: windows on other monitors and
/// rebuilding the windows are not measured again.
pub fn first_frame_shown(what: &str) {
    static DONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if DONE.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    let since_host = HOST_STARTED.get().map_or(0, |t| t.elapsed().as_millis() as u64);
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64);
    let env = std::env::var(rexon_core::wallpaper_engine::APPLY_STARTED_ENV).unwrap_or_default();
    let since_apply = rexon_core::wallpaper_engine::since_apply_ms(&env, now_ms);
    rexon_core::log_info!(
        "wallpaper-host",
        "{}",
        rexon_core::wallpaper_engine::first_frame_log_line(what, since_apply, since_host)
    );
    cinnamon_layer::surface_shown(&format!("{what} слоем стола"));
}

/// Cinnamon with layer-shell (Muffin 6.8+): the wallpaper is a surface of
/// the background layer, and the shell's background service must lie under
/// it. The decisions are `rexon_core::wallpaper_backend` (the "Cinnamon with
/// layer-shell" section); here is what needs our own Wayland client: asking
/// the compositor and watching the monitors.
pub mod cinnamon_layer {
    use rexon_core::wallpaper_backend as wb;
    use wayland_client::protocol::wl_registry;
    use wayland_client::{Connection, Dispatch, QueueHandle};

    /// Find out the fact of a "Cinnamon with layer-shell" session (the
    /// compositor's globals; without an answer — the background service) and
    /// remember it for the process. Outside Cinnamon on Wayland we do not ask
    /// the compositor.
    pub fn init_fact() -> bool {
        if !wb::cinnamon_wayland_session_live() {
            wb::set_cinnamon_layer_shell(false);
            return false;
        }
        let facts = crate::screensaver_bridge::wayland_idle::probe();
        wb::init_cinnamon_layer_shell_live(facts.as_ref().map(|f| f.globals.as_slice()))
    }

    /// Our window got the background layer (before showing): order restacking
    /// of the background service with the FALLBACK timeout
    /// ([`wb::BG_RESTACK_FALLBACK_MS`]) and watch that it stays that way (the
    /// service came up again, a monitor was connected). The real reason to
    /// restack is the [`surface_shown`] notice: the host sends it when the
    /// first frame (for video — a clip frame) is on screen; before it there is
    /// nothing to show under the background service except a black window.
    /// Outside Cinnamon on Wayland — nothing.
    pub fn surface_up(reason: &str) {
        if !wb::cinnamon_wayland_session_live() {
            return;
        }
        wb::restack_cinnamon_background_after(reason, wb::BG_RESTACK_FALLBACK_MS);
        wb::spawn_cinnamon_bg_owner_watch_live();
        spawn_output_watch();
    }

    /// The first frame of our surface on screen: lay the background service
    /// under it now rather than by the fallback timeout. Outside Cinnamon on
    /// Wayland — nothing.
    pub fn surface_shown(reason: &str) {
        if !wb::cinnamon_layer_shell_fact() {
            return;
        }
        wb::restack_cinnamon_background_shown(reason);
    }

    struct Outputs {
        /// The first pass over the registry is over: from now on every `wl_output`
        /// is new.
        ready: bool,
    }

    impl Dispatch<wl_registry::WlRegistry, ()> for Outputs {
        fn event(
            state: &mut Self,
            _: &wl_registry::WlRegistry,
            event: wl_registry::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            if let wl_registry::Event::Global { interface, .. } = event {
                if state.ready && interface == "wl_output" {
                    // Both the background service and we will get a surface on the new monitor,
                    // and who comes later is unknown: restack.
                    wb::restack_cinnamon_background_soon("подключён монитор");
                }
            }
        }
    }

    /// Our own thread with our own connection to the compositor: a new
    /// `wl_output` means restacking the background. One per process.
    fn spawn_output_watch() {
        static STARTED: std::sync::Once = std::sync::Once::new();
        STARTED.call_once(|| {
            let spawned = std::thread::Builder::new().name("rexon-output-watch".into()).spawn(|| {
                let run = || -> Result<(), String> {
                    let conn = Connection::connect_to_env().map_err(|e| e.to_string())?;
                    let mut queue = conn.new_event_queue::<Outputs>();
                    let _registry = conn.display().get_registry(&queue.handle(), ());
                    let mut state = Outputs { ready: false };
                    queue.roundtrip(&mut state).map_err(|e| e.to_string())?;
                    state.ready = true;
                    loop {
                        queue.blocking_dispatch(&mut state).map_err(|e| e.to_string())?;
                    }
                };
                if let Err(e) = run() {
                    rexon_core::log_warn!("cinnamon-bg", "слежка за мониторами оборвалась: {e}");
                }
            });
            if let Err(e) = spawned {
                rexon_core::log_warn!("cinnamon-bg", "слежка за мониторами не запущена: {e}");
            }
        });
    }
}

/// One wallpaper display host per session: a `flock` lock in
/// `$XDG_RUNTIME_DIR`. The decisions (which file, who takes the lock, whom
/// to send a signal) are the core's (`rexon_core::wallpaper_engine`, the
/// "One wallpaper host per session" section); here are the system calls.
pub mod session_lock {
    use rexon_core::wallpaper_engine as wpe;
    use std::io::{Read, Seek, Write};
    use std::os::fd::AsRawFd;

    /// How long to wait for the previous host to exit on SIGTERM before SIGKILL.
    const TERM_WAIT_MS: u64 = 3000;
    /// How long to wait after SIGKILL.
    const KILL_WAIT_MS: u64 = 1000;
    const POLL_MS: u64 = 50;

    fn try_lock(file: &std::fs::File) -> bool {
        // SAFETY: a live file descriptor.
        unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) == 0 }
    }

    fn wait_lock(file: &std::fs::File, ms: u64) -> bool {
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(ms);
        loop {
            if try_lock(file) {
                return true;
            }
            if std::time::Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(POLL_MS));
        }
    }

    /// The holder's PID, if it is our host and not ourselves.
    fn holder(file: &mut std::fs::File) -> Option<i32> {
        let mut text = String::new();
        file.seek(std::io::SeekFrom::Start(0)).ok()?;
        file.read_to_string(&mut text).ok()?;
        let pid = wpe::parse_host_lock_pid(&text)?;
        if pid as u32 == std::process::id() {
            return None;
        }
        let cmdline = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
        wpe::is_wallpaper_host_cmdline(&cmdline).then_some(pid)
    }

    /// Take the session lock, replacing the previous host, and hold it until
    /// the process exits. If it fails, the display goes on anyway (better two
    /// hosts than none), with a line in the log.
    pub fn take(mode: wpe::HostMode, args: &[String]) {
        if !wpe::host_takes_session_lock(mode, args) {
            return;
        }
        let env = |n: &str| std::env::var(n).unwrap_or_default();
        let Some(path) = wpe::host_session_lock_path(&env("XDG_RUNTIME_DIR"), &env("WAYLAND_DISPLAY"), &env("DISPLAY"))
        else {
            return;
        };
        // `File::options` opens with O_CLOEXEC: the Chromium helpers of web mode
        // will not inherit the lock and will not prolong its life.
        let mut file = match std::fs::File::options().read(true).write(true).create(true).truncate(false).open(&path) {
            Ok(f) => f,
            Err(e) => {
                rexon_core::log_warn!("wallpaper-host", "замок одного хоста обоев не открыт ({path}): {e}");
                return;
            }
        };
        if !try_lock(&file) {
            match holder(&mut file) {
                Some(pid) => {
                    rexon_core::log_info!(
                        "wallpaper-host",
                        "в сеансе уже идёт хост обоев PID {pid} — сменяю его, двух показов разом не бывает"
                    );
                    // SAFETY: an ordinary signal to a process whose command line is our host.
                    unsafe { libc::kill(pid, libc::SIGTERM) };
                    if !wait_lock(&file, TERM_WAIT_MS) {
                        // SAFETY: the same.
                        unsafe { libc::kill(pid, libc::SIGKILL) };
                        wait_lock(&file, KILL_WAIT_MS);
                    }
                }
                // The holder is not our host (the PID from the file is stale, and someone
                // else holds the lock) — wait a little.
                None => {
                    wait_lock(&file, KILL_WAIT_MS);
                }
            }
        }
        if !try_lock(&file) {
            rexon_core::log_warn!("wallpaper-host", "замок одного хоста обоев не взят ({path}) — показываю без него");
            return;
        }
        let _ = file.set_len(0);
        let _ = file.seek(std::io::SeekFrom::Start(0));
        let _ = write!(file, "{}", std::process::id());
        let _ = file.flush();
        // The lock lives while the descriptor is open: we keep it until the
        // process exits.
        std::mem::forget(file);
    }
}

/// Do not quit the application together with the last window.
///
/// Warning: this is mandatory for wallpapers, and here is why. The host has
/// as many windows as there are monitors: a monitor is disconnected — its
/// window is destroyed. With Qt's default ("the last window closed — we
/// quit") the departure of the last monitor would kill the whole wallpaper
/// process, and a monitor coming back a second later would stay empty
/// forever — there is nobody to restart the host. The wallpaper has exactly
/// one exit: a signal from the executor (`rexon-core::wallpaper_exec`), and
/// for runs — `Qt.exit` from QML.
///
/// Call AFTER `QGuiApplication` is created.
pub fn keep_alive_without_windows() {
    qobject::rexon_wp_set_quit_on_last_window_closed(false);
}

// ── Host exit ──────────────────────────────────────────────────────────────

/// How long we give Qt for the teardown before exiting by force.
///
/// Three seconds: an honest teardown fits into tens of milliseconds
/// (measured on the bench), and even the Chromium teardown of web
/// wallpapers into fractions of a second. Taking more is not allowed: the
/// wallpaper executor considers the host alive if it did not die within a
/// second (`wallpaper_engine::SPAWN_ALIVE_GRACE_SEC`), and a long refusal
/// would look to the user like "the wallpaper was applied".
pub const QT_SHUTDOWN_GRACE: std::time::Duration = std::time::Duration::from_secs(3);

/// The exit code when the host broke from the inside (a panic). 101 is the
/// same number every panicking Rust program exits with, and it deliberately
/// does not overlap with the `wallpaper_engine::host_exit` table.
pub const PANIC_EXIT: i32 = 101;

/// The verdict is given: name the code and do not let Qt delay the exit
/// forever.
///
/// Warning: why this exists. Measurement of 16.09.2026, an X11 bench with
/// two screens joined through `+xinerama`: the wallpaper host did not finish
/// AT ALL — not even in 120 seconds. The stack of the live process showed
/// that the main thread stood in `QThread::wait()` inside the destruction of
/// `QXcbEventQueue`, while the event queue thread itself hung in
/// `xcb_wait_for_event()`, and there was nothing left to wake it up. That
/// is, it was not the display that hung but Qt's TEARDOWN on exit. A
/// wallpaper host hanging forever is the worst possible outcome: no
/// wallpaper, the process is alive, and nobody can take it down. So the
/// teardown has a deadline, and when it expires — an honest complaint and
/// an exit with the same verdict.
pub fn verdict(code: i32) -> i32 {
    std::thread::spawn(move || {
        std::thread::sleep(QT_SHUTDOWN_GRACE);
        rexon_core::log_warn!(
            "wallpaper-host",
            "REXON-WPH-WARN Qt не отпускает хост обоев дольше {} с — закрываемся силой; \
             приговор прежний, код {code}",
            QT_SHUTDOWN_GRACE.as_secs()
        );
        // `_exit` does not flush the buffers — we do it ourselves, otherwise the
        // complaint would be lost exactly where it is needed most.
        use std::io::Write;
        let _ = std::io::stderr().flush();
        // Warning: precisely `_exit`, not `abort`: SIGABRT means a dump and a "the
        // program crashed" window in front of the user, while we close by our own
        // decision.
        unsafe { libc::_exit(code) };
    });
    code
}

/// A panic of the wallpaper host closes the process at once, without
/// unwinding the stack.
///
/// Warning: unwinding would go through the destruction of
/// `QGuiApplication`, and on X11 it waits for the event queue thread — and
/// it was exactly on this wait that the host hung forever (the analysis is
/// at [`verdict`]). No kind of wallpaper benefits from unwinding: there is
/// nothing to show after a panic, and the wallpaper executor
/// (`rexon-core::wallpaper_exec`) will bring the host up again by itself.
///
/// Call as the first line of `main`, before any Qt.
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

/// Set up the wallpaper host log — the SHARED Rexon file
/// (`rexon_core::applog`, `~/.local/state/rexon/rexon.log`), not a separate
/// one.
///
/// Why shared: the host is a continuation of the user's action in the Rexon
/// window ("turn on the wallpaper"), and whoever analyses it needs one
/// chronology: what the window asked and what the host did about it, in one
/// file, by time. The lines of the processes are distinguished by the
/// process number in every line and the `wallpaper-host` area; the "Copy
/// log" button takes one file. The rotation is the same — it is shared for
/// the file, and the neighbouring process takes it into account
/// (`applog::rotate_sink`).
///
/// Why at all: the executor launches the host with the error stream into
/// `/dev/null` (`wallpaper_exec::LiveSys::spawn` — nobody reads a pipe on
/// live wallpapers), and before this change everything the host said about
/// itself (`REXON-WPH-…` from Rust and from QML, Qt and FFmpeg messages)
/// vanished without a trace. Now the Rust lines go to the file, and
/// descriptor 2 at `/dev/null` is redirected there too by the same path as
/// for the window (`cli::install_qt_message_logger`), so the QML lines get
/// there as well. From a terminal the error stream is not intercepted.
///
/// Warning: the self-test (`--selftest`) does not write into the profile —
/// just like the window's.
pub fn install_host_log(selftest: bool) {
    if selftest {
        return;
    }
    rexon_core::applog::init();
    rexon_core::applog::install_fatal_signal_logger();
    rexon_core::cli::install_qt_message_logger();
}

/// The bare window of Flash mode (`--mode swf`): creation, showing, size and
/// native handles for wgpu. The shim is `cpp/rexon_wallpaper_host.*`; the
/// decisions are the caller's (`src/bin/wallpaper_host/swf.rs`).
pub mod swf_window {
    use super::qobject::{self, QObject};

    /// A window without QML and without a Qt buffer on screen `screen_index`
    /// (< 0 — the primary one); `width`/`height` ≤ 0 mean the size and corner of
    /// that screen. A live `QGuiApplication` is needed.
    ///
    /// # Safety
    /// Called in the main thread after the Qt application is created.
    pub unsafe fn create(width: i32, height: i32, screen_index: i32) -> *mut QObject {
        unsafe { qobject::rexon_wp_swf_window_new(width, height, screen_index) }
    }

    /// How many screens the session has right now; 0 means no monitors at all.
    pub fn screen_count() -> i32 {
        qobject::rexon_wp_screen_count()
    }

    /// # Safety
    /// `window` is a window from [`create`].
    pub unsafe fn show(window: *mut QObject) {
        unsafe { qobject::rexon_wp_swf_window_show(window) }
    }

    /// Remove the window entirely — the GPU refused to draw into it.
    ///
    /// # Safety
    /// `window` is a window from [`create`], and it is not accessed any more.
    pub unsafe fn close(window: *mut QObject) {
        unsafe { qobject::rexon_wp_swf_window_close(window) }
    }

    /// # Safety
    /// `window` is a window from [`create`].
    pub unsafe fn exposed(window: *mut QObject) -> bool {
        unsafe { qobject::rexon_wp_swf_window_exposed(window) }
    }

    /// Size in physical pixels.
    ///
    /// # Safety
    /// `window` is a window from [`create`].
    pub unsafe fn size(window: *mut QObject) -> (i32, i32) {
        unsafe { (qobject::rexon_wp_swf_window_width(window), qobject::rexon_wp_swf_window_height(window)) }
    }

    /// X11 window identifier (0 — not X11). The platform window is created on
    /// the first call; showing is not needed for this.
    ///
    /// # Safety
    /// `window` is a window from [`create`].
    pub unsafe fn x11_window_id(window: *mut QObject) -> i64 {
        unsafe { qobject::rexon_wp_window_id(window) }
    }

    pub fn process_events() {
        qobject::rexon_wp_process_events();
    }

    /// The application's `wl_display*`; 0 means not Wayland.
    pub fn wayland_display() -> usize {
        qobject::rexon_wp_wl_display()
    }

    /// The window's `wl_surface*`; 0 means not Wayland or there is no window.
    ///
    /// # Safety
    /// `window` is a window from [`create`], already created by the platform
    /// (shown).
    pub unsafe fn wayland_surface(window: *mut QObject) -> usize {
        unsafe { qobject::rexon_wp_wl_surface(window) }
    }

    /// The application's `xcb_connection_t*`; 0 means not X11.
    pub fn xcb_connection() -> usize {
        qobject::rexon_wp_xcb_connection()
    }
}

// ── Argument parsing ───────────────────────────────────────────────────────

/// Command line keys of the host.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Cli {
    pub path: String,
    pub windowed: bool,
    pub verbose: bool,
    /// `--audio` / `--no-audio`; `None` — as written in `project.json`.
    pub audio: Option<bool>,
    /// `--volume` as a fraction of one; `None` — as written in `project.json`.
    pub volume: Option<f64>,
    /// Self-test: after how many milliseconds the window reports and closes.
    pub selftest_ms: i64,
}

/// The `--help` text — ONE for the wallpaper host and all its modes. In
/// human terms and in Russian (rule #13).
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
      --preview         web only: full screen on top of everything, any key
                        or click closes it (screensaver preview)
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

/// Take the mode key off the command line (`--mode video|web` or
/// `--mode=…`) and return the mode with the remaining arguments.
///
/// Without the key — [`HostMode::DEFAULT`] (video): the binary's former
/// contract, and the shell extension already stores such a line in the
/// customer's gsettings. The mode values are parsed by the core — there is
/// no second list of modes here.
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

/// Mode names for messages — from the core, joined with "|".
fn mode_names() -> String {
    HostMode::ALL
        .iter()
        .map(|m| m.as_arg())
        .collect::<Vec<_>>()
        .join("|")
}

/// Parsing the arguments of video mode (the `--mode` key has already been
/// taken off by [`take_mode`]). An error is a ready string for the user.
///
/// Warning: `--audio` and `--volume` are accepted exactly here: the
/// wallpaper engine calls the host with them
/// (`rexon-core::wallpaper_engine::video_native_plan`), while the former
/// Python parser did not know them and crashed — video wallpapers with
/// sound did not start at all.
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

/// Default self-test pause: that much is needed for the compositor's
/// `configure` to arrive and for the player to deliver the first frame.
pub const DEFAULT_SELFTEST_MS: i64 = 3000;

fn parse_volume(raw: &str) -> Result<f64, String> {
    let v: f64 = raw
        .trim()
        .parse()
        .map_err(|_| format!("громкость «{raw}» — не число"))?;
    // Converting 0..100 → 0..1 and clamping to the bounds is done by the core:
    // the same rule as for the volume from project.json.
    Ok(rexon_core::wallpaper::as_volume01(
        Some(&Value::from(v)),
        rexon_core::wallpaper::DEFAULT_VOLUME,
    ))
}

/// Overlay the command line keys onto the wallpaper properties.
///
/// Precisely overlay rather than create a separate branch: further on both
/// are parsed by the same core `video_context_properties`, so "sound from
/// the key" and "sound from project.json" cannot diverge.
pub fn apply_cli_overrides(props: &mut Map<String, Value>, cli: &Cli) {
    if let Some(audio) = cli.audio {
        props.insert("audioenabled".to_string(), Value::Bool(audio));
    }
    if let Some(volume) = cli.volume {
        props.insert("volume".to_string(), Value::from(volume));
    }
}

/// A QML property value into a Qt variant — ONE function for all wallpaper
/// hosts.
///
/// There is exactly one branching here, and it is not obvious: a local path
/// has to be wrapped with `QUrl::from_local_file`, and a ready link must
/// not. A second copy of this rule would have drifted apart already once:
/// web wallpapers accept both, and `from_local_file("https://…")` gives
/// "file:///https:/…", that is, a silently empty page instead of an honest
/// refusal (rule #12).
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

/// How the window gets into the background.
pub const PLACEMENT_LAYER: &str = "layer";
pub const PLACEMENT_X11: &str = "x11";
pub const PLACEMENT_OFFSCREEN: &str = "offscreen";

/// The display method by the core's decision. The core answers with the QML
/// file name of the former layout (three files); here it is translated into
/// one of the three modes of the single file.
pub fn placement(has_mmap_output: bool, wayland: bool) -> &'static str {
    match rexon_core::wallpaper::video_qml_file(has_mmap_output, wayland) {
        rexon_core::wallpaper::QML_OFFSCREEN => PLACEMENT_OFFSCREEN,
        rexon_core::wallpaper::QML_WAYLAND => PLACEMENT_LAYER,
        _ => PLACEMENT_X11,
    }
}

/// The display method for the wallpaper host taking `--windowed` into
/// account — ONE for video and web. A debug window is a window on screen,
/// so it cancels the frame stream: otherwise on the `offscreen` platform it
/// would simply not be visible. (Previously only the web host decided this
/// way; the video host moved `--windowed` to offscreen when
/// `REXON_WP_OUTPUT` was set.)
pub fn host_placement(has_mmap_output: bool, windowed: bool, wayland: bool) -> &'static str {
    placement(has_mmap_output && !windowed, wayland)
}

/// Set the Qt platform for the display method and the host mode. Call
/// BEFORE creating QGuiApplication — its constructor chooses the platform.
/// The decision is the core's (`wallpaper_mmap::host_qpa_platform`: video
/// and Flash in stream mode — `offscreen`, web — the session platform under
/// `QQuickRenderControl`), here there is only observing the environment and
/// writing the variable.
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
        // SAFETY: called from main before the Qt application is created and before
        // any threads — there are no readers of the environment yet.
        unsafe { std::env::set_var("QT_QPA_PLATFORM", platform) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ключи_звука_принимаются() {
        // Exactly the line the wallpaper engine calls with. The former host exited
        // with an error on it — video wallpapers with sound did not start.
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
        // Garbage is no reason to be left without a wallpaper, but no reason to
        // play silently either: the key is parsed strictly, whereas a value from
        // project.json is rolled back to the default by the core. These are
        // different sources.
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

    /// The lines the core calls the host with are parsed into their mode, and
    /// the mode key does not reach the mode's parser.
    #[test]
    fn режим_снимается_с_командной_строки() {
        let (mode, rest) = take_mode(&args(&["--mode", "web", "https://a"])).unwrap();
        assert_eq!(mode, HostMode::Web);
        assert_eq!(rest, args(&["https://a"]));
        let (mode, rest) =
            take_mode(&args(&["--mode=video", "/v.mp4", "--no-audio"])).unwrap();
        assert_eq!(mode, HostMode::Video);
        assert_eq!(rest, args(&["/v.mp4", "--no-audio"]));
        // The core's whole line (without the binary name) passes both parsers.
        let argv = rexon_core::wallpaper_engine::video_host_argv("/h", "/v.mp4", false);
        let (mode, rest) = take_mode(&argv[1..]).unwrap();
        assert_eq!(mode, HostMode::Video);
        assert_eq!(parse_args(&rest).unwrap().audio, Some(false));
        let argv = rexon_core::wallpaper_engine::wallpaper_host_argv("/h", HostMode::Web, "/i.html");
        let (mode, rest) = take_mode(&argv[1..]).unwrap();
        assert_eq!(mode, HostMode::Web);
        assert_eq!(crate::web_host::parse_args(&rest).unwrap().target, "/i.html");
        // Flash: the core's line `--mode swf <file>` and the sound keys — the same parsing as for video.
        let argv = rexon_core::wallpaper_engine::wallpaper_host_argv("/h", HostMode::Swf, "/r.swf");
        let (mode, rest) = take_mode(&argv[1..]).unwrap();
        assert_eq!(mode, HostMode::Swf);
        assert_eq!(parse_args(&rest).unwrap().path, "/r.swf");
        let cli = parse_args(&args(&["/r.swf", "--audio", "--volume", "0.3"])).unwrap();
        assert_eq!((cli.audio, cli.volume), (Some(true), Some(0.3)));
    }

    /// Without the key — video: the binary's former contract, the shell
    /// extension stores such a line. Garbage and a repeat are an error, not a
    /// silent video.
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

    /// The help is one, and it names all modes and all exit codes of the core's
    /// table.
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
        // A frame stream without --windowed — offscreen (there is no window on
        // screen).
        assert_eq!(host_placement(true, false, true), PLACEMENT_OFFSCREEN);
        // --windowed with REXON_WP_OUTPUT set — a window on screen, otherwise on
        // the offscreen platform it would simply not be visible.
        assert_eq!(host_placement(true, true, true), PLACEMENT_LAYER);
        assert_eq!(host_placement(true, true, false), PLACEMENT_X11);
        assert_eq!(host_placement(false, false, true), PLACEMENT_LAYER);
    }
}
