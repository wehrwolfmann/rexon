// Transport shims of the Wallpaper Engine tab: what cxx-qt 0.9 cannot do.
//
// There are exactly four classes of things here, and each is not a whim but
// a limitation:
//
// 1. `new`/`delete` for a QObject. cxx-qt does not create instances of
//    either its own or other QObjects from Rust — the constructor exists
//    only in C++. The same technique is already used in the crate
//    (cpp/rexon_gui_factory.cpp).
// 2. QtDBus. cxx-qt-lib 0.9.1 has not a single wrapper, and
//    `QDBusInterface::call` is variadic over QVariant, which cannot be
//    expressed through cxx in principle.
// 3. QTimer. There are no wrappers; the needed methods are static, and cxx
//    does not support statics.
// 4. Qt methods with default arguments (`QWebSocket::close`): cxx takes the
//    method ADDRESS by the written signature, and it does not match the
//    real one.
#pragma once

#include <QtCore/QString>

#include <cstdint>

class QObject;
class QWebSocket;

extern "C" {

/// A timer tick back into Rust: `id` is the number issued when the timer was
/// set up.
typedef void (*RexonWeTimerTick)(int id);

/// A socket error back into Rust as ready text.
///
/// A separate callback rather than a `#[qsignal]` in the bridge:
/// `QWebSocket::errorOccurred` carries a `QAbstractSocket::SocketError`, and
/// cxx-qt-lib does not have this enum — there is nothing to declare the
/// signal with.
typedef void (*RexonWeSockErrorHook)(const char* utf8, int len);

/// Where to call when timers fire. nullptr removes the handler.
void rexon_we_sock_set_timer_tick(RexonWeTimerTick tick);

/// Where to call on a socket error. nullptr removes the handler.
void rexon_we_sock_set_error_hook(RexonWeSockErrorHook hook);

}  // extern "C"

// ── D-Bus ─────────────────────────────────────────────────────────────────

/// A daemon property via `org.freedesktop.DBus.Properties.Get` on the
/// session bus. `ok` — whether the property was read at all (in Python this
/// is `reply.type() == ReplyMessage` plus non-empty arguments); with `false`
/// the returned string means nothing.
///
/// The bus addresses and the timeout come as parameters, not as file
/// constants: their only source is the core (`rexon_core::we_client`), and
/// there must not be a third copy of the literals in the project (rule #12).
///
/// The value is given as a string rather than a QVariant: the core converts
/// it with its `DbusValue` anyway (`WsPort` is a number,
/// `Version`/`CurrentWallpaperId` are strings), and there is no reason to
/// carry a QVariant across the boundary.
QString rexon_we_sock_daemon_property(const QString& service, const QString& path,
                                      const QString& iface, const QString& name,
                                      int timeout_ms, bool& ok);

// ── socket ────────────────────────────────────────────────────────────────

/// Create a QWebSocket without an owner. Removed by rexon_we_sock_destroy().
/// In a thread without an event dispatcher it first sets one up (see .cpp);
/// nullptr — the dispatcher could not be set up (no QCoreApplication).
QWebSocket* rexon_we_sock_new();

/// Delete the object immediately (not deleteLater): the transport closes the
/// socket itself and immediately creates a new one on reconnection.
void rexon_we_sock_destroy(QObject* object);

/// Close the socket. A shim rather than a direct call from cxx:
/// `QWebSocket::close` has two default arguments (see item 4 in the header).
void rexon_we_sock_close(QWebSocket* socket);

/// Close a one-off socket: if connected — with an ordinary close(), if not
/// (the handshake is not finished) — abort(), without a close frame into an
/// unconnected link.
void rexon_we_sock_shutdown(QWebSocket* socket);

/// Any socket error exits the nested loop (`rexon_we_sock_run_loop`).
void rexon_we_sock_quit_loop_on_error(QWebSocket* socket);

/// Whether the socket is open — strictly
/// `state() == QAbstractSocket::ConnectedState`. This is NOT "pinging the
/// daemon": exactly the same meaning as Python's `is_open()`.
bool rexon_we_sock_is_open(QWebSocket* socket);

/// Text of the last socket error (`QWebSocket::errorString`).
QString rexon_we_sock_error_string(QWebSocket* socket);

/// Subscribe to this socket's `errorOccurred`: the text will go to the
/// callback set by `rexon_we_sock_set_error_hook`.
void rexon_we_sock_watch_error(QWebSocket* socket);

// ── timers ────────────────────────────────────────────────────────────────

/// Set up (or restart) the timer with this number. `single_shot` — a
/// one-off. A repeated call with the same number restarts the timer rather
/// than setting up a second one.
void rexon_we_sock_timer_start(int id, int interval_ms, bool single_shot);

/// Stop the timer with this number. Idempotent.
void rexon_we_sock_timer_stop(int id);

/// One tick of the timer with this number — in the main thread, on the
/// nearest cycle of its event loop. It can be called from ANY thread: this
/// is how a background watcher asks the transport (it lives in the main
/// thread) to do its part.
void rexon_we_sock_post_tick(int id);

// ── environment ───────────────────────────────────────────────────────────

/// Whether there is a live `QCoreApplication`. Without it neither QWebSocket
/// nor QEventLoop work — this is the `("no_qt", 0)` branch of the synchronous
/// wallpaper stop.
bool rexon_we_sock_qt_available();

// ── nested event loop ─────────────────────────────────────────────────────

/// Spin a nested `QEventLoop` for no longer than `ms` milliseconds.
///
/// Needed by the synchronous wallpaper stop (`stop_active_wallpapers`): the
/// static wallpaper installer calls it from its own thread and does not keep
/// an event loop of its own. Return: 0 — exited by
/// `rexon_we_sock_quit_loop()`, 1 — by the watchdog.
int rexon_we_sock_run_loop(int ms);

/// Exit the nested loop. Idempotent and safe outside the loop.
void rexon_we_sock_quit_loop();
