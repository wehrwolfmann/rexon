//! Исполнитель операций над рабочими столами — порт ДЕЙСТВУЮЩЕЙ половины
//! `install_de` из `rexon/system.py` вместе с тем, что она зовёт:
//! `PackageManager.install_batch`, `ensure_display_manager`, пост-установкой
//! MATE, подключением overlay Gentoo и защитой приложений по умолчанию
//! (`SystemDefaultsProtector`).
//!
//! ЗАЧЕМ. Решения этих операций переехали в ядро давно: планы и тексты
//! скриптов — [`crate::system_plan`], совместимость дистрибутива —
//! [`crate::distro_compat`], показ хода — [`crate::de_progress`], защита
//! умолчаний — [`crate::defaults`]. Исполнял их только Python, и поэтому окно
//! на Rust на кнопках «Установить», «Проверить / Восстановить» и «Проверить
//! мой стол» могло лишь отослать человека в классическое окно.
//!
//! Разделение то же, что у соседа [`crate::system_exec`]: **ядро решает,
//! исполнитель делает**.
//!   * ЧТО делать — планы и тексты ядра; здесь нет ни одной своей команды
//!     пакетного менеджера, ни одного своего пути к привилегированному
//!     помощнику;
//!   * ЧЕМ делать — трейт [`DeOpsIo`] (надстройка над [`SystemIo`]): в
//!     продукте [`LiveDeOpsIo`], в сверках — подставной, и живая машина при
//!     этом не трогается вовсе;
//!   * ПОРЯДОК шагов и ход дела — [`DeOpsExec`], события — в [`DeOpSink`].
//!
//! ⛔ ЗАЩИТА ОТ ДУРАКА, которая обязана работать и здесь:
//!   * отказ дистрибутива объясняется ДО первого побочного действия — ни
//!     снимка умолчаний, ни копии панели, ни запроса пароля;
//!   * экран входа после установки обязан быть: если включённого нет,
//!     SDDM ставится и включается САМ, без вопроса (решение —
//!     [`plan::ensure_dm_plan`], зовётся для ЛЮБОГО стола, как в оригинале).
//!
//! ⭐ ОТМЕНА. У Python её не было вовсе: кнопка окна хода была выключена до
//! конца. Здесь отменить можно, но ТОЛЬКО пока система не тронута: до
//! первого запуска пакетного менеджера и во время ожидания чужого замка
//! (оно длится до пяти минут, и сидеть перед окном без выхода — ровно та
//! ловушка, которую правило #13 запрещает). Как только пошла транзакция,
//! отмена выключается: оборвать `pacman` посреди установки — значит
//! сломать систему своими руками.
//!
//! ⭐ ЧТО ПОЧИНЕНО ПРИ ПЕРЕНОСЕ (правило #16):
//!   1. Строки хода, которые оригинал писал человеку жаргоном («Сохранение
//!      текущих defaults», «MIME-ассоциации DE», «ensure_display_manager:
//!      установка sddm код 1»), здесь сказаны по-русски.
//!   2. Хвост сборки из AUR на пост-установке MATE больше не отбрасывает
//!      полосу хода назад: оригинал слал его стадией `aur` с долей 75 %
//!      посреди этапа, стоящего на 85 %.
//!   3. План установки проверяется на незнакомые шаги ДО первого из них, а
//!      не посреди исполнения: незнакомый шаг, найденный после записи
//!      скрипта, оставлял бы мусор во временном каталоге.

use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde_json::Value;

use crate::aeroshell;
use crate::backup;
use crate::compliance::{self as comp, ComplianceIo, ScanEntry};
use crate::de_progress::{self, InstallVerdict};
use crate::defaults as dfl;
use crate::distro_compat as dc;
use crate::system::{self, DistroDetector, PkgFamily};
use crate::privilege::op::{PkgSource, PrivOp};
use crate::system_exec::{RunOut, SystemExec, SystemIo};
use crate::system_plan::{self as plan, LockWaitStep, LogTailKind, PlanStep, RunStepId};
use crate::i18n::{ntr, tr, trf};
use crate::utils::{self, RunOutcome};

/// Связка «композитор + оболочка»: установка, вход и снятие.
mod shells;
pub use shells::{shell_install_live, shell_remove_live, shell_switch_live};

// ============================================================================
// Тексты хода дела
// ============================================================================

/// Стол, которого нет в базе ядра.
pub fn unknown_de_message(de_id: &str) -> String {
    // TRANSLATORS: %1 is a desktop environment id.
    trf("Rexon doesn't know the “%1” desktop.", &[&de_id])
}

/// Итог отменённой операции. Отмена возможна только пока система не
/// тронута, поэтому это обещание, а не надежда.
pub fn cancelled_message() -> String {
    tr("The operation was cancelled. The system wasn't changed.")
}
/// Заголовок окна хода после отмены — ключ этапа хода (его пишет и
/// `installer_exec` в `progress`), не подпись: подпись — [`status_cancelled`].
pub const STATUS_CANCELLED: &str = "Отменено";
/// Заголовок окна хода после отмены — на языке интерфейса.
pub fn status_cancelled() -> String {
    // TRANSLATORS: status line of the operation window after cancelling.
    tr("Cancelled")
}
/// Вторая операция, пока идёт первая.
pub fn busy_message() -> String {
    tr("A desktop operation is already running — wait until it finishes.")
}

/// Подписи кнопок окна операции. «Да» и «Нет» — как у вопроса классического
/// окна; безопасный ответ («Нет») стоит под фокусом первым кадром.
pub fn dialog_yes() -> String {
    // TRANSLATORS: button in a question dialog.
    tr("Yes")
}
pub fn dialog_no() -> String {
    // TRANSLATORS: button in a question dialog.
    tr("No")
}
/// Кнопка отмены — видна только пока система не тронута.
pub fn dialog_cancel() -> String {
    // TRANSLATORS: button that cancels a running desktop operation.
    tr("Cancel")
}

/// Строка подвала, пока окно ждёт ответа человека.
pub fn awaiting_answer(title: &str) -> String {
    // TRANSLATORS: %1 is the title of a question window.
    trf("“%1” — answer in the window that opened.", &[&title])
}

/// Строка подвала с итогом операции.
pub fn operation_footer(title: &str, status: &str) -> String {
    format!("{title} — {status}")
}

pub fn snapshot_message() -> String {
    tr("🔒 Remembering your default apps")
}
pub fn plasma_backup_message() -> String {
    tr("🛡️ Saving a copy of the KDE Plasma panel")
}
pub fn post_install_message() -> String {
    tr("🔧 Post-install setup")
}
pub fn dm_step_message() -> String {
    tr("⚙️ Login screen")
}
pub fn restore_message() -> String {
    tr("🔒 Restoring your default apps")
}
pub fn mime_message() -> String {
    tr("🛡️ Setting up which apps open files in each desktop")
}

fn snapshot_failed(e: &str) -> String {
    trf("Couldn't remember the default apps: %1", &[&e])
}

fn plasma_backup_failed(e: &str) -> String {
    trf("The KDE panel copy wasn't saved: %1", &[&e])
}

fn install_intro(main: usize, aur: usize) -> String {
    let head = ntr("📦 Installing: %n package", "📦 Installing: %n packages", main as u64);
    if aur > 0 {
        // TRANSLATORS: %1 is the sentence about packages, %2 how many more come from the AUR.
        trf("%1 + %2 from the AUR", &[&head, &aur])
    } else {
        head
    }
}

fn post_install_failed(msg: &str) -> String {
    trf("Post-install setup didn't fully finish: %1", &[&msg])
}

fn dm_already(manager: &str) -> String {
    trf("⚙️ The login screen is already set up (%1) — leaving it", &[&manager])
}

fn dm_enabled(unit: &str) -> String {
    trf("✅ Login screen %1 enabled", &[&unit])
}

fn dm_enable_failed(unit: &str, why: &str) -> String {
    trf("Couldn't enable login screen %1: %2", &[&unit, &why])
}

fn switcher_message(display: &str) -> String {
    trf("🔄 Default apps for %1", &[&display])
}

fn restore_failed(e: &str) -> String {
    trf("Couldn't restore the default apps: %1", &[&e])
}

fn installed_message(display: &str) -> String {
    // TRANSLATORS: %1 is a desktop environment name.
    trf("✅ %1 installed", &[&display])
}

fn temp_failed(e: &str) -> String {
    trf("Couldn't create a temporary file for the installation: %1", &[&e])
}

fn write_failed(path: &str, e: &str) -> String {
    trf("Couldn't write the temporary file %1: %2", &[&path, &e])
}

fn unknown_step_message(what: &str) -> String {
    trf("The installation plan has an unknown step (%1) — the installation wasn't started.", &[&what])
}

fn gentoo_overlay_intro(overlay: &str) -> String {
    trf("📦 Gentoo: adding the “%1” overlay…", &[&overlay])
}

fn gentoo_overlay_done(overlay: &str) -> String {
    trf("✅ Overlay “%1” is ready", &[&overlay])
}

fn gentoo_overlay_failed(overlay: &str) -> String {
    trf("Couldn't add the Gentoo overlay “%1”", &[&overlay])
}

fn ensure_dm_install_failed(package: &str, why: &str) -> String {
    trf("Couldn't install the login screen (%1): %2", &[&package, &why])
}

fn ensure_dm_enable_failed(why: &str) -> String {
    trf("Couldn't enable the login screen: %1", &[&why])
}

/// Код возврата либо «не ответила» — для строк хода.
fn run_why(out: &RunOut) -> String {
    match out.outcome {
        RunOutcome::Timeout => tr("the command ran out of time"),
        RunOutcome::Error => tr("the command didn't start"),
        _ => {
            let text = if !out.stderr.trim().is_empty() {
                &out.stderr
            } else {
                &out.stdout
            };
            match text.trim().lines().last() {
                Some(line) if !line.trim().is_empty() => line.trim().to_string(),
                _ => trf("code %1", &[&out.returncode.unwrap_or(-1)]),
            }
        }
    }
}

// ============================================================================
// Трейт исполнителя
// ============================================================================

/// Показания часов для имён файлов и отметок времени.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clock {
    /// `datetime.now().isoformat()`.
    pub iso: String,
    /// `datetime.now().isoformat(timespec="seconds")`.
    pub iso_seconds: String,
    /// Миллисекунды эпохи — для событий моста.
    pub epoch_ms: u64,
}

/// Событие моста BonziBuddy об операции со столом.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeEvent {
    DeInstalled(String),
    DeSwitch { from: String, to: String },
    DeRemoved(String),
}

fn removing_message(display: &str) -> String {
    trf("🗑️ Removing %1", &[&display])
}

/// Всё, что есть у удаляемого стола, нужно столу, в котором человек сидит,
/// или его экрану входа: снимать нечего.
fn remove_all_shared_message(display: &str) -> String {
    // TRANSLATORS: %1 is a desktop environment name.
    trf(
        "Nothing to remove: everything %1 consists of is also used by your current desktop or its login screen.",
        &[&display],
    )
}

fn removed_message(display: &str) -> String {
    // TRANSLATORS: %1 is a desktop environment name.
    trf("✅ %1 removed", &[&display])
}

/// Удаление не уложилось во время — это не то же, что «не удалось».
pub fn remove_timeout_message() -> String {
    tr("The removal ran out of time. Check whether another program is using the package manager.")
}

pub fn logout_message() -> String {
    tr("👋 Logging out…")
}
pub fn logout_failed_message() -> String {
    tr("Couldn't log out")
}

fn config_backup_message(display: &str) -> String {
    trf("💾 Saving a copy of the %1 settings", &[&display])
}

fn config_backup_done(name: &str, count: usize) -> String {
    // TRANSLATORS: %1 is the backup name, %n the number of files in it.
    ntr("✅ Copy saved: %1 (%n file)", "✅ Copy saved: %1 (%n files)", count as u64).replace("%1", name)
}

fn config_backup_failed(e: &str) -> String {
    trf("The settings copy wasn't saved: %1", &[&e])
}

fn logout_timeout(cmd: &str) -> String {
    // TRANSLATORS: %1 is a command name.
    trf("%1: didn't respond in time", &[&cmd])
}

fn logout_not_started(cmd: &str) -> String {
    // TRANSLATORS: %1 is a command name.
    trf("%1: didn't start", &[&cmd])
}

/// Побочные действия операций над столами СВЕРХ [`SystemIo`]. Ни одного
/// решения: каждый метод — один поход наружу или один готовый исполнитель
/// ядра.
pub trait DeOpsIo: SystemIo {
    /// Запуск с выводом (stdout и stderr вместе) в файл — для сборки AUR,
    /// чей ход читается из этого файла на лету.
    fn run_to_file(&self, argv: &[String], timeout_secs: Option<u64>, log_path: &str) -> RunOut;
    /// `tempfile.mkstemp(prefix, suffix)` → путь.
    fn temp_file(&self, prefix: &str, suffix: &str) -> Result<String, String>;
    /// `tempfile.mkdtemp(prefix, dir=parent)` → путь.
    fn temp_dir(&self, prefix: &str, parent: Option<&str>) -> Result<String, String>;
    /// Записать файл целиком и выставить права.
    fn write_file_mode(&self, path: &str, content: &str, mode: u32) -> Result<(), String>;
    /// `os.makedirs(path, exist_ok=True)`.
    fn make_dirs(&self, path: &str) -> Result<(), String>;
    /// Удалить файл; ошибки не важны.
    fn remove_file(&self, path: &str);
    /// `shutil.rmtree(path, ignore_errors=True)`.
    fn remove_tree(&self, path: &str);
    /// Монотонные секунды — для ожидания замка.
    fn elapsed_secs(&self) -> f64;
    /// Поспать.
    fn sleep_secs(&self, secs: u64);
    /// Часы.
    fn clock(&self) -> Clock;
    /// Читать растущий журнал, отдавая строки хода. `None` — читать нечем
    /// (подставной исполнитель).
    fn tail_log(
        &self,
        path: &str,
        kind: LogTailKind,
        emit: Arc<dyn Fn(&str, &str) + Send + Sync>,
    ) -> Option<plan::LogTail>;
    /// Полная копия настроек Plasma (`PlasmaProtector.backup_panel(full=True)`).
    fn plasma_backup(&self, reason: &str) -> Result<(bool, String), String>;
    /// Приложения по умолчанию под стол (`DEAppSwitcher.on_switch_de`).
    fn app_defaults_switch(&self, de_id: &str) -> (bool, Vec<String>);
    /// Копия конфигов стола (`BackupManager.create_backup`) → имя копии и
    /// сколько путей в неё легло.
    fn de_config_backup(&self, de_id: &str, paths: &[String]) -> Result<(String, usize), String>;
    /// Событие моста BonziBuddy. Мост вспомогательный — отказ не важен.
    fn bridge_notify(&self, event: &BridgeEvent);
    /// Lock-файлы пакетного менеджера, которых ждать перед его запуском.
    ///
    /// Путь — факт о машине, а не о порядке действий, поэтому приходит от
    /// исполнителя: стенд на подставных `pkexec`/`pacman` ждёт свой замок во
    /// временном каталоге, а не настоящий `/var/lib/pacman/db.lck`, который
    /// держит живой `pacman -Syu` владельца (до 300 с ожидания).
    fn pkg_lock_paths(&self, family: PkgFamily) -> Vec<String> {
        plan::lock_paths(family).iter().map(|p| p.to_string()).collect()
    }
    /// Довести переменные из `~/.config/environment.d` до сеансов, которые
    /// этот каталог не читают (`compositor_autostart::sync_session_env`).
    /// Подставной исполнитель живой профиль не трогает — по умолчанию
    /// ничего не делает; живой зовёт настоящую синхронизацию.
    fn sync_session_env(&self) {}
}

/// Куда исполнитель отдаёт ход дела. Зовётся из рабочего потока И из потока
/// чтения журнала — отсюда `Send + Sync`.
pub trait DeOpSink: Send + Sync {
    /// Сообщение о ходе: этап, текст, доля (`None` — полосу не двигать).
    fn progress(&self, stage: &str, message: &str, fraction: Option<f64>);
    /// Можно ли сейчас отменить операцию.
    fn cancellable(&self, allowed: bool);
}

/// Флаг отмены, общий для окна и рабочего потока.
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// Чем кончилась операция.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeOpOutcome {
    pub ok: bool,
    pub message: String,
    /// Отменена человеком, пока система не была тронута.
    pub cancelled: bool,
    /// Сделанное заработает только после перезагрузки (новое ядро или модуль
    /// ядра, впервые включённый экран входа) — тогда итог установки стола
    /// говорит «перезагрузите», иначе «выйдите из сеанса».
    pub restart_needed: bool,
}

/// Почему операция остановилась раньше конца.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Stop {
    Failed(String),
    Cancelled,
}

type Step<T> = Result<T, Stop>;

/// Отправитель событий: помнит, объявлялась ли ошибка (для правила
/// [`de_progress::worker_error_emit`]), и годится для потока чтения журнала.
#[derive(Clone)]
struct Emitter {
    sink: Arc<dyn DeOpSink>,
    error_seen: Arc<AtomicBool>,
}

impl Emitter {
    fn emit(&self, stage: &str, message: &str, fraction: Option<f64>) {
        if stage == de_progress::STAGE_ERROR {
            self.error_seen.store(true, Ordering::SeqCst);
        }
        self.sink.progress(stage, message, fraction);
    }

    /// Сообщение шагов пакетного менеджера: долю полосы по стадии считает
    /// ядро (`install_de_fraction`) — порт замыкания `_pkg_cb`.
    fn pkg(&self, stage: &str, message: &str) {
        self.emit(stage, message, plan::install_de_fraction(stage, message));
    }

    fn pkg_callback(&self) -> Arc<dyn Fn(&str, &str) + Send + Sync> {
        let me = self.clone();
        Arc::new(move |stage: &str, message: &str| me.pkg(stage, message))
    }
}

// ============================================================================
// Исполнитель
// ============================================================================

/// Порядок шагов операций над столами поверх [`DeOpsIo`].
pub struct DeOpsExec<'a, IO: DeOpsIo + ?Sized> {
    io: &'a IO,
    sys: SystemExec<'a, IO>,
    out: Emitter,
    cancel: CancelToken,
    cancellable: Cell<Option<bool>>,
    /// См. [`DeOpOutcome::restart_needed`]; ставят шаги операции.
    restart_needed: Cell<bool>,
}

impl<'a, IO: DeOpsIo + ?Sized> DeOpsExec<'a, IO> {
    pub fn new(io: &'a IO, sink: Arc<dyn DeOpSink>, cancel: CancelToken) -> Self {
        DeOpsExec {
            io,
            sys: SystemExec::new(io),
            out: Emitter {
                sink,
                error_seen: Arc::new(AtomicBool::new(false)),
            },
            cancel,
            cancellable: Cell::new(None),
            restart_needed: Cell::new(false),
        }
    }

    fn emit(&self, stage: &str, message: &str, fraction: Option<f64>) {
        self.out.emit(stage, message, fraction);
    }

    fn warn(&self, message: &str) {
        self.emit(de_progress::STAGE_WARNING, message, None);
    }

    fn raw(&self, message: &str) {
        self.emit(de_progress::STAGE_RAW, message, None);
    }

    fn set_cancellable(&self, allowed: bool) {
        if self.cancellable.get() != Some(allowed) {
            self.cancellable.set(Some(allowed));
            self.out.sink.cancellable(allowed);
        }
    }

    /// Точка, где отмена ещё безопасна.
    fn checkpoint(&self) -> Step<()> {
        if self.cancel.is_cancelled() {
            Err(Stop::Cancelled)
        } else {
            Ok(())
        }
    }

    /// Дальше отмены нет: пошли изменения системы.
    fn point_of_no_return(&self) -> Step<()> {
        self.checkpoint()?;
        self.set_cancellable(false);
        Ok(())
    }

    /// Итог: ошибка, о которой по ходу никто не сказал, объявляется здесь —
    /// тем же правилом ядра, что у Python-воркера.
    fn finish(&self, result: Step<String>) -> DeOpOutcome {
        self.set_cancellable(false);
        match result {
            Ok(message) => DeOpOutcome {
                ok: true,
                message,
                cancelled: false,
                restart_needed: self.restart_needed.get(),
            },
            Err(Stop::Cancelled) => {
                self.warn(&cancelled_message());
                DeOpOutcome {
                    ok: false,
                    message: cancelled_message(),
                    cancelled: true,
                    restart_needed: false,
                }
            }
            Err(Stop::Failed(message)) => {
                let seen = self.out.error_seen.load(Ordering::SeqCst);
                if let Some(text) = de_progress::worker_error_emit(false, seen, &message) {
                    self.emit(de_progress::STAGE_ERROR, &text, None);
                }
                DeOpOutcome {
                    ok: false,
                    message,
                    cancelled: false,
                    restart_needed: false,
                }
            }
        }
    }

    // ── Окружение ────────────────────────────────────────────────────────

    fn home(&self) -> String {
        let h = self.io.env("HOME");
        if h.is_empty() {
            "/".to_string()
        } else {
            h
        }
    }

    fn env_opt(&self, name: &str) -> Option<String> {
        let v = self.io.env(name);
        (!v.is_empty()).then_some(v)
    }

    fn config_home(&self) -> String {
        utils::config_home(self.env_opt("XDG_CONFIG_HOME").as_deref(), &self.home())
    }

    fn expand(&self, path: &str) -> String {
        utils::expand_user_xdg(
            path,
            &self.home(),
            self.env_opt("XDG_CONFIG_HOME").as_deref(),
            self.env_opt("XDG_DATA_HOME").as_deref(),
        )
    }

    fn detector(&self) -> DistroDetector {
        DistroDetector::from_os_release_files(
            self.io.read_text("/etc/os-release").as_deref(),
            self.io.read_text("/usr/lib/os-release").as_deref(),
        )
    }

    /// SteamOS ли под нами — тем же детектом, что решает об установке
    /// (`DistroCompat::is_steamos`), по os-release из того же ввода-вывода.
    fn on_steamos(&self) -> bool {
        dc::is_steamos_id(&self.detector().distro_id())
    }

    fn lang(&self) -> dc::Lang {
        for key in ["LC_ALL", "LC_MESSAGES", "LANG"] {
            let v = self.io.env(key);
            if !v.is_empty() {
                return dc::Lang::from_locale(&v);
            }
        }
        dc::Lang::En
    }

    fn run_ok(&self, argv: &[String]) -> bool {
        self.io.run(argv, None, false).rc() == Some(0)
    }

    // ── Совместимость дистрибутива ───────────────────────────────────────

    /// Вердикт «можно ли ставить этот стол здесь» — порт
    /// `distro_compat.explain_de_install`. Решает ядро
    /// (`DistroCompat::install_reason`); здесь только сырьё: os-release,
    /// семейство и, на Astra SE, состояние ЗПС.
    pub fn install_verdict(&self, de_id: &str) -> InstallVerdict {
        let det = self.detector();
        let compat = dc::DistroCompat::new(dc::Detector {
            distro_id: det.distro_id(),
            family: self.sys.family(),
            pretty_name: det.pretty_name(),
            variant_id: det
                .info
                .get("VARIANT_ID")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            name: det
                .info
                .get("NAME")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
        });
        // ЗПС спрашивается лениво — только на Astra SE, как у оригинала.
        let zps = det.distro_id() == "astra" && compat.is_astra_se() && self.zps_enabled();
        let reason = compat.install_reason(de_id, zps);
        let lang = self.lang();
        let verdict = InstallVerdict {
            allowed: reason.allowed(),
            outcome: reason.outcome().code().to_string(),
            title: reason.title(lang),
            explanation: reason.explanation(lang),
        };
        // Deepin — только на Treeland (`crate::deepin_treeland`): поверх
        // обычного Deepin — отказ до пароля, иначе предупреждение
        // «экспериментально» в том же вопросе.
        if de_id != crate::deepin_treeland::DE_ID || !verdict.allowed {
            return verdict;
        }
        match crate::deepin_treeland::install_verdict(self.sys.family(), &|cmd: &plan::Cmd| self.preview_run(cmd)) {
            Some((allowed, title, explanation)) => InstallVerdict {
                allowed,
                outcome: if allowed { "warning" } else { "blocked" }.to_string(),
                title,
                explanation,
            },
            None => verdict,
        }
    }

    /// Команда пробы (без прав root) с ответом в виде [`plan::PreviewAnswer`].
    fn preview_run(&self, cmd: &plan::Cmd) -> plan::PreviewAnswer {
        let r = self.io.run(&cmd.argv, cmd.timeout_secs, cmd.lang_c);
        r.rc().map(|rc| (rc, r.stdout.clone(), r.stderr.clone()))
    }

    /// Порт `DistroCompat._is_zps_enabled`: модуль `digsig_verif` в `lsmod`,
    /// затем файл конфигурации.
    fn zps_enabled(&self) -> bool {
        let r = self.io.run(&["lsmod".to_string()], None, false);
        if r.rc() == Some(0) && dc::zps_enabled_from(Some(&r.stdout), None) {
            return true;
        }
        let conf = self.io.read_text("/etc/digsig/digsig_initramfs.conf");
        dc::zps_enabled_from(None, conf.as_deref())
    }

    // ── Установка (она же проверка и восстановление) ─────────────────────

    /// Порт `install_de`. «Проверить / Восстановить» — это она же: пакетный
    /// менеджер с `--needed` сам пропускает стоящее.
    pub fn install_de(&self, de_id: &str, current_de: &str) -> DeOpOutcome {
        self.install_de_replacing(de_id, current_de, &[])
    }

    /// [`Self::install_de`] с согласием заменить установленные пакеты
    /// `replace` («Заменить River 0.5» на карточке): конфликт пробы с ними
    /// снимается той же транзакцией, если это замена из таблицы
    /// (`plan::conflict_swap_approved`). Любой другой конфликт — прежний
    /// отказ до пароля.
    pub fn install_de_replacing(&self, de_id: &str, current_de: &str, replace: &[String]) -> DeOpOutcome {
        let result = self.install_de_steps(de_id, current_de, replace);
        self.finish(result)
    }

    fn install_de_steps(&self, de_id: &str, current_de: &str, replace: &[String]) -> Step<String> {
        let Some(meta) = system::DE_META.iter().find(|m| m.id == de_id) else {
            return Err(Stop::Failed(unknown_de_message(de_id)));
        };
        let display = meta.display_name;
        self.set_cancellable(true);

        // 1. Совместимость — ДО любых действий.
        let verdict = self.install_verdict(de_id);
        if !verdict.allowed {
            let text = format!("{}\n\n{}", verdict.title, verdict.explanation);
            self.emit(de_progress::STAGE_ERROR, &text, None);
            return Err(Stop::Failed(text));
        }
        if verdict.outcome == "warning" {
            self.warn(&verdict.explanation);
        }
        self.checkpoint()?;

        let family = self.sys.family();
        let detector = self.detector();
        // Deepin — только на Treeland: набор без deepin-session (его даёт
        // своя сборка, поставленная до этого шага), иначе pacman потянул бы
        // его с deepin-kwin. Своей сборки нет — отказ, группу целиком Rexon
        // не ставит (`crate::deepin_treeland`).
        let mut main_pkgs = if de_id == crate::deepin_treeland::DE_ID {
            match crate::deepin_treeland::install_set(family, &|cmd: &plan::Cmd| self.preview_run(cmd)) {
                Ok(set) => set,
                Err(text) => {
                    self.emit(de_progress::STAGE_ERROR, &text, None);
                    return Err(Stop::Failed(text));
                }
            }
        } else {
            system::get_packages(de_id, family)
        };
        // Помеченное в базе как AUR, но лежащее в подключённых репозиториях
        // (CachyOS: темы и значки Mint в `cachyos`), ставит pacman в общей
        // транзакции — и без AUR-помощника тоже.
        let (aur_in_repo, aur_pkgs) =
            plan::split_repo_available(family, &system::get_aur_packages(de_id, family), &|cmd: &plan::Cmd| {
                let r = self.io.run(&cmd.argv, cmd.timeout_secs, cmd.lang_c);
                r.rc().map(|rc| (rc, r.stdout.clone(), r.stderr.clone()))
            });
        for p in aur_in_repo {
            if !main_pkgs.contains(&p) {
                main_pkgs.push(p);
            }
        }
        let aur_helper = plan::pick_aur_helper(family, &|h| self.io.which(h));

        // 1а. Пробная транзакция без прав root (Arch): если необязательный
        //     член группы в репозитории сейчас не ставится (08.10.2026 —
        //     switchboard-plug-online-accounts у Pantheon ждал пересборки),
        //     ставим группу без него и предупреждаем; если не ставится то,
        //     без чего стола нет, — понятный отказ ДО пароля, а не обрыв
        //     pacman посреди установки. Не ответила — ставим как раньше.
        // `deepin-kwin` (оконный менеджер Deepin X11) не ставится никогда —
        // ни членом набора, ни зависимостью.
        let preview = plan::install_preview(family, &main_pkgs, &|cmd: &plan::Cmd| self.preview_run(cmd)).map(|mut p| {
            crate::deepin_treeland::forbid_x11_wm(&mut p);
            p
        });
        // Что из набора ставится лишь зависимостью (DKMS-модуль и заголовки
        // под стоящие ядра) — помечается `--asdeps` в той же транзакции.
        let deps_only: Vec<String> = preview.as_ref().map(|p| p.deps_only.clone()).unwrap_or_default();
        // Замена с согласия: только то, что проба нашла конфликтующим и что
        // человек согласился заменить; прочие имена из `replace` не идут.
        let replace: Vec<String> = match &preview {
            Some(p) if plan::conflict_swap_approved(p, replace) => {
                plan::conflict_swaps(p).into_iter().map(|c| c.installed).collect()
            }
            _ => Vec::new(),
        };
        // Перезагрузка нужна, если встаёт ядро или модуль ядра
        // (`plan::restart_packages`) либо экрана входа до установки не было:
        // включённый сейчас он поднимется только при старте, а без него
        // «выбрать стол на экране входа» негде.
        if preview.as_ref().is_some_and(|p| !p.restart_packages.is_empty()) || !self.sys.display_manager_present() {
            self.restart_needed.set(true);
        }
        let main_pkgs = match preview {
            Some(p) if !p.installable() && !replace.is_empty() => p.targets,
            Some(p) if !p.installable() => {
                let text = crate::qt_desktops::packages_notice(&p);
                self.emit(de_progress::STAGE_ERROR, &text, None);
                return Err(Stop::Failed(text));
            }
            Some(p) if !p.skipped.is_empty() => {
                self.warn(&crate::qt_desktops::packages_notice(&p));
                p.targets
            }
            // Проба могла добавить в набор DKMS-модуль и заголовки под
            // стоящие ядра (`plan::kernel_module_extras`) — ставим то, что
            // проверено.
            Some(p) => p.targets,
            None => main_pkgs,
        };
        self.checkpoint()?;

        // 2. Снимок умолчаний и копия панели — до пакетного менеджера.
        self.emit("snapshot", &snapshot_message(), Some(0.02));
        if let Err(e) = self.defaults_snapshot() {
            self.warn(&snapshot_failed(&e));
        }
        if backup::is_plasma_desktop(&self.io.env("XDG_CURRENT_DESKTOP")) {
            self.emit("plasma_backup", &plasma_backup_message(), Some(0.04));
            match self.io.plasma_backup("before_install") {
                Ok((true, _)) => {}
                Ok((false, msg)) | Err(msg) => self.warn(&plasma_backup_failed(&msg)),
            }
        }
        self.checkpoint()?;

        // 3. Пакеты.
        self.emit("install", &install_intro(main_pkgs.len(), aur_pkgs.len()), Some(0.05));
        let aur_for_batch: Vec<String> = if aur_helper.is_some() {
            aur_pkgs.clone()
        } else {
            Vec::new()
        };
        let used_de_helper = match self.install_batch(
            family,
            &detector,
            &main_pkgs,
            &deps_only,
            &replace,
            &aur_for_batch,
            aur_helper.as_deref(),
            de_id,
        ) {
            Ok(used) => used,
            Err(Stop::Failed(msg)) => {
                self.emit(de_progress::STAGE_ERROR, &msg, None);
                return Err(Stop::Failed(msg));
            }
            Err(stop) => return Err(stop),
        };
        // Сюда можно прийти, так и не запустив пакетный менеджер (нечего
        // ставить) — дальше всё равно идут изменения системы.
        self.set_cancellable(false);

        // 4. Пост-установка.
        self.emit("post_install", &post_install_message(), Some(0.85));
        let (post_ok, post_msg) = self.run_post_install(de_id, family, aur_helper.as_deref());
        if !post_ok {
            self.warn(&post_install_failed(&post_msg));
        }

        // 5. Экран входа из пакет-листа стола (если помощник его уже не
        //    включил в том же сеансе авторизации).
        self.emit("dm_enable", &dm_step_message(), Some(0.90));
        if !used_de_helper {
            if self.sys.display_manager_present() {
                self.emit("dm_enable", &dm_already(&self.sys.active_login_manager()), None);
            } else {
                let mut all = main_pkgs.clone();
                all.extend(aur_pkgs.iter().cloned());
                if let Some((unit, cmd)) = plan::dm_enable_step(
                    family,
                    &all,
                    false,
                    self.sys.pkg_helper_available(),
                    false,
                ) {
                    // Сеанс прав окна; его нет — прежняя команда плана.
                    let op = PrivOp::EnableDm { family, unit: unit.clone() };
                    let r = self
                        .io
                        .run_priv(&op, &[], None)
                        .unwrap_or_else(|| self.io.run(&cmd.argv, cmd.timeout_secs, cmd.lang_c));
                    if r.rc() == Some(0) {
                        self.emit("dm_enable", &dm_enabled(&unit), None);
                    } else {
                        self.warn(&dm_enable_failed(&unit, &run_why(&r)));
                    }
                }
            }
        }

        // 6. Защита от дурака: включённый экран входа обязан быть.
        self.ensure_display_manager(family);

        // 7. Приложения по умолчанию под новый стол.
        self.emit("switcher", &switcher_message(display), Some(0.93));
        let (_, lines) = self.io.app_defaults_switch(de_id);
        for line in lines {
            self.raw(&line);
        }

        self.io
            .bridge_notify(&BridgeEvent::DeInstalled(de_id.to_string()));

        // 8. Вернуть защищённые умолчания, которые установка перетёрла.
        self.emit("restore", &restore_message(), Some(0.96));
        match self.defaults_restore() {
            Ok(results) => {
                for (app_type, status) in results {
                    self.raw(&format!("  {app_type}: {status}"));
                }
            }
            Err(e) => self.warn(&restore_failed(&e)),
        }

        // 9. Свой mimeapps.list у каждого установленного стола.
        self.emit("mime", &mime_message(), Some(0.98));
        let mut installed: Vec<String> = system::DE_META
            .iter()
            .filter(|m| {
                let pkg = system::get_check_binary(m.id).unwrap_or(m.id);
                self.sys.is_pkg_installed(pkg)
            })
            .map(|m| m.id.to_string())
            .collect();
        if !installed.iter().any(|d| d == de_id) {
            installed.push(de_id.to_string());
        }
        for line in self.protect_de_mimeapps(current_de, &installed) {
            self.raw(&line);
        }

        self.emit("done", &installed_message(display), Some(1.0));
        Ok(de_progress::MSG_OK.to_string())
    }

    // ── Переключение на другой стол ──────────────────────────────────────

    /// Порт `switch_de`: копия панели и конфигов текущего стола,
    /// приложения по умолчанию под новый, событие моста, выход из сессии.
    pub fn switch_de(&self, target: &str, current_de: &str) -> DeOpOutcome {
        let result = self.switch_de_steps(target, current_de);
        self.finish(result)
    }

    fn switch_de_steps(&self, target: &str, current_de: &str) -> Step<String> {
        // Левая часть карточки: стол САМ ПО СЕБЕ — отметки оболочек этого
        // композитора снимаются (см. `crate::shell_bundle`).
        self.switch_steps_with(target, current_de, None)
    }

    /// Переключение на стол; `shell` — войти в него с этой оболочкой
    /// (отметка режима кладётся, а не снимается).
    fn switch_steps_with(
        &self,
        target: &str,
        current_de: &str,
        shell: Option<system::DesktopShell>,
    ) -> Step<String> {
        let Some(meta) = system::DE_META.iter().find(|m| m.id == target) else {
            return Err(Stop::Failed(unknown_de_message(target)));
        };
        // Защита ядра: на текущий и на неопознанный стол не переключаемся.
        if let Some(refusal) = plan::switch_de_guard(current_de, target, meta.display_name) {
            return Err(Stop::Failed(refusal));
        }
        self.set_cancellable(true);

        if backup::is_plasma_desktop(&self.io.env("XDG_CURRENT_DESKTOP")) {
            self.emit("plasma_backup", &plasma_backup_message(), Some(0.10));
            match self.io.plasma_backup("before_switch") {
                Ok((true, _)) => {}
                Ok((false, msg)) | Err(msg) => self.warn(&plasma_backup_failed(&msg)),
            }
        }
        self.checkpoint()?;

        if let Some(current) = system::DE_META.iter().find(|m| m.id == current_de) {
            self.emit(
                "config_backup",
                &config_backup_message(current.display_name),
                Some(0.30),
            );
            let home = self.home();
            let paths: Vec<String> = system::get_config_paths(current.id)
                .iter()
                .map(|p| self.expand(p).replace('~', &home))
                .collect();
            match self.io.de_config_backup(current.id, &paths) {
                Ok((name, count)) => {
                    self.emit("config_backup", &config_backup_done(&name, count), None)
                }
                Err(e) => self.warn(&config_backup_failed(&e)),
            }
        }

        // Дальше меняются приложения по умолчанию и закрывается сессия.
        self.point_of_no_return()?;
        self.set_shell_mode(target, shell)?;
        self.emit("switcher", &switcher_message(meta.display_name), Some(0.60));
        let (_, lines) = self.io.app_defaults_switch(target);
        for line in lines {
            self.raw(&line);
        }
        self.io.bridge_notify(&BridgeEvent::DeSwitch {
            from: current_de.to_string(),
            to: target.to_string(),
        });

        self.emit("done", &logout_message(), Some(1.0));
        self.logout_session()
    }

    /// Порт `logout_session`: команды по окружению — ядро
    /// (`logout_cmds`), первая успешная выигрывает.
    fn logout_session(&self) -> Step<String> {
        let cmds = plan::logout_cmds(
            &self.io.env("XDG_CURRENT_DESKTOP"),
            &self.io.env("DESKTOP_SESSION"),
            &self.io.env("XDG_SESSION_ID"),
        );
        let mut last = String::new();
        for cmd in cmds {
            let name = cmd.argv.first().cloned().unwrap_or_default();
            let r = self.io.run(&cmd.argv, cmd.timeout_secs, cmd.lang_c);
            match r.outcome {
                RunOutcome::Ok => return Ok(de_progress::MSG_OK.to_string()),
                RunOutcome::Timeout => last = logout_timeout(&name),
                RunOutcome::Error => last = logout_not_started(&name),
                RunOutcome::Failed => last = r.stderr.trim().to_string(),
            }
        }
        Err(Stop::Failed(if last.is_empty() {
            logout_failed_message()
        } else {
            last
        }))
    }

    // ── Удаление стола ───────────────────────────────────────────────────

    /// Порт `remove_de(force=False)`. ⛔ Обхода защиты у окна НЕТ: текущий
    /// стол ядро не удалит даже прямым вызовом (правило архитектуры #13,
    /// второй рубеж — кнопки на текущем столе нет вовсе).
    pub fn remove_de(&self, de_id: &str, current_de: &str) -> DeOpOutcome {
        let result = self.remove_de_steps(de_id, current_de);
        self.finish(result)
    }

    fn remove_de_steps(&self, de_id: &str, current_de: &str) -> Step<String> {
        let Some(meta) = system::DE_META.iter().find(|m| m.id == de_id) else {
            return Err(Stop::Failed(unknown_de_message(de_id)));
        };
        // SteamOS (Steam Deck и Steam Machine): пакетов не трогаем ни в какую
        // сторону — удаление потребовало бы снять защиту системного раздела,
        // а следующее обновление вернуло бы всё как было. Отказ — до первого
        // побочного действия, тем же текстом, что стоит у неактивной кнопки.
        if let Some(reason) = dc::SteamOsBlock::Desktops.on(self.on_steamos()) {
            self.emit(de_progress::STAGE_ERROR, &reason, None);
            return Err(Stop::Failed(reason));
        }
        let family = self.sys.family();
        let packages = system::get_packages(de_id, family);
        let aur = system::get_aur_packages(de_id, family);
        let all = match plan::remove_de_decision(
            de_id,
            meta.display_name,
            current_de,
            false,
            &packages,
            &aur,
        ) {
            plan::RemoveDeDecision::Refused(text) => return Err(Stop::Failed(text)),
            plan::RemoveDeDecision::Proceed(all) => all,
        };
        // Пакеты текущего стола и его экран входа не снимаются никогда
        // (общий `lightdm` у Xfce и Cinnamon) — `system::current_desktop_keep`.
        let keep = system::current_desktop_keep(
            family,
            current_de,
            self.sys.active_login_manager_opt().as_deref(),
        );
        let group = |p: &str| self.group_members_output(family, p);
        let all = system::without_current_desktop(family, &all, &keep, &group);
        if all.is_empty() {
            let message = remove_all_shared_message(meta.display_name);
            self.emit(de_progress::STAGE_ERROR, &message, None);
            return Err(Stop::Failed(message));
        }
        self.set_cancellable(true);
        self.emit("remove", &removing_message(meta.display_name), Some(0.10));
        let (ok, message) = self.pkg_remove(family, &all)?;
        if !ok {
            self.emit(de_progress::STAGE_ERROR, &message, None);
            return Err(Stop::Failed(message));
        }
        self.io
            .bridge_notify(&BridgeEvent::DeRemoved(de_id.to_string()));
        self.emit("done", &removed_message(meta.display_name), Some(1.0));
        Ok(de_progress::MSG_OK.to_string())
    }

    /// Порт `PackageManager.remove`: пробы (группы, установлен ли, кому
    /// нужен) исполняются при сборке плана ядра, здесь — ожидание замка и
    /// сама команда удаления.
    /// Вывод вопроса «что входит в группу» (`pacman -Qg`) при успехе.
    fn group_members_output(&self, family: PkgFamily, p: &str) -> Option<String> {
        let q = plan::package_group_query(family, p)?;
        if !self.io.which(&q.binary) {
            return None;
        }
        let r = self.io.run(&q.cmd.argv, q.cmd.timeout_secs, q.cmd.lang_c);
        (r.rc() == Some(0)).then_some(r.stdout)
    }

    fn pkg_remove(&self, family: PkgFamily, packages: &[String]) -> Step<(bool, String)> {
        let group = |p: &str| self.group_members_output(family, p);
        let meta = |p: &str| self.sys.pkg_meta_stdout(p, family);
        let installed = |p: &str| self.sys.is_pkg_installed(p);
        let removal = plan::remove_plan(
            family,
            packages,
            &group,
            &meta,
            &installed,
            self.sys.pkg_helper_available(),
        )
        .map_err(Stop::Failed)?;
        if let Some((ok, msg)) = removal.result_without_exec {
            return Ok((ok, msg));
        }
        for step in &removal.steps {
            match step {
                PlanStep::WaitPkgLock { timeout_secs, .. } => {
                    // Пути замка — от исполнителя (см. `DeOpsIo::pkg_lock_paths`).
                    let paths = self.io.pkg_lock_paths(family);
                    let (ok, msg) = self.wait_pkg_lock(&paths, *timeout_secs)?;
                    if !ok {
                        return Ok((false, msg));
                    }
                }
                PlanStep::Run {
                    id: RunStepId::PkgRemove,
                    cmd,
                    ..
                } => {
                    self.point_of_no_return()?;
                    // Сеанс прав окна снимает тот же набор (`removal.safe`);
                    // его нет — прежняя команда плана.
                    let op = PrivOp::PkgRemove { family, packages: removal.safe.clone() };
                    let r = self
                        .io
                        .run_priv(&op, &[], None)
                        .unwrap_or_else(|| self.io.run(&cmd.argv, cmd.timeout_secs, cmd.lang_c));
                    if r.outcome == RunOutcome::Error {
                        return Ok((false, tr("Command not found")));
                    }
                    if r.outcome == RunOutcome::Timeout {
                        return Ok((false, remove_timeout_message()));
                    }
                    if r.returncode == Some(0) {
                        // Человеку — только что пропущено (нужно другим
                        // столам); вывод пакетного менеджера остаётся итогом
                        // для кода, как у оригинала.
                        if !removal.success_note.trim().is_empty() {
                            self.raw(removal.success_note.trim());
                        }
                        let removed = tr("Removed");
                        let out = if r.stdout.is_empty() {
                            removed.as_str()
                        } else {
                            r.stdout.as_str()
                        };
                        return Ok((true, format!("{out}{}", removal.success_note)));
                    }
                    let mut err = if !r.stderr.is_empty() {
                        r.stderr.clone()
                    } else if !r.stdout.is_empty() {
                        r.stdout.clone()
                    } else {
                        tr("Removal error")
                    };
                    if err.contains("Not authorized") || r.returncode == Some(126) {
                        err = removal.sudo_hint.clone();
                    }
                    return Ok((false, err));
                }
                _ => {}
            }
        }
        Ok((true, String::new()))
    }

    // ── Пакетная установка ───────────────────────────────────────────────

    /// Порт `PackageManager.install_batch` → использовался ли rexon-de-helper.
    fn install_batch(
        &self,
        family: PkgFamily,
        detector: &DistroDetector,
        packages: &[String],
        deps_only: &[String],
        replace: &[String],
        aur_packages: &[String],
        aur_helper: Option<&str>,
        de_id: &str,
    ) -> Step<bool> {
        let steamos = detector.is_steamos();
        let prefix = if steamos {
            "rexon_steamos_"
        } else {
            "rexon_install_"
        };
        let tmp_sh = self
            .io
            .temp_file(prefix, ".sh")
            .map_err(|e| Stop::Failed(temp_failed(&e)))?;
        let tmp_log = match self.io.temp_file(prefix, ".log") {
            Ok(p) => p,
            Err(e) => {
                self.io.remove_file(&tmp_sh);
                return Err(Stop::Failed(temp_failed(&e)));
            }
        };
        let batch = match plan::install_batch_plan_with_deps(
            family,
            detector,
            packages,
            deps_only,
            replace,
            aur_packages,
            aur_helper,
            true,
            Some(de_id),
            self.sys.de_helper_available(),
            &tmp_sh,
            &tmp_log,
        ) {
            Ok(b) => b,
            Err(e) => {
                self.io.remove_file(&tmp_sh);
                self.io.remove_file(&tmp_log);
                return Err(Stop::Failed(e));
            }
        };

        // Незнакомый шаг — отказ ДО первого действия (пункт 3 шапки).
        for step in &batch.steps {
            if let PlanStep::Run { id, .. } = step {
                if !matches!(
                    id,
                    RunStepId::PkgMain | RunStepId::PkgNix | RunStepId::PkgAur | RunStepId::PkgUpdateDb
                ) {
                    self.io.remove_file(&tmp_sh);
                    self.io.remove_file(&tmp_log);
                    return Err(Stop::Failed(unknown_step_message(id.as_str())));
                }
            }
        }

        // Скрипт-обёртка нужна только прежнему пути; иначе пустой файл долой.
        let writes_sh = batch
            .steps
            .iter()
            .any(|s| matches!(s, PlanStep::WriteFile { path, .. } if *path == tmp_sh));
        let tmp_sh = if writes_sh {
            Some(tmp_sh)
        } else {
            self.io.remove_file(&tmp_sh);
            None
        };
        let cleanup = || {
            if let Some(p) = &tmp_sh {
                self.io.remove_file(p);
            }
            self.io.remove_file(&tmp_log);
        };

        if let Some((ok, msg)) = &batch.early_result {
            cleanup();
            if !ok {
                let stage = if family == PkgFamily::Nix {
                    "nixos"
                } else {
                    "immutable"
                };
                self.out.pkg(stage, msg);
                return Err(Stop::Failed(msg.clone()));
            }
            return Ok(false);
        }

        if steamos {
            self.out.pkg("steamos", &tr("🎮 SteamOS: installing through steamos-readonly…"));
            self.out
                .pkg("steamos", &tr("⚠️ Packages may be removed when SteamOS updates!"));
        } else if !packages.is_empty() {
            self.out.pkg("update", &tr("📥 Updating the package database…"));
        }

        // Сеансу прав окна: простой набор стола (все имена — из его таблицы,
        // без DKMS и замены) — `PkgInstall` по таблице; набор сверх неё
        // (Deepin на Treeland, член группы, DKMS `--asdeps`, замена
        // конфликтующего) — `DeInstallSet`, его правило ядра помощник
        // повторяет сам по pacman машины. Сеанса нет — прежняя обёртка
        // плана. Nix — без прав root; SteamOS ставит столы образом
        // (`crate::steamos_sysext`), сюда приходит только прежним путём.
        let plain = deps_only.is_empty()
            && replace.is_empty()
            && de_id != crate::deepin_treeland::DE_ID
            && crate::privilege::validate::check_install(
                family,
                packages,
                crate::privilege::validate::InstallSource::Desktop { de_id },
            )
            .is_ok();
        let session_op = if family == PkgFamily::Nix || steamos || packages.is_empty() {
            None
        } else if plain {
            Some(PrivOp::PkgInstall {
                family,
                packages: packages.to_vec(),
                source: PkgSource::Desktop { de_id: de_id.to_string() },
            })
        } else if family == PkgFamily::Arch {
            Some(PrivOp::DeInstallSet {
                family,
                de_id: Some(de_id.to_string()),
                packages: packages.to_vec(),
                // Как в плане: пометка — только того, что есть в наборе.
                deps_only: deps_only.iter().filter(|d| packages.contains(d)).cloned().collect(),
                replace: replace.to_vec(),
                // Обёртка плана обновляет базу перед установкой — так же.
                update_first: true,
            })
        } else {
            None
        };
        let mut session_used = false;
        let mut r_main: Option<RunOut> = None;
        let mut main_seen = false;
        // Ожидание замка перед главным шагом фатально; перед хвостом AUR —
        // нет (как у оригинала).
        let mut main_pending = !packages.is_empty();

        for step in &batch.steps {
            match step {
                PlanStep::WriteFile {
                    path,
                    content,
                    mode,
                } => {
                    if let Err(e) = self.io.write_file_mode(path, content, *mode) {
                        cleanup();
                        return Err(Stop::Failed(write_failed(path, &e)));
                    }
                }
                PlanStep::EnsureGentooOverlay { overlay } => {
                    if let Err(stop) = self.point_of_no_return() {
                        cleanup();
                        return Err(stop);
                    }
                    self.out.pkg("overlay", &gentoo_overlay_intro(overlay));
                    if !self.ensure_gentoo_overlay(family, de_id) {
                        cleanup();
                        return Err(Stop::Failed(gentoo_overlay_failed(overlay)));
                    }
                    self.out.pkg("overlay_done", &gentoo_overlay_done(overlay));
                }
                PlanStep::WaitPkgLock { timeout_secs, .. } => match self
                    .wait_pkg_lock(&self.io.pkg_lock_paths(family), *timeout_secs)
                {
                    Err(stop) => {
                        cleanup();
                        return Err(stop);
                    }
                    Ok((false, msg)) if main_pending => {
                        cleanup();
                        return Err(Stop::Failed(msg));
                    }
                    Ok(_) => {}
                },
                PlanStep::RemoveFiles { paths } => {
                    for p in paths {
                        self.io.remove_file(p);
                    }
                }
                PlanStep::Run { id, cmd, .. } => match id {
                    RunStepId::PkgMain => {
                        if let Err(stop) = self.point_of_no_return() {
                            cleanup();
                            return Err(stop);
                        }
                        self.out
                            .pkg("install", &plan::pkg_install_start_message(packages.len(), steamos));
                        let mut tail =
                            self.io
                                .tail_log(&tmp_log, LogTailKind::Pacman, self.out.pkg_callback());
                        // Ход помощника сеанса — в тот же журнал, что читает
                        // `tail_log`, поэтому строки человек видит те же.
                        let via_session =
                            session_op.as_ref().and_then(|op| self.io.run_priv(op, &[], Some(&tmp_log)));
                        session_used = via_session.is_some();
                        let r = via_session
                            .unwrap_or_else(|| self.io.run(&cmd.argv, cmd.timeout_secs, cmd.lang_c));
                        if let Some(t) = tail.as_mut() {
                            t.stop();
                        }
                        r_main = Some(r);
                        main_seen = true;
                        main_pending = false;
                    }
                    RunStepId::PkgNix => {
                        if let Err(stop) = self.point_of_no_return() {
                            cleanup();
                            return Err(stop);
                        }
                        r_main = Some(self.io.run(&cmd.argv, cmd.timeout_secs, cmd.lang_c));
                        main_seen = true;
                        main_pending = false;
                    }
                    RunStepId::PkgAur => {
                        // Итог главного шага проверяется ДО хвоста AUR.
                        let (ok, msg) =
                            self.batch_main_result(r_main.as_ref(), main_seen, packages, steamos, family);
                        if !ok {
                            return Err(Stop::Failed(msg));
                        }
                        main_seen = false;
                        if let Err(stop) = self.point_of_no_return() {
                            return Err(stop);
                        }
                        self.batch_run_aur(&cmd.argv, cmd.timeout_secs, aur_packages, steamos);
                    }
                    _ => {
                        // Обновление базы на Nix — простая команда.
                        let _ = self.io.run(&cmd.argv, cmd.timeout_secs, cmd.lang_c);
                    }
                },
            }
        }

        if main_seen {
            let (ok, msg) =
                self.batch_main_result(r_main.as_ref(), main_seen, packages, steamos, family);
            if !ok {
                return Err(Stop::Failed(msg));
            }
        }
        // Экран входа включает DE-помощник; при сеансе прав его не было —
        // включение идёт отдельным шагом (`EnableDm`).
        Ok(batch.used_de_helper && !session_used)
    }

    /// Порт `_batch_main_result`: тексты и строки хода — ядро.
    fn batch_main_result(
        &self,
        r: Option<&RunOut>,
        main_seen: bool,
        packages: &[String],
        steamos: bool,
        family: PkgFamily,
    ) -> (bool, String) {
        let result = r.and_then(|r| {
            r.rc()
                .map(|rc| (rc, r.stderr.as_str(), r.stdout.as_str()))
        });
        let agent = || self.polkit_agent_running();
        let installed = || {
            plan::POLKIT_AGENT_PATHS.iter().find(|p| self.io.is_file(p)).map(|p| p.to_string())
        };
        let out = plan::batch_main_result(result, main_seen, packages, steamos, family, &agent, &installed);
        for (stage, message) in &out.logs {
            self.out.pkg(stage, message);
        }
        (out.ok, out.message)
    }

    /// Запущено ли окно пароля (агент polkit) для нашего сеанса — спрашиваем
    /// сам polkit (`plan::polkit_agent_probe_argv`). Не вышло спросить —
    /// считаем, что агент есть (как у оригинала).
    ///
    /// ⚠️ Починка при порте: оригинал искал `pgrep -f "polkit.*agent|polkitd"`,
    /// а под `polkitd` попадает системная служба, которая работает всегда, —
    /// ответ был «да» и без единого агента.
    fn polkit_agent_running(&self) -> bool {
        let mut session = self.io.env("XDG_SESSION_ID");
        if session.is_empty() {
            let r = self.io.run(&plan::login_session_argv(), Some(10), true);
            if r.rc() == Some(0) {
                session = r.stdout.trim().to_string();
            }
        }
        if !plan::login_session_id_ok(&session) {
            return true;
        }
        let r = self.io.run(&plan::polkit_agent_probe_argv(&session), Some(10), true);
        plan::polkit_agent_probe_answer(r.rc(), &r.stderr).unwrap_or(true)
    }

    /// Порт `_batch_run_aur`.
    fn batch_run_aur(
        &self,
        argv: &[String],
        timeout_secs: Option<u64>,
        aur_packages: &[String],
        steamos: bool,
    ) {
        let (stage, intro) = plan::batch_aur_intro(steamos, aur_packages.len());
        self.out.pkg(&stage, &intro);
        let emit = self.out.pkg_callback();
        let forward: Arc<dyn Fn(&str, &str) + Send + Sync> =
            Arc::new(move |_stage: &str, msg: &str| emit("aur", msg));
        let r = self.run_aur_with_progress(argv, timeout_secs, forward);
        let ok = r.rc() == Some(0);
        let (mut failed, mut installed) = (Vec::new(), Vec::new());
        if !steamos && !ok {
            for p in aur_packages {
                if self.sys.is_pkg_installed(p) {
                    installed.push(p.clone());
                } else {
                    failed.push(p.clone());
                }
            }
        }
        for (stage, message) in plan::batch_aur_result(
            steamos,
            ok,
            aur_packages.len(),
            &failed,
            &installed,
            r.outcome == RunOutcome::Timeout,
        ) {
            self.out.pkg(&stage, &message);
        }
        // Почему не встало — словами, из вывода помощника, а не «смотрите
        // журнал» (`aur_install::failure_cause`).
        if !ok && r.outcome != RunOutcome::Timeout && (steamos || !failed.is_empty()) {
            if let Some(cause) = crate::aur_install::failure_cause(&r.stdout) {
                let reason = crate::aur_install::failure_text(&cause);
                self.out.pkg("aur_warn", &format!("   {}", trf("Reason: %1", &[&reason])));
            }
        }
    }

    /// Порт `_run_aur_with_progress`: вывод сборки — в файл, ход — читателем
    /// ядра (вид `aur`).
    fn run_aur_with_progress(
        &self,
        argv: &[String],
        timeout_secs: Option<u64>,
        emit: Arc<dyn Fn(&str, &str) + Send + Sync>,
    ) -> RunOut {
        let Ok(log) = self.io.temp_file("rexon_aur_", ".log") else {
            return RunOut::missing();
        };
        let mut tail = self.io.tail_log(&log, LogTailKind::Aur, emit);
        let mut r = self.io.run_to_file(argv, timeout_secs, &log);
        if let Some(t) = tail.as_mut() {
            t.stop();
        }
        let text = self.io.read_text(&log).unwrap_or_default();
        self.io.remove_file(&log);
        if !r.no_answer() {
            r.stdout = text;
            r.stderr = String::new();
        }
        r
    }

    /// Ожидание замка пакетного менеджера с отменой. Решение на каждом
    /// обороте — ядро (`lock_wait_step`); здесь часы, сон и флаг отмены.
    fn wait_pkg_lock(&self, paths: &[String], timeout_secs: u64) -> Step<(bool, String)> {
        let start = self.io.elapsed_secs();
        let mut notified = false;
        loop {
            self.checkpoint()?;
            let locked = !paths.is_empty() && paths.iter().any(|p| self.io.exists(p));
            match plan::lock_wait_step(locked, self.io.elapsed_secs() - start, timeout_secs, notified)
            {
                LockWaitStep::Ready { message } => return Ok((true, message)),
                LockWaitStep::TimedOut { message } => return Ok((false, message)),
                LockWaitStep::Notify {
                    stage,
                    message,
                    poll_secs,
                } => {
                    notified = true;
                    self.out.pkg(&stage, &message);
                    self.io.sleep_secs(poll_secs);
                }
                LockWaitStep::Wait { poll_secs } => self.io.sleep_secs(poll_secs),
            }
        }
    }

    /// Порт `ensure_gentoo_overlay`.
    fn ensure_gentoo_overlay(&self, family: PkgFamily, de_id: &str) -> bool {
        if family != PkgFamily::Gentoo {
            return true;
        }
        let Some(overlay) = plan::gentoo_overlay_for(de_id) else {
            return true;
        };
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let listed = self
            .io
            .run(&s(&["eselect", "repository", "list", "-i"]), None, false);
        if !listed.no_answer() && listed.stdout.contains(overlay) {
            return true;
        }
        if self.io.which("eselect") {
            if let Some(tool) =
                plan::gentoo_overlay_tool_install_cmd(family, self.sys.pkg_helper_available())
            {
                // Сеанс прав окна; его нет — прежняя команда.
                let op = PrivOp::PkgInstall {
                    family,
                    packages: vec![plan::GENTOO_OVERLAY_TOOL_PACKAGE.to_string()],
                    source: PkgSource::GentooTool,
                };
                if self.io.run_priv(&op, &[], None).is_none() {
                    let _ = self.io.run(&tool, None, false);
                }
            }
            let enabled = self.io.run(
                &s(&["pkexec", "eselect", "repository", "enable", overlay]),
                None,
                false,
            );
            if enabled.rc() == Some(0) {
                if let Some(sync) = plan::sync_repo_cmd(family, overlay) {
                    let _ = self.io.run(&sync, None, false);
                }
                return true;
            }
        }
        if self.io.which("layman") {
            let r = self
                .io
                .run(&s(&["pkexec", "layman", "-a", overlay]), Some(300), false);
            if r.rc() == Some(0) {
                return true;
            }
        }
        false
    }

    /// Порт `ensure_display_manager`: решение — [`plan::ensure_dm_plan`].
    /// Не рушит установку: экран входа — лучшее усилие поверх уже
    /// поставленного стола, но каждая неудача сказана вслух.
    fn ensure_display_manager(&self, family: PkgFamily) {
        let dm_plan = plan::ensure_dm_plan(
            family,
            self.sys.display_manager_present(),
            self.io.which("systemctl"),
            self.sys.pkg_helper_available(),
        );
        match dm_plan {
            plan::EnsureDmPlan::Present { message, .. } => {
                self.emit("dm_ensure", &message, None);
            }
            plan::EnsureDmPlan::Skipped { message, .. } => {
                if let Some(m) = message {
                    self.emit("dm_ensure", &m, None);
                }
            }
            plan::EnsureDmPlan::Install {
                message,
                fraction,
                package,
                install,
                enable,
                done_message,
                done_fraction,
                ..
            } => {
                self.emit("dm_ensure", &message, Some(fraction));
                // Сеанс прав окна; его нет — прежние команды плана.
                let op = PrivOp::PkgInstall {
                    family,
                    packages: vec![package.clone()],
                    source: PkgSource::DisplayManager,
                };
                let r = self
                    .io
                    .run_priv(&op, &[], None)
                    .unwrap_or_else(|| self.io.run(&install.argv, install.timeout_secs, install.lang_c));
                if r.rc() != Some(0) {
                    self.warn(&ensure_dm_install_failed(&package, &run_why(&r)));
                    return;
                }
                // Защита ставит и включает именно SDDM (`plan::ensure_dm_plan`).
                let op = PrivOp::EnableDm { family, unit: "sddm".to_string() };
                let r = self
                    .io
                    .run_priv(&op, &[], None)
                    .unwrap_or_else(|| self.io.run(&enable.argv, enable.timeout_secs, enable.lang_c));
                if r.rc() != Some(0) {
                    self.warn(&ensure_dm_enable_failed(&run_why(&r)));
                    return;
                }
                self.emit("dm_ensure", &done_message, Some(done_fraction));
            }
        }
    }

    // ── Пост-установка ───────────────────────────────────────────────────

    /// Порт `run_post_install`: хуки у MATE (вид Ubuntu MATE) и у River
    /// (стартовый init — без него River стартует без клавиш). Тайловым и
    /// Wayfire — ещё раскладка клавиатуры в конфиг
    /// ([`crate::keyboard_layout`]): сами они включают одну `us`.
    fn run_post_install(
        &self,
        de_id: &str,
        family: PkgFamily,
        aur_helper: Option<&str>,
    ) -> (bool, String) {
        let (ok, message) = match de_id {
            "mate" => self.post_install_mate(family, aur_helper),
            "river" => self.post_install_river(),
            _ => (true, String::new()),
        };
        // Неудача раскладки — предупреждение, а не провал: стол уже стоит.
        let keyboard = crate::keyboard_layout::ensure_after_install(self.io, de_id, &self.home());
        match &keyboard {
            Some((true, line)) => self.post(line),
            Some((false, line)) => self.warn(line),
            None => {}
        }
        match (message.is_empty(), keyboard) {
            (true, Some((_, line))) => (ok, line),
            (true, None) => (ok, tr("No post-install actions")),
            (false, _) => (ok, message),
        }
    }

    /// Стартовый init River — только если своего ещё нет
    /// ([`plan::river_starter_init`]). Не вышло — предупреждение, а не
    /// провал установки: пакеты уже стоят.
    fn post_install_river(&self) -> (bool, String) {
        let path = plan::river_init_path(&self.io.env("XDG_CONFIG_HOME"), &self.home());
        if self.io.exists(&path) {
            let line = plan::river_init_kept(&path);
            self.post(&line);
            return (true, line);
        }
        let dir = path.rsplit_once('/').map(|(d, _)| d).unwrap_or(".");
        let written = self
            .io
            .make_dirs(dir)
            .and_then(|()| self.io.write_file_mode(&path, plan::river_starter_init(), plan::RIVER_INIT_MODE));
        match written {
            Ok(()) => {
                let line = plan::river_init_written(&path);
                self.post(&line);
                (true, line)
            }
            Err(e) => {
                let line = plan::river_init_failed(&path, &e);
                (false, line)
            }
        }
    }

    fn post(&self, message: &str) {
        self.out.pkg("post", message);
    }

    /// Порт `_post_install_mate`: доводка MATE до вида Ubuntu MATE.
    fn post_install_mate(&self, family: PkgFamily, aur_helper: Option<&str>) -> (bool, String) {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let mut errors: Vec<String> = Vec::new();

        // 1. Подписи «Общих задач» Центра управления на языке человека.
        self.post(&tr("🌐 Fixing the MATE Control Center localization…"));
        let lang = self.io.env("LANG");
        match plan::mate_locale_actions(&lang) {
            Some((key, actions)) => {
                let argv = s(&[
                    "gsettings",
                    "set",
                    "org.mate.control-center",
                    "cc-actions-list",
                    actions,
                ]);
                if self.run_ok(&argv) {
                    self.post(&format!("  {}", trf("✅ “Common tasks” localized (%1)", &[&key])));
                } else {
                    self.post(&format!("  {}", tr("⚠️ gsettings isn't available (will apply when you log in to MATE)")));
                }
            }
            None => self.post(&format!("  {}", trf("ℹ️ Locale %1 — skipping", &[&lang]))),
        }

        // 2. Все восемь раскладок панели Ubuntu MATE.
        self.post(&tr("📐 Loading all 8 Ubuntu MATE layouts…"));
        self.post("   (Familiar, Redmond, Cupertino, Mutiny, Pantheon,");
        self.post("    Contemporary, Netbook, Traditional)");
        if !self.io.which("git") {
            let hint = utils::tool_package_hint("git", family.as_str());
            self.post(format!("  {}", trf("⚠️ git isn't installed, skipping the layouts. %1", &[&hint])).trim_end());
            errors.push(tr("git not found"));
        } else {
            self.mate_layouts(&mut errors);
        }

        // 3. Пакеты сверх дистрибутива — ДО записи оформления (см. оригинал:
        //    иначе в умолчания уходила тема, которая так и не встала).
        for (tag, title, pkgs) in plan::MATE_EXTRA_PACKAGES {
            let pkgs: Vec<String> = pkgs.iter().map(|p| p.to_string()).collect();
            self.ensure_extra_packages(family, aur_helper, &tr(title), &pkgs, &mut errors, tag);
        }

        // 4. Оформление — только из того, что реально лежит на диске.
        self.post(&tr("🎨 Setting up the MATE appearance (Yaru-MATE, Ubuntu fonts)…"));
        let look = self.mate_look();
        let mut all_set = true;
        for argv in plan::mate_user_settings_argv(&look) {
            if !self.run_ok(&argv) {
                all_set = false;
            }
        }
        if all_set {
            for line in plan::mate_gschema_success_lines(&look) {
                self.post(&line);
            }
        } else {
            self.post(&format!("  {}", tr("⚠️ Some appearance settings couldn't be written (no MATE schemas?)")));
            errors.push("mate look settings failed".to_string());
        }
        self.drop_legacy_mate_gschema();

        let (lines, result) = plan::mate_post_install_summary(errors.len());
        for line in lines {
            self.post(&line);
        }
        (true, result)
    }

    fn mate_layouts(&self, errors: &mut Vec<String>) {
        let layouts_dst = "/usr/share/mate-panel/layouts";
        let repo = match self.io.temp_dir("rexon_mate_settings_", None) {
            Ok(p) => p,
            Err(e) => {
                self.post(&format!("  {}", trf("⚠️ Couldn't create a temporary folder: %1", &[&e])));
                errors.push("mkdtemp failed".to_string());
                return;
            }
        };
        // `git clone` в существующий каталог не пойдёт — как оригинал,
        // убираем пустой каталог перед клоном.
        self.io.remove_tree(&repo);
        let clone = [
            "git",
            "clone",
            "--depth",
            "1",
            "https://github.com/ubuntu-mate/ubuntu-mate-settings.git",
        ]
        .iter()
        .map(|x| x.to_string())
        .chain(std::iter::once(repo.clone()))
        .collect::<Vec<_>>();
        if !self.run_ok(&clone) {
            self.post(&format!("  {}", tr("⚠️ Couldn't clone ubuntu-mate-settings (no internet?)")));
            errors.push("git clone failed".to_string());
            self.io.remove_tree(&repo);
            return;
        }
        let src = format!("{repo}/usr/share/mate-panel/layouts");
        if !self.io.is_dir(&src) {
            self.post(&format!("  {}", tr("⚠️ The layouts folder wasn't found")));
            errors.push("layouts dir missing in repo".to_string());
            self.io.remove_tree(&repo);
            return;
        }
        let mut files: Vec<String> = self
            .io
            .list_dir(&src)
            .unwrap_or_default()
            .into_iter()
            .filter(|f| f.ends_with(".layout") || f.ends_with(".dock"))
            .collect();
        files.sort();
        if files.is_empty() {
            self.post(&format!("  {}", tr("⚠️ No layout files found")));
            errors.push("no layout files in repo".to_string());
            self.io.remove_tree(&repo);
            return;
        }
        // Помощник раскладки пускает только каталоги под /tmp.
        let staged = match self.io.temp_dir("rexon_layouts_stage_", Some("/tmp")) {
            Ok(p) => p,
            Err(e) => {
                self.post(&format!("  {}", trf("⚠️ Couldn't create a temporary folder: %1", &[&e])));
                errors.push("mkdtemp stage failed".to_string());
                self.io.remove_tree(&repo);
                return;
            }
        };
        for f in &files {
            let dst = format!("{staged}/{f}");
            if self.io.copy_file(&format!("{src}/{f}"), &dst).is_err() {
                continue;
            }
            if f.ends_with(".layout") {
                if let Some(text) = self.io.read_text(&dst) {
                    let _ = self
                        .io
                        .write_text(&dst, &plan::mate_layout_fix_snap_paths(&text));
                }
            }
        }
        let r2 = match self.sys.privileged_stage_cmd("mate-layouts", "rexon", &staged) {
            // Сеанс прав окна (каталог раскладки — архивом по дескриптору);
            // его нет — прежний помощник раскладки.
            Some(argv) => {
                let op = PrivOp::Stage {
                    category: "mate-layouts".to_string(),
                    name: "rexon".to_string(),
                    source: crate::privilege::op::LocalFile { slot: 0, label: staged.clone() },
                };
                self.io
                    .run_priv(&op, std::slice::from_ref(&staged), None)
                    .unwrap_or_else(|| self.io.run(&argv, None, false))
            }
            None => {
                let staged_files: Vec<String> = files.iter().map(|f| format!("{staged}/{f}")).collect();
                let script = plan::mate_layouts_stage_script(layouts_dst, &staged_files);
                self.run_root_script("rexon_layouts_", &script)
            }
        };
        self.io.remove_tree(&staged);
        if r2.rc() == Some(0) {
            let n_layout = files.iter().filter(|f| f.ends_with(".layout")).count();
            let n_dock = files.iter().filter(|f| f.ends_with(".dock")).count();
            self.post(&format!(
                "  {}",
                // TRANSLATORS: %1 layout files and %2 dock files were copied.
                trf("✅ Copied: %1 layouts, %2 dock files", &[&n_layout, &n_dock])
            ));
            self.post(&format!("  {}", tr("✅ Snap paths adapted for Arch/CachyOS")));
        } else {
            self.post(&format!(
                "  {}",
                tr("⚠️ Couldn't copy the layouts (administrator password not confirmed?)")
            ));
            errors.push("pkexec copy layouts failed".to_string());
        }
        self.io.remove_tree(&repo);
    }

    /// `pkexec bash <временный скрипт>` — скрипт собирает ядро.
    fn run_root_script(&self, prefix: &str, script: &str) -> RunOut {
        let Ok(tmp) = self.io.temp_file(prefix, ".sh") else {
            return RunOut::missing();
        };
        let r = match self.io.write_file_mode(&tmp, script, 0o755) {
            Ok(()) => self.io.run(
                &["pkexec".to_string(), "bash".to_string(), tmp.clone()],
                None,
                false,
            ),
            Err(_) => RunOut::missing(),
        };
        self.io.remove_file(&tmp);
        r
    }

    /// Порт `_ensure_extra_packages`.
    fn ensure_extra_packages(
        &self,
        family: PkgFamily,
        aur_helper: Option<&str>,
        title: &str,
        packages: &[String],
        errors: &mut Vec<String>,
        tag: &str,
    ) {
        self.post(title);
        if family != PkgFamily::Arch {
            self.post(&plan::extra_packages_texts(tag, &[], None).not_arch);
            return;
        }
        let missing: Vec<String> = packages
            .iter()
            .filter(|p| !self.sys.is_pkg_installed(p))
            .cloned()
            .collect();
        let texts = plan::extra_packages_texts(tag, &missing, aur_helper);
        if missing.is_empty() {
            self.post(&texts.already);
            return;
        }
        if aur_helper.is_none() {
            self.post(&texts.no_helper);
            errors.push(texts.no_helper_error);
            return;
        }
        self.post(&texts.installing);
        // Ход сборки — без доли: этап стоит на своём месте полосы (пункт 2).
        let sink = self.out.clone();
        let forward: Arc<dyn Fn(&str, &str) + Send + Sync> =
            Arc::new(move |_stage: &str, msg: &str| sink.emit("aur", msg, None));
        let r = self.run_aur_with_progress(&texts.argv, Some(texts.timeout_secs), forward);
        if r.rc() == Some(0) {
            self.post(&texts.done);
        } else if r.outcome == RunOutcome::Timeout {
            self.post(&texts.failed_timeout);
            errors.push(texts.failed_timeout_error);
        } else {
            self.post(&texts.failed);
            errors.push(texts.failed_error);
        }
    }

    /// Порт `_names_in`: имена подкаталогов, с `marker` — только те, где он
    /// есть.
    fn names_in(&self, dirs: &[&str], marker: Option<&str>) -> Vec<String> {
        let mut found: Vec<String> = Vec::new();
        for d in dirs {
            let root = self.expand(d);
            let Some(mut entries) = self.io.list_dir(&root) else {
                continue;
            };
            entries.sort();
            for name in entries {
                let path = format!("{root}/{name}");
                if !self.io.is_dir(&path) {
                    continue;
                }
                if let Some(m) = marker {
                    if !self.io.is_dir(&format!("{path}/{m}")) {
                        continue;
                    }
                }
                if !found.contains(&name) {
                    found.push(name);
                }
            }
        }
        found
    }

    /// Порт `_mate_look`.
    fn mate_look(&self) -> plan::MateLook {
        const THEME_DIRS: &[&str] = &["/usr/share/themes", "~/.themes", "~/.local/share/themes"];
        const ICON_DIRS: &[&str] = &["/usr/share/icons", "~/.icons", "~/.local/share/icons"];
        let fonts = self
            .io
            .run(&["fc-list".to_string(), ":family".to_string()], None, false);
        let input = plan::MateLookInput {
            gtk_themes: self.names_in(THEME_DIRS, Some("gtk-3.0")),
            marco_themes: self.names_in(THEME_DIRS, Some("metacity-1")),
            icon_themes: self.names_in(ICON_DIRS, None),
            cursor_themes: self.names_in(ICON_DIRS, Some("cursors")),
            ubuntu_font: fonts.rc() == Some(0) && fonts.stdout.contains("Ubuntu"),
        };
        let look = plan::mate_look(&input);
        for note in &look.notes {
            self.post(note);
        }
        look
    }

    /// Порт `_drop_legacy_mate_gschema`.
    fn drop_legacy_mate_gschema(&self) {
        let path = plan::MATE_GSCHEMA_PATH;
        if !self.io.exists(path) {
            return;
        }
        let r = self.run_root_script("rexon_gschema_", &plan::mate_gschema_cleanup_script());
        if r.rc() == Some(0) {
            self.post(&format!("  {}", tr("✅ Removed the system-wide schema override from older versions")));
        } else {
            self.post(&format!(
                "  {}",
                trf("⚠️ The system-wide schema override from older versions is still there: %1", &[&path])
            ));
        }
    }

    // ── Защита приложений по умолчанию ───────────────────────────────────
    //
    // Порт исполняемой половины `SystemDefaultsProtector`: решения —
    // `crate::defaults`, здесь запросы `xdg-mime`, чтение и запись файлов
    // `~/.config/rexon/*.json`.

    fn protector_dir(&self) -> String {
        format!("{}/rexon", self.config_home())
    }

    fn read_json(&self, path: &str) -> Option<Value> {
        serde_json::from_str(&self.io.read_text(path)?).ok()
    }

    fn protector_rules(&self) -> dfl::Rules {
        let rules = dfl::Rules::defaults();
        match self.read_json(&format!("{}/protection_rules.json", self.protector_dir())) {
            Some(Value::Object(m)) => rules.updated(&m),
            _ => rules,
        }
    }

    fn protector_snapshot_file(&self) -> String {
        format!("{}/protected_defaults.json", self.protector_dir())
    }

    fn protector_pending_file(&self) -> String {
        format!("{}/pending_choices.json", self.protector_dir())
    }

    fn protector_protected(&self) -> Vec<(String, Option<String>)> {
        match self.read_json(&self.protector_snapshot_file()) {
            Some(v @ Value::Object(_)) => dfl::load_protected_apps(&v),
            _ => Vec::new(),
        }
    }

    fn protector_pending(&self) -> Vec<(String, (String, String))> {
        let Some(Value::Object(m)) = self.read_json(&self.protector_pending_file()) else {
            return Vec::new();
        };
        m.iter()
            .filter_map(|(k, v)| {
                let old = v.get("old")?.as_str()?.to_string();
                let new = v.get("new")?.as_str()?.to_string();
                Some((k.clone(), (old, new)))
            })
            .collect()
    }

    fn xdg_query(&self, mime: &str) -> Option<String> {
        let argv = vec![
            "xdg-mime".to_string(),
            "query".to_string(),
            "default".to_string(),
            mime.to_string(),
        ];
        let r = self.io.run(&argv, None, false);
        if r.rc() != Some(0) {
            return None;
        }
        let v = r.stdout.trim().to_string();
        (!v.is_empty()).then_some(v)
    }

    /// Порт `_get_current`: все типы приложений + терминал.
    fn defaults_current(&self) -> Vec<(String, Option<String>)> {
        let mut out: Vec<(String, Option<String>)> = dfl::MIME_TYPES
            .iter()
            .map(|(t, mime)| (t.to_string(), self.xdg_query(mime)))
            .collect();
        out.push(("terminal".to_string(), self.current_terminal()));
        out
    }

    /// Порт `_get_current_terminal` (с той же ленивостью источников).
    fn current_terminal(&self) -> Option<String> {
        let xdg_list = self
            .io
            .read_text(&format!("{}/xdg-terminals.list", self.config_home()));
        let de = self.io.env("XDG_CURRENT_DESKTOP");
        if let Some(content) = &xdg_list {
            if content.lines().next().is_some_and(|l| !l.trim().is_empty()) {
                if let Some(t) = dfl::get_current_terminal(Some(content), &de, None, None) {
                    return Some(t);
                }
                return self.detect_installed_terminal();
            }
        }
        let de_l = de.to_lowercase();
        let mut kdeglobals = None;
        let mut gsettings_exec = None;
        let gsettings = |schema: &str| {
            let argv = vec![
                "gsettings".to_string(),
                "get".to_string(),
                schema.to_string(),
                "exec".to_string(),
            ];
            let r = self.io.run(&argv, None, false);
            (r.rc() == Some(0)).then_some(r.stdout)
        };
        if de_l.contains("kde") || de_l.contains("plasma") {
            kdeglobals = self
                .io
                .read_text(&format!("{}/kdeglobals", self.config_home()));
        } else if let Some(schema) = dfl::terminal_exec_schema(&de) {
            gsettings_exec = gsettings(schema);
        }
        dfl::get_current_terminal(
            xdg_list.as_deref(),
            &de,
            kdeglobals.as_deref(),
            gsettings_exec.as_deref(),
        )
        .or_else(|| self.detect_installed_terminal())
    }

    /// Порт `detect_installed_terminal`.
    fn detect_installed_terminal(&self) -> Option<String> {
        let available: Vec<String> = dfl::TERMINAL_CANDIDATES
            .iter()
            .filter(|(b, _)| self.io.which(b))
            .map(|(b, _)| b.to_string())
            .collect();
        if available.is_empty() {
            return None;
        }
        let mut dirs = vec![format!("{}/.local/share/applications", self.home())];
        let data_dirs = self
            .env_opt("XDG_DATA_DIRS")
            .unwrap_or_else(|| "/usr/local/share:/usr/share".to_string());
        dirs.extend(
            data_dirs
                .split(':')
                .filter(|d| !d.is_empty())
                .map(|d| format!("{d}/applications")),
        );
        let wanted: Vec<&str> = dfl::TERMINAL_CANDIDATES
            .iter()
            .flat_map(|(_, ids)| ids.iter().copied())
            .collect();
        let mut present: Vec<String> = Vec::new();
        for d in dirs {
            for name in self.io.list_dir(&d).unwrap_or_default() {
                if wanted.contains(&name.as_str()) && !present.contains(&name) {
                    present.push(name);
                }
            }
        }
        dfl::pick_terminal(&available, &present)
    }

    fn desktop_file_exists(&self, desktop: &str) -> bool {
        self.io.exists(&format!("/usr/share/applications/{desktop}"))
            || self
                .io
                .exists(&format!("{}/.local/share/applications/{desktop}", self.home()))
    }

    /// Порт `_set_default` вместе с `_set_terminal`.
    fn set_default(&self, app_type: &str, desktop: &str) {
        match dfl::set_default(app_type, desktop) {
            dfl::SetDefault::Terminal(t) => {
                let de = self.io.env("XDG_CURRENT_DESKTOP");
                for step in dfl::set_terminal_plan(&t, &de, &self.home()) {
                    match step {
                        dfl::TermStep::WriteFile { path, content } => {
                            let _ = self.io.write_text(&path, &content);
                        }
                        dfl::TermStep::MakeDirs { path } => {
                            let _ = self.io.make_dirs(&path);
                        }
                        dfl::TermStep::Run { argv } => {
                            let _ = self.io.run(&argv, None, false);
                        }
                        dfl::TermStep::SetIni {
                            path,
                            section,
                            key,
                            value,
                        } => {
                            let existing = self.io.read_text(&path);
                            let text =
                                dfl::set_ini_value(existing.as_deref(), &section, &key, &value);
                            let _ = self.io.write_text(&path, &text);
                        }
                    }
                }
                // `rexon-terminal.conf` лёг в environment.d — без этого
                // `TERMINAL` дошёл бы до sway/labwc/Hyprland без uwsm только
                // при следующей правке оформления.
                self.io.sync_session_env();
            }
            dfl::SetDefault::Xdg(cmds) => {
                for argv in cmds {
                    let _ = self.io.run(&argv, None, false);
                }
            }
        }
    }

    /// Порт `SystemDefaultsProtector.snapshot`. Отдаёт, сколько приложений
    /// теперь запомнено: старт окна пишет это число в журнал, а установке
    /// стола хватает признака удачи.
    fn defaults_snapshot(&self) -> Result<usize, String> {
        self.io.make_dirs(&self.protector_dir())?;
        let rules = self.protector_rules();
        let protected =
            dfl::compute_snapshot(&self.defaults_current(), &rules, &self.protector_protected());
        let text = dfl::serialize_snapshot(&protected, &self.io.clock().iso);
        self.io.write_text(&self.protector_snapshot_file(), &text)?;
        Ok(protected.len())
    }

    /// Первое снятие эталона защиты — при старте окна.
    ///
    /// Снимать или нет, решает чистая функция ядра
    /// ([`dfl::should_snapshot_at_startup`]); само снятие — тот же
    /// [`Self::defaults_snapshot`], что и перед установкой стола, второй
    /// копии нет. `None` — снимать было нечего (эталон уже есть либо защита
    /// выключена), и ни одного запроса к системе не сделано.
    fn defaults_snapshot_if_never(&self) -> Option<Result<usize, String>> {
        let rules = self.protector_rules();
        let remembered = self.protector_protected();
        if !dfl::should_snapshot_at_startup(&rules, &remembered) {
            return None;
        }
        Some(self.defaults_snapshot())
    }

    /// Порт `SystemDefaultsProtector.restore_defaults(de_switched=False)`.
    fn defaults_restore(&self) -> Result<Vec<(String, String)>, String> {
        let rules = self.protector_rules();
        if !rules.enabled() {
            return Ok(Vec::new());
        }
        let current = self.defaults_current();
        let outcome = dfl::restore_defaults(
            &self.protector_protected(),
            &rules,
            &current,
            false,
            &self.protector_pending(),
            &|d| self.desktop_file_exists(d),
        );
        for (app_type, desktop) in &outcome.set_calls {
            self.set_default(app_type, desktop);
        }
        self.io.make_dirs(&self.protector_dir())?;
        self.io.write_text(
            &self.protector_pending_file(),
            &dfl::serialize_pending(&outcome.pending),
        )?;
        Ok(outcome.results)
    }

    /// Порт `protect_de_mimeapps` → строки для журнала хода.
    fn protect_de_mimeapps(&self, current_de: &str, installed: &[String]) -> Vec<String> {
        let config_dir = self.config_home();
        let outcome = dfl::protect_de_mimeapps(
            current_de,
            installed,
            &config_dir,
            &self.io.clock().iso_seconds,
            &|d| self.desktop_file_exists(d),
            &|p| self.io.read_text(p),
        );
        let mut lines = Vec::new();
        let mut writes = outcome.writes.iter().zip(outcome.valid_counts.iter());
        for (de_id, action) in &outcome.results {
            match action.as_str() {
                "skipped" => lines.push(format!("  {}", trf("⏭️ %1: no installed apps", &[&de_id]))),
                "created" | "updated" => {
                    if let Some(((path, content), count)) = writes.next() {
                        let _ = self.io.write_text(path, content);
                        lines.push(format!(
                            "  {}",
                            // TRANSLATORS: %1 desktop id, %2 “created”/“updated”, %3 entry count, %4 file path.
                            trf("✅ %1: %2 (%3 entries) → %4", &[&de_id, &action, &count, &path])
                        ));
                    }
                }
                _ => {}
            }
        }
        lines
    }
}

// ============================================================================
// Комплектность рабочего стола: «Проверить и доустановить»
// ============================================================================
//
// Порт кнопки вкладки «Настройки» (`rexon/qt_settings/window.py`,
// `_compliance_task`). Python звал проверку ядра сразу с правом лечить, и она
// ставила недостающее под pkexec молча: без вопроса, без окна хода, по
// паролю на каждый пакет. Здесь та же проверка ([`comp::Compliance`]) идёт в
// два шага, и второй — только после «Да»:
//   1. [`DeOpsExec::completeness_survey`] — проверка БЕЗ пароля и сети (тот
//      же режим, что при входе в систему): что на месте и что осталось
//      доустановить ([`comp::deferred_fixes`]);
//   2. [`DeOpsExec::complete_de`] — та же проверка с правом лечить. Каждая
//      команда починки (pkexec, помощник AUR, загрузка из сети) идёт через
//      ЭТОТ исполнитель — `DeOpsIo::run`, ход дела в окно операции, ожидание
//      чужого замка и отмена теми же точками, что у установки стола. Прочие
//      запуски проверки (`pacman -Q`, `gsettings`) — её собственным
//      исполнителем `ComplianceIo`.
//
// ⭐ ЧТО ПОЧИНЕНО ПРИ ПЕРЕНОСЕ (правило #16):
//   1. Отказ пароля не повторяется по разу на пакет: после первого
//      закрытого окна пароля остальные команды починки не запускаются, и
//      человек видит одно объяснение, а не серию окон.
//   2. Перед каждой установкой ждём, пока освободится установщик пакетов:
//      post-hook соседней транзакции держит замок до ~30 с, и Python получал
//      «не удалось установить» на ровном месте.
//   3. Недостающее называется словами до запроса пароля — Python спрашивал
//      пароль, не сказав, ради чего.

/// Заголовок окна хода доустановки.
pub fn completeness_operation_title() -> String {
    tr("Installing What's Missing")
}
/// Заголовок окна вопроса.
pub fn completeness_confirm_title() -> String {
    tr("Install What's Missing?")
}
/// Первая строка хода: повторная проверка перед установкой.
pub fn completeness_checking_message() -> String {
    tr("🔍 Checking what's missing")
}
/// Итог, когда доустановлено всё, о чём спрашивали.
pub fn completeness_done_message() -> String {
    tr("Everything that was missing is installed.")
}

/// Текст вопроса: что именно будет доустановлено и что понадобится пароль.
pub fn completeness_confirm_text(labels: &[String]) -> String {
    let list: Vec<String> = labels.iter().map(|l| format!("• {l}")).collect();
    trf(
        // TRANSLATORS: %1 is a bulleted list of missing parts, one per line.
        "Your desktop is missing:\n%1\n\nRexon will install it for you. \
         You'll need the administrator password.",
        &[&list.join("\n")],
    )
}

/// Итог, когда доустановить удалось не всё.
pub fn completeness_partial_message(labels: &[String]) -> String {
    trf("Not everything could be installed. Not installed: %1.", &[&labels.join("; ")])
}

/// Итог отказа пароля: первая строка — та же, что у отказов установки и
/// удаления стола; кнопка названа тем же текстом ядра, что написан на ней.
pub fn completeness_denied_message() -> String {
    let button = crate::qt_settings::text("compl.button", &[]).unwrap_or_default();
    trf(
        // TRANSLATORS: %1 is the label of the “Check and Install Missing” button.
        "The administrator password wasn't confirmed — the installation was stopped.\n\
         Click “%1” again when you're ready to enter the password.",
        &[&button],
    )
}

/// Строка хода, когда команда починки не удалась.
fn fix_failed_message(why: &str) -> String {
    trf("Didn't work: %1", &[&why])
}

/// Команда починки — то, что просит пароль администратора или ходит в сеть.
/// Всё остальное проверка запускает своим исполнителем.
fn is_fix_command(argv: &[String]) -> bool {
    match argv.first().map(String::as_str) {
        Some("pkexec" | "git") => true,
        Some(prog) => comp::AUR_HELPERS.contains(&prog),
        None => false,
    }
}

/// Пакеты, которые ставит команда (`pacman -S …`, помощник AUR, `… install …`).
fn fix_targets(argv: &[String]) -> Vec<String> {
    let Some(pos) = argv.iter().position(|a| a == "-S" || a == "install") else {
        return Vec::new();
    };
    // Флаги помощника AUR со значением (`--sudo pkexec`, `--answerclean
    // None`) — значение не пакет.
    let mut out = Vec::new();
    let mut skip_value = false;
    for a in &argv[pos + 1..] {
        if skip_value {
            skip_value = false;
            continue;
        }
        if a.starts_with('-') {
            skip_value = crate::aur_install::VALUE_FLAGS.contains(&a.as_str());
            continue;
        }
        out.push(a.clone());
    }
    out
}

/// Что сказать в окне хода о команде починки — без argv и путей.
pub(crate) fn fix_step_message(argv: &[String]) -> String {
    let prog = argv.first().map(String::as_str).unwrap_or("");
    let targets = fix_targets(argv).join(", ");
    if prog == "git" {
        tr("📥 Downloading the missing files from the internet")
    } else if comp::AUR_HELPERS.contains(&prog) {
        trf("📦 Installing from the AUR: %1", &[&targets])
    } else if !targets.is_empty() {
        trf("📦 Installing: %1", &[&targets])
    } else {
        tr("📂 Copying files to the system folder")
    }
}

/// Итог первого шага — проверки без пароля.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletenessSurvey {
    /// Опознанный стол; `None` — проверок не было (стол не опознан).
    pub de_id: Option<String>,
    /// «Проверка → статус» в порядке проверок.
    pub results: comp::Results,
    /// Журнал проверки (для журнала программы, не для окна).
    pub logs: Vec<String>,
    /// Что осталось доустановить: ключ проверки и подпись для человека.
    pub fixes: Vec<(String, String)>,
}

/// Итог второго шага — доустановки.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletenessOutcome {
    pub outcome: DeOpOutcome,
    pub results: comp::Results,
    pub logs: Vec<String>,
}

/// Исполнитель проверки на время доустановки: команды починки — через
/// [`DeOpsExec`] (замок, отмена, ход, код pkexec), прочее — исполнителем
/// проверки как есть.
struct FixRouter<'r, 'a, C: ComplianceIo, IO: DeOpsIo + ?Sized> {
    exec: &'r DeOpsExec<'a, IO>,
    inner: &'r mut C,
    lock_paths: Vec<String>,
    /// Сколько починок ждали по итогам первого шага — для полосы.
    expected: usize,
    started: usize,
    /// Почему дальше команды починки не запускаются.
    stop: Option<Stop>,
}

impl<C: ComplianceIo, IO: DeOpsIo + ?Sized> FixRouter<'_, '_, C, IO> {
    /// `session` — операция и её файлы для сеанса прав окна; его нет —
    /// прежняя команда `argv` (отказ сеанса ею не повторяется, З5).
    fn fix(
        &mut self,
        argv: &[String],
        timeout: Option<u64>,
        session: Option<(&PrivOp, &[String])>,
    ) -> Option<comp::RunResult> {
        let prog = argv.first().map(String::as_str).unwrap_or("");
        // Замок ждём перед каждой командой, которая может звать пакетный
        // менеджер: после соседней транзакции его держат post-hook'и.
        if prog != "git" && !self.lock_paths.is_empty() {
            match self.exec.wait_pkg_lock(&self.lock_paths, plan::PKG_LOCK_TIMEOUT_SECS) {
                Ok((true, _)) => {}
                Ok((false, message)) => {
                    self.stop = Some(Stop::Failed(message));
                    return None;
                }
                Err(stop) => {
                    self.stop = Some(stop);
                    return None;
                }
            }
        }
        if let Err(stop) = self.exec.point_of_no_return() {
            self.stop = Some(stop);
            return None;
        }
        self.started += 1;
        let fraction = self.started as f64 / (self.expected.max(self.started) + 1) as f64;
        self.exec.emit("install", &fix_step_message(argv), Some(fraction));

        let r = session
            .and_then(|(op, files)| self.exec.io.run_priv(op, files, None))
            .unwrap_or_else(|| self.exec.io.run(argv, timeout, false));
        if prog == "pkexec" {
            if r.outcome == RunOutcome::Error {
                // Программы запроса пароля нет — дальше пробовать нечем.
                self.stop = Some(Stop::Failed(self.exec.sys.polkit_missing_message()));
                return None;
            }
            if let Some(code) = r.returncode {
                if code != 0 && plan::pkexec_auth_denied(code, &r.stderr) {
                    self.stop = Some(Stop::Failed(completeness_denied_message()));
                    return Some(comp::RunResult {
                        ok: false,
                        stdout: r.stdout,
                    });
                }
            }
        }
        match r.outcome {
            RunOutcome::Ok | RunOutcome::Failed => {
                let ok = r.returncode == Some(0);
                if !ok {
                    self.exec.warn(&fix_failed_message(&run_why(&r)));
                }
                Some(comp::RunResult {
                    ok,
                    stdout: r.stdout,
                })
            }
            RunOutcome::Timeout | RunOutcome::Error => {
                self.exec.warn(&fix_failed_message(&run_why(&r)));
                None
            }
        }
    }
}

impl<C: ComplianceIo, IO: DeOpsIo + ?Sized> ComplianceIo for FixRouter<'_, '_, C, IO> {
    fn run(&mut self, argv: &[String], timeout: Option<u64>) -> Option<comp::RunResult> {
        if !is_fix_command(argv) {
            return self.inner.run(argv, timeout);
        }
        if self.stop.is_some() {
            // Пароль уже не подтверждён либо операция отменена: следующее
            // окно пароля человеку не показываем.
            return None;
        }
        self.fix(argv, timeout, None)
    }
    fn run_priv(
        &mut self,
        op: &PrivOp,
        files: &[String],
        argv: &[String],
        timeout: Option<u64>,
    ) -> Option<comp::RunResult> {
        if !is_fix_command(argv) {
            return self.inner.run_priv(op, files, argv, timeout);
        }
        if self.stop.is_some() {
            return None;
        }
        self.fix(argv, timeout, Some((op, files)))
    }
    fn which(&mut self, name: &str) -> bool {
        self.inner.which(name)
    }
    fn env(&mut self, name: &str) -> String {
        self.inner.env(name)
    }
    fn exists(&mut self, path: &str) -> bool {
        self.inner.exists(path)
    }
    fn is_file(&mut self, path: &str) -> bool {
        self.inner.is_file(path)
    }
    fn is_dir(&mut self, path: &str) -> bool {
        self.inner.is_dir(path)
    }
    fn read_text(&mut self, path: &str) -> Option<String> {
        self.inner.read_text(path)
    }
    fn scandir(&mut self, path: &str) -> Option<Vec<ScanEntry>> {
        self.inner.scandir(path)
    }
    fn listdir(&mut self, path: &str) -> Vec<String> {
        self.inner.listdir(path)
    }
    fn makedirs(&mut self, path: &str) {
        self.inner.makedirs(path)
    }
    fn write_text(&mut self, path: &str, content: &str) {
        self.inner.write_text(path, content)
    }
    fn replace(&mut self, src: &str, dst: &str) {
        self.inner.replace(src, dst)
    }
    fn realpath(&mut self, path: &str) -> String {
        self.inner.realpath(path)
    }
    fn mkdtemp(&mut self, prefix: &str, dir: Option<&str>) -> String {
        self.inner.mkdtemp(prefix, dir)
    }
    fn mkstemp(&mut self, prefix: &str, suffix: &str) -> String {
        self.inner.mkstemp(prefix, suffix)
    }
    fn chmod(&mut self, path: &str, mode: u32) {
        self.inner.chmod(path, mode)
    }
    fn remove_file(&mut self, path: &str) {
        self.inner.remove_file(path)
    }
    fn remove_tree(&mut self, path: &str) {
        self.inner.remove_tree(path)
    }
    fn copy_file(&mut self, src: &str, dst: &str) {
        self.inner.copy_file(src, dst)
    }
}

impl<IO: DeOpsIo + ?Sized> DeOpsExec<'_, IO> {
    /// Прогон проверки текущего стола. Окружение и наличие помощников — с
    /// ЭТОГО исполнителя, как у установки стола.
    fn run_compliance<C: ComplianceIo>(
        &self,
        cio: &mut C,
        allow_privileged: bool,
    ) -> (Option<String>, comp::Results, Vec<String>) {
        let desktop = self.io.env("XDG_CURRENT_DESKTOP");
        let home = self.home();
        let xdg_config = self.env_opt("XDG_CONFIG_HOME");
        let family = self.sys.family();
        let pkg_helper = self.sys.pkg_helper_available();
        let stage_helper = self.sys.stage_helper_available();
        let mut logs = Vec::new();
        let results = comp::Compliance::with_xdg(
            cio,
            &home,
            xdg_config.as_deref(),
            family,
            allow_privileged,
            pkg_helper,
            stage_helper,
        )
        .run_for_current_de(&desktop, &mut logs);
        (comp::detect_current_de(&desktop), results, logs)
    }

    /// Шаг 1: проверка без пароля и сети — что на месте, чего не хватает.
    pub fn completeness_survey<C: ComplianceIo>(&self, cio: &mut C) -> CompletenessSurvey {
        let (de_id, results, logs) = self.run_compliance(cio, false);
        let fixes = comp::deferred_fixes(&results);
        CompletenessSurvey {
            de_id,
            results,
            logs,
            fixes,
        }
    }

    /// Шаг 2: доустановка. `expected` — что обещали человеку в вопросе
    /// (итог шага 1); по нему судится, всё ли получилось.
    pub fn complete_de<C: ComplianceIo>(
        &self,
        cio: &mut C,
        expected: &[(String, String)],
    ) -> CompletenessOutcome {
        let mut results = Vec::new();
        let mut logs = Vec::new();
        let step = self.complete_de_steps(cio, expected, &mut results, &mut logs);
        CompletenessOutcome {
            outcome: self.finish(step),
            results,
            logs,
        }
    }

    fn complete_de_steps<C: ComplianceIo>(
        &self,
        cio: &mut C,
        expected: &[(String, String)],
        results: &mut comp::Results,
        logs: &mut Vec<String>,
    ) -> Step<String> {
        self.set_cancellable(true);
        self.emit("check", &completeness_checking_message(), None);
        self.checkpoint()?;
        let lock_paths = self.io.pkg_lock_paths(self.sys.family());
        let mut router = FixRouter {
            exec: self,
            inner: cio,
            lock_paths,
            expected: expected.len(),
            started: 0,
            stop: None,
        };
        let (_, res, lg) = self.run_compliance(&mut router, true);
        let stop = router.stop.take();
        *results = res;
        *logs = lg;
        if let Some(stop) = stop {
            return Err(stop);
        }
        let status = |key: &str| {
            results
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.as_str())
        };
        let left: Vec<String> = expected
            .iter()
            .filter(|(key, _)| !matches!(status(key), Some("fixed" | "ok")))
            .map(|(_, label)| label.clone())
            .collect();
        if left.is_empty() {
            Ok(completeness_done_message())
        } else {
            Err(Stop::Failed(completeness_partial_message(&left)))
        }
    }
}

/// Приёмник хода, которому показывать нечего (вопросы без окна хода).
struct SilentSink;

impl DeOpSink for SilentSink {
    fn progress(&self, _: &str, _: &str, _: Option<f64>) {}
    fn cancellable(&self, _: bool) {}
}

/// Шаг 1 на живой машине: проверка без пароля и сети.
pub fn completeness_survey_live() -> CompletenessSurvey {
    let io = LiveDeOpsIo::new();
    let mut cio = crate::appdefaults_exec::LiveComplianceIo;
    DeOpsExec::new(&io, Arc::new(SilentSink), CancelToken::new()).completeness_survey(&mut cio)
}

/// Шаг 2 на живой машине: доустановка с ходом дела в окно.
pub fn complete_de_live(
    sink: Arc<dyn DeOpSink>,
    cancel: CancelToken,
    expected: &[(String, String)],
) -> CompletenessOutcome {
    complete_de_with(&LiveDeOpsIo::new(), sink, cancel, expected)
}

/// Шаг 2 на данном исполнителе с живой проверкой — та же проводка, что у
/// [`complete_de_live`]; стенд подаёт сюда живой исполнитель со своим замком.
pub fn complete_de_with<IO: DeOpsIo + ?Sized>(
    io: &IO,
    sink: Arc<dyn DeOpSink>,
    cancel: CancelToken,
    expected: &[(String, String)],
) -> CompletenessOutcome {
    let mut cio = crate::appdefaults_exec::LiveComplianceIo;
    DeOpsExec::new(io, sink, cancel).complete_de(&mut cio, expected)
}

// ============================================================================
// Оформление AeroShell: сверка и пересборка
// ============================================================================
//
// Порт `check_aeroshell` / `rebuild_aeroshell` (`rexon/system.py`) и окна
// `offer_aeroshell_rebuild` (`rexon/qt_main/de_progress.py`). Решения — в
// [`crate::aeroshell`]: расходится ли, чем собирать, команда, разбор итога.
// Здесь — порядок и походы наружу тем же исполнителем, что ставит столы:
//   1. [`DeOpsExec::aeroshell_survey`] — сверка без пароля: список пакетов,
//      версии KWin и Plasma, файлы пакетов оформления;
//   2. [`DeOpsExec::rebuild_aeroshell`] — пересборка ТОЛЬКО после «Да» в окне:
//      проверки «чем собирать» до первого побочного действия, ожидание замка
//      пакетного менеджера (с отменой), запуск сборщика с ходом дела в окно.
//
// ⭐ ЧТО ПОЧИНЕНО ПРИ ПЕРЕНОСЕ (правило #16):
//   1. Отменить можно, пока сборка не началась, — в том числе во время
//      ожидания чужого замка (у Python окно хода не отпускало до пяти минут).
//   2. Отказ пароля назван отказом пароля, а не пятнадцатью строками вывода
//      сборщика, и говорит, что делать.
//   3. Неудача сборки начинается с главного для человека — система цела, —
//      а хвост вывода идёт следом, а не вместо.

impl<IO: DeOpsIo + ?Sized> DeOpsExec<'_, IO> {
    /// Сверка оформления с установленной Plasma — без пароля и без сети.
    /// Не Arch, нет pacman или пакетов оформления — пустой итог.
    ///
    /// `read_file` — содержимое модуля эффекта (двоичный файл: у [`SystemIo`]
    /// чтение только текстовое).
    pub fn aeroshell_survey(&self, read_file: &dyn Fn(&str) -> Option<Vec<u8>>) -> aeroshell::Report {
        let family = self.sys.family();
        if family != PkgFamily::Arch {
            return aeroshell::Report::empty();
        }
        let ask = |query: Option<plan::InstalledPkgQuery>| -> Option<RunOut> {
            let q = query?;
            if !self.io.which(&q.binary) {
                return None;
            }
            Some(self.io.run(&q.cmd.argv, q.cmd.timeout_secs, q.cmd.lang_c))
        };
        let installed = ask(plan::installed_packages_query(family))
            .map(|r| plan::parse_installed_packages(r.rc() == Some(0), &r.stdout))
            .unwrap_or_default();
        let forks = plan::filter_plasma_forks(&installed);
        if forks.is_empty() {
            return aeroshell::Report::empty();
        }
        let version = |package: &str| {
            ask(plan::installed_version_query(family, package))
                .and_then(|r| plan::parse_installed_version(family, r.rc() == Some(0), &r.stdout))
        };
        let kwin = version("kwin");
        // Версия стека — у plasma-workspace: форк libplasma ЗАМЕНЯЕТ стоковый
        // libplasma, и спрашивать его — сравнивать пакет сам с собой.
        let plasma = version("plasma-workspace");
        let file_list = |package: &str| {
            ask(plan::package_files_query(family, package))
                .and_then(|r| plan::package_files_output(r.rc() == Some(0), &r.stdout))
        };
        let qt = version("qt6-base");
        // `ldd -r` по модулю Qt — ненайденные библиотеки и символы. Только
        // чтение; нет ldd — шаг пропускается.
        let has_ldd = self.io.which("ldd");
        let ldd = |path: &str| -> Option<String> {
            if !has_ldd {
                return None;
            }
            let r = self.io.run(&["ldd".to_string(), "-r".to_string(), path.to_string()], Some(15), true);
            (r.outcome != RunOutcome::Timeout && r.outcome != RunOutcome::Error)
                .then(|| format!("{}\n{}", r.stdout, r.stderr))
        };
        aeroshell::check_modules_with(
            &forks,
            kwin.as_deref(),
            plasma.as_deref(),
            qt.as_deref(),
            &file_list,
            read_file,
            &ldd,
        )
    }

    /// Сверка и совет: что предложить человеку. Только чтение — без пароля.
    ///
    /// Одинокому форку ([`aeroshell::lonely_libplasma_fork`]) заранее
    /// делается предпросмотр отката (`pacman -Qi`/`-Si`): если штатного
    /// пакета в репозиториях нет, предлагать «вернуть» бессмысленно — человек
    /// получит отказ сразу и словами, а не после пароля.
    pub fn aeroshell_advice(
        &self,
        read_file: &dyn Fn(&str) -> Option<Vec<u8>>,
        file_mtime: &dyn Fn(&str) -> Option<u64>,
    ) -> (aeroshell::Report, aeroshell::Advice) {
        let mut report = self.aeroshell_survey(read_file);
        // Журнал сеанса — точное подтверждение и уточнение сверки по версиям
        // (и единственный источник для сторонних модулей). Одинокому форку
        // он ничего не добавит: совет там — вернуть штатную Plasma.
        if !report.lonely_fork {
            let findings = self.plugin_journal(read_file, file_mtime);
            aeroshell::apply_journal(&mut report, findings);
        }
        let preview = (report.lonely_fork && !report.restore.is_empty())
            .then(|| self.sys.plasma_fork_reset_preview(Some(&report.restore)));
        // Сеть — только когда без неё совет был бы «пересобрать».
        let upstream = match aeroshell::advise(&report, preview.clone()) {
            aeroshell::Advice::Rebuild { packages } => self.aeroshell_upstream(&report, &packages),
            _ => None,
        };
        let advice = aeroshell::advise_with_upstream(&report, preview, upstream);
        if advice != aeroshell::Advice::Nothing {
            return (report, advice);
        }
        // Сейчас всё работает — не ждёт ли в репозиториях Plasma, которая
        // сломает набор (предупредить ДО обновления).
        let advice = self.aeroshell_before_update(&report);
        (report, advice)
    }

    /// Модули KWin и Plasma, которые не загрузились в этом сеансе
    /// ([`crate::plugin_journal`]). Только чтение, без root: журнал
    /// пользователя текущей загрузки, `pacman -Qo` / `-Qm`. Нет journalctl
    /// или журнала — пусто, молча.
    fn plugin_journal(
        &self,
        read_file: &dyn Fn(&str) -> Option<Vec<u8>>,
        file_mtime: &dyn Fn(&str) -> Option<u64>,
    ) -> Vec<crate::plugin_journal::Finding> {
        use crate::plugin_journal as pj;
        if !self.io.which("journalctl") {
            return Vec::new();
        }
        let mut text = None;
        for grep in [true, false] {
            let r = self.io.run(&pj::journal_argv(grep), Some(pj::JOURNAL_TIMEOUT_SECS), true);
            match pj::journal_answer(grep, r.rc(), &r.stdout, &r.stderr) {
                pj::JournalAnswer::Text => {
                    text = Some(r.stdout);
                    break;
                }
                pj::JournalAnswer::RetryWithoutGrep => continue,
                pj::JournalAnswer::Empty | pj::JournalAnswer::Unavailable => break,
            }
        }
        let Some(text) = text else { return Vec::new() };
        let failures = pj::failures_from_journal(&text);
        if failures.is_empty() {
            return Vec::new();
        }
        let family = self.sys.family();
        let home = self.home();
        let data_home = utils::data_home(self.env_opt("XDG_DATA_HOME").as_deref(), &home);
        let config_home = self.config_home();
        let data_dirs = pj::data_dirs(&self.io.env("XDG_DATA_DIRS"));
        let plugin_dirs = pj::qt_plugin_dirs(&self.io.env("QT_PLUGIN_PATH"));
        let locale = crate::i18n::current_language();
        let owners = |paths: &[String]| {
            let mut map = std::collections::HashMap::new();
            for q in plan::file_owner_queries(family, paths).unwrap_or_default() {
                if !self.io.which(&q.binary) {
                    break;
                }
                let r = self.io.run(&q.cmd.argv, q.cmd.timeout_secs, q.cmd.lang_c);
                map.extend(crate::installed_scan::parse_qo_output(&r.stdout));
            }
            map
        };
        // Пакеты не из репозиториев — один вопрос на всю сверку.
        let foreign_list: std::cell::OnceCell<Vec<String>> = std::cell::OnceCell::new();
        let foreign = |pkg: &str| {
            foreign_list
                .get_or_init(|| {
                    let Some(q) = plan::foreign_packages_query(family) else { return Vec::new() };
                    if !self.io.which(&q.binary) {
                        return Vec::new();
                    }
                    let r = self.io.run(&q.cmd.argv, q.cmd.timeout_secs, q.cmd.lang_c);
                    r.stdout
                        .lines()
                        .filter_map(|l| l.split_whitespace().next().map(str::to_string))
                        .collect()
                })
                .iter()
                .any(|p| p == pkg)
        };
        let list = |dir: &str| {
            self.io
                .list_dir(dir)
                .unwrap_or_default()
                .into_iter()
                .map(|name| format!("{}/{name}", dir.trim_end_matches('/')))
                .collect()
        };
        let read = |path: &str| read_file(path).or_else(|| self.io.read_text(path).map(String::into_bytes));
        let env = pj::ResolveEnv {
            home: &home,
            data_home: &data_home,
            config_home: &config_home,
            data_dirs: &data_dirs,
            plugin_dirs: &plugin_dirs,
            locale: &locale,
            mtime_us: file_mtime,
            read: &read,
            list: &list,
            owners: &owners,
            foreign: &foreign,
        };
        pj::resolve(&failures, &env)
    }

    /// Предупреждение до обновления ([`aeroshell::advise_before_update`]).
    /// Что придёт — `checkupdates` (своя копия базы, без прав); его нет или
    /// он не смог — [`aeroshell::Advice::Nothing`]. Сеть к авторам форка —
    /// только когда в репозиториях новая серия Plasma.
    fn aeroshell_before_update(&self, report: &aeroshell::Report) -> aeroshell::Advice {
        if report.rebuild.is_empty() {
            return aeroshell::Advice::Nothing;
        }
        let Some(updates) = self.pending_updates() else {
            return aeroshell::Advice::Nothing;
        };
        let new_series = aeroshell::pending_plasma(&updates)
            .is_some_and(|(from, to)| aeroshell::plasma_series(&from) != aeroshell::plasma_series(&to));
        let fork = if new_series { self.aeroshell_fork_version(&report.rebuild) } else { None };
        aeroshell::advise_before_update(report, &updates, fork.as_deref())
    }

    /// Что придёт с обновлением системы: разобранный вывод `checkupdates`.
    /// None — не Arch, программы нет или она не смогла (нет сети) — честно
    /// «не знаем». Код 2 у `checkupdates` — «обновлений нет».
    fn pending_updates(&self) -> Option<Vec<aeroshell::PendingUpdate>> {
        crate::update_guard::pending_updates(self.io, self.sys.family())
    }

    /// Догнали ли авторы форка libplasma Plasma из репозиториев:
    /// `(версия Plasma, версия форка в ветке по умолчанию)`. `None` — форка
    /// libplasma в наборе нет либо узнать не удалось (молча: без сети
    /// проверка просто пропускается). Только чтение, без пароля.
    fn aeroshell_upstream(&self, report: &aeroshell::Report, packages: &[String]) -> Option<(String, String)> {
        let family = self.sys.family();
        packages.iter().find(|p| p.starts_with(aeroshell::LIBPLASMA_FORK_PREFIX))?;
        // Plasma, к которой идёт система: установленная или уже лежащая в
        // базе репозиториев (её подтягивает и Discover) — старшая.
        let repo = plan::repo_version_query(family, "libplasma").and_then(|q| {
            if !self.io.which(&q.binary) {
                return None;
            }
            let r = self.io.run(&q.cmd.argv, q.cmd.timeout_secs, q.cmd.lang_c);
            (r.rc() == Some(0)).then(|| plan::parse_repo_version(family, &r.stdout)).flatten()
        });
        let plasma = aeroshell::newest_version(&[report.plasma_version.clone(), repo])?;
        Some((plasma, self.aeroshell_fork_version(packages)?))
    }

    /// Версия форка libplasma в ветке по умолчанию его репозитория
    /// (`PROJECT_VERSION` её `CMakeLists.txt`). None — форка среди
    /// `packages` нет либо узнать не удалось (нет сети, git, curl, чужой
    /// хостинг). Только чтение, без пароля.
    fn aeroshell_fork_version(&self, packages: &[String]) -> Option<String> {
        let family = self.sys.family();
        let fork = packages
            .iter()
            .find(|p| p.starts_with(aeroshell::LIBPLASMA_FORK_PREFIX))?;
        let project_url = self
            .sys
            .pkg_meta_stdout(fork, family)
            .and_then(|out| crate::system::parse_qi_field(&out, "URL").into_iter().next())?;
        let git_url = aeroshell::upstream_git_url(&project_url)?;
        if !self.io.which("git") || !self.io.which("curl") {
            return None;
        }
        let timeout = Some(aeroshell::UPSTREAM_TIMEOUT_SECS);
        let argv = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<String>>();
        // Без вопросов в никуда: git не должен спрашивать логин у закрытого
        // или переехавшего репозитория.
        let head = self.io.run(
            &argv(&["env", "GIT_TERMINAL_PROMPT=0", "git", "ls-remote", "--symref", &git_url, "HEAD"]),
            timeout,
            true,
        );
        if head.rc() != Some(0) {
            return None;
        }
        let branch = aeroshell::default_branch(&head.stdout)?;
        let raw = aeroshell::raw_file_url(&project_url, &branch, "CMakeLists.txt")?;
        let secs = aeroshell::UPSTREAM_TIMEOUT_SECS.to_string();
        let cmake = self.io.run(&argv(&["curl", "-fsSL", "-m", &secs, &raw]), timeout, true);
        if cmake.rc() != Some(0) {
            return None;
        }
        aeroshell::cmake_project_version(&cmake.stdout)
    }

    /// Вернуть штатную Plasma вместо оставшегося без оболочки форка.
    /// ⛔ Зовётся только после «Да» в окне. Сам откат — общий
    /// (`SystemExec::remove_plasma_forks`, один запрос пароля); здесь —
    /// ход дела в окно и ожидание чужого замка с отменой.
    pub fn restore_stock_plasma(&self, forks: &[String]) -> DeOpOutcome {
        let step = self.restore_stock_plasma_steps(forks);
        self.finish(step)
    }

    fn restore_stock_plasma_steps(&self, forks: &[String]) -> Step<String> {
        self.set_cancellable(true);
        self.emit(aeroshell::REBUILD_START_STAGE, &aeroshell::restore_start_message(), Some(0.05));
        self.checkpoint()?;
        let lock_paths = self.io.pkg_lock_paths(self.sys.family());
        if !lock_paths.is_empty() {
            let (free, message) = self.wait_pkg_lock(&lock_paths, plan::PKG_LOCK_TIMEOUT_SECS)?;
            if !free {
                return Err(Stop::Failed(message));
            }
        }
        self.point_of_no_return()?;
        let (ok, message) = self.sys.remove_plasma_forks(Some(forks));
        if !ok {
            return Err(Stop::Failed(message));
        }
        self.emit(aeroshell::REBUILD_DONE_STAGE, &aeroshell::restore_done_message(), Some(1.0));
        Ok(message)
    }

    /// Поставить либо снять пользовательские юниты фоновой проверки
    /// ([`crate::update_guard::watch_plan`]; прежние юниты AeroThemePlasma
    /// снимаются). Без root: только `~/.config/systemd/user` и
    /// `systemctl --user`. Всё совпадает — ничего не делается и `systemctl`
    /// не зовётся.
    pub fn update_watch(&self, wanted: bool, exec_start: Option<&str>, program: Option<&str>) -> Result<(), String> {
        let plan = crate::update_guard::watch_plan(&self.config_home(), wanted, exec_start, program, &|p| {
            self.io.read_text(p)
        });
        crate::update_guard::apply_watch_plan(self.io, &plan)
    }

    /// Пересборка пакетов оформления. ⛔ Зовётся только после «Да» в окне.
    pub fn rebuild_aeroshell(&self, packages: &[String]) -> DeOpOutcome {
        let step = self.rebuild_aeroshell_steps(packages);
        self.finish(step)
    }

    fn rebuild_aeroshell_steps(&self, packages: &[String]) -> Step<String> {
        self.set_cancellable(true);
        // Чем собирать и чем спросить права — до первого побочного действия.
        let helper = aeroshell::pick_helper(&|name| self.io.which(name));
        let chroot = helper
            .as_deref()
            .is_some_and(|h| aeroshell::chroot_available(h, &|name| self.io.which(name)));
        let polkit_ok = self.sys.polkit_available();
        let missing = if polkit_ok {
            String::new()
        } else {
            self.sys.polkit_missing_message()
        };
        let rebuild = aeroshell::rebuild_plan(packages, helper.as_deref(), chroot, polkit_ok, &missing);
        if let Some(error) = rebuild.error {
            return Err(Stop::Failed(error));
        }
        let helper = helper.unwrap_or_default();
        self.emit(&rebuild.start_stage, &rebuild.start_message, Some(0.05));
        self.checkpoint()?;

        // Хуки соседней транзакции держат замок до полуминуты: сборщик иначе
        // падает на ровном месте. Ждать можно с отменой.
        let lock_paths = self.io.pkg_lock_paths(self.sys.family());
        if !lock_paths.is_empty() {
            let (free, message) = self.wait_pkg_lock(&lock_paths, plan::PKG_LOCK_TIMEOUT_SECS)?;
            if !free {
                return Err(Stop::Failed(message));
            }
        }
        self.point_of_no_return()?;

        let log = self
            .io
            .temp_file("rexon_aeroshell_", ".log")
            .map_err(|e| Stop::Failed(aeroshell::rebuild_spawn_failure(&helper, &e)))?;
        let out = self.out.clone();
        let emit: Arc<dyn Fn(&str, &str) + Send + Sync> =
            Arc::new(move |stage: &str, message: &str| out.emit(stage, message, None));
        let mut tail = self.io.tail_log(&log, LogTailKind::Build, emit);
        // Таймаут — по классу команды (сборка из исходников, часы).
        let r = self
            .io
            .run_to_file(&aeroshell::rebuild_argv_c_locale(&rebuild.argv), None, &log);
        let live_tail = tail.as_mut().map(|t| t.stop()).is_some();
        let text = self.io.read_text(&log).unwrap_or_default();
        self.io.remove_file(&log);
        let lines: Vec<String> = text.lines().filter_map(aeroshell::rebuild_line).collect();
        if !live_tail {
            // Читателя нет (подставной исполнитель) — ход дела после конца.
            for line in &lines {
                self.emit(aeroshell::REBUILD_LINE_STAGE, line, None);
            }
        }

        match r.outcome {
            RunOutcome::Error => {
                return Err(Stop::Failed(aeroshell::rebuild_spawn_failure(&helper, &r.stderr)))
            }
            RunOutcome::Timeout => {
                return Err(Stop::Failed(aeroshell::rebuild_timeout_message()))
            }
            RunOutcome::Ok | RunOutcome::Failed => {}
        }
        let code = r.returncode.unwrap_or(-1);
        if code != 0 && aeroshell::rebuild_auth_denied(&text) {
            return Err(Stop::Failed(aeroshell::rebuild_denied_message()));
        }
        let from = lines.len().saturating_sub(aeroshell::REBUILD_TAIL_LINES);
        let outcome = aeroshell::rebuild_outcome(code, &lines[from..]);
        if !outcome.ok {
            return Err(Stop::Failed(aeroshell::rebuild_failure_message(&outcome.message)));
        }
        if let (Some(stage), Some(message)) = (&outcome.log_stage, &outcome.log_message) {
            self.emit(stage, message, Some(1.0));
        }
        Ok(outcome.message)
    }
}

/// Сверка оформления на живой машине.
pub fn aeroshell_survey_live() -> aeroshell::Report {
    let io = LiveDeOpsIo::new();
    DeOpsExec::new(&io, Arc::new(SilentSink), CancelToken::new())
        .aeroshell_survey(&|path| std::fs::read(path).ok())
}

/// Модули KWin и Plasma, не загрузившиеся в этом сеансе (журнал
/// пользователя, `pacman -Qo`) — для сторожа сеанса
/// ([`crate::session_watch`]): кто виноват в падениях после входа.
pub fn plugin_findings_live() -> Vec<crate::plugin_journal::Finding> {
    let io = LiveDeOpsIo::new();
    DeOpsExec::new(&io, Arc::new(SilentSink), CancelToken::new())
        .plugin_journal(&|path| std::fs::read(path).ok(), &file_mtime_us_live)
}

/// Сверка и совет на живой машине.
pub fn aeroshell_advice_live() -> (aeroshell::Report, aeroshell::Advice) {
    let io = LiveDeOpsIo::new();
    DeOpsExec::new(&io, Arc::new(SilentSink), CancelToken::new())
        .aeroshell_advice(&|path| std::fs::read(path).ok(), &file_mtime_us_live)
}

/// Время изменения файла, микросекунды эпохи; None — файла нет.
fn file_mtime_us_live(path: &str) -> Option<u64> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    Some(modified.duration_since(std::time::UNIX_EPOCH).ok()?.as_micros() as u64)
}

/// Юниты фоновой проверки на живой машине: нужны на Arch (пароля не
/// надо). Заводить Rexon нечем — слежка снимается (мёртвая строка запуска
/// хуже отсутствия слежки).
pub fn update_watch_live() -> Result<(), String> {
    let entry = crate::installer::launch_entry_live();
    let exec = crate::app::unit_launch_command(&entry, None, &[crate::update_guard::CHECK_FLAG]);
    let program = match &entry {
        crate::installer::LaunchEntry::MainWindow(path) => Some(path.clone()),
        crate::installer::LaunchEntry::Python => None,
    };
    let io = LiveDeOpsIo::new();
    let ex = DeOpsExec::new(&io, Arc::new(SilentSink), CancelToken::new());
    let wanted = ex.sys.family() == PkgFamily::Arch && exec.is_some();
    // Сторож входа в Plasma — вместе со слежкой: ставится, пока стоит
    // AeroThemePlasma, и снимается вместе с ней.
    if let Err(e) = crate::session_guard::sync_env_script_live() {
        crate::log_warn!("session-guard", "сценарий входа: {e}");
    }
    ex.update_watch(wanted, exec.as_deref(), program.as_deref())
}

/// Безоконная проверка (`rexon --aeroshell-check [--login]`), её зовёт
/// пользовательский юнит. Дождаться конца транзакции pacman → сверить
/// (только чтение) → при беде уведомление с кнопкой «Открыть Rexon». Пароль
/// здесь не спрашивается никогда. → код возврата процесса (0 — проверка
/// прошла, что бы она ни нашла).
pub fn aeroshell_check_route_live(login: bool, out: &mut dyn FnMut(&str)) -> i32 {
    // Проверку выключили в настройках — молчим и здесь.
    let saved = crate::settings::SettingsManager::new()
        .ok()
        .and_then(|m| m.get(aeroshell::CHECK_ON_START_SETTING).and_then(Value::as_bool));
    if !aeroshell::check_on_start(saved) {
        out("aeroshell-check: disabled in settings");
        return 0;
    }
    // Срабатывание пришлось на середину обновления — ждать его конца:
    // сверять наполовину обновлённую систему значит пугать зря.
    let started = std::time::Instant::now();
    while std::path::Path::new(aeroshell::PACMAN_DB_LOCK).exists() {
        if started.elapsed().as_secs() > aeroshell::LOCK_WAIT_SECS {
            out("aeroshell-check: package manager still busy, giving up");
            return 0;
        }
        std::thread::sleep(std::time::Duration::from_secs(3));
    }
    let (report, advice) = aeroshell_advice_live();
    let fingerprint = aeroshell::notify_fingerprint(&report, &advice);
    out(&format!("aeroshell-check: {fingerprint}"));
    let last = std::fs::read_to_string(aeroshell_notice_stamp_live()).ok();
    let notify = aeroshell::should_notify(&advice, &fingerprint, last.as_deref(), login);
    aeroshell_notice_mark_live(&fingerprint);
    if !notify {
        return 0;
    }
    let Some(notice) = aeroshell::notice_for_report(&advice, &report) else {
        return 0;
    };
    let io = LiveDeOpsIo::new();
    let icon = "rexon";
    if io.which("notify-send") {
        // `--wait` держит процесс до закрытия уведомления; нажатая кнопка
        // приходит в stdout своим именем.
        let argv: Vec<String> = vec![
            "notify-send".into(),
            format!("--app-name={}", utils::NOTIFY_APP_NAME),
            format!("--icon={icon}"),
            "--urgency=critical".into(),
            "--wait".into(),
            format!("--action=open={}", notice.button),
            notice.summary.clone(),
            notice.body.clone(),
        ];
        let r = io.run(&argv, Some(aeroshell::NOTIFY_WAIT_SECS), false);
        if r.stdout.trim() == "open" {
            open_rexon_for_repair(out);
        }
    } else if io.which("dbus-send") {
        // Без notify-send кнопки нет: служба уведомлений шлёт нажатие
        // сигналом, а dbus-send его не ловит. Текст всё равно доходит.
        let argv = utils::notify_dbus_argv("dbus-send", &notice.summary, &notice.body, icon, 0);
        let _ = io.run(&argv, Some(10), false);
    }
    0
}

/// Файл с отпечатком состояния, о котором уже сказали
/// ([`aeroshell::notify_fingerprint`]). Общий у уведомления и окна Rexon:
/// предупреждение, показанное окном, уведомлением не повторяется.
fn aeroshell_notice_stamp_live() -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    let cache = utils::cache_home(std::env::var("XDG_CACHE_HOME").ok().as_deref().filter(|v| !v.is_empty()), &home);
    format!("{cache}/rexon/aeroshell-notified")
}

/// Говорили ли уже об этом состоянии (окно Rexon при запуске и уведомление).
pub fn aeroshell_notice_seen_live(fingerprint: &str) -> bool {
    std::fs::read_to_string(aeroshell_notice_stamp_live()).is_ok_and(|last| last.trim() == fingerprint)
}

/// Запомнить, что об этом состоянии сказали.
pub fn aeroshell_notice_mark_live(fingerprint: &str) {
    let stamp = aeroshell_notice_stamp_live();
    if let Some(dir) = std::path::Path::new(&stamp).parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(&stamp, fingerprint);
}

/// Открыть Rexon сразу на проверке ([`aeroshell::OPEN_FLAG`]) отдельным
/// процессом, не дожидаясь его.
fn open_rexon_for_repair(out: &mut dyn FnMut(&str)) {
    if let Err(e) = crate::installer::open_rexon_live(&[aeroshell::OPEN_FLAG]) {
        out(&format!("aeroshell-check: {e}"));
    }
}

/// Возврат штатной Plasma на живой машине с ходом дела в окно.
pub fn restore_stock_plasma_live(
    sink: Arc<dyn DeOpSink>,
    cancel: CancelToken,
    forks: &[String],
) -> DeOpOutcome {
    let io = LiveDeOpsIo::new();
    DeOpsExec::new(&io, sink, cancel).restore_stock_plasma(forks)
}

/// Пересборка оформления на живой машине с ходом дела в окно.
pub fn rebuild_aeroshell_live(
    sink: Arc<dyn DeOpSink>,
    cancel: CancelToken,
    packages: &[String],
) -> DeOpOutcome {
    let mut outcome = rebuild_aeroshell_with(&LiveDeOpsIo::new(), sink, cancel, packages);
    // Сторож входа выключал оформление до пересборки — вернуть сохранённое.
    if outcome.ok {
        if let Some(extra) = crate::session_guard::after_rebuild_live() {
            outcome.message = format!("{}\n{extra}", outcome.message);
        }
    }
    outcome
}

/// Пересборка оформления на данном исполнителе — та же проводка, что у
/// [`rebuild_aeroshell_live`]; стенд подаёт сюда живой исполнитель со своим
/// замком.
pub fn rebuild_aeroshell_with<IO: DeOpsIo + ?Sized>(
    io: &IO,
    sink: Arc<dyn DeOpSink>,
    cancel: CancelToken,
    packages: &[String],
) -> DeOpOutcome {
    DeOpsExec::new(io, sink, cancel).rebuild_aeroshell(packages)
}

/// Установка стола на живой машине одним вызовом — то, чем пользуется окно.
pub fn install_de_live(
    sink: Arc<dyn DeOpSink>,
    cancel: CancelToken,
    de_id: &str,
    current_de: &str,
) -> DeOpOutcome {
    // SteamOS (Steam Deck и Steam Machine): пакетами не ставим — стол
    // собирается образом systemd-sysext (`crate::steamos_sysext`).
    if dc::running_on_steamos() {
        return crate::steamos_sysext::install_live(sink, cancel, de_id);
    }
    // Deepin — только на Treeland: сначала своя сборка deepin-session.
    if de_id == crate::deepin_treeland::DE_ID {
        return crate::deepin_treeland::install_from_card_live(sink, cancel, current_de);
    }
    install_de_packages_live(sink, cancel, de_id, current_de)
}

/// Установка набора стола пакетным менеджером, без особых путей
/// (Deepin на Treeland зовёт её после своей сборки).
pub fn install_de_packages_live(
    sink: Arc<dyn DeOpSink>,
    cancel: CancelToken,
    de_id: &str,
    current_de: &str,
) -> DeOpOutcome {
    let io = LiveDeOpsIo::new();
    DeOpsExec::new(&io, sink, cancel).install_de(de_id, current_de)
}

/// Установка стола с заменой конфликтующего (`replace`, согласие человека)
/// на живой машине — кнопка замены на карточке стола.
pub fn install_de_replacing_live(
    sink: Arc<dyn DeOpSink>,
    cancel: CancelToken,
    de_id: &str,
    current_de: &str,
    replace: &[String],
) -> DeOpOutcome {
    if dc::running_on_steamos() {
        return crate::steamos_sysext::install_live(sink, cancel, de_id);
    }
    // Замены из таблицы у Deepin нет; путь — тот же, что у установки.
    if de_id == crate::deepin_treeland::DE_ID {
        return crate::deepin_treeland::install_from_card_live(sink, cancel, current_de);
    }
    let io = LiveDeOpsIo::new();
    DeOpsExec::new(&io, sink, cancel).install_de_replacing(de_id, current_de, replace)
}

/// Удаление стола на живой машине одним вызовом (защита ядра — внутри).
pub fn remove_de_live(
    sink: Arc<dyn DeOpSink>,
    cancel: CancelToken,
    de_id: &str,
    current_de: &str,
) -> DeOpOutcome {
    if dc::running_on_steamos() {
        return crate::steamos_sysext::remove_live(sink, cancel, de_id, current_de);
    }
    let io = LiveDeOpsIo::new();
    DeOpsExec::new(&io, sink, cancel).remove_de(de_id, current_de)
}

/// Переключение стола на живой машине одним вызовом.
pub fn switch_de_live(
    sink: Arc<dyn DeOpSink>,
    cancel: CancelToken,
    target: &str,
    current_de: &str,
) -> DeOpOutcome {
    let io = LiveDeOpsIo::new();
    DeOpsExec::new(&io, sink, cancel).switch_de(target, current_de)
}

/// Первое снятие эталона защиты приложений на живой машине.
///
/// Зовётся при старте окна. `None` — эталон уже снят либо защита выключена;
/// в этом случае система ни о чём не спрашивается, читаются только два файла
/// профиля.
pub fn startup_defaults_snapshot_live() -> Option<Result<usize, String>> {
    let io = LiveDeOpsIo::new();
    DeOpsExec::new(&io, Arc::new(SilentSink), CancelToken::new()).defaults_snapshot_if_never()
}

/// Вердикт совместимости на живой машине — для окна подтверждения.
pub fn install_verdict_live(de_id: &str) -> InstallVerdict {
    let io = LiveDeOpsIo::new();
    DeOpsExec::new(&io, Arc::new(SilentSink), CancelToken::new()).install_verdict(de_id)
}

// ============================================================================
// Живой исполнитель
// ============================================================================

/// Настоящий исполнитель: ФС, подпроцессы, часы. Наблюдения и запуск —
/// тем же [`crate::system_exec::LiveSystemIo`], своих команд здесь нет.
pub struct LiveDeOpsIo {
    base: crate::system_exec::LiveSystemIo,
    start: std::time::Instant,
    /// Свои lock-файлы вместо системных (`None` — системные по семейству,
    /// замок pacman — из настоящего `DBPath`).
    lock_paths: Option<Vec<String>>,
}

impl LiveDeOpsIo {
    pub fn new() -> Self {
        LiveDeOpsIo {
            base: crate::system_exec::LiveSystemIo,
            start: std::time::Instant::now(),
            lock_paths: None,
        }
    }

    /// Живой исполнитель, ждущий ДРУГИЕ lock-файлы — для стендов на
    /// подставном пакетном менеджере. Продукт зовёт [`LiveDeOpsIo::new`].
    pub fn with_pkg_lock_paths(paths: Vec<String>) -> Self {
        LiveDeOpsIo {
            lock_paths: Some(paths),
            ..Self::new()
        }
    }

    fn path_env() -> Option<String> {
        std::env::var("PATH").ok()
    }

    fn unique_name(prefix: &str, suffix: &str) -> String {
        use std::sync::atomic::AtomicU64;
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        format!("{prefix}{}_{nanos:x}_{n}{suffix}", std::process::id())
    }
}

impl Default for LiveDeOpsIo {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemIo for LiveDeOpsIo {
    fn run_priv(&self, op: &PrivOp, files: &[String], log_path: Option<&str>) -> Option<RunOut> {
        crate::privilege::run::run_live(op, files, log_path, &mut |_| {})
    }

    fn exists(&self, path: &str) -> bool {
        self.base.exists(path)
    }
    fn is_file(&self, path: &str) -> bool {
        self.base.is_file(path)
    }
    fn is_dir(&self, path: &str) -> bool {
        self.base.is_dir(path)
    }
    fn is_executable(&self, path: &str) -> bool {
        self.base.is_executable(path)
    }
    fn realpath(&self, path: &str) -> Option<String> {
        self.base.realpath(path)
    }
    fn read_text(&self, path: &str) -> Option<String> {
        self.base.read_text(path)
    }
    fn list_dir(&self, dir: &str) -> Option<Vec<String>> {
        self.base.list_dir(dir)
    }
    fn which(&self, name: &str) -> bool {
        self.base.which(name)
    }
    fn run(&self, argv: &[String], timeout_secs: Option<u64>, lang_c: bool) -> RunOut {
        self.base.run(argv, timeout_secs, lang_c)
    }
    fn env(&self, name: &str) -> String {
        self.base.env(name)
    }
    fn which_path(&self, name: &str) -> Option<String> {
        self.base.which_path(name)
    }
    fn write_text(&self, path: &str, content: &str) -> Result<(), String> {
        self.base.write_text(path, content)
    }
    fn copy_file(&self, src: &str, dst: &str) -> Result<(), String> {
        self.base.copy_file(src, dst)
    }
}

impl DeOpsIo for LiveDeOpsIo {
    fn sync_session_env(&self) {
        let _ = crate::compositor_autostart::sync_session_env_live();
    }

    fn pkg_lock_paths(&self, family: PkgFamily) -> Vec<String> {
        match &self.lock_paths {
            Some(paths) => paths.clone(),
            // Замок pacman — из настоящего `DBPath`, а не жёсткий путь.
            None => plan::lock_paths_live(family),
        }
    }

    fn run_to_file(&self, argv: &[String], timeout_secs: Option<u64>, log_path: &str) -> RunOut {
        if argv.is_empty() {
            return RunOut::missing();
        }
        let spec = utils::RunSpec {
            argv: argv.to_vec(),
            output: utils::RunOutput::File(log_path.to_string()),
            timeout_secs,
            stderr_to_stdout: true,
            ..utils::RunSpec::default()
        };
        let report = utils::run(&spec, Self::path_env().as_deref());
        RunOut {
            outcome: report.outcome,
            returncode: report.returncode,
            stdout: String::new(),
            stderr: report.error,
        }
    }

    fn temp_file(&self, prefix: &str, suffix: &str) -> Result<String, String> {
        use std::os::unix::fs::OpenOptionsExt;
        let dir = std::env::temp_dir();
        for _ in 0..16 {
            let path = dir.join(Self::unique_name(prefix, suffix));
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&path)
            {
                Ok(_) => return Ok(path.to_string_lossy().into_owned()),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e.to_string()),
            }
        }
        Err(tr("no free file name found"))
    }

    fn temp_dir(&self, prefix: &str, parent: Option<&str>) -> Result<String, String> {
        use std::os::unix::fs::DirBuilderExt;
        let dir = parent
            .map(std::path::PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        for _ in 0..16 {
            let path = dir.join(Self::unique_name(prefix, ""));
            match std::fs::DirBuilder::new().mode(0o700).create(&path) {
                Ok(()) => return Ok(path.to_string_lossy().into_owned()),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e.to_string()),
            }
        }
        Err(tr("no free file name found"))
    }

    fn write_file_mode(&self, path: &str, content: &str, mode: u32) -> Result<(), String> {
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(path, content).map_err(|e| e.to_string())?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
            .map_err(|e| e.to_string())
    }

    fn make_dirs(&self, path: &str) -> Result<(), String> {
        std::fs::create_dir_all(path).map_err(|e| e.to_string())
    }

    fn remove_file(&self, path: &str) {
        let _ = std::fs::remove_file(path);
    }

    fn remove_tree(&self, path: &str) {
        let _ = std::fs::remove_dir_all(path);
    }

    fn elapsed_secs(&self) -> f64 {
        self.start.elapsed().as_secs_f64()
    }

    fn sleep_secs(&self, secs: u64) {
        std::thread::sleep(std::time::Duration::from_secs(secs));
    }

    fn clock(&self) -> Clock {
        let now = crate::qt_app::Timestamp::now_local().unwrap_or_default();
        let mut seconds = now.clone();
        seconds.microsecond = 0;
        let epoch_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        Clock {
            iso: now.isoformat(),
            iso_seconds: seconds.isoformat(),
            epoch_ms,
        }
    }

    fn tail_log(
        &self,
        path: &str,
        kind: LogTailKind,
        emit: Arc<dyn Fn(&str, &str) + Send + Sync>,
    ) -> Option<plan::LogTail> {
        Some(plan::tail_log(path, kind, move |s: &str, m: &str| emit(s, m)))
    }

    fn plasma_backup(&self, reason: &str) -> Result<(bool, String), String> {
        let home = self.env("HOME");
        let xdg_config = std::env::var("XDG_CONFIG_HOME").ok().filter(|v| !v.is_empty());
        let xdg_data = std::env::var("XDG_DATA_HOME").ok().filter(|v| !v.is_empty());
        let configs = backup::expand_tilde_xdg(backup::FULL_CONFIGS, &home, xdg_config.as_deref());
        let dir = format!(
            "{}/rexon/backups/plasma-panel",
            utils::data_home(xdg_data.as_deref(), &home)
        );
        let stamp = crate::qt_app::Timestamp::now_local()
            .unwrap_or_default()
            .file_stamp();
        let outcome = backup::plasma_backup_run(&dir, &configs, reason, true, &stamp);
        if matches!(outcome, Ok((true, _))) {
            let bridge = crate::rexon_bridge::RexonBridge::new_default();
            let clock = self.clock();
            let _ = bridge
                .ensure_dirs()
                .and_then(|_| bridge.notify_backup_created(&clock.iso, clock.epoch_ms));
            bridge.cleanup_commands(300.0, clock.epoch_ms as f64 / 1000.0);
        }
        outcome
    }

    fn app_defaults_switch(&self, de_id: &str) -> (bool, Vec<String>) {
        use crate::appdefaults_exec::{AppDefaultsEnv, DeAppSwitcher, LiveAppDefaultsIo};
        let mut logs = Vec::new();
        let ok = DeAppSwitcher::new(&LiveAppDefaultsIo, AppDefaultsEnv::from_process())
            .on_switch_de(de_id, &mut logs);
        (ok, logs)
    }

    fn de_config_backup(&self, de_id: &str, paths: &[String]) -> Result<(String, usize), String> {
        use std::path::Path;
        let home = self.env("HOME");
        let xdg_data = std::env::var("XDG_DATA_HOME").ok().filter(|v| !v.is_empty());
        let root = format!("{}/rexon/backups", utils::data_home(xdg_data.as_deref(), &home));
        // Переезд со старого места — как `BackupManager.__init__`.
        let old = format!("{home}/.rexon-backups");
        if Path::new(&old).is_dir() && !Path::new(&root).is_dir() {
            if let Some(parent) = Path::new(&root).parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            std::fs::rename(&old, &root).map_err(|e| e.to_string())?;
        }
        std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
        let meta_file = format!("{root}/metadata.json");
        let mut metadata = std::fs::read_to_string(&meta_file)
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .and_then(|v| v.as_object().cloned())
            .unwrap_or_default();
        let stamp = crate::qt_app::Timestamp::now_local()
            .unwrap_or_default()
            .file_stamp();
        let name = backup::backup_name(de_id, &stamp);
        let path = format!("{root}/{name}");
        std::fs::create_dir_all(&path).map_err(|e| e.to_string())?;
        let present: Vec<String> = paths
            .iter()
            .filter(|p| Path::new(p.as_str()).exists())
            .cloned()
            .collect();
        let mut saved = Vec::new();
        for (src, dst) in backup::snapshot_items(&present, &path, &home) {
            backup::copy_config_into(&src, &dst).map_err(|e| format!("{src}: {e}"))?;
            saved.push(src);
        }
        metadata.insert(name.clone(), backup::de_meta_entry(de_id, &stamp, &saved, &path));
        let text =
            serde_json::to_string_pretty(&Value::Object(metadata)).map_err(|e| e.to_string())?;
        // Общая дверь ядра: половина описи — это копии настроек стола,
        // которые лежат на диске, но восстановить их уже нечем.
        crate::utils::atomic_write(&meta_file, &text).map_err(|e| e.to_string())?;
        Ok((name, saved.len()))
    }

    fn bridge_notify(&self, event: &BridgeEvent) {
        let bridge = crate::rexon_bridge::RexonBridge::new_default();
        let clock = self.clock();
        let sent = bridge.ensure_dirs().and_then(|_| match event {
            BridgeEvent::DeInstalled(de) => {
                bridge.notify_de_installed(de, &clock.iso, clock.epoch_ms)
            }
            BridgeEvent::DeSwitch { from, to } => {
                bridge.notify_de_switch(from, to, &clock.iso, clock.epoch_ms)
            }
            BridgeEvent::DeRemoved(de) => {
                bridge.notify_de_removed(de, &clock.iso, clock.epoch_ms)
            }
        });
        if let Err(e) = sent {
            crate::log_warn!("bridge", "мост BonziBuddy: событие не отправлено — {e}");
        }
        bridge.cleanup_commands(300.0, clock.epoch_ms as f64 / 1000.0);
    }
}

// ============================================================================
// Сверки на подставном исполнителе
// ============================================================================

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::{HashMap, HashSet};
    use std::sync::Mutex;

    pub(crate) type Responder = Box<dyn Fn(&[String]) -> RunOut>;

    pub(crate) fn out(rc: i32, stdout: &str, stderr: &str) -> RunOut {
        RunOut {
            outcome: if rc == 0 {
                RunOutcome::Ok
            } else {
                RunOutcome::Failed
            },
            returncode: Some(rc),
            stdout: stdout.to_string(),
            stderr: stderr.to_string(),
        }
    }

    /// Подставной исполнитель: ФС в памяти, журнал вызовов, ответы команд —
    /// замыканием сценария. Живая машина не трогается вовсе.
    pub(crate) struct FakeIo {
        pub calls: RefCell<Vec<String>>,
        pub files: RefCell<HashMap<String, String>>,
        pub dirs: RefCell<HashSet<String>>,
        pub bins: HashSet<String>,
        pub env: HashMap<String, String>,
        pub responder: Responder,
        pub clock: Cell<f64>,
        pub temp_n: Cell<u32>,
        pub cancel_on_sleep: Option<CancelToken>,
        /// Сеанс прав окна есть: ответ его помощника на любую операцию.
        pub session: Option<RunOut>,
        /// Операции, ушедшие помощнику сеанса.
        pub priv_ops: RefCell<Vec<PrivOp>>,
    }

    impl FakeIo {
        pub(crate) fn arch(responder: Responder) -> Self {
            let mut files = HashMap::new();
            files.insert(
                "/etc/os-release".to_string(),
                "ID=arch\nNAME=\"Arch Linux\"\nPRETTY_NAME=\"Arch Linux\"\n".to_string(),
            );
            let bins = ["pacman", "pkexec", "systemctl", "xdg-mime", "pgrep"]
                .iter()
                .map(|s| s.to_string())
                .collect();
            let mut env = HashMap::new();
            env.insert("HOME".to_string(), "/home/t".to_string());
            env.insert("LANG".to_string(), "ru_RU.UTF-8".to_string());
            FakeIo {
                calls: RefCell::new(Vec::new()),
                files: RefCell::new(files),
                dirs: RefCell::new(HashSet::new()),
                bins,
                env,
                responder,
                clock: Cell::new(0.0),
                temp_n: Cell::new(0),
                cancel_on_sleep: None,
                session: None,
                priv_ops: RefCell::new(Vec::new()),
            }
        }

        fn note(&self, s: String) {
            self.calls.borrow_mut().push(s);
        }

        pub(crate) fn runs(&self) -> Vec<String> {
            self.calls
                .borrow()
                .iter()
                .filter_map(|c| c.strip_prefix("run: ").map(str::to_string))
                .collect()
        }

        pub(crate) fn index_of(&self, needle: &str) -> Option<usize> {
            self.calls.borrow().iter().position(|c| c.contains(needle))
        }
    }

    impl SystemIo for FakeIo {
        fn run_priv(&self, op: &PrivOp, _files: &[String], log: Option<&str>) -> Option<RunOut> {
            let r = self.session.clone()?;
            self.note(format!("priv: {}", serde_json::to_string(op).unwrap_or_default()));
            if let Some(log) = log {
                self.note(format!("priv_log: {log}"));
            }
            self.priv_ops.borrow_mut().push(op.clone());
            Some(r)
        }
        fn exists(&self, path: &str) -> bool {
            self.files.borrow().contains_key(path) || self.dirs.borrow().contains(path)
        }
        fn is_file(&self, path: &str) -> bool {
            self.files.borrow().contains_key(path)
        }
        fn is_dir(&self, path: &str) -> bool {
            self.dirs.borrow().contains(path)
        }
        fn is_executable(&self, _path: &str) -> bool {
            false
        }
        fn realpath(&self, _path: &str) -> Option<String> {
            None
        }
        fn read_text(&self, path: &str) -> Option<String> {
            self.files.borrow().get(path).cloned()
        }
        fn list_dir(&self, _dir: &str) -> Option<Vec<String>> {
            None
        }
        fn which(&self, name: &str) -> bool {
            self.bins.contains(name)
        }
        fn run(&self, argv: &[String], _timeout: Option<u64>, _lang_c: bool) -> RunOut {
            self.note(format!("run: {}", argv.join(" ")));
            (self.responder)(argv)
        }
        fn env(&self, name: &str) -> String {
            self.env.get(name).cloned().unwrap_or_default()
        }
        fn which_path(&self, _name: &str) -> Option<String> {
            None
        }
        fn write_text(&self, path: &str, content: &str) -> Result<(), String> {
            self.note(format!("write: {path}"));
            self.files
                .borrow_mut()
                .insert(path.to_string(), content.to_string());
            Ok(())
        }
        fn copy_file(&self, src: &str, dst: &str) -> Result<(), String> {
            let text = self.read_text(src).ok_or("нет файла")?;
            self.write_text(dst, &text)
        }
    }

    impl DeOpsIo for FakeIo {
        fn run_to_file(&self, argv: &[String], timeout: Option<u64>, log: &str) -> RunOut {
            let r = self.run(argv, timeout, false);
            // Вывод сборщика — в его журнал, как у живого исполнителя. Только
            // у журнала пересборки оформления: прочие сверки ждут пустой файл.
            if log.starts_with("/tmp/rexon_aeroshell_") {
                self.files
                    .borrow_mut()
                    .insert(log.to_string(), format!("{}{}", r.stdout, r.stderr));
            }
            r
        }
        fn temp_file(&self, prefix: &str, suffix: &str) -> Result<String, String> {
            let n = self.temp_n.get() + 1;
            self.temp_n.set(n);
            let p = format!("/tmp/{prefix}{n}{suffix}");
            self.files.borrow_mut().insert(p.clone(), String::new());
            Ok(p)
        }
        fn temp_dir(&self, prefix: &str, _parent: Option<&str>) -> Result<String, String> {
            let n = self.temp_n.get() + 1;
            self.temp_n.set(n);
            let p = format!("/tmp/{prefix}{n}");
            self.dirs.borrow_mut().insert(p.clone());
            Ok(p)
        }
        fn write_file_mode(&self, path: &str, content: &str, mode: u32) -> Result<(), String> {
            self.note(format!("write_mode: {path} {mode:o}"));
            self.files
                .borrow_mut()
                .insert(path.to_string(), content.to_string());
            Ok(())
        }
        fn make_dirs(&self, path: &str) -> Result<(), String> {
            self.dirs.borrow_mut().insert(path.to_string());
            Ok(())
        }
        fn remove_file(&self, path: &str) {
            self.note(format!("remove: {path}"));
            self.files.borrow_mut().remove(path);
        }
        fn remove_tree(&self, path: &str) {
            self.dirs.borrow_mut().remove(path);
        }
        fn elapsed_secs(&self) -> f64 {
            self.clock.get()
        }
        fn sleep_secs(&self, secs: u64) {
            self.note(format!("sleep: {secs}"));
            self.clock.set(self.clock.get() + secs as f64);
            if let Some(token) = &self.cancel_on_sleep {
                token.cancel();
            }
        }
        fn clock(&self) -> Clock {
            Clock {
                iso: "2026-09-12T10:00:00.5".to_string(),
                iso_seconds: "2026-09-12T10:00:00".to_string(),
                epoch_ms: 1,
            }
        }
        fn tail_log(
            &self,
            _path: &str,
            _kind: LogTailKind,
            _emit: Arc<dyn Fn(&str, &str) + Send + Sync>,
        ) -> Option<plan::LogTail> {
            None
        }
        fn plasma_backup(&self, reason: &str) -> Result<(bool, String), String> {
            self.note(format!("plasma_backup: {reason}"));
            Ok((true, "ok".to_string()))
        }
        fn app_defaults_switch(&self, de_id: &str) -> (bool, Vec<String>) {
            self.note(format!("app_defaults: {de_id}"));
            (true, vec!["  ✅ профиль".to_string()])
        }
        fn bridge_notify(&self, event: &BridgeEvent) {
            self.note(format!("bridge: {event:?}"));
        }
        fn sync_session_env(&self) {
            self.note("sync_session_env".to_string());
        }
        fn de_config_backup(&self, de_id: &str, paths: &[String]) -> Result<(String, usize), String> {
            self.note(format!("config_backup: {de_id} {}", paths.len()));
            Ok((format!("{de_id}_20260912_100000"), paths.len()))
        }
    }

    /// Подставной приёмник хода.
    #[derive(Default)]
    pub(crate) struct Recorder {
        pub events: Mutex<Vec<(String, String, Option<f64>)>>,
        pub cancellable: Mutex<Vec<bool>>,
    }

    impl DeOpSink for Recorder {
        fn progress(&self, stage: &str, message: &str, fraction: Option<f64>) {
            self.events
                .lock()
                .unwrap()
                .push((stage.to_string(), message.to_string(), fraction));
        }
        fn cancellable(&self, allowed: bool) {
            self.cancellable.lock().unwrap().push(allowed);
        }
    }

    impl Recorder {
        pub(crate) fn stages(&self) -> Vec<String> {
            self.events.lock().unwrap().iter().map(|e| e.0.clone()).collect()
        }
        pub(crate) fn count(&self, stage: &str) -> usize {
            self.stages().iter().filter(|s| *s == stage).count()
        }
        pub(crate) fn messages(&self, stage: &str) -> Vec<String> {
            self.events
                .lock()
                .unwrap()
                .iter()
                .filter(|e| e.0 == stage)
                .map(|e| e.1.clone())
                .collect()
        }
    }

    /// Ответы «исправной машины»: экран входа включён, пакеты не стоят,
    /// пароль принят.
    pub(crate) fn healthy(argv: &[String]) -> RunOut {
        let a: Vec<&str> = argv.iter().map(String::as_str).collect();
        match a.as_slice() {
            ["systemctl", "is-enabled", unit] if *unit == "sddm.service" => out(0, "enabled\n", ""),
            ["systemctl", "is-enabled", ..] => out(1, "disabled\n", ""),
            ["pacman", "-Q", ..] => out(1, "", "error: package was not found"),
            ["xdg-mime", "query", "default", _] => out(0, "org.kde.dolphin.desktop\n", ""),
            _ => out(0, "", ""),
        }
    }

    fn run_install(io: &FakeIo, de: &str, cancel: CancelToken) -> (DeOpOutcome, Arc<Recorder>) {
        let rec = Arc::new(Recorder::default());
        let sink: Arc<dyn DeOpSink> = rec.clone();
        let outcome = DeOpsExec::new(io, sink, cancel).install_de(de, "kde");
        (outcome, rec)
    }

    /// Успех: замок проверен ДО пароля, главный шаг один, временные файлы
    /// убраны, экран входа не тронут (он уже есть), итог — «done».
    #[test]
    fn install_success_runs_plan_in_order() {
        let io = FakeIo::arch(Box::new(healthy));
        let (outcome, rec) = run_install(&io, "xfce", CancelToken::new());
        assert!(outcome.ok, "{outcome:?}");
        let runs = io.runs();
        let main: Vec<_> = runs.iter().filter(|r| r.starts_with("pkexec bash /tmp/rexon_install_")).collect();
        assert_eq!(main.len(), 1, "{runs:#?}");
        // Замок проверяется до пароля: шагов сна нет (замка нет), но скрипт
        // записан до запуска.
        let write = io.index_of("write_mode: /tmp/rexon_install_1.sh").unwrap();
        let run = io.index_of("run: pkexec bash").unwrap();
        assert!(write < run);
        assert!(io.index_of("remove: /tmp/rexon_install_1.sh").unwrap() > run);
        assert!(io.index_of("remove: /tmp/rexon_install_2.log").unwrap() > run);
        // Экран входа уже включён — ни установки SDDM, ни systemctl enable.
        assert!(
            !runs.iter().any(|r| r.contains("systemctl enable") || r.contains("-S sddm")),
            "{runs:#?}"
        );
        assert_eq!(rec.stages().last().map(String::as_str), Some("done"));
        assert_eq!(rec.count(de_progress::STAGE_ERROR), 0);
        // Отмена была доступна в начале и выключена к главному шагу.
        let flags = rec.cancellable.lock().unwrap().clone();
        assert_eq!(flags.first(), Some(&true));
        assert_eq!(flags.last(), Some(&false));
        // Снимок умолчаний записан до пакетного менеджера.
        assert!(io.index_of("write: /home/t/.config/rexon/protected_defaults.json").unwrap() < run);
        assert!(io.index_of("bridge: DeInstalled(\"xfce\")").is_some());
    }

    /// Помеченное в базе как AUR, но лежащее в репозиториях машины (CachyOS:
    /// темы и значки Mint в `cachyos`), уходит в общую транзакцию pacman;
    /// чего в репозиториях нет — остаётся AUR. Решает `pacman -Si`, а не
    /// таблица.
    #[test]
    fn install_moves_repo_available_aur_packages_to_pacman() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let in_repo = ["mint-themes", "mint-x-icons", "mint-y-icons"];
        let io = FakeIo::arch(Box::new(move |argv: &[String]| {
            let a: Vec<&str> = argv.iter().map(String::as_str).collect();
            match a.as_slice() {
                ["pacman", "-Si", p] if in_repo.contains(p) => {
                    out(0, &format!("Repository      : cachyos\nName            : {p}\n"), "")
                }
                ["pacman", "-Si", _] => out(1, "", "error: package 'x' was not found"),
                _ => healthy(argv),
            }
        }));
        let (outcome, rec) = run_install(&io, "cinnamon-mint", CancelToken::new());
        assert!(outcome.ok, "{outcome:?}");
        let main = system::get_packages("cinnamon-mint", PkgFamily::Arch).len();
        let aur = system::get_aur_packages("cinnamon-mint", PkgFamily::Arch).len();
        assert_eq!(rec.messages("install").first(), Some(&install_intro(main + 3, aur - 3)), "{:#?}", rec.events);
        let runs = io.runs();
        for p in in_repo {
            assert!(runs.iter().any(|r| r == &format!("pacman -Si {p}")), "{runs:#?}");
        }

        // Ни один не нашёлся — всё остаётся в AUR, как в таблице.
        let io = FakeIo::arch(Box::new(|argv: &[String]| {
            let a: Vec<&str> = argv.iter().map(String::as_str).collect();
            match a.as_slice() {
                ["pacman", "-Si", _] => out(1, "", "error: package 'x' was not found"),
                _ => healthy(argv),
            }
        }));
        let (outcome, rec) = run_install(&io, "cinnamon-mint", CancelToken::new());
        assert!(outcome.ok, "{outcome:?}");
        assert_eq!(rec.messages("install").first(), Some(&install_intro(main, aur)));
    }

    /// Отказ пароля (pkexec 126): человек видит объяснение ОДИН раз, дальше
    /// установка не идёт — ни приложений по умолчанию, ни экрана входа.
    #[test]
    fn install_pkexec_denied_stops_with_one_error() {
        // Тексты — из каталога перевода; язык исходный (русский сверяют
        // сверки ядра на po/ru.po).
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let io = FakeIo::arch(Box::new(|argv: &[String]| {
            if argv.first().map(String::as_str) == Some("pkexec") {
                out(126, "", "Error executing command as another user: Request dismissed")
            } else {
                healthy(argv)
            }
        }));
        let (outcome, rec) = run_install(&io, "xfce", CancelToken::new());
        assert!(!outcome.ok);
        assert!(!outcome.cancelled);
        assert!(outcome.message.contains("The administrator password wasn't confirmed"), "{}", outcome.message);
        assert_eq!(rec.count(de_progress::STAGE_ERROR), 1, "{:#?}", rec.stages());
        assert!(io.index_of("app_defaults:").is_none());
        assert_eq!(io.runs().iter().filter(|r| r.starts_with("pkexec")).count(), 1);
        // Временные файлы убраны и при отказе.
        assert!(!io.files.borrow().keys().any(|k| k.starts_with("/tmp/rexon_install_")));
    }

    /// «Окно пароля запущено?» спрашивается у самого polkit, а не по именам
    /// процессов: под прежний шаблон `pgrep` всегда попадала системная
    /// служба polkitd, и ответ был «да» даже на голом sway.
    #[test]
    fn denied_password_asks_polkit_whether_agent_runs() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        for (agent_answer, expected) in [
            (out(0, "()\n", ""), "The service that shows the password window isn't running."),
            (
                out(
                    1,
                    "",
                    "Error: GDBus.Error:org.freedesktop.PolicyKit1.Error.Failed: \
                     An authentication agent already exists for the given subject\n",
                ),
                "The administrator password wasn't confirmed",
            ),
        ] {
            let mut io = FakeIo::arch(Box::new(move |argv: &[String]| match argv[0].as_str() {
                "pkexec" => out(126, "", "Error executing command as another user: Not authorized"),
                "gdbus" => agent_answer.clone(),
                _ => healthy(argv),
            }));
            io.env.insert("XDG_SESSION_ID".to_string(), "2".to_string());
            let (outcome, _) = run_install(&io, "xfce", CancelToken::new());
            assert!(outcome.message.starts_with(expected), "{}", outcome.message);
            let probe = io.runs().into_iter().find(|r| r.starts_with("gdbus")).expect("polkit не спросили");
            assert!(probe.contains("RegisterAuthenticationAgent"), "{probe}");
            assert!(probe.contains("<'2'>"), "{probe}");
            assert!(!io.runs().iter().any(|r| r.starts_with("pgrep")));
        }
    }

    /// Нет окна пароля — подсказка называет агент, который реально лежит на
    /// этой машине (у polkit-gnome путь свой в каждом дистрибутиве), а не
    /// путь, угаданный за всех.
    #[test]
    fn missing_agent_hint_names_agent_found_here() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let no_agent = "Error executing command as another user: No authentication agent found.\n";
        let io = FakeIo::arch(Box::new(move |argv: &[String]| match argv[0].as_str() {
            "pkexec" => out(127, "", no_agent),
            _ => healthy(argv),
        }));
        let agent = "/usr/lib/polkit-gnome/polkit-gnome-authentication-agent-1";
        io.files.borrow_mut().insert(agent.to_string(), String::new());
        let (outcome, _) = run_install(&io, "xfce", CancelToken::new());
        assert!(outcome.message.contains(&format!("\n  {agent} &\n")), "{}", outcome.message);
        assert!(!outcome.message.contains("KDE:"), "{}", outcome.message);

        // Ни одного агента на машине — общий совет, без выдуманного пути.
        let io = FakeIo::arch(Box::new(move |argv: &[String]| match argv[0].as_str() {
            "pkexec" => out(127, "", no_agent),
            _ => healthy(argv),
        }));
        let (outcome, _) = run_install(&io, "xfce", CancelToken::new());
        assert!(outcome.message.contains("polkit-gnome"), "{}", outcome.message);
        assert!(!outcome.message.contains("/usr/lib"), "{}", outcome.message);
    }

    /// Занятый замок, который так и не освободился: пароль НЕ спрашивается,
    /// итог — текст ядра о зависшей транзакции.
    #[test]
    fn install_lock_never_freed_does_not_ask_password() {
        let io = FakeIo::arch(Box::new(healthy));
        io.files
            .borrow_mut()
            .insert("/var/lib/pacman/db.lck".to_string(), String::new());
        let (outcome, rec) = run_install(&io, "xfce", CancelToken::new());
        assert!(!outcome.ok);
        assert_eq!(outcome.message, plan::pkg_lock_timeout_message(plan::PKG_LOCK_TIMEOUT_SECS));
        assert!(!io.runs().iter().any(|r| r.starts_with("pkexec")));
        assert_eq!(rec.count(plan::PKG_LOCK_WAIT_STAGE), 1, "ожидание объявлено один раз");
        assert!(!io.files.borrow().keys().any(|k| k.starts_with("/tmp/rexon_install_")));
    }

    /// Падение главного шага (транзакция не прошла): текст пакетного
    /// менеджера доходит до человека, дальнейшие шаги не идут.
    #[test]
    fn install_main_step_failure_reports_manager_error() {
        let io = FakeIo::arch(Box::new(|argv: &[String]| {
            if argv.first().map(String::as_str) == Some("pkexec") {
                out(1, "", "error: failed to commit transaction (conflicting files)")
            } else {
                healthy(argv)
            }
        }));
        let (outcome, rec) = run_install(&io, "xfce", CancelToken::new());
        assert!(!outcome.ok);
        assert!(outcome.message.contains("failed to commit transaction"));
        assert_eq!(rec.count(de_progress::STAGE_ERROR), 1);
        assert!(!rec.stages().contains(&"done".to_string()));
    }

    /// Конфликт, который всё же дошёл до настоящей транзакции (записанный
    /// вывод pacman 19.10.2026): человек видит объяснение, а не сырой
    /// английский текст pacman.
    #[test]
    fn install_conflict_from_pacman_is_explained() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let io = FakeIo::arch(Box::new(|argv: &[String]| {
            if argv.first().map(String::as_str) == Some("pkexec") {
                out(
                    1,
                    "",
                    ":: deepin-kwin-6.0.8-12.1 and kwin-6.8.0-1.1 are in conflict. Remove kwin? [y/N] \n\
                     error: unresolvable package conflicts detected\n\
                     error: failed to prepare transaction (conflicting dependencies)\n\
                     :: deepin-kwin-6.0.8-12.1 and kwin-6.8.0-1.1 are in conflict\n",
                )
            } else {
                healthy(argv)
            }
        }));
        let (outcome, _rec) = run_install(&io, "pantheon", CancelToken::new());
        assert!(!outcome.ok);
        assert_eq!(
            outcome.message,
            plan::conflict_explanation(&[plan::PkgConflict {
                new_package: "deepin-kwin".into(),
                installed: "kwin".into(),
                ..plan::PkgConflict::default()
            }])
        );
        assert!(outcome.message.contains("KDE Plasma"), "{}", outcome.message);
    }

    /// Конфликт найден пробой ДО пароля: отказ с объяснением, pkexec не
    /// запускается вовсе.
    #[test]
    fn install_refuses_conflicting_set_before_password() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let io = FakeIo::arch(Box::new(|argv: &[String]| {
            let a: Vec<&str> = argv.iter().map(String::as_str).collect();
            match a.as_slice() {
                ["pacman", "-Sp", ..] => out(0, "deepin-kwin\nlightdm\n", ""),
                ["pacman", "-Qq"] => out(0, "glibc\nkwin\n", ""),
                ["pacman", "-Si", ..] => out(0, "Name            : deepin-kwin\nConflicts With  : kwin  kwin-x11\n", ""),
                ["pacman", "-Qi"] => out(0, "Name            : kwin\nConflicts With  : None\n", ""),
                _ => healthy(argv),
            }
        }));
        let (outcome, rec) = run_install(&io, "pantheon", CancelToken::new());
        assert!(!outcome.ok);
        assert!(outcome.message.contains("deepin-kwin") && outcome.message.contains("KDE Plasma"), "{}", outcome.message);
        assert!(!io.runs().iter().any(|r| r.starts_with("pkexec")), "{:#?}", io.runs());
        assert_eq!(rec.count(de_progress::STAGE_ERROR), 1);
    }

    /// Пробы набора River на машине с River 0.5 из extra.
    fn river_over_05(argv: &[String]) -> RunOut {
        let a: Vec<&str> = argv.iter().map(String::as_str).collect();
        match a.as_slice() {
            ["pacman", "-Sp", ..] => out(0, "river-classic\nwaybar\n", ""),
            ["pacman", "-Qq"] => out(0, "glibc\nriver\n", ""),
            ["pacman", "-Si", ..] => out(
                0,
                "Name            : river-classic\nVersion         : 0.3.17-2.1\nConflicts With  : river\n",
                "",
            ),
            ["pacman", "-Qi"] => out(
                0,
                "Name            : river\nVersion         : 0.5.0-1.1\nRequired By     : None\n\
                 Conflicts With  : river-classic\n",
                "",
            ),
            _ => healthy(argv),
        }
    }

    /// River 0.5 стоит, согласия на замену нет: отказ до пароля с текстом
    /// про ветки River, а не общим «конфликтует с установленным пакетом».
    #[test]
    fn install_river_over_05_refuses_without_consent() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let io = FakeIo::arch(Box::new(river_over_05));
        let (outcome, rec) = run_install(&io, "river", CancelToken::new());
        assert!(!outcome.ok);
        assert!(outcome.message.contains("River 0.5") && outcome.message.contains("river-classic"), "{}", outcome.message);
        assert!(!io.runs().iter().any(|r| r.starts_with("pkexec")), "{:#?}", io.runs());
        assert_eq!(rec.count(de_progress::STAGE_ERROR), 1);
    }

    /// С согласием заменить river установка идёт: один pkexec обёрткой
    /// (не помощником), стартовый init River положен.
    #[test]
    fn install_river_over_05_replaces_with_consent() {
        let io = FakeIo::arch(Box::new(river_over_05));
        let rec = Arc::new(Recorder::default());
        let sink: Arc<dyn DeOpSink> = rec.clone();
        let outcome = DeOpsExec::new(&io, sink, CancelToken::new()).install_de_replacing(
            "river",
            "kde",
            &["river".to_string()],
        );
        assert!(outcome.ok, "{outcome:?}");
        let runs = io.runs();
        let main: Vec<_> = runs.iter().filter(|r| r.starts_with("pkexec bash /tmp/rexon_install_")).collect();
        assert_eq!(main.len(), 1, "{runs:#?}");
        assert!(io.files.borrow().contains_key("/home/t/.config/river/init"));
    }

    /// Согласие на замену не снимает чужой конфликт: deepin-kwin рядом с KDE
    /// Plasma и «согласие» заменить kwin — прежний отказ до пароля.
    #[test]
    fn consent_does_not_lift_a_foreign_conflict() {
        let io = FakeIo::arch(Box::new(|argv: &[String]| {
            let a: Vec<&str> = argv.iter().map(String::as_str).collect();
            match a.as_slice() {
                ["pacman", "-Sp", ..] => out(0, "deepin-kwin\nlightdm\n", ""),
                ["pacman", "-Qq"] => out(0, "glibc\nkwin\n", ""),
                ["pacman", "-Si", ..] => out(0, "Name            : deepin-kwin\nConflicts With  : kwin  kwin-x11\n", ""),
                ["pacman", "-Qi"] => out(0, "Name            : kwin\nConflicts With  : None\n", ""),
                _ => healthy(argv),
            }
        }));
        let rec = Arc::new(Recorder::default());
        let sink: Arc<dyn DeOpSink> = rec.clone();
        let outcome = DeOpsExec::new(&io, sink, CancelToken::new()).install_de_replacing(
            "pantheon",
            "kde",
            &["kwin".to_string()],
        );
        assert!(!outcome.ok);
        assert!(!io.runs().iter().any(|r| r.starts_with("pkexec")), "{:#?}", io.runs());
    }

    /// Ответы машины с поставленной (или нет) своей сборкой deepin-session
    /// для Treeland.
    fn deepin_machine(built: bool) -> impl Fn(&[String]) -> RunOut {
        move |argv: &[String]| {
            let a: Vec<&str> = argv.iter().map(String::as_str).collect();
            match a.as_slice() {
                ["pacman", "-Q", "deepin-session-treeland" | "deepin-session"] if built => {
                    out(0, "deepin-session-treeland 2.0.33-1.1\n", "")
                }
                ["pacman", "-Sg", "deepin"] => out(0, "deepin ddm\ndeepin deepin-session\ndeepin treeland\n", ""),
                ["pacman", "-Sp", ..] => out(0, "ddm\ntreeland\n", ""),
                _ => healthy(argv),
            }
        }
    }

    /// Deepin без своей сборки: группу целиком (с deepin-session и
    /// deepin-kwin) исполнитель не ставит — отказ до пароля.
    #[test]
    fn deepin_without_own_build_is_refused_before_password() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let io = FakeIo::arch(Box::new(deepin_machine(false)));
        let (outcome, rec) = run_install(&io, "deepin", CancelToken::new());
        assert!(!outcome.ok);
        assert!(outcome.message.starts_with("Rexon supports Deepin only on Treeland"), "{}", outcome.message);
        assert!(!io.runs().iter().any(|r| r.starts_with("pkexec") || r.contains("-Sp")), "{:#?}", io.runs());
        assert_eq!(rec.count(de_progress::STAGE_ERROR), 1);
    }

    /// Своя сборка стоит: ставится набор варианта — члены группы без
    /// deepin-session, и ни одна команда не называет deepin-kwin.
    #[test]
    fn deepin_with_own_build_installs_the_treeland_set() {
        let io = FakeIo::arch(Box::new(deepin_machine(true)));
        let (outcome, _rec) = run_install(&io, "deepin", CancelToken::new());
        assert!(outcome.ok, "{outcome:?}");
        let runs = io.runs();
        let probe = runs.iter().find(|r| r.contains("pacman -Sp")).expect("проба набора");
        let words: Vec<&str> = probe.split_whitespace().collect();
        assert!(words.contains(&"treeland") && words.contains(&"ddm"), "{probe}");
        assert!(!words.iter().any(|w| ["deepin", "deepin-session", "deepin-kwin"].contains(w)), "{probe}");
        assert!(!runs.iter().any(|r| r.contains("deepin-kwin")), "{runs:#?}");
        assert!(runs.iter().any(|r| r.starts_with("pkexec bash /tmp/rexon_install_")), "{runs:#?}");
    }

    /// Сеанс есть: Deepin на Treeland — набором варианта операцией
    /// `DeInstallSet` (правило ядра помощник повторяет сам), без `pkexec` и
    /// без deepin-session / deepin-kwin.
    #[test]
    fn deepin_set_goes_through_privilege_session() {
        let io = with_session(FakeIo::arch(Box::new(deepin_machine(true))), out(0, "", ""));
        let (outcome, _rec) = run_install(&io, "deepin", CancelToken::new());
        assert!(outcome.ok, "{outcome:?}");
        assert!(!io.runs().iter().any(|r| r.starts_with("pkexec bash")), "{:#?}", io.runs());
        let ops = io.priv_ops.borrow();
        let set = ops
            .iter()
            .find_map(|op| match op {
                PrivOp::DeInstallSet { family: PkgFamily::Arch, de_id: Some(de), packages, deps_only, replace, update_first: true }
                    if de == "deepin" && deps_only.is_empty() && replace.is_empty() =>
                {
                    Some(packages.clone())
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("{ops:?}"));
        assert!(set.contains(&"treeland".to_string()) && set.contains(&"ddm".to_string()), "{set:?}");
        assert!(!set.iter().any(|p| ["deepin", "deepin-session", "deepin-kwin"].contains(&p.as_str())), "{set:?}");
    }

    /// Сеанс есть: замена River 0.5 с согласия — `DeInstallSet` с ровно тем
    /// `replace`, что одобрила проба (`plan::conflict_swaps`), без `pkexec`.
    #[test]
    fn river_replacement_goes_through_privilege_session() {
        let io = with_session(FakeIo::arch(Box::new(river_over_05)), out(0, "", ""));
        let rec = Arc::new(Recorder::default());
        let sink: Arc<dyn DeOpSink> = rec.clone();
        let outcome = DeOpsExec::new(&io, sink, CancelToken::new()).install_de_replacing(
            "river",
            "kde",
            &["river".to_string(), "kwin".to_string()],
        );
        assert!(outcome.ok, "{outcome:?}");
        assert!(!io.runs().iter().any(|r| r.starts_with("pkexec bash")), "{:#?}", io.runs());
        let ops = io.priv_ops.borrow();
        assert!(
            ops.iter().any(|op| matches!(op,
                PrivOp::DeInstallSet { de_id: Some(de), replace, .. } if de == "river" && replace == &["river".to_string()])),
            "{ops:?}"
        );
    }

    /// Вопрос перед установкой Deepin — с предупреждением «экспериментально»;
    /// поверх обычного Deepin — отказ.
    #[test]
    fn deepin_verdict_is_experimental_or_refused() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let fresh = FakeIo::arch(Box::new(deepin_machine(false)));
        let rec: Arc<dyn DeOpSink> = Arc::new(Recorder::default());
        let v = DeOpsExec::new(&fresh, rec.clone(), CancelToken::new()).install_verdict("deepin");
        assert!(v.allowed && v.outcome == "warning", "{v:?}");
        let ordinary = FakeIo::arch(Box::new(|argv: &[String]| {
            let a: Vec<&str> = argv.iter().map(String::as_str).collect();
            match a.as_slice() {
                ["pacman", "-Q", "deepin-session"] => out(0, "deepin-session 2.0.33-1\n", ""),
                _ => healthy(argv),
            }
        }));
        let v = DeOpsExec::new(&ordinary, rec, CancelToken::new()).install_verdict("deepin");
        assert!(!v.allowed, "{v:?}");
        assert_eq!(v.title, "Rexon supports Deepin only on Treeland");
    }

    /// Отмена во время ожидания чужого замка: пароль не спрашивается,
    /// временные файлы убраны, итог — «отменено, система не изменена».
    #[test]
    fn install_cancel_while_waiting_lock_changes_nothing() {
        let mut io = FakeIo::arch(Box::new(healthy));
        io.files
            .borrow_mut()
            .insert("/var/lib/pacman/db.lck".to_string(), String::new());
        let token = CancelToken::new();
        io.cancel_on_sleep = Some(token.clone());
        let (outcome, _rec) = run_install(&io, "xfce", token);
        assert!(outcome.cancelled, "{outcome:?}");
        assert_eq!(outcome.message, cancelled_message());
        assert!(!io.runs().iter().any(|r| r.starts_with("pkexec")));
        assert!(!io.files.borrow().keys().any(|k| k.starts_with("/tmp/rexon_install_")));
    }

    /// Отмена, нажатая до начала: ни снимка, ни копии панели, ни команд.
    #[test]
    fn install_cancelled_before_start_touches_nothing() {
        let io = FakeIo::arch(Box::new(healthy));
        let token = CancelToken::new();
        token.cancel();
        let (outcome, _rec) = run_install(&io, "xfce", token);
        assert!(outcome.cancelled);
        assert!(io.index_of("write").is_none(), "{:#?}", io.calls.borrow());
        assert!(!io.runs().iter().any(|r| r.starts_with("pkexec")));
    }

    /// Отказ дистрибутива объясняется ДО первого побочного действия.
    #[test]
    fn install_blocked_distro_explains_before_any_side_effect() {
        let io = FakeIo::arch(Box::new(healthy));
        io.files.borrow_mut().insert(
            "/etc/os-release".to_string(),
            "ID=osnova\nNAME=\"OSnova\"\nPRETTY_NAME=\"ОСнова\"\n".to_string(),
        );
        let (outcome, rec) = run_install(&io, "xfce", CancelToken::new());
        assert!(!outcome.ok);
        assert_eq!(rec.count(de_progress::STAGE_ERROR), 1);
        assert!(io.index_of("write").is_none(), "{:#?}", io.calls.borrow());
        assert!(io.index_of("plasma_backup").is_none());
        assert!(io.runs().is_empty(), "{:#?}", io.runs());
    }

    // ── Сеанс прав окна (Б8) ─────────────────────────────────────────────

    fn with_session(mut io: FakeIo, answer: RunOut) -> FakeIo {
        io.session = Some(answer);
        io
    }

    /// Сеанс есть: набор стола уходит помощнику операцией с таблицей этого
    /// стола, ход — в журнал, который читает `tail_log`; `pkexec` нет.
    #[test]
    fn install_goes_through_privilege_session() {
        let io = with_session(FakeIo::arch(Box::new(healthy)), out(0, "", ""));
        let (outcome, rec) = run_install(&io, "xfce", CancelToken::new());
        assert!(outcome.ok, "{outcome:?}");
        assert!(pkexec_runs(&io).is_empty(), "{:#?}", io.runs());
        let ops = io.priv_ops.borrow();
        let [PrivOp::PkgInstall { family: PkgFamily::Arch, packages, source }] = ops.as_slice() else {
            panic!("{ops:?}");
        };
        assert!(!packages.is_empty());
        assert_eq!(source, &PkgSource::Desktop { de_id: "xfce".into() });
        assert!(io.index_of("priv_log: /tmp/rexon_install_").is_some(), "{:#?}", io.calls.borrow());
        assert_eq!(rec.stages().last().map(String::as_str), Some("done"));
    }

    /// Защита «нет экрана входа» через сеанс: SDDM из таблицы экранов
    /// входа и включение его юнита — операциями, без `pkexec`.
    #[test]
    fn missing_login_manager_is_fixed_through_session() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let io = FakeIo::arch(Box::new(|argv: &[String]| {
            let a: Vec<&str> = argv.iter().map(String::as_str).collect();
            match a.as_slice() {
                ["systemctl", "is-enabled", ..] => out(1, "disabled\n", ""),
                _ => healthy(argv),
            }
        }));
        let io = with_session(io, out(0, "", ""));
        let (outcome, _rec) = run_install(&io, "niri", CancelToken::new());
        assert!(outcome.ok, "{outcome:?}");
        assert!(pkexec_runs(&io).is_empty(), "{:#?}", io.runs());
        let ops = io.priv_ops.borrow();
        assert!(ops.contains(&PrivOp::PkgInstall {
            family: PkgFamily::Arch,
            packages: vec!["sddm".into()],
            source: PkgSource::DisplayManager,
        }), "{ops:?}");
        assert!(ops.contains(&PrivOp::EnableDm { family: PkgFamily::Arch, unit: "sddm".into() }), "{ops:?}");
    }

    /// Отказ помощника сеанса — ошибка установки, прежней команды после
    /// него нет (З5).
    #[test]
    fn install_session_refusal_has_no_raw_retry() {
        let refused = RunOut {
            outcome: RunOutcome::Failed,
            returncode: Some(1),
            stdout: String::new(),
            stderr: "rexon-priv-helper: помощник отказал: refused.not-in-table".into(),
        };
        let io = with_session(FakeIo::arch(Box::new(healthy)), refused);
        let (outcome, rec) = run_install(&io, "xfce", CancelToken::new());
        assert!(!outcome.ok, "{outcome:?}");
        assert!(rec.count(de_progress::STAGE_ERROR) >= 1);
        assert!(pkexec_runs(&io).is_empty(), "{:#?}", io.runs());
        assert_eq!(io.priv_ops.borrow().len(), 1);
    }

    /// Сеанс есть: снятие стола — операцией над тем же набором, что
    /// снимала бы прежняя команда.
    #[test]
    fn remove_goes_through_privilege_session() {
        let plain = FakeIo::arch(Box::new(removable));
        let (outcome, _) = run_remove(&plain, "xfce", "kde");
        assert!(outcome.ok);
        let raw = plain.runs().into_iter().find(|r| r.starts_with("pkexec pacman -Rns --noconfirm")).unwrap();
        let want: Vec<String> =
            raw.split_whitespace().skip(4).filter(|w| *w != "--").map(str::to_string).collect();

        let io = with_session(FakeIo::arch(Box::new(removable)), out(0, "", ""));
        let (outcome, rec) = run_remove(&io, "xfce", "kde");
        assert!(outcome.ok, "{outcome:?}");
        assert!(pkexec_runs(&io).is_empty(), "{:#?}", io.runs());
        assert_eq!(
            io.priv_ops.borrow().as_slice(),
            [PrivOp::PkgRemove { family: PkgFamily::Arch, packages: want }]
        );
        assert_eq!(rec.stages().last().map(String::as_str), Some("done"));
    }

    /// Тайловый WM на машине без экрана входа: SDDM ставится и включается
    /// САМ, без вопроса.
    #[test]
    fn tiling_wm_without_login_manager_gets_sddm() {
        // Тексты — из каталога перевода; язык исходный (русский сверяют
        // сверки ядра на po/ru.po).
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let io = FakeIo::arch(Box::new(|argv: &[String]| {
            let a: Vec<&str> = argv.iter().map(String::as_str).collect();
            match a.as_slice() {
                ["systemctl", "is-enabled", ..] => out(1, "disabled\n", ""),
                _ => healthy(argv),
            }
        }));
        let (outcome, rec) = run_install(&io, "niri", CancelToken::new());
        assert!(outcome.ok, "{outcome:?}");
        let runs = io.runs();
        assert!(runs.iter().any(|r| r.starts_with("pkexec pacman -S") && r.contains("sddm")), "{runs:#?}");
        assert!(runs.iter().any(|r| r == "pkexec systemctl enable sddm.service"), "{runs:#?}");
        assert!(rec.messages("dm_ensure").iter().any(|m| m.contains("SDDM installed")));
        // Экрана входа не было: поднимется он только при старте — итог
        // говорит «перезагрузите».
        assert!(outcome.restart_needed, "{outcome:?}");
    }

    /// River: ставится river-classic, а после пакетов кладётся стартовый
    /// init (исполняемый) — без него River стартует без единой клавиши. Свой
    /// init человека не трогается.
    #[test]
    fn river_gets_starter_init_once() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let io = FakeIo::arch(Box::new(healthy));
        let (outcome, _) = run_install(&io, "river", CancelToken::new());
        assert!(outcome.ok, "{outcome:?}");
        // Экран входа уже стоит, ни ядра, ни модуля ядра в плане — хватит
        // выйти из сеанса (живая установка 24.10.2026 звала перезагружаться).
        assert!(!outcome.restart_needed, "{outcome:?}");
        let runs = io.runs();
        assert!(runs.iter().any(|r| r.starts_with("pacman -Sp") && r.contains(" river-classic ")), "{runs:#?}");
        let init = "/home/t/.config/river/init";
        // Стартовый init плюс раскладка по языку (у подставной машины LANG=ru).
        let kb = crate::keyboard_layout::detect(None, None, None, "ru_RU.UTF-8").unwrap();
        let expected = match crate::keyboard_layout::apply(
            crate::keyboard_layout::Desk::River, plan::river_starter_init(), &[], &kb,
        ) {
            crate::keyboard_layout::Apply::Write(t) => t,
            other => panic!("{other:?}"),
        };
        assert_eq!(io.files.borrow().get(init).map(String::as_str), Some(expected.as_str()));
        assert!(expected.contains("riverctl keyboard-layout -options grp:alt_shift_toggle us,ru\n"));
        assert!(io.calls.borrow().iter().any(|c| c == &format!("write_mode: {init} 755")), "{:#?}", io.calls.borrow());

        // Свой init человека: раскладку он задал сам — не трогается вовсе.
        let mine = "#!/bin/sh\nriverctl keyboard-layout de\nmine\n";
        let io = FakeIo::arch(Box::new(healthy));
        io.files.borrow_mut().insert(init.to_string(), mine.to_string());
        let (outcome, _) = run_install(&io, "river", CancelToken::new());
        assert!(outcome.ok, "{outcome:?}");
        assert_eq!(io.files.borrow().get(init).map(String::as_str), Some(mine));
    }

    /// Тайловым и Wayfire после установки дописывается раскладка: у sway —
    /// свой config с `include` системного, у Wayfire — раздел `[input]`.
    /// Раскладку, заданную человеком, Rexon не трогает; английскому языку
    /// без системной раскладки писать нечего.
    #[test]
    fn tiling_desktops_get_keyboard_layout() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let io = FakeIo::arch(Box::new(healthy));
        io.files.borrow_mut().insert("/etc/sway/config".into(), "set $mod Mod4\n".into());
        let (outcome, _) = run_install(&io, "sway", CancelToken::new());
        assert!(outcome.ok, "{outcome:?}");
        let cfg = io.files.borrow().get("/home/t/.config/sway/config").cloned().unwrap_or_default();
        assert!(cfg.starts_with("include /etc/sway/config\n"), "{cfg}");
        assert!(cfg.contains("xkb_layout \"us,ru\"") && cfg.contains("xkb_options \"grp:alt_shift_toggle\""), "{cfg}");

        let io = FakeIo::arch(Box::new(healthy));
        let (outcome, _) = run_install(&io, "wayfire", CancelToken::new());
        assert!(outcome.ok, "{outcome:?}");
        let ini = io.files.borrow().get("/home/t/.config/wayfire.ini").cloned().unwrap_or_default();
        assert!(ini.starts_with("[input]\n") && ini.contains("xkb_layout = us,ru\n"), "{ini}");

        // Своя раскладка в wayfire.ini — файл бит-в-бит прежний.
        let own = "[input]\nxkb_layout = de\n";
        let io = FakeIo::arch(Box::new(healthy));
        io.files.borrow_mut().insert("/home/t/.config/wayfire.ini".into(), own.into());
        let (outcome, _) = run_install(&io, "wayfire", CancelToken::new());
        assert!(outcome.ok, "{outcome:?}");
        assert_eq!(io.files.borrow().get("/home/t/.config/wayfire.ini").map(String::as_str), Some(own));

        // Английский и пустая система — конфиг не заводится.
        let mut io = FakeIo::arch(Box::new(healthy));
        io.env.insert("LANG".into(), "en_US.UTF-8".into());
        let (outcome, _) = run_install(&io, "wayfire", CancelToken::new());
        assert!(outcome.ok, "{outcome:?}");
        assert!(!io.files.borrow().contains_key("/home/t/.config/wayfire.ini"));

        // Hyprland без конфига и без образца: файл не заводится (Hyprland
        // положит свой при первом входе), в журнале — подсказка.
        let io = FakeIo::arch(Box::new(healthy));
        let (outcome, _) = run_install(&io, "hyprland", CancelToken::new());
        assert!(outcome.ok, "{outcome:?}");
        assert!(!io.files.borrow().keys().any(|k| k.contains("/hypr/")), "{:?}", io.files.borrow().keys());
    }

    /// Незнакомый стол не доходит ни до одной команды.
    #[test]
    fn unknown_desktop_is_refused() {
        let io = FakeIo::arch(Box::new(healthy));
        let (outcome, rec) = run_install(&io, "no-such-de", CancelToken::new());
        assert!(!outcome.ok);
        assert_eq!(outcome.message, unknown_de_message("no-such-de"));
        assert_eq!(rec.count(de_progress::STAGE_ERROR), 1);
        assert!(io.runs().is_empty());
    }

    fn run_switch(io: &FakeIo, target: &str, current: &str) -> (DeOpOutcome, Arc<Recorder>) {
        let rec = Arc::new(Recorder::default());
        let sink: Arc<dyn DeOpSink> = rec.clone();
        let outcome = DeOpsExec::new(io, sink, CancelToken::new()).switch_de(target, current);
        (outcome, rec)
    }

    /// На текущий стол и на неопознанный не переключаемся — ни копий, ни
    /// выхода из сессии.
    #[test]
    fn switch_guard_refuses_without_side_effects() {
        for current in ["xfce", "unknown"] {
            let io = FakeIo::arch(Box::new(healthy));
            let (outcome, rec) = run_switch(&io, "xfce", current);
            assert!(!outcome.ok);
            assert_eq!(
                Some(outcome.message.clone()),
                plan::switch_de_guard(current, "xfce", "XFCE")
            );
            assert_eq!(rec.count(de_progress::STAGE_ERROR), 1);
            assert!(io.calls.borrow().is_empty(), "{:#?}", io.calls.borrow());
        }
    }

    /// Порядок как у оригинала: копии → приложения по умолчанию → мост →
    /// выход; первая удачная команда выхода — последняя.
    #[test]
    fn switch_backs_up_then_logs_out() {
        let mut io = FakeIo::arch(Box::new(|argv: &[String]| {
            if argv.iter().any(|a| a == "--dest=org.kde.LogoutPrompt") {
                out(1, "", "The name is not activatable")
            } else {
                healthy(argv)
            }
        }));
        io.env.insert("XDG_CURRENT_DESKTOP".into(), "KDE".into());
        io.env.insert("XDG_SESSION_ID".into(), "7".into());
        let (outcome, rec) = run_switch(&io, "xfce", "kde");
        assert!(outcome.ok, "{outcome:?}");
        let plasma = io.index_of("plasma_backup: before_switch").unwrap();
        let config = io.index_of("config_backup: kde").unwrap();
        let apps = io.index_of("app_defaults: xfce").unwrap();
        let bridge = io.index_of("bridge: DeSwitch").unwrap();
        let logout = io.index_of("run: dbus-send").unwrap();
        assert!(plasma < config && config < apps && apps < bridge && bridge < logout);
        let runs = io.runs();
        assert_eq!(runs.iter().filter(|r| r.starts_with("dbus-send")).count(), 2, "{runs:#?}");
        assert!(!runs.iter().any(|r| r.starts_with("loginctl")));
        assert_eq!(rec.messages("done"), vec![logout_message()]);
    }

    /// Ни одна команда выхода не сработала — человек видит, почему.
    #[test]
    fn switch_logout_failure_is_reported() {
        let mut io = FakeIo::arch(Box::new(|argv: &[String]| {
            if argv.first().map(String::as_str) == Some("xfce4-session-logout")
                || argv.first().map(String::as_str) == Some("loginctl")
            {
                out(1, "", "нет такой сессии")
            } else {
                healthy(argv)
            }
        }));
        io.env.insert("XDG_CURRENT_DESKTOP".into(), "XFCE".into());
        io.env.insert("XDG_SESSION_ID".into(), "7".into());
        let (outcome, rec) = run_switch(&io, "kde", "xfce");
        assert!(!outcome.ok);
        assert_eq!(outcome.message, "нет такой сессии");
        assert_eq!(rec.count(de_progress::STAGE_ERROR), 1);
        assert!(io.runs().iter().any(|r| r == "loginctl terminate-session 7"));
    }

    fn run_remove(io: &FakeIo, de: &str, current: &str) -> (DeOpOutcome, Arc<Recorder>) {
        let rec = Arc::new(Recorder::default());
        let sink: Arc<dyn DeOpSink> = rec.clone();
        let outcome = DeOpsExec::new(io, sink, CancelToken::new()).remove_de(de, current);
        (outcome, rec)
    }

    /// Пакеты стола стоят, никому снаружи не нужны, пароль принят.
    fn removable(argv: &[String]) -> RunOut {
        let a: Vec<&str> = argv.iter().map(String::as_str).collect();
        match a.as_slice() {
            ["pacman", "-Q", _] => out(0, "", ""),
            ["pacman", flag, ..] if flag.contains('g') || flag.contains('i') => {
                out(1, "", "")
            }
            _ => healthy(argv),
        }
    }

    /// ⛔ Текущий стол не удаляется ДАЖЕ прямым вызовом исполнителя — ни
    /// одной пробы пакетного менеджера, ни пароля. И кнопки на нём нет.
    #[test]
    fn current_desktop_cannot_be_removed_even_by_direct_call() {
        // Тексты — из каталога перевода; язык исходный (русский сверяют
        // сверки ядра на po/ru.po).
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let io = FakeIo::arch(Box::new(removable));
        let (outcome, rec) = run_remove(&io, "kde", "kde");
        assert!(!outcome.ok);
        assert!(outcome.message.starts_with("You can't remove your current desktop"), "{}", outcome.message);
        assert_eq!(rec.count(de_progress::STAGE_ERROR), 1);
        assert!(io.calls.borrow().is_empty(), "{:#?}", io.calls.borrow());
        let kinds: Vec<&str> = crate::qt_desktops::card_buttons(true, true, true)
            .iter()
            .map(|b| b.kind)
            .collect();
        assert!(!kinds.contains(&"remove"), "{kinds:?}");
    }

    /// Обычный путь: пробы → замок → одна привилегированная команда
    /// удаления → событие моста → «удалён».
    #[test]
    fn remove_waits_lock_then_runs_privileged_remove() {
        let io = FakeIo::arch(Box::new(removable));
        let (outcome, rec) = run_remove(&io, "xfce", "kde");
        assert!(outcome.ok, "{outcome:?}");
        let runs = io.runs();
        let removes: Vec<_> = runs
            .iter()
            .filter(|r| r.starts_with("pkexec pacman -Rns --noconfirm"))
            .collect();
        assert_eq!(removes.len(), 1, "{runs:#?}");
        assert!(io.index_of("bridge: DeRemoved(\"xfce\")").unwrap() > io.index_of("run: pkexec").unwrap());
        assert_eq!(rec.stages().last().map(String::as_str), Some("done"));
    }

    /// Сидишь в Xfce — удаление Cinnamon не снимает общий экран входа
    /// `lightdm` и приветствия LightDM (дыра 28.10.2026).
    #[test]
    fn remove_other_desktop_keeps_shared_login_screen() {
        let io = FakeIo::arch(Box::new(removable));
        let (outcome, _rec) = run_remove(&io, "cinnamon-mint", "xfce");
        assert!(outcome.ok, "{outcome:?}");
        let runs = io.runs();
        let remove = runs
            .iter()
            .find(|r| r.starts_with("pkexec pacman -Rns --noconfirm"))
            .unwrap_or_else(|| panic!("{runs:#?}"));
        let names: Vec<&str> = remove.split_whitespace().collect();
        assert!(names.iter().any(|n| n.starts_with("cinnamon")), "{remove}");
        assert!(!names.contains(&"lightdm"), "{remove}");
        assert!(!names.iter().any(|n| system::is_lightdm_greeter(n)), "{remove}");
    }

    /// Отказ пароля — подсказка ядра, стол не объявлен удалённым.
    #[test]
    fn remove_denied_password_shows_hint() {
        // Тексты — из каталога перевода; язык исходный (русский сверяют
        // сверки ядра на po/ru.po).
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let io = FakeIo::arch(Box::new(|argv: &[String]| {
            if argv.first().map(String::as_str) == Some("pkexec") {
                out(126, "", "Request dismissed")
            } else {
                removable(argv)
            }
        }));
        let (outcome, rec) = run_remove(&io, "xfce", "kde");
        assert!(!outcome.ok);
        assert!(outcome.message.contains("The administrator password wasn't confirmed"), "{}", outcome.message);
        assert_eq!(rec.count(de_progress::STAGE_ERROR), 1);
        assert!(io.index_of("bridge:").is_none());
    }

    /// Ничего из пакетов стола не стоит — пароль не спрашивается.
    #[test]
    fn remove_with_nothing_installed_does_not_ask_password() {
        let io = FakeIo::arch(Box::new(healthy));
        let (outcome, _rec) = run_remove(&io, "xfce", "kde");
        assert!(outcome.ok, "{outcome:?}");
        assert!(!io.runs().iter().any(|r| r.starts_with("pkexec")));
    }

    /// Строки хода этого модуля — на «вы» и без прежнего жаргона.
    #[test]
    fn progress_texts_have_no_jargon() {
        for text in [
            snapshot_message(),
            restore_message(),
            mime_message(),
            post_install_message(),
            install_intro(3, 1),
            ensure_dm_enable_failed("code 1"),
        ] {
            for jargon in ["defaults", "MIME", "ensure_display_manager", "snapshot"] {
                assert!(!text.contains(jargon), "жаргон «{jargon}»: {text}");
            }
        }
        assert_eq!(install_intro(1, 0), "📦 Installing: 1 package");
        assert_eq!(install_intro(5, 2), "📦 Installing: 5 packages + 2 from the AUR");
    }

    // ── Комплектность рабочего стола ─────────────────────────────────────

    /// Подставная сторона проверки: какие пакеты стоят, какие программы
    /// есть. Команды починки сюда НЕ приходят — их исполняет `FakeIo`, и по
    /// его журналу видно, спрашивался ли пароль.
    #[derive(Default)]
    struct FakeChecks {
        installed: HashSet<String>,
        bins: HashSet<String>,
        env: HashMap<String, String>,
        runs: Vec<String>,
    }

    impl FakeChecks {
        fn with(installed: &[&str], bins: &[&str]) -> Self {
            let mut env = HashMap::new();
            env.insert("LANG".to_string(), "ru_RU.UTF-8".to_string());
            FakeChecks {
                installed: installed.iter().map(|s| s.to_string()).collect(),
                bins: bins.iter().map(|s| s.to_string()).collect(),
                env,
                runs: Vec::new(),
            }
        }
    }

    impl ComplianceIo for FakeChecks {
        fn run(&mut self, argv: &[String], _timeout: Option<u64>) -> Option<comp::RunResult> {
            self.runs.push(argv.join(" "));
            let a: Vec<&str> = argv.iter().map(String::as_str).collect();
            let ok = match a.as_slice() {
                ["pacman", "-Q", pkg] => self.installed.contains(*pkg),
                _ => true,
            };
            Some(comp::RunResult {
                ok,
                stdout: "'Adwaita'".to_string(),
            })
        }
        fn which(&mut self, name: &str) -> bool {
            self.bins.contains(name)
        }
        fn env(&mut self, name: &str) -> String {
            self.env.get(name).cloned().unwrap_or_default()
        }
        fn exists(&mut self, _: &str) -> bool {
            false
        }
        fn is_file(&mut self, _: &str) -> bool {
            false
        }
        fn is_dir(&mut self, _: &str) -> bool {
            false
        }
        fn read_text(&mut self, _: &str) -> Option<String> {
            None
        }
        fn scandir(&mut self, _: &str) -> Option<Vec<ScanEntry>> {
            None
        }
        fn listdir(&mut self, _: &str) -> Vec<String> {
            Vec::new()
        }
        fn makedirs(&mut self, _: &str) {}
        fn write_text(&mut self, _: &str, _: &str) {}
        fn replace(&mut self, _: &str, _: &str) {}
        fn realpath(&mut self, path: &str) -> String {
            path.to_string()
        }
        fn mkdtemp(&mut self, prefix: &str, _: Option<&str>) -> String {
            format!("/tmp/{prefix}1")
        }
        fn mkstemp(&mut self, prefix: &str, suffix: &str) -> String {
            format!("/tmp/{prefix}1{suffix}")
        }
        fn chmod(&mut self, _: &str, _: u32) {}
        fn remove_file(&mut self, _: &str) {}
        fn remove_tree(&mut self, _: &str) {}
        fn copy_file(&mut self, _: &str, _: &str) {}
    }

    /// Машина на COSMIC: у этого стола две починки (обои и значки) и одна
    /// проверка без починки (gnome-keyring).
    fn cosmic_io(responder: Responder) -> FakeIo {
        let mut io = FakeIo::arch(responder);
        io.env
            .insert("XDG_CURRENT_DESKTOP".to_string(), "COSMIC".to_string());
        io
    }

    fn pkexec_runs(io: &FakeIo) -> Vec<String> {
        io.runs()
            .into_iter()
            .filter(|r| r.starts_with("pkexec"))
            .collect()
    }

    fn survey(io: &FakeIo, checks: &mut FakeChecks) -> CompletenessSurvey {
        DeOpsExec::new(io, Arc::new(Recorder::default()), CancelToken::new())
            .completeness_survey(checks)
    }

    fn complete(
        io: &FakeIo,
        checks: &mut FakeChecks,
        expected: &[(String, String)],
        cancel: CancelToken,
    ) -> (CompletenessOutcome, Arc<Recorder>) {
        let rec = Arc::new(Recorder::default());
        let sink: Arc<dyn DeOpSink> = rec.clone();
        let outcome = DeOpsExec::new(io, sink, cancel).complete_de(checks, expected);
        (outcome, rec)
    }

    /// Всё на месте: доустанавливать нечего, пароль не спрашивается вовсе.
    #[test]
    fn completeness_all_present_asks_nothing() {
        let io = cosmic_io(Box::new(healthy));
        let mut checks =
            FakeChecks::with(&["cosmic-wallpapers", "cosmic-icon-theme", "gnome-keyring"], &[]);
        let s = survey(&io, &mut checks);
        assert_eq!(s.de_id.as_deref(), Some("cosmic"));
        assert!(s.fixes.is_empty(), "{s:?}");
        assert!(s.results.iter().all(|(_, v)| v == "ok"), "{s:?}");
        assert!(io.runs().is_empty(), "проверка без пароля что-то запустила: {:?}", io.runs());
    }

    /// Нехватка → план словами → «Да» → успех: пароль спрашивается только на
    /// втором шаге, ровно по разу на недостающее, и итог — «всё доустановлено».
    #[test]
    fn completeness_missing_plan_then_success() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let io = cosmic_io(Box::new(healthy));
        let mut checks = FakeChecks::with(&["gnome-keyring"], &[]);
        let s = survey(&io, &mut checks);
        let label = |k: &str| comp::fix_label(k).unwrap().to_string();
        assert_eq!(
            s.fixes,
            vec![
                ("cosmic_wallpapers".to_string(), label("cosmic_wallpapers")),
                ("cosmic_icons".to_string(), label("cosmic_icons")),
            ]
        );
        assert!(pkexec_runs(&io).is_empty(), "пароль спрошен до вопроса");
        let labels: Vec<String> = s.fixes.iter().map(|f| f.1.clone()).collect();
        let question = completeness_confirm_text(&labels);
        let bullets = format!("• {}\n• {}", label("cosmic_wallpapers"), label("cosmic_icons"));
        assert!(question.contains(&bullets), "{question}");
        assert!(question.contains("administrator password"), "{question}");

        let (done, rec) = complete(&io, &mut checks, &s.fixes, CancelToken::new());
        assert!(done.outcome.ok, "{done:?}");
        assert_eq!(done.outcome.message, completeness_done_message());
        let runs = pkexec_runs(&io);
        assert_eq!(runs.len(), 2, "{runs:#?}");
        assert!(runs[0].ends_with("cosmic-wallpapers"), "{runs:#?}");
        assert!(runs[1].ends_with("cosmic-icon-theme"), "{runs:#?}");
        let status = |k: &str| done.results.iter().find(|(key, _)| key == k).map(|(_, v)| v.clone());
        assert_eq!(status("cosmic_wallpapers").as_deref(), Some("fixed"));
        assert_eq!(status("cosmic_icons").as_deref(), Some("fixed"));
        // Ход — словами, без argv; отмена была и погасла к первому паролю.
        let steps = rec.messages("install");
        assert_eq!(steps, vec!["📦 Installing: cosmic-wallpapers", "📦 Installing: cosmic-icon-theme"]);
        let flags = rec.cancellable.lock().unwrap().clone();
        assert_eq!(flags.first(), Some(&true));
        assert_eq!(flags.last(), Some(&false));
        assert_eq!(rec.count(de_progress::STAGE_ERROR), 0);
        // Проверка пакетов шла исполнителем проверки, а не через pkexec.
        assert!(checks.runs.iter().any(|r| r == "pacman -Q cosmic-wallpapers"));
        assert!(!checks.runs.iter().any(|r| r.starts_with("pkexec")));
    }

    /// Б9: сеанс прав есть — доустановка уходит помощнику операцией по
    /// таблице починки (`DesktopRepair`), по пакету на починку; `pkexec` нет.
    #[test]
    fn completeness_fix_goes_through_privilege_session() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let io = with_session(cosmic_io(Box::new(healthy)), out(0, "", ""));
        let mut checks = FakeChecks::with(&["gnome-keyring"], &[]);
        let s = survey(&io, &mut checks);
        let (done, _) = complete(&io, &mut checks, &s.fixes, CancelToken::new());
        assert!(done.outcome.ok, "{done:?}");
        assert!(pkexec_runs(&io).is_empty(), "{:#?}", io.runs());
        let want: Vec<PrivOp> = ["cosmic-wallpapers", "cosmic-icon-theme"]
            .iter()
            .map(|p| PrivOp::PkgInstall {
                family: PkgFamily::Arch,
                packages: vec![p.to_string()],
                source: PkgSource::DesktopRepair,
            })
            .collect();
        assert_eq!(io.priv_ops.borrow().as_slice(), want.as_slice());
        // Каждое имя проходит проверку помощника по этой таблице.
        for op in &want {
            let PrivOp::PkgInstall { family, packages, source } = op else { unreachable!() };
            assert!(crate::privilege::validate::check_install(*family, packages, source.install_source("cosmic")).is_ok());
        }
    }

    /// Отказ пароля: одно окно пароля, одно объяснение; второй пакет пароль
    /// уже не спрашивает.
    #[test]
    fn completeness_password_refused_asks_once() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let io = cosmic_io(Box::new(|argv: &[String]| {
            if argv.first().map(String::as_str) == Some("pkexec") {
                out(126, "", "Error executing command as another user: Request dismissed")
            } else {
                healthy(argv)
            }
        }));
        let mut checks = FakeChecks::with(&["gnome-keyring"], &[]);
        let s = survey(&io, &mut checks);
        let (done, rec) = complete(&io, &mut checks, &s.fixes, CancelToken::new());
        assert!(!done.outcome.ok);
        assert!(!done.outcome.cancelled);
        assert_eq!(done.outcome.message, completeness_denied_message());
        assert!(done.outcome.message.starts_with("The administrator password wasn't confirmed"));
        // Подпись кнопки — из каталога «Настроек» на языке интерфейса.
        let button = crate::qt_settings::text("compl.button", &[]).unwrap();
        assert!(done.outcome.message.contains(&format!("“{button}”")));
        assert_eq!(pkexec_runs(&io).len(), 1, "{:#?}", io.runs());
        assert_eq!(rec.count(de_progress::STAGE_ERROR), 1, "{:#?}", rec.stages());
    }

    /// Сбой пакета: второй пакет всё равно ставится, итог называет словами
    /// только то, что не встало, а причина пакетного менеджера — в ходе.
    #[test]
    fn completeness_package_failure_names_what_is_left() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let io = cosmic_io(Box::new(|argv: &[String]| {
            if argv.first().map(String::as_str) == Some("pkexec")
                && argv.last().map(String::as_str) == Some("cosmic-wallpapers")
            {
                out(1, "", "error: target not found: cosmic-wallpapers")
            } else {
                healthy(argv)
            }
        }));
        let mut checks = FakeChecks::with(&["gnome-keyring"], &[]);
        let s = survey(&io, &mut checks);
        let (done, rec) = complete(&io, &mut checks, &s.fixes, CancelToken::new());
        assert!(!done.outcome.ok);
        assert_eq!(
            done.outcome.message,
            completeness_partial_message(&[comp::fix_label("cosmic_wallpapers").unwrap().to_string()])
        );
        assert_eq!(pkexec_runs(&io).len(), 2);
        let warnings = rec.messages(de_progress::STAGE_WARNING);
        assert_eq!(warnings, vec!["Didn't work: error: target not found: cosmic-wallpapers"]);
        assert_eq!(rec.count(de_progress::STAGE_ERROR), 1);
    }

    /// Отмена до первого пароля: ничего не запущено, итог — «отменено».
    /// И во время ожидания чужого замка — тоже.
    #[test]
    fn completeness_cancel_before_password_changes_nothing() {
        let io = cosmic_io(Box::new(healthy));
        let mut checks = FakeChecks::with(&["gnome-keyring"], &[]);
        let s = survey(&io, &mut checks);
        let token = CancelToken::new();
        token.cancel();
        let (done, _) = complete(&io, &mut checks, &s.fixes, token);
        assert!(done.outcome.cancelled, "{done:?}");
        assert!(pkexec_runs(&io).is_empty());

        let mut io = cosmic_io(Box::new(healthy));
        io.files
            .borrow_mut()
            .insert("/var/lib/pacman/db.lck".to_string(), String::new());
        let token = CancelToken::new();
        io.cancel_on_sleep = Some(token.clone());
        let (done, rec) = complete(&io, &mut checks, &s.fixes, token);
        assert!(done.outcome.cancelled, "{done:?}");
        assert!(pkexec_runs(&io).is_empty());
        assert_eq!(rec.count(plan::PKG_LOCK_WAIT_STAGE), 1);
    }

    /// Замок так и не освободился: пароль не спрашивается, итог — текст ядра.
    #[test]
    fn completeness_lock_never_freed_does_not_ask_password() {
        let io = cosmic_io(Box::new(healthy));
        io.files
            .borrow_mut()
            .insert("/var/lib/pacman/db.lck".to_string(), String::new());
        let mut checks = FakeChecks::with(&["gnome-keyring"], &[]);
        let s = survey(&io, &mut checks);
        let (done, _) = complete(&io, &mut checks, &s.fixes, CancelToken::new());
        assert!(!done.outcome.ok);
        assert_eq!(done.outcome.message, plan::pkg_lock_timeout_message(plan::PKG_LOCK_TIMEOUT_SECS));
        assert!(pkexec_runs(&io).is_empty());
    }

    /// ⭐ Сторож полноты подписей: у КАЖДОЙ починки, которую проверка умеет
    /// отложить, есть подпись для окна вопроса, и в таблице нет лишних.
    /// Проверки всех столов гоняются на «пустой» машине, где не стоит ничего.
    #[test]
    fn every_deferred_fix_has_a_label() {
        let mut deferred = std::collections::BTreeSet::new();
        for de in ["mate", "kde", "gnome", "cosmic", "xfce", "cinnamon-mint", "budgie", "hyprland"] {
            assert!(comp::has_checks(de), "{de}");
            let mut checks = FakeChecks::with(&[], &["git", "yay"]);
            let mut log = Vec::new();
            let results = comp::Compliance::new(&mut checks, "/home/t", PkgFamily::Arch, false, false, false)
                .run_for_de(de, &mut log);
            for (key, label) in comp::deferred_fixes(&results) {
                assert!(comp::fix_label(&key).is_some(), "{de}: у починки «{key}» нет подписи");
                assert_ne!(key, label);
                deferred.insert(key);
            }
        }
        let labelled: std::collections::BTreeSet<String> =
            comp::FIX_LABELS.iter().map(|(k, _)| k.to_string()).collect();
        assert_eq!(deferred, labelled, "таблица подписей разошлась с проверками");
    }

    /// Английские тексты хода операций со столом — без русских букв и
    /// читаются фразами (правило перевода: русский только в `po/ru.po`).
    #[test]
    fn english_de_op_texts_have_no_russian() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let cyrillic = |s: &str| s.chars().any(|c| matches!(c, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё'));
        let e = "E";
        let failed = RunOut { outcome: RunOutcome::Timeout, ..out(1, "", "") };
        let labels = vec!["COSMIC wallpapers".to_string()];
        let texts = [
            unknown_de_message("xyz"),
            cancelled_message(),
            status_cancelled(),
            busy_message(),
            dialog_yes(),
            dialog_no(),
            dialog_cancel(),
            awaiting_answer("Install KDE Plasma"),
            snapshot_message(),
            plasma_backup_message(),
            post_install_message(),
            dm_step_message(),
            restore_message(),
            mime_message(),
            snapshot_failed(e),
            plasma_backup_failed(e),
            install_intro(1, 0),
            install_intro(5, 2),
            post_install_failed(e),
            dm_already("sddm"),
            dm_enabled("sddm.service"),
            dm_enable_failed("sddm.service", e),
            switcher_message("KDE Plasma"),
            restore_failed(e),
            installed_message("KDE Plasma"),
            temp_failed(e),
            write_failed("/tmp/x", e),
            unknown_step_message(e),
            gentoo_overlay_intro("guru"),
            gentoo_overlay_done("guru"),
            gentoo_overlay_failed("guru"),
            ensure_dm_install_failed("sddm", e),
            ensure_dm_enable_failed(e),
            run_why(&failed),
            run_why(&out(3, "", "")),
            removing_message("KDE Plasma"),
            removed_message("KDE Plasma"),
            remove_timeout_message(),
            logout_message(),
            logout_failed_message(),
            config_backup_message("KDE Plasma"),
            config_backup_done("kde-2026", 1),
            config_backup_done("kde-2026", 3),
            config_backup_failed(e),
            logout_timeout("qdbus6"),
            logout_not_started("qdbus6"),
            completeness_operation_title(),
            completeness_confirm_title(),
            completeness_checking_message(),
            completeness_done_message(),
            completeness_confirm_text(&labels),
            completeness_partial_message(&labels),
            completeness_denied_message(),
            fix_failed_message(e),
        ];
        for text in &texts {
            assert!(!cyrillic(text), "русское в английском интерфейсе: {text}");
        }
        assert_eq!(install_intro(1, 0), "📦 Installing: 1 package");
        assert_eq!(install_intro(5, 2), "📦 Installing: 5 packages + 2 from the AUR");
        assert_eq!(config_backup_done("kde-2026", 1), "✅ Copy saved: kde-2026 (1 file)");
        assert_eq!(run_why(&out(3, "", "")), "code 3");
        assert_eq!(unknown_de_message("xyz"), "Rexon doesn't know the “xyz” desktop.");
    }

    /// Тексты доустановки — без служебных слов; ход не показывает argv.
    #[test]
    fn completeness_texts_have_no_jargon() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let labels: Vec<String> = comp::FIX_LABELS.iter().map(|(_, l)| l.to_string()).collect();
        let texts = [
            completeness_confirm_text(&labels),
            completeness_partial_message(&labels),
            completeness_denied_message(),
            completeness_operation_title(),
            completeness_confirm_title(),
            completeness_checking_message(),
            completeness_done_message(),
            fix_step_message(&s(&["git", "clone", "--depth", "1", "https://x", "/tmp/r"])),
            fix_step_message(&s(&["pkexec", plan::STAGE_HELPER_PATH, "mate-layouts", "rexon", "/tmp/s"])),
            fix_step_message(&s(&["pkexec", "bash", "/tmp/rexon_layouts_1.sh"])),
            fix_step_message(&s(&["yay", "-S", "--needed", "--noconfirm", "mint-themes"])),
            fix_step_message(&s(&["pkexec", plan::PKG_HELPER_PATH, "arch", "install", "budgie-extras"])),
        ];
        for text in &texts {
            for jargon in ["pkexec", "needs_action", "helper", "хелпер", "/tmp", "/usr", "--", "git", "DE", "compliance"] {
                assert!(!text.contains(jargon), "жаргон «{jargon}»: {text}");
            }
        }
        assert_eq!(texts[10], "📦 Installing from the AUR: mint-themes");
        assert_eq!(texts[11], "📦 Installing: budgie-extras");
    }

    // ── Оформление AeroShell ─────────────────────────────────────────────

    const EFFECT_SO: &str = "/usr/lib/qt6/plugins/kwin/effects/plugins/aeroglide.so";
    const KWIN_COMPONENTS: &str = "aeroshell-kwin-components-git";
    const LIBPLASMA: &str = "aeroshell-libplasma-git";

    /// Машина с оформлением: модуль эффекта собран под KWin 6.7.4, форк
    /// libplasma — под Plasma 6.7.4, пакет значков без собранного кода.
    /// `kwin` — версия KWin, стоящая сейчас; `build` — ответ сборщика.
    fn aero_io(kwin: &'static str, build: Responder) -> FakeIo {
        FakeIo::arch(Box::new(move |argv: &[String]| {
            let a: Vec<&str> = argv.iter().map(String::as_str).collect();
            match a.as_slice() {
                ["pacman", "-Qq"] => out(
                    0,
                    &format!("bash\n{KWIN_COMPONENTS}\n{LIBPLASMA}\naerothemeplasma-icons-git\nkwin\n"),
                    "",
                ),
                ["pacman", "-Q", "kwin"] => out(0, &format!("kwin {kwin}-1\n"), ""),
                ["pacman", "-Q", "plasma-workspace"] => out(0, "plasma-workspace 6.7.4-1.1\n", ""),
                ["pacman", "-Ql", KWIN_COMPONENTS] => {
                    out(0, &format!("{KWIN_COMPONENTS} {EFFECT_SO}\n"), "")
                }
                ["pacman", "-Ql", LIBPLASMA] => out(
                    0,
                    &format!("{LIBPLASMA} /usr/lib/libPlasma.so.6.7.4\n{LIBPLASMA} /usr/lib/libPlasma.so.7\n"),
                    "",
                ),
                ["pacman", "-Ql", "aerothemeplasma-icons-git"] => {
                    out(0, "aerothemeplasma-icons-git /usr/share/icons/a.png\n", "")
                }
                ["env", "LANG=C", "LC_ALL=C", ..] => build(argv),
                _ => healthy(argv),
            }
        }))
    }

    fn aero_survey(io: &FakeIo) -> aeroshell::Report {
        DeOpsExec::new(io, Arc::new(Recorder::default()), CancelToken::new()).aeroshell_survey(
            &|path: &str| {
                (path == EFFECT_SO).then(|| b"\0org.kde.kwin.EffectPluginFactory6.7.4\0".to_vec())
            },
        )
    }

    fn aero_rebuild(io: &FakeIo, packages: &[String], cancel: CancelToken) -> (DeOpOutcome, Arc<Recorder>) {
        let rec = Arc::new(Recorder::default());
        let sink: Arc<dyn DeOpSink> = rec.clone();
        let outcome = DeOpsExec::new(io, sink, cancel).rebuild_aeroshell(packages);
        (outcome, rec)
    }

    fn builder_runs(io: &FakeIo) -> Vec<String> {
        io.runs().into_iter().filter(|r| r.starts_with("env ")).collect()
    }

    fn built(_: &[String]) -> RunOut {
        out(0, "==> Making package: aeroshell-libplasma-git\n\n==> Finished making\n", "")
    }

    /// Машина как у владельца 04.10.2026: от набора остался один форк
    /// libplasma, оболочки нет, версии сходятся. `si_libplasma` — есть ли
    /// штатный libplasma в репозиториях; `remove` — ответ транзакции отката.
    fn lonely_io(si_libplasma: bool, remove: Responder) -> FakeIo {
        FakeIo::arch(Box::new(move |argv: &[String]| {
            let a: Vec<&str> = argv.iter().map(String::as_str).collect();
            match a.as_slice() {
                ["pacman", "-Qq"] => out(0, &format!("bash\n{LIBPLASMA}\nkwin\n"), ""),
                ["pacman", "-Q", "kwin"] => out(0, "kwin 6.7.5-1.1\n", ""),
                ["pacman", "-Q", "plasma-workspace"] => out(0, "plasma-workspace 6.7.5-1.1\n", ""),
                ["pacman", "-Ql", LIBPLASMA] => out(
                    0,
                    &format!("{LIBPLASMA} /usr/lib/libPlasma.so.6.7.5\n{LIBPLASMA} /usr/lib/libPlasma.so.7\n"),
                    "",
                ),
                ["pacman", "-Qi", LIBPLASMA] => out(
                    0,
                    "Name            : aeroshell-libplasma-git\n\
                     Provides        : libplasma  aeroshell-libplasma\n\
                     Replaces        : plasma-framework\n",
                    "",
                ),
                ["pacman", "-Si", "libplasma"] if si_libplasma => {
                    out(0, "Repository      : extra\nName            : libplasma\nVersion         : 6.7.5-1\n", "")
                }
                ["pacman", "-Si", _] => out(1, "", "error: package 'x' was not found"),
                ["pkexec", ..] => remove(argv),
                _ => healthy(argv),
            }
        }))
    }

    fn lonely_advice(io: &FakeIo) -> (aeroshell::Report, aeroshell::Advice) {
        DeOpsExec::new(io, Arc::new(Recorder::default()), CancelToken::new())
            .aeroshell_advice(&|_p: &str| None, &|_p: &str| None)
    }

    /// Одинокий форк: совет — вернуть штатную Plasma, пересборки нет и
    /// пароль до «Да» не спрашивается.
    #[test]
    fn lonely_fork_advises_restore_without_touching_anything() {
        let io = lonely_io(true, Box::new(|_| out(0, "", "")));
        let (report, advice) = lonely_advice(&io);
        assert!(report.lonely_fork && !report.needs_rebuild, "{report:?}");
        assert_eq!(advice, aeroshell::Advice::RestoreStock { forks: vec![LIBPLASMA.to_string()] });
        let runs = io.runs();
        assert!(runs.iter().all(|r| r.starts_with("pacman ")), "{runs:#?}");
        assert!(runs.iter().any(|r| r == "pacman -Si libplasma"), "{runs:#?}");
    }

    /// Журнал сеанса: KWin назвал эффект набора несовместимым, хотя по
    /// версиям в файлах всё сходится, — совет «пересобрать», с версиями из
    /// журнала; сторонний виджет назван отдельной строкой. Читается только
    /// журнал пользователя и база пакетов, без root.
    #[test]
    fn advice_reads_session_journal() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let journal = [
            serde_json::json!({"MESSAGE": "Effect \"aeroglide\" was built for a different version of KWin and needs to be rebuilt, expected \"org.kde.kwin.EffectPluginFactory6.7.4\" got \"org.kde.kwin.EffectPluginFactory6.7.3\"", "__REALTIME_TIMESTAMP": "500"}),
            serde_json::json!({"MESSAGE": "error when loading applet \"Nori.Weather\" QList(\"x\", \"y\")", "__REALTIME_TIMESTAMP": "600"}),
        ]
        .iter()
        .map(|v| v.to_string())
        .collect::<Vec<_>>()
        .join("\n");
        let mut io = aero_io(
            "6.7.4",
            Box::new(|_| out(0, "", "")),
        );
        let inner = std::mem::replace(&mut io.responder, Box::new(|_| out(0, "", "")));
        io.responder = Box::new(move |argv: &[String]| {
            let a: Vec<&str> = argv.iter().map(String::as_str).collect();
            match a.as_slice() {
                ["journalctl", "--user", "-b", ..] => out(0, &journal, ""),
                ["pacman", "-Qq"] => out(
                    0,
                    &format!("{KWIN_COMPONENTS}\n{LIBPLASMA}\naerothemeplasma-desktop-git\nkwin\n"),
                    "",
                ),
                ["pacman", "-Qo", ..] => out(0, &format!("{EFFECT_SO} is owned by {KWIN_COMPONENTS} 6.7.0_61-1\n"), ""),
                ["pacman", "-Qm"] => out(0, &format!("{KWIN_COMPONENTS} 6.7.0_61-1\n{LIBPLASMA} 6.7.4-1\n"), ""),
                _ => inner(argv),
            }
        });
        io.bins.insert("journalctl".to_string());
        let (report, advice) = DeOpsExec::new(&io, Arc::new(Recorder::default()), CancelToken::new()).aeroshell_advice(
            &|path: &str| (path == EFFECT_SO).then(|| b"\0org.kde.kwin.EffectPluginFactory6.7.4\0".to_vec()),
            &|path: &str| (path == EFFECT_SO).then_some(100),
        );
        assert!(report.stale.contains(&KWIN_COMPONENTS.to_string()), "{report:#?}");
        let v = report.packages.iter().find(|p| p.package == KWIN_COMPONENTS).unwrap();
        assert_eq!((v.built_for.as_deref(), v.installed.as_deref()), (Some("6.7.3"), Some("6.7.4")));
        assert_eq!(report.journal.len(), 2, "{:#?}", report.journal);
        let lines = aeroshell::journal_lines(&report);
        assert!(lines[0].contains("KWin 6.7.3") && lines[0].contains(KWIN_COMPONENTS), "{lines:?}");
        assert!(lines[1].contains("Nori.Weather") && lines[1].contains("isn't installed anymore"), "{lines:?}");
        assert!(!matches!(advice, aeroshell::Advice::Nothing), "{advice:?}");
        let runs = io.runs();
        assert!(runs.iter().all(|r| r.starts_with("pacman ") || r.starts_with("journalctl ")), "{runs:#?}");
        assert!(runs.iter().any(|r| r.starts_with("journalctl --user -b") && r.contains("--grep=")), "{runs:#?}");
    }

    /// Штатного libplasma в репозиториях нет — отказ словами сразу.
    #[test]
    fn lonely_fork_without_stock_package_is_blocked() {
        let io = lonely_io(false, Box::new(|_| out(0, "", "")));
        let (_, advice) = lonely_advice(&io);
        assert!(matches!(advice, aeroshell::Advice::RestoreBlocked { .. }), "{advice:?}");
    }

    /// «Да» → одна транзакция отката под одним паролем, ход дела в окно.
    #[test]
    fn restore_stock_runs_one_transaction() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let io = lonely_io(true, Box::new(|_| out(0, "готово\n", "")));
        let rec = Arc::new(Recorder::default());
        let sink: Arc<dyn DeOpSink> = rec.clone();
        let done = DeOpsExec::new(&io, sink, CancelToken::new())
            .restore_stock_plasma(&[LIBPLASMA.to_string()]);
        assert!(done.ok, "{done:?}");
        let pkexecs: Vec<String> = io.runs().into_iter().filter(|r| r.starts_with("pkexec")).collect();
        assert_eq!(pkexecs.len(), 1, "{pkexecs:#?}");
        assert!(pkexecs[0].contains("pacman -S --noconfirm --needed libplasma"), "{pkexecs:?}");
        assert_eq!(rec.messages("install"), vec![aeroshell::restore_start_message()]);
        assert_eq!(rec.messages("done"), vec![aeroshell::restore_done_message()]);
    }

    /// Оболочка стоит, Plasma ушла на 6.8, а ветка форка ещё 6.7: совет —
    /// ждать авторов, а не пересобирать. `fork_cmake` — ответ сырого файла
    /// (None — сеть недоступна).
    fn upstream_io(fork_cmake: Option<&'static str>) -> FakeIo {
        let mut io = FakeIo::arch(Box::new(move |argv: &[String]| {
            let a: Vec<&str> = argv.iter().map(String::as_str).collect();
            match a.as_slice() {
                ["pacman", "-Qq"] => out(0, &format!("{LIBPLASMA}\naerothemeplasma-desktop-git\n"), ""),
                ["pacman", "-Q", "kwin"] => out(0, "kwin 6.8.0-1\n", ""),
                ["pacman", "-Q", "plasma-workspace"] => out(0, "plasma-workspace 6.8.0-1\n", ""),
                ["pacman", "-Ql", LIBPLASMA] => {
                    out(0, &format!("{LIBPLASMA} /usr/lib/libPlasma.so.6.7.5\n"), "")
                }
                ["pacman", "-Qi", LIBPLASMA] => {
                    out(0, "Name            : x\nURL             : https://gitgud.io/aeroshell/libplasma\n", "")
                }
                ["pacman", "-Si", "libplasma"] => out(0, "Version         : 6.8.0-1\n", ""),
                ["env", "GIT_TERMINAL_PROMPT=0", "git", "ls-remote", "--symref", url, "HEAD"] => {
                    assert_eq!(*url, "https://gitgud.io/aeroshell/libplasma.git");
                    out(0, "ref: refs/heads/Plasma/6.7\tHEAD\n6ed0921\tHEAD\n", "")
                }
                ["curl", "-fsSL", "-m", _, url] => {
                    assert_eq!(*url, "https://gitgud.io/aeroshell/libplasma/-/raw/Plasma/6.7/CMakeLists.txt");
                    match fork_cmake {
                        Some(text) => out(0, text, ""),
                        None => out(6, "", "curl: (6) Could not resolve host"),
                    }
                }
                _ => healthy(argv),
            }
        }));
        io.bins.extend(["git", "curl"].map(String::from));
        io
    }

    #[test]
    fn rebuild_waits_while_fork_branch_lags_behind() {
        let io = upstream_io(Some("cmake_minimum_required(VERSION 3.16)\nset(PROJECT_VERSION \"6.7.5\")\n"));
        let (report, advice) = lonely_advice(&io);
        assert!(report.needs_rebuild && !report.lonely_fork, "{report:?}");
        assert_eq!(
            advice,
            aeroshell::Advice::WaitForUpstream { plasma: "6.8.0".into(), fork: "6.7.5".into() }
        );
        assert!(!io.runs().iter().any(|r| r.contains("pkexec") || r.starts_with("env LANG")));
    }

    #[test]
    fn rebuild_offered_once_fork_caught_up() {
        let io = upstream_io(Some("set(PROJECT_VERSION \"6.8.0\")\n"));
        let (_, advice) = lonely_advice(&io);
        assert_eq!(advice, aeroshell::Advice::Rebuild { packages: vec![LIBPLASMA.to_string()] });
    }

    /// Нет сети — проверка молча пропущена, совет прежний.
    #[test]
    fn rebuild_offered_without_network() {
        let io = upstream_io(None);
        let (_, advice) = lonely_advice(&io);
        assert_eq!(advice, aeroshell::Advice::Rebuild { packages: vec![LIBPLASMA.to_string()] });
    }

    /// Сейчас всё работает (6.7.5), а `checkupdates` видит в репозиториях
    /// 6.8.0. `checkupdates` — его ответ (None — программы нет), `fork_cmake`
    /// — `CMakeLists.txt` ветки форка по умолчанию.
    fn before_update_io(checkupdates: Option<(i32, &'static str)>, fork_cmake: &'static str) -> FakeIo {
        let mut io = FakeIo::arch(Box::new(move |argv: &[String]| {
            let a: Vec<&str> = argv.iter().map(String::as_str).collect();
            match a.as_slice() {
                ["pacman", "-Qq"] => out(0, &format!("{LIBPLASMA}\naerothemeplasma-desktop-git\n"), ""),
                ["pacman", "-Q", "kwin"] => out(0, "kwin 6.7.5-1.1\n", ""),
                ["pacman", "-Q", "plasma-workspace"] => out(0, "plasma-workspace 6.7.5-1.1\n", ""),
                ["pacman", "-Ql", LIBPLASMA] => {
                    out(0, &format!("{LIBPLASMA} /usr/lib/libPlasma.so.6.7.5\n"), "")
                }
                ["pacman", "-Qi", LIBPLASMA] => {
                    out(0, "Name            : x\nURL             : https://gitgud.io/aeroshell/libplasma\n", "")
                }
                ["checkupdates"] => {
                    let (rc, text) = checkupdates.expect("checkupdates не должен зваться");
                    out(rc, text, if rc == 1 { "==> ERROR: Cannot fetch updates" } else { "" })
                }
                ["env", "GIT_TERMINAL_PROMPT=0", "git", "ls-remote", "--symref", _, "HEAD"] => {
                    out(0, "ref: refs/heads/Plasma/6.7\tHEAD\n6ed0921\tHEAD\n", "")
                }
                ["curl", "-fsSL", "-m", _, _] => out(0, fork_cmake, ""),
                _ => healthy(argv),
            }
        }));
        io.bins.extend(["git", "curl"].map(String::from));
        if checkupdates.is_some() {
            io.bins.insert("checkupdates".to_string());
        }
        io
    }

    const PENDING_68: &str = "kwin 6.7.5-1.1 -> 6.8.0-1\nplasma-workspace 6.7.5-1.1 -> 6.8.0-1\n";

    /// Новая серия в репозиториях, авторы ещё на 6.7 — «не обновляйтесь»;
    /// ничего не ставится и пароль не спрашивается.
    #[test]
    fn pending_plasma_series_with_lagging_fork_says_hold() {
        let io = before_update_io(Some((0, PENDING_68)), "set(PROJECT_VERSION \"6.7.5\")\n");
        let (report, advice) = lonely_advice(&io);
        assert!(!report.needs_rebuild && !report.lonely_fork, "{report:?}");
        assert_eq!(advice, aeroshell::Advice::HoldUpdate { plasma: "6.8.0".into(), fork: "6.7.5".into() });
        let runs = io.runs();
        assert!(runs.iter().any(|r| r == "checkupdates"), "{runs:?}");
        assert!(!runs.iter().any(|r| r.contains("pkexec") || r.contains("sudo") || r.contains("-Sy")), "{runs:?}");
    }

    /// Авторы уже на 6.8 либо их версию узнать не удалось — мягко:
    /// пересборка после обновления понадобится.
    #[test]
    fn pending_plasma_series_caught_up_or_unknown_is_soft() {
        let soft = aeroshell::Advice::RebuildAfterUpdate { from: "6.7.5".into(), to: "6.8.0".into() };
        let io = before_update_io(Some((0, PENDING_68)), "set(PROJECT_VERSION \"6.8.0\")\n");
        assert_eq!(lonely_advice(&io).1, soft);
        // Сырой файл ветки не разобрался (чужой ответ) — версия неизвестна.
        let io = before_update_io(Some((0, PENDING_68)), "<html>not found</html>");
        assert_eq!(lonely_advice(&io).1, soft);
    }

    /// Точечный выпуск — мягко и без похода к авторам в сеть.
    #[test]
    fn pending_point_release_is_soft_without_network() {
        let io = before_update_io(
            Some((0, "kwin 6.7.5-1.1 -> 6.7.6-1\nplasma-workspace 6.7.5-1.1 -> 6.7.6-1\n")),
            "set(PROJECT_VERSION \"6.7.5\")\n",
        );
        assert_eq!(
            lonely_advice(&io).1,
            aeroshell::Advice::RebuildAfterUpdate { from: "6.7.5".into(), to: "6.7.6".into() }
        );
        assert!(!io.runs().iter().any(|r| r.contains("git ls-remote") || r.starts_with("curl")));
    }

    /// Нет `checkupdates`, он не смог (код 1) или обновлений нет (код 2) —
    /// молчим, системную базу не трогаем.
    #[test]
    fn no_checkupdates_or_failure_is_quiet() {
        for answer in [None, Some((1, "")), Some((2, ""))] {
            let io = before_update_io(answer, "set(PROJECT_VERSION \"6.7.5\")\n");
            assert_eq!(lonely_advice(&io).1, aeroshell::Advice::Nothing, "{answer:?}");
            assert!(!io.runs().iter().any(|r| r.contains("git ls-remote")), "{answer:?}");
        }
    }

    /// Слежка: ставится один раз (файлы + daemon-reload + enable), повтор
    /// ничего не делает, снятие — disable, файлы, daemon-reload.
    #[test]
    fn update_watch_installs_once_and_removes() {
        use crate::update_guard as ug;
        let mut io = FakeIo::arch(Box::new(healthy));
        io.bins.insert("systemctl".to_string());
        let ex = DeOpsExec::new(&io, Arc::new(Recorder::default()), CancelToken::new());
        let home_cfg = ex.config_home();
        let dir = ug::user_unit_dir(&home_cfg);
        let exec = "/usr/lib/rexon/rexon-main-window --update-check";
        let program = Some("/usr/lib/rexon/rexon-main-window");
        ex.update_watch(true, Some(exec), program).unwrap();
        let path_unit = io.files.borrow().get(&format!("{dir}/{}", ug::WATCH_PATH_UNIT)).cloned().unwrap();
        assert!(path_unit.contains("PathChanged=/var/lib/pacman/local"), "{path_unit}");
        let service = io.files.borrow().get(&format!("{dir}/{}", ug::WATCH_SERVICE_UNIT)).cloned().unwrap();
        assert!(service.contains("ExecStart=/usr/lib/rexon/rexon-main-window --update-check"), "{service}");
        let sc = |io: &FakeIo| io.runs().into_iter().filter(|r| r.starts_with("systemctl")).collect::<Vec<_>>();
        assert_eq!(
            sc(&io),
            vec![
                "systemctl --user daemon-reload".to_string(),
                "systemctl --user enable --now rexon-update-check.path rexon-update-check.timer".to_string(),
            ]
        );
        // Повтор с тем же — ни записи, ни systemctl.
        ex.update_watch(true, Some(exec), program).unwrap();
        assert_eq!(sc(&io).len(), 2);
        // Снятие.
        ex.update_watch(false, None, None).unwrap();
        let runs = sc(&io);
        assert_eq!(runs[2], "systemctl --user disable --now rexon-update-check.path rexon-update-check.timer");
        assert_eq!(runs[3], "systemctl --user daemon-reload");
        assert!(!io.files.borrow().contains_key(&format!("{dir}/{}", ug::WATCH_PATH_UNIT)));
        // Снимать нечего — ничего не зовётся.
        ex.update_watch(false, None, None).unwrap();
        assert_eq!(sc(&io).len(), 4);
        // Прежние юниты AeroThemePlasma снимаются, общие ставятся.
        io.files.borrow_mut().insert(format!("{dir}/rexon-aeroshell-check.timer"), String::new());
        ex.update_watch(true, Some(exec), program).unwrap();
        let runs = sc(&io);
        assert_eq!(runs[4], "systemctl --user disable --now rexon-aeroshell-check.timer");
        assert_eq!(runs[5], "systemctl --user daemon-reload");
        assert!(!io.files.borrow().contains_key(&format!("{dir}/rexon-aeroshell-check.timer")));
    }

    /// Всё свежее: сверка только спрашивает pacman, окна нет, сборки нет.
    #[test]
    fn aeroshell_up_to_date_asks_nothing() {
        let io = aero_io("6.7.4", Box::new(built));
        let report = aero_survey(&io);
        assert!(!report.needs_rebuild, "{report:?}");
        assert!(report.stale.is_empty());
        assert_eq!(report.packages.len(), 3, "{report:?}");
        assert_eq!(report.kwin_version.as_deref(), Some("6.7.4"));
        let runs = io.runs();
        assert!(runs.iter().all(|r| r.starts_with("pacman ")), "{runs:#?}");
    }

    /// Не Arch: пакетов оформления там не бывает, pacman не спрашивается.
    #[test]
    fn aeroshell_survey_off_arch_is_empty() {
        let io = aero_io("6.7.5", Box::new(built));
        io.files.borrow_mut().insert(
            "/etc/os-release".to_string(),
            "ID=fedora\nNAME=Fedora\n".to_string(),
        );
        let report = aero_survey(&io);
        assert_eq!(report, aeroshell::Report::empty());
        assert!(io.runs().is_empty(), "{:#?}", io.runs());
    }

    /// Устарело → план → «Да» → успех: пересобирается весь собранный набор
    /// одной командой, ход сборки идёт в окно, временный журнал убран.
    #[test]
    fn aeroshell_stale_plan_then_success() {
        // Тексты — из каталога перевода; язык исходный (русский сверяют
        // сверки ядра на po/ru.po).
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let mut io = aero_io("6.7.5", Box::new(built));
        io.bins.insert("yay".to_string());
        let report = aero_survey(&io);
        assert!(report.needs_rebuild);
        assert_eq!(report.stale, vec![KWIN_COMPONENTS.to_string()]);
        assert_eq!(report.rebuild, vec![KWIN_COMPONENTS.to_string(), LIBPLASMA.to_string()]);
        let details = aeroshell::report_details(&report);
        assert!(details.contains("built for 6.7.4, installed 6.7.5 — outdated"), "{details}");
        assert!(builder_runs(&io).is_empty(), "сборка до «Да»");

        let (done, rec) = aero_rebuild(&io, &report.rebuild, CancelToken::new());
        assert!(done.ok, "{done:?}");
        assert_eq!(done.message, aeroshell::rebuild_success_message());
        assert_eq!(
            builder_runs(&io),
            vec![format!(
                "env LANG=C LC_ALL=C yay -S --rebuild --diffmenu=false --noconfirm --sudo pkexec \
                 --sudoflags  {KWIN_COMPONENTS} {LIBPLASMA}"
            )]
        );
        assert_eq!(rec.messages("install"), vec![aeroshell::rebuild_start_message(2)]);
        assert_eq!(
            rec.messages(aeroshell::REBUILD_LINE_STAGE),
            vec!["==> Making package: aeroshell-libplasma-git", "==> Finished making"]
        );
        assert_eq!(rec.messages("done"), vec![aeroshell::rebuild_done_message()]);
        assert_eq!(rec.count(de_progress::STAGE_ERROR), 0);
        let flags = rec.cancellable.lock().unwrap().clone();
        assert_eq!(flags.first(), Some(&true));
        assert_eq!(flags.last(), Some(&false));
        let run = io.index_of("run: env LANG=C").unwrap();
        assert!(io.index_of("remove: /tmp/rexon_aeroshell_").unwrap() > run);
    }

    /// paru и devtools есть — сборка в чистом окружении.
    #[test]
    fn aeroshell_rebuild_prefers_paru_chroot() {
        let mut io = aero_io("6.7.5", Box::new(built));
        io.bins.extend(["yay", "paru", "mkarchroot"].map(String::from));
        let (done, _) = aero_rebuild(&io, &[LIBPLASMA.to_string()], CancelToken::new());
        assert!(done.ok, "{done:?}");
        let runs = builder_runs(&io);
        assert_eq!(runs.len(), 1);
        assert!(runs[0].starts_with("env LANG=C LC_ALL=C paru -S --rebuild --chroot --skipreview"), "{runs:?}");
    }

    /// Нет ни paru, ни yay: отказ до первого побочного действия — ни замка,
    /// ни сборщика, одно объяснение.
    #[test]
    fn aeroshell_no_builder_refuses_before_anything() {
        let io = aero_io("6.7.5", Box::new(built));
        let (done, rec) = aero_rebuild(&io, &[LIBPLASMA.to_string()], CancelToken::new());
        assert!(!done.ok && !done.cancelled, "{done:?}");
        assert_eq!(done.message, aeroshell::rebuild_no_helper());
        assert!(io.runs().is_empty(), "{:#?}", io.runs());
        assert_eq!(rec.count(de_progress::STAGE_ERROR), 1);
        assert_eq!(rec.count("install"), 0);
    }

    /// Нечем спросить пароль — объяснение ядра с именем пакета.
    #[test]
    fn aeroshell_without_polkit_refuses() {
        let mut io = aero_io("6.7.5", Box::new(built));
        io.bins.insert("yay".to_string());
        io.bins.remove("pkexec");
        let (done, _) = aero_rebuild(&io, &[LIBPLASMA.to_string()], CancelToken::new());
        assert!(!done.ok);
        assert_eq!(done.message, SystemExec::new(&io).polkit_missing_message());
        assert!(builder_runs(&io).is_empty());
    }

    /// Отказ пароля: одно объяснение словами вместо хвоста сборщика.
    #[test]
    fn aeroshell_password_refused_says_so_once() {
        let mut io = aero_io(
            "6.7.5",
            Box::new(|_: &[String]| {
                out(
                    1,
                    "==> Finished making: aeroshell-libplasma-git\n",
                    "Error executing command as another user: Request dismissed\n \
                     -> error installing repo packages: exit status 126\n",
                )
            }),
        );
        io.bins.insert("yay".to_string());
        let (done, rec) = aero_rebuild(&io, &[LIBPLASMA.to_string()], CancelToken::new());
        assert!(!done.ok && !done.cancelled, "{done:?}");
        assert_eq!(done.message, aeroshell::rebuild_denied_message());
        assert_eq!(builder_runs(&io).len(), 1);
        assert_eq!(rec.count(de_progress::STAGE_ERROR), 1, "{:#?}", rec.stages());
    }

    /// Сбой сборки: сначала «система цела», потом хвост вывода; без вывода —
    /// хотя бы код возврата.
    #[test]
    fn aeroshell_build_failure_keeps_tail() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let mut io = aero_io(
            "6.7.5",
            Box::new(|_: &[String]| out(1, "==> ERROR: A failure occurred in build().\n    Aborting...\n", "")),
        );
        io.bins.insert("yay".to_string());
        let (done, rec) = aero_rebuild(&io, &[LIBPLASMA.to_string()], CancelToken::new());
        assert!(!done.ok);
        assert!(done.message.starts_with(&aeroshell::rebuild_failed_head()), "{}", done.message);
        // Главное в неудаче — что система цела; текст — тот, что берёт aeroshell.
        let intact = crate::de_progress::aeroshell_fail_note();
        assert!(done.message.contains(&intact), "{}", done.message);
        assert!(done.message.ends_with("==> ERROR: A failure occurred in build().\n    Aborting..."), "{}", done.message);
        assert_eq!(rec.count(de_progress::STAGE_ERROR), 1);

        let mut io = aero_io("6.7.5", Box::new(|_: &[String]| out(2, "", "")));
        io.bins.insert("yay".to_string());
        let (done, _) = aero_rebuild(&io, &[LIBPLASMA.to_string()], CancelToken::new());
        assert!(done.message.ends_with(&aeroshell::rebuild_exit_code_message(2)), "{}", done.message);

        let mut io = aero_io("6.7.5", Box::new(|_: &[String]| RunOut::missing()));
        io.bins.insert("yay".to_string());
        let (done, _) = aero_rebuild(&io, &[LIBPLASMA.to_string()], CancelToken::new());
        assert!(done.message.starts_with("Couldn't start yay"), "{}", done.message);
    }

    /// Отмена до сборки — ничего не запущено; и во время ожидания чужого
    /// замка — тоже.
    #[test]
    fn aeroshell_cancel_before_build_changes_nothing() {
        let mut io = aero_io("6.7.5", Box::new(built));
        io.bins.insert("yay".to_string());
        let token = CancelToken::new();
        token.cancel();
        let (done, _) = aero_rebuild(&io, &[LIBPLASMA.to_string()], token);
        assert!(done.cancelled, "{done:?}");
        assert_eq!(done.message, cancelled_message());
        assert!(builder_runs(&io).is_empty());

        let mut io = aero_io("6.7.5", Box::new(built));
        io.bins.insert("yay".to_string());
        io.files
            .borrow_mut()
            .insert("/var/lib/pacman/db.lck".to_string(), String::new());
        let token = CancelToken::new();
        io.cancel_on_sleep = Some(token.clone());
        let (done, rec) = aero_rebuild(&io, &[LIBPLASMA.to_string()], token);
        assert!(done.cancelled, "{done:?}");
        assert!(builder_runs(&io).is_empty());
        assert_eq!(rec.count(plan::PKG_LOCK_WAIT_STAGE), 1);
    }

    /// Замок так и не освободился: сборщик не запускается, итог — текст ядра.
    #[test]
    fn aeroshell_lock_never_freed_does_not_build() {
        let mut io = aero_io("6.7.5", Box::new(built));
        io.bins.insert("yay".to_string());
        io.files
            .borrow_mut()
            .insert("/var/lib/pacman/db.lck".to_string(), String::new());
        let (done, _) = aero_rebuild(&io, &[LIBPLASMA.to_string()], CancelToken::new());
        assert!(!done.ok && !done.cancelled);
        assert_eq!(done.message, plan::pkg_lock_timeout_message(plan::PKG_LOCK_TIMEOUT_SECS));
        assert!(builder_runs(&io).is_empty());
    }

    // ── Первое снятие эталона защиты приложений ──────────────────────────
    //
    // Раздел «Защита приложений по умолчанию» включён по умолчанию, но до
    // первого эталона сверять подмену не с чем. Проверяются обе ветки
    // решения и то, что второй запуск эталон НЕ переписывает.

    /// Подставной профиль без эталона: `xdg-mime` отвечает на любой запрос,
    /// терминал берётся из `xdg-terminals.list`.
    fn protector_io() -> FakeIo {
        let mut io = FakeIo::arch(Box::new(|argv: &[String]| {
            if argv.first().map(String::as_str) == Some("xdg-mime") {
                return out(0, "some-app.desktop\n", "");
            }
            out(1, "", "")
        }));
        io.files.borrow_mut().insert(
            "/home/t/.config/xdg-terminals.list".to_string(),
            "konsole.desktop\n".to_string(),
        );
        io.env.insert("XDG_CURRENT_DESKTOP".to_string(), "KDE".to_string());
        io
    }

    const SNAPSHOT_FILE: &str = "/home/t/.config/rexon/protected_defaults.json";

    fn snapshot_if_never(io: &FakeIo) -> Option<Result<usize, String>> {
        DeOpsExec::new(io, Arc::new(Recorder::default()), CancelToken::new())
            .defaults_snapshot_if_never()
    }

    /// Решение — чистая функция ядра: обе ветки.
    #[test]
    fn startup_snapshot_decision_both_ways() {
        let rules = dfl::Rules::defaults();
        // Ничего не запомнено — снимать.
        assert!(dfl::should_snapshot_at_startup(&rules, &[]));
        // Эталон уже есть — не трогать.
        let remembered = vec![("file_manager".to_string(), Some("dolphin.desktop".to_string()))];
        assert!(!dfl::should_snapshot_at_startup(&rules, &remembered));
        // Защита выключена — не снимать, даже если не запомнено ничего.
        let off = rules.updated(&serde_json::from_str(r#"{"enabled": false}"#).unwrap());
        assert!(!dfl::should_snapshot_at_startup(&off, &[]));
    }

    /// Чистый профиль: эталон снимается сам и получается непустым.
    #[test]
    fn startup_snapshot_taken_on_clean_profile() {
        let io = protector_io();
        let count = snapshot_if_never(&io).expect("на чистом профиле снимаем").unwrap();
        assert!(count > 0, "эталон пуст: {count}");
        let text = io.files.borrow().get(SNAPSHOT_FILE).cloned().expect("файл эталона");
        let parsed: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(dfl::load_protected_apps(&parsed).len(), count);
        // Терминал спрашивается тоже, и он из списка xdg.
        assert_eq!(
            dfl::load_protected_apps(&parsed)
                .iter()
                .find(|(k, _)| k == "terminal")
                .and_then(|(_, v)| v.clone()),
            Some("konsole.desktop".to_string())
        );
    }

    /// Второй запуск на том же профиле: эталон НЕ переписывается и система
    /// ни о чём не спрашивается.
    #[test]
    fn startup_snapshot_not_retaken() {
        let io = protector_io();
        snapshot_if_never(&io).expect("первый запуск снимает").unwrap();
        let first = io.files.borrow().get(SNAPSHOT_FILE).cloned().unwrap();
        io.calls.borrow_mut().clear();
        // Часы подставного исполнителя идут — при пересъёмке `last_updated`
        // стал бы другим, и совпадение содержимого это поймает.
        io.clock.set(io.clock.get() + 3600.0);

        assert!(snapshot_if_never(&io).is_none(), "второй раз снимать нечего");
        assert_eq!(io.files.borrow().get(SNAPSHOT_FILE).cloned().unwrap(), first);
        assert!(io.runs().is_empty(), "лишние запросы к системе: {:#?}", io.runs());
        assert!(
            !io.calls.borrow().iter().any(|c| c.starts_with("write:")),
            "эталон переписан: {:#?}",
            io.calls.borrow()
        );
    }

    /// Эталон стёрли руками — чинится само на следующем запуске.
    #[test]
    fn startup_snapshot_restored_after_deletion() {
        let io = protector_io();
        snapshot_if_never(&io).expect("первый запуск снимает").unwrap();
        io.files.borrow_mut().remove(SNAPSHOT_FILE);
        let count = snapshot_if_never(&io).expect("эталона нет — снимаем снова").unwrap();
        assert!(count > 0);
        assert!(io.files.borrow().contains_key(SNAPSHOT_FILE));
    }

    /// Терминал по умолчанию: после записи `rexon-terminal.conf` переменные
    /// сеанса сразу доводятся до композиторов без environment.d.
    #[test]
    fn terminal_default_syncs_session_env_after_write() {
        let io = protector_io();
        DeOpsExec::new(&io, Arc::new(Recorder::default()), CancelToken::new())
            .set_default("terminal", "kitty.desktop");
        let conf = io
            .index_of("environment.d/rexon-terminal.conf")
            .expect("rexon-terminal.conf записан");
        let sync = io.index_of("sync_session_env").expect("переменные сеанса не синхронизированы");
        assert!(conf < sync, "синхронизация раньше записи: {:#?}", io.calls.borrow());

        // Обычное приложение по умолчанию переменных сеанса не касается.
        let io = protector_io();
        DeOpsExec::new(&io, Arc::new(Recorder::default()), CancelToken::new())
            .set_default("browser", "firefox.desktop");
        assert!(io.index_of("sync_session_env").is_none(), "{:#?}", io.calls.borrow());
    }

    /// Защита выключена — профиль не трогается вовсе.
    #[test]
    fn startup_snapshot_skipped_when_protection_off() {
        let io = protector_io();
        io.files.borrow_mut().insert(
            "/home/t/.config/rexon/protection_rules.json".to_string(),
            r#"{"enabled": false}"#.to_string(),
        );
        assert!(snapshot_if_never(&io).is_none());
        assert!(!io.files.borrow().contains_key(SNAPSHOT_FILE));
        assert!(io.runs().is_empty(), "{:#?}", io.runs());
    }
}
