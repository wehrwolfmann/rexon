#include "rexon_we_transport.h"

#include <QtCore/QByteArray>
#include <QtCore/QHash>
#include <QtCore/QMetaObject>
#include <QtCore/QPointer>

#include <cstdint>
#include <iterator>

namespace {

/// token → the query that issued it. A QPointer rather than a bare pointer:
/// it is exactly what turns a late answer from a crash into silence.
QHash<qint64, QPointer<QObject>>& targets()
{
    static QHash<qint64, QPointer<QObject>> map;
    return map;
}

/// Throw out the records of dead queries. Called on every send: we have no
/// "the query died" point of our own (Qt does not report it), and there is no
/// reason to keep nulled QPointers in the map.
void forget_dead()
{
    auto& map = targets();
    for (auto it = map.begin(); it != map.end();)
        it = it.value().isNull() ? map.erase(it) : std::next(it);
}

}  // namespace

void rexon_we_remember(qint64 token, QObject* query)
{
    forget_dead();
    targets().insert(token, QPointer<QObject>(query));
}

int rexon_we_do_deliver_response(qint64 token, const char* data, int len)
{
    const QPointer<QObject> target = targets().take(token);
    if (target.isNull())
        return 0;
    // DirectConnection: the socket parses the answer on the main thread, and
    // the queries live there too. A queue here would only defer the same
    // thing.
    const bool ok = QMetaObject::invokeMethod(
        target, "deliverResponse", Qt::DirectConnection,
        Q_ARG(::std::int64_t, token), Q_ARG(QByteArray, QByteArray(data, len)));
    return ok ? 1 : 0;
}

int rexon_we_do_pending_count()
{
    forget_dead();
    return static_cast<int>(targets().size());
}

int rexon_we_do_fail_pending(const char* data, int len)
{
    // We take the map WHOLE before the broadcast: the error handler is free to
    // start a new request, and it must not fall under the same broadcast.
    const QHash<qint64, QPointer<QObject>> dying = targets();
    targets().clear();

    const QByteArray frame(data, len);
    int delivered = 0;
    for (auto it = dying.constBegin(); it != dying.constEnd(); ++it) {
        const QPointer<QObject> target = it.value();
        if (target.isNull())
            continue;
        if (QMetaObject::invokeMethod(
                target, "deliverResponse", Qt::DirectConnection,
                Q_ARG(::std::int64_t, it.key()), Q_ARG(QByteArray, frame)))
            ++delivered;
    }
    return delivered;
}
