#include "rexon_we_socket.h"

#include <QtCore/QCoreApplication>
#include <QtCore/QEventLoop>
#include <QtCore/QHash>
#include <QtCore/QPointer>
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

/// Номер таймера → сам таймер. Номера раздаёт Rust (`we_socket.rs`), здесь
/// только исполнение: своего смысла у номера тут нет. Таймеры транспорта
/// живут в главном потоке — там же, где сокет вкладки.
QHash<int, QPointer<QTimer>>& timers()
{
    static QHash<int, QPointer<QTimer>> map;
    return map;
}

/// Вложенный цикл событий синхронной остановки обоев.
///
/// thread_local, а не один на процесс: остановку зовёт установщик статичных
/// обоев, а он работает в своём потоке (QThread-воркер операций с рабочими
/// столами) — цикл обязан крутиться там же, где создан сокет прогона.
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
    // Properties.Get отдаёт QDBusVariant — развернуть до значения, иначе
    // toString() отдаёт пустую строку на живом демоне.
    if (value.canConvert<QDBusVariant>())
        value = value.value<QDBusVariant>().variant();
    ok = true;
    return value.toString();
}

// ── сокет ─────────────────────────────────────────────────────────────────

QWebSocket* rexon_we_sock_new()
{
    // Без родителя: владеет транспорт, снимает rexon_we_sock_destroy().
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
    // Контекст соединения — сам сокет: подписка умрёт вместе с ним, и
    // лямбда не переживёт объект, чей errorString() читает.
    QObject::connect(socket, &QWebSocket::errorOccurred, socket,
                     [socket](QAbstractSocket::SocketError) {
                         if (g_error_hook == nullptr)
                             return;
                         const QByteArray text = socket->errorString().toUtf8();
                         g_error_hook(text.constData(),
                                      static_cast<int>(text.size()));
                     });
}

// ── таймеры ───────────────────────────────────────────────────────────────

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
        // Родитель — приложение: транспорт живёт столько же, а без родителя
        // таймер утёк бы при выходе.
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

// ── окружение ─────────────────────────────────────────────────────────────

bool rexon_we_sock_qt_available()
{
    return QCoreApplication::instance() != nullptr;
}

// ── вложенный цикл событий ────────────────────────────────────────────────

int rexon_we_sock_run_loop(int ms)
{
    if (nested_loop() != nullptr)
        return 1;                       // вложенность запрещена — см. шапку
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
