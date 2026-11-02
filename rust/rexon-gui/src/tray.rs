// Значок Rexon в системном лотке — исполнитель решений ядра.
//
// ЧТО ГДЕ. Состав меню, подписи на языке покупателя, подсказка, кандидаты
// значка и содержимое записи автозапуска — в `rexon_core::tray`, и там же они
// накрыты тестами. Здесь только исполнение: снять состояние (демон на шине,
// режим очереди, интервал смены, запись автозапуска), скормить план шиму
// (`cpp/rexon_tray.cpp`) и развести нажатия по действиям.
//
// ⚠️ Ни одной подписи для человека в этом файле быть не должно — как и в
// мостах вкладок. Всё, что видно покупателю, приезжает из ядра.
//
// ПОЧЕМУ QSystemTrayIcon, А НЕ ksni. Оба дают в Plasma один и тот же
// StatusNotifierItem, но ksni поднимает СВОЙ поток и СВОЁ подключение к шине,
// и каждое нажатие приезжает в чужом потоке — а пункты этого меню трогают
// главное окно приложения (поднять, переключить вкладку, спрятать), то есть
// обязаны выполняться в потоке GUI. Через QSystemTrayIcon нажатие приходит
// сразу туда, где окно живёт, лишних потоков и подключений нет, а
// перерегистрацию значка при перезапуске оболочки Qt делает сам. Плата —
// зависимость от QtWidgets, но процесс Rexon и так QApplication.

use std::ffi::{c_char, CStr, CString};
use std::sync::Mutex;

use rexon_core::tray as core_tray;
use rexon_core::privilege::revoke::Reason;

// ── шим ────────────────────────────────────────────────────────────────────

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
    // Нулевой байт в подписи невозможен (таблица ядра), но падать на нём
    // значку незачем — обрезаем по первому нулю.
    CString::new(text).unwrap_or_else(|e| {
        let bytes = e.into_vec();
        let head = bytes.split(|b| *b == 0).next().unwrap_or(&[]).to_vec();
        CString::new(head).expect("после обрезки нулей строка валидна")
    })
}

// ── число ItemKind для шима ───────────────────────────────────────────────

/// Числовой код вида пункта. Единственное место, где перечисление ядра
/// превращается в число: шим о смысле значений не знает.
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

// ── живое состояние значка ────────────────────────────────────────────────

/// То, что значок помнит между открытиями меню.
struct Runtime {
    /// Идентификаторы вкладок главного окна в порядке самого окна.
    tabs: Vec<String>,
    /// Обои на паузе / звук заглушён — НАШ учёт: демон этих двух состояний на
    /// шине не публикует (см. `rexon_core::tray::TrayState`).
    paused: bool,
    muted: bool,
    /// Был ли демон на шине при прошлой сборке меню. Переход «не было → есть»
    /// означает свежий демон: он играет и звучит, наш учёт пора обнулить.
    daemon_was: bool,
}

fn runtime() -> &'static Mutex<Option<Runtime>> {
    static RUNTIME: Mutex<Option<Runtime>> = Mutex::new(None);
    &RUNTIME
}

// ── чтение состояния демона ───────────────────────────────────────────────

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

// ── автозапуск при входе ──────────────────────────────────────────────────

fn autostart_file() -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    let cfg = std::env::var("XDG_CONFIG_HOME").ok();
    core_tray::autostart_path(cfg.as_deref(), &home)
}

/// Включён ли автозапуск Rexon при входе — и сработает ли он в ЭТОМ сеансе.
///
/// Одной записи `.desktop` мало: sway, Hyprland без uwsm, river, labwc и
/// Wayfire её не исполняют, и галочка горела бы впустую. Там автозапуск
/// держит строка в конфиге композитора, и правду говорит ядро
/// (`compositor_autostart::works_live`).
pub fn autostart_enabled() -> bool {
    match std::fs::read_to_string(autostart_file()) {
        Ok(text) => rexon_core::compositor_autostart::works_live(ID_LOGIN, Some(&text)),
        Err(_) => false,
    }
}

/// Починить записи автозапуска Rexon, чья программа исчезла (старый путь
/// установки разработчика, переезд пакета): программа — та же, что у
/// ярлыка (`app::launch_program`), у движка обоев — найденный бинарь
/// демона. Каждая починка — строкой в журнал. Звать вне потока GUI.
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

/// Метка строки лотка в конфигах композиторов.
const ID_LOGIN: &str = rexon_core::compositor_autostart::ID_TRAY;

/// Есть ли в этой сессии системный лоток.
///
/// Спрашивается сам Qt, а не имя оболочки: лоток в сессии либо есть, либо
/// нет, и гадать по `XDG_CURRENT_DESKTOP` значило бы завести вторую таблицу
/// окружений, которая молча разойдётся с настоящим положением дел.
/// Вкладка «Настройки» показывает по этому ответу предупреждение: без лотка
/// настройка автозапуска в лоток ничего не даст.
pub fn available() -> bool {
    unsafe { rexon_tray_available() }
}

/// Включить или выключить автозапуск. `Err` — текст ошибки файловой системы
/// или отказ «в этом окружении не сработает» (с готовой строкой для ручной
/// вставки в конфиг композитора).
///
/// Выключение именно СНОСИТ запись, а не правит её содержимое: мёртвая запись
/// с `Hidden=true` пережила бы удаление Rexon и осталась бы в редакторе
/// автозапуска строкой про несуществующую программу.
pub fn set_autostart(on: bool) -> Result<(), String> {
    let path = autostart_file();
    let file = std::path::Path::new(&path);
    if !on {
        rexon_core::compositor_autostart::remove_live(ID_LOGIN);
        return match std::fs::remove_file(file) {
            Ok(()) => Ok(()),
            // Записи не было — просили выключить, выключено.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.to_string()),
        };
    }

    let path_env = std::env::var("PATH").ok();
    let rexon = rexon_core::utils::which("rexon", path_env.as_deref());
    // Вход — по тому же правилу, что у ярлыка (одна функция ядра): иначе
    // лоток и ярлык разъедутся по разным хозяевам при общем замке.
    let entry = rexon_core::installer::launch_entry_live();
    // ⛔Заводить нечем — записи не будет: мёртвая строка в автозапуске
    // оживает при входе в сеанс, когда объяснить поломку уже некому.
    let exec = core_tray::autostart_exec(&entry, rexon.as_deref())
        .ok_or_else(|| rexon_core::app::MSG_NOTHING_TO_LAUNCH.to_string())?;
    let body = core_tray::autostart_desktop_content(&exec);
    // Там, где окружение `.desktop` не исполняет, — строка в конфиге
    // композитора; от таких окружений запись прячется (`NotShowIn=`), чтобы
    // лоток не стартовал дважды.
    let (body, refusal) = rexon_core::compositor_autostart::mirror_desktop_live(ID_LOGIN, &body);

    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    rexon_core::utils::atomic_write(&file.to_string_lossy(), &body).map_err(|e| e.to_string())?;
    // Запись для других окружений легла, но в этом сеансе не сработает —
    // сказать это, а не рапортовать «включено».
    match refusal {
        Some(msg) => Err(msg),
        None => Ok(()),
    }
}

// ── сборка меню ───────────────────────────────────────────────────────────

/// Снять живое состояние и перестроить меню значка.
fn rebuild_menu() {
    let mut guard = runtime().lock().expect("состояние значка не отравлено");
    let Some(rt) = guard.as_mut() else {
        return;
    };

    let daemon = unsafe { rexon_tray_daemon_alive() };
    if daemon && !rt.daemon_was {
        // Демон только что появился: свежий играет и звучит.
        rt.paused = false;
        rt.muted = false;
    }
    rt.daemon_was = daemon;

    let state = core_tray::TrayState {
        daemon,
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

// ── нажатия ───────────────────────────────────────────────────────────────

extern "C" fn on_menu_show() {
    rebuild_menu();
}

extern "C" fn on_activate() {
    unsafe { rexon_tray_raise_window() };
}

extern "C" fn on_click(id: i32) {
    // Разделы главного окна: номер пункта — это порядковый номер вкладки.
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

    // Интервал автосмены обоев.
    if let Some(secs) = core_tray::rotate_secs_for(id) {
        let method = cstr("SetRotationInterval");
        unsafe { rexon_tray_daemon_call_u32(method.as_ptr(), secs.max(0) as u32) };
        return;
    }

    match id {
        core_tray::ID_OPEN => {
            unsafe { rexon_tray_raise_window() };
        }
        core_tray::ID_NEXT => {
            daemon_call("Next");
        }
        core_tray::ID_PREV => {
            daemon_call("Previous");
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
            // Окна на экране может не быть — без уведомления нажатие
            // выглядело бы так, будто ничего не произошло.
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

/// Крестик главного окна при живом значке. `true` — закрытие проглотить.
///
/// Окно уходит в лоток — право изменять систему гаснет (решение
/// Wehrwolfmann 28.09): помощник сеанса прав получает конец связи. Обои,
/// значок и автозапуск живут дальше — им права не нужны.
extern "C" fn on_window_close() -> bool {
    let ended = rexon_core::privilege::run::end_session(Reason::WindowToTray);
    if let Some(reason) = ended {
        rexon_core::log_info!("privilege", "окно ушло в лоток: сеанс прав закрыт ({})", reason.code());
    }
    // Язык подписей — язык интерфейса (`rexon_core::i18n`), выбранный
    // входом приложения до первого окна; своего выбора у значка нет.
    show_close_notice(ended);
    true
}

/// Сказать человеку, что окно ушло в лоток (решает ядро —
/// `privilege::run::tray_close_notice`).
///
/// Молча спрятать окно нельзя: покупатель решит, что закрыл программу, и
/// удивится живому процессу. Подсказка «остался в лотке» — ровно один раз за
/// установку (отметка живёт в настройках Rexon), а погасшее право — каждый
/// раз: о нём отметка не знает.
fn show_close_notice(ended: Option<Reason>) {
    let mut manager = rexon_core::settings::SettingsManager::new().ok();
    // Настройки не читаются — подсказку не показываем (не знаем, видели
    // ли), но о погасшем праве говорим всё равно.
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

/// Всплывающее уведомление от значка Rexon.
fn notify(title: &str, body: &str) {
    let t = cstr(title);
    let b = cstr(body);
    unsafe { rexon_tray_notify(t.as_ptr(), b.as_ptr()) };
}

/// Поставить значок Rexon в лоток.
///
/// `tab_ids` — идентификаторы вкладок главного окна в порядке самого окна
/// (их знает каркас окна, поэтому подменю «Разделы» не может с ним
/// разъехаться). `repo_root` — корень дерева разработчика для поиска файла
/// значка («» у покупателя). `start_hidden` — окно не показывать, Rexon
/// поднялся сразу в лоток (автозапуск при входе).
///
/// `true` — значок стоит. При `false` (лотка в сессии нет либо значок не
/// создан) вызывающий ОБЯЗАН оставить окно видимым и обычное поведение
/// крестика: спрятать окно без значка — это потерять программу.
///
/// ⚠️ Звать из потока GUI и только когда главное окно уже создано: значок
/// сразу берёт его под сторожа закрытия.
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
    // Строки автозапуска в конфигах композиторов — по уже лежащим записям
    // (композитор могли поставить после включения). Вне потока GUI: это
    // чтение и, изредка, запись нескольких файлов.
    std::thread::spawn(|| {
        repair_autostart_entries();
        rexon_core::compositor_autostart::resync_rexon_live();
    });
    // Сторож рисовальщика живых обоев: упал — поднять обои заново. Живёт,
    // пока жив значок, то есть всё время, пока Rexon запущен.
    static RENDERER_WATCH: std::sync::Once = std::sync::Once::new();
    RENDERER_WATCH.call_once(|| {
        let spawned = std::thread::Builder::new()
            .name("renderer-watch".into())
            .spawn(rexon_core::renderer_watch::run_forever);
        if let Err(e) = spawned {
            rexon_core::log_warn!("wallpaper", "сторож рисовальщика не запущен: {e}");
        }
    });
    // Сторож пакета движка: обновление Rexon положило плагин новее того,
    // что держит живой демон, — сверить и перезапустить демона сразу, а не
    // при следующем открытии вкладки обоев (`we_socket::spawn_package_watch`).
    crate::we_socket::spawn_package_watch();
    unsafe { rexon_tray_guard_window() };
    if start_hidden {
        unsafe { rexon_tray_hide_window() };
    }
    true
}

/// Поставить значок главному окну на QML: разделы, значок и `--tray` — сами.
///
/// Одна дверь вместо трёх решений на стороне вызывающего, потому что все три
/// уже есть в ядре и второй их копии в продукте быть не должно: список
/// разделов — `qt_main::section_ids()` (тот же, что рисует полосу окна),
/// корень дерева для файла значка — `tray::repo_root_from_exe`, флаг «сразу
/// в лоток» — `tray::starts_hidden`.
///
/// `args` — аргументы командной строки БЕЗ имени программы.
///
/// Ответ `false` — лотка в сессии нет либо значок не создан. ⚠️ Вызывающий
/// ОБЯЗАН в этом случае оставить окно видимым, даже если просили `--tray`:
/// без значка спрятанное окно — это потерянная программа (правило #13).
/// Файл значка на случай, если темы не хватит («» — файла нет нигде).
fn icon_file(repo_root: &str) -> String {
    core_tray::icon_candidates(repo_root)
        .into_iter()
        .find(|p| std::path::Path::new(p).is_file())
        .unwrap_or_default()
}

/// Значок всех окон процесса — та же картинка, что у значка в лотке.
///
/// 01.11.2026, MATE на X11: в панели задач у окна Rexon был общий значок.
/// `setDesktopFileName` хватает Wayland (оболочка находит ярлык по app id),
/// а на X11 панель берёт картинку из `_NET_WM_ICON` окна — и его не было:
/// значка окну Qt не давал никто. Ставится после создания приложения и до
/// первого окна; действует на все окна процесса (главное, диалоги).
///
/// `false` — ни тема, ни файл картинку не дали (в журнал, не человеку).
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
    // Мост «не гасить экран» — со значком и без: ему нужен лишь процесс,
    // живущий весь сеанс, а это и есть процесс главного окна.
    crate::screensaver_bridge::spawn_once();
    // Сторож «Rexon обновлён» — тоже со значком и без: уведомление с кнопкой
    // живёт в службе уведомлений, лоток ему не нужен.
    spawn_self_update_watch();
    install(&tabs, &root, core_tray::starts_hidden(args))
}

/// Запомнить главное окно на QML — корень движка — до первого показа.
///
/// Окно грузится спрятанным (см. [`first_show_main_window`]), а спрятанных
/// окон верхнего уровня в процессе несколько: значок, ставящийся следом,
/// искал бы «первое спрятанное» и мог взять чужое. Потому окно называется
/// прямо, по движку.
///
/// # Safety
/// `engine` — живой `QQmlApplicationEngine` после `load`; поток GUI.
pub unsafe fn adopt_main_window(engine: *mut std::ffi::c_void) -> bool {
    unsafe { rexon_tray_adopt_qml_window(engine) }
}

/// Первый показ главного окна — в самом конце запуска, перед циклом событий.
///
/// 02.11.2026, MATE на X11: при запуске сначала стояло прозрачное окно и
/// только потом интерфейс. Замер на стенде: окно с `visible: true` выходило
/// на экран посреди `engine.load`, а кадр Qt рисует только из цикла событий —
/// между ними поток GUI ещё догружал QML, ставил значок и заводил сторожей
/// (на стенде ~0,5 с; у Wehrwolfmann по журналу
/// от загрузки QML до значка 0,6–1,2 с), и всё это время на экране было
/// окно без единого пикселя. Теперь окно грузится спрятанным, показывается
/// здесь, когда до цикла событий не осталось работы, и ещё до показа
/// получает фон X11 цветом скина (`wallpaper_host::paint_x11_background`):
/// оставшиеся до первого кадра миллисекунды оно стоит в своей краске.
///
/// `show == false` — запуск в лоток (значок есть и просили `--tray`): окно
/// остаётся спрятанным, его покажет значок.
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

/// Завести сторожа «Rexon обновлён» один раз на процесс. Только у Rexon из
/// пакета: в дереве разработчика бинарь меняет каждая сборка, и уведомление
/// после каждой `cargo build-app` было бы шумом.
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

/// Опрос своего исполняемого файла: пакет заменил его под работающим
/// процессом — один раз на обновление предложить перезапуск. Кнопка ведёт
/// тем же путём, что «Перезапустить» в «Настройках» (проверка новой копии,
/// сторож её старта, передача замка). Решения — ядро
/// (`single_instance::exe_change`, `SelfUpdateNotice`).
fn self_update_watch_loop() {
    use rexon_core::single_instance as si;
    let link = std::path::Path::new(si::PROC_SELF_EXE);
    let mut notice = si::SelfUpdateNotice::default();
    loop {
        // Толчок — сеанс прав встретил помощника новой сборки: обновление
        // уже подтверждено, говорим сразу (`SelfUpdateNotice::confirm`).
        let nudged = si::wait_self_update_poll(std::time::Duration::from_secs(si::SELF_UPDATE_POLL_SECS));
        let change = si::exe_change_at(link, si::FileStamp::of(link));
        let show = if nudged { notice.confirm(&change) } else { notice.step(&change) };
        if !show {
            continue;
        }
        if let si::ExeChange::Replaced { path, .. } = &change {
            rexon_core::log_info!("update", "исполняемый файл заменён обновлением ({path}) — предлагаю перезапуск");
        }
        // Ждёт нажатия в этом потоке: опрос на это время стоит, и второе
        // уведомление поверх первого не встанет.
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
            // Процесс уходит — сторожить больше нечего.
            Ok(()) => return,
            Err(text) => {
                rexon_core::utils::notify_live(&si::self_update_restart_failed_title(), &text);
            }
        }
    }
}

/// Выйти из Rexon по-настоящему: снять значок, закрыть окно со стоком
/// воркеров вкладок и остановить цикл событий.
///
/// Звать можно из любого потока — работу шим переносит в поток GUI сам.
pub fn quit() {
    unsafe { rexon_tray_quit() };
}

/// Переключить главное окно на раздел с этим номером (порядок — тот же, что
/// у полосы разделов). `false` — полосы нет либо номера такого нет.
///
/// Звать можно из любого потока — по той же причине и тем же способом, что
/// и подъём окна ниже.
pub fn select_section(index: i32) -> bool {
    unsafe { rexon_tray_select_tab(index) }
}

/// Поднять главное окно этого процесса. `false` — окна не нашлось.
///
/// ⭐ Звать можно ИЗ ЛЮБОГО ПОТОКА, и это здесь главное: слушатель второго
/// запуска (`rexon_core::single_instance::ActivationListener`) живёт в своём
/// потоке, а окно у Qt разрешено трогать только в потоке GUI. Переезд в
/// нужный поток делает шим (`rexon_tray_raise_window`), и он же ждёт
/// настоящего ответа — врать второму запуску «поднял» при пустом экране
/// нельзя.
pub fn raise_main_window() -> bool {
    unsafe { rexon_tray_raise_window() }
}

/// Открыть раздел (и подраздел, `compare` — «Сравнение DE») главного окна
/// на QML. Зовётся из потока слушателя; в поток GUI переносит шим и ждёт
/// честного ответа. false — окна нет либо раздела такого нет.
pub fn open_section(section: &str, subpage: &str) -> bool {
    let (id, sub) = (cstr(section), cstr(subpage));
    unsafe { rexon_tray_open_section(id.as_ptr(), sub.as_ptr()) }
}

/// Сколько окно ждёт перед выходом, уступая замок классическому окну.
///
/// Ответ «ухожу» (`bye`) слушатель отправляет ПОСЛЕ возврата обработчика, и
/// выход не смеет его обогнать: ушедший раньше ответа процесс оставил бы
/// новый запуск с оборванным соединением. Новый процесс ждёт освобождения
/// замка до `single_instance::HANDOVER_WAIT` (5 с) — запаса хватает.
pub const HANDOVER_QUIT_DELAY_MS: i32 = 200;

/// Запланировать выход (значок, окно, приложение) и вернуться сразу.
/// `reason` — кто попросил (для журнала: «перезапуск», «передача хозяина»).
pub fn request_quit(reason: &str) {
    note_quit_reason(reason);
    rexon_core::log_info!("exit", "выход запрошен: {reason}");
    unsafe { rexon_tray_request_quit(HANDOVER_QUIT_DELAY_MS) }
}

/// Кто попросил выйти — для строки журнала после цикла событий.
static QUIT_REASON: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// Запомнить причину выхода (первая побеждает: перезапуск важнее
/// последовавшего за ним закрытия окна).
pub fn note_quit_reason(reason: &str) {
    if let Ok(mut slot) = QUIT_REASON.lock() {
        slot.get_or_insert_with(|| reason.to_string());
    }
}

/// Причина выхода; не записана — значит закрыли окно (крестик без значка в
/// лотке, «Выход» в меню окна, Ctrl+Q).
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

    /// Коды видов пунктов — те же числа, что разбирает `switch` в шиме
    /// (`cpp/rexon_tray.cpp`). Разъедутся — меню молча станет плоским.
    #[test]
    fn коды_видов_пунктов_совпадают_с_шимом() {
        assert_eq!(kind_code(core_tray::ItemKind::Action), 0);
        assert_eq!(kind_code(core_tray::ItemKind::Separator), 1);
        assert_eq!(kind_code(core_tray::ItemKind::Check), 2);
        assert_eq!(kind_code(core_tray::ItemKind::Radio), 3);
        assert_eq!(kind_code(core_tray::ItemKind::SubmenuBegin), 4);
        assert_eq!(kind_code(core_tray::ItemKind::SubmenuEnd), 5);
    }

    /// Имена методов демона, которые печатает диспетчер нажатий, обязаны
    /// существовать у него на шине. Сверяем с ядром, где тот же клиент.
    #[test]
    fn методы_демона_названы_как_в_ядре() {
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
    fn строка_с_нулём_не_роняет_значок() {
        assert_eq!(cstr("Выход").to_string_lossy(), "Выход");
        assert_eq!(cstr("Вы\0ход").to_string_lossy(), "Вы");
    }
}
