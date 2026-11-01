// Транспорт вкладки Wallpaper Engine: сокет к демону waywallen и его
// обвязка —
// перенос `rexon/qt_wallpaper_engine/client.py` на Rust/cxx-qt.
//
// Что здесь есть и чего здесь нет:
//
//   * ЕСТЬ жизненный цикл сокета (open/close/is_open), карта ожидающих,
//     разбор входящего кадра, раздвоение событий, поиск и автозапуск
//     демона, реконнект и синхронная остановка обоев;
//   * НЕТ ни одного РЕШЕНИЯ — все они уже портированы в ядро
//     (`rexon_core::we_client`): адреса шины, таймауты, поиск бинаря,
//     `ensure_running_decision`, `poll_decision`, выдача `request_id`,
//     сборка кадров, `stop_active_gate`/`stop_active_result`. Здесь только
//     Qt-исполнение. Второй копии правил в крейте быть не должно.
//
// ⚠️ Всё в этом модуле живёт в ОДНОМ потоке — том, где его позвали. Чужих
// потоков в транспорте нет вовсе (это осознанное решение оригинала:
// «Никаких QThread», client.py:16), поэтому состояние держится в
// `thread_local` и синхронизации не требует. Исключение — счётчик
// `request_id`: его спрашивают и запросы вкладки, и синхронная остановка
// обоев из потока установщика, поэтому он под мьютексом.
//
// ⚠️ Ответ демона приходит В ГЛАВНОМ ПОТОКЕ и доставляется СИНХРОННО.
// Очередь `CxxQtThread` сюда ставить нельзя: она всегда откладывает вызов
// на такт цикла событий (замер макета: `beforeLoop=0`), то есть меняет
// наблюдаемый порядок сигналов запроса, а он записан голденами. Живучесть
// адресата держит `QPointer` в `cpp/rexon_we_transport.cpp`.

/// Мост cxx-qt: чужой C++-QObject (QWebSocket) и шимы транспорта.
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qbytearray.h");
        type QByteArray = cxx_qt_lib::QByteArray;
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qurl.h");
        type QUrl = cxx_qt_lib::QUrl;
        // ⚠️ qint64, а не i64. cxx печатает i64 как `::std::int64_t`, на
        // LP64 это `long`, а Qt объявляет `qint64` как `long long` —
        // указатель на метод не приводится, C++ не собирается.
        include!("cxx-qt-lib/qtypes.h");
        type qint64 = cxx_qt_lib::qint64;
    }

    // QWebSocket объявляется КАК ЕСТЬ: обёртки в cxx-qt-lib 0.9.1 нет — в
    // крейте вообще нет модуля network (только core/gui/qml/quickcontrols).
    // `#[qobject]` обязателен, иначе `#[qsignal]` в блоке не разрешён.
    unsafe extern "C++Qt" {
        include!(<QtWebSockets/QWebSocket>);
        #[qobject]
        type QWebSocket;

        #[rust_name = "open_url"]
        fn open(self: Pin<&mut QWebSocket>, url: &QUrl);

        #[cxx_name = "sendBinaryMessage"]
        fn send_binary_message(self: Pin<&mut QWebSocket>, data: &QByteArray) -> qint64;

        // ⚠️ close() объявить здесь НЕЛЬЗЯ: у него два аргумента по
        // умолчанию, а cxx берёт АДРЕС метода по написанной сигнатуре.
        // Ушло в шим — rexon_we_sock_close().

        #[qsignal]
        fn connected(self: Pin<&mut QWebSocket>);
        #[qsignal]
        fn disconnected(self: Pin<&mut QWebSocket>);
        #[qsignal]
        #[cxx_name = "binaryMessageReceived"]
        fn binary_message_received(self: Pin<&mut QWebSocket>, message: &QByteArray);
        // errorOccurred(QAbstractSocket::SocketError) объявить нельзя по той
        // же причине, что и close(): аргумент — чужой enum, которого в
        // cxx-qt-lib нет. Подписку делает шим и отдаёт готовый текст.
    }

    // Шимы: всё, чего cxx-qt не умеет (см. cpp/rexon_we_socket.h).
    unsafe extern "C++" {
        include!("rexon_we_socket.h");

        #[rust_name = "sock_daemon_property"]
        fn rexon_we_sock_daemon_property(
            service: &QString,
            path: &QString,
            iface: &QString,
            name: &QString,
            timeout_ms: i32,
            ok: &mut bool,
        ) -> QString;

        #[rust_name = "sock_new"]
        fn rexon_we_sock_new() -> *mut QWebSocket;

        /// # Safety
        /// `object` создан фабрикой выше и ещё не удалён.
        #[allow(clippy::missing_safety_doc)]
        #[rust_name = "sock_destroy"]
        unsafe fn rexon_we_sock_destroy(object: *mut QObject);

        /// # Safety
        /// `socket` — живой QWebSocket либо nullptr.
        #[allow(clippy::missing_safety_doc)]
        #[rust_name = "sock_close"]
        unsafe fn rexon_we_sock_close(socket: *mut QWebSocket);

        /// # Safety
        /// `socket` — живой QWebSocket либо nullptr.
        #[allow(clippy::missing_safety_doc)]
        #[rust_name = "sock_is_open"]
        unsafe fn rexon_we_sock_is_open(socket: *mut QWebSocket) -> bool;

        /// # Safety
        /// `socket` — живой QWebSocket либо nullptr.
        #[allow(clippy::missing_safety_doc)]
        #[rust_name = "sock_error_string"]
        unsafe fn rexon_we_sock_error_string(socket: *mut QWebSocket) -> QString;

        /// # Safety
        /// `socket` — живой QWebSocket.
        #[allow(clippy::missing_safety_doc)]
        #[rust_name = "sock_watch_error"]
        unsafe fn rexon_we_sock_watch_error(socket: *mut QWebSocket);

        #[rust_name = "sock_timer_start"]
        fn rexon_we_sock_timer_start(id: i32, interval_ms: i32, single_shot: bool);
        #[rust_name = "sock_timer_stop"]
        fn rexon_we_sock_timer_stop(id: i32);
        /// Звать можно из любого потока (см. `rexon_we_socket.h`).
        #[rust_name = "sock_post_tick"]
        fn rexon_we_sock_post_tick(id: i32);

        #[rust_name = "sock_qt_available"]
        fn rexon_we_sock_qt_available() -> bool;

        #[rust_name = "sock_run_loop"]
        fn rexon_we_sock_run_loop(ms: i32) -> i32;
        #[rust_name = "sock_quit_loop"]
        fn rexon_we_sock_quit_loop();
    }
}

use core::pin::Pin;
use cxx_qt::QMetaObjectConnectionGuard;
use cxx_qt_lib::{QByteArray, QString, QUrl};
use prost::Message;
use qobject::QWebSocket;
use std::cell::{Cell, RefCell};
use std::ffi::c_char;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use rexon_core::proto::control as pb;
use rexon_core::waywallen_client::DbusValue;
use rexon_core::we_client;

// Указатели на функции через cxx не выражаются — эти два шима объявляются
// обычным `extern "C"`, как это уже сделано в `we_transport.rs`.
unsafe extern "C" {
    fn rexon_we_sock_set_timer_tick(tick: Option<extern "C" fn(i32)>);
    fn rexon_we_sock_set_error_hook(hook: Option<extern "C" fn(*const c_char, i32)>);
}

// ── номера таймеров шима ────────────────────────────────────────────────────

/// Поллинг D-Bus после автозапуска демона (`DaemonControl._poll_once`).
const TIMER_POLL: i32 = 1;
/// Реконнект после обрыва (переехал из `window.py:1136-1147`).
const TIMER_RECONNECT: i32 = 2;

/// Сторож ответа `health` (переехал из `qml_window.py`: был `QTimer` окна).
const TIMER_HEALTH: i32 = 3;
/// Отложенный старт демона при открытии вкладки (`QTimer.singleShot` окна).
const TIMER_START: i32 = 4;
/// Поднять демона, ушедшего по `Quit` ради плагина из пакета. Не таймер, а
/// разовый тик из фонового потока перезапуска (`sock_post_tick`).
const TIMER_PLUGIN_RELAUNCH: i32 = 5;
/// Придержанные кадры применения обоев на KDE: проверка «пора отпускать»
/// (повторяющийся таймер плюс разовый тик из потока назначения плагина).
const TIMER_KDE_HOLD: i32 = 6;
/// Откат обоев стола закончился (разовый тик из потока отката) — сказать
/// человеку, чем кончилось.
const TIMER_KDE_REVERT: i32 = 7;
/// Подготовка значков стола X11 закончилась (разовый тик из её потока) —
/// сказать человеку, что со значками.
const TIMER_DESKTOP_ICONS: i32 = 8;
/// Шаг проверки придержанных кадров.
const KDE_HOLD_TICK_MS: i32 = 250;

/// Пауза реконнекта — та же, что стояла в окне: 3000 мс, без бэкоффа и без
/// лимита попыток. Улучшать здесь нечего и не время.
const RECONNECT_MS: i32 = 3000;

// ── выдача request_id ───────────────────────────────────────────────────────

/// Счётчик `request_id` — ОДИН на процесс.
///
/// Раньше номеров было два: свой токен у Rust-запроса и свой `request_id` у
/// питоновского клиента, который ради этого разбирал готовый кадр `prost`-ом
/// и собирал его обратно. Теперь номер один, разбор один.
fn ids() -> &'static Mutex<we_client::RequestIds> {
    static IDS: OnceLock<Mutex<we_client::RequestIds>> = OnceLock::new();
    IDS.get_or_init(|| Mutex::new(we_client::RequestIds::default()))
}

/// Следующий `request_id`.
pub fn alloc_request_id() -> i64 {
    let mut guard = ids().lock().expect("счётчик request_id не отравлен");
    guard.alloc() as i64
}

/// Начать нумерацию заново (нужно стендам: голден нумерует кадры с единицы).
pub fn reset_request_ids() {
    *ids().lock().expect("счётчик request_id не отравлен") = we_client::RequestIds::default();
}

// ── состояние транспорта ────────────────────────────────────────────────────

/// Колбэки пробы транспорта (`set_hooks`). `extern "C"`, потому что
/// `on_socket_error` с той же сигнатурой ставится и в C++-шим сокета.
pub type VoidHook = Option<extern "C" fn()>;
pub type TextHook = Option<extern "C" fn(*const c_char, i32)>;
pub type FrameHook = Option<extern "C" fn(i64, *const c_char, i32)>;

thread_local! {
    /// Живой сокет вкладки. Голый указатель, а не RefCell со структурой:
    /// обработчик ответа синхронно зовёт запрос, а тот может тут же послать
    /// новый кадр — при долгом заимствовании это была бы паника RefCell.
    static SOCKET: Cell<*mut QWebSocket> = const { Cell::new(std::ptr::null_mut()) };
    /// Стражи подключений к сигналам сокета. Живут ровно столько же, сколько
    /// сам сокет: снимаются ПЕРЕД удалением, иначе страж дёрнет мёртвый объект.
    static GUARDS: RefCell<Vec<QMetaObjectConnectionGuard>> = const { RefCell::new(Vec::new()) };
    /// Порт последнего успешного `open` — реконнекту он не нужен (порт
    /// перечитывается по шине), а журналу и стенду нужен.
    static PORT: Cell<i32> = const { Cell::new(0) };
    /// Было ли соединение установлено хоть раз. Реконнект без этого не
    /// стартует — ровно как `_was_connected` в окне вкладки.
    static WAS_CONNECTED: Cell<bool> = const { Cell::new(false) };
    /// Сторож идущего запуска демона (решения — ядра,
    /// `we_client::StartWatch`) и чем запущен движок. `None` — запуска нет.
    static START: RefCell<Option<StartRun>> = const { RefCell::new(None) };
    /// Сокет закрывают НАМЕРЕННО. Без этого флага обработчик обрыва тут же
    /// заводил бы реконнект и открывал сокет обратно — «закрыть» перестало
    /// бы означать «закрыть».
    static CLOSING: Cell<bool> = const { Cell::new(false) };
    /// Показывать ли тостом очередную ошибку транспорта — решение ядра
    /// (`we_client::TransportErrorToast`): один тост на обрыв.
    static ERROR_TOAST: RefCell<we_client::TransportErrorToast> =
        RefCell::new(we_client::TransportErrorToast::default());

    static ON_CONNECTED: Cell<VoidHook> = const { Cell::new(None) };
    static ON_DISCONNECTED: Cell<VoidHook> = const { Cell::new(None) };
    static ON_ERROR: Cell<TextHook> = const { Cell::new(None) };
    static ON_RESPONSE: Cell<FrameHook> = const { Cell::new(None) };
    static ON_EVENT: Cell<TextHook> = const { Cell::new(None) };

    /// Подписчики на Rust (объект действий страницы, `we_actions.rs`).
    ///
    /// Ячейки `ON_*` выше — слот ОДНОГО потребителя (пробы транспорта), и
    /// забирает его последний поставивший. Подписчиков на Rust может быть
    /// сколько угодно, поэтому событие уходит КАЖДОМУ: слоту и всем из
    /// этого списка.
    static SUBSCRIBERS: RefCell<Vec<(u64, Rc<dyn Fn(&DaemonEvent)>)>> =
        const { RefCell::new(Vec::new()) };
    static NEXT_SUBSCRIBER: Cell<u64> = const { Cell::new(0) };
    /// `request_id` ожидаемого ответа `health`; 0 — не ждём.
    static HEALTH_ID: Cell<u64> = const { Cell::new(0) };
    /// `request_id` своего `plugin_list` (сверка плагинов с пакетом); 0 — не ждём.
    static PLUGINS_ID: Cell<u64> = const { Cell::new(0) };
    /// Дисплеи демона по его событиям (`display_snapshot/changed/removed`).
    /// Нужны одному решению: есть ли куда показывать применённые обои.
    static DISPLAY_IDS: RefCell<std::collections::BTreeSet<u64>> =
        const { RefCell::new(std::collections::BTreeSet::new()) };
    /// Кадры применения, придержанные до назначения плагина KDE.
    static HELD: RefCell<Vec<Vec<u8>>> = const { RefCell::new(Vec::new()) };
    /// Когда начали придерживать.
    static HELD_SINCE: Cell<Option<std::time::Instant>> = const { Cell::new(None) };
    /// Сторож отпущенного применения (решение — ядро, `ApplyWatch`) и
    /// момент отправки кадра: не удалось — столам возвращаются прежние обои.
    static APPLY_WATCH: RefCell<Option<(rexon_core::waywallen_kde_plugin::ApplyWatch, std::time::Instant)>> =
        const { RefCell::new(None) };
}

fn call_text(hook: &'static std::thread::LocalKey<Cell<TextHook>>, text: &[u8]) {
    let Some(f) = hook.with(Cell::get) else { return };
    let len = i32::try_from(text.len()).unwrap_or(i32::MAX);
    f(text.as_ptr().cast::<c_char>(), len);
}

// ── разбор входящего кадра ──────────────────────────────────────────────────

/// Что приехало по сокету. `request_id` лежит ВНУТРИ `Response`, а не в
/// оболочке `ServerFrame` — это и есть ключ маршрутизации.
enum Frame {
    Response(u64),
    Event(pb::Event),
}

fn read_frame(bytes: &[u8]) -> Result<Frame, prost::DecodeError> {
    let frame = pb::ServerFrame::decode(bytes)?;
    match frame.kind {
        Some(pb::server_frame::Kind::Response(resp)) => Ok(Frame::Response(resp.request_id)),
        Some(pb::server_frame::Kind::Event(event)) => Ok(Frame::Event(event)),
        None => Err(prost::DecodeError::new("пустой кадр сервера")),
    }
}

/// Кадр `ServerFrame{Response{request_id, status, message}}` без полезной
/// нагрузки — им закрываются ожидающие запросы при обрыве соединения.
///
/// Не новый механизм, а тот же путь ответа: `we_query::take_response` видит
/// не-OK статус и переводит запрос в Error с этим текстом. Отдельных
/// инвокаблов «транспорт умер» заводить не пришлось.
fn error_frame(request_id: u64, message: &str) -> Vec<u8> {
    let resp = pb::Response {
        request_id,
        status: pb::Status::Internal as i32,
        message: message.to_owned(),
        ..Default::default()
    };
    pb::ServerFrame { kind: Some(pb::server_frame::Kind::Response(resp)) }.encode_to_vec()
}

/// Текст, который увидит пользователь вместо вечного спиннера.
const DISCONNECTED_TEXT: &str = rexon_core::i18n::n_("The connection to the wallpaper engine was lost");

/// Раздать кадр демона его адресатам.
///
/// Адресаты:
///   * ответ → своим служебным запросам (`health`, `plugin_list`), иначе
///     Rust-запросу по карте `QPointer` (шим), иначе слоту пробы;
///   * событие → прогрессу длинных задач, синглтонам `Notify`/`App` и слоту
///     пробы. Раздвоение обязательно: потребители разные.
pub fn dispatch_frame(bytes: &[u8]) -> bool {
    match read_frame(bytes) {
        Ok(Frame::Response(request_id)) => {
            // Ответ на СВОЙ `health` (его шлёт `request_health`) — ни Rust-
            // запросу вкладки, ни пробе он не адресован.
            if request_id != 0 && request_id == HEALTH_ID.with(Cell::get) {
                HEALTH_ID.with(|c| c.set(0));
                qobject::sock_timer_stop(TIMER_HEALTH);
                broadcast(&DaemonEvent::Health(health_state(bytes)));
                request_plugin_check();
                return true;
            }
            // Ответ на СВОЙ `plugin_list` (сверка с пакетом) — тоже ничей.
            if request_id != 0 && request_id == PLUGINS_ID.with(Cell::get) {
                PLUGINS_ID.with(|c| c.set(0));
                on_plugin_list(bytes);
                return true;
            }
            watch_response(request_id, bytes);
            if crate::we_transport::deliver_response_bytes(request_id as i64, bytes) {
                return true;
            }
            let Some(f) = ON_RESPONSE.with(Cell::get) else { return false };
            let len = i32::try_from(bytes.len()).unwrap_or(i32::MAX);
            f(request_id as i64, bytes.as_ptr().cast::<c_char>(), len);
            true
        }
        Ok(Frame::Event(event)) => {
            let changed = DISPLAY_IDS.with(|ids| {
                rexon_core::waywallen_kde_plugin::track_displays(&mut ids.borrow_mut(), &event)
            });
            if changed {
                check_kde_hold();
            }
            // Сторож отпущенного применения: связи дисплеев, появление и
            // снятие рендереров (плейлист рендерер в ответе не называет —
            // ядро узнаёт его по дисплеям, `ApplyWatch::on_event`).
            watch_verdict(|w| w.on_event(&event));
            crate::we_transport::deliver_event_bytes(bytes);
            crate::we_singletons::deliver_singleton_event_bytes(bytes);
            call_text(&ON_EVENT, bytes);
            true
        }
        Err(e) => {
            // Битый кадр обязан давать ошибку и теряться, а не ронять
            // процесс: демон может быть новее нас.
            let text = rexon_core::we_client::frame_parse_error_message(&e.to_string());
            report_error(text.as_bytes());
            false
        }
    }
}

// ── жизненный цикл сокета ───────────────────────────────────────────────────

fn socket_ptr() -> *mut QWebSocket {
    SOCKET.with(Cell::get)
}

/// Создать сокет и подписаться на его сигналы. Идемпотентно.
fn ensure_socket() -> *mut QWebSocket {
    let existing = socket_ptr();
    if !existing.is_null() {
        return existing;
    }
    let socket = qobject::sock_new();
    if socket.is_null() {
        return socket;
    }
    let mut sock = unsafe { Pin::new_unchecked(&mut *socket) };

    let on_connected = sock.as_mut().on_connected(|_sock| {
        WAS_CONNECTED.with(|c| c.set(true));
        ERROR_TOAST.with(|t| t.borrow_mut().on_connected());
        qobject::sock_timer_stop(TIMER_RECONNECT);
        // Запросы вкладки спрашивают соединение перед отправкой кадра. В
        // Python это делал каждый сам (`_client.is_open()`), здесь состояние
        // им приносит транспорт — одно место на всех (правило #12).
        crate::discover_queries::set_connected(true);
        if let Some(f) = ON_CONNECTED.with(Cell::get) {
            f();
        }
        broadcast(&DaemonEvent::Connected);
    });
    let on_message = sock.as_mut().on_binary_message_received(|_sock, message| {
        let bytes: Vec<u8> = message.into();
        dispatch_frame(&bytes);
    });
    let on_disconnected = sock.as_mut().on_disconnected(|_sock| on_socket_closed());

    GUARDS.with(|g| {
        let mut list = g.borrow_mut();
        list.push(on_connected);
        list.push(on_message);
        list.push(on_disconnected);
    });
    unsafe { qobject::sock_watch_error(socket) };

    SOCKET.with(|c| c.set(socket));
    socket
}

/// Обрыв соединения: ожидающие получают ошибку, подписчики — уведомление,
/// реконнект — старт.
fn on_socket_closed() {
    crate::discover_queries::set_connected(false);
    // Дисплеи придут заново снимком после переподключения. Придержанные
    // кадры выбрасываются: их запросы сейчас же получат ошибку обрыва
    // (`fail_pending` ниже), и слать их в новое соединение было бы поздно.
    DISPLAY_IDS.with(|ids| ids.borrow_mut().clear());
    drop_kde_hold();
    // ⚠️ Починка бага оригинала (правило #16): `_on_disconnected`
    // чистил карту молча — колбэки не звались НИКОГДА, запрос оставался в
    // статусе Querying навсегда, и пользователь видел вечный спиннер вместо
    // ошибки. Теперь каждому ожидающему уходит кадр с не-OK статусом.
    let frame = error_frame(0, &rexon_core::i18n::tr(DISCONNECTED_TEXT));
    crate::we_transport::fail_pending(&frame);

    if let Some(f) = ON_DISCONNECTED.with(Cell::get) {
        f();
    }
    // `Notify` обязан узнать об обрыве сам: раньше ему сообщал питоновский
    // хозяин окна, а с его уходом не сообщал никто — фаза демона оставалась
    // «готов» и без демона, и всё, что по ней живёт (карточка текущих обоев
    // в рейке, подсказки вендорных страниц), показывало вчерашний день.
    crate::we_singletons::transport_disconnected();
    // Ответа `health` с оборванного сокета уже не будет. Сторож при этом
    // снимается, а не дожидается срока: у питоновского окна он оставался
    // заведённым, и обрыв посреди проверки через 10 с объявлял движок
    // мёртвым — хотя реконнект к тому времени уже вернул страницы.
    HEALTH_ID.with(|c| c.set(0));
    PLUGINS_ID.with(|c| c.set(0));
    qobject::sock_timer_stop(TIMER_HEALTH);
    broadcast(&DaemonEvent::Disconnected {
        was_connected: WAS_CONNECTED.with(Cell::get),
    });
    // Реконнект только после состоявшегося соединения — та же оговорка,
    // что стояла в окне вкладки (`_was_connected`), — и только если рвалось
    // не по нашей воле.
    if WAS_CONNECTED.with(Cell::get) && !CLOSING.with(Cell::get) {
        qobject::sock_timer_start(TIMER_RECONNECT, RECONNECT_MS, false);
    }
    CLOSING.with(|c| c.set(false));
}

/// Открыть сокет на этот порт. URL захардкожен так же, как в оригинале
/// (`client.py:513`): демон слушает только петлю.
pub fn open(port: i32) -> bool {
    let socket = ensure_socket();
    if socket.is_null() || port <= 0 {
        return false;
    }
    CLOSING.with(|c| c.set(false));
    PORT.with(|c| c.set(port));
    let mut sock = unsafe { Pin::new_unchecked(&mut *socket) };
    sock.as_mut()
        .open_url(&QUrl::from(format!("ws://127.0.0.1:{port}/").as_str()));
    true
}

pub fn close() {
    qobject::sock_timer_stop(TIMER_RECONNECT);
    let socket = socket_ptr();
    if socket.is_null() {
        return;
    }
    CLOSING.with(|c| c.set(true));
    unsafe { qobject::sock_close(socket) };
}

pub fn is_open() -> bool {
    let socket = socket_ptr();
    !socket.is_null() && unsafe { qobject::sock_is_open(socket) }
}

/// Отправить кадр запроса `field` с учётом того, кто поднимает дисплей.
///
/// Единственный вход для кадров запросов вкладки (`we_transport::send`).
/// Там, где дисплей демона поднимает Rexon (`wallpaper_backend::DisplaySetup`:
/// KDE — плагин, X11 — хост-клиент вывода), применение обоев при нуле
/// дисплеев придерживается: демон без дисплея его отклонит
/// (`NoDisplayRegistered`). Дисплей поднимается здесь — по действию
/// человека, а не при открытии вкладки.
pub fn send_request_frame(field: &str, frame: &[u8]) {
    use rexon_core::waywallen_kde_plugin::{apply_gate, ApplyGate};
    if !is_open() {
        send_frame(frame);
        return;
    }
    let holding = HELD.with(|h| !h.borrow().is_empty());
    let displays = DISPLAY_IDS.with(|ids| ids.borrow().len());
    let gate = if holding && rexon_core::waywallen_kde_plugin::request_shows_wallpaper(field) {
        // Уже ждём дисплея — новый кадр применения встаёт в ту же очередь,
        // чтобы порядок применений не перепутался.
        ApplyGate::HoldAndSelect
    } else {
        apply_gate(display_setup().rexon_brings_display(), field, displays)
    };
    if rexon_core::waywallen_kde_plugin::request_shows_wallpaper(field) {
        // Человек применил что-то ещё — следить за прежним применением
        // незачем: его новое действие главнее.
        watch_verdict(|w| w.on_new_apply());
        if display_setup() == rexon_core::wallpaper_backend::DisplaySetup::RexonHost {
            start_desktop_icons_prepare();
        }
    }
    match gate {
        ApplyGate::Send => {
            send_frame(frame);
        }
        ApplyGate::HoldAndSelect => {
            HELD.with(|h| h.borrow_mut().push(frame.to_vec()));
            if holding {
                return;
            }
            HELD_SINCE.with(|c| c.set(Some(std::time::Instant::now())));
            if let Ok(mut slot) = KDE_PREVIOUS.lock() {
                *slot = None;
            }
            KDE_SELECTED_NOW.store(false, Ordering::SeqCst);
            KDE_REVERT_WANTED.store(false, Ordering::SeqCst);
            KDE_SELECT_STATE.store(KDE_SELECT_RUNNING, Ordering::SeqCst);
            qobject::sock_timer_start(TIMER_KDE_HOLD, KDE_HOLD_TICK_MS, false);
            if display_setup() == rexon_core::wallpaper_backend::DisplaySetup::RexonHost {
                rexon_core::log_info!("x11-display", "применение обоев: дисплеев нет — поднимаем вывод на рабочий стол X11, кадр «{field}» ждёт");
                start_rexon_display_host();
            } else {
                rexon_core::log_info!("kde-plugin", "применение обоев: дисплеев нет — назначаем плагин обоев KDE, кадр «{field}» ждёт");
                let daemon_version = KDE_DAEMON_VERSION.lock().ok().and_then(|v| v.clone());
                prepare_display_backend(daemon_version, find_daemon_binary(), false, KdePurpose::Apply);
            }
        }
    }
}

/// Состояние назначения плагина ради придержанных кадров.
static KDE_SELECT_STATE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(KDE_SELECT_IDLE);
const KDE_SELECT_IDLE: u8 = 0;
const KDE_SELECT_RUNNING: u8 = 1;
const KDE_SELECT_OK: u8 = 2;
const KDE_SELECT_FAILED: u8 = 3;

/// Плагины столов до назначения — снимок для отката.
static KDE_PREVIOUS: Mutex<Option<Vec<rexon_core::waywallen_kde_plugin::DesktopPlugin>>> = Mutex::new(None);
/// Плагин назначен ради этого применения (до него его на столах не было).
static KDE_SELECTED_NOW: AtomicBool = AtomicBool::new(false);
/// Откат нужен, но назначение ещё не отчиталось — сделать по его концу.
static KDE_REVERT_WANTED: AtomicBool = AtomicBool::new(false);
/// Итог потока отката для главного потока.
static KDE_REVERT_RESULT: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(KDE_REVERT_NONE);
const KDE_REVERT_NONE: u8 = 0;
const KDE_REVERT_OK: u8 = 1;
const KDE_REVERT_FAILED: u8 = 2;

/// Вывод поднят не назначением плагина (X11): откатывать нечего, дальше
/// придержку ведёт то же ядро (`release_held`).
fn kde_select_finished_without_plugin(ok: bool) {
    if let Ok(mut slot) = KDE_PREVIOUS.lock() {
        *slot = None;
    }
    KDE_SELECTED_NOW.store(false, Ordering::SeqCst);
    KDE_SELECT_STATE.store(if ok { KDE_SELECT_OK } else { KDE_SELECT_FAILED }, Ordering::SeqCst);
    qobject::sock_post_tick(TIMER_KDE_HOLD);
}

/// Поток назначения закончил (любой поток) — разбудить проверку в главном.
///
/// `previous` — плагины столов до назначения (есть, только когда плагин
/// назначен только что).
fn kde_select_finished(
    selected: bool,
    previous: Option<Vec<rexon_core::waywallen_kde_plugin::DesktopPlugin>>,
) {
    if let Ok(mut slot) = KDE_PREVIOUS.lock() {
        *slot = previous;
    }
    KDE_SELECTED_NOW.store(selected, Ordering::SeqCst);
    KDE_SELECT_STATE.store(if selected { KDE_SELECT_OK } else { KDE_SELECT_FAILED }, Ordering::SeqCst);
    qobject::sock_post_tick(TIMER_KDE_HOLD);
}

/// Отпустить придержанные кадры, если пора (решение — ядро), и вести
/// сторож уже отпущенного применения.
fn check_kde_hold() {
    if KDE_REVERT_WANTED.load(Ordering::SeqCst)
        && KDE_SELECT_STATE.load(Ordering::SeqCst) != KDE_SELECT_RUNNING
    {
        KDE_REVERT_WANTED.store(false, Ordering::SeqCst);
        start_kde_revert();
    }
    if HELD.with(|h| h.borrow().is_empty()) {
        if let Some(elapsed) = APPLY_WATCH.with(|w| w.borrow().as_ref().map(|(_, t)| t.elapsed())) {
            let ms = u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX);
            watch_verdict(|w| w.on_tick(ms));
        }
        let watching = APPLY_WATCH.with(|w| w.borrow().is_some());
        if !watching && !KDE_REVERT_WANTED.load(Ordering::SeqCst) {
            qobject::sock_timer_stop(TIMER_KDE_HOLD);
        }
        return;
    }
    let state = KDE_SELECT_STATE.load(Ordering::SeqCst);
    let done = matches!(state, KDE_SELECT_OK | KDE_SELECT_FAILED);
    let ok = state == KDE_SELECT_OK;
    let displays = DISPLAY_IDS.with(|ids| ids.borrow().len());
    let waited = HELD_SINCE
        .with(Cell::get)
        .map_or(u64::MAX, |t| u64::try_from(t.elapsed().as_millis()).unwrap_or(u64::MAX));
    if !rexon_core::waywallen_kde_plugin::release_held(done, ok, displays, waited) {
        return;
    }
    let frames = HELD.with(|h| std::mem::take(&mut *h.borrow_mut()));
    HELD_SINCE.with(|c| c.set(None));
    rexon_core::log_info!(
        "kde-plugin",
        "применение обоев: отпускаем {} кадр(ов) — дисплеев {displays}, вывод: {}, ждали {waited} мс",
        frames.len(),
        rexon_core::waywallen_kde_plugin::select_state_for_log(done, ok)
    );
    // Что окажется на столе, решает ПОСЛЕДНИЙ кадр — за ним и следим.
    let last_id = frames.last().and_then(|f| pb::Request::decode(f.as_slice()).ok()).map(|r| r.request_id);
    for frame in &frames {
        send_frame(frame);
    }
    APPLY_WATCH.with(|w| {
        *w.borrow_mut() = last_id
            .map(|id| (rexon_core::waywallen_kde_plugin::ApplyWatch::new(id), std::time::Instant::now()));
    });
    // Таймер продолжает тикать — теперь для сторожа.
}

/// Ответ демона на наблюдаемый кадр применения — отдать сторожу.
fn watch_response(request_id: u64, bytes: &[u8]) {
    let watched = APPLY_WATCH.with(|w| w.borrow().as_ref().is_some_and(|(w, _)| w.watches(request_id)));
    if !watched {
        return;
    }
    let Ok(pb::ServerFrame { kind: Some(pb::server_frame::Kind::Response(rsp)) }) =
        pb::ServerFrame::decode(bytes)
    else {
        return;
    };
    let ok = rsp.status == pb::Status::Ok as i32;
    let renderer_id = match &rsp.payload {
        Some(pb::response::Payload::WallpaperApply(apply)) => apply.renderer_id.clone(),
        _ => String::new(),
    };
    if !ok {
        rexon_core::log_warn!("kde-plugin", "применение обоев не удалось: {}", rsp.message);
    }
    watch_verdict(|w| w.on_response(request_id, ok, &renderer_id));
}

/// Шаг сторожа применения; исход — по решению ядра.
fn watch_verdict(step: impl FnOnce(&mut rexon_core::waywallen_kde_plugin::ApplyWatch) -> rexon_core::waywallen_kde_plugin::WatchVerdict) {
    use rexon_core::waywallen_kde_plugin::WatchVerdict;
    let verdict = APPLY_WATCH.with(|w| w.borrow_mut().as_mut().map(|(watch, _)| step(watch)));
    match verdict {
        None | Some(WatchVerdict::Keep) => {}
        Some(WatchVerdict::Done) => {
            APPLY_WATCH.with(|w| *w.borrow_mut() = None);
        }
        Some(WatchVerdict::Revert) => {
            APPLY_WATCH.with(|w| *w.borrow_mut() = None);
            if KDE_SELECT_STATE.load(Ordering::SeqCst) == KDE_SELECT_RUNNING {
                // Снимка прежних плагинов ещё нет — откат по концу назначения.
                KDE_REVERT_WANTED.store(true, Ordering::SeqCst);
                qobject::sock_timer_start(TIMER_KDE_HOLD, KDE_HOLD_TICK_MS, false);
            } else {
                start_kde_revert();
            }
        }
    }
}

/// Вернуть столам плагин, стоявший до назначения (решение — ядро). Шина
/// Plasma медленная — в отдельном потоке; итог — тиком `TIMER_KDE_REVERT`.
fn start_kde_revert() {
    if display_setup() == rexon_core::wallpaper_backend::DisplaySetup::RexonHost {
        start_desktop_icons_restore();
    }
    let previous = KDE_PREVIOUS.lock().ok().and_then(|mut slot| slot.take());
    let selected_now = KDE_SELECTED_NOW.swap(false, Ordering::SeqCst);
    let Some(js) = rexon_core::waywallen_kde_plugin::revert_plugins_js(selected_now, previous.as_deref()) else {
        rexon_core::log_info!(
            "kde-plugin",
            "применение обоев не удалось; откатывать нечего (плагин стоял до применения или снимка нет)"
        );
        return;
    };
    rexon_core::log_info!("kde-plugin", "применение обоев не удалось — возвращаем столам прежние обои");
    let spawned = std::thread::Builder::new().name("rexon-kde-revert".into()).spawn(move || {
        let ok = {
            let _guard = KDE_ENSURE_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            plasma_evaluate(&KdeEnv::live(), &js, 15.0).is_some()
        };
        KDE_REVERT_RESULT.store(if ok { KDE_REVERT_OK } else { KDE_REVERT_FAILED }, Ordering::SeqCst);
        qobject::sock_post_tick(TIMER_KDE_REVERT);
    });
    if let Err(e) = spawned {
        rexon_core::log_warn!("kde-plugin", "откат обоев не запущен: {e}");
        KDE_REVERT_RESULT.store(KDE_REVERT_FAILED, Ordering::SeqCst);
        kde_revert_finished();
    }
}

/// Поток отката закончил — сказать человеку, чем кончилось.
fn kde_revert_finished() {
    use rexon_core::waywallen_kde_plugin as wkp;
    let text = match KDE_REVERT_RESULT.swap(KDE_REVERT_NONE, Ordering::SeqCst) {
        KDE_REVERT_OK => {
            rexon_core::log_info!("kde-plugin", "прежние обои столов возвращены");
            wkp::revert_done_text()
        }
        KDE_REVERT_FAILED => {
            rexon_core::log_warn!("kde-plugin", "прежние обои столов вернуть не вышло: Plasma не ответила");
            // Здесь человеку есть что СДЕЛАТЬ самому (вернуть обои руками) —
            // это тост ошибки, он держится до закрытия, как у вендора.
            crate::we_singletons::client_error(&wkp::revert_failed_text());
            return;
        }
        _ => return,
    };
    // Обои уже возвращены, делать ничего не надо — сообщение
    // информационное и гаснет само (дефект 30.10.2026: тост ошибки висел
    // больше минуты и закрывал кнопки панели, в том числе фильтр).
    broadcast(&DaemonEvent::Notice(text));
}

/// Снять придержку целиком (кадры выбрасываются).
fn drop_kde_hold() {
    APPLY_WATCH.with(|w| *w.borrow_mut() = None);
    KDE_REVERT_WANTED.store(false, Ordering::SeqCst);
    HELD.with(|h| h.borrow_mut().clear());
    HELD_SINCE.with(|c| c.set(None));
    KDE_SELECT_STATE.store(KDE_SELECT_IDLE, Ordering::SeqCst);
    qobject::sock_timer_stop(TIMER_KDE_HOLD);
}

/// Отправить готовый кадр `Request`. `false` — сокет закрыт.
pub fn send_frame(frame: &[u8]) -> bool {
    let socket = socket_ptr();
    if socket.is_null() || !unsafe { qobject::sock_is_open(socket) } {
        return false;
    }
    let mut sock = unsafe { Pin::new_unchecked(&mut *socket) };
    let payload = QByteArray::from(frame);
    let sent = i64::from(sock.as_mut().send_binary_message(&payload));
    sent > 0
}

// ── DaemonControl: поиск и автозапуск демона ────────────────────────────────

/// Живой транспорт трейта `WaywallenIo` ядра поверх `Properties.Get`.
struct DbusProps;

impl rexon_core::waywallen_client::WaywallenIo for DbusProps {
    fn is_valid(&mut self) -> bool {
        true
    }

    fn get_property(&mut self, name: &str) -> DbusValue {
        let mut ok = false;
        let value = qobject::sock_daemon_property(
            &QString::from(we_client::DBUS_SERVICE),
            &QString::from(we_client::DBUS_PATH),
            &QString::from(we_client::DBUS_IFACE),
            &QString::from(name),
            i32::try_from(we_client::PROPERTY_TIMEOUT_MS).unwrap_or(1000),
            &mut ok,
        );
        if !ok {
            return DbusValue::None;
        }
        DbusValue::Str(value.to_string())
    }

    fn call(
        &mut self,
        _method: &str,
        _args: &[DbusValue],
    ) -> rexon_core::waywallen_client::DbusReply {
        // Методов шины транспорт не зовёт: всё, что ему нужно, — три
        // свойства. Мёртвый `ListTasks` удалён из продукта 18.08.
        rexon_core::waywallen_client::DbusReply {
            error_name: "org.freedesktop.DBus.Error.NotSupported".to_owned(),
            error_message: "транспорт не вызывает методов шины".to_owned(),
            arguments: Vec::new(),
        }
    }
}

pub fn probe() -> we_client::DaemonStatus {
    we_client::probe(&mut DbusProps)
}

/// Поиск бинаря демона: порядок кандидатов решает ядро, живые проверки ФС —
/// общая на крейт `we_env::FsProbe` (второй копии нет, правило #12).
pub fn find_daemon_binary() -> Option<String> {
    crate::we_env::daemon_binary()
}

/// Порт `DaemonControl.ensure_running`: демон жив → `ready` сразу; нет →
/// автозапуск и поллинг шины. Решения целиком у ядра.
pub fn ensure_running() {
    // Единственная точка, через которую проходит КАЖДОЕ открытие вкладки WE, —
    // здесь же догоняем починки шаблона автозапуска у тех, кто включил его
    // давно и переключатель больше не трогал.
    crate::autostart_queries::refresh_autostart_unit();
    install_shim_hooks();

    let st = probe();
    let binary = find_daemon_binary();
    // Плагины из пакета (Open Wallpaper Engine) демон, чей файл живёт не
    // под тем же префиксом, видит только по флагу `--plugin`.
    let plugin_root = crate::we_env::engine_plugin_root_for(binary.as_deref());
    // Включён автозапуск — демон поднимается через службу, а не вторым
    // процессом мимо неё: демон один и под присмотром службы.
    let service = !st.running && crate::autostart_queries::autostart_service_present();
    let decision = we_client::ensure_running_decision_service(
        &st,
        binary.as_deref(),
        plugin_root.as_deref(),
        service,
    );

    if decision.ensure_kde_plugin() {
        // Best-effort подготовка показа (на KDE — Plasma-плагин обоев).
        // Версия живого демона нужна сторожу совместимости плагина; демона
        // ещё нет — подготовка спросит `--version` у того бинаря, что будет
        // запущен.
        let daemon_version = (st.running && !st.version.trim().is_empty())
            .then(|| st.version.trim().to_owned());
        // ⛔ Только установка: открытие вкладки обоев рабочего стола НЕ
        // меняет (дефект 30.10.2026 — чёрный стол при пустой библиотеке).
        // Плагин назначается обоями при применении — `send_request_frame`.
        prepare_display_backend(daemon_version, binary.clone(), false, KdePurpose::Prepare);
    }

    match decision {
        we_client::EnsureAction::Ready { status } => daemon_ready(&status),
        we_client::EnsureAction::NoBinary { message } => daemon_failed(&message),
        we_client::EnsureAction::Launch {
            program,
            args,
            polls_left: _,
            poll_interval_ms,
        } => {
            // Запуск уже идёт (повторное нажатие, вторая вкладка) — второй
            // процесс не нужен: сторож первого доведёт дело до конца.
            if START.with(|s| s.borrow().is_some()) {
                broadcast(&DaemonEvent::Starting { slow: false });
                return;
            }
            // Отдельной сессией: демон должен пережить закрытие Rexon — обои
            // не гаснут вместе с окном.
            let Some(child) = spawn_engine(&program, &args) else {
                daemon_failed(&we_client::daemon_launch_failed_message(&program));
                return;
            };
            // Под службой запущен `systemctl`, а не движок: о движке спрашиваем
            // службу. Иначе судим по PID самого процесса.
            let kind = if service { StartKind::Service } else { StartKind::Process(child.id()) };
            let watch = we_client::StartWatch::new(matches!(kind, StartKind::Process(p) if p != 0));
            START.with(|s| {
                *s.borrow_mut() = Some(StartRun {
                    watch,
                    kind,
                    slow: false,
                    step_ms: poll_interval_ms,
                    child: Some(child),
                })
            });
            broadcast(&DaemonEvent::Starting { slow: false });
            qobject::sock_timer_start(
                TIMER_POLL,
                i32::try_from(poll_interval_ms).unwrap_or(500),
                false,
            );
        }
    }
}

/// Чем запущен движок — от этого зависит, у кого спрашивать признаки жизни.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StartKind {
    /// Отдельный процесс с этим PID (0 — PID неизвестен).
    Process(u32),
    /// Служба автозапуска (`systemctl --user start`).
    Service,
}

/// Идущий запуск демона.
struct StartRun {
    watch: we_client::StartWatch,
    kind: StartKind,
    /// Уже сказали человеку, что запуск долгий.
    slow: bool,
    step_ms: u64,
    /// Запущенный процесс (движок или `systemctl`): по нему узнаётся код
    /// выхода. После запуска его подбирает поток-сборщик (`reap_engine`).
    child: Option<std::process::Child>,
}

/// Запустить движок своим потомком в отдельной сессии.
///
/// Прежде шёл `QProcess::startDetached`: код выхода у чужого потомка не
/// узнать, а stderr демона наследовал дескриптор 2 окна — rexon.log — и
/// смешивался с нашими строками. Теперь вывод — в свой файл рядом с
/// журналом (`we_client::DAEMON_OUTPUT_LOG`, заново на каждый запуск), а код
/// выхода видит сторож запуска. Закрытие окна демон по-прежнему переживает:
/// своя сессия (`setsid`), сигналов при выходе родителя он не получает.
fn spawn_engine(program: &str, args: &[String]) -> Option<std::process::Child> {
    use std::os::unix::process::CommandExt;
    use std::process::Stdio;
    let out_path = engine_output_path();
    let (stdout, stderr) = match std::fs::File::create(&out_path) {
        Ok(f) => match f.try_clone() {
            Ok(c) => (Stdio::from(f), Stdio::from(c)),
            Err(_) => (Stdio::null(), Stdio::from(f)),
        },
        Err(e) => {
            rexon_core::log_warn!("we", "файл вывода движка {out_path} не создан: {e}");
            (Stdio::null(), Stdio::null())
        }
    };
    let mut cmd = std::process::Command::new(program);
    cmd.args(args).stdin(Stdio::null()).stdout(stdout).stderr(stderr);
    // SAFETY: в дочернем процессе до exec — только setsid (async-signal-safe).
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    match cmd.spawn() {
        Ok(child) => {
            rexon_core::log_info!("we", "движок запущен: pid {} ({program} {}), вывод — {out_path}", child.id(), args.join(" "));
            Some(child)
        }
        Err(e) => {
            rexon_core::log_warn!("we", "движок не запустился ({program}): {e}");
            None
        }
    }
}

/// Файл вывода движка — рядом с журналом Rexon.
fn engine_output_path() -> String {
    let log = rexon_core::applog::log_path();
    let dir = log.parent().map(|d| d.to_string_lossy().into_owned()).unwrap_or_else(|| ".".into());
    let _ = std::fs::create_dir_all(&dir);
    we_client::daemon_output_log_path(&dir)
}

/// Подобрать процесс после запуска в отдельном потоке: без этого умерший
/// демон висел бы зомби, пока открыто окно. Код выхода — строкой в журнал.
fn reap_engine(mut child: std::process::Child) {
    let spawned = std::thread::Builder::new().name("we-reap".into()).spawn(move || {
        let pid = child.id();
        if let Ok(status) = child.wait() {
            rexon_core::log_info!("we", "процесс движка pid {pid} завершился: {status}");
        }
    });
    if let Err(e) = spawned {
        rexon_core::log_warn!("we", "поток-сборщик движка не запущен: {e}");
    }
}

/// Строки журнала об отказе запуска (`we_client::daemon_exit_report`):
/// код выхода, кто держит имя шины, хвост вывода демона.
fn log_engine_exit(child: Option<&mut std::process::Child>, kind: StartKind) {
    use std::os::unix::process::ExitStatusExt;
    let StartKind::Process(pid) = kind else {
        // Под службой — `journalctl --user -u <юнит>`; путь к нему в
        // журнале уже назван строкой отказа.
        return;
    };
    let exit = match child.map(|c| c.try_wait()) {
        Some(Ok(Some(st))) => match (st.code(), st.signal()) {
            (Some(c), _) => we_client::EngineExit::Code(c),
            (None, Some(s)) => we_client::EngineExit::Signal(s),
            _ => we_client::EngineExit::Unknown,
        },
        _ => we_client::EngineExit::Unknown,
    };
    let owner = run(&we_client::daemon_name_owner_pid_argv(), we_client::BUSCTL_TIMEOUT_SECS as f64).map(|out| {
        match we_client::parse_name_owner_pid(&out.stdout) {
            Some(p) => {
                let cmdline = std::fs::read(format!("/proc/{p}/cmdline"))
                    .map(|b| String::from_utf8_lossy(&b).replace('\0', " ").trim().to_string())
                    .unwrap_or_default();
                we_client::BusNameOwner::Process { pid: p, cmdline }
            }
            None => we_client::BusNameOwner::Free,
        }
    });
    let path = engine_output_path();
    let text = std::fs::read(&path).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
    let tail = we_client::text_tail(&text, we_client::DAEMON_OUTPUT_TAIL_LINES);
    for line in we_client::daemon_exit_report(pid, exit, owner.as_ref(), &path, &tail) {
        rexon_core::log_warn!("we", "{line}");
    }
}

/// Признаки жизни запускаемого движка. Решения тут нет — только сбор
/// снимка для ядра.
fn engine_life(kind: StartKind) -> Option<we_client::EngineLife> {
    match kind {
        StartKind::Process(0) => None,
        StartKind::Process(pid) => we_client::read_process_life(pid),
        StartKind::Service => {
            let out = run(&we_client::engine_unit_show_argv(), we_client::BUSCTL_TIMEOUT_SECS as f64)?;
            let entries = we_client::read_proc_entries();
            we_client::service_life(&out.stdout, &entries)
        }
    }
}

/// Пускатель демона для вендорных страниц (их кнопка запуска зовёт
/// `rexon_we_launch_daemon` шима синглтонов).
///
/// Один на оба процесса. У питоновского хозяина пускатель создавал НОВЫЙ
/// `DaemonControl`, и тот забирал себе колбэки потока — после нажатия окно
/// вкладки переставало узнавать «демон готов / не поднялся». Здесь
/// колбэки не переставляются: события уходят тем, кто уже подписан.
pub extern "C" fn launch_daemon_hook() -> bool {
    if find_daemon_binary().is_none() {
        return false;
    }
    ensure_running();
    true
}

/// Тик сторожа запуска: шина, признаки жизни процесса — решение ядра.
fn poll_once() {
    let Some((kind, step_ms)) = START.with(|s| s.borrow().as_ref().map(|r| (r.kind, r.step_ms))) else {
        qobject::sock_timer_stop(TIMER_POLL);
        return;
    };
    let st = probe();
    // Пока шины нет, спрашиваем процесс; поднялась — спрашивать незачем.
    let life = if st.running && st.ws_port != 0 { None } else { engine_life(kind) };
    let step = START.with(|s| {
        s.borrow_mut().as_mut().map(|r| r.watch.tick(&st, life, step_ms))
    });
    match step {
        Some(we_client::StartStep::Ready { status }) => {
            finish_start();
            daemon_ready(&status);
        }
        Some(we_client::StartStep::Failed { message }) => {
            let waited = START.with(|s| s.borrow().as_ref().map_or(0, |r| r.watch.elapsed_ms()));
            rexon_core::log_warn!("we", "движок не запустился за {waited} мс: {message}");
            START.with(|s| {
                if let Some(run) = s.borrow_mut().as_mut() {
                    log_engine_exit(run.child.as_mut(), run.kind);
                }
            });
            finish_start();
            daemon_failed(&message);
        }
        Some(we_client::StartStep::Wait { slow }) => {
            let newly_slow = START.with(|s| {
                let mut s = s.borrow_mut();
                let run = s.as_mut()?;
                let newly = slow && !run.slow;
                run.slow |= slow;
                Some(newly)
            });
            if newly_slow == Some(true) {
                rexon_core::log_info!(
                    "we",
                    "движок запускается дольше обычного, процесс жив — ждём ({kind:?})"
                );
                broadcast(&DaemonEvent::Starting { slow: true });
            }
        }
        None => qobject::sock_timer_stop(TIMER_POLL),
    }
}

/// Запуск кончился (успехом или отказом): сторож снят, таймер стоит,
/// процесс передан сборщику.
fn finish_start() {
    qobject::sock_timer_stop(TIMER_POLL);
    if let Some(child) = START.with(|s| s.borrow_mut().take()).and_then(|r| r.child) {
        reap_engine(child);
    }
}

/// Одна попытка реконнекта: порт перечитывается по шине каждый раз — демон
/// после перезапуска слушает ДРУГОЙ порт.
fn reconnect_once() {
    if is_open() {
        qobject::sock_timer_stop(TIMER_RECONNECT);
        return;
    }
    // Демон ушёл по нашей просьбе ради плагина из пакета — поднять его
    // обычным путём (служба или запуск с `--plugin`), а не ждать, пока
    // поднимет кто-то другой.
    if PLUGIN_RELAUNCH.load(Ordering::SeqCst) {
        plugin_relaunch();
        return;
    }
    let st = probe();
    if st.running && st.ws_port != 0 {
        open(i32::try_from(st.ws_port).unwrap_or(0));
    }
}

/// Тик таймеров шима. Один вход на все таймеры: своего смысла у номера в
/// C++ нет, разбирает его Rust.
extern "C" fn on_timer_tick(id: i32) {
    match id {
        TIMER_POLL => poll_once(),
        TIMER_RECONNECT => reconnect_once(),
        TIMER_HEALTH => health_timed_out(),
        TIMER_START => ensure_running(),
        TIMER_PLUGIN_RELAUNCH => plugin_relaunch(),
        TIMER_KDE_HOLD => check_kde_hold(),
        TIMER_KDE_REVERT => kde_revert_finished(),
        TIMER_DESKTOP_ICONS => desktop_icons_prepared(),
        _ => {}
    }
}

/// Ошибка сокета из шима (подписка на `errorOccurred`).
extern "C" fn on_socket_error(text: *const c_char, len: i32) {
    if text.is_null() || len < 0 {
        return;
    }
    let bytes = unsafe { std::slice::from_raw_parts(text.cast::<u8>(), len as usize) };
    report_error(bytes);
}

/// Ошибка транспорта — слоту `ON_ERROR` (проба) и тостом окна вкладки через
/// `App.errorOccurred`, как у оригинала.
///
/// Тост — не на каждую ошибку: пока демона нет, каждая попытка
/// переподключения даёт ту же ошибку сокета, и решение «показывать ли»
/// принимает ядро (`we_client::TransportErrorToast`). Проба получает всё.
fn report_error(text: &[u8]) {
    call_text(&ON_ERROR, text);
    let text = String::from_utf8_lossy(text);
    if ERROR_TOAST.with(|t| t.borrow_mut().on_error(&text)) {
        crate::we_singletons::client_error(&text);
    }
}

// ── подписчики на Rust ──────────────────────────────────────────────────────
//
// Шесть швов, которые раньше держало питоновское окно вкладки
// (`qml_window.py`: демон готов / не поднялся, сокет открыт / оборван, ответ
// `health` / его нет). Решений в них нет: вердикт по `health` выносит ядро
// (`we_window::health_verdict`), слова экранов — тоже ядро. Здесь только
// рассылка, а принимает её объект действий страницы (`we_actions.rs`).

/// Событие демона и сокета.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DaemonEvent {
    /// Демон жив, порт сокета известен.
    Ready(we_client::DaemonStatus),
    /// Демон не поднялся; текст — сырая причина от ядра.
    Failed(String),
    /// Сокет открыт (это ещё не «движок рабочий» — см. `Health`).
    Connected,
    /// Сокет оборван. `was_connected` — соединение хоть раз было, то есть
    /// это перезапуск движка, а не неудачный первый заход.
    Disconnected { was_connected: bool },
    /// Ответ `health`: поле `state` (пусто — старый демон его не шлёт).
    Health(String),
    /// Демон подключился и молчит на `health` дольше срока ядра.
    HealthTimeout,
    /// Сообщение человеку от транспорта (уже переведённое), показывается
    /// ИНФОРМАЦИОННЫМ тостом вкладки, который гаснет сам: например, «живые
    /// обои не запустились — прежние обои стола возвращены».
    Notice(String),
    /// Движок запущен и стартует (сторож запуска ещё ждёт шину). `slow` —
    /// обычный срок старта позади, а процесс жив и работает: экран ожидания
    /// говорит, что первый запуск бывает долгим.
    Starting { slow: bool },
}

/// Подписаться на события демона. Возвращает номер для `unsubscribe`.
///
/// Колбэк зовётся СИНХРОННО в потоке транспорта (главном); что с событием
/// делать дальше — ставить ли его в очередь объекта — решает подписчик.
pub fn subscribe(callback: impl Fn(&DaemonEvent) + 'static) -> u64 {
    let id = NEXT_SUBSCRIBER.with(|c| {
        let next = c.get() + 1;
        c.set(next);
        next
    });
    SUBSCRIBERS.with(|s| s.borrow_mut().push((id, Rc::new(callback))));
    id
}

/// Снять подписку. Идемпотентно: `0` — подписки не было.
pub fn unsubscribe(id: u64) {
    if id == 0 {
        return;
    }
    SUBSCRIBERS.with(|s| s.borrow_mut().retain(|(entry, _)| *entry != id));
}

/// Разослать событие всем подписчикам на Rust.
///
/// Список снимается копией ДО вызовов: подписчик вправе тут же отписаться
/// или завести нового, а при живом заимствовании это была бы паника RefCell.
fn broadcast(event: &DaemonEvent) {
    let list: Vec<Rc<dyn Fn(&DaemonEvent)>> =
        SUBSCRIBERS.with(|s| s.borrow().iter().map(|(_, f)| Rc::clone(f)).collect());
    for callback in list {
        callback(event);
    }
}

fn daemon_ready(status: &we_client::DaemonStatus) {
    broadcast(&DaemonEvent::Ready(status.clone()));
}

fn daemon_failed(message: &str) {
    broadcast(&DaemonEvent::Failed(message.to_owned()));
}

/// Тики таймеров и ошибки сокета шим отдаёт в Rust: разбирает их состояние
/// транспорта, а не хозяин вкладки. Ставятся при любом входе в транспорт,
/// а не только из `set_hooks` пробы: без этого у окна поллинг демона и
/// реконнект молча не тикали бы никогда.
fn install_shim_hooks() {
    unsafe {
        rexon_we_sock_set_timer_tick(Some(on_timer_tick));
        rexon_we_sock_set_error_hook(Some(on_socket_error));
    }
}

/// Поднять демона через `delay_ms` (0 — на следующем такте цикла событий).
///
/// Отложено, как у питоновского окна: детект и автозапуск ходят в D-Bus и
/// в файловую систему, а вкладка обязана успеть показаться раньше.
pub fn ensure_running_later(delay_ms: i32) {
    install_shim_hooks();
    qobject::sock_timer_start(TIMER_START, delay_ms.max(0), true);
}

/// Спросить у подключившегося демона `health`; сторож — `timeout_ms`.
///
/// `false` — сокет закрыт, спрашивать некого. Ответ приходит событием
/// `Health`, молчание — `HealthTimeout`.
pub fn request_health(timeout_ms: i32) -> bool {
    install_shim_hooks();
    if !is_open() {
        return false;
    }
    let request_id = alloc_request_id() as u64;
    HEALTH_ID.with(|c| c.set(request_id));
    if !send_frame(&we_client::health(request_id)) {
        HEALTH_ID.with(|c| c.set(0));
        return false;
    }
    // Нулевой срок сторожа не заводит: иначе он сработал бы на первом же
    // тике и объявил здоровый движок мёртвым.
    if timeout_ms > 0 {
        qobject::sock_timer_start(TIMER_HEALTH, timeout_ms, true);
    }
    true
}

// ── сверка плагинов живого демона с пакетом ─────────────────────────────────
//
// Демон выбирает копию плагина (пакет или человек) ОДИН раз — при старте.
// Запущенный до обновления пакета, он держит старую копию, пока его не
// перезапустят (разбор — `we_client::plugin_refresh_decision`). Сверка идёт
// двумя путями, решение и перезапуск у них общие:
//   * после каждого ответа `health` вкладки — `plugin_list` по её сокету;
//   * сразу после обновления пакета — сторож лотка (`spawn_package_watch`)
//     слышит запись в каталог движка и спрашивает `plugin_list` разовым
//     сокетом из своего потока.
// Отстал — перезапуск, один раз на расхождение.

/// Ключ расхождения, ради которого уже перезапускали в этом процессе.
static PLUGIN_REFRESH_TRIED: Mutex<Option<String>> = Mutex::new(None);
/// Демон ушёл по `Quit` ради плагина — его поднимет тик
/// [`TIMER_PLUGIN_RELAUNCH`] (или тик реконнекта вкладки, кто раньше).
static PLUGIN_RELAUNCH: AtomicBool = AtomicBool::new(false);

/// Спросить `plugin_list`, если есть с чем сверять (Rexon из пакета).
fn request_plugin_check() {
    if crate::we_env::package_engine().is_none() || !is_open() {
        return;
    }
    let request_id = alloc_request_id() as u64;
    PLUGINS_ID.with(|c| c.set(request_id));
    if !send_frame(&we_client::plugin_list(request_id)) {
        PLUGINS_ID.with(|c| c.set(0));
    }
}

fn on_plugin_list(frame: &[u8]) {
    let Some(active) = we_client::active_plugins_from_frame(frame) else {
        return;
    };
    if let Some((key, stale)) = plugin_refresh_step(&active) {
        std::thread::spawn(move || {
            restart_for_plugins(key, stale);
        });
    }
}

/// Общая часть обоих путей: сверить активные копии с пакетом и решить.
/// `Some` — перезапускать (ключ уже запомнен); `None` — не надо или уже
/// пробовали.
fn plugin_refresh_step(active: &[we_client::PluginCopy]) -> Option<(String, Vec<we_client::StalePlugin>)> {
    let bundled = crate::we_env::bundled_plugins();
    let tried = PLUGIN_REFRESH_TRIED.lock().map(|g| g.clone()).unwrap_or(None);
    match we_client::plugin_refresh_decision(&bundled, active, tried.as_deref()) {
        we_client::PluginRefresh::Fine => None,
        we_client::PluginRefresh::GiveUp { stale } => {
            rexon_core::log_warn!(
                "we",
                "демон после перезапуска всё равно не взял плагины из пакета ({}); \
                 его, вероятно, запускает не Rexon и без --plugin",
                we_client::stale_plugins_key(&stale)
            );
            None
        }
        we_client::PluginRefresh::Restart { key, stale } => {
            if let Ok(mut g) = PLUGIN_REFRESH_TRIED.lock() {
                *g = Some(key.clone());
            }
            Some((key, stale))
        }
    }
}

/// Чем кончилась попытка перезапуска.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RestartOutcome {
    Restarted,
    /// Демон занят задачами — не трогали, ключ попытки снят.
    Busy,
    Failed,
}

/// Перезапустить демона ради плагинов пакета; удалось — уведомление.
fn restart_for_plugins(key: String, stale: Vec<we_client::StalePlugin>) -> RestartOutcome {
    let outcome = restart_daemon(&key, &PLUGIN_REFRESH_TRIED, "ради плагинов из пакета");
    if outcome == RestartOutcome::Restarted {
        let kind = crate::we_env::package_engine().map_or(we_client::EngineKind::Bundled, |l| l.kind);
        rexon_core::utils::notify_live(
            &we_client::plugin_refresh_title(),
            &we_client::plugin_refresh_body_for(kind, &stale),
        );
    }
    outcome
}

/// Ключ расхождения версий движка, ради которого уже перезапускали.
static ENGINE_REFRESH_TRIED: Mutex<Option<String>> = Mutex::new(None);

/// Сверить живого демона (`live` — его `Version`) с движком из пакета
/// (вшитым или пакета дистрибутива). `Some((ключ, версия на диске, чей
/// движок))` — перезапускать (ключ уже запомнен).
fn engine_refresh_step(live: &str) -> Option<(String, String, we_client::EngineKind)> {
    use rexon_core::we_client::DaemonFs;
    let layout = crate::we_env::daemon_binary()
        .and_then(|b| we_client::layout_of(&we_client::engine_layouts(), &b).cloned());
    let runs_bundled = layout.is_some();
    let kind = layout.as_ref().map_or(we_client::EngineKind::Bundled, |l| l.kind);
    let on_disk = match &layout {
        Some(l) => crate::we_env::FsProbe.binary_version(&l.daemon).unwrap_or_default(),
        None => String::new(),
    };
    let tried = ENGINE_REFRESH_TRIED.lock().map(|g| g.clone()).unwrap_or(None);
    match we_client::engine_refresh_decision(runs_bundled, live, &on_disk, tried.as_deref()) {
        we_client::EngineRefresh::Fine => None,
        we_client::EngineRefresh::GiveUp => {
            rexon_core::log_warn!(
                "we",
                "демон после перезапуска всё равно рапортует {live}, а движок комплекта — {on_disk}; \
                 его, вероятно, запускает не Rexon"
            );
            None
        }
        we_client::EngineRefresh::Restart { key } => {
            if let Ok(mut g) = ENGINE_REFRESH_TRIED.lock() {
                *g = Some(key.clone());
            }
            Some((key, on_disk, kind))
        }
    }
}

/// Перезапустить демона ради нового движка из пакета; удалось — уведомление.
fn restart_for_engine(key: String, live: &str, on_disk: &str, kind: we_client::EngineKind) -> RestartOutcome {
    let outcome = restart_daemon(&key, &ENGINE_REFRESH_TRIED, "ради нового движка из пакета");
    if outcome == RestartOutcome::Restarted {
        rexon_core::utils::notify_live(
            &we_client::plugin_refresh_title(),
            &we_client::engine_refresh_body_for(kind, live, on_disk),
        );
    }
    outcome
}

/// Перезапустить демона (`why` — для журнала). Фоновый поток: `systemctl` и
/// ожидание ухода демона главный поток не держат. ⛔ Никаких `kill` —
/// служба (`systemctl --user restart`) или его же `Quit` по шине. Демон
/// занят — ключ попытки в `tried` снимается, чтобы сверка повторилась.
fn restart_daemon(key: &str, tried: &Mutex<Option<String>>, why: &str) -> RestartOutcome {
    let secs = we_client::SYSTEMCTL_TIMEOUT_SECS as f64;
    // Идёт скачивание или сканирование — не рвать; попробуем при следующем
    // подключении (или повтором сторожа пакета).
    let tasks = run(&we_client::busctl_tasks_argv(), we_client::BUSCTL_TIMEOUT_SECS as f64)
        .map(|out| we_client::parse_busctl_tasks(&out.stdout))
        .unwrap_or_default();
    if we_client::daemon_busy(&tasks) {
        if let Ok(mut g) = tried.lock() {
            *g = None;
        }
        rexon_core::log_info!("we", "демон занят задачами — перезапуск {why} отложен ({key})");
        return RestartOutcome::Busy;
    }
    let eu = rexon_core::engine_update::engine_unit_active_argv();
    let unit_active = run(&eu, secs).is_some_and(|out| out.rc == 0);
    let ok = if unit_active {
        let argv = we_client::systemctl_user_argv(&["restart", we_client::AUTOSTART_UNIT]);
        run(&argv, secs).is_some_and(|out| out.rc == 0)
    } else {
        let quit = rexon_core::engine_update::busctl_quit_argv();
        let asked = run(&quit, rexon_core::engine_update::BUSCTL_TIMEOUT_SECS as f64)
            .is_some_and(|out| out.rc == 0);
        let mut gone = false;
        if asked {
            for _ in 0..rexon_core::engine_update::QUIT_WAIT_STEPS {
                // Шина спрашивается `busctl`, а не QtDBus транспорта: тот
                // живёт в главном потоке.
                let on_bus = run(
                    &rexon_core::engine_update::busctl_version_argv(),
                    rexon_core::engine_update::BUSCTL_TIMEOUT_SECS as f64,
                )
                .is_some_and(|out| {
                    !rexon_core::engine_update::parse_busctl_version(&out.stdout).is_empty()
                });
                if !on_bus {
                    gone = true;
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(
                    rexon_core::engine_update::QUIT_WAIT_STEP_MS,
                ));
            }
        }
        if gone {
            // Поднимает главный поток (там живут запуск и поллинг шины).
            // Тик ставится сразу, а не ждёт реконнекта вкладки: у лотка без
            // открытой вкладки реконнекта нет вовсе.
            PLUGIN_RELAUNCH.store(true, Ordering::SeqCst);
            qobject::sock_post_tick(TIMER_PLUGIN_RELAUNCH);
        }
        gone
    };
    if !ok {
        rexon_core::log_warn!("we", "не удалось перезапустить демон {why} ({key})");
        return RestartOutcome::Failed;
    }
    rexon_core::log_info!("we", "демон перезапущен {why} ({key})");
    RestartOutcome::Restarted
}

/// Поднять демона, ушедшего по `Quit` ради плагина, обычным путём (служба
/// или запуск с `--plugin`). Один раз на уход: флаг снимается.
fn plugin_relaunch() {
    if PLUGIN_RELAUNCH.swap(false, Ordering::SeqCst) {
        qobject::sock_timer_stop(TIMER_RECONNECT);
        ensure_running();
    }
}

// ── сторож пакета движка (лоток) ────────────────────────────────────────────

thread_local! {
    /// Разовый `plugin_list` сторожа: свой сокет и свой цикл событий в его
    /// потоке, как у синхронной остановки обоев.
    static PLUGIN_QUERY: RefCell<PluginQuery> = RefCell::new(PluginQuery::default());
}

#[derive(Default)]
struct PluginQuery {
    request_id: u64,
    socket: *mut QWebSocket,
    answer: Option<Vec<we_client::PluginCopy>>,
}

/// Активные плагины демона на порту `port` — разовым сокетом, синхронно, в
/// потоке вызывающего (сокет вкладки живёт в главном потоке, трогать его
/// отсюда нельзя). `None` — не подключились, не ответил или ответ не тот.
fn query_active_plugins(port: i64, timeout_ms: i32) -> Option<Vec<we_client::PluginCopy>> {
    if port <= 0 || !qobject::sock_qt_available() {
        return None;
    }
    let socket = qobject::sock_new();
    if socket.is_null() {
        return None;
    }
    PLUGIN_QUERY.with(|q| *q.borrow_mut() = PluginQuery { socket, ..PluginQuery::default() });
    let mut sock = unsafe { Pin::new_unchecked(&mut *socket) };
    let on_connected = sock.as_mut().on_connected(|_sock| {
        let request_id = alloc_request_id() as u64;
        let socket = PLUGIN_QUERY.with(|q| {
            let mut q = q.borrow_mut();
            q.request_id = request_id;
            q.socket
        });
        if !stop_send(socket, &we_client::plugin_list(request_id)) {
            qobject::sock_quit_loop();
        }
    });
    let on_message = sock.as_mut().on_binary_message_received(|_sock, message| {
        let bytes: Vec<u8> = message.into();
        let Ok(Frame::Response(request_id)) = read_frame(&bytes) else { return };
        if request_id == 0 || request_id != PLUGIN_QUERY.with(|q| q.borrow().request_id) {
            return;
        }
        PLUGIN_QUERY.with(|q| q.borrow_mut().answer = we_client::active_plugins_from_frame(&bytes));
        qobject::sock_quit_loop();
    });
    let on_disconnected = sock.as_mut().on_disconnected(|_sock| qobject::sock_quit_loop());
    sock.as_mut().open_url(&QUrl::from(format!("ws://127.0.0.1:{port}/").as_str()));
    qobject::sock_run_loop(timeout_ms);
    // Стражи снимаются РАНЬШЕ сокета, иначе они дёрнут мёртвый объект.
    drop(on_connected);
    drop(on_message);
    drop(on_disconnected);
    unsafe {
        qobject::sock_close(socket);
        qobject::sock_destroy(socket.cast());
    }
    PLUGIN_QUERY.with(|q| std::mem::take(&mut *q.borrow_mut()).answer)
}

/// Имена папок в `plugins/` пакета — под слежку за их `plugin.toml`.
fn bundled_plugin_dirs(layout: &we_client::EngineLayout) -> Vec<String> {
    std::fs::read_dir(layout.plugins_dir())
        .map(|it| {
            it.flatten()
                .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default()
}

/// Одна сверка сторожа после изменения пакета. `true` — демон занят, сверку
/// стоит повторить позже.
fn package_check() -> bool {
    let st = probe();
    if !st.running || st.ws_port == 0 {
        // Демона нет — сверять нечего: при запуске Rexon сам передаст ему
        // `--plugin`, а чужой запуск поймает сверка вкладки при подключении.
        rexon_core::log_info!("we", "сверка с пакетом движка: демон не запущен — сверять не с чем");
        return false;
    }
    // Сначала сам движок: перезапуск ради него поднимет демона и с
    // плагинами комплекта, отдельная сверка плагинов тогда не нужна.
    if let Some((key, on_disk, kind)) = engine_refresh_step(&st.version) {
        return restart_for_engine(key, &st.version, &on_disk, kind) == RestartOutcome::Busy;
    }
    let Some(active) = query_active_plugins(st.ws_port, we_client::ENGINE_WATCH_QUERY_MS) else {
        rexon_core::log_warn!("we", "сверка с пакетом движка: демон не ответил на plugin_list");
        return false;
    };
    match plugin_refresh_step(&active) {
        Some((key, stale)) => restart_for_plugins(key, stale) == RestartOutcome::Busy,
        None => false,
    }
}

/// Сторож пакета: живёт, пока жив лоток. Ждёт записи в каталог движка,
/// конца пачки и сверяет; слежка переставляется после каждой пачки (могли
/// появиться новые папки плагинов или замениться каталоги целиком).
fn package_watch_loop() {
    // Цикл событий в этом потоке заводится ДО первого сокета: QWebSocket,
    // открытый в потоке без диспетчера событий, не создаст своих
    // уведомителей. Нулевой прогон цикла создаёт диспетчер на весь поток.
    qobject::sock_run_loop(0);
    // Следим за ВСЕМИ раскладками пакетов, а не только стоящей сейчас:
    // движок мог прийти пакетом дистрибутива уже после запуска лотка.
    let targets = || {
        we_client::engine_layouts()
            .iter()
            .flat_map(|l| we_client::engine_watch_targets(l, &bundled_plugin_dirs(l)))
            .collect::<Vec<_>>()
    };
    let Some(mut watch) = rexon_core::fs_watch::Watch::new(targets()) else {
        rexon_core::log_warn!("we", "сторож пакета движка не запущен: inotify недоступен");
        return;
    };
    // Первая сверка — сразу при старте, без записи в пакет: Rexon могли
    // перезапустить уже ПОСЛЕ обновления (кнопкой «Перезапустить Rexon»), а
    // демон-служба так и держит прежний движок.
    let mut first = true;
    loop {
        if !std::mem::take(&mut first) {
            watch.wait_burst(we_client::ENGINE_WATCH_QUIET_MS, we_client::ENGINE_WATCH_MAX_MS);
            // Слежка — заново ДО сверки: запись посреди сверки не потеряется.
            if let Some(fresh) = rexon_core::fs_watch::Watch::new(targets()) {
                watch = fresh;
            }
        }
        rexon_core::log_info!("we", "сверяю движок и плагины живого демона с пакетом");
        let mut retries = 0;
        while package_check() && retries < we_client::ENGINE_WATCH_BUSY_RETRIES {
            retries += 1;
            // Новая пачка записей во время ожидания — сверить после неё.
            if watch.wait(we_client::ENGINE_WATCH_BUSY_RETRY_MS) {
                watch.settle(we_client::ENGINE_WATCH_QUIET_MS, we_client::ENGINE_WATCH_MAX_MS);
            }
        }
    }
}

/// Завести сторожа пакета движка один раз на процесс. Только у Rexon из
/// пакета (есть `plugins/` движка): в дереве разработчика сверять не с чем.
/// Звать из главного потока — здесь ставятся хуки тиков транспорта.
pub fn spawn_package_watch() {
    static STARTED: std::sync::Once = std::sync::Once::new();
    if crate::we_env::package_engine().is_none() {
        return;
    }
    install_shim_hooks();
    STARTED.call_once(|| {
        let spawned = std::thread::Builder::new()
            .name("engine-package-watch".into())
            .spawn(package_watch_loop);
        if let Err(e) = spawned {
            rexon_core::log_warn!("we", "сторож пакета движка не запущен: {e}");
        }
    });
}

fn health_timed_out() {
    if HEALTH_ID.with(|c| c.replace(0)) == 0 {
        return;
    }
    broadcast(&DaemonEvent::HealthTimeout);
}

/// Поле `state` из ответа `health`. Разбор кадра — ядро (`reply_fields`).
///
/// Пустая строка значит и «старый демон поля не шлёт», и «ответ пришёл не о
/// здоровье» — вердикт ядра в обоих случаях один.
fn health_state(frame: &[u8]) -> String {
    match we_client::reply_fields(frame) {
        Ok(f) if f.payload == "health" => f
            .text
            .into_iter()
            .find(|(key, _)| key == "state")
            .map(|(_, value)| value)
            .unwrap_or_default(),
        _ => String::new(),
    }
}

// ── подготовка показа обоев (KDE) ───────────────────────────────────────────
//
// Перенос `DaemonControl._ensure_display_backend` и
// `rexon/waywallen_kde_plugin.ensure_waywallen_plugin`. Решения — все в ядре
// (`wallpaper_backend::pick`, `waywallen_kde_plugin::*`: конвейер
// kpackagetool6, JS выбора плагина, команды рестарта plasmashell, итоговое
// сообщение); здесь только процессы и файлы.
//
// ⭐ Отличие от Python: подготовка идёт в ФОНОВОМ потоке. У Python она шла
// прямо в главном, и первый показ вкладки на KDE мог стоять до 30 с на
// kpackagetool6 и ещё 15 с на plasmashell. Порядок с демоном от этого не
// зависит: демон с `spawn=External` сам ждёт, пока плагин выберут.

/// Подготовка уже идёт — второй заход (кнопка «Повторить» в это время)
/// ничего не добавил бы, кроме двух kpackagetool6 над одним каталогом.
static KDE_PREPARE_BUSY: AtomicBool = AtomicBool::new(false);

/// Что вкладка показывает о плагине обоев KDE. Решение — ядро
/// (`waywallen_kde_plugin::plan_plugin`), здесь только доставка.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct KdePluginState {
    /// Сообщение человеку; `None` — говорить нечего.
    pub notice: Option<rexon_core::waywallen_kde_plugin::PluginNotice>,
    /// Идёт установка по согласию (кнопка «Поставить версию Rexon»).
    pub installing: bool,
    /// Установка по согласию не удалась — сырая причина kpackagetool6.
    pub failure: Option<String>,
}

type KdeSink = std::sync::Arc<dyn Fn(&KdePluginState) + Send + Sync>;

/// Последнее состояние — для страницы, открытой после проверки.
static KDE_STATE: Mutex<Option<KdePluginState>> = Mutex::new(None);
/// Подписчики состояния. В отличие от событий демона, зовутся из ФОНОВОГО
/// потока подготовки — подписчик сам ставит вызов в очередь своего объекта.
static KDE_SINKS: Mutex<Vec<(u64, KdeSink)>> = Mutex::new(Vec::new());
static KDE_NEXT_SINK: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// Версия демона из последней проверки — нужна установке по согласию.
static KDE_DAEMON_VERSION: Mutex<Option<String>> = Mutex::new(None);

/// Подписаться на состояние плагина KDE. Уже известное состояние приходит
/// сразу, в потоке вызова.
pub fn subscribe_kde_plugin(sink: impl Fn(&KdePluginState) + Send + Sync + 'static) -> u64 {
    let id = KDE_NEXT_SINK.fetch_add(1, Ordering::SeqCst) + 1;
    let sink: KdeSink = std::sync::Arc::new(sink);
    if let Ok(mut sinks) = KDE_SINKS.lock() {
        sinks.push((id, sink.clone()));
    }
    let current = KDE_STATE.lock().ok().and_then(|s| s.clone());
    if let Some(state) = current {
        sink(&state);
    }
    id
}

/// Снять подписку; `0` — подписки не было.
pub fn unsubscribe_kde_plugin(id: u64) {
    if id == 0 {
        return;
    }
    if let Ok(mut sinks) = KDE_SINKS.lock() {
        sinks.retain(|(entry, _)| *entry != id);
    }
}

fn publish_kde_plugin(state: KdePluginState) {
    if let Ok(mut slot) = KDE_STATE.lock() {
        *slot = Some(state.clone());
    }
    let sinks: Vec<KdeSink> =
        KDE_SINKS.lock().map(|s| s.iter().map(|(_, f)| f.clone()).collect()).unwrap_or_default();
    for sink in sinks {
        sink(&state);
    }
}

use rexon_core::waywallen_kde_plugin::EnsurePurpose as KdePurpose;

/// Подготовку плагина ведёт один поток за раз: применение обоев, пришедшее
/// посреди установки при открытии вкладки, ждёт её конца, а не пропускается.
static KDE_ENSURE_LOCK: Mutex<()> = Mutex::new(());

/// Кто показывает живые обои в этом сеансе (решение ядра).
fn session_backend() -> rexon_core::wallpaper_backend::WallpaperBackend {
    let env = |name: &str| std::env::var(name).unwrap_or_default();
    rexon_core::wallpaper_backend::pick(
        &env("XDG_CURRENT_DESKTOP"),
        &env("XDG_SESSION_TYPE"),
        &env("WAYLAND_DISPLAY"),
        &env("WAYFIRE_SOCKET"),
    )
}

/// На этом сеансе обои waywallen показывает Plasma-плагин.
pub fn kde_plugin_backend() -> bool {
    session_backend().needs_rexon_setup()
}

/// Как в этом сеансе у демона появляется дисплей (решение ядра).
pub fn display_setup() -> rexon_core::wallpaper_backend::DisplaySetup {
    session_backend().display_setup()
}

// ── X11: хост обоев Rexon — клиент вывода демона ───────────────────────────
//
// Решения и разбор — `rexon_core::wallpaper_backend` (раздел «дисплей для
// демона»). Здесь — исполнение: определение вывода для будущих стартов
// демона и запуск хоста для живого демона сейчас. Перезапускать демон ради
// определения нельзя: обрыв сокета выбросил бы придержанный кадр, и
// «Применить» пропало бы.

/// Хост обоев: рядом с окном Rexon (дерево разработчика), иначе по `PATH`.
fn display_host_binary() -> Option<String> {
    use rexon_core::we_client::DaemonFs;
    let mut fs = crate::we_env::FsProbe;
    std::env::current_exe()
        .ok()
        .and_then(|exe| rexon_core::wallpaper_backend::rexon_display_host_sibling(&exe.to_string_lossy()))
        .filter(|cand| fs.is_exec_file(cand))
        .or_else(|| fs.which(rexon_core::wallpaper_engine::WALLPAPER_HOST_BINARY))
}

/// `$XDG_DATA_HOME` или `~/.local/share` — там демон ищет определения вывода.
fn xdg_data_home() -> String {
    match std::env::var("XDG_DATA_HOME") {
        Ok(v) if !v.is_empty() => v,
        _ => format!("{}/.local/share", crate::we_env::home()),
    }
}

/// Положить демону определение вывода, если его нет или оно устарело.
/// `Err` — текст для журнала.
fn ensure_display_manifest(host: &str) -> Result<bool, String> {
    use rexon_core::wallpaper_backend as wb;
    let path = wb::rexon_display_manifest_path(&xdg_data_home());
    let current = std::fs::read_to_string(&path).ok();
    if !wb::rexon_display_manifest_stale(current.as_deref(), host) {
        return Ok(false);
    }
    let dir = std::path::Path::new(&path).parent().ok_or("у пути определения нет каталога")?;
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let tmp = format!("{path}.tmp");
    std::fs::write(&tmp, wb::rexon_display_manifest(host)).map_err(|e| format!("{tmp}: {e}"))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("{path}: {e}"))?;
    Ok(true)
}

/// Держит ли вывод уже другой экземпляр хоста (его блокировка занята).
fn display_host_running() -> bool {
    let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_default();
    if runtime.is_empty() {
        return false;
    }
    let path = rexon_core::wallpaper_backend::rexon_host_lock_path(&runtime);
    let Ok(file) = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(&path) else {
        return false;
    };
    match file.try_lock() {
        Ok(()) => {
            let _ = file.unlock();
            false
        }
        Err(_) => true,
    }
}

/// Поднять вывод на X11: определение для демона и хост сейчас. Итог — в
/// состоянии придержки (`kde_select_finished`): дисплей появится сам, когда
/// хост пройдёт рукопожатие; не вышло — кадр уйдёт сразу, и демон честно
/// ответит «нет дисплея».
fn start_rexon_display_host() {
    let spawned = std::thread::Builder::new().name("rexon-x11-display".into()).spawn(|| {
        let ok = match display_host_binary() {
            None => {
                rexon_core::log_warn!(
                    "x11-display",
                    "хоста обоев {} нет ни рядом с Rexon, ни в PATH — выводить сцены нечем",
                    rexon_core::wallpaper_engine::WALLPAPER_HOST_BINARY
                );
                false
            }
            Some(host) => {
                match ensure_display_manifest(&host) {
                    Ok(true) => rexon_core::log_info!("x11-display", "движку обоев записано определение вывода (хост {host}) — со следующего старта движок поднимает вывод сам"),
                    Ok(false) => {}
                    Err(e) => rexon_core::log_warn!("x11-display", "определение вывода для движка не записано: {e}"),
                }
                if display_host_running() {
                    rexon_core::log_info!("x11-display", "хост вывода уже работает — ждём его дисплей");
                    true
                } else {
                    spawn_display_host(&host)
                }
            }
        };
        kde_select_finished_without_plugin(ok);
    });
    if let Err(e) = spawned {
        rexon_core::log_warn!("x11-display", "подъём вывода не запущен: {e}");
        kde_select_finished_without_plugin(false);
    }
}

/// Запустить хост отдельной сессией (переживает закрытие окна Rexon; дальше
/// за ним следит блокировка, а со следующего старта — сам демон).
fn spawn_display_host(host: &str) -> bool {
    use std::os::unix::process::CommandExt;
    let argv = rexon_core::wallpaper_backend::rexon_display_host_argv(host, "");
    let mut cmd = std::process::Command::new(&argv[0]);
    cmd.args(&argv[1..])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    // SAFETY: в дочернем процессе до exec — только setsid (async-signal-safe).
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    match cmd.spawn() {
        Ok(mut child) => {
            rexon_core::log_info!("x11-display", "хост вывода запущен: pid {} ({})", child.id(), argv.join(" "));
            // Подобрать код выхода, чтобы не оставлять зомби.
            std::thread::spawn(move || {
                if let Ok(status) = child.wait() {
                    rexon_core::log_info!("x11-display", "хост вывода завершился: {status}");
                }
            });
            true
        }
        Err(e) => {
            rexon_core::log_warn!("x11-display", "хост вывода не запустился ({}): {e}", argv.join(" "));
            false
        }
    }
}

// ── X11: значки рабочего стола под окном вывода ─────────────────────────
//
// Решения — `rexon_core::wallpaper_backend` (раздел «значки рабочего стола
// под живыми обоями»): включить сквозной фон Caja, где он есть, иначе честно
// сказать, что значки на время живых обоев скрыты. Порядок окон держит сам
// хост вывода (`keepX11Order`). gsettings и пересоздание окна стола Caja
// идут секунды — в отдельном потоке; сообщение — тиком в главном.

/// Сообщение о значках, ждущее показа главным потоком.
static DESKTOP_ICONS_NOTICE: Mutex<Option<String>> = Mutex::new(None);
/// Предупреждение «значки скрыты» уже показано в этом запуске — на каждом
/// применении его не повторять.
static DESKTOP_ICONS_WARNED: AtomicBool = AtomicBool::new(false);
/// Подготовка идёт — второе применение подряд второй поток не заводит.
static DESKTOP_ICONS_BUSY: AtomicBool = AtomicBool::new(false);

/// Кто рисует значки в этом сеансе (решение ядра по окружению).
fn session_desktop_icons_owner() -> Option<rexon_core::wallpaper_backend::DesktopIconsOwner> {
    let env = |name: &str| std::env::var(name).unwrap_or_default();
    let session = rexon_core::wallpaper_engine::detect_session_type(
        &env("XDG_SESSION_TYPE"),
        &env("WAYLAND_DISPLAY"),
        &env("DISPLAY"),
    );
    let de = rexon_core::wallpaper_engine::detect_de(&env("XDG_CURRENT_DESKTOP"), &session);
    rexon_core::wallpaper_backend::desktop_icons_owner(&de, &session)
}

fn start_desktop_icons_prepare() {
    let Some(owner) = session_desktop_icons_owner() else {
        return;
    };
    if DESKTOP_ICONS_BUSY.swap(true, Ordering::SeqCst) {
        return;
    }
    let spawned = std::thread::Builder::new().name("rexon-desktop-icons".into()).spawn(move || {
        let mut sys = rexon_core::wallpaper_exec::LiveSys::new();
        let mut io = rexon_core::wallpaper_exec::DesktopIconsSys::new(&mut sys);
        let notice = rexon_core::wallpaper_backend::prepare_desktop_icons(&mut io, owner);
        DESKTOP_ICONS_BUSY.store(false, Ordering::SeqCst);
        if let Some(text) = notice {
            if let Ok(mut slot) = DESKTOP_ICONS_NOTICE.lock() {
                *slot = Some(text);
            }
            qobject::sock_post_tick(TIMER_DESKTOP_ICONS);
        }
    });
    if let Err(e) = spawned {
        DESKTOP_ICONS_BUSY.store(false, Ordering::SeqCst);
        rexon_core::log_warn!("desktop-icons", "подготовка значков стола не запущена: {e}");
    }
}

/// Главный поток: показать сообщение о значках информационным тостом.
fn desktop_icons_prepared() {
    let Some(text) = DESKTOP_ICONS_NOTICE.lock().ok().and_then(|mut slot| slot.take()) else {
        return;
    };
    let owner = session_desktop_icons_owner();
    let hidden = owner.is_some_and(|o| text == rexon_core::wallpaper_backend::desktop_icons_hidden_text(o));
    if hidden && DESKTOP_ICONS_WARNED.swap(true, Ordering::SeqCst) {
        return;
    }
    broadcast(&DaemonEvent::Notice(text));
}

/// Неудачное применение откатывается — вернуть и стол, как был.
fn start_desktop_icons_restore() {
    let spawned = std::thread::Builder::new().name("rexon-desktop-icons".into()).spawn(|| {
        let mut sys = rexon_core::wallpaper_exec::LiveSys::new();
        rexon_core::wallpaper_backend::restore_desktop_icons(&mut rexon_core::wallpaper_exec::DesktopIconsSys::new(&mut sys));
    });
    if let Err(e) = spawned {
        rexon_core::log_warn!("desktop-icons", "возврат стола не запущен: {e}");
    }
}

fn prepare_display_backend(
    daemon_version: Option<String>,
    binary: Option<String>,
    consent: bool,
    purpose: KdePurpose,
) {
    if !kde_plugin_backend() {
        if purpose.selects() {
            kde_select_finished(false, None);
        }
        return;
    }
    // Применение не отбрасывается из-за идущей подготовки: оно встанет в
    // очередь на замке. Отбрасывается только второй заход подготовки.
    let gated = !purpose.selects();
    if gated && KDE_PREPARE_BUSY.swap(true, Ordering::SeqCst) {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("rexon-kde-plugin".into())
        .spawn(move || {
            // Демона ещё нет — версию говорит бинарь, который будет запущен.
            let daemon_version = daemon_version.or_else(|| {
                use rexon_core::we_client::DaemonFs;
                binary.as_deref().and_then(|bin| crate::we_env::FsProbe.binary_version(bin))
            });
            if let Ok(mut slot) = KDE_DAEMON_VERSION.lock() {
                if daemon_version.is_some() {
                    slot.clone_from(&daemon_version);
                }
            }
            let (ok, selected, previous, message) = {
                let _guard = KDE_ENSURE_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                ensure_kde_plugin(&KdeEnv::live(), daemon_version, consent, purpose)
            };
            if purpose.selects() {
                kde_select_finished(selected, previous);
            }
            if ok {
                rexon_core::log_info!("kde-plugin", "плагин обоев waywallen для KDE: {message}");
            } else {
                rexon_core::log_warn!("kde-plugin", "плагин обоев waywallen для KDE (не удалось): {message}");
            }
            if consent {
                finish_consent_install();
            }
            if gated {
                KDE_PREPARE_BUSY.store(false, Ordering::SeqCst);
            }
        });
    if let Err(e) = spawned {
        rexon_core::log_warn!("kde-plugin", "подготовка показа обоев не запущена: {e}");
        if gated {
            KDE_PREPARE_BUSY.store(false, Ordering::SeqCst);
        }
        if consent {
            finish_consent_install();
        }
        if purpose.selects() {
            kde_select_finished(false, None);
        }
    }
}

/// Установка по согласию кончилась любым путём — снять «ставим», чтобы
/// кнопка не осталась выключенной навсегда.
fn finish_consent_install() {
    let current = KDE_STATE.lock().ok().and_then(|s| s.clone());
    if let Some(state) = current.filter(|s| s.installing) {
        publish_kde_plugin(KdePluginState { installing: false, ..state });
    }
}

/// Кнопка «Поставить версию Rexon»: человек согласился заменить плагин.
///
/// `false` — подготовка уже идёт (второй заход ничего бы не добавил).
pub fn install_kde_plugin_with_consent() -> bool {
    if KDE_PREPARE_BUSY.load(Ordering::SeqCst) {
        return false;
    }
    let daemon_version = KDE_DAEMON_VERSION.lock().ok().and_then(|v| v.clone());
    let previous = KDE_STATE.lock().ok().and_then(|s| s.clone()).unwrap_or_default();
    publish_kde_plugin(KdePluginState { installing: true, failure: None, ..previous });
    prepare_display_backend(daemon_version, find_daemon_binary(), true, KdePurpose::Prepare);
    // Подготовка могла не начаться (не KDE, уже идёт) — тогда «ставим» снять.
    if !KDE_PREPARE_BUSY.load(Ordering::SeqCst) {
        finish_consent_install();
    }
    true
}

/// Окружение подготовки плагина. Живое — из процесса; сверки подают своё
/// (поддельные `kpackagetool6`/`dbus-send`/`systemctl` в PATH).
#[derive(Debug, Clone)]
struct KdeEnv {
    /// `XDG_CURRENT_DESKTOP`.
    desktop: String,
    home: String,
    /// Каталог пакета `rexon/` (в нём `kpackage/`).
    pkg_dir: String,
    /// `PATH` для поиска инструментов.
    path: Option<String>,
}

impl KdeEnv {
    fn live() -> KdeEnv {
        KdeEnv {
            desktop: std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default(),
            home: crate::we_env::home(),
            pkg_dir: crate::we_env::pkg_dir(),
            path: std::env::var("PATH").ok(),
        }
    }
}

/// Состояние плагина на диске — всё, что нужно сторожу версии.
fn read_plugin_check(
    bundle: &str,
    install: &str,
    marker: &str,
    daemon_version: Option<String>,
    consent: bool,
) -> rexon_core::waywallen_kde_plugin::PluginCheck {
    use rexon_core::waywallen_kde_plugin as wkp;
    use std::path::Path;

    let sha = |p: &str| std::fs::read(p).ok().map(|bytes| wkp::file_sha(&bytes));
    let vendor_version =
        |base: &str| std::fs::read_to_string(wkp::vendor_toml_path(base)).ok().and_then(|t| wkp::parse_vendor_version(&t));
    let bundle_so = wkp::find_runtime_so(bundle, None);
    let bundle_rel = bundle_so.as_deref().and_then(|so| {
        Path::new(so).strip_prefix(bundle).ok().and_then(|r| r.to_str().map(str::to_owned))
    });
    let present = Path::new(&wkp::metadata_json_path(install)).is_file();
    let installed = if present {
        wkp::InstalledPlugin {
            present,
            vendor_version: vendor_version(install),
            marker: std::fs::read_to_string(marker).is_ok_and(|t| !t.trim().is_empty()),
            runtime_sha: wkp::find_runtime_so(install, bundle_rel.as_deref()).and_then(|so| sha(&so)),
        }
    } else {
        wkp::InstalledPlugin::default()
    };
    wkp::PluginCheck {
        bundle_version: vendor_version(bundle),
        bundle_sha: bundle_so.as_deref().and_then(sha),
        installed,
        daemon_version,
        consent,
    }
}

/// Порт `ensure_waywallen_plugin` со сторожем версии
/// (`waywallen_kde_plugin::plan_plugin`).
///
/// ⛔ Обоями рабочих столов плагин назначается ТОЛЬКО при `purpose ==
/// Apply` (человек применил обои). Прежде это делалось при каждом открытии
/// вкладки — с перезапуском plasmashell, — и у человека с пустой
/// библиотекой стол становился чёрным (дефект 30.10.2026).
///
/// Возвращает (успех, плагин назначен обоями только что, сообщение для
/// журнала).
fn ensure_kde_plugin(
    env: &KdeEnv,
    daemon_version: Option<String>,
    consent: bool,
    purpose: KdePurpose,
) -> (bool, bool, Option<Vec<rexon_core::waywallen_kde_plugin::DesktopPlugin>>, String) {
    use rexon_core::waywallen_kde_plugin as wkp;
    use std::path::Path;

    if !wkp::is_kde(&env.desktop) {
        return (true, false, None, "не KDE — пропускаем (no-op)".to_owned());
    }
    let home = env.home.clone();
    let bundle = wkp::bundle_dir(&env.pkg_dir);
    let install = wkp::install_dir(&home);
    let marker = wkp::version_marker(&home);
    let path_env = env.path.clone();
    let tool = rexon_core::utils::resolve_tool("kpackagetool6", &[], path_env.as_deref());

    let check = read_plugin_check(&bundle, &install, &marker, daemon_version, consent);
    let plan = wkp::plan_plugin(&check);
    let summary = format!(
        "{} (бандл {}, установлен {}, демон {})",
        plan.reason,
        check.bundle_version.as_deref().unwrap_or("?"),
        plan.installed_version.as_deref().unwrap_or(if check.installed.present { "?" } else { "—" }),
        check.daemon_version.as_deref().unwrap_or("?"),
    );

    let outcome = if plan.action.installs() {
        let bundle_valid = Path::new(&bundle).is_dir() && Path::new(&wkp::metadata_json_path(&bundle)).is_file();
        // Маркер «ставили мы»: версия бандла, без неё — хэш рантайма.
        let marker_value = check.bundle_version.clone().or_else(|| check.bundle_sha.clone());
        let mut launch_error: Option<String> = None;
        let outcome = wkp::install_or_upgrade(
            tool.as_deref(),
            &bundle,
            bundle_valid,
            check.installed.present,
            // Решение «ставить» уже принято сторожем — no-op конвейера не нужен.
            None,
            marker_value.as_deref(),
            &mut |argv| match run(argv, wkp::KPACKAGETOOL_TIMEOUT as f64) {
                Some(out) => out,
                None => {
                    launch_error.get_or_insert_with(|| argv.join(" "));
                    wkp::RunOutput { rc: -1, stdout: String::new(), stderr: String::new() }
                }
            },
        );
        let failure = launch_error
            .map(|cmd| format!("kpackagetool6 ошибка запуска: {cmd}"))
            .or_else(|| (!outcome.ok).then(|| outcome.message.clone()));
        if let Some(reason) = failure {
            if consent {
                let previous = KDE_STATE.lock().ok().and_then(|s| s.clone()).unwrap_or_default();
                publish_kde_plugin(KdePluginState {
                    installing: false,
                    failure: Some(reason.clone()),
                    ..previous
                });
            }
            return (false, false, None, format!("{reason}; {summary}"));
        }
        if let Some(version) = &outcome.marker_to_write {
            if let Err(e) = rexon_core::utils::atomic_write(&marker, version) {
                rexon_core::log_warn!("kde-plugin", "не удалось записать метку версии плагина обоев: {e}");
            }
        }
        publish_kde_plugin(KdePluginState::default());
        outcome
    } else {
        publish_kde_plugin(KdePluginState { notice: plan.notice.clone(), installing: false, failure: None });
        wkp::InstallOutcome {
            calls: Vec::new(),
            ok: true,
            message: summary.clone(),
            installed_now: false,
            marker_to_write: None,
        }
    };
    let base = if plan.action.installs() { format!("{}; {summary}", outcome.message) } else { summary };

    // Что сейчас стоит обоями столов. `None` — Plasma не ответила.
    let current = || {
        plasma_evaluate(env, &wkp::current_plugins_js(), 10.0).map(|out| wkp::parse_current_plugins(&out))
    };

    // Перезапуск — только если ОБНОВИЛИ плагин, который уже стоит обоями:
    // старый `.so` в памяти оболочки. Свежую установку оболочка подхватит
    // сама, когда плагин назначат.
    // Спрашиваем Plasma, только когда ответ что-то решает.
    let in_use = outcome.installed_now
        && check.installed.present
        && current().is_some_and(|p| wkp::any_selected(&p));
    let restarted = wkp::restart_after_install(outcome.installed_now, check.installed.present, in_use)
        && restart_plasmashell(env);

    if !purpose.selects() {
        return (
            true,
            false,
            None,
            format!("{}; на десктопах не выбираем до применения обоев", wkp::ensure_message(&base, false, restarted)),
        );
    }
    if !plan.select {
        return (true, false, None, format!("{}; на десктопах не выбираем", outcome.message));
    }

    // Третье значение — «назначили только что»: лишь тогда стоит ждать,
    // пока plasmashell загрузит плагин и тот зарегистрирует дисплей. Стоял
    // плагин уже везде, а дисплея нет — ожидание ничего не даст.
    let mut previous = None;
    let (selected, selected_now) = match current().map(|p| wkp::all_selected(&p)) {
        // Спросить не удалось — и назначать нечем: ответ и назначение идут
        // одним каналом. Молча «успех» не рапортуем.
        None => {
            rexon_core::log_warn!("kde-plugin", "плагин обоев не назначен: Plasma недоступна");
            (false, false)
        }
        Some(all) if wkp::should_reselect(outcome.installed_now, all) => {
            // Снимок «что стояло до нас» — чтобы вернуть, если живые обои
            // не запустятся (`waywallen_kde_plugin::revert_plugins_js`).
            let before = plasma_evaluate(env, &wkp::plugins_by_desktop_js(), 10.0)
                .map(|out| wkp::parse_plugins_by_desktop(&out));
            let ok = plasma_evaluate(env, &wkp::select_plugin_js(), 15.0).is_some();
            if ok {
                previous = before;
            }
            (ok, ok)
        }
        Some(_) => (true, false),
    };
    (true, selected_now, previous, wkp::ensure_message(&base, selected, restarted))
}

/// Запуск команды с таймаутом; `None` — не запустилась или не уложилась.
fn run(argv: &[String], timeout_secs: f64) -> Option<rexon_core::waywallen_kde_plugin::RunOutput> {
    rexon_core::splash_exec::run_capture(argv, timeout_secs).map(|out| {
        rexon_core::waywallen_kde_plugin::RunOutput {
            rc: i32::try_from(out.rc).unwrap_or(-1),
            stdout: out.stdout,
            stderr: out.stderr,
        }
    })
}

/// Порт `rexon.utils.plasma_evaluate_script`: JS в plasmashell по шине.
/// `None` — неудача (нет бинаря, нет службы, ошибка, таймаут), а не пустой
/// успех. Ответ приводится к «виду qdbus» — как у `dbus_call` на Python.
fn plasma_evaluate(env: &KdeEnv, js: &str, timeout_secs: f64) -> Option<String> {
    use rexon_core::utils;
    let path_env = env.path.clone();
    let (kind, exe) = utils::DBUS_TOOL_PREFERENCE.iter().find_map(|(tool, kind)| {
        utils::resolve_tool(tool, &[], path_env.as_deref()).map(|exe| (*kind, exe))
    })?;
    let (dest, path, method) = utils::PLASMA_EVALUATE_SCRIPT;
    let argv = utils::dbus_call_argv(kind, &exe, dest, path, method, &[utils::DbusArg::Str(js.to_owned())]);
    let out = run(&argv, timeout_secs)?;
    if out.rc != 0 {
        return None;
    }
    Some(if kind == "dbus-send" {
        utils::parse_dbus_send_reply(&out.stdout)
    } else {
        out.stdout.trim().to_owned()
    })
}

/// Порт `_restart_plasmashell`: команды из ядра по порядку, успех — код
/// последней.
fn restart_plasmashell(env: &KdeEnv) -> bool {
    let mut last = None;
    for mut call in rexon_core::waywallen_kde_plugin::restart_plasmashell_cmds() {
        // Инструмент ищется по тому же PATH, что и остальные: иначе подмена
        // окружения в сверках не дотянулась бы до перезапуска оболочки.
        if let Some(exe) = rexon_core::utils::resolve_tool(&call.argv[0], &[], env.path.as_deref()) {
            call.argv[0] = exe;
        }
        last = run(&call.argv, call.timeout_secs as f64);
    }
    last.is_some_and(|out| out.rc == 0)
}

// ── синхронная остановка активных обоев ─────────────────────────────────────

thread_local! {
    /// Состояние одного прогона `stop_active_wallpapers`. Отдельно от
    /// основного транспорта: у остановки свой сокет и свой цикл событий, и
    /// зовут её из потока установщика статичных обоев.
    static STOP: RefCell<StopState> = RefCell::new(StopState::default());
}

#[derive(Default)]
struct StopState {
    /// Кого ещё не сняли: `request_id` каждого `renderer_kill` в полёте.
    awaited: usize,
    killed: i64,
    ok: bool,
    list_id: u64,
    /// Сокет прогона — нужен обработчикам, чтобы дослать `renderer_kill`.
    socket: *mut QWebSocket,
}

/// Порт `client.stop_active_wallpapers` — синхронно снять все активные обои.
///
/// Возвращает пару ядра (`stop_active_result`): статус и число снятых.
/// Исключений не бросает ни при каком состоянии: демона нет / нет Qt /
/// обрыв — это статус, а не ошибка (контракт установщика статичных обоев).
pub fn stop_active_wallpapers(timeout_ms: i32) -> (String, i64) {
    let st = probe();
    let gate = we_client::stop_active_gate(&st, qobject::sock_qt_available());
    if let Some((status, killed)) = gate {
        return (status.to_owned(), killed);
    }

    let socket = qobject::sock_new();
    if socket.is_null() {
        let (status, killed) = we_client::stop_active_result(false, 0);
        return (status.to_owned(), killed);
    }
    STOP.with(|s| *s.borrow_mut() = StopState { socket, ..StopState::default() });

    let mut sock = unsafe { Pin::new_unchecked(&mut *socket) };
    let on_connected = sock.as_mut().on_connected(|_sock| stop_on_connected());
    let on_message = sock.as_mut().on_binary_message_received(|_sock, message| {
        let bytes: Vec<u8> = message.into();
        stop_on_frame(&bytes);
    });
    // Обрыв до завершения — выходим: это ветка `("error", 0)` оригинала.
    let on_disconnected = sock.as_mut().on_disconnected(|_sock| qobject::sock_quit_loop());

    sock.as_mut().open_url(&QUrl::from(
        format!("ws://127.0.0.1:{}/", st.ws_port).as_str(),
    ));
    qobject::sock_run_loop(timeout_ms);

    // Стражи снимаются РАНЬШЕ сокета, иначе они дёрнут мёртвый объект.
    drop(on_connected);
    drop(on_message);
    drop(on_disconnected);
    unsafe {
        qobject::sock_close(socket);
        qobject::sock_destroy(socket.cast());
    }

    let (ok, killed) = STOP.with(|s| {
        let st = s.borrow();
        (st.ok, st.killed)
    });
    STOP.with(|s| *s.borrow_mut() = StopState::default());
    let (status, killed) = we_client::stop_active_result(ok, killed);
    (status.to_owned(), killed)
}

/// Крюк ядра `wallpaper_exec::set_we_stopper`: исполнитель обоев зовёт его
/// перед своими обоями (из рабочего потока применения — у снятия свой сокет
/// и свой цикл событий, общий транспорт вкладки оно не трогает).
pub fn stop_we_wallpapers_hook(timeout_ms: u64) -> (String, i64) {
    stop_active_wallpapers(i32::try_from(timeout_ms).unwrap_or(i32::MAX))
}

fn stop_send(socket: *mut QWebSocket, frame: &[u8]) -> bool {
    if socket.is_null() {
        return false;
    }
    let mut sock = unsafe { Pin::new_unchecked(&mut *socket) };
    let payload = QByteArray::from(frame);
    i64::from(sock.as_mut().send_binary_message(&payload)) > 0
}

fn stop_on_connected() {
    let request_id = alloc_request_id() as u64;
    let socket = STOP.with(|s| {
        let mut st = s.borrow_mut();
        st.list_id = request_id;
        st.socket
    });
    stop_send(socket, &we_client::renderer_list(request_id));
}

fn stop_on_frame(bytes: &[u8]) {
    let Ok(frame) = pb::ServerFrame::decode(bytes) else { return };
    let Some(pb::server_frame::Kind::Response(resp)) = frame.kind else {
        return;
    };
    let (list_id, socket) = STOP.with(|s| {
        let st = s.borrow();
        (st.list_id, st.socket)
    });

    if resp.request_id == list_id {
        // Разбор кадра — в ядре (`renderer_ids_from_frame`). `None` = «не наш
        // кадр»: снимать нечего, ведём себя как при пустом списке.
        let ids = we_client::renderer_ids_from_frame(bytes).unwrap_or_default();
        if ids.is_empty() {
            STOP.with(|s| s.borrow_mut().ok = true);
            qobject::sock_quit_loop();
            return;
        }
        STOP.with(|s| s.borrow_mut().awaited = ids.len());
        for rid in ids {
            let kill_id = alloc_request_id() as u64;
            stop_send(socket, &we_client::renderer_kill(kill_id, &rid));
        }
        return;
    }

    // Ответ на renderer_kill: считаем снятых, как это делал `_on_kill_reply`.
    let done = STOP.with(|s| {
        let mut st = s.borrow_mut();
        if st.awaited == 0 {
            return false;
        }
        st.killed += 1;
        if st.killed as usize >= st.awaited {
            st.ok = true;
            return true;
        }
        false
    });
    if done {
        qobject::sock_quit_loop();
    }
}

// ══ колбэки пробы ═══════════════════════════════════════════════════════════

/// Поставить колбэки сокета. Любой из них может быть `None`.
///
/// Нужны одной пробе транспорта (`bin/we_socket_probe.rs`): она считает
/// подключения, кадры и ошибки на живом демоне без окна вкладки. Сама
/// вкладка слушает транспорт подпиской на Rust (`subscribe`), а ответы
/// получает по карте шима (`we_transport.rs`).
pub fn set_hooks(
    connected: VoidHook,
    disconnected: VoidHook,
    error: TextHook,
    response: FrameHook,
    event: TextHook,
) {
    ON_CONNECTED.with(|c| c.set(connected));
    ON_DISCONNECTED.with(|c| c.set(disconnected));
    ON_ERROR.with(|c| c.set(error));
    ON_RESPONSE.with(|c| c.set(response));
    ON_EVENT.with(|c| c.set(event));
    install_shim_hooks();
}

/// Подготовка плагина KDE на поддельном вводе: `kpackagetool6`, `dbus-send`
/// и `systemctl` — скрипты-ловушки, пишущие свои аргументы в журнал.
/// Настоящая Plasma не трогается.
#[cfg(test)]
mod kde_plugin_tests {
    use super::{ensure_kde_plugin, KdeEnv, KdePurpose};
    use rexon_core::waywallen_kde_plugin as wkp;
    use std::path::{Path, PathBuf};

    struct Stand {
        root: PathBuf,
        env: KdeEnv,
    }

    impl Stand {
        /// `plugins` — что «стоит обоями» столов по ответу Plasma;
        /// `installed` — версия уже установленного плагина (`None` — нет).
        fn new(name: &str, plugins: &str, installed: Option<&str>) -> Stand {
            use std::os::unix::fs::PermissionsExt;
            let root = std::env::temp_dir()
                .join(format!("rexon-kde-stand-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            let bin = root.join("bin");
            let home = root.join("home");
            let pkg = root.join("pkg");
            std::fs::create_dir_all(&bin).unwrap();
            std::fs::create_dir_all(&home).unwrap();
            let package = |base: &Path, version: &str, so: &[u8]| {
                let so_dir = base.join("contents/ui/Plugin/WaywallenDisplayEmbed");
                std::fs::create_dir_all(&so_dir).unwrap();
                std::fs::write(base.join("metadata.json"), "{}").unwrap();
                std::fs::write(base.join("vendor.toml"), format!("version = \"{version}\"\n")).unwrap();
                std::fs::write(so_dir.join(wkp::RUNTIME_SO_NAME), so).unwrap();
            };
            package(Path::new(&wkp::bundle_dir(pkg.to_str().unwrap())), "0.4.3", b"bundle");
            if let Some(version) = installed {
                // Та же версия — тот же рантайм: иначе сторож версии решит
                // «освежить» и честно переставит плагин.
                let so: &[u8] = if version == "0.4.3" { b"bundle" } else { b"old" };
                package(Path::new(&wkp::install_dir(home.to_str().unwrap())), version, so);
            }
            let log = root.join("calls.log");
            let script = |tool: &str, body: &str| {
                let path = bin.join(tool);
                std::fs::write(
                    &path,
                    format!("#!/bin/sh\necho \"{tool} $*\" >> '{}'\n{body}\nexit 0\n", log.display()),
                )
                .unwrap();
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            };
            script("kpackagetool6", "");
            script("systemctl", "");
            // Чтение плагинов столов отвечает списком, назначение — пустым
            // ответом, как настоящая Plasma.
            // Снимок по столам (`id=плагин`) — номера столов с единицы.
            let by_desktop: Vec<String> = plugins
                .split(',')
                .filter(|p| !p.is_empty())
                .enumerate()
                .map(|(i, p)| format!("{}={p}", i + 1))
                .collect();
            let by_desktop = by_desktop.join(";");
            script(
                "dbus-send",
                &format!(
                    "case \"$*\" in *out.push*) echo 'method return time=1 sender=:1.2'; \
                     echo '   string \"{plugins}\"';; \
                     *ds\\[i\\].id*) echo 'method return time=1 sender=:1.2'; \
                     echo '   string \"{by_desktop}\"';; esac"
                ),
            );
            let env = KdeEnv {
                desktop: "KDE".to_owned(),
                home: home.to_string_lossy().into_owned(),
                pkg_dir: pkg.to_string_lossy().into_owned(),
                path: Some(bin.to_string_lossy().into_owned()),
            };
            Stand { root, env }
        }

        fn calls(&self) -> String {
            std::fs::read_to_string(self.root.join("calls.log")).unwrap_or_default()
        }

        /// Назначали ли плагин обоями (JS `select_plugin`).
        fn selected(&self) -> bool {
            self.calls().lines().any(|l| l.starts_with("dbus-send") && l.contains("wallpaperPlugin ="))
        }

        fn restarted(&self) -> bool {
            self.calls().lines().any(|l| l.starts_with("systemctl") && l.contains("restart"))
        }
    }

    impl Drop for Stand {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    /// ⛔ Дефект 30.10.2026: первое открытие вкладки при пустой библиотеке
    /// назначало плагин обоями всех столов и перезапускало plasmashell —
    /// стол становился чёрным. Теперь только установка.
    #[test]
    fn tab_open_with_empty_library_keeps_desktop_wallpaper() {
        let stand = Stand::new("open", "org.kde.image", None);
        let (ok, selected_now, previous, message) =
            ensure_kde_plugin(&stand.env, None, false, KdePurpose::Prepare);
        assert!(ok, "{message}");
        assert!(!selected_now);
        assert!(previous.is_none(), "ничего не назначали — и откатывать нечего");
        assert!(
            stand.calls().lines().any(|l| l.starts_with("kpackagetool6") && l.contains(" -i ")),
            "плагин ставится заранее: {}",
            stand.calls()
        );
        assert!(!stand.selected(), "обои стола не меняются: {}", stand.calls());
        assert!(!stand.restarted(), "plasmashell не перезапускается: {}", stand.calls());
        assert!(!message.contains("выбран на десктопах"), "{message}");
    }

    /// Применение обоев из библиотеки назначает плагин — без перезапуска
    /// оболочки при свежей установке.
    #[test]
    fn applying_wallpaper_selects_plugin() {
        let stand = Stand::new("apply", "org.kde.image", None);
        let (ok, selected_now, previous, message) =
            ensure_kde_plugin(&stand.env, None, false, KdePurpose::Apply);
        assert!(ok, "{message}");
        assert!(selected_now, "{message}");
        assert!(stand.selected(), "плагин назначен обоями: {}", stand.calls());
        // Снимок прежних обоев — для отката, если живые обои не запустятся.
        assert_eq!(
            previous,
            Some(vec![rexon_core::waywallen_kde_plugin::DesktopPlugin {
                desktop_id: 1,
                plugin: "org.kde.image".to_owned()
            }])
        );
        assert!(!stand.restarted(), "свежая установка оболочку не трогает: {}", stand.calls());
    }

    /// Плагин уже везде — повторно не назначается, ждать дисплея незачем.
    #[test]
    fn applying_with_plugin_everywhere_does_not_reselect() {
        let stand = Stand::new("everywhere", "org.waywallen.kde,org.waywallen.kde", Some("0.4.3"));
        let (ok, selected_now, previous, message) =
            ensure_kde_plugin(&stand.env, None, false, KdePurpose::Apply);
        assert!(ok, "{message}");
        assert!(!selected_now);
        assert!(previous.is_none(), "плагин стоял до применения — откатывать нечего");
        assert!(!stand.selected(), "{}", stand.calls());
    }

    /// Обновление плагина, который уже стоит обоями, перезапускает оболочку
    /// (старый `.so` в памяти), но обоев не назначает.
    #[test]
    fn upgrading_plugin_in_use_restarts_shell_without_selecting() {
        let stand = Stand::new("upgrade", "org.waywallen.kde,org.kde.image", Some("0.4.0"));
        let (ok, _, _, message) = ensure_kde_plugin(&stand.env, None, false, KdePurpose::Prepare);
        assert!(ok, "{message}");
        assert!(stand.restarted(), "{}", stand.calls());
        assert!(!stand.selected(), "{}", stand.calls());
    }

    /// Обновление невыбранного плагина оболочку не трогает.
    #[test]
    fn upgrading_unused_plugin_does_not_restart_shell() {
        let stand = Stand::new("unused", "org.kde.image", Some("0.4.0"));
        let (ok, _, _, message) = ensure_kde_plugin(&stand.env, None, false, KdePurpose::Prepare);
        assert!(ok, "{message}");
        assert!(!stand.restarted(), "{}", stand.calls());
        assert!(!stand.selected(), "{}", stand.calls());
    }
}

#[cfg(test)]
mod tests {
    use rexon_core::we_query::{self, QueryState, STATUS_ERROR};

    /// Обрыв соединения обязан ЗАКРЫТЬ ожидающий запрос ошибкой,
    /// а не молча забыть о нём.
    ///
    /// Проверяется тот самый кадр, который уходит всем ожидающим при
    /// `disconnected`: он проходит обычный путь разбора ответа и оставляет
    /// запрос в Error с человеческим текстом. Раньше колбэки не звались
    /// никогда, и страница показывала вечный спиннер.
    #[test]
    fn obryv_zakryvaet_ozhidayushchij_zapros_oshibkoj() {
        let frame = super::error_frame(7, super::DISCONNECTED_TEXT);
        let mut st = QueryState::default();
        let signals = we_query::apply_ack(&mut st, &frame).expect("кадр разбирается");

        assert_eq!(st.status, STATUS_ERROR, "запрос обязан уйти в Error");
        assert_eq!(st.error, super::DISCONNECTED_TEXT);
        assert!(
            signals
                .iter()
                .any(|s| matches!(s, we_query::QuerySignal::ErrorOccurred(_))),
            "страница обязана получить errorOccurred, а не остаться в Querying"
        );
    }

    /// Номер запроса в кадре обрыва не важен для маршрутизации (адресата
    /// выбирает карта QPointer), но он обязан быть валидным `Response`.
    #[test]
    fn kadr_obryva_eto_nastoyashchij_otvet_servera() {
        use prost::Message;
        let frame = super::error_frame(42, "тест");
        let decoded = rexon_core::proto::control::ServerFrame::decode(&frame[..])
            .expect("кадр сервера разбирается");
        match decoded.kind {
            Some(rexon_core::proto::control::server_frame::Kind::Response(r)) => {
                assert_eq!(r.request_id, 42);
                assert_ne!(r.status, rexon_core::proto::control::Status::Ok as i32);
            }
            other => panic!("ожидался Response, получено {other:?}"),
        }
    }

    /// Кадр ответа `health` с этим состоянием движка.
    fn health_frame(request_id: u64, state: &str) -> Vec<u8> {
        use prost::Message;
        use rexon_core::proto::control as pb;
        let resp = pb::Response {
            request_id,
            status: pb::Status::Ok as i32,
            payload: Some(pb::response::Payload::Health(pb::HealthResponse {
                state: state.to_owned(),
                ..Default::default()
            })),
            ..Default::default()
        };
        pb::ServerFrame { kind: Some(pb::server_frame::Kind::Response(resp)) }.encode_to_vec()
    }

    /// Поле `state` доезжает из ответа `health`, а чужой ответ даёт пустоту,
    /// а не чужое значение.
    #[test]
    fn health_state_reads_only_health_payload() {
        assert_eq!(super::health_state(&health_frame(3, "ready")), "ready");
        assert_eq!(super::health_state(&health_frame(3, "")), "");
        assert_eq!(super::health_state(&super::error_frame(3, "x")), "");
    }

    /// Свой ответ `health` уходит подписчикам событием и НЕ идёт дальше —
    /// ни в карту Rust-запросов, ни к Python. Чужой номер не перехватывается.
    #[test]
    fn own_health_reply_reaches_subscribers_once() {
        use std::cell::RefCell;
        use std::rc::Rc;
        let seen: Rc<RefCell<Vec<super::DaemonEvent>>> = Rc::default();
        let sink = Rc::clone(&seen);
        let id = super::subscribe(move |ev| sink.borrow_mut().push(ev.clone()));

        super::HEALTH_ID.with(|c| c.set(41));
        assert!(super::dispatch_frame(&health_frame(41, "ready")));
        assert_eq!(super::HEALTH_ID.with(std::cell::Cell::get), 0, "ожидание снято");
        // Повтор того же номера — уже не наш: ждать больше нечего.
        super::dispatch_frame(&health_frame(41, "ready"));

        super::unsubscribe(id);
        super::dispatch_frame(&health_frame(41, "ready"));
        assert_eq!(*seen.borrow(), vec![super::DaemonEvent::Health("ready".into())]);
    }

    /// Рассылка доходит до КАЖДОГО подписчика, а отписка снимает ровно одного.
    #[test]
    fn broadcast_reaches_every_subscriber() {
        use std::cell::Cell;
        use std::rc::Rc;
        let a = Rc::new(Cell::new(0));
        let b = Rc::new(Cell::new(0));
        let (a2, b2) = (Rc::clone(&a), Rc::clone(&b));
        let ia = super::subscribe(move |_| a2.set(a2.get() + 1));
        let ib = super::subscribe(move |_| b2.set(b2.get() + 1));
        super::broadcast(&super::DaemonEvent::Connected);
        super::unsubscribe(ia);
        super::broadcast(&super::DaemonEvent::HealthTimeout);
        super::unsubscribe(ib);
        super::unsubscribe(0);
        assert_eq!((a.get(), b.get()), (1, 2));
    }
}
