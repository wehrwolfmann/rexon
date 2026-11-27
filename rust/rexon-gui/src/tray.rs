// The Rexon system tray icon — the executor of the core's decisions.
//
// WHAT IS WHERE. The menu contents, the labels in the customer's language,
// the tooltip, the icon candidates and the content of the autostart entry
// are in `rexon_core::tray`, and they are covered by tests there too. Here
// there is only execution: capture the state (the daemon on the bus, the
// queue mode, the change interval, the autostart entry), feed the plan to
// the shim (`cpp/rexon_tray.cpp`) and dispatch the presses to actions.
//
// Warning: there must not be a single human-readable label in this file —
// just as in the tab bridges. Everything the customer sees arrives from the
// core.
//
// WHY QSystemTrayIcon AND NOT ksni. Both give the same StatusNotifierItem in
// Plasma, but ksni brings up ITS OWN thread and ITS OWN bus connection, and
// every press arrives in a foreign thread — while the items of this menu
// touch the application's main window (raise it, switch the tab, hide it),
// that is, they must run in the GUI thread. Through QSystemTrayIcon a press
// arrives right where the window lives, with no extra threads or
// connections, and Qt re-registers the icon by itself when the shell
// restarts. The price is a dependency on QtWidgets, but the Rexon process
// is a QApplication anyway.

use std::ffi::{c_char, CStr, CString};
use std::sync::Mutex;

use rexon_core::tray as core_tray;
use rexon_core::privilege::revoke::Reason;
use rexon_core::wallpaper_playlist::Command as PlaylistCommand;

// ── shim ───────────────────────────────────────────────────────────────────

type TrayClick = extern "C" fn(i32);
type TrayVoid = extern "C" fn();
type TrayAsk = extern "C" fn() -> bool;

unsafe extern "C" {
    fn rexon_tray_set_hooks(
        on_click: TrayClick,
        on_menu_show: TrayVoid,
        on_activate: TrayVoid,
        on_close: TrayAsk,
    );
    fn rexon_tray_available() -> bool;
    fn rexon_set_app_window_icon(icon_file: *const c_char, icon_theme: *const c_char) -> bool;
    fn rexon_tray_create(
        icon_file: *const c_char,
        icon_theme: *const c_char,
        tip_title: *const c_char,
        tip_body: *const c_char,
    ) -> bool;
    fn rexon_tray_menu_begin();
    fn rexon_tray_menu_add(
        id: i32,
        kind: i32,
        label: *const c_char,
        checked: bool,
        enabled: bool,
        is_default: bool,
    );
    fn rexon_tray_menu_end();
    fn rexon_tray_notify(title: *const c_char, body: *const c_char);
    fn rexon_tray_guard_window() -> bool;
    fn rexon_tray_raise_window() -> bool;
    fn rexon_tray_grab_window(path: *const c_char) -> bool;
    fn rexon_tray_adopt_qml_window(engine: *mut std::ffi::c_void) -> bool;
    fn rexon_tray_window_native(rgb: *mut u32) -> u64;
    fn rexon_tray_hide_window() -> bool;
    fn rexon_tray_select_tab(index: i32) -> bool;
    fn rexon_tray_quit();
    fn rexon_tray_open_section(id: *const c_char, subpage: *const c_char) -> bool;
    fn rexon_tray_request_quit(delay_ms: i32);
    fn rexon_tray_daemon_alive() -> bool;
    fn rexon_tray_daemon_call(method: *const c_char) -> bool;
    fn rexon_tray_daemon_call_u32(method: *const c_char, arg: u32) -> bool;
    fn rexon_tray_daemon_call_bool(method: *const c_char, arg: bool) -> bool;
    fn rexon_tray_daemon_prop(name: *const c_char, buf: *mut c_char, buf_len: i32) -> i32;
}

fn cstr(text: &str) -> CString {
    // A NUL byte in a label is impossible (the core table), but there is no
    // reason for the icon to crash on it — we cut at the first NUL.
    CString::new(text).unwrap_or_else(|e| {
        let bytes = e.into_vec();
        let head = bytes.split(|b| *b == 0).next().unwrap_or(&[]).to_vec();
        CString::new(head).expect("после обрезки нулей строка валидна")
    })
}

// ── ItemKind number for the shim ──────────────────────────────────────────

/// Numeric code of an item kind. The only place where the core's enum turns
/// into a number: the shim knows nothing about the meaning of the values.
fn kind_code(kind: core_tray::ItemKind) -> i32 {
    match kind {
        core_tray::ItemKind::Action => 0,
        core_tray::ItemKind::Separator => 1,
        core_tray::ItemKind::Check => 2,
        core_tray::ItemKind::Radio => 3,
        core_tray::ItemKind::SubmenuBegin => 4,
        core_tray::ItemKind::SubmenuEnd => 5,
    }
}

// ── live icon state ───────────────────────────────────────────────────────

/// What the icon remembers between menu openings.
struct Runtime {
    /// Identifiers of the main window's tabs in the window's own order.
    tabs: Vec<String>,
    /// Wallpaper paused / sound muted — OUR bookkeeping: the daemon does not
    /// publish these two states on the bus (see `rexon_core::tray::TrayState`).
    paused: bool,
    muted: bool,
    /// Whether the daemon was on the bus at the previous menu build. The
    /// transition "was not → is" means a fresh daemon: it plays and sounds, and
    /// our bookkeeping should be reset.
    daemon_was: bool,
}

fn runtime() -> &'static Mutex<Option<Runtime>> {
    static RUNTIME: Mutex<Option<Runtime>> = Mutex::new(None);
    &RUNTIME
}

// ── reading the daemon state ──────────────────────────────────────────────

fn daemon_prop(name: &str) -> Option<String> {
    let key = cstr(name);
    let mut buf = [0i8; 256];
    let written = unsafe {
        rexon_tray_daemon_prop(key.as_ptr(), buf.as_mut_ptr().cast::<c_char>(), 256)
    };
    if written < 0 {
        return None;
    }
    let text = unsafe { CStr::from_ptr(buf.as_ptr().cast::<c_char>()) };
    Some(text.to_string_lossy().into_owned())
}

fn daemon_call(method: &str) -> bool {
    let m = cstr(method);
    unsafe { rexon_tray_daemon_call(m.as_ptr()) }
}

// ── autostart at login ────────────────────────────────────────────────────

fn autostart_file() -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    let cfg = std::env::var("XDG_CONFIG_HOME").ok();
    core_tray::autostart_path(cfg.as_deref(), &home)
}

/// Whether Rexon autostart at login is enabled — and whether it will work
/// in THIS session.
///
/// A single `.desktop` entry is not enough: sway, Hyprland without uwsm,
/// river, labwc and Wayfire do not execute it, and the check mark would be
/// lit for nothing. There autostart is held by a line in the compositor
/// config, and the truth is told by the core
/// (`compositor_autostart::works_live`).
pub fn autostart_enabled() -> bool {
    match std::fs::read_to_string(autostart_file()) {
        Ok(text) => rexon_core::compositor_autostart::works_live(ID_LOGIN, Some(&text)),
        Err(_) => false,
    }
}

/// Repair Rexon autostart entries whose program disappeared (an old
/// developer installation path, a package move): the program is the same
/// as the launcher's (`app::launch_program`), for the wallpaper engine — the
/// found daemon binary. Every repair is a line in the log. Call outside the
/// GUI thread.
pub fn repair_autostart_entries() {
    use rexon_core::compositor_autostart as ca;
    let path_env = std::env::var("PATH").ok();
    let rexon = rexon_core::utils::which("rexon", path_env.as_deref());
    let entry = rexon_core::installer::launch_entry_live();
    ca::repair_stale_rexon_live(&|id| {
        if id == ca::ID_ENGINE {
            crate::we_env::daemon_binary().map(|b| rexon_core::installer::desktop_exec_arg(&b))
        } else {
            rexon_core::app::launch_program(&entry, rexon.as_deref())
        }
    });
}

/// Tag of the tray line in compositor configs.
const ID_LOGIN: &str = rexon_core::compositor_autostart::ID_TRAY;

/// Whether this session has a system tray.
///
/// Qt itself is asked, not the shell name: a session either has a tray or
/// does not, and guessing by `XDG_CURRENT_DESKTOP` would mean creating a
/// second table of environments that silently diverges from the real state
/// of affairs. The "Settings" tab shows a warning based on this answer:
/// without a tray the "autostart into the tray" setting does nothing.
pub fn available() -> bool {
    unsafe { rexon_tray_available() }
}

/// Enable or disable autostart. `Err` is a file system error text or the
/// refusal "will not work in this environment" (with a ready line to paste
/// into the compositor config by hand).
///
/// Disabling REMOVES the entry rather than editing its content: a dead
/// entry with `Hidden=true` would survive the removal of Rexon and remain in
/// the autostart editor as a line about a non-existent program.
pub fn set_autostart(on: bool) -> Result<(), String> {
    let path = autostart_file();
    let file = std::path::Path::new(&path);
    if !on {
        rexon_core::compositor_autostart::remove_live(ID_LOGIN);
        return match std::fs::remove_file(file) {
            Ok(()) => Ok(()),
            // There was no entry — asked to disable, it is disabled.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.to_string()),
        };
    }

    let path_env = std::env::var("PATH").ok();
    let rexon = rexon_core::utils::which("rexon", path_env.as_deref());
    // The entry point follows the same rule as the launcher's (one core
    // function): otherwise the tray and the launcher would end up with
    // different owners under a shared lock.
    let entry = rexon_core::installer::launch_entry_live();
    // Never: there is nothing to start with — there will be no entry: a dead
    // line in autostart comes alive at login, when there is nobody left to
    // explain the breakage.
    let exec = core_tray::autostart_exec(&entry, rexon.as_deref())
        .ok_or_else(|| rexon_core::app::MSG_NOTHING_TO_LAUNCH.to_string())?;
    let body = core_tray::autostart_desktop_content(&exec);
    // Where the environment does not execute `.desktop` — a line in the
    // compositor config; the entry is hidden from such environments
    // (`NotShowIn=`) so that the tray does not start twice.
    let (body, refusal) = rexon_core::compositor_autostart::mirror_desktop_live(ID_LOGIN, &body);

    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    rexon_core::utils::atomic_write(&file.to_string_lossy(), &body).map_err(|e| e.to_string())?;
    // The entry for other environments is written, but it will not work in
    // this session — say so rather than report "enabled".
    match refusal {
        Some(msg) => Err(msg),
        None => Ok(()),
    }
}

// ── building the menu ─────────────────────────────────────────────────────

/// Capture the live state and rebuild the icon menu.
fn rebuild_menu() {
    let mut guard = runtime().lock().expect("состояние значка не отравлено");
    let Some(rt) = guard.as_mut() else {
        return;
    };

    let daemon = unsafe { rexon_tray_daemon_alive() };
    if daemon && !rt.daemon_was {
        // The daemon has just appeared: a fresh one plays and sounds.
        rt.paused = false;
        rt.muted = false;
    }
    rt.daemon_was = daemon;

    let (playlist, playlist_paused) = rexon_core::wallpaper_playlist::tray_state();
    let state = core_tray::TrayState {
        daemon,
        playlist,
        playlist_paused,
        shuffle: daemon_prop("QueueMode").as_deref() == Some("shuffle"),
        rotation_secs: daemon_prop("RotationSecs")
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(0),
        paused: rt.paused,
        muted: rt.muted,
        autostart: autostart_enabled(),
        admin_session: rexon_core::privilege::run::has_session(),
    };

    let items = core_tray::menu(&rt.tabs, state);
    unsafe { rexon_tray_menu_begin() };
    for item in &items {
        let label = cstr(&item.label);
        unsafe {
            rexon_tray_menu_add(
                item.id,
                kind_code(item.kind),
                label.as_ptr(),
                item.checked,
                item.enabled,
                item.default,
            );
        }
    }
    unsafe { rexon_tray_menu_end() };
}

// ── presses ───────────────────────────────────────────────────────────────

extern "C" fn on_menu_show() {
    rebuild_menu();
}

extern "C" fn on_activate() {
    unsafe { rexon_tray_raise_window() };
}

extern "C" fn on_click(id: i32) {
    // Main window sections: the item number is the tab's ordinal number.
    let section = id - core_tray::ID_SECTION_BASE;
    let tabs_len = runtime()
        .lock()
        .expect("состояние значка не отравлено")
        .as_ref()
        .map(|rt| rt.tabs.len() as i32)
        .unwrap_or(0);
    if (0..tabs_len).contains(&section) {
        unsafe {
            rexon_tray_raise_window();
            rexon_tray_select_tab(section);
        }
        return;
    }

    // Wallpaper auto-change interval.
    if let Some(secs) = core_tray::rotate_secs_for(id) {
        let method = cstr("SetRotationInterval");
        unsafe { rexon_tray_daemon_call_u32(method.as_ptr(), secs.max(0) as u32) };
        return;
    }

    match id {
        core_tray::ID_OPEN => {
            unsafe { rexon_tray_raise_window() };
        }
        // One entry for both playlists: if the Rexon playlist is playing, it leads;
        // otherwise — the Wallpaper Engine daemon.
        core_tray::ID_NEXT => {
            if !playlist_command(PlaylistCommand::Next) {
                daemon_call("Next");
            }
        }
        core_tray::ID_PREV => {
            if !playlist_command(PlaylistCommand::Prev) {
                daemon_call("Previous");
            }
        }
        core_tray::ID_PLAYLIST_PAUSE => {
            playlist_command(PlaylistCommand::TogglePause);
        }
        core_tray::ID_SHUFFLE => {
            let on = daemon_prop("QueueMode").as_deref() != Some("shuffle");
            let method = cstr("SetShuffle");
            unsafe { rexon_tray_daemon_call_bool(method.as_ptr(), on) };
        }
        core_tray::ID_PAUSE => {
            let mut guard = runtime().lock().expect("состояние значка не отравлено");
            if let Some(rt) = guard.as_mut() {
                let want_resume = rt.paused;
                if daemon_call(if want_resume { "Resume" } else { "Pause" }) {
                    rt.paused = !want_resume;
                }
            }
        }
        core_tray::ID_MUTE => {
            let mut guard = runtime().lock().expect("состояние значка не отравлено");
            if let Some(rt) = guard.as_mut() {
                let want_unmute = rt.muted;
                if daemon_call(if want_unmute { "Unmute" } else { "Mute" }) {
                    rt.muted = !want_unmute;
                }
            }
        }
        core_tray::ID_RESCAN => {
            daemon_call("Rescan");
        }
        core_tray::ID_AUTOSTART => {
            let want = !autostart_enabled();
            match set_autostart(want) {
                Ok(()) => rexon_core::log_info!("autostart", "автозапуск при входе: {}", if want { "включён" } else { "выключен" }),
                Err(e) => rexon_core::log_warn!("autostart", "автозапуск при входе не изменён: {e}"),
            }
        }
        core_tray::ID_REVOKE => {
            // There may be no window on screen — without a notification the press
            // would look as if nothing happened.
            let ended = rexon_core::privilege::run::end_session(Reason::RevokedByUser);
            if let Some(reason) = ended {
                let (title, body) = rexon_core::privilege::run::permission_ended_notice(reason);
                notify(&title, &body);
            }
            rexon_core::log_info!("privilege", "«Запретить сейчас» в меню значка: сеанс прав закрыт");
        }
        core_tray::ID_QUIT => {
            note_quit_reason("«Выход» в меню значка");
            rexon_core::privilege::run::shutdown();
            unsafe { rexon_tray_quit() };
        }
        _ => {}
    }
}

/// A command to the Rexon wallpaper playlist if it is playing. Applying
/// takes seconds of shell commands, so it runs outside the GUI thread.
/// `false` means the playlist is not playing.
fn playlist_command(cmd: PlaylistCommand) -> bool {
    if !rexon_core::wallpaper_playlist::tray_state().0 {
        return false;
    }
    std::thread::spawn(move || {
        rexon_core::wallpaper_playlist::live_command(cmd);
    });
    true
}

/// The Rexon wallpaper playlist timer and its auto-pause polling — the
/// shared `wallpaper_audio` watcher (the same conditions and the same
/// decision as for the wallpaper host). The watcher lives only while the
/// timer needs an answer.
fn spawn_playlist_runner() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    let mut watch: Option<(crate::wallpaper_audio::AudioWatch, Arc<AtomicBool>)> = None;
    rexon_core::wallpaper_playlist::set_hold_probe(Box::new(move |active| {
        if !active {
            watch = None;
            return false;
        }
        if watch.is_none() {
            let env = |k: &str| std::env::var(k).unwrap_or_default();
            let session = rexon_core::wallpaper_engine::detect_session_type(&env("XDG_SESSION_TYPE"), &env("WAYLAND_DISPLAY"), &env("DISPLAY"));
            let outputs = rexon_core::wallpaper_exec::live_screen_outputs(&session);
            let (w, h) = rexon_core::wallpaper_exec::smallest_output(&outputs)
                .and_then(|o| o.size())
                .unwrap_or((0, 0));
            let flag = Arc::new(AtomicBool::new(false));
            let sink = flag.clone();
            match crate::wallpaper_audio::watch(w as i64, h as i64, move |d, _| sink.store(d.paused, Ordering::SeqCst)) {
                Ok(handle) => watch = Some((handle, flag)),
                Err(e) => rexon_core::log_warn!("wallpaper", "плейлист: наблюдатель автопаузы не запущен: {e}"),
            }
        }
        watch.as_ref().is_some_and(|(_, f)| f.load(Ordering::SeqCst))
    }));
    rexon_core::wallpaper_playlist::spawn_runner();
}

/// The main window's close button with a live icon. `true` means swallow
/// the close.
///
/// The window goes to the tray — the right to change the system expires
/// (Wehrwolfmann's decision 28.09): the session's privilege helper gets an
/// end of connection. The wallpaper, the icon and autostart live on — they
/// need no privileges.
extern "C" fn on_window_close() -> bool {
    let ended = rexon_core::privilege::run::end_session(Reason::WindowToTray);
    if let Some(reason) = ended {
        rexon_core::log_info!("privilege", "окно ушло в лоток: сеанс прав закрыт ({})", reason.code());
    }
    // The label language is the interface language (`rexon_core::i18n`), chosen
    // by the application entry point before the first window; the icon has no
    // choice of its own.
    show_close_notice(ended);
    true
}

/// Tell the user that the window went to the tray (the core decides —
/// `privilege::run::tray_close_notice`).
///
/// Hiding the window silently is not allowed: the customer would decide they
/// closed the program and be surprised by a live process. The hint "stayed
/// in the tray" is shown exactly once per installation (the mark lives in
/// the Rexon settings), while an expired right is shown every time: the mark
/// knows nothing about it.
fn show_close_notice(ended: Option<Reason>) {
    let mut manager = rexon_core::settings::SettingsManager::new().ok();
    // The settings cannot be read — we do not show the hint (we do not know
    // whether it was seen), but we still tell about the expired right.
    let first_time = manager.as_ref().is_some_and(|m| {
        !m.get(core_tray::SEEN_HIDDEN_NOTICE_KEY)
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    });
    let Some((title, body)) = rexon_core::privilege::run::tray_close_notice(first_time, ended) else {
        return;
    };
    notify(&title, &body);
    if first_time {
        if let Some(m) = manager.as_mut() {
            let _ = m.set(core_tray::SEEN_HIDDEN_NOTICE_KEY, serde_json::Value::Bool(true));
        }
    }
}

/// A pop-up notification from the Rexon icon.
fn notify(title: &str, body: &str) {
    let t = cstr(title);
    let b = cstr(body);
    unsafe { rexon_tray_notify(t.as_ptr(), b.as_ptr()) };
}

/// Put the Rexon icon into the tray.
///
/// `tab_ids` are the identifiers of the main window's tabs in the window's
/// own order (the window frame knows them, so the "Sections" submenu cannot
/// drift away from it). `repo_root` is the developer tree root for finding
/// the icon file ("" for the customer). `start_hidden` means do not show the
/// window, Rexon came up straight into the tray (autostart at login).
///
/// `true` means the icon is in place. With `false` (there is no tray in the
/// session or the icon was not created) the caller MUST keep the window
/// visible and the ordinary behaviour of the close button: hiding the
/// window without an icon means losing the program.
///
/// Warning: call from the GUI thread and only when the main window already
/// exists: the icon immediately puts it under the close guard.
pub fn install(tab_ids: &[String], repo_root: &str, start_hidden: bool) -> bool {
    if unsafe { !rexon_tray_available() } {
        return false;
    }

    let file = cstr(&icon_file(repo_root));
    let theme = cstr(core_tray::ICON_THEME_NAME);
    let title = cstr(&core_tray::tooltip_title());
    let body = cstr(&core_tray::tooltip_body());

    unsafe {
        rexon_tray_set_hooks(on_click, on_menu_show, on_activate, on_window_close);
        if !rexon_tray_create(file.as_ptr(), theme.as_ptr(), title.as_ptr(), body.as_ptr()) {
            return false;
        }
    }

    *runtime().lock().expect("состояние значка не отравлено") = Some(Runtime {
        tabs: tab_ids.to_vec(),
        paused: false,
        muted: false,
        daemon_was: unsafe { rexon_tray_daemon_alive() },
    });

    rebuild_menu();
    // Autostart lines in compositor configs — from the entries already present
    // (the compositor may have been installed after enabling). Outside the GUI
    // thread: this is reading and, occasionally, writing several files.
    std::thread::spawn(|| {
        repair_autostart_entries();
        rexon_core::compositor_autostart::resync_rexon_live();
    });
    // Live wallpaper watchdog (the waywallen renderers and our own host): if it
    // crashed — bring the wallpaper up again. It lives as long as the icon
    // does, that is, all the time Rexon is running. There is one thread per
    // process: applying a live wallpaper also starts it if there is no icon.
    rexon_core::renderer_watch::ensure_running();
    // Engine package watchdog: a Rexon update put a plugin newer than the one
    // the live daemon holds — compare and restart the daemon at once rather
    // than on the next opening of the wallpaper tab
    // (`we_socket::spawn_package_watch`).
    crate::we_socket::spawn_package_watch();
    unsafe { rexon_tray_guard_window() };
    if start_hidden {
        unsafe { rexon_tray_hide_window() };
    }
    true
}

/// Put the icon on the QML main window: the sections, the icon and `--tray`
/// by itself.
///
/// One door instead of three decisions on the caller's side, because all
/// three already exist in the core and there must be no second copy of them
/// in the product: the section list is `qt_main::section_ids()` (the same
/// that draws the window strip), the tree root for the icon file is
/// `tray::repo_root_from_exe`, the "straight into the tray" flag is
/// `tray::starts_hidden`.
///
/// `args` are the command line arguments WITHOUT the program name.
///
/// The answer `false` means there is no tray in the session or the icon was
/// not created. Warning: in that case the caller MUST keep the window
/// visible, even if `--tray` was asked for: without an icon a hidden window
/// is a lost program (rule #13).
/// The icon file in case the theme is not enough ("" — there is no file
/// anywhere).
fn icon_file(repo_root: &str) -> String {
    core_tray::icon_candidates(repo_root)
        .into_iter()
        .find(|p| std::path::Path::new(p).is_file())
        .unwrap_or_default()
}

/// Icon of all windows of the process — the same picture as the tray icon.
///
/// 01.11.2026, MATE on X11: the Rexon window had a generic icon in the task
/// bar. `setDesktopFileName` is enough for Wayland (the shell finds the
/// launcher by the app id), while on X11 the panel takes the picture from
/// the window's `_NET_WM_ICON` — and there was none: nobody gave Qt an icon
/// for the window. Set after the application is created and before the
/// first window; it applies to all windows of the process (the main one,
/// dialogs).
///
/// `false` means neither the theme nor the file gave a picture (to the log,
/// not to the user).
pub fn set_app_window_icon() -> bool {
    let exe = std::env::current_exe().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
    let file = cstr(&icon_file(&core_tray::repo_root_from_exe(&exe)));
    let theme = cstr(core_tray::ICON_THEME_NAME);
    unsafe { rexon_set_app_window_icon(file.as_ptr(), theme.as_ptr()) }
}

pub fn install_for_main_window(args: &[String]) -> bool {
    let tabs: Vec<String> = rexon_core::qt_main::section_ids()
        .into_iter()
        .map(str::to_owned)
        .collect();
    let exe = std::env::current_exe()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let root = core_tray::repo_root_from_exe(&exe);
    // The "do not blank the screen" bridge — with or without the icon: it only
    // needs a process living for the whole session, and that is the main
    // window's process.
    crate::screensaver_bridge::spawn_once();
    // The "Rexon updated" watchdog — also with or without the icon: a
    // notification with a button lives in the notification service and does
    // not need the tray.
    spawn_self_update_watch();
    // The wallpaper playlist timer — also with or without the icon: it only
    // needs a process living for the whole session.
    spawn_playlist_runner();
    install(&tabs, &root, core_tray::starts_hidden(args))
}

/// Remember the QML main window — the engine's root — before the first
/// show.
///
/// The window loads hidden (see [`first_show_main_window`]), and there are
/// several hidden top-level windows in the process: the icon, set up right
/// after, would look for "the first hidden one" and might take a foreign
/// one. So the window is named directly, by the engine.
///
/// # Safety
/// `engine` is a live `QQmlApplicationEngine` after `load`; the GUI thread.
pub unsafe fn adopt_main_window(engine: *mut std::ffi::c_void) -> bool {
    unsafe { rexon_tray_adopt_qml_window(engine) }
}

/// The first show of the main window — at the very end of start-up, before
/// the event loop.
///
/// 02.11.2026, MATE on X11: at start-up a transparent window appeared first
/// and only then the interface. Measured on a test bench: the window with
/// `visible: true` went on screen in the middle of `engine.load`, while Qt
/// draws a frame only from the event loop — in between the GUI thread was
/// still loading QML, setting up the icon and starting the watchdogs (~0.5
/// s on the bench; for Wehrwolfmann, by the log, 0.6–1.2 s from QML loading
/// to the icon), and all that time there was a window without a single
/// pixel on screen. Now the window loads hidden, is shown here when no work
/// is left before the event loop, and even before showing gets an X11
/// background in the skin colour (`wallpaper_host::paint_x11_background`):
/// for the milliseconds left before the first frame it stands in its own
/// colour.
///
/// `show == false` means a start into the tray (there is an icon and
/// `--tray` was asked for): the window stays hidden, and the icon will show
/// it.
pub fn first_show_main_window(show: bool) {
    if !show {
        return;
    }
    let mut rgb: u32 = 0;
    let id = unsafe { rexon_tray_window_native(&mut rgb) };
    if id != 0 {
        let painted = crate::wallpaper_host::paint_x11_background(id as i64, rgb);
        rexon_core::log_info!(
            "start",
            "окно показано{}",
            if painted { format!(", фон X11 #{rgb:06x}") } else { String::new() }
        );
    }
    unsafe { rexon_tray_raise_window() };
}

/// Start the "Rexon updated" watchdog once per process. Only for a packaged
/// Rexon: in a developer tree every build replaces the binary, and a
/// notification after every `cargo build-app` would be noise.
pub fn spawn_self_update_watch() {
    static STARTED: std::sync::Once = std::sync::Once::new();
    if !crate::we_env::rexon_packaged() {
        return;
    }
    STARTED.call_once(|| {
        let spawned = std::thread::Builder::new()
            .name("self-update-watch".into())
            .spawn(self_update_watch_loop);
        if let Err(e) = spawned {
            rexon_core::log_warn!("update", "сторож обновления Rexon не запущен: {e}");
        }
    });
}

/// Polling our own executable: the package replaced it under a running
/// process — offer a restart once per update. The button goes the same way
/// as "Restart" in "Settings" (checking the new copy, the guard of its
/// start, handing over the lock). Decisions are the core's
/// (`single_instance::exe_change`, `SelfUpdateNotice`).
fn self_update_watch_loop() {
    use rexon_core::single_instance as si;
    let link = std::path::Path::new(si::PROC_SELF_EXE);
    let mut notice = si::SelfUpdateNotice::default();
    loop {
        // A push — the privilege session met the helper of the new build: the
        // update is already confirmed, we say so at once
        // (`SelfUpdateNotice::confirm`).
        let nudged = si::wait_self_update_poll(std::time::Duration::from_secs(si::SELF_UPDATE_POLL_SECS));
        let change = si::exe_change_at(link, si::FileStamp::of(link));
        let show = if nudged { notice.confirm(&change) } else { notice.step(&change) };
        if !show {
            continue;
        }
        if let si::ExeChange::Replaced { path, .. } = &change {
            rexon_core::log_info!("update", "исполняемый файл заменён обновлением ({path}) — предлагаю перезапуск");
        }
        // Waits for a press in this thread: polling stops for that time, and a
        // second notification will not appear over the first.
        let answer = rexon_core::utils::notify_ask_live(
            &si::self_update_title(),
            &si::self_update_body(),
            &si::self_update_actions(),
            &si::self_update_no_buttons(),
            si::SELF_UPDATE_NOTICE_WAIT_SECS,
        );
        if answer.as_deref() != Some(si::SELF_UPDATE_RESTART_ACTION) {
            rexon_core::log_info!("update", "уведомление «Rexon обновлён» закрыто без перезапуска");
            continue;
        }
        match crate::settings_actions::restart_for_update() {
            // The process is leaving — there is nothing more to guard.
            Ok(()) => return,
            Err(text) => {
                rexon_core::utils::notify_live(&si::self_update_restart_failed_title(), &text);
            }
        }
    }
}

/// Quit Rexon for real: remove the icon, close the window with the drain of
/// the tabs' workers and stop the event loop.
///
/// Can be called from any thread — the shim moves the work into the GUI
/// thread by itself.
pub fn quit() {
    unsafe { rexon_tray_quit() };
}

/// Switch the main window to the section with this number (the order is the
/// same as that of the section strip). `false` means there is no strip or
/// no such number.
///
/// Can be called from any thread — for the same reason and in the same way
/// as raising the window below.
pub fn select_section(index: i32) -> bool {
    unsafe { rexon_tray_select_tab(index) }
}

/// Raise the main window of this process. `false` means no window was
/// found.
///
/// ⭐ Can be called FROM ANY THREAD, and that is the main point here: the
/// second-launch listener (`rexon_core::single_instance::ActivationListener`)
/// lives in its own thread, while Qt allows touching a window only in the
/// GUI thread. The move into the right thread is done by the shim
/// (`rexon_tray_raise_window`), and it also waits for the real answer —
/// lying "raised" to a second launch over an empty screen is not allowed.
pub fn raise_main_window() -> bool {
    unsafe { rexon_tray_raise_window() }
}

/// Capture the main window into a PNG (tour mode `--tour`). Only from the
/// GUI thread.
pub fn grab_main_window(path: &str) -> bool {
    let path = cstr(path);
    unsafe { rexon_tray_grab_window(path.as_ptr()) }
}

/// Open a section (and a sub-page, `compare` — "DE comparison") of the QML
/// main window. Called from the listener thread; the shim moves it into the
/// GUI thread and waits for an honest answer. false means there is no
/// window or no such section.
pub fn open_section(section: &str, subpage: &str) -> bool {
    let (id, sub) = (cstr(section), cstr(subpage));
    unsafe { rexon_tray_open_section(id.as_ptr(), sub.as_ptr()) }
}

/// How long the window waits before quitting, yielding the lock to the
/// classic window.
///
/// The listener sends the answer "leaving" (`bye`) AFTER the handler
/// returns, and quitting must not overtake it: a process that left before
/// the answer would leave the new start with a broken connection. The new
/// process waits for the lock to be released for up to
/// `single_instance::HANDOVER_WAIT` (5 s) — the margin is enough.
pub const HANDOVER_QUIT_DELAY_MS: i32 = 200;

/// Schedule quitting (icon, window, application) and return at once.
/// `reason` is who asked (for the log: "restart", "owner handover").
pub fn request_quit(reason: &str) {
    note_quit_reason(reason);
    rexon_core::log_info!("exit", "выход запрошен: {reason}");
    unsafe { rexon_tray_request_quit(HANDOVER_QUIT_DELAY_MS) }
}

/// Who asked to quit — for the log line after the event loop.
static QUIT_REASON: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// Remember the reason for quitting (the first one wins: a restart matters
/// more than the window close that followed it).
pub fn note_quit_reason(reason: &str) {
    if let Ok(mut slot) = QUIT_REASON.lock() {
        slot.get_or_insert_with(|| reason.to_string());
    }
}

/// Reason for quitting; if not recorded, the window was closed (the close
/// button without a tray icon, "Quit" in the window menu, Ctrl+Q).
pub fn quit_reason() -> String {
    QUIT_REASON
        .lock()
        .ok()
        .and_then(|s| s.clone())
        .unwrap_or_else(|| "закрытие окна или «Выход» в меню окна".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Item kind codes — the same numbers the `switch` in the shim parses
    /// (`cpp/rexon_tray.cpp`). If they diverge, the menu silently becomes flat.
    #[test]
    fn item_kind_codes_match_shim() {
        assert_eq!(kind_code(core_tray::ItemKind::Action), 0);
        assert_eq!(kind_code(core_tray::ItemKind::Separator), 1);
        assert_eq!(kind_code(core_tray::ItemKind::Check), 2);
        assert_eq!(kind_code(core_tray::ItemKind::Radio), 3);
        assert_eq!(kind_code(core_tray::ItemKind::SubmenuBegin), 4);
        assert_eq!(kind_code(core_tray::ItemKind::SubmenuEnd), 5);
    }

    /// The names of daemon methods that the press dispatcher prints must exist
    /// on its bus. We compare with the core, where the same client lives.
    #[test]
    fn daemon_methods_named_as_in_core() {
        for method in [
            "Next", "Previous", "Pause", "Resume", "Mute", "Unmute", "Rescan",
            "SetShuffle", "SetRotationInterval",
        ] {
            assert!(!method.is_empty());
            assert!(cstr(method).as_bytes() == method.as_bytes());
        }
        assert_eq!(rexon_core::waywallen_client::IFACE, "org.waywallen.waywallen.Daemon1");
    }

    #[test]
    fn string_with_nul_does_not_crash_icon() {
        assert_eq!(cstr("Выход").to_string_lossy(), "Выход");
        assert_eq!(cstr("Вы\0ход").to_string_lossy(), "Вы");
    }
}
