// Starting the engine at login.
//
// Warning: a DELIBERATE difference from the original, not a defect: at the
// vendor (`ui/src/query/autostart_query.cpp` @e5626c1) autostart is an RPC
// to the daemon (`autostart_get`/`autostart_set`), and it works only in
// Flatpak through the Background portal. In Rexon it is routed LOCALLY: a
// systemd user unit with a fallback to XDG autostart (roadmap item
// 0-ter). No frames go to the daemon at all, so these two queries have
// neither `sendRequest` nor a `deliverResponse` branch with a frame — only
// the base signals.
//
// The QML contract is THE SAME as the original's: `enabled`, `status`,
// `error`, `reload()`. SettingsPage re-reads the state on `status == 2` and
// shows a toast with `.error` on `status == 3`. With waywallen#380
// (14.11.2026) `supported` and `unavailableReason` were added to the
// contract (+ the `availabilityChanged` signal): the switch is grey while
// `supported` is false, and the reason is a hint at the label. Upstream,
// without the daemon's answer `supported` stays false; for us the answer
// is local ([`autostart_state_on`]) and exists from the moment the object
// is created, so the switch is live even with the daemon down.
//
// Decisions (which mechanism, which files, which argv, which messages) are
// entirely in the core (`rexon_core::we_client`); here there is only
// execution: starting processes and writing files. Exactly the same
// boundary as `client.py` had, only the executor is no longer Python:
// since 07.09.2026 the Python `autostart_enabled`/`set_autostart` no longer
// exist, and this module is the ONLY place where the engine's autostart is
// edited on disk.

/// cxx-qt bridge: reading and writing autostart.
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;

        include!("cxx-qt-lib/qbytearray.h");
        type QByteArray = cxx_qt_lib::QByteArray;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(i32, status, READ = status, NOTIFY = status_changed)]
        #[qproperty(QString, error, READ = error, NOTIFY = error_changed)]
        #[qproperty(bool, querying, READ = querying, NOTIFY = querying_changed)]
        #[qproperty(bool, forward_error, cxx_name = "forwardError", READ = forward_error,
                    WRITE = set_forward_error, NOTIFY = forward_error_changed)]
        #[qproperty(bool, enabled, READ = enabled, NOTIFY = enabled_changed)]
        #[qproperty(bool, supported, READ = supported, NOTIFY = availability_changed)]
        #[qproperty(QString, unavailable_reason, cxx_name = "unavailableReason",
                    READ = unavailable_reason, NOTIFY = availability_changed)]
        type AutostartGetQuery = super::AutostartGetQueryRust;

        #[qobject]
        #[qml_element]
        #[qproperty(i32, status, READ = status, NOTIFY = status_changed)]
        #[qproperty(QString, error, READ = error, NOTIFY = error_changed)]
        #[qproperty(bool, querying, READ = querying, NOTIFY = querying_changed)]
        #[qproperty(bool, forward_error, cxx_name = "forwardError", READ = forward_error,
                    WRITE = set_forward_error, NOTIFY = forward_error_changed)]
        #[qproperty(bool, enabled, READ = enabled, WRITE = set_enabled,
                    NOTIFY = enabled_changed)]
        type AutostartSetQuery = super::AutostartSetQueryRust;
    }

    unsafe extern "RustQt" {
        #[qsignal]
        #[cxx_name = "statusChanged"]
        fn status_changed(self: Pin<&mut AutostartGetQuery>, status: i32);
        #[qsignal]
        #[cxx_name = "queryingChanged"]
        fn querying_changed(self: Pin<&mut AutostartGetQuery>);
        #[qsignal]
        #[cxx_name = "errorChanged"]
        fn error_changed(self: Pin<&mut AutostartGetQuery>);
        #[qsignal]
        #[cxx_name = "forwardErrorChanged"]
        fn forward_error_changed(self: Pin<&mut AutostartGetQuery>);
        #[qsignal]
        fn finished(self: Pin<&mut AutostartGetQuery>);
        #[qsignal]
        #[cxx_name = "errorOccurred"]
        fn error_occurred(self: Pin<&mut AutostartGetQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "appErrorOccurred"]
        fn app_error_occurred(self: Pin<&mut AutostartGetQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "sendRequest"]
        fn send_request(self: Pin<&mut AutostartGetQuery>, token: i64, field: QString,
                        request: QByteArray);

        #[qsignal]
        #[cxx_name = "statusChanged"]
        fn status_changed(self: Pin<&mut AutostartSetQuery>, status: i32);
        #[qsignal]
        #[cxx_name = "queryingChanged"]
        fn querying_changed(self: Pin<&mut AutostartSetQuery>);
        #[qsignal]
        #[cxx_name = "errorChanged"]
        fn error_changed(self: Pin<&mut AutostartSetQuery>);
        #[qsignal]
        #[cxx_name = "forwardErrorChanged"]
        fn forward_error_changed(self: Pin<&mut AutostartSetQuery>);
        #[qsignal]
        fn finished(self: Pin<&mut AutostartSetQuery>);
        #[qsignal]
        #[cxx_name = "errorOccurred"]
        fn error_occurred(self: Pin<&mut AutostartSetQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "appErrorOccurred"]
        fn app_error_occurred(self: Pin<&mut AutostartSetQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "sendRequest"]
        fn send_request(self: Pin<&mut AutostartSetQuery>, token: i64, field: QString,
                        request: QByteArray);

        #[qsignal]
        #[cxx_name = "enabledChanged"]
        fn enabled_changed(self: Pin<&mut AutostartGetQuery>);
        #[qsignal]
        #[cxx_name = "enabledChanged"]
        fn enabled_changed(self: Pin<&mut AutostartSetQuery>);
        /// One notification for both availability properties — as upstream.
        #[qsignal]
        #[cxx_name = "availabilityChanged"]
        fn availability_changed(self: Pin<&mut AutostartGetQuery>);
    }

    extern "RustQt" {
        fn status(self: &AutostartGetQuery) -> i32;
        fn error(self: &AutostartGetQuery) -> QString;
        fn querying(self: &AutostartGetQuery) -> bool;
        #[cxx_name = "forwardError"]
        fn forward_error(self: &AutostartGetQuery) -> bool;
        #[cxx_name = "setForwardError"]
        fn set_forward_error(self: Pin<&mut AutostartGetQuery>, value: bool);

        fn status(self: &AutostartSetQuery) -> i32;
        fn error(self: &AutostartSetQuery) -> QString;
        fn querying(self: &AutostartSetQuery) -> bool;
        #[cxx_name = "forwardError"]
        fn forward_error(self: &AutostartSetQuery) -> bool;
        #[cxx_name = "setForwardError"]
        fn set_forward_error(self: Pin<&mut AutostartSetQuery>, value: bool);

        fn enabled(self: &AutostartGetQuery) -> bool;
        fn supported(self: &AutostartGetQuery) -> bool;
        #[cxx_name = "unavailableReason"]
        fn unavailable_reason(self: &AutostartGetQuery) -> QString;
        fn enabled(self: &AutostartSetQuery) -> bool;
        #[cxx_name = "setEnabled"]
        fn set_enabled(self: Pin<&mut AutostartSetQuery>, value: bool);
    }

    extern "RustQt" {
        #[qinvokable]
        fn reload(self: Pin<&mut AutostartGetQuery>);
        #[qinvokable]
        #[cxx_name = "deliverResponse"]
        fn deliver_response(self: Pin<&mut AutostartGetQuery>, token: i64,
                            frame: &QByteArray);

        #[qinvokable]
        fn reload(self: Pin<&mut AutostartSetQuery>);
        #[qinvokable]
        #[cxx_name = "deliverResponse"]
        fn deliver_response(self: Pin<&mut AutostartSetQuery>, token: i64,
                            frame: &QByteArray);
    }

    // The state is read already in the constructor: the vendored page calls
    // `reload()` only when the daemon is ready, while Rexon's autostart does
    // not involve the daemon. The trait declaration must be IN THE BRIDGE.
    impl cxx_qt::Initialize for AutostartGetQuery {}
}

use core::pin::Pin;
use cxx_qt::CxxQtType;
use cxx_qt_lib::{QByteArray, QString};

use rexon_core::we_client;
use rexon_core::we_query::{self, QuerySignal, QueryState, STATUS_ERROR, STATUS_FINISHED,
                           STATUS_QUERYING};

use crate::impl_query_base;
use crate::we_env;

// ── executors of the core's decisions ────────────────────────────────────

/// File system checks for `find_daemon_binary`.
///
/// There is no copy of our own here any more: the tab's transport
/// (`we_socket.rs`) needs the same two methods, and two copies would
/// silently drift apart (rule #12).
use crate::we_env::FsProbe as Fs;

/// Run argv and return `(code, stdout, stderr)`.
fn run(argv: &[String]) -> Option<(i32, String, String)> {
    let (first, rest) = argv.split_first()?;
    let out = std::process::Command::new(first).args(rest).output().ok()?;
    Some((
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    ))
}

/// Whether there is a live systemd user manager.
fn systemd_available() -> bool {
    let present = we_client::DaemonFs::which(&mut Fs, "systemctl").is_some();
    let dir = we_client::systemd_runtime_dir(
        // getuid without libc: the numeric identifier is taken from /proc/self.
        we_env::uid(),
    );
    we_client::systemd_user_available(present, std::path::Path::new(&dir).is_dir())
}

/// Everything the executor takes from the LIVE machine: the environment,
/// whether systemd is there, the engine binary and starting processes.
///
/// Moved into a trait not for looks. The end-to-end cycle "enable → read →
/// disable" used to be checked only in Python (`we_client_roundtrip`,
/// `autostart_cycle`), where these same four observations were replaced
/// with stubs. The executor moved to Rust — the check had to move here too,
/// otherwise removing the Python code would have taken it along (rule
/// #11). The substitution is needed exactly here: without it the comparison
/// would call `systemctl --user enable` of the live session and edit the
/// developer's autostart.
pub trait AutostartHost {
    fn xdg_config_home(&self) -> Option<String>;
    fn home(&self) -> String;
    fn systemd_available(&self) -> bool;
    /// Absolute path of the engine — a port of `find_daemon_binary()`.
    fn daemon_binary(&self) -> Option<String>;
    /// Engine directory from the package with plugins
    /// (`we_env::engine_plugin_root`). `None` by default — fake machines have no
    /// package comparisons.
    fn engine_plugin_root(&self) -> Option<String> {
        None
    }
    /// Run argv → `(code, stdout, stderr)`; `None` means the process did not
    /// start.
    fn run(&mut self, argv: &[String]) -> Option<(i32, String, String)>;
    /// Environment for autostart lines in compositor configs
    /// (`compositor_autostart`). `None` by default — fake machines do not touch
    /// the comparisons of live configs.
    fn compositor_env(&self) -> Option<ca::SessionEnv> {
        None
    }
    /// Whether what starts the engine without our line will work in this
    /// session: `graphical-session.target` for the unit, the XDG target for the
    /// `.desktop`.
    fn session_runs_it(&mut self, systemd: bool) -> bool {
        let _ = systemd;
        true
    }
}

use rexon_core::compositor_autostart as ca;

/// The live machine — the product's only `AutostartHost`.
pub struct Machine;

impl AutostartHost for Machine {
    fn xdg_config_home(&self) -> Option<String> {
        we_env::xdg_config_home()
    }

    fn home(&self) -> String {
        we_env::home()
    }

    fn systemd_available(&self) -> bool {
        systemd_available()
    }

    fn daemon_binary(&self) -> Option<String> {
        we_client::find_daemon_binary(&mut Fs, &we_env::pkg_dir(), &we_env::home())
    }

    fn engine_plugin_root(&self) -> Option<String> {
        we_env::engine_plugin_root_for(self.daemon_binary().as_deref())
    }

    fn run(&mut self, argv: &[String]) -> Option<(i32, String, String)> {
        run(argv)
    }

    fn compositor_env(&self) -> Option<ca::SessionEnv> {
        Some(ca::SessionEnv::live())
    }

    fn session_runs_it(&mut self, systemd: bool) -> bool {
        if systemd {
            ca::graphical_session_target_active()
        } else {
            ca::xdg_autostart_target_active()
        }
    }
}

/// `os.remove`; a missing file is not an error. → whether the file was
/// removed.
fn remove_quiet(path: &str) -> bool {
    std::fs::remove_file(path).is_ok()
}

/// Writing through the core's shared door: the autostart line and the
/// service unit come alive already at login, when there is nobody to
/// explain a truncated file to the customer.
fn write_file(path: &str, content: &str) -> std::io::Result<()> {
    if let Some(parent) = std::path::Path::new(path).parent() {
        std::fs::create_dir_all(parent)?;
    }
    rexon_core::utils::atomic_write(path, content)
}

/// Refresh the autostart unit already on disk if it is outdated.
///
/// The unit is written once — when "Start at login" is enabled — and then
/// lives with the user as is. Without this pass, template fixes (the first
/// of them — `KillMode=mixed`, otherwise stopping the service killed the
/// engine with a BUS signal) would reach only those who toggled the switch
/// again. The decision is entirely the core's ([`we_client::refresh_unit_plan`]);
/// here there is reading, writing and `daemon-reload`. → whether the unit
/// was rewritten.
pub fn refresh_autostart_unit() -> bool {
    if !systemd_available() {
        return false;
    }
    let xdg = we_env::xdg_config_home();
    let home = we_env::home();
    let path = we_client::autostart_unit_path(xdg.as_deref(), &home);
    let existing = std::fs::read_to_string(&path).ok();

    let Some(written) = existing.as_deref().and_then(we_client::unit_binary) else {
        return false;
    };
    // The unit must start the same engine as the window: the written one may
    // have gone away with a package update (since 30.10.2026 the Rexon package
    // does not bundle the engine) or stayed an old copy next to the package's
    // engine (`we_client::unit_refresh_binary`).
    let found = we_env::daemon_binary();
    let binary = we_client::unit_refresh_binary(&written, found.as_deref());
    let plugin_root = we_env::engine_plugin_root_for(Some(&binary));
    match we_client::refresh_unit_plan_on(
        existing.as_deref(),
        &binary,
        xdg.as_deref(),
        &home,
        plugin_root.as_deref(),
    ) {
        we_client::UnitRefresh::Rewrite {
            unit_path,
            unit_content,
            daemon_reload_argv,
        } => {
            if let Err(e) = write_file(&unit_path, &unit_content) {
                rexon_core::log_warn!("we", "юнит автозапуска {unit_path} не переписан: {e}");
                return false;
            }
            if binary == written {
                rexon_core::log_info!("we", "юнит автозапуска {unit_path} переписан по новому образцу");
            } else {
                rexon_core::log_info!(
                    "we",
                    "юнит автозапуска {unit_path} переписан: движок {written} → {binary}"
                );
            }
            run(&daemon_reload_argv);
            true
        }
        _ => false,
    }
}

/// Whether there is an engine autostart service to start it through: the
/// systemd user manager is alive and the unit is written
/// (`we_client::ensure_running_decision_service`).
pub fn autostart_service_present() -> bool {
    if !systemd_available() {
        return false;
    }
    let xdg = we_env::xdg_config_home();
    let path = we_client::autostart_unit_path(xdg.as_deref(), &we_env::home());
    std::path::Path::new(&path).is_file()
}

/// Autostart state for `W.AutostartGetQuery`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutostartState {
    pub enabled: bool,
    /// Whether Rexon can set up autostart here
    /// ([`we_client::autostart_availability`]).
    pub supported: bool,
    /// Why it cannot; empty when `supported`.
    pub unavailable_reason: String,
}

/// Capture the whole state at once — without the daemon, only the `host`
/// observations.
pub fn autostart_state_on<H: AutostartHost + ?Sized>(host: &mut H) -> AutostartState {
    let (supported, unavailable_reason) =
        we_client::autostart_availability(host.daemon_binary().as_deref());
    AutostartState {
        enabled: autostart_enabled_on(host),
        supported,
        unavailable_reason,
    }
}

/// A port of `client.autostart_enabled()` — on the live machine.
pub fn autostart_enabled() -> bool {
    autostart_enabled_on(&mut Machine)
}

/// A port of `client.set_autostart()` → `(ok, message)` — on the live
/// machine.
pub fn set_autostart(enabled: bool) -> (bool, String) {
    set_autostart_on(&mut Machine, enabled)
}

/// Whether autostart is enabled. The observations are captured by `host`,
/// the verdict is given by the core.
pub fn autostart_enabled_on<H: AutostartHost + ?Sized>(host: &mut H) -> bool {
    let observed = if host.systemd_available() {
        host.run(&we_client::autostart_is_enabled_argv())
            .map(|(code, stdout, _)| (code, stdout))
    } else {
        None
    };
    let desktop =
        we_client::autostart_desktop_path(host.xdg_config_home().as_deref(), &host.home());
    let unit_on = observed
        .as_ref()
        .is_some_and(|(c, s)| *c == 0 && s.trim() == "enabled");
    let on = we_client::autostart_enabled(
        observed.as_ref().map(|(c, s)| (*c, s.as_str())),
        std::path::Path::new(&desktop).is_file(),
    );
    // Enabled — but will it come up in THIS session? Bare compositors start
    // neither XDG entries nor `graphical-session.target`.
    match host.compositor_env() {
        Some(env) if on => {
            let text = if unit_on { None } else { std::fs::read_to_string(&desktop).ok() };
            let mut fallback = || host.session_runs_it(unit_on);
            ca::works_in_current_session(&env, ca::ID_ENGINE, text.as_deref(), &mut fallback)
        }
        _ => on,
    }
}

/// Idempotently set/remove autostart → `(ok, message)`.
///
/// The daemon is NOT started or stopped here: this is only about login,
/// the current session is not touched. Enabling writes the ABSOLUTE path of
/// today's engine — enabling again simply updates the path.
pub fn set_autostart_on<H: AutostartHost + ?Sized>(host: &mut H, enabled: bool) -> (bool, String) {
    let xdg = host.xdg_config_home();
    let home = host.home();
    let systemd = host.systemd_available();

    if !enabled {
        let unit = we_client::autostart_unit_path(xdg.as_deref(), &home);
        let plan = we_client::disable_plan(
            xdg.as_deref(),
            &home,
            systemd,
            std::path::Path::new(&unit).is_file(),
        );
        if let Some(argv) = plan.disable_argv.clone() {
            host.run(&argv);
        }
        if remove_quiet(&plan.remove_unit) {
            if let Some(argv) = plan.daemon_reload_argv.clone() {
                host.run(&argv);
            }
        }
        remove_quiet(&plan.remove_desktop);
        if let Some(env) = host.compositor_env() {
            ca::remove_everywhere(&env, ca::ID_ENGINE);
        }
        return (true, plan.message);
    }

    let binary = host.daemon_binary();
    let plugin_root = host.engine_plugin_root();
    match we_client::enable_plan_with(
        binary.as_deref(),
        xdg.as_deref(),
        &home,
        systemd,
        plugin_root.as_deref(),
    ) {
        we_client::EnablePlan::NoBinary { message } => (false, message),
        we_client::EnablePlan::Systemd {
            unit_path,
            unit_content,
            daemon_reload_argv,
            enable_argv,
            remove_desktop,
            message,
        } => {
            if let Err(e) = write_file(&unit_path, &unit_content) {
                return (false, we_client::set_autostart_error_message(&e.to_string()));
            }
            host.run(&daemon_reload_argv);
            match host.run(&enable_argv) {
                Some((0, _, _)) => {
                    // Do not keep a second mechanism in parallel.
                    remove_quiet(&remove_desktop);
                    // Where `graphical-session.target` does not come up, the unit is started
                    // by a line in the compositor config.
                    if let Some(env) = host.compositor_env() {
                        let report = ca::sync_command(
                            &env,
                            ca::ID_ENGINE,
                            Some(&we_client::autostart_unit_start_argv()),
                        );
                        if let Some(msg) = report.current_refusal(&env) {
                            return (false, msg);
                        }
                    }
                    (true, message)
                }
                Some((_, stdout, stderr)) => {
                    (false, we_client::enable_failed_message(&stderr, &stdout))
                }
                None => (
                    false,
                    we_client::set_autostart_error_message(&rexon_core::i18n::tr("systemctl did not start")),
                ),
            }
        }
        we_client::EnablePlan::Xdg {
            desktop_path,
            desktop_content,
            message,
        } => {
            let (desktop_content, refusal) = match host.compositor_env() {
                Some(env) => {
                    let m = ca::mirror_desktop(&env, ca::ID_ENGINE, &desktop_content);
                    let refusal = m.report.current_refusal(&env);
                    (m.desktop, refusal)
                }
                None => (desktop_content, None),
            };
            match write_file(&desktop_path, &desktop_content) {
                Ok(()) => match refusal {
                    Some(msg) => (false, msg),
                    None => (true, message),
                },
                Err(e) => (false, we_client::set_autostart_error_message(&e.to_string())),
            }
        }
    }
}

// ════════════════════════════════════════════════════════════════════════
// AutostartGetQuery
// ════════════════════════════════════════════════════════════════════════

#[derive(Default)]
pub struct AutostartGetQueryRust {
    base: QueryState,
    enabled: bool,
    supported: bool,
    unavailable_reason: String,
}

impl_query_base!(AutostartGetQuery);

impl cxx_qt::Initialize for qobject::AutostartGetQuery {
    /// The first read is in the constructor, not on `reload()`: the waywallen
    /// page (both v0.6.0 and with #380) calls `reload()` only when the daemon is
    /// ready, and without this, with the daemon down, the switch would be grey
    /// and "off". The status is not touched: this is not a request but the
    /// initial value of the properties.
    fn initialize(mut self: Pin<&mut Self>) {
        let state = autostart_state_on(&mut Machine);
        let signals = self.as_mut().apply_state(state);
        self.dispatch(signals);
    }
}

impl qobject::AutostartGetQuery {
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn supported(&self) -> bool {
        self.supported
    }

    pub fn unavailable_reason(&self) -> QString {
        QString::from(self.unavailable_reason.as_str())
    }

    /// Spread the captured state over the fields → notifications ONLY about
    /// what changed (as in the original); one for both availability
    /// properties.
    fn apply_state(self: Pin<&mut Self>, state: AutostartState) -> Vec<QuerySignal> {
        let mut signals = Vec::new();
        let mut rust = self.rust_mut();
        we_query::assign(&mut rust.enabled, state.enabled, "enabled", &mut signals);
        if rust.supported != state.supported || rust.unavailable_reason != state.unavailable_reason {
            rust.supported = state.supported;
            rust.unavailable_reason = state.unavailable_reason;
            signals.push(QuerySignal::Property("availability"));
        }
        signals
    }

    pub fn reload(mut self: Pin<&mut Self>) {
        let signals = we_query::set_status(&mut self.as_mut().rust_mut().base, STATUS_QUERYING);
        self.as_mut().dispatch(signals);
        let state = autostart_state_on(&mut Machine);
        let signals = self.as_mut().apply_state(state);
        let signals = we_query::finish(&mut self.as_mut().rust_mut().base, signals);
        self.dispatch(signals);
    }

    /// This query sends no frames — there is nothing to deliver.
    pub fn deliver_response(self: Pin<&mut Self>, _token: i64, _frame: &QByteArray) {
        let _ = self;
    }

    pub fn emit_property(mut self: Pin<&mut Self>, name: &str) {
        match name {
            "enabled" => self.as_mut().enabled_changed(),
            "availability" => self.as_mut().availability_changed(),
            other => debug_assert!(false, "нет сигнала для свойства {other}"),
        }
    }

    pub fn dispatch(self: Pin<&mut Self>, signals: Vec<QuerySignal>) {
        self.dispatch_base(signals);
    }
}

// ════════════════════════════════════════════════════════════════════════
// AutostartSetQuery
// ════════════════════════════════════════════════════════════════════════

#[derive(Default)]
pub struct AutostartSetQueryRust {
    base: QueryState,
    enabled: bool,
}

impl_query_base!(AutostartSetQuery);

impl qobject::AutostartSetQuery {
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn set_enabled(mut self: Pin<&mut Self>, value: bool) {
        if self.enabled == value {
            return;
        }
        self.as_mut().rust_mut().enabled = value;
        self.enabled_changed();
    }

    pub fn reload(mut self: Pin<&mut Self>) {
        let signals = we_query::set_status(&mut self.as_mut().rust_mut().base, STATUS_QUERYING);
        self.as_mut().dispatch(signals);
        let wanted = self.enabled;
        let (ok, message) = set_autostart(wanted);
        let signals = {
            let mut rust = self.as_mut().rust_mut();
            if ok {
                we_query::set_status(&mut rust.base, STATUS_FINISHED)
            } else {
                let mut out = we_query::set_error(&mut rust.base, &message);
                out.extend(we_query::set_status(&mut rust.base, STATUS_ERROR));
                out
            }
        };
        self.dispatch(signals);
    }

    pub fn deliver_response(self: Pin<&mut Self>, _token: i64, _frame: &QByteArray) {
        let _ = self;
    }

    pub fn emit_property(mut self: Pin<&mut Self>, name: &str) {
        match name {
            "enabled" => self.as_mut().enabled_changed(),
            other => debug_assert!(false, "нет сигнала для свойства {other}"),
        }
    }

    pub fn dispatch(self: Pin<&mut Self>, signals: Vec<QuerySignal>) {
        self.dispatch_base(signals);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fake machine: replaces exactly the four observations that were
    /// monkey-patched in the Python comparison. Not a single `systemctl` call
    /// goes out — the argv are only recorded.
    struct FakeHost {
        root_dir: std::path::PathBuf,
        systemd: bool,
        binary_file: String,
        calls: Vec<Vec<String>>,
    }

    impl AutostartHost for FakeHost {
        fn xdg_config_home(&self) -> Option<String> {
            Some(self.root_dir.join("cfg").to_string_lossy().into_owned())
        }

        fn home(&self) -> String {
            self.root_dir.to_string_lossy().into_owned()
        }

        fn systemd_available(&self) -> bool {
            self.systemd
        }

        fn daemon_binary(&self) -> Option<String> {
            Some(self.binary_file.clone())
        }

        fn run(&mut self, argv: &[String]) -> Option<(i32, String, String)> {
            self.calls.push(argv.to_vec());
            if argv.iter().any(|a| a == "is-enabled") {
                return Some((0, "enabled\n".to_string(), String::new()));
            }
            Some((0, String::new(), String::new()))
        }
    }

    /// The end-to-end cycle "enable → read → disable" with the executor's real
    /// code, but in a fake HOME. A port of `autostart_cycle` from the bridge
    /// comparison: that one goes away together with the Python executor, the
    /// coverage does not.
    fn cycle(systemd: bool) {
        let root_dir = std::env::temp_dir().join(format!(
            "rexon-autostart-cycle-{}-{systemd}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root_dir);
        std::fs::create_dir_all(&root_dir).unwrap();
        let binary_file = root_dir.join("waywallen.AppImage").to_string_lossy().into_owned();
        let mut host = FakeHost {
            root_dir: root_dir.clone(),
            systemd,
            binary_file: binary_file.clone(),
            calls: Vec::new(),
        };

        let xdg = host.xdg_config_home();
        let home = host.home();
        let unit_file = we_client::autostart_unit_path(xdg.as_deref(), &home);
        let desktop_entry = we_client::autostart_desktop_path(xdg.as_deref(), &home);

        let (ok, message_text) = set_autostart_on(&mut host, true);
        assert!(ok, "включение обязано пройти: {message_text}");
        assert!(autostart_enabled_on(&mut host), "включённый автозапуск обязан читаться");

        let (was_written, second) = if systemd { (&unit_file, &desktop_entry) } else { (&desktop_entry, &unit_file) };
        let body = std::fs::read_to_string(was_written).expect("включение ничего не записало");
        assert!(body.contains(&binary_file), "в файл ушёл не тот бинарь: {body}");
        assert!(body.contains("--no-ui --no-tray"), "{body}");
        assert!(
            !std::path::Path::new(second).exists(),
            "второй механизм держать параллельно нельзя"
        );

        let (ok, message_text) = set_autostart_on(&mut host, false);
        assert!(ok);
        assert_eq!(message_text, rexon_core::i18n::tr("Autostart disabled"));
        for p in [&unit_file, &desktop_entry] {
            assert!(!std::path::Path::new(p).exists(), "после выключения остался {p}");
        }
        // Disabling again is idempotent.
        assert!(set_autostart_on(&mut host, false).0);

        if systemd {
            let argv: Vec<String> =
                host.calls.iter().map(|c| c.join(" ")).collect();
            assert!(
                argv.contains(&"systemctl --user enable rexon-wallpaper-engine.service".to_string()),
                "{argv:?}"
            );
            assert!(
                argv.contains(&"systemctl --user disable rexon-wallpaper-engine.service".to_string()),
                "{argv:?}"
            );
        } else {
            assert!(host.calls.is_empty(), "без systemd процессов быть не должно");
        }

        let _ = std::fs::remove_dir_all(&root_dir);
    }

    /// The state for `W.AutostartGetQuery` (`enabled`, `supported`,
    /// `unavailableReason`) is captured without the daemon: at most
    /// `systemctl --user is-enabled` goes out, not a single frame to the engine.
    /// The switch is grey exactly when there is nothing to enable — no engine.
    #[test]
    fn autostart_state_without_daemon() {
        for systemd in [true, false] {
            let root = std::env::temp_dir().join(format!(
                "rexon-autostart-state-{}-{systemd}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(&root).unwrap();
            let binary = root.join("waywallen").to_string_lossy().into_owned();
            let mut host = FakeHost {
                root_dir: root.clone(),
                systemd,
                binary_file: binary,
                calls: Vec::new(),
            };

            let before = autostart_state_on(&mut host);
            assert!(before.supported, "движок есть — переключатель обязан быть живым");
            assert_eq!(before.unavailable_reason, "");
            // FakeHost answers "enabled" to is-enabled, so without systemd the enabled
            // state is decided only by the XDG file — which does not exist yet.
            assert_eq!(before.enabled, systemd);
            for call in &host.calls {
                assert_eq!(call, &we_client::autostart_is_enabled_argv(), "лишний вызов {call:?}");
            }

            // After enabling, reading sees it at once — without the daemon.
            assert!(set_autostart_on(&mut host, true).0);
            assert!(autostart_state_on(&mut host).enabled);
            assert!(set_autostart_on(&mut host, false).0);

            struct NoEngine(FakeHost);
            impl AutostartHost for NoEngine {
                fn xdg_config_home(&self) -> Option<String> {
                    self.0.xdg_config_home()
                }
                fn home(&self) -> String {
                    self.0.home()
                }
                fn systemd_available(&self) -> bool {
                    self.0.systemd_available()
                }
                fn daemon_binary(&self) -> Option<String> {
                    None
                }
                fn run(&mut self, argv: &[String]) -> Option<(i32, String, String)> {
                    self.0.run(argv)
                }
            }
            let mut bare = NoEngine(host);
            let state = autostart_state_on(&mut bare);
            assert!(!state.supported);
            // The translation itself is compared by the core (`we_client_parity`);
            // what matters here is that the reason is the same translatable string,
            // not emptiness.
            assert_eq!(state, AutostartState {
                // FakeHost always answers "enabled" to is-enabled.
                enabled: systemd,
                supported: false,
                unavailable_reason: we_client::autostart_availability(None).1,
            });
            assert!(!state.unavailable_reason.is_empty());
            let _ = std::fs::remove_dir_all(&root);
        }
    }

    #[test]
    fn autostart_cycle_systemd() {
        cycle(true);
    }

    #[test]
    fn autostart_cycle_xdg_fallback() {
        cycle(false);
    }

    /// Refreshing the unit is run with the REAL code (reading the file, writing,
    /// `daemon-reload`), but in a fake `XDG_CONFIG_HOME` — the live session's
    /// unit is not touched.
    #[test]
    fn refresh_rewrites_unit_of_previous_form() {
        if !systemd_available() {
            return; // without systemd there is nothing to refresh
        }
        let root_dir = std::env::temp_dir().join(format!(
            "rexon-autostart-refresh-{}",
            std::process::id()
        ));
        let previous = std::env::var("XDG_CONFIG_HOME").ok();
        std::env::set_var("XDG_CONFIG_HOME", &root_dir);

        let entry_path = we_client::autostart_unit_path(
            Some(root_dir.to_str().unwrap()),
            &we_env::home(),
        );
        let binary_path = root_dir.join("waywallen.AppImage");
        std::fs::create_dir_all(&root_dir).unwrap();
        std::fs::write(&binary_path, "#!/bin/sh\n").unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&binary_path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let binary_file = binary_path.to_str().unwrap();
        // The `--plugin` tail follows the engine from the package: the refresh
        // takes it from the machine (`we_env::engine_plugin_root_for`), and the
        // expectation must take it from the same place — otherwise the comparison
        // goes red exactly where Rexon is installed as a package and green in a
        // bare developer tree.
        // The refreshed unit points at the engine Rexon would start now
        // (`we_client::unit_refresh_binary`); on a machine without an engine — the
        // previous one.
        let expected_binary = we_env::daemon_binary().unwrap_or_else(|| binary_file.to_string());
        let fresh = we_client::autostart_unit_content_with(
            &expected_binary,
            we_env::engine_plugin_root_for(Some(&expected_binary)).as_deref(),
        );
        let old_unit: String = fresh
            .lines()
            .filter(|l| !l.starts_with("KillMode=") && !l.starts_with('#'))
            .map(|l| format!("{l}\n"))
            .collect();
        write_file(&entry_path, &old_unit).unwrap();

        assert!(refresh_autostart_unit(), "юнит прежнего образца обязан переписаться");
        assert_eq!(std::fs::read_to_string(&entry_path).unwrap(), fresh);
        // A repeated pass does nothing.
        assert!(!refresh_autostart_unit());

        let _ = std::fs::remove_dir_all(&root_dir);
        match previous {
            Some(v) => std::env::set_var("XDG_CONFIG_HOME", v),
            None => std::env::remove_var("XDG_CONFIG_HOME"),
        }
    }
}
