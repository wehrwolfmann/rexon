// DaemonDBusClient — the QML singleton of the wallpaper daemon's
// availability over D-Bus.
//
// A port of waywallen's `ui/src/daemon_dbus.cpp` (MIT, (c) hypengw;
// attribution in NOTICE).
//
// Whom it serves: the vendored `dialog/DaemonNotRunDialog.qml` — the
// «daemon is not running» dialog with a list of hung processes, a stop
// button and a launch button.
//
// Warning: the main fix of the batch: LIVE SUBSCRIPTIONS. The original has
// three — a `QDBusServiceWatcher` on the name appearing/disappearing, the
// daemon's `Ready`/`ShuttingDown` signals and `PropertiesChanged`. Our
// Python bridge had none: the state was updated ONLY by an explicit
// `refreshWsPort()` from QML, so the dialog noticed neither the daemon
// starting nor it stopping — it showed a snapshot at the moment of opening
// and froze. The subscriptions live in the shim
// (`cpp/rexon_we_singletons.cpp`) and come down to one
// `onDaemonBusChanged()` invokable: what exactly changed is found out by a
// repeated poll anyway.
//
// Warning: a deliberate Rexon divergence inherited from the Python bridge:
// we do NOT enable the `VersionMismatch` branch. The original compares the
// daemon's `Version` with the version of ITS OWN application and refuses to
// work on a mismatch; Rexon has its own engine update cycle (the tab
// installs the daemon itself), so a live daemon with a port is considered
// compatible. `VersionMissing` stays — that is a build without the
// `Version` property, and there is really nothing to talk to it about.

/// cxx-qt bridge: the `DaemonDBusClient` singleton.
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qbytearray.h");
        type QByteArray = cxx_qt_lib::QByteArray;

        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;

        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;

        include!("cxx-qt-lib/qlist.h");
        type QList_QVariant = cxx_qt_lib::QList<cxx_qt_lib::QVariant>;

        include!("cxx-qt-lib/qmap.h");
        type QMap_QString_QVariant = cxx_qt_lib::QMap<cxx_qt_lib::QMapPair_QString_QVariant>;
    }

    unsafe extern "C++" {
        include!("rexon_we_singletons.h");
        /// Polling the daemon over the bus: JSON `{connected, port, version,
        /// versionMissing}`.
        fn rexon_we_dbus_probe() -> QByteArray;

        /// Subscribe to bus events. Returns the number of successful subscriptions.
        ///
        /// # Safety
        /// `receiver` is a live QObject (a pointer to itself).
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_we_dbus_watch(receiver: *mut QObject) -> i32;

        fn rexon_we_launch_daemon() -> bool;
        fn rexon_we_kill_process(pid: i32) -> bool;

        /// Ask the daemon to stop (the `Quit` method of its interface).
        fn rexon_we_quit_daemon() -> bool;

        /// Ask the daemon to poll the displays again (the `RefreshDisplays`
        /// method of its interface).
        fn rexon_we_refresh_displays() -> bool;

        /// A yes/no toggle in the application's `QSettings` — in the same place
        /// the original keeps it.
        #[cxx_name = "rexon_we_settings_bool"]
        fn settings_bool(key: &QString, fallback: bool) -> bool;
        #[cxx_name = "rexon_we_set_settings_bool"]
        fn set_settings_bool(key: &QString, value: bool);
    }

    /// The daemon availability state. The values come from the original
    /// (`daemon_dbus.cppm`), QML reads them as `DaemonDBusClient.Connected`.
    #[qenum(DaemonDBusClient)]
    enum Status {
        Disconnected,
        VersionMissing,
        VersionMismatch,
        Connected,
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qml_singleton]
        #[qproperty(i32, status, READ = status, NOTIFY = status_changed)]
        #[qproperty(u16, ws_port, cxx_name = "wsPort",
                    READ = ws_port, NOTIFY = ws_port_changed)]
        #[qproperty(QString, daemon_version, cxx_name = "daemonVersion",
                    READ = daemon_version, NOTIFY = status_changed)]
        /// waywallen 0.6.2: the daemon release against the vendored interface:
        /// < 0 older, > 0 newer, 0 the same or unknown.
        #[qproperty(i32, daemon_version_order, cxx_name = "daemonVersionOrder",
                    READ = daemon_version_order, NOTIFY = status_changed)]
        #[qproperty(bool, daemon_available, cxx_name = "daemonAvailable",
                    READ = daemon_available, NOTIFY = status_changed)]
        #[qproperty(bool, quit_on_daemon_shutdown, cxx_name = "quitOnDaemonShutdown",
                    READ = quit_on_daemon_shutdown, WRITE = set_quit_on_daemon_shutdown,
                    NOTIFY = quit_on_daemon_shutdown_changed)]
        #[qproperty(bool, daemon_shutdown_expected, cxx_name = "daemonShutdownExpected",
                    READ = daemon_shutdown_expected,
                    NOTIFY = daemon_shutdown_expected_changed)]
        type DaemonDBusClient = super::DaemonDBusClientRust;
    }

    unsafe extern "RustQt" {
        #[qsignal]
        #[cxx_name = "statusChanged"]
        fn status_changed(self: Pin<&mut DaemonDBusClient>);

        #[qsignal]
        #[cxx_name = "wsPortChanged"]
        fn ws_port_changed(self: Pin<&mut DaemonDBusClient>, port: u16);

        #[qsignal]
        #[cxx_name = "quitOnDaemonShutdownChanged"]
        fn quit_on_daemon_shutdown_changed(self: Pin<&mut DaemonDBusClient>);

        #[qsignal]
        #[cxx_name = "daemonShutdownExpectedChanged"]
        fn daemon_shutdown_expected_changed(self: Pin<&mut DaemonDBusClient>);
    }

    extern "RustQt" {
        fn status(self: &DaemonDBusClient) -> i32;
        #[cxx_name = "wsPort"]
        fn ws_port(self: &DaemonDBusClient) -> u16;
        #[cxx_name = "daemonVersion"]
        fn daemon_version(self: &DaemonDBusClient) -> QString;
        #[cxx_name = "daemonVersionOrder"]
        fn daemon_version_order(self: &DaemonDBusClient) -> i32;
        #[cxx_name = "daemonAvailable"]
        fn daemon_available(self: &DaemonDBusClient) -> bool;
        #[cxx_name = "quitOnDaemonShutdown"]
        fn quit_on_daemon_shutdown(self: &DaemonDBusClient) -> bool;
        #[cxx_name = "setQuitOnDaemonShutdown"]
        fn set_quit_on_daemon_shutdown(self: Pin<&mut DaemonDBusClient>, value: bool);
        #[cxx_name = "daemonShutdownExpected"]
        fn daemon_shutdown_expected(self: &DaemonDBusClient) -> bool;

        /// A synchronous poll of the daemon (port + version). Returns the fresh port.
        #[qinvokable]
        #[cxx_name = "refreshWsPort"]
        fn refresh_ws_port(self: Pin<&mut DaemonDBusClient>) -> u16;

        /// Launch the daemon. Returns whether it worked.
        #[qinvokable]
        #[cxx_name = "launchDaemon"]
        fn launch_daemon(self: Pin<&mut DaemonDBusClient>) -> bool;

        /// Stop the daemon with its own `Quit` method. Returns whether the call
        /// was accepted; the «Status» page shows a refusal as a toast itself.
        #[qinvokable]
        #[cxx_name = "quitDaemon"]
        fn quit_daemon(self: Pin<&mut DaemonDBusClient>) -> bool;

        /// Poll the displays again: the «Refresh displays» button of the
        /// «Displays» page (`DisplaysPage.qml`). Returns whether the call was
        /// accepted; on a refusal the page shows a toast itself.
        #[qinvokable]
        #[cxx_name = "refreshDisplays"]
        fn refresh_displays(self: &DaemonDBusClient) -> bool;

        /// Live daemon processes by /proc: `{pid, cmdline}`.
        #[qinvokable]
        #[cxx_name = "listWaywallenProcesses"]
        fn list_waywallen_processes(self: &DaemonDBusClient) -> QList_QVariant;

        /// SIGTERM to a hung daemon process.
        #[qinvokable]
        #[cxx_name = "killProcess"]
        fn kill_process(self: &DaemonDBusClient, pid: u32) -> bool;

        /// The bus reported that something changed (the daemon appearing or
        /// disappearing, `Ready`, `ShuttingDown`, `PropertiesChanged`). Called
        /// by the shim's subscriptions — the name must match the string in
        /// `SLOT(...)`.
        #[qinvokable]
        #[cxx_name = "onDaemonBusChanged"]
        fn on_daemon_bus_changed(self: Pin<&mut DaemonDBusClient>);
    }

    impl cxx_qt::Initialize for DaemonDBusClient {}
}

use core::pin::Pin;
use cxx_qt::CxxQtType;
use cxx_qt_lib::{QList, QMap, QMapPair_QString_QVariant, QString, QVariant};

/// Values of the `Status` enum — the same as in the original.
const STATUS_DISCONNECTED: i32 = 0;
const STATUS_VERSION_MISSING: i32 = 1;
const STATUS_CONNECTED: i32 = 3;

/// The daemon process name in `/proc/<pid>/comm` (as in the original).
const DAEMON_COMM: &str = "waywallen";

/// The toggle key in `QSettings` — the same as in the original
/// (`daemon_dbus.cpp: kQuitOnDaemonShutdownKey`).
const QUIT_ON_SHUTDOWN_KEY: &str = "quitOnDaemonShutdown";

/// The toggle's default value — as in the original.
const QUIT_ON_SHUTDOWN_DEFAULT: bool = true;

pub struct DaemonDBusClientRust {
    status: i32,
    ws_port: u16,
    version: String,
    /// The toggle from the settings page. It lives in `QSettings`, as in the
    /// original.
    quit_on_daemon_shutdown: bool,
    /// WE stopped the daemon: the «daemon is not running» dialog is not shown
    /// in this case (`DaemonNotRunDialog.qml`).
    daemon_shutdown_expected: bool,
}

impl Default for DaemonDBusClientRust {
    fn default() -> Self {
        Self {
            status: 0,
            ws_port: 0,
            version: String::new(),
            quit_on_daemon_shutdown: QUIT_ON_SHUTDOWN_DEFAULT,
            daemon_shutdown_expected: false,
        }
    }
}

impl cxx_qt::Initialize for qobject::DaemonDBusClient {
    fn initialize(mut self: Pin<&mut Self>) {
        let this: *mut qobject::DaemonDBusClient =
            unsafe { self.as_mut().get_unchecked_mut() };
        // The subscriptions are set up BEFORE the first poll: between the poll
        // and the subscription the daemon could appear, and we would not notice.
        unsafe { qobject::rexon_we_dbus_watch(this.cast()) };
        let stored = qobject::settings_bool(
            &QString::from(QUIT_ON_SHUTDOWN_KEY),
            QUIT_ON_SHUTDOWN_DEFAULT,
        );
        self.as_mut().rust_mut().quit_on_daemon_shutdown = stored;
        self.refresh_ws_port();
    }
}

impl qobject::DaemonDBusClient {
    fn status(&self) -> i32 {
        self.rust().status
    }

    fn ws_port(&self) -> u16 {
        self.rust().ws_port
    }

    fn daemon_version(&self) -> QString {
        QString::from(&self.rust().version)
    }

    fn daemon_version_order(&self) -> i32 {
        let module_dir = rexon_core::remote_l10n::ui_module_dir(&crate::we_env::pkg_dir());
        let ui = crate::we_app::vendored_ui_version(&module_dir);
        rexon_core::we_util::version_order(&self.rust().version, &ui)
    }

    fn daemon_available(&self) -> bool {
        self.rust().status == STATUS_CONNECTED
    }

    fn quit_on_daemon_shutdown(&self) -> bool {
        self.rust().quit_on_daemon_shutdown
    }

    /// Warning: a deliberate Rexon divergence. In the original this toggle
    /// also CLOSES the application when the daemon reported `ShuttingDown`:
    /// there the application is waywallen's own UI, and without the daemon it
    /// has nothing to show. For us this is a tab inside Rexon, and closing the
    /// whole of Rexon because the wallpaper daemon left is not allowed. So
    /// only storing the value remains here: the toggle remembers the choice
    /// and survives a restart, while the exit branch is not ported.
    fn set_quit_on_daemon_shutdown(mut self: Pin<&mut Self>, value: bool) {
        if self.rust().quit_on_daemon_shutdown == value {
            return;
        }
        self.as_mut().rust_mut().quit_on_daemon_shutdown = value;
        qobject::set_settings_bool(&QString::from(QUIT_ON_SHUTDOWN_KEY), value);
        self.quit_on_daemon_shutdown_changed();
    }

    fn daemon_shutdown_expected(&self) -> bool {
        self.rust().daemon_shutdown_expected
    }

    /// The original's `set_daemon_shutdown_expected`: a signal only on a real
    /// change.
    fn set_daemon_shutdown_expected(mut self: Pin<&mut Self>, value: bool) {
        if self.rust().daemon_shutdown_expected == value {
            return;
        }
        self.as_mut().rust_mut().daemon_shutdown_expected = value;
        self.daemon_shutdown_expected_changed();
    }

    fn refresh_ws_port(mut self: Pin<&mut Self>) -> u16 {
        let reply = qobject::rexon_we_dbus_probe();
        let probe: serde_json::Value =
            serde_json::from_slice(reply.as_slice()).unwrap_or(serde_json::Value::Null);
        let flag = |key: &str| probe.get(key).and_then(|v| v.as_bool()).unwrap_or(false);
        let port = probe
            .get("port")
            .and_then(|v| v.as_u64())
            .unwrap_or(0)
            .min(u16::MAX as u64) as u16;
        let version = probe
            .get("version")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let status = if !flag("connected") {
            if flag("versionMissing") && port != 0 {
                STATUS_VERSION_MISSING
            } else {
                STATUS_DISCONNECTED
            }
        } else {
            STATUS_CONNECTED
        };

        if self.rust().ws_port != port {
            self.as_mut().rust_mut().ws_port = port;
            self.as_mut().ws_port_changed(port);
        }
        if self.rust().status != status || self.rust().version != version {
            {
                let mut rust = self.as_mut().rust_mut();
                rust.status = status;
                rust.version = version;
            }
            self.as_mut().status_changed();
        }
        // The daemon is back in touch — there is no stop to wait for any more.
        // In the original the flag is cleared by `launchDaemon`, but there the
        // poll and the launch go one way; for us the daemon may also be brought
        // up by Rexon itself (`DaemonControl`), and then nobody else would clear
        // the flag.
        if status == STATUS_CONNECTED {
            self.as_mut().set_daemon_shutdown_expected(false);
        }
        self.rust().ws_port
    }

    fn on_daemon_bus_changed(self: Pin<&mut Self>) {
        self.refresh_ws_port();
    }

    fn launch_daemon(mut self: Pin<&mut Self>) -> bool {
        self.as_mut().set_daemon_shutdown_expected(false);
        qobject::rexon_we_launch_daemon()
    }

    /// A port of `DaemonDBusClient::quitDaemon`: "waiting for the stop" is
    /// raised BEFORE the call and cleared if the daemon refused — otherwise the
    /// «daemon is not running» dialog would never appear.
    fn quit_daemon(mut self: Pin<&mut Self>) -> bool {
        if self.rust().status != STATUS_CONNECTED {
            return false;
        }
        self.as_mut().set_daemon_shutdown_expected(true);
        if qobject::rexon_we_quit_daemon() {
            return true;
        }
        self.set_daemon_shutdown_expected(false);
        false
    }

    /// The original's `refreshDisplays`: without a connection to the daemon —
    /// a refusal at once, no call over the bus is made.
    fn refresh_displays(&self) -> bool {
        if self.rust().status != STATUS_CONNECTED {
            return false;
        }
        qobject::rexon_we_refresh_displays()
    }

    fn kill_process(&self, pid: u32) -> bool {
        if pid == 0 {
            return false;
        }
        qobject::rexon_we_kill_process(pid as i32)
    }

    /// A port of `listWaywallenProcesses`: walking `/proc`,
    /// `comm == "waywallen"`.
    ///
    /// Plain Rust rather than a shim: Qt is no more needed for reading files
    /// here than `std::fs`, and checking "is this a pid" by parsing the
    /// directory name is the same thing the original does through
    /// `QString::toUInt`.
    fn list_waywallen_processes(&self) -> qobject::QList_QVariant {
        let mut out = QList::<QVariant>::default();
        let Ok(entries) = std::fs::read_dir("/proc") else {
            return out;
        };
        // The /proc walk order is arbitrary — we sort by pid, otherwise the list
        // in the dialog would jump between openings.
        let mut found: Vec<(u32, String)> = Vec::new();
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            let Ok(pid) = name.parse::<u32>() else {
                continue;
            };
            let Ok(comm) = std::fs::read_to_string(format!("/proc/{pid}/comm")) else {
                continue;
            };
            if comm.trim() != DAEMON_COMM {
                continue;
            }
            // cmdline is argv separated by null bytes with a trailing null.
            let cmdline = std::fs::read(format!("/proc/{pid}/cmdline"))
                .map(|raw| {
                    String::from_utf8_lossy(&raw)
                        .replace('\0', " ")
                        .trim()
                        .to_string()
                })
                .unwrap_or_default();
            let cmdline = if cmdline.is_empty() {
                DAEMON_COMM.to_string()
            } else {
                cmdline
            };
            found.push((pid, cmdline));
        }
        found.sort_by_key(|(pid, _)| *pid);
        for (pid, cmdline) in found {
            let mut map = QMap::<QMapPair_QString_QVariant>::default();
            map.insert(QString::from("pid"), QVariant::from(&(pid as i32)));
            map.insert(
                QString::from("cmdline"),
                QVariant::from(&QString::from(&cmdline)),
            );
            out.append(QVariant::from(&map));
        }
        out
    }
}
