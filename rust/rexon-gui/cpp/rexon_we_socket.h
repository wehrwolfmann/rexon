// Шимы транспорта вкладки Wallpaper Engine: то, чего cxx-qt 0.9 не умеет.
//
// Здесь ровно четыре класса вещей, и каждая — не прихоть, а ограничение:
//
// 1. `new`/`delete` для QObject. cxx-qt не создаёт экземпляры ни своих, ни
//    чужих QObject'ов из Rust — конструктор существует только в C++. Тот же
//    приём уже применён в крейте (cpp/rexon_gui_factory.cpp).
// 2. QtDBus. Обёрток в cxx-qt-lib 0.9.1 нет ни одной, а
//    `QDBusInterface::call` — вариадик по QVariant, через cxx его не
//    выразить в принципе.
// 3. QTimer. Обёрток нет; нужные методы —
//    статические, а cxx статику не поддерживает.
// 4. Методы Qt с аргументами по умолчанию (`QWebSocket::close`): cxx берёт
//    АДРЕС метода по написанной сигнатуре, а она не совпадает с настоящей.
#pragma once

#include <QtCore/QString>

#include <cstdint>

class QObject;
class QWebSocket;

extern "C" {

/// Тик таймера обратно в Rust: `id` — номер, выданный при заводе таймера.
typedef void (*RexonWeTimerTick)(int id);

/// Ошибка сокета обратно в Rust готовым текстом.
///
/// Отдельный колбэк, а не `#[qsignal]` в мосте: `QWebSocket::errorOccurred`
/// несёт `QAbstractSocket::SocketError`, а этого enum в cxx-qt-lib нет —
/// объявить сигнал с ним нечем.
typedef void (*RexonWeSockErrorHook)(const char* utf8, int len);

/// Куда звать при срабатывании таймеров. nullptr снимает обработчик.
void rexon_we_sock_set_timer_tick(RexonWeTimerTick tick);

/// Куда звать при ошибке сокета. nullptr снимает обработчик.
void rexon_we_sock_set_error_hook(RexonWeSockErrorHook hook);

}  // extern "C"

// ── D-Bus ─────────────────────────────────────────────────────────────────

/// Свойство демона по `org.freedesktop.DBus.Properties.Get` на сессионной
/// шине. `ok` — было ли свойство прочитано вообще (в Python это
/// `reply.type() == ReplyMessage` плюс непустые аргументы); при `false`
/// возвращённая строка не значит ничего.
///
/// Адреса шины и таймаут приходят параметрами, а не константами файла:
/// единственный их источник — ядро (`rexon_core::we_client`), и третьей
/// копии литералов в проекте быть не должно (правило #12).
///
/// Значение отдаётся строкой, а не QVariant: ядро всё равно приводит его
/// своим `DbusValue` (`WsPort` — число, `Version`/`CurrentWallpaperId` —
/// строки), и таскать QVariant через границу незачем.
QString rexon_we_sock_daemon_property(const QString& service, const QString& path,
                                      const QString& iface, const QString& name,
                                      int timeout_ms, bool& ok);

// ── сокет ─────────────────────────────────────────────────────────────────

/// Создать QWebSocket без владельца. Снимается rexon_we_sock_destroy().
QWebSocket* rexon_we_sock_new();

/// Немедленно удалить объект (не deleteLater): транспорт закрывает сокет
/// сам и сразу заводит новый при переподключении.
void rexon_we_sock_destroy(QObject* object);

/// Закрыть сокет. Шим, а не прямой вызов из cxx: у `QWebSocket::close` два
/// аргумента по умолчанию (см. п. 4 в шапке).
void rexon_we_sock_close(QWebSocket* socket);

/// Открыт ли сокет — строго `state() == QAbstractSocket::ConnectedState`.
/// Это НЕ «пингую демона»: ровно тот же смысл, что у `is_open()` Python.
bool rexon_we_sock_is_open(QWebSocket* socket);

/// Текст последней ошибки сокета (`QWebSocket::errorString`).
QString rexon_we_sock_error_string(QWebSocket* socket);

/// Подписаться на `errorOccurred` этого сокета: текст уйдёт в колбэк,
/// поставленный `rexon_we_sock_set_error_hook`.
void rexon_we_sock_watch_error(QWebSocket* socket);

// ── таймеры ───────────────────────────────────────────────────────────────

/// Завести (или перезавести) таймер с этим номером. `single_shot` —
/// одноразовый. Повторный вызов с тем же номером перезапускает таймер, а
/// не заводит второй.
void rexon_we_sock_timer_start(int id, int interval_ms, bool single_shot);

/// Остановить таймер с этим номером. Идемпотентно.
void rexon_we_sock_timer_stop(int id);

/// Один тик таймера с этим номером — в главном потоке, на ближайшем такте
/// его цикла событий. Звать можно из ЛЮБОГО потока: так фоновый сторож
/// просит транспорт (он живёт в главном потоке) сделать свою часть.
void rexon_we_sock_post_tick(int id);

// ── окружение ─────────────────────────────────────────────────────────────

/// Есть ли живой `QCoreApplication`. Без него ни QWebSocket, ни QEventLoop
/// не работают — это ветка `("no_qt", 0)` синхронной остановки обоев.
bool rexon_we_sock_qt_available();

// ── вложенный цикл событий ────────────────────────────────────────────────

/// Крутить вложенный `QEventLoop` не дольше `ms` миллисекунд.
///
/// Нужен синхронной остановке обоев (`stop_active_wallpapers`): установщик
/// статичных обоев зовёт её из своего потока и своего event loop не держит.
/// Возврат: 0 — вышли по `rexon_we_sock_quit_loop()`, 1 — по сторожу.
int rexon_we_sock_run_loop(int ms);

/// Выйти из вложенного цикла. Идемпотентно и безопасно вне цикла.
void rexon_we_sock_quit_loop();
