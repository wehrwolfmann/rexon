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

// The bus names are taken from the original (ui/src/daemon_dbus.cpp) — the
// same ones as in rexon/qt_wallpaper_engine/client.py.
constexpr const char* kBusName = "org.waywallen.waywallen.Daemon";
constexpr const char* kObjectPath = "/org/waywallen/waywallen/Daemon";
constexpr const char* kInterface = "org.waywallen.waywallen.Daemon1";
constexpr const char* kPropsIface = "org.freedesktop.DBus.Properties";

/// Singletons receiving daemon events. A QPointer rather than a bare
/// pointer: the singleton outlives the QML engine but not the end of the
/// program, and handing an event to a dead object is not allowed.
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

// ── replay cache of the initial package ───────────────────────────────────
//
// The daemon sends the state snapshot EXACTLY ONCE per WebSocket connection
// (display_snapshot, renderer_snapshot, library_snapshot, status_sync,
// playlist_changed — six frames of ~490 bytes) and then stays silent until
// the state changes. A receiver born after this package would stay empty
// FOREVER: the tab showed «No connected displays», and the «Apply» button
// was grey.
//
// Being born later is normal, not an accident: `#[qml_singleton]` is built
// by the QML engine, and the tab sets up the engine lazily, on the first
// visit to a QML page (`window._ensure_qml_host`), while the socket opens
// right when the daemon is ready. Early construction in
// `rexon_do_register_we_types` covers this order, but it rests on one line
// and on `set_client` being called before `client.open()`. The cache
// removes the order dependency itself: frames that arrived before the first
// registration wait for it here and are replayed to the receiver in the
// original order.
//
// EACH receiver has ITS OWN queue: Notify and App are registered by
// different constructors, and a shared queue would double the frames for
// whoever came first on replay.
constexpr int kReplayMaxFrames = 256;
constexpr ::std::int64_t kReplayMaxBytes = 1 << 20;  // 1 MiB

/// A ring buffer with a ceiling: the initial package is six frames of half a
/// kilobyte, so the ceiling here is not the working mode but insurance
/// against unbounded growth if the receiver never appears (the tab was never
/// opened). An overflow throws out the OLDEST frames: fresh ones are closer to
/// the daemon's current state.
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

/// Hand a frame to one receiver or defer it until registration.
///
/// DirectConnection: the frame arrives on the main thread, and the singletons
/// live there too. A queue would only defer the same thing.
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

/// Replay what was accumulated to a new receiver. The queue is emptied
/// BEFORE handing out: the frame handler may get to a new `deliver`, and the
/// replay must not see its own queue a second time.
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
    // The property arrives wrapped in a QDBusVariant — as in the original, we
    // unwrap it, otherwise toUInt() gives zero on a live daemon.
    if (v.canConvert<QDBusVariant>())
        v = v.value<QDBusVariant>().variant();
    return v;
}

bool is_unknown_property_error(const QString& name)
{
    // Different bus implementations name "no such property" differently — we
    // accept both names (the original's branch).
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

// ── QML network cache ─────────────────────────────────────────────────────

QString& cache_dir()
{
    static QString dir;
    return dir;
}

::std::int64_t& cache_max()
{
    // The original's default (app.cpp: 1 GiB). It is overridden anyway by the
    // very first setNetworkCacheMaximumSize call from Global.qml.
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
        // We keep a weak reference: QML asks App for the size and the cleanup,
        // and it is precisely the live cache that computes them — walking the
        // directory would give a different figure.
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

/// The size of the cache directory when there is no live QNetworkDiskCache
/// yet (the settings page opens before the first network picture).
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

// ── daemon events ─────────────────────────────────────────────────────────

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

    // Name watcher: the daemon appeared on the bus or left it. A connection by
    // a method pointer + a lambda with a context object — moc is not needed
    // here, and it is not run for the crate's cpp_files anyway.
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

    // Signals of the daemon itself. The slot is taken from the metaobject by
    // name, and the Rust singleton's metaobject is emitted by moc (cxx-qt-build
    // runs it), so the `onDaemonBusChanged()` invokable is visible here. QtDBus
    // allows a slot to take FEWER arguments than the signal carries — so one
    // argument-less handler suits all three.
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
    // The Rexon launcher (if set) knows more than the original: it looks for
    // the binary where the tab itself installs it, and on KDE it finishes the
    // display plugin. Without it the original's branch remains.
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

// Calling a daemon method without arguments and without an answer. Returns
// whether the call was accepted; a refusal goes into the log as a warning.
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

// ── QML network cache ─────────────────────────────────────────────────────

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
    // The factory outlives the engine: the tab may have several engines (a page
    // on a QQuickWidget), while their cache directory is one.
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
    // There may be no live cache (no network pictures have been loaded yet) —
    // then we clean the directory ourselves, otherwise the «Clear» button
    // would lie.
    if (!cleared && !cache_dir().isEmpty())
        QDir(cache_dir()).removeRecursively();
}

// ── interface language of the vendored pages ──────────────────────────────
//
// A port of waywallen's `ui/src/ui_language.cpp`. Who does what:
//   * THE CORE (`rexon_core::remote_l10n`) decides which languages are
//     available, what code to bring the choice to and which catalogue to
//     load;
//   * THE BRIDGE (`src/we_app.rs`) holds the choice and gives it to the
//     vendored QML;
//   * HERE — only Qt: the locale's own name, the QTranslator itself and
//     re-evaluating the engine's bindings.
//
// Warning: a deliberate divergence from the original: the umbrella catalogue
// of Qt itself (`qt`/`qtbase`) is NOT installed. waywallen has its own
// process, and there is nothing to translate in it except its own windows;
// in our process lives the whole rest of the Rexon window, and a vendored
// page has no right to change the language of Qt's standard buttons for the
// whole program. Rexon takes its own strings from the core, the vendored
// ones from the vendor catalogue, and neither layer depends on this.

namespace {

/// The tab's QML engines. There may be several of them (the shared page host
/// and the warm-up engine), while their language is one.
QList<QPointer<QQmlEngine>>& qml_engines()
{
    static QList<QPointer<QQmlEngine>> engines;
    return engines;
}

/// The live translator of the vendored pages. Exactly one: a language change
/// removes the previous one and installs a new one, otherwise two catalogues
/// would overlap.
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
            // The catalogue is mandatory only for an explicitly chosen language: for
            // «System» and English its absence is a normal course.
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

    // Re-evaluate the `qsTr()` bindings of an already shown page. Without this
    // the choice would change the language, but the page would stay as before
    // until a restart.
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

// ── the Action singleton ──────────────────────────────────────────────────

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
