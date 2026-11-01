#include "rexon_we_singletons.h"

#include <QtCore/QByteArray>
#include <QtCore/QCoreApplication>
#include <QtCore/QDir>
#include <QtCore/QDirIterator>
#include <QtCore/QFile>
#include <QtCore/QJsonDocument>
#include <QtCore/QJsonObject>
#include <QtCore/QLocale>
#include <QtCore/QTranslator>
#include <QtCore/QList>
#include <QtCore/QMetaObject>
#include <QtCore/QPointer>
#include <QtCore/QProcess>
#include <QtCore/QSettings>
#include <QtCore/QString>
#include <QtCore/QVariant>
#include <QtDBus/QDBusConnection>
#include <QtDBus/QDBusConnectionInterface>
#include <QtDBus/QDBusMessage>
#include <QtDBus/QDBusServiceWatcher>
#include <QtDBus/QDBusVariant>
#include <QtGui/QClipboard>
#include <QtGui/QGuiApplication>
#include <QtNetwork/QNetworkAccessManager>
#include <QtNetwork/QNetworkDiskCache>
#include <QtQml/QQmlEngine>
#include <QtQml/QQmlNetworkAccessManagerFactory>

#include <csignal>
#include <sys/types.h>

namespace {

// Имена шины взяты из оригинала (ui/src/daemon_dbus.cpp) — они же в
// rexon/qt_wallpaper_engine/client.py.
constexpr const char* kBusName = "org.waywallen.waywallen.Daemon";
constexpr const char* kObjectPath = "/org/waywallen/waywallen/Daemon";
constexpr const char* kInterface = "org.waywallen.waywallen.Daemon1";
constexpr const char* kPropsIface = "org.freedesktop.DBus.Properties";

/// Синглтоны-получатели событий демона. QPointer, а не голый указатель:
/// синглтон переживает движок QML, но не переживает конец программы, и
/// раздавать событие мёртвому объекту нельзя.
QPointer<QObject>& notify_sink()
{
    static QPointer<QObject> sink;
    return sink;
}

QPointer<QObject>& app_sink()
{
    static QPointer<QObject> sink;
    return sink;
}

// ── кэш повтора стартового пакета ─────────────────────────────────────────
//
// Демон шлёт снимок состояния РОВНО ОДИН РАЗ на подключение WebSocket
// (display_snapshot, renderer_snapshot, library_snapshot, status_sync,
// playlist_changed — шесть кадров на ~490 байт) и дальше молчит, пока
// состояние не изменится. Получатель, родившийся позже этого пакета,
// остался бы пустым НАВСЕГДА: вкладка показывала «Нет подключённых
// дисплеев», а кнопка «Применить» — серой.
//
// Родиться позже — норма, а не авария: `#[qml_singleton]` строит движок
// QML, а движок вкладка заводит лениво, при первом заходе на QML-страницу
// (`window._ensure_qml_host`), тогда как сокет открывается сразу по
// готовности демона. Ранняя постройка в `rexon_do_register_we_types`
// закрывает этот порядок, но держится на одной строке и на том, что
// `set_client` зовут раньше `client.open()`. Кэш убирает саму зависимость
// от порядка: кадры, пришедшие до первой регистрации, ждут её здесь и
// проигрываются получателю в исходном порядке.
//
// Очередь СВОЯ У КАЖДОГО получателя: Notify и App регистрируются разными
// конструкторами, и общая очередь при повторе задвоила бы кадры тому, кто
// успел раньше.
constexpr int kReplayMaxFrames = 256;
constexpr ::std::int64_t kReplayMaxBytes = 1 << 20;  // 1 МиБ

/// Кольцевой буфер с потолком: стартовый пакет — шесть кадров на полкило-
/// байта, поэтому потолок здесь не рабочий режим, а страховка от роста без
/// границ, если получатель не появится никогда (вкладку так и не открыли).
/// Переполнение выбрасывает САМЫЕ СТАРЫЕ кадры: свежие ближе к текущему
/// состоянию демона.
struct ReplayQueue {
    QList<QByteArray> frames;
    ::std::int64_t bytes = 0;

    void push(const QByteArray& frame)
    {
        frames.append(frame);
        bytes += frame.size();
        while (frames.size() > kReplayMaxFrames || bytes > kReplayMaxBytes) {
            bytes -= frames.constFirst().size();
            frames.removeFirst();
        }
    }

    void clear()
    {
        frames.clear();
        bytes = 0;
    }
};

ReplayQueue& notify_pending()
{
    static ReplayQueue queue;
    return queue;
}

ReplayQueue& app_pending()
{
    static ReplayQueue queue;
    return queue;
}

/// Раздать кадр одному получателю либо отложить его до регистрации.
///
/// DirectConnection: кадр приходит на главном потоке, там же живут
/// синглтоны. Очередь только отложила бы то же самое.
bool deliver_or_queue(const QPointer<QObject>& sink, ReplayQueue& pending,
                      const QByteArray& frame)
{
    QObject* target = sink.data();
    if (target == nullptr) {
        pending.push(frame);
        return false;
    }
    return QMetaObject::invokeMethod(target, "deliverEvent", Qt::DirectConnection,
                                     Q_ARG(QByteArray, frame));
}

/// Проиграть накопленное новому получателю. Очередь опустошается ДО раздачи:
/// обработчик кадра может дойти до нового `deliver`, и повтор не должен
/// увидеть свою же очередь второй раз.
int replay_pending(QObject* target, ReplayQueue& pending)
{
    if (target == nullptr || pending.frames.isEmpty())
        return 0;
    const QList<QByteArray> frames = pending.frames;
    pending.clear();
    int delivered = 0;
    for (const QByteArray& frame : frames) {
        if (QMetaObject::invokeMethod(target, "deliverEvent", Qt::DirectConnection,
                                      Q_ARG(QByteArray, frame)))
            ++delivered;
    }
    return delivered;
}

RexonWeLaunchHook g_launch_hook = nullptr;

QVariant unwrap_variant(QVariant v)
{
    // Свойство приезжает завёрнутым в QDBusVariant — как и у оригинала,
    // разворачиваем, иначе toUInt() отдаст ноль на живом демоне.
    if (v.canConvert<QDBusVariant>())
        v = v.value<QDBusVariant>().variant();
    return v;
}

bool is_unknown_property_error(const QString& name)
{
    // Разные реализации шины называют «нет такого свойства» по-разному —
    // принимаем оба имени (ветка оригинала).
    return name == QLatin1String("org.freedesktop.DBus.Error.UnknownProperty")
        || name == QLatin1String("org.freedesktop.DBus.Error.InvalidArgs");
}

QDBusMessage call_get(QDBusConnection& bus, const QString& prop)
{
    QDBusMessage msg = QDBusMessage::createMethodCall(
        QLatin1String(kBusName), QLatin1String(kObjectPath),
        QLatin1String(kPropsIface), QStringLiteral("Get"));
    msg << QLatin1String(kInterface) << prop;
    return bus.call(msg, QDBus::Block, 2000);
}

// ── сетевой кэш QML ───────────────────────────────────────────────────────

QString& cache_dir()
{
    static QString dir;
    return dir;
}

::std::int64_t& cache_max()
{
    // Дефолт оригинала (app.cpp: 1 ГиБ). Всё равно перекрывается первым же
    // вызовом setNetworkCacheMaximumSize из Global.qml.
    static ::std::int64_t max_bytes = 1024LL * 1024LL * 1024LL;
    return max_bytes;
}

QList<QPointer<QNetworkDiskCache>>& caches()
{
    static QList<QPointer<QNetworkDiskCache>> list;
    return list;
}

class RexonNamFactory : public QQmlNetworkAccessManagerFactory {
public:
    QNetworkAccessManager* create(QObject* parent) override
    {
        auto* manager = new QNetworkAccessManager(parent);
        if (cache_dir().isEmpty())
            return manager;
        auto* cache = new QNetworkDiskCache(manager);
        cache->setCacheDirectory(cache_dir());
        cache->setMaximumCacheSize(cache_max());
        manager->setCache(cache);
        // Держим слабую ссылку: размер и очистку QML спрашивает у App, а
        // считает их именно живой кэш — обход каталога дал бы другую цифру.
        caches().append(QPointer<QNetworkDiskCache>(cache));
        return manager;
    }
};

QNetworkDiskCache* live_cache()
{
    auto& list = caches();
    for (auto it = list.begin(); it != list.end();) {
        if (it->isNull()) {
            it = list.erase(it);
            continue;
        }
        return it->data();
    }
    return nullptr;
}

/// Размер каталога кэша, когда живого QNetworkDiskCache ещё нет (страница
/// настроек открывается раньше первой сетевой картинки).
::std::int64_t dir_size(const QString& path)
{
    if (path.isEmpty())
        return 0;
    ::std::int64_t total = 0;
    QDirIterator it(path, QDir::Files | QDir::NoSymLinks, QDirIterator::Subdirectories);
    while (it.hasNext()) {
        it.next();
        total += it.fileInfo().size();
    }
    return total;
}

}  // namespace

// ── события демона ────────────────────────────────────────────────────────

void rexon_we_register_notify(QObject* notify)
{
    notify_sink() = notify;
    replay_pending(notify, notify_pending());
}

void rexon_we_register_app(QObject* app)
{
    app_sink() = app;
    replay_pending(app, app_pending());
}

int rexon_we_do_deliver_event(const char* data, int len)
{
    const QByteArray frame(data, len);
    int delivered = 0;
    if (deliver_or_queue(notify_sink(), notify_pending(), frame))
        ++delivered;
    if (deliver_or_queue(app_sink(), app_pending(), frame))
        ++delivered;
    return delivered;
}

int rexon_we_do_transport_disconnected()
{
    QObject* target = notify_sink().data();
    if (target == nullptr)
        return 0;
    return QMetaObject::invokeMethod(target, "transportDisconnected",
                                     Qt::DirectConnection)
               ? 1
               : 0;
}

int rexon_we_do_client_error(const char* message, int len)
{
    QObject* target = app_sink().data();
    if (target == nullptr)
        return 0;
    const QString text = QString::fromUtf8(message, len);
    return QMetaObject::invokeMethod(target, "clientError", Qt::DirectConnection,
                                     Q_ARG(QString, text))
               ? 1
               : 0;
}

int rexon_we_do_renderers_replace_frame(const char* frame, int len)
{
    QObject* target = app_sink().data();
    if (target == nullptr)
        return 0;
    return QMetaObject::invokeMethod(target, "replaceRenderers",
                                     Qt::DirectConnection,
                                     Q_ARG(QByteArray, QByteArray(frame, len)))
               ? 1
               : 0;
}

// ── D-Bus ─────────────────────────────────────────────────────────────────

QByteArray rexon_we_dbus_probe()
{
    QJsonObject out;
    out.insert(QStringLiteral("connected"), false);
    out.insert(QStringLiteral("port"), 0);
    out.insert(QStringLiteral("version"), QString());
    out.insert(QStringLiteral("versionMissing"), false);

    QDBusConnection bus = QDBusConnection::sessionBus();
    if (!bus.isConnected())
        return QJsonDocument(out).toJson(QJsonDocument::Compact);

    const QDBusMessage port_reply = call_get(bus, QStringLiteral("WsPort"));
    if (port_reply.type() != QDBusMessage::ReplyMessage)
        return QJsonDocument(out).toJson(QJsonDocument::Compact);
    const auto port_args = port_reply.arguments();
    if (port_args.isEmpty())
        return QJsonDocument(out).toJson(QJsonDocument::Compact);
    bool ok = false;
    const uint port = unwrap_variant(port_args.front()).toUInt(&ok);
    if (!ok || port == 0)
        return QJsonDocument(out).toJson(QJsonDocument::Compact);
    out.insert(QStringLiteral("port"), static_cast<int>(port));

    const QDBusMessage ver_reply = call_get(bus, QStringLiteral("Version"));
    if (ver_reply.type() != QDBusMessage::ReplyMessage) {
        if (is_unknown_property_error(ver_reply.errorName()))
            out.insert(QStringLiteral("versionMissing"), true);
        return QJsonDocument(out).toJson(QJsonDocument::Compact);
    }
    const auto ver_args = ver_reply.arguments();
    const QString version =
        ver_args.isEmpty() ? QString() : unwrap_variant(ver_args.front()).toString();
    if (version.isEmpty()) {
        out.insert(QStringLiteral("versionMissing"), true);
        return QJsonDocument(out).toJson(QJsonDocument::Compact);
    }
    out.insert(QStringLiteral("version"), version);
    out.insert(QStringLiteral("connected"), true);
    return QJsonDocument(out).toJson(QJsonDocument::Compact);
}

int rexon_we_dbus_watch(QObject* receiver)
{
    if (receiver == nullptr)
        return 0;
    QDBusConnection bus = QDBusConnection::sessionBus();
    if (!bus.isConnected())
        return 0;

    int ok = 0;

    // Сторож имени: демон появился на шине или ушёл с неё. Соединение
    // указателем на метод + лямбда с объектом-контекстом — moc здесь не
    // нужен, а он у cpp_files крейта и не запускается.
    auto* watcher = new QDBusServiceWatcher(
        QLatin1String(kBusName), bus,
        QDBusServiceWatcher::WatchForRegistration
            | QDBusServiceWatcher::WatchForUnregistration,
        receiver);
    QObject::connect(watcher, &QDBusServiceWatcher::serviceRegistered, receiver,
                     [receiver](const QString&) {
                         QMetaObject::invokeMethod(receiver, "onDaemonBusChanged",
                                                   Qt::DirectConnection);
                     });
    QObject::connect(watcher, &QDBusServiceWatcher::serviceUnregistered, receiver,
                     [receiver](const QString&) {
                         QMetaObject::invokeMethod(receiver, "onDaemonBusChanged",
                                                   Qt::DirectConnection);
                     });
    ok += 2;

    // Сигналы самого демона. Слот берётся из метаобъекта по имени, а
    // метаобъект Rust-синглтону печатает moc (его cxx-qt-build прогоняет),
    // поэтому инвокабл `onDaemonBusChanged()` тут виден. QtDBus разрешает
    // слоту принимать МЕНЬШЕ аргументов, чем несёт сигнал, — поэтому один
    // безаргументный обработчик подходит всем трём.
    for (const char* signal : {"Ready", "ShuttingDown"}) {
        if (bus.connect(QLatin1String(kBusName), QLatin1String(kObjectPath),
                        QLatin1String(kInterface), QLatin1String(signal), receiver,
                        SLOT(onDaemonBusChanged())))
            ++ok;
    }
    if (bus.connect(QLatin1String(kBusName), QLatin1String(kObjectPath),
                    QLatin1String(kPropsIface), QStringLiteral("PropertiesChanged"),
                    receiver, SLOT(onDaemonBusChanged())))
        ++ok;

    return ok;
}

void rexon_we_do_set_launch_hook(RexonWeLaunchHook hook)
{
    g_launch_hook = hook;
}

bool rexon_we_launch_daemon()
{
    // Пускатель Rexon (если поставлен) знает больше оригинала: он ищет
    // бинарь там, куда его ставит сама вкладка, и на KDE доводит плагин
    // показа. Без него остаётся ветка оригинала.
    if (g_launch_hook != nullptr)
        return g_launch_hook();
    return QProcess::startDetached(QStringLiteral("waywallen"), {});
}

bool rexon_we_kill_process(int pid)
{
    if (pid <= 0)
        return false;
    return ::kill(static_cast<pid_t>(pid), SIGTERM) == 0;
}

// Вызов метода демона без аргументов и без ответа. Возвращает, принят ли
// вызов; отказ уходит в журнал предупреждением.
static bool call_daemon_method(const QString& method)
{
    QDBusConnection bus = QDBusConnection::sessionBus();
    if (!bus.isConnected())
        return false;
    QDBusMessage msg = QDBusMessage::createMethodCall(
        QLatin1String(kBusName), QLatin1String(kObjectPath),
        QLatin1String(kInterface), method);
    const QDBusMessage reply = bus.call(msg, QDBus::Block, 2000);
    if (reply.type() == QDBusMessage::ReplyMessage)
        return true;
    qWarning("демон обоев отклонил вызов %s: %s", qPrintable(method),
             qPrintable(reply.errorMessage()));
    return false;
}

bool rexon_we_quit_daemon()
{
    return call_daemon_method(QStringLiteral("Quit"));
}

bool rexon_we_refresh_displays()
{
    return call_daemon_method(QStringLiteral("RefreshDisplays"));
}

bool rexon_we_settings_bool(const QString& key, bool fallback)
{
    QSettings settings;
    return settings.value(key, fallback).toBool();
}

void rexon_we_set_settings_bool(const QString& key, bool value)
{
    QSettings settings;
    settings.setValue(key, value);
}

// ── сетевой кэш QML ───────────────────────────────────────────────────────

void rexon_we_do_set_network_cache(const char* dir, ::std::int64_t max_bytes)
{
    cache_dir() = QString::fromUtf8(dir);
    if (max_bytes > 0)
        cache_max() = max_bytes;
}

void rexon_we_install_network_cache(QObject* engine)
{
    auto* qml_engine = qobject_cast<QQmlEngine*>(engine);
    if (qml_engine == nullptr)
        return;
    // Фабрика переживает движок: движков вкладки может быть несколько
    // (страница на QQuickWidget), а каталог кэша у них один.
    static RexonNamFactory factory;
    qml_engine->setNetworkAccessManagerFactory(&factory);
}

::std::int64_t rexon_we_network_cache_size()
{
    if (auto* cache = live_cache())
        return cache->cacheSize();
    return dir_size(cache_dir());
}

::std::int64_t rexon_we_network_cache_max()
{
    if (auto* cache = live_cache())
        return cache->maximumCacheSize();
    return cache_max();
}

void rexon_we_set_network_cache_max(::std::int64_t bytes)
{
    if (bytes <= 0)
        return;
    cache_max() = bytes;
    auto& list = caches();
    for (auto it = list.begin(); it != list.end();) {
        if (it->isNull()) {
            it = list.erase(it);
            continue;
        }
        (*it)->setMaximumCacheSize(bytes);
        ++it;
    }
}

void rexon_we_clear_network_cache()
{
    bool cleared = false;
    auto& list = caches();
    for (auto it = list.begin(); it != list.end();) {
        if (it->isNull()) {
            it = list.erase(it);
            continue;
        }
        (*it)->clear();
        cleared = true;
        ++it;
    }
    // Живого кэша может не быть (сетевых картинок ещё не грузили) — тогда
    // чистим каталог сами, иначе кнопка «Очистить» врала бы.
    if (!cleared && !cache_dir().isEmpty())
        QDir(cache_dir()).removeRecursively();
}

// ── язык интерфейса вендорных страниц ─────────────────────────────────────
//
// Порт `ui/src/ui_language.cpp` waywallen. Что чем занято:
//   * ЯДРО (`rexon_core::remote_l10n`) решает, какие языки доступны, к
//     какому коду привести выбор и какой каталог грузить;
//   * МОСТ (`src/we_app.rs`) держит выбор и отдаёт его вендорному QML;
//   * ЗДЕСЬ — только Qt: самоназвание локали, сам QTranslator и пересборка
//     привязок движка.
//
// ⚠️ Осознанное расхождение с оригиналом: зонтичный каталог самого Qt
// (`qt`/`qtbase`) НЕ ставится. У waywallen процесс свой, и переводить в нём
// нечего, кроме его собственных окон; у нас в том же процессе живёт всё
// остальное окно Rexon, и вендорная страница не вправе менять язык
// стандартных кнопок Qt всей программы. Свои строки Rexon берёт из ядра,
// вендорные — из вендорного каталога, и оба слоя от этого не зависят.

namespace {

/// Движки QML вкладки. Их может быть несколько (общий хост страниц и
/// прогревочный движок), а язык у них один.
QList<QPointer<QQmlEngine>>& qml_engines()
{
    static QList<QPointer<QQmlEngine>> engines;
    return engines;
}

/// Живой переводчик вендорных страниц. Ровно один: смена языка снимает
/// прежний и ставит новый, иначе два каталога наложились бы друг на друга.
QTranslator*& ui_translator()
{
    static QTranslator* translator = nullptr;
    return translator;
}

}  // namespace

void rexon_we_do_register_qml_engine(QObject* engine)
{
    auto* qml_engine = qobject_cast<QQmlEngine*>(engine);
    if (qml_engine == nullptr)
        return;
    auto& list = qml_engines();
    for (auto it = list.begin(); it != list.end();) {
        if (it->isNull()) {
            it = list.erase(it);
            continue;
        }
        if (it->data() == qml_engine)
            return;
        ++it;
    }
    list.append(QPointer<QQmlEngine>(qml_engine));
}

QString rexon_we_apply_ui_language(const QString& locale_name, const QString& qm_path,
                                   bool required)
{
    auto* app = QCoreApplication::instance();
    if (app == nullptr)
        return {};

    const QLocale locale = locale_name.isEmpty() ? QLocale::system() : QLocale(locale_name);

    QTranslator* next = nullptr;
    if (!qm_path.isEmpty()) {
        next = new QTranslator();
        if (!next->load(qm_path)) {
            delete next;
            next = nullptr;
            // Каталог обязателен только для явно выбранного языка: у
            // «Системного» и английского его отсутствие — нормальный ход.
            if (required)
                return {};
        }
    } else if (required) {
        return {};
    }

    if (auto* previous = ui_translator()) {
        app->removeTranslator(previous);
        previous->deleteLater();
        ui_translator() = nullptr;
    }
    if (next != nullptr) {
        if (!app->installTranslator(next)) {
            delete next;
            if (required)
                return {};
        } else {
            ui_translator() = next;
        }
    }

    // Пересобрать привязки `qsTr()` уже показанной страницы. Без этого
    // выбор язык бы поменял, а страница осталась бы прежней до перезапуска.
    auto& list = qml_engines();
    for (auto it = list.begin(); it != list.end();) {
        if (it->isNull()) {
            it = list.erase(it);
            continue;
        }
        (*it)->setUiLanguage(locale.bcp47Name());
        (*it)->retranslate();
        ++it;
    }
    return locale.bcp47Name();
}

// ── синглтон Action ───────────────────────────────────────────────────────

namespace {
QPointer<QObject>& action_storage()
{
    static QPointer<QObject> storage;
    return storage;
}
}  // namespace

void rexon_we_action_set_storage(QObject* storage)
{
    action_storage() = storage;
}

QObject* rexon_we_action_storage()
{
    return action_storage().data();
}

void rexon_we_copy_to_clipboard(const QString& text)
{
    if (auto* clipboard = QGuiApplication::clipboard())
        clipboard->setText(text);
}
