//! Исполняющий слой `rexon/system.py` как ЧИСТЫЕ «планы команд» —
//! продолжение порта system.py (первая часть — `system.rs`).
//!
//! Здесь НИЧЕГО не выполняется: каждая функция по входным данным строит
//! последовательность шагов (`PlanStep`) — команда, аргументы, нужен-ли-root,
//! таймаут, окружение (LANG=C), описание. Исполнитель (пока живой Python
//! через мост, позже rexon-gui) просто прогоняет план. Эталон паритета —
//! ЗАПИСАННЫЕ вызовы safe_run/subprocess.run живого Python (argv/timeout/env)
//! на тех же входах: план обязан совпасть 1-в-1 (см. `system_plan_parity.rs`).
//!
//! Портировано:
//!   * таблицы команд per `PkgFamily` (11 семейств): `install_cmd` /
//!     `remove_cmd` / `query_cmd` / `update_db_cmd` / `pkg_manager_name`
//!     (+ тексты RuntimeError для UNKNOWN — как в Python);
//!   * `LOCK_PATHS` + шаг-примитив `WaitPkgLock` (poll 2 с, таймаут 300 с —
//!     snap-pac post-hooks держат лок после соседней транзакции);
//!   * `install_argv` — ЕДИНСТВЕННЫЙ ответ на «какой командой поставить эти
//!     пакеты»: таблица флагов плюс выбор «именованный путь org.rexon.pkg или
//!     сырой pkexec» (и NixOS без root). Спрашивают его и `install_plan`, и
//!     доустановка видеозависимостей (`wallpaper_engine`), и починка
//!     соответствия (`compliance`) — обе последние держали СВОИ таблицы и
//!     обе успели с движком разойтись;
//!   * планы `update_db_plan` / `install_plan` / `remove_plan` (полный
//!     пайплайн remove: раскрытие групп → query установленности → forbidden
//!     set → транзитивное замыкание → команда удаления) /
//!     `install_batch_plan` (legacy `pkexec bash <tmp_sh>` со скриптом и
//!     обёрткой с chown-граблей fs.protected_regular, helper-путь
//!     rexon-de-helper, SteamOS steamos-readonly, NixOS/immutable ранние
//!     выходы, Gentoo overlay, AUR-хвост без root);
//!   * оркестрация DE: `dm_enable_step` (пары пакет/юнит, `--force` для
//!     Plasma Login Manager), `remove_de_decision` (2-уровневая защита от
//!     удаления активного DE, force-флаг — правило архитектуры #13),
//!     `switch_de_guard`, `logout_cmds` (подбор команд логаута по окружению);
//!   * polkit-слой: генерация ТЕКСТОВ привилегированных helper-скриптов
//!     (rexon-de-helper / rexon-pkg-helper / rexon-stage-helper), policy XML,
//!     passwordless-правила org.rexon.*, `privileged_pkg_cmd` /
//!     `privileged_stage_cmd` (named-действия vs сырой pkexec) и
//!     `de_helper_supports`. Тексты сверены с Python текст-в-текст.
//!   * версия пакета в репозиториях: `clean_pkg_version`, `valid_pkg_name`,
//!     `repo_version_query` (команда + бинарь + LANG=C + таймаут 10 с),
//!     `parse_repo_version` (поля Version/Candidate по семейству);
//!   * установленный пакет — ещё два САМОСТОЯТЕЛЬНЫХ вопроса к движку (не
//!     «установлен ли» и не «что в репозиториях»): `installed_version_query`
//!     + `parse_installed_version` (версия того, что стоит на машине) и
//!     `package_files_query` + `package_files_output` + `package_file_paths`
//!     (список файлов пакета и его разбор — единственный на весь проект),
//!     `installed_packages_query` + `parse_installed_packages` (перечислить
//!     ВСЁ установленное — вопрос без аргумента-пакета),
//!     `installed_meta_query` (`pacman -Qi`, сырьё разбора полей) и
//!     `repo_target_exists` (есть ли цель в репозиториях — по исходу того же
//!     запроса, что и версия);
//!   * ожидание замка пакетного менеджера: `lock_wait_step` — чистый шаг
//!     цикла (свободен / сказать человеку / подождать / не дождались),
//!     общий для установки пакетов и для пересборки оформления;
//!   * откат тяжёлых форков Plasma: `filter_plasma_forks`,
//!     `fork_stock_candidates` + `fork_stock_target` (сток из живых
//!     provides/replaces, проверенный на существование в репозиториях),
//!     `plasma_fork_split`, `plasma_fork_recovery_command`,
//!     `plasma_fork_unresolved_message`, `plasma_fork_failure_message`,
//!     `plasma_fork_reset_plan` (одна pkexec-транзакция `-Rns` + `-S`,
//!     либо отказ `Blocked`, если возвращать нечего), `plasma_fork_preview`
//!     (что будет сделано, без транзакции), `plasma_fork_reset_outcome`
//!     (разбор исхода pkexec) и `TargetExistsMemo` (память пробы);
//!   * Plymouth: `plymouth_theme_from_link`, `plymouth_theme_from_output`,
//!     `plymouth_theme_candidates` + `plymouth_installed_themes` (что считать
//!     установленной темой), `plymouth_theme_file`, `plymouth_package_default`,
//!     `target_plymouth_theme` (брендинг дистро → дефолт пакета → запасные),
//!     `plymouth_reset_command`, `plymouth_reset_plan` (выбор
//!     plymouth-set-default-theme vs update-alternatives),
//!     `plymouth_reset_decision` + `plymouth_reset_outcome` и тексты ответов;
//!   * SDDM: `sddm_conf_read_order`, `sddm_conf_entries`,
//!     `sddm_kde_settings_path`, `sddm_files_to_remove`,
//!     `sddm_current_theme_from`,
//!     `sddm_distro_default_theme`, `sddm_reset_plan`, `sddm_reset_command`,
//!     `sddm_reset_outcome`,
//!     тексты сообщений (`sddm_reset_success_message`,
//!     `pkexec_denied_message`);
//!   * детект экрана входа: таблицы юнитов (`KNOWN_DM_UNITS` —
//!     16 DM, `LOGIN_MANAGER_PROBE_UNITS`, `DM_ENABLED_STATES`),
//!     `dm_unit_from_symlink`, `active_login_manager_from`,
//!     `display_manager_present_from` (сами `systemctl is-enabled` — колбэком).
//!   * живой ход установки: `parse_pacman_line` — строка вывода pacman в
//!     стадию и готовое русское сообщение (шаги, хуки, скачивание, итоги,
//!     разделы, ошибки, «уже установлено» у `--needed`).
//!   * итог установки и её ход: `batch_main_result` (объяснения про отказ
//!     pkexec и отсутствующий polkit-агент), `batch_aur_intro` /
//!     `batch_aur_result`, `install_de_fraction` (доля полосы по стадии),
//!     `pick_aur_helper`, `pkg_lock_timeout_message`, `gentoo_overlay_for`,
//!     `extra_packages_texts`;
//!   * идиот-защита экрана входа: `SDDM_PKG_FOR_FAMILY` + `ensure_dm_plan`
//!     (что ставить, чем ставить, чем включать и что сказать);
//!   * пост-установка MATE: подписи «Общих задач» по языку, карта snap-путей,
//!     gschema-оверрайд и оба привилегированных скрипта, списки пакетов
//!     сверх дистрибутива, тексты итога.
//!
//! НЕ портировано (граница — исполнение при подключении к живому Python):
//!   реальный запуск команд/pkexec и чтение их вывода, tail-поток лог-файла
//!   установки (сам разбор строк — `parse_pacman_line`), проба
//!   polkit-агента (вызов `gdbus` к polkit), походы наружу пост-установки MATE (git clone,
//!   копирование файлов, gsettings), `ensure_gentoo_overlay` (сам шаг в
//!   плане есть — `EnsureGentooOverlay`), `active_login_manager`, наличие
//!   привилегированных хелперов (подаётся параметром), оркестрация
//!   install_de/switch_de вокруг Python-модулей (PlasmaProtector,
//!   SystemDefaultsProtector, DEAppSwitcher, RexonBridge, BackupManager).

use std::collections::HashMap;

use serde::Serialize;

use crate::ext_manager::shlex_quote;
use crate::i18n::{fill, n_, ntr, tr, trc, trf};
use crate::system::{
    de_entry_packages_for, expand_groups, qi_field, DistroDetector, PkgFamily,
};

// ============================================================================
// Шаги плана
// ============================================================================

/// Команда исполнителю — то, что Python передаёт в `safe_run`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Cmd {
    pub argv: Vec<String>,
    /// None → таймаут ПО КЛАССУ команды (`utils::default_timeout_secs`):
    /// опрос 30 с, установка из репозиториев 30 мин, сборка из исходников
    /// 4 ч. Своё число ставим только там, где по argv класс не виден —
    /// прежде всего за `pkexec bash <скрипт>`.
    pub timeout_secs: Option<u64>,
    /// Добавить LANG=C, LC_ALL=C поверх окружения (парсящие шаги — русская
    /// локаль ломает разбор вывода pacman).
    pub lang_c: bool,
}

impl Cmd {
    fn new(argv: Vec<String>, timeout_secs: Option<u64>, lang_c: bool) -> Self {
        Cmd { argv, timeout_secs, lang_c }
    }
}

/// Машинный признак шага [`PlanStep::Run`] — по нему исполнитель узнаёт,
/// ЧТО это за запуск, и не заглядывает в подпись для человека.
///
/// ⚠️ ЗАЧЕМ ОТДЕЛЬНОЕ ПОЛЕ. Исполнитель разбирал шаги сравнением с русской
/// строкой `description` («Установка пакетов (один pkexec)» и соседние).
/// Из-за этого переименование подписи — правка чисто косметическая, ровно
/// та, которую поощряет правило про человеческий язык, — МОЛЧА ломало
/// установку рабочего стола: главный привилегированный шаг терял живой
/// прогресс из лог-файла и проверку результата, а план при этом оставался
/// верным и сверки ничего не замечали. Подпись теперь только для показа
/// человеку и переименовывается свободно; ветвление идёт по `id`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum RunStepId {
    /// Обновление базы пакетов (`update_db`, и тот же шаг внутри nix-ветки).
    #[serde(rename = "pkg.update_db")]
    PkgUpdateDb,
    /// Одиночная установка пакетов (`install`, без пакетной обвязки).
    #[serde(rename = "pkg.install")]
    PkgInstall,
    /// ГЛАВНЫЙ привилегированный шаг пакетной установки: `pkexec` поверх
    /// скрипта-обёртки (обычный путь и SteamOS) либо rexon-de-helper.
    /// Исполнитель показывает по нему живой ход из лог-файла и проверяет
    /// результат.
    #[serde(rename = "pkg.main")]
    PkgMain,
    /// Главный шаг пакетной установки на Nix (без pkexec и без лог-файла).
    #[serde(rename = "pkg.nix")]
    PkgNix,
    /// Хвост установки из AUR через помощника (yay/paru).
    #[serde(rename = "pkg.aur")]
    PkgAur,
    /// Удаление пакетов.
    #[serde(rename = "pkg.remove")]
    PkgRemove,
    /// Проба «что входит в группу» (раскрытие групп при удалении).
    #[serde(rename = "pkg.query_group")]
    PkgQueryGroup,
    /// Проба «пакет установлен?».
    #[serde(rename = "pkg.query")]
    PkgQuery,
    /// Проба «кому нужен пакет» при сборе запретного множества.
    #[serde(rename = "pkg.required_by")]
    PkgRequiredBy,
    /// Проба «кому нужен пакет» на проходе транзитивного замыкания.
    #[serde(rename = "pkg.closure")]
    PkgClosure,
    /// Повторная проба «кому нужен пакет» ради пояснения к пропущенному.
    #[serde(rename = "pkg.skip_note")]
    PkgSkipNote,
    /// Установка самого Rexon в систему (один пароль).
    #[serde(rename = "app.install")]
    AppInstall,
    /// Удаление самого Rexon из системы.
    #[serde(rename = "app.uninstall")]
    AppUninstall,
}

impl RunStepId {
    /// Строковый вид признака — тот же, что уходит в план для исполнителя.
    pub fn as_str(self) -> &'static str {
        match self {
            RunStepId::PkgUpdateDb => "pkg.update_db",
            RunStepId::PkgInstall => "pkg.install",
            RunStepId::PkgMain => "pkg.main",
            RunStepId::PkgNix => "pkg.nix",
            RunStepId::PkgAur => "pkg.aur",
            RunStepId::PkgRemove => "pkg.remove",
            RunStepId::PkgQueryGroup => "pkg.query_group",
            RunStepId::PkgQuery => "pkg.query",
            RunStepId::PkgRequiredBy => "pkg.required_by",
            RunStepId::PkgClosure => "pkg.closure",
            RunStepId::PkgSkipNote => "pkg.skip_note",
            RunStepId::AppInstall => "app.install",
            RunStepId::AppUninstall => "app.uninstall",
        }
    }
}

/// Все признаки шагов `Run` — исполнителю, чтобы сверять план со своим
/// разбором и не молчать о незнакомом шаге.
pub const RUN_STEP_IDS: &[RunStepId] = &[
    RunStepId::PkgUpdateDb,
    RunStepId::PkgInstall,
    RunStepId::PkgMain,
    RunStepId::PkgNix,
    RunStepId::PkgAur,
    RunStepId::PkgRemove,
    RunStepId::PkgQueryGroup,
    RunStepId::PkgQuery,
    RunStepId::PkgRequiredBy,
    RunStepId::PkgClosure,
    RunStepId::PkgSkipNote,
    RunStepId::AppInstall,
    RunStepId::AppUninstall,
];

/// Шаг плана. Исполнение — при подключении к живому Python.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "step")]
pub enum PlanStep {
    /// Запустить команду (safe_run). `needs_root` = argv начинается с pkexec.
    /// `id` — машинный признак для исполнителя, `description` — подпись
    /// человеку (менять свободно, ветвиться по ней НЕЛЬЗЯ).
    Run { id: RunStepId, cmd: Cmd, needs_root: bool, description: String },
    /// Подождать освобождения lock-файла пакетного менеджера (poll 2 с;
    /// пустой `paths` — для family без лока — означает «свободен сразу»).
    WaitPkgLock { paths: Vec<String>, timeout_secs: u64, poll_secs: u64 },
    /// Записать файл (сгенерированный скрипт/лог) с правами `mode`.
    WriteFile { path: String, content: String, mode: u32 },
    /// Удалить временные файлы (ошибки игнорируются).
    RemoveFiles { paths: Vec<String> },
    /// Подключить Gentoo overlay (`ensure_gentoo_overlay` — исполнение
    /// при подключении к живому Python).
    EnsureGentooOverlay { overlay: String },
}

/// Отсечка стандартного ввода для КАЖДОГО скрипта, под которым работает
/// пакетный менеджер (обёртка legacy-пути, обёртка SteamOS, rexon-de-helper).
///
/// ⚠️ Заведено после опыта: `pacman -S <группа>` без `--noconfirm` и с живым,
/// но молчащим stdin висит на «Enter a selection (default=all):» вечно —
/// проверено в песочнице (`fakeroot pacman -r …`, stdin = незакрытая труба:
/// вопрос напечатан, дальше тишина до убийства по таймауту). С `--noconfirm`
/// pacman отвечает себе сам и не читает ввод — но полагаться на то, что
/// флаг никогда не потеряется в одиннадцати таблицах команд, нельзя. Из
/// интерфейса Rexon терминала нет, повиснуть там некому и заметить некому:
/// человек полчаса смотрит на замерший ход установки.
const STDIN_GUARD: &str = "exec </dev/null\n";

fn run_step(id: RunStepId, argv: Vec<String>, timeout: Option<u64>,
            lang_c: bool, description: &str) -> PlanStep {
    let needs_root = argv.first().map(|a| a == "pkexec").unwrap_or(false);
    PlanStep::Run {
        id,
        cmd: Cmd::new(argv, timeout, lang_c),
        needs_root,
        description: description.to_string(),
    }
}

// ============================================================================
// Таблицы команд per family — порт _install_cmd/_remove_cmd/_query_cmd/
// _update_db_cmd/get_pkg_manager_name
// ============================================================================

fn unknown_family_error(verb: &str, tail: &str) -> String {
    // Текст RuntimeError в Python: f"Пакетный менеджер не определён
    // ({self.family}). {Verb} … вручную: {…}". Достижимо только для UNKNOWN
    // (остальные семейства есть в таблицах); f-string от (str, Enum) даёт
    // "PkgFamily.UNKNOWN" — сверено golden-эталоном.
    trf(
        // TRANSLATORS: %1 is an instruction like “Install the packages
        // manually:”, %2 the package names. PkgFamily.UNKNOWN stays as is.
        "The package manager wasn't detected (PkgFamily.UNKNOWN). %1 %2",
        &[&verb, &tail],
    )
}

/// Проверка имён перед сборкой ЛЮБОЙ команды пакетного менеджера.
///
/// ⛔ Стоит в общей таблице команд, а не только перед привилегированным
/// помощником: сырой откат того же плана (`pkexec apt-get install …`)
/// поставил бы подсунутый ЛОКАЛЬНЫЙ ФАЙЛ от root ровно так же, как это
/// делал помощник до починки 25.09.2026. Правило — [`is_safe_pkg_arg`].
fn check_pkg_names(family: PkgFamily, packages: &[String]) -> Result<(), String> {
    match packages.iter().find(|p| !is_safe_pkg_arg(family, p)) {
        Some(bad) => Err(bad_pkg_name_error(bad)),
        None => Ok(()),
    }
}

/// Команда установки пакетов — порт `PackageManager._install_cmd`.
pub fn install_cmd(family: PkgFamily, packages: &[String]) -> Result<Vec<String>, String> {
    check_pkg_names(family, packages)?;
    let head: &[&str] = match family {
        PkgFamily::Arch => &["pacman", "-S", "--needed", "--noconfirm"],
        PkgFamily::Debian => &["apt-get", "install", "-y", "--no-install-recommends"],
        PkgFamily::Fedora => &["dnf", "install", "-y"],
        PkgFamily::Suse => &["zypper", "install", "-y", "--no-recommends"],
        PkgFamily::Gentoo => &["emerge", "--ask=n", "--quiet"],
        PkgFamily::Alpine => &["apk", "add", "--no-progress"],
        PkgFamily::Void => &["xbps-install", "-y"],
        PkgFamily::Nix => &["nix-env", "-iA"],
        PkgFamily::Solus => &["eopkg", "it", "-y"],
        PkgFamily::Clear => &["swupd", "bundle-add"],
        PkgFamily::Alt => &["apt-get", "install", "-y"],
        PkgFamily::Unknown => {
            return Err(unknown_family_error(
                &tr("Install the packages manually:"),
                &packages.join(" "),
            ))
        }
    };
    let mut cmd: Vec<String> = head.iter().map(|s| s.to_string()).collect();
    if family == PkgFamily::Nix {
        cmd.extend(packages.iter().map(|p| format!("nixpkgs.{p}")));
    } else {
        cmd.extend(packages.iter().cloned());
    }
    Ok(cmd)
}

/// Команда удаления пакетов — порт `PackageManager._remove_cmd`.
pub fn remove_cmd(family: PkgFamily, packages: &[String]) -> Result<Vec<String>, String> {
    check_pkg_names(family, packages)?;
    // Жизненно важное не снимается ни одним путём, сырой `pkexec` тоже.
    if let Some(v) = packages.iter().find(|p| is_vital_package(p)) {
        return Err(vital_remove_error(v));
    }
    let head: &[&str] = match family {
        PkgFamily::Arch => &["pacman", "-Rns", "--noconfirm"],
        PkgFamily::Debian => &["apt-get", "remove", "-y", "--purge"],
        PkgFamily::Fedora => &["dnf", "remove", "-y"],
        PkgFamily::Suse => &["zypper", "remove", "-y"],
        PkgFamily::Gentoo => &["emerge", "--unmerge"],
        PkgFamily::Alpine => &["apk", "del"],
        PkgFamily::Void => &["xbps-remove", "-y"],
        PkgFamily::Nix => &["nix-env", "-e"],
        PkgFamily::Solus => &["eopkg", "rm", "-y"],
        PkgFamily::Clear => &["swupd", "bundle-remove"],
        PkgFamily::Alt => &["apt-get", "remove", "-y"],
        PkgFamily::Unknown => {
            return Err(unknown_family_error(
                &tr("Remove the packages manually:"),
                &packages.join(" "),
            ))
        }
    };
    let mut cmd: Vec<String> = head.iter().map(|s| s.to_string()).collect();
    cmd.extend(packages.iter().cloned());
    Ok(cmd)
}

/// Установлен ли пакет — разбор ИСХОДА команды [`query_cmd`].
///
/// `None` = команду не удалось запустить вовсе (`safe_run` вернул None: нет
/// такого пакетного менеджера либо таймаут); `Some(false)` — завершилась не
/// нулём. Вывод команды не читается никем: ответ даёт только код возврата,
/// поэтому мусор в stdout ни на что не влияет.
///
/// Единственное место, где этот вывод делается. Одну и ту же строку
/// `r is not None and r.returncode == 0` проект держал ЧЕТЫРЬМЯ копиями:
/// `PackageManager.is_installed`, `_is_pkg_installed` у Plymouth,
/// `ComplianceCheck._pkg_state` и фильтр «сверх дистрибутива»
/// (`_ensure_extra_packages`).
pub fn pkg_installed_from_run(ok: Option<bool>) -> bool {
    ok == Some(true)
}

/// То же по коду возврата — форма для Python-шима, который отдаёт
/// `r.returncode` (или `None`, если `safe_run` вернул None).
pub fn pkg_installed_from_rc(returncode: Option<i32>) -> bool {
    pkg_installed_from_run(returncode.map(|c| c == 0))
}

/// Команда проверки установленности — порт `PackageManager._query_cmd`.
pub fn query_cmd(family: PkgFamily, package: &str) -> Result<Vec<String>, String> {
    let head: &[&str] = match family {
        PkgFamily::Arch => &["pacman", "-Q"],
        PkgFamily::Debian => &["dpkg", "-s"],
        PkgFamily::Fedora => &["rpm", "-q"],
        PkgFamily::Suse => &["rpm", "-q"],
        PkgFamily::Gentoo => &["equery", "list"],
        PkgFamily::Alpine => &["apk", "info", "-e"],
        PkgFamily::Void => &["xbps-query"],
        PkgFamily::Nix => &["nix-env", "-q"],
        PkgFamily::Solus => &["eopkg", "info"],
        PkgFamily::Clear => &["swupd", "bundle-info"],
        PkgFamily::Alt => &["rpm", "-q"],
        PkgFamily::Unknown => {
            return Err(unknown_family_error(
                &tr("Check the package manually:"), package,
            ))
        }
    };
    let mut cmd: Vec<String> = head.iter().map(|s| s.to_string()).collect();
    cmd.push(package.to_string());
    Ok(cmd)
}

/// Команда обновления БД пакетов — порт `PackageManager._update_db_cmd`
/// (None для UNKNOWN — Python `cmds.get` → None → «нечего делать»).
pub fn update_db_cmd(family: PkgFamily) -> Option<Vec<String>> {
    let cmd: &[&str] = match family {
        PkgFamily::Arch => &["pacman", "-Sy", "--noconfirm"],
        PkgFamily::Debian => &["apt-get", "update", "-q"],
        PkgFamily::Fedora => &["dnf", "makecache", "-q"],
        PkgFamily::Suse => &["zypper", "refresh", "-q"],
        PkgFamily::Gentoo => &["emerge", "--sync"],
        PkgFamily::Alpine => &["apk", "update"],
        PkgFamily::Void => &["xbps-install", "-S"],
        PkgFamily::Nix => &["nix-channel", "--update"],
        PkgFamily::Solus => &["eopkg", "ur"],
        PkgFamily::Clear => &["swupd", "update", "--download"],
        PkgFamily::Alt => &["apt-get", "update", "-q"],
        PkgFamily::Unknown => return None,
    };
    Some(cmd.iter().map(|s| s.to_string()).collect())
}

/// Годится ли имя репозитория для вопроса [`sync_repo_cmd`].
///
/// Пустое имя и ведущий `-` отсекаются первыми: имя уходит менеджеру
/// позиционным аргументом, и `--root` вместо имени уехало бы ключом. Дальше —
/// узкий allowlist: имена репозиториев Gentoo состоят из букв, цифр, `_`, `-`
/// и точки (`guru`, `cosmic-overlay`, `sarahmiaoverlay`); ни пробела, ни
/// перевода строки, ни `/` в них не бывает.
fn valid_repo_name(repo: &str) -> bool {
    !repo.is_empty()
        && !repo.starts_with('-')
        && repo
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// Команда «синхронизировать ОДИН репозиторий» — девятый самостоятельный
/// вопрос движка пакетов.
///
/// Ни к одному прежнему он не сводится. [`update_db_cmd`] обновляет ВСЁ:
/// `emerge --sync` тянет и дерево портежей, и каждый подключённый overlay, —
/// а предмет этого вопроса один названный репозиторий, только что
/// подключённый и потому ещё пустой. Спрашивает его `ensure_gentoo_overlay`
/// (`rexon/system.py`), где команда прежде была зашита.
///
/// Ответ — ПОЛНАЯ команда, вместе с `pkexec`, в отличие от [`update_db_cmd`],
/// который отдаёт голый argv и оставляет выбор «именованный путь или сырой»
/// за [`update_db_plan`]. Выбирать здесь не из чего: rexon-pkg-helper знает
/// четыре операции ([`PKG_HELPER_OPS`]), и такой среди них нет — его `update`
/// синхронизирует всё и позиционных аргументов не принимает вовсе. Заводить
/// пятую операцию значит менять файл, уже разложенный по чужим машинам, ради
/// одного семейства; поэтому путь один — сырой `pkexec`, ровно как было.
///
/// `None` у всех прочих семейств, и это не лень: репозиторий, подключаемый
/// ОТДЕЛЬНО от набора дистрибутива, есть только у Gentoo — это и есть
/// overlay, и только он бывает пустым сразу после подключения.
/// * У pacman пообъектного обновления нет вовсе: `-Sy` обновляет все базы
///   разом, имени репозитория команда не принимает.
/// * У apt то же самое: `apt-get update` читает весь `sources.list`. Сузить
///   его можно лишь подменой `Dir::Etc::sourcelist`, то есть подсовыванием
///   другого файла настроек, а это не операция «обнови вот этот».
/// * У dnf (`--repo`) и zypper (`refresh <алиас>`) синтаксис есть, но задавать
///   им этот вопрос некому: ни один путь Rexon не подключает репозиториев на
///   этих семействах — ни `add-apt-repository`, ни `dnf config-manager
///   --add-repo`, ни `zypper ar` в дереве не встречаются, — так что имени
///   репозитория взять неоткуда. Привилегированная команда, ни разу нами не
///   выполненная и никем не спрашиваемая, хуже честного «не знаю».
pub fn sync_repo_cmd(family: PkgFamily, repo: &str) -> Option<Vec<String>> {
    if family != PkgFamily::Gentoo || !valid_repo_name(repo) {
        return None;
    }
    Some(vec![
        "pkexec".to_string(),
        "emerge".to_string(),
        "--sync".to_string(),
        "--repo".to_string(),
        repo.to_string(),
    ])
}

/// Имя пакетного менеджера — порт `get_pkg_manager_name`.
pub fn pkg_manager_name(family: PkgFamily, aur_helper: Option<&str>) -> String {
    match family {
        PkgFamily::Arch => match aur_helper {
            Some(h) => format!("pacman + {h}"),
            None => "pacman".to_string(),
        },
        PkgFamily::Debian => "apt".to_string(),
        PkgFamily::Fedora => "dnf".to_string(),
        PkgFamily::Suse => "zypper".to_string(),
        PkgFamily::Gentoo => "emerge".to_string(),
        PkgFamily::Alpine => "apk".to_string(),
        PkgFamily::Void => "xbps".to_string(),
        PkgFamily::Nix => "nix-env".to_string(),
        PkgFamily::Solus => "eopkg".to_string(),
        PkgFamily::Clear => "swupd".to_string(),
        PkgFamily::Alt => "apt-get (APT-RPM)".to_string(),
        PkgFamily::Unknown => "unknown".to_string(),
    }
}

/// Lock-файлы пакетных менеджеров — порт `_LOCK_PATHS`. Удерживаются и
/// транзакцией, и post-hooks (snap-pac → snapper → btrfs snapshot).
pub fn lock_paths(family: PkgFamily) -> &'static [&'static str] {
    match family {
        PkgFamily::Arch => &["/var/lib/pacman/db.lck"],
        PkgFamily::Debian => &["/var/lib/dpkg/lock-frontend", "/var/lib/dpkg/lock"],
        PkgFamily::Fedora => &["/var/lib/rpm/.rpm.lock"],
        PkgFamily::Suse => &["/var/run/zypp.pid"],
        PkgFamily::Gentoo => &["/var/lock/portage"],
        PkgFamily::Void => &["/var/lock/xbps"],
        PkgFamily::Alpine => &["/var/lib/apk/db.lock"],
        PkgFamily::Solus => &["/var/lib/eopkg/lock"],
        _ => &[],
    }
}

/// Каталог базы pacman по умолчанию — тот, что берёт сам pacman, когда в
/// `/etc/pacman.conf` нет `DBPath`.
pub const PACMAN_DEFAULT_DBPATH: &str = "/var/lib/pacman/";

/// Имя замка внутри каталога базы pacman.
pub const PACMAN_LOCK_NAME: &str = "db.lck";

/// Замок pacman по выводу `pacman-conf DBPath`. Чистая.
///
/// ЗАЧЕМ. Каталог базы у pacman настраивается (`DBPath` в `/etc/pacman.conf`),
/// и замок лежит в нём. Жёсткий `/var/lib/pacman/db.lck` при своём `DBPath`
/// смотрел мимо: занятая база выглядела свободной, и установка падала
/// «база занята» вместо того, чтобы подождать.
///
/// `conf_stdout` — stdout `pacman-conf DBPath`; `None` — команды нет, она
/// упала или не уложилась. Годится только одна непустая строка с
/// абсолютным путём без управляющих знаков; иначе (пусто, сообщение об
/// ошибке, несколько строк) — запасной [`PACMAN_DEFAULT_DBPATH`].
pub fn pacman_lock_path(conf_stdout: Option<&str>) -> String {
    let dbpath = conf_stdout
        .and_then(|out| {
            let mut lines = out.lines().map(str::trim).filter(|l| !l.is_empty());
            let first = lines.next()?;
            if lines.next().is_some() {
                return None;
            }
            Some(first)
        })
        .filter(|p| p.starts_with('/') && !p.chars().any(char::is_control))
        .unwrap_or(PACMAN_DEFAULT_DBPATH);
    if dbpath.ends_with('/') {
        format!("{dbpath}{PACMAN_LOCK_NAME}")
    } else {
        format!("{dbpath}/{PACMAN_LOCK_NAME}")
    }
}

/// Lock-файлы пакетного менеджера на ЖИВОЙ машине.
///
/// Как [`lock_paths`], но замок pacman — из настоящего `DBPath`
/// (`pacman-conf DBPath`, разбор — [`pacman_lock_path`]). Ответ pacman-conf
/// кэшируется на процесс: настройка не меняется за время работы, а ждать
/// замка зовут на каждой установке.
pub fn lock_paths_live(family: PkgFamily) -> Vec<String> {
    match family {
        PkgFamily::Arch => vec![pacman_lock_path_live().to_string()],
        _ => lock_paths(family).iter().map(|s| s.to_string()).collect(),
    }
}

fn pacman_lock_path_live() -> &'static str {
    static CACHE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    CACHE.get_or_init(|| {
        let report = crate::utils::run(
            &crate::utils::RunSpec {
                argv: vec!["pacman-conf".to_string(), "DBPath".to_string()],
                output: crate::utils::RunOutput::Capture,
                timeout_secs: Some(10),
                lang_c: true,
                env: Vec::new(),
                stderr_to_stdout: false,
            },
            None,
        );
        let stdout = (report.outcome == crate::utils::RunOutcome::Ok)
            .then(|| String::from_utf8_lossy(&report.stdout).into_owned());
        pacman_lock_path(stdout.as_deref())
    })
}

/// Шаг ожидания lock — porт `_wait_for_pkg_lock` (poll 2 с, максимум 300 с).
fn wait_lock_step(family: PkgFamily) -> PlanStep {
    PlanStep::WaitPkgLock {
        paths: lock_paths(family).iter().map(|s| s.to_string()).collect(),
        timeout_secs: 300,
        poll_secs: 2,
    }
}

// ============================================================================
// update_db / install — порт PackageManager.update_db / install
// ============================================================================

/// Семейства, где update_db идёт под root (все кроме NIX/UNKNOWN) — как
/// кортеж `privileged` в Python.
fn update_db_privileged(family: PkgFamily) -> bool {
    !matches!(family, PkgFamily::Nix | PkgFamily::Unknown)
}

/// План `update_db`: пустой план = «нечего делать, успех».
pub fn update_db_plan(family: PkgFamily, pkg_helper_available: bool) -> Vec<PlanStep> {
    let Some(cmd) = update_db_cmd(family) else {
        return Vec::new();
    };
    let full = if update_db_privileged(family) {
        match privileged_pkg_cmd(family, "update", &[], pkg_helper_available) {
            Some(named) => named,
            None => {
                let mut v = vec!["pkexec".to_string()];
                v.extend(cmd);
                v
            }
        }
    } else {
        cmd
    };
    // 120 с не хватало на медленном зеркале: обновление базы — сеть.
    vec![run_step(RunStepId::PkgUpdateDb, full, Some(600), false,
              "Обновление базы пакетов")]
}

/// Привилегированная команда установки: именованный путь через
/// rexon-pkg-helper, иначе сырой `pkexec` поверх [`install_cmd`].
///
/// Выбор один на все места, где Rexon ставит пакеты. Рукописная копия этого
/// «иначе» и была тем, как в `ensure_gentoo_overlay` завёлся ТРЕТИЙ вариант
/// команды Gentoo — `emerge --ask=n` без `--quiet`, расходящийся и с таблицей
/// [`install_cmd`], и со встроенным шаблоном самого helper'а.
fn privileged_install_argv(
    family: PkgFamily,
    packages: &[String],
    base: &[String],
    helper_available: bool,
) -> Vec<String> {
    if let Some(named) = privileged_pkg_cmd(family, "install", packages, helper_available) {
        return named;
    }
    let mut argv = vec!["pkexec".to_string()];
    argv.extend(base.iter().cloned());
    argv
}

/// Выбор пути установки поверх уже собранной [`install_cmd`]: NixOS ставит
/// пакеты в профиль ПОЛЬЗОВАТЕЛЯ (`nix-env -iA`) и root'а не спрашивает —
/// `pkexec` там не нужен и был бы прямой ошибкой; остальные десять семейств
/// идут привилегированно.
fn choose_install_argv(
    family: PkgFamily,
    packages: &[String],
    base: &[String],
    helper_available: bool,
) -> Vec<String> {
    if family == PkgFamily::Nix {
        return base.to_vec();
    }
    privileged_install_argv(family, packages, base, helper_available)
}

/// **Единственный** ответ движка на вопрос «какой командой поставить вот эти
/// пакеты» — от таблицы флагов до выбора «именованный путь или сырой pkexec».
///
/// Отдельная точка входа рядом с [`install_plan`] нужна тем местам, которым
/// нужна одна команда, а не план с шагами, таймаутом и подсказкой sudo:
/// доустановка видеозависимостей движка обоев
/// ([`crate::wallpaper_engine::install_deps_argv`]) и починка соответствия
/// ([`crate::compliance::pkg_install_arch`]). Обе прежде держали СВОИ таблицы
/// и обе успели с движком разойтись.
///
/// `None` — команды нет и выдумывать её нельзя:
/// * список пакетов пуст — «поставить ничего» командой не выражается, а
///   `pacman -S --needed --noconfirm` без единого имени вернул бы ошибку
///   использования от имени root;
/// * семейство `UNKNOWN` — менеджер не опознан, и вызывающий говорит человеку,
///   что доустановить руками.
pub fn install_argv(
    family: PkgFamily,
    packages: &[String],
    helper_available: bool,
) -> Option<Vec<String>> {
    if packages.is_empty() {
        return None;
    }
    let base = install_cmd(family, packages).ok()?;
    Some(choose_install_argv(family, packages, &base, helper_available))
}

/// План `install` + подсказка sudo при отказе pkexec.
pub struct InstallPlan {
    pub steps: Vec<PlanStep>,
    /// Ранний выход без шагов: (ok, message).
    pub early_result: Option<(bool, String)>,
    /// Текст ошибки «Пароль администратора не подтверждён…» (Python показывает его при
    /// returncode 126 / "Not authorized").
    pub sudo_hint: String,
}

/// Порт `PackageManager.install`.
pub fn install_plan(
    family: PkgFamily,
    packages: &[String],
    aur: bool,
    aur_helper: Option<&str>,
    pkg_helper_available: bool,
) -> Result<InstallPlan, String> {
    if packages.is_empty() {
        return Ok(InstallPlan {
            steps: Vec::new(),
            early_result: Some((true, tr("No packages to install"))),
            sudo_hint: String::new(),
        });
    }
    let base_cmd = install_cmd(family, packages)?;
    let aur_argv = if aur && family == PkgFamily::Arch {
        aur_helper.map(|helper| crate::aur_install::install_argv(helper, packages, false))
    } else {
        None
    };
    let argv = match aur_argv {
        Some(v) => v,
        // Тот же выбор, что отдаёт `install_argv` наружу: второй копии
        // «pkexec или именованный путь, и Nix без root» быть не должно.
        None => choose_install_argv(family, packages, &base_cmd, pkg_helper_available),
    };
    let sudo_hint = trf(
        "The administrator password wasn't confirmed.\nYou can install from a terminal: sudo %1",
        &[&base_cmd.join(" ")],
    );
    Ok(InstallPlan {
        steps: vec![run_step(RunStepId::PkgInstall, argv, None, false,
                             "Установка пакетов")],
        early_result: None,
        sudo_hint,
    })
}

// ============================================================================
// remove — порт PackageManager.remove (полный пайплайн как план)
// ============================================================================

/// План `remove`. Пробные шаги (`phase` в description: qg / query /
/// required-by / closure / skip-note) идут в порядке Python; порядок
/// closure-фазы в Python недетерминирован (итерация set) — паритет-тест
/// сортирует этот сегмент с обеих сторон.
pub struct RemovePlan {
    pub steps: Vec<PlanStep>,
    /// Результат, если исполняющих шагов (wait+remove) нет: (ok, message).
    pub result_without_exec: Option<(bool, String)>,
    pub safe: Vec<String>,
    pub skipped: Vec<String>,
    /// Аннотации skipped: "pkg (требуется: a, b)" / "(нужен другому skipped
    /// пакету)".
    pub skipped_annotated: Vec<String>,
    /// Добавка к stdout при успехе ("\n\nПропущено…" или "").
    pub success_note: String,
    /// Текст ошибки при отказе pkexec.
    pub sudo_hint: String,
}

/// Порт `PackageManager.remove`. Вывод pacman подаётся замыканиями
/// (как в `system.rs`): `qg`/`qi` — stdout при rc==0, `query_installed` —
/// установлен ли пакет (rc==0 query-команды).
pub fn remove_plan(
    family: PkgFamily,
    packages: &[String],
    qg: &dyn Fn(&str) -> Option<String>,
    qi: &dyn Fn(&str) -> Option<String>,
    query_installed: &dyn Fn(&str) -> bool,
    pkg_helper_available: bool,
) -> Result<RemovePlan, String> {
    let mut plan = RemovePlan {
        steps: Vec::new(),
        result_without_exec: None,
        safe: Vec::new(),
        skipped: Vec::new(),
        skipped_annotated: Vec::new(),
        success_note: String::new(),
        sudo_hint: String::new(),
    };
    if packages.is_empty() {
        plan.result_without_exec = Some((true, String::new()));
        return Ok(plan);
    }

    // 1. Раскрыть группы (шаг «qg» на каждый входной пакет). Команду задаёт
    //    движок пакетов: он же решает, у какого семейства вопрос «что входит
    //    в группу» вообще есть — где его нет, шага не будет.
    for p in packages {
        if let Some(q) = package_group_query(family, p) {
            plan.steps.push(run_step(
                RunStepId::PkgQueryGroup,
                q.cmd.argv, q.cmd.timeout_secs, q.cmd.lang_c, "qg",
            ));
        }
    }
    // Жизненно важное (член группы — тоже) из набора выпадает молча:
    // снимать его не станет ни помощник, ни сырой путь (`remove_cmd`).
    let expanded: Vec<String> = expand_groups(family, packages, qg)
        .into_iter()
        .filter(|p| !is_vital_package(p))
        .collect();

    // 2. Отфильтровать не установленные (query-команда на каждый).
    let mut installed: Vec<String> = Vec::new();
    for p in &expanded {
        plan.steps.push(run_step(RunStepId::PkgQuery, query_cmd(family, p)?,
                                 None, false, "query"));
        if query_installed(p) {
            installed.push(p.clone());
        }
    }
    if installed.is_empty() {
        plan.result_without_exec =
            Some((true, tr("No installed packages to remove")));
        return Ok(plan);
    }

    // 3. Forbidden set + транзитивное замыкание (шаги -Qi; та же логика, что
    //    system::filter_removable_transitive, но с записью шагов плана).
    let candidate: std::collections::HashSet<String> =
        installed.iter().cloned().collect();
    let mut forbidden: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    if family == PkgFamily::Arch {
        for p in &installed {
            if let Some(q) = installed_meta_query(family, p) {
                plan.steps.push(run_step(
                    RunStepId::PkgRequiredBy,
                    q.cmd.argv, q.cmd.timeout_secs, q.cmd.lang_c, "required-by",
                ));
            }
            let outside: Vec<String> = qi_field(family, p, "Required By", qi)
                .into_iter()
                .filter(|d| !candidate.contains(d))
                .collect();
            if !outside.is_empty() {
                forbidden.insert(p.clone());
            }
        }
        // Замыкание — снапшот forbidden на каждый проход (list(forbidden)
        // в Python); порядок прохода здесь — по installed (детерминирован).
        let mut changed = true;
        while changed {
            changed = false;
            let snapshot: Vec<String> = installed
                .iter()
                .filter(|p| forbidden.contains(*p))
                .cloned()
                .collect();
            for p in snapshot {
                if let Some(q) = installed_meta_query(family, &p) {
                    plan.steps.push(run_step(
                        RunStepId::PkgClosure,
                        q.cmd.argv, q.cmd.timeout_secs, q.cmd.lang_c, "closure",
                    ));
                }
                let deps: std::collections::HashSet<String> =
                    qi_field(family, &p, "Depends On", qi).into_iter().collect();
                for d in deps.intersection(&candidate) {
                    if !forbidden.contains(d) {
                        forbidden.insert(d.clone());
                        changed = true;
                    }
                }
            }
        }
    }
    plan.safe = installed
        .iter()
        .filter(|p| !forbidden.contains(*p))
        .cloned()
        .collect();
    plan.skipped = installed
        .iter()
        .filter(|p| forbidden.contains(*p))
        .cloned()
        .collect();

    // Аннотации skipped (повторные -Qi «Required By» — как в Python).
    for p in &plan.skipped {
        plan.steps.push(run_step(
            RunStepId::PkgSkipNote,
            vec!["pacman".into(), "-Qi".into(), p.clone()],
            None, true, "skip-note",
        ));
        let outsiders: Vec<String> = qi_field(family, p, "Required By", qi)
            .into_iter()
            .filter(|d| !candidate.contains(d))
            .collect();
        if !outsiders.is_empty() {
            let head: Vec<&str> =
                outsiders.iter().take(3).map(String::as_str).collect();
            plan.skipped_annotated
                .push(trf("%1 (required by: %2)", &[&p, &head.join(", ")]));
        } else {
            plan.skipped_annotated
                .push(trf("%1 (needed by another skipped package)", &[&p]));
        }
    }

    if plan.safe.is_empty() {
        let bullets: Vec<String> = plan
            .skipped_annotated
            .iter()
            .map(|s| format!("  • {s}"))
            .collect();
        plan.result_without_exec = Some((
            false,
            trf(
                "All of this desktop's packages are needed by other installed programs:\n%1",
                &[&bullets.join("\n")],
            ),
        ));
        return Ok(plan);
    }

    // 4. Ожидание lock + команда удаления (timeout 600).
    plan.steps.push(wait_lock_step(family));
    let base_cmd = remove_cmd(family, &plan.safe)?;
    let argv = if family == PkgFamily::Nix {
        base_cmd.clone()
    } else {
        match privileged_pkg_cmd(family, "remove", &plan.safe, pkg_helper_available)
        {
            Some(named) => named,
            None => {
                let mut v = vec!["pkexec".to_string()];
                v.extend(base_cmd.iter().cloned());
                v
            }
        }
    };
    plan.steps.push(run_step(RunStepId::PkgRemove, argv,
                             Some(crate::utils::DOWNLOAD_TIMEOUT_SECS),
                             false, "remove"));

    if !plan.skipped_annotated.is_empty() {
        let bullets: Vec<String> = plan
            .skipped_annotated
            .iter()
            .map(|s| format!("  • {s}"))
            .collect();
        plan.success_note = format!(
            "\n\n{}",
            trf("Skipped (needed by other desktops):\n%1", &[&bullets.join("\n")])
        );
    }
    // ⭐ Починка: подсказка отказа при УДАЛЕНИИ говорила «Установите из
    // терминала».
    plan.sudo_hint = trf(
        "The administrator password wasn't confirmed.\nYou can remove from a terminal: sudo %1",
        &[&base_cmd.join(" ")],
    );
    Ok(plan)
}

// ============================================================================
// install_batch — порт PackageManager.install_batch (план + тексты скриптов)
// ============================================================================

/// NixOS: сниппеты для configuration.nix — порт `_NIXOS_DE_SNIPPETS`.
pub const NIXOS_DE_SNIPPETS: &[(&str, &str)] = &[
    ("kde", "services.desktopManager.plasma6.enable = true;\nservices.displayManager.sddm.enable = true;"),
    ("gnome", "services.xserver.desktopManager.gnome.enable = true;\nservices.xserver.displayManager.gdm.enable = true;"),
    ("xfce", "services.xserver.desktopManager.xfce.enable = true;\nservices.displayManager.lightdm.enable = true;"),
    ("cinnamon-mint", "services.xserver.desktopManager.cinnamon.enable = true;\nservices.displayManager.lightdm.enable = true;"),
    ("mate", "services.xserver.desktopManager.mate.enable = true;\nservices.displayManager.lightdm.enable = true;"),
    ("budgie", "services.xserver.desktopManager.budgie.enable = true;\nservices.displayManager.lightdm.enable = true;"),
    // Модуль nixpkgs `nixos/modules/services/desktop-managers/pantheon.nix`
    // (`config.services.desktopManager.pantheon`) сам включает приветствие
    // Pantheon, но требует lightdm и предупреждает, если тот выключен.
    ("pantheon", "services.xserver.enable = true;\nservices.xserver.displayManager.lightdm.enable = true;\nservices.desktopManager.pantheon.enable = true;"),
    ("cosmic", "services.desktopManager.cosmic.enable = true;\nservices.displayManager.cosmic-greeter.enable = true;"),
    ("hyprland", "programs.hyprland.enable = true;"),
    ("lxqt", "services.xserver.desktopManager.lxqt.enable = true;\nservices.displayManager.sddm.enable = true;"),
];

/// Приписки к сниппетам [`NIXOS_DE_SNIPPETS`] — комментарий `# + …` в конце
/// сниппета. Это подсказка человеку, поэтому она из каталога перевода, а
/// сам сниппет (код configuration.nix) не переводится.
pub const NIXOS_DE_NOTES: &[(&str, &str)] = &[
    ("hyprland", n_("set up waybar, wofi, kitty, etc.")),
];

/// Сниппет configuration.nix для стола вместе с припиской на языке
/// интерфейса; `None` — стол на NixOS через Rexon не ставится.
pub fn nixos_de_snippet(de: &str) -> Option<String> {
    let snippet = table_get(NIXOS_DE_SNIPPETS, de)?;
    Some(match table_get(NIXOS_DE_NOTES, de) {
        Some(note) => format!("{snippet}\n# + {}", tr(note)),
        None => snippet.to_string(),
    })
}

/// Строка `echo '…'` сценария установки: вывод сценария идёт в журнал окна,
/// поэтому текст — из каталога перевода, а одинарная кавычка в переводе
/// экранируется для оболочки.
fn echo_line(text: &str) -> String {
    format!("echo '{}'", text.replace('\'', "'\\''"))
}

/// Какие DE требуют overlay на Gentoo — порт `_GENTOO_OVERLAYS`.
pub const GENTOO_OVERLAYS: &[(&str, &str)] = &[
    ("budgie", "sarahmiaoverlay"),
    ("cosmic", "cosmic-overlay"),
];

/// Пакет, дающий подкоманду `eselect repository`: без него overlay нечем
/// подключить. Имя жило зашитым в `ensure_gentoo_overlay` (`rexon/system.py`),
/// а место ему здесь — рядом с таблицей overlay'ев, где лежат остальные
/// данные о них.
pub const GENTOO_OVERLAY_TOOL_PACKAGE: &str = "app-eselect/eselect-repository";

fn table_get<'a>(table: &'a [(&str, &str)], key: &str) -> Option<&'a str> {
    table.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
}

/// План `install_batch` + тексты сгенерированных скриптов.
pub struct BatchPlan {
    pub steps: Vec<PlanStep>,
    /// Ранний выход (пусто / NixOS с de_id / иммутабельный): (ok, message).
    pub early_result: Option<(bool, String)>,
    /// Основные пакеты ставились через rexon-de-helper (DM уже включён в том
    /// же сеансе авторизации — install_de не делает 2-й pkexec).
    pub used_de_helper: bool,
    /// Итоговое сообщение при успехе.
    pub final_message: String,
}

/// Порт `PackageManager.install_batch`. Пути tmp-файлов подаются параметрами
/// (исполнитель создаёт их через mkstemp) — поэтому argv и тексты скриптов
/// детерминированы и сверяемы с Python.
#[allow(clippy::too_many_arguments)]
pub fn install_batch_plan(
    family: PkgFamily,
    detector: &DistroDetector,
    packages: &[String],
    aur_packages: &[String],
    aur_helper: Option<&str>,
    update_first: bool,
    de_id: Option<&str>,
    de_helper_available: bool,
    tmp_sh: &str,
    tmp_log: &str,
) -> Result<BatchPlan, String> {
    install_batch_plan_with_deps(
        family, detector, packages, &[], &[], aur_packages, aur_helper, update_first, de_id,
        de_helper_available, tmp_sh, tmp_log,
    )
}

/// Строка скрипта установки: пометить `deps` установленными как
/// зависимости (`pacman -D --asdeps`), чтобы удаление стола (`-Rns`) их
/// сняло. Только Arch; пусто — строки нет.
pub fn mark_deps_line(family: PkgFamily, deps: &[String]) -> Result<Option<String>, String> {
    if family != PkgFamily::Arch || deps.is_empty() {
        return Ok(None);
    }
    check_pkg_names(family, deps)?;
    Ok(Some(format!("pacman -D --asdeps {}", deps.join(" "))))
}

/// [`install_batch_plan`] и `deps_only` — пакеты из `packages`, которые
/// транзакция ставит лишь как зависимости набора (DKMS-модуль и заголовки
/// под стоящие ядра, [`InstallPreview::deps_only`]). Помечаются в том же
/// скрипте, после успешной установки, — без второго пароля. `replace` —
/// установленные пакеты, которые человек согласился заменить
/// ([`conflict_swap_approved`]): та же транзакция снимает их
/// ([`install_cmd_replacing`]), и идёт она обёрткой, а не помощником —
/// у его именованного действия согласия на замену нет.
#[allow(clippy::too_many_arguments)]
pub fn install_batch_plan_with_deps(
    family: PkgFamily,
    detector: &DistroDetector,
    packages: &[String],
    deps_only: &[String],
    replace: &[String],
    aur_packages: &[String],
    aur_helper: Option<&str>,
    update_first: bool,
    de_id: Option<&str>,
    de_helper_available: bool,
    tmp_sh: &str,
    tmp_log: &str,
) -> Result<BatchPlan, String> {
    let deps_only: Vec<String> = deps_only.iter().filter(|d| packages.contains(d)).cloned().collect();
    let mark_deps = mark_deps_line(family, &deps_only)?;
    let mut plan = BatchPlan {
        steps: Vec::new(),
        early_result: None,
        used_de_helper: false,
        final_message: tr("Installation complete"),
    };
    if packages.is_empty() && aur_packages.is_empty() {
        plan.early_result = Some((true, tr("No packages to install")));
        return Ok(plan);
    }

    // ── NixOS: DE ставится через configuration.nix, не через nix-env ──
    if family == PkgFamily::Nix {
        if let Some(de) = de_id {
            let msg = match nixos_de_snippet(de) {
                Some(snippet) => trf(
                    // TRANSLATORS: %1 is a configuration.nix snippet.
                    "NixOS uses a declarative model.\n\
                     Add this to /etc/nixos/configuration.nix:\n\n\
                     %1\n\n\
                     Then run:\n  sudo nixos-rebuild switch",
                    &[&snippet],
                ),
                None => trf(
                    "Rexon can't set up the '%1' desktop on NixOS.\n\
                     Set it up yourself in /etc/nixos/configuration.nix.",
                    &[&de],
                ),
            };
            plan.early_result = Some((false, msg));
            return Ok(plan);
        }
    }

    // ── SteamOS: установка через steamos-readonly ──
    if detector.is_steamos() {
        let update = update_db_cmd(family);
        let install = install_cmd_replacing(family, packages, replace)?;

        let mut script_lines: Vec<String> = vec![
            "#!/bin/bash".into(),
            "set -e".into(),
            // Вывод pacman разбирает парсер хода установки (`parse_pacman_line`):
            // «warning:», «error:», «is up to date -- skipping». Как у обычного
            // пути ниже: иначе на русской или немецкой системе разбор не
            // находит ничего. `LC_ALL=C` заодно глушит и `LANGUAGE`, которую
            // окно Rexon ставит своим языком: gettext при C-локали её не читает.
            "export LC_ALL=C LANG=C".into(),
            String::new(),
            "# ── Шаг 1: Отключение read-only ──".into(),
            echo_line(&tr("🔓 Turning off steamos-readonly…")),
            "steamos-readonly disable".into(),
            String::new(),
            "# ── Шаг 2: Инициализация ключей pacman ──".into(),
            echo_line(&tr("🔑 Setting up the pacman keys…")),
            "pacman-key --init 2>/dev/null || true".into(),
            "pacman-key --populate archlinux 2>/dev/null || true".into(),
            "pacman-key --populate holo 2>/dev/null || true".into(),
            String::new(),
        ];
        if update_first {
            if let Some(upd) = &update {
                script_lines.push("# ── Шаг 3: Обновление БД пакетов ──".into());
                script_lines.push(upd.join(" "));
                script_lines.push(String::new());
            }
        }
        let install_line = install.join(" ").replace(
            "--noconfirm",
            "--noconfirm --noprogressbar --color never",
        );
        script_lines.extend([
            "# ── Шаг 4: Установка пакетов ──".to_string(),
            // TRANSLATORS: %1 is a number of packages.
            echo_line(&trf("📦 Installing packages: %1…", &[&packages.len()])),
            install_line,
        ]);
        script_lines.extend(mark_deps.clone());
        script_lines.extend([
            String::new(),
            "# ── Шаг 5: Включение read-only обратно ──".to_string(),
            echo_line(&tr("🔒 Turning steamos-readonly back on…")),
            "steamos-readonly enable".to_string(),
            String::new(),
            echo_line(&format!("✅ {}", tr("Installation on SteamOS complete"))),
        ]);
        let script_body = script_lines.join("\n") + "\n";

        // chown в начале — fs.protected_regular=1 блокирует root от truncate
        // файлов, owned by обычного юзера, в /tmp (sticky-bit). Только chmod
        // 0o666 не помогает — ядро смотрит на владельца, не на права.
        let wrapper = format!(
            "#!/bin/bash\n\
             {STDIN_GUARD}\
             chown root:root {tmp_log} 2>/dev/null\n\
             > {tmp_log}\n\
             (\n\
             {script_body}\
             ) >> {tmp_log} 2>&1\n\
             EXIT_CODE=$?\n\
             echo \"__REXON_EXIT__$EXIT_CODE\" >> {tmp_log}\n\
             # Всегда включаем readonly обратно при ошибке\n\
             if [ $EXIT_CODE -ne 0 ]; then\n\
             \x20 steamos-readonly enable 2>/dev/null || true\n\
             fi\n\
             # Возвращаем владельца, чтобы user мог удалить tmp-файлы\n\
             chown $PKEXEC_UID:$PKEXEC_UID {tmp_log} {tmp_sh} 2>/dev/null\n\
             exit $EXIT_CODE\n"
        );
        plan.steps.push(PlanStep::WriteFile {
            path: tmp_sh.to_string(),
            content: wrapper,
            mode: 0o755,
        });
        plan.steps.push(PlanStep::WriteFile {
            path: tmp_log.to_string(),
            content: String::new(),
            mode: 0o666,
        });
        plan.steps.push(wait_lock_step(family));
        plan.steps.push(run_step(
            RunStepId::PkgMain,
            vec!["pkexec".into(), "bash".into(), tmp_sh.to_string()],
            Some(crate::utils::DOWNLOAD_TIMEOUT_SECS), false, "SteamOS: установка пакетов",
        ));
        plan.steps.push(PlanStep::RemoveFiles {
            paths: vec![tmp_sh.to_string(), tmp_log.to_string()],
        });
        // AUR на SteamOS — сразу, без ожидания lock (как в Python).
        if !aur_packages.is_empty() {
            if let Some(helper) = aur_helper {
                let argv = crate::aur_install::install_argv(helper, aur_packages, false);
                plan.steps.push(run_step(RunStepId::PkgAur, argv, None, false,
                                         "Установка AUR"));
            }
        }
        plan.final_message = tr("Installation on SteamOS complete");
        return Ok(plan);
    }

    // ── Иммутабельные (ostree) дистрибутивы ──
    if detector.is_immutable() && detector.distro_id() != "nixos" {
        let distro_id = detector.distro_id();
        let pkg_str = packages.join(" ");
        let variant_ostree = detector
            .info
            .get("VARIANT_ID")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_lowercase()
            .contains("ostree");
        let msg = if matches!(distro_id.as_str(),
                              "silverblue" | "kinoite" | "sericea" | "onyx")
            || detector.name().to_lowercase().contains("atomic")
            || variant_ostree
        {
            trf(
                // TRANSLATORS: %1 is the distribution name, %2 package names.
                "%1 is an immutable distribution.\n\
                 The regular dnf doesn't work here. Use:\n\n  \
                 rpm-ostree install %2\n  \
                 systemctl reboot\n\n\
                 Or install the desktop with Fedora Toolbox / Distrobox.",
                &[&detector.pretty_name(), &pkg_str],
            )
        } else if distro_id == "bazzite" {
            trf(
                "Bazzite is an immutable (ostree) distribution.\n\
                 To install packages:\n\n  \
                 rpm-ostree install %1\n  \
                 systemctl reboot\n\n\
                 Or use Distrobox for an isolated environment.",
                &[&pkg_str],
            )
        } else {
            trf(
                "%1 is an immutable distribution.\n\
                 Installing packages with the regular package manager may not work.\n\
                 Use Flatpak, Distrobox or your distribution's own tools.",
                &[&detector.pretty_name()],
            )
        };
        plan.early_result = Some((false, msg));
        return Ok(plan);
    }

    // ── 0. Gentoo: подключить overlay если нужен ──
    if family == PkgFamily::Gentoo {
        if let Some(de) = de_id {
            if let Some(overlay) = table_get(GENTOO_OVERLAYS, de) {
                plan.steps.push(PlanStep::EnsureGentooOverlay {
                    overlay: overlay.to_string(),
                });
            }
        }
    }

    // ── 1. Основные пакеты (один pkexec) ──
    if !packages.is_empty() {
        let update = update_db_cmd(family);
        let install = install_cmd_replacing(family, packages, replace)?;

        if family == PkgFamily::Nix {
            // Nix не использует pkexec.
            if update_first {
                if let Some(upd) = update {
                    plan.steps.push(run_step(RunStepId::PkgUpdateDb, upd,
                                             None, false,
                                             "Обновление базы пакетов"));
                }
            }
            // nix умеет собирать из исходников — класс команды это знает.
            plan.steps.push(run_step(RunStepId::PkgNix, install, None, false,
                                     "Установка пакетов (nix-env)"));
        } else {
            // Часть B: именованное действие rexon-de-helper (auth_admin_keep:
            // подтверждённый пароль держится несколько минут и только для
            // окна Rexon); иначе legacy `pkexec bash <tmp_sh>` (пароль каждый
            // раз).
            // Помощник ставит набор из СВОЕЙ таблицы, а не переданный: если
            // набор поправлен (выкинут член группы, добавлен DKMS-модуль под
            // стоящее ядро, вариант Deepin на Treeland), ставит обёртка.
            let use_de_helper = de_id
                .map(|de| de_helper_available
                     && de_helper_supports(de, family.as_str())
                     && de_helper_covers(de, family, packages)
                     && mark_deps.is_none()
                     && replace.is_empty())
                .unwrap_or(false);
            plan.used_de_helper = use_de_helper;

            if !use_de_helper {
                // Один bash-скрипт → один pkexec (legacy-путь). LC_ALL=C —
                // стабильный английский вывод pacman для парсера прогресса.
                let mut script_lines: Vec<String> = vec![
                    "#!/bin/bash".into(),
                    "set -e".into(),
                    "export LC_ALL=C LANG=C".into(),
                ];
                if update_first {
                    if let Some(upd) = &update {
                        script_lines.push(upd.join(" "));
                    }
                }
                let install_line = install.join(" ").replace(
                    "--noconfirm",
                    "--noconfirm --noprogressbar --color never",
                );
                script_lines.push(install_line);
                script_lines.extend(mark_deps.clone());
                let script_body = script_lines.join("\n") + "\n";

                // Обёртка: весь вывод pacman → лог-файл; chown-грабля
                // fs.protected_regular (root:root в начале, PKEXEC_UID в конце).
                let wrapper = format!(
                    "#!/bin/bash\n\
                     {STDIN_GUARD}\
                     chown root:root {tmp_log} 2>/dev/null\n\
                     > {tmp_log}\n\
                     (\n\
                     {script_body}\
                     ) >> {tmp_log} 2>&1\n\
                     EXIT_CODE=$?\n\
                     echo \"__REXON_EXIT__$EXIT_CODE\" >> {tmp_log}\n\
                     # Возвращаем владельца, чтобы user мог удалить tmp-файлы\n\
                     chown $PKEXEC_UID:$PKEXEC_UID {tmp_log} {tmp_sh} 2>/dev/null\n\
                     exit $EXIT_CODE\n"
                );
                plan.steps.push(PlanStep::WriteFile {
                    path: tmp_sh.to_string(),
                    content: wrapper,
                    mode: 0o755,
                });
            }
            // Лог: очистить и дать доступ root (fs.protected_regular=2).
            plan.steps.push(PlanStep::WriteFile {
                path: tmp_log.to_string(),
                content: String::new(),
                mode: 0o666,
            });
            plan.steps.push(wait_lock_step(family));
            let pkexec_argv = if use_de_helper {
                let mut argv = vec!["pkexec".to_string(), de_helper_path().to_string()];
                // Язык сообщений помощника (решение 30.09.2026).
                argv.extend(crate::system_l10n::lang_args(de_helper_path()));
                argv.extend([
                    de_id.unwrap_or_default().to_string(),
                    family.as_str().to_string(), tmp_log.to_string(),
                ]);
                argv
            } else {
                vec!["pkexec".to_string(), "bash".to_string(),
                     tmp_sh.to_string()]
            };
            // За `pkexec bash <скрипт>` стоит установка целого стола:
            // 600 с хватало не всегда, и это скачивание, а не опрос.
            plan.steps.push(run_step(RunStepId::PkgMain, pkexec_argv,
                                     Some(crate::utils::DOWNLOAD_TIMEOUT_SECS),
                                     false, "Установка пакетов (один pkexec)"));
            let mut cleanup = Vec::new();
            if !use_de_helper {
                cleanup.push(tmp_sh.to_string());
            }
            cleanup.push(tmp_log.to_string());
            plan.steps.push(PlanStep::RemoveFiles { paths: cleanup });
        }
    }

    // ── 2. AUR пакеты (через yay/paru — без pkexec) ──
    if !aur_packages.is_empty() {
        if let Some(helper) = aur_helper {
            // snap-pac post-hook от завершённого pacman -S может держать lock.
            plan.steps.push(wait_lock_step(family));
            // Права — окном polkit, вопросы помощника отвечены заранее:
            // у окна нет терминала (`aur_install::install_argv`).
            let argv = crate::aur_install::install_argv(helper, aur_packages, false);
            // None = по классу команды: помощник AUR СОБИРАЕТ из исходников,
            // и меряется это часами, а не полуминутным дефолтом. Ровно здесь
            // 2026-09-01 умерла установка стола MATE — девять пакетов сняли
            // по таймауту 30 с, и человек прочитал «не установились».
            plan.steps.push(run_step(RunStepId::PkgAur, argv, None, false,
                                         "Установка AUR"));
        }
    }
    Ok(plan)
}

// ============================================================================
// Оркестрация DE: dm-enable / remove_de / switch_de / logout
// ============================================================================

/// Пары (имя ПАКЕТА, имя ЮНИТА) display-manager'ов — порт `dm_pairs` из
/// `install_de`. Обычно совпадают; Plasma Login Manager (Arch, Plasma 6.6+) —
/// исключение: пакет plasma-login-manager, юнит plasmalogin. Порядок =
/// приоритет.
pub const DM_PAIRS: &[(&str, &str)] = &[
    ("plasma-login-manager", "plasmalogin"),
    ("cosmic-greeter", "cosmic-greeter"),
    ("lightdm", "lightdm"),
    ("gdm", "gdm"),
    ("sddm", "sddm"),
    // Debian: пакет и юнит gdm3 (GNOME там ставится с ним).
    ("gdm3", "gdm3"),
    ("lxdm", "lxdm"),
];

/// DM-юниты, требующие `systemctl enable --force` (не Provides
/// display-manager.service) — порт `_DM_FORCE_ENABLE`.
pub const DM_FORCE_ENABLE: &[&str] = &["plasmalogin"];

/// Маппинг ИМЯ-ЮНИТА → ИМЯ-ПАКЕТА — порт `_DM_PKG_FOR_UNIT`.
pub const DM_PKG_FOR_UNIT: &[(&str, &str)] = &[("plasmalogin", "plasma-login-manager")];

/// Шаг включения display-manager'а после установки DE — порт dm-ветки
/// `install_de`. None — если DM в пакет-листе не найден, ИЛИ основные пакеты
/// шли через rexon-de-helper (он уже разобрался с экраном входа в том же
/// сеансе авторизации), ИЛИ включённый экран входа в системе уже есть.
/// Возвращает (имя юнита, команда с timeout 30).
///
/// ⚠️ `dm_present` (ответ `display_manager_present_from`) — то самое правило
/// «чужой экран входа не трогаем»: подмена настроенного DM заметна человеку
/// и не делается заодно с установкой стола. Установить свой DM можно только
/// там, где экрана входа нет вовсе — иначе после перезагрузки не будет входа
/// в графику. Правило одно на все пути: здесь, в rexon-de-helper и в
/// `ensure_dm_plan`.
pub fn dm_enable_step(
    family: PkgFamily,
    installed_pkgs: &[String],
    used_de_helper: bool,
    pkg_helper_available: bool,
    dm_present: bool,
) -> Option<(String, Cmd)> {
    if used_de_helper || dm_present {
        return None;
    }
    for (dm_pkg, dm_unit) in DM_PAIRS {
        if installed_pkgs.iter().any(|p| p == dm_pkg) {
            let unit = (*dm_unit).to_string();
            let argv = match privileged_pkg_cmd(
                family, "enable-dm",
                std::slice::from_ref(&unit), pkg_helper_available,
            ) {
                Some(named) => named,
                None => {
                    // --force: PLM не Provides display-manager.service.
                    let mut raw = vec![
                        "pkexec".to_string(), "systemctl".to_string(),
                        "enable".to_string(),
                    ];
                    if DM_FORCE_ENABLE.contains(dm_unit) {
                        raw.push("--force".to_string());
                    }
                    raw.push(format!("{dm_unit}.service"));
                    raw
                }
            };
            return Some((unit, Cmd::new(argv, Some(30), false)));
        }
    }
    None
}

/// Решение `remove_de` — 2-уровневая защита от удаления активного DE
/// (правило архитектуры #13: backend отказывает при force=false, UI обязан
/// дополнительно блокировать кнопку).
pub enum RemoveDeDecision {
    /// Отказ: текст ошибки.
    Refused(String),
    /// Можно удалять: полный список пакетов (основные + AUR — иначе
    /// AUR-расширения блокируют удаление основных через зависимости).
    Proceed(Vec<String>),
}

/// Порт guard-части `remove_de`.
pub fn remove_de_decision(
    de_id: &str,
    display_name: &str,
    current_de: &str,
    force: bool,
    packages: &[String],
    aur: &[String],
) -> RemoveDeDecision {
    if !force && de_id == current_de {
        // Текст на «вы» и без служебных слов («DE», «logout»): прежде он
        // говорил покупателю «переключись» (исправлено 13.09.2026).
        return RemoveDeDecision::Refused(trf(
            "You can't remove your current desktop (%1): after you log out \
             there would be nothing to log in to. Switch to another installed \
             desktop first.",
            &[&display_name],
        ));
    }
    if packages.is_empty() && aur.is_empty() {
        return RemoveDeDecision::Refused(
            tr("No packages for this distribution"),
        );
    }
    let mut all = packages.to_vec();
    all.extend(aur.iter().cloned());
    RemoveDeDecision::Proceed(all)
}

/// Guard-часть `switch_de`: None — можно переключаться, Some — текст ошибки.
pub fn switch_de_guard(
    current_de_name: &str,
    target_name: &str,
    target_display: &str,
) -> Option<String> {
    if current_de_name == "unknown" {
        return Some(tr("Couldn't detect your current desktop"));
    }
    if current_de_name == target_name {
        return Some(trf("Already in use: %1", &[&target_display]));
    }
    None
}

/// Команды завершения сессии — порт `logout_session` (выбор по окружению;
/// исполнитель пробует по порядку до первой успешной, timeout 5 c у каждой).
pub fn logout_cmds(
    xdg_current_desktop: &str,
    desktop_session: &str,
    xdg_session_id: &str,
) -> Vec<Cmd> {
    let de = xdg_current_desktop.to_lowercase();
    let session_env = desktop_session.to_lowercase();
    let mut cmds: Vec<Vec<&str>> = Vec::new();
    if de.contains("kde") || de.contains("plasma") || session_env.contains("plasma") {
        cmds.extend([
            vec!["dbus-send", "--session", "--type=method_call",
                 "--dest=org.kde.LogoutPrompt", "/LogoutPrompt",
                 "org.kde.LogoutPrompt.promptLogout"],
            vec!["dbus-send", "--session", "--type=method_call",
                 "--dest=org.kde.Shutdown", "/Shutdown",
                 "org.kde.Shutdown.logout"],
            vec!["dbus-send", "--session", "--type=method_call",
                 "--dest=org.kde.ksmserver", "/KSMServer",
                 "org.kde.KSMServerInterface.logout",
                 "int32:0", "int32:3", "int32:0"],
            vec!["qdbus6", "org.kde.Shutdown", "/Shutdown", "logout"],
        ]);
    } else if de.contains("xfce") || session_env.contains("xfce") {
        cmds.push(vec!["xfce4-session-logout", "--logout"]);
    } else if de.contains("mate") || session_env.contains("mate") {
        cmds.push(vec!["mate-session-save", "--logout"]);
    } else if de.contains("cinnamon") || session_env.contains("cinnamon") {
        cmds.push(vec!["cinnamon-session-quit", "--logout", "--no-prompt"]);
    } else if de.split(':').any(|t| t == "dde" || t == "deepin")
        || session_env.contains("deepin")
    {
        // Deepin v23/v25: выход без вопроса — `Logout` менеджера сеанса
        // dde-session (`RequestLogout` показал бы окно выбора).
        cmds.push(vec!["dbus-send", "--session", "--type=method_call",
                       "--dest=org.deepin.dde.SessionManager1",
                       "/org/deepin/dde/SessionManager1",
                       "org.deepin.dde.SessionManager1.Logout"]);
    } else if de.contains("gnome")
        || session_env.contains("gnome")
        // Сеанс Pantheon ведёт gnome-session (`Exec=gnome-session
        // --session=pantheon`), он же и выходит.
        || de.contains("pantheon")
        || session_env.contains("pantheon")
    {
        cmds.push(vec!["gnome-session-quit", "--logout", "--no-prompt"]);
    } else if de.contains("cosmic") {
        cmds.push(vec!["cosmic-session", "--logout"]);
    } else if de.contains("hyprland") {
        cmds.push(vec!["hyprctl", "dispatch", "exit"]);
    } else if de.contains("lxqt") || session_env.contains("lxqt") {
        cmds.push(vec!["lxqt-leave", "--logout"]);
    }
    let mut out: Vec<Cmd> = cmds
        .into_iter()
        .map(|argv| Cmd::new(argv.into_iter().map(str::to_string).collect(),
                             Some(5), false))
        .collect();
    if !xdg_session_id.is_empty() {
        out.push(Cmd::new(
            vec!["loginctl".into(), "terminate-session".into(),
                 xdg_session_id.to_string()],
            Some(5), false,
        ));
    }
    out
}

// ============================================================================
// Polkit-слой: named-действия org.rexon.*, тексты helper-скриптов и политик
// ============================================================================

pub const DE_INSTALL_ACTION: &str = "org.rexon.de-install";
pub const DE_HELPER_PATH: &str = "/usr/local/lib/rexon-de-helper";
pub const DE_POLICY_PATH: &str =
    "/usr/share/polkit-1/actions/org.rexon.de-install.policy";
pub const REXON_POLKIT_RULES_PATH: &str =
    "/etc/polkit-1/rules.d/49-rexon-nopasswd.rules";
pub const PKG_ACTION: &str = "org.rexon.pkg";
pub const PKG_HELPER_PATH: &str = "/usr/local/lib/rexon-pkg-helper";
pub const PKG_POLICY_PATH: &str =
    "/usr/share/polkit-1/actions/org.rexon.pkg.policy";
pub const STAGE_ACTION: &str = "org.rexon.stage";
pub const STAGE_HELPER_PATH: &str = "/usr/local/lib/rexon-stage-helper";
pub const STAGE_POLICY_PATH: &str =
    "/usr/share/polkit-1/actions/org.rexon.stage.policy";

// ── Сеанс прав окна («один пароль на запуск», вариант «Б», пункт Б11) ──
//
// Действие помощника [`crate::privilege`]: окно поднимает его одним
// `pkexec` на сеанс. ⛔ `auth_admin` БЕЗ `_keep` — пароль держит сам
// помощник (он живёт, пока открыто окно), а не polkit: пятиминутное окно
// polkit отдало бы второй сеанс любому процессу Rexon без пароля. Действие
// ⛔ не стоит в [`PASSWORDLESS_ACTIONS`], ⛔ не входит в чужой `imply` и
// ⛔ само ничего не `imply`: тихой цепочки к сеансу нет ни с одной стороны.
// Сторож — `session_action_tests`.
pub const SESSION_ACTION: &str = "org.rexon.session";
/// Имя помощника сеанса — одно с клиентом ([`crate::privilege::client::HELPER_NAME`]).
pub const SESSION_HELPER_NAME: &str = crate::privilege::client::HELPER_NAME;
/// Путь помощника сеанса у РУЧНОЙ установки (пакет кладёт его в
/// [`HELPER_DIR_PACKAGE`]; клиент ищет по [`HELPER_DIRS`] и проверяет
/// владение сам).
pub const SESSION_HELPER_PATH: &str = "/usr/local/lib/rexon-priv-helper";
pub const SESSION_POLICY_PATH: &str =
    "/usr/share/polkit-1/actions/org.rexon.session.policy";

// ── Где физически лежат помощники: ОДНА правда на две раскладки ──────
//
// Раскладок две, и обе живые:
//   * пакет дистрибутива кладёт помощников в [`HELPER_DIR_PACKAGE`]
//     (`/usr/lib`) — нормы Arch запрещают пакету писать в `/usr/local`, а
//     файлы вне базы pacman не снял бы `pacman -R`;
//   * ручная установка (`installer_exec`) кладёт их в
//     [`HELPER_DIR_SYSTEM`] (`/usr/local`), как раньше.
//
// ⚠️ Дефект, который это закрывает: путь был прибит к `/usr/local/lib`
// одной строкой, помощников в пакете не было вовсе — и у человека с
// пакетом КАЖДАЯ системная операция спрашивала пароль сырым `pkexec`
// (вариант «один пароль за сессию» не включался никогда).
//
// Правда одна: имя помощника + список каталогов. Константы
// `*_HELPER_PATH` ниже остаются путями РУЧНОЙ установки — по ним
// установщик кладёт файлы и по ним же пишет `exec.path` своей политики.
// Куда смотреть при запуске, отвечают [`de_helper_path`],
// [`pkg_helper_path`] и [`stage_helper_path`].

/// Имя помощника установки рабочих столов.
pub const DE_HELPER_NAME: &str = "rexon-de-helper";
/// Имя помощника операций пакетного менеджера.
pub const PKG_HELPER_NAME: &str = "rexon-pkg-helper";
/// Имя помощника раскладки файлов в `/usr/share`.
pub const STAGE_HELPER_NAME: &str = "rexon-stage-helper";

/// Каталог помощников у пакета дистрибутива.
pub const HELPER_DIR_PACKAGE: &str = "/usr/lib";
/// Каталог помощников у ручной установки в систему.
pub const HELPER_DIR_SYSTEM: &str = "/usr/local/lib";

/// Каталоги помощников по убыванию предпочтения: пакет раньше ручной
/// установки — его файлы разложил пакетный менеджер, и его же политика
/// лежит в `/usr/share/polkit-1/actions`.
pub const HELPER_DIRS: [&str; 2] = [HELPER_DIR_PACKAGE, HELPER_DIR_SYSTEM];

/// Полный путь помощника в заданном каталоге.
pub fn helper_path_in(dir: &str, name: &str) -> String {
    format!("{}/{name}", dir.trim_end_matches('/'))
}

/// Все места, где помощник может лежать, по убыванию предпочтения.
pub fn helper_candidates(name: &str) -> Vec<String> {
    HELPER_DIRS.iter().map(|d| helper_path_in(d, name)).collect()
}

/// Выбрать путь помощника пробой файловой системы: первый исполняемый
/// кандидат, а если ни одного нет — путь ручной установки (чтобы путь в
/// сообщениях и журнале оставался определённым).
///
/// Проба приходит снаружи: решение остаётся чистым и проверяемым.
pub fn pick_helper_path(name: &str, is_exec: &dyn Fn(&str) -> bool) -> String {
    helper_candidates(name)
        .into_iter()
        .find(|p| is_exec(p))
        .unwrap_or_else(|| helper_path_in(HELPER_DIR_SYSTEM, name))
}

/// Живой ответ по этой машине, посчитанный один раз за процесс.
fn helper_path_live(slot: &'static std::sync::OnceLock<String>, name: &str) -> &'static str {
    slot.get_or_init(|| pick_helper_path(name, &|p| is_exec_file(p)))
}

/// Существующий файл с битом запуска. Наружу — чтобы проверка «помощник
/// развёрнут» спрашивала ровно то же, что и выбор пути.
pub fn is_exec_file(path: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

/// Где на этой машине лежит помощник установки рабочих столов.
pub fn de_helper_path() -> &'static str {
    static SLOT: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    helper_path_live(&SLOT, DE_HELPER_NAME)
}

/// Где на этой машине лежит помощник пакетного менеджера.
pub fn pkg_helper_path() -> &'static str {
    static SLOT: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    helper_path_live(&SLOT, PKG_HELPER_NAME)
}

/// Где на этой машине лежит помощник раскладки файлов.
pub fn stage_helper_path() -> &'static str {
    static SLOT: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    helper_path_live(&SLOT, STAGE_HELPER_NAME)
}

/// Переписать в эталонном тексте путь помощника на тот, который выбирает
/// ЭТА машина.
///
/// ⚠️ Зачем. Каталога у помощников два, и оба законны ([`HELPER_DIRS`]):
/// пакет дистрибутива кладёт их в `/usr/lib`, ручная установка — в
/// `/usr/local/lib`, а [`pick_helper_path`] выбирает пробой файловой
/// системы. Эталоны golden-сверок сняты с живого Python на машине, где
/// пакета не было, и держат `/usr/local/lib`. Стоило поставить свой же
/// пакет — и 16 сверок покраснели разом (18.09.2026), причём не на
/// дефекте, а на состоянии машины: зелёные до установки, красные после.
/// Ровно такой мины и не должно быть — сверка, зависящая от живой
/// машины, обязана либо не зависеть, либо быть помечена
/// `isolated-checks` (уговор в `rust/.cargo/config.toml`).
///
/// Что пин при этом проверяет, не меняется: «идём ИМЕНОВАННЫМ помощником,
/// а не сырым `pkexec`, и ровно с этими аргументами». Сырой откат
/// (`pkexec pacman -S …`) и любая правка набора аргументов по-прежнему
/// валят сверку — подменяется ТОЛЬКО каталог помощника, и только на
/// второй законный. На машине без пакета замена вообще тождественна:
/// живой ответ и есть `/usr/local/lib`.
pub fn retarget_helper_paths(text: &str) -> String {
    let mut out = text.to_string();
    for (name, live) in [
        (DE_HELPER_NAME, de_helper_path()),
        (PKG_HELPER_NAME, pkg_helper_path()),
        (STAGE_HELPER_NAME, stage_helper_path()),
    ] {
        let manual = helper_path_in(HELPER_DIR_SYSTEM, name);
        if manual != live {
            out = out.replace(&manual, live);
        }
        out = insert_live_lang_arg(&out, live);
    }
    out
}

/// Вставить за путём помощника в `pkexec`-команде эталона (JSON-массив
/// `"pkexec", "<помощник>", …`) аргумент языка, который ЭТА машина
/// передала бы при действующем языке интерфейса
/// ([`crate::system_l10n::lang_args`]).
///
/// ⚠️ Зачем. `--lang=<код>` идёт помощнику, только если язык не английский
/// И в установленном файле помощника есть отметка «понимаю `--lang`». Эталоны
/// сняты там, где такого помощника не было, — и стоило поставить свой пакет
/// с новыми помощниками, как сверки команд покраснели на состоянии машины,
/// а не на дефекте (04.10.2026; тот же класс, что с каталогом помощника
/// выше). Пин по-прежнему ловит сырой `pkexec` и любую правку остальных
/// аргументов; подставляется ТОЛЬКО то, что зависит от машины.
///
/// Язык берётся действующий: сверка обязана закрепить его (`i18n_pin`) ДО
/// разбора эталона.
fn insert_live_lang_arg(text: &str, live: &str) -> String {
    let args = crate::system_l10n::lang_args(live);
    if args.is_empty() {
        return text.to_string();
    }
    let needle = format!("\"{live}\",");
    let extra: String = args.iter().map(|a| format!(" \"{a}\",")).collect();
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(&needle) {
        let (head, tail) = rest.split_at(at + needle.len());
        out.push_str(head);
        if head[..at].trim_end().ends_with("\"pkexec\",") {
            out.push_str(&extra);
        }
        rest = tail;
    }
    out.push_str(rest);
    out
}

/// Семейства, для которых DE-helper умеет ставить из официальных репо без AUR.
pub const DE_HELPER_FAMILIES: &[&str] = &[
    "arch", "debian", "fedora", "suse", "alpine", "void", "solus", "clear", "alt",
];

/// install-команда DE-helper'а per family ({PKGS} подставляет сам helper).
const DE_HELPER_PKGCMD: &[(&str, &str)] = &[
    ("arch", "pacman -S --needed --noconfirm --noprogressbar --color never {PKGS}"),
    ("debian", "apt-get install -y --no-install-recommends {PKGS}"),
    ("fedora", "dnf install -y {PKGS}"),
    ("suse", "zypper install -y --no-recommends {PKGS}"),
    ("alpine", "apk add --no-progress {PKGS}"),
    ("void", "xbps-install -y {PKGS}"),
    ("solus", "eopkg it -y {PKGS}"),
    ("clear", "swupd bundle-add {PKGS}"),
    ("alt", "apt-get install -y {PKGS}"),
];

/// update-команда DE-helper'а per family.
const DE_HELPER_UPDCMD: &[(&str, &str)] = &[
    ("arch", "pacman -Sy --noconfirm --noprogressbar --color never"),
    ("debian", "apt-get update -q"),
    ("fedora", "dnf makecache -q"),
    ("suse", "zypper refresh -q"),
    ("alpine", "apk update"),
    ("void", "xbps-install -S"),
    ("solus", "eopkg ur"),
    ("clear", "swupd update --download"),
    ("alt", "apt-get update -q"),
];

/// DM'ы, которые helper'ы умеют включать — порт `_DE_HELPER_DISPLAY_MANAGERS`.
pub const DE_HELPER_DISPLAY_MANAGERS: &[&str] = &[
    "plasmalogin", "cosmic-greeter", "lightdm", "gdm", "sddm",
];

/// Операции rexon-pkg-helper — порт `_PKG_HELPER_OPS`.
///
/// `build-deps` и `install-built` — шаги пересборки по рецепту Arch
/// ([`crate::repo_rebuild`]); их argv строит она сама, [`privileged_pkg_cmd`]
/// их не собирает.
pub const PKG_HELPER_OPS: &[&str] = &[
    "install", "remove", "update", "enable-dm", "switch-dm", "build-deps", "install-built",
];

/// Семейства rexon-pkg-helper (NIX исключён намеренно) — порт
/// `_PKG_HELPER_FAMILIES`.
pub const PKG_HELPER_FAMILIES: &[&str] = &[
    "arch", "debian", "fedora", "suse", "gentoo",
    "alpine", "void", "solus", "clear", "alt",
];

const PKG_HELPER_INSTALL: &[(&str, &str)] = &[
    ("arch", "pacman -S --needed --noconfirm {PKGS}"),
    ("debian", "apt-get install -y --no-install-recommends {PKGS}"),
    ("fedora", "dnf install -y {PKGS}"),
    ("suse", "zypper install -y --no-recommends {PKGS}"),
    ("gentoo", "emerge --ask=n --quiet {PKGS}"),
    ("alpine", "apk add --no-progress {PKGS}"),
    ("void", "xbps-install -y {PKGS}"),
    ("solus", "eopkg it -y {PKGS}"),
    ("clear", "swupd bundle-add {PKGS}"),
    ("alt", "apt-get install -y {PKGS}"),
];
const PKG_HELPER_REMOVE: &[(&str, &str)] = &[
    ("arch", "pacman -Rns --noconfirm {PKGS}"),
    ("debian", "apt-get remove -y --purge {PKGS}"),
    ("fedora", "dnf remove -y {PKGS}"),
    ("suse", "zypper remove -y {PKGS}"),
    ("gentoo", "emerge --unmerge {PKGS}"),
    ("alpine", "apk del {PKGS}"),
    ("void", "xbps-remove -y {PKGS}"),
    ("solus", "eopkg rm -y {PKGS}"),
    ("clear", "swupd bundle-remove {PKGS}"),
    ("alt", "apt-get remove -y {PKGS}"),
];
const PKG_HELPER_UPDATE: &[(&str, &str)] = &[
    ("arch", "pacman -Sy --noconfirm"),
    ("debian", "apt-get update -q"),
    ("fedora", "dnf makecache -q"),
    ("suse", "zypper refresh -q"),
    ("gentoo", "emerge --sync --quiet"),
    ("alpine", "apk update"),
    ("void", "xbps-install -S"),
    ("solus", "eopkg ur"),
    ("clear", "swupd update --download"),
    ("alt", "apt-get update -q"),
];

/// Категории file-staging — порт `_STAGE_CATEGORIES`.
pub const STAGE_CATEGORIES: &[&str] = &[
    "gdm-bg", "lightdm-bg", "cosmic-greeter", "login-bg", "lightdm-webkit", "mate-layouts",
];

/// Корневые каталоги-назначения dir-категорий — порт `_STAGE_DIR_ROOT`.
///
/// ⚠️ Правило #16: путь тем webkit-приветствия был неверен ДАЖЕ на Arch —
/// пакет `lightdm-webkit2-greeter` кладёт темы в `/usr/share/lightdm-webkit`,
/// каталога `lightdm-webkit2-greeter` не существует нигде. Тема уезжала в
/// несуществующий каталог, а пользователь получал «установлено».
/// Преемник greeter'а (`web-greeter`) держит темы у себя, поэтому корней
/// два и выбирается тот, что реально есть в системе.
pub const STAGE_DIR_ROOT: &[(&str, &str)] = &[
    ("lightdm-webkit", "/usr/share/lightdm-webkit/themes"),
    ("mate-layouts", "/usr/share/mate-panel/layouts"),
];

/// Второй корень тем webkit-приветствия — преемник `web-greeter`.
pub const WEB_GREETER_ROOT: &str = "/usr/share/web-greeter/themes";

/// Все 8 layout'ов Ubuntu MATE — порт `MATE_LAYOUTS`.
pub const MATE_LAYOUTS: &[&str] = &[
    "familiar", "redmond", "cupertino", "mutiny",
    "pantheon", "contemporary", "netbook", "traditional",
];

// ── Имя пакета — ИМЯ, а не путь: ОДНА проверка на все пути ───────────
//
// ⛔ ДЫРА, ЗАКРЫТАЯ 25.09.2026. Прежние две проверки (`is_safe_pkg_name`
// здесь и `is_safe_helper_arg` ниже) разрешали в имени пакета символ `/`:
//
//     s.chars().all(|c| c.is_ascii_alphanumeric()
//                       || matches!(c, '@'|'.'|'_'|'+'|':'|'/'|'-'))
//
// а привилегированный помощник собирал команду через `eval`. Вместе это
// значило: `pkexec rexon-pkg-helper debian install /tmp/evil.deb` —
// apt-get (и точно так же dnf, zypper) ставит ЛОКАЛЬНЫЙ ФАЙЛ, а его
// сценарии установки идут ОТ ROOT. Тот же `/` пролезал и в сырой откат
// (`pkexec apt-get install …`), поэтому проверка стоит в самих таблицах
// команд ([`install_cmd`], [`remove_cmd`]), а не только перед помощником.
//
// Правда одна и здесь: атом имени пакета. Путь в него не складывается ни
// одним способом, а место, где путь ДЕЙСТВИТЕЛЬНО нужен (раскладка файлов
// оформления), — это отдельное действие `org.rexon.stage` со своей
// проверкой категории и корня, а не поблажка в общем имени.

/// Атом имени пакета: начинается с буквы или цифры, дальше — только
/// `A-Za-z0-9._+:-`. Ни `/` (путь), ни ведущего `-` (подмена пакета на
/// флаг), ни ведущей точки, ни `..`, ни хвоста файла пакета.
fn is_pkg_atom(s: &str) -> bool {
    match s.chars().next() {
        Some(c) if c.is_ascii_alphanumeric() => {}
        _ => return false,
    }
    !s.contains("..")
        && !looks_like_package_file(s)
        && s.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '+' | ':' | '-')
        })
}

/// Хвост файла пакета или архива. Нужно ОТДЕЛЬНО от запрета `/`: `dnf
/// install evil.rpm` и `zypper install evil.rpm` берут файл из ТЕКУЩЕГО
/// каталога, а его помощнику задаёт вызывающий процесс (pkexec рабочий
/// каталог не меняет) — слэша в таком аргументе нет вовсе.
fn looks_like_package_file(s: &str) -> bool {
    const EXT: &[&str] = &[
        ".deb", ".rpm", ".apk", ".eopkg", ".xbps", ".tar", ".tgz", ".txz",
        ".zst", ".xz", ".gz", ".bz2", ".lz4", ".snap", ".flatpakref",
    ];
    let low = s.to_ascii_lowercase();
    EXT.iter().any(|e| low.ends_with(e))
}

/// Допустимое имя пакета для привилегированной операции — с поправкой на
/// семейство, и ровно на две:
///   * Fedora: ведущее `@` — группа dnf (`@gnome-desktop`), и это
///     единственное законное `@` во всём проекте;
///   * Gentoo: атом Portage `категория/имя` — РОВНО один `/`, обе половины
///     сами атомы. Ни ведущего `/` (абсолютный путь), ни `@` (наборы вида
///     `@world`, которым `emerge --unmerge` снёс бы систему).
/// Остальным семействам `/` не нужен нигде и запрещён.
pub fn is_safe_pkg_arg(family: PkgFamily, s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let body = if family == PkgFamily::Fedora {
        s.strip_prefix('@').unwrap_or(s)
    } else {
        s
    };
    if family == PkgFamily::Gentoo {
        if let Some((category, name)) = body.split_once('/') {
            return !name.contains('/') && is_pkg_atom(category) && is_pkg_atom(name);
        }
    }
    is_pkg_atom(body)
}

/// Текст отказа снять жизненно важный пакет ([`is_vital_package`]) — один
/// на все пути снятия.
pub fn vital_remove_error(name: &str) -> String {
    trf("Rexon does not remove %1: the system needs it to work", &[&name])
}

/// Текст отказа для имени, которое не имя: один на все пути, чтобы человек
/// видел одну и ту же причину и в плане, и в журнале.
fn bad_pkg_name_error(name: &str) -> String {
    trf(
        "Invalid package name: %1. A package name is expected, not a file path.",
        &[&name],
    )
}

/// Жизненно важные пакеты: удалять их помощник не станет никогда.
///
/// ⛔ Вторая половина той же дыры: `remove` проверял имя ТОЛЬКО на символы,
/// а идёт оно от root и с «не спрашивай». Одной строки
/// `pkexec rexon-pkg-helper arch remove linux` хватало, чтобы оставить
/// машину без загрузки, а `… remove polkit` — чтобы отобрать у человека
/// сам механизм подтверждения. Ни одного из этих имён нет ни в одном
/// рабочем столе базы Rexon (сверка
/// `вечный_список_не_пересекается_с_рабочими_столами`),
/// поэтому запрет не мешает ни одному честному сценарию.
pub const PKG_HELPER_VITAL: &[&str] = &[
    "base", "base-files", "filesystem", "glibc", "gcc-libs", "bash",
    "coreutils", "util-linux", "systemd", "systemd-libs",
    "systemd-sysvcompat", "dbus", "dbus-broker",
    "linux", "linux-lts", "linux-zen", "linux-hardened", "linux-firmware",
    "kernel", "kernel-core", "grub", "limine", "systemd-boot",
    "pacman", "apt", "dpkg", "rpm", "dnf", "yum", "zypper", "apk-tools",
    "xbps", "eopkg", "swupd-client", "portage", "emerge",
    "polkit", "polkit-qt5", "sudo", "openssl", "zlib", "rexon",
];

/// Добавка к [`PKG_HELPER_VITAL`] — семейства, которые точными именами не
/// перечислить: ядра (`linux-cachyos`, `linux-image-…`, `kernel-modules`),
/// драйверы видео (`nvidia-…`, `mesa`, `vulkan-…`, `xf86-video-…`),
/// прошивки и микрокод, сборщики initramfs.
///
/// ⛔ Дыра, которую это закрыло (28.10.2026): вечный список знал только
/// точные имена — `linux`, `linux-lts`, `linux-zen`, `linux-hardened`, — и
/// `rexon-pkg-helper arch remove linux-cachyos` (а равно `nvidia-utils`,
/// `mesa`, `amd-ucode`, `linux-image-amd64`) проходил. Ни одно имя таблиц
/// столов сюда не попадает (сверка `вечное_не_задевает_столы`), поэтому
/// удалению стола добавка не мешает.
pub const VITAL_PREFIXES: &[&str] = &[
    "linux-", "kernel-", "gentoo-kernel", "gentoo-sources", "systemd-", "glibc-",
    "grub-", "sudo-", "opendoas", "polkit-qt", "pacman-", "nvidia", "lib32-nvidia",
    "mesa", "lib32-mesa", "vulkan-", "lib32-vulkan-", "xf86-video-", "firmware-",
    "mkinitcpio", "dracut", "booster",
];
/// Хвосты имён жизненно важного: прошивки, микрокод, модули DKMS.
pub const VITAL_SUFFIXES: &[&str] = &["-firmware", "-ucode", "-dkms"];

/// Имя для сверки с вечным списком: без группы dnf (`@`) и без категории
/// Gentoo (`sys-kernel/gentoo-kernel` → `gentoo-kernel`) — как `vital_key`
/// в `rexon-pkg-helper`.
pub fn vital_key(name: &str) -> &str {
    let n = name.strip_prefix('@').unwrap_or(name);
    n.rsplit('/').next().unwrap_or(n)
}

/// Жизненно важный пакет: из [`PKG_HELPER_VITAL`] или из семейств ядра,
/// драйверов, прошивок и загрузки ([`VITAL_PREFIXES`], [`VITAL_SUFFIXES`]).
/// ОДНО правило на все пути снятия: помощник пакетов (тот же список в его
/// bash-тексте), план удаления [`remove_plan`], [`remove_cmd`], снятие
/// пакетов «Как в дистрибутиве» и проверки помощника «Б».
pub fn is_vital_package(name: &str) -> bool {
    let key = vital_key(name);
    PKG_HELPER_VITAL.contains(&key)
        || VITAL_PREFIXES.iter().any(|p| key.starts_with(p))
        || VITAL_SUFFIXES.iter().any(|s| key.ends_with(s))
}

/// То же правило шаблоном `case` для bash-помощника: точные имена,
/// `префикс*`, `*хвост`. Имена — из констант ядра (буквы, цифры, `-`), ни
/// одного символа шаблона в них нет (сверка в тестах).
fn vital_case_pattern() -> String {
    PKG_HELPER_VITAL
        .iter()
        .map(|v| v.to_string())
        .chain(VITAL_PREFIXES.iter().map(|p| format!("{p}*")))
        .chain(VITAL_SUFFIXES.iter().map(|s| format!("*{s}")))
        .collect::<Vec<_>>()
        .join("|")
}

/// Белый список операции `install` помощника пакетов для семейства: все
/// таблицы ядра, по которым Rexon ставит пакеты через помощник, — столы
/// (основные и AUR), экраны входа ([`DM_PAIRS`], [`SDDM_PKG_FOR_FAMILY`]),
/// починка соответствия, видеозависимости обоев, yt-dlp, инструмент
/// overlay Gentoo, оболочки, программы виджетов Conky и распаковщик 7z
/// (`utils::TOOL_PACKAGES`), Conky, части раскладок MATE, значки и курсоры
/// тем, образы «Как в дистрибутиве». Вшивается в текст помощника
/// ([`pkg_helper_script`]): имя вне списка помощник от root не ставит.
///
/// ⛔ Дыра, которую это закрыло (28.10.2026): `install` принимал любое имя
/// пакета, и любой процесс сеанса с паролем администратора на 5 минут
/// (`auth_admin_keep`) мог поставить что угодно из подключённых
/// репозиториев — со сценариями установки от root.
///
/// ⚠️ Новое место, которое ставит пакеты через помощник, обязано брать
/// имена из таблицы и добавить её сюда — иначе помощник откажет. Сверка
/// `белый_список_покрывает_все_таблицы` держит перечень.
pub fn pkg_helper_install_allowlist(family: PkgFamily) -> Vec<String> {
    let fam = family.as_str();
    let mut out: Vec<String> = Vec::new();
    let mut add = |p: &str| {
        if is_safe_pkg_arg(family, p) && !out.iter().any(|o| o == p) {
            out.push(p.to_string());
        }
    };
    for (de_id, _) in crate::system::DE_DATABASE {
        for p in crate::system::desktop_packages(de_id, family) {
            add(&p);
        }
    }
    for (p, _) in DM_PAIRS {
        add(p);
    }
    if let Some(p) = table_get(SDDM_PKG_FOR_FAMILY, fam) {
        add(p);
    }
    for list in [
        crate::compliance::AYATANA_PACKAGES,
        crate::compliance::MATE_AUR_PACKAGES,
        crate::compliance::FIX_PACKAGES,
    ] {
        for p in list {
            add(p);
        }
    }
    for session in ["wayland", "x11"] {
        for p in crate::wallpaper_engine::install_deps_packages(fam, session) {
            add(&p);
        }
    }
    add(crate::youtube_wallpaper::YTDLP_PACKAGE);
    if family == PkgFamily::Gentoo {
        add(GENTOO_OVERLAY_TOOL_PACKAGE);
    }
    for p in crate::shell_bundle::all_packages(family) {
        add(p);
    }
    for p in crate::utils::tool_packages_for(fam) {
        add(p);
    }
    if let Some(p) = crate::de_apply::conky_package(fam) {
        add(p);
    }
    for p in crate::mate_panel::all_part_packages(fam) {
        add(p);
    }
    for p in crate::themes::known_set_packages(fam) {
        add(p);
    }
    for p in crate::distro_looks::builtin_package_names(family) {
        add(&p);
    }
    out
}

/// Ветки `case "$family"` белого списка установки для текста помощника.
fn install_allow_cases() -> String {
    PKG_HELPER_FAMILIES
        .iter()
        .filter_map(|f| PkgFamily::from_value(f))
        .map(|fam| {
            format!(
                "        {}) list=\"{}\" ;;",
                fam.as_str(),
                pkg_helper_install_allowlist(fam).join(" ")
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Пакеты столов для защиты текущего стола в помощнике: `"<de>|<семейство>")
/// echo …` — основные и AUR по таблице ядра.
fn desktop_protect_cases() -> String {
    let mut lines: Vec<String> = Vec::new();
    for (de_id, _) in crate::system::DE_DATABASE {
        if !is_safe_de_id(de_id) {
            continue;
        }
        for f in PKG_HELPER_FAMILIES {
            let Some(fam) = PkgFamily::from_value(f) else { continue };
            let pkgs: Vec<String> = crate::system::desktop_packages(de_id, fam)
                .into_iter()
                .filter(|p| is_safe_pkg_arg(fam, p))
                .collect();
            if !pkgs.is_empty() {
                lines.push(format!("        \"{de_id}|{f}\") echo {} ;;", pkgs.join(" ")));
            }
        }
    }
    lines.join("\n")
}

/// Стол по `Desktop=` сеанса logind — та же таблица, что у
/// [`crate::system::detect_current_de`]: точный токен, затем подстрока.
fn desktop_token_cases() -> (String, String) {
    let table = crate::system::XDG_TOKEN_TO_DE;
    let exact = table
        .iter()
        .map(|(k, de)| format!("            {k}) printf '%s' {de}; return 0 ;;"))
        .collect::<Vec<_>>()
        .join("\n");
    let substr = table
        .iter()
        .map(|(k, de)| format!("        *{k}*) printf '%s' {de}; return 0 ;;"))
        .collect::<Vec<_>>()
        .join("\n");
    (exact, substr)
}

/// Пакет экрана входа по юниту для помощника (`семейство|юнит`): Debian —
/// [`DM_DEBIAN_PKG_FOR_UNIT`], остальные — [`DM_PKG_FOR_UNIT`].
fn dm_protect_map() -> String {
    DM_DEBIAN_PKG_FOR_UNIT
        .iter()
        .map(|(u, p)| format!("        debian\\|{u}) printf '%s\\n' {p} ;;"))
        .chain(DM_PKG_FOR_UNIT.iter().map(|(u, p)| format!("        *\\|{u}) printf '%s\\n' {p} ;;")))
        .collect::<Vec<_>>()
        .join("\n")
}

fn is_safe_de_id(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// Действия, которые по СОГЛАСИЮ человека идут без повторного пароля
/// (правило [`passwordless_rules_js`], ставит его установка Rexon в систему
/// только при включённом согласии).
///
/// ⭐⭐ Починка 15.09.2026: прежнее правило (`rexon_polkit_rules_js`, удалено
/// 25.09.2026 вместе с генератором — см. ниже) молча отдавало ВСЁ
/// семейство `org.rexon.*`, включая `org.rexon.pkg` — установку
/// и УДАЛЕНИЕ любых пакетов по имени. Без пароля это мог бы сделать любой
/// процесс сеанса (`pkexec rexon-pkg-helper arch remove linux`). Теперь
/// разрешено одно действие, которое не принимает произвольного ввода:
///   * `org.rexon.de-install` — установка рабочего стола только из таблицы
///     ядра (набор пакетов зашит в помощника, имя пакета не принимается).
///
/// ⛔ 28.09.2026 (решение Wehrwolfmann по плану «один пароль»): из правила
/// сняты `org.rexon.system-install` (темы SDDM и Plymouth) и
/// `org.rexon.stage` (фон и темы экранов входа, раскладки MATE). Оба берут
/// СОДЕРЖИМОЕ из каталога пользователя и кладут его в `/usr/share`, а тема
/// Plymouth грузится от root при загрузке: строку `ModuleName` plymouth
/// превращает в путь модуля без проверки `../`, и тема с `.so` внутри была
/// бы кодом от root — без пароля, из любого процесса сеанса. Помощник тем
/// такое теперь отвергает, но «без пароля» для этих действий не вернётся:
/// после перехода на «один пароль на запуск» правило снимается целиком.
///
/// `org.rexon.pkg` сюда НЕ входит никогда и спрашивает пароль как обычно
/// (`auth_admin_keep` — пять минут на процесс окна). Он же — «зонтик»
/// ([`PKG_IMPLIES`]): направление `imply` только от него к остальным.
///
/// ⛔ 25.09.2026 генератор прежнего правила удалён совсем. Он оставался
/// мёртвым кодом (Rust его не звал, Python писал в файл, который ядро при
/// отсутствии согласия тут же снимало `rm -f`), но лежал готовой миной:
/// один вызов — и `org.rexon.pkg` уходит без пароля. Восстанавливать его
/// нельзя ни в каком виде: правило «без пароля» знает ТОЛЬКО список
/// [`PASSWORDLESS_ACTIONS`].
pub const PASSWORDLESS_ACTIONS: &[&str] = &[DE_INSTALL_ACTION];

/// Группы администраторов, которым polkit дистрибутива разрешает действия
/// администратора (`addAdminRule` / `AdminIdentities`).
///
/// ⭐ Починка 15.09.2026: правило знало только `wheel`, а в Debian и Ubuntu
/// администраторы состоят в группе `sudo` (в старых Ubuntu — ещё `admin`),
/// в Solus — тоже `sudo`. Там правило не срабатывало никогда. Незнакомое
/// семейство получает `wheel` — лишний раз спросить пароль безопаснее, чем
/// угадать чужую группу.
pub fn polkit_admin_groups(family: PkgFamily) -> &'static [&'static str] {
    match family {
        PkgFamily::Debian => &["sudo", "admin"],
        PkgFamily::Solus => &["sudo"],
        _ => &["wheel"],
    }
}

/// Правило polkit «без повторного пароля» для [`PASSWORDLESS_ACTIONS`]:
/// администратор (по [`polkit_admin_groups`] семейства) в активной
/// локальной сессии. Кладётся по [`REXON_POLKIT_RULES_PATH`] — тем же путём,
/// что прежнее, поэтому удаление Rexon его снимает.
pub fn passwordless_rules_js(family: PkgFamily) -> String {
    let actions = PASSWORDLESS_ACTIONS
        .iter()
        .map(|a| format!("\"{a}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let groups = polkit_admin_groups(family)
        .iter()
        .map(|g| format!("subject.isInGroup(\"{g}\")"))
        .collect::<Vec<_>>()
        .join(" || ");
    format!(
        "// Rexon — без повторного пароля только для перечисленных действий\n\
         // (администратор в активной локальной сессии). Поставлено с согласия\n\
         // при установке Rexon. Установка и удаление пакетов сюда не входят.\n\
         var rexonActions = [{actions}];\n\
         polkit.addRule(function(action, subject) {{\n\
         \x20   if (rexonActions.indexOf(action.id) >= 0 &&\n\
         \x20       subject.active && subject.local &&\n\
         \x20       ({groups})) {{\n\
         \x20       return polkit.Result.YES;\n\
         \x20   }}\n\
         }});\n"
    )
}

/// bash case-таблица `de_id|family → packages` — порт `_de_helper_pkg_table`.
/// ЕДИНЫЙ ИСТОЧНИК ПРАВДЫ — DE_DATABASE (см. system.rs), helper не «дрейфует»
/// от основной БД. Имена safe (allowlist) → shlex.quote в Python — no-op.
pub fn de_helper_pkg_table() -> String {
    let mut lines: Vec<String> = Vec::new();
    for (de_id, packages) in crate::system::de_database_packages() {
        if !is_safe_de_id(de_id) || !de_helper_allowed(de_id) {
            continue;
        }
        for (fam, pkgs) in packages {
            let fam_v = fam.as_str();
            if !DE_HELPER_FAMILIES.contains(&fam_v) {
                continue;
            }
            let clean: Vec<&str> = pkgs
                .iter()
                .filter(|p| is_safe_pkg_arg(*fam, p))
                .copied()
                .collect();
            if clean.is_empty() {
                continue;
            }
            lines.push(format!(
                "        \"{de_id}|{fam_v}\") echo {} ;;",
                clean.join(" ")
            ));
        }
    }
    lines.join("\n")
}

/// Столы, которых в таблице помощника нет вовсе. Deepin: группа `deepin`
/// тянет `deepin-session` с `deepin-kwin`, а Rexon ставит Deepin только на
/// Treeland — набором без них, обёрткой (`crate::deepin_treeland`). Помощник
/// от root не должен уметь поставить группу целиком.
fn de_helper_allowed(de_id: &str) -> bool {
    de_id != crate::deepin_treeland::DE_ID
}

/// Умеет ли DE-helper ставить пару de_id|family — порт `_de_helper_supports`.
pub fn de_helper_supports(de_id: &str, family: &str) -> bool {
    if !DE_HELPER_FAMILIES.contains(&family) || !de_helper_allowed(de_id) {
        return false;
    }
    if !is_safe_de_id(de_id) {
        return false;
    }
    let fam = match PkgFamily::from_value(family) {
        Some(f) => f,
        None => return false,
    };
    match de_entry_packages_for(de_id, family) {
        Some(pkgs) => pkgs.iter().any(|p| is_safe_pkg_arg(fam, p)),
        None => false,
    }
}

/// Поставит ли помощник всё из `packages`: пакеты стола он берёт из
/// встроенной таблицы ([`de_helper_pkg_table`]), а не из аргументов. Набор
/// из её же имён (весь или часть — как было всегда) он покрывает; имя не из
/// таблицы (DKMS-модуль, раскрытый член группы, AUR-пакет из репозитория)
/// помощник молча не поставил бы.
pub fn de_helper_covers(de_id: &str, family: PkgFamily, packages: &[String]) -> bool {
    let Some(table) = de_entry_packages_for(de_id, family.as_str()) else {
        return false;
    };
    packages
        .iter()
        .all(|p| table.iter().any(|t| *t == p.as_str() && is_safe_pkg_arg(family, t)))
}

/// Тело rexon-de-helper — порт `_de_helper_script` (шаблон + подстановки).
pub fn de_helper_script() -> String {
    let pkg_table = de_helper_pkg_table();
    // Команды — МАССИВАМИ аргументов: `eval` из помощника убран целиком
    // (25.09.2026, вместе с дырой в rexon-pkg-helper).
    let fam_cases = DE_HELPER_FAMILIES
        .iter()
        .map(|f| {
            let pkg = fam_cmd_case(DE_HELPER_PKGCMD, f, "    ");
            let upd = fam_cmd_case(DE_HELPER_UPDCMD, f, "    ");
            format!(
                "{}\n{}",
                pkg.replace("cmd=(", "PKG_CMD=("),
                upd.replace("cmd=(", "UPD_CMD=("),
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let dm_cases = DE_HELPER_DISPLAY_MANAGERS
        .iter()
        .map(|dm| format!("\"{dm}\""))
        .collect::<Vec<_>>()
        .join(" | ");
    let dm_pkgmap = DM_PKG_FOR_UNIT
        .iter()
        .map(|(unit, pkg)| format!("                {unit}) printf '%s' '{pkg}' ;;"))
        .collect::<Vec<_>>()
        .join("\n");
    let dm_force = DM_FORCE_ENABLE
        .iter()
        .map(|dm| format!("\"{dm}\""))
        .collect::<Vec<_>>()
        .join(" | ");
    let dm_list = DE_HELPER_DISPLAY_MANAGERS.join(" ");
    // Список известных экранов входа и «включённых» состояний — тот же, по
    // которому Rexon отвечает на вопрос «экран входа есть?»: helper обязан
    // видеть чужой DM ровно так же, иначе подменит его молча.
    let dm_known = KNOWN_DM_UNITS.join(" ");
    let dm_states = DM_ENABLED_STATES.join("|");
    let text = DE_HELPER_SCRIPT_TMPL
        .replace("__PKG_TABLE__", &pkg_table)
        .replace("__FAM_CASES__", &fam_cases)
        .replace("__DM_LIST__", &dm_list)
        .replace("__DM_PKGMAP__", &dm_pkgmap)
        .replace("__DM_FORCE__", &dm_force)
        .replace("__DM_CASES__", &dm_cases)
        .replace("__DM_KNOWN__", &dm_known)
        .replace("__DM_STATES__", &dm_states)
        .replace("__DM_SYMLINK__", DM_SYMLINK);
    crate::system_l10n::localize_script(DE_HELPER_NAME, &text)
}

/// Шаблон rexon-de-helper (текст-в-текст с Python `_de_helper_script`).
const DE_HELPER_SCRIPT_TMPL: &str = r#"#!/bin/bash
# Rexon DE helper — привилегированная установка/восстановление DE.
# Запускается ИСКЛЮЧИТЕЛЬНО через pkexec (polkit action org.rexon.de-install,
# auth_admin_keep: подтверждённый пароль держится несколько минут для окна
# Rexon). Принимает только строго валидируемые de_id + family (+ опц.
# log_path под /tmp). Имена пакетов берёт из ВСТРОЕННОЙ таблицы. НЕ выполняет произвольный код.
set -euo pipefail

# Стабильная C-локаль: вывод pacman/apt/… всегда по-английски, чтобы
# Python-парсер прогресса (_RE_SKIP «is up to date -- skipping» и т.п.)
# ловил пропуски/прогресс на ЛЮБОЙ системной локали (иначе на русской
# pacman пишет «не устарел -- пропускается» и «✓ уже установлено» не
# срабатывает).
export LC_ALL=C LANG=C

# ⚠️ Никакого ввода с клавиатуры. Пакетный менеджер задаёт вопросы сам себе
# (члены группы, выбор поставщика) и при --noconfirm отвечает на них по
# умолчанию, не читая stdin. Но если завтра в какой-то ветке --noconfirm
# потеряется, а stdin окажется живым (терминал, из которого запустили Rexon,
# или незакрытая труба), вопрос ЗАВИСНЕТ — молча, до самого таймаута в
# полчаса. Отрезаем stdin один раз здесь: тогда худшее, что может случиться,
# — быстрый отказ вместо вечного ожидания.
exec </dev/null

__L10N__

[ "$#" -ge 2 ] && [ "$#" -le 3 ] \
    || die de_args
de_id="$1"
family="$2"
log_path="${3:-}"

# de_id / family — строгий формат (allowlist символов). Реальная проверка
# валидности пары — через таблицу ниже (неизвестная пара → die).
printf '%s' "$de_id"  | grep -Eq '^[A-Za-z0-9._-]+$' || die bad_de_id "$de_id"
printf '%s' "$family" | grep -Eq '^[a-z]+$'          || die bad_family_arg "$family"

# log_path (если задан): журнал хода установки для окна Rexon. Файл создаёт
# Rexon заранее (mkstemp), и принадлежит он ВЫЗВАВШЕМУ. Поэтому и пишет его
# процесс С ПРАВАМИ ВЫЗВАВШЕГО (setpriv), а не root: какой бы файл ни
# оказался по этому пути к моменту записи, root туда не пишет и владельца
# не меняет.
#
# ⛔ Дыра, которую это закрыло (28.09.2026). Прежде: realpath, затем
# `chown root` + запись root'ом + в конце `chown $PKEXEC_UID` — по строке
# пути, через минуты установки. Каталог в пути принадлежит пользователю, и
# подменить его ссылкой (/tmp/a/b → /etc) можно было в любой момент до
# конца установки: в конце root отдавал пользователю, скажем, /etc/shadow.
# Действие org.rexon.de-install при этом стояло в правиле «без пароля» —
# молчаливый root для любого процесса сеанса.
#
# exit-маркер __REXON_EXIT__ фильтруется tail-парсером Rexon.
if [ -n "$log_path" ]; then
    case "$log_path" in
        /tmp/?*|/var/tmp/?*) : ;;
        *) die log_outside_tmp "$log_path" ;;
    esac
    log_uid="${PKEXEC_UID:-}"
    printf '%s' "$log_uid" | grep -Eq '^[0-9]+$' || die not_via_pkexec
    log_gid="$(getent passwd "$log_uid" 2>/dev/null | cut -d: -f4)"
    printf '%s' "$log_gid" | grep -Eq '^[0-9]+$' || die no_user_group "$log_uid"
    if command -v setpriv >/dev/null 2>&1; then
        exec > >(exec setpriv --reuid="$log_uid" --regid="$log_gid" --clear-groups \
                     -- tee -a -- "$log_path" >/dev/null) 2>&1
        log_pid=$!
        trap 'ec=$?; echo "__REXON_EXIT__$ec"; exec >&- 2>&-; wait "$log_pid" 2>/dev/null || true; exit "$ec"' EXIT
    else
        # Без setpriv журнал не ведём вовсе: писать чужой файл от root нельзя.
        rx_say no_setpriv >&2
    fi
fi

# Пакеты для пары de_id|family — ТОЛЬКО из встроенной таблицы (allowlist).
pkgs="$(
  case "$de_id|$family" in
__PKG_TABLE__
    *) die unknown_de_pair "$de_id|$family" ;;
  esac
)"
[ -n "$pkgs" ] || die empty_pkg_list "$de_id|$family"

# Команды менеджера пакетов по семейству (--needed = restore/repair:
# установленное пропускается).
case "$family" in
__FAM_CASES__
    *) die unsupported_family "$family" ;;
esac

# ⛔ Никакого `eval`: команды — МАССИВЫ аргументов, список пакетов
# разбивается на слова РОВНО один раз и оболочке на разбор не отдаётся.
# Прежнее `eval "${PKG_CMD/'{PKGS}'/$pkgs}"` держало дверь открытой: стоило
# в таблицу попасть имени с метасимволом, и это была команда, а не пакет.

# 1) Обновление БД (иначе --needed может упасть на устаревшей БД).
"${UPD_CMD[@]}" || die db_update_failed

# 2) Установка пакетов DE (имена — только из встроенной таблицы выше).
read -r -a pkg_list <<<"$pkgs"
[ "${#pkg_list[@]}" -gt 0 ] || die empty_pkg_list "$de_id|$family"
"${PKG_CMD[@]}" "${pkg_list[@]}"

# 3) Экран входа в ТОМ ЖЕ сеансе авторизации (без 2-го пароля). Только при
# systemd. Имя DM берётся ТОЛЬКО из встроенного списка И обязано
# присутствовать в $pkgs данного DE.
#
# ⚠️ Чужой экран входа НЕ трогаем. Подмена экрана входа — заметное для
# человека вмешательство в общесистемную сущность, и делать её молча,
# «заодно» с установкой стола, нельзя: у человека может стоять plasmalogin,
# gdm, greetd. Включаем DM этого стола ТОЛЬКО когда включённого экрана входа
# в системе нет вовсе (иначе после перезагрузки не будет входа в графику).
#
# ⚠️ И в любом исходе говорим правду: сделал — что сделал, не смог — почему.
# Раньше ошибка systemctl уходила в /dev/null, а «включён» печаталось
# безусловно — вывод сообщал об успехе там, где ничего не произошло.
if command -v systemctl >/dev/null 2>&1; then
    # Что стоит сейчас: цель симлинка display-manager.service, а если
    # симлинка нет — первый включённый юнит из известного списка.
    cur_dm=""
    if [ -L "__DM_SYMLINK__" ]; then
        cur_target="$(readlink -f "__DM_SYMLINK__" 2>/dev/null || true)"
        cur_dm="${cur_target##*/}"
        cur_dm="${cur_dm%.service}"
        [ -n "$cur_dm" ] || cur_dm="display-manager"
    else
        for known in __DM_KNOWN__; do
            state="$(systemctl is-enabled "$known.service" 2>/dev/null || true)"
            case "$state" in
                __DM_STATES__) cur_dm="$known"; break ;;
            esac
        done
    fi

    if [ -n "$cur_dm" ]; then
        rx_say dm_already "$cur_dm"
    else
        # Маппинг ИМЯ-ЮНИТА → ИМЯ-ПАКЕТА (имена обычно совпадают; исключение —
        # Plasma Login Manager: юнит plasmalogin, пакет plasma-login-manager).
        pkg_for_dm() {
            case "$1" in
__DM_PKGMAP__
                *) printf '%s' "$1" ;;
            esac
        }
        dm_done=""
        for dm in __DM_LIST__; do
            case "$dm" in
                __DM_CASES__) : ;;
                *) continue ;;
            esac
            dm_pkg="$(pkg_for_dm "$dm")"
            # dm-ПАКЕТ должен быть в пакет-листе ЭТОГО DE (точное слово).
            printf ' %s ' "$pkgs" | grep -q " $dm_pkg " || continue
            # И юнит реально установлен (unit-файл существует).
            systemctl cat "$dm.service" >/dev/null 2>&1 || continue
            # --force для DM, не Provides display-manager.service (PLM).
            force=""
            case "$dm" in
                __DM_FORCE__) force="--force" ;;
            esac
            # shellcheck disable=SC2086
            if err="$(systemctl enable $force "$dm.service" 2>&1)"; then
                rx_say dm_enabled "$dm"
            else
                if [ -n "$err" ]; then
                    rx_say dm_enable_failed "$dm" "$err"
                else
                    rx_say dm_enable_failed_plain "$dm"
                fi
            fi
            dm_done="$dm"
            break
        done
        [ -n "$dm_done" ] || rx_say dm_none
    fi
fi
exit 0
"#;

// Подписи окна пароля (polkit): все языки каталога сразу (`xml:lang`),
// polkit выбирает сам по локали сеанса ([`crate::system_l10n`]).
// TRANSLATORS: Name of the system action in the password dialog (polkit).
const DE_POLICY_DESCRIPTION: &str = n_("Install or repair a desktop environment");
// TRANSLATORS: Text of the system password dialog (polkit).
const DE_POLICY_MESSAGE: &str = n_("Authentication is required to install a desktop environment");
// TRANSLATORS: Name of the system action in the password dialog (polkit).
const PKG_POLICY_DESCRIPTION: &str = n_("Install, remove or update packages");
// TRANSLATORS: Text of the system password dialog (polkit).
const PKG_POLICY_MESSAGE: &str = n_("Authentication is required to manage packages");
// TRANSLATORS: Name of the system action in the password dialog (polkit): login screen background, greeter theme or MATE panel layout.
const STAGE_POLICY_DESCRIPTION: &str = n_("Install a system login-screen or panel layout");
// TRANSLATORS: Name of the system action in the password dialog (polkit): one password lets Rexon make system changes until its window is closed.
const SESSION_POLICY_DESCRIPTION: &str = n_("Allow Rexon system changes while its window is open");
// TRANSLATORS: Text of the system password dialog (polkit): one password for all system changes until the Rexon window is closed.
const SESSION_POLICY_MESSAGE: &str = n_("Authentication is required to let Rexon make system changes until its window is closed");

/// XML polkit-политики DE_INSTALL_ACTION — порт `_de_policy_xml`.
///
/// Путь ручной установки: по нему установщик кладёт помощника, его же и
/// annotate'ит. Пакет печатает свою политику [`de_policy_xml_for`] — у него
/// помощник лежит в [`HELPER_DIR_PACKAGE`].
pub fn de_policy_xml() -> String {
    de_policy_xml_for(DE_HELPER_PATH)
}

/// XML той же политики для помощника, лежащего по заданному пути.
///
/// `exec.path` — договор с polkit: не совпал с тем, что реально
/// запускается, и pkexec наше действие не выберет (пароль каждый раз).
pub fn de_policy_xml_for(helper_path: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE policyconfig PUBLIC\n\
         \x20\"-//freedesktop//DTD polkit Policy Configuration 1.0//EN\"\n\
         \x20\"http://www.freedesktop.org/software/polkit/policyconfig-1.dtd\">\n\
         <policyconfig>\n\
         \x20 <vendor>Rexon</vendor>\n\
         \x20 <vendor_url>https://github.com/Wehrwolfmann</vendor_url>\n\
         \x20 <action id=\"{DE_INSTALL_ACTION}\">\n\
         {description}\
         {message}\
         \x20   <defaults>\n\
         \x20     <allow_any>auth_admin</allow_any>\n\
         \x20     <allow_inactive>auth_admin</allow_inactive>\n\
         \x20     <allow_active>auth_admin_keep</allow_active>\n\
         \x20   </defaults>\n\
         \x20   <annotate key=\"org.freedesktop.policykit.exec.path\">\
         {helper_path}</annotate>\n\
         \x20   <annotate key=\"org.freedesktop.policykit.exec.allow_gui\">\
         true</annotate>\n\
         \x20 </action>\n\
         </policyconfig>\n",
        description = crate::system_l10n::polkit_elem("    ", "description", DE_POLICY_DESCRIPTION),
        message = crate::system_l10n::polkit_elem("    ", "message", DE_POLICY_MESSAGE),
    )
}

/// Ветки `case` «ID из os-release → семейство» для помощника: ОДНА правда
/// с ядром ([`crate::system::ID_MAP`]).
///
/// Зачем генерировать, а не писать руками: помощник обязан узнавать
/// машину ТАК ЖЕ, как её узнаёт Rexon. Второй список молча разошёлся бы —
/// и на каком-нибудь производном дистрибутиве помощник отказал бы там, где
/// движок уверен, что всё поддержано.
fn id_map_cases() -> String {
    let mut by_family: Vec<(&str, Vec<&str>)> = Vec::new();
    for (id, fam) in crate::system::ID_MAP {
        let fam = fam.as_str();
        match by_family.iter_mut().find(|(f, _)| *f == fam) {
            Some((_, ids)) => ids.push(id),
            None => by_family.push((fam, vec![id])),
        }
    }
    by_family
        .into_iter()
        .map(|(fam, ids)| {
            format!("        {}) printf '%s' {fam}; return 0 ;;", ids.join("|"))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Ветки `case` «семейство → бинарник пакетного менеджера» — хвост той же
/// детекции, что у ядра (`DistroDetector.family`): дистрибутив, которого
/// нет в [`crate::system::ID_MAP`], узнаётся по установленному менеджеру.
const HELPER_BIN_PROBE: &[(&str, &str)] = &[
    ("pacman", "arch"),
    ("dnf", "fedora"),
    ("yum", "fedora"),
    ("zypper", "suse"),
    ("emerge", "gentoo"),
    ("apk", "alpine"),
    ("xbps-install", "void"),
    ("eopkg", "solus"),
    ("swupd", "clear"),
];

/// Одна ветка `case`: команда семейства — МАССИВОМ аргументов, без
/// оболочки и без `eval`. `{PKGS}` из таблицы уходит: пакеты помощник
/// добавляет сам, отдельными аргументами.
fn fam_cmd_case(table: &[(&str, &str)], fam: &str, indent: &str) -> String {
    let tmpl = table_get(table, fam).expect("family в таблице");
    let args: Vec<&str> = tmpl
        .split_whitespace()
        .filter(|t| *t != "{PKGS}")
        .collect();
    for a in &args {
        // Массив bash — не оболочка, но слово с метасимволом всё равно
        // означало бы, что таблица команд перестала быть таблицей.
        assert!(
            a.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '=' | '.' | '_' | '/' | ':')),
            "слово команды {a} требует кавычек — таблица команд сломана"
        );
    }
    format!("{indent}{fam}) cmd=({}) ;;", args.join(" "))
}

/// Тело rexon-pkg-helper.
///
/// ⛔ ЗДЕСЬ ЗАКРЫТА ДЫРА 25.09.2026 (подробности — в шапке самого скрипта).
/// Три правила, которые нельзя ослаблять ни одной правкой:
///   1. семейство пакетного менеджера скрипт определяет САМ по машине
///      ([`id_map_cases`], [`HELPER_BIN_PROBE`]), а не берёт из аргумента;
///   2. имя пакета — ИМЯ, а не путь ([`is_safe_pkg_arg`]): `/` только в
///      атоме Gentoo, файл пакета не принимается даже без `/`;
///   3. команда собирается МАССИВОМ аргументов — `eval` в помощнике нет.
pub fn pkg_helper_script() -> String {
    let fam_install = PKG_HELPER_FAMILIES
        .iter()
        .map(|f| fam_cmd_case(PKG_HELPER_INSTALL, f, "        "))
        .collect::<Vec<_>>()
        .join("\n");
    let fam_remove = PKG_HELPER_FAMILIES
        .iter()
        .map(|f| fam_cmd_case(PKG_HELPER_REMOVE, f, "        "))
        .collect::<Vec<_>>()
        .join("\n");
    let fam_update = PKG_HELPER_FAMILIES
        .iter()
        .map(|f| fam_cmd_case(PKG_HELPER_UPDATE, f, "        "))
        .collect::<Vec<_>>()
        .join("\n");
    let bin_probe = HELPER_BIN_PROBE
        .iter()
        .map(|(bin, fam)| {
            format!("    if have {bin}; then printf '%s' {fam}; return 0; fi")
        })
        .collect::<Vec<_>>()
        .join("\n");
    let fam_list = PKG_HELPER_FAMILIES.join("|");
    let dm_cases = DE_HELPER_DISPLAY_MANAGERS.join("|");
    let (desktop_exact, desktop_substr) = desktop_token_cases();
    let dm_force = DM_FORCE_ENABLE.join("|");
    let text = PKG_HELPER_SCRIPT_TMPL
        .replace("__ID_CASES__", &id_map_cases())
        .replace("__BIN_PROBE__", &bin_probe)
        .replace("__FAM_LIST__", &fam_list)
        .replace("__FAM_INSTALL__", &fam_install)
        .replace("__FAM_REMOVE__", &fam_remove)
        .replace("__FAM_UPDATE__", &fam_update)
        .replace("__VITAL__", &PKG_HELPER_VITAL.join(" "))
        .replace("__VITAL_CASE__", &vital_case_pattern())
        .replace("__DM_KNOWN__", &KNOWN_DM_UNITS.join(" "))
        .replace("__DM_STATES__", &DM_ENABLED_STATES.join("|"))
        .replace("__DM_PROTECT_MAP__", &dm_protect_map())
        .replace("__DESKTOP_EXACT__", &desktop_exact)
        .replace("__DESKTOP_SUBSTR__", &desktop_substr)
        .replace("__DESKTOP_PKGS__", &desktop_protect_cases())
        .replace("__INSTALL_ALLOW__", &install_allow_cases())
        .replace("__DM_FORCE__", &dm_force)
        .replace("__DM_CASES__", &dm_cases)
        .replace("__DM_SWITCH_CASES__", &dm_switch_units().join("|"))
        .replace("__DM_SWITCH_PAM__", &dm_switch_pam_cases())
        .replace("__DM_DEBIAN_PKG__", &dm_debian_pkg_cases())
        .replace("__DM_FALLBACK_SCRIPT_PATH__", DM_FALLBACK_SCRIPT_PATH)
        .replace("__DM_FALLBACK_SCRIPT__", DM_FALLBACK_SCRIPT.trim_end())
        .replace("__DM_FALLBACK_UNIT__", &dm_fallback_unit().trim_end())
        .replace("__DM_FALLBACK_SERVICE__", DM_FALLBACK_SERVICE)
        .replace("__DM_FALLBACK_DROPIN__", DM_FALLBACK_DROPIN)
        .replace("__DM_FALLBACK_DIR__", DM_FALLBACK_DIR)
        .replace("__PREVIOUS_DM_FILE__", PREVIOUS_DM_FILE)
        .replace("__LIGHTDM_GREETER_CONF__", LIGHTDM_GREETER_CONF)
        .replace("__PKG_HELPER_NAME__", PKG_HELPER_NAME)
        .replace("__REBUILD_REC_DIR__", REBUILD_REC_DIR)
        .replace("__DM_SYMLINK__", DM_SYMLINK);
    crate::system_l10n::localize_script(PKG_HELPER_NAME, &text)
}

/// Записи помощника пакетов «что поставил build-deps» (по файлу на
/// вызвавшего, root 0700/0600; `/run` чистится при перезагрузке).
pub const REBUILD_REC_DIR: &str = "/run/rexon-rebuild";

// ── switch-dm: страховка и файлы на диске ────────────────────────────────

/// Каталог Rexon в `/etc`: подсказка для консоли и данные страховки.
pub const DM_FALLBACK_DIR: &str = "/etc/rexon";
/// Подсказка «какой экран входа был и как его вернуть» — читается из
/// текстовой консоли командой `cat` (решение Wehrwolfmann 20.10.2026).
pub const PREVIOUS_DM_FILE: &str = "/etc/rexon/previous-display-manager";
/// Сценарий службы-отката.
pub const DM_FALLBACK_SCRIPT_PATH: &str = "/etc/rexon/dm-fallback";
/// Служба-откат: её зовёт `OnFailure=` нового экрана входа.
pub const DM_FALLBACK_SERVICE: &str = "rexon-dm-fallback.service";
/// Drop-in нового экрана входа с `OnFailure=`.
pub const DM_FALLBACK_DROPIN: &str = "50-rexon-fallback.conf";
/// Выбранное человеком приветствие LightDM.
pub const LIGHTDM_GREETER_CONF: &str = "/etc/lightdm/lightdm.conf.d/50-rexon-greeter.conf";

/// Экраны входа, которые помощник умеет делать основными
/// ([`DmSpec::switchable`]).
pub fn dm_switch_units() -> Vec<&'static str> {
    LOGIN_MANAGER_SPECS.iter().filter(|s| s.switchable).map(|s| s.unit).collect()
}

fn dm_switch_pam_cases() -> String {
    LOGIN_MANAGER_SPECS
        .iter()
        .filter(|s| s.switchable)
        .map(|s| format!("        {}) pam={} ;;", s.unit, s.pam))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Пакет экрана входа на Debian (вопрос debconf и `dpkg-reconfigure`):
/// там, где имя юнита с пакетом не совпадает.
pub const DM_DEBIAN_PKG_FOR_UNIT: &[(&str, &str)] =
    &[("gdm", "gdm3"), ("plasmalogin", "plasma-login-manager")];

fn dm_debian_pkg_cases() -> String {
    DM_DEBIAN_PKG_FOR_UNIT
        .iter()
        .map(|(u, p)| format!("            {u}) printf '%s' {p} ;;"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Юнит службы-отката. `StartLimitIntervalSec=0`: `OnFailure=` зовёт её на
/// каждом падении экрана входа, и без этого она сама упёрлась бы в предел
/// запусков (5 за 10 с) раньше последнего, решающего вызова.
pub fn dm_fallback_unit() -> String {
    format!(
        "[Unit]\n\
         Description=Rexon: bring back the previous login screen\n\
         DefaultDependencies=no\n\
         After=sysinit.target\n\
         StartLimitIntervalSec=0\n\
         \n\
         [Service]\n\
         Type=oneshot\n\
         ExecStart={DM_FALLBACK_SCRIPT_PATH}\n"
    )
}

/// Сценарий службы-отката: запускается systemd через `OnFailure=` нового
/// экрана входа, когда тот упал при загрузке (в том числе после
/// `start-limit-hit`, как Plasma Login Manager 08.10.2026).
///
/// systemd зовёт `OnFailure=` и на падении, после которого сам перезапустит
/// юнит (замер на стенде archlinux 20.10.2026): такие вызовы пропускаются —
/// откат только когда юнит окончательно `failed`.
///
/// Защита от зацикливания: отметка `dm-fallback.done` — откат делается один
/// раз; drop-in снимается с нового экрана входа, а у прежнего его не было,
/// так что, если упадёт и прежний, ничего больше не запустится — останется
/// текстовая консоль и подсказка в [`PREVIOUS_DM_FILE`]. Не срабатывает при
/// выключении машины и если основным уже выбран другой экран входа.
/// Данные — из `dm-fallback.conf`, значения проверяются ещё раз; ничего не
/// исполняется из файла.
pub const DM_FALLBACK_SCRIPT: &str = r#"#!/bin/bash
# Rexon: bring back the previous login screen when the new one failed to
# start (systemd OnFailure= of the new login screen). Runs once.
set -u
export LC_ALL=C LANG=C
export PATH=/usr/sbin:/usr/bin:/sbin:/bin
conf=/etc/rexon/dm-fallback.conf
stamp=/etc/rexon/dm-fallback.done
link=/etc/systemd/system/display-manager.service
val() { sed -n "s/^$1=//p" "$conf" 2>/dev/null | head -n1; }
name_ok() { printf '%s' "$1" | grep -Eq '^[A-Za-z0-9][A-Za-z0-9._-]*$'; }
[ -r "$conf" ] || exit 0
[ ! -e "$stamp" ] || exit 0
old="$(val old)"; new="$(val new)"; new_real="$(val new_real)"
name_ok "$old" && name_ok "$new" || exit 0
case "$(systemctl is-system-running 2>/dev/null || true)" in
    stopping) exit 0 ;;
esac
[ "$(readlink -f "$link" 2>/dev/null || true)" = "$new_real" ] || exit 0
# systemd triggers OnFailure= on every failed run, also when it is about to
# restart the unit itself. Wait until it gives up (start-limit-hit or no
# more restarts): the unit is then "failed" and this runs again.
state="$(systemctl show -p ActiveState --value "$new.service" 2>/dev/null || true)"
sub="$(systemctl show -p SubState --value "$new.service" 2>/dev/null || true)"
if [ "$state" != failed ] || [ "$sub" = auto-restart ] || [ "$sub" = auto-restart-queued ]; then
    echo "rexon-dm-fallback: $new is $state/$sub, waiting for systemd to give up"
    exit 0
fi
date > "$stamp"
echo "rexon-dm-fallback: $new failed to start, bringing back $old"
if ! systemctl enable --force "$old.service"; then
    echo "rexon-dm-fallback: could not enable $old; see /etc/rexon/previous-display-manager"
    exit 1
fi
rm -f "/etc/systemd/system/$new.service.d/50-rexon-fallback.conf"
rmdir "/etc/systemd/system/$new.service.d" 2>/dev/null || true
old_x11="$(val old_x11)"
if [ -n "$old_x11" ] && [ -f /etc/X11/default-display-manager ]; then
    printf '%s\n' "$old_x11" > /etc/X11/default-display-manager
fi
old_pkg="$(val old_pkg)"
if [ -n "$old_pkg" ] && name_ok "$old_pkg" && command -v debconf-communicate >/dev/null 2>&1; then
    printf 'set shared/default-x-display-manager %s\n' "$old_pkg" | debconf-communicate >/dev/null 2>&1 || true
fi
if [ "$(val greeter_written)" = 1 ]; then
    if [ -f /etc/rexon/dm-fallback.greeter-prev ]; then
        cp -p /etc/rexon/dm-fallback.greeter-prev /etc/lightdm/lightdm.conf.d/50-rexon-greeter.conf
    else
        rm -f /etc/lightdm/lightdm.conf.d/50-rexon-greeter.conf
    fi
fi
systemctl daemon-reload
systemctl reset-failed "$new.service" display-manager.service 2>/dev/null || true
systemctl start --no-block display-manager.service
"#;

const PKG_HELPER_SCRIPT_TMPL: &str = r#"#!/bin/bash
# Rexon pkg helper — привилегированные операции пакетного менеджера.
#
# Запускается ИСКЛЮЧИТЕЛЬНО через pkexec (polkit action org.rexon.pkg).
# Принимает операцию и ИМЕНА пакетов. Команды — из ВСТРОЕННЫХ таблиц,
# семейство пакетного менеджера помощник определяет САМ по ЭТОЙ машине.
# Ни оболочки, ни `eval`, ни путей: произвольный код не выполняется.
# Единственный файл, который помощник принимает, — пакет, собранный
# пересборкой Rexon от пользователя (операция install-built, только из
# каталога пересборок вызвавшего; см. её раздел ниже).
#
# ⛔ ДЫРА, КОТОРУЮ ЭТО ЗАКРЫЛО (25.09.2026)
# Прежний помощник (1) брал семейство из АРГУМЕНТА, (2) разрешал в именах
# пакетов символ '/' (allowlist '^[A-Za-z0-9@._+:/-]+$') и (3) собирал
# команду через eval. Любой процесс сеанса мог позвать
#     pkexec rexon-pkg-helper debian install /tmp/evil.deb
# и apt-get (а равно dnf, zypper) поставил бы ЛОКАЛЬНЫЙ ФАЙЛ, сценарии
# установки которого идут ОТ ROOT. Семейство при этом не обязано было
# совпадать с машиной — хватало любого установленного менеджера.
# Вторая половина дыры — 'remove': имя не проверялось ничем, кроме
# символов, и 'remove linux' сносил ядро.
set -euo pipefail
export LC_ALL=C LANG=C
export PATH=/usr/sbin:/usr/bin:/sbin:/bin
# Ни ввода с клавиатуры (вопрос менеджера завис бы молча до таймаута), ни
# чужого рабочего каталога: pkexec его НЕ меняет, а 'dnf install evil.rpm'
# ищет файл именно в нём.
exec </dev/null
cd /

__L10N__
have() { command -v "$1" >/dev/null 2>&1; }

# ── Семейство пакетного менеджера: ТОЛЬКО по этой машине ─────────────
# Порядок тот же, что у ядра (DistroDetector.family): ID из os-release →
# ID_LIKE → наличие бинарников. os-release не исполняется и в оболочку не
# подаётся — значение вынимает sed.
osr_field() {
    local f v
    for f in /etc/os-release /usr/lib/os-release; do
        [ -r "$f" ] || continue
        v="$(sed -n "s/^$1=//p" "$f" | head -n1)"
        v="${v%\"}"; v="${v#\"}"; v="${v%\'}"; v="${v#\'}"
        printf '%s' "$v" | tr 'A-Z' 'a-z'
        return 0
    done
}

family_of_id() {
    case "$1" in
__ID_CASES__
        *) return 1 ;;
    esac
}

detect_family() {
    local id like
    id="$(osr_field ID)"
    if [ -n "$id" ] && family_of_id "$id"; then return 0; fi
    for like in $(osr_field ID_LIKE); do
        if family_of_id "$like"; then return 0; fi
    done
    # APT-RPM (ALT): apt-get есть, dpkg нет.
    if have apt-get; then
        if have dpkg; then printf '%s' debian; return 0; fi
        if have rpm; then printf '%s' alt; return 0; fi
        printf '%s' debian; return 0
    fi
__BIN_PROBE__
    return 1
}

# ── Аргументы ────────────────────────────────────────────────────────
# argv прежний (family op [pkg...]) — по нему зовёт движок, — но ПЕРВЫЙ
# аргумент больше ничего не решает: команду выбирает только определённое
# выше семейство. Расхождение не отказ, а запись в журнал: подсунуть через
# него чужой менеджер пакетов уже нельзя.
[ "$#" -ge 2 ] || die pkg_args
claimed="$1"; op="$2"; shift 2

family="$(detect_family || true)"
[ -n "$family" ] || die family_unknown
case "$family" in
    __FAM_LIST__) : ;;
    *) die unsupported_family "$family" ;;
esac
if [ "$claimed" != "$family" ]; then
    rx_say family_mismatch "$claimed" "$family" >&2
fi

case "$op" in
    install|remove|update|enable-dm|switch-dm|build-deps|install-built) : ;;
    *) die bad_op "$op" ;;
esac

# ── Пересборка по рецепту Arch: две узкие операции ───────────────────
# Пересборка Rexon (repo_rebuild) собирает пакет ОТ ПОЛЬЗОВАТЕЛЯ и
# зовёт помощник дважды, под тем же действием polkit, что и установка
# остального, — пароль спрашивают один раз:
#   build-deps dep...                         недостающее для сборки,
#       pacman -S --needed --asdeps; строка — имя и, возможно,
#       ограничение версии ('cmake>=3.20'), как её отдаёт 'pacman -T';
#   install-built plain|replace file [pkg...] собранный файл, pacman -U
#       (replace — '--ask=4': заменить конфликтующий установленный), затем
#       'pacman -Rns' сборочных pkg; не снялись — не ошибка.
# ⛔ Снять install-built может ТОЛЬКО то, что поставил предыдущий
# build-deps этого же вызвавшего: build-deps записывает новые пакеты
# (разница 'pacman -Qq' до и после) в свой файл root под __REBUILD_REC_DIR__,
# install-built снимает пересечение переданного списка с записью, и лишь
# пакеты с пометкой «зависимость» ('pacman -Qqd'); запись после установки
# стирается. Нет записи — не снимается ничего: иначе через install-built
# можно было бы снять любой пакет системы, кроме вечного списка.
# ⛔ file — НЕ «любой файл»: только из каталога пересборок вызвавшего
# (~/.cache/rexon/rebuild/<подкаталог>, домашний каталог — из passwd по
# PKEXEC_UID, не из окружения), обычный файл вызвавшего, не ссылка, имя
# файла makepkg (*.pkg.tar[.сжатие]). pacman получает СВОЮ копию в
# каталоге 0700 root, снятую с проверенного файла (cp без разыменования
# ссылок отказывает, если файл подменили во время копии), — подмена
# после проверки ничего не меняет. Так помощник не поставит ни системный
# файл, ни старую версию из кэша pacman, ни чужой файл.
if [ "$op" = build-deps ] || [ "$op" = install-built ]; then
    [ "$family" = arch ] || die rebuild_arch_only "$op"
fi
built=""; built_mode=""
if [ "$op" = install-built ]; then
    [ "$#" -ge 2 ] || die built_args
    built_mode="$1"; built="$2"; shift 2
    case "$built_mode" in
        plain|replace) : ;;
        *) die built_args ;;
    esac
fi

# ── Имя пакета — ИМЯ, а не путь ──────────────────────────────────────
# '/' запрещён везде, кроме атома Gentoo «категория/имя»; ведущий '-' —
# подмена пакета на флаг; ведущая точка и '..' — всегда путь; хвост файла
# пакета ('.deb', '.rpm', …) — файл даже без '/'; '@' законно только как
# группа dnf на Fedora (набор Portage '@world' в 'emerge --unmerge' снёс
# бы систему). Правило то же, что у ядра в is_safe_pkg_arg.
atom_ok() {
    printf '%s' "$1" | grep -Eq '^[A-Za-z0-9][A-Za-z0-9._+:-]*$'
}

pkg_ok() {
    local a="$1" ctg nm
    case "$a" in
        *..*) return 1 ;;
        *.deb|*.rpm|*.apk|*.eopkg|*.xbps|*.tar|*.tgz|*.txz|*.zst|*.xz|*.gz|*.bz2|*.lz4|*.snap|*.flatpakref)
            return 1 ;;
    esac
    [ "$family" != fedora ] || a="${a#@}"
    case "$a" in
        */*)
            [ "$family" = gentoo ] || return 1
            ctg="${a%%/*}"; nm="${a#*/}"
            case "$nm" in */*) return 1 ;; esac
            atom_ok "$ctg" && atom_ok "$nm" ;;
        *) atom_ok "$a" ;;
    esac
}

# Строка зависимости для build-deps: имя (то же правило) и, возможно,
# ограничение версии.
dep_ok() {
    local nm="${1%%[<>=]*}" ver
    ver="${1#"$nm"}"
    pkg_ok "$nm" || return 1
    [ -z "$ver" ] || printf '%s' "$ver" | grep -Eq '^(>=|<=|=|<|>)[A-Za-z0-9][A-Za-z0-9._+:~-]*$'
}

for a in "$@"; do
    case "$a" in
        -*) die arg_dash "$a" ;;
    esac
    if [ "$op" = build-deps ]; then
        dep_ok "$a" || die not_a_name "$a"
    else
        pkg_ok "$a" || die not_a_name "$a"
    fi
done

# ── Удаление: жизненно важное не сносим никогда ──────────────────────
# 'remove' идёт от root и с «не спрашивай»: 'remove linux' оставил бы
# машину без загрузки, 'remove polkit' — без механизма подтверждения. Ни
# одно из этих имён не входит ни в один рабочий стол базы Rexon, поэтому
# честному удалению стола список не мешает.
# ⛔ 28.10.2026: не только точные имена, но и семейства — ядра
# ('linux-cachyos', 'linux-image-…'), драйверы ('nvidia-…', 'mesa'),
# прошивки и микрокод ('*-firmware', '*-ucode'), сборщики initramfs.
# Шаблон — то же правило, что у ядра (is_vital_package); без eval.
is_vital() {
    local k="${1#@}"
    k="${k##*/}"
    case "$k" in
        __VITAL_CASE__) return 0 ;;
    esac
    return 1
}
if [ "$op" = "remove" ]; then
    for a in "$@"; do
        if is_vital "$a"; then
            die vital_pkg "$a"
        fi
    done
fi
# install-built: прежний точный список (сборочные вроде 'linux-headers'
# снимаются законно; их границы — у раздела install-built).
VITAL="__VITAL__"
if [ "$op" = install-built ]; then
    for a in "$@"; do
        for v in $VITAL; do
            if [ "$a" = "$v" ] || [ "${a##*/}" = "$v" ]; then
                die vital_pkg "$a"
            fi
        done
    done
fi

# ── Удаление: стол, в котором сидят, и его экран входа — не снимаются ─
# Окну помощник не верит: что держать, он узнаёт сам, от root.
#   1. Активный экран входа: цель ссылки display-manager.service (нет
#      ссылки — первый включённый юнит из известного списка); держится
#      имя юнита, его пакет по таблице ядра и владелец файла юнита у
#      пакетного менеджера. У LightDM — ещё и приветствия
#      ('lightdm-*greeter*'): lightdm от них не зависит.
#   2. Графические сеансы logind (Type x11/wayland/mir, Class user — любого
#      пользователя, не только вызвавшего): стол — по 'Desktop=' сеанса
#      (XDG_SESSION_DESKTOP) той же таблицей, что у ядра; пакеты стола — из
#      таблицы ядра, группы Arch раскрываются ('pacman -Qqg').
# Ограничения: без systemd/elogind ('loginctl') пункт 2 молчит; сеанс без
# 'Desktop=' (startx, часть входов через greetd) стола не называет —
# держится только экран входа.
dm_unit_now() {
    local link="__DM_SYMLINK__" tgt known state
    if [ -L "$link" ]; then
        tgt="$(readlink -f "$link" 2>/dev/null || true)"
        if [ -n "$tgt" ] && [ -e "$tgt" ]; then
            printf '%s\n%s\n' "${tgt##*/}" "$tgt"
            return 0
        fi
    fi
    have systemctl || return 0
    for known in __DM_KNOWN__; do
        [ "$known" != display-manager ] || continue
        state="$(systemctl is-enabled "$known.service" 2>/dev/null || true)"
        case "$state" in
            __DM_STATES__)
                tgt="$(systemctl show -p FragmentPath --value "$known.service" 2>/dev/null || true)"
                printf '%s\n%s\n' "$known.service" "$tgt"
                return 0 ;;
        esac
    done
}

owner_of() {
    [ -n "$1" ] && [ -e "$1" ] || return 0
    case "$family" in
        arch) pacman -Qqo -- "$1" 2>/dev/null || true ;;
        debian) dpkg-query -S -- "$1" 2>/dev/null | sed -n 's/^\([^:,]*\)[:,].*/\1/p' | head -n1 || true ;;
        fedora|suse|alt|clear) rpm -qf --qf '%{NAME}\n' -- "$1" 2>/dev/null || true ;;
        alpine) apk info -Wq -- "$1" 2>/dev/null || true ;;
        void) xbps-query -o -- "$1" 2>/dev/null | sed -n 's/^\([^:]*\)-[^-:]*:.*/\1/p' | head -n1 || true ;;
    esac
}

dm_pkg_of_unit() {
    case "$family|$1" in
__DM_PROTECT_MAP__
        *) printf '%s\n' "$1" ;;
    esac
}

de_of_desktop() {
    local d t
    d="$(printf '%s' "$1" | tr 'A-Z' 'a-z')"
    [ -n "$d" ] || return 1
    # Токены через read, не через разбиение слов: ни одного раскрытия шаблона.
    while IFS= read -r t; do
        case "$t" in
__DESKTOP_EXACT__
        esac
    done < <(printf '%s\n' "$d" | tr ':' '\n')
    case "$d" in
__DESKTOP_SUBSTR__
    esac
    return 1
}

de_pkgs() {
    case "$1" in
__DESKTOP_PKGS__
    esac
}

held_packages() {
    local unit file s ty cl de p
    { read -r unit; read -r file; } < <(dm_unit_now; printf '\n\n')
    unit="${unit%.service}"
    if [ -n "$unit" ]; then
        printf '%s\n' "$unit"
        dm_pkg_of_unit "$unit"
        owner_of "$file"
    fi
    have loginctl || return 0
    for s in $(loginctl list-sessions --no-legend 2>/dev/null | awk '{print $1}'); do
        case "$s" in *[!A-Za-z0-9._-]*|'') continue ;; esac
        ty="$(loginctl show-session "$s" -p Type --value 2>/dev/null || true)"
        case "$ty" in x11|wayland|mir) : ;; *) continue ;; esac
        cl="$(loginctl show-session "$s" -p Class --value 2>/dev/null || true)"
        [ "$cl" = user ] || continue
        de="$(de_of_desktop "$(loginctl show-session "$s" -p Desktop --value 2>/dev/null || true)" || true)"
        [ -n "$de" ] || continue
        for p in $(de_pkgs "$de|$family"); do
            printf '%s\n' "$p"
            if [ "$family" = arch ]; then
                pacman -Qqg -- "$p" 2>/dev/null || true
            fi
        done
    done
}

if [ "$op" = "remove" ]; then
    held="$(held_packages | sed 's|.*/||' | LC_ALL=C sort -u || true)"
    for a in "$@"; do
        b="${a##*/}"
        # here-string, не конвейер (pipefail и SIGPIPE).
        if grep -Fxq -- "$b" <<<"$held"; then
            die current_de_pkg "$a"
        fi
        if grep -Fxq lightdm <<<"$held"; then
            case "$b" in lightdm-*greeter*) die current_de_pkg "$a" ;; esac
        fi
    done
fi

# ── Установка: только пакеты из таблиц ядра ──────────────────────────
# Белый список вшит при генерации помощника: столы (основные и AUR),
# экраны входа, починка соответствия, видеозависимости обоев, yt-dlp,
# оболочки, программы виджетов Conky и распаковщик 7z, части раскладок
# MATE, значки и курсоры тем, образы «Как в дистрибутиве». Имя вне списка
# помощник от root не ставит, что бы ни прислал вызывающий.
install_allowed() {
    local list=""
    case "$family" in
__INSTALL_ALLOW__
    esac
    case " $list " in
        *" $1 "*) return 0 ;;
    esac
    return 1
}
if [ "$op" = install ]; then
    for a in "$@"; do
        install_allowed "$a" || die not_in_tables "$a"
    done
fi

# Команда — массив аргументов; исполняется напрямую, без оболочки.
cmd=()
run_cmd() {
    [ "${#cmd[@]}" -gt 0 ] || die no_cmd "$family"
    have "${cmd[0]}" || die no_pm "${cmd[0]}"
    "${cmd[@]}" "$@"
}

uid_ok() {
    [ -n "${PKEXEC_UID:-}" ] || die not_via_pkexec
    case "$PKEXEC_UID" in
        *[!0-9]*) die not_via_pkexec ;;
    esac
}
REC_DIR="__REBUILD_REC_DIR__"

if [ "$op" = build-deps ]; then
    [ "$#" -ge 1 ] || die op_needs_pkg "$op"
    uid_ok
    have pacman || die no_pm pacman
    before="$(pacman -Qq 2>/dev/null | LC_ALL=C sort -u || true)"
    rc=0
    pacman -S --needed --noconfirm --asdeps -- "$@" || rc=$?
    after="$(pacman -Qq 2>/dev/null | LC_ALL=C sort -u || true)"
    mkdir -p -- "$REC_DIR"
    chmod 0700 -- "$REC_DIR"
    rm -f -- "$REC_DIR/$PKEXEC_UID"
    if [ -n "$before" ] && [ -n "$after" ]; then
        ( umask 077
          LC_ALL=C comm -13 <(printf '%s\n' "$before") <(printf '%s\n' "$after") > "$REC_DIR/$PKEXEC_UID" )
    fi
    exit "$rc"
fi

if [ "$op" = install-built ]; then
    uid_ok
    have pacman || die no_pm pacman
    name="${built##*/}"
    case "$name" in
        .*|-*) die built_bad_name "$built" ;;
        *.pkg.tar|*.pkg.tar.zst|*.pkg.tar.xz|*.pkg.tar.gz|*.pkg.tar.bz2|*.pkg.tar.lz4|*.pkg.tar.lzo|*.pkg.tar.lrz|*.pkg.tar.lz|*.pkg.tar.Z) : ;;
        *) die built_bad_name "$built" ;;
    esac
    printf '%s' "$name" | grep -Eq '^[A-Za-z0-9@_+][A-Za-z0-9@._+:-]*$' || die built_bad_name "$built"
    case "$built" in
        /?*/*) : ;;
        *) die built_outside "$built" ;;
    esac
    home="$(getent passwd "$PKEXEC_UID" | cut -d: -f6 || true)"
    case "$home" in
        /?*) : ;;
        *) die built_no_home "$PKEXEC_UID" ;;
    esac
    # Корень пересборок — свой у вызвавшего; ~/.cache может быть ссылкой
    # на другой диск, поэтому сравниваются настоящие пути.
    rebuild_root="$(cd -P -- "$home/.cache/rexon/rebuild" 2>/dev/null && pwd -P)" \
        || die built_outside "$built"
    [ "$(stat -c %u -- "$rebuild_root")" = "$PKEXEC_UID" ] || die built_outside "$built"
    # Сначала ВХОДИМ в каталог файла: дальше имя без '/' считается от него,
    # и подмена части пути ссылкой уже ничего не меняет.
    cd -P -- "${built%/*}" 2>/dev/null || die built_unreachable "$built"
    here="$(pwd -P)"
    case "$here" in
        "$rebuild_root"/?*) : ;;
        *) die built_outside "$built" ;;
    esac
    [ "$(stat -c %u .)" = "$PKEXEC_UID" ] || die built_not_callers "$built"
    # Предварительно — чтобы cp не повис на FIFO; решает проверка копии.
    { [ -f "./$name" ] && [ ! -L "./$name" ]; } || die built_not_file "$built"
    work="$(mktemp -d /var/tmp/rexon-pkg-helper.XXXXXXXX)" || die no_work_dir
    trap 'rm -rf -- "$work"' EXIT
    cp -P --preserve=ownership -- "./$name" "$work/$name" 2>/dev/null || die built_copy_failed "$built"
    cd /
    { [ -f "$work/$name" ] && [ ! -L "$work/$name" ]; } || die built_not_file "$built"
    [ "$(stat -c %u -- "$work/$name")" = "$PKEXEC_UID" ] || die built_not_callers "$built"
    chown 0:0 -- "$work/$name"
    chmod 0600 -- "$work/$name"
    rc=0
    if [ "$built_mode" = replace ]; then
        pacman -U --noconfirm --ask=4 -- "$work/$name" || rc=$?
    else
        pacman -U --noconfirm -- "$work/$name" || rc=$?
    fi
    [ "$rc" -eq 0 ] || exit "$rc"
    rec="$REC_DIR/$PKEXEC_UID"
    drop=()
    if [ "$#" -gt 0 ]; then
        ours=""; asdeps=""
        if [ -f "$rec" ] && [ ! -L "$rec" ]; then
            ours="$(cat -- "$rec")"
            asdeps="$(pacman -Qqd 2>/dev/null || true)"
        fi
        for a in "$@"; do
            # here-string, не конвейер: при pipefail ранний выход grep -q
            # мог бы уронить printf (SIGPIPE) и «не найти» найденное.
            if grep -Fxq -- "$a" <<<"$ours" && grep -Fxq -- "$a" <<<"$asdeps"; then
                drop+=("$a")
            else
                rx_say build_only_not_ours "$a"
            fi
        done
    fi
    rm -f -- "$rec"
    if [ "${#drop[@]}" -gt 0 ]; then
        pacman -Rns --noconfirm -- "${drop[@]}" || rx_say build_only_kept
    fi
    exit 0
fi

if [ "$op" = "update" ]; then
    [ "$#" -eq 0 ] || die update_no_pkgs
    case "$family" in
__FAM_UPDATE__
    esac
    run_cmd
    exit $?
fi

if [ "$op" = "enable-dm" ]; then
    [ "$#" -eq 1 ] || die enable_dm_one
    dm="$1"
    case "$dm" in
        __DM_CASES__) : ;;
        *) die bad_dm "$dm" ;;
    esac
    have systemctl || die no_systemctl
    # --force для DM, не Provides display-manager.service (Plasma Login
    # Manager: иначе enable отказывается из-за alias-конфликта).
    cmd=(systemctl enable)
    case "$dm" in
        __DM_FORCE__) cmd+=(--force) ;;
    esac
    cmd+=("$dm.service")
    "${cmd[@]}"
    exit $?
fi

# ── switch-dm: «Сделать основным» экран входа ────────────────────────
# Только systemd. Помощник САМ определяет прежний экран входа и механизм
# выбора, САМ повторяет критичные проверки (между пробой интерфейса и
# паролем мог пройти час) и сверяет результат; при расхождении — откат.
# ⛔ Только enable: никаких запусков и остановок служб в живом сеансе —
# новый экран входа появится после перезагрузки. Запускает
# display-manager.service лишь служба-откат, и только при загрузке,
# когда новый экран входа упал.
if [ "$op" = "switch-dm" ]; then
    [ "$#" -ge 1 ] && [ "$#" -le 2 ] || die switch_dm_args
    new="$1"; greeter="${2:-}"
    case "$new" in
        __DM_SWITCH_CASES__) : ;;
        *) die bad_dm "$new" ;;
    esac
    have systemctl || die no_systemctl
    [ -d /run/systemd/system ] || die no_systemd
    DM_LINK="__DM_SYMLINK__"
    GREETER_CONF="__LIGHTDM_GREETER_CONF__"
    REXON_ETC="__DM_FALLBACK_DIR__"

    # Механизм выбора — по этой машине (то же решение, что dm_switch_backend).
    backend=systemd
    for p in "$DM_LINK" /usr/lib/systemd/system/display-manager.service; do
        if [ -f "$p" ] && [ "$(basename "$(readlink -f "$p")")" = display-manager.service ]; then
            die dm_backend_unsupported
        fi
    done
    if [ "$family" = debian ] && [ -f /etc/X11/default-display-manager ]; then
        backend=debian
    fi

    unit_path() {
        local d
        for d in /etc/systemd/system /usr/local/lib/systemd/system /usr/lib/systemd/system /lib/systemd/system; do
            if [ -e "$d/$1.service" ] || [ -L "$d/$1.service" ]; then
                printf '%s' "$d/$1.service"; return 0
            fi
        done
        return 1
    }
    unit_file="$(unit_path "$new" || true)"
    [ -n "$unit_file" ] || die dm_no_unit "$new"
    [ "$(readlink -f "$unit_file")" != /dev/null ] || die dm_masked "$new"
    unit_real="$(readlink -f "$unit_file")"

    # Программа из ExecStart (префиксы systemd -@:+! снимаются).
    exec_line="$(sed -n 's/^[[:space:]]*ExecStart=//p' "$unit_real" | tail -n1)"
    exec_bin="$(printf '%s\n' "$exec_line" | sed 's/^[-@:+!]*//' | awk '{print $1}')"
    [ -n "$exec_bin" ] || die dm_no_program "$new"
    have "$exec_bin" || die dm_no_program "$exec_bin"
    bin_path="$(command -v "$exec_bin")"

    pam=""
    case "$new" in
__DM_SWITCH_PAM__
    esac
    if [ -n "$pam" ] && [ ! -f "/etc/pam.d/$pam" ] && [ ! -f "/usr/lib/pam.d/$pam" ]; then
        die dm_no_pam "$pam"
    fi

    # greetd и всё, что идёт через него (cosmic-greeter): приветствие — из
    # СВОЕГО конфига. Текстовый вход (agreety, голая оболочка) не годится.
    if [ "$(basename "$exec_bin")" = greetd ]; then
        cfg="$(printf '%s\n' "$exec_line" | sed -n 's/.*\(--config\|-c\)[= ]\([^ ]*\).*/\2/p')"
        [ -n "$cfg" ] || cfg=/etc/greetd/config.toml
        gcmd="$(sed -n '/^\[default_session\]/,/^\[/{s/^[[:space:]]*command[[:space:]]*=[[:space:]]*//p}' "$cfg" 2>/dev/null | head -n1)"
        gcmd="${gcmd#\"}"; gcmd="${gcmd%\"}"; gcmd="${gcmd#\'}"; gcmd="${gcmd%\'}"
        gprog=""
        set -f
        for w in $gcmd; do
            case "$w" in
                env) continue ;;
                /*) gprog="$w"; break ;;
                *=*) continue ;;
                *) gprog="$w"; break ;;
            esac
        done
        set +f
        case "$(basename "${gprog:-agreety}")" in
            agreety|login|sh|bash|dash|zsh|fish) die dm_text_login ;;
        esac
        have "$gprog" || die dm_no_greeter_prog "$gprog"
    fi

    if [ -n "$greeter" ]; then
        [ "$new" = lightdm ] || die greeter_not_lightdm
        printf '%s' "$greeter" | grep -Eq '^[A-Za-z0-9][A-Za-z0-9._-]*$' || die bad_greeter "$greeter"
        gfile="/usr/share/xgreeters/$greeter.desktop"
        [ -f "$gfile" ] || die dm_no_greeter_prog "$greeter"
        gexec="$(sed -n 's/^Exec=//p' "$gfile" | head -n1 | awk '{print $1}')"
        { [ -n "$gexec" ] && have "$gexec"; } || die dm_no_greeter_prog "$greeter"
    fi
    if [ "$new" = lightdm ]; then
        ls /usr/share/xgreeters/*.desktop >/dev/null 2>&1 || die lightdm_no_greeter
        { have Xorg || [ -x /usr/lib/Xorg ]; } || die dm_no_xorg
    fi

    # Прежний экран входа — только по живой ссылке.
    old=""; old_real=""
    if [ -L "$DM_LINK" ] && [ -e "$DM_LINK" ]; then
        old_real="$(readlink -f "$DM_LINK")"
        old="$(basename "$old_real" .service)"
        printf '%s' "$old" | grep -Eq '^[A-Za-z0-9][A-Za-z0-9._-]*$' || { old=""; old_real=""; }
    fi
    if [ -n "$old_real" ] && [ "$old_real" = "$unit_real" ]; then
        rx_say dm_already_default "$new"
        exit 0
    fi

    debian_pkg() {
        case "$1" in
__DM_DEBIAN_PKG__
            *) printf '%s' "$1" ;;
        esac
    }
    old_x11=""; old_pkg=""
    if [ "$backend" = debian ]; then
        old_x11="$(head -n1 /etc/X11/default-display-manager 2>/dev/null || true)"
        [ -z "$old" ] || old_pkg="$(debian_pkg "$old")"
    fi

    # Прежняя страховка снимается: она относилась к прошлому переключению.
    for f in /etc/systemd/system/*.service.d/__DM_FALLBACK_DROPIN__; do
        [ -e "$f" ] || continue
        rm -f "$f"
        rmdir "$(dirname "$f")" 2>/dev/null || true
    done
    rm -f "$REXON_ETC/dm-fallback.done" "$REXON_ETC/dm-fallback.conf" "$REXON_ETC/dm-fallback.greeter-prev"

    greeter_written=0
    if [ -n "$greeter" ]; then
        mkdir -p "$(dirname "$GREETER_CONF")" "$REXON_ETC"
        [ ! -f "$GREETER_CONF" ] || cp -p "$GREETER_CONF" "$REXON_ETC/dm-fallback.greeter-prev"
        printf '[Seat:*]\ngreeter-session=%s\n' "$greeter" > "$GREETER_CONF"
        greeter_written=1
    fi

    debconf_set() {
        if have debconf-communicate; then
            printf 'set shared/default-x-display-manager %s\n' "$1" | debconf-communicate >/dev/null 2>&1 || true
        fi
    }
    rollback() {
        if [ -n "$old" ]; then
            systemctl enable --force "$old.service" >/dev/null 2>&1 || true
        fi
        if [ "$backend" = debian ] && [ -n "$old_x11" ]; then
            printf '%s\n' "$old_x11" > /etc/X11/default-display-manager || true
            [ -z "$old_pkg" ] || debconf_set "$old_pkg"
        fi
        if [ "$greeter_written" = 1 ]; then
            if [ -f "$REXON_ETC/dm-fallback.greeter-prev" ]; then
                cp -p "$REXON_ETC/dm-fallback.greeter-prev" "$GREETER_CONF" || true
            else
                rm -f "$GREETER_CONF"
            fi
        fi
    }

    if [ "$backend" = debian ]; then
        new_pkg="$(debian_pkg "$new")"
        debconf_set "$new_pkg"
        printf '%s\n' "$bin_path" > /etc/X11/default-display-manager
        if have dpkg-reconfigure; then
            # policy-rc.d: сценарий пакета не должен запустить службу в
            # живом сеансе.
            own_policy=0
            if [ ! -e /usr/sbin/policy-rc.d ]; then
                printf '#!/bin/sh\nexit 101\n' > /usr/sbin/policy-rc.d
                chmod 0755 /usr/sbin/policy-rc.d
                own_policy=1
            fi
            DEBIAN_FRONTEND=noninteractive dpkg-reconfigure "$new_pkg" >/dev/null 2>&1 || true
            [ "$own_policy" = 0 ] || rm -f /usr/sbin/policy-rc.d
        fi
    fi

    if ! out="$(systemctl enable --force "$new.service" 2>&1)"; then
        rollback
        die dm_switch_failed "$new" "$out"
    fi

    # Проверка «после»: ссылка, состояние юнита, на Debian — файл выбора.
    ok=1
    [ "$(readlink -f "$DM_LINK" 2>/dev/null || true)" = "$unit_real" ] || ok=0
    case "$(systemctl is-enabled "$new.service" 2>/dev/null || true)" in
        enabled|enabled-runtime|alias) : ;;
        *) ok=0 ;;
    esac
    if [ "$backend" = debian ] && [ "$(head -n1 /etc/X11/default-display-manager 2>/dev/null || true)" != "$bin_path" ]; then
        ok=0
    fi
    if [ "$ok" = 0 ]; then
        rollback
        die dm_switch_mismatch "$new"
    fi

    # Прежний с WantedBy= (самодельный юнит) иначе поднялся бы вторым.
    if [ -n "$old" ] && grep -Eq '^[[:space:]]*WantedBy=' "$old_real" 2>/dev/null; then
        systemctl disable "$old.service" >/dev/null 2>&1 || true
    fi

    # Страховка: новый экран входа упал при загрузке (в том числе
    # start-limit-hit) — служба-откат вернёт прежний. Только когда есть,
    # к чему возвращаться.
    mkdir -p "$REXON_ETC"
    if [ -n "$old" ]; then
        {
            printf 'old=%s\n' "$old"
            printf 'old_real=%s\n' "$old_real"
            printf 'new=%s\n' "$new"
            printf 'new_real=%s\n' "$unit_real"
            printf 'old_x11=%s\n' "$old_x11"
            printf 'old_pkg=%s\n' "$old_pkg"
            printf 'greeter_written=%s\n' "$greeter_written"
        } > "$REXON_ETC/dm-fallback.conf"
        cat > "__DM_FALLBACK_SCRIPT_PATH__" <<'REXON_DM_FALLBACK'
__DM_FALLBACK_SCRIPT__
REXON_DM_FALLBACK
        chmod 0755 "__DM_FALLBACK_SCRIPT_PATH__"
        cat > "/etc/systemd/system/__DM_FALLBACK_SERVICE__" <<'REXON_DM_FALLBACK_UNIT'
__DM_FALLBACK_UNIT__
REXON_DM_FALLBACK_UNIT
        mkdir -p "/etc/systemd/system/$new.service.d"
        printf '[Unit]\nOnFailure=__DM_FALLBACK_SERVICE__\n' > "/etc/systemd/system/$new.service.d/__DM_FALLBACK_DROPIN__"
        if [ "$backend" = debian ]; then
            back="sudo dpkg-reconfigure $old_pkg && sudo systemctl enable --force $old.service && sudo reboot"
        else
            back="sudo systemctl enable --force $old.service && sudo reboot"
        fi
        # Строки для человека — без имени помощника в начале.
        line() { printf '%s\n' "${1#__PKG_HELPER_NAME__: }"; }
        {
            line "$(rx_say prev_dm_header "$old" "$new")"
            line "$(rx_say prev_dm_back "$old")"
            printf '    %s\n' "$back"
            line "$(rx_say prev_dm_auto "$new" "$old")"
        } > "__PREVIOUS_DM_FILE__"
        rx_say dm_switched "$new" "$old"
    else
        rm -f "__PREVIOUS_DM_FILE__"
        rx_say dm_switched_first "$new"
    fi
    exit 0
fi

# install / remove — нужен хотя бы один пакет.
[ "$#" -ge 1 ] || die op_needs_pkg "$op"
case "$op" in
    install)
        case "$family" in
__FAM_INSTALL__
        esac ;;
    remove)
        case "$family" in
__FAM_REMOVE__
        esac ;;
esac
run_cmd "$@"
exit $?
"#;

/// XML polkit-политики PKG_ACTION — порт `_pkg_policy_xml` (путь ручной
/// установки; пакет печатает [`pkg_policy_xml_for`]).
pub fn pkg_policy_xml() -> String {
    pkg_policy_xml_for(PKG_HELPER_PATH)
}

/// Действия, которые покрывает подтверждённое `org.rexon.pkg` —
/// аннотация polkit `org.freedesktop.policykit.imply` («зонтик»).
///
/// Как это работает (polkit, `check_authorization_sync`): спрашивая
/// разрешение на действие X, polkit смотрит и действия, в чьём `imply`
/// стоит X, и проверяет их БЕЗ окна пароля — неявное разрешение и
/// временное (`auth_admin_keep`) для того же процесса. Значит, пароль,
/// введённый окном Rexon для `org.rexon.pkg`, пять минут покрывает и
/// установку стола, и раскладку файлов экрана входа, и темы
/// SDDM/Plymouth — одно окно вместо четырёх.
///
/// Направление одно и выбрано намеренно: зонтик держит самое сильное
/// действие (установка и удаление пакетов по имени), и оно НИКОГДА не
/// бывает «без пароля» ([`PASSWORDLESS_ACTIONS`]). Обратное (`imply` у
/// действия из правила «без пароля») отдало бы пакеты без пароля любому
/// процессу сеанса — это дыра, закрытая 25.09.2026. Сторож —
/// `imply_umbrella_never_hangs_on_passwordless_action`.
pub const PKG_IMPLIES: &[&str] = &[
    DE_INSTALL_ACTION,
    STAGE_ACTION,
    crate::installer::SYSTEM_INSTALL_ACTION,
];

/// XML той же политики для помощника, лежащего по заданному пути.
pub fn pkg_policy_xml_for(helper_path: &str) -> String {
    let implies = PKG_IMPLIES.join(" ");
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE policyconfig PUBLIC\n\
         \x20\"-//freedesktop//DTD polkit Policy Configuration 1.0//EN\"\n\
         \x20\"http://www.freedesktop.org/software/polkit/policyconfig-1.dtd\">\n\
         <policyconfig>\n\
         \x20 <vendor>Rexon</vendor>\n\
         \x20 <vendor_url>https://github.com/Wehrwolfmann</vendor_url>\n\
         \x20 <action id=\"{PKG_ACTION}\">\n\
         {description}\
         {message}\
         \x20   <defaults>\n\
         \x20     <allow_any>auth_admin</allow_any>\n\
         \x20     <allow_inactive>auth_admin</allow_inactive>\n\
         \x20     <allow_active>auth_admin_keep</allow_active>\n\
         \x20   </defaults>\n\
         \x20   <annotate key=\"org.freedesktop.policykit.exec.path\">\
         {helper_path}</annotate>\n\
         \x20   <annotate key=\"org.freedesktop.policykit.exec.allow_gui\">\
         true</annotate>\n\
         \x20   <annotate key=\"org.freedesktop.policykit.imply\">\
         {implies}</annotate>\n\
         \x20 </action>\n\
         </policyconfig>\n",
        description = crate::system_l10n::polkit_elem("    ", "description", PKG_POLICY_DESCRIPTION),
        message = crate::system_l10n::polkit_elem("    ", "message", PKG_POLICY_MESSAGE),
    )
}

/// Тело rexon-stage-helper — порт `_stage_helper_script`.
pub fn stage_helper_script() -> String {
    let cat_list = STAGE_CATEGORIES.join("|");
    let webkit_root = table_get(STAGE_DIR_ROOT, "lightdm-webkit").unwrap();
    let layouts_root = table_get(STAGE_DIR_ROOT, "mate-layouts").unwrap();
    let text = STAGE_HELPER_SCRIPT_TMPL
        .replace("__CAT_LIST__", &cat_list)
        .replace("__WEBKIT_ROOT__", webkit_root)
        .replace("__WEB_GREETER_ROOT__", WEB_GREETER_ROOT)
        .replace("__LAYOUTS_ROOT__", layouts_root);
    crate::system_l10n::localize_script(STAGE_HELPER_NAME, &text)
}

const STAGE_HELPER_SCRIPT_TMPL: &str = r#"#!/bin/bash
# Rexon stage helper — привилегированный file-staging в /usr/share.
# Запускается ИСКЛЮЧИТЕЛЬНО через pkexec (polkit action org.rexon.stage,
# auth_admin_keep: подтверждённый пароль держится несколько минут для окна
# Rexon; в правило «без пароля» не входит с 28.09.2026). Копирует
# ВАЛИДИРОВАННЫЙ staged-каталог в ФИКСИРОВАННЫЙ путь по category. НЕ
# принимает произвольный dest. НЕ выполняет произвольный код.
set -euo pipefail
export LC_ALL=C LANG=C

__L10N__

[ "$#" -eq 3 ] || die stage_args
category="$1"; name="$2"; staged="$3"

# 1) category — строгий allowlist.
case "$category" in
    __CAT_LIST__) : ;;
    *) die bad_category "$category" ;;
esac

# 2) name — только безопасные символы; без / .. и ведущего '-'/'.'/пустоты.
case "$name" in
    ""|-*|.*|*/*|*..*) die bad_name "$name" ;;
esac
printf '%s' "$name" | grep -Eq '^[A-Za-z0-9._-]+$' \
    || die bad_name_chars "$name"

# 3) staged — каталог вызвавшего под /tmp|/var/tmp (Rexon создаёт его через
#    mkdtemp). Сначала ВХОДИМ в него: дальше работаем с тем самым каталогом,
#    и подмена части пути ссылкой уже ничего не меняет. Место и владельца
#    проверяем у входа, копию «.» кладём в свой каталог 0700 и дальше
#    работаем ТОЛЬКО с ней — её пользователь не тронет. Прежде: realpath,
#    затем cp по строке пути — окно подменить путь и унести root'ом чужой
#    каталог в общедоступный /usr/share.
[ -n "${PKEXEC_UID:-}" ] || die not_via_pkexec
[ -d "$staged" ] || die staged_not_dir "$staged"
cd -P -- "$staged" 2>/dev/null || die staged_unreachable "$staged"
here="$(pwd -P)"
case "$here" in
    /tmp/?*|/var/tmp/?*) : ;;
    *) die staged_outside_tmp "$here" ;;
esac
[ "$(stat -c %u .)" = "$PKEXEC_UID" ] || die staged_not_callers "$here"
work="$(mktemp -d /var/tmp/rexon-stage-helper.XXXXXXXX)" || die no_work_dir
trap 'rm -rf -- "$work"' EXIT
real_staged="$work/staged"
cp -R -P -- . "$real_staged" || die staged_copy_failed
cd /
odd="$(find "$real_staged" -mindepth 1 ! -type f ! -type d -print -quit)"
[ -z "$odd" ] || die staged_odd "${odd#"$real_staged"/}"

# ── Обработка по category ────────────────────────────────────────────
case "$category" in
    gdm-bg|lightdm-bg|cosmic-greeter|login-bg)
        # staged содержит РОВНО один файл-картинку. Имя файла берём из
        # каталога, расширение нормализуем (только [A-Za-z0-9]).
        img="$(find "$real_staged" -maxdepth 1 -type f \
            \( -iname '*.png' -o -iname '*.jpg' -o -iname '*.jpeg' \
               -o -iname '*.webp' -o -iname '*.bmp' \) | head -n1)"
        [ -n "$img" ] || die no_image
        ext="${img##*.}"
        printf '%s' "$ext" | grep -Eq '^[A-Za-z0-9]+$' || ext="png"
        dest_bg="/usr/share/backgrounds/rexon-login.$ext"
        install -d /usr/share/backgrounds
        # Удаляем прошлые rexon-login.* (расширение могло смениться).
        rm -f /usr/share/backgrounds/rexon-login.* 2>/dev/null || true
        cp -f "$img" "$dest_bg"
        chmod 644 "$dest_bg"
        case "$category" in
            gdm-bg)
                install -d /etc/dconf/db/gdm.d
                cat > /etc/dconf/db/gdm.d/02-rexon-login <<REXON_EOF
[org/gnome/desktop/background]
picture-uri='file://$dest_bg'
picture-uri-dark='file://$dest_bg'
picture-options='zoom'
REXON_EOF
                dconf update 2>/dev/null || true
                ;;
            lightdm-bg)
                # slick-greeter.
                if [ -f /etc/lightdm/slick-greeter.conf ]; then
                    sed -i "s|^background=.*|background=$dest_bg|" \
                        /etc/lightdm/slick-greeter.conf
                elif [ -d /etc/lightdm ]; then
                    cat >> /etc/lightdm/slick-greeter.conf <<REXON_EOF
[Greeter]
background=$dest_bg
REXON_EOF
                fi
                # gtk-greeter.
                if [ -f /etc/lightdm/lightdm-gtk-greeter.conf ]; then
                    sed -i "s|^background=.*|background=$dest_bg|" \
                        /etc/lightdm/lightdm-gtk-greeter.conf
                fi
                ;;
            cosmic-greeter|login-bg)
                : # только фон скопирован (login-bg — фон берёт учётная запись)
                ;;
        esac
        rx_say bg_installed "$dest_bg"
        ;;
    lightdm-webkit)
        # Корень выбираем по факту: тот, который в системе реально есть
        # (оба пути фиксированы здесь же — произвольный dest по-прежнему
        # не принимается).
        if [ -d "__WEBKIT_ROOT__" ]; then
            root="__WEBKIT_ROOT__"
        elif [ -d "__WEB_GREETER_ROOT__" ]; then
            root="__WEB_GREETER_ROOT__"
        else
            die no_greeter_dir
        fi
        dest="$root/$name"
        install -d "$dest"
        cp -rT "$real_staged" "$dest"
        chmod -R a+rX "$dest"
        rx_say webkit_theme "$name"
        ;;
    mate-layouts)
        # staged = каталог с .layout/.dock (+ опц. sed-адаптация snap-путей
        # выполнена Rexon ДО staging, helper только копирует). Файлы кладём
        # прямо в корень layouts (без <name>-подкаталога). name здесь —
        # технический маркер ('rexon'), уже провалидирован выше.
        dst="__LAYOUTS_ROOT__"
        install -d "$dst"
        # Копируем ТОЛЬКО .layout/.dock (anti-«лишние файлы»).
        found=0
        for f in "$real_staged"/*.layout "$real_staged"/*.dock; do
            [ -e "$f" ] || continue
            cp -f "$f" "$dst/"
            found=1
        done
        [ "$found" = "1" ] || die no_layouts
        chmod -R a+rX "$dst"
        rx_say layouts_installed
        ;;
esac
exit 0
"#;

/// XML polkit-политики STAGE_ACTION — порт `_stage_policy_xml` (путь ручной
/// установки; пакет печатает [`stage_policy_xml_for`]).
pub fn stage_policy_xml() -> String {
    stage_policy_xml_for(STAGE_HELPER_PATH)
}

/// XML той же политики для помощника, лежащего по заданному пути.
pub fn stage_policy_xml_for(helper_path: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE policyconfig PUBLIC\n\
         \x20\"-//freedesktop//DTD polkit Policy Configuration 1.0//EN\"\n\
         \x20\"http://www.freedesktop.org/software/polkit/policyconfig-1.dtd\">\n\
         <policyconfig>\n\
         \x20 <vendor>Rexon</vendor>\n\
         \x20 <vendor_url>https://github.com/Wehrwolfmann</vendor_url>\n\
         \x20 <action id=\"{STAGE_ACTION}\">\n\
         {description}\
         {message}\
         \x20   <defaults>\n\
         \x20     <allow_any>auth_admin</allow_any>\n\
         \x20     <allow_inactive>auth_admin</allow_inactive>\n\
         \x20     <allow_active>auth_admin_keep</allow_active>\n\
         \x20   </defaults>\n\
         \x20   <annotate key=\"org.freedesktop.policykit.exec.path\">\
         {helper_path}</annotate>\n\
         \x20   <annotate key=\"org.freedesktop.policykit.exec.allow_gui\">\
         true</annotate>\n\
         \x20 </action>\n\
         </policyconfig>\n",
        description = crate::system_l10n::polkit_elem("    ", "description", STAGE_POLICY_DESCRIPTION),
        message = crate::system_l10n::polkit_elem("    ", "message", crate::installer::POLICY_MESSAGE_SYSTEM_THEME),
    )
}

/// XML polkit-политики [`SESSION_ACTION`] (путь ручной установки; пакет
/// печатает [`session_policy_xml_for`]).
pub fn session_policy_xml() -> String {
    session_policy_xml_for(SESSION_HELPER_PATH)
}

/// XML политики сеанса прав для помощника, лежащего по заданному пути.
///
/// ⛔ `allow_active` — `auth_admin`, не `auth_admin_keep`: один пароль на
/// сеанс окна держит помощник. Аннотации `imply` нет намеренно.
pub fn session_policy_xml_for(helper_path: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE policyconfig PUBLIC\n\
         \x20\"-//freedesktop//DTD polkit Policy Configuration 1.0//EN\"\n\
         \x20\"http://www.freedesktop.org/software/polkit/policyconfig-1.dtd\">\n\
         <policyconfig>\n\
         \x20 <vendor>Rexon</vendor>\n\
         \x20 <vendor_url>https://github.com/Wehrwolfmann</vendor_url>\n\
         \x20 <action id=\"{SESSION_ACTION}\">\n\
         {description}\
         {message}\
         \x20   <defaults>\n\
         \x20     <allow_any>auth_admin</allow_any>\n\
         \x20     <allow_inactive>auth_admin</allow_inactive>\n\
         \x20     <allow_active>auth_admin</allow_active>\n\
         \x20   </defaults>\n\
         \x20   <annotate key=\"org.freedesktop.policykit.exec.path\">\
         {helper_path}</annotate>\n\
         \x20   <annotate key=\"org.freedesktop.policykit.exec.allow_gui\">\
         true</annotate>\n\
         \x20 </action>\n\
         </policyconfig>\n",
        description = crate::system_l10n::polkit_elem("    ", "description", SESSION_POLICY_DESCRIPTION),
        message = crate::system_l10n::polkit_elem("    ", "message", SESSION_POLICY_MESSAGE),
    )
}

/// pkexec-команда операции пакетного менеджера — порт `privileged_pkg_cmd`.
/// None → вызывающий код откатывается на сырой `pkexec <pm> …` (graceful
/// fallback; наличие helper'а — параметр, файлы не проверяем).
pub fn privileged_pkg_cmd(
    family: PkgFamily,
    op: &str,
    packages: &[String],
    helper_available: bool,
) -> Option<Vec<String>> {
    let fam = family.as_str();
    if !PKG_HELPER_OPS.contains(&op) || matches!(op, "build-deps" | "install-built") {
        return None;
    }
    if !PKG_HELPER_FAMILIES.contains(&fam) {
        return None;
    }
    if !helper_available {
        return None;
    }
    let mut args = vec![pkg_helper_path().to_string()];
    // Язык сообщений помощника (решение 30.09.2026) — сразу за его путём.
    args.extend(crate::system_l10n::lang_args(pkg_helper_path()));
    args.extend([fam.to_string(), op.to_string()]);
    if matches!(op, "install" | "remove" | "enable-dm") {
        if packages.is_empty() {
            return None;
        }
        for p in packages {
            // ⛔ Отказ здесь НЕ откат на сырой путь: сырой путь для таких
            // имён тоже закрыт — [`install_cmd`] и [`remove_cmd`] отдают
            // ошибку раньше, чем план дойдёт до `pkexec`.
            if p.starts_with('-') || !is_safe_pkg_arg(family, p) {
                return None;
            }
        }
        if op == "enable-dm"
            && (packages.len() != 1
                || !DE_HELPER_DISPLAY_MANAGERS
                    .contains(&packages[0].as_str()))
        {
            return None;
        }
        args.extend(packages.iter().cloned());
    } else if op == "switch-dm" {
        // Юнит из списка умеющих; приветствие — только для LightDM и только
        // имя файла (его наличие помощник проверит сам).
        let (unit, greeter) = match packages {
            [u] => (u, None),
            [u, g] => (u, Some(g)),
            _ => return None,
        };
        if !dm_switch_units().contains(&unit.as_str()) {
            return None;
        }
        if let Some(g) = greeter {
            if unit != "lightdm" || !greeter_name_ok(g) {
                return None;
            }
        }
        args.extend(packages.iter().cloned());
    } else if op == "update" && !packages.is_empty() {
        return None;
    }
    let mut cmd = vec!["pkexec".to_string()];
    cmd.extend(args);
    Some(cmd)
}

/// pkexec-команда file-staging — порт `privileged_stage_cmd`. Файловые
/// проверки Python (`os.path.isdir`, `os.path.realpath`) вынесены в
/// параметры `staged_is_dir` / `staged_realpath` (None = не существует).
pub fn privileged_stage_cmd(
    category: &str,
    name: &str,
    staged: &str,
    staged_is_dir: bool,
    staged_realpath: Option<&str>,
    helper_available: bool,
) -> Option<Vec<String>> {
    if !STAGE_CATEGORIES.contains(&category) {
        return None;
    }
    if name.is_empty() || name.starts_with(['-', '.', '/']) {
        return None;
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        || name.contains("..")
    {
        return None;
    }
    if staged.is_empty() || !staged_is_dir {
        return None;
    }
    let real = staged_realpath?;
    if !(real.starts_with("/tmp/") || real.starts_with("/var/tmp/")) {
        return None;
    }
    if !helper_available {
        return None;
    }
    let mut cmd = vec!["pkexec".to_string(), stage_helper_path().to_string()];
    // Язык сообщений помощника (решение 30.09.2026) — сразу за его путём.
    cmd.extend(crate::system_l10n::lang_args(stage_helper_path()));
    cmd.extend([category.to_string(), name.to_string(), staged.to_string()]);
    Some(cmd)
}

// ============================================================================
// Версия пакета в репозиториях — порт `_clean_pkg_version`/`query_repo_version`
// ============================================================================

/// Порт `_clean_pkg_version`: снять epoch (`1:50.3` → `50.3`) и
/// pkgrel/debian-ревизию (`6.7.3-1.1` → `6.7.3`). Пользователь ждёт
/// апстрим-версию, а не служебные счётчики порядка обновлений.
pub fn clean_pkg_version(raw: &str) -> String {
    let mut v = raw.trim();
    if let Some((_, rest)) = v.split_once(':') {
        v = rest;
    }
    if let Some((head, _)) = v.rsplit_once('-') {
        v = head;
    }
    v.trim().to_string()
}

/// Порт валидации имени пакета в `query_repo_version`
/// (`re.fullmatch(r"[A-Za-z0-9@._+-]+")`): защита от подстановки аргументов
/// даже без shell.
pub fn valid_pkg_name(pkg: &str) -> bool {
    !pkg.is_empty()
        && pkg
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '@' | '.' | '_' | '+' | '-'))
}

/// Запрос версии пакета в репозиториях — чистая часть `query_repo_version`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RepoVersionQuery {
    /// Бинарь, наличие которого оригинал проверяет `shutil.which` ДО запуска
    /// (нет бинаря → None, команда не выполняется).
    pub binary: String,
    pub cmd: Cmd,
    /// `apt-cache policy` в оригинале НЕ сверяет returncode (нет пакета →
    /// пустой stdout → и так None), у остальных `returncode != 0` = None.
    pub check_returncode: bool,
}

/// Команда за версией пакета для семейства. None — семейство без запроса
/// (gentoo/void/nix/alpine/solus/clear/unknown: UI просто скроет строку) либо
/// имя пакета не прошло валидацию.
pub fn repo_version_query(family: PkgFamily, package: &str) -> Option<RepoVersionQuery> {
    let pkg = package.trim();
    if !valid_pkg_name(pkg) {
        return None;
    }
    let (binary, argv, check_returncode): (&str, Vec<&str>, bool) = match family {
        PkgFamily::Arch => ("pacman", vec!["pacman", "-Si", pkg], true),
        PkgFamily::Debian | PkgFamily::Alt => {
            ("apt-cache", vec!["apt-cache", "policy", pkg], false)
        }
        PkgFamily::Fedora => ("dnf", vec!["dnf", "info", pkg], true),
        PkgFamily::Suse => {
            ("zypper", vec!["zypper", "--non-interactive", "info", pkg], true)
        }
        _ => return None,
    };
    Some(RepoVersionQuery {
        binary: binary.to_string(),
        // LANG=C обязателен: на русской локали менеджеры печатают «Версия»
        // вместо «Version» и парсер поля ничего не находит.
        cmd: Cmd::new(argv.into_iter().map(String::from).collect(), Some(10), true),
        check_returncode,
    })
}

/// Разбор вывода запроса версии. None — поля нет / значение пустое /
/// `(none)` у apt.
pub fn parse_repo_version(family: PkgFamily, stdout: &str) -> Option<String> {
    let non_empty = |v: String| if v.is_empty() { None } else { Some(v) };
    match family {
        PkgFamily::Arch => {
            for line in stdout.lines() {
                // Оригинал не обрезает строку: `pacman -Si` печатает поля с
                // нулевой колонки.
                if line.starts_with("Version") {
                    // В оригинале строка без ':' роняет IndexError, внешний
                    // `except` отдаёт None — здесь тот же None, без паники.
                    let (_, rest) = line.split_once(':')?;
                    return non_empty(clean_pkg_version(rest));
                }
            }
            None
        }
        PkgFamily::Debian | PkgFamily::Alt => {
            for line in stdout.lines() {
                if let Some(rest) = line.trim().strip_prefix("Candidate:") {
                    let cand = rest.trim();
                    if cand.is_empty() || cand == "(none)" {
                        return None;
                    }
                    return non_empty(clean_pkg_version(cand));
                }
            }
            None
        }
        PkgFamily::Fedora | PkgFamily::Suse => {
            for line in stdout.lines() {
                let s = line.trim();
                if s.starts_with("Version") && s.contains(':') {
                    let (_, rest) = s.split_once(':')?;
                    return non_empty(clean_pkg_version(rest));
                }
            }
            None
        }
        _ => None,
    }
}

/// Есть ли такая цель в подключённых репозиториях — ответ по ИСХОДУ той же
/// команды, что и у [`repo_version_query`]: второго похода к менеджеру этот
/// вопрос не требует.
///
/// `returncode` = `None`, когда команду запустить не удалось (менеджера нет,
/// таймаут) — тогда и ответ `None`. `None` и `Some(false)` РАЗНЫЕ: по `false`
/// цель бракуется и откат форков не запускается, по `None` решение остаётся
/// прежним, иначе на не-Arch мы отказывали бы на ровном месте.
///
/// Читать код возврата можно НЕ у всех: `apt-cache policy` отвечает нулём и
/// на несуществующий пакет (потому у [`RepoVersionQuery`] и есть поле
/// `check_returncode`), поэтому у Debian/ALT ответ даёт наличие версии в
/// выводе. Семейства без запроса вовсе отвечают `None` — «не знаю».
pub fn repo_target_exists(family: PkgFamily, returncode: Option<i32>, stdout: &str) -> Option<bool> {
    let rc = returncode?;
    match family {
        PkgFamily::Arch | PkgFamily::Fedora | PkgFamily::Suse => Some(rc == 0),
        PkgFamily::Debian | PkgFamily::Alt => {
            Some(parse_repo_version(family, stdout).is_some())
        }
        _ => None,
    }
}

// ============================================================================
// Установленный пакет: ВЕРСИЯ и СПИСОК ФАЙЛОВ
// ============================================================================
//
// Это два САМОСТОЯТЕЛЬНЫХ вопроса к движку пакетов, и ни один из них не
// сводится к уже имеющимся:
//   * «установлен ли» — [`query_cmd`] + [`pkg_installed_from_run`], ответ
//     даёт только код возврата, вывод не читается вовсе;
//   * «какая версия в РЕПОЗИТОРИЯХ» — [`repo_version_query`] +
//     [`parse_repo_version`], спрашивается у сети/кэша репозиториев.
// Здесь — «что стоит НА МАШИНЕ»: версия установленного пакета и список его
// файлов. Раньше оба вопроса задавались зашитым `pacman` мимо движка
// (`_pacman_version` / `_pacman_file_list` в `rexon/system.py`).

/// Запрос к пакетному менеджеру об установленном пакете.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct InstalledPkgQuery {
    /// Бинарь, наличие которого проверяется ДО запуска.
    pub binary: String,
    pub cmd: Cmd,
}

/// Как в выводе команды лежит версия.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VersionShape {
    /// `<имя> <версия>` — так печатает `pacman -Q`.
    NameThenVersion,
    /// Одна только версия — так печатают `dpkg-query -W -f` и `rpm -q --qf`.
    Bare,
}

/// Форма вывода команды [`installed_version_query`] для семейства.
fn installed_version_shape(family: PkgFamily) -> VersionShape {
    match family {
        PkgFamily::Arch => VersionShape::NameThenVersion,
        _ => VersionShape::Bare,
    }
}

/// Команда за версией УСТАНОВЛЕННОГО пакета.
///
/// `None` — спросить нечем, и выдумывать команду мы не будем: у gentoo,
/// alpine, void, nix, solus и clear формат ответа руками не проверен, а
/// молча разобрать чужой вывод неверно хуже, чем честно ответить «не знаю»
/// (тот же приём, что у [`repo_version_query`], который для этих семейств
/// тоже отдаёт None). Прежний код спрашивал зашитый `pacman` и на всех
/// не-Arch отвечал None и так.
pub fn installed_version_query(family: PkgFamily, package: &str) -> Option<InstalledPkgQuery> {
    let pkg = package.trim();
    if !valid_pkg_name(pkg) {
        return None;
    }
    let (binary, argv): (&str, Vec<&str>) = match family {
        PkgFamily::Arch => ("pacman", vec!["pacman", "-Q", pkg]),
        PkgFamily::Debian => (
            "dpkg-query",
            vec!["dpkg-query", "-W", "-f=${Version}", pkg],
        ),
        PkgFamily::Fedora | PkgFamily::Suse | PkgFamily::Alt => (
            "rpm",
            vec!["rpm", "-q", "--qf", "%{VERSION}-%{RELEASE}", pkg],
        ),
        _ => return None,
    };
    Some(InstalledPkgQuery {
        binary: binary.to_string(),
        // Таймаут по классу команды (опрос, 30 с) — как и было у прежнего
        // safe_run без явного числа. LANG=C: на русской локали менеджеры
        // печатают локализованные поля и ошибки, и разбор их не находит.
        cmd: Cmd::new(argv.into_iter().map(String::from).collect(), None, true),
    })
}

/// Версия установленного пакета из вывода [`installed_version_query`].
///
/// `ok` — команда завершилась нулём (не запустилась вовсе → false). Пустой
/// вывод и вывод без версии дают None: «не знаю», а не пустую строку.
pub fn parse_installed_version(family: PkgFamily, ok: bool, stdout: &str) -> Option<String> {
    if !ok {
        return None;
    }
    let value = match installed_version_shape(family) {
        VersionShape::NameThenVersion => stdout.split_whitespace().nth(1)?.to_string(),
        VersionShape::Bare => stdout.trim().to_string(),
    };
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

/// Команда за списком файлов установленного пакета. `None` — семейство, чей
/// формат ответа не проверен (см. [`installed_version_query`]).
pub fn package_files_query(family: PkgFamily, package: &str) -> Option<InstalledPkgQuery> {
    let pkg = package.trim();
    if !valid_pkg_name(pkg) {
        return None;
    }
    let (binary, argv): (&str, Vec<&str>) = match family {
        PkgFamily::Arch => ("pacman", vec!["pacman", "-Ql", pkg]),
        PkgFamily::Debian => ("dpkg", vec!["dpkg", "-L", pkg]),
        PkgFamily::Fedora | PkgFamily::Suse | PkgFamily::Alt => ("rpm", vec!["rpm", "-ql", pkg]),
        _ => return None,
    };
    Some(InstalledPkgQuery {
        binary: binary.to_string(),
        cmd: Cmd::new(argv.into_iter().map(String::from).collect(), None, true),
    })
}

/// Сырой вывод [`package_files_query`] либо None.
///
/// ⚠️ Пустой вывод при нулевом коде — это НЕ ошибка: так отвечает метапакет,
/// у которого своих файлов нет вовсе (проверено на живой машине: `pacman -Ql
/// base` печатает ноль байт с кодом 0). Прежний Python отдавал здесь `""`
/// (`r.stdout or ""`) — то же самое, и это правильно: «файлов нет» и
/// «спросить не вышло» должны различаться.
pub fn package_files_output(ok: bool, stdout: &str) -> Option<String> {
    if ok {
        Some(stdout.to_string())
    } else {
        None
    }
}

/// Пути файлов пакета из вывода [`package_files_query`].
///
/// Единственный разбор этого вывода на весь проект. Формы две: `pacman -Ql`
/// печатает `<пакет> <путь>`, а `dpkg -L` и `rpm -ql` — один только путь.
/// Общий фильтр обоих: берём абсолютные пути и выбрасываем каталоги (у них
/// хвост «/») — так же отсеиваются и служебные заголовки вроде
/// «package diverts others to:».
pub fn package_file_paths(family: PkgFamily, output: &str) -> Vec<String> {
    output
        .lines()
        .filter_map(|line| match family {
            PkgFamily::Arch => line.split_once(' ').map(|(_pkg, path)| path.trim()),
            _ => Some(line.trim()),
        })
        .filter(|path| path.starts_with('/') && !path.ends_with('/'))
        .map(str::to_string)
        .collect()
}

/// Команда «перечислить ВСЁ, что установлено» — вопрос БЕЗ аргумента-пакета.
///
/// Четвёртый самостоятельный вопрос движка: `query_cmd` спрашивает про один
/// названный пакет, а здесь имя заранее неизвестно. Проект держал этот вопрос
/// двумя копиями зашитого `pacman -Qq` мимо движка — в `detect_plasma_forks`
/// (`rexon/system.py`) и в `aur_client.installed_set`; обе теперь ходят сюда.
///
/// `None` — семейство, чей формат ответа руками не проверен (тот же приём,
/// что у [`installed_version_query`]): выдумать команду и молча разобрать
/// чужой вывод хуже, чем честно ответить «не знаю».
pub fn installed_packages_query(family: PkgFamily) -> Option<InstalledPkgQuery> {
    let (binary, argv): (&str, Vec<&str>) = match family {
        PkgFamily::Arch => ("pacman", vec!["pacman", "-Qq"]),
        PkgFamily::Debian => (
            "dpkg-query",
            vec!["dpkg-query", "-W", "-f=${Package}\n"],
        ),
        PkgFamily::Fedora | PkgFamily::Suse | PkgFamily::Alt => {
            ("rpm", vec!["rpm", "-qa", "--qf", "%{NAME}\n"])
        }
        _ => return None,
    };
    Some(InstalledPkgQuery {
        binary: binary.to_string(),
        cmd: Cmd::new(argv.into_iter().map(String::from).collect(), None, true),
    })
}

/// Имена установленных пакетов из вывода [`installed_packages_query`].
///
/// `ok` — команда завершилась нулём (не запустилась вовсе → false). Разбор
/// один на все семейства: команды выше печатают по имени в строке, а `split`
/// по пробелам переживает и лишние пустые строки. Порядок сохраняется — его
/// задают те, кому ответ нужен ([`filter_plasma_forks`] сортирует сам).
pub fn parse_installed_packages(ok: bool, stdout: &str) -> Vec<String> {
    if !ok {
        return Vec::new();
    }
    stdout.split_whitespace().map(str::to_string).collect()
}

/// Команда за МЕТАДАННЫМИ установленного пакета (`pacman -Qi`) — сырьё для
/// [`fork_stock_candidates`] и прочих читателей полей `Provides`/`Replaces`.
///
/// Только Arch, и это не лень: разбор полей ([`crate::system::parse_qi_field`])
/// написан под формат `pacman -Qi`; у `dpkg -s` и `rpm -qi` поля называются
/// иначе, и отдать их тому же разбору значило бы молча получать пустые
/// списки. На прочих семействах ответ честный — `None`.
pub fn installed_meta_query(family: PkgFamily, package: &str) -> Option<InstalledPkgQuery> {
    let pkg = package.trim();
    if family != PkgFamily::Arch || !valid_pkg_name(pkg) {
        return None;
    }
    Some(InstalledPkgQuery {
        binary: "pacman".to_string(),
        cmd: Cmd::new(
            vec!["pacman".into(), "-Qi".into(), pkg.to_string()],
            None,
            true,
        ),
    })
}

/// Команда за составом ГРУППЫ пакетов — шестой самостоятельный вопрос движка.
///
/// Ни к одному из пяти прежних он не сводится: `query_cmd` и
/// [`installed_version_query`] спрашивают про НАЗВАННЫЙ пакет,
/// [`installed_packages_query`] — про всё сразу, а здесь предмет вопроса —
/// группа, и ответом служит список её членов. Проект держал этот вопрос
/// зашитым `pacman -Qg` мимо движка в двух местах: колбэк
/// `PackageManager._pacman_query_stdout` в `rexon/system.py` и шаг «qg»
/// плана удаления здесь же, в [`remove_plan`]; обе копии теперь ходят сюда.
///
/// Только Arch, и это не лень. Понятие «группа пакетов» есть далеко не у
/// всех: у Debian его нет вовсе (метапакет — это обычный пакет, и он уже
/// отвечается [`query_cmd`]), а у dnf и zypper группы называются иначе
/// (`dnf group info`, `zypper info --type pattern`), спрашиваются другими
/// подкомандами и печатают совсем другой формат, руками не проверенный.
/// Выдумать команду и молча скормить чужой вывод разбору, написанному под
/// строки «группа член», значило бы тихо получать пустые списки вместо
/// ответа. Честный ответ этих семейств — `None`, и по нему спрашивающий
/// оставляет имя как есть: там оно и есть обычный пакет.
pub fn package_group_query(family: PkgFamily, group: &str) -> Option<InstalledPkgQuery> {
    let name = group.trim();
    if family != PkgFamily::Arch || !valid_pkg_name(name) {
        return None;
    }
    Some(InstalledPkgQuery {
        binary: "pacman".to_string(),
        cmd: Cmd::new(
            vec!["pacman".into(), "-Qg".into(), name.to_string()],
            None,
            true,
        ),
    })
}

// ============================================================================
// Предпросмотр установки: что РЕАЛЬНО поставится (pacman -Sg / -Sp)
// ============================================================================
//
// ЗАЧЕМ. Карточка стола считала ИМЕНА из базы ядра: у Pantheon это
// «pantheon lightdm lightdm-gtk-greeter» — «К установке 3 пакета», хотя
// `pantheon` — группа из 63 пакетов, а с зависимостями на чистой системе
// ставится больше 80. И установка падала целиком, когда ОДИН необязательный
// член группы в репозитории не ставится (08.10.2026:
// switchboard-plug-online-accounts ждёт пересборки под новый
// evolution-data-server), — человек узнавал об этом только из журнала
// pacman после пароля.
//
// Обе беды лечит один вопрос без прав root: `pacman -Sp --needed` печатает
// ровно то, что поставит транзакция (группы раскрыты, зависимости взяты,
// стоящее пропущено), а на неразрешимой зависимости называет виновника.
// Только Arch: у остальных семейств такой «пробной» транзакции без прав
// нет или её вывод не проверен руками — там честный ответ `None`, и
// карточка остаётся при прежнем счёте имён.

/// Сколько раз выкидывать неустановимых членов групп и спрашивать снова.
/// Каждый круг выкидывает всех найденных разом; круг больше трёх означал бы
/// цепочку поломок, которую карточка всё равно не опишет.
const PREVIEW_MAX_ROUNDS: usize = 4;

/// Итог предпросмотра установки набора пакетов.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct InstallPreview {
    /// Группы набора и число их членов (без повторов между репозиториями).
    pub groups: Vec<(String, usize)>,
    /// Что отдавать пакетному менеджеру. Совпадает с исходным набором, пока
    /// выкидывать нечего; иначе группы раскрыты в члены без [`Self::skipped`].
    pub targets: Vec<String>,
    /// Пакеты набора после раскрытия групп (то, что человек «получит» как
    /// стол, без зависимостей и без [`Self::skipped`]).
    pub members: Vec<String>,
    /// Что транзакция поставит ЗАНОВО: члены набора и их зависимости, которых
    /// в системе нет. Пусто — всё на месте.
    pub new_packages: Vec<String>,
    /// Что транзакция ОБНОВИТ: пакеты из её плана, которые уже установлены, —
    /// для них в репозитории вышла новая сборка (`--needed` пропускает только
    /// стоящее той же версии). Пропажей это не считается: установлен ли пакет,
    /// решает список установленного ([`installed_packages_query`]), а не план
    /// транзакции.
    pub updates: Vec<String>,
    /// Необязательные члены групп, которые сейчас не ставятся (неразрешимая
    /// зависимость или нет в репозитории): из установки выкинуты.
    pub skipped: Vec<String>,
    /// Пакеты, без которых установка не пройдёт: названы в наборе прямо
    /// либо поломка глубже, в зависимостях. Непусто — ставить нельзя.
    pub blocked: Vec<String>,
    /// Что из встающего конфликтует с установленным (`Conflicts With` с
    /// любой стороны). Непусто — ставить нельзя: pacman с `--noconfirm`
    /// откажется удалять установленное, а удалять чужой стол Rexon сам не
    /// берётся ([`conflict_explanation`]).
    pub conflicts: Vec<PkgConflict>,
    /// Пакеты из [`Self::targets`], которые транзакция ставит ЗАНОВО лишь как
    /// зависимости набора: DKMS-модуль и заголовки под стоящие ядра
    /// ([`kernel_module_extras`]). Ставятся помеченными `--asdeps`, чтобы
    /// удаление стола их сняло; уже стоявшие сюда не входят.
    pub deps_only: Vec<String>,
    /// Что из встающего ЗАНОВО заработает только после перезагрузки: ядро и
    /// модули ядра (DKMS или готовые сборки под ядро) — [`restart_packages`].
    /// Пусто — хватит выйти из сеанса.
    pub restart_packages: Vec<String>,
}

/// Конфликт пакетов: что встанет — и с чем установленным не уживается.
///
/// Кроме имён — чьи это пакеты: человеку говорится «Pantheon нельзя рядом с
/// Budgie», а не «mutter49 конфликтует с mutter». Пустые поля — сведений нет
/// (разбор упавшей транзакции): тогда стол угадывается по таблице ключевых
/// пакетов.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct PkgConflict {
    pub new_package: String,
    pub installed: String,
    /// Версии из `pacman -Si`/`-Qi` (как их пишет pacman), пусто — неизвестна.
    pub new_version: String,
    pub installed_version: String,
    /// Это одна программа разных версий (`mutter49` и `mutter`): тогда
    /// объяснение говорит о версиях, а не о «конфликте пакетов».
    pub same_program: bool,
    /// Столы, ради которых встаёт `new_package` (по `Depends On` набора).
    pub new_desktops: Vec<String>,
    /// Столы, которые держат установленный пакет (по `Required By`).
    pub installed_desktops: Vec<String>,
    /// Кто требует установленный пакет напрямую (`Required By`).
    pub required_by: Vec<String>,
}

impl InstallPreview {
    /// Установка пройдёт (возможно, без [`Self::skipped`]).
    pub fn installable(&self) -> bool {
        self.blocked.is_empty() && self.conflicts.is_empty()
    }

    /// Пакеты набора, которых в системе нет (ставятся заново).
    pub fn missing_members(&self) -> Vec<String> {
        self.members
            .iter()
            .filter(|m| self.new_packages.contains(m))
            .cloned()
            .collect()
    }

    /// Сколько пакетов набора уже стоит.
    pub fn installed_members(&self) -> usize {
        self.members.len() - self.missing_members().len()
    }

    /// Сколько зависимостей транзакция добавит сверх набора.
    pub fn dependency_count(&self) -> usize {
        self.new_packages.len() - self.missing_members().len()
    }
}

/// Вопрос о составе групп в РЕПОЗИТОРИЯХ (`pacman -Sg имена…`). Не-группы
/// pacman отклоняет строкой в stderr и кодом 1, но группы из того же вызова
/// всё равно печатает — поэтому код возврата разбору не важен.
pub fn sync_groups_cmd(family: PkgFamily, names: &[String]) -> Option<Cmd> {
    if family != PkgFamily::Arch || names.is_empty() || !names.iter().all(|n| valid_pkg_name(n)) {
        return None;
    }
    let mut argv: Vec<String> = vec!["pacman".into(), "-Sg".into()];
    argv.extend(names.iter().cloned());
    Some(Cmd::new(argv, None, true))
}

/// Пробная транзакция без прав root: `pacman -Sp --needed` печатает по
/// имени в строке всё, что поставится, и ничего не трогает.
pub fn sync_preview_cmd(family: PkgFamily, targets: &[String]) -> Option<Cmd> {
    if family != PkgFamily::Arch || targets.is_empty() || !targets.iter().all(|n| valid_pkg_name(n)) {
        return None;
    }
    let mut argv: Vec<String> = vec![
        "pacman".into(),
        "-Sp".into(),
        "--needed".into(),
        "--noconfirm".into(),
        "--print-format".into(),
        "%n".into(),
    ];
    argv.extend(targets.iter().cloned());
    Some(Cmd::new(argv, None, true))
}

/// Группы из вывода `pacman -Sg`: «группа член» в строке, члены без повторов
/// (одна и та же группа приходит из нескольких репозиториев), порядок
/// первого появления.
pub fn parse_sync_groups(stdout: &str) -> Vec<(String, Vec<String>)> {
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    for line in stdout.lines() {
        let mut parts = line.split_whitespace();
        let (Some(group), Some(member), None) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        if !valid_pkg_name(group) || !valid_pkg_name(member) {
            continue;
        }
        let slot = match out.iter().position(|(g, _)| g == group) {
            Some(i) => i,
            None => {
                out.push((group.to_string(), Vec::new()));
                out.len() - 1
            }
        };
        if !out[slot].1.iter().any(|m| m == member) {
            out[slot].1.push(member.to_string());
        }
    }
    out
}

/// Виновники отказа пробной транзакции (вывод под LANG=C): пакет, чья
/// зависимость неразрешима («unable to satisfy dependency 'x' required by
/// ПАКЕТ»), и имя, которого нет в репозиториях («target not found: ИМЯ»).
pub fn parse_preview_culprits(stderr: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in stderr.lines() {
        let name = if let Some((_, tail)) = line.split_once("required by ") {
            tail.trim()
        } else if let Some((_, tail)) = line.split_once("target not found: ") {
            tail.trim()
        } else {
            continue;
        };
        if valid_pkg_name(name) && !out.iter().any(|n| n == name) {
            out.push(name.to_string());
        }
    }
    out
}

/// Ответ команды для предпросмотра: (код, stdout, stderr); `None` — команда
/// не ответила вовсе.
pub type PreviewAnswer = Option<(i32, String, String)>;

/// Что поставит установка набора `packages` — и можно ли её начинать.
///
/// `run` исполняет команду (живой запуск либо подставной ответ в сверке).
/// `None` — спросить не вышло (не Arch, нет pacman, команда молчит): тогда
/// зовущий остаётся при прежнем поведении, ничего не выдумывая.
///
/// Неустановимый член ГРУППЫ выкидывается: группа — это набор «по вкусу»,
/// и стол без одного модуля настроек лучше, чем никакого стола. Названный
/// в наборе прямо пакет (сеанс, экран входа) и поломка в зависимостях
/// выкидываться не могут — тогда [`InstallPreview::blocked`].
pub fn install_preview(
    family: PkgFamily,
    packages: &[String],
    run: &dyn Fn(&Cmd) -> PreviewAnswer,
) -> Option<InstallPreview> {
    if family != PkgFamily::Arch || packages.is_empty() {
        return None;
    }
    let groups_cmd = sync_groups_cmd(family, packages)?;
    let (_, groups_out, _) = run(&groups_cmd)?;
    let groups: Vec<(String, Vec<String>)> = parse_sync_groups(&groups_out)
        .into_iter()
        .filter(|(g, _)| packages.iter().any(|p| p == g))
        .collect();
    // Набор после раскрытия групп: прямые имена плюс члены групп.
    let mut expanded: Vec<String> = Vec::new();
    for p in packages {
        match groups.iter().find(|(g, _)| g == p) {
            Some((_, members)) => {
                for m in members {
                    if !expanded.contains(m) {
                        expanded.push(m.clone());
                    }
                }
            }
            None => {
                if !expanded.contains(p) {
                    expanded.push(p.clone());
                }
            }
        }
    }
    let is_group_member = |name: &str| {
        !packages.iter().any(|p| p == name)
            && groups.iter().any(|(_, members)| members.iter().any(|m| m == name))
    };

    let mut preview = InstallPreview {
        groups: groups.iter().map(|(g, m)| (g.clone(), m.len())).collect(),
        targets: packages.to_vec(),
        members: expanded.clone(),
        ..InstallPreview::default()
    };
    // Модули ядра под стоящие ядра ([`kernel_module_extras`]): добавляются
    // в транзакцию один раз, переживают выкидывание членов групп и в
    // «пакеты стола» (`members`) не входят — это зависимости.
    let mut kernel_extras: Vec<String> = Vec::new();
    for _ in 0..=PREVIEW_MAX_ROUNDS {
        let cmd = sync_preview_cmd(family, &preview.targets)?;
        let (rc, stdout, stderr) = run(&cmd)?;
        if rc == 0 {
            let planned: Vec<String> = stdout
                .lines()
                .map(str::trim)
                .filter(|l| valid_pkg_name(l))
                .map(str::to_string)
                .collect();
            let installed = installed_names(family, run);
            if kernel_extras.is_empty() {
                let extras = kernel_module_extras(family, &planned, installed.as_deref(), run);
                if !extras.is_empty() {
                    for e in &extras {
                        if !preview.targets.contains(e) {
                            preview.targets.push(e.clone());
                        }
                    }
                    kernel_extras = extras;
                    continue;
                }
            }
            let (updates, new_packages) = split_installed(planned, installed.clone());
            // ⚠️ `-Sp` конфликтов с установленным НЕ проверяет (замер
            // 19.10.2026: Deepin при стоящем KWin — код 0), а настоящая
            // транзакция на них падает. Спрашиваем `Conflicts With` сами.
            let mut new_info: Vec<PkgInfo> = Vec::new();
            if let Some(installed) = &installed {
                let (conflicts, info) = package_conflicts(family, &new_packages, installed, run);
                preview.conflicts = conflicts;
                new_info = info;
            }
            preview.restart_packages = restart_packages(&new_packages, &new_info);
            preview.deps_only = kernel_extras.iter().filter(|e| new_packages.contains(e)).cloned().collect();
            preview.new_packages = new_packages;
            preview.updates = updates;
            return Some(preview);
        }
        // ⚠️ Строку «error: …» pacman пишет в stderr, а сами «:: unable to
        // satisfy dependency … required by …» — в stdout (замер 08.10.2026):
        // разбираются оба потока.
        let culprits = parse_preview_culprits(&format!("{stdout}\n{stderr}"));
        if culprits.is_empty() {
            // Отказ, которого разбор не понимает (база заперта, нет сети к
            // зеркалу и т.п.): ничего не утверждать.
            return None;
        }
        let (droppable, hard): (Vec<String>, Vec<String>) =
            culprits.into_iter().partition(|c| is_group_member(c));
        if !hard.is_empty() {
            preview.blocked = hard;
            preview.skipped.extend(droppable);
            return Some(preview);
        }
        preview.skipped.extend(droppable);
        preview.targets = expanded
            .iter()
            .filter(|m| !preview.skipped.contains(m))
            .cloned()
            .collect();
        preview.members = preview.targets.clone();
        for e in &kernel_extras {
            if !preview.targets.contains(e) {
                preview.targets.push(e.clone());
            }
        }
    }
    None
}

// ── Модули ядра под стоящие ядра ──────────────────────────────────────────
//
// Часть пакетов Arch зависит от виртуального модуля ядра
// (`DEEPIN-ANYTHING-MODULE`, `VHBA-MODULE`, `NVIDIA-MODULE`), а дают его два
// поставщика: готовая сборка под штатное ядро (`deepin-anything-arch`,
// `Depends On: linux`) и DKMS-вариант. pacman с `--noconfirm` берёт первого
// — и на машине с другим ядром (CachyOS: `linux-cachyos`) ставит лишнее
// ядро `linux`, которое потом висит в загрузчике (20.10.2026, Deepin).
// Поэтому: если транзакция тянет `linux`, а его нет среди стоящих ядер, —
// каждый пакет, которому нужен именно `linux`, заменяется DKMS-вариантом,
// а к нему идут заголовки КАЖДОГО стоящего ядра (иначе DKMS не соберёт).

/// Штатное ядро Arch: от него зависят готовые сборки модулей.
const ARCH_KERNEL: &str = "linux";

/// Готовые сборки модулей, чей DKMS-вариант назван не по правилу
/// «имя-dkms» / «имя без -arch, плюс -dkms».
const DKMS_ALTERNATIVES: &[(&str, &str)] = &[("virtualbox-host-modules-arch", "virtualbox-host-dkms")];

/// Кандидаты на DKMS-замену готовой сборки модуля `name` — по порядку
/// предпочтения; какие из них есть в репозиториях, решает зовущий.
pub fn dkms_candidates(name: &str) -> Vec<String> {
    let mut out: Vec<String> = DKMS_ALTERNATIVES
        .iter()
        .filter(|(prebuilt, _)| *prebuilt == name)
        .map(|(_, dkms)| dkms.to_string())
        .collect();
    if let Some(stem) = name.strip_suffix("-arch") {
        out.push(format!("{stem}-dkms"));
    }
    out.push(format!("{name}-dkms"));
    out.dedup();
    out
}

/// Нужна ли замена: транзакция тянет штатное ядро, а стоят другие.
/// Ядра не узнать (контейнер без ядра, `find` молчит) — не трогаем:
/// собирать DKMS-модуль не под что.
pub fn needs_dkms_modules(planned: &[String], kernels: &[String]) -> bool {
    planned.iter().any(|p| p == ARCH_KERNEL) && !kernels.is_empty() && !kernels.iter().any(|k| k == ARCH_KERNEL)
}

/// Готовые сборки модулей из плана: пакеты, зависящие от штатного ядра.
pub fn prebuilt_module_packages(planned_info: &[PkgInfo]) -> Vec<String> {
    planned_info
        .iter()
        .filter(|i| i.name != ARCH_KERNEL && i.depends.iter().any(|d| d == ARCH_KERNEL))
        .map(|i| i.name.clone())
        .collect()
}

/// Заголовки стоящих ядер, которых ещё нет (`linux-cachyos` →
/// `linux-cachyos-headers`).
pub fn missing_kernel_headers(kernels: &[String], installed: Option<&[String]>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for k in kernels {
        let h = format!("{k}-headers");
        if !installed.is_some_and(|i| i.contains(&h)) && !out.contains(&h) {
            out.push(h);
        }
    }
    out
}

/// Стоящие ядра — пакеты, которым принадлежат `/usr/lib/modules/*/pkgbase`
/// (так их узнают mkinitcpio и kernel-install). Пусто — узнать не вышло.
fn installed_kernels(run: &dyn Fn(&Cmd) -> PreviewAnswer) -> Vec<String> {
    let find = Cmd::new(
        names_argv(&["find", "/usr/lib/modules", "-mindepth", "2", "-maxdepth", "2", "-name", "pkgbase"]),
        None,
        true,
    );
    let Some((_, files, _)) = run(&find) else {
        return Vec::new();
    };
    let files: Vec<String> = files
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with("/usr/lib/modules/") && !l.contains(char::is_whitespace))
        .map(str::to_string)
        .collect();
    if files.is_empty() {
        return Vec::new();
    }
    let mut argv = names_argv(&["pacman", "-Qqo"]);
    argv.extend(files);
    // Чужой файл (ядро поставлено мимо pacman) pacman отвергает в stderr с
    // кодом 1, а владельцев остальных всё равно печатает.
    let Some((_, owners, _)) = run(&Cmd::new(argv, None, true)) else {
        return Vec::new();
    };
    let mut out: Vec<String> = Vec::new();
    for o in owners.lines().map(str::trim).filter(|o| valid_pkg_name(o)) {
        if !out.iter().any(|k| k == o) {
            out.push(o.to_string());
        }
    }
    out
}

fn names_argv(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

/// Что добавить в транзакцию `planned`, чтобы модули ядра встали под
/// стоящие ядра, а не потянули штатное `linux`: DKMS-варианты и заголовки.
/// Пусто — менять нечего (или заменить нечем: тогда как было).
pub fn kernel_module_extras(
    family: PkgFamily,
    planned: &[String],
    installed: Option<&[String]>,
    run: &dyn Fn(&Cmd) -> PreviewAnswer,
) -> Vec<String> {
    if family != PkgFamily::Arch || !planned.iter().any(|p| p == ARCH_KERNEL) {
        return Vec::new();
    }
    let kernels = installed_kernels(run);
    if !needs_dkms_modules(planned, &kernels) || !planned.iter().all(|n| valid_pkg_name(n)) {
        return Vec::new();
    }
    let sync_info = |names: &[String]| -> Vec<PkgInfo> {
        let mut argv = names_argv(&["pacman", "-Si"]);
        argv.extend(names.iter().cloned());
        run(&Cmd::new(argv, Some(60), true))
            .map(|(_, stdout, _)| parse_pkg_info(&stdout))
            .unwrap_or_default()
    };
    let prebuilt = prebuilt_module_packages(&sync_info(planned));
    if prebuilt.is_empty() {
        return Vec::new();
    }
    let headers = missing_kernel_headers(&kernels, installed);
    let mut wanted: Vec<String> = prebuilt.iter().flat_map(|p| dkms_candidates(p)).collect();
    wanted.extend(headers.iter().cloned());
    let available: Vec<String> = sync_info(&wanted).into_iter().map(|i| i.name).collect();
    let mut out: Vec<String> = Vec::new();
    for p in &prebuilt {
        match dkms_candidates(p).into_iter().find(|c| available.contains(c)) {
            Some(c) if !out.contains(&c) => out.push(c),
            Some(_) => {}
            // Заменить нечем — оставить pacman его выбор целиком.
            None => return Vec::new(),
        }
    }
    // Заголовков своего ядра в репозиториях нет (ядро из AUR) — DKMS под
    // него не соберёт, но и лишнего ядра не будет.
    out.extend(headers.into_iter().filter(|h| available.contains(h)));
    out
}

/// Имена из поля `Conflicts With` по блокам вывода `pacman -Si` / `-Qi`
/// (LANG=C): (пакет, с чем конфликтует). Ограничения версией (`foo<2`)
/// пропускаются: решить их без сравнения версий нельзя, а ложный отказ хуже
/// пропуска — настоящая транзакция всё равно скажет своё, и
/// [`parse_transaction_conflicts`] это объяснит.
pub fn parse_info_conflicts(stdout: &str) -> Vec<(String, Vec<String>)> {
    parse_pkg_info(stdout)
        .into_iter()
        .filter(|i| !i.conflicts.is_empty())
        .map(|i| (i.name, i.conflicts))
        .collect()
}

/// Сведения о пакете из блока `pacman -Si` / `-Qi` (LANG=C) — то, что нужно,
/// чтобы назвать конфликт столом: версия, группы, что даёт, от чего зависит,
/// кто его требует.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PkgInfo {
    pub name: String,
    pub version: String,
    pub groups: Vec<String>,
    /// `Provides` без версий (`libmutter-17.so=0-64` → `libmutter-17.so`).
    pub provides: Vec<String>,
    /// `Depends On` без ограничений версией.
    pub depends: Vec<String>,
    pub required_by: Vec<String>,
    /// `Conflicts With`; ограниченные версией (`foo<2`) пропущены.
    pub conflicts: Vec<String>,
}

/// Блоки `pacman -Si` / `-Qi` (LANG=C). Один пакет из двух репозиториев —
/// одна запись: версия и связи от первого блока, конфликты сливаются.
pub fn parse_pkg_info(stdout: &str) -> Vec<PkgInfo> {
    fn names(values: &str, strip_version: bool) -> Vec<String> {
        values
            .split_whitespace()
            .filter(|v| *v != "None")
            .filter_map(|v| {
                if strip_version {
                    v.split(['<', '>', '=']).next()
                } else if v.contains(['<', '>', '=']) {
                    None
                } else {
                    Some(v)
                }
            })
            .filter(|v| valid_pkg_name(v))
            .map(str::to_string)
            .collect()
    }
    fn extend(have: &mut Vec<String>, more: Vec<String>) {
        for v in more {
            if !have.contains(&v) {
                have.push(v);
            }
        }
    }
    let mut out: Vec<PkgInfo> = Vec::new();
    let mut cur: Option<PkgInfo> = None;
    let mut field = String::new();
    let flush = |cur: &mut Option<PkgInfo>, out: &mut Vec<PkgInfo>| {
        let Some(info) = cur.take() else { return };
        if info.name.is_empty() {
            return;
        }
        match out.iter_mut().find(|i| i.name == info.name) {
            Some(have) => extend(&mut have.conflicts, info.conflicts),
            None => out.push(info),
        }
    };
    let apply = |cur: &mut Option<PkgInfo>, key: &str, value: &str| {
        let Some(info) = cur.as_mut() else { return };
        match key {
            "Version" if info.version.is_empty() => info.version = value.trim().to_string(),
            "Groups" => extend(&mut info.groups, names(value, false)),
            "Provides" => extend(&mut info.provides, names(value, true)),
            "Depends On" => extend(&mut info.depends, names(value, true)),
            "Required By" => extend(&mut info.required_by, names(value, false)),
            "Conflicts With" => extend(&mut info.conflicts, names(value, false)),
            _ => {}
        }
    };
    for line in stdout.lines() {
        if line.starts_with(char::is_whitespace) && !line.trim().is_empty() {
            // Перенос длинного поля: продолжение того, что было выше.
            let key = field.clone();
            apply(&mut cur, &key, line);
            continue;
        }
        let Some((key, value)) = line.split_once(" : ") else {
            if line.trim().is_empty() {
                flush(&mut cur, &mut out);
                field.clear();
            }
            continue;
        };
        field = key.trim().to_string();
        if field == "Name" {
            flush(&mut cur, &mut out);
            cur = Some(PkgInfo { name: value.trim().to_string(), ..PkgInfo::default() });
            continue;
        }
        let key = field.clone();
        apply(&mut cur, &key, value);
    }
    flush(&mut cur, &mut out);
    out
}

/// Конфликты того, что встанет (`new_packages`), с установленным: в обе
/// стороны — объявленные встающими пакетами (`pacman -Si`) и объявленные
/// установленными (`pacman -Qi`). Только Arch; не ответила команда — пусто
/// (как и было: проверить не вышло — ничего не утверждаем).
/// Отдаёт ещё и сведения `pacman -Si` о встающем — по ним
/// [`restart_packages`] решает, нужна ли перезагрузка, без второго запроса.
fn package_conflicts(
    family: PkgFamily,
    new_packages: &[String],
    installed: &[String],
    run: &dyn Fn(&Cmd) -> PreviewAnswer,
) -> (Vec<PkgConflict>, Vec<PkgInfo>) {
    if family != PkgFamily::Arch || new_packages.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let installed: std::collections::HashSet<&str> = installed.iter().map(String::as_str).collect();
    let mut out: Vec<PkgConflict> = Vec::new();
    let mut add = |new_package: &str, inst: &str| {
        let c = PkgConflict {
            new_package: new_package.to_string(),
            installed: inst.to_string(),
            ..PkgConflict::default()
        };
        if new_package != inst && !out.contains(&c) {
            out.push(c);
        }
    };
    let mut new_info: Vec<PkgInfo> = Vec::new();
    if new_packages.iter().all(|n| valid_pkg_name(n)) {
        let mut argv: Vec<String> = vec!["pacman".into(), "-Si".into()];
        argv.extend(new_packages.iter().cloned());
        if let Some((_, stdout, _)) = run(&Cmd::new(argv, Some(60), true)) {
            new_info = parse_pkg_info(&stdout);
            for info in new_info.iter().filter(|i| new_packages.contains(&i.name)) {
                for c in info.conflicts.iter().filter(|c| installed.contains(c.as_str())) {
                    add(&info.name, c);
                }
            }
        }
    }
    let mut installed_info: Vec<PkgInfo> = Vec::new();
    if let Some((_, stdout, _)) = run(&Cmd::new(vec!["pacman".into(), "-Qi".into()], Some(60), true)) {
        installed_info = parse_pkg_info(&stdout);
        for info in &installed_info {
            for c in info.conflicts.iter().filter(|c| new_packages.contains(c)) {
                add(c, &info.name);
            }
        }
    }
    // Чьи это пакеты: встающий — по `Depends On` набора (кто из него его
    // тянет), установленный — по `Required By`.
    let find = |list: &[PkgInfo], name: &str| list.iter().position(|i| i.name == name);
    for c in &mut out {
        if let Some(i) = find(&new_info, &c.new_package) {
            c.new_version = new_info[i].version.clone();
        }
        if let Some(i) = find(&installed_info, &c.installed) {
            c.installed_version = installed_info[i].version.clone();
            c.required_by = installed_info[i].required_by.clone();
        }
        c.same_program = same_program(c, new_info.iter().find(|i| i.name == c.new_package));
        c.new_desktops = holding_desktops(&c.new_package, &new_info, &|n: &str| {
            let provides: Vec<&str> = new_info
                .iter()
                .find(|i| i.name == n)
                .map(|i| i.provides.iter().map(String::as_str).collect())
                .unwrap_or_default();
            new_info
                .iter()
                .filter(|i| i.depends.iter().any(|d| d == n || provides.contains(&d.as_str())))
                .map(|i| i.name.clone())
                .collect()
        });
        c.installed_desktops = holding_desktops(&c.installed, &installed_info, &|n: &str| {
            installed_info
                .iter()
                .find(|i| i.name == n)
                .map(|i| i.required_by.clone())
                .unwrap_or_default()
        });
    }
    (out, new_info)
}

/// Что из встающего заново (`new_packages`) требует перезагрузки: само ядро
/// и модули ядра — DKMS (`*-dkms`, зависимость от `dkms`) и готовые сборки
/// под штатное ядро (зависимость от `linux`). Модуль собирается при
/// установке, а грузится при старте (Deepin: `deepin-anything-dkms`), новое
/// ядро — тем более. Всё прочее (сам стол, его службы, группы) подхватывает
/// новый вход в сеанс. `new_info` — сведения `pacman -Si` о встающем (может
/// быть пуст: тогда решают одни имена).
pub fn restart_packages(new_packages: &[String], new_info: &[PkgInfo]) -> Vec<String> {
    new_packages
        .iter()
        .filter(|name| {
            let info = new_info.iter().find(|i| &i.name == *name);
            name.as_str() == ARCH_KERNEL
                || name.ends_with("-dkms")
                || info.is_some_and(|i| i.depends.iter().any(|d| d == "dkms" || d == ARCH_KERNEL))
        })
        .cloned()
        .collect()
}

/// Одна ли это программа разных версий: `mutter49` против `mutter`
/// (имя без хвостовых цифр) либо встающий пакет даёт имя установленного.
fn same_program(c: &PkgConflict, new_info: Option<&PkgInfo>) -> bool {
    let bare = |s: &str| s.trim_end_matches(|ch: char| ch.is_ascii_digit()).to_string();
    bare(&c.new_package) == c.installed
        || bare(&c.installed) == c.new_package
        || new_info.is_some_and(|i| i.provides.contains(&c.installed))
}

/// Группы pacman, по которым пакет — часть стола, сверх имён столов и их
/// наборов в базе (`system::get_packages`).
const DESKTOP_GROUPS: &[(&str, &str)] = &[("plasma", "kde")];

/// Стол, к которому пакет относится сам: ключевой пакет стола либо член его
/// группы.
fn own_desktop(name: &str, info: Option<&PkgInfo>) -> Option<&'static str> {
    if let Some(d) = conflict_desktop(name) {
        return Some(d);
    }
    let groups = info.map(|i| i.groups.as_slice()).unwrap_or_default();
    for g in groups {
        let id = DESKTOP_GROUPS.iter().find(|(grp, _)| grp == g).map(|(_, id)| *id);
        for m in crate::system::DE_META {
            let hit = id == Some(m.id)
                || g == m.id
                || crate::system::get_packages(m.id, PkgFamily::Arch).iter().any(|p| p == g);
            if hit {
                return Some(m.display_name);
            }
        }
    }
    None
}

/// Столы, которые держат пакет: обход «кто его требует» (`parents`) до
/// первого пакета, принадлежащего столу. Никто не требует — пакет стоит сам
/// по себе, и стол угадывается по нему самому. Требуют, но не столы —
/// пусто: табличное «mutter — это GNOME» соврало бы, если mutter держит
/// Budgie.
fn holding_desktops(start: &str, infos: &[PkgInfo], parents: &dyn Fn(&str) -> Vec<String>) -> Vec<String> {
    let info = |n: &str| infos.iter().find(|i| i.name == n);
    let first = parents(start);
    if first.is_empty() {
        return own_desktop(start, info(start)).map(|d| vec![d.to_string()]).unwrap_or_default();
    }
    let mut found: Vec<String> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    seen.insert(start.to_string());
    let mut queue: std::collections::VecDeque<String> = first.into_iter().collect();
    while let Some(n) = queue.pop_front() {
        if !seen.insert(n.clone()) || seen.len() > 4000 {
            continue;
        }
        if let Some(d) = own_desktop(&n, info(&n)) {
            if !found.iter().any(|f| f == d) {
                found.push(d.to_string());
            }
            continue;
        }
        queue.extend(parents(&n));
    }
    found
}

/// Конфликты из вывода НАСТОЯЩЕЙ транзакции pacman (LANG=C):
/// `:: deepin-kwin-6.0.8-12.1 and kwin-6.8.0-1.1 are in conflict…`.
/// Версия (`-версия-сборка`) отрезается.
pub fn parse_transaction_conflicts(text: &str) -> Vec<PkgConflict> {
    fn strip_version(s: &str) -> &str {
        let mut parts = s.rsplitn(3, '-');
        let (_rel, _ver, name) = (parts.next(), parts.next(), parts.next());
        name.unwrap_or(s)
    }
    let mut out: Vec<PkgConflict> = Vec::new();
    for line in text.lines() {
        let Some(head) = line.split(" are in conflict").next().filter(|_| line.contains(" are in conflict")) else {
            continue;
        };
        let head = head.trim().trim_start_matches("::").trim();
        let Some((a, b)) = head.split_once(" and ") else { continue };
        let c = PkgConflict {
            new_package: strip_version(a.trim()).to_string(),
            installed: strip_version(b.trim()).to_string(),
            ..PkgConflict::default()
        };
        if valid_pkg_name(&c.new_package) && valid_pkg_name(&c.installed) && !out.contains(&c) {
            out.push(c);
        }
    }
    out
}

/// Ключевые пакеты столов на Arch: по ним конфликт называется столом, а не
/// именем пакета. Только то, без чего стола нет (оконный менеджер, оболочка,
/// сеанс): по библиотеке стол не угадывается.
const CONFLICT_DESKTOP_PACKAGES: &[(&str, &str)] = &[
    ("kwin", "kde"),
    ("kwin-x11", "kde"),
    ("plasma-workspace", "kde"),
    ("plasma-desktop", "kde"),
    ("gnome-shell", "gnome"),
    ("mutter", "gnome"),
    ("cinnamon", "cinnamon-mint"),
    ("muffin", "cinnamon-mint"),
    ("xfwm4", "xfce"),
    ("xfce4-session", "xfce"),
    ("marco", "mate"),
    ("mate-session-manager", "mate"),
    ("budgie-desktop", "budgie"),
    ("magpie-wm", "budgie"),
    ("gala", "pantheon"),
    ("pantheon-session", "pantheon"),
    ("deepin-kwin", "deepin"),
    ("deepin-session", "deepin"),
    ("deepin-shell", "deepin"),
    ("cosmic-comp", "cosmic"),
    ("cosmic-session", "cosmic"),
    ("lxqt-session", "lxqt"),
    ("hyprland", "hyprland"),
    ("sway", "sway"),
    ("niri", "niri"),
    // river-classic и river 0.4+ взаимно исключают друг друга
    // (`conflicts=('river')`): обе стороны конфликта — стол River.
    ("river-classic", "river"),
    ("river", "river"),
    ("wayfire", "wayfire"),
];

/// Как назвать пакет человеку: роль (переводится) или собственное имя.
const CONFLICT_PACKAGE_ROLES: &[(&str, &str)] = &[("deepin-kwin", n_("the Deepin window manager"))];
const CONFLICT_PACKAGE_NAMES: &[(&str, &str)] = &[
    ("kwin", "KWin"),
    ("kwin-x11", "KWin"),
    ("mutter", "Mutter"),
    ("muffin", "Muffin"),
    ("marco", "Marco"),
    ("xfwm4", "Xfwm"),
    ("gala", "Gala"),
];

/// Конфликт, который Rexon снимает ЗАМЕНОЙ — только с согласия человека.
///
/// Обычный конфликт — чужой стол или чужая программа, и удалять их Rexon сам
/// не берётся ([`conflict_explanation`]). Здесь другое: установленный пакет —
/// другая ветка той же программы, которую ставит стол, и рядом им не
/// ужиться. Тогда карточка объясняет, что стоит и почему его надо заменить,
/// и предлагает кнопку замены ([`conflict_swaps`]); по «Да» pacman снимает
/// установленное и ставит набор в ОДНОЙ транзакции ([`install_cmd_replacing`]).
struct ConflictSwapRule {
    new_package: &'static str,
    installed: &'static str,
    /// Как назвать установленное человеку (к имени дописывается версия).
    installed_name: &'static str,
    /// Объяснение на карточке. %1 — установленное с версией («River 0.5»).
    notice: &'static str,
    /// Приписка к вопросу «Заменить?» (пусто — нет). %1 — как в `notice`.
    confirm_note: &'static str,
}

const CONFLICT_SWAPS: &[ConflictSwapRule] = &[
    // River 0.4+ из extra — «немонолитная» ветка без своего тайлинга
    // (`system.rs`, запись `river`), Rexon ставит river-classic. Пакеты
    // взаимно объявляют `Conflicts With`; `Provides` у river нет, и снятие
    // его ничего не ломает, пока его никто не требует.
    ConflictSwapRule {
        new_package: "river-classic",
        installed: "river",
        installed_name: "River",
        notice: n_(
            "⛔ You have %1 installed — the new River branch, which has no built-in tiling. \
             Rexon installs the classic River branch (river-classic) with its own tiling, \
             and for that %1 has to be replaced.",
        ),
        confirm_note: n_(
            "Your ~/.config/river/init stays as it is. If it was written for %1, the classic \
             River will run it too; if River then starts without keys, rename that file, and \
             “Check / Repair” will put the Rexon starter one in its place.",
        ),
    },
];

fn swap_rule(c: &PkgConflict) -> Option<&'static ConflictSwapRule> {
    CONFLICT_SWAPS
        .iter()
        .find(|r| r.new_package == c.new_package && r.installed == c.installed)
}

/// Короткая версия для человека: `1:0.5.0-1.1` → `0.5`.
fn short_version(v: &str) -> String {
    let v = v.split_once(':').map(|(_, rest)| rest).unwrap_or(v);
    let v = v.split('-').next().unwrap_or(v);
    v.split('.').take(2).collect::<Vec<_>>().join(".")
}

/// Установленное в замене — именем с версией («River 0.5»); версии нет —
/// именем пакета.
fn swap_label(c: &PkgConflict) -> String {
    match swap_rule(c) {
        Some(r) if !c.installed_version.is_empty() => {
            format!("{} {}", r.installed_name, short_version(&c.installed_version))
        }
        _ => c.installed.clone(),
    }
}

/// Конфликты пробы, которые снимаются заменой с согласия человека. Пусто —
/// замена не поможет: есть конфликт не из таблицы замен, установленное
/// кто-то требует (pacman снял бы и его) или набор не встанет и без
/// конфликта ([`InstallPreview::blocked`]).
pub fn conflict_swaps(p: &InstallPreview) -> Vec<PkgConflict> {
    let all = !p.conflicts.is_empty()
        && p.blocked.is_empty()
        && p.conflicts.iter().all(|c| swap_rule(c).is_some() && c.required_by.is_empty());
    if all {
        p.conflicts.clone()
    } else {
        Vec::new()
    }
}

/// Согласие на замену покрывает все конфликты пробы: тогда установка идёт
/// с заменой ([`install_cmd_replacing`]). `replace` — установленные пакеты,
/// которые человек согласился заменить.
pub fn conflict_swap_approved(p: &InstallPreview, replace: &[String]) -> bool {
    let swaps = conflict_swaps(p);
    !swaps.is_empty() && swaps.iter().all(|c| replace.contains(&c.installed))
}

fn swap_labels(swaps: &[PkgConflict]) -> String {
    let mut labels: Vec<String> = Vec::new();
    for c in swaps {
        let l = swap_label(c);
        if !labels.contains(&l) {
            labels.push(l);
        }
    }
    labels.join(", ")
}

/// Подпись кнопки замены на карточке стола.
pub fn conflict_swap_button(swaps: &[PkgConflict]) -> String {
    // TRANSLATORS: %1 is the installed program being replaced (e.g. “River 0.5”).
    trf("🔁 Replace %1", &[&swap_labels(swaps)])
}

/// Пояснение под кнопкой замены.
pub fn conflict_swap_hint(swaps: &[PkgConflict]) -> String {
    let new: Vec<&str> = swaps.iter().map(|c| c.new_package.as_str()).collect();
    trf(
        // TRANSLATORS: %1 is the installed program (e.g. “River 0.5”), %2 the
        // package installed in its place.
        "Rexon will ask first. %1 is removed and %2 installed in the same pacman \
         transaction: if the installation doesn't go through, %1 stays.",
        &[&swap_labels(swaps), &new.join(", ")],
    )
}

/// Заголовок вопроса и окна операции замены.
pub fn conflict_swap_title(swaps: &[PkgConflict]) -> String {
    // TRANSLATORS: %1 is the installed program being replaced (e.g. “River 0.5”).
    trf("Replace %1", &[&swap_labels(swaps)])
}

/// Вопрос «Заменить?»: что снимется, что встанет и что будет со столом
/// `desktop`.
pub fn conflict_swap_confirm_text(swaps: &[PkgConflict], desktop: &str) -> String {
    let new: Vec<&str> = swaps.iter().map(|c| c.new_package.as_str()).collect();
    let installed: Vec<&str> = swaps.iter().map(|c| c.installed.as_str()).collect();
    let mut text = trf(
        // TRANSLATORS: %1 is the installed program (e.g. “River 0.5”), %2 its
        // package, %3 the package installed in its place, %4 the desktop.
        "%1 (package %2) will be removed, and %3 will be installed in its place; then \
         %4 is set up as usual. It all happens in one pacman transaction: if it doesn't \
         go through, %1 stays installed.\n\nReplace it?",
        &[&swap_labels(swaps), &installed.join(", "), &new.join(", "), &desktop],
    );
    let mut notes: Vec<String> = Vec::new();
    for c in swaps {
        if let Some(r) = swap_rule(c).filter(|r| !r.confirm_note.is_empty()) {
            let note = trf(r.confirm_note, &[&swap_label(c)]);
            if !notes.contains(&note) {
                notes.push(note);
            }
        }
    }
    for n in notes {
        text.push_str("\n\n");
        text.push_str(&n);
    }
    text
}

/// Команда установки с заменой конфликтующего: тот же `pacman -S`, что
/// [`install_cmd`], плюс `--ask=4` — ответ «да» на вопрос pacman «X и Y
/// конфликтуют. Удалить Y?» (ALPM_QUESTION_CONFLICT_PKG = 1 << 2) при
/// `--noconfirm`. Снятие и установка идут одной транзакцией: не встал
/// набор — установленное остаётся. Только Arch; `replace` пуст — ровно
/// [`install_cmd`]. Какие конфликты снимаются, решено до пароля
/// ([`conflict_swap_approved`]).
pub fn install_cmd_replacing(family: PkgFamily, packages: &[String], replace: &[String]) -> Result<Vec<String>, String> {
    let mut cmd = install_cmd(family, packages)?;
    if replace.is_empty() {
        return Ok(cmd);
    }
    if family != PkgFamily::Arch {
        return Err(tr("Rexon can replace a conflicting package only with pacman."));
    }
    check_pkg_names(family, replace)?;
    let at = cmd.iter().position(|a| a == "--noconfirm").map(|i| i + 1).unwrap_or(cmd.len());
    cmd.insert(at, "--ask=4".to_string());
    Ok(cmd)
}

fn conflict_desktop(package: &str) -> Option<&'static str> {
    let id = CONFLICT_DESKTOP_PACKAGES.iter().find(|(p, _)| *p == package)?.1;
    crate::system::DE_META.iter().find(|m| m.id == id).map(|m| m.display_name)
}

/// Объяснение конфликта для человека — одно на отказ до пароля, карточку
/// стола и разбор упавшей транзакции.
pub fn conflict_explanation(conflicts: &[PkgConflict]) -> String {
    let mut lines: Vec<String> = Vec::new();
    for c in conflicts.iter().take(3) {
        // Другая ветка той же программы — своё объяснение (замена с
        // согласия, [`conflict_swaps`]).
        if let Some(rule) = swap_rule(c) {
            let line = trf(rule.notice, &[&swap_label(c)]);
            if !lines.contains(&line) {
                lines.push(line);
            }
            continue;
        }
        let join = |v: &[String]| (!v.is_empty()).then(|| v.join(", "));
        let target = join(&c.new_desktops).or_else(|| conflict_desktop(&c.new_package).map(str::to_string));
        // Кто-то требует установленный пакет, но не стол — по таблице стол не
        // угадываем (mutter держит Budgie, а не GNOME).
        let other = join(&c.installed_desktops).or_else(|| {
            c.required_by.is_empty().then(|| conflict_desktop(&c.installed).map(str::to_string)).flatten()
        });
        let line = match (target, other) {
            (Some(t), Some(o)) if t != o && c.same_program && !c.new_version.is_empty() && !c.installed_version.is_empty() => {
                let bare = |s: &str| s.trim_end_matches(|ch: char| ch.is_ascii_digit()).to_string();
                let program = if bare(&c.new_package) == c.installed { c.installed.clone() } else { bare(&c.installed) };
                trf(
                    // TRANSLATORS: %1 is the desktop being installed, %2 the installed
                    // desktop, %3 a program both need (e.g. mutter), %4 the version %1
                    // needs, %5 the package of that version, %6 the version %2 needs.
                    "⛔ %1 can't be installed next to %2: %1 needs %3 %4 (%5), and %2 needs \
                     %3 %6; the two can't be installed at once. To install %1, you'd have to \
                     remove %2 — Rexon doesn't remove another desktop by itself.",
                    &[
                        &t,
                        &o,
                        &program,
                        &major_version(&c.new_version),
                        &c.new_package,
                        &major_version(&c.installed_version),
                    ],
                )
            }
            (Some(t), Some(o)) if t != o => {
                let new_label = match CONFLICT_PACKAGE_ROLES.iter().find(|(p, _)| *p == c.new_package) {
                    Some((_, role)) => format!("{} ({})", tr(role), c.new_package),
                    None => c.new_package.clone(),
                };
                let installed_label = CONFLICT_PACKAGE_NAMES
                    .iter()
                    .find(|(p, _)| *p == c.installed)
                    .map(|(_, n)| n.to_string())
                    .unwrap_or_else(|| c.installed.clone());
                trf(
                    // TRANSLATORS: %1 is the desktop being installed, %2 the installed
                    // desktop, %3 the package of %1 (role and name), %4 the package of %2.
                    "⛔ %1 can't be installed next to %2: %3 conflicts with %4. To install %1, \
                     you'd have to remove %2 — Rexon doesn't remove another desktop by itself.",
                    &[&t, &o, &new_label, &installed_label],
                )
            }
            _ if !c.required_by.is_empty() => trf(
                // TRANSLATORS: %1 is the package being installed, %2 an installed
                // package, %3 the installed packages that need %2 (comma-separated).
                "⛔ Installation won't work: %1 conflicts with the installed package %2 \
                 (needed by %3). pacman would have to remove %2 along with them, and Rexon \
                 doesn't do that by itself.",
                &[&c.new_package, &c.installed, &c.required_by.join(", ")],
            ),
            _ => trf(
                // TRANSLATORS: %1 is the package being installed, %2 an installed package.
                "⛔ Installation won't work: %1 conflicts with the installed package %2. \
                 pacman would have to remove %2, and Rexon doesn't do that by itself.",
                &[&c.new_package, &c.installed],
            ),
        };
        if !lines.contains(&line) {
            lines.push(line);
        }
    }
    lines.join("\n")
}

/// Старшая часть версии для человека: `1:49.8-1.1` → `49`.
fn major_version(v: &str) -> String {
    let v = v.split_once(':').map(|(_, rest)| rest).unwrap_or(v);
    v.split(['.', '-', '+']).next().unwrap_or(v).to_string()
}

/// Имена установленных пакетов через тот же `run`, что и пробная
/// транзакция. `None` — спросить не вышло: тогда никто не объявляется
/// установленным.
fn installed_names(family: PkgFamily, run: &dyn Fn(&Cmd) -> PreviewAnswer) -> Option<Vec<String>> {
    let query = installed_packages_query(family)?;
    let (rc, stdout, _) = run(&query.cmd)?;
    Some(parse_installed_packages(rc == 0, &stdout)).filter(|names| !names.is_empty())
}

/// Разделить план транзакции на (установленные — обновления, новые).
/// Без списка установленного всё остаётся новым.
pub fn split_installed(planned: Vec<String>, installed: Option<Vec<String>>) -> (Vec<String>, Vec<String>) {
    let Some(installed) = installed else {
        return (Vec::new(), planned);
    };
    let installed: std::collections::HashSet<String> = installed.into_iter().collect();
    planned.into_iter().partition(|name| installed.contains(name))
}

/// Живой предпросмотр: команды идут через `utils::run` (LANG=C). Тихо, без
/// паники: ошибка запуска — `None`.
pub fn install_preview_live(family: PkgFamily, packages: &[String]) -> Option<InstallPreview> {
    crate::utils::which("pacman", None)?;
    install_preview(family, packages, &|cmd: &Cmd| {
        let report = crate::utils::run(
            &crate::utils::RunSpec {
                argv: cmd.argv.clone(),
                output: crate::utils::RunOutput::Capture,
                timeout_secs: cmd.timeout_secs,
                lang_c: cmd.lang_c,
                ..Default::default()
            },
            None,
        );
        let rc = report.returncode?;
        Some((
            rc,
            crate::utils::decode_text(&report.stdout),
            crate::utils::decode_text(&report.stderr),
        ))
    })
}

/// Разделить пакеты, помеченные в базе столов как AUR, по ФАКТУ: что есть в
/// подключённых репозиториях этой машины, а что только в AUR.
///
/// Метка «AUR» в таблице — свойство чистого Arch, а не машины: на CachyOS
/// темы и значки Mint лежат в репозитории `cachyos`. Без этой сверки на
/// машине без yay/paru такие пакеты молча выпадали бы вместе с AUR-списком,
/// хотя pacman поставил бы их сам. Вопрос — тот же `pacman -Si`, что у
/// [`repo_version_query`]; ответ «не знаю» (`None`, команда не ответила)
/// оставляет пакет в AUR, как было.
///
/// Возвращает (из репозиториев, только AUR), порядок сохраняется.
pub fn split_repo_available(
    family: PkgFamily,
    names: &[String],
    run: &dyn Fn(&Cmd) -> PreviewAnswer,
) -> (Vec<String>, Vec<String>) {
    let mut repo = Vec::new();
    let mut aur = Vec::new();
    for name in names {
        let in_repo = repo_version_query(family, name).and_then(|q| {
            let (rc, stdout, _) = run(&q.cmd)?;
            repo_target_exists(family, Some(rc), &stdout)
        });
        if in_repo == Some(true) {
            repo.push(name.clone());
        } else {
            aur.push(name.clone());
        }
    }
    (repo, aur)
}

/// Команда за пакетами НЕ ИЗ РЕПОЗИТОРИЕВ — седьмой самостоятельный вопрос
/// движка.
///
/// Предмет вопроса — ПРОИСХОЖДЕНИЕ уже установленного: собран ли пакет на
/// месте (AUR, свои сборки) или пришёл из репозитория дистрибутива. Ни к
/// одному прежнему вопросу он не сводится: [`installed_packages_query`]
/// перечисляет всё установленное, ничего не говоря о происхождении, а
/// [`repo_version_query`] спрашивает про НАЗВАННЫЙ пакет и ходит в базу
/// репозиториев. Спрашивает его один читатель — классификация тем
/// «Системные / AUR» во вкладке «Оформление»
/// (`rexon/appearance/installed_scan.py`), разбор ответа —
/// [`crate::installed_scan::parse_qm_output`], политика повторов при занятой
/// базе — [`crate::installed_scan::foreign_pkgs_from_attempts`].
///
/// Только Arch, и это не лень: понятия «пакет не из репозиториев» у
/// остальных семейств в том же смысле НЕТ.
/// * У Debian его нет вовсе. `dpkg` знает лишь то, что пакет установлен, и
///   не помнит, откуда он взялся; а `dpkg --print-foreign-architectures` —
///   ложный друг: «foreign» там про АРХИТЕКТУРУ (i386 рядом с amd64), а не
///   про происхождение. Ближайший по смыслу ответ дают `aptitude search
///   '~i!~ODebian'` и `apt-forktracer`, но ни того, ни другого в базовой
///   установке нет.
/// * У dnf ответ есть — `dnf repoquery --extras`, — но печатает он полные
///   NEVRA (`имя-версия-релиз.арх`), а не «имя версия», и разбор, писанный
///   под `pacman -Qm`, молча вернул бы имена с версией внутри.
/// * У zypper ближайшее — `zypper packages --orphaned`, и это другой вопрос
///   («пакет, которого больше нет ни в одном репозитории»), да и формат —
///   таблица с рамками.
/// Выдумать команду и скормить чужой вывод чужому разбору значило бы тихо
/// зачислить AUR-темы в «Системные» — ровно то, от чего исключение AUR и
/// заводилось. Честный ответ этих семейств — `None`, и по нему спрашивающий
/// не показывает «Системные» вовсе.
pub fn foreign_packages_query(family: PkgFamily) -> Option<InstalledPkgQuery> {
    if family != PkgFamily::Arch {
        return None;
    }
    Some(InstalledPkgQuery {
        binary: "pacman".to_string(),
        // Таймаут по классу команды (опрос, 30 с). LANG=C: русская локаль
        // переводит сообщение о занятой базе, а политика повторов ищет в нём
        // "lck"/"lock" — без LANG=C повтор молча не случался бы.
        cmd: Cmd::new(vec!["pacman".into(), "-Qm".into()], None, true),
    })
}

/// Сколько путей уходит в ОДИН вопрос о владельце файла.
///
/// Число — защита от ARG_MAX, а не от менеджера: сотни коротких путей в него
/// не упираются, но на машине с тысячами тем упереться можно. Резать на
/// пачки — дело того, кто собирает argv, поэтому число живёт здесь, рядом с
/// командой (прежде оно было константой `_QO_BATCH` у исполнителя, в
/// `rexon/appearance/installed_scan.py`).
pub const FILE_OWNER_BATCH: usize = 200;

/// Таймаут ОДНОГО вопроса о владельце файла, секунды.
///
/// Здесь своё число, а не класс: класс «опрос» (30 с) откалиброван на вопрос
/// об ОДНОМ пакете, а эта команда отвечает сразу про две сотни путей. Число
/// взято из оригинала (`subprocess.run(..., timeout=60)`), измеренная работа
/// на этой машине — около 0,12 с на пачку.
pub const FILE_OWNER_TIMEOUT_SECS: u64 = 60;

/// Годится ли путь для вопроса о владельце файла.
///
/// Абсолютный: относительный менеджер считал бы от текущего каталога, а он у
/// нас не задан. Тем же условием отсекается и ведущий `-` — иначе путь уехал
/// бы менеджеру как ключ. Перенос строки разорвал бы строку ответа надвое, и
/// разбор молча приписал бы файл чужому пакету; нулевой байт argv не
/// переживёт вовсе.
fn valid_owner_path(path: &str) -> bool {
    path.starts_with('/') && !path.contains('\n') && !path.contains('\0')
}

/// Команды за ВЛАДЕЛЬЦЕМ файлов — восьмой самостоятельный вопрос движка.
///
/// Предмет вопроса — ПУТЬ, а не имя пакета, и ответ идёт в обратную сторону:
/// [`package_files_query`] спрашивает «какие файлы у этого пакета», здесь —
/// «чей этот файл». Спрашивает его тот же единственный читатель, что и
/// [`foreign_packages_query`]; разбор ответа —
/// [`crate::installed_scan::parse_qo_output`].
///
/// Ответ — СПИСОК команд: путей бывают тысячи, и режет их на пачки по
/// [`FILE_OWNER_BATCH`] тот, кто собирает argv. Пустой список значит «годных
/// путей не осталось» и запускать нечего; `None` — семейство, которое этого
/// вопроса не понимает, и это разные вещи.
///
/// Только Arch, и это не лень: формат ответа у остальных другой, а разбор на
/// весь проект один. `dpkg -S` печатает «пакет: путь» — порядок обратный, и
/// на путь-символическую ссылку отвечает несколькими строками; `rpm -qf`
/// печатает только NEVRA пакета, вовсе без пути, так что сопоставить ответ с
/// вопросом можно лишь по порядку строк. Ни то, ни другое руками не
/// проверено, и молча скормить их разбору, писанному под «путь is owned by
/// пакет версия», значило бы получать пустую карту владельцев — то есть
/// пустой список «Системных» вместо ответа. Честный ответ — `None`.
pub fn file_owner_queries(
    family: PkgFamily,
    paths: &[String],
) -> Option<Vec<InstalledPkgQuery>> {
    if family != PkgFamily::Arch {
        return None;
    }
    let good: Vec<&String> = paths.iter().filter(|p| valid_owner_path(p)).collect();
    Some(
        good.chunks(FILE_OWNER_BATCH)
            .map(|chunk| {
                let mut argv: Vec<String> = vec!["pacman".into(), "-Qo".into()];
                argv.extend(chunk.iter().map(|p| (*p).clone()));
                InstalledPkgQuery {
                    binary: "pacman".to_string(),
                    // LANG=C обязателен: на русской локали ответ звучит
                    // «принадлежит пакету», и разбор не находит ничего.
                    cmd: Cmd::new(argv, Some(FILE_OWNER_TIMEOUT_SECS), true),
                }
            })
            .collect(),
    )
}

// ============================================================================
// Ожидание замка пакетного менеджера — чистый шаг цикла
// ============================================================================

/// Как часто перепроверять замок (секунды). Меньше нельзя: hook'и snap-pac
/// держат замок ещё около полуминуты после конца транзакции, и опрос чаще
/// только греет процессор.
pub const PKG_LOCK_POLL_SECS: u64 = 2;
/// Сколько всего ждать замок (секунды).
pub const PKG_LOCK_TIMEOUT_SECS: u64 = 300;
/// Стадия, под которой ожидание показывается в окне прогресса.
pub const PKG_LOCK_WAIT_STAGE: &str = "wait_lock";
/// Что сказать человеку, когда ждём чужую транзакцию.
pub fn pkg_lock_wait_message() -> String {
    tr("⏳ Waiting for the package manager to become free (another transaction \
        is still running its hooks)…")
}
/// Ответ «замок свободен» — им же отвечают семейства без замка вовсе.
pub const PKG_LOCK_OK: &str = "ok";

/// Что делать на очередном обороте ожидания замка.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum LockWaitStep {
    /// Замок свободен — идти дальше.
    Ready { message: String },
    /// Не дождались: отдать отказ с этим текстом.
    TimedOut { message: String },
    /// Сказать человеку, что ждём, и подождать. Говорится ОДИН раз.
    Notify {
        stage: String,
        message: String,
        poll_secs: u64,
    },
    /// Просто подождать.
    Wait { poll_secs: u64 },
}

/// Чистый шаг ожидания замка — общий для установки пакетов и для пересборки
/// оформления (правило «один класс — один движок»: копий цикла было две).
///
/// `locked` — существует ли хоть один файл из [`lock_paths`] (для семейства
/// без замка вызывающий передаёт false и получает `Ready` сразу);
/// `elapsed_secs` — сколько уже ждём; `notified` — говорили ли уже человеку.
pub fn lock_wait_step(
    locked: bool,
    elapsed_secs: f64,
    timeout_secs: u64,
    notified: bool,
) -> LockWaitStep {
    if !locked {
        return LockWaitStep::Ready {
            message: PKG_LOCK_OK.to_string(),
        };
    }
    if elapsed_secs >= timeout_secs as f64 {
        return LockWaitStep::TimedOut {
            message: pkg_lock_timeout_message(timeout_secs),
        };
    }
    if !notified {
        return LockWaitStep::Notify {
            stage: PKG_LOCK_WAIT_STAGE.to_string(),
            message: pkg_lock_wait_message(),
            poll_secs: PKG_LOCK_POLL_SECS,
        };
    }
    LockWaitStep::Wait {
        poll_secs: PKG_LOCK_POLL_SECS,
    }
}

/// Побочные действия ожидания замка: посмотреть ФС, поспать, сказать человеку.
///
/// Ядро само в ФС не лезет и само не спит — тот же приём, что у `DaemonFs`:
/// снаружи транспорт, внутри порядок. Благодаря этому цикл ожидания
/// проверяется без единой настоящей секунды сна.
pub trait LockWaitIo {
    /// Существует ли хоть один из lock-файлов.
    fn any_locked(&mut self, paths: &[String]) -> bool;
    /// Сколько секунд прошло с начала ожидания.
    fn elapsed_secs(&mut self) -> f64;
    /// Поспать перед следующим оборотом.
    fn sleep_secs(&mut self, secs: u64);
    /// Сказать человеку, что ждём чужую транзакцию. Зовётся ОДИН раз.
    fn notify(&mut self, stage: &str, message: &str);
}

/// Ждать освобождения замка пакетного менеджера. → `(свободен, сообщение)`.
///
/// ЗАЧЕМ ЭТО ЗДЕСЬ, А НЕ В PYTHON. Решение (`lock_wait_step`) жило в ядре
/// давно, а крутил цикл шим: `os.path.exists` в списковом выражении и
/// `time.sleep`. Пока это так, ни один Rust-потребитель — включая хозяина
/// главного окна — не мог дождаться замка сам и упирался в границу.
///
/// ⚠️ Ждать приходится по-настоящему: snap-pac держит `/var/lib/pacman/db.lck`
/// ещё около тридцати секунд ПОСЛЕ того, как транзакция закончилась (post-hook
/// снимает снимок btrfs). Установка, начатая сразу, падала бы «база занята».
pub fn wait_for_pkg_lock(
    io: &mut dyn LockWaitIo,
    paths: &[String],
    timeout_secs: u64,
) -> (bool, String) {
    let mut notified = false;
    loop {
        // Семейство без замка: `paths` пуст — ответ «свободен» на первом обороте.
        let locked = !paths.is_empty() && io.any_locked(paths);
        match lock_wait_step(locked, io.elapsed_secs(), timeout_secs, notified) {
            LockWaitStep::Ready { message } => return (true, message),
            LockWaitStep::TimedOut { message } => return (false, message),
            LockWaitStep::Notify {
                stage,
                message,
                poll_secs,
            } => {
                notified = true;
                io.notify(&stage, &message);
                io.sleep_secs(poll_secs);
            }
            LockWaitStep::Wait { poll_secs } => io.sleep_secs(poll_secs),
        }
    }
}

/// Живые побочные действия ожидания: настоящие ФС, часы и сон.
///
/// `notify` — куда сказать человеку; у потребителя без окна прогресса это
/// пустое замыкание.
pub struct LiveLockWait<N: FnMut(&str, &str)> {
    start: std::time::Instant,
    notify: N,
}

impl<N: FnMut(&str, &str)> LiveLockWait<N> {
    /// Отсчёт времени начинается здесь.
    pub fn new(notify: N) -> Self {
        Self {
            start: std::time::Instant::now(),
            notify,
        }
    }
}

impl<N: FnMut(&str, &str)> LockWaitIo for LiveLockWait<N> {
    fn any_locked(&mut self, paths: &[String]) -> bool {
        paths.iter().any(|p| std::path::Path::new(p).exists())
    }

    fn elapsed_secs(&mut self) -> f64 {
        self.start.elapsed().as_secs_f64()
    }

    fn sleep_secs(&mut self, secs: u64) {
        std::thread::sleep(std::time::Duration::from_secs(secs));
    }

    fn notify(&mut self, stage: &str, message: &str) {
        (self.notify)(stage, message);
    }
}

// ============================================================================
// Живой ход установки: чтение растущего журнала
// ============================================================================
//
// ЗАЧЕМ. Главный шаг установки идёт под `pkexec` с захваченным выводом: пока
// он не кончится, человек не видит ни строки и не отличает работу от
// зависания. Поэтому вывод уходит в файл, а отдельный поток дочитывает его на
// ходу и выносит наверх строки-стадии.
//
// ⚠️ ЧИТАТЕЛЕЙ БЫЛО ДВА, И ЭТО БЫЛА ОДНА И ТА ЖЕ РАБОТА, НАПИСАННАЯ ДВАЖДЫ:
// установка из репозиториев (`_tail_pkg_log`) и сборка из AUR
// (`_run_aur_with_progress`) держали каждый свой поток, свой буфер, свой
// разбор остатка и свой интервал опроса. Различались они ровно одним — тем,
// что делать с очередной СТРОКОЙ. Здесь движок один, а различие вынесено в
// [`LogTailKind`] (правило «один класс — один движок»).

/// Маркеры строк сборки AUR, которые стоит показать человеку: заголовки
/// стадий `makepkg` (`==>`), сообщения помощника (`::`) и переходы (` -> `).
/// Всё остальное — шум компилятора на десятки тысяч строк.
pub const AUR_PROGRESS_MARKERS: &[&str] = &["==>", "::", " -> "];

/// Служебная метка кода возврата в журнале установки: она для скрипта, а не
/// для человека.
pub const LOG_EXIT_MARKER: &str = "__REXON_EXIT__";

/// Чей журнал читаем — от этого зависит, что делать со строкой.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogTailKind {
    /// Установка из репозиториев: строка разбирается на стадию и сообщение,
    /// неразобранная показывается как есть.
    Pacman,
    /// Сборка из AUR: показываются только строки-стадии, остальное — шум.
    Aur,
    /// Пересборка оформления AeroShell: каждая непустая строка сборщика
    /// идёт в окно хода как есть (`aeroshell::rebuild_line`), как у Python.
    Build,
    /// Каждая непустая строка как есть — для того, кто разбирает вывод сам
    /// (стадии установки AUR, `aur_install::stage_of_line`: строки pacman
    /// «loading packages...», «installing x...» меток `Aur` не несут).
    Raw,
}

impl LogTailKind {
    /// Имя для моста в Python.
    pub fn as_str(self) -> &'static str {
        match self {
            LogTailKind::Pacman => "pacman",
            LogTailKind::Aur => "aur",
            LogTailKind::Build => "build",
            LogTailKind::Raw => "raw",
        }
    }

    /// Как часто заглядывать в файл, миллисекунды.
    pub fn poll_ms(self) -> u64 {
        match self {
            LogTailKind::Pacman => 100,
            LogTailKind::Aur | LogTailKind::Build | LogTailKind::Raw => 300,
        }
    }
}

/// Строка журнала → `(стадия, сообщение)` либо `None` — показывать нечего.
pub fn log_tail_line(kind: LogTailKind, raw: &str) -> Option<(String, String)> {
    match kind {
        LogTailKind::Pacman => {
            let line = raw.trim_end();
            if line.is_empty() || line.starts_with(LOG_EXIT_MARKER) {
                return None;
            }
            Some(match parse_pacman_line(line) {
                Some(p) => (p.stage, p.message),
                None => ("raw".to_string(), line.to_string()),
            })
        }
        LogTailKind::Aur => {
            let line = raw.trim();
            if line.is_empty() || !AUR_PROGRESS_MARKERS.iter().any(|m| line.contains(m)) {
                return None;
            }
            Some(("aur".to_string(), format!("    {line}")))
        }
        LogTailKind::Build => crate::aeroshell::rebuild_line(raw)
            .map(|line| (crate::aeroshell::REBUILD_LINE_STAGE.to_string(), line)),
        LogTailKind::Raw => {
            let line = raw.trim_end();
            (!line.trim().is_empty()).then(|| ("raw".to_string(), line.to_string()))
        }
    }
}

/// Живой читатель растущего журнала. Останавливается [`LogTail::stop`].
pub struct LogTail {
    done: std::sync::Arc<std::sync::atomic::AtomicBool>,
    handle: Option<std::thread::JoinHandle<Vec<String>>>,
}

impl LogTail {
    /// Остановить чтение, дочитать остаток и отдать показанные строки.
    pub fn stop(&mut self) -> Vec<String> {
        self.done.store(true, std::sync::atomic::Ordering::SeqCst);
        match self.handle.take() {
            Some(h) => h.join().unwrap_or_default(),
            None => Vec::new(),
        }
    }
}

impl Drop for LogTail {
    /// Забытый читатель не должен пережить установку: поток демонический по
    /// смыслу, но висящий файловый дескриптор нам не нужен.
    fn drop(&mut self) {
        if self.handle.is_some() {
            self.stop();
        }
    }
}

/// Запустить чтение растущего журнала. `emit` зовётся на каждую показываемую
/// строку `(стадия, сообщение)`.
///
/// ⚠️ Файл читается БАЙТАМИ и режется по `\n`: чтение текстом с произвольной
/// позиции — неопределённое поведение, а очередной кусок может оборваться
/// посреди многобайтового символа. Хвост без перевода строки остаётся в
/// буфере до следующего оборота и дочитывается в самом конце.
pub fn tail_log(
    path: &str,
    kind: LogTailKind,
    mut emit: impl FnMut(&str, &str) + Send + 'static,
) -> LogTail {
    use std::io::{Read, Seek, SeekFrom};

    let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = done.clone();
    let path = path.to_string();
    let handle = std::thread::spawn(move || {
        let mut shown: Vec<String> = Vec::new();
        let mut pos: u64 = 0;
        let mut buf: Vec<u8> = Vec::new();

        let read_new = |pos: &mut u64| -> Vec<u8> {
            let Ok(mut f) = std::fs::File::open(&path) else {
                return Vec::new();
            };
            if f.seek(SeekFrom::Start(*pos)).is_err() {
                return Vec::new();
            }
            let mut chunk = Vec::new();
            if f.read_to_end(&mut chunk).is_err() {
                return Vec::new();
            }
            *pos += chunk.len() as u64;
            chunk
        };

        let flush = |raw: &[u8], shown: &mut Vec<String>, emit: &mut dyn FnMut(&str, &str)| {
            for part in raw.split(|b| *b == b'\n') {
                let line = String::from_utf8_lossy(part);
                if let Some((stage, message)) = log_tail_line(kind, &line) {
                    shown.push(message.clone());
                    emit(&stage, &message);
                }
            }
        };

        loop {
            let stopping = flag.load(std::sync::atomic::Ordering::SeqCst);
            buf.extend_from_slice(&read_new(&mut pos));
            if let Some(idx) = buf.iter().rposition(|b| *b == b'\n') {
                let rest = buf.split_off(idx + 1);
                flush(&buf, &mut shown, &mut emit);
                buf = rest;
            }
            if stopping {
                // Последний оборот: хвост без перевода строки тоже показываем.
                flush(&buf, &mut shown, &mut emit);
                return shown;
            }
            std::thread::sleep(std::time::Duration::from_millis(kind.poll_ms()));
        }
    });
    LogTail {
        done,
        handle: Some(handle),
    }
}

// ============================================================================
// Тяжёлые форки Plasma (AeroThemePlasma / AeroShell) — детект и откат на сток
// ============================================================================
//
// Форки ЗАМЕНЯЮТ бинарники/QML Plasma, поэтому сброс темы их не убирает —
// лечится только pacman'ом. Провайдера стокового пакета нельзя `-R` (его
// держат обратные зависимости), нужен `pacman -S <сток>` одной транзакцией.

/// Префиксы имён пакетов-форков (покрывают -git/-x11-git варианты).
pub const PLASMA_FORK_PREFIXES: &[&str] = &["aerothemeplasma-", "aeroshell-"];
/// Известные форки вне префиксов.
pub const PLASMA_FORK_EXTRA: &[&str] = &["uac-polkit-agent-git"];
/// ЗАПАСНАЯ карта «стоковый пакет ← префиксы форков-провайдеров». Основной
/// путь (`fork_stock_candidates`) берёт имена из живых provides/replaces
/// pacman; карта добавляет своего кандидата последним — на случай, когда
/// метаданные форка пусты. Любое имя отсюда всё равно проходит проверку
/// существования в репозиториях, поэтому устареть незаметно оно не может.
pub const PLASMA_FORK_STOCK_REPLACEMENTS: &[(&str, &[&str])] = &[
    ("libplasma", &["aeroshell-libplasma"]),
    ("polkit-kde-agent", &["uac-polkit-agent"]),
];

fn is_plasma_fork_name(name: &str) -> bool {
    PLASMA_FORK_PREFIXES.iter().any(|p| name.starts_with(p))
        || PLASMA_FORK_EXTRA.contains(&name)
}

/// Порт `filter_plasma_forks`: отобрать форки из списка имён установленных
/// пакетов. Отсортировано, без дедупликации (как `sorted(генератор)`).
pub fn filter_plasma_forks(package_names: &[String]) -> Vec<String> {
    let mut out: Vec<String> = package_names
        .iter()
        .filter(|p| is_plasma_fork_name(p))
        .cloned()
        .collect();
    out.sort();
    out
}

/// Значения поля `pacman -Qi <pkg>` для module-level функций откатa форков —
/// порт `_pacman_qi_field`/`_qi_field` (вне экземпляра менеджера, поэтому
/// семейство фиксировано Arch: на прочих pacman'а нет и колбэк отдаёт None).
fn pacman_qi_field(
    package: &str,
    field: &str,
    qi_output: &dyn Fn(&str) -> Option<String>,
) -> Vec<String> {
    qi_field(PkgFamily::Arch, package, field, qi_output)
}

/// Имена-кандидаты в стоковую замену форку, в порядке доверия.
///
/// ⭐ Порядок именно такой, и это главное место всей починки:
///   1. `Provides` — что форк подменяет СЕЙЧАС. Сборка форка обязана держать
///      это поле в актуальном виде, иначе её собственные обратные зависимости
///      не разрешатся, — значит имя здесь живое по построению.
///   2. `Replaces` — список ИСТОРИЧЕСКИХ имён: он копится годами и хранит
///      мёртвые названия. Ровно на этом сломался откат: AeroShell тащит
///      `replaces=(plasma-framework)` с времён Plasma 5, KDE давно
///      переименовала пакет в `libplasma`, и `pacman -S plasma-framework`
///      падал «не найдена цель».
///   3. Запасная карта — только если метаданных не оказалось вовсе.
///
/// Отсеиваются сам форк и любые имена его же семейства (`aeroshell-…`):
/// стоком они быть не могут. Дубликаты убираются с сохранением порядка.
pub fn fork_stock_candidates(
    fork: &str,
    qi_output: &dyn Fn(&str) -> Option<String>,
) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let push = |name: String, out: &mut Vec<String>| {
        if name.is_empty() || name == fork || is_plasma_fork_name(&name) {
            return;
        }
        if !out.contains(&name) {
            out.push(name);
        }
    };
    for field in ["Provides", "Replaces"] {
        for name in pacman_qi_field(fork, field, qi_output) {
            push(name, &mut out);
        }
    }
    for (stock, prefixes) in PLASMA_FORK_STOCK_REPLACEMENTS {
        if prefixes.iter().any(|p| fork.starts_with(p)) {
            push((*stock).to_string(), &mut out);
        }
    }
    out
}

/// Чем закончился поиск стокового пакета под конкретный форк.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind")]
pub enum ForkStockTarget {
    /// Автономный форк: стокового аналога нет, снимается прямым `-Rns`.
    Standalone,
    /// Сток найден И существует в подключённых репозиториях.
    Found { package: String },
    /// Форк объявляет замену, но НИ ОДНОГО из имён репозитории не знают.
    /// Откат по такому форку невозможен — вернуть будет нечего.
    Unresolved { candidates: Vec<String> },
}

/// Стоковый пакет, которым форк заменяется при откате.
///
/// `target_exists(имя)` — существует ли такая цель в репозиториях:
/// `Some(true)` есть, `Some(false)` нет, `None` — проверить НЕ УДАЛОСЬ
/// (pacman недоступен, база не синхронизирована). Непроверяемого кандидата
/// не бракуем: без pacman мы всё равно ничего не запустим, а на слепой
/// отказ пользователь ругаться будет справедливо. Бракуем только тогда, когда
/// pacman ответил внятно и ответ отрицательный по ВСЕМ именам.
pub fn fork_stock_target(
    fork: &str,
    qi_output: &dyn Fn(&str) -> Option<String>,
    target_exists: &dyn Fn(&str) -> Option<bool>,
) -> ForkStockTarget {
    let candidates = fork_stock_candidates(fork, qi_output);
    if candidates.is_empty() {
        return ForkStockTarget::Standalone;
    }
    let mut unchecked: Option<String> = None;
    for name in &candidates {
        match target_exists(name) {
            Some(true) => return ForkStockTarget::Found { package: name.clone() },
            Some(false) => {}
            None => {
                if unchecked.is_none() {
                    unchecked = Some(name.clone());
                }
            }
        }
    }
    match unchecked {
        Some(package) => ForkStockTarget::Found { package },
        None => ForkStockTarget::Unresolved { candidates },
    }
}

/// Форк, под который сток не нашёлся, — с именами, которые он предлагал.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UnresolvedFork {
    pub fork: String,
    pub candidates: Vec<String>,
}

/// Разбиение набора форков на шаги отката.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlasmaForkSplit {
    /// Автономные форки — прямое `pacman -Rns`.
    pub removable: Vec<String>,
    /// Стоковые пакеты под форки-провайдеры — `pacman -S` (провайдера нельзя
    /// `-R`: его держат обратные зависимости, транзакция оборвётся).
    pub stock: Vec<String>,
    /// Форки, для которых сток не нашёлся: откат по ним запрещён.
    pub unresolved: Vec<UnresolvedFork>,
}

/// Порт `_plasma_fork_split`. Решение по КАЖДОМУ форку берётся из его
/// метаданных pacman и проверяется по репозиториям, поэтому новый
/// форк-провайдер (и следующее переименование в KDE) обрабатывается верно
/// без правки кода.
pub fn plasma_fork_split(
    forks: &[String],
    qi_output: &dyn Fn(&str) -> Option<String>,
    target_exists: &dyn Fn(&str) -> Option<bool>,
) -> PlasmaForkSplit {
    let mut out = PlasmaForkSplit {
        removable: Vec::new(),
        stock: Vec::new(),
        unresolved: Vec::new(),
    };
    for f in forks {
        match fork_stock_target(f, qi_output, target_exists) {
            ForkStockTarget::Found { package } => {
                if !out.stock.contains(&package) {
                    out.stock.push(package);
                }
            }
            ForkStockTarget::Standalone => {
                if !out.removable.contains(f) {
                    out.removable.push(f.clone());
                }
            }
            ForkStockTarget::Unresolved { candidates } => {
                if !out.unresolved.iter().any(|u| &u.fork == f) {
                    out.unresolved.push(UnresolvedFork {
                        fork: f.clone(),
                        candidates,
                    });
                }
            }
        }
    }
    out
}

/// Порт `plasma_fork_recovery_command`: человекочитаемая команда полного
/// отката (для показа в UI/CLI). None — форков нет либо действий нет.
/// В команду попадают только существующие цели: показать пользователю
/// `pacman -S` с несуществующим пакетом — это и есть тот мусор, который он
/// увидел в диалоге.
pub fn plasma_fork_recovery_command(
    forks: &[String],
    qi_output: &dyn Fn(&str) -> Option<String>,
    target_exists: &dyn Fn(&str) -> Option<bool>,
) -> Option<String> {
    if forks.is_empty() {
        return None;
    }
    let split = plasma_fork_split(forks, qi_output, target_exists);
    let mut parts: Vec<String> = Vec::new();
    if !split.removable.is_empty() {
        parts.push(format!("sudo pacman -Rns {}", split.removable.join(" ")));
    }
    if !split.stock.is_empty() {
        parts.push(format!("sudo pacman -S {}", split.stock.join(" ")));
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("  &&  "))
    }
}

/// Объяснение отказа: почему откат не запускается и что делать человеку.
/// Без единого слова из вывода pacman — пользователю оно ничего не говорит.
pub fn plasma_fork_unresolved_message(unresolved: &[UnresolvedFork]) -> String {
    let mut s = format!(
        "{}\n\n",
        tr("The rollback didn't start: no stock package was found to replace the fork.")
    );
    for u in unresolved {
        let names = if u.candidates.is_empty() {
            tr("(the fork didn't name any package)")
        } else {
            u.candidates.join(", ")
        };
        s.push_str(&format!(
            "  • {}\n",
            trf("%1 — not in the repositories: %2", &[&u.fork, &names])
        ));
    }
    s.push('\n');
    s.push_str(&tr(
        "Usually this means KDE renamed the stock package, while the fork build \
         still uses the old name.\n\
         Nothing was removed — your system is untouched.\n\n\
         What to do: refresh the package database (System Update) and try \
         again. If the name still can't be found, the fork needs a fresh \
         rebuild — it will bring the correct stock name.",
    ));
    s
}

/// Объяснение поверх сырого вывода pacman, когда транзакция всё же упала.
/// `remaining` — команда, которую осталось выполнить (пересчитанная ПОСЛЕ
/// падения: часть шагов могла пройти).
pub fn plasma_fork_failure_message(raw: &str, remaining: Option<&str>) -> String {
    let raw = raw.trim();
    let mut s = format!("{}\n\n", tr("pacman didn't finish the rollback."));
    if !raw.is_empty() {
        s.push_str(&format!("{}\n\n", trf("What pacman said:\n%1", &[&raw])));
    }
    match remaining {
        Some(cmd) => s.push_str(&trf(
            "Still to run (you can do it by hand in a terminal):\n%1",
            &[&cmd],
        )),
        None => s.push_str(&tr(
            "No forks are left on the system — the rollback seems to have gone \
             through after all. Log out and log back in.",
        )),
    }
    s
}

/// Решение `remove_plasma_forks` без исполнения. Все варианты, кроме `Run`,
/// несут ГОТОВЫЙ ответ человеку (`ok` + `message`): исполнителю остаётся
/// отдать его как есть, своих текстов и своего разбора «хорошо это или
/// плохо» у него нет.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind")]
pub enum PlasmaForkReset {
    /// Форков нет вовсе — «уже так, как надо» (`ok = true`).
    NoForks { ok: bool, message: String },
    /// Форки есть, но split не дал ни одного шага — тоже `ok = true`.
    NoActions { ok: bool, message: String },
    /// ⛔ Хотя бы под один форк сток не нашёлся — НЕ ЗАПУСКАЕМ НИЧЕГО.
    /// `-Rns` необратим, а вернуть сток взамен нечем: именно так владелец и
    /// остался с наполовину снятым оформлением. Отказ до pacman'а, целиком.
    Blocked {
        ok: bool,
        unresolved: Vec<UnresolvedFork>,
        message: String,
    },
    /// Один `pkexec sh -c` (один запрос пароля). `shell` — тело БЕЗ префикса
    /// `set -e; ` (как в PlymouthReset/SddmReset), `removable`/`stock` — для
    /// лога.
    Run {
        cmd: Cmd,
        shell: String,
        removable: Vec<String>,
        stock: Vec<String>,
    },
}

/// Порт решающей части `remove_plasma_forks`. Восстановительную команду для
/// сообщения об отказе pkexec оригинал считает ЛЕНИВО (повторный проход по
/// pacman), поэтому здесь её нет — зови `plasma_fork_recovery_command`
/// в ветке ошибки, как в оригинале.
///
/// Порядок шагов (`-Rns`, затем `-S`) менять НЕЛЬЗЯ: автономные форки
/// (`aeroshell-workspace-git`, `aeroshell-kwin-components-git`) зависят от
/// провайдера `aeroshell-libplasma`, и `pacman -S libplasma` первым шагом
/// оборвался бы на «ломает зависимость». Поэтому единственная защита от
/// «удалили, а вернуть нечем» — проверка целей ДО транзакции, ветка `Blocked`.
pub fn plasma_fork_reset_plan(
    forks: &[String],
    qi_output: &dyn Fn(&str) -> Option<String>,
    target_exists: &dyn Fn(&str) -> Option<bool>,
) -> PlasmaForkReset {
    if forks.is_empty() {
        return PlasmaForkReset::NoForks {
            ok: true,
            message: plasma_fork_none_message(),
        };
    }
    let split = plasma_fork_split(forks, qi_output, target_exists);
    if !split.unresolved.is_empty() {
        let message = plasma_fork_unresolved_message(&split.unresolved);
        return PlasmaForkReset::Blocked {
            ok: false,
            unresolved: split.unresolved,
            message,
        };
    }
    let mut steps: Vec<String> = Vec::new();
    if !split.removable.is_empty() {
        steps.push(format!(
            "pacman -Rns --noconfirm {}",
            split.removable.iter().map(|f| shlex_quote(f)).collect::<Vec<_>>().join(" ")
        ));
    }
    if !split.stock.is_empty() {
        steps.push(format!(
            "pacman -S --noconfirm --needed {}",
            split.stock.iter().map(|s| shlex_quote(s)).collect::<Vec<_>>().join(" ")
        ));
    }
    if steps.is_empty() {
        return PlasmaForkReset::NoActions {
            ok: true,
            message: plasma_fork_no_actions_message(),
        };
    }
    let shell = steps.join("; ");
    PlasmaForkReset::Run {
        // set -e: если -Rns упал, сток поверх битого состояния не ставим.
        cmd: Cmd::new(
            vec![
                "pkexec".into(),
                "sh".into(),
                "-c".into(),
                format!("set -e; {shell}"),
            ],
            Some(600),
            false,
        ),
        shell,
        removable: split.removable,
        stock: split.stock,
    }
}

/// Память ответов пробы «есть ли цель в репозиториях» — порт
/// `_target_exists_probe`.
///
/// Один и тот же стоковый пакет всплывает у нескольких форков (`libplasma`
/// объявляют и `aeroshell-libplasma-git`, и запасная карта), а полный откат
/// проходит по набору форков ДВАЖДЫ — планом и человекочитаемой командой.
/// Без памяти `pacman -Si` дёргался бы на каждое повторение. Решение
/// «спрашивать или взять из памяти» — здесь; сам поход наружу подаётся
/// колбэком.
#[derive(Debug, Default, Clone)]
pub struct TargetExistsMemo {
    seen: HashMap<String, Option<bool>>,
}

impl TargetExistsMemo {
    pub fn new() -> Self {
        Self::default()
    }

    /// Ответ по имени: спрашивает пробу ТОЛЬКО о том, чего ещё не спрашивал.
    /// Запоминается и `None` («спросить не удалось») — переспрашивать нечего,
    /// ответ не изменится, а лишний запуск pacman стоит секунд.
    pub fn answer(
        &mut self,
        package: &str,
        probe: &dyn Fn(&str) -> Option<bool>,
    ) -> Option<bool> {
        if let Some(known) = self.seen.get(package) {
            return *known;
        }
        let answer = probe(package);
        self.seen.insert(package.to_string(), answer);
        answer
    }

    /// Сколько РАЗНЫХ имён уже спрошено (для проверок числа запусков).
    pub fn asked(&self) -> usize {
        self.seen.len()
    }
}

/// Форков в системе нет — предпросмотру откатывать нечего.
pub fn plasma_fork_nothing_message() -> String {
    tr("No Plasma forks found — nothing to roll back.")
}
/// Форки есть, но команда отката не собралась.
pub fn plasma_fork_no_command_message() -> String {
    tr("Couldn't build the rollback command.")
}
/// Идемпотентный ответ `remove_plasma_forks`, когда форков нет вовсе.
pub fn plasma_fork_none_message() -> String {
    tr("No Plasma forks found")
}
/// Форки есть, но ни одного шага отката из них не вышло.
pub fn plasma_fork_no_actions_message() -> String {
    tr("Nothing to do")
}
/// Чем подменяется пустой вывод удавшейся транзакции.
pub fn plasma_fork_reset_done_message() -> String {
    tr("Forks removed, stock packages restored.")
}

/// Ответ операции над форками человеку: получилось ли и что сказать.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlasmaForkAnswer {
    pub ok: bool,
    pub message: String,
}

/// Порт `plasma_fork_reset_preview`: что будет сделано — БЕЗ транзакции.
///
/// `(true, команда)` — откат возможен, и это ровно то, что выполнится;
/// `(false, объяснение)` — откатывать нечем либо нечего, и человеку сказано
/// почему по-русски, а не выводом pacman.
///
/// Пробу существования цели оборачиваем [`TargetExistsMemo`]: план и
/// человекочитаемая команда проходят по форкам дважды, и без памяти `pacman
/// -Si` спрашивался бы по два раза на каждое имя.
pub fn plasma_fork_preview(
    forks: &[String],
    qi_output: &dyn Fn(&str) -> Option<String>,
    target_exists: &dyn Fn(&str) -> Option<bool>,
) -> PlasmaForkAnswer {
    let nothing = || PlasmaForkAnswer {
        ok: false,
        message: plasma_fork_nothing_message(),
    };
    if forks.is_empty() {
        return nothing();
    }
    let memo = std::cell::RefCell::new(TargetExistsMemo::new());
    let cached = |pkg: &str| memo.borrow_mut().answer(pkg, target_exists);
    match plasma_fork_reset_plan(forks, qi_output, &cached) {
        PlasmaForkReset::Blocked { message, .. } => PlasmaForkAnswer { ok: false, message },
        PlasmaForkReset::NoForks { .. } | PlasmaForkReset::NoActions { .. } => nothing(),
        PlasmaForkReset::Run { .. } => {
            match plasma_fork_recovery_command(forks, qi_output, &cached) {
                Some(cmd) => PlasmaForkAnswer { ok: true, message: cmd },
                None => PlasmaForkAnswer {
                    ok: false,
                    message: plasma_fork_no_command_message(),
                },
            }
        }
    }
}

/// Порт разбирающей части `remove_plasma_forks`: чем закончился pkexec.
///
/// Внешнего здесь нет: Python отдаёт код возврата и оба потока, решение и
/// текст — тут. `returncode` = `None`, когда pkexec не нашёлся вовсе.
///
/// Обе восстановительные команды считаются ЛЕНИВО и РАЗНЫЕ, как в оригинале:
///   * `planned_command` — то, что собирались выполнить; нужна только при
///     отказе авторизации, когда система не тронута;
///   * `remaining_command` — пересчёт по ЖИВОМУ состоянию после падения:
///     часть шагов могла пройти, и человеку нужен остаток, а не исходная
///     команда.
///
/// ⭐ Починка при переносе: оригинал подставлял в подсказку `str(None)` и
/// показывал человеку «Выполни вручную в терминале: None», если команда
/// отката не собралась. Здесь на её месте тело самой транзакции —
/// `sudo <shell>`, ровно то же, что делает общий [`pkexec_failure_message`].
pub fn plasma_fork_reset_outcome(
    shell: &str,
    returncode: Option<i32>,
    stdout: &str,
    stderr: &str,
    planned_command: &dyn Fn() -> Option<String>,
    remaining_command: &dyn Fn() -> Option<String>,
) -> PlasmaForkAnswer {
    let Some(rc) = returncode else {
        return PlasmaForkAnswer {
            ok: false,
            message: pkexec_missing_message(),
        };
    };
    if rc == 0 {
        // Вывод pacman НЕ обрезается: оригинал отдаёт его как есть.
        return PlasmaForkAnswer {
            ok: true,
            message: if stdout.is_empty() {
                plasma_fork_reset_done_message()
            } else {
                stdout.to_string()
            },
        };
    }
    let raw = if !stderr.is_empty() { stderr } else { stdout };
    let err = raw.trim().to_string();
    let message = if pkexec_auth_denied(rc, &err) {
        let manual = planned_command().unwrap_or_else(|| format!("sudo {shell}"));
        pkexec_denied_message(&manual)
    } else {
        plasma_fork_failure_message(&err, remaining_command().as_deref())
    };
    PlasmaForkAnswer { ok: false, message }
}

// ============================================================================
// Каталоги: общий отбор записей
// ============================================================================

/// Что берём из перечисления каталога и в каком порядке. Вход — сырой список
/// `(имя, годится ли)`: перечисляет каталог и проверяет признак Python
/// (`os.listdir`, `os.path.isdir`), а решение — здесь.
///
/// Порядок задаётся ЗДЕСЬ и только здесь: обход каталога в файловой системе
/// произволен, а обеим сторонам нужен лексикографический — SDDM так мёрджит
/// свои `*.conf` (поздние перекрывают ранние), а список тем Plymouth так
/// показывается человеку. Признак «годится» у форматов разный (у SDDM —
/// суффикс имени, у Plymouth — что это подкаталог), поэтому он подаётся
/// готовым, а общими остаются отбор и порядок.
pub fn dir_entries_selected(entries: &[(String, bool)]) -> Vec<String> {
    let mut out: Vec<String> = entries
        .iter()
        .filter(|(_, keep)| *keep)
        .map(|(name, _)| name.clone())
        .collect();
    out.sort();
    out
}

// ============================================================================
// Plymouth (загрузочная заставка) — детект, выбор дистро-дефолта, сброс
// ============================================================================
//
// Plymouth — СИСТЕМНЫЙ уровень (root, /etc + initramfs), сброс Plasma-темы его
// не трогает. Сброс пересобирает initramfs → только из явного recovery-шага.

pub const PLYMOUTH_THEMES_DIR: &str = "/usr/share/plymouth/themes";
pub const PLYMOUTH_DEFAULTS: &str = "/usr/share/plymouth/plymouthd.defaults";
/// Тема брендинга дистро → пакет-поставщик. Если пакет установлен — это «как
/// на стоке дистро» и приоритетнее общего дефолта пакета plymouth (bgrt,
/// который показывает BGRT-логотип ПРОШИВКИ).
pub const PLYMOUTH_DISTRO_BRANDING: &[(&str, &str)] =
    &[("cachyos-bootanimation", "cachyos-plymouth-bootanimation")];
/// Запасные кандидаты, если ни брендинг, ни дефолт пакета не подошли.
pub const PLYMOUTH_FALLBACK_CANDIDATES: &[&str] = &["bgrt", "spinner"];

/// Порт `_plymouth_theme_from_default_link`: имя темы из цели симлинка
/// `default.plymouth` (путь для дистро без бинаря plymouth-set-default-theme,
/// напр. Ubuntu). `realpath_existing` — realpath ссылки, если такой путь
/// существует (None = ссылки нет / цель битая / OSError).
pub fn plymouth_theme_from_link(realpath_existing: Option<&str>) -> Option<String> {
    let target = realpath_existing?;
    // basename(dirname(target)) — каталог темы.
    let dir = target.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    let name = dir.rsplit('/').next().unwrap_or("");
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

/// Путь к описанию темы — `<каталог тем>/<тема>/<тема>.plymouth`. Именно этот
/// файл делает подкаталог темой: без него `plymouth-set-default-theme` тему не
/// видит и `update-alternatives` ссылаться не на что. Одна склейка на весь
/// модуль: раньше та же строка стояла тремя копиями.
pub fn plymouth_theme_file(theme: &str) -> String {
    plymouth_theme_file_in(PLYMOUTH_THEMES_DIR, theme)
}

/// То же в ЗАДАННОМ каталоге тем. Каталог подаётся аргументом, чтобы
/// предпросмотр и тесты не уходили в живой `/usr/share/plymouth/themes`:
/// у `list_plymouth_themes` каталог свой (`_PLYMOUTH_THEMES_DIR`), и проба
/// файла обязана идти в ТОТ ЖЕ каталог, который перечислили. На боевом пути
/// (`themes_dir` = [`PLYMOUTH_THEMES_DIR`]) ответ прежний.
pub fn plymouth_theme_file_in(themes_dir: &str, theme: &str) -> String {
    let dir = themes_dir.trim_end_matches('/');
    if dir.is_empty() {
        return format!("{theme}/{theme}.plymouth");
    }
    format!("{dir}/{theme}/{theme}.plymouth")
}

/// Первая половина `list_plymouth_themes`: какие записи каталога тем вообще
/// стоит проверять. Вход — сырое перечисление парами `(имя, это каталог?)`.
pub fn plymouth_theme_candidates(entries: &[(String, bool)]) -> Vec<String> {
    dir_entries_selected(entries)
}

/// Вторая половина `list_plymouth_themes`: установленные темы из проб
/// `(имя, есть ли <тема>.plymouth)`.
///
/// ⚠️ Починка при порте. Оригинал отдавал ВСЕ подкаталоги
/// `/usr/share/plymouth/themes` и уверял в комментарии, что это «тот же
/// результат», что и `plymouth-set-default-theme --list`. Это неправда: сам
/// plymouth перечисляет только каталоги с `<тема>.plymouth` внутри. Пустой
/// каталог остаётся после удаления пакета темы (в нём лежит чужой файл или
/// просто ничего), и такая «тема» проходила в
/// [`target_plymouth_theme`] как установленная: если её звали `bgrt` или
/// `spinner`, Rexon объявлял её родным дефолтом дистрибутива и сбрасывал
/// заставку на несуществующее — `plymouth-set-default-theme bgrt -R`
/// перебирал initramfs впустую. Тот же модуль на пути alternatives файл темы
/// проверял ([`PlymouthReset::MissingThemeFile`]) — проверка была, но только
/// в одной ветке из двух. Теперь список честен для обеих.
pub fn plymouth_installed_themes(probes: &[(String, bool)]) -> Vec<String> {
    dir_entries_selected(probes)
}

/// Порт разбора вывода `plymouth-set-default-theme` без аргументов: он
/// печатает имя текущей темы. `None` в ответе — имени нет (бинарь не
/// запустился, упал или промолчал), вызывающий идёт к `default.plymouth`.
/// `returncode` = `None`, когда `safe_run` вернул None.
pub fn plymouth_theme_from_output(returncode: Option<i32>, stdout: &str) -> Option<String> {
    if returncode != Some(0) {
        return None;
    }
    let name = stdout.trim();
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

/// Порт `_plymouth_package_default`: строка `Theme=` из plymouthd.defaults
/// (None — файла нет либо значение пустое).
pub fn plymouth_package_default(defaults_text: Option<&str>) -> Option<String> {
    let text = defaults_text?;
    for line in text.lines() {
        let line = line.trim();
        let bytes = line.as_bytes();
        if bytes.len() >= 6 && bytes[..6].eq_ignore_ascii_case(b"theme=") {
            // Первые 6 байт — ASCII, срез по границе символа безопасен.
            let v = line[6..].trim();
            // Оригинал возвращает с ПЕРВОЙ подходящей строки, даже пустую.
            return if v.is_empty() { None } else { Some(v.to_string()) };
        }
    }
    None
}

/// Порт `target_plymouth_theme`: родной дефолт дистрибутива, без хардкода
/// «только cachyos». `pkg_installed` и `package_default` вызываются ЛЕНИВО
/// в порядке оригинала (брендинг → plymouthd.defaults → запасные).
pub fn target_plymouth_theme(
    installed_themes: &[String],
    pkg_installed: &dyn Fn(&str) -> bool,
    package_default: &dyn Fn() -> Option<String>,
) -> Option<String> {
    if installed_themes.is_empty() {
        return None;
    }
    let installed = |t: &str| installed_themes.iter().any(|x| x == t);
    for (theme, pkg) in PLYMOUTH_DISTRO_BRANDING {
        if installed(theme) && pkg_installed(pkg) {
            return Some((*theme).to_string());
        }
    }
    if let Some(d) = package_default() {
        if installed(&d) {
            return Some(d);
        }
    }
    for cand in PLYMOUTH_FALLBACK_CANDIDATES {
        if installed(cand) {
            return Some((*cand).to_string());
        }
    }
    None
}

/// Порт `plymouth_reset_command`: человекочитаемая команда сброса (для показа).
/// Способ — по факту наличия инструмента, а не по семейству (на Ubuntu бинаря
/// нет вовсе, тему переключают только alternatives).
pub fn plymouth_reset_command(target: &str, has_set_default_theme: bool) -> Option<String> {
    if target.is_empty() {
        return None;
    }
    if has_set_default_theme {
        return Some(format!(
            "sudo plymouth-set-default-theme {} -R",
            shlex_quote(target)
        ));
    }
    let pl = plymouth_theme_file(target);
    Some(format!(
        "sudo update-alternatives --set default.plymouth {} && sudo update-initramfs -u",
        shlex_quote(&pl)
    ))
}

/// Решение `reset_plymouth_to_default` без исполнения.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind")]
pub enum PlymouthReset {
    /// `(False, "Не удалось определить родной дефолт Plymouth")`.
    NoTarget,
    /// Идемпотентность: текущая тема уже целевая.
    AlreadyDefault { target: String },
    /// Путь alternatives, но в теме нет `<тема>.plymouth`.
    MissingThemeFile { target: String, path: String },
    /// Нечем переключить: ни plymouth-set-default-theme, ни
    /// update-alternatives.
    NoTool,
    /// Один `pkexec sh -c "set -e; …"`; `shell` — тело для текста ошибки.
    Run { cmd: Cmd, shell: String, target: String },
}

/// Порт решающей части `reset_plymouth_to_default`.
///
/// `current` оригинал читает ТОЛЬКО после того, как цель определена, — шим
/// обязан сохранить эту ленивость (`None`, пока цели нет).
/// `theme_file_exists(path)` — `os.path.isfile` для пути alternatives.
pub fn plymouth_reset_plan(
    target: Option<&str>,
    current: Option<&str>,
    has_set_default_theme: bool,
    has_update_alternatives: bool,
    theme_file_exists: &dyn Fn(&str) -> bool,
) -> PlymouthReset {
    let target = match target {
        Some(t) if !t.is_empty() => t,
        _ => return PlymouthReset::NoTarget,
    };
    if current == Some(target) {
        return PlymouthReset::AlreadyDefault { target: target.to_string() };
    }
    let shell = if has_set_default_theme {
        format!("plymouth-set-default-theme {} -R", shlex_quote(target))
    } else if has_update_alternatives {
        let pl = plymouth_theme_file(target);
        if !theme_file_exists(&pl) {
            return PlymouthReset::MissingThemeFile {
                target: target.to_string(),
                path: pl,
            };
        }
        format!(
            "update-alternatives --set default.plymouth {} && update-initramfs -u",
            shlex_quote(&pl)
        )
    } else {
        return PlymouthReset::NoTool;
    };
    PlymouthReset::Run {
        cmd: Cmd::new(
            vec![
                "pkexec".into(),
                "sh".into(),
                "-c".into(),
                format!("set -e; {shell}"),
            ],
            Some(600),
            false,
        ),
        shell,
        target: target.to_string(),
    }
}

/// Не удалось определить родной дефолт дистрибутива — сбрасывать некуда.
pub fn plymouth_no_target_message() -> String {
    tr("Couldn't determine the distribution's default Plymouth theme")
}
/// Ни `plymouth-set-default-theme`, ни `update-alternatives` в системе нет.
pub fn plymouth_no_tool_message() -> String {
    tr("Nothing to switch the boot splash with: neither \
        plymouth-set-default-theme nor update-alternatives was found")
}
/// Текст, которым подменяется пустой вывод упавшего pkexec.
pub fn plymouth_reset_failed_message() -> String {
    tr("Couldn't reset Plymouth")
}

/// Идемпотентный ответ: тема уже та, к которой сбрасывают.
pub fn plymouth_already_default_message(target: &str) -> String {
    trf("Plymouth is already on the default theme “%1”", &[&target])
}

/// Путь alternatives, а описания темы в каталоге нет.
pub fn plymouth_missing_theme_file_message(target: &str) -> String {
    // TRANSLATORS: %1 is a theme name; “%1.plymouth” is its file name.
    trf("The “%1” theme has no %1.plymouth file", &[&target])
}

/// Успех: initramfs пересобран, увидится после перезагрузки.
pub fn plymouth_reset_success_message(target: &str) -> String {
    trf(
        "Plymouth reset to “%1” (initramfs rebuilt; takes effect after a restart).",
        &[&target],
    )
}

/// Итог `reset_plymouth_to_default` — `(ok, сообщение)`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlymouthResetOutcome {
    pub ok: bool,
    pub message: String,
}

/// Ответ, который вызывающий получает БЕЗ запуска pkexec. `None` только у
/// [`PlymouthReset::Run`]: там ответ даёт [`plymouth_reset_outcome`] по коду
/// возврата. Раньше эти четыре текста жили строками в Python.
pub fn plymouth_reset_precheck(plan: &PlymouthReset) -> Option<PlymouthResetOutcome> {
    let (ok, message) = match plan {
        PlymouthReset::NoTarget => (false, plymouth_no_target_message()),
        PlymouthReset::AlreadyDefault { target } => {
            (true, plymouth_already_default_message(target))
        }
        PlymouthReset::MissingThemeFile { target, .. } => {
            (false, plymouth_missing_theme_file_message(target))
        }
        PlymouthReset::NoTool => (false, plymouth_no_tool_message()),
        PlymouthReset::Run { .. } => return None,
    };
    Some(PlymouthResetOutcome { ok, message })
}

/// План сброса вместе с готовым ответом для исходов, где идти наружу не за
/// чем. Python на этом не решает ничего: либо отдаёт `(ok, message)`, либо
/// запускает `cmd` и зовёт [`plymouth_reset_outcome`].
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlymouthResetDecision {
    #[serde(flatten)]
    pub plan: PlymouthReset,
    /// `None` = решение требует запуска (`kind` = `Run`).
    pub ok: Option<bool>,
    pub message: Option<String>,
}

/// [`plymouth_reset_plan`] вместе с текстом ответа — то, что уезжает в мост.
pub fn plymouth_reset_decision(
    target: Option<&str>,
    current: Option<&str>,
    has_set_default_theme: bool,
    has_update_alternatives: bool,
    theme_file_exists: &dyn Fn(&str) -> bool,
) -> PlymouthResetDecision {
    let plan = plymouth_reset_plan(
        target,
        current,
        has_set_default_theme,
        has_update_alternatives,
        theme_file_exists,
    );
    let pre = plymouth_reset_precheck(&plan);
    PlymouthResetDecision {
        plan,
        ok: pre.as_ref().map(|o| o.ok),
        message: pre.map(|o| o.message),
    }
}

/// Разбор результата pkexec для `reset_plymouth_to_default`. Внешнего здесь
/// нет: Python отдаёт код возврата и оба потока, решение и текст — тут.
///
/// `returncode` = `None`, когда pkexec не нашёлся (`safe_run` вернул `None`).
/// `shell` — тело команды из [`PlymouthReset::Run`], оно же идёт в подсказку
/// «выполни вручную».
pub fn plymouth_reset_outcome(
    target: &str,
    shell: &str,
    returncode: Option<i32>,
    stdout: &str,
    stderr: &str,
) -> PlymouthResetOutcome {
    let Some(code) = returncode else {
        return PlymouthResetOutcome {
            ok: false,
            message: pkexec_missing_message(),
        };
    };
    if code == 0 {
        return PlymouthResetOutcome {
            ok: true,
            message: plymouth_reset_success_message(target),
        };
    }
    PlymouthResetOutcome {
        ok: false,
        message: pkexec_failure_message(
            code,
            stdout,
            stderr,
            &plymouth_reset_failed_message(),
            shell,
        ),
    }
}

// ============================================================================
// SDDM (экран входа) — detect-driven сброс к СТОКУ ДИСТРИБУТИВА
// ============================================================================
//
// Rexon никогда не пишет в чужие файлы SDDM: свою тему он кладёт в собственный
// оверрайд /etc/sddm.conf.d/zz-rexon-theme.conf. SDDM мёрджит
// /etc/sddm.conf.d/*.conf поверх дистро-дефолтов /usr/lib/sddm/sddm.conf.d/,
// поэтому «вернуть как из коробки» = просто УДАЛИТЬ наш оверрайд и файлы-следы
// форков — без хардкода темы и без перезаписи kde_settings.conf.

pub const SDDM_CONF_D: &str = "/etc/sddm.conf.d";
pub const SDDM_DISTRO_CONF_D: &str = "/usr/lib/sddm/sddm.conf.d";
/// Оверрайды, которые создаёт сам Rexon (всегда безопасно удалить).
pub const SDDM_REXON_FILES: &[&str] = &["zz-rexon-theme.conf"];
/// Токены форк-тем в имени файла ИЛИ содержимом оверрайда. Оригинальный
/// `_SDDM_FORK_RE` — альтернация литералов с `re.I`, что эквивалентно поиску
/// подстроки без учёта регистра (свой regex-движок в ядро не тянем).
pub const SDDM_FORK_TOKENS: &[&str] = &[
    "wackyideas", "sevenstart", "aeroshell", "aerothemeplasma", "se7enaero", "win7os",
];

fn has_sddm_fork_token(s: &str) -> bool {
    let low = s.to_lowercase();
    SDDM_FORK_TOKENS.iter().any(|t| low.contains(t))
}

/// Порт `_sddm_files_to_remove`. Вход: (basename, содержимое). Удаляем свои
/// оверрайды по имени + файлы-следы форков (токен в ИМЕНИ или СОДЕРЖИМОМ);
/// kde_settings.conf (файл KDE/дистро) и прочие пользовательские конфиги не
/// трогаем.
pub fn sddm_files_to_remove(entries: &[(String, String)]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (name, content) in entries {
        let base = name.rsplit('/').next().unwrap_or(name).to_string();
        if SDDM_REXON_FILES.contains(&base.as_str()) {
            out.push(base);
            continue;
        }
        if has_sddm_fork_token(&base) || has_sddm_fork_token(content) {
            out.push(base);
        }
    }
    out.sort();
    out.dedup();
    out
}

/// `[Theme] Current` из текста конфига. Порт `configparser(strict=False)`:
/// имена секций регистрозависимы, ключи — нет; при дубликатах ключа
/// побеждает последний.
fn ini_theme_current(text: &str) -> Option<String> {
    let mut in_section = false;
    let mut found: Option<String> = None;
    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') || t.starts_with(';') {
            continue;
        }
        if t.starts_with('[') && t.ends_with(']') && t.len() >= 2 {
            in_section = &t[1..t.len() - 1] == "Theme";
            continue;
        }
        if !in_section {
            continue;
        }
        let Some(pos) = t.find(['=', ':']) else {
            continue;
        };
        if t[..pos].trim().eq_ignore_ascii_case("current") {
            found = Some(t[pos + 1..].trim().to_string());
        }
    }
    found
}

/// Тема SDDM по набору конфигов — общее ядро `_sddm_read_current_theme`.
/// Вход должен быть отсортирован по имени: SDDM мёрджит лексикографически,
/// поздние файлы перекрывают. Пустое значение НЕ сбрасывает найденное ранее.
pub fn sddm_current_theme_from(entries: &[(String, String)]) -> Option<String> {
    let mut result: Option<String> = None;
    for (name, content) in entries {
        if !name.ends_with(".conf") {
            continue;
        }
        if let Some(v) = ini_theme_current(content) {
            if !v.is_empty() {
                result = Some(v);
            }
        }
    }
    result
}

/// Порт `sddm_distro_default_theme`: дистро-дефолты пакета sddm, поверх них —
/// `/etc/sddm.conf.d/kde_settings.conf` (его ставит KDE/дистро, это «как из
/// коробки», НЕ Rexon). None — определить не удалось.
pub fn sddm_distro_default_theme(
    distro_entries: &[(String, String)],
    kde_settings: Option<&str>,
) -> Option<String> {
    let mut result = sddm_current_theme_from(distro_entries);
    if let Some(text) = kde_settings {
        if let Some(v) = ini_theme_current(text) {
            if !v.is_empty() {
                result = Some(v);
            }
        }
    }
    result
}

/// Порт `sddm_reset_plan`: что удалим и какая тема станет дефолтом.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SddmResetPlan {
    pub to_remove: Vec<String>,
    pub default_theme: Option<String>,
}

/// `conf_entries` — (имя, содержимое) файлов /etc/sddm.conf.d в порядке
/// сортировки; фильтр `.conf` применяется здесь (как в оригинале).
pub fn sddm_reset_plan(
    conf_entries: &[(String, String)],
    distro_entries: &[(String, String)],
    kde_settings: Option<&str>,
) -> SddmResetPlan {
    let conf: Vec<(String, String)> = conf_entries
        .iter()
        .filter(|(n, _)| n.ends_with(".conf"))
        .cloned()
        .collect();
    SddmResetPlan {
        to_remove: sddm_files_to_remove(&conf),
        default_theme: sddm_distro_default_theme(distro_entries, kde_settings),
    }
}

/// Решение `reset_sddm_to_default` без исполнения.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind")]
pub enum SddmReset {
    /// Идемпотентный no-op — `(True, "…уже на стоковом конфиге", [])`.
    AlreadyStock,
    /// Один `pkexec sh -c "set -e; rm -f …"`; `shell` — тело для текста ошибки.
    Run {
        cmd: Cmd,
        shell: String,
        removed: Vec<String>,
        default_theme: Option<String>,
    },
}

/// Порт решающей части `reset_sddm_to_default`. `conf_d` — каталог, к которому
/// приклеиваются basename'ы из плана (по умолчанию [`SDDM_CONF_D`]).
pub fn sddm_reset_command(plan: &SddmResetPlan, conf_d: &str) -> SddmReset {
    if plan.to_remove.is_empty() {
        return SddmReset::AlreadyStock;
    }
    let quoted = plan
        .to_remove
        .iter()
        .map(|b| shlex_quote(&format!("{conf_d}/{b}")))
        .collect::<Vec<_>>()
        .join(" ");
    let shell = format!("rm -f {quoted}");
    SddmReset::Run {
        cmd: Cmd::new(
            vec![
                "pkexec".into(),
                "sh".into(),
                "-c".into(),
                format!("set -e; {shell}"),
            ],
            Some(120),
            false,
        ),
        shell,
        removed: plan.to_remove.clone(),
        default_theme: plan.default_theme.clone(),
    }
}

/// Текст успеха `reset_sddm_to_default` (сверяется текст-в-текст с Python).
pub fn sddm_reset_success_message(removed: &[String], default_theme: Option<&str>) -> String {
    let tail = match default_theme {
        Some(t) if !t.is_empty() => trf(
            "The login screen will go back to the distribution default “%1”.",
            &[&t],
        ),
        _ => tr("The login screen will go back to the distribution default."),
    };
    format!(
        "{} {tail}",
        trf("Removed SDDM overrides: %1.", &[&removed.join(", ")])
    )
}

/// Имя файла KDE/дистро внутри `/etc/sddm.conf.d`. Его кладёт установка KDE —
/// это «как из коробки», НЕ Rexon: он читается отдельно от общего скана и
/// никогда не попадает в список удаления.
pub const SDDM_KDE_SETTINGS_FILE: &str = "kde_settings.conf";

/// Путь к `kde_settings.conf` внутри заданного каталога — порт склейки
/// `os.path.join` из `_sddm_kde_settings_text`.
///
/// ⚠️ Починка при порте: оригинал склеивал путь с ЖЁСТКО зашитым
/// `_SDDM_CONF_D` и не смотрел на аргумент `conf_d`, который принимают
/// `sddm_reset_plan` и `_sddm_read_current_theme`. Из-за этого предпросмотр
/// или тест на подставленном каталоге всё равно читал настоящий
/// `/etc/sddm.conf.d/kde_settings.conf` живой машины и мог объявить
/// дистро-дефолтом тему, которой в этом каталоге нет. Теперь каталог задаётся
/// аргументом; на боевом пути (`conf_d` = [`SDDM_CONF_D`]) ответ прежний.
pub fn sddm_kde_settings_path(conf_d: &str) -> String {
    let dir = conf_d.trim_end_matches('/');
    if dir.is_empty() {
        return SDDM_KDE_SETTINGS_FILE.to_string();
    }
    format!("{dir}/{SDDM_KDE_SETTINGS_FILE}")
}

/// Какие файлы каталога SDDM читать и в каком порядке — чистая часть
/// `_sddm_conf_entries`. Вход — сырое перечисление каталога.
///
/// Порядок обязателен: SDDM мёрджит `*.conf` лексикографически, поздние
/// перекрывают ранние. Всё, что не `*.conf`, каталогом настроек не считается.
/// Оригинал считал то же самое верно (`sorted(os.listdir(...))`) — правило
/// переехало сюда не как починка, а чтобы жить в одном месте с разбором,
/// который от этого порядка зависит.
pub fn sddm_conf_read_order(names: &[String]) -> Vec<String> {
    let entries: Vec<(String, bool)> = names
        .iter()
        .map(|n| (n.clone(), n.ends_with(".conf")))
        .collect();
    dir_entries_selected(&entries)
}

/// Сборка пар (имя, содержимое) из результатов чтения — вторая чистая часть
/// `_sddm_conf_entries`. `None` = файл не прочитался: он всё равно остаётся в
/// списке с ПУСТЫМ содержимым, потому что может опознаться по одному ИМЕНИ
/// (токен форка), как в оригинале.
pub fn sddm_conf_entries(reads: &[(String, Option<String>)]) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = reads
        .iter()
        .filter(|(n, _)| n.ends_with(".conf"))
        .map(|(n, c)| (n.clone(), c.clone().unwrap_or_default()))
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

// ============================================================================
// Сеанс по умолчанию у экрана входа: X11-сеанс → его пара на Wayland
// ============================================================================
//
// Защита от дурака: экран входа сам открывает (автовход) или предлагает
// первым сеанс X11, который уходит из системы (Plasma (X11) при Plasma 6.8),
// — человек застревает на входе. Rexon переводит ТОЛЬКО ключи сеанса на
// пару из таблицы; автовход не включает и не выключает, прочие ключи и
// строки файлов не трогает. Новая пара — новая запись таблицы.
//
// Где экраны входа держат сеанс (по их исходникам и man-страницам):
// * SDDM — `[Autologin] Session=` (sddm.conf(5): файлы
//   `/usr/lib/sddm/sddm.conf.d/*.conf`, затем `/etc/sddm.conf.d/*.conf` по
//   имени, последним `/etc/sddm.conf`; поздний перекрывает) и последний
//   выбранный `[Last] Session=` в `state.conf` домашнего каталога
//   пользователя sddm — полный путь файла сеанса; его приветствие
//   предлагает первым, а пустой `[Autologin] Session=` при автовходе
//   берёт его же.
// * Plasma Login Manager — `[Autologin] Session=` (демон) и
//   `[Greeter] PreselectedSession=` (приветствие; перекрывает последний
//   выбранный). Главный файл `/etc/plasmalogin.conf` (его целиком пишет
//   модуль «Параметров системы») перекрывает каталоги
//   `/usr/lib/plasmalogin/plasmalogin.conf.d`, затем `/etc/plasmalogin.conf.d`.
//   Последний выбранный сеанс приветствие хранит у себя (файл состояния
//   пользователя plasmalogin, по человеку) — его Rexon не пишет: туда даже
//   модуль настроек ходит, сбросив права до этого пользователя, а сеанс
//   там сменится сам при первом входе в Wayland.

/// X11-сеанс, у которого есть пара на Wayland.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionSuccessor {
    /// Имя файла X11-сеанса без `.desktop` (`/usr/share/xsessions/<имя>`).
    pub x11: &'static str,
    /// Имя файла пары в [`WAYLAND_SESSIONS_DIR`] без `.desktop`.
    pub wayland: &'static str,
    /// Имена для человека — как в списке экрана входа (не переводятся).
    pub x11_name: &'static str,
    pub wayland_name: &'static str,
}

/// X11-сеансы, которые уходят из систем, и их пары на Wayland.
pub const X11_SESSION_SUCCESSORS: &[SessionSuccessor] = &[SessionSuccessor {
    x11: "plasmax11",
    wayland: "plasma",
    x11_name: "Plasma (X11)",
    wayland_name: "Plasma (Wayland)",
}];

/// Каталог сеансов Wayland (общий у SDDM и Plasma Login Manager).
pub const WAYLAND_SESSIONS_DIR: &str = "/usr/share/wayland-sessions";

/// Последний выбранный сеанс SDDM (`[Last] Session=`).
pub const SDDM_STATE_FILE: &str = "/var/lib/sddm/state.conf";

/// Ключ сеанса в группе настроек экрана входа.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionKey {
    pub group: &'static str,
    pub key: &'static str,
}

/// Источник настроек экрана входа: каталоги `*.conf` по порядку чтения,
/// затем файл (поздний перекрывает) и ключи сеанса в них.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionSource {
    pub dirs: &'static [&'static str],
    pub file: &'static str,
    pub keys: &'static [SessionKey],
}

/// Все места, где экраны входа держат сеанс по умолчанию.
pub const LOGIN_SESSION_SOURCES: &[SessionSource] = &[
    SessionSource {
        dirs: &[SDDM_DISTRO_CONF_D, SDDM_CONF_D],
        file: "/etc/sddm.conf",
        keys: &[SessionKey { group: "Autologin", key: "Session" }],
    },
    SessionSource { dirs: &[], file: SDDM_STATE_FILE, keys: &[SessionKey { group: "Last", key: "Session" }] },
    SessionSource {
        dirs: &["/usr/lib/plasmalogin/plasmalogin.conf.d", "/etc/plasmalogin.conf.d"],
        file: "/etc/plasmalogin.conf",
        keys: &[
            SessionKey { group: "Autologin", key: "Session" },
            SessionKey { group: "Greeter", key: "PreselectedSession" },
        ],
    },
];

/// Пара для значения `Session=` (`plasmax11`, `plasmax11.desktop`, полный
/// путь) — по имени файла сеанса.
pub fn session_successor(value: &str) -> Option<&'static SessionSuccessor> {
    let name = crate::update_guard::session_file_name(value);
    X11_SESSION_SUCCESSORS.iter().find(|s| s.x11 == name)
}

/// Файл сеанса-пары на Wayland.
pub fn wayland_session_file(s: &SessionSuccessor) -> String {
    format!("{WAYLAND_SESSIONS_DIR}/{}.desktop", s.wayland)
}

/// Файлы источника по порядку чтения (`list` — имена в каталоге).
fn source_files(src: &SessionSource, list: &dyn Fn(&str) -> Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    for dir in src.dirs {
        out.extend(sddm_conf_read_order(&list(dir)).iter().map(|n| format!("{dir}/{n}")));
    }
    out.push(src.file.to_string());
    out
}

/// Сеансы, которые экраны входа открывают или предлагают сами: итоговое
/// значение каждого ключа каждого источника (пустые отброшены).
pub fn login_session_values(read: &dyn Fn(&str) -> Option<String>, list: &dyn Fn(&str) -> Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    for src in LOGIN_SESSION_SOURCES {
        let texts: Vec<String> = source_files(src, list).iter().filter_map(|p| read(p)).collect();
        for k in src.keys {
            out.extend(crate::login_rescue::ini_value(&texts, k.group, k.key));
        }
    }
    out.retain(|s| !s.trim().is_empty());
    out
}

/// Сеанс X11 из таблицы, который экран входа открывает или предлагает сам.
pub fn x11_login_default(values: &[String]) -> Option<&'static SessionSuccessor> {
    values.iter().find_map(|v| session_successor(v))
}

/// Новое значение в том же виде, что старое: полный путь — полным путём,
/// с `.desktop` — с ним, голое имя — именем.
fn successor_value(old: &str, s: &SessionSuccessor) -> String {
    let old = old.trim();
    if old.contains('/') {
        wayland_session_file(s)
    } else if old.ends_with(".desktop") {
        format!("{}.desktop", s.wayland)
    } else {
        s.wayland.to_string()
    }
}

/// Текст файла, где ключи `keys`, указывающие на `s.x11`, переведены на
/// пару. Меняется только значение таких строк — прочие строки, отступы,
/// комментарии и концы строк остаются байт в байт. `None` — менять нечего.
pub fn retarget_session(text: &str, keys: &[SessionKey], s: &SessionSuccessor) -> Option<String> {
    rewrite_ini_values(
        text,
        keys,
        &|v| crate::update_guard::session_file_name(v) == s.x11,
        &|v| successor_value(v, s),
    )
}

/// Общая правка настроек экрана входа: у строк `ключ=значение` из `keys`
/// (группа — по последнему заголовку `[…]`), чьё значение `hit`, значение
/// заменяется на `new(старое)`. Прочие строки, отступы, комментарии и концы
/// строк остаются байт в байт. `None` — ни одна строка не изменилась.
pub fn rewrite_ini_values(
    text: &str,
    keys: &[SessionKey],
    hit: &dyn Fn(&str) -> bool,
    new: &dyn Fn(&str) -> String,
) -> Option<String> {
    let mut group = String::new();
    let mut changed = false;
    let mut out = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        let body = line.trim_end_matches(['\n', '\r']);
        let t = body.trim();
        if let Some(name) = t.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
            group = name.trim().to_string();
            out.push_str(line);
            continue;
        }
        let matched = !t.starts_with('#')
            && !t.starts_with(';')
            && body.split_once('=').is_some_and(|(k, v)| {
                keys.iter().any(|sk| sk.group == group && sk.key == k.trim()) && hit(v)
            });
        if !matched {
            out.push_str(line);
            continue;
        }
        let eq = body.find('=').unwrap_or(body.len());
        let (head, value) = body.split_at(eq + 1);
        let pad = &value[..value.len() - value.trim_start().len()];
        let replaced = new(value);
        if replaced == value.trim() {
            out.push_str(line);
            continue;
        }
        out.push_str(head);
        out.push_str(pad);
        out.push_str(&replaced);
        out.push_str(&line[body.len()..]);
        changed = true;
    }
    changed.then_some(out)
}

/// Файл, который правка вправе переписать: настройки в `/etc` и файл
/// последнего сеанса SDDM. Файлы пакетов (`/usr/lib/…`) — нет: их вернёт
/// следующее обновление.
pub fn session_file_writable(path: &str) -> bool {
    path.starts_with("/etc/") || path == SDDM_STATE_FILE
}

/// Правка одного файла: путь и новый текст.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionEdit {
    pub path: String,
    pub text: String,
}

/// Решение «сделать пару сеансом по умолчанию».
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionSwitch {
    /// Переписать эти файлы — после них экран входа сам X11-сеанс не
    /// открывает и не предлагает.
    Edits(Vec<SessionEdit>),
    /// Менять нечего: X11-сеанс нигде не по умолчанию.
    Nothing,
    /// X11-сеанс задан там, где Rexon не правит (файл пакета), — ничего
    /// не менять.
    Elsewhere,
}

/// Что переписать, чтобы экран входа перестал сам открывать `s.x11`.
/// Сверка итога — тем же чтением, что у сторожа обновлений
/// ([`login_session_values`]) поверх новых текстов.
pub fn plan_wayland_default(
    s: &SessionSuccessor,
    read: &dyn Fn(&str) -> Option<String>,
    list: &dyn Fn(&str) -> Vec<String>,
) -> SessionSwitch {
    let mut edits = Vec::new();
    for src in LOGIN_SESSION_SOURCES {
        for path in source_files(src, list) {
            if !session_file_writable(&path) {
                continue;
            }
            if let Some(text) = read(&path).and_then(|t| retarget_session(&t, src.keys, s)) {
                edits.push(SessionEdit { path, text });
            }
        }
    }
    let after = |p: &str| edits.iter().find(|e| e.path == p).map(|e| e.text.clone()).or_else(|| read(p));
    let still = login_session_values(&after, list).iter().any(|v| crate::update_guard::session_file_name(v) == s.x11);
    if still {
        SessionSwitch::Elsewhere
    } else if edits.is_empty() {
        SessionSwitch::Nothing
    } else {
        SessionSwitch::Edits(edits)
    }
}

/// Тема экрана входа SDDM — `[Theme] Current=` в тех же настройках и том же
/// порядке чтения, что сеанс: каталог пакета, каталог `/etc`, затем
/// `/etc/sddm.conf` — он читается ПОСЛЕДНИМ и перекрывает все `*.conf`.
pub const SDDM_THEME_SOURCE: SessionSource = SessionSource {
    dirs: &[SDDM_DISTRO_CONF_D, SDDM_CONF_D],
    file: "/etc/sddm.conf",
    keys: &[SessionKey { group: "Theme", key: "Current" }],
};

/// Свой файл темы SDDM: пишется, только когда ни один файл `/etc` тему не
/// задаёт (`zz-` — после `kde_settings.conf` и прочих в порядке чтения).
pub const SDDM_REXON_THEME_CONF: &str = "/etc/sddm.conf.d/zz-rexon-theme.conf";

/// Файлы, из которых SDDM берёт тему, в его порядке чтения (последний
/// выигрывает): каталог пакета, каталог `/etc`, затем `/etc/sddm.conf`.
/// `list` — имена в каталоге. По ним же «Оформление» узнаёт текущую тему
/// экрана входа (`installed_scan::LiveScan::active_map`).
pub fn sddm_theme_files(list: &dyn Fn(&str) -> Vec<String>) -> Vec<String> {
    source_files(&SDDM_THEME_SOURCE, list)
}

/// Тема, которую SDDM покажет при таких настройках (`None` — встроенная).
pub fn sddm_effective_theme(read: &dyn Fn(&str) -> Option<String>, list: &dyn Fn(&str) -> Vec<String>) -> Option<String> {
    let texts: Vec<String> = source_files(&SDDM_THEME_SOURCE, list).iter().filter_map(|p| read(p)).collect();
    crate::login_rescue::ini_value(&texts, "Theme", "Current")
}

/// Что переписать, чтобы SDDM показал тему `theme` (кнопка «Применить» у
/// установленной темы экрана входа).
///
/// ⭐ Дефект 30.10.2026: прежде помощник писал только свой
/// [`SDDM_REXON_THEME_CONF`]. Но `/etc/sddm.conf` SDDM читает после всех
/// `*.conf`, и `Current=` в нём (так у CachyOS: `Current=breeze`) молча
/// перекрывал выбор — «Применено», а экран входа прежний. Теперь значение
/// меняется в КАЖДОМ файле `/etc`, где ключ уже есть (и настройки KDE
/// `kde_settings.conf` показывают ту же тему), а свой файл пишется, только
/// если итог без него не тот. Итог сверяется тем же чтением, что у SDDM.
/// Файлы пакетов (`/usr/lib/…`) не правятся: их перекрывает `/etc`.
pub fn plan_sddm_theme(
    theme: &str,
    read: &dyn Fn(&str) -> Option<String>,
    list: &dyn Fn(&str) -> Vec<String>,
) -> SessionSwitch {
    let src = &SDDM_THEME_SOURCE;
    let mut edits: Vec<SessionEdit> = Vec::new();
    for path in source_files(src, list) {
        if !path.starts_with("/etc/") {
            continue;
        }
        if let Some(text) = read(&path).and_then(|t| rewrite_ini_values(&t, src.keys, &|_| true, &|_| theme.to_string())) {
            edits.push(SessionEdit { path, text });
        }
    }
    let effective = |edits: &[SessionEdit]| {
        let after = |p: &str| edits.iter().find(|e| e.path == p).map(|e| e.text.clone()).or_else(|| read(p));
        let list_after = |d: &str| {
            let mut names = list(d);
            for e in edits {
                if let Some(n) = e.path.strip_prefix(&format!("{d}/")) {
                    if !names.iter().any(|x| x == n) {
                        names.push(n.to_string());
                    }
                }
            }
            names
        };
        sddm_effective_theme(&after, &list_after)
    };
    if effective(&edits).as_deref() != Some(theme) && !edits.iter().any(|e| e.path == SDDM_REXON_THEME_CONF) {
        edits.push(SessionEdit {
            path: SDDM_REXON_THEME_CONF.to_string(),
            text: format!("[Theme]\nCurrent={theme}\n"),
        });
    }
    if effective(&edits).as_deref() != Some(theme) {
        SessionSwitch::Elsewhere
    } else if edits.is_empty() {
        SessionSwitch::Nothing
    } else {
        SessionSwitch::Edits(edits)
    }
}

// ── Экран входа SDDM: фон темы и перенос оформления стола ─────────────────
//
// Паритет с модулем KDE «Экран входа (SDDM)» (sddm-kcm 6.8.2: `src/sddmkcm.cpp`
// `save`/`synchronizeSettings`/`resetSyncronizedSettings`, `sddmauthhelper.cpp`,
// `src/themesmodel.cpp`, `src/ui/main.qml`):
// * «Изменить фон» темы: картинка копируется в каталог темы, её имя — ключ
//   `[General] background=` файла `<ConfigFile>.user` рядом с файлом
//   настроек темы (`ConfigFile=` в `metadata.desktop`), плюс `type=image`.
//   Тема без файла настроек фон не принимает (`supportsBackground`).
// * «Применить параметры Plasma»: тема и размер курсора (`[Theme]
//   CursorTheme/CursorSize`), шрифт (`[Theme] Font`), NumLock (`[General]
//   Numlock`), DPI (`[X11] ServerArguments=-dpi N`) — в
//   `/etc/sddm.conf.d/kde_settings.conf` (тот же ключ из `/etc/sddm.conf`
//   снимается); файлы `kdeglobals` (цвета), `plasmarc` (тема Plasma),
//   `kcminputrc`, `kwinoutputconfig.json` (мониторы на Wayland) и fontconfig
//   — в `~sddm/.config`, кэш `~sddm/.cache` стирается. «Сбросить» снимает
//   ключи и файлы.
//
// Отличия от kcm — намеренные:
// * картинка ложится под своим именем [`SDDM_BACKGROUND_STEM`], только PNG
//   или JPEG (по сигнатуре); прежняя удаляется, только если она наша (kcm
//   удаляет файл по прежнему значению ключа — и абсолютный путь вне темы);
// * «Фон темы» возвращает фон самой темы (у kcm «Clear Image» ставит
//   `type=color` — сплошной цвет);
// * `-dpi N` дописывается к прежним аргументам X-сервера, а не заменяет их
//   (kcm затирает `-nolisten tcp` из настроек пакета); при `forceFontDPI=0`
//   (масштаб не задан) DPI не трогается, у kcm уходит `-dpi 0`;
// * ключ пишется туда, откуда SDDM его действительно прочтёт (как
//   [`plan_sddm_theme`]), итог сверяется тем же чтением;
// * файлы в домашнем каталоге `sddm` пишутся от имени `sddm`, не root (так
//   делает Plasma Login Manager с 6.7 после CVE-2026-25710);
// * fontconfig — только `fonts.conf` (как у Plasma Login Manager);
//   раскладка мониторов X11 (`~/.local/share/kscreen`) не переносится, при
//   «Сбросить» снимается.

/// Настройки, которые пишет модуль KDE «Экран входа (SDDM)».
pub const SDDM_KDE_SETTINGS_CONF: &str = "/etc/sddm.conf.d/kde_settings.conf";
/// Имя картинки фона, которую Rexon кладёт в каталог темы (расширение — по
/// сигнатуре файла).
pub const SDDM_BACKGROUND_STEM: &str = "rexon-background";
/// Предел картинки фона.
pub const SDDM_BACKGROUND_MAX_BYTES: u64 = 64 * 1024 * 1024;
/// Предел одного файла настроек стола, переносимого на экран входа (у
/// Plasma Login Manager — тот же 1 МиБ на конфиг).
pub const SDDM_LOOK_FILE_MAX_BYTES: u64 = 1024 * 1024;
/// Системная учётная запись приветствия SDDM.
pub const SDDM_USER: &str = "sddm";
/// Файлы стола, которые переносятся в `~sddm/.config` (путь — внутри
/// `.config` и пользователя, и `sddm`). Других помощник не примет.
pub const SDDM_LOOK_FILES: &[&str] =
    &["kdeglobals", "plasmarc", "kcminputrc", "kwinoutputconfig.json", "fontconfig/fonts.conf"];
/// Что снимает «Сбросить» в домашнем каталоге `sddm` (кроме кэша): файлы
/// переноса и раскладка мониторов kscreen, которую клал kcm.
pub const SDDM_LOOK_RESET_PATHS: &[&str] = &[
    ".config/kdeglobals",
    ".config/plasmarc",
    ".config/kcminputrc",
    ".config/kwinoutputconfig.json",
    ".config/fontconfig/fonts.conf",
    ".local/share/kscreen",
];

/// Ключи настроек SDDM, которые трогает перенос оформления (как у kcm).
pub const SDDM_LOOK_KEYS: &[SessionKey] = &[
    SessionKey { group: "Theme", key: "CursorTheme" },
    SessionKey { group: "Theme", key: "CursorSize" },
    SessionKey { group: "Theme", key: "Font" },
    SessionKey { group: "X11", key: "ServerArguments" },
    SessionKey { group: "General", key: "Numlock" },
];

const SDDM_LOOK_SOURCE: SessionSource =
    SessionSource { dirs: &[SDDM_DISTRO_CONF_D, SDDM_CONF_D], file: "/etc/sddm.conf", keys: SDDM_LOOK_KEYS };

/// NumLock при входе (`[General] Numlock` SDDM).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SddmNumlock {
    On,
    Off,
    None,
}

impl SddmNumlock {
    /// Значение ключа SDDM.
    pub fn value(self) -> &'static str {
        match self {
            SddmNumlock::On => "on",
            SddmNumlock::Off => "off",
            SddmNumlock::None => "none",
        }
    }

    /// Из `kcminputrc [Keyboard] NumLock` Plasma (0 — включить, 1 —
    /// выключить, 2 — не трогать), как переводит kcm.
    pub fn from_kcminput(v: &str) -> Option<Self> {
        match v.trim() {
            "0" => Some(SddmNumlock::On),
            "1" => Some(SddmNumlock::Off),
            "2" => Some(SddmNumlock::None),
            _ => None,
        }
    }
}

/// Значения стола для экрана входа. `None` у курсора — «убрать» (как kcm,
/// когда в Plasma ключа нет); у шрифта, NumLock и DPI — «не трогать».
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SddmLook {
    pub cursor_theme: Option<String>,
    pub cursor_size: Option<u32>,
    pub font: Option<String>,
    pub numlock: Option<SddmNumlock>,
    pub dpi: Option<u32>,
}

/// Аргументы X-сервера с `-dpi N`: прежнее `-dpi …` убирается, новое
/// дописывается (`None` — только убрать).
pub fn server_args_with_dpi(current: &str, dpi: Option<u32>) -> String {
    let words: Vec<&str> = current.split_whitespace().collect();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < words.len() {
        if words[i] == "-dpi" {
            i += 2;
            continue;
        }
        out.push(words[i].to_string());
        i += 1;
    }
    if let Some(d) = dpi {
        out.push("-dpi".into());
        out.push(d.to_string());
    }
    out.join(" ")
}

/// Каким должен стать ключ `k` (итог чтения SDDM `current`): `None` — не
/// трогать, `Some(None)` — убрать из файлов `/etc`, `Some(Some(v))` —
/// значение. `look: None` — «Сбросить».
fn sddm_look_want(k: &SessionKey, look: Option<&SddmLook>, current: Option<&str>) -> Option<Option<String>> {
    let nonempty = |s: String| (!s.trim().is_empty()).then_some(s);
    match (look, k.key) {
        (Some(l), "CursorTheme") => Some(l.cursor_theme.clone()),
        (Some(l), "CursorSize") => Some(l.cursor_size.map(|n| n.to_string())),
        (Some(l), "Font") => l.font.clone().map(Some),
        (Some(l), "Numlock") => l.numlock.map(|n| Some(n.value().to_string())),
        (Some(l), "ServerArguments") => l.dpi.map(|d| nonempty(server_args_with_dpi(current.unwrap_or(""), Some(d)))),
        (None, "ServerArguments") => current
            .filter(|c| c.split_whitespace().any(|w| w == "-dpi"))
            .map(|c| nonempty(server_args_with_dpi(c, None))),
        (None, _) => Some(None),
        _ => None,
    }
}

/// Текст без строк `ключ=…` из `keys` (группа — по последнему заголовку).
/// Группа, которая от этого опустела (ни одного ключа, только пустые
/// строки и заголовок), снимается целиком. `None` — таких строк не было.
pub fn drop_ini_keys(text: &str, keys: &[SessionKey]) -> Option<String> {
    // Разбор на группы: (заголовок или пусто для начала файла, строки).
    let mut groups: Vec<(String, Vec<&str>)> = vec![(String::new(), Vec::new())];
    for line in text.split_inclusive('\n') {
        let t = line.trim();
        if let Some(name) = t.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
            groups.push((name.trim().to_string(), vec![line]));
        } else if let Some(g) = groups.last_mut() {
            g.1.push(line);
        }
    }
    let mut changed = false;
    let mut out = String::with_capacity(text.len());
    for (name, lines) in groups {
        let mut dropped = false;
        let kept: Vec<&str> = lines
            .into_iter()
            .filter(|line| {
                let t = line.trim();
                let hit = !t.starts_with('#')
                    && !t.starts_with(';')
                    && !t.starts_with('[')
                    && t.split_once('=').is_some_and(|(k, _)| keys.iter().any(|sk| sk.group == name && sk.key == k.trim()));
                dropped |= hit;
                !hit
            })
            .collect();
        changed |= dropped;
        let empty = kept.iter().all(|l| {
            let t = l.trim();
            t.is_empty() || t.starts_with('[')
        });
        if dropped && !name.is_empty() && empty {
            continue;
        }
        kept.iter().for_each(|l| out.push_str(l));
    }
    changed.then_some(out)
}

/// Записать `[group] key=value` в текст (`None` — файла нет): значение
/// правится на месте; новый ключ встаёт после последнего ключа группы (до
/// пустых строк перед следующей); группы нет — дописывается в конец.
pub(crate) fn ini_put(existing: Option<&str>, group: &str, key: &str, value: &str) -> String {
    let text = existing.unwrap_or("");
    if text.trim().is_empty() {
        return format!("[{group}]\n{key}={value}\n");
    }
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let mut cur = String::new();
    let mut last_in_group: Option<usize> = None;
    for (i, line) in lines.iter().enumerate() {
        let t = line.trim();
        if let Some(name) = t.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
            cur = name.trim().to_string();
            if cur == group {
                last_in_group = Some(i);
            }
            continue;
        }
        if cur != group || t.is_empty() {
            continue;
        }
        if !t.starts_with('#') && !t.starts_with(';') && t.split_once('=').is_some_and(|(k, _)| k.trim() == key) {
            let mut out: String = lines[..i].concat();
            out.push_str(&format!("{key}={value}\n"));
            out.push_str(&lines[i + 1..].concat());
            return out;
        }
        last_in_group = Some(i);
    }
    let mut out = String::with_capacity(text.len() + key.len() + value.len() + 4);
    match last_in_group {
        Some(i) => {
            out.push_str(&lines[..=i].concat());
            if !out.ends_with('\n') {
                out.push('\n');
            }
            out.push_str(&format!("{key}={value}\n"));
            out.push_str(&lines[i + 1..].concat());
        }
        None => {
            out.push_str(text);
            if !out.ends_with('\n') {
                out.push('\n');
            }
            if !out.ends_with("\n\n") {
                out.push('\n');
            }
            out.push_str(&format!("[{group}]\n{key}={value}\n"));
        }
    }
    out
}

/// Что переписать, чтобы экран входа SDDM получил значения стола `look`
/// (`None` — «Сбросить»: ключи переноса снимаются, из аргументов X-сервера
/// уходит только `-dpi`). Как [`plan_sddm_theme`]: правятся файлы `/etc`,
/// где ключ уже задан; если итог чтения SDDM не тот —
/// [`SDDM_KDE_SETTINGS_CONF`]. Файлы пакетов не правятся. `Elsewhere` —
/// значение перекрыто там, где Rexon не правит.
pub fn plan_sddm_look(
    look: Option<&SddmLook>,
    read: &dyn Fn(&str) -> Option<String>,
    list: &dyn Fn(&str) -> Vec<String>,
) -> SessionSwitch {
    let src = &SDDM_LOOK_SOURCE;
    let mut edits: Vec<SessionEdit> = Vec::new();
    let after = |edits: &[SessionEdit], p: &str| edits.iter().find(|e| e.path == p).map(|e| e.text.clone()).or_else(|| read(p));
    let files_after = |edits: &[SessionEdit]| {
        let list_after = |d: &str| {
            let mut names = list(d);
            for e in edits {
                if let Some(n) = e.path.strip_prefix(&format!("{d}/")) {
                    if !names.iter().any(|x| x == n) {
                        names.push(n.to_string());
                    }
                }
            }
            names
        };
        source_files(src, &list_after)
    };
    let effective = |edits: &[SessionEdit], k: &SessionKey| {
        let texts: Vec<String> = files_after(edits).iter().filter_map(|p| after(edits, p)).collect();
        crate::login_rescue::ini_value(&texts, k.group, k.key)
    };
    let put = |edits: &mut Vec<SessionEdit>, path: &str, text: String| match edits.iter_mut().find(|e| e.path == path) {
        Some(e) => e.text = text,
        None => edits.push(SessionEdit { path: path.to_string(), text }),
    };
    let mut wanted: Vec<(SessionKey, String)> = Vec::new();
    for k in SDDM_LOOK_KEYS {
        let current = effective(&edits, k);
        let Some(mut want) = sddm_look_want(k, look, current.as_deref()) else { continue };
        // «Сбросить»: если без строк `/etc` ключ и так даёт нужное (`-dpi`
        // ушло, осталось значение пакета), строки убираются, а не
        // переписываются копией значения пакета.
        if look.is_none() && want.is_some() {
            let mut trial = edits.clone();
            for path in files_after(&trial).into_iter().filter(|p| p.starts_with("/etc/")) {
                if let Some(n) = after(&trial, &path).and_then(|t| drop_ini_keys(&t, std::slice::from_ref(k))) {
                    put(&mut trial, &path, n);
                }
            }
            if effective(&trial, k) == want {
                want = None;
            }
        }
        for path in files_after(&edits).into_iter().filter(|p| p.starts_with("/etc/")) {
            let Some(text) = after(&edits, &path) else { continue };
            let new = match &want {
                None => drop_ini_keys(&text, std::slice::from_ref(k)),
                Some(v) => rewrite_ini_values(&text, std::slice::from_ref(k), &|_| true, &|_| v.clone()),
            };
            if let Some(n) = new {
                put(&mut edits, &path, n);
            }
        }
        if let Some(v) = want {
            if effective(&edits, k).as_deref() != Some(v.as_str()) {
                let base = after(&edits, SDDM_KDE_SETTINGS_CONF);
                put(&mut edits, SDDM_KDE_SETTINGS_CONF, ini_put(base.as_deref(), k.group, k.key, &v));
            }
            wanted.push((*k, v));
        }
    }
    if wanted.iter().any(|(k, v)| effective(&edits, k).as_deref() != Some(v.as_str())) {
        SessionSwitch::Elsewhere
    } else if edits.is_empty() {
        SessionSwitch::Nothing
    } else {
        SessionSwitch::Edits(edits)
    }
}

/// Имя файла настроек темы SDDM (`[SddmGreeterTheme] ConfigFile=` в
/// `metadata.desktop`) — только простое имя в каталоге темы.
pub fn sddm_theme_config_name(metadata: &str) -> Option<String> {
    let v = crate::login_rescue::ini_value(&[metadata.to_string()], "SddmGreeterTheme", "ConfigFile")?;
    let ok = !v.is_empty()
        && !v.contains('/')
        && !v.starts_with('.')
        && !v.starts_with('-')
        && !v.chars().any(|c| c.is_control());
    ok.then_some(v)
}

/// Фон темы SDDM, как его видит kcm (`ThemesModel::add`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct SddmBackground {
    /// Тема принимает свой фон: у неё есть файл настроек.
    pub supported: bool,
    /// Полный путь файла настроек темы (пусто — нет).
    pub config: String,
    /// Полный путь картинки, которую тема покажет (пусто — не задана).
    pub image: String,
    /// Фон задан поверх темы (`<ConfigFile>.user`).
    pub custom: bool,
    /// Этот фон положил Rexon ([`SDDM_BACKGROUND_STEM`]).
    pub ours: bool,
    /// «Показывать часы» (`showClock` из `.user` поверх файла темы; нет —
    /// да, как у kcm `ThemesModel::add`).
    pub clock: bool,
    /// Qt приветствия темы ([`sddm_theme_qt_version`]).
    pub qt: u32,
    /// Приветствие этой темы покажет WebP: у его Qt есть модуль WebP
    /// ([`qt_has_webp`]). Заполняет вызывающий — по файлам темы не узнать.
    pub webp: bool,
    /// Своя копия Rexon ([`kept_login_background`]); пусто — нет. Заполняет
    /// вызывающий.
    pub kept: String,
    /// Файл `image` читается этим пользователем. Заполняет вызывающий.
    pub readable: bool,
}

/// Значение флага так, как его читает KConfig (`readEntry(…, bool)`):
/// `true`/`on`/`yes`/`1` — да, `false`/`off`/`no`/`0` — нет, прочее —
/// не задано.
pub fn kconfig_bool(v: &str) -> Option<bool> {
    match v.trim().to_ascii_lowercase().as_str() {
        "true" | "on" | "yes" | "1" => Some(true),
        "false" | "off" | "no" | "0" => Some(false),
        _ => None,
    }
}

/// Qt приветствия темы SDDM: `[SddmGreeterTheme] QtVersion` (нет или не
/// число — 5, как `ThemeMetadata` SDDM 0.21).
pub fn sddm_theme_qt_version(metadata: &str) -> u32 {
    crate::login_rescue::ini_value(&[metadata.to_string()], "SddmGreeterTheme", "QtVersion")
        .and_then(|v| v.trim().parse::<u32>().ok())
        .filter(|v| *v >= 5)
        .unwrap_or(5)
}

/// Фон темы в каталоге `theme_dir` по её файлам.
pub fn sddm_theme_background(theme_dir: &str, read: &dyn Fn(&str) -> Option<String>) -> SddmBackground {
    let dir = theme_dir.trim_end_matches('/');
    let metadata = read(&format!("{dir}/metadata.desktop"));
    let qt = metadata.as_deref().map(sddm_theme_qt_version).unwrap_or(5);
    let Some(name) = metadata.and_then(|m| sddm_theme_config_name(&m)) else {
        return SddmBackground { qt, ..SddmBackground::default() };
    };
    let config = format!("{dir}/{name}");
    let Some(base) = read(&config) else {
        return SddmBackground { qt, ..SddmBackground::default() };
    };
    let user = read(&format!("{config}.user"));
    let user_bg = user.as_ref().and_then(|u| crate::login_rescue::ini_value(&[u.clone()], "General", "background"));
    // Как KConfig с каскадом: значение `.user` сильнее файла темы.
    let clock = user
        .as_ref()
        .and_then(|u| crate::login_rescue::ini_value(&[u.clone()], "General", "showClock"))
        .or_else(|| crate::login_rescue::ini_value(&[base.clone()], "General", "showClock"))
        .and_then(|v| kconfig_bool(&v))
        .unwrap_or(true);
    let bg = user_bg.clone().or_else(|| crate::login_rescue::ini_value(&[base], "General", "background")).unwrap_or_default();
    let image = if bg.is_empty() {
        String::new()
    } else if bg.starts_with('/') {
        bg
    } else {
        format!("{dir}/{bg}")
    };
    let custom = user_bg.as_deref().is_some_and(|b| !b.is_empty());
    let ours = user_bg.as_deref().is_some_and(|b| sddm_background_is_ours(b));
    SddmBackground { supported: true, config, image, custom, ours, clock, qt, ..SddmBackground::default() }
}

/// Имя файла — картинка фона, положенная Rexon (`rexon-background.png`).
pub fn sddm_background_is_ours(name: &str) -> bool {
    name.strip_prefix(SDDM_BACKGROUND_STEM)
        .and_then(|r| r.strip_prefix('.'))
        .is_some_and(|ext| matches!(ext, "png" | "jpg" | "webp"))
}

/// Новый текст `<ConfigFile>.user` с «Показывать часы» = `show` (как kcm:
/// ключ `showClock` пишется явно и при «да»). `None` — менять нечего.
pub fn sddm_clock_user_text(existing: Option<&str>, show: bool) -> Option<String> {
    let t = ini_put(existing, "General", "showClock", if show { "true" } else { "false" });
    (existing != Some(t.as_str())).then_some(t)
}

/// Где у Qt `qt` лежит модуль чтения WebP (`imageformats/libqwebp.so`):
/// раскладки Arch, Fedora/openSUSE и Debian/Ubuntu.
pub fn qt_webp_plugin_paths(qt: u32) -> Vec<String> {
    let triplet = format!("{}-linux-gnu", std::env::consts::ARCH);
    let dirs: Vec<String> = if qt == 5 {
        vec![
            "/usr/lib/qt/plugins".into(),
            "/usr/lib64/qt5/plugins".into(),
            format!("/usr/lib/{triplet}/qt5/plugins"),
        ]
    } else {
        vec![
            format!("/usr/lib/qt{qt}/plugins"),
            format!("/usr/lib64/qt{qt}/plugins"),
            format!("/usr/lib/{triplet}/qt{qt}/plugins"),
        ]
    };
    dirs.into_iter().map(|d| format!("{d}/imageformats/libqwebp.so")).collect()
}

/// Покажет ли приветствие на Qt `qt` картинку WebP: модуль есть хоть в
/// одной из раскладок ([`qt_webp_plugin_paths`]).
pub fn qt_has_webp(qt: u32, exists: &dyn Fn(&str) -> bool) -> bool {
    qt_webp_plugin_paths(qt).iter().any(|p| exists(p))
}

/// [`qt_has_webp`] на живой машине.
pub fn qt_has_webp_live(qt: u32) -> bool {
    qt_has_webp(qt, &|p| std::path::Path::new(p).is_file())
}

/// Новый текст `<ConfigFile>.user`: `image` — имя картинки в каталоге темы
/// (`background=`, `type=image`); `None` — вернуть фон темы (оба ключа
/// снимаются). `None` в ответе — менять нечего.
pub fn sddm_background_user_text(existing: Option<&str>, image: Option<&str>) -> Option<String> {
    match image {
        Some(name) => {
            let t = ini_put(existing, "General", "background", name);
            let t = ini_put(Some(&t), "General", "type", "image");
            (existing != Some(t.as_str())).then_some(t)
        }
        None => drop_ini_keys(
            existing?,
            &[SessionKey { group: "General", key: "background" }, SessionKey { group: "General", key: "type" }],
        ),
    }
}

/// Расширение картинки по сигнатуре: PNG или JPEG — то, что приветствие
/// SDDM на Qt покажет без дополнительных модулей изображений.
pub fn login_image_ext(head: &[u8]) -> Option<&'static str> {
    if head.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]) {
        Some("png")
    } else if head.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("jpg")
    } else {
        None
    }
}

/// Сигнатура WebP: `RIFF`, размер, `WEBP`.
pub fn is_webp(head: &[u8]) -> bool {
    head.len() >= 12 && head.starts_with(b"RIFF") && &head[8..12] == b"WEBP"
}

/// Как [`login_image_ext`], но `webp` — приветствие покажет и WebP (у его
/// Qt есть модуль, [`qt_has_webp`]).
pub fn login_image_ext_with(head: &[u8], webp: bool) -> Option<&'static str> {
    login_image_ext(head).or_else(|| (webp && is_webp(head)).then_some("webp"))
}

/// Отказ до пароля: файл — не картинка, которую покажет экран входа.
/// `webp` — модуль WebP у Qt приветствия есть; `qt` — версия Qt (для
/// подсказки, какого модуля не хватает).
pub fn login_image_refusal(head: &[u8], webp: bool, qt: u32) -> String {
    if is_webp(head) && !webp {
        // TRANSLATORS: %1 is a Qt major version, e.g. "6"; "WebP" and "Qt" are names, keep them as is.
        return trf(
            "The login screen can't show WebP pictures: the WebP image module of Qt %1 isn't installed \
             (package qt%1-imageformats or similar). Pick a PNG or JPEG file.",
            &[&qt.to_string()],
        );
    }
    if webp {
        tr("The login screen shows PNG, JPEG and WebP pictures only — pick another file.")
    } else {
        tr("The login screen shows PNG and JPEG pictures only — pick another file.")
    }
}

// ── Своя копия картинки фона экрана входа ───────────────────────────────
//
// Картинку, поставленную на экран входа через Rexon, Rexon держит и у
// себя: `<данные>/rexon/login-background/<вид>.<расширение>` плюс отметка
// `<вид>.json` (какой файл выбран и когда). Зачем: дом `plasmalogin` закрыт
// (0750) — копию в нём окно не прочтёт ни для превью, ни для повторной
// отправки помощнику PLM; у SDDM — то же правило для единообразия. Вид —
// [`LOGIN_BG_SDDM`] или [`LOGIN_BG_PLM`]. «Вернуть стандартный фон» копию
// убирает.

/// Вид копии фона: экран входа SDDM.
pub const LOGIN_BG_SDDM: &str = "sddm";
/// Вид копии фона: Plasma Login Manager.
pub const LOGIN_BG_PLM: &str = "plm";

/// Каталог копий в корне данных пользователя `data_home`.
pub fn login_background_keep_dir(data_home: &str) -> String {
    format!("{}/rexon/login-background", data_home.trim_end_matches('/'))
}

/// Отметка копии: имя файла в каталоге, откуда взята картинка и когда
/// (секунды Unix).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct KeptBackground {
    pub file: String,
    pub source: String,
    pub set_at: u64,
}

/// Текст отметки `<вид>.json`.
pub fn kept_background_marker(kept: &KeptBackground) -> String {
    serde_json::json!({"file": kept.file, "source": kept.source, "set_at": kept.set_at}).to_string() + "\n"
}

/// Копия вида `kind` в каталоге `dir`: путь к картинке и отметка. Нет
/// отметки, файл не наш по имени (`<вид>.<png|jpg|webp>`) или его нет —
/// `None`.
pub fn kept_login_background(
    dir: &str,
    kind: &str,
    read: &dyn Fn(&str) -> Option<String>,
    exists: &dyn Fn(&str) -> bool,
) -> Option<(String, KeptBackground)> {
    let marker: serde_json::Value = serde_json::from_str(&read(&format!("{dir}/{kind}.json"))?).ok()?;
    let file = marker.get("file")?.as_str()?.to_string();
    let ok = file.strip_prefix(kind).and_then(|r| r.strip_prefix('.')).is_some_and(|e| matches!(e, "png" | "jpg" | "webp"));
    let path = format!("{dir}/{file}");
    if !ok || !exists(&path) {
        return None;
    }
    let kept = KeptBackground {
        file,
        source: marker.get("source").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
        set_at: marker.get("set_at").and_then(|v| v.as_u64()).unwrap_or(0),
    };
    Some((path, kept))
}

/// Положить копию картинки `source` (расширение `ext` — по сигнатуре) как
/// вид `kind` в каталог `dir` с отметкой; прежние копии этого вида уходят.
pub fn keep_login_background_in(dir: &str, kind: &str, source: &str, ext: &str, now: u64) -> std::io::Result<String> {
    drop_login_background_in(dir, kind);
    std::fs::create_dir_all(dir)?;
    let file = format!("{kind}.{ext}");
    let path = format!("{dir}/{file}");
    std::fs::copy(source, &path)?;
    let marker = kept_background_marker(&KeptBackground { file, source: source.to_string(), set_at: now });
    std::fs::write(format!("{dir}/{kind}.json"), marker)?;
    Ok(path)
}

/// Убрать копию вида `kind` (картинки всех расширений и отметку).
pub fn drop_login_background_in(dir: &str, kind: &str) {
    for name in ["png", "jpg", "webp", "json"].map(|e| format!("{dir}/{kind}.{e}")) {
        let _ = std::fs::remove_file(name);
    }
}

/// Каталог копий на живой машине (`$XDG_DATA_HOME` или `~/.local/share`).
pub fn login_background_keep_dir_live() -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    let xdg = std::env::var("XDG_DATA_HOME").ok().filter(|v| !v.is_empty());
    login_background_keep_dir(&crate::utils::data_home(xdg.as_deref(), &home))
}

/// Путь своей копии вида `kind` на живой машине.
pub fn kept_login_background_live(kind: &str) -> Option<String> {
    let dir = login_background_keep_dir_live();
    kept_login_background(&dir, kind, &|p| std::fs::read_to_string(p).ok(), &|p| std::path::Path::new(p).is_file())
        .map(|(p, _)| p)
}

/// После удачной смены фона: картинка `image` — положить копию (сбой копии
/// не отменяет смену, только пишется в журнал), `None` — убрать копию.
pub fn sync_kept_login_background_live(kind: &str, image: Option<&str>, webp: bool) {
    use std::io::Read;
    let dir = login_background_keep_dir_live();
    let Some(src) = image else {
        drop_login_background_in(&dir, kind);
        return;
    };
    let mut head = [0u8; 16];
    let n = std::fs::File::open(src).and_then(|mut f| f.read(&mut head)).unwrap_or(0);
    let Some(ext) = login_image_ext_with(&head[..n], webp) else {
        return;
    };
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    if let Err(e) = keep_login_background_in(&dir, kind, src, ext, now) {
        crate::log_warn!("login", "копия фона экрана входа не сохранилась: {e}");
    }
}

/// Превью и подпись своего фона (задан поверх стандартного/темы). `ours` —
/// картинку положил Rexon (`rexon-background.*`); `current` — путь, на
/// который указывает настройка; `kept` — своя копия Rexon; `readable` —
/// можно ли прочесть файл. Чужой или нечитаемый — честная подпись без
/// превью.
pub fn custom_background_preview(
    ours: bool,
    current: &str,
    kept: Option<&str>,
    readable: &dyn Fn(&str) -> bool,
) -> (String, String) {
    if ours {
        if let Some(k) = kept {
            return (k.to_string(), tr("Background: your picture"));
        }
        if readable(current) {
            return (current.to_string(), tr("Background: your picture"));
        }
        return (String::new(), tr("Background: your picture — no preview, Rexon has no copy of it"));
    }
    if readable(current) {
        (current.to_string(), tr("Background: set outside Rexon"))
    } else {
        (String::new(), tr("Background: set outside Rexon — no preview, the file can't be read"))
    }
}

/// Фильтр окна выбора фона экрана входа.
pub fn login_image_filter(webp: bool) -> String {
    if webp {
        // TRANSLATORS: file dialog filter; keep the patterns in parentheses as is.
        tr("Images (*.png *.jpg *.jpeg *.webp)")
    } else {
        // TRANSLATORS: file dialog filter; keep the patterns in parentheses as is.
        tr("Images (*.png *.jpg *.jpeg)")
    }
}

/// Что с этого стола уйдёт на экран входа: значения, файлы
/// (имя из [`SDDM_LOOK_FILES`] → путь у пользователя) и подписи для
/// человека — что переносится и чего нет.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DesktopLook {
    pub look: SddmLook,
    pub files: Vec<(String, String)>,
    pub items: Vec<String>,
    pub missing: Vec<String>,
}

/// Курсор и шрифт вне Plasma — как их читает «Оформление» для этого стола
/// (шрифт — строкой стола: «Noto Sans 11» или запись Qt).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OtherDesktopLook {
    pub cursor_theme: Option<String>,
    pub cursor_size: Option<u32>,
    pub font: Option<String>,
}

fn look_item_cursor() -> String {
    tr("Cursor theme and size")
}
fn look_item_font() -> String {
    tr("Font")
}
fn look_item_rendering() -> String {
    tr("Font rendering")
}
fn look_item_colors() -> String {
    tr("Color scheme")
}
fn look_item_plasma_theme() -> String {
    tr("Plasma theme")
}
fn look_item_numlock() -> String {
    tr("NumLock preference")
}
fn look_item_dpi() -> String {
    tr("Scaling DPI")
}
fn look_item_screens() -> String {
    tr("Screen configuration (Wayland only)")
}

/// Значения и файлы стола для экрана входа. `plasma` — сеанс Plasma:
/// тогда всё как у kcm (`kcminputrc`, `kdeglobals`, `kcmfonts` в
/// `config_home`); иначе курсор и шрифт — из `other`, а из файлов —
/// только цвета Qt (`kdeglobals`) и `fontconfig`, если они есть.
/// `exists` — есть ли файл у пользователя.
pub fn desktop_greeter_look(
    plasma: bool,
    config_home: &str,
    read: &dyn Fn(&str) -> Option<String>,
    exists: &dyn Fn(&str) -> bool,
    other: &OtherDesktopLook,
) -> DesktopLook {
    let cfg = config_home.trim_end_matches('/');
    let ini = |file: &str, group: &str, key: &str| {
        read(&format!("{cfg}/{file}")).and_then(|t| crate::themes::ini_key_value(&t, group, key))
    };
    let mut out = DesktopLook::default();
    if plasma {
        out.look.cursor_theme = ini("kcminputrc", "Mouse", "cursorTheme");
        out.look.cursor_size = ini("kcminputrc", "Mouse", "cursorSize").and_then(|v| crate::themes::parse_cursor_size(&v));
        out.look.font = ini("kdeglobals", "General", "font");
        out.look.numlock = ini("kcminputrc", "Keyboard", "NumLock").and_then(|v| SddmNumlock::from_kcminput(&v));
        out.look.dpi = ini("kcmfonts", "General", "forceFontDPI")
            .and_then(|v| v.parse::<u32>().ok())
            .filter(|d| (48..=480).contains(d));
    } else {
        out.look.cursor_theme = other.cursor_theme.clone();
        out.look.cursor_size = other.cursor_size;
        out.look.font = other.font.as_deref().and_then(|f| {
            let (family, size, face) = crate::qt_settings::parse_font_spec_face(f);
            (!family.is_empty())
                .then(|| crate::qt_settings::qt_font_spec_face(&family, if size > 0 { size } else { 10 }, face))
        });
    }
    let wanted: &[&str] = if plasma { SDDM_LOOK_FILES } else { &["kdeglobals", "fontconfig/fonts.conf"] };
    for name in wanted {
        let path = format!("{cfg}/{name}");
        if exists(&path) {
            out.files.push((name.to_string(), path));
        }
    }
    let has = |n: &str| out.files.iter().any(|(f, _)| f == n);
    let mut rows: Vec<(String, bool)> = vec![
        (look_item_colors(), has("kdeglobals")),
        (look_item_cursor(), out.look.cursor_theme.is_some()),
        (look_item_font(), out.look.font.is_some()),
        (look_item_rendering(), has("fontconfig/fonts.conf")),
    ];
    if plasma {
        rows.push((look_item_numlock(), out.look.numlock.is_some()));
        rows.push((look_item_plasma_theme(), has("plasmarc")));
        rows.push((look_item_dpi(), out.look.dpi.is_some()));
        rows.push((look_item_screens(), has("kwinoutputconfig.json")));
    } else {
        // Вне Plasma этого у стола нет в виде, понятном экрану входа.
        for item in [look_item_plasma_theme(), look_item_numlock(), look_item_dpi(), look_item_screens()] {
            rows.push((item, false));
        }
    }
    for (label, present) in rows {
        if present {
            out.items.push(label);
        } else {
            out.missing.push(label);
        }
    }
    out
}

/// Раздел «Фон и оформление экрана входа» при SDDM — данные для окна.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SddmGreeterView {
    /// Экран входа — SDDM, тема известна.
    pub visible: bool,
    pub title: String,
    /// «Тема: Breeze».
    pub theme: String,
    /// Можно менять (не SteamOS и не неизменяемая система).
    pub can_change: bool,
    /// Почему нельзя (серым, до пароля).
    pub blocked: String,
    pub background: SddmBackgroundView,
    pub look: SddmLookView,
    pub clock: GreeterClockView,
    pub preview: GreeterPreviewView,
}

/// Галочка «Показывать часы» (у SDDM — `showClock` темы, у Plasma Login
/// Manager — `[Greeter] ShowClock`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GreeterClockView {
    pub supported: bool,
    pub checked: bool,
    pub label: String,
}

/// Кнопка живого превью экрана входа (тестовый режим приветствия).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GreeterPreviewView {
    /// Показать можно; иначе кнопка заперта и `note` — почему.
    pub available: bool,
    pub label: String,
    /// Подпись той же кнопки, пока превью открыто.
    pub close: String,
    pub tooltip: String,
    pub note: String,
}

/// Потолок жизни превью экрана входа: дольше Rexon его не держит (выйти из
/// тестового режима приветствия самому бывает нечем).
pub const LOGIN_PREVIEW_MAX_SECS: u64 = 120;

/// Кнопка превью: `plm` — Plasma Login Manager (выход — подставной вход,
/// его надо назвать); `problem` — почему показать нельзя.
pub fn greeter_preview_view(plm: bool, problem: Option<String>) -> GreeterPreviewView {
    let tooltip = if plm {
        tr("Shows the login screen full screen without logging you out; nothing is changed. To close it, \
            log in as any user with the password “mypassword” (Plasma Login Manager's test mode), or wait \
            two minutes. Its Sleep, Restart and Shut Down buttons work for real.")
    } else {
        tr("Shows the login screen with this theme full screen without logging you out; nothing is \
            changed. It closes by itself after two minutes, or switch back to Rexon (Alt+Tab) and click \
            “Close Preview”.")
    };
    GreeterPreviewView {
        available: problem.is_none(),
        label: tr("Preview"),
        close: tr("Close Preview"),
        tooltip,
        note: problem.unwrap_or_default(),
    }
}

/// Галочка «Показывать часы».
pub fn greeter_clock_view(supported: bool, checked: bool) -> GreeterClockView {
    GreeterClockView { supported, checked, label: tr("Show clock") }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SddmBackgroundView {
    pub supported: bool,
    /// Картинка для превью (`file:` — забота окна); пусто — не задана.
    pub image: String,
    /// Подпись под превью: «свой» или «фон темы».
    pub state: String,
    /// Почему фон не сменить (тема без файла настроек).
    pub note: String,
    pub choose: String,
    /// «Фон темы» — только когда задан свой.
    pub reset: String,
    pub can_reset: bool,
    /// Фильтр окна выбора файла.
    pub filter: String,
    pub dialog_title: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SddmLookView {
    pub apply: String,
    pub reset: String,
    pub confirm_title: String,
    pub confirm: String,
    pub reset_confirm: String,
}

/// Решение раздела по фактам: `theme` — тема SDDM (`None` — встроенная),
/// `name` — её название из `metadata.desktop`, `background` —
/// [`sddm_theme_background`], `look` — [`desktop_greeter_look`], `blocked`
/// — причина, по которой системные файлы не меняются (пусто — можно).
pub fn sddm_greeter_view(
    theme: Option<&str>,
    name: Option<&str>,
    background: &SddmBackground,
    look: &DesktopLook,
    plasma: bool,
    blocked: &str,
) -> SddmGreeterView {
    let Some(theme) = theme.filter(|t| !t.trim().is_empty()) else {
        return SddmGreeterView::default();
    };
    let label = name.filter(|n| !n.trim().is_empty()).unwrap_or(theme);
    let (image, state) = if !background.supported {
        (background.image.clone(), String::new())
    } else if background.custom {
        let kept = (!background.kept.is_empty()).then_some(background.kept.as_str());
        custom_background_preview(background.ours, &background.image, kept, &|_| background.readable)
    } else {
        (background.image.clone(), tr("Background: the theme's own"))
    };
    let list = |items: &[String]| items.iter().map(|i| format!("• {i}")).collect::<Vec<_>>().join("\n");
    let mut confirm = if plasma {
        tr("The login screen will reflect your customizations to the following Plasma settings:")
    } else {
        tr("The login screen will reflect these settings of your desktop:")
    };
    confirm.push('\n');
    confirm.push_str(&list(&look.items));
    if !look.missing.is_empty() {
        confirm.push_str("\n\n");
        confirm.push_str(&if plasma {
            tr("Not set on this desktop, so the login screen keeps its own:")
        } else {
            tr("Not taken from this desktop — the login screen keeps its own:")
        });
        confirm.push('\n');
        confirm.push_str(&list(&look.missing));
    }
    SddmGreeterView {
        visible: true,
        title: tr("Login screen background and look"),
        // TRANSLATORS: %1 is the name of a login screen theme, e.g. "Breeze".
        theme: trf("Theme: %1", &[&label]),
        can_change: blocked.is_empty(),
        blocked: blocked.to_string(),
        background: SddmBackgroundView {
            supported: background.supported,
            image,
            state,
            note: if background.supported {
                String::new()
            } else {
                tr("This theme doesn't let you change its background.")
            },
            choose: tr("Change Background…"),
            reset: tr("Use the Theme's Background"),
            can_reset: background.custom,
            filter: login_image_filter(background.webp),
            dialog_title: tr("Choose a Login Screen Background"),
        },
        clock: greeter_clock_view(background.supported, background.clock),
        preview: greeter_preview_view(false, None),
        look: SddmLookView {
            apply: if plasma { tr("Apply Plasma Settings…") } else { tr("Apply Desktop Settings…") },
            reset: tr("Reset to Default Settings…"),
            confirm_title: if plasma { tr("Apply Plasma Settings") } else { tr("Apply Desktop Settings") },
            confirm,
            reset_confirm: tr(
                "The login screen will go back to its own cursor, font, colors and other settings. \
                 Its theme and background stay as they are.",
            ),
        },
    }
}

/// Системный шрифт стола `de` строкой его хранилища (вне Plasma).
fn live_desktop_font(de: &str, config_home: &str) -> Option<String> {
    use crate::qt_settings as qs;
    let raw = match qs::font_plan(de, "system").primary {
        qs::FontStore::Cmd(backend) => {
            let argv = qs::sound_read_argv(&backend, "kreadconfig6")?;
            let report = crate::utils::run(
                &crate::utils::RunSpec {
                    argv,
                    output: crate::utils::RunOutput::Capture,
                    timeout_secs: Some(5),
                    lang_c: true,
                    ..Default::default()
                },
                None,
            );
            if report.returncode != Some(0) {
                return None;
            }
            crate::utils::decode_text(&report.stdout)
        }
        qs::FontStore::File(file) => {
            let text = std::fs::read_to_string(format!("{}/{}", config_home.trim_end_matches('/'), file.rel_path())).ok()?;
            file.read_spec(&text)
        }
    };
    let v = raw.trim().trim_matches(['\'', '"']).trim().to_string();
    (!v.is_empty()).then_some(v)
}

/// [`desktop_greeter_look`] на живой машине для стола `de` (`kde` — Plasma).
pub fn desktop_greeter_look_live(de: &str) -> DesktopLook {
    let home = std::env::var("HOME").unwrap_or_default();
    let xdg = std::env::var("XDG_CONFIG_HOME").ok().filter(|v| !v.is_empty());
    let cfg = crate::utils::config_home(xdg.as_deref(), &home);
    let plasma = de == "kde";
    let other = if plasma {
        OtherDesktopLook::default()
    } else {
        let cursor = |size| crate::appearance_installer::live_cursor_value(de, &home, xdg.as_deref(), size);
        OtherDesktopLook {
            cursor_theme: cursor(false),
            cursor_size: cursor(true).and_then(|v| crate::themes::parse_cursor_size(&v)),
            font: live_desktop_font(de, &cfg),
        }
    };
    desktop_greeter_look(
        plasma,
        &cfg,
        &|p| std::fs::read_to_string(p).ok(),
        &|p| std::path::Path::new(p).is_file(),
        &other,
    )
}

/// Тема, которую SDDM покажет сейчас (`None` — встроенная), — тем же
/// чтением, что у самого SDDM.
pub fn sddm_current_theme_live() -> Option<String> {
    let read = |p: &str| std::fs::read_to_string(p).ok();
    let list = |d: &str| -> Vec<String> {
        std::fs::read_dir(d)
            .map(|it| it.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect())
            .unwrap_or_default()
    };
    sddm_effective_theme(&read, &list).filter(|t| !t.trim().is_empty())
}

/// [`sddm_greeter_view`] на живой машине: тема SDDM — тем же чтением, что
/// у самого SDDM; на SteamOS менять нельзя (причина ядра).
pub fn sddm_greeter_view_live(de: &str, look: &DesktopLook) -> SddmGreeterView {
    let read = |p: &str| std::fs::read_to_string(p).ok();
    let theme = sddm_current_theme_live();
    let dir = theme.as_deref().map(|t| format!("{SDDM_THEMES_DIR}/{t}")).unwrap_or_default();
    let name = theme.as_ref().and_then(|_| read(&format!("{dir}/metadata.desktop"))).and_then(|m| {
        crate::login_rescue::ini_value(&[m], "SddmGreeterTheme", "Name")
    });
    let mut background = if theme.is_some() { sddm_theme_background(&dir, &read) } else { SddmBackground::default() };
    background.webp = qt_has_webp_live(background.qt);
    background.readable = std::fs::File::open(&background.image).is_ok();
    if background.ours {
        background.kept = kept_login_background_live(LOGIN_BG_SDDM).unwrap_or_default();
    }
    let blocked = if crate::distro_compat::running_on_steamos() {
        crate::distro_compat::SteamOsBlock::LoginScreen.reason()
    } else {
        String::new()
    };
    let mut view = sddm_greeter_view(theme.as_deref(), name.as_deref(), &background, look, de == "kde", &blocked);
    if let Some(t) = theme.as_deref() {
        view.preview = greeter_preview_view(false, sddm_preview_argv_live(t).err());
    }
    view
}

/// Итог операции сеанса прав для человека: `Ok(done)` при коде 0, иначе
/// готовый текст отказа помощника или `failed`.
fn greeter_outcome(out: Option<crate::system_exec::RunOut>, done: String, failed: String) -> Result<String, String> {
    let Some(out) = out else {
        return Err(crate::privilege::client::ClientError::HelperMissing.human_text());
    };
    if out.returncode == Some(0) {
        return Ok(done);
    }
    let why = out.stderr.lines().map(str::trim).rfind(|l| !l.is_empty()).map(str::to_string);
    Err(why.unwrap_or(failed))
}

/// Сменить фон темы SDDM `theme` на картинку `image` (`None` — вернуть фон
/// темы) через сеанс прав окна ([`crate::privilege::op::PrivOp::SddmBackground`]).
/// До пароля — проверка картинки по первым байтам (`head`): заведомый отказ
/// пароля не стоит. Всё остальное помощник от root проверяет сам; сырого
/// пути через `pkexec` нет (З5).
/// `webp`/`qt` — покажет ли приветствие темы WebP и на каком оно Qt
/// ([`qt_has_webp`]).
pub fn sddm_background_with(
    theme: &str,
    image: Option<&str>,
    webp: bool,
    qt: u32,
    head: &dyn Fn(&str) -> Option<Vec<u8>>,
    run: &mut dyn FnMut(&crate::privilege::op::PrivOp, &[String]) -> Option<crate::system_exec::RunOut>,
) -> Result<String, String> {
    use crate::privilege::op::{LocalFile, PrivOp};
    if let Some(path) = image {
        let bytes = head(path).unwrap_or_default();
        if login_image_ext_with(&bytes, webp).is_none() {
            return Err(login_image_refusal(&bytes, webp, qt));
        }
    }
    let op = PrivOp::SddmBackground {
        theme: theme.to_string(),
        image: image.map(|p| LocalFile { slot: 0, label: p.to_string() }),
    };
    let files: Vec<String> = image.map(|p| vec![p.to_string()]).unwrap_or_default();
    let done = if image.is_some() {
        tr("Done. The new background will show the next time the login screen appears.")
    } else {
        tr("Done. The login screen shows the theme's own background again.")
    };
    greeter_outcome(run(&op, &files), done, tr("Couldn't change the login screen background."))
}

/// [`sddm_background_with`] на живой машине.
pub fn sddm_background_live(theme: &str, image: Option<&str>) -> Result<String, String> {
    use std::io::Read;
    let qt = std::fs::read_to_string(format!("{SDDM_THEMES_DIR}/{theme}/metadata.desktop"))
        .map(|m| sddm_theme_qt_version(&m))
        .unwrap_or(5);
    let webp = qt_has_webp_live(qt);
    let done = sddm_background_with(
        theme,
        image,
        webp,
        qt,
        &|p| {
            let mut buf = [0u8; 16];
            let n = std::fs::File::open(p).and_then(|mut f| f.read(&mut buf)).ok()?;
            Some(buf[..n].to_vec())
        },
        &mut |op, files| crate::privilege::run::run_live(op, files, None, &mut |_| {}),
    )?;
    sync_kept_login_background_live(LOGIN_BG_SDDM, image, webp);
    Ok(done)
}

/// «Показывать часы» у темы SDDM `theme` (`showClock` в `<ConfigFile>.user`,
/// как kcm) через сеанс прав окна ([`crate::privilege::op::PrivOp::SddmClock`]).
pub fn sddm_clock_with(
    theme: &str,
    show: bool,
    run: &mut dyn FnMut(&crate::privilege::op::PrivOp, &[String]) -> Option<crate::system_exec::RunOut>,
) -> Result<String, String> {
    let op = crate::privilege::op::PrivOp::SddmClock { theme: theme.to_string(), show };
    let done = if show {
        tr("Done. The login screen will show the clock.")
    } else {
        tr("Done. The login screen won't show the clock.")
    };
    greeter_outcome(run(&op, &[]), done, tr("Couldn't change the login screen clock."))
}

/// [`sddm_clock_with`] на живой машине.
pub fn sddm_clock_live(theme: &str, show: bool) -> Result<String, String> {
    sddm_clock_with(theme, show, &mut |op, files| crate::privilege::run::run_live(op, files, None, &mut |_| {}))
}

// ── Живое превью экрана входа ────────────────────────────────────────────
//
// Сверено с исходниками SDDM 0.21 (`src/greeter/GreeterApp.cpp`,
// `src/daemon/Greeter.cpp`, `src/common/ThemeMetadata.cpp`):
//   * приветствие темы — `sddm-greeter` для тем на Qt 5 и `sddm-greeter-qt6`
//     для `[SddmGreeterTheme] QtVersion=6` (нет ключа — 5); лежит рядом с
//     `sddm` (`BIN_INSTALL_DIR`);
//   * тестовый режим — `--test-mode --theme <каталог темы>`: без демона и
//     без входа, от обычного пользователя, окно во весь экран на каждом
//     мониторе; тема читает `theme.conf` и `theme.conf.user` сама;
//   * закрытое окно процесс не завершает (`setQuitOnLastWindowClosed(false)`),
//     поэтому превью держит и гасит Rexon ([`run_greeter_preview`]).
// Модуль KDE 6.8 (sddm-kcm) кнопки превью не имеет — она сверх него.

/// Каталог, где SDDM держит свои программы.
pub const SDDM_BIN_DIR: &str = "/usr/bin";

/// Приветствие SDDM для тем на Qt `qt` (как `Greeter::greeterPathForQt`).
pub fn sddm_greeter_bin(qt: u32) -> String {
    if qt == 5 {
        format!("{SDDM_BIN_DIR}/sddm-greeter")
    } else {
        format!("{SDDM_BIN_DIR}/sddm-greeter-qt{qt}")
    }
}

/// Имя темы — простое имя каталога.
fn plain_theme_name(theme: &str) -> bool {
    !theme.is_empty()
        && theme != "."
        && theme != ".."
        && !theme.contains('/')
        && !theme.starts_with('-')
        && !theme.chars().any(|c| c.is_control())
}

/// Команда превью темы SDDM `theme` из [`SDDM_THEMES_DIR`]. `read` — текст
/// файла, `executable` — можно ли запустить программу. `Err` — почему
/// показать нельзя (для человека).
pub fn sddm_preview_argv(
    theme: &str,
    read: &dyn Fn(&str) -> Option<String>,
    executable: &dyn Fn(&str) -> bool,
) -> Result<Vec<String>, String> {
    let missing = || tr("This login screen theme isn't installed in the system folder, so there's nothing to preview.");
    if !plain_theme_name(theme) {
        return Err(missing());
    }
    let dir = format!("{SDDM_THEMES_DIR}/{theme}");
    let Some(meta) = read(&format!("{dir}/metadata.desktop")) else {
        return Err(missing());
    };
    let bin = sddm_greeter_bin(sddm_theme_qt_version(&meta));
    if !executable(&bin) {
        // TRANSLATORS: %1 is a program path, e.g. "/usr/bin/sddm-greeter".
        return Err(trf("Can't preview this theme: it needs %1, which isn't installed.", &[&bin]));
    }
    Ok(vec![bin, "--test-mode".into(), "--theme".into(), dir])
}

/// Можно ли запустить файл `path` (обычный файл с битом исполнения).
pub fn is_executable_live(path: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

/// [`sddm_preview_argv`] на живой машине.
pub fn sddm_preview_argv_live(theme: &str) -> Result<Vec<String>, String> {
    sddm_preview_argv(theme, &|p| std::fs::read_to_string(p).ok(), &is_executable_live)
}

/// Превью не удалось; `lines` — последние строки, что сказала программа.
pub fn login_preview_failed(lines: &[String]) -> String {
    let head = tr("Couldn't show the login screen preview.");
    // Строка длиннее 300 знаков — обрезается: человеку нужна причина, а не дамп.
    let tail: Vec<String> = lines
        .iter()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .map(|l| if l.chars().count() > 300 { format!("{}…", l.chars().take(300).collect::<String>()) } else { l.to_string() })
        .collect();
    if tail.is_empty() {
        head
    } else {
        format!("{head}\n{}", tail[tail.len().saturating_sub(5)..].join("\n"))
    }
}

/// Показать превью экрана входа `argv` и дождаться конца: сам закрылся,
/// `stop` поднят (кнопка «Закрыть предпросмотр») или прошло `max`. Rexon
/// умер — превью гаснет с ним (`PR_SET_PDEATHSIG`; поэтому звать из потока,
/// который живёт до конца показа). Вывод программы читается всё время (иначе
/// болтливое приветствие встанет на полном канале), в отказ идут последние
/// строки. Закрытие нами — не ошибка.
pub fn run_greeter_preview(argv: &[String], stop: &std::sync::atomic::AtomicBool, max: std::time::Duration) -> Result<(), String> {
    use std::io::BufRead;
    use std::os::unix::process::CommandExt;
    use std::sync::atomic::Ordering;
    let Some((bin, args)) = argv.split_first() else {
        return Err(login_preview_failed(&[]));
    };
    let mut cmd = std::process::Command::new(bin);
    cmd.args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped());
    // SAFETY: между fork и exec — только async-signal-safe prctl.
    unsafe {
        cmd.pre_exec(|| {
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
            Ok(())
        });
    }
    let mut child = cmd.spawn().map_err(|e| login_preview_failed(&[e.to_string()]))?;
    let tail = std::sync::Arc::new(std::sync::Mutex::new(std::collections::VecDeque::<String>::new()));
    let drain = child.stderr.take().map(|err| {
        let tail = tail.clone();
        std::thread::spawn(move || {
            for line in std::io::BufReader::new(err).lines().map_while(Result::ok) {
                if let Ok(mut t) = tail.lock() {
                    if t.len() >= 20 {
                        t.pop_front();
                    }
                    t.push_back(line);
                }
            }
        })
    });
    let started = std::time::Instant::now();
    let mut ours = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break Some(st),
            Ok(None) => {}
            Err(_) => break None,
        }
        if !ours && (stop.load(Ordering::SeqCst) || started.elapsed() >= max) {
            ours = true;
            // SAFETY: процесс — наш ещё не собранный потомок, номер не занят другим.
            unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGTERM) };
            let soft = std::time::Instant::now();
            while soft.elapsed() < std::time::Duration::from_secs(3) {
                if let Ok(Some(_)) = child.try_wait() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            let _ = child.kill();
            break child.wait().ok();
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    };
    if let Some(d) = drain {
        let _ = d.join();
    }
    if ours || status.is_some_and(|s| s.success()) {
        return Ok(());
    }
    let lines: Vec<String> = tail.lock().map(|t| t.iter().cloned().collect()).unwrap_or_default();
    Err(login_preview_failed(&lines))
}

/// Команда превью экрана входа по знаку кнопки: `sddm:<тема>` — тема SDDM
/// с плитки; пусто — то, что покажет экран входа сейчас (Plasma Login
/// Manager или текущая тема SDDM).
pub fn login_preview_argv_live(token: &str) -> Result<Vec<String>, String> {
    if let Some(theme) = token.strip_prefix("sddm:") {
        return sddm_preview_argv_live(theme);
    }
    if is_plasma_login(&crate::appearance_installer::active_login_manager()) {
        return crate::plasma_login::preview_argv(&is_executable_live);
    }
    match sddm_current_theme_live() {
        Some(theme) => sddm_preview_argv_live(&theme),
        None => Err(tr("SDDM uses its built-in theme here, so there's no theme to preview.")),
    }
}

/// Перенести оформление стола на экран входа SDDM (`look: Some`) или
/// «Сбросить» (`None`) через сеанс прав окна
/// ([`crate::privilege::op::PrivOp::SddmLook`]). Файл больше
/// [`SDDM_LOOK_FILE_MAX_BYTES`] не отправляется (`size` — его размер).
pub fn sddm_look_with(
    look: Option<&DesktopLook>,
    size: &dyn Fn(&str) -> Option<u64>,
    run: &mut dyn FnMut(&crate::privilege::op::PrivOp, &[String]) -> Option<crate::system_exec::RunOut>,
) -> Result<String, String> {
    use crate::privilege::op::{LocalFile, LookFile, PrivOp};
    let (op, files, done) = match look {
        Some(l) => {
            let sent: Vec<&(String, String)> = l
                .files
                .iter()
                .filter(|(_, p)| size(p).is_some_and(|s| s <= SDDM_LOOK_FILE_MAX_BYTES))
                .collect();
            let files: Vec<LookFile> = sent
                .iter()
                .enumerate()
                .map(|(i, (name, path))| LookFile { source: LocalFile { slot: i as u8, label: path.clone() }, name: name.clone() })
                .collect();
            let paths = sent.iter().map(|(_, p)| p.clone()).collect();
            (
                PrivOp::SddmLook { look: Some(l.look.clone()), files },
                paths,
                tr("Done. The login screen will use these settings the next time it appears."),
            )
        }
        None => (
            PrivOp::SddmLook { look: None, files: Vec::new() },
            Vec::new(),
            tr("Done. The login screen settings are back to default."),
        ),
    };
    greeter_outcome(run(&op, &files), done, tr("Couldn't change the login screen settings."))
}

/// [`sddm_look_with`] на живой машине.
pub fn sddm_look_live(look: Option<&DesktopLook>) -> Result<String, String> {
    sddm_look_with(
        look,
        &|p| std::fs::metadata(p).ok().filter(|m| m.is_file()).map(|m| m.len()),
        &mut |op, files| crate::privilege::run::run_live(op, files, None, &mut |_| {}),
    )
}

/// [`x11_login_default`] на живой машине — по тому, что окно может
/// прочесть (последний сеанс SDDM закрыт от пользователя; его помощник от
/// root сверит сам). Пара предлагается, только если её сеанс установлен.
pub fn x11_login_default_live() -> Option<&'static SessionSuccessor> {
    let read = |p: &str| std::fs::read_to_string(p).ok();
    let list = |d: &str| -> Vec<String> {
        std::fs::read_dir(d)
            .map(|it| it.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect())
            .unwrap_or_default()
    };
    x11_login_default(&login_session_values(&read, &list))
        .filter(|s| std::path::Path::new(&wayland_session_file(s)).is_file())
}

/// Начало ключа отказа «пары нет в системе» (+ имя X11-сеанса).
pub const WAYLAND_SESSION_MISSING: &str = "refused.wayland-session-missing.";
/// Начало ключа отказа «сеанс задан в файле пакета» (+ имя X11-сеанса).
pub const SESSION_SET_ELSEWHERE: &str = "refused.session-set-elsewhere.";

// TRANSLATORS: %1 is a session name as shown on the login screen, e.g. "Plasma (Wayland)".
const T_WAYLAND_DEFAULT_BUTTON: &str = n_("Make %1 the default session");
const T_WAYLAND_DEFAULT_CONFIRM: &str = n_(
    // TRANSLATORS: %1 and %2 are session names as shown on the login screen, e.g. "Plasma (X11)" and "Plasma (Wayland)".
    "The login screen will start %2 instead of %1 — for automatic sign-in and as the session it offers first. \
     Only this setting changes: automatic sign-in stays on or off as it is now, and you can still pick another \
     session on the login screen.",
);
const T_WAYLAND_DEFAULT_BANNER: &str = n_(
    // TRANSLATORS: %1 and %2 are session names as shown on the login screen, e.g. "Plasma (X11)" and "Plasma (Wayland)".
    "The login screen starts %1 by default. This session is no longer supported and may stop working after an \
     update. Make %2 the default instead.",
);
// TRANSLATORS: %1 is a session name as shown on the login screen, e.g. "Plasma (Wayland)".
const T_WAYLAND_DEFAULT_DONE: &str = n_("%1 is now the default session on the login screen.");
const T_WAYLAND_SESSION_MISSING: &str =
    // TRANSLATORS: %1 is a session name as shown on the login screen, e.g. "Plasma (Wayland)".
    n_("Rexon didn't change the login screen: %1 isn't installed. Nothing was changed.");
const T_SESSION_SET_ELSEWHERE: &str = n_(
    // TRANSLATORS: %1 is a session name as shown on the login screen, e.g. "Plasma (X11)".
    "Rexon didn't change the login screen: %1 is set as the default in a file of the system itself, which \
     Rexon doesn't edit. Nothing was changed.",
);
const T_WAYLAND_DEFAULT_FAILED: &str = n_("Rexon couldn't change the default session. Nothing was changed.");

/// Строки «сеанса по умолчанию» — шаблоны (для сверки переводов).
pub const WAYLAND_DEFAULT_TEXTS: &[&str] = &[
    T_WAYLAND_DEFAULT_BUTTON,
    T_WAYLAND_DEFAULT_CONFIRM,
    T_WAYLAND_DEFAULT_BANNER,
    T_WAYLAND_DEFAULT_DONE,
    T_WAYLAND_SESSION_MISSING,
    T_SESSION_SET_ELSEWHERE,
    T_WAYLAND_DEFAULT_FAILED,
];

/// Кнопка и заголовок окна подтверждения.
pub fn wayland_default_button(s: &SessionSuccessor) -> String {
    trf(T_WAYLAND_DEFAULT_BUTTON, &[&s.wayland_name])
}

/// Текст подтверждения.
pub fn wayland_default_confirm(s: &SessionSuccessor) -> String {
    trf(T_WAYLAND_DEFAULT_CONFIRM, &[&s.x11_name, &s.wayland_name])
}

/// Полоса в разделе экрана входа.
pub fn wayland_default_banner(s: &SessionSuccessor) -> String {
    trf(T_WAYLAND_DEFAULT_BANNER, &[&s.x11_name, &s.wayland_name])
}

/// Итог: пара стала сеансом по умолчанию.
pub fn wayland_default_done(s: &SessionSuccessor) -> String {
    trf(T_WAYLAND_DEFAULT_DONE, &[&s.wayland_name])
}

/// Отказ помощника: пары нет в системе.
pub fn wayland_session_missing_text(s: &SessionSuccessor) -> String {
    trf(T_WAYLAND_SESSION_MISSING, &[&s.wayland_name])
}

/// Отказ помощника: X11-сеанс задан в файле пакета.
pub fn session_set_elsewhere_text(s: &SessionSuccessor) -> String {
    trf(T_SESSION_SET_ELSEWHERE, &[&s.x11_name])
}

/// Общий провал (помощник не ответил, запись не удалась).
pub fn wayland_default_failed() -> String {
    tr(T_WAYLAND_DEFAULT_FAILED)
}

/// Полоса «сеанс по умолчанию уходит» для раздела экрана входа.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WaylandDefaultOffer {
    /// Имя X11-сеанса из таблицы — его окно вернёт в
    /// `makeWaylandDefaultSession` (помощник сверит с таблицей сам).
    pub x11: String,
    pub text: String,
    pub button: String,
    pub confirm: String,
}

/// Подписи полосы для пары `s`.
pub fn wayland_default_offer(s: &SessionSuccessor) -> WaylandDefaultOffer {
    WaylandDefaultOffer {
        x11: s.x11.to_string(),
        text: wayland_default_banner(s),
        button: wayland_default_button(s),
        confirm: wayland_default_confirm(s),
    }
}

/// Сделать пару сеансом по умолчанию через сеанс прав окна
/// ([`crate::privilege`]): что меняется ровно ключ сеанса, что пара
/// установлена и что после правки экран входа X11-сеанс сам не открывает,
/// проверяет помощник от root; окну он не верит. Сырого пути через `pkexec`
/// нет намеренно. `Ok` — итог для человека, `Err` — готовый текст отказа.
pub fn make_wayland_default_with(
    s: &SessionSuccessor,
    run: &mut dyn FnMut(&crate::privilege::op::PrivOp) -> Option<crate::system_exec::RunOut>,
) -> Result<String, String> {
    let op = crate::privilege::op::PrivOp::LoginSessionWayland { x11: s.x11.to_string() };
    let Some(out) = run(&op) else {
        return Err(crate::privilege::client::ClientError::HelperMissing.human_text());
    };
    if out.returncode == Some(0) {
        return Ok(wayland_default_done(s));
    }
    let why = out.stderr.lines().map(str::trim).rfind(|l| !l.is_empty()).map(str::to_string);
    Err(why.unwrap_or_else(wayland_default_failed))
}

/// [`make_wayland_default_with`] на живой машине.
pub fn make_wayland_default_live(s: &SessionSuccessor) -> Result<String, String> {
    make_wayland_default_with(s, &mut |op| crate::privilege::run::run_live(op, &[], None, &mut |_| {}))
}

/// Идемпотентный ответ `reset_sddm_to_default`, когда удалять нечего.
pub fn sddm_already_stock_message() -> String {
    tr("The login screen (SDDM) already uses the stock configuration")
}
/// Текст, которым подменяется пустой вывод упавшего pkexec.
pub fn sddm_reset_failed_message() -> String {
    tr("Couldn't reset SDDM")
}
/// `safe_run` не нашёл pkexec — команда не запускалась вовсе.
///
/// ⭐ Починка 15.09.2026: прежде было «pkexec не найден» — служебное слово
/// и ни слова о том, что делать. Имена пакетов во второй строке — не слова,
/// а то, что надо поставить: в Arch, Fedora и openSUSE программа запроса
/// пароля лежит в пакете polkit, в Debian и Ubuntu — в отдельном pkexec.
pub fn pkexec_missing_message() -> String {
    tr("Couldn't ask for the administrator password: a component needed for \
        that is missing on this system. Nothing was changed.\n\
        Install the polkit package (pkexec on Debian and Ubuntu) and try again.")
}

/// Отказ авторизации polkit опознаётся одинаково во всех recovery-операциях
/// (форки Plasma, Plymouth, SDDM): либо текст агента, либо код 126.
pub fn pkexec_auth_denied(returncode: i32, err: &str) -> bool {
    err.contains("Not authorized") || returncode == 126
}

/// Что показать человеку, когда pkexec вернул НЕ ноль. Общее для всех
/// recovery-операций (Plymouth, SDDM): выбрать поток, обрезать пробелы, а
/// отказ авторизации подменить подсказкой «выполни вручную».
///
/// Порт `(r.stderr or r.stdout or "<запасной текст>").strip()`: пустая строка
/// в Python ложна, поэтому пустой stderr уступает stdout, а пустые оба —
/// запасному тексту операции.
pub fn pkexec_failure_message(
    returncode: i32,
    stdout: &str,
    stderr: &str,
    fallback: &str,
    shell: &str,
) -> String {
    let raw = if !stderr.is_empty() {
        stderr
    } else if !stdout.is_empty() {
        stdout
    } else {
        fallback
    };
    let err = raw.trim().to_string();
    if pkexec_auth_denied(returncode, &err) {
        pkexec_denied_message(&format!("sudo {shell}"))
    } else {
        err
    }
}

/// Итог `reset_sddm_to_default` — `(ok, сообщение, удалённые)`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SddmResetOutcome {
    pub ok: bool,
    pub message: String,
    pub removed: Vec<String>,
}

/// Разбор результата pkexec для `reset_sddm_to_default`. Внешнего здесь нет:
/// Python отдаёт код возврата и оба потока, решение и текст — тут.
///
/// `returncode` = `None`, когда pkexec не нашёлся (`safe_run` вернул `None`).
/// `shell` — тело команды из [`SddmReset::Run`], оно же идёт в подсказку
/// «выполни вручную».
pub fn sddm_reset_outcome(
    removed: &[String],
    default_theme: Option<&str>,
    shell: &str,
    returncode: Option<i32>,
    stdout: &str,
    stderr: &str,
) -> SddmResetOutcome {
    let Some(code) = returncode else {
        return SddmResetOutcome {
            ok: false,
            message: pkexec_missing_message(),
            removed: Vec::new(),
        };
    };
    if code == 0 {
        return SddmResetOutcome {
            ok: true,
            message: sddm_reset_success_message(removed, default_theme),
            removed: removed.to_vec(),
        };
    }
    SddmResetOutcome {
        ok: false,
        message: pkexec_failure_message(
            code,
            stdout,
            stderr,
            &sddm_reset_failed_message(),
            shell,
        ),
        removed: Vec::new(),
    }
}

/// Текст отказа авторизации pkexec: одинаков для всех recovery-операций
/// (плазма-форки/plymouth/SDDM) — «не подтверждён, сделать можно вручную
/// <команда>».
///
/// ⭐ Починка 15.09.2026: прежде было «Авторизация pkexec отклонена.» —
/// служебные слова для покупателя и неверный смысл: код 126 приходит и
/// тогда, когда человек сам закрыл окно пароля, ничего ему не «отклоняли».
/// Первая строка — та же, что у отказов установки и удаления
/// (`install_plan`, `remove_plan`, `batch_main_result`). Команда для
/// терминала остаётся как есть: это не слова, а то, что надо набрать.
pub fn pkexec_denied_message(manual_cmd: &str) -> String {
    trf(
        "The administrator password wasn't confirmed.\nYou can do this by hand in a terminal:\n%1",
        &[&manual_cmd],
    )
}

// ============================================================================
// Детект экрана входа — порт `active_login_manager`/`_display_manager_present`
// ============================================================================

/// Стандартный systemd-симлинк активного DM.
pub const DM_SYMLINK: &str = "/etc/systemd/system/display-manager.service";
/// ШИРОКИЙ список известных DM-юнитов — порт `_KNOWN_DM_UNITS`. Нужен честному
/// ответу «экран входа есть/нет» (идиот-защита `ensure_display_manager`:
/// ставить SDDM ТОЛЬКО когда DM реально отсутствует).
pub const KNOWN_DM_UNITS: &[&str] = &[
    "display-manager", "plasmalogin", "sddm", "gdm", "gdm3", "lightdm",
    "lxdm", "greetd", "ly", "cosmic-greeter", "slim", "nodm", "xdm",
    "entrance", "emptty", "tbsm",
];
/// Порядок опроса в `active_login_manager` (Plasma Login Manager первым: у
/// ранних его выпусков `Provides: None`, симлинка могло не быть вовсе; на
/// живой машине 03.09.2026 юнит уже несёт `Alias=display-manager.service`).
pub const LOGIN_MANAGER_PROBE_UNITS: &[&str] =
    &["plasmalogin", "sddm", "gdm", "lightdm", "cosmic-greeter"];
/// Состояния `systemctl is-enabled`, означающие «стартует при загрузке».
pub const DM_ENABLED_STATES: &[&str] =
    &["enabled", "enabled-runtime", "alias", "indirect", "static"];
/// Дефолт `active_login_manager`, когда определить нечем.
pub const DEFAULT_LOGIN_MANAGER: &str = "sddm";
/// Юнит Plasma Login Manager (Plasma 6.6+) — экрана входа БЕЗ тем: вид у
/// него один, собранный в сам бинарь, и QML-темы SDDM он не грузит.
pub const PLASMA_LOGIN_UNIT: &str = "plasmalogin";

/// Рисует ли экран входа Plasma Login Manager — по ответу
/// [`active_login_manager_from`]. Одно решение на продукт: его спрашивают и
/// раздел «Оформления» (какую полку показать), и отметка активной темы
/// SDDM, и последний отказ перед установкой SDDM-темы.
pub fn is_plasma_login(active_unit: &str) -> bool {
    active_unit.trim() == PLASMA_LOGIN_UNIT
}

/// Юнит из цели симлинка display-manager.service: basename без `.service`.
/// None — цели нет либо имя пустое. Живость цели здесь не проверяется —
/// это забота `dm_symlink_unit`.
pub fn dm_unit_from_symlink(realpath_target: Option<&str>) -> Option<String> {
    let target = realpath_target?;
    let base = target.rsplit('/').next().unwrap_or(target);
    let base = base.strip_suffix(".service").unwrap_or(base);
    if base.is_empty() {
        None
    } else {
        Some(base.to_string())
    }
}

/// Означает ли вывод `systemctl is-enabled` «стартует при загрузке».
pub fn dm_state_is_enabled(state: &str) -> bool {
    DM_ENABLED_STATES.contains(&state.trim())
}

/// Юнит, на который ЖИВЫМ симлинком указывает display-manager.service.
///
/// `symlink_target` — realpath симлинка (None = симлинка нет),
/// `target_exists` — существует ли файл юнита по этому пути.
///
/// ⚠️ Починка при порте. Висячий симлинк — обычное состояние после
/// удаления пакета экрана входа: `systemctl enable sddm` кладёт симлинк в
/// /etc, а `pacman -R sddm` (в том числе наш `remove_de` для KDE и LXQt,
/// где sddm стоит в списке пакетов) его не трогает. Python считал такой
/// симлинк живым DM (`islink or exists`): `_display_manager_present` отвечал
/// «есть», идиот-защита `ensure_display_manager` пропускала установку, и
/// после перезагрузки человек оставался без входа в графику — ровно та беда,
/// от которой защита заведена. Висячий симлинк теперь — «симлинка нет»:
/// решение уходит к опросу `systemctl is-enabled`.
pub fn dm_symlink_unit(symlink_target: Option<&str>, target_exists: bool) -> Option<String> {
    if !target_exists {
        return None;
    }
    dm_unit_from_symlink(symlink_target)
}

/// Порт `active_login_manager`. `is_enabled(unit)` — stdout
/// `systemctl is-enabled <unit>.service` либо None (команда не дала ответа);
/// вызывается ЛЕНИВО и только пока не найден первый включённый юнит.
pub fn active_login_manager_from(
    symlink_target: Option<&str>,
    target_exists: bool,
    has_systemctl: bool,
    is_enabled: &dyn Fn(&str) -> Option<String>,
) -> String {
    login_manager_by(
        symlink_target,
        target_exists,
        has_systemctl,
        LOGIN_MANAGER_PROBE_UNITS.iter().copied(),
        &dm_state_is_enabled,
        is_enabled,
    )
    .unwrap_or_else(|| DEFAULT_LOGIN_MANAGER.to_string())
}

/// Состояния `systemctl is-enabled`, при которых юнит экрана входа ДЕЙСТВИТЕЛЬНО
/// стартует при загрузке как display-manager.service. Уже, чем
/// [`DM_ENABLED_STATES`]: `static` и `indirect` у юнита DM не значат, что он
/// встанет на место display-manager.service, и для подписи «основной» не
/// годятся (для честного «экран входа есть» широкий список остаётся).
pub const DM_ACTIVE_STATES: &[&str] = &["enabled", "enabled-runtime", "alias"];

/// Означает ли вывод `systemctl is-enabled`, что юнит — основной экран входа.
pub fn dm_state_is_active(state: &str) -> bool {
    DM_ACTIVE_STATES.contains(&state.trim())
}

/// Порядок опроса для [`active_login_manager_opt`]: сначала узкий список
/// прежнего детекта (тот же приоритет), затем остальные известные юниты —
/// иначе включённый `gdm3` на Debian без симлинка остался бы «не найден».
pub fn active_probe_units() -> Vec<&'static str> {
    let mut out: Vec<&'static str> = LOGIN_MANAGER_PROBE_UNITS.to_vec();
    for unit in KNOWN_DM_UNITS {
        if *unit != "display-manager" && !out.contains(unit) {
            out.push(unit);
        }
    }
    out
}

/// Общее ядро детекта активного экрана входа: живой симлинк, затем ленивый
/// опрос `units` до первого юнита, чьё состояние принимает `accept`.
fn login_manager_by<'a>(
    symlink_target: Option<&str>,
    target_exists: bool,
    has_systemctl: bool,
    units: impl IntoIterator<Item = &'a str>,
    accept: &dyn Fn(&str) -> bool,
    is_enabled: &dyn Fn(&str) -> Option<String>,
) -> Option<String> {
    if let Some(unit) = dm_symlink_unit(symlink_target, target_exists) {
        return Some(unit);
    }
    if has_systemctl {
        for unit in units {
            if let Some(state) = is_enabled(unit) {
                if accept(&state) {
                    return Some(unit.to_string());
                }
            }
        }
    }
    None
}

/// Активный экран входа БЕЗ подстановки: `None` — определить нечем (нет
/// живого симлинка и ни один известный юнит не включён, или нет systemctl).
///
/// Для подписи «сейчас основной» нужен именно этот ответ: прежний
/// [`active_login_manager_from`] в таком случае отвечает `sddm`, и человек
/// увидел бы чужой экран входа как свой. Прежний остаётся для старых
/// потребителей (выбор полки тем и т. п.) и ведёт себя как раньше.
///
/// Отличия от прежнего: включённым считается только [`DM_ACTIVE_STATES`] и
/// опрашивается широкий список [`active_probe_units`].
pub fn active_login_manager_opt(
    symlink_target: Option<&str>,
    target_exists: bool,
    has_systemctl: bool,
    is_enabled: &dyn Fn(&str) -> Option<String>,
) -> Option<String> {
    login_manager_by(
        symlink_target,
        target_exists,
        has_systemctl,
        active_probe_units(),
        &dm_state_is_active,
        is_enabled,
    )
}

/// Приветствие (greeter) экрана входа, между которыми выбирает человек —
/// у LightDM это файл `/usr/share/xgreeters/<id>.desktop`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DmGreeter {
    /// Имя файла без `.desktop` — то, что пишется в `greeter-session=`.
    pub id: String,
    /// `Name=` из файла (пусто — не задано).
    pub name: String,
    /// Первое слово `Exec=` — программа приветствия.
    pub exec: String,
}

/// Готов ли экран входа стать основным.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "detail", rename_all = "snake_case")]
pub enum DmReadiness {
    /// Всё нужное на месте.
    Ready,
    /// Работать будет, но сначала человек выбирает приветствие из
    /// установленных.
    NeedsGreeterChoice(Vec<DmGreeter>),
    /// Нельзя: причина для человека.
    Blocked(String),
    /// Rexon этот экран входа (или эту систему) переключать не умеет.
    Unsupported,
}

/// Один установленный экран входа — строка будущего списка в разделе
/// «Экран входа в систему».
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LoginManagerInfo {
    /// Юнит без `.service` (`plasmalogin`, `sddm`…).
    pub unit: String,
    /// Человеческое имя; пусто — незнакомый экран входа.
    pub label: String,
    /// Программа из `ExecStart` юнита (как написано в юните).
    pub binary: Option<String>,
    /// Юнит найден и его программа на месте.
    pub present: bool,
    /// Основной: на него указывает display-manager.service.
    pub active: bool,
    pub readiness: DmReadiness,
    /// Предупреждения, не мешающие переключению (для человека).
    pub warnings: Vec<String>,
}

/// Порт `_display_manager_present` — ЧЕСТНЫЙ булев ответ «экран входа есть».
///
/// Без systemd (`has_systemctl == false`) — true: авто-enable там невозможен, а
/// экраном входа управляет сам init, вмешиваться нельзя (иначе наплодим второй
/// DM).
///
/// Симлинк display-manager.service считается ТОЛЬКО с живой целью (см.
/// `dm_symlink_unit`): висячий после `pacman -R sddm` означает «экрана входа
/// нет», и ответ берётся из опроса юнитов.
pub fn display_manager_present_from(
    symlink_target: Option<&str>,
    target_exists: bool,
    has_systemctl: bool,
    is_enabled: &dyn Fn(&str) -> Option<String>,
) -> bool {
    if symlink_target.is_some() && target_exists {
        return true;
    }
    if !has_systemctl {
        return true;
    }
    for unit in KNOWN_DM_UNITS {
        if let Some(state) = is_enabled(unit) {
            if dm_state_is_enabled(&state) {
                return true;
            }
        }
    }
    false
}

// ── Установленные экраны входа: юниты с Alias=display-manager.service ────

/// Каталоги юнитов systemd в порядке приоритета: файл из `/etc` перекрывает
/// одноимённый пакетный. `/lib/systemd/system` — старые Debian без слияния
/// `/usr` (там, где слияние есть, это ссылка и одноимённые файлы отсеются).
pub const DM_UNIT_DIRS: &[&str] = &[
    "/etc/systemd/system",
    "/usr/local/lib/systemd/system",
    "/usr/lib/systemd/system",
    "/lib/systemd/system",
];

/// Алиас, по которому systemd запускает экран входа.
pub const DM_ALIAS: &str = "display-manager.service";

/// Имя юнита без `.service` по пути файла; `None` — не `.service`.
pub fn unit_name_from_path(path: &str) -> Option<&str> {
    let base = path.rsplit('/').next().unwrap_or(path);
    base.strip_suffix(".service").filter(|n| !n.is_empty())
}

/// Может ли юнит с таким именем быть экраном входа в списке: сам алиас,
/// шаблоны `@` и устаревший `display-manager-legacy` (openSUSE) — нет.
pub fn dm_unit_name_listable(name: &str) -> bool {
    !(name == "display-manager"
        || name.starts_with("display-manager-")
        || name.ends_with("-legacy")
        || name.contains('@'))
}

/// Значения ключа секции юнит-файла с правилами systemd: ключ может стоять
/// несколько раз, значение — список через пробел, пустое присваивание
/// сбрасывает накопленное.
pub fn unit_values(text: &str, section: &str, key: &str) -> Vec<String> {
    let mut here = false;
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        let s = line.trim();
        if s.is_empty() || s.starts_with('#') || s.starts_with(';') {
            continue;
        }
        if s.starts_with('[') && s.ends_with(']') {
            here = &s[1..s.len() - 1] == section;
            continue;
        }
        if !here {
            continue;
        }
        let Some((k, v)) = s.split_once('=') else { continue };
        if k.trim() != key {
            continue;
        }
        let v = v.trim();
        if v.is_empty() {
            out.clear();
        } else {
            out.extend(v.split_whitespace().map(String::from));
        }
    }
    out
}

/// `ExecStart` юнита словами, без префиксов systemd (`-`, `@`, `:`, `+`,
/// `!`) и кавычек. `None` — строки нет. Последняя непустая побеждает.
pub fn unit_exec_start(text: &str) -> Option<Vec<String>> {
    let mut here = false;
    let mut found: Option<String> = None;
    for line in text.lines() {
        let s = line.trim();
        if s.starts_with('[') && s.ends_with(']') {
            here = &s[1..s.len() - 1] == "Service";
            continue;
        }
        if !here {
            continue;
        }
        if let Some((k, v)) = s.split_once('=') {
            if k.trim() == "ExecStart" {
                let v = v.trim();
                found = (!v.is_empty()).then(|| v.to_string());
            }
        }
    }
    let line = found?;
    let line = line.trim_start_matches(['-', '@', ':', '+', '!']);
    let words: Vec<String> = line
        .split_whitespace()
        .map(|w| w.trim_matches(['"', '\'']).to_string())
        .filter(|w| !w.is_empty())
        .collect();
    (!words.is_empty()).then_some(words)
}

/// Установленные экраны входа по юнит-файлам: те, у кого в `[Install]`
/// стоит `Alias=display-manager.service`. Это ловит и экраны входа, которых
/// нет в наших таблицах.
///
/// `files` — `(путь, текст)` в порядке приоритета каталогов
/// ([`DM_UNIT_DIRS`]): из одноимённых решает ПЕРВЫЙ, как у systemd (свой
/// файл в `/etc` перекрывает пакетный). Отсеваются алиас, шаблоны и
/// `-legacy`. Порядок ответа — порядок [`KNOWN_DM_UNITS`], незнакомые —
/// за ними по алфавиту.
pub fn dm_units_from_unit_files(files: &[(String, String)]) -> Vec<String> {
    let mut seen: Vec<&str> = Vec::new();
    let mut out: Vec<String> = Vec::new();
    for (path, text) in files {
        let Some(name) = unit_name_from_path(path) else { continue };
        if seen.contains(&name) {
            continue;
        }
        seen.push(name);
        if !dm_unit_name_listable(name) {
            continue;
        }
        if unit_values(text, "Install", "Alias").iter().any(|a| a == DM_ALIAS) {
            out.push(name.to_string());
        }
    }
    let rank = |u: &str| KNOWN_DM_UNITS.iter().position(|k| *k == u).unwrap_or(usize::MAX);
    out.sort_by(|a, b| rank(a).cmp(&rank(b)).then_with(|| a.cmp(b)));
    out
}

/// Что нужно экрану входа, чтобы на нём был рабочий вход: одна строка на
/// юнит. Таблица общая для подписи, проверок готовности и (дальше) помощника.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DmSpec {
    /// Юнит без `.service`.
    pub unit: &'static str,
    /// Человеческое имя (латиницей — это имя программы).
    pub label: &'static str,
    /// Имя программы экрана входа (каталог у дистрибутивов разный — путь
    /// берётся из `ExecStart` юнита).
    pub binary: &'static str,
    /// Пакет (имя на Arch; у Debian свои имена — `gdm3` отдельной строкой).
    pub package: &'static str,
    /// Системный пользователь, от которого работает приветствие; `None` —
    /// своего пользователя нет или он задаётся конфигом (greetd).
    pub user: Option<&'static str>,
    /// Файл службы PAM (`/etc/pam.d` или `/usr/lib/pam.d`).
    pub pam: &'static str,
    /// Приветствие всегда на X — без Xorg не поднимется. У SDDM зависит от
    /// `DisplayServer=` — решает проверка готовности.
    pub needs_xorg: bool,
    /// Rexon умеет проверить готовность и предложить «Сделать основным».
    /// Остальные только показываются в списке.
    pub switchable: bool,
}

/// Таблица экранов входа. Порядок — порядок [`KNOWN_DM_UNITS`] (без
/// алиаса и tbsm, у которого нет своего входа в графику).
pub const LOGIN_MANAGER_SPECS: &[DmSpec] = &[
    DmSpec {
        unit: PLASMA_LOGIN_UNIT, label: "Plasma Login Manager", binary: "plasmalogin",
        package: "plasma-login-manager", user: Some("plasmalogin"), pam: "plasmalogin",
        needs_xorg: false, switchable: true,
    },
    DmSpec {
        unit: "sddm", label: "SDDM", binary: "sddm", package: "sddm",
        user: Some("sddm"), pam: "sddm", needs_xorg: true, switchable: true,
    },
    DmSpec {
        unit: "gdm", label: "GDM", binary: "gdm", package: "gdm",
        user: Some("gdm"), pam: "gdm-password", needs_xorg: false, switchable: true,
    },
    DmSpec {
        unit: "gdm3", label: "GDM", binary: "gdm3", package: "gdm3",
        user: Some("Debian-gdm"), pam: "gdm-password", needs_xorg: false, switchable: true,
    },
    DmSpec {
        unit: "lightdm", label: "LightDM", binary: "lightdm", package: "lightdm",
        user: Some("lightdm"), pam: "lightdm", needs_xorg: true, switchable: true,
    },
    DmSpec {
        unit: "lxdm", label: "LXDM", binary: "lxdm", package: "lxdm",
        user: None, pam: "lxdm", needs_xorg: true, switchable: false,
    },
    DmSpec {
        unit: "greetd", label: "greetd", binary: "greetd", package: "greetd",
        user: None, pam: "greetd", needs_xorg: false, switchable: true,
    },
    DmSpec {
        unit: "ly", label: "Ly", binary: "ly", package: "ly",
        user: None, pam: "ly", needs_xorg: false, switchable: false,
    },
    DmSpec {
        unit: "cosmic-greeter", label: "COSMIC Greeter", binary: "cosmic-greeter",
        package: "cosmic-greeter", user: Some("cosmic-greeter"), pam: "greetd",
        needs_xorg: false, switchable: true,
    },
    DmSpec {
        unit: "slim", label: "SLiM", binary: "slim", package: "slim",
        user: None, pam: "slim", needs_xorg: true, switchable: false,
    },
    DmSpec {
        unit: "nodm", label: "nodm", binary: "nodm", package: "nodm",
        user: None, pam: "nodm", needs_xorg: true, switchable: false,
    },
    DmSpec {
        unit: "xdm", label: "XDM", binary: "xdm", package: "xorg-xdm",
        user: None, pam: "xdm", needs_xorg: true, switchable: false,
    },
    DmSpec {
        unit: "entrance", label: "Entrance", binary: "entrance", package: "entrance",
        user: None, pam: "entrance", needs_xorg: true, switchable: false,
    },
    DmSpec {
        unit: "emptty", label: "emptty", binary: "emptty", package: "emptty",
        user: None, pam: "emptty", needs_xorg: false, switchable: false,
    },
];

/// Строка таблицы по юниту; `None` — незнакомый экран входа.
pub fn login_manager_spec(unit: &str) -> Option<&'static DmSpec> {
    let unit = unit.trim();
    LOGIN_MANAGER_SPECS.iter().find(|s| s.unit == unit)
}

// ── Как на этой системе выбирается экран входа ─────────────────────────────

/// Механизм выбора экрана входа на системе — от него зависит, может ли
/// Rexon предложить «Сделать основным» и какие шаги понадобятся.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "init", rename_all = "snake_case")]
pub enum DmSwitchBackend {
    /// Только ссылка display-manager.service (Arch, Fedora и т. п.).
    Systemd,
    /// systemd + `/etc/X11/default-display-manager` и debconf (Debian,
    /// Ubuntu, Mint): файл и ссылка обязаны совпадать.
    SystemdDebian,
    /// openSUSE: `display-manager.service` — обычный файл, выбор через
    /// `update-alternatives default-displaymanager`.
    SystemdSuseAlternatives,
    /// NixOS: экран входа задаётся в configuration.nix.
    NixosDeclarative,
    /// Без systemd: экраном входа управляет своя init (имя для человека;
    /// пусто — не опознали).
    Unsupported(String),
}

impl DmSwitchBackend {
    /// Даёт ли Rexon кнопку «Сделать основным». Первая версия (решение
    /// Wehrwolfmann 20.10.2026): только systemd-системы Arch-семейства,
    /// Fedora и Debian/Ubuntu; openSUSE — после проверки на стенде.
    pub fn switch_supported(&self) -> bool {
        matches!(self, DmSwitchBackend::Systemd | DmSwitchBackend::SystemdDebian)
    }
}

/// Пробы системы для [`dm_switch_backend`] — собирает обвязка, только чтение.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DmBackendProbe {
    /// systemd — PID 1: есть `/run/systemd/system` (так проверяет и сам
    /// systemd). Одного `systemctl` в PATH мало: в Artix он бывает без systemd.
    pub systemd_running: bool,
    /// Есть `systemctl`.
    pub has_systemctl: bool,
    /// Есть `/etc/X11/default-display-manager`.
    pub x11_default_dm_file: bool,
    /// `display-manager.service` в `/etc/systemd/system` или
    /// `/usr/lib/systemd/system` — обычный файл, а не ссылка (openSUSE).
    pub dm_service_is_regular_file: bool,
    /// Есть альтернатива `default-displaymanager` (`/etc/alternatives`).
    pub suse_dm_alternative: bool,
    /// Опознанная init без systemd (`OpenRC`, `runit`, `s6`, `dinit`,
    /// `SysVinit`); пусто — не опознали.
    pub other_init: String,
}

/// Механизм выбора экрана входа по семейству дистрибутива и пробам.
pub fn dm_switch_backend(family: PkgFamily, probe: &DmBackendProbe) -> DmSwitchBackend {
    if family == PkgFamily::Nix {
        return DmSwitchBackend::NixosDeclarative;
    }
    if !(probe.systemd_running && probe.has_systemctl) {
        return DmSwitchBackend::Unsupported(probe.other_init.clone());
    }
    match family {
        PkgFamily::Suse if probe.dm_service_is_regular_file && probe.suse_dm_alternative => {
            DmSwitchBackend::SystemdSuseAlternatives
        }
        PkgFamily::Debian if probe.x11_default_dm_file => DmSwitchBackend::SystemdDebian,
        _ => DmSwitchBackend::Systemd,
    }
}

/// Имя init без systemd по найденным признакам (порядок — от точных к
/// общим). `has(path)` — есть ли путь, `which(name)` — есть ли программа.
pub fn other_init_name(has: &dyn Fn(&str) -> bool, which: &dyn Fn(&str) -> bool) -> String {
    let name = if has("/run/openrc") || which("openrc") || which("rc-update") {
        "OpenRC"
    } else if has("/run/runit") || has("/etc/runit") || which("runit") {
        "runit"
    } else if has("/run/s6-rc") || which("s6-rc") {
        "s6"
    } else if which("dinitctl") {
        "dinit"
    } else if has("/etc/inittab") {
        "SysVinit"
    } else {
        ""
    };
    name.to_string()
}

/// Команда для ручного переключения на системе без systemd — её показывают
/// вместо кнопки. `None` — для этой init готовой команды нет. Это не слова
/// для перевода, а то, что набирают в терминале.
pub fn dm_manual_switch_command(init: &str, family: PkgFamily, unit: &str) -> Option<String> {
    let dm = unit.trim();
    if dm.is_empty() || !dm.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c)) {
        return None;
    }
    Some(match (init, family) {
        ("OpenRC", _) => format!(
            "sudo sed -i 's/^DISPLAYMANAGER=.*/DISPLAYMANAGER=\"{dm}\"/' /etc/conf.d/display-manager && sudo rc-update add display-manager default"
        ),
        ("runit", PkgFamily::Void) => format!("sudo ln -s /etc/sv/{dm} /var/service/"),
        ("runit", _) => format!("sudo ln -s /etc/runit/sv/{dm} /run/runit/service/"),
        ("dinit", _) => format!("sudo dinitctl enable {dm}"),
        ("SysVinit", PkgFamily::Debian) => format!("sudo dpkg-reconfigure {dm}"),
        _ => return None,
    })
}

/// Стоит ли читать юнит-файл ради списка экранов входа: имя знакомое либо
/// текст упоминает алиас. Решение для обвязки, чтобы не держать в памяти
/// сотни чужих юнитов.
pub fn dm_unit_file_candidate(name: &str, text: &str) -> bool {
    dm_unit_name_listable(name) && (KNOWN_DM_UNITS.contains(&name) || text.contains(DM_ALIAS))
}

// ── Готовность: LightDM и его приветствия ──────────────────────────────────

/// Каталог приветствий LightDM: `<id>.desktop`, `greeter-session=<id>`.
pub const XGREETERS_DIR: &str = "/usr/share/xgreeters";
/// Вшитое умолчание LightDM, собранного без правок дистрибутива.
pub const LIGHTDM_UPSTREAM_DEFAULT_GREETER: &str = "example-gtk-gnome";
/// Приветствие, которое Rexon предлагает первым, когда выбирать приходится
/// человеку: Slick — то, что по умолчанию стоит в Linux Mint.
pub const LIGHTDM_SUGGESTED_GREETER: &str = "lightdm-slick-greeter";
/// Его программа — на случай, если файл назван иначе.
const LIGHTDM_SUGGESTED_GREETER_EXEC: &str = "slick-greeter";
/// Имя-альтернатива Debian (`update-alternatives lightdm-greeter`): ссылка
/// на одно из настоящих приветствий, отдельным пунктом не показывается.
const LIGHTDM_DEBIAN_ALT_GREETER: &str = "lightdm-greeter";

/// `greeter-session` из вывода `lightdm --show-config` (секция `[Seat:*]`,
/// старое имя `[SeatDefaults]`). Строки там вида `A  ключ=значение`: буква —
/// метка файла-источника. `None` — не задано.
pub fn lightdm_greeter_from_show_config(text: &str) -> Option<String> {
    let mut here = false;
    let mut found = None;
    for line in text.lines() {
        let s = line.trim();
        if s.starts_with('[') && s.ends_with(']') {
            here = matches!(&s[1..s.len() - 1], "Seat:*" | "SeatDefaults");
            continue;
        }
        if !here {
            continue;
        }
        let Some((k, v)) = s.split_once('=') else { continue };
        if k.split_whitespace().last() == Some("greeter-session") {
            let v = v.trim();
            found = (!v.is_empty()).then(|| v.to_string());
        }
    }
    found
}

/// Приветствие, которое LightDM берёт, когда `greeter-session` не задан:
/// вшито при сборке и у дистрибутивов разное. `None` — не подтверждено
/// (тогда выбирает человек).
pub fn lightdm_default_greeter(family: PkgFamily) -> Option<&'static str> {
    match family {
        PkgFamily::Arch => Some("lightdm-gtk-greeter"),
        PkgFamily::Debian => Some(LIGHTDM_DEBIAN_ALT_GREETER),
        PkgFamily::Gentoo | PkgFamily::Void | PkgFamily::Alpine => {
            Some(LIGHTDM_UPSTREAM_DEFAULT_GREETER)
        }
        _ => None,
    }
}

/// Установленные приветствия LightDM по файлам `XGREETERS_DIR/*.desktop`:
/// `(путь, текст)`. Берутся только те, чья программа из `Exec=` есть
/// (`exec_exists`); `Hidden=true` отсеивается. Альтернатива Debian
/// `lightdm-greeter` с той же программой, что у настоящего, второй раз не
/// показывается. Порядок — по `id`.
pub fn xgreeters_from(
    files: &[(String, String)],
    exec_exists: &dyn Fn(&str) -> bool,
) -> Vec<DmGreeter> {
    let value = |text: &str, key: &str| {
        crate::de_apply::ini_value(text, "Desktop Entry", key).unwrap_or_default()
    };
    let mut all: Vec<DmGreeter> = Vec::new();
    for (path, text) in files {
        let base = path.rsplit('/').next().unwrap_or(path);
        let Some(id) = base.strip_suffix(".desktop").filter(|s| !s.is_empty()) else {
            continue;
        };
        if value(text, "Hidden").eq_ignore_ascii_case("true") {
            continue;
        }
        let exec = value(text, "Exec")
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_string();
        if exec.is_empty() || !exec_exists(&exec) {
            continue;
        }
        all.push(DmGreeter { id: id.to_string(), name: value(text, "Name"), exec });
    }
    // Альтернатива — в конец, чтобы при совпадении программы остался
    // настоящий файл.
    all.sort_by(|a, b| {
        (a.id == LIGHTDM_DEBIAN_ALT_GREETER)
            .cmp(&(b.id == LIGHTDM_DEBIAN_ALT_GREETER))
            .then_with(|| a.id.cmp(&b.id))
    });
    let mut out: Vec<DmGreeter> = Vec::new();
    for g in all {
        if g.id == LIGHTDM_DEBIAN_ALT_GREETER && out.iter().any(|o| o.exec == g.exec) {
            continue;
        }
        out.push(g);
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

/// Предвыбор в списке приветствий: Slick, если установлен, иначе первое.
pub fn suggested_greeter(greeters: &[DmGreeter]) -> Option<&DmGreeter> {
    greeters
        .iter()
        .find(|g| g.id == LIGHTDM_SUGGESTED_GREETER || g.exec == LIGHTDM_SUGGESTED_GREETER_EXEC)
        .or_else(|| greeters.first())
}

/// Готовность LightDM по приветствию.
///
/// * задано (`greeter-session`) и установлено — `Ready`;
/// * не задано, умолчание дистрибутива установлено и оно единственное —
///   `Ready`;
/// * задано, но не установлено; не задано и умолчание не подтверждено; или
///   приветствий несколько — `NeedsGreeterChoice` (предвыбор —
///   [`suggested_greeter`]): иначе LightDM упадёт на несуществующем
///   приветствии или покажет не то, что человек ждёт;
/// * приветствий нет вовсе — `Blocked`.
pub fn lightdm_greeter_readiness(
    greeter_session: Option<&str>,
    default_greeter: Option<&str>,
    greeters: &[DmGreeter],
) -> DmReadiness {
    if greeters.is_empty() {
        return DmReadiness::Blocked(tr(
            "LightDM has no greeter installed, so it can't show a login screen.",
        ));
    }
    let installed = |id: &str| greeters.iter().any(|g| g.id == id);
    match greeter_session.map(str::trim).filter(|s| !s.is_empty()) {
        Some(set) if installed(set) => DmReadiness::Ready,
        Some(_) => DmReadiness::NeedsGreeterChoice(greeters.to_vec()),
        None => match default_greeter {
            Some(d) if installed(d) && greeters.len() == 1 => DmReadiness::Ready,
            _ => DmReadiness::NeedsGreeterChoice(greeters.to_vec()),
        },
    }
}

// ── Готовность: greetd ─────────────────────────────────────────────────────

/// Конфиг greetd, если юнит не передал свой (`--config`).
pub const GREETD_DEFAULT_CONFIG: &str = "/etc/greetd/config.toml";
/// Приветствия greetd, которые дают только текстовый вход (вход в
/// оболочку, а не в рабочий стол): встроенный `agreety` и голые оболочки.
pub const GREETD_TEXT_GREETERS: &[&str] =
    &["agreety", "login", "sh", "bash", "dash", "zsh", "fish"];

/// Что запускает greetd как приветствие (`[default_session] command`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "program", rename_all = "snake_case")]
pub enum GreetdCommand {
    /// Графическое или TUI-приветствие, запускающее рабочий стол
    /// (`tuigreet`, `regreet`, `gtkgreet`, `nwg-hello`, обёртки вроде
    /// `cage`): программа — первое слово команды.
    Graphical(String),
    /// Текстовый вход прямо в оболочку (`agreety`).
    Text(String),
    /// Команды нет.
    Missing,
}

/// Путь конфига greetd по словам `ExecStart` (`--config X`, `-c X`,
/// `--config=X`); иначе [`GREETD_DEFAULT_CONFIG`].
pub fn greetd_config_path(exec_start: &[String]) -> String {
    let mut it = exec_start.iter().skip(1);
    while let Some(w) = it.next() {
        if let Some(v) = w.strip_prefix("--config=") {
            return v.to_string();
        }
        if w == "--config" || w == "-c" {
            if let Some(v) = it.next() {
                return v.clone();
            }
        }
    }
    GREETD_DEFAULT_CONFIG.to_string()
}

/// Строковое значение ключа в секции TOML-файла greetd (простая форма
/// `ключ = "значение"`, достаточная для его конфигов).
pub fn toml_string_value(text: &str, section: &str, key: &str) -> Option<String> {
    let mut here = false;
    let mut found = None;
    for line in text.lines() {
        let s = line.trim();
        if s.is_empty() || s.starts_with('#') {
            continue;
        }
        if s.starts_with('[') && s.ends_with(']') {
            here = s[1..s.len() - 1].trim() == section;
            continue;
        }
        if !here {
            continue;
        }
        let Some((k, v)) = s.split_once('=') else { continue };
        if k.trim() != key {
            continue;
        }
        let v = v.trim();
        let unq = if let Some(rest) = v.strip_prefix('"') {
            rest.split_once('"').map_or(rest, |(a, _)| a)
        } else if let Some(rest) = v.strip_prefix('\'') {
            rest.split_once('\'').map_or(rest, |(a, _)| a)
        } else {
            v.split('#').next().unwrap_or(v).trim()
        };
        found = Some(unq.to_string());
    }
    found.filter(|s| !s.trim().is_empty())
}

/// Разбор `[default_session] command` конфига greetd. Присваивания
/// `ИМЯ=значение` и обёртка `env` перед программой пропускаются.
pub fn greetd_command_kind(config_toml: &str) -> GreetdCommand {
    let Some(cmd) = toml_string_value(config_toml, "default_session", "command") else {
        return GreetdCommand::Missing;
    };
    let program = cmd
        .split_whitespace()
        .find(|w| *w != "env" && !(w.contains('=') && !w.starts_with('/')))
        .unwrap_or_default()
        .to_string();
    if program.is_empty() {
        return GreetdCommand::Missing;
    }
    let base = program.rsplit('/').next().unwrap_or(&program);
    if GREETD_TEXT_GREETERS.contains(&base) {
        GreetdCommand::Text(program)
    } else {
        GreetdCommand::Graphical(program)
    }
}

/// Пользователь приветствия greetd (`[default_session] user`).
pub fn greetd_session_user(config_toml: &str) -> Option<String> {
    toml_string_value(config_toml, "default_session", "user")
}

// ── Готовность: SDDM ───────────────────────────────────────────────────────

/// Каталог тем SDDM по умолчанию (`[Theme] ThemeDir`).
pub const SDDM_THEMES_DIR: &str = "/usr/share/sddm/themes";
/// Приветствие SDDM для тем Qt 6 и для тем без `QtVersion=6` (Qt 5).
pub const SDDM_GREETER_QT6: &str = "sddm-greeter-qt6";
pub const SDDM_GREETER_QT5: &str = "sddm-greeter";

/// Значение ключа по файлам конфигурации в порядке чтения: последний
/// непустой выигрывает (так сливает свои файлы SDDM).
pub fn ini_last_value(texts: &[&str], group: &str, key: &str) -> Option<String> {
    texts
        .iter()
        .filter_map(|t| crate::de_apply::ini_value(t, group, key))
        .last()
}

/// Нужен ли SDDM Xorg: `[General] DisplayServer` (по умолчанию `x11`).
pub fn sddm_uses_x11(confs: &[&str]) -> bool {
    !matches!(
        ini_last_value(confs, "General", "DisplayServer").as_deref(),
        Some("wayland")
    )
}

/// Программа-компоситор для приветствия SDDM на Wayland
/// (`[Wayland] CompositorCommand`, по умолчанию `weston`).
pub fn sddm_wayland_compositor(confs: &[&str]) -> String {
    ini_last_value(confs, "Wayland", "CompositorCommand")
        .and_then(|c| c.split_whitespace().next().map(String::from))
        .unwrap_or_else(|| "weston".to_string())
}

/// Состояние темы SDDM.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SddmThemeState {
    /// `Current=` пуст — встроенная запасная тема, это рабочий вариант.
    Builtin,
    /// Тема на месте, приветствие под её Qt есть.
    Ready { theme: String },
    /// Каталога темы или `metadata.desktop` нет: SDDM покажет запасную —
    /// не смертельно, но человека стоит предупредить.
    Missing { theme: String },
    /// Тема есть, а приветствия под её версию Qt нет.
    NeedsGreeter { theme: String, greeter: String },
}

/// Готова ли тема SDDM. `metadata_text` — `metadata.desktop` темы (`None` —
/// нет каталога или файла); `has_greeter(имя)` — есть ли программа
/// приветствия.
pub fn sddm_theme_ready(
    current: Option<&str>,
    metadata_text: Option<&str>,
    has_greeter: &dyn Fn(&str) -> bool,
) -> SddmThemeState {
    let Some(theme) = current.map(str::trim).filter(|t| !t.is_empty()) else {
        return SddmThemeState::Builtin;
    };
    let theme = theme.to_string();
    let Some(meta) = metadata_text else {
        return SddmThemeState::Missing { theme };
    };
    let qt6 = crate::de_apply::ini_value(meta, "SddmGreeterTheme", "QtVersion").as_deref()
        == Some("6");
    let greeter = if qt6 { SDDM_GREETER_QT6 } else { SDDM_GREETER_QT5 };
    if has_greeter(greeter) {
        SddmThemeState::Ready { theme }
    } else {
        SddmThemeState::NeedsGreeter { theme, greeter: greeter.to_string() }
    }
}

// ── Готовность: общие проверки ─────────────────────────────────────────────

/// Объявляет ли файл `sysusers.d` пользователя (`u имя …`): тогда
/// `systemd-sysusers` создаст его при загрузке, и его отсутствие сейчас —
/// не блокировка.
pub fn sysusers_declares(text: &str, user: &str) -> bool {
    text.lines().any(|l| {
        let mut w = l.split_whitespace();
        matches!(w.next(), Some("u" | "u!")) && w.next() == Some(user)
    })
}

/// Есть ли пользователь в тексте `/etc/passwd`.
pub fn passwd_has_user(passwd: &str, user: &str) -> bool {
    passwd.lines().any(|l| l.split(':').next() == Some(user))
}

/// Данные о greetd для проверки готовности.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GreetdFacts {
    pub command: GreetdCommand,
    /// Есть ли программа из `command` (для `Graphical`).
    pub program_exists: bool,
}

/// Данные о SDDM для проверки готовности.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SddmFacts {
    pub theme: SddmThemeState,
    /// Приветствие на Wayland: `(компоситор, есть ли он)`; `None` — на X.
    pub wayland_compositor: Option<(String, bool)>,
}

/// Данные о LightDM для проверки готовности.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LightdmFacts {
    pub greeter_session: Option<String>,
    pub default_greeter: Option<String>,
    pub greeters: Vec<DmGreeter>,
}

/// Заранее собранные ответы о системе для [`dm_readiness`] — всё без root.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DmFacts {
    /// Программа из `ExecStart` юнита на месте.
    pub binary_exists: bool,
    /// Юнит замаскирован (`/etc/systemd/system/<юнит>.service → /dev/null`).
    pub masked: bool,
    /// Пользователь приветствия; `None` — проверять некого.
    pub user: Option<String>,
    /// Пользователь есть или его создаст `systemd-sysusers`.
    pub user_ok: bool,
    /// Файл PAM службы (имя) и есть ли он в `/etc/pam.d` или `/usr/lib/pam.d`.
    pub pam: String,
    pub pam_exists: bool,
    /// Приветствие на X — нужен Xorg.
    pub needs_xorg: bool,
    pub xorg: bool,
    /// Число сеансов в `xsessions` и `wayland-sessions`.
    pub x_sessions: usize,
    pub wayland_sessions: usize,
    pub lightdm: Option<LightdmFacts>,
    pub greetd: Option<GreetdFacts>,
    pub sddm: Option<SddmFacts>,
}

/// Готов ли экран входа стать основным: первая найденная причина отказа
/// для человека, выбор приветствия (LightDM) или `Ready`. `Unsupported` —
/// Rexon на этой системе или для этого экрана входа кнопку не даёт.
///
/// Проба здоровья бинарей Qt (`ldd -r`) сюда пока не входит.
pub fn dm_readiness(
    spec: Option<&DmSpec>,
    backend: &DmSwitchBackend,
    facts: &DmFacts,
) -> DmReadiness {
    let Some(spec) = spec.filter(|s| s.switchable) else {
        return DmReadiness::Unsupported;
    };
    if !backend.switch_supported() {
        return DmReadiness::Unsupported;
    }
    let blocked = DmReadiness::Blocked;
    if facts.masked {
        return blocked(tr("This login screen is switched off in the system (the service is masked)."));
    }
    if !facts.binary_exists {
        // TRANSLATORS: %1 is a program name, e.g. sddm.
        return blocked(trf("The login screen program %1 is missing.", &[&spec.binary]));
    }
    if !facts.pam_exists {
        // TRANSLATORS: %1 is a PAM service file name, e.g. sddm.
        return blocked(trf("The sign-in settings file (PAM) “%1” is missing.", &[&facts.pam]));
    }
    if let (Some(user), false) = (facts.user.as_deref(), facts.user_ok) {
        return blocked(trf(
            // TRANSLATORS: %1 is a system user name, e.g. sddm.
            "The system user “%1” that the login screen runs as doesn't exist.",
            &[&user],
        ));
    }
    if facts.x_sessions + facts.wayland_sessions == 0 {
        return blocked(tr("No desktop sessions are installed — there would be nothing to sign in to."));
    }
    if facts.needs_xorg && !facts.xorg {
        return blocked(tr("This login screen needs Xorg, and Xorg isn't installed."));
    }
    if let Some(g) = &facts.greetd {
        match &g.command {
            GreetdCommand::Text(_) | GreetdCommand::Missing => {
                return blocked(tr("greetd is set up for text sign-in — the desktop won't start."));
            }
            GreetdCommand::Graphical(program) if !g.program_exists => {
                return blocked(trf(
                    // TRANSLATORS: %1 is a program name, e.g. tuigreet.
                    "The greeter %1 that greetd is set up to start isn't installed.",
                    &[program],
                ));
            }
            GreetdCommand::Graphical(_) => {}
        }
    }
    if let Some(s) = &facts.sddm {
        if let SddmThemeState::NeedsGreeter { theme, greeter } = &s.theme {
            return blocked(trf(
                // TRANSLATORS: %1 is a theme name, %2 a program name (sddm-greeter-qt6).
                "The SDDM theme “%1” needs %2, which isn't installed.",
                &[theme, greeter],
            ));
        }
        if let Some((compositor, false)) = &s.wayland_compositor {
            return blocked(trf(
                // TRANSLATORS: %1 is a program name, e.g. weston or kwin_wayland.
                "SDDM is set to start on Wayland with %1, which isn't installed.",
                &[compositor],
            ));
        }
    }
    if let Some(l) = &facts.lightdm {
        return lightdm_greeter_readiness(
            l.greeter_session.as_deref(),
            l.default_greeter.as_deref(),
            &l.greeters,
        );
    }
    DmReadiness::Ready
}

/// Предупреждения, которые не мешают переключению.
pub fn dm_warnings(facts: &DmFacts) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(SddmFacts { theme: SddmThemeState::Missing { theme }, .. }) = &facts.sddm {
        out.push(trf(
            // TRANSLATORS: %1 is a theme name.
            "The SDDM theme “%1” isn't installed — SDDM will show its built-in look.",
            &[theme],
        ));
    }
    out
}

/// Экран входа на системе без systemd по файлам выбора init: OpenRC —
/// `DISPLAYMANAGER=` в `/etc/conf.d/display-manager`, Devuan — путь к
/// программе в `/etc/X11/default-display-manager`. Ответ — юнит из таблицы
/// по имени программы; `None` — не определить.
pub fn dm_from_init_files(conf_d: Option<&str>, x11_default: Option<&str>) -> Option<String> {
    let from_conf = conf_d.and_then(|t| {
        t.lines().rev().find_map(|l| {
            let v = l.trim().strip_prefix("DISPLAYMANAGER=")?;
            Some(v.trim().trim_matches(['"', '\'']).to_string())
        })
    });
    let from_x11 = x11_default.and_then(|t| {
        let line = t.lines().map(str::trim).find(|l| !l.is_empty())?;
        Some(line.rsplit('/').next().unwrap_or(line).to_string())
    });
    let name = from_conf.or(from_x11).filter(|n| !n.is_empty())?;
    LOGIN_MANAGER_SPECS
        .iter()
        .find(|s| s.binary == name || s.unit == name)
        .map(|s| s.unit.to_string())
        .or(Some(name))
}

impl DmReadiness {
    /// Приветствие, предлагаемое по умолчанию, когда выбирать человеку.
    pub fn suggested_greeter(&self) -> Option<&DmGreeter> {
        match self {
            DmReadiness::NeedsGreeterChoice(list) => suggested_greeter(list),
            _ => None,
        }
    }
}

impl LoginManagerInfo {
    /// Приветствие, предлагаемое по умолчанию (см. [`DmReadiness::suggested_greeter`]).
    pub fn suggested_greeter(&self) -> Option<&DmGreeter> {
        self.readiness.suggested_greeter()
    }
}

// ── «Сделать основным»: команда и тексты ──────────────────────────────────

/// Имя приветствия LightDM — имя файла без `.desktop`, без путей и флагов.
pub fn greeter_name_ok(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphanumeric())
        && chars.all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
}

/// `pkexec rexon-pkg-helper … switch-dm <юнит> [<приветствие>]`. `None` —
/// помощника нет или аргументы не прошли проверку: сырого `pkexec
/// systemctl` нет намеренно — он не повторил бы проверок и не откатил бы
/// ошибку (тогда интерфейс говорит [`login_manager_needs_helper`]).
pub fn switch_dm_cmd(
    family: PkgFamily,
    unit: &str,
    greeter: Option<&str>,
    helper_available: bool,
) -> Option<Vec<String>> {
    let mut args = vec![unit.to_string()];
    args.extend(greeter.map(String::from));
    privileged_pkg_cmd(family, "switch-dm", &args, helper_available)
}

/// Команда, которой из текстовой консоли возвращают прежний экран входа.
/// Та же строка, что помощник пишет в [`PREVIOUS_DM_FILE`].
pub fn dm_rollback_command(old_unit: &str, backend: &DmSwitchBackend) -> Option<String> {
    let old = old_unit.trim();
    if !greeter_name_ok(old) {
        return None;
    }
    let enable = format!("sudo systemctl enable --force {old}.service && sudo reboot");
    match backend {
        DmSwitchBackend::Systemd => Some(enable),
        DmSwitchBackend::SystemdDebian => {
            let pkg = DM_DEBIAN_PKG_FOR_UNIT
                .iter()
                .find(|(u, _)| *u == old)
                .map_or(old, |(_, p)| *p);
            Some(format!("sudo dpkg-reconfigure {pkg} && {enable}"))
        }
        _ => None,
    }
}

fn dm_label(unit: &str) -> String {
    let label = crate::categories::login_manager_label(unit);
    if label.is_empty() { unit.trim().to_string() } else { label.to_string() }
}

/// Заголовок окна подтверждения.
pub fn login_manager_switch_title(new_unit: &str) -> String {
    // TRANSLATORS: %1 is a login screen name (SDDM, LightDM, …).
    trf("Make %1 the login screen?", &[&dm_label(new_unit)])
}

/// Подсказка «как вернуть прежний экран входа из консоли». `None` — для
/// этого механизма команды нет.
pub fn login_manager_rollback_hint(old_unit: &str, backend: &DmSwitchBackend) -> Option<String> {
    let cmd = dm_rollback_command(old_unit, backend)?;
    Some(trf(
        // TRANSLATORS: %1 is a terminal command, %2 a file path.
        "If there's no login screen after the restart: press Ctrl+Alt+F3, sign in with your name and password and run:\n%1\nThe same is written in %2.",
        &[&cmd, &PREVIOUS_DM_FILE],
    ))
}

/// Текст окна подтверждения (до пароля, правило #13 — условие названо
/// заранее): что станет основным, какое приветствие, когда появится и как
/// вернуть. `old_unit` — нынешний основной (`None` — не определён).
pub fn login_manager_switch_confirm(
    old_unit: Option<&str>,
    new_unit: &str,
    greeter: Option<&DmGreeter>,
    backend: &DmSwitchBackend,
) -> String {
    let new = dm_label(new_unit);
    let mut parts = vec![trf(
        // TRANSLATORS: %1 is a login screen name.
        "%1 will appear after a restart. Your current session won't be interrupted.",
        &[&new],
    )];
    if let Some(g) = greeter {
        let name = if g.name.is_empty() { &g.id } else { &g.name };
        // TRANSLATORS: %1 is a LightDM greeter name (Slick Greeter, …).
        parts.push(trf("Greeter: %1.", &[name]));
    }
    if let Some(old) = old_unit.map(str::trim).filter(|o| !o.is_empty()) {
        parts.push(trf(
            // TRANSLATORS: %1 is the new login screen name, %2 the current one.
            "If %1 doesn't start, Rexon brings back %2 by itself on that boot.",
            &[&new, &dm_label(old)],
        ));
        parts.extend(login_manager_rollback_hint(old, backend));
    }
    parts.join("\n\n")
}

/// Итог «сделано».
pub fn login_manager_switch_done(new_unit: &str) -> String {
    // TRANSLATORS: %1 is a login screen name.
    trf("Done: %1 will be the login screen after a restart.", &[&dm_label(new_unit)])
}

/// Итог «не вышло» с причиной от помощника.
pub fn login_manager_switch_failed(reason: &str) -> String {
    let reason = reason.trim();
    if reason.is_empty() {
        tr("Couldn't change the login screen.")
    } else {
        // TRANSLATORS: %1 is the reason reported by Rexon's helper.
        trf("Couldn't change the login screen: %1", &[&reason])
    }
}

/// Помощника нет — переключать нечем.
pub fn login_manager_needs_helper() -> String {
    tr("Changing the login screen needs the Rexon helper, and it isn't installed. Reinstall Rexon to add it.")
}

/// Ответ помощника на `switch-dm`: `(сделано, текст для человека)`. Причина
/// отказа — его последняя строка без имени помощника; код 126/127 —
/// пароль не подтверждён.
pub fn switch_dm_outcome(
    returncode: Option<i32>,
    stdout: &str,
    stderr: &str,
    new_unit: &str,
    manual_cmd: &str,
) -> (bool, String) {
    if returncode == Some(0) {
        return (true, login_manager_switch_done(new_unit));
    }
    if matches!(returncode, Some(126 | 127)) {
        return (false, pkexec_denied_message(manual_cmd));
    }
    let prefix = format!("{PKG_HELPER_NAME}: ");
    let reason = stderr
        .lines()
        .chain(stdout.lines())
        .filter(|l| !l.trim().is_empty())
        .last()
        .map(|l| l.strip_prefix(&prefix).unwrap_or(l).to_string())
        .unwrap_or_default();
    (false, login_manager_switch_failed(&reason))
}

/// Пояснение вместо кнопки там, где Rexon экран входа не переключает.
/// `manual_cmd` — команда для этой init ([`dm_manual_switch_command`]).
pub fn login_manager_unsupported_note(
    backend: &DmSwitchBackend,
    manual_cmd: Option<&str>,
) -> Option<String> {
    Some(match backend {
        DmSwitchBackend::Systemd | DmSwitchBackend::SystemdDebian => return None,
        DmSwitchBackend::NixosDeclarative => tr(
            "On NixOS the login screen is chosen in configuration.nix (services.displayManager), so Rexon doesn't change it.",
        ),
        DmSwitchBackend::SystemdSuseAlternatives => {
            tr("On openSUSE Rexon can't change the login screen yet.")
        }
        DmSwitchBackend::Unsupported(init) => {
            let init = if init.is_empty() { tr("its own init system") } else { init.clone() };
            match manual_cmd {
                Some(cmd) => trf(
                    // TRANSLATORS: %1 is an init system name (OpenRC, runit, …), %2 a terminal command.
                    "Here the login screen is switched by %1, and Rexon can't do it yet. In a terminal:\n%2",
                    &[&init, &cmd],
                ),
                // TRANSLATORS: %1 is an init system name (OpenRC, runit, …).
                None => trf("Here the login screen is switched by %1, and Rexon can't do it yet.", &[&init]),
            }
        }
    })
}

// ============================================================================
// Живой разбор вывода pacman (порт `PackageManager._parse_pacman_line`)
// ============================================================================
//
// Установка рабочего стола показывает ход дела построчно: хвост лог-файла
// читает Python, а СМЫСЛ строки — какой это шаг, какой пакет, какое
// сообщение показать человеку — считается здесь. Раньше смысл считали семь
// регулярок в Python; регулярки перенесены разбором «по буквам», потому что
// ядро обходится без крейта regex.
//
// Разбор чистый: ни файлов, ни процессов. Эталон — записанные ответы живого
// Python на тех же строках (`golden/system_plan_texts.json`, ключ
// `pacman_progress`).
//
// ⚠️ Починка при порте: у Python-версии был параметр `total_packages`,
// который не читала ни одна ветка. Он не перенесён — вызывающему коду он
// тоже был не нужен.

/// Разобранная строка вывода pacman: стадия, готовое сообщение для человека
/// и разложенные поля (там, где они есть).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PacmanLine {
    pub stage: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<u64>,
}

impl PacmanLine {
    fn plain(stage: &str, message: String) -> Self {
        PacmanLine {
            stage: stage.to_string(),
            message,
            current: None,
            total: None,
            action: None,
            package: None,
            filename: None,
            count: None,
        }
    }
}

/// `\w` Python: буква, цифра или подчёркивание (Unicode-осведомлённо).
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Регистронезависимое «строка начинается с»: возвращает хвост.
fn ci_strip_prefix<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    let mut si = s.char_indices();
    for pc in prefix.chars() {
        let (_, sc) = si.next()?;
        if !sc.eq_ignore_ascii_case(&pc) {
            return None;
        }
    }
    Some(&s[si.next().map(|(i, _)| i).unwrap_or(s.len())..])
}

/// `\s+` в начале: хвост после хотя бы одного пробельного символа.
fn strip_ws1(s: &str) -> Option<&str> {
    let t = s.trim_start();
    if t.len() == s.len() {
        None
    } else {
        Some(t)
    }
}

/// `(.+?)\.{3}\s*$` — тело до завершающего многоточия (непустое).
fn strip_ellipsis(s: &str) -> Option<&str> {
    let body = s.trim_end().strip_suffix("...")?;
    if body.is_empty() {
        None
    } else {
        Some(body)
    }
}

/// `^\((\d+)/(\d+)\)\s+` — счётчик шага и хвост строки после него.
fn counter_prefix(line: &str) -> Option<(u64, u64, &str)> {
    let rest = line.strip_prefix('(')?;
    let close = rest.find(')')?;
    let (cur, total) = rest[..close].split_once('/')?;
    if cur.is_empty()
        || total.is_empty()
        || !cur.bytes().all(|b| b.is_ascii_digit())
        || !total.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    Some((
        cur.parse().ok()?,
        total.parse().ok()?,
        strip_ws1(&rest[close + 1..])?,
    ))
}

/// Разделить «действие \s+ имя» ровно так, как это делала жадная `\s+` с
/// откатом: имя — всё после пробелов, а если после них пусто, откатываемся
/// на один пробельный символ (он и станет именем).
fn split_after_action(rest: &str) -> Option<String> {
    let name = rest.trim_start();
    if name.len() == rest.len() {
        return None; // `\s+` требует хотя бы один пробел
    }
    if !name.is_empty() {
        return Some(name.to_string());
    }
    // Только пробелы: последний из них и есть `(.+?)`.
    rest.chars().next_back().map(|c| c.to_string())
}

/// Действие и пакет из хвоста строки `(N/M) …`, либо None.
fn operation_action(payload: &str) -> Option<(String, String)> {
    for act in [
        "installing",
        "upgrading",
        "removing",
        "reinstalling",
        "downgrading",
    ] {
        if let Some(rest) = payload.strip_prefix(act) {
            if let Some(pkg) = split_after_action(rest) {
                return Some((act.to_string(), pkg));
            }
        }
    }
    // `checking \w[\w ]*` / `loading \w[\w ]*` — жадный набор слов и
    // пробелов, откат до места, где остаётся ещё хотя бы одно слово.
    for act in ["checking ", "loading "] {
        let Some(rest) = payload.strip_prefix(act) else {
            continue;
        };
        let Some(first) = rest.chars().next() else {
            continue;
        };
        if !is_word_char(first) {
            continue;
        }
        let run: Vec<usize> = rest
            .char_indices()
            .take_while(|(_, c)| is_word_char(*c) || *c == ' ')
            .map(|(i, c)| i + c.len_utf8())
            .collect();
        for &end in run.iter().rev() {
            if let Some(pkg) = split_after_action(&rest[end..]) {
                return Some((
                    format!("{}{}", act, &rest[..end]),
                    pkg,
                ));
            }
        }
    }
    None
}

/// `^Packages\s+\((\d+)\)` — сколько всего пакетов в транзакции.
fn packages_count(line: &str) -> Option<u64> {
    let rest = strip_ws1(ci_strip_prefix(line, "Packages")?)?;
    let inner = rest.strip_prefix('(')?;
    let close = inner.find(')')?;
    let digits = &inner[..close];
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// `^<заголовок>:\s+([\d.]+)\s+(\w+)` — размер и единица измерения.
fn total_size(line: &str, header: &str) -> Option<String> {
    let rest = strip_ws1(ci_strip_prefix(line, header)?)?;
    let num_len = rest
        .bytes()
        .take_while(|b| b.is_ascii_digit() || *b == b'.')
        .count();
    if num_len == 0 {
        return None;
    }
    let unit_rest = strip_ws1(&rest[num_len..])?;
    let unit_len: usize = unit_rest
        .char_indices()
        .take_while(|(_, c)| is_word_char(*c))
        .map(|(_, c)| c.len_utf8())
        .sum();
    if unit_len == 0 {
        return None;
    }
    Some(format!("{} {}", &rest[..num_len], &unit_rest[..unit_len]))
}

/// `^(\S+)\s+is up to date\s*--\s*skipping` — имя пакета, который pacman
/// пропустил как уже установленный (`--needed`).
fn skip_package(body: &str) -> Option<&str> {
    let name_len = body.chars().take_while(|c| !c.is_whitespace()).count();
    let name_len: usize = body
        .chars()
        .take(name_len)
        .map(|c| c.len_utf8())
        .sum();
    if name_len == 0 {
        return None;
    }
    let rest = strip_ws1(&body[name_len..])?;
    let rest = ci_strip_prefix(rest, "is up to date")?;
    let rest = rest.trim_start();
    let rest = rest.strip_prefix("--")?;
    let rest = rest.trim_start();
    ci_strip_prefix(rest, "skipping")?;
    Some(&body[..name_len])
}

// ── Про пересортировку зеркал (решено 2026-09-01, НЕ делаем) ────────────
//
// Соблазн: увидели подряд несколько отказов зеркал — предложить человеку
// перетасовать список (`cachyos-rate-mirrors`, `rate-mirrors`, `reflector` —
// на машине Wehrwolfmann есть все три). Отказано, и вот почему.
//
// 1. Чинить нечего. Отказ зеркала, после которого транзакция ЗАВЕРШИЛАСЬ
//    удачей, — это не поломка, а штатная работа запасного списка: 12 зеркал
//    в cachyos-mirrorlist и 66 в mirrorlist ровно для этого и лежат. Звать
//    человека чинить то, что само себя починило, — худший род ложной тревоги.
// 2. Цена ошибки несимметрична. Список зеркал — общесистемный, root-овый, и
//    им пользуется ВСЯ система, а не одна наша установка. Неудачная
//    пересортировка ломает любое будущее обновление, включая наше
//    собственное. Выигрыш — минуты, проигрыш — работающий пакетный менеджер.
// 3. Файлы чужие. `/etc/pacman.d/cachyos-mirrorlist` принадлежит пакету
//    cachyos-mirrorlist, `/etc/pacman.d/mirrorlist` — пакету
//    pacman-mirrorlist. Переписав их, мы получаем .pacnew при каждом
//    обновлении и спорим с инструментами самого дистрибутива.
// 4. Это медленно и требует сети. Пересортировка сама ходит по десяткам
//    зеркал; делать её посреди установки стола значит удлинить успешную
//    операцию ради симптома, которого уже нет.
//
// Что делаем вместо: говорим правду построчно (ниже) и не красим переживаемое
// красным. Если загрузка провалится ПО-НАСТОЯЩЕМУ, человек увидит настоящую
// ошибку pacman и решит сам — совет про зеркала уместен там, а не здесь.

/// `failed retrieving file '<файл>' from <зеркало> : <причина>` — имя зеркала.
///
/// Это ПЕРЕЖИВАЕМЫЙ сбой: pacman тут же берёт следующее зеркало из списка, и
/// установка идёт дальше. Настоящий отказ загрузки pacman объявляет ОТДЕЛЬНОЙ
/// строкой (`failed to retrieve some files` / `failed to commit transaction`),
/// поэтому опознать одно по другому можно построчно, не дожидаясь итога.
fn mirror_retrieve_failure(body: &str) -> Option<&str> {
    let rest = ci_strip_prefix(body, "failed retrieving file")?;
    let rest = strip_ws1(rest)?;
    // Имя файла в кавычках — само по себе не нужно: без него одинаковые
    // отказы одного зеркала сворачиваются в одну строку.
    let rest = rest.strip_prefix('\'')?;
    let close = rest.find('\'')?;
    let rest = strip_ws1(&rest[close + 1..])?;
    let rest = ci_strip_prefix(rest, "from")?;
    let rest = strip_ws1(rest)?;
    // Хвост «<зеркало> : <причина>»: имя зеркала — до первого пробела.
    let host_len: usize = rest
        .chars()
        .take_while(|c| !c.is_whitespace())
        .map(|c| c.len_utf8())
        .sum();
    if host_len == 0 {
        return None;
    }
    Some(&rest[..host_len])
}

/// `too many errors from <зеркало>, skipping for the remainder of this
/// transaction` — имя зеркала, которое pacman вычеркнул до конца транзакции.
/// Тоже переживаемо: список зеркал на то и список.
fn mirror_skipped(body: &str) -> Option<&str> {
    let rest = ci_strip_prefix(body, "too many errors from")?;
    let rest = strip_ws1(rest)?;
    let comma = rest.find(',')?;
    let host = rest[..comma].trim_end();
    if host.is_empty() || host.contains(char::is_whitespace) {
        return None;
    }
    ci_strip_prefix(rest[comma + 1..].trim_start(), "skipping")?;
    Some(host)
}

/// `There are <N> <вид> ... <предмет>:` — разбор шапки интерактивного вопроса
/// pacman. Возвращает (N, предмет) для вопроса нужного вида.
///
/// Вопросов ДВА, и оба pacman задаёт сам:
///   * `There are 24 members in group lxqt:` — какие члены группы ставить;
///   * `There are 2 providers available for libQt6Xdg.so=4-64:` — чей пакет
///     закрывает зависимость.
/// При `--noconfirm` (а мы зовём pacman только так) ни один из них ввода НЕ
/// ждёт: вопрос печатается, берётся значение по умолчанию. Но в журнале он
/// выглядел загадкой — человек видел вопрос и не видел ответа.
fn there_are_question<'a>(line: &'a str, kind: &str) -> Option<(u64, &'a str)> {
    let rest = strip_ws1(ci_strip_prefix(line, "There are")?)?;
    let num_len: usize = rest
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .map(|c| c.len_utf8())
        .sum();
    if num_len == 0 {
        return None;
    }
    let count: u64 = rest[..num_len].parse().ok()?;
    let rest = strip_ws1(&rest[num_len..])?;
    let rest = ci_strip_prefix(rest, kind)?;
    let rest = strip_ws1(rest)?;
    let subject = rest.trim_end().strip_suffix(':')?.trim_end();
    if subject.is_empty() {
        return None;
    }
    Some((count, subject))
}

/// `re.sub(r'-[^-\s]+-[^-\s]+$', '', name)` — отрезать «-<версия>-<релиз>».
fn strip_version_suffix(name: &str) -> &str {
    let ok = |seg: &str| !seg.is_empty() && !seg.contains(|c: char| c == '-' || c.is_whitespace());
    let Some(last) = name.rfind('-') else {
        return name;
    };
    if !ok(&name[last + 1..]) {
        return name;
    }
    let Some(prev) = name[..last].rfind('-') else {
        return name;
    };
    if !ok(&name[prev + 1..last]) {
        return name;
    }
    &name[..prev]
}

/// Разобрать одну строку вывода pacman: стадия + сообщение для человека.
/// None — строка ничего не значит для показа хода дела.
pub fn parse_pacman_line(raw: &str) -> Option<PacmanLine> {
    let line = raw.trim();
    if line.is_empty() {
        return None;
    }

    // (N/M) installing|upgrading|… <пакет>...
    if let Some((cur, total, rest)) = counter_prefix(line) {
        if let Some(payload) = strip_ellipsis(rest) {
            if let Some((action, pkg)) = operation_action(payload) {
                let check = action.starts_with("checking") || action.starts_with("loading");
                let label = match action.as_str() {
                    "installing" => trc("pacman action", "Installing"),
                    "upgrading" => trc("pacman action", "Upgrading"),
                    "removing" => trc("pacman action", "Removing"),
                    "reinstalling" => trc("pacman action", "Reinstalling"),
                    "downgrading" => trc("pacman action", "Downgrading"),
                    other => other.to_string(),
                };
                return Some(PacmanLine {
                    stage: (if check { "check" } else { "pkg_install" }).to_string(),
                    message: if check {
                        format!("🔍 ({}/{}) {} {}", cur, total, action, pkg)
                    } else {
                        format!("📦 ({}/{}) {}: {}", cur, total, label, pkg)
                    },
                    current: Some(cur),
                    total: Some(total),
                    action: Some(action),
                    package: Some(pkg),
                    filename: None,
                    count: None,
                });
            }
        }
    }

    // downloading <файл>...
    if let Some(rest) = ci_strip_prefix(line, "downloading") {
        if let Some(rest) = strip_ws1(rest) {
            if let Some(name) = strip_ellipsis(rest) {
                let mut ev = PacmanLine::plain(
                    "download",
                    trf("⬇️  Downloading: %1", &[&name]),
                );
                ev.filename = Some(name.to_string());
                return Some(ev);
            }
        }
    }

    // Packages (N)
    if let Some(count) = packages_count(line) {
        let mut ev = PacmanLine::plain("summary", trf("📋 Total packages: %1", &[&count]));
        ev.count = Some(count);
        return Some(ev);
    }

    // Total Download Size / Total Installed Size
    if let Some(size) = total_size(line, "Total Download Size:") {
        return Some(PacmanLine::plain("summary", trf("⬇️  Download size: %1", &[&size])));
    }
    if let Some(size) = total_size(line, "Total Installed Size:") {
        return Some(PacmanLine::plain("summary", trf("💾 Installed size: %1", &[&size])));
    }

    // :: Заголовок раздела — но сперва два интерактивных вопроса pacman,
    // которые он же сам себе и отвечает при `--noconfirm`.
    if let Some(rest) = line.strip_prefix("::") {
        if let Some(body) = strip_ws1(rest) {
            // «There are N members in group G:» — ставим всю группу.
            if let Some((count, group)) = there_are_question(body, "members in group") {
                let mut ev = PacmanLine::plain(
                    "prompt",
                    fill(
                        &ntr(
                            // TRANSLATORS: %1 is a package group name.
                            "🧩 Group %1 (%n package) — installing all of it, nothing to answer",
                            "🧩 Group %1 (%n packages) — installing all of it, nothing to answer",
                            count,
                        ),
                        &[&group],
                    ),
                );
                ev.count = Some(count);
                ev.package = Some(group.to_string());
                return Some(ev);
            }
            // «There are N providers available for X:» — берём поставщика из
            // репозитория с наивысшим приоритетом. Это НЕ случайность порядка:
            // pacman перечисляет поставщиков в порядке репозиториев из
            // pacman.conf, а первым там стоит тот, который человек (или его
            // дистрибутив) объявил главным. Ровно это же взял бы `pacman -S`
            // из терминала, поэтому решать за него мы не вправе.
            if let Some((count, subject)) =
                there_are_question(body, "providers available for")
            {
                let mut ev = PacmanLine::plain(
                    "prompt",
                    trf(
                        // TRANSLATORS: %1 is what is being provided, %2 how many
                        // packages can provide it.
                        "🔀 %1: %2 providers — taking the one from the first \
                         repository in pacman.conf priority order",
                        &[&subject, &count],
                    ),
                );
                ev.count = Some(count);
                ev.package = Some(subject.to_string());
                return Some(ev);
            }
            if !body.is_empty() {
                return Some(PacmanLine::plain("section", format!("📌 {}", body)));
            }
        }
    }

    // «Enter a selection (default=all):» / «Enter a number (default=1):» —
    // хвост уже объяснённого вопроса. Показывать его нечего: ответ назван
    // строкой выше, а сам вопрос ввода не ждёт.
    //
    // ⚠️ Именно ПУСТОЕ сообщение, а не `None`: `None` у этого разбора значит
    // «строка мне незнакома», и исполнитель показывает её сырой (этап
    // `raw`). Поймано показом журнала целиком: вопрос был объяснён строкой
    // выше и тут же напечатан по-английски ещё раз.
    if let Some(rest) = ci_strip_prefix(line, "Enter a") {
        if let Some(rest) = strip_ws1(rest) {
            if (ci_strip_prefix(rest, "selection").is_some()
                || ci_strip_prefix(rest, "number").is_some())
                && rest.contains("default=")
            {
                return Some(PacmanLine::plain("prompt", String::new()));
            }
        }
    }

    // error: / warning:
    if let Some(rest) = ci_strip_prefix(line, "error:") {
        if let Some(body) = strip_ws1(rest) {
            if !body.is_empty() {
                // ⚠️ Сбой ОДНОГО зеркала — не наша беда: pacman берёт
                // следующее и доводит установку до конца. Красный крест на
                // такой строке говорил покупателю «всё сломалось» там, где
                // всё в порядке. Мерило: транзакция кончилась удачей →
                // крестов в журнале нет вовсе; настоящий отказ загрузки
                // pacman объявляет отдельно («failed to retrieve some
                // files», «failed to commit transaction»), и он остаётся
                // красным.
                if let Some(host) = mirror_retrieve_failure(body) {
                    let mut ev = PacmanLine::plain(
                        "mirror",
                        trf("🌐 mirror didn't respond (%1), trying another one", &[&host]),
                    );
                    ev.package = Some(host.to_string());
                    return Some(ev);
                }
                return Some(PacmanLine::plain("error", format!("❌ {}", body)));
            }
        }
    }
    if let Some(rest) = ci_strip_prefix(line, "warning:") {
        if let Some(body) = strip_ws1(rest) {
            if !body.is_empty() {
                // «<pkg>-<ver> is up to date -- skipping» — это не беда, а
                // нормальный ответ `--needed` в режиме «Проверить /
                // Восстановить»: показываем дружелюбным «уже установлено».
                // «too many errors from <зеркало>, skipping…» — из того же
                // класса, что и отказ загрузки выше: список зеркал на то и
                // список. Показываем спокойно, той же строкой.
                if let Some(host) = mirror_skipped(body) {
                    let mut ev = PacmanLine::plain(
                        "mirror",
                        trf(
                            "🌐 no longer asking mirror %1 for the rest of this installation",
                            &[&host],
                        ),
                    );
                    ev.package = Some(host.to_string());
                    return Some(ev);
                }
                if let Some(pkg) = skip_package(body) {
                    let base = strip_version_suffix(pkg);
                    let base = if base.is_empty() { pkg } else { base };
                    let mut ev = PacmanLine::plain(
                        "skip",
                        trf("✅ already installed: %1", &[&base]),
                    );
                    ev.package = Some(base.to_string());
                    return Some(ev);
                }
                return Some(PacmanLine::plain("warning", format!("⚠️  {}", body)));
            }
        }
    }

    // (N/M) <хук>... — пост-транзакционные хуки pacman
    if let Some((cur, total, rest)) = counter_prefix(line) {
        if let Some(hook) = strip_ellipsis(rest) {
            return Some(PacmanLine {
                stage: "hook".to_string(),
                message: format!("🔧 ({}/{}) {}", cur, total, hook),
                current: Some(cur),
                total: Some(total),
                action: None,
                package: None,
                filename: None,
                count: None,
            });
        }
    }

    if line.starts_with("resolving ") || line.starts_with("looking for ") {
        return Some(PacmanLine::plain("check", format!("🔍 {}", line)));
    }
    if line.to_lowercase().contains("retrieving packages") {
        return Some(PacmanLine::plain(
            "download",
            tr("⬇️  Downloading packages…"),
        ));
    }
    None
}

// ============================================================================
// Идиот-защита «экран входа обязан быть» (порт `ensure_display_manager`)
// ============================================================================
//
// Установка тайлового WM (niri/sway/Hyprland) сама display-manager не тянет:
// sddm намеренно убран из их пакет-листов, потому что экран входа —
// общесистемная сущность, а не часть конкретного стола. Без этой защиты
// человек после перезагрузки остаётся без входа в графику.
//
// Решение целиком здесь: есть ли что ставить, чем ставить, чем включать и
// что при этом сказать. Живые пробы (есть ли DM, есть ли systemctl, развёрнут
// ли привилегированный helper) подаются параметрами, запуск команд — за
// исполнителем.

/// Имя ПАКЕТА sddm по семейству. Gentoo — категория. NIX исключён (там
/// декларативная сборка, не pkexec-путь), CLEAR — sddm не поставляется
/// отдельным bundle; на них автоустановка мягко пропускается.
pub const SDDM_PKG_FOR_FAMILY: &[(&str, &str)] = &[
    ("arch", "sddm"),
    ("debian", "sddm"),
    ("fedora", "sddm"),
    ("suse", "sddm"),
    ("gentoo", "x11-misc/sddm"),
    ("alpine", "sddm"),
    ("void", "sddm"),
    ("solus", "sddm"),
    ("alt", "sddm"),
];

/// Что делать с экраном входа после установки рабочего стола.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind")]
pub enum EnsureDmPlan {
    /// Включённый экран входа уже есть — не вмешиваемся.
    Present { message: String, code: String },
    /// Ставить нечего или нечем: не systemd, нет имени пакета для семейства,
    /// нет команды установки. Не ошибка — установка стола не рушится.
    Skipped { message: Option<String>, code: String },
    /// Экрана входа нет: ставим SDDM и включаем его юнит.
    Install {
        message: String,
        fraction: f64,
        package: String,
        install: Cmd,
        enable: Cmd,
        done_message: String,
        done_fraction: f64,
        code: String,
    },
}

/// Решение и команды идиот-защиты экрана входа.
///
/// `dm_present` — ответ `display_manager_present_from`, `has_systemctl` —
/// наличие systemd, `pkg_helper_available` — развёрнут ли rexon-pkg-helper
/// (с ним обе команды идут именованными действиями org.rexon.* и не просят
/// пароль второй раз).
pub fn ensure_dm_plan(
    family: PkgFamily,
    dm_present: bool,
    has_systemctl: bool,
    pkg_helper_available: bool,
) -> EnsureDmPlan {
    if dm_present {
        return EnsureDmPlan::Present {
            message: tr("⚙️ Login screen already set up — skipping"),
            code: "present".to_string(),
        };
    }
    if !has_systemctl {
        // Не systemd: экраном входа заведует сам init, второй DM не плодим.
        return EnsureDmPlan::Skipped { message: None, code: "skipped".to_string() };
    }
    let fam = family.as_str();
    let Some(pkg) = table_get(SDDM_PKG_FOR_FAMILY, fam) else {
        return EnsureDmPlan::Skipped {
            message: Some(tr(
                "⚙️ Login screen: automatic setup isn't available for this distribution",
            )),
            code: "skipped".to_string(),
        };
    };
    let pkgs = [pkg.to_string()];
    let install_argv = match privileged_pkg_cmd(family, "install", &pkgs, pkg_helper_available) {
        Some(argv) => argv,
        None => {
            // Сырой pkexec по общей install-таблице семейства.
            let Some(tmpl) = table_get(PKG_HELPER_INSTALL, fam) else {
                return EnsureDmPlan::Skipped {
                    message: None,
                    code: "skipped".to_string(),
                };
            };
            let mut argv = vec!["pkexec".to_string()];
            argv.extend(
                tmpl.replace("{PKGS}", pkg)
                    .split_whitespace()
                    .map(str::to_string),
            );
            argv
        }
    };
    let sddm_unit = ["sddm".to_string()];
    let enable_argv =
        privileged_pkg_cmd(family, "enable-dm", &sddm_unit, pkg_helper_available)
            .unwrap_or_else(|| {
                vec![
                    "pkexec".to_string(),
                    "systemctl".to_string(),
                    "enable".to_string(),
                    "sddm.service".to_string(),
                ]
            });
    EnsureDmPlan::Install {
        message: tr("🖥️ No login screen — installing SDDM…"),
        fraction: 0.90,
        package: pkg.to_string(),
        install: Cmd::new(install_argv, Some(crate::utils::DOWNLOAD_TIMEOUT_SECS), false),
        enable: Cmd::new(enable_argv, Some(30), false),
        done_message: tr("✅ SDDM installed and enabled"),
        done_fraction: 0.92,
        code: "installed".to_string(),
    }
}

// ============================================================================
// Мелочи установки, жившие в Python: AUR-хелпер, доли прогресса, тексты итога
// ============================================================================

/// Помощники AUR в порядке предпочтения — порт `_find_aur_helper`.
pub const AUR_HELPERS: &[&str] = &["yay", "paru", "trizen", "pikaur", "aurman"];

/// Первый найденный помощник AUR (только Arch), либо None.
/// `has` — живая проверка наличия бинаря (`shutil.which`).
pub fn pick_aur_helper(
    family: PkgFamily,
    has: &dyn Fn(&str) -> bool,
) -> Option<String> {
    if family != PkgFamily::Arch {
        return None;
    }
    AUR_HELPERS
        .iter()
        .find(|h| has(h))
        .map(|h| h.to_string())
}

/// Overlay Gentoo, нужный этому рабочему столу (либо None).
pub fn gentoo_overlay_for(de_id: &str) -> Option<&'static str> {
    table_get(GENTOO_OVERLAYS, de_id)
}

/// Команда, которой ставится инструмент подключения overlay'ев.
///
/// Собрана из двух вещей движка: имени [`GENTOO_OVERLAY_TOOL_PACKAGE`] и
/// таблицы [`install_cmd`]; выбор именованного или сырого пути — общий
/// ([`privileged_install_argv`]). Прежде всё это было переписано руками в
/// `ensure_gentoo_overlay`, и копия успела разойтись с движком: потеряла
/// `--quiet`.
///
/// `None` — не Gentoo: overlay'ев там нет, и ставить нечего.
pub fn gentoo_overlay_tool_install_cmd(
    family: PkgFamily,
    helper_available: bool,
) -> Option<Vec<String>> {
    if family != PkgFamily::Gentoo {
        return None;
    }
    let packages = vec![GENTOO_OVERLAY_TOOL_PACKAGE.to_string()];
    let base = install_cmd(family, &packages).ok()?;
    Some(privileged_install_argv(family, &packages, &base, helper_available))
}

/// Текст отказа, когда замок пакетного менеджера так и не освободился.
pub fn pkg_lock_timeout_message(timeout_secs: u64) -> String {
    trf(
        // TRANSLATORS: %1 is a number of seconds.
        "The package manager lock wasn't released within %1 s. Another \
         transaction or a snapper snapshot may be stuck. Check with: pgrep -af \
         'pacman|yay|paru|snap-pac|snapper'",
        &[&timeout_secs],
    )
}

/// Первый счётчик вида «(N/M)» в тексте — порт `re.search(r"\((\d+)/(\d+)\)")`.
fn find_counter(text: &str) -> Option<(u64, u64)> {
    let bytes = text.as_bytes();
    for (i, b) in bytes.iter().enumerate() {
        if *b != b'(' {
            continue;
        }
        let rest = &text[i + 1..];
        let Some(close) = rest.find(')') else { continue };
        let Some((a, b2)) = rest[..close].split_once('/') else { continue };
        if a.is_empty()
            || b2.is_empty()
            || !a.bytes().all(|c| c.is_ascii_digit())
            || !b2.bytes().all(|c| c.is_ascii_digit())
        {
            continue;
        }
        if let (Ok(cur), Ok(total)) = (a.parse(), b2.parse()) {
            return Some((cur, total));
        }
    }
    None
}

/// Доля выполнения установки рабочего стола по стадии и сообщению.
///
/// Порт замыкания `_pkg_cb` из `install_de`: шаги пакетного менеджера
/// занимают 5–65 % полосы, пост-транзакционные хуки 70–75 %, хвост AUR
/// доводит до 85 %. Счётчик «(N/M)» берётся из готового сообщения — того
/// самого, что собрал `parse_pacman_line`.
pub fn install_de_fraction(stage: &str, message: &str) -> Option<f64> {
    let ratio = || -> Option<f64> {
        let (cur, total) = find_counter(message)?;
        Some(cur as f64 / total.max(1) as f64)
    };
    match stage {
        "pkg_install" => Some(0.05 + 0.60 * ratio()?),
        "hook" => Some(0.70 + 0.05 * ratio()?),
        "done_main" => Some(0.70),
        "aur" => Some(0.75),
        "aur_done" | "aur_warn" => Some(0.85),
        _ => None,
    }
}

/// Итог главного шага пакетной установки: успех со строками журнала либо
/// отказ с объяснением для человека.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BatchMainOutcome {
    pub ok: bool,
    pub message: String,
    /// Пары (стадия, текст) для колбэка прогресса.
    pub logs: Vec<(String, String)>,
}

/// Где лежат программы окна пароля (агенты polkit) — по спискам файлов
/// пакетов: Arch (`polkit-gnome`, `polkit-kde-agent`, `mate-polkit`,
/// `lxqt-policykit`, `lxsession`, `xfce-polkit` из AUR), Debian/Ubuntu
/// (`policykit-1-gnome`, `polkit-kde-agent-1`, `mate-polkit`), Fedora
/// (`polkit-gnome` до 40, `polkit-kde`, `mate-polkit`, `lxqt-policykit`,
/// `xfce-polkit`) и openSUSE Tumbleweed. Подсказка называет первый
/// найденный на машине.
pub const POLKIT_AGENT_PATHS: &[&str] = &[
    // polkit-gnome: Arch; Debian и Ubuntu; Fedora и openSUSE.
    "/usr/lib/polkit-gnome/polkit-gnome-authentication-agent-1",
    "/usr/lib/policykit-1-gnome/polkit-gnome-authentication-agent-1",
    "/usr/libexec/polkit-gnome-authentication-agent-1",
    // polkit-kde-agent: Arch; Debian и Ubuntu; Fedora; openSUSE.
    "/usr/lib/polkit-kde-authentication-agent-1",
    "/usr/lib/x86_64-linux-gnu/libexec/polkit-kde-authentication-agent-1",
    "/usr/lib/aarch64-linux-gnu/libexec/polkit-kde-authentication-agent-1",
    "/usr/libexec/kf6/polkit-kde-authentication-agent-1",
    "/usr/libexec/polkit-kde-authentication-agent-1",
    // mate-polkit: Arch; Debian, Fedora, openSUSE.
    "/usr/lib/mate-polkit/polkit-mate-authentication-agent-1",
    "/usr/libexec/polkit-mate-authentication-agent-1",
    // lxqt-policykit: Arch, Debian, openSUSE; Fedora.
    "/usr/bin/lxqt-policykit-agent",
    "/usr/libexec/lxqt-policykit-agent",
    // xfce-polkit: AUR; Fedora.
    "/usr/lib/xfce-polkit/xfce-polkit",
    "/usr/libexec/xfce-polkit",
    // lxpolkit (LXDE): Arch, Debian.
    "/usr/bin/lxpolkit",
];

/// Сеанс входа, к которому polkit привяжет окно пароля: свой сеанс
/// процесса, а у процесса вне сеанса — графический сеанс пользователя
/// (`auto` в logind; так же выбирает и сам polkit).
pub fn login_session_argv() -> Vec<String> {
    ["loginctl", "show-session", "auto", "-p", "Id", "--value"].iter().map(|s| s.to_string()).collect()
}

/// Проба «запущено ли окно пароля (агент polkit) для сеанса `session_id`».
///
/// Спрашиваем сам polkit, а не ищем процессы по имени: агентов много
/// (polkit-gnome, polkit-kde-agent, mate-polkit, xfce-polkit,
/// lxqt-policykit, lxpolkit), а оболочки Noctalia и DankMaterialShell
/// держат агент внутри своего процесса quickshell — по имени его не найти.
/// Зато любой агент регистрируется у polkit на сеанс, и вторую регистрацию
/// на тот же сеанс polkit отклоняет («already exists»). Это и есть ответ.
/// Если агента нет, регистрация проходит — и снимается самим polkit, как
/// только процесс `gdbus` завершится и уйдёт с шины (так же pkexec
/// поднимает свой текстовый агент на время одной команды).
pub fn polkit_agent_probe_argv(session_id: &str) -> Vec<String> {
    vec![
        "gdbus".into(),
        "call".into(),
        "--system".into(),
        "--dest".into(),
        "org.freedesktop.PolicyKit1".into(),
        "--object-path".into(),
        "/org/freedesktop/PolicyKit1/Authority".into(),
        "--method".into(),
        "org.freedesktop.PolicyKit1.Authority.RegisterAuthenticationAgent".into(),
        format!("('unix-session', {{'session-id': <'{session_id}'>}})"),
        "C".into(),
        "/org/rexon/PolkitAgentProbe".into(),
    ]
}

/// Годится ли идентификатор сеанса для пробы (logind даёт `2`, `c1`).
pub fn login_session_id_ok(id: &str) -> bool {
    !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric())
}

/// Ответ пробы: `Some(true)` — агент уже есть, `Some(false)` — агента нет,
/// `None` — спросить не вышло (нет `gdbus`, сеанс не тот и т. п.). Текст
/// ошибки polkit не переводит.
pub fn polkit_agent_probe_answer(rc: Option<i32>, stderr: &str) -> Option<bool> {
    match rc {
        Some(0) => Some(false),
        _ if stderr.contains("An authentication agent already exists") => Some(true),
        _ => None,
    }
}

/// Порт `_batch_main_result`: разбор кода возврата главного pkexec-шага.
///
/// `result` — None, если команду не удалось даже запустить. Проверку
/// «работает ли polkit-агент» зовём ЛЕНИВО, только в ветке отказа
/// авторизации, — ровно как оригинал.
///
/// ⚠️ Починка при порте: если семейство неизвестно, оригинал падал прямо
/// в ветке ошибки (`_install_cmd` бросает RuntimeError вместо подсказки
/// «поставьте из терминала»). Теперь вместо падения человек видит тот же
/// текст объяснением.
pub fn batch_main_result(
    result: Option<(i32, &str, &str)>,
    main_seen: bool,
    packages: &[String],
    steamos: bool,
    family: PkgFamily,
    polkit_agent_running: &dyn Fn() -> bool,
    installed_agent: &dyn Fn() -> Option<String>,
) -> BatchMainOutcome {
    let ok = |logs: Vec<(String, String)>| BatchMainOutcome {
        ok: true,
        message: String::new(),
        logs,
    };
    let fail = |message: String| BatchMainOutcome {
        ok: false,
        message,
        logs: Vec::new(),
    };
    if !main_seen || packages.is_empty() {
        return ok(Vec::new());
    }
    let Some((code, stderr, stdout)) = result else {
        return fail(tr("Command not found"));
    };
    if code != 0 {
        let err = if !stderr.is_empty() { stderr } else { stdout };
        // pkexec без агента polkit (голые sway, niri и прочие тайловые его
        // не запускают): код 127 и своя фраза, слов «Not authorized» нет.
        let no_agent = ["No authentication agent found", "Агент аутентификации не найден"]
            .iter()
            .any(|n| err.contains(n));
        if err.contains("Not authorized") || code == 126 || no_agent {
            // Подсказка «поставьте руками»: обычно это готовая команда, а на
            // неизвестном семействе — объяснение, почему команды нет.
            // ⚠️ Починка при порте: оригинал строил её ДО ветвления и на
            // неизвестном семействе падал RuntimeError вместо любого из
            // объяснений ниже — включая случаи, где команда не нужна вовсе.
            let hint = |lead: &str| match install_cmd(family, packages) {
                Ok(argv) => format!("{}\n  sudo {}", lead, argv.join(" ")),
                Err(e) => e,
            };
            if steamos {
                return fail(match install_cmd(family, packages) {
                    Ok(argv) => trf(
                        // TRANSLATORS: %1 is a package manager command line.
                        "The administrator password wasn't confirmed.\n\
                         You can install from a terminal:\n  \
                         sudo steamos-readonly disable\n  \
                         sudo %1\n  \
                         sudo steamos-readonly enable",
                        &[&argv.join(" ")],
                    ),
                    Err(e) => format!(
                        "{}\n{}",
                        tr("The administrator password wasn't confirmed."),
                        e
                    ),
                });
            }
            if no_agent || !polkit_agent_running() {
                let or_install = hint(&tr("Or install from a terminal:"));
                // Путь — команда для терминала, не слово для человека: без
                // него нечего было бы запустить. Берём найденный на этой
                // машине: у одного и того же агента он свой в каждом
                // дистрибутиве ([`POLKIT_AGENT_PATHS`]).
                return fail(match installed_agent() {
                    Some(agent) => trf(
                        // TRANSLATORS: %1 is the path of the program to start, %2 a hint
                        // with a command to run in a terminal.
                        "The service that shows the password window isn't running.\n\n\
                         Start it yourself:\n  %1 &\n\n%2",
                        &[&agent, &or_install],
                    ),
                    None => trf(
                        // TRANSLATORS: %1 is a hint with a command to run in a terminal.
                        // polkit-gnome is a package name.
                        "The service that shows the password window isn't running.\n\n\
                         No such service is installed here. Install one for your desktop \
                         (for example, polkit-gnome) — after that Rexon will show here how \
                         to start it.\n\n%1",
                        &[&or_install],
                    ),
                });
            }
            return fail(trf(
                // TRANSLATORS: %1 is a hint with a command to run in a terminal.
                "The administrator password wasn't confirmed.\n\n\
                 Possible reasons:\n\
                 • the password window didn't appear;\n\
                 • the password was wrong;\n\
                 • the password window was closed.\n\n\
                 %1",
                &[&hint(&tr("Try installing from a terminal:"))],
            ));
        }
        // Конфликт с установленным (pacman с `--noconfirm` ответил «N» и ничего
        // не удалил) — объяснить, а не показывать сырой английский текст.
        let conflicts = parse_transaction_conflicts(&format!("{stdout}\n{stderr}"));
        if !conflicts.is_empty() {
            return fail(conflict_explanation(&conflicts));
        }
        return fail(if err.is_empty() {
            tr("Installation failed")
        } else {
            err.to_string()
        });
    }
    if steamos {
        return ok(vec![
            ("steamos_done".into(), tr("✅ Desktop installed on SteamOS")),
            (
                "steamos_warn".into(),
                tr("⚠️ A SteamOS update may remove these packages."),
            ),
            (
                "steamos_warn".into(),
                format!("   {}", tr("Tip: pin the packages with pacman -D --asexplicit")),
            ),
        ]);
    }
    ok(vec![(
        "done_main".into(),
        ntr(
            "✅ Main packages installed (%n package)",
            "✅ Main packages installed (%n packages)",
            packages.len() as u64,
        ),
    )])
}

/// Вступительная строка AUR-хвоста установки — порт начала `_batch_run_aur`.
pub fn batch_aur_intro(steamos: bool, count: usize) -> (String, String) {
    (
        "aur".to_string(),
        if steamos {
            trf("🎨 Installing from the AUR (%1)…", &[&count])
        } else {
            ntr(
                "🎨 Installing %n package from the AUR…",
                "🎨 Installing %n packages from the AUR…",
                count as u64,
            )
        },
    )
}

/// Итог AUR-хвоста: что сказать после попытки поставить пакеты из AUR.
///
/// `failed` / `installed` — разбор по живому `is_installed`, который делает
/// исполнитель; на SteamOS оригинал в этот разбор не ходит вовсе, поэтому
/// списки там не нужны.
///
/// `timed_out` — помощник AUR не уложился в отведённое время. Отказ по
/// времени ОБЯЗАН называть себя: «не установились» и «не дождались сборки»
/// — разные новости, и делать с ними человеку надо разное. 2026-09-01
/// покупатель прочитал «⚠️ Не установились AUR (9 из 9)» там, где на деле
/// сборку убили через полминуты.
pub fn batch_aur_result(
    steamos: bool,
    succeeded: bool,
    total: usize,
    failed: &[String],
    installed: &[String],
    timed_out: bool,
) -> Vec<(String, String)> {
    if steamos {
        return vec![if succeeded {
            ("aur_done".into(), tr("✅ AUR packages installed"))
        } else if timed_out {
            ("aur_warn".into(), aur_timeout_message(total))
        } else {
            ("aur_warn".into(), tr("⚠️ The AUR packages didn't install"))
        }];
    }
    if succeeded {
        return vec![("aur_done".into(), tr("✅ AUR packages installed"))];
    }
    if timed_out {
        let mut out = vec![("aur_warn".to_string(), aur_timeout_message(total))];
        if !installed.is_empty() {
            out.push((
                "aur_warn".to_string(),
                format!("   {}", trf("✅ Built in time: %1", &[&installed.join(", ")])),
            ));
        }
        return out;
    }
    if failed.is_empty() {
        // yay иногда отдаёт ненулевой код, поставив всё до единого.
        return vec![("aur_done".into(), tr("✅ AUR packages installed"))];
    }
    let mut out = vec![(
        "aur_warn".to_string(),
        trf(
            // TRANSLATORS: %1 of %2 packages failed; %3 lists them.
            "⚠️ Some AUR packages didn't install (%1 of %2): %3",
            &[&failed.len(), &total, &failed.join(", ")],
        ),
    )];
    if !installed.is_empty() {
        out.push((
            "aur_warn".to_string(),
            format!("   {}", fill(&trc("AUR packages", "✅ Installed: %1"), &[&installed.join(", ")])),
        ));
    }
    out
}

/// Отказ по времени — отдельным текстом, чтобы его нельзя было спутать с
/// «пакеты не встали». Человеку сразу говорим, чем продолжить вручную.
pub fn aur_timeout_message(total: usize) -> String {
    ntr(
        "⏳ The AUR build ran out of time (%n package) — this is NOT an \
         installation failure. To finish it yourself: yay -S --needed <packages>",
        "⏳ The AUR build ran out of time (%n packages) — this is NOT an \
         installation failure. To finish it yourself: yay -S --needed <packages>",
        total as u64,
    )
}

/// Отметка в окне прогресса о начале установки пакетов — порт двух строк
/// исполнителя плана (`📦 Установка …` и её вариант для SteamOS).
///
/// ⭐ Починка при переносе: обе показывали покупателю «пакет(ов)» и
/// «1 пакетов» вместо русского слова. Склонение в проекте уже было
/// (каталог перевода, `ntr`), эти две строки им просто не пользовались.
pub fn pkg_install_start_message(packages: usize, steamos: bool) -> String {
    let n = packages as u64;
    if steamos {
        ntr(
            "📦 SteamOS: installing %n package…",
            "📦 SteamOS: installing %n packages…",
            n,
        )
    } else {
        ntr("📦 Installing %n package…", "📦 Installing %n packages…", n)
    }
}

// ============================================================================
// Пост-установка MATE: данные и тексты (порт `_post_install_mate`)
// ============================================================================
//
// Доводка MATE до вида Ubuntu MATE — единственный пост-установочный хук в
// проекте. Сам он состоит из походов наружу (git clone, копирование файлов,
// gsettings, pkexec), но всё, ЧТО именно копируется и пишется, — это данные:
// подписи «Общих задач» на разных языках, карта snap-путей, содержимое
// gschema-оверрайда, тексты обоих привилегированных скриптов и списки
// пакетов, которых нет в обычных репозиториях. Данные живут здесь, походы —
// у исполнителя.

/// Подписи «Общих задач» Центра управления MATE по языку интерфейса.
/// Ключ сверяется префиксом переменной LANG, как в оригинале.
pub const MATE_LOCALE_ACTIONS: &[(&str, &str)] = &[
    (
        "ru",
        "['Изменить тему;mate-appearance-properties.desktop', \
'Предпочтительные приложения;mate-default-applications-properties.desktop', \
'Информация о системе MATE;mate-system-info.desktop']",
    ),
    (
        "de",
        "['Design ändern;mate-appearance-properties.desktop', \
'Bevorzugte Anwendungen;mate-default-applications-properties.desktop', \
'MATE-Systeminformation;mate-system-info.desktop']",
    ),
];

/// Язык интерфейса → (ключ локали, значение cc-actions-list), либо None.
pub fn mate_locale_actions(lang: &str) -> Option<(&'static str, &'static str)> {
    MATE_LOCALE_ACTIONS
        .iter()
        .find(|(k, _)| lang.starts_with(k))
        .map(|(k, v)| (*k, *v))
}

/// Ярлыки snap-сборок Ubuntu → их обычные имена на Arch/CachyOS.
pub const MATE_SNAP_MAP: &[(&str, &str)] = &[
    ("firefox_firefox.desktop", "firefox.desktop"),
    ("thunderbird_thunderbird.desktop", "thunderbird.desktop"),
    ("snap-store_snap-store.desktop", "org.gnome.Software.desktop"),
    (
        "snap-store_ubuntu-mate-welcome.desktop",
        "org.gnome.Software.desktop",
    ),
    ("software-properties-drivers.desktop", "cachyos-hello.desktop"),
];

/// Переписать snap-пути в тексте раскладки панели на обычные.
pub fn mate_layout_fix_snap_paths(text: &str) -> String {
    let mut out = text.to_string();
    for (snap, native) in MATE_SNAP_MAP {
        out = out.replace(
            &format!("/var/lib/snapd/desktop/applications/{}", snap),
            &format!("/usr/share/applications/{}", native),
        );
    }
    out
}

/// Куда ложится gschema-оверрайд Rexon.
pub const MATE_GSCHEMA_PATH: &str =
    "/usr/share/glib-2.0/schemas/90_rexon-mate-settings.gschema.override";

// ── Умолчания MATE строятся ТОЛЬКО из того, что реально лежит на диске ──
//
// ЗАЧЕМ. Оверрайд прописывал `Yaru-MATE-dark` темой по умолчанию ДО того,
// как пакет с темой пробовали поставить, — и не проверял результат. Пакета
// `yaru-mate-gtk-theme` в AUR не существует вовсе, установка падала, а стол
// оставался настроен на несуществующую тему (дефект 2026-09-01). Класс шире
// одной темы: в умолчания нельзя писать то, чего мы не поставили.
//
// Поэтому имя каждой темы выбирается из цепочки предпочтений по СПИСКУ
// РЕАЛЬНО ПРИСУТСТВУЮЩИХ (его собирает исполнитель, обходя каталоги тем), а
// про каждую подмену человеку говорится вслух.
//
// ⚠️ Marco отдельной цепочкой, и это не придирка: у темы `Yaru-MATE-dark`
// нет каталога `metacity-1` — оконному менеджеру она не тема вовсе, и
// прежний оверрайд просил Marco о том, чего не бывает даже после успешной
// установки.

/// Оформление GTK — по убыванию предпочтения.
pub const MATE_GTK_THEME_CHAIN: &[&str] = &[
    "Yaru-MATE-dark", "Yaru-MATE", "Yaru-dark", "Yaru",
    "BlackMATE", "Menta", "Adwaita-dark", "Adwaita",
];

/// Оформление рамок окон (Marco). Нужен каталог `metacity-1`.
pub const MATE_MARCO_THEME_CHAIN: &[&str] = &[
    "Yaru-MATE-dark", "Yaru-dark", "YaruOk", "YaruGreen",
    "BlackMATE", "Menta", "TraditionalOk", "Adwaita",
];

/// Набор значков.
pub const MATE_ICON_THEME_CHAIN: &[&str] = &[
    "Yaru-MATE-dark", "Yaru-MATE", "Yaru", "menta", "mate", "Adwaita",
];

/// Указатель мыши.
pub const MATE_CURSOR_THEME_CHAIN: &[&str] = &["Yaru", "mate", "Adwaita"];

/// Первое ПРИСУТСТВУЮЩЕЕ имя из цепочки предпочтений.
///
/// ЕДИНАЯ точка ответа на вопрос «что из желаемого у нас на самом деле
/// есть». Заведена вместо россыпи `if os.path.isdir(...)`: пропущенная
/// проверка оставляет умолчание, указывающее в пустоту, а человек видит
/// сломанный стол без единого слова о причине.
pub fn first_present<'a>(chain: &[&'a str], available: &[String]) -> Option<&'a str> {
    chain
        .iter()
        .find(|want| available.iter().any(|have| have == *want))
        .copied()
}

/// Что реально можно записать в умолчания MATE — собирается исполнителем
/// обходом каталогов ПОСЛЕ установки пакетов.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MateLookInput {
    /// Имена тем с каталогом `gtk-3.0`.
    pub gtk_themes: Vec<String>,
    /// Имена тем с каталогом `metacity-1` (их понимает Marco).
    pub marco_themes: Vec<String>,
    pub icon_themes: Vec<String>,
    pub cursor_themes: Vec<String>,
    /// Стоит ли семейство шрифтов Ubuntu.
    pub ubuntu_font: bool,
}

/// Проверенное оформление MATE + что пришлось заменить.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MateLook {
    pub gtk_theme: Option<String>,
    pub marco_theme: Option<String>,
    pub icon_theme: Option<String>,
    pub cursor_theme: Option<String>,
    pub ubuntu_font: bool,
    /// Строки для человека о каждой подмене и о каждом пропуске.
    pub notes: Vec<String>,
}

/// Выбрать оформление из наличного и объяснить каждую замену.
pub fn mate_look(input: &MateLookInput) -> MateLook {
    let mut notes = Vec::new();
    let mut pick = |what: String, chain: &[&str], available: &[String]| -> Option<String> {
        let want = chain[0];
        match first_present(chain, available) {
            Some(got) if got == want => Some(got.to_string()),
            Some(got) => {
                notes.push(format!(
                    "  {}",
                    // TRANSLATORS: %1 is a part of the look (theme, icons…),
                    // %2 the wanted theme, %3 the one kept instead.
                    trf("⚠️ %1: “%2” isn't installed — keeping “%3”", &[&what, &want, &got])
                ));
                Some(got.to_string())
            }
            None => {
                notes.push(format!(
                    "  {}",
                    trf(
                        "⚠️ %1: “%2” isn't installed and there's no replacement \
                         either — keeping the MATE default",
                        &[&what, &want],
                    )
                ));
                None
            }
        }
    };
    let gtk_theme = pick(trc("MATE look part", "Theme"), MATE_GTK_THEME_CHAIN, &input.gtk_themes);
    let marco_theme = pick(
        trc("MATE look part", "Window borders"),
        MATE_MARCO_THEME_CHAIN,
        &input.marco_themes,
    );
    let icon_theme = pick(trc("MATE look part", "Icons"), MATE_ICON_THEME_CHAIN, &input.icon_themes);
    let cursor_theme = pick(
        trc("MATE look part", "Pointer"),
        MATE_CURSOR_THEME_CHAIN,
        &input.cursor_themes,
    );
    if !input.ubuntu_font {
        notes.push(format!(
            "  {}",
            tr("⚠️ Fonts: the Ubuntu font family isn't installed — keeping the MATE font")
        ));
    }
    MateLook {
        gtk_theme,
        marco_theme,
        icon_theme,
        cursor_theme,
        ubuntu_font: input.ubuntu_font,
        notes,
    }
}

/// Настройки MATE, которые Rexon ставит пользователю: (схема, ключ,
/// значение) — ровно те, что прежде уезжали в общесистемный оверрайд схем.
///
/// ЗАЧЕМ ПОЛЬЗОВАТЕЛЮ, А НЕ В СИСТЕМУ. Прежде эти же значения ложились в
/// `/usr/share/glib-2.0/schemas/90_rexon-mate-settings.gschema.override` —
/// файл root'а, ничей по учёту пакетов. Он действовал на ВСЕ окружения и
/// всех людей за машиной, его нельзя было отменить сменой стола, и он
/// переживал удаление самого Rexon: человек снёс программу, а её след
/// оставался в системе навсегда. Настройка стола — про пользователя, а не
/// про машину, поэтому пишем в его же слой (`gsettings`), откуда любое
/// значение отменяется штатным `gsettings reset`.
///
/// `look` — уже ПРОВЕРЕННОЕ оформление: имён несуществующих тем здесь не
/// появляется вовсе.
pub fn mate_user_settings(look: &MateLook) -> Vec<(String, String, String)> {
    let mut out: Vec<(String, String, String)> = Vec::new();
    let mut put = |schema: &str, key: &str, value: &str| {
        out.push((schema.to_string(), key.to_string(), value.to_string()));
    };
    if let Some(t) = &look.gtk_theme {
        put("org.mate.interface", "gtk-theme", t);
    }
    if let Some(t) = &look.icon_theme {
        put("org.mate.interface", "icon-theme", t);
    }
    if look.ubuntu_font {
        put("org.mate.interface", "font-name", "Ubuntu 11");
        put("org.mate.interface", "document-font-name", "Ubuntu 11");
        put("org.mate.interface", "monospace-font-name", "Ubuntu Mono 13");
    }
    put(
        "org.mate.interface",
        "gtk-decoration-layout",
        "close,minimize,maximize:",
    );
    if let Some(t) = &look.marco_theme {
        put("org.mate.Marco.general", "theme", t);
    }
    if look.ubuntu_font {
        put("org.mate.Marco.general", "titlebar-font", "Ubuntu Bold 11");
    }
    put("org.mate.Marco.general", "compositing-manager", "true");
    put("org.mate.Marco.general", "center-new-windows", "false");
    put("org.mate.Marco.general", "allow-tiling", "true");
    if let Some(t) = &look.cursor_theme {
        put("org.mate.peripherals-mouse", "cursor-theme", t);
        put("org.mate.peripherals-mouse", "cursor-size", "24");
    }
    put("org.mate.background", "show-desktop-icons", "true");
    put("org.mate.caja.desktop", "computer-icon-visible", "true");
    put("org.mate.caja.desktop", "home-icon-visible", "true");
    put("org.mate.caja.desktop", "trash-icon-visible", "true");
    put("org.mate.panel", "default-layout", "familiar");
    put(
        "org.mate.session",
        "required-components-list",
        "['windowmanager', 'panel', 'filemanager']",
    );
    put("org.mate.session.required-components", "windowmanager", "marco");
    put("org.mate.session.required-components", "panel", "mate-panel");
    put("org.mate.session.required-components", "filemanager", "caja");
    put("org.mate.NotificationDaemon", "theme", "slider");
    if look.ubuntu_font {
        put("org.mate.pluma", "editor-font", "Ubuntu Mono 13");
        put("org.mate.pluma", "use-default-font", "false");
        put(
            "org.mate.terminal.profile:/org/mate/terminal/profiles/default/",
            "font",
            "Ubuntu Mono 13",
        );
        put(
            "org.mate.terminal.profile:/org/mate/terminal/profiles/default/",
            "use-system-font",
            "false",
        );
    }
    out
}

/// argv вызовов `gsettings set` для `mate_user_settings`.
pub fn mate_user_settings_argv(look: &MateLook) -> Vec<Vec<String>> {
    mate_user_settings(look)
        .into_iter()
        .map(|(schema, key, value)| {
            vec!["gsettings".to_string(), "set".to_string(), schema, key, value]
        })
        .collect()
}

/// Привилегированная уборка ЧУЖОГО следа прошлых версий: общесистемный
/// оверрайд схем, который Rexon клал до перехода на пользовательский слой.
///
/// Оставлять его нельзя даже безвредным: файл ничей по учёту пакетов, он
/// объявляет умолчания для всей машины и переживает удаление продукта.
pub fn mate_gschema_cleanup_script() -> String {
    format!(
        "#!/bin/bash\nset -e\nrm -f '{}'\n\
glib-compile-schemas /usr/share/glib-2.0/schemas/\n",
        MATE_GSCHEMA_PATH
    )
}

/// Сырой (без named-действия) скрипт раскладки файлов панели в системный
/// каталог. Все пути экранируются — имена файлов приходят из чужого
/// репозитория и под root подставлять их в bash как есть нельзя.
pub fn mate_layouts_stage_script(dst: &str, staged_files: &[String]) -> String {
    let qdst = shlex_quote(dst);
    let mut lines = vec![
        "#!/bin/bash".to_string(),
        "set -e".to_string(),
        format!("install -d {}", qdst),
    ];
    for f in staged_files {
        lines.push(format!(
            "cp -f {} {}",
            shlex_quote(f),
            shlex_quote(&format!("{}/", dst))
        ));
    }
    lines.push(format!("chmod -R a+rX {}", qdst));
    format!("{}\n", lines.join("\n"))
}

/// Пакеты «сверх дистрибутива», которых нет в обычных репозиториях:
/// (метка для журнала ошибок, заголовок шага — `msgid` каталога, переводится
/// в месте показа, — список пакетов).
pub const MATE_EXTRA_PACKAGES: &[(&str, &str, &[&str])] = &[
    // ⛔ Прежние имена `yaru-mate-gtk-theme` / `yaru-mate-icon-theme` НЕ
    // существуют — ни в AUR, ни в репозиториях (проверено RPC 2026-09-01,
    // resultcount 0). Установка падала каждый раз, а оверрайд при этом уже
    // объявил `Yaru-MATE-dark` темой по умолчанию. Настоящие поставщики тем
    // Ubuntu MATE в AUR — `ubuntu-mate-themes` (даёт /usr/share/themes/
    // Yaru-MATE, Yaru-MATE-dark, Yaru-MATE-light) и `ubuntu-mate-icon-themes`
    // (даёт Yaru, Yaru-MATE-dark, Yaru-MATE-light). Значки Yaru-MATE
    // наследуются от Humanity — без `humanity-icon-theme` набор почти пуст.
    (
        "yaru-mate",
        n_("🎨 Checking the Ubuntu MATE themes (Yaru-MATE)…"),
        &["ubuntu-mate-themes", "ubuntu-mate-icon-themes", "humanity-icon-theme"],
    ),
    (
        "ayatana",
        n_("🔔 Checking the Ayatana indicator stack…"),
        &[
            "ayatana-indicator-application",
            "ayatana-indicator-session",
            "ayatana-indicator-datetime",
            "ayatana-indicator-sound",
            "ayatana-indicator-notifications",
        ],
    ),
    (
        "vala-panel-appmenu",
        n_("📋 Checking vala-panel-appmenu (global menu)…"),
        // Не общий `vala-panel-appmenu` — он тянет budgie-desktop и vala-panel.
        &["vala-panel-appmenu-mate", "vala-panel-appmenu-registrar"],
    ),
];

/// Строки журнала о записанном оформлении — перечисляют то, что
/// действительно записано, а не то, что хотелось записать.
pub fn mate_gschema_success_lines(look: &MateLook) -> Vec<String> {
    let mut out = vec![format!(
        "  {}",
        tr("✅ The MATE look was saved to your user settings:")
    )];
    let mut say = |label: &str, value: &Option<String>| {
        if let Some(v) = value {
            out.push(format!("     • {label}: {v}"));
        }
    };
    say("GTK", &look.gtk_theme);
    say(&trc("MATE look summary", "Icons"), &look.icon_theme);
    if let Some(c) = &look.cursor_theme {
        out.push(format!("     • {}", trf("Cursor: %1 (24px)", &[&c])));
    }
    if look.ubuntu_font {
        out.push(format!("     • {}", tr("Font: Ubuntu 11 / Ubuntu Mono 13")));
    }
    if let Some(m) = &look.marco_theme {
        out.push(format!("     • {}", trf("WM: Marco with compositing, theme %1", &[&m])));
    } else {
        out.push(format!("     • {}", tr("WM: Marco with compositing")));
    }
    out.push(format!("     • {}", tr("Default layout: Familiar")));
    out
}

/// Итог пост-установки MATE: строки журнала и текст результата.
pub fn mate_post_install_summary(errors: usize) -> (Vec<String>, String) {
    if errors > 0 {
        return (
            vec![format!(
                "\n{}",
                ntr(
                    "⚠️ MATE post-install: %n warning (not critical)",
                    "⚠️ MATE post-install: %n warnings (not critical)",
                    errors as u64,
                )
            )],
            ntr("Partly done (%n warning)", "Partly done (%n warnings)", errors as u64),
        );
    }
    (
        vec![
            format!("\n{}", tr("✅ MATE post-install complete — the full Ubuntu MATE!")),
            format!("   → {}", tr("8 layouts: mate-tweak → Panel")),
            "   → Familiar | Redmond | Cupertino | Mutiny".into(),
            "   → Pantheon | Contemporary | Netbook | Traditional".into(),
            format!("   → {}", tr("Yaru-MATE theme + Ubuntu fonts + Ayatana")),
            "   → CLI: mate-tweak --layout redmond".into(),
        ],
        tr("MATE post-install complete"),
    )
}

// ============================================================================
// Пост-установка river: стартовый init
// ============================================================================
//
// river-classic без исполняемого `$XDG_CONFIG_HOME/river/init` стартует
// БЕЗ ЕДИНОЙ привязки клавиш: ни терминала, ни выхода — только Ctrl+Alt+F1…F12.
// Пример init river-classic кладёт в /usr/share как образец и сам его не
// читает. Поэтому после установки Rexon кладёт свой стартовый init — но только
// если файла ещё нет: чужие (и свои прежние) настройки не трогаются никогда.

/// Где river ищет init: `$XDG_CONFIG_HOME/river/init`, без него —
/// `$HOME/.config/river/init` (river(1), раздел CONFIGURATION).
pub fn river_init_path(xdg_config_home: &str, home: &str) -> String {
    let base = if xdg_config_home.is_empty() {
        format!("{}/.config", home.trim_end_matches('/'))
    } else {
        xdg_config_home.trim_end_matches('/').to_string()
    };
    format!("{base}/river/init")
}

/// Права стартового init: river запускает его как программу.
pub const RIVER_INIT_MODE: u32 = 0o755;

/// Стартовый init river: клавиши как у соседних тайловых (Super+Enter —
/// терминал, Super+D — меню программ, Super+Q — закрыть окно), панель
/// waybar, уведомления mako, окно пароля polkit и раскладка rivertile.
/// Программы — ровно те, что ставит запись `river` базы столов.
///
/// `XDG_CURRENT_DESKTOP` river-classic сам не выставляет, а по нему
/// xdg-desktop-portal выбирает портал (wlr: `UseIn=…;river;…`) и Rexon
/// узнаёт стол. Поэтому имя кладётся и в окружение служб пользователя.
pub fn river_starter_init() -> &'static str {
    r#"#!/bin/sh
# Starter river setup written by Rexon. It is yours now: change it freely,
# Rexon never overwrites an existing init. Reference: riverctl(1), rivertile(1).

export XDG_CURRENT_DESKTOP=river
dbus-update-activation-environment --systemd WAYLAND_DISPLAY XDG_CURRENT_DESKTOP=river 2>/dev/null

# Super+Enter: terminal, Super+D: applications, Super+Q: close the window,
# Super+Shift+E: log out.
riverctl map normal Super Return spawn foot
riverctl map normal Super D spawn fuzzel
riverctl map normal Super Q close
riverctl map normal Super+Shift E exit

riverctl map normal Super J focus-view next
riverctl map normal Super K focus-view previous
riverctl map normal Super+Shift J swap next
riverctl map normal Super+Shift K swap previous
riverctl map normal Super+Shift Return zoom
riverctl map normal Super H send-layout-cmd rivertile "main-ratio -0.05"
riverctl map normal Super L send-layout-cmd rivertile "main-ratio +0.05"
riverctl map normal Super Space toggle-float
riverctl map normal Super F toggle-fullscreen
riverctl map-pointer normal Super BTN_LEFT move-view
riverctl map-pointer normal Super BTN_RIGHT resize-view

# Super+1..9: show a tag, Super+Shift+1..9: move the window there,
# Super+0: show everything.
for i in 1 2 3 4 5 6 7 8 9; do
    tags=$((1 << (i - 1)))
    riverctl map normal Super "$i" set-focused-tags "$tags"
    riverctl map normal Super+Shift "$i" set-view-tags "$tags"
done
riverctl map normal Super 0 set-focused-tags $(((1 << 32) - 1))

for mode in normal locked; do
    riverctl map $mode None XF86AudioRaiseVolume spawn 'wpctl set-volume -l 1 @DEFAULT_AUDIO_SINK@ 5%+'
    riverctl map $mode None XF86AudioLowerVolume spawn 'wpctl set-volume @DEFAULT_AUDIO_SINK@ 5%-'
    riverctl map $mode None XF86AudioMute spawn 'wpctl set-mute @DEFAULT_AUDIO_SINK@ toggle'
done

riverctl set-repeat 50 300
riverctl background-color 0x1e1e2e
riverctl border-color-focused 0x89b4fa
riverctl border-color-unfocused 0x45475a

riverctl spawn waybar
riverctl spawn mako
for agent in /usr/lib/polkit-gnome/polkit-gnome-authentication-agent-1 \
             /usr/libexec/polkit-gnome-authentication-agent-1; do
    if [ -x "$agent" ]; then
        riverctl spawn "$agent"
        break
    fi
done

riverctl default-layout rivertile
rivertile -view-padding 6 -outer-padding 6 &
"#
}

/// Строка журнала: стартовый init river записан.
pub fn river_init_written(path: &str) -> String {
    // TRANSLATORS: %1 is a file path.
    trf("✅ River: starter settings written (%1) — Super+Enter opens a terminal, Super+D the app menu", &[&path])
}

/// Строка журнала: init river уже был — оставлен как есть.
pub fn river_init_kept(path: &str) -> String {
    // TRANSLATORS: %1 is a file path.
    trf("ℹ️ River: your own settings are kept (%1)", &[&path])
}

/// Строка журнала: записать стартовый init river не вышло.
pub fn river_init_failed(path: &str, why: &str) -> String {
    // TRANSLATORS: %1 is a file path, %2 the reason.
    trf("⚠️ River: couldn't write the starter settings (%1): %2. Without them River starts with no keyboard shortcuts.", &[&path, &why])
}

#[cfg(test)]
mod river_init_tests {
    use super::*;

    #[test]
    fn init_path_follows_xdg_config_home() {
        assert_eq!(river_init_path("", "/home/t"), "/home/t/.config/river/init");
        assert_eq!(river_init_path("", "/home/t/"), "/home/t/.config/river/init");
        assert_eq!(river_init_path("/x/cfg/", "/home/t"), "/x/cfg/river/init");
    }

    /// Стартовый init зовёт только то, что ставит запись `river` базы
    /// столов (на Arch), и `rivertile`/`riverctl` из самого river-classic.
    #[test]
    fn starter_init_uses_only_installed_programs() {
        let init = river_starter_init();
        assert!(init.starts_with("#!/bin/sh\n"));
        let arch = crate::system::get_packages("river", PkgFamily::Arch);
        for program in ["foot", "fuzzel", "waybar", "mako", "polkit-gnome"] {
            assert!(init.contains(program.split('-').next().unwrap()), "{program}");
            assert!(arch.iter().any(|p| p == program), "{program} не ставится с River");
        }
        assert!(arch.iter().any(|p| p == "river-classic"));
        assert!(init.contains("riverctl default-layout rivertile"));
        assert!(init.contains("export XDG_CURRENT_DESKTOP=river"));
        // Выход из сеанса есть всегда — иначе из River не выбраться.
        assert!(init.contains("Super+Shift E exit"));
    }
}

/// Тексты и команда шага «доустановить пакеты сверх дистрибутива»
/// (порт `_ensure_extra_packages`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ExtraPkgTexts {
    /// Не Arch: собирать эти пакеты больше негде — честно говорим и уходим.
    pub not_arch: String,
    pub already: String,
    pub no_helper: String,
    pub no_helper_error: String,
    pub installing: String,
    pub done: String,
    pub failed: String,
    pub failed_error: String,
    /// Отдельный текст на случай, когда сборка не уложилась во время.
    pub failed_timeout: String,
    pub failed_timeout_error: String,
    /// Команда помощника AUR (пусто, если помощника нет).
    pub argv: Vec<String>,
    pub timeout_secs: u64,
}

/// Собрать всё, что шаг доустановки может сказать и запустить.
pub fn extra_packages_texts(
    tag: &str,
    missing: &[String],
    helper: Option<&str>,
) -> ExtraPkgTexts {
    let list = missing.join(", ");
    let mut argv = Vec::new();
    if let Some(h) = helper {
        argv = crate::aur_install::install_argv(h, missing, false);
    }
    ExtraPkgTexts {
        not_arch: format!(
            "  {}",
            tr("ℹ️ These add-ons are only built for Arch/CachyOS for now — \
                skipping this step on this system")
        ),
        already: format!("  {}", tr("✅ Already installed")),
        no_helper: format!(
            "  {}",
            trf("⚠️ No AUR helper found — install these yourself: %1", &[&list])
        ),
        no_helper_error: format!("no aur helper for {}", tag),
        installing: format!("  {}", trf("📥 Installing: %1…", &[&list])),
        done: format!("  {}", trc("add-on install step", "✅ Installed")),
        failed: format!("  {}", tr("⚠️ Couldn't install")),
        failed_error: format!("{} install failed", tag),
        failed_timeout: format!(
            "  {}",
            trf(
                "⏳ The build ran out of time — this is NOT an installation \
                 failure. To finish it yourself: yay -S --needed %1",
                &[&list],
            )
        ),
        failed_timeout_error: format!("{} install timed out", tag),
        argv,
        // Сборка из исходников — класс `build`: 1800 с не хватало уже на
        // среднем наборе, а время здесь задаёт машина, а не мы.
        timeout_secs: crate::utils::BUILD_TIMEOUT_SECS,
    }
}

#[cfg(test)]
mod helper_path_tests {
    use super::*;

    /// Константы ручной установки обязаны быть тем же, что склеивает
    /// [`helper_path_in`] из имени и каталога: иначе правда о пути снова
    /// раздвоится, и политика разойдётся с argv.
    #[test]
    fn константы_ручной_установки_склеиваются_из_имени() {
        assert_eq!(DE_HELPER_PATH, helper_path_in(HELPER_DIR_SYSTEM, DE_HELPER_NAME));
        assert_eq!(PKG_HELPER_PATH, helper_path_in(HELPER_DIR_SYSTEM, PKG_HELPER_NAME));
        assert_eq!(
            STAGE_HELPER_PATH,
            helper_path_in(HELPER_DIR_SYSTEM, STAGE_HELPER_NAME)
        );
    }

    /// Пакет предпочитается ручной установке, а без файлов ответ —
    /// путь ручной установки (определённый, а не пустой).
    #[test]
    fn выбор_пути_помощника() {
        let pkg = helper_path_in(HELPER_DIR_PACKAGE, PKG_HELPER_NAME);
        assert_eq!(
            helper_candidates(PKG_HELPER_NAME),
            vec![pkg.clone(), PKG_HELPER_PATH.to_string()]
        );
        assert_eq!(pick_helper_path(PKG_HELPER_NAME, &|_| true), pkg);
        assert_eq!(
            pick_helper_path(PKG_HELPER_NAME, &|p| p == PKG_HELPER_PATH),
            PKG_HELPER_PATH
        );
        assert_eq!(pick_helper_path(PKG_HELPER_NAME, &|_| false), PKG_HELPER_PATH);
    }
}


#[cfg(test)]
mod no_polkit_agent_tests {
    use super::*;

    /// Нет агента polkit (голые sway и niri его сами не запускают): pkexec
    /// отвечает кодом 127 и «No authentication agent found» — без слов «Not
    /// authorized». Прежде человек получал сырую строку pkexec вместо
    /// объяснения, что окна пароля нет и как его поднять. Проверка `pgrep`
    /// тут не помощник: под шаблон `polkitd` попадает системная служба,
    /// которая работает всегда.
    #[test]
    fn missing_agent_is_explained() {
        let pkgs = vec!["noctalia".to_string()];
        for err in [
            "Error executing command as another user: No authentication agent found.\n",
            "Ошибка выполнения команды от имени другого пользователя: Агент аутентификации не найден.\n",
        ] {
            let out =
                batch_main_result(Some((127, err, "")), true, &pkgs, false, PkgFamily::Arch, &|| true, &|| None);
            assert!(!out.ok);
            assert!(
                out.message.starts_with("The service that shows the password window isn't running."),
                "{}",
                out.message
            );
            assert!(out.message.contains("sudo pacman"), "{}", out.message);
        }
    }
}

// ============================================================================
// Укус привилегированного помощника: имя пакета против пути
// ============================================================================
//
// ⛔ Эти сверки стоят на месте настоящей дыры (25.09.2026): помощник
// `org.rexon.pkg` принимал `/` в имени пакета, брал семейство пакетного
// менеджера из аргумента и собирал команду через `eval`. Строка
// `pkexec rexon-pkg-helper debian install /tmp/evil.deb` давала исполнение
// чужого кода ОТ ROOT на любой машине с apt-get/dnf/zypper. Ослабить любую
// из проверок ниже — значит открыть её заново.

#[cfg(test)]
mod pkg_arg_security_tests {
    use super::*;

    /// Всё, чем можно подсунуть путь или команду вместо имени пакета.
    const ATTACKS: &[&str] = &[
        "/tmp/evil.deb",
        "/tmp/evil.rpm",
        "/tmp/evil",
        "./evil.deb",
        "evil.deb",
        "evil.rpm",
        "evil.pkg.tar.zst",
        "../../etc/passwd",
        "..",
        "../mate",
        "mate/../../tmp/evil",
        "a/b/c",
        ".hidden",
        "-Rns",
        "--root=/tmp",
        "mate;rm -rf /",
        "mate&&id",
        "mate|id",
        "$(id)",
        "`id`",
        "${PATH}",
        "mate linux",
        "mate\nlinux",
        "mate\tlinux",
        "'mate'",
        "\"mate\"",
        "",
    ];

    /// Наборы Portage: `emerge --unmerge @world` снёс бы систему целиком.
    /// Отдельным списком, потому что ведущее `@` законно РОВНО в одном
    /// месте — группа dnf на Fedora (`@gnome-desktop`).
    const SET_ATTACKS: &[&str] = &["@world", "@system", "@installed", "@profile"];

    /// Слэш посреди имени: относительный путь. Законен РОВНО у Portage —
    /// это его атом `категория/имя` (`x11-misc/sddm`), и там он проверяется
    /// половина за половиной.
    const SLASH_ATTACKS: &[&str] = &["a/b", "usr/bin/id", "tmp/evil"];

    /// Ни одно семейство не принимает путь, файл пакета, флаг или
    /// метасимвол — ни в allowlist'е, ни в таблице команд, ни в argv
    /// привилегированного помощника.
    ///
    /// Таблицы команд проверяются отдельно от помощника намеренно: отказ
    /// помощника означал бы «откат на сырой `pkexec apt-get install …`», а
    /// это та же дыра без помощника.
    #[test]
    fn путь_вместо_имени_пакета_отвергнут() {
        let families = [
            PkgFamily::Arch, PkgFamily::Debian, PkgFamily::Fedora,
            PkgFamily::Suse, PkgFamily::Gentoo, PkgFamily::Alpine,
            PkgFamily::Void, PkgFamily::Solus, PkgFamily::Clear,
            PkgFamily::Alt, PkgFamily::Nix,
        ];
        for family in families {
            for bad in ATTACKS {
                let pkgs = vec![(*bad).to_string()];
                assert!(
                    !is_safe_pkg_arg(family, bad),
                    "{}: allowlist пропустил {bad:?}", family.as_str()
                );
                assert!(
                    install_cmd(family, &pkgs).is_err(),
                    "{}: install_cmd собрал команду на {bad:?}", family.as_str()
                );
                assert!(
                    remove_cmd(family, &pkgs).is_err(),
                    "{}: remove_cmd собрал команду на {bad:?}", family.as_str()
                );
                assert!(
                    privileged_pkg_cmd(family, "install", &pkgs, true).is_none(),
                    "{}: помощник позван с {bad:?}", family.as_str()
                );
                assert!(
                    privileged_pkg_cmd(family, "remove", &pkgs, true).is_none(),
                    "{}: помощник позван с {bad:?}", family.as_str()
                );
                // Честное имя рядом с подложенным не отмывает его.
                let mixed = vec!["mate".to_string(), (*bad).to_string()];
                assert!(install_cmd(family, &mixed).is_err(), "{bad:?} в хвосте");
            }
            for bad in SLASH_ATTACKS {
                if family == PkgFamily::Gentoo {
                    continue; // атом Portage — единственное законное '/'
                }
                let pkgs = vec![(*bad).to_string()];
                assert!(
                    !is_safe_pkg_arg(family, bad),
                    "{}: относительный путь {bad:?} принят за имя", family.as_str()
                );
                assert!(install_cmd(family, &pkgs).is_err(), "{bad:?} в установке");
                assert!(
                    privileged_pkg_cmd(family, "install", &pkgs, true).is_none(),
                    "{}: помощник позван с путём {bad:?}", family.as_str()
                );
            }
            for bad in SET_ATTACKS {
                if family == PkgFamily::Fedora {
                    continue; // у dnf `@имя` — группа пакетов, и только она
                }
                let pkgs = vec![(*bad).to_string()];
                assert!(
                    !is_safe_pkg_arg(family, bad),
                    "{}: набор {bad:?} принят за имя пакета", family.as_str()
                );
                assert!(remove_cmd(family, &pkgs).is_err(), "{bad:?} в удалении");
                assert!(
                    privileged_pkg_cmd(family, "remove", &pkgs, true).is_none(),
                    "{}: помощник позван с набором {bad:?}", family.as_str()
                );
            }
        }
    }

    /// Обратная половина: честные имена проходят КАК БЫЛИ — сверка не о
    /// строгости, а о том, что продукт остался рабочим.
    #[test]
    fn честные_имена_пакетов_проходят() {
        let cases: &[(PkgFamily, &[&str])] = &[
            (PkgFamily::Arch, &[
                "plasma-desktop", "qt6-base", "python-pyside6", "xorg-server",
                "lib32-mesa", "ttf-dejavu", "gcc-libs", "libstdc++5", "g++",
                "sddm", "mate", "mate-extra", "cinnamon-translations",
            ]),
            (PkgFamily::Debian, &[
                "plasma-desktop", "qt6-base-dev", "python3-pyside6",
                "libstdc++6", "libc6:i386", "xfce4", "lightdm",
            ]),
            (PkgFamily::Fedora, &[
                "@gnome-desktop", "@kde-desktop-environment", "qt6-qtbase",
                "lightdm-gtk-greeter",
            ]),
            (PkgFamily::Gentoo, &[
                "gnome-extra/cinnamon", "dev-lang/python:3.12",
                "x11-misc/sddm", "kde-plasma/plasma-meta", "mate-base/mate",
            ]),
            (PkgFamily::Suse, &["plasma6-desktop", "libQt6Core6"]),
            (PkgFamily::Alt, &["plasma5-desktop", "libstdc++6"]),
        ];
        for (family, names) in cases {
            for n in *names {
                assert!(
                    is_safe_pkg_arg(*family, n),
                    "{}: честное имя {n:?} отвергнуто", family.as_str()
                );
            }
            let pkgs: Vec<String> = names.iter().map(|s| (*s).to_string()).collect();
            let cmd = install_cmd(*family, &pkgs)
                .unwrap_or_else(|e| panic!("{}: {e}", family.as_str()));
            for n in *names {
                assert!(
                    cmd.iter().any(|a| a == n),
                    "{}: имя {n:?} не попало в команду целиком", family.as_str()
                );
            }
            // Жизненно важное (`gcc-libs`, `lib32-mesa`) сырой путь удаления
            // не снимет; остальное — снимет.
            let removable: Vec<String> =
                pkgs.iter().filter(|p| !is_vital_package(p)).cloned().collect();
            remove_cmd(*family, &removable).expect("удаление честных имён");
            if removable.len() != pkgs.len() {
                assert!(remove_cmd(*family, &pkgs).is_err(), "вечное в сыром удалении");
            }
            if PKG_HELPER_FAMILIES.contains(&family.as_str()) {
                let argv = privileged_pkg_cmd(*family, "install", &pkgs, true)
                    .expect("помощник берёт честные имена");
                for n in *names {
                    assert!(
                        argv.iter().any(|a| a == n),
                        "{}: помощник потерял {n:?}", family.as_str()
                    );
                }
            }
        }
    }

    /// Пакеты рабочих столов Rexon и вечный список не пересекаются: значит
    /// запрет на удаление жизненно важного не ломает ни одного честного
    /// сценария (единственный, кто зовёт `remove`, — удаление стола).
    #[test]
    fn вечный_список_не_пересекается_с_рабочими_столами() {
        for (de_id, packages) in crate::system::de_database_packages() {
            for (_fam, pkgs) in packages {
                for p in *pkgs {
                    assert!(
                        !PKG_HELPER_VITAL.contains(p),
                        "{de_id}: {p} и в столе, и в вечном списке"
                    );
                }
            }
        }
        for dm in DE_HELPER_DISPLAY_MANAGERS {
            assert!(!PKG_HELPER_VITAL.contains(dm), "экран входа {dm} в вечном списке");
        }
    }

    /// Текст помощников: ни `eval`, ни семейства из аргумента, ни прежнего
    /// allowlist'а с `/`.
    #[test]
    fn помощники_без_eval_и_без_семейства_из_аргумента() {
        let pkg = pkg_helper_script();
        let de = de_helper_script();
        // Смотрим КОД, а не комментарии: и `eval`, и прежний allowlist
        // названы в шапках помощников как раз затем, чтобы их не вернули.
        let code = |s: &str| -> String {
            s.lines()
                .filter(|l| !l.trim_start().starts_with('#'))
                .collect::<Vec<_>>()
                .join("\n")
        };
        for (name, s) in [("pkg", &pkg), ("de", &de)] {
            let body = code(s);
            assert!(
                !body.contains("eval"),
                "{name}-helper снова собирает команду через eval"
            );
            assert!(
                !body.contains("[A-Za-z0-9@._+:/-]"),
                "{name}-helper вернул allowlist с '/' в имени пакета"
            );
        }
        // Семейство помощник определяет САМ — по той же таблице, что ядро.
        assert!(pkg.contains("detect_family"), "нет детекции семейства");
        assert!(
            pkg.contains("family=\"$(detect_family"),
            "семейство снова берётся не из детекции"
        );
        assert!(
            pkg.contains("claimed=\"$1\""),
            "argv помощника изменился без нужды"
        );
        for (id, fam) in crate::system::ID_MAP {
            assert!(pkg.contains(id), "ID {id} потерян в детекции помощника");
            assert!(pkg.contains(fam.as_str()), "семейство {} потеряно", fam.as_str());
        }
        // Команда — массив аргументов, а не строка для оболочки.
        for f in PKG_HELPER_FAMILIES {
            assert!(
                pkg.contains(&format!("{f}) cmd=(")),
                "команда {f} не массивом"
            );
        }
        assert!(de.contains("PKG_CMD=("), "de-helper: команда не массивом");
        assert!(de.contains("UPD_CMD=("), "de-helper: обновление не массивом");
        // Имя пакета проверяется тем же правилом, что в ядре.
        assert!(
            pkg.contains("'^[A-Za-z0-9][A-Za-z0-9._+:-]*$'"),
            "в помощнике нет проверки атома имени"
        );
        assert!(pkg.contains("*.deb|*.rpm"), "помощник берёт файл пакета");
        for v in PKG_HELPER_VITAL {
            assert!(pkg.contains(v), "вечный список потерял {v}");
        }
    }

    /// ⭐ УКУС ЖИВЬЁМ: помощник запускается системным bash и обязан
    /// ОТКАЗАТЬ на каждой попытке подсунуть путь. root здесь не нужен и не
    /// участвует: отказ случается до любого вызова пакетного менеджера, а
    /// честный прогон уходит в подставные бинарники из своего каталога.
    #[test]
    #[cfg_attr(feature = "isolated-checks", ignore = "нужен системный bash")]
    fn помощник_живьём_отказывает_на_пути() {
        use std::io::Write;
        use std::process::Command;

        let dir = std::env::temp_dir()
            .join(format!("rexon_pkg_helper_stand_{}", std::process::id()));
        let stub = dir.join("stub");
        std::fs::create_dir_all(&stub).expect("каталог стенда");
        // Подставные менеджеры пакетов: печатают argv и ничего не делают.
        // Настоящий менеджер в этой сверке не запускается ни разу.
        for bin in ["pacman", "apt-get", "dpkg", "rpm", "dnf", "yum", "zypper",
                    "emerge", "apk", "xbps-install", "eopkg", "swupd",
                    "systemctl"] {
            let p = stub.join(bin);
            let mut f = std::fs::File::create(&p).expect("подставной бинарь");
            writeln!(f, "#!/bin/bash\necho \"STUB $(basename \"$0\") $*\"")
                .expect("тело подставного бинаря");
            drop(f);
            set_exec(&p);
        }
        // Утилиты, которыми помощник разбирает os-release и имена.
        for tool in ["sed", "grep", "head", "tr", "basename", "bash", "cat"] {
            for root in ["/usr/bin", "/bin", "/usr/sbin", "/sbin"] {
                let from = std::path::Path::new(root).join(tool);
                if from.exists() {
                    let _ = std::os::unix::fs::symlink(&from, stub.join(tool));
                    break;
                }
            }
        }
        // Копия помощника со ОДНОЙ правкой: PATH стенда вместо системного.
        // Правка обязана примениться — иначе сверка молча мерила бы не то.
        // Та же строка есть и у сценария отката экрана входа
        // (`DM_FALLBACK_SCRIPT`): помощник встраивает его целиком и по
        // `switch-dm` пишет в `DM_FALLBACK_SCRIPT_PATH`. Подменяется только
        // СВОЯ строка помощника — первая, до встроенного сценария; у отката
        // PATH остаётся системным.
        let src = pkg_helper_script();
        let want = "export PATH=/usr/sbin:/usr/bin:/sbin:/bin";
        assert_eq!(PKG_HELPER_SCRIPT_TMPL.matches(want).count(), 1, "строка PATH помощника изменилась");
        assert_eq!(DM_FALLBACK_SCRIPT.matches(want).count(), 1, "строка PATH сценария отката изменилась");
        assert_eq!(src.matches(want).count(), 2, "строка PATH встречается не там, где ждали");
        let own = src.find(want).expect("строка PATH помощника");
        let embedded = src.find(DM_FALLBACK_SCRIPT.trim_end()).expect("встроенный сценарий отката");
        assert!(own < embedded, "первая строка PATH — не помощника, а сценария отката");
        let script = src.replacen(want, &format!("export PATH={}", stub.display()), 1);
        let path = dir.join("rexon-pkg-helper");
        std::fs::write(&path, &script).expect("запись помощника");
        set_exec(&path);

        let run = |args: &[&str]| -> (i32, String, String) {
            let out = Command::new("bash")
                .arg(&path)
                .args(args)
                .output()
                .expect("запуск помощника");
            (
                out.status.code().unwrap_or(-1),
                String::from_utf8_lossy(&out.stdout).to_string(),
                String::from_utf8_lossy(&out.stderr).to_string(),
            )
        };

        // 1. Путь, файл пакета, обход каталогов, метасимволы, флаг — отказ 2,
        //    и ни одного STUB в выводе (менеджер не запускался).
        for (op, arg) in [
            ("install", "/tmp/evil.deb"),
            ("install", "/tmp/evil.rpm"),
            ("install", "./evil.deb"),
            ("install", "evil.deb"),
            ("install", "../../etc/passwd"),
            ("install", "mate/../../tmp/evil"),
            ("install", "mate;rm -rf /"),
            ("install", "$(id)"),
            ("install", "`id`"),
            ("install", "mate linux"),
            ("install", "-Rns"),
            ("install", "@world"),
            ("remove", "/tmp/evil.deb"),
            ("remove", "linux"),
            ("remove", "systemd"),
            ("remove", "polkit"),
            ("remove", "pacman"),
        ] {
            for claimed in ["arch", "debian", "fedora", "suse", "gentoo"] {
                let (code, out, err) = run(&[claimed, op, arg]);
                assert_eq!(
                    code, 2,
                    "{claimed} {op} {arg:?}: помощник не отказал ({err})"
                );
                assert!(
                    !out.contains("STUB"),
                    "{claimed} {op} {arg:?}: менеджер пакетов всё-таки позван: {out}"
                );
                assert!(
                    err.contains("rexon-pkg-helper:"),
                    "{claimed} {op} {arg:?}: отказ без объяснения"
                );
            }
        }

        // 2. Честные имена (из таблиц ядра — белый список установки с
        //    28.10.2026) проходят и уходят ОТДЕЛЬНЫМИ аргументами.
        let (code, out, err) =
            run(&["arch", "install", "konsole", "sddm", "mpv"]);
        if err.contains("unsupported family") {
            // NixOS и незнакомые машины: помощник честно отказывается, и это
            // ровно то, что должно быть, — сверять argv тогда не на чем.
            assert_eq!(code, 2);
        } else {
            assert_eq!(code, 0, "честные имена отвергнуты: {err}");
            assert!(out.starts_with("STUB "), "менеджер не позван: {out}");
            assert!(
                out.trim_end()
                    .ends_with(" konsole sddm mpv"),
                "имена попали в команду не отдельными аргументами: {out}"
            );
        }

        // 3. Соврать про семейство больше нельзя: выбирается определённое по
        //    машине, а не то, что назвал вызывающий.
        let (_, out_true, _) = run(&["arch", "update"]);
        let (_, out_lie, _) = run(&["debian", "update"]);
        assert_eq!(
            out_true, out_lie,
            "семейство из аргумента всё ещё меняет команду"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    fn set_exec(p: &std::path::Path) {
        use std::os::unix::fs::PermissionsExt;
        let mut perm = std::fs::metadata(p).expect("права").permissions();
        perm.set_mode(0o755);
        std::fs::set_permissions(p, perm).expect("бит запуска");
    }
}

// ============================================================================
// «Зонтик» polkit: одно окно пароля на четыре помощника (М1, 28.09.2026)
// ============================================================================

#[cfg(test)]
mod imply_umbrella_tests {
    use super::*;
    use crate::installer::{system_policy_xml, system_policy_xml_for, SYSTEM_INSTALL_ACTION};

    const IMPLY_KEY: &str = "org.freedesktop.policykit.imply";

    /// Все политики Rexon: (действие, XML ручной установки, XML пакета).
    fn policies() -> Vec<(&'static str, String, String)> {
        let pkg_dir = |name| helper_path_in(HELPER_DIR_PACKAGE, name);
        vec![
            (DE_INSTALL_ACTION, de_policy_xml(), de_policy_xml_for(&pkg_dir(DE_HELPER_NAME))),
            (PKG_ACTION, pkg_policy_xml(), pkg_policy_xml_for(&pkg_dir(PKG_HELPER_NAME))),
            (STAGE_ACTION, stage_policy_xml(), stage_policy_xml_for(&pkg_dir(STAGE_HELPER_NAME))),
            (
                SYSTEM_INSTALL_ACTION,
                system_policy_xml(),
                system_policy_xml_for(&pkg_dir(crate::installer::SYSTEM_HELPER_NAME)),
            ),
        ]
    }

    /// `org.rexon.pkg` покрывает три остальных действия — и в пакете, и в
    /// ручной установке.
    #[test]
    fn pkg_policy_carries_the_umbrella() {
        let want = format!(
            "<annotate key=\"{IMPLY_KEY}\">{DE_INSTALL_ACTION} {STAGE_ACTION} \
             {SYSTEM_INSTALL_ACTION}</annotate>"
        );
        for (action, manual, package) in policies() {
            if action == PKG_ACTION {
                assert!(manual.contains(&want), "{manual}");
                assert!(package.contains(&want), "{package}");
            }
        }
        assert!(!PKG_IMPLIES.contains(&PKG_ACTION));
    }

    /// ⛔ Направление `imply` одно: от действия, которое никогда не бывает
    /// «без пароля», к остальным. `imply` у действия из правила «без
    /// пароля» сделал бы БЕЗ пароля и всё, что оно покрывает, — для любого
    /// процесса сеанса. Поэтому аннотация есть ровно у одной политики, и эта
    /// политика в правило не входит.
    #[test]
    fn imply_umbrella_never_hangs_on_passwordless_action() {
        assert!(!PASSWORDLESS_ACTIONS.contains(&PKG_ACTION));
        for (action, manual, package) in policies() {
            let carries = manual.contains(IMPLY_KEY) || package.contains(IMPLY_KEY);
            assert_eq!(carries, action == PKG_ACTION, "imply у {action}");
            if carries {
                assert!(
                    !PASSWORDLESS_ACTIONS.contains(&action),
                    "{action} с imply стоит в правиле «без пароля»"
                );
            }
        }
    }

    /// Решение Wehrwolfmann 28.09.2026: темы экрана входа и загрузки и фон
    /// экрана входа из правила «без пароля» сняты.
    #[test]
    fn passwordless_rule_no_longer_covers_login_and_boot_screens() {
        assert_eq!(PASSWORDLESS_ACTIONS, &[DE_INSTALL_ACTION]);
        for family in [PkgFamily::Arch, PkgFamily::Debian] {
            let rule = passwordless_rules_js(family);
            for gone in [SYSTEM_INSTALL_ACTION, STAGE_ACTION, PKG_ACTION] {
                assert!(!rule.contains(&format!("\"{gone}\"")), "{gone} в правиле:\n{rule}");
            }
        }
    }
}

// ============================================================================
// Действие сеанса прав org.rexon.session (Б11)
// ============================================================================

#[cfg(test)]
mod session_action_tests {
    use super::*;
    use crate::installer::{system_policy_xml, SYSTEM_INSTALL_ACTION};

    const IMPLY_KEY: &str = "org.freedesktop.policykit.imply";

    /// Значение элемента `<tag>…</tag>` (первое вхождение).
    fn elem<'a>(xml: &'a str, tag: &str) -> &'a str {
        let open = format!("<{tag}>");
        let at = xml.find(&open).unwrap_or_else(|| panic!("нет <{tag}>")) + open.len();
        &xml[at..at + xml[at..].find('<').unwrap()]
    }

    /// ⛔ Пароль на КАЖДЫЙ подъём сеанса: `auth_admin` во всех трёх полях,
    /// без `_keep` (его держит помощник, а не polkit), путь — тот, по
    /// которому помощник лежит, окно пароля в графике.
    #[test]
    fn session_policy_asks_password_every_time_and_points_to_helper() {
        let pkg = helper_path_in(HELPER_DIR_PACKAGE, SESSION_HELPER_NAME);
        assert_eq!(pkg, "/usr/lib/rexon-priv-helper");
        assert_eq!(SESSION_HELPER_PATH, helper_path_in(HELPER_DIR_SYSTEM, SESSION_HELPER_NAME));
        assert_eq!(SESSION_POLICY_PATH, format!("/usr/share/polkit-1/actions/{SESSION_ACTION}.policy"));
        for (path, xml) in [(SESSION_HELPER_PATH.to_string(), session_policy_xml()), (pkg.clone(), session_policy_xml_for(&pkg))] {
            assert!(xml.contains(&format!("<action id=\"{SESSION_ACTION}\">")), "{xml}");
            for tag in ["allow_any", "allow_inactive", "allow_active"] {
                assert_eq!(elem(&xml, tag), "auth_admin", "{tag}:\n{xml}");
            }
            assert!(!xml.contains("auth_admin_keep"), "{xml}");
            assert!(!xml.contains(">yes<"), "{xml}");
            assert!(xml.contains(&format!(
                "<annotate key=\"org.freedesktop.policykit.exec.path\">{path}</annotate>"
            )));
            assert!(xml.contains("<annotate key=\"org.freedesktop.policykit.exec.allow_gui\">true</annotate>"));
            assert!(xml.contains("<description>") && xml.contains("<message>"));
            assert_eq!(xml.matches("<action ").count(), 1);
        }
    }

    /// ⛔ Ни одной тихой дороги к сеансу: действия нет в правиле «без
    /// пароля», его не покрывает ничей `imply`, и само оно ничего не `imply`.
    #[test]
    fn session_action_is_not_passwordless_and_not_implied() {
        assert!(!PASSWORDLESS_ACTIONS.contains(&SESSION_ACTION));
        assert!(!PKG_IMPLIES.contains(&SESSION_ACTION));
        for family in [PkgFamily::Arch, PkgFamily::Debian, PkgFamily::Fedora, PkgFamily::Unknown] {
            let rule = passwordless_rules_js(family);
            assert!(!rule.contains(SESSION_ACTION), "{SESSION_ACTION} в правиле:\n{rule}");
        }
        for xml in [de_policy_xml(), pkg_policy_xml(), stage_policy_xml(), system_policy_xml()] {
            assert!(!xml.contains(SESSION_ACTION), "чужая политика упоминает {SESSION_ACTION}:\n{xml}");
        }
        assert!(!session_policy_xml().contains(IMPLY_KEY));
        // И обратно: сеанс не тянет за собой прежние действия.
        let own = session_policy_xml();
        for other in [DE_INSTALL_ACTION, PKG_ACTION, STAGE_ACTION, SYSTEM_INSTALL_ACTION] {
            assert!(!own.contains(other), "{other} в политике сеанса");
        }
    }

    /// Подписи окна пароля переведены на языки каталога тем же механизмом,
    /// что у остальных политик (`xml:lang`).
    #[test]
    fn session_policy_texts_are_translated() {
        let xml = session_policy_xml();
        for code in ["ru", "de"] {
            if crate::system_l10n::languages().contains(&code) {
                assert!(xml.contains(&format!("<description xml:lang=\"{code}\">")), "{code}:\n{xml}");
                assert!(xml.contains(&format!("<message xml:lang=\"{code}\">")), "{code}:\n{xml}");
            }
        }
    }
}

// ============================================================================
// Журнал помощника установки стола пишет не root (28.09.2026)
// ============================================================================

#[cfg(test)]
mod de_helper_log_tests {
    use super::*;
    use std::process::Command;

    /// Помощник до выбора пакетов, затем отметка и код 3. Гоняется от
    /// обычного пользователя с подменным `setpriv` (без прав root он и
    /// нужен-то только как «запусти остальное»).
    fn run_log_part(tag: &str, log_path: &str) -> (i32, String) {
        let full = de_helper_script();
        let cut = full.find("# Пакеты для пары").expect("раздел пакетов в помощнике");
        let script = format!("{}echo REXON_BODY\nexit 3\n", &full[..cut]);
        let dir = std::env::temp_dir().join(format!("rexon-de-log-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let stub = dir.join("setpriv");
        std::fs::write(&stub, "#!/bin/bash\nwhile [ \"$1\" != \"--\" ]; do shift; done; shift; exec \"$@\"\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
        let uid = String::from_utf8(Command::new("id").arg("-u").output().unwrap().stdout).unwrap();
        let path = format!("{}:{}", dir.display(), std::env::var("PATH").unwrap_or_default());
        let out = Command::new("bash")
            .arg("-c")
            .arg(script)
            .arg("rexon-de-helper")
            .args(["plasma", "arch", log_path])
            .env("PATH", path)
            .env("PKEXEC_UID", uid.trim())
            .output()
            .unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        (out.status.code().unwrap_or(-1), String::from_utf8_lossy(&out.stderr).into_owned())
    }

    #[test]
    fn журнал_пишет_вызвавший_а_не_root() {
        let log = format!("/tmp/rexon-de-log-test-{}.log", std::process::id());
        std::fs::write(&log, "").unwrap();
        let (code, err) = run_log_part("own", &log);
        let text = std::fs::read_to_string(&log).unwrap();
        let _ = std::fs::remove_file(&log);
        assert_eq!(code, 3, "{err}");
        assert!(text.contains("REXON_BODY"), "{text}");
        assert!(text.trim_end().ends_with("__REXON_EXIT__3"), "{text}");
    }

    #[test]
    fn журнал_вне_tmp_отвергнут() {
        let (code, err) = run_log_part("etc", "/etc/shadow");
        assert_eq!(code, 2, "{err}");
        assert!(err.contains("outside /tmp"), "{err}");
    }

    /// ⛔ Ни chown, ни записи в файл пользователя от root: владелец файла по
    /// строке пути — то, что пользователь успевает подменить.
    #[test]
    fn помощник_стола_не_меняет_владельца_и_не_пишет_журнал_от_root() {
        let text = de_helper_script();
        for line in text.lines().map(str::trim_start).filter(|l| !l.starts_with('#')) {
            assert!(!line.contains("chown"), "chown в помощнике установки стола: {line}");
        }
        assert!(!text.contains("exec >>"), "запись журнала от root");
        assert!(text.contains("setpriv --reuid=\"$log_uid\""));
    }
}

// ============================================================================
// Помощник раскладки берёт свою копию каталога (28.09.2026)
// ============================================================================

#[cfg(test)]
mod stage_helper_copy_tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    fn dir(tag: &str) -> PathBuf {
        let d = PathBuf::from("/tmp").join(format!("rexon-stage-test-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn uid() -> String {
        String::from_utf8(Command::new("id").arg("-u").output().unwrap().stdout)
            .unwrap()
            .trim()
            .to_string()
    }

    /// Помощник до раскладки по категориям, затем отметка. От обычного
    /// пользователя — в систему ничего не пишется.
    fn run(staged: &Path, pkexec_uid: &str) -> (bool, String) {
        let full = stage_helper_script();
        let cut = full.find("# ── Обработка по category").expect("раздел раскладки");
        let script = format!(
            "{}[ -f \"$real_staged/bg.png\" ] && echo REXON_VALIDATED\nexit 0\n",
            &full[..cut]
        );
        let out = Command::new("bash")
            .arg("-c")
            .arg(script)
            .arg("rexon-stage-helper")
            .args(["gdm-bg", "rexon", &staged.to_string_lossy()])
            .env("PKEXEC_UID", pkexec_uid)
            .output()
            .unwrap();
        let ok = out.status.success() && String::from_utf8_lossy(&out.stdout).contains("REXON_VALIDATED");
        (ok, String::from_utf8_lossy(&out.stderr).into_owned())
    }

    #[test]
    fn свой_каталог_проходит_копией() {
        let d = dir("good");
        std::fs::write(d.join("bg.png"), b"\x89PNG").unwrap();
        let (ok, err) = run(&d, &uid());
        assert!(ok, "{err}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn ссылка_чужой_и_внешний_каталог_отвергнуты() {
        let d = dir("bad");
        std::fs::write(d.join("bg.png"), b"\x89PNG").unwrap();
        symlink("/etc/shadow", d.join("shadow.png")).unwrap();
        let (ok, err) = run(&d, &uid());
        assert!(!ok && err.contains("link"), "{err}");
        std::fs::remove_file(d.join("shadow.png")).unwrap();
        let (ok, err) = run(&d, "0");
        assert!(!ok && err.contains("does not belong to the caller"), "{err}");
        let link = PathBuf::from("/tmp").join(format!("rexon-stage-test-lnk-{}", std::process::id()));
        let _ = std::fs::remove_file(&link);
        symlink("/usr", &link).unwrap();
        let (ok, err) = run(&link, &uid());
        assert!(!ok && err.contains("outside /tmp"), "{err}");
        let _ = std::fs::remove_file(&link);
        let _ = std::fs::remove_dir_all(&d);
    }
}

// ============================================================================
// Помощник пакетов: шаги пересборки (build-deps, install-built), 28.10.2026
// ============================================================================

/// Помощник целиком, от обычного пользователя, с подставными `pacman`,
/// `getent` и `chown` (настоящий pacman не зовётся ни разу), своим
/// os-release (`ID=arch`) и своим рабочим каталогом вместо `/var/tmp`.
#[cfg(test)]
mod pkg_helper_rebuild_tests {
    use super::*;
    use std::os::unix::fs::{symlink, PermissionsExt};
    use std::path::{Path, PathBuf};
    use std::process::Command;

    struct Stand {
        root: PathBuf,
        home: PathBuf,
        script: PathBuf,
    }

    impl Drop for Stand {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn uid() -> String {
        String::from_utf8(Command::new("id").arg("-u").output().unwrap().stdout)
            .unwrap()
            .trim()
            .to_string()
    }

    fn exec(path: &Path, body: &str) {
        std::fs::write(path, body).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn stand(tag: &str) -> Stand {
        let root = std::env::temp_dir().join(format!("rexon-pkg-rebuild-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let stub = root.join("stub");
        let home = root.join("home");
        let work = root.join("work");
        for d in [&stub, &home.join(".cache/rexon/rebuild/foo"), &work] {
            std::fs::create_dir_all(d).unwrap();
        }
        // pacman: печатает argv; у `-U` — содержимое и права полученного
        // файла (это должна быть копия помощника, не файл пользователя).
        // База пакетов — два файла стенда: всё установленное и «зависимости».
        // `-S` дописывает поставленное в обе (как `--asdeps`).
        exec(
            &stub.join("pacman"),
            "#!/bin/bash\n\
             case \"$1\" in -Qq) cat \"$FAKE_DB\"; exit 0 ;; -Qqd) cat \"$FAKE_DEPS\"; exit 0 ;; esac\n\
             echo \"STUB pacman $*\"\n\
             if [ \"$1\" = -S ]; then while [ \"$1\" != -- ]; do shift; done; shift; \
             for a in \"$@\"; do n=\"${a%%[<>=]*}\"; echo \"$n\" >> \"$FAKE_DB\"; echo \"$n\" >> \"$FAKE_DEPS\"; done; \
             exit \"${FAKE_S_RC:-0}\"; fi\n\
             if [ \"$1\" = -U ]; then f=\"${@: -1}\"; echo \"GOT $(cat \"$f\") $(stat -c %a \"$f\")\"; \
             exit \"${FAKE_U_RC:-0}\"; fi\n\
             if [ \"$1\" = -Rns ]; then exit \"${FAKE_R_RC:-0}\"; fi\nexit 0\n",
        );
        std::fs::write(root.join("db"), "bash\nfirefox\ngimp\nlinux\n").unwrap();
        std::fs::write(root.join("deps"), "firefox\n").unwrap();
        exec(&stub.join("getent"), "#!/bin/bash\necho \"u:x:$3:$3::$FAKE_HOME:/bin/bash\"\n");
        exec(&stub.join("chown"), "#!/bin/bash\nexit 0\n");
        let osr = root.join("os-release");
        std::fs::write(&osr, "ID=arch\n").unwrap();

        let mut text = pkg_helper_script();
        for (from, to) in [
            ("export PATH=/usr/sbin:/usr/bin:/sbin:/bin", format!("export PATH={}:/usr/bin:/bin", stub.display())),
            ("for f in /etc/os-release /usr/lib/os-release; do", format!("for f in {}; do", osr.display())),
            ("mktemp -d /var/tmp/rexon-pkg-helper.XXXXXXXX", format!("mktemp -d {}/h.XXXXXXXX", work.display())),
        ] {
            assert!(text.contains(from), "строка помощника изменилась: {from}");
            text = text.replacen(from, &to, 1);
        }
        // Каталог записей build-deps — везде (он и в комментарии шапки).
        assert!(text.contains(&format!("REC_DIR=\"{REBUILD_REC_DIR}\"")));
        text = text.replace(REBUILD_REC_DIR, &root.join("rec").display().to_string());
        let script = root.join("rexon-pkg-helper");
        exec(&script, &text);
        Stand { root, home, script }
    }

    impl Stand {
        fn rebuild(&self) -> PathBuf {
            self.home.join(".cache/rexon/rebuild/foo")
        }

        fn run_env(&self, args: &[&str], pkexec_uid: Option<&str>, env: &[(&str, &str)]) -> (i32, String, String) {
            let mut cmd = Command::new("bash");
            cmd.arg(&self.script)
                .args(args)
                .env("FAKE_HOME", &self.home)
                .env("FAKE_DB", self.root.join("db"))
                .env("FAKE_DEPS", self.root.join("deps"))
                .env_remove("PKEXEC_UID");
            if let Some(u) = pkexec_uid {
                cmd.env("PKEXEC_UID", u);
            }
            for (k, v) in env {
                cmd.env(k, v);
            }
            let out = cmd.output().unwrap();
            (
                out.status.code().unwrap_or(-1),
                String::from_utf8_lossy(&out.stdout).into_owned(),
                String::from_utf8_lossy(&out.stderr).into_owned(),
            )
        }

        fn run(&self, args: &[&str]) -> (i32, String, String) {
            self.run_env(args, Some(&uid()), &[])
        }

        fn record(&self) -> Option<String> {
            std::fs::read_to_string(self.root.join("rec").join(uid())).ok()
        }

        fn work_is_clean(&self) -> bool {
            std::fs::read_dir(self.root.join("work")).unwrap().next().is_none()
        }
    }

    #[test]
    #[cfg_attr(feature = "isolated-checks", ignore = "нужен системный bash")]
    fn свой_собранный_файл_ставится_копией_и_сборочные_снимаются() {
        let st = stand("ok");
        let file = st.rebuild().join("foo-1:2.0-1-x86_64.pkg.tar.zst");
        std::fs::write(&file, "PKG").unwrap();
        let f = file.to_string_lossy().into_owned();
        let (code, _, err) = st.run(&["arch", "build-deps", "vala", "base-devel"]);
        assert_eq!(code, 0, "{err}");
        assert_eq!(st.record().as_deref(), Some("base-devel\nvala\n"), "запись build-deps");
        let (code, out, err) = st.run(&["arch", "install-built", "plain", &f, "vala", "base-devel"]);
        assert_eq!(code, 0, "{err}");
        let u = out.lines().find(|l| l.starts_with("STUB pacman -U")).expect("pacman -U");
        assert!(u.starts_with("STUB pacman -U --noconfirm -- "), "{u}");
        assert!(!u.contains("--ask") && !u.contains(&f), "pacman получил файл пользователя: {u}");
        assert!(out.contains("GOT PKG 600"), "{out}");
        assert!(out.contains("STUB pacman -Rns --noconfirm -- vala base-devel"), "{out}");
        assert!(st.work_is_clean(), "своя копия не убрана");
        assert_eq!(st.record(), None, "запись build-deps не стёрта");

        let (code, out, _) = st.run(&["arch", "install-built", "replace", &f]);
        assert_eq!(code, 0);
        assert!(out.contains("STUB pacman -U --noconfirm --ask=4 -- "), "{out}");
        assert!(!out.contains("-Rns"), "{out}");

        // Сборочные не снялись — не ошибка, но вслух.
        st.run(&["arch", "build-deps", "gperf"]);
        let (code, out, err) = st.run_env(&["arch", "install-built", "plain", &f, "gperf"], Some(&uid()), &[("FAKE_R_RC", "1")]);
        assert_eq!(code, 0, "{err}");
        assert!(out.contains("build-only packages kept"), "{out}");
        // Не встал — код pacman, сборочные не трогаются.
        let (code, out, _) = st.run_env(&["arch", "install-built", "plain", &f, "vala"], Some(&uid()), &[("FAKE_U_RC", "1")]);
        assert_eq!(code, 1);
        assert!(!out.contains("-Rns"), "{out}");
    }

    /// Ничего, кроме своего файла makepkg из своего каталога пересборок:
    /// каждый отказ — код 2 и ни одного вызова pacman.
    #[test]
    #[cfg_attr(feature = "isolated-checks", ignore = "нужен системный bash")]
    fn чужой_внешний_ссылка_и_не_пакет_отвергнуты() {
        let st = stand("bad");
        let good = st.rebuild().join("foo-1-1-x86_64.pkg.tar.zst");
        std::fs::write(&good, "PKG").unwrap();
        // Снаружи каталога пересборок.
        let outside = st.root.join("foo-1-1-x86_64.pkg.tar.zst");
        std::fs::write(&outside, "EVIL").unwrap();
        // Ссылка внутри каталога на файл снаружи.
        let link = st.rebuild().join("bar-1-1-x86_64.pkg.tar.zst");
        symlink(&outside, &link).unwrap();
        // Ссылка-каталог внутри корня пересборок, ведущая наружу.
        let evil_dir = st.root.join("evil");
        std::fs::create_dir_all(&evil_dir).unwrap();
        std::fs::write(evil_dir.join("foo-1-1-x86_64.pkg.tar.zst"), "EVIL").unwrap();
        symlink(&evil_dir, st.home.join(".cache/rexon/rebuild/evil")).unwrap();
        // Не файл пакета по имени.
        for name in ["foo.tar.gz", "foo.sh", "foo.pkg.tar.zst.sig", ".foo-1-1-x86_64.pkg.tar.zst"] {
            std::fs::write(st.rebuild().join(name), "EVIL").unwrap();
        }
        // Каталог с именем пакета.
        std::fs::create_dir_all(st.rebuild().join("dir-1-1-x86_64.pkg.tar.zst")).unwrap();

        let p = |path: &Path| path.to_string_lossy().into_owned();
        let rb = p(&st.rebuild());
        let cases: Vec<(Vec<String>, &str)> = vec![
            (vec![p(&outside)], "outside"),
            (vec![p(&link)], "link"),
            (vec![format!("{}/evil/foo-1-1-x86_64.pkg.tar.zst", p(&st.home.join(".cache/rexon/rebuild")))], "outside"),
            (vec![format!("{rb}/foo.tar.gz")], "makepkg"),
            (vec![format!("{rb}/foo.sh")], "makepkg"),
            (vec![format!("{rb}/foo.pkg.tar.zst.sig")], "makepkg"),
            (vec![format!("{rb}/.foo-1-1-x86_64.pkg.tar.zst")], "makepkg"),
            (vec![format!("{rb}/dir-1-1-x86_64.pkg.tar.zst")], "not a regular file"),
            (vec!["foo-1-1-x86_64.pkg.tar.zst".into()], "outside"),
            (vec!["/usr/pacman-7.0.0-1-x86_64.pkg.tar.zst".into()], "outside"),
            (vec![format!("{rb}/../foo/foo-1-1-x86_64.pkg.tar.zst"), "linux".into()], "vital"),
            (vec![p(&good), "/tmp/x.pkg.tar.zst".into()], "not a path"),
            (vec![p(&good), "-dd".into()], "'-'"),
        ];
        for (rest, why) in &cases {
            let mut args = vec!["arch", "install-built", "plain"];
            args.extend(rest.iter().map(String::as_str));
            let (code, out, err) = st.run(&args);
            assert_eq!(code, 2, "{rest:?}: не отказал ({err})");
            assert!(!out.contains("STUB"), "{rest:?}: pacman позван: {out}");
            assert!(err.contains(why), "{rest:?}: ждали «{why}»: {err}");
        }
        // Режим только из двух слов.
        let (code, out, _) = st.run(&["arch", "install-built", "--ask=4", &p(&good)]);
        assert_eq!(code, 2);
        assert!(!out.contains("STUB"));
        // Не через pkexec и чужой вызвавший (каталог не его).
        let (code, out, err) = st.run_env(&["arch", "install-built", "plain", &p(&good)], None, &[]);
        assert!(code == 2 && !out.contains("STUB") && err.contains("pkexec"), "{err}");
        let other = (uid().parse::<u64>().unwrap() + 1).to_string();
        let (code, out, err) = st.run_env(&["arch", "install-built", "plain", &p(&good)], Some(&other), &[]);
        assert!(code == 2 && !out.contains("STUB") && err.contains("outside"), "{err}");
        assert!(st.work_is_clean());
    }

    /// ⛔ Через install-built не снять посторонний пакет: только то, что
    /// поставил build-deps ЭТОГО вызвавшего, и только с пометкой
    /// «зависимость». Без записи — не снимается ничего.
    #[test]
    #[cfg_attr(feature = "isolated-checks", ignore = "нужен системный bash")]
    fn посторонний_пакет_не_снимается() {
        let st = stand("foreign");
        let file = st.rebuild().join("foo-1-1-x86_64.pkg.tar.zst");
        std::fs::write(&file, "PKG").unwrap();
        let f = file.to_string_lossy().into_owned();
        // Записи нет вовсе: firefox (установлен, даже «зависимость») остаётся.
        let (code, out, err) = st.run(&["arch", "install-built", "plain", &f, "firefox", "gimp"]);
        assert_eq!(code, 0, "{err}");
        assert!(out.contains("STUB pacman -U"), "{out}");
        assert!(!out.contains("-Rns"), "снято без записи: {out}");
        assert!(out.contains("firefox was not installed for this build"), "{out}");
        // Запись есть, но в ней только vala: посторонние не снимаются.
        st.run(&["arch", "build-deps", "vala"]);
        let (code, out, _) = st.run(&["arch", "install-built", "plain", &f, "vala", "firefox", "gimp"]);
        assert_eq!(code, 0);
        assert!(out.contains("STUB pacman -Rns --noconfirm -- vala\n"), "{out}");
        // Поставлено build-deps, но пометку «зависимость» сняли — остаётся.
        st.run(&["arch", "build-deps", "meson"]);
        std::fs::write(st.root.join("deps"), "firefox\n").unwrap();
        let (_, out, _) = st.run(&["arch", "install-built", "plain", &f, "meson"]);
        assert!(!out.contains("-Rns"), "{out}");
        // Чужая запись (другой вызвавший) не годится.
        st.run(&["arch", "build-deps", "ninja"]);
        let rec = st.root.join("rec");
        std::fs::rename(rec.join(uid()), rec.join("999999")).unwrap();
        let (_, out, _) = st.run(&["arch", "install-built", "plain", &f, "ninja"]);
        assert!(!out.contains("-Rns"), "{out}");
    }

    #[test]
    #[cfg_attr(feature = "isolated-checks", ignore = "нужен системный bash")]
    fn сборочные_зависимости_только_по_именам() {
        let st = stand("deps");
        let (code, out, err) = st.run(&["arch", "build-deps", "cmake>=3.20", "vala", "libfoo.so=1-64", "base-devel"]);
        assert_eq!(code, 0, "{err}");
        assert_eq!(
            out.trim(),
            "STUB pacman -S --needed --noconfirm --asdeps -- cmake>=3.20 vala libfoo.so=1-64 base-devel"
        );
        for bad in ["/tmp/x.pkg.tar.zst", "x.pkg.tar.zst", "-dd", "cmake>=$(id)", "cmake>=", "a>=1 b", "../x", ">=1"] {
            let (code, out, _) = st.run(&["arch", "build-deps", bad]);
            assert_eq!(code, 2, "{bad}");
            assert!(!out.contains("STUB"), "{bad}: {out}");
        }
        let (code, out, _) = st.run(&["arch", "build-deps"]);
        assert!(code == 2 && !out.contains("STUB"));
    }

    /// Не Arch — обе операции отказывают сразу.
    #[test]
    #[cfg_attr(feature = "isolated-checks", ignore = "нужен системный bash")]
    fn шаги_пересборки_только_на_arch() {
        let st = stand("deb");
        std::fs::write(st.root.join("os-release"), "ID=debian\n").unwrap();
        for args in [["debian", "build-deps", "vala"], ["arch", "install-built", "plain"]] {
            let (code, out, err) = st.run(&args);
            assert_eq!(code, 2, "{args:?}");
            assert!(!out.contains("STUB"));
            assert!(err.contains("only with pacman"), "{err}");
        }
    }
}

#[cfg(test)]
mod pacman_lock_path_tests {
    use super::*;

    const DEFAULT_LOCK: &str = "/var/lib/pacman/db.lck";

    #[test]
    fn usual_dbpath() {
        assert_eq!(pacman_lock_path(Some("/var/lib/pacman/\n")), DEFAULT_LOCK);
    }

    #[test]
    fn own_dbpath() {
        assert_eq!(pacman_lock_path(Some("/srv/pacdb/\n")), "/srv/pacdb/db.lck");
    }

    #[test]
    fn dbpath_without_trailing_slash() {
        assert_eq!(pacman_lock_path(Some("/srv/pacdb\n")), "/srv/pacdb/db.lck");
        assert_eq!(pacman_lock_path(Some("  /srv/pacdb  ")), "/srv/pacdb/db.lck");
    }

    #[test]
    fn empty_or_missing_output_falls_back() {
        assert_eq!(pacman_lock_path(None), DEFAULT_LOCK);
        assert_eq!(pacman_lock_path(Some("")), DEFAULT_LOCK);
        assert_eq!(pacman_lock_path(Some("\n  \n")), DEFAULT_LOCK);
    }

    #[test]
    fn garbage_falls_back() {
        for junk in [
            "error: config file /etc/pacman.conf could not be read: No such file\n",
            "var/lib/pacman/\n",
            "/a/\n/b/\n",
            "/srv/pa\u{1b}[0mcdb/\n",
            "DBPath = /srv/pacdb/\n",
        ] {
            assert_eq!(pacman_lock_path(Some(junk)), DEFAULT_LOCK, "{junk:?}");
        }
    }

    #[test]
    fn other_families_keep_static_paths() {
        for f in [PkgFamily::Debian, PkgFamily::Fedora, PkgFamily::Suse, PkgFamily::Unknown] {
            let want: Vec<String> = lock_paths(f).iter().map(|s| s.to_string()).collect();
            assert_eq!(lock_paths_live(f), want, "{}", f.as_str());
        }
    }
}

#[cfg(test)]
mod install_preview_tests {
    use super::*;

    /// Как pacman делит отказ по потокам (замер 08.10.2026): «error:» — в
    /// stderr, перечень неразрешимых зависимостей — в stdout.
    const ERR_ONLINE_ACCOUNTS: &str = "error: failed to prepare transaction (could not satisfy dependencies)\n";
    const OUT_ONLINE_ACCOUNTS: &str = ":: unable to satisfy dependency 'libcamel-1.2.so=67-64' required by switchboard-plug-online-accounts\n\
        :: unable to satisfy dependency 'libedataserver-1.2.so=27-64' required by switchboard-plug-online-accounts\n";

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    /// Подставной pacman по записанному 08.10.2026 поведению: группа
    /// `pantheon` (повтор из двух репозиториев), сломанный член группы.
    fn pantheon(cmd: &Cmd) -> PreviewAnswer {
        let a = &cmd.argv;
        if a[1] == "-Sg" {
            return Some((
                1,
                "pantheon gala\npantheon switchboard-plug-online-accounts\npantheon wingpanel\n\
                 pantheon gala\npantheon wingpanel\n"
                    .into(),
                "error: package group 'lightdm' was not found\n".into(),
            ));
        }
        if a[1] == "-Qq" {
            return Some((0, "glibc\n".into(), String::new()));
        }
        if a[1] == "-Si" || a[1] == "-Qi" {
            // Сверка конфликтов: у этого набора их нет.
            return Some((0, String::new(), String::new()));
        }
        assert_eq!(&a[..6], &names(&["pacman", "-Sp", "--needed", "--noconfirm", "--print-format", "%n"])[..]);
        assert!(cmd.lang_c, "вывод pacman разбирается только под LANG=C");
        if a.iter().any(|x| x == "pantheon" || x == "switchboard-plug-online-accounts") {
            return Some((1, OUT_ONLINE_ACCOUNTS.into(), ERR_ONLINE_ACCOUNTS.into()));
        }
        Some((0, "gala\nwingpanel\nmutter\nlightdm\n".into(), String::new()))
    }

    fn conflict_pairs(c: &[PkgConflict]) -> Vec<(&str, &str)> {
        c.iter().map(|c| (c.new_package.as_str(), c.installed.as_str())).collect()
    }

    /// Записано 19.10.2026 (CachyOS, KDE Plasma 6.8): `LC_ALL=C pacman -Si
    /// deepin-kwin` — блок из cachyos-extra-znver4 и блок из extra.
    const SI_DEEPIN_KWIN: &str = "Repository      : cachyos-extra-znver4\n\
        Name            : deepin-kwin\n\
        Version         : 6.0.8-12.1\n\
        Provides        : None\n\
        Depends On      : qt6-base  kwayland  deepin-qt6platform-plugins\n\
        Conflicts With  : kwin  kwin-x11\n\
        Replaces        : None\n\
        \n\
        Repository      : extra\n\
        Name            : deepin-kwin\n\
        Version         : 6.0.8-12\n\
        Conflicts With  : kwin  kwin-x11\n\
        \n\
        Repository      : extra\n\
        Name            : deepin-session-shell\n\
        Conflicts With  : deepin-session-ui<5\n\
        \n\
        Repository      : extra\n\
        Name            : lightdm\n\
        Conflicts With  : None\n";

    /// Установка Deepin на машине с KDE Plasma (живой случай владельца
    /// 19.10.2026): пробная транзакция `-Sp` конфликтов НЕ видит (код 0), а
    /// настоящая падает на «deepin-kwin and kwin are in conflict». Конфликт
    /// находится заранее по `Conflicts With` того, что встанет, и ставить
    /// нельзя. Ограничение версией (`deepin-session-ui<5`) не считается.
    fn deepin_on_kde(cmd: &Cmd) -> PreviewAnswer {
        let a: Vec<&str> = cmd.argv.iter().map(String::as_str).collect();
        assert!(cmd.lang_c, "вывод pacman разбирается только под LANG=C: {a:?}");
        match a.as_slice() {
            ["pacman", "-Sg", ..] => Some((1, String::new(), String::new())),
            ["pacman", "-Sp", ..] => Some((
                0,
                "deepin-kwin\ndeepin-session-shell\nlightdm\n".into(),
                String::new(),
            )),
            ["pacman", "-Qq"] => Some((0, "glibc\nkwin\nplasma-workspace\ndeepin-session-ui\n".into(), String::new())),
            ["pacman", "-Si", rest @ ..] => {
                assert_eq!(rest, ["deepin-kwin", "deepin-session-shell", "lightdm"]);
                Some((0, SI_DEEPIN_KWIN.into(), String::new()))
            }
            ["pacman", "-Qi"] => Some((
                0,
                "Name            : kwin\nConflicts With  : None\n\n\
                 Name            : glibc\nConflicts With  : None\n"
                    .into(),
                String::new(),
            )),
            other => panic!("неожиданная команда {other:?}"),
        }
    }

    #[test]
    fn conflict_with_installed_package_blocks_install() {
        let p = install_preview(PkgFamily::Arch, &names(&["deepin", "lightdm"]), &deepin_on_kde).unwrap();
        assert_eq!(
            conflict_pairs(&p.conflicts),
            vec![("deepin-kwin", "kwin")]
        );
        assert!(!p.installable());
        assert!(p.blocked.is_empty());
    }

    /// Обратная сторона: конфликт объявлен у УСТАНОВЛЕННОГО пакета
    /// (`tuned-ppd` против `power-profiles-daemon`, которого ждёт COSMIC).
    #[test]
    fn conflict_declared_by_installed_package_blocks_install() {
        let run = |cmd: &Cmd| -> PreviewAnswer {
            let a: Vec<&str> = cmd.argv.iter().map(String::as_str).collect();
            match a.as_slice() {
                ["pacman", "-Sg", ..] => Some((1, String::new(), String::new())),
                ["pacman", "-Sp", ..] => Some((0, "power-profiles-daemon\ncosmic-comp\n".into(), String::new())),
                ["pacman", "-Qq"] => Some((0, "glibc\ntuned-ppd\n".into(), String::new())),
                ["pacman", "-Si", ..] => Some((
                    0,
                    "Name            : power-profiles-daemon\nConflicts With  : None\n\n\
                     Name            : cosmic-comp\nConflicts With  : None\n"
                        .into(),
                    String::new(),
                )),
                ["pacman", "-Qi"] => Some((
                    0,
                    "Name            : tuned-ppd\nVersion         : 2.26.0-1\n\
                     Conflicts With  : power-profiles-daemon\n\n\
                     Name            : glibc\nConflicts With  : None\n"
                        .into(),
                    String::new(),
                )),
                other => panic!("неожиданная команда {other:?}"),
            }
        };
        let p = install_preview(PkgFamily::Arch, &names(&["cosmic-comp", "power-profiles-daemon"]), &run).unwrap();
        assert_eq!(
            conflict_pairs(&p.conflicts),
            vec![("power-profiles-daemon", "tuned-ppd")]
        );
        assert!(!p.installable());
    }

    /// Записано 20.10.2026 (CachyOS, MATE; рядом стоит Budgie):
    /// `LC_ALL=C pacman -Si mutter49 gala lightdm` и `pacman -Qi` — mutter 51
    /// на машине держит budgie-desktop, а Pantheon (gala) нужен mutter49,
    /// объявляющий `Conflicts With: mutter`.
    const SI_PANTHEON_MUTTER49: &str = "Repository      : cachyos-extra-znver4\n\
        Name            : mutter49\n\
        Version         : 49.8-1.1\n\
        Groups          : None\n\
        Provides        : libmutter-17.so=0-64\n\
        Depends On      : at-spi2-core  cairo  glib2\n\
        \x20                 gtk4  xorg-xwayland\n\
        Conflicts With  : mutter\n\
        \n\
        Repository      : extra\n\
        Name            : mutter49\n\
        Version         : 49.8-1\n\
        Conflicts With  : mutter\n\
        \n\
        Repository      : cachyos-extra-znver4\n\
        Name            : gala\n\
        Version         : 8.6.2-1.1\n\
        Groups          : pantheon\n\
        Depends On      : at-spi2-core  cairo  dconf  libgee  mutter49  pango\n\
        Conflicts With  : None\n\
        \n\
        Repository      : extra\n\
        Name            : lightdm\n\
        Conflicts With  : None\n";
    const QI_BUDGIE_MUTTER: &str = "Name            : glibc\n\
        Version         : 2.42-1\n\
        Required By     : at-spi2-core  bash  budgie-desktop  mutter\n\
        Conflicts With  : None\n\
        \n\
        Name            : mutter\n\
        Version         : 51.0-3\n\
        Groups          : None\n\
        Provides        : libmutter-51.so=0-64  mutter\n\
        Required By     : budgie-desktop\n\
        Conflicts With  : None\n\
        \n\
        Name            : budgie-desktop\n\
        Version         : 10.10.1-1\n\
        Groups          : budgie\n\
        Required By     : None\n\
        Conflicts With  : None\n";

    fn pantheon_on_budgie(cmd: &Cmd) -> PreviewAnswer {
        let a: Vec<&str> = cmd.argv.iter().map(String::as_str).collect();
        assert!(cmd.lang_c, "вывод pacman разбирается только под LANG=C: {a:?}");
        match a.as_slice() {
            ["pacman", "-Sg", ..] => Some((1, String::new(), String::new())),
            ["pacman", "-Sp", ..] => Some((0, "mutter49\ngala\nlightdm\n".into(), String::new())),
            ["pacman", "-Qq"] => Some((0, "glibc\nmutter\nbudgie-desktop\n".into(), String::new())),
            ["pacman", "-Si", ..] => Some((0, SI_PANTHEON_MUTTER49.into(), String::new())),
            ["pacman", "-Qi"] => Some((0, QI_BUDGIE_MUTTER.into(), String::new())),
            other => panic!("неожиданная команда {other:?}"),
        }
    }

    /// Конфликт называется СТОЛАМИ, а не пакетами: mutter стоит потому, что
    /// его требует budgie-desktop (Required By), а mutter49 тянет gala из
    /// группы pantheon (Depends On). Табличное «mutter — это GNOME» здесь
    /// было бы враньём: GNOME на машине нет.
    #[test]
    fn conflict_names_desktops_through_required_by() {
        let p = install_preview(PkgFamily::Arch, &names(&["gala", "lightdm"]), &pantheon_on_budgie).unwrap();
        assert!(!p.installable());
        assert_eq!(p.conflicts.len(), 1, "{:#?}", p.conflicts);
        let c = &p.conflicts[0];
        assert_eq!((c.new_package.as_str(), c.installed.as_str()), ("mutter49", "mutter"));
        assert_eq!(c.new_desktops, names(&["Pantheon"]));
        assert_eq!(c.installed_desktops, names(&["Budgie"]));
        assert_eq!(c.new_version, "49.8-1.1");
        assert_eq!(c.installed_version, "51.0-3");
        assert!(c.same_program);
    }

    /// Записано 24.10.2026 (CachyOS): `LC_ALL=C pacman -Si river-classic` и
    /// `pacman -Qi river` при установленном River 0.5 из extra.
    const SI_RIVER_CLASSIC: &str = "Repository      : cachyos-extra-znver4\n\
        Name            : river-classic\n\
        Version         : 0.3.17-2.1\n\
        Provides        : wayland-compositor\n\
        Depends On      : glibc  libevdev  libinput  libxkbcommon  mesa  pixman  sh  wayland  wlroots0.20  xorg-xwayland\n\
        Conflicts With  : river\n\
        Replaces        : None\n\
        \n\
        Repository      : extra\n\
        Name            : waybar\n\
        Conflicts With  : None\n";
    const QI_RIVER_05: &str = "Name            : river\n\
        Version         : 0.5.0-1.1\n\
        Provides        : None\n\
        Required By     : None\n\
        Conflicts With  : river-classic\n\
        \n\
        Name            : glibc\n\
        Conflicts With  : None\n";

    /// Проба набора River на машине с River 0.5; `required_by` — кто
    /// требует установленный river (пусто — никто).
    fn river_over_05(required_by: &'static str) -> impl Fn(&Cmd) -> PreviewAnswer {
        move |cmd: &Cmd| {
            let a: Vec<&str> = cmd.argv.iter().map(String::as_str).collect();
            match a.as_slice() {
                ["pacman", "-Sg", ..] => Some((1, String::new(), String::new())),
                ["pacman", "-Sp", ..] => Some((0, "river-classic\nwaybar\n".into(), String::new())),
                ["pacman", "-Qq"] => Some((0, "glibc\nriver\n".into(), String::new())),
                ["pacman", "-Si", ..] => Some((0, SI_RIVER_CLASSIC.into(), String::new())),
                ["pacman", "-Qi"] => Some((
                    0,
                    QI_RIVER_05.replace("Required By     : None", &format!("Required By     : {required_by}")),
                    String::new(),
                )),
                other => panic!("неожиданная команда {other:?}"),
            }
        }
    }

    /// River 0.5 стоит: набор не ставится молча (отказ с понятным текстом
    /// про ветки River), но конфликт снимается заменой с согласия.
    #[test]
    fn river_05_conflict_is_explained_and_offered_for_replacement() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let p = install_preview(PkgFamily::Arch, &names(&["river-classic", "waybar"]), &river_over_05("None")).unwrap();
        assert_eq!(conflict_pairs(&p.conflicts), vec![("river-classic", "river")]);
        assert!(!p.installable());
        let text = conflict_explanation(&p.conflicts);
        assert!(text.contains("River 0.5") && text.contains("river-classic"), "{text}");
        assert!(!text.contains("conflicts with the installed package"), "общий текст не нужен: {text}");

        let swaps = conflict_swaps(&p);
        assert_eq!(conflict_pairs(&swaps), vec![("river-classic", "river")]);
        assert_eq!(conflict_swap_button(&swaps), "🔁 Replace River 0.5");
        let confirm = conflict_swap_confirm_text(&swaps, "River");
        assert!(confirm.contains("River 0.5 (package river)") && confirm.contains("~/.config/river/init"), "{confirm}");

        // Без согласия — замены нет; согласие на чужое имя — тоже.
        assert!(!conflict_swap_approved(&p, &[]));
        assert!(!conflict_swap_approved(&p, &names(&["kwin"])));
        assert!(conflict_swap_approved(&p, &names(&["river"])));
    }

    /// Установленный river кто-то требует — pacman снял бы и его: замену не
    /// предлагаем, остаётся отказ.
    #[test]
    fn river_05_needed_by_another_package_is_not_offered_for_replacement() {
        let p = install_preview(PkgFamily::Arch, &names(&["river-classic", "waybar"]), &river_over_05("kwm")).unwrap();
        assert!(!p.installable());
        assert!(conflict_swaps(&p).is_empty());
        assert!(!conflict_swap_approved(&p, &names(&["river"])));
    }

    /// Чужой стол заменой не снимается: Deepin рядом с KDE Plasma.
    #[test]
    fn foreign_desktop_conflict_is_never_offered_for_replacement() {
        let p = install_preview(PkgFamily::Arch, &names(&["deepin", "lightdm"]), &deepin_on_kde).unwrap();
        assert!(conflict_swaps(&p).is_empty());
        assert!(!conflict_swap_approved(&p, &names(&["kwin"])));
    }

    #[test]
    fn short_version_keeps_major_and_minor() {
        assert_eq!(short_version("0.5.0-1.1"), "0.5");
        assert_eq!(short_version("1:0.5.2-3"), "0.5");
        assert_eq!(short_version("7-1"), "7");
    }

    /// Замена — та же транзакция `pacman -S` с ответом «да» на вопрос о
    /// конфликте; только Arch, имена проверяются как у установки.
    #[test]
    fn install_cmd_replacing_answers_the_conflict_question() {
        let pkgs = names(&["river-classic", "waybar"]);
        assert_eq!(install_cmd_replacing(PkgFamily::Arch, &pkgs, &[]).unwrap(), install_cmd(PkgFamily::Arch, &pkgs).unwrap());
        assert_eq!(
            install_cmd_replacing(PkgFamily::Arch, &pkgs, &names(&["river"])).unwrap(),
            names(&["pacman", "-S", "--needed", "--noconfirm", "--ask=4", "river-classic", "waybar"])
        );
        assert!(install_cmd_replacing(PkgFamily::Fedora, &pkgs, &names(&["river"])).is_err());
        assert!(install_cmd_replacing(PkgFamily::Arch, &pkgs, &names(&["/tmp/x.pkg.tar.zst"])).is_err());
    }

    /// С заменой набор идёт обёрткой (у помощника согласия нет), и в её
    /// скрипте — установка с `--ask=4`.
    #[test]
    fn batch_plan_with_replacement_skips_the_helper() {
        let det = DistroDetector::from_os_release_files(Some("ID=cachyos\nNAME=CachyOS\n"), None);
        let table = names(crate::system::de_entry_packages_for("river", "arch").unwrap());
        let plan = install_batch_plan_with_deps(
            PkgFamily::Arch, &det, &table, &[], &names(&["river"]), &[], None, true, Some("river"), true,
            "/tmp/a.sh", "/tmp/a.log",
        )
        .unwrap();
        assert!(!plan.used_de_helper);
        let script = script_of(&plan, "/tmp/a.sh");
        assert!(script.contains("pacman -S --needed --noconfirm --noprogressbar --color never --ask=4 river-classic"), "{script}");
    }

    /// Владелец установленного пакета — не стол: называем, кто его требует.
    #[test]
    fn conflict_names_requiring_packages_when_no_desktop_holds_it() {
        let qi = QI_BUDGIE_MUTTER
            .replace("Required By     : budgie-desktop\n", "Required By     : some-app\n")
            .replace("Name            : budgie-desktop", "Name            : some-app")
            .replace("Groups          : budgie\n", "Groups          : None\n");
        let run = |cmd: &Cmd| -> PreviewAnswer {
            if cmd.argv[1] == "-Qi" {
                return Some((0, qi.clone(), String::new()));
            }
            pantheon_on_budgie(cmd)
        };
        let p = install_preview(PkgFamily::Arch, &names(&["gala", "lightdm"]), &run).unwrap();
        let c = &p.conflicts[0];
        assert!(c.installed_desktops.is_empty(), "{c:#?}");
        assert_eq!(c.required_by, names(&["some-app"]));
    }

    /// Вывод настоящей транзакции владельца 19.10.2026 (`--noconfirm`
    /// отвечает «N», ничего не удаляется).
    #[test]
    fn transaction_conflict_is_parsed() {
        let out = ":: deepin-kwin-6.0.8-12.1 and kwin-6.8.0-1.1 are in conflict. Remove kwin? [y/N] \n\
                   error: unresolvable package conflicts detected\n\
                   error: failed to prepare transaction (conflicting dependencies)\n\
                   :: deepin-kwin-6.0.8-12.1 and kwin-6.8.0-1.1 are in conflict\n";
        assert_eq!(
            parse_transaction_conflicts(out),
            vec![PkgConflict { new_package: "deepin-kwin".into(), installed: "kwin".into(), ..PkgConflict::default() }]
        );
        // Конфликт через «предоставляет» и эпоха в версии.
        assert_eq!(
            parse_transaction_conflicts(":: pipewire-pulse-1:1.4.9-1 and pulseaudio-17.0-3 are in conflict (pulseaudio). Remove pulseaudio? [y/N]"),
            vec![PkgConflict { new_package: "pipewire-pulse".into(), installed: "pulseaudio".into(), ..PkgConflict::default() }]
        );
        assert!(parse_transaction_conflicts("error: target not found: foo\n").is_empty());
    }

    #[test]
    fn broken_group_member_is_skipped_and_group_expanded() {
        let p = install_preview(PkgFamily::Arch, &names(&["pantheon", "lightdm"]), &pantheon).unwrap();
        assert_eq!(p.groups, vec![("pantheon".to_string(), 3)]);
        assert_eq!(p.skipped, names(&["switchboard-plug-online-accounts"]));
        assert!(p.installable());
        assert_eq!(p.targets, names(&["gala", "wingpanel", "lightdm"]));
        assert_eq!(p.new_packages.len(), 4);
        assert_eq!(p.missing_members(), names(&["gala", "wingpanel", "lightdm"]));
        assert_eq!(p.dependency_count(), 1);
        assert_eq!(p.installed_members(), 0);
    }

    #[test]
    fn broken_named_package_blocks() {
        let run = |cmd: &Cmd| -> PreviewAnswer {
            if cmd.argv[1] == "-Sg" {
                return Some((1, String::new(), String::new()));
            }
            Some((1, String::new(), "error: target not found: lightdm\n".into()))
        };
        let p = install_preview(PkgFamily::Arch, &names(&["lightdm"]), &run).unwrap();
        assert!(!p.installable());
        assert_eq!(p.blocked, names(&["lightdm"]));
    }

    #[test]
    fn nothing_new_when_all_installed() {
        let run = |cmd: &Cmd| -> PreviewAnswer {
            if cmd.argv[1] == "-Sg" {
                return Some((0, "cosmic a\ncosmic b\n".into(), String::new()));
            }
            Some((0, String::new(), "warning: a is up to date -- skipping\n".into()))
        };
        let p = install_preview(PkgFamily::Arch, &names(&["cosmic"]), &run).unwrap();
        assert!(p.new_packages.is_empty());
        assert_eq!(p.members, names(&["a", "b"]));
        assert_eq!(p.installed_members(), 2);
    }

    /// 19.10.2026, MATE: `pacman -Sp --needed` печатает и УСТАНОВЛЕННЫЕ
    /// пакеты, для которых вышла новая сборка (mate-applets 1.28.1-3.1 при
    /// стоящей 1.28.1-2.1), — карточка писала «Не хватает 2 пакетов».
    /// Установлен ли пакет, решает список установленного, а не план
    /// транзакции: такие пакеты — обновления, а не пропажа.
    #[test]
    fn outdated_installed_members_are_updates_not_missing() {
        let run = |cmd: &Cmd| -> PreviewAnswer {
            match cmd.argv[1].as_str() {
                "-Sg" => Some((0, "mate mate-applets\nmate mate-panel\nmate mate-netbook\n".into(), String::new())),
                "-Qq" => Some((0, "mate-applets\nmate-netbook\nglibc\n".into(), String::new())),
                _ => Some((0, "mate-applets\nmate-netbook\nmate-panel\nlibmatepanel\n".into(), String::new())),
            }
        };
        let p = install_preview(PkgFamily::Arch, &names(&["mate"]), &run).unwrap();
        assert_eq!(p.new_packages, names(&["mate-panel", "libmatepanel"]));
        assert_eq!(p.updates, names(&["mate-applets", "mate-netbook"]));
        assert_eq!(p.missing_members(), names(&["mate-panel"]));
        assert_eq!(p.installed_members(), 2);
        assert_eq!(p.dependency_count(), 1);
    }

    /// Список установленного не получен — прежнее поведение: всё, что
    /// поставит транзакция, считается новым (ничего не выдумываем).
    #[test]
    fn without_installed_list_everything_counts_as_new() {
        let run = |cmd: &Cmd| -> PreviewAnswer {
            match cmd.argv[1].as_str() {
                "-Sg" => Some((1, String::new(), String::new())),
                "-Qq" => None,
                _ => Some((0, "a\nb\n".into(), String::new())),
            }
        };
        let p = install_preview(PkgFamily::Arch, &names(&["a"]), &run).unwrap();
        assert_eq!(p.new_packages, names(&["a", "b"]));
        assert!(p.updates.is_empty());
    }

    #[test]
    fn unknown_failure_and_other_families_say_nothing() {
        let run = |cmd: &Cmd| -> PreviewAnswer {
            if cmd.argv[1] == "-Sg" {
                return Some((1, String::new(), String::new()));
            }
            Some((1, String::new(), "error: failed to init transaction (unable to lock database)\n".into()))
        };
        assert!(install_preview(PkgFamily::Arch, &names(&["kde"]), &run).is_none());
        assert!(install_preview(PkgFamily::Debian, &names(&["kde"]), &run).is_none());
        assert!(install_preview(PkgFamily::Arch, &names(&["bad name"]), &run).is_none());
    }

    // ── Модули ядра под стоящие ядра (Deepin на CachyOS, 20.10.2026) ──

    #[test]
    fn dkms_modules_only_without_the_arch_kernel() {
        let plan = names(&["deepin-file-manager", "deepin-anything", "deepin-anything-arch", "linux"]);
        // Стоит штатное ядро — готовая сборка годится.
        assert!(!needs_dkms_modules(&plan, &names(&["linux"])));
        assert!(!needs_dkms_modules(&plan, &names(&["linux", "linux-cachyos"])));
        // Только чужие ядра — штатное в транзакцию не пускаем.
        assert!(needs_dkms_modules(&plan, &names(&["linux-cachyos"])));
        assert!(needs_dkms_modules(&plan, &names(&["linux-cachyos", "linux-cachyos-lts"])));
        assert!(needs_dkms_modules(&plan, &names(&["linux-lts"])));
        // Ядер не узнать — как было.
        assert!(!needs_dkms_modules(&plan, &[]));
        // `linux` не тянется — менять нечего.
        assert!(!needs_dkms_modules(&names(&["deepin-anything-dkms", "dkms"]), &names(&["linux-cachyos"])));
    }

    #[test]
    fn dkms_names_and_headers() {
        assert_eq!(dkms_candidates("deepin-anything-arch"), names(&["deepin-anything-dkms", "deepin-anything-arch-dkms"]));
        assert_eq!(dkms_candidates("vhba-module"), names(&["vhba-module-dkms"]));
        assert_eq!(dkms_candidates("virtualbox-host-modules-arch")[0], "virtualbox-host-dkms");
        let installed = names(&["linux-cachyos", "linux-cachyos-headers", "linux-cachyos-lts"]);
        assert_eq!(
            missing_kernel_headers(&names(&["linux-cachyos", "linux-cachyos-lts"]), Some(&installed)),
            names(&["linux-cachyos-lts-headers"])
        );
        assert_eq!(missing_kernel_headers(&names(&["linux-lts"]), None), names(&["linux-lts-headers"]));
        let info = parse_pkg_info(
            "Name            : deepin-anything-arch\nDepends On      : linux\n\n\
             Name            : deepin-anything\nDepends On      : DEEPIN-ANYTHING-MODULE  qt6-base\n\n\
             Name            : linux\nDepends On      : coreutils  kmod\n",
        );
        assert_eq!(prebuilt_module_packages(&info), names(&["deepin-anything-arch"]));
    }

    /// Подставной pacman машины владельца: ядра `kernels` (+ `-headers` из
    /// `headers_installed`), набор `deepin` тянет `deepin-anything-arch` и
    /// `linux`, пока в целях нет `deepin-anything-dkms`.
    fn deepin_machine(kernels: &'static [&'static str], installed_extra: &'static [&'static str]) -> impl Fn(&Cmd) -> PreviewAnswer {
        move |cmd: &Cmd| {
            let a: Vec<&str> = cmd.argv.iter().map(String::as_str).collect();
            match a.as_slice() {
                ["pacman", "-Sg", ..] => Some((0, "deepin deepin-file-manager\ndeepin deepin-session\n".into(), String::new())),
                ["pacman", "-Qq"] => {
                    let mut list: Vec<&str> = vec!["glibc"];
                    list.extend(kernels);
                    list.extend(installed_extra);
                    Some((0, list.join("\n") + "\n", String::new()))
                }
                ["find", "/usr/lib/modules", ..] => Some((
                    0,
                    kernels.iter().enumerate().map(|(i, _)| format!("/usr/lib/modules/{i}.0-x/pkgbase\n")).collect(),
                    String::new(),
                )),
                ["pacman", "-Qqo", files @ ..] => {
                    assert_eq!(files.len(), kernels.len());
                    Some((0, kernels.iter().map(|k| format!("{k}\n")).collect(), String::new()))
                }
                ["pacman", "-Sp", ..] => {
                    let mut out = "deepin-file-manager\ndeepin-session\ndeepin-anything\n".to_string();
                    if a.contains(&"deepin-anything-dkms") {
                        out.push_str("deepin-anything-dkms\ndkms\n");
                        for t in a.iter().filter(|t| t.ends_with("-headers")) {
                            out.push_str(&format!("{t}\n"));
                        }
                    } else {
                        out.push_str("deepin-anything-arch\n");
                        if !kernels.contains(&"linux") {
                            out.push_str("linux\n");
                        }
                    }
                    Some((0, out, String::new()))
                }
                ["pacman", "-Si", rest @ ..] => {
                    let mut out = String::new();
                    for n in rest {
                        let deps = match *n {
                            "deepin-anything-arch" => "linux",
                            "deepin-anything-dkms" => "dkms",
                            "deepin-anything-arch-dkms" => continue,
                            _ => "glibc",
                        };
                        out.push_str(&format!("Name            : {n}\nDepends On      : {deps}\n\n"));
                    }
                    Some((0, out, String::new()))
                }
                ["pacman", "-Qi"] => Some((0, String::new(), String::new())),
                other => panic!("неожиданная команда {other:?}"),
            }
        }
    }

    #[test]
    fn deepin_on_cachyos_takes_dkms_not_the_arch_kernel() {
        let run = deepin_machine(&["linux-cachyos", "linux-cachyos-lts"], &["linux-cachyos-headers"]);
        let p = install_preview(PkgFamily::Arch, &names(&["deepin", "lightdm"]), &run).unwrap();
        assert_eq!(p.targets, names(&["deepin", "lightdm", "deepin-anything-dkms", "linux-cachyos-lts-headers"]));
        assert!(!p.new_packages.iter().any(|n| n == "linux" || n == "deepin-anything-arch"), "{:?}", p.new_packages);
        assert!(p.new_packages.iter().any(|n| n == "deepin-anything-dkms"));
        // Это зависимости, а не пакеты стола; уже стоявшие заголовки
        // (`linux-cachyos-headers`) в пометку не попадают.
        assert!(!p.members.iter().any(|n| n == "deepin-anything-dkms"));
        assert_eq!(p.deps_only, names(&["deepin-anything-dkms", "linux-cachyos-lts-headers"]));
        // Модуль ядра грузится при старте — после установки нужна
        // перезагрузка; заголовки и сам `dkms` её не требуют.
        assert_eq!(p.restart_packages, names(&["deepin-anything-dkms"]));
    }

    #[test]
    fn deepin_with_the_arch_kernel_keeps_the_prebuilt_module() {
        for kernels in [&["linux"][..], &["linux", "linux-cachyos"][..]] {
            let kernels: &'static [&'static str] = Box::leak(kernels.to_vec().into_boxed_slice());
            let p = install_preview(PkgFamily::Arch, &names(&["deepin", "lightdm"]), &deepin_machine(kernels, &[])).unwrap();
            assert_eq!(p.targets, names(&["deepin", "lightdm"]));
            assert!(p.new_packages.iter().any(|n| n == "deepin-anything-arch"));
            assert!(p.deps_only.is_empty());
            // Готовая сборка модуля под штатное ядро — тоже перезагрузка.
            assert_eq!(p.restart_packages, names(&["deepin-anything-arch"]));
        }
    }

    /// Перезагрузку требуют только ядро и модули ядра; стол, его службы и
    /// экран входа — нет. Решают зависимости из `pacman -Si`, а без них —
    /// имена.
    #[test]
    fn restart_packages_are_kernels_and_kernel_modules() {
        let info = |name: &str, deps: &[&str]| PkgInfo {
            name: name.to_string(),
            depends: names(deps),
            ..PkgInfo::default()
        };
        let new = names(&["river", "foot", "wlroots0.19", "lightdm", "nvidia", "zfs-dkms", "linux", "linux-headers"]);
        let infos = [
            info("river", &["wlroots0.19", "libxkbcommon"]),
            info("foot", &["fcft"]),
            info("lightdm", &["systemd"]),
            info("nvidia", &["linux", "nvidia-utils"]),
            info("zfs-dkms", &["dkms"]),
            info("linux-headers", &["pahole"]),
        ];
        assert_eq!(restart_packages(&new, &infos), names(&["nvidia", "zfs-dkms", "linux"]));
        // Сведений нет — решают имена: `*-dkms` и само ядро.
        assert_eq!(restart_packages(&new, &[]), names(&["zfs-dkms", "linux"]));
        // Обычный стол (River 24.10.2026) — хватит выйти из сеанса.
        assert!(restart_packages(&names(&["river", "foot"]), &infos).is_empty());
    }

    #[test]
    fn deepin_on_arch_lts_only_gets_lts_headers() {
        let p = install_preview(PkgFamily::Arch, &names(&["deepin"]), &deepin_machine(&["linux-lts"], &[])).unwrap();
        assert_eq!(p.targets, names(&["deepin", "deepin-anything-dkms", "linux-lts-headers"]));
        assert!(!p.new_packages.iter().any(|n| n == "linux"));
    }

    #[test]
    fn de_helper_installs_only_its_own_table() {
        let table = names(crate::system::de_entry_packages_for("pantheon", "arch").unwrap());
        assert!(de_helper_covers("pantheon", PkgFamily::Arch, &table));
        let mut more = table.clone();
        more.push("vhba-module-dkms".into());
        assert!(!de_helper_covers("pantheon", PkgFamily::Arch, &more));
        // Часть таблицы помощник ставит, как всегда (сверка с Python,
        // кейс arch-de-helper-kde).
        assert!(de_helper_covers("pantheon", PkgFamily::Arch, &table[1..]));
        assert!(!de_helper_covers("pantheon", PkgFamily::Arch, &names(&["gala"])));
    }

    /// Группу `deepin` (с deepin-session и deepin-kwin) помощник от root не
    /// ставит: Deepin — только на Treeland, обёрткой.
    #[test]
    fn de_helper_has_no_deepin() {
        assert!(!de_helper_supports("deepin", "arch"));
        assert!(!de_helper_pkg_table().contains("\"deepin|"));
        assert!(de_helper_pkg_table().contains("\"pantheon|arch\")"));
    }

    fn script_of(plan: &BatchPlan, path: &str) -> String {
        plan.steps
            .iter()
            .find_map(|s| match s {
                PlanStep::WriteFile { path: p, content, .. } if p == path => Some(content.clone()),
                _ => None,
            })
            .unwrap_or_default()
    }

    #[test]
    fn kernel_module_deps_are_marked_in_the_same_script() {
        let det = DistroDetector::from_os_release_files(Some("ID=cachyos\nNAME=CachyOS\n"), None);
        let pkgs = names(&["deepin", "lightdm", "deepin-anything-dkms", "linux-cachyos-lts-headers"]);
        // Чужое имя в пометке (не из набора) отбрасывается.
        let deps = names(&["deepin-anything-dkms", "linux-cachyos-lts-headers", "linux"]);
        let plan = install_batch_plan_with_deps(
            PkgFamily::Arch, &det, &pkgs, &deps, &[], &[], None, true, Some("deepin"), true, "/tmp/a.sh", "/tmp/a.log",
        )
        .unwrap();
        // Набор не из таблицы помощника — обёртка, один pkexec.
        assert!(!plan.used_de_helper);
        let script = script_of(&plan, "/tmp/a.sh");
        let install = script.find("pacman -S --needed").expect(&script);
        let mark = script.find("pacman -D --asdeps deepin-anything-dkms linux-cachyos-lts-headers\n").expect(&script);
        assert!(mark > install, "{script}");
        assert!(script.contains("set -e"), "пометка только после успешной установки");
        let pkexecs = plan.steps.iter().filter(|s| matches!(s, PlanStep::Run { needs_root: true, .. })).count();
        assert_eq!(pkexecs, 1);

        // Без пометки — как было: набор из таблицы идёт помощником.
        let table = names(crate::system::de_entry_packages_for("pantheon", "arch").unwrap());
        let plan = install_batch_plan_with_deps(
            PkgFamily::Arch, &det, &table, &[], &[], &[], None, true, Some("pantheon"), true, "/tmp/a.sh", "/tmp/a.log",
        )
        .unwrap();
        assert!(plan.used_de_helper);
        // Deepin — никогда помощником, даже набором из таблицы.
        let table = names(crate::system::de_entry_packages_for("deepin", "arch").unwrap());
        let plan = install_batch_plan_with_deps(
            PkgFamily::Arch, &det, &table, &[], &[], &[], None, true, Some("deepin"), true, "/tmp/a.sh", "/tmp/a.log",
        )
        .unwrap();
        assert!(!plan.used_de_helper);
        assert_eq!(mark_deps_line(PkgFamily::Debian, &deps).unwrap(), None);
    }
}

#[cfg(test)]
mod wayland_default_session_tests {
    use super::*;
    use std::collections::HashMap;

    fn fs(files: &[(&str, &str)]) -> HashMap<String, String> {
        files.iter().map(|(p, t)| (p.to_string(), t.to_string())).collect()
    }

    fn plan_on(files: &HashMap<String, String>) -> SessionSwitch {
        let read = |p: &str| files.get(p).cloned();
        let list = |d: &str| {
            files.keys().filter_map(|p| p.strip_prefix(&format!("{d}/")).map(str::to_string)).collect::<Vec<_>>()
        };
        plan_wayland_default(&X11_SESSION_SUCCESSORS[0], &read, &list)
    }

    #[test]
    fn только_значение_ключа_сеанса_и_в_прежнем_виде() {
        let s = &X11_SESSION_SUCCESSORS[0];
        let keys = LOGIN_SESSION_SOURCES[2].keys;
        // Голое имя, с `.desktop`, полный путь; чужие ключи и группы, тот же
        // ключ в чужой группе, комментарий — не трогаются.
        let text = "# Session=plasmax11\n[Autologin]\nUser=me\nSession=plasmax11\n[Greeter]\nPreselectedSession =  plasmax11.desktop\n\
                    Session=plasmax11\n[Other]\nSession=plasmax11\n[Autologin][seat1]\nSession=plasmax11";
        assert_eq!(
            retarget_session(text, keys, s).unwrap(),
            "# Session=plasmax11\n[Autologin]\nUser=me\nSession=plasma\n[Greeter]\nPreselectedSession =  plasma.desktop\n\
             Session=plasmax11\n[Other]\nSession=plasmax11\n[Autologin][seat1]\nSession=plasmax11"
        );
        let sddm_state = "[Last]\nSession=/usr/share/xsessions/plasmax11.desktop\n";
        assert_eq!(
            retarget_session(sddm_state, LOGIN_SESSION_SOURCES[1].keys, s).unwrap(),
            "[Last]\nSession=/usr/share/wayland-sessions/plasma.desktop\n"
        );
        // Уже Wayland либо другой X11-сеанс — менять нечего.
        assert_eq!(retarget_session("[Autologin]\nSession=plasma\n", keys, s), None);
        assert_eq!(retarget_session("[Autologin]\nSession=xfce\n", keys, s), None);
    }

    fn theme_on(theme: &str, files: &HashMap<String, String>) -> SessionSwitch {
        let read = |p: &str| files.get(p).cloned();
        let list = |d: &str| {
            files.keys().filter_map(|p| p.strip_prefix(&format!("{d}/")).map(str::to_string)).collect::<Vec<_>>()
        };
        plan_sddm_theme(theme, &read, &list)
    }

    #[test]
    fn тема_sddm_перекрытая_etc_sddm_conf_меняется_и_там() {
        // Как на CachyOS: тема в `/etc/sddm.conf` (читается последним) и в
        // настройках KDE. Правятся оба, свой файл не нужен, прочее — байт в байт.
        let files = fs(&[
            ("/etc/sddm.conf.d/kde_settings.conf", "[Autologin]\nSession=plasma\n\n[Theme]\nCurrent=breeze\nCursorTheme=breeze_cursors\n"),
            ("/etc/sddm.conf", "[General]\nX=1\n\n[Theme]\nCurrent=breeze\nCursorSize=32\n"),
        ]);
        let SessionSwitch::Edits(e) = theme_on("sugar-dark", &files) else { panic!() };
        assert_eq!(
            e,
            vec![
                SessionEdit {
                    path: "/etc/sddm.conf.d/kde_settings.conf".into(),
                    text: "[Autologin]\nSession=plasma\n\n[Theme]\nCurrent=sugar-dark\nCursorTheme=breeze_cursors\n".into(),
                },
                SessionEdit { path: "/etc/sddm.conf".into(), text: "[General]\nX=1\n\n[Theme]\nCurrent=sugar-dark\nCursorSize=32\n".into() },
            ]
        );
        // Итог — по тому же чтению, что у SDDM.
        let mut after = files.clone();
        for x in e {
            after.insert(x.path, x.text);
        }
        let read = |p: &str| after.get(p).cloned();
        let list = |d: &str| after.keys().filter_map(|p| p.strip_prefix(&format!("{d}/")).map(str::to_string)).collect::<Vec<_>>();
        assert_eq!(sddm_effective_theme(&read, &list).as_deref(), Some("sugar-dark"));
    }

    #[test]
    fn тема_sddm_без_ключа_в_etc_свой_файл() {
        // Нигде не задана — свой файл.
        assert_eq!(
            theme_on("t", &fs(&[])),
            SessionSwitch::Edits(vec![SessionEdit { path: SDDM_REXON_THEME_CONF.into(), text: "[Theme]\nCurrent=t\n".into() }])
        );
        // Только в файле пакета — его не трогаем, перекрываем своим.
        let SessionSwitch::Edits(e) = theme_on("t", &fs(&[("/usr/lib/sddm/sddm.conf.d/default.conf", "[Theme]\nCurrent=\n")])) else {
            panic!()
        };
        assert_eq!(e.iter().map(|e| e.path.as_str()).collect::<Vec<_>>(), [SDDM_REXON_THEME_CONF]);
        // Ключ в чужой группе и в комментарии — не тема.
        let SessionSwitch::Edits(e) = theme_on("t", &fs(&[("/etc/sddm.conf", "# Current=x\n[General]\nCurrent=x\n")])) else {
            panic!()
        };
        assert_eq!(e.iter().map(|e| e.path.as_str()).collect::<Vec<_>>(), [SDDM_REXON_THEME_CONF]);
    }

    #[test]
    fn тема_sddm_уже_выбрана_или_свой_файл_правится_на_месте() {
        assert_eq!(theme_on("t", &fs(&[("/etc/sddm.conf", "[Theme]\nCurrent=t\n")])), SessionSwitch::Nothing);
        // Прежний свой файл — правится, второго не появляется; пустое
        // `Current=` в `/etc/sddm.conf` (встроенная тема) — тоже значение.
        let SessionSwitch::Edits(e) = theme_on(
            "new",
            &fs(&[(SDDM_REXON_THEME_CONF, "[Theme]\nCurrent=old\n"), ("/etc/sddm.conf", "[Theme]\nCurrent=\n")]),
        ) else {
            panic!()
        };
        assert_eq!(
            e,
            vec![
                SessionEdit { path: SDDM_REXON_THEME_CONF.into(), text: "[Theme]\nCurrent=new\n".into() },
                SessionEdit { path: "/etc/sddm.conf".into(), text: "[Theme]\nCurrent=new\n".into() },
            ]
        );
    }

    #[test]
    fn правит_только_etc_и_состояние_sddm_и_сверяет_итог() {
        // Пусто — менять нечего.
        assert_eq!(plan_on(&fs(&[])), SessionSwitch::Nothing);
        // `/etc` и последний сеанс SDDM — правятся; файл пакета без X11 —
        // не трогается.
        let SessionSwitch::Edits(e) = plan_on(&fs(&[
            ("/usr/lib/sddm/sddm.conf.d/default.conf", "[Autologin]\nRelogin=false\n"),
            ("/etc/sddm.conf.d/kde_settings.conf", "[Autologin]\nSession=plasmax11.desktop\n"),
            ("/etc/sddm.conf", "[Autologin]\nSession=plasmax11\n"),
            (SDDM_STATE_FILE, "[Last]\nSession=/usr/share/xsessions/plasmax11.desktop\n"),
            ("/etc/plasmalogin.conf.d/10.conf", "[Greeter]\nPreselectedSession=plasmax11.desktop\n"),
        ])) else {
            panic!()
        };
        let paths: Vec<&str> = e.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(
            paths,
            ["/etc/sddm.conf.d/kde_settings.conf", "/etc/sddm.conf", SDDM_STATE_FILE, "/etc/plasmalogin.conf.d/10.conf"]
        );
        // X11 — в файле пакета, и `/etc` его не перекрывает: ничего не менять.
        assert_eq!(
            plan_on(&fs(&[
                ("/usr/lib/plasmalogin/plasmalogin.conf.d/x.conf", "[Autologin]\nSession=plasmax11\n"),
                ("/etc/sddm.conf", "[Autologin]\nSession=plasmax11\n"),
            ])),
            SessionSwitch::Elsewhere
        );
        // Перекрыт поздним файлом `/etc` — тогда правится он, и итог чист.
        assert!(matches!(
            plan_on(&fs(&[
                ("/usr/lib/sddm/sddm.conf.d/x.conf", "[Autologin]\nSession=plasmax11\n"),
                ("/etc/sddm.conf", "[Autologin]\nSession=plasmax11\n"),
            ])),
            SessionSwitch::Edits(e) if e.len() == 1 && e[0].path == "/etc/sddm.conf"
        ));
    }

    #[test]
    fn операция_уходит_помощнику_с_именем_из_таблицы() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let s = &X11_SESSION_SUCCESSORS[0];
        let out = |rc: i32, stderr: String| crate::system_exec::RunOut {
            outcome: crate::utils::RunOutcome::Ok,
            returncode: Some(rc),
            stdout: String::new(),
            stderr,
        };
        let mut seen = Vec::new();
        let ok = make_wayland_default_with(s, &mut |op| {
            seen.push(op.clone());
            Some(out(0, String::new()))
        });
        assert_eq!(ok.as_deref(), Ok("Plasma (Wayland) is now the default session on the login screen."));
        assert_eq!(seen, [crate::privilege::op::PrivOp::LoginSessionWayland { x11: "plasmax11".into() }]);
        let refused = make_wayland_default_with(s, &mut |_| {
            Some(out(1, crate::privilege::client::key_text("refused.wayland-session-missing.plasmax11")))
        });
        assert_eq!(
            refused,
            Err("Rexon didn't change the login screen: Plasma (Wayland) isn't installed. Nothing was changed.".into())
        );
        assert!(make_wayland_default_with(s, &mut |_| None).is_err());
    }

    #[test]
    fn сеанс_экрана_входа_по_умолчанию_опознаётся() {
        let found = |v: &[&str]| x11_login_default(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>()).map(|s| s.x11);
        assert_eq!(found(&["plasma", "/usr/share/xsessions/plasmax11.desktop"]), Some("plasmax11"));
        assert_eq!(found(&["plasma.desktop", "xfce"]), None);
        // Каждая пара — разная, и X11-сеанс не совпадает со своей парой.
        for s in X11_SESSION_SUCCESSORS {
            assert_ne!(s.x11, s.wayland);
        }
    }
}
