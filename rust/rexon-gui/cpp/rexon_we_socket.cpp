#include "rexon_we_socket.h"

#include <QtCore/QAbstractEventDispatcher>
#include <QtCore/QCoreApplication>
#include <QtCore/QEventLoop>
#include <QtCore/QHash>
#include <QtCore/QPointer>
#include <QtCore/QThread>
#include <QtCore/QTimer>
#include <QtCore/QVariant>
#include <QtDBus/QDBusConnection>
#include <QtDBus/QDBusMessage>
#include <QtDBus/QDBusVariant>
#include <QtNetwork/QAbstractSocket>
#include <QtWebSockets/QWebSocket>

namespace {

RexonWeTimerTick g_tick = nullptr;
RexonWeSockErrorHook g_error_hook = nullptr;

/// Timer number → the timer itself. The numbers are handed out by Rust
/// (`we_socket.rs`), here there is only execution: the number has no meaning
/// of its own here. The transport's timers live in the main thread — the
/// same place as the tab's socket.
QHash<int, QPointer<QTimer>>& timers()
{
    static QHash<int, QPointer<QTimer>> map;
    return map;
}

/// The nested event loop of the synchronous wallpaper stop.
///
/// thread_local rather than one per process: the stop is called by the
/// static wallpaper installer, and it works in its own thread (the QThread
/// worker of desktop operations) — the loop must spin in the same place
/// where the run's socket was created.
QEventLoop*& nested_loop()
{
    static thread_local QEventLoop* loop = nullptr;
    return loop;
}

}  // namespace

// ── D-Bus ─────────────────────────────────────────────────────────────────

QString rexon_we_sock_daemon_property(const QString& service, const QString& path,
                                      const QString& iface, const QString& name,
                                      int timeout_ms, bool& ok)
{
    ok = false;
    QDBusConnection bus = QDBusConnection::sessionBus();
    if (!bus.isConnected())
        return {};

    QDBusMessage msg = QDBusMessage::createMethodCall(
        service, path, QStringLiteral("org.freedesktop.DBus.Properties"),
        QStringLiteral("Get"));
    msg.setArguments({iface, name});
    const QDBusMessage reply = bus.call(msg, QDBus::Block, timeout_ms);
    if (reply.type() != QDBusMessage::ReplyMessage || reply.arguments().isEmpty())
        return {};

    QVariant value = reply.arguments().constFirst();
    // Properties.Get gives a QDBusVariant — unwrap it to the value, otherwise
    // toString() gives an empty string on a live daemon.
    if (value.canConvert<QDBusVariant>())
        value = value.value<QDBusVariant>().variant();
    ok = true;
    return value.toString();
}

// ── socket ────────────────────────────────────────────────────────────────

QWebSocket* rexon_we_sock_new()
{
    // The event dispatcher goes BEFORE the socket. One-off sockets (removing
    // live wallpapers, the engine package watcher) are opened from std::thread
    // worker threads that do not have a dispatcher of their own yet. A socket
    // opened in such a thread sets up no notifiers: nobody hears the answer to
    // the connection, the WebSocket handshake does not go out, and the nested
    // loop exits by the watchdog with an error, while close() writes a close
    // frame into an unconnected socket («QNativeSocketEngine::write() was not
    // called in QAbstractSocket::ConnectedState»; the daemon sees «Handshake
    // not finished»). An empty QEventLoop creates a dispatcher for the whole
    // thread; in the main thread one already exists, and this changes nothing.
    if (QCoreApplication::instance() != nullptr
        && QAbstractEventDispatcher::instance(QThread::currentThread()) == nullptr) {
        QEventLoop warm;
        Q_UNUSED(warm);
    }
    if (QAbstractEventDispatcher::instance(QThread::currentThread()) == nullptr)
        return nullptr;
    // No parent: the transport owns it, rexon_we_sock_destroy() removes it.
    return new QWebSocket();
}

void rexon_we_sock_destroy(QObject* object)
{
    delete object;
}

void rexon_we_sock_close(QWebSocket* socket)
{
    if (socket != nullptr)
        socket->close();
}

void rexon_we_sock_shutdown(QWebSocket* socket)
{
    if (socket == nullptr)
        return;
    // The handshake is not finished — there is nowhere to send a close frame:
    // we cut the link.
    if (socket->state() == QAbstractSocket::ConnectedState)
        socket->close();
    else
        socket->abort();
}

void rexon_we_sock_quit_loop_on_error(QWebSocket* socket)
{
    if (socket == nullptr)
        return;
    // A connection refusal and a disconnection before the handshake — out of
    // the loop at once, not by the watchdog a few seconds later.
    QObject::connect(socket, &QWebSocket::errorOccurred, socket,
                     [](QAbstractSocket::SocketError) { rexon_we_sock_quit_loop(); });
}

bool rexon_we_sock_is_open(QWebSocket* socket)
{
    return socket != nullptr
        && socket->state() == QAbstractSocket::ConnectedState;
}

QString rexon_we_sock_error_string(QWebSocket* socket)
{
    if (socket == nullptr)
        return {};
    return socket->errorString();
}

void rexon_we_sock_watch_error(QWebSocket* socket)
{
    if (socket == nullptr)
        return;
    // The connection context is the socket itself: the subscription dies with
    // it, and the lambda will not outlive the object whose errorString() it
    // reads.
    QObject::connect(socket, &QWebSocket::errorOccurred, socket,
                     [socket](QAbstractSocket::SocketError) {
                         if (g_error_hook == nullptr)
                             return;
                         const QByteArray text = socket->errorString().toUtf8();
                         g_error_hook(text.constData(),
                                      static_cast<int>(text.size()));
                     });
}

// ── timers ────────────────────────────────────────────────────────────────

void rexon_we_sock_set_timer_tick(RexonWeTimerTick tick)
{
    g_tick = tick;
}

void rexon_we_sock_set_error_hook(RexonWeSockErrorHook hook)
{
    g_error_hook = hook;
}

void rexon_we_sock_timer_start(int id, int interval_ms, bool single_shot)
{
    QPointer<QTimer>& slot = timers()[id];
    if (slot.isNull()) {
        // The parent is the application: the transport lives as long, and without
        // a parent the timer would leak on exit.
        auto* timer = new QTimer(QCoreApplication::instance());
        QObject::connect(timer, &QTimer::timeout, timer, [id]() {
            if (g_tick != nullptr)
                g_tick(id);
        });
        slot = timer;
    }
    slot->setSingleShot(single_shot);
    slot->start(interval_ms);
}

void rexon_we_sock_timer_stop(int id)
{
    const QPointer<QTimer> timer = timers().value(id);
    if (!timer.isNull())
        timer->stop();
}

void rexon_we_sock_post_tick(int id)
{
    QCoreApplication* app = QCoreApplication::instance();
    if (app == nullptr)
        return;
    QMetaObject::invokeMethod(
        app,
        [id]() {
            if (g_tick != nullptr)
                g_tick(id);
        },
        Qt::QueuedConnection);
}

// ── environment ───────────────────────────────────────────────────────────

bool rexon_we_sock_qt_available()
{
    return QCoreApplication::instance() != nullptr;
}

// ── nested event loop ─────────────────────────────────────────────────────

int rexon_we_sock_run_loop(int ms)
{
    if (nested_loop() != nullptr)
        return 1;                       // nesting is forbidden — see the header
    QEventLoop loop;
    nested_loop() = &loop;

    bool by_guard = false;
    QTimer guard;
    guard.setSingleShot(true);
    QObject::connect(&guard, &QTimer::timeout, &loop, [&loop, &by_guard]() {
        by_guard = true;
        loop.quit();
    });
    guard.start(ms);

    loop.exec();
    guard.stop();
    nested_loop() = nullptr;
    return by_guard ? 1 : 0;
}

void rexon_we_sock_quit_loop()
{
    if (nested_loop() != nullptr)
        nested_loop()->quit();
}
