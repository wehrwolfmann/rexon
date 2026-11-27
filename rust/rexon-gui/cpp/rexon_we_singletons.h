// Qt services of the Wallpaper Engine tab's singletons that exist neither in
// cxx-qt-lib nor in the core: the D-Bus bus, the QML network cache,
// broadcasting daemon events to the live singletons and packing a QObject*
// into a QVariant.
//
// Why a shim and not Rust: QtDBus, QNetworkDiskCache and
// QQmlNetworkAccessManagerFactory have no wrappers in cxx-qt-lib 0.9 at
// all, and a QVariant from cxx-qt-lib cannot hold a pointer to a QObject —
// exactly the kind of value with which the vendored QML reads the manager
// lists (`Repeater { model: App.rendererManager.renderers }`).
//
// The Notify/App singletons are different cxx-qt types; the daemon frame,
// the disconnection and the error are handed to them by calling an
// invokable by NAME through the metaobject (`QMetaObject::invokeMethod`),
// and they are held by a QPointer — cxx-qt-lib has neither. There are no
// decisions here: what to hand out and when is decided by Rust
// (src/we_socket.rs, src/we_singletons.rs).
//
// Why ordinary functions and not a QObject with signals: the crate's
// cpp_files do not go through moc (cxx-qt-build builds them as is). Only the
// crate itself calls them, through `extern "C"` (src/we_singletons.rs); it
// does not export C symbols from the .so.
#pragma once

#include <QtCore/QtGlobal>

#include <cstdint>

class QByteArray;
class QObject;
class QString;

extern "C" {

// ── daemon events → live singletons ───────────────────────────────────────

/// Record a singleton to which daemon frames are handed out. The pointers
/// are held by a QPointer, so the record does not need to be removed — a dead
/// object drops out by itself. There are exactly two receivers, and each has
/// its own share of work: Notify computes the state and broadcasts signals,
/// App feeds its live models (renderers/displays/libraries).
///
/// Registration REPLAYS to the receiver everything that arrived before it
/// (see the replay cache in the .cpp): the daemon gives the state snapshot
/// once per WebSocket connection, while the singletons are built by the QML
/// engine, which the tab sets up lazily.
void rexon_we_register_notify(QObject* notify);
void rexon_we_register_app(QObject* app);

/// Hand the RAW `ServerFrame` frame to both receivers (the
/// `deliverEvent(QByteArray)` invokable). Returns how many it reached RIGHT
/// NOW; a frame for a not yet registered receiver is not lost but goes into
/// the replay cache and will be delivered to it on registration.
///
/// The Rust wrapper is `we_singletons::deliver_singleton_event_bytes`.
int rexon_we_do_deliver_event(const char* data, int len);

/// Tell Notify that the WebSocket was disconnected (the
/// `transportDisconnected()` invokable). Returns 1 if it got through.
int rexon_we_do_transport_disconnected();

/// Transport error → a window toast (the `clientError(QString)` invokable of
/// the App singleton). Called by `we_socket::report_error`; 0 — there is no
/// App yet.
int rexon_we_do_client_error(const char* message, int len);

// ── D-Bus: daemon availability ────────────────────────────────────────────

/// A synchronous poll of the daemon over the bus: `Get(WsPort)` and
/// `Get(Version)`.
///
/// Returns JSON `{"connected":bool,"port":int,"version":str,
/// "versionMissing":bool}` — a map rather than a set of out parameters:
/// mutable references to Qt types do not cross cxx, and QByteArray is already
/// used by the neighbouring shim for exactly this.
QByteArray rexon_we_dbus_probe();

/// Subscribe `receiver` to the daemon appearing on/disappearing from the bus
/// and to its `Ready`/`ShuttingDown`/`PropertiesChanged` signals. All of them
/// come down to one `onDaemonBusChanged()` invokable — what exactly changed
/// is found out by a repeated poll anyway.
///
/// Returns the number of successful subscriptions (0 — there is no bus at
/// all).
///
/// Our Python had no live subscriptions AT ALL: the daemon status was updated
/// only by an explicit `refreshWsPort()` from QML, so the "daemon is not
/// running" dialog noticed neither the daemon starting nor it stopping. The
/// original (`ui/src/daemon_dbus.cpp`) has subscriptions.
int rexon_we_dbus_watch(QObject* receiver);

/// Launch the daemon. By default — as in the original,
/// `QProcess::startDetached("waywallen")`; if the Rexon launcher is set
/// (`we_socket::launch_daemon_hook`, it also installs the KDE display
/// plugin), it is called.
bool rexon_we_launch_daemon();

/// The daemon launcher callback (set by Rust: `we_singletons::set_launch_hook`).
typedef bool (*RexonWeLaunchHook)();
void rexon_we_do_set_launch_hook(RexonWeLaunchHook hook);

/// SIGTERM to a process. A separate function because the crate has no `libc`
/// dependency, and pulling it in for one call costs more than three lines of
/// C++.
bool rexon_we_kill_process(int pid);

/// Ask the daemon to stop — the `Quit` method of its interface (a port of
/// `DaemonDBusClient::quitDaemon`). Returns whether the call was accepted.
bool rexon_we_quit_daemon();

/// Ask the daemon to poll the displays again — the `RefreshDisplays` method
/// of its interface: in response the daemon broadcasts `Ready`, and the
/// display clients reconnect. Returns whether the call was accepted.
bool rexon_we_refresh_displays();

// ── QML network cache ─────────────────────────────────────────────────────

/// Give the QML engine a network manager factory with a disk cache.
/// `engine` is a pointer to a QQmlEngine (a checked cast inside).
void rexon_we_install_network_cache(QObject* engine);

/// Set the cache directory and limit before installing the factory (called
/// by `we_env::prepare_engine`; the path is computed by the core —
/// `qml_modules::net_cache_dir`).
void rexon_we_do_set_network_cache(const char* dir, ::std::int64_t max_bytes);

// ── interface language of the vendored pages ──────────────────────────────
//
// A port of waywallen's `ui/src/ui_language.cpp`: the «Language» combo box
// on the settings page. The decisions (which languages are available, what
// to load) are made by the core (`rexon_core::remote_l10n`), and the
// language itself is shared with Rexon (`ui_language`); here there is only
// what Rust lacks: the QTranslator and re-evaluating the engine's bindings.

/// Remember the tab's QML engine: without it a language change would not
/// re-evaluate the `qsTr()` bindings of an already shown page. The pointer
/// is under a QPointer — the tab may recreate the engine.
void rexon_we_do_register_qml_engine(QObject* engine);

/// Install the vendored pages' catalogue and re-evaluate the QML bindings.
///
/// The language is chosen by Rexon (`ui_language`), and the list labels are
/// its too (`qt_settings::language_choices`): the wallpaper tab no longer has
/// its own language choice, its own «System» labels or its own `QSettings`
/// key.
///
/// `locale_name` is a Qt locale name (`ru`, `zh_CN`); empty — the system one.
/// `qm_path` is the path of the built catalogue; empty — the source (English)
/// strings. `required` — count the change as failed without a catalogue.
///
/// Returns the resolved language in BCP 47 form (`ru`, `zh-CN`) — the same
/// one the original gives to plugin labels. Empty — the language was not
/// applied.
QString rexon_we_apply_ui_language(const QString& locale_name, const QString& qm_path,
                                   bool required);

::std::int64_t rexon_we_network_cache_size();
::std::int64_t rexon_we_network_cache_max();
void rexon_we_set_network_cache_max(::std::int64_t bytes);
void rexon_we_clear_network_cache();
}

// ── the Action singleton ──────────────────────────────────────────────────

/// The wallpaper selection storage under a QPointer. In the original
/// (`ui/src/action.cppm`) this is a `QPointer<QObject>` field; here it lives
/// in the shim, because cxx-qt-lib gives no QPointer wrapper, and a bare
/// pointer to a QML page's object dangles after the page is closed.
void rexon_we_action_set_storage(QObject* storage);
QObject* rexon_we_action_storage();

/// Text into the system clipboard (a port of `Action::copyToClipboard`).
void rexon_we_copy_to_clipboard(const QString& text);

// ── tab settings in QSettings ─────────────────────────────────────────────

/// A yes/no setting in the application's `QSettings`.
///
/// A generic pair rather than a pair for one key: the original keeps other
/// tab toggles in `QSettings` too, and the second one will need exactly the
/// same (rule #12).
bool rexon_we_settings_bool(const QString& key, bool fallback);
void rexon_we_set_settings_bool(const QString& key, bool value);

// ── the live renderer list outside of events ──────────────────────────────

/// A full replacement of the renderer list from the `renderer_list` answer.
///
/// `RendererListQuery` (status_queries.rs) and the live model
/// `App.rendererManager` are different cxx-qt bridges, and the query can
/// reach the model only through the shim. In the original this
/// synchronization is done by the query itself
/// (`RendererListQuery::reload` → `RendererManager::replaceAll`), and it
/// must not be lost: without it the renderer list is updated only by daemon
/// events.
///
/// `frame` is the RAW frame of the `renderer_list` answer (the same one the
/// query parsed).
extern "C" {
// C linkage: the declaration is called by Rust with an ordinary
// `extern "C"` block (we_singletons.rs), not with a cxx bridge — without
// this wrapper the name would go into the table mangled, and the build
// would not link.
int rexon_we_do_renderers_replace_frame(const char* frame, int len);
}
