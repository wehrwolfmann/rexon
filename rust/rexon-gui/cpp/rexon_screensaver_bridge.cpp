#include "rexon_screensaver_bridge.h"

#include <QtCore/QCoreApplication>
#include <QtCore/QList>
#include <QtCore/QMetaObject>
#include <QtCore/QObject>
#include <QtCore/QString>
#include <QtCore/QStringList>
#include <QtCore/QThread>
#include <QtCore/QVariant>
#include <QtDBus/QDBusConnection>
#include <QtDBus/QDBusConnectionInterface>
#include <QtDBus/QDBusMessage>
#include <QtDBus/QDBusPendingCall>
#include <QtDBus/QDBusPendingCallWatcher>
#include <QtDBus/QDBusServiceWatcher>
#include <QtDBus/QDBusVariant>
#include <QtDBus/QDBusVirtualObject>

#include <functional>
#include <map>
#include <vector>

namespace {

RexonSsbMessage g_on_message = nullptr;
RexonSsbCallDone g_on_call_done = nullptr;
RexonSsbVanished g_on_vanished = nullptr;
RexonSsbLost g_on_lost = nullptr;

// Квитанции: пришедшие вызовы, ответа на которые Rust ещё не дал.
std::map<uint64_t, QDBusMessage> g_pending;
uint64_t g_next_token = 1;

QDBusServiceWatcher *g_watcher = nullptr;
QDBusServiceWatcher *g_self_watcher = nullptr;
QStringList g_paths;

/// Значение аргумента строкой: так оно и уходит через границу.
QByteArray to_text(QVariant value)
{
    if (value.canConvert<QDBusVariant>())
        value = value.value<QDBusVariant>().variant();
    return value.toString().toUtf8();
}

/// Строки Rust → значения D-Bus по сигнатуре из простых типов.
QList<QVariant> from_texts(const char *signature, const char *const *values, int32_t count)
{
    QList<QVariant> out;
    const QByteArray sig(signature != nullptr ? signature : "");
    for (int32_t i = 0; i < count && i < sig.size(); ++i) {
        const QString text = QString::fromUtf8(values[i]);
        switch (sig.at(i)) {
        case 'u':
            out << QVariant::fromValue<uint>(text.toUInt());
            break;
        case 'i':
            out << QVariant::fromValue<int>(text.toInt());
            break;
        case 'b':
            out << QVariant::fromValue<bool>(text == QLatin1String("true"));
            break;
        default:
            out << QVariant::fromValue<QString>(text);
            break;
        }
    }
    return out;
}

/// Объект на шине: каждый вызов — в Rust как есть, решения здесь нет.
class Bridge : public QDBusVirtualObject
{
public:
    explicit Bridge(QString xml)
        : m_xml(std::move(xml))
    {
    }

    QString introspect(const QString &) const override { return m_xml; }

    bool handleMessage(const QDBusMessage &message, const QDBusConnection &) override
    {
        if (message.type() != QDBusMessage::MethodCallMessage || g_on_message == nullptr)
            return false;
        const uint64_t token = g_next_token++;
        g_pending[token] = message;

        QList<QByteArray> texts;
        for (const QVariant &arg : message.arguments())
            texts << to_text(arg);
        std::vector<const char *> ptrs;
        for (const QByteArray &t : texts)
            ptrs.push_back(t.constData());

        const QByteArray sender = message.service().toUtf8();
        const QByteArray path = message.path().toUtf8();
        const QByteArray iface = message.interface().toUtf8();
        const QByteArray member = message.member().toUtf8();
        const QByteArray sig = message.signature().toUtf8();
        g_on_message(token, sender.constData(), path.constData(), iface.constData(),
                     member.constData(), sig.constData(), ptrs.data(),
                     static_cast<int32_t>(ptrs.size()));
        return true;
    }

private:
    QString m_xml;
};

Bridge *g_bridge = nullptr;

bool on_gui_thread(const std::function<int32_t()> &work, int32_t *result)
{
    QCoreApplication *app = QCoreApplication::instance();
    if (app == nullptr)
        return false;
    if (QThread::currentThread() == app->thread()) {
        *result = work();
        return true;
    }
    QMetaObject::invokeMethod(
        app, [&work, result] { *result = work(); }, Qt::BlockingQueuedConnection);
    return true;
}

} // namespace

extern "C" {

void rexon_ssb_set_hooks(RexonSsbMessage on_message, RexonSsbCallDone on_call_done,
                         RexonSsbVanished on_vanished, RexonSsbLost on_lost)
{
    g_on_lost = on_lost;
    g_on_message = on_message;
    g_on_call_done = on_call_done;
    g_on_vanished = on_vanished;
}

int32_t rexon_ssb_name_has_owner(const char *name)
{
    QDBusConnection bus = QDBusConnection::sessionBus();
    if (!bus.isConnected() || bus.interface() == nullptr)
        return -1;
    const auto reply = bus.interface()->isServiceRegistered(QString::fromUtf8(name));
    if (!reply.isValid())
        return -1;
    return reply.value() ? 1 : 0;
}

int32_t rexon_ssb_start(const char *service, const char *const *paths, int32_t count,
                        const char *introspection_xml)
{
    const QString name = QString::fromUtf8(service);
    QStringList where;
    for (int32_t i = 0; i < count; ++i)
        where << QString::fromUtf8(paths[i]);
    const QString xml = QString::fromUtf8(introspection_xml);

    int32_t result = 2;
    const bool ran = on_gui_thread(
        [&]() -> int32_t {
            QDBusConnection bus = QDBusConnection::sessionBus();
            if (!bus.isConnected() || bus.interface() == nullptr)
                return 2;
            if (g_bridge != nullptr)
                return 0;
            // Сначала объект, потом имя: клиент, увидевший имя, сразу шлёт
            // вызов — объект к этому моменту обязан уже стоять.
            auto *bridge = new Bridge(xml);
            bool all = true;
            for (const QString &p : where)
                all = bus.registerVirtualObject(p, bridge) && all;
            if (!all) {
                for (const QString &p : where)
                    bus.unregisterObject(p);
                delete bridge;
                return 3;
            }
            // Без очереди: занято — значит, у сеанса уже есть настоящий
            // хозяин, и вставать за ним в очередь незачем. С уступкой: если
            // хозяин появится позже и попросит имя с заменой, оно уйдёт ему.
            const auto reply = bus.interface()->registerService(
                name, QDBusConnectionInterface::DontQueueService,
                QDBusConnectionInterface::AllowReplacement);
            if (!reply.isValid() || reply.value() != QDBusConnectionInterface::ServiceRegistered) {
                for (const QString &p : where)
                    bus.unregisterObject(p);
                delete bridge;
                return 1;
            }
            g_bridge = bridge;
            g_watcher = new QDBusServiceWatcher(bridge);
            g_watcher->setConnection(bus);
            g_watcher->setWatchMode(QDBusServiceWatcher::WatchForUnregistration);
            QObject::connect(g_watcher, &QDBusServiceWatcher::serviceUnregistered, bridge,
                             [](const QString &gone) {
                                 if (g_on_vanished != nullptr)
                                     g_on_vanished(gone.toUtf8().constData());
                             });
            // Своё имя — под отдельной слежкой: хозяин, попросивший его с
            // заменой, забирает его, и мосту пора сойти.
            g_paths = where;
            g_self_watcher = new QDBusServiceWatcher(name, bus,
                                                     QDBusServiceWatcher::WatchForOwnerChange, bridge);
            const QString me = bus.baseService();
            QObject::connect(g_self_watcher, &QDBusServiceWatcher::serviceOwnerChanged, bridge,
                             [me](const QString &, const QString &, const QString &owner) {
                                 if (owner != me && g_on_lost != nullptr)
                                     g_on_lost();
                             });
            return 0;
        },
        &result);
    return ran ? result : 2;
}

void rexon_ssb_reply(uint64_t token, const char *signature, const char *const *values,
                     int32_t count)
{
    auto it = g_pending.find(token);
    if (it == g_pending.end())
        return;
    const QDBusMessage call = it->second;
    g_pending.erase(it);
    QDBusConnection::sessionBus().send(call.createReply(from_texts(signature, values, count)));
}

void rexon_ssb_reply_error(uint64_t token, const char *error_name, const char *text)
{
    auto it = g_pending.find(token);
    if (it == g_pending.end())
        return;
    const QDBusMessage call = it->second;
    g_pending.erase(it);
    QDBusConnection::sessionBus().send(call.createErrorReply(QString::fromUtf8(error_name),
                                                              QString::fromUtf8(text)));
}

void rexon_ssb_call(uint64_t ctx, const char *service, const char *path, const char *iface,
                    const char *method, const char *signature, const char *const *values,
                    int32_t count, int32_t timeout_ms)
{
    QDBusMessage msg = QDBusMessage::createMethodCall(
        QString::fromUtf8(service), QString::fromUtf8(path), QString::fromUtf8(iface),
        QString::fromUtf8(method));
    msg.setArguments(from_texts(signature, values, count));
    QDBusPendingCall pending = QDBusConnection::sessionBus().asyncCall(msg, timeout_ms);
    // Родитель — приложение, а не мост: мост сходит (`rexon_ssb_stop`), а
    // поздний ответ сеанса на проброс должен дойти до ядра — иначе выданную
    // им куки никто не снимет.
    auto *watcher = new QDBusPendingCallWatcher(pending, QCoreApplication::instance());
    QObject::connect(watcher, &QDBusPendingCallWatcher::finished, watcher,
                     [ctx](QDBusPendingCallWatcher *w) {
                         const QDBusMessage reply = w->reply();
                         w->deleteLater();
                         if (g_on_call_done == nullptr)
                             return;
                         if (reply.type() == QDBusMessage::ReplyMessage) {
                             const QByteArray value = reply.arguments().isEmpty()
                                 ? QByteArray()
                                 : to_text(reply.arguments().first());
                             g_on_call_done(ctx, true, value.constData());
                         } else {
                             const QByteArray err =
                                 (reply.errorName() + QLatin1Char(' ') + reply.errorMessage())
                                     .toUtf8();
                             g_on_call_done(ctx, false, err.constData());
                         }
                     });
}

void rexon_ssb_watch(const char *name)
{
    if (g_watcher != nullptr)
        g_watcher->addWatchedService(QString::fromUtf8(name));
}

void rexon_ssb_unwatch(const char *name)
{
    if (g_watcher != nullptr)
        g_watcher->removeWatchedService(QString::fromUtf8(name));
}

} // extern "C"

extern "C" void rexon_ssb_stop(const char *service)
{
    if (g_bridge == nullptr)
        return;
    QDBusConnection bus = QDBusConnection::sessionBus();
    for (const QString &p : g_paths)
        bus.unregisterObject(p);
    bus.unregisterService(QString::fromUtf8(service));
    // Квитанции без ответа закрыть: клиенты не должны ждать до таймаута.
    for (auto &kv : g_pending)
        bus.send(kv.second.createErrorReply(QStringLiteral("org.freedesktop.DBus.Error.Failed"),
                                            QString()));
    g_pending.clear();
    g_bridge->deleteLater();
    g_bridge = nullptr;
    g_watcher = nullptr;
    g_self_watcher = nullptr;
}
