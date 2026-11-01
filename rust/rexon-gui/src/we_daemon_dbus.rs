// DaemonDBusClient — QML-синглтон доступности демона обоев по D-Bus.
//
// Порт
// `ui/src/daemon_dbus.cpp` waywallen (MIT, (c) hypengw; атрибуция в NOTICE).
//
// Кого он обслуживает: вендорный `dialog/DaemonNotRunDialog.qml` — диалог
// «демон не запущен» со списком зависших процессов, кнопкой остановки и
// кнопкой запуска.
//
// ⚠️ Главная починка партии: ЖИВЫЕ ПОДПИСКИ. У оригинала их три —
// `QDBusServiceWatcher` на появление/исчезновение имени, сигналы демона
// `Ready`/`ShuttingDown` и `PropertiesChanged`. В нашем Python-мосте не было
// ни одной: состояние обновлялось ТОЛЬКО явным `refreshWsPort()` из QML,
// поэтому диалог не замечал ни запуска демона, ни его остановки — он
// показывал снимок на момент открытия и застывал. Подписки живут в шиме
// (`cpp/rexon_we_singletons.cpp`) и сводятся к одному инвокаблу
// `onDaemonBusChanged()`: что именно изменилось, всё равно выясняет
// повторный опрос.
//
// ⚠️ Осознанная развилка Rexon, унаследованная от Python-моста: ветку
// `VersionMismatch` мы НЕ включаем. Оригинал сравнивает `Version` демона с
// версией СВОЕГО приложения и при расхождении отказывается работать; у
// Rexon свой цикл обновления движка (вкладка ставит демона сама), поэтому
// живой демон с портом считается совместимым. `VersionMissing` остаётся —
// это сборка без свойства `Version`, с ней говорить действительно не о чем.

/// Мост cxx-qt: синглтон `DaemonDBusClient`.
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
        /// Опрос демона по шине: JSON `{connected, port, version,
        /// versionMissing}`.
        fn rexon_we_dbus_probe() -> QByteArray;

        /// Подписаться на события шины. Возвращает число удавшихся подписок.
        ///
        /// # Safety
        /// `receiver` — живой QObject (указатель на самого себя).
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_we_dbus_watch(receiver: *mut QObject) -> i32;

        fn rexon_we_launch_daemon() -> bool;
        fn rexon_we_kill_process(pid: i32) -> bool;

        /// Попросить демона остановиться (метод `Quit` его интерфейса).
        fn rexon_we_quit_daemon() -> bool;

        /// Попросить демона заново опросить дисплеи (метод
        /// `RefreshDisplays` его интерфейса).
        fn rexon_we_refresh_displays() -> bool;

        /// Переключатель «да/нет» в `QSettings` приложения — там же, где
        /// его держит оригинал.
        #[cxx_name = "rexon_we_settings_bool"]
        fn settings_bool(key: &QString, fallback: bool) -> bool;
        #[cxx_name = "rexon_we_set_settings_bool"]
        fn set_settings_bool(key: &QString, value: bool);
    }

    /// Состояние доступности демона. Значения — из оригинала
    /// (`daemon_dbus.cppm`), QML читает их как `DaemonDBusClient.Connected`.
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
        #[cxx_name = "daemonAvailable"]
        fn daemon_available(self: &DaemonDBusClient) -> bool;
        #[cxx_name = "quitOnDaemonShutdown"]
        fn quit_on_daemon_shutdown(self: &DaemonDBusClient) -> bool;
        #[cxx_name = "setQuitOnDaemonShutdown"]
        fn set_quit_on_daemon_shutdown(self: Pin<&mut DaemonDBusClient>, value: bool);
        #[cxx_name = "daemonShutdownExpected"]
        fn daemon_shutdown_expected(self: &DaemonDBusClient) -> bool;

        /// Синхронный опрос демона (порт + версия). Возвращает свежий порт.
        #[qinvokable]
        #[cxx_name = "refreshWsPort"]
        fn refresh_ws_port(self: Pin<&mut DaemonDBusClient>) -> u16;

        /// Запустить демона. Возвращает, удалось ли.
        #[qinvokable]
        #[cxx_name = "launchDaemon"]
        fn launch_daemon(self: Pin<&mut DaemonDBusClient>) -> bool;

        /// Остановить демона его же методом `Quit`. Возвращает, принят ли
        /// вызов; отказ страница «Статус» показывает тостом сама.
        #[qinvokable]
        #[cxx_name = "quitDaemon"]
        fn quit_daemon(self: Pin<&mut DaemonDBusClient>) -> bool;

        /// Заново опросить дисплеи: кнопка «Обновить дисплеи» страницы
        /// «Дисплеи» (`DisplaysPage.qml`). Возвращает, принят ли вызов;
        /// на отказ страница сама показывает тост.
        #[qinvokable]
        #[cxx_name = "refreshDisplays"]
        fn refresh_displays(self: &DaemonDBusClient) -> bool;

        /// Живые процессы демона по /proc: `{pid, cmdline}`.
        #[qinvokable]
        #[cxx_name = "listWaywallenProcesses"]
        fn list_waywallen_processes(self: &DaemonDBusClient) -> QList_QVariant;

        /// SIGTERM зависшему процессу демона.
        #[qinvokable]
        #[cxx_name = "killProcess"]
        fn kill_process(self: &DaemonDBusClient, pid: u32) -> bool;

        /// Шина сообщила, что что-то изменилось (появление/исчезновение
        /// демона, `Ready`, `ShuttingDown`, `PropertiesChanged`). Зовётся
        /// подписками шима — имя обязано совпадать со строкой в `SLOT(...)`.
        #[qinvokable]
        #[cxx_name = "onDaemonBusChanged"]
        fn on_daemon_bus_changed(self: Pin<&mut DaemonDBusClient>);
    }

    impl cxx_qt::Initialize for DaemonDBusClient {}
}

use core::pin::Pin;
use cxx_qt::CxxQtType;
use cxx_qt_lib::{QList, QMap, QMapPair_QString_QVariant, QString, QVariant};

/// Значения перечисления `Status` — те же, что у оригинала.
const STATUS_DISCONNECTED: i32 = 0;
const STATUS_VERSION_MISSING: i32 = 1;
const STATUS_CONNECTED: i32 = 3;

/// Имя процесса демона в `/proc/<pid>/comm` (как в оригинале).
const DAEMON_COMM: &str = "waywallen";

/// Ключ переключателя в `QSettings` — тот же, что у оригинала
/// (`daemon_dbus.cpp: kQuitOnDaemonShutdownKey`).
const QUIT_ON_SHUTDOWN_KEY: &str = "quitOnDaemonShutdown";

/// Значение переключателя по умолчанию — как у оригинала.
const QUIT_ON_SHUTDOWN_DEFAULT: bool = true;

pub struct DaemonDBusClientRust {
    status: i32,
    ws_port: u16,
    version: String,
    /// Переключатель со страницы настроек. Живёт в `QSettings`, как у
    /// оригинала.
    quit_on_daemon_shutdown: bool,
    /// Демона остановили МЫ: диалог «демон не запущен» в этом случае не
    /// показывается (`DaemonNotRunDialog.qml`).
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
        // Подписки ставятся ДО первого опроса: между опросом и подпиской
        // демон мог бы появиться, и мы бы этого не заметили.
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

    fn daemon_available(&self) -> bool {
        self.rust().status == STATUS_CONNECTED
    }

    fn quit_on_daemon_shutdown(&self) -> bool {
        self.rust().quit_on_daemon_shutdown
    }

    /// ⚠️ Осознанная развилка Rexon. У оригинала этот переключатель ещё и
    /// ЗАКРЫВАЕТ приложение, когда демон сообщил `ShuttingDown`: там
    /// приложение — это UI самого waywallen, и без демона ему нечего
    /// показывать. У нас это вкладка внутри Rexon, и закрывать по уходу
    /// демона обоев весь Rexon нельзя. Поэтому здесь остаётся только
    /// хранение значения: переключатель помнит выбор и переживает
    /// перезапуск, а ветку выхода мы не переносим.
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

    /// `set_daemon_shutdown_expected` оригинала: сигнал только на реальном
    /// изменении.
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
        // Демон снова на связи — ждать его остановку больше нечего.
        // У оригинала признак снимает `launchDaemon`, но там опрос и
        // запуск идут одной дорогой; у нас демона может поднять и сам
        // Rexon (`DaemonControl`), и тогда снять признак больше некому.
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

    /// Порт `DaemonDBusClient::quitDaemon`: «ждём остановку» поднимается
    /// ДО вызова и снимается, если демон отказался, — иначе диалог
    /// «демон не запущен» так и не появился бы.
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

    /// `refreshDisplays` оригинала: без связи с демоном — отказ сразу,
    /// вызов по шине не делается.
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

    /// Порт `listWaywallenProcesses`: обход `/proc`, `comm == "waywallen"`.
    ///
    /// Чистый Rust, а не шим: чтение файлов Qt тут не нужнее `std::fs`, а
    /// проверять «это pid» разбором имени каталога — то же самое, что делает
    /// оригинал через `QString::toUInt`.
    fn list_waywallen_processes(&self) -> qobject::QList_QVariant {
        let mut out = QList::<QVariant>::default();
        let Ok(entries) = std::fs::read_dir("/proc") else {
            return out;
        };
        // Порядок обхода /proc произволен — сортируем по pid, иначе список
        // в диалоге прыгал бы между открытиями.
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
            // cmdline — argv через нулевые байты с хвостовым нулём.
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
