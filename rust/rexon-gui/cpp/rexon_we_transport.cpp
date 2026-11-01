#include "rexon_we_transport.h"

#include <QtCore/QByteArray>
#include <QtCore/QHash>
#include <QtCore/QMetaObject>
#include <QtCore/QPointer>

#include <cstdint>
#include <iterator>

namespace {

/// token → запрос, который его выдал. QPointer, а не голый указатель:
/// именно он превращает поздний ответ из падения в тишину.
QHash<qint64, QPointer<QObject>>& targets()
{
    static QHash<qint64, QPointer<QObject>> map;
    return map;
}

/// Выбросить записи умерших запросов. Зовётся при каждой отправке: своей
/// точки «запрос умер» у нас нет (Qt о ней не сообщает), а держать в карте
/// обнулённые QPointer'ы незачем.
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
    // DirectConnection: сокет разбирает ответ на главном потоке, там же
    // живут запросы. Очередь здесь только отложила бы то же самое.
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
    // Карту забираем ЦЕЛИКОМ до рассылки: обработчик ошибки волен начать
    // новый запрос, и он не должен попасть под ту же рассылку.
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
