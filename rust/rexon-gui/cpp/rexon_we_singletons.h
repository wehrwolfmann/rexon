// Qt-услуги синглтонов вкладки Wallpaper Engine, которых нет ни в
// cxx-qt-lib, ни в ядре: шина D-Bus, сетевой кэш QML, рассылка событий
// демона по живым синглтонам и упаковка QObject* в QVariant.
//
// Почему шим, а не Rust: QtDBus, QNetworkDiskCache и
// QQmlNetworkAccessManagerFactory обёрток в cxx-qt-lib 0.9 не имеют вовсе, а
// QVariant из cxx-qt-lib не умеет держать указатель на QObject — именно этим
// видом значения вендорный QML читает списки менеджеров
// (`Repeater { model: App.rendererManager.renderers }`).
//
// Синглтоны Notify/App — разные cxx-qt-типы; кадр демона, обрыв и ошибку
// им отдаёт вызов инвокабла по ИМЕНИ через метаобъект
// (`QMetaObject::invokeMethod`), а держит их QPointer — ни того, ни другого
// в cxx-qt-lib нет. Решений здесь нет: что раздавать и когда, решает Rust
// (src/we_socket.rs, src/we_singletons.rs).
//
// Почему обычные функции, а не QObject с сигналами: cpp_files крейта через
// moc не проходят (их собирает cxx-qt-build как есть). Зовёт их только сам
// крейт через `extern "C"` (src/we_singletons.rs); C-символов наружу из .so
// он не отдаёт.
#pragma once

#include <QtCore/QtGlobal>

#include <cstdint>

class QByteArray;
class QObject;
class QString;

extern "C" {

// ── события демона → живые синглтоны ──────────────────────────────────────

/// Записать синглтон, которому раздаются кадры демона. Указатели держит
/// QPointer, поэтому снимать запись не нужно — умерший объект выпадает сам.
/// Получателей ровно два, и у каждого своя доля работы: Notify считает
/// состояние и рассылает сигналы, App кормит свои живые модели
/// (рендереры/дисплеи/библиотеки).
///
/// Регистрация ПРОИГРЫВАЕТ получателю всё, что пришло до неё (см. кэш
/// повтора в .cpp): демон отдаёт снимок состояния один раз на подключение
/// WebSocket, а синглтоны строит движок QML, который вкладка заводит лениво.
void rexon_we_register_notify(QObject* notify);
void rexon_we_register_app(QObject* app);

/// Раздать СЫРОЙ кадр `ServerFrame` обоим получателям (инвокабл
/// `deliverEvent(QByteArray)`). Возвращает, скольким доехало ПРЯМО СЕЙЧАС;
/// кадр для ещё не зарегистрированного получателя не теряется, а ложится в
/// кэш повтора и уедет ему при регистрации.
///
/// Обёртка Rust — `we_singletons::deliver_singleton_event_bytes`.
int rexon_we_do_deliver_event(const char* data, int len);

/// Сообщить Notify, что WebSocket оборвался (инвокабл
/// `transportDisconnected()`). Возвращает 1, если доехало.
int rexon_we_do_transport_disconnected();

/// Ошибка транспорта → тост окна (инвокабл `clientError(QString)`
/// синглтона App). Зовёт `we_socket::report_error`; 0 — App ещё нет.
int rexon_we_do_client_error(const char* message, int len);

// ── D-Bus: доступность демона ─────────────────────────────────────────────

/// Синхронный опрос демона по шине: `Get(WsPort)` и `Get(Version)`.
///
/// Отдаёт JSON `{"connected":bool,"port":int,"version":str,
/// "versionMissing":bool}` — карта, а не набор out-параметров: через cxx
/// изменяемые ссылки на Qt-типы не ходят, а QByteArray уже используется
/// соседним шимом ровно для этого.
QByteArray rexon_we_dbus_probe();

/// Подписать `receiver` на появление/исчезновение демона на шине и на его
/// сигналы `Ready`/`ShuttingDown`/`PropertiesChanged`. Все они сводятся к
/// одному инвокаблу `onDaemonBusChanged()` — что именно изменилось, всё
/// равно выясняет повторный опрос.
///
/// Возвращает число удавшихся подписок (0 — шины нет вовсе).
///
/// Живых подписок у нашего Python не было ВООБЩЕ: статус демона обновлялся
/// только по явному `refreshWsPort()` из QML, поэтому диалог «демон не
/// запущен» не замечал ни запуска демона, ни его остановки. У оригинала
/// (`ui/src/daemon_dbus.cpp`) подписки есть.
int rexon_we_dbus_watch(QObject* receiver);

/// Запустить демона. По умолчанию — как в оригинале,
/// `QProcess::startDetached("waywallen")`; если поставлен пускатель Rexon
/// (`we_socket::launch_daemon_hook`, он ещё и ставит KDE-плагин показа),
/// зовётся он.
bool rexon_we_launch_daemon();

/// Колбэк пускателя демона (ставит Rust: `we_singletons::set_launch_hook`).
typedef bool (*RexonWeLaunchHook)();
void rexon_we_do_set_launch_hook(RexonWeLaunchHook hook);

/// SIGTERM процессу. Отдельной функцией, потому что зависимости `libc` у
/// крейта нет, а тащить её ради одного вызова дороже трёх строк на C++.
bool rexon_we_kill_process(int pid);

/// Попросить демона остановиться — метод `Quit` его интерфейса
/// (порт `DaemonDBusClient::quitDaemon`). Возвращает, приняли ли вызов.
bool rexon_we_quit_daemon();

/// Попросить демона заново опросить дисплеи — метод `RefreshDisplays` его
/// интерфейса: демон в ответ рассылает `Ready`, и клиенты показа
/// переподключаются. Возвращает, приняли ли вызов.
bool rexon_we_refresh_displays();

// ── сетевой кэш QML ───────────────────────────────────────────────────────

/// Поставить движку QML фабрику менеджера сети с дисковым кэшем.
/// `engine` — указатель на QQmlEngine (проверенное приведение внутри).
void rexon_we_install_network_cache(QObject* engine);

/// Задать каталог и предел кэша до установки фабрики (зовёт
/// `we_env::prepare_engine`; путь считает ядро — `qml_modules::net_cache_dir`).
void rexon_we_do_set_network_cache(const char* dir, ::std::int64_t max_bytes);

// ── язык интерфейса вендорных страниц ─────────────────────────────────────
//
// Порт `ui/src/ui_language.cpp` waywallen: комбобокс «Язык» на странице
// настроек. Решения (какие языки доступны, что грузить) принимает ядро
// (`rexon_core::remote_l10n`), а сам язык — общий с Rexon (`ui_language`);
// здесь только то, чего в Rust нет: QTranslator и пересборка привязок движка.

/// Запомнить движок QML вкладки: без него смена языка не пересобрала бы
/// привязки `qsTr()` уже показанной страницы. Указатель под QPointer —
/// вкладка вправе пересоздать движок.
void rexon_we_do_register_qml_engine(QObject* engine);

/// Поставить каталог вендорных страниц и пересобрать привязки QML.
///
/// Язык выбирает Rexon (`ui_language`), подписи списка — тоже его
/// (`qt_settings::language_choices`): своего выбора языка, своих подписей
/// «Системный» и своего ключа `QSettings` у вкладки обоев больше нет.
///
/// `locale_name` — имя локали Qt (`ru`, `zh_CN`); пусто — системная.
/// `qm_path` — путь собранного каталога; пусто — исходные (английские)
/// строки. `required` — без каталога считать смену неудачной.
///
/// Возвращает разрешённый язык в форме BCP 47 (`ru`, `zh-CN`) — его же
/// оригинал отдаёт подписям плагинов. Пусто — язык не применён.
QString rexon_we_apply_ui_language(const QString& locale_name, const QString& qm_path,
                                   bool required);

::std::int64_t rexon_we_network_cache_size();
::std::int64_t rexon_we_network_cache_max();
void rexon_we_set_network_cache_max(::std::int64_t bytes);
void rexon_we_clear_network_cache();
}

// ── синглтон Action ───────────────────────────────────────────────────────

/// Хранилище выбора обоей под QPointer'ом. У оригинала (`ui/src/action.cppm`)
/// это поле `QPointer<QObject>`; здесь оно живёт в шиме, потому что
/// cxx-qt-lib обёртки QPointer не даёт, а голый указатель на объект QML-
/// страницы после её закрытия висячий.
void rexon_we_action_set_storage(QObject* storage);
QObject* rexon_we_action_storage();

/// Текст в системный буфер обмена (порт `Action::copyToClipboard`).
void rexon_we_copy_to_clipboard(const QString& text);

// ── настройки вкладки в QSettings ─────────────────────────────────────────

/// Настройка «да/нет» в `QSettings` приложения.
///
/// Обобщённая пара, а не пара под один ключ: у оригинала в `QSettings`
/// живут и другие переключатели вкладки, и второму понадобится ровно это
/// же (правило #12).
bool rexon_we_settings_bool(const QString& key, bool fallback);
void rexon_we_set_settings_bool(const QString& key, bool value);

// ── живой список рендереров вне событий ───────────────────────────────────

/// Полная замена списка рендереров из ответа `renderer_list`.
///
/// `RendererListQuery` (status_queries.rs) и живая модель
/// `App.rendererManager` — разные cxx-qt-мосты, и добраться до модели
/// запрос может только через шим. У оригинала эту синхронизацию делает сам
/// запрос (`RendererListQuery::reload` → `RendererManager::replaceAll`), и
/// терять её нельзя: без неё список рендереров обновляется только
/// событиями демона.
///
/// `frame` — СЫРОЙ кадр ответа `renderer_list` (тот же, что разобрал запрос).
extern "C" {
// Связка C: объявление зовёт Rust обычным `extern "C"`-блоком
// (we_singletons.rs), а не мостом cxx — без этой обёртки имя ушло бы
// в таблицу искажённым, и сборка не слинковалась бы.
int rexon_we_do_renderers_replace_frame(const char* frame, int len);
}
