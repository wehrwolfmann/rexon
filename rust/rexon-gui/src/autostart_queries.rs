// Автозапуск движка при входе в систему.
//
// ⚠️ СОЗНАТЕЛЬНОЕ отличие от оригинала, а не дефект: у вендора
// (`ui/src/query/autostart_query.cpp` @e5626c1) автозапуск — RPC демону
// (`autostart_get`/`autostart_set`), и работает он только во Flatpak через
// портал Background. У Rexon он роутится ЛОКАЛЬНО: systemd user unit с
// фолбэком на XDG autostart (роадмап п.0-ter). Демону кадры НЕ уходят
// вовсе, поэтому у этих двух запросов нет ни `sendRequest`, ни
// `deliverResponse`-ветки с кадром — только сигналы базы.
//
// QML-контракт при этом ТОТ ЖЕ, что у оригинала: `enabled`, `status`,
// `error`, `reload()`. SettingsPage по `status == 2` перечитывает
// состояние, по `status == 3` показывает тост с `.error`.
//
// Решения (какой механизм, какие файлы, какие argv, какие сообщения) —
// целиком в ядре (`rexon_core::we_client`), здесь только исполнение:
// запуск процессов и запись файлов. Ровно та же граница, что была у
// `client.py`, только исполнитель теперь не Python: с 07.09.2026
// питоновских `autostart_enabled`/`set_autostart` не существует, и этот
// модуль — ЕДИНСТВЕННОЕ место, где автозапуск движка правится на диске.

/// Мост cxx-qt: чтение и запись автозапуска.
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
}

use core::pin::Pin;
use cxx_qt::CxxQtType;
use cxx_qt_lib::QByteArray;

use rexon_core::we_client;
use rexon_core::we_query::{self, QuerySignal, QueryState, STATUS_ERROR, STATUS_FINISHED,
                           STATUS_QUERYING};

use crate::impl_query_base;
use crate::we_env;

// ── исполнители решений ядра ─────────────────────────────────────────────

/// Проверки файловой системы для `find_daemon_binary`.
///
/// Своей копии здесь больше нет: те же два метода нужны транспорту вкладки
/// (`we_socket.rs`), а две копии разъехались бы молча (правило #12).
use crate::we_env::FsProbe as Fs;

/// Выполнить argv и вернуть `(код, stdout, stderr)`.
fn run(argv: &[String]) -> Option<(i32, String, String)> {
    let (first, rest) = argv.split_first()?;
    let out = std::process::Command::new(first).args(rest).output().ok()?;
    Some((
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    ))
}

/// Есть ли живой user-менеджер systemd.
fn systemd_available() -> bool {
    let present = we_client::DaemonFs::which(&mut Fs, "systemctl").is_some();
    let dir = we_client::systemd_runtime_dir(
        // getuid без libc: числовой идентификатор берётся из /proc/self.
        we_env::uid(),
    );
    we_client::systemd_user_available(present, std::path::Path::new(&dir).is_dir())
}

/// Всё, что исполнитель берёт у ЖИВОЙ машины: окружение, наличие systemd,
/// бинарь движка и запуск процессов.
///
/// Вынесено в трейт не ради красоты. Сквозной цикл «включить → прочитать →
/// выключить» до сих пор проверялся только в Python (`we_client_roundtrip`,
/// `autostart_cycle`), где эти же четыре наблюдения подменялись заглушками.
/// Исполнитель уехал в Rust — сюда обязана была уехать и проверка, иначе
/// снятие Python-кода забрало бы её с собой (правило #11). Подмена нужна
/// именно здесь: без неё сверка звала бы `systemctl --user enable` живой
/// сессии и правила бы автозапуск разработчика.
pub trait AutostartHost {
    fn xdg_config_home(&self) -> Option<String>;
    fn home(&self) -> String;
    fn systemd_available(&self) -> bool;
    /// Абсолютный путь движка — порт `find_daemon_binary()`.
    fn daemon_binary(&self) -> Option<String>;
    /// Каталог движка из пакета с плагинами (`we_env::engine_plugin_root`).
    /// По умолчанию `None` — у подставных машин сверок пакета нет.
    fn engine_plugin_root(&self) -> Option<String> {
        None
    }
    /// Запустить argv → `(код, stdout, stderr)`; `None` — процесс не стартовал.
    fn run(&mut self, argv: &[String]) -> Option<(i32, String, String)>;
    /// Окружение для строк автозапуска в конфигах композиторов
    /// (`compositor_autostart`). По умолчанию `None` — подставные машины
    /// сверок живых конфигов не касаются.
    fn compositor_env(&self) -> Option<ca::SessionEnv> {
        None
    }
    /// Сработает ли в этом сеансе то, что поднимает движок без нашей строки:
    /// `graphical-session.target` для юнита, XDG-цель — для `.desktop`.
    fn session_runs_it(&mut self, systemd: bool) -> bool {
        let _ = systemd;
        true
    }
}

use rexon_core::compositor_autostart as ca;

/// Живая машина — единственный `AutostartHost` продукта.
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

/// `os.remove`, отсутствие файла — не ошибка. → был ли файл удалён.
fn remove_quiet(path: &str) -> bool {
    std::fs::remove_file(path).is_ok()
}

/// Запись общей дверью ядра: строка автозапуска и юнит службы оживают уже
/// при входе в сеанс, когда объяснить покупателю обрезанный файл некому.
fn write_file(path: &str, content: &str) -> std::io::Result<()> {
    if let Some(parent) = std::path::Path::new(path).parent() {
        std::fs::create_dir_all(parent)?;
    }
    rexon_core::utils::atomic_write(path, content)
}

/// Освежить уже лежащий на диске юнит автозапуска, если он устарел.
///
/// Юнит пишется один раз — при включении «Start at login», и дальше живёт у
/// пользователя как есть. Без этого прохода починки шаблона (первая из них —
/// `KillMode=mixed`, иначе остановка службы валила движок сигналом BUS)
/// достались бы только тем, кто заново щёлкнул переключатель. Решение целиком
/// у ядра ([`we_client::refresh_unit_plan`]), здесь — чтение, запись и
/// `daemon-reload`. → был ли юнит переписан.
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
    // Движок, записанный в юнит, мог уйти вместе с обновлением пакета (с
    // 30.10.2026 пакет Rexon не вшивает движок) — тогда юнит пересобирается
    // на найденном сейчас; живой выбор человека не трогается.
    let alive = we_client::DaemonFs::is_exec_file(&mut Fs, &written);
    let found = if alive { None } else { we_env::daemon_binary() };
    let binary = we_client::unit_refresh_binary(&written, alive, found.as_deref());
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
            if write_file(&unit_path, &unit_content).is_err() {
                return false;
            }
            run(&daemon_reload_argv);
            true
        }
        _ => false,
    }
}

/// Есть ли служба автозапуска движка, через которую его поднимать: systemd
/// пользователя жив и юнит записан (`we_client::ensure_running_decision_service`).
pub fn autostart_service_present() -> bool {
    if !systemd_available() {
        return false;
    }
    let xdg = we_env::xdg_config_home();
    let path = we_client::autostart_unit_path(xdg.as_deref(), &we_env::home());
    std::path::Path::new(&path).is_file()
}

/// Порт `client.autostart_enabled()` — на живой машине.
pub fn autostart_enabled() -> bool {
    autostart_enabled_on(&mut Machine)
}

/// Порт `client.set_autostart()` → `(ok, сообщение)` — на живой машине.
pub fn set_autostart(enabled: bool) -> (bool, String) {
    set_autostart_on(&mut Machine, enabled)
}

/// Включён ли автозапуск. Наблюдения снимает `host`, вердикт выносит ядро.
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
    // Включено — но поднимется ли в ЭТОМ сеансе? Голые композиторы не
    // поднимают ни XDG-записей, ни `graphical-session.target`.
    match host.compositor_env() {
        Some(env) if on => {
            let text = if unit_on { None } else { std::fs::read_to_string(&desktop).ok() };
            let mut fallback = || host.session_runs_it(unit_on);
            ca::works_in_current_session(&env, ca::ID_ENGINE, text.as_deref(), &mut fallback)
        }
        _ => on,
    }
}

/// Идемпотентно поставить/снять автозапуск → `(ok, сообщение)`.
///
/// Демона здесь НЕ запускаем и НЕ останавливаем: речь только про вход в
/// систему, текущая сессия не трогается. Включение пишет АБСОЛЮТНЫЙ путь
/// сегодняшнего движка — повторное включение просто обновляет путь.
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
                    // Не держать второй механизм параллельно.
                    remove_quiet(&remove_desktop);
                    // Где `graphical-session.target` не поднимается, юнит
                    // стартует строкой в конфиге композитора.
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
}

impl_query_base!(AutostartGetQuery);

impl qobject::AutostartGetQuery {
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn reload(mut self: Pin<&mut Self>) {
        let signals = we_query::set_status(&mut self.as_mut().rust_mut().base, STATUS_QUERYING);
        self.as_mut().dispatch(signals);
        let enabled = autostart_enabled();
        // Сигнал ТОЛЬКО при изменении — как у оригинала.
        let mut signals = Vec::new();
        {
            let mut rust = self.as_mut().rust_mut();
            we_query::assign(&mut rust.enabled, enabled, "enabled", &mut signals);
        }
        let signals = we_query::finish(&mut self.as_mut().rust_mut().base, signals);
        self.dispatch(signals);
    }

    /// Кадров этот запрос не шлёт — доставлять нечего.
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

    /// Подставная машина: подменяет ровно те четыре наблюдения, что в
    /// Python-сверке подменялись monkey-patch'ем. Ни одного вызова
    /// `systemctl` наружу — argv только записываются.
    struct FakeHost {
        корень: std::path::PathBuf,
        systemd: bool,
        бинарь: String,
        вызовы: Vec<Vec<String>>,
    }

    impl AutostartHost for FakeHost {
        fn xdg_config_home(&self) -> Option<String> {
            Some(self.корень.join("cfg").to_string_lossy().into_owned())
        }

        fn home(&self) -> String {
            self.корень.to_string_lossy().into_owned()
        }

        fn systemd_available(&self) -> bool {
            self.systemd
        }

        fn daemon_binary(&self) -> Option<String> {
            Some(self.бинарь.clone())
        }

        fn run(&mut self, argv: &[String]) -> Option<(i32, String, String)> {
            self.вызовы.push(argv.to_vec());
            if argv.iter().any(|a| a == "is-enabled") {
                return Some((0, "enabled\n".to_string(), String::new()));
            }
            Some((0, String::new(), String::new()))
        }
    }

    /// Сквозной цикл «включить → прочитать → выключить» настоящим кодом
    /// исполнителя, но в подставном HOME. Порт `autostart_cycle` из сверки
    /// моста: она уходит вместе с Python-исполнителем, покрытие — нет.
    fn цикл(systemd: bool) {
        let корень = std::env::temp_dir().join(format!(
            "rexon-autostart-cycle-{}-{systemd}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&корень);
        std::fs::create_dir_all(&корень).unwrap();
        let бинарь = корень.join("waywallen.AppImage").to_string_lossy().into_owned();
        let mut host = FakeHost {
            корень: корень.clone(),
            systemd,
            бинарь: бинарь.clone(),
            вызовы: Vec::new(),
        };

        let xdg = host.xdg_config_home();
        let home = host.home();
        let юнит = we_client::autostart_unit_path(xdg.as_deref(), &home);
        let десктоп = we_client::autostart_desktop_path(xdg.as_deref(), &home);

        let (ok, сообщение) = set_autostart_on(&mut host, true);
        assert!(ok, "включение обязано пройти: {сообщение}");
        assert!(autostart_enabled_on(&mut host), "включённый автозапуск обязан читаться");

        let (записан, второй) = if systemd { (&юнит, &десктоп) } else { (&десктоп, &юнит) };
        let тело = std::fs::read_to_string(записан).expect("включение ничего не записало");
        assert!(тело.contains(&бинарь), "в файл ушёл не тот бинарь: {тело}");
        assert!(тело.contains("--no-ui --no-tray"), "{тело}");
        assert!(
            !std::path::Path::new(второй).exists(),
            "второй механизм держать параллельно нельзя"
        );

        let (ok, сообщение) = set_autostart_on(&mut host, false);
        assert!(ok);
        assert_eq!(сообщение, rexon_core::i18n::tr("Autostart disabled"));
        for p in [&юнит, &десктоп] {
            assert!(!std::path::Path::new(p).exists(), "после выключения остался {p}");
        }
        // Повторное выключение идемпотентно.
        assert!(set_autostart_on(&mut host, false).0);

        if systemd {
            let argv: Vec<String> =
                host.вызовы.iter().map(|c| c.join(" ")).collect();
            assert!(
                argv.contains(&"systemctl --user enable rexon-wallpaper-engine.service".to_string()),
                "{argv:?}"
            );
            assert!(
                argv.contains(&"systemctl --user disable rexon-wallpaper-engine.service".to_string()),
                "{argv:?}"
            );
        } else {
            assert!(host.вызовы.is_empty(), "без systemd процессов быть не должно");
        }

        let _ = std::fs::remove_dir_all(&корень);
    }

    #[test]
    fn цикл_автозапуска_systemd() {
        цикл(true);
    }

    #[test]
    fn цикл_автозапуска_xdg_фолбэк() {
        цикл(false);
    }

    /// Освежение юнита прогоняется НАСТОЯЩИМ кодом (чтение файла, запись,
    /// `daemon-reload`), но в подставном `XDG_CONFIG_HOME` — юнит живой
    /// сессии не трогается.
    #[test]
    fn освежение_переписывает_юнит_прежнего_образца() {
        if !systemd_available() {
            return; // без systemd освежать нечего
        }
        let корень = std::env::temp_dir().join(format!(
            "rexon-autostart-refresh-{}",
            std::process::id()
        ));
        let прежний = std::env::var("XDG_CONFIG_HOME").ok();
        std::env::set_var("XDG_CONFIG_HOME", &корень);

        let путь = we_client::autostart_unit_path(
            Some(корень.to_str().unwrap()),
            &we_env::home(),
        );
        // Движок, записанный в юнит, обязан быть на диске: пропавший
        // освежение заменяет найденным (`we_client::unit_refresh_binary`).
        let бинарь_путь = корень.join("waywallen.AppImage");
        std::fs::create_dir_all(&корень).unwrap();
        std::fs::write(&бинарь_путь, "#!/bin/sh\n").unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&бинарь_путь, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let бинарь = бинарь_путь.to_str().unwrap();
        // Хвост `--plugin` следует за движком из пакета: освежение берёт
        // его у машины (`we_env::engine_plugin_root_for`), и ожидание обязано
        // брать там же — иначе сверка краснеет ровно там, где Rexon стоит
        // пакетом, и зеленеет в голом дереве разработчика.
        let свежий = we_client::autostart_unit_content_with(
            бинарь,
            we_env::engine_plugin_root_for(Some(бинарь)).as_deref(),
        );
        let старый: String = свежий
            .lines()
            .filter(|l| !l.starts_with("KillMode=") && !l.starts_with('#'))
            .map(|l| format!("{l}\n"))
            .collect();
        write_file(&путь, &старый).unwrap();

        assert!(refresh_autostart_unit(), "юнит прежнего образца обязан переписаться");
        assert_eq!(std::fs::read_to_string(&путь).unwrap(), свежий);
        // Повторный проход ничего не делает.
        assert!(!refresh_autostart_unit());

        let _ = std::fs::remove_dir_all(&корень);
        match прежний {
            Some(v) => std::env::set_var("XDG_CONFIG_HOME", v),
            None => std::env::remove_var("XDG_CONFIG_HOME"),
        }
    }
}
