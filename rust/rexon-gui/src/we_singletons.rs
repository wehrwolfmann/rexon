// Обёртки Rust над шимом синглтонов вкладки WE (cpp/rexon_we_singletons.cpp):
// события демона — живым синглтонам Notify/App, ошибка транспорта — тостом
// окна, пускатель демона, сетевой кэш QML и движок для смены языка.
//
// Зовёт их только сам крейт (we_socket.rs, we_env.rs, status_queries.rs):
// экспортных C-символов наружу нет — прежний хозяин вкладки на Python, ради
// которого они были, в архиве. Почему сам шим на C++ — в шапке его заголовка.
//
// Файл НЕ мост cxx-qt: обычные `extern "C"`, moc'у здесь работы нет.

use std::ffi::{c_char, c_void};

/// Колбэк «запусти демона» (у Rexon — `we_socket::launch_daemon_hook`).
///
/// Пускатель Rexon знает больше оригинала: он ищет бинарь там, куда его
/// ставит сама вкладка, и на KDE доводит плагин показа. Без колбэка шим
/// падает в ветку оригинала — `QProcess::startDetached("waywallen")`.
pub type LaunchHook = Option<extern "C" fn() -> bool>;

unsafe extern "C" {
    fn rexon_we_do_deliver_event(data: *const c_char, len: i32) -> i32;
    fn rexon_we_do_transport_disconnected() -> i32;
    fn rexon_we_do_client_error(message: *const c_char, len: i32) -> i32;
    fn rexon_we_do_renderers_replace_frame(frame: *const c_char, len: i32) -> i32;
    fn rexon_we_do_set_launch_hook(hook: LaunchHook);
    fn rexon_we_do_set_network_cache(dir: *const c_char, max_bytes: i64);
    fn rexon_we_install_network_cache(engine: *mut c_void);
    fn rexon_we_do_register_qml_engine(engine: *mut c_void);
}

/// Раздать синглтонам сырой кадр `ServerFrame` от демона.
///
/// Возвращает, скольким синглтонам кадр доехал ПРЯМО СЕЙЧАС. Ноль — не
/// потеря: кадр для ещё не построенного синглтона ложится в кэш повтора и
/// уедет ему при регистрации (`cpp/rexon_we_singletons.cpp`).
pub fn deliver_singleton_event_bytes(frame: &[u8]) -> i32 {
    let Ok(len) = i32::try_from(frame.len()) else {
        return 0;
    };
    unsafe { rexon_we_do_deliver_event(frame.as_ptr().cast::<c_char>(), len) }
}

/// Обрыв WebSocket: `Notify` возвращается к пессимистичному состоянию.
pub fn transport_disconnected() -> i32 {
    unsafe { rexon_we_do_transport_disconnected() }
}

/// Ошибка транспорта → сигнал `App.errorOccurred` (тост окна вкладки).
///
/// Возвращает, доехало ли: `false` — синглтона `App` ещё нет (вкладку не
/// открывали), и показывать ошибку некому.
pub fn client_error(message: &str) -> bool {
    let Ok(len) = i32::try_from(message.len()) else {
        return false;
    };
    unsafe { rexon_we_do_client_error(message.as_ptr().cast::<c_char>(), len) != 0 }
}

/// Полная замена списка рендереров ответом `renderer_list`.
///
/// `frame` — СЫРОЙ кадр демона, тот же, что разобрал запрос. Дверь
/// внутренняя (наружу C-символа нет): `RendererListQuery` живёт в другом
/// мосте того же крейта, и добраться до живых моделей `App` он может
/// только через шим — у оригинала этот запрос и менеджер лежат в одном
/// C++-объектном графе.
///
/// Возвращает, доехал ли кадр: `false` — синглтона `App` ещё нет.
pub fn renderers_replace_frame(frame: &[u8]) -> bool {
    if frame.is_empty() || i32::try_from(frame.len()).is_err() {
        return false;
    }
    let ok = unsafe {
        rexon_we_do_renderers_replace_frame(frame.as_ptr().cast::<c_char>(), frame.len() as i32)
    };
    ok != 0
}

/// Поставить пускатель демона. `None` снимает его.
pub fn set_launch_hook(hook: LaunchHook) {
    unsafe { rexon_we_do_set_launch_hook(hook) }
}

/// Задать каталог и предел сетевого кэша QML-картинок (до установки фабрики).
///
/// # Safety
/// `dir` — живая нуль-терминированная C-строка.
pub unsafe fn set_network_cache(dir: *const c_char, max_bytes: i64) {
    if dir.is_null() {
        return;
    }
    unsafe { rexon_we_do_set_network_cache(dir, max_bytes) }
}

/// Поставить движку QML фабрику менеджера сети с дисковым кэшем.
///
/// Приведение к QQmlEngine внутри шима проверенное, чужой объект он не
/// примет.
///
/// # Safety
/// `engine` — живой QObject.
pub unsafe fn install_qml_network_cache(engine: *mut c_void) {
    if engine.is_null() {
        return;
    }
    unsafe { rexon_we_install_network_cache(engine) }
}

/// Записать движок QML вкладки для смены языка на лету.
///
/// Комбобокс «Язык» на вендорной странице настроек меняет каталог
/// переводов, а привязки `qsTr()` уже показанной страницы пересобирает сам
/// движок (`QQmlEngine::retranslate`). Без этой записи выбор запоминался бы,
/// но страница оставалась бы на прежнем языке до перезапуска.
///
/// # Safety
/// `engine` — живой QObject.
pub unsafe fn register_qml_engine(engine: *mut c_void) {
    if engine.is_null() {
        return;
    }
    unsafe { rexon_we_do_register_qml_engine(engine) }
}
