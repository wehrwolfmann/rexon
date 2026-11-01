//! Живой исполнитель системного слоя — порт ДЕЙСТВУЮЩЕЙ половины
//! `rexon/system.py`.
//!
//! ЗАЧЕМ. Решения этого слоя переехали в ядро давно и целиком:
//! [`crate::system`] держит базу рабочих столов и разбор os-release, а
//! [`crate::system_plan`] — планы установки, тексты привилегированных
//! helper-скриптов, откат форков Plasma, Plymouth и SDDM, детект экрана
//! входа. Не было ровно того, что эти планы ИСПОЛНЯЕТ: походов наружу —
//! `pkexec`, `systemctl is-enabled`, опроса пакетного менеджера, чтения
//! `/etc/sddm.conf.d` и `/usr/share/plymouth/themes`. Держал их Python, и
//! из-за этого аварийный маршрут `--recover-kde` — тот самый, который обязан
//! работать, когда сломан Qt, — упирался в интерпретатор с колесом моста.
//!
//! Разделение здесь такое же, как у соседних исполнителей
//! ([`crate::wallpaper_exec`], [`crate::splash_exec`], [`crate::ext_exec`],
//! [`crate::imp_live`], [`crate::appdefaults_exec`]): **ядро решает,
//! исполнитель делает**.
//!   * ЧТО делать — [`crate::system_plan`] (планы, argv, все тексты ответов);
//!   * ЧЕМ делать — трейт [`SystemIo`]: каждый метод ровно один побочный
//!     вызов или наблюдение. В продукте его исполняет [`LiveSystemIo`]
//!     (настоящие ФС и подпроцессы), в сверках — подставной исполнитель, и
//!     живая машина при этом не трогается вовсе;
//!   * ПОРЯДОК шагов — [`SystemExec`].
//!
//! ⛔ ГРАНИЦА БЕЗОПАСНОСТИ. Всё, что этот модуль умеет менять, меняется
//! ровно тремя запусками `pkexec` (форки Plasma, Plymouth, SDDM), и каждый
//! из них собирает не этот файл, а ядро. Здесь нет ни одной строки команды:
//! исполнитель берёт готовый `argv` из плана и отдаёт код возврата обратно в
//! ядро на разбор. Поэтому «что именно выполнится» проверяется сверками
//! ядра, а «в каком порядке и по каким наблюдениям» — сверками этого файла
//! на подставном вводе-выводе.
//!
//! ⭐ ЧТО ПОЧИНЕНО ПРИ ПЕРЕНОСЕ (правило #16 — дефекты оригинала не
//! консервируем):
//!
//!   1. **Экран входа сбрасывается ровно тем планом, который показали
//!      человеку.** Оригинальный маршрут (`rexon/app.py:_reset_sddm`)
//!      считал план `sddm_reset_plan()`, печатал список файлов, а потом звал
//!      `reset_sddm_to_default()`, который сканировал каталог ЗАНОВО и
//!      удалял то, что нашёл во второй раз. Между двумя сканами состояние
//!      могло измениться (сосед-установщик тем пишет туда же), и удалялось
//!      не то, что обещано. Здесь план считается один раз
//!      ([`SystemExec::sddm_reset_plan`]) и исполняется как есть
//!      ([`SystemExec::apply_sddm_reset`]); прежняя однокнопочная форма
//!      ([`SystemExec::reset_sddm_to_default`]) осталась и зовёт обе.
//!   2. **Висячий симлинк экрана входа теперь НАЗЫВАЕТСЯ вслух.** Решение
//!      «висячий симлинк — это не экран входа» ядро приняло раньше
//!      ([`crate::system_plan::dm_symlink_unit`]), но добыть имя мёртвой
//!      цели было нечем: `std::fs::canonicalize` на висячем симлинке просто
//!      падает. [`realpath_lenient`] разбирает путь по звеньям и отдаёт цель
//!      даже когда её нет на диске, поэтому
//!      [`SystemExec::dm_symlink_target`] возвращает `(Some(цель), false)` —
//!      и защита от дурака может сказать человеку, ЧЕЙ юнит удалён, а не
//!      молчать.
//!   3. **Дистрибутив определяется один раз на исполнитель.** Оригинальный
//!      `_is_pkg_installed` на КАЖДУЮ проверку пакета создавал
//!      `PackageManager(DistroDetector())`, то есть заново читал
//!      `/etc/os-release` и заново искал бинарники в `PATH`; выбор дефолтной
//!      темы Plymouth зовёт его в цикле. Здесь ответ считается лениво и
//!      запоминается ([`SystemExec::family`]).
//!   4. **Привилегированный helper считается развёрнутым только если его
//!      можно ЗАПУСТИТЬ.** Оригинальный `_helper_available` спрашивал лишь
//!      `os.path.exists`. Файл без бита запуска (сорванная установка,
//!      распакованный не тем способом архив) проходил проверку, `pkexec`
//!      падал на `exec`, и вызывающий получал отказ операции — хотя ради
//!      ровно этого случая заведён мягкий откат на сырой `pkexec <pm>`.
//!      Теперь проверяется и бит запуска ([`SystemIo::is_executable`]), и
//!      откат срабатывает как задумано. Штатная установка кладёт helper'ы
//!      через `install -Dm755` (`installer::helper_install_block`), так что
//!      на исправной машине ответ прежний.
//!   5. **«Не смогли» и «не дождались» больше не одно и то же.** `safe_run`
//!      оригинала на оба случая отдавал `None`, и `logout_session`
//!      единственный отличал таймаут — своей веткой `except`. Здесь исход
//!      команды несёт [`RunOut::outcome`], а `None`-семантика оригинала
//!      осталась одним методом [`RunOut::rc`], которым и кормится ядро.
//!
//! ⚠️ ЧЕГО ЗДЕСЬ НЕТ. Установка, переключение и удаление рабочих столов
//! (`install_de`/`switch_de`/`remove_de`, `PackageManager.install_batch`,
//! `ensure_display_manager`, пересборка AeroShell) — следующий заход:
//! у них свой исполнитель с прогрессом и хвостом журнала, и мешать его в
//! один файл с аварийным откатом незачем.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};

use crate::app;
use crate::qt_settings;
use crate::system::{self, DistroDetector, PkgFamily};
use crate::system_plan as plan;
use crate::utils::{self, RunOutcome, RunOutput, RunSpec};
use serde::Serialize;

// ============================================================================
// Трейт исполнителя
// ============================================================================

/// Итог запуска внешней команды — то, что оригинал получал от `safe_run`,
/// плюс различие «не смогли / не дождались» (см. пункт 5 в шапке модуля).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOut {
    pub outcome: RunOutcome,
    pub returncode: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl RunOut {
    /// Исход «команда не дала ответа вовсе» — `safe_run(...) is None`.
    pub fn no_answer(&self) -> bool {
        matches!(self.outcome, RunOutcome::Timeout | RunOutcome::Error)
    }

    /// Код возврата в форме, которую ждёт ядро: `None` = ответа не было.
    pub fn rc(&self) -> Option<i32> {
        if self.no_answer() {
            None
        } else {
            self.returncode
        }
    }

    /// Ответ «команды нет / не запустилась» — им заполняются ветки, где
    /// исполнитель до запуска не дошёл.
    pub fn missing() -> Self {
        RunOut {
            outcome: RunOutcome::Error,
            returncode: None,
            stdout: String::new(),
            stderr: String::new(),
        }
    }
}

/// Побочные действия и наблюдения системного слоя — ровно те, что делал
/// Python. Ни одного решения: каждый метод — один поход наружу.
///
/// Методы берут `&self`, а не `&mut self`: решающий слой
/// [`crate::system_plan`] принимает наблюдения замыканиями
/// (`&dyn Fn(&str) -> Option<String>` и родня), и с `&mut self` каждый такой
/// вызов упирался бы в двойное заимствование исполнителя. Подставным
/// исполнителям это ничего не стоит — журнал они держат в `RefCell`.
pub trait SystemIo {
    /// `os.path.exists` (симлинки разыменовываются, как в Python).
    fn exists(&self, path: &str) -> bool;
    /// `os.path.isfile`.
    fn is_file(&self, path: &str) -> bool;
    /// `os.path.isdir`.
    fn is_dir(&self, path: &str) -> bool;
    /// Есть ли у файла бит запуска (`os.access(p, os.X_OK)`).
    fn is_executable(&self, path: &str) -> bool;
    /// `os.path.realpath` — НЕстрогий: у висячего симлинка отдаёт мёртвую
    /// цель, а не ошибку (см. пункт 2 в шапке модуля). `None` — путь пуст
    /// либо ФС ответила ошибкой.
    fn realpath(&self, path: &str) -> Option<String>;
    /// Чтение текстового файла; `None` — не прочли (аналог `except OSError`).
    fn read_text(&self, path: &str) -> Option<String>;
    /// Имена файлов каталога БЕЗ порядка. `None` — каталога нет либо он не
    /// читается (`except OSError`): это НЕ то же, что пустой каталог.
    /// Порядок задаёт ядро — обход ФС произволен.
    fn list_dir(&self, dir: &str) -> Option<Vec<String>>;
    /// `shutil.which(name) is not None` (с учётом синонимов бинаря).
    fn which(&self, name: &str) -> bool;
    /// `safe_run(argv, capture_output=True, timeout=…, lang_c=…)`.
    fn run(&self, argv: &[String], timeout_secs: Option<u64>, lang_c: bool) -> RunOut;
    /// Переменная окружения; отсутствующая — пустая строка.
    fn env(&self, name: &str) -> String;
    /// `resolve_tool(name)` — ПУТЬ к программе с учётом синонимов имени
    /// (`kwriteconfig6` → `kwriteconfig5`) и каталогов вне `PATH`.
    ///
    /// Отдельно от [`SystemIo::which`] намеренно: тому достаточно ответа
    /// «есть», а сборщикам argv нужен сам путь — тот же, каким команда
    /// потом и запустится, иначе проверка и запуск разъезжаются.
    fn which_path(&self, name: &str) -> Option<String>;
    /// Записать текстовый файл целиком. `Err` — текст ошибки для человека
    /// (порт `except OSError as e` с показом `{e}`).
    fn write_text(&self, path: &str, content: &str) -> Result<(), String>;
    /// Скопировать файл вместе с правами (`shutil.copy2`). `Err` — текст
    /// ошибки для человека.
    fn copy_file(&self, src: &str, dst: &str) -> Result<(), String>;
    /// Удалить файл (не каталог). `Err` — текст ошибки. По умолчанию
    /// исполнитель удалять не умеет: удаление нужно только сбросу шрифтов.
    fn unlink_file(&self, path: &str) -> Result<(), String> {
        Err(format!("cannot remove {path}"))
    }
    /// Поставить на место файла `path` символьную ссылку на `target` (одним
    /// переименованием: файл либо прежний, либо уже ссылка). По умолчанию не
    /// умеет — см. [`SystemIo::unlink_file`].
    fn replace_with_symlink(&self, path: &str, target: &str) -> Result<(), String> {
        Err(format!("cannot link {path} to {target}"))
    }
    /// Операция от root через сеанс прав окна ([`crate::privilege::run`]):
    /// `files` — пути файлов операции, ход — в журнал `log_path` (если есть)
    /// и в `stdout` ответа. `None` — сеанса прав нет или помощник операцию не
    /// исполняет: исполнитель идёт прежней командой (`run`). Отказ помощника —
    /// `Some` с ненулевым кодом, прежней команды после него нет (З5).
    /// По умолчанию `None`: подделки и стенды живут прежним путём.
    fn run_priv(
        &self,
        op: &crate::privilege::op::PrivOp,
        files: &[String],
        log_path: Option<&str>,
    ) -> Option<RunOut> {
        let _ = (op, files, log_path);
        None
    }
}

// ============================================================================
// Исполнитель
// ============================================================================

/// Порядок шагов системного слоя поверх [`SystemIo`].
///
/// Живёт ровно столько, сколько длится операция: два запомненных ответа
/// (`pkexec` есть / семейство дистрибутива) в оригинале были глобальными
/// на процесс, из-за чего один раз посчитанный ответ переживал и установку
/// polkit, и любую сверку. Здесь память привязана к исполнителю: новый
/// исполнитель — новый ответ, а подставной ничего не наследует от живой
/// машины.
pub struct SystemExec<'a, IO: SystemIo + ?Sized> {
    io: &'a IO,
    /// Порт кеша `_polkit_state`.
    polkit: Cell<Option<bool>>,
    /// Порт `PackageManager(DistroDetector()).family` — считается лениво.
    family: Cell<Option<PkgFamily>>,
    /// Строки, которые оригинал писал в журнал (`_log.error`), — в порядке
    /// появления. Вызывающий решает, показывать их человеку или нет.
    logs: RefCell<Vec<String>>,
}

impl<'a, IO: SystemIo + ?Sized> SystemExec<'a, IO> {
    pub fn new(io: &'a IO) -> Self {
        SystemExec {
            io,
            polkit: Cell::new(None),
            family: Cell::new(None),
            logs: RefCell::new(Vec::new()),
        }
    }

    /// Накопленные строки журнала (порт `_log.error`/`_log.debug`).
    pub fn logs(&self) -> Vec<String> {
        self.logs.borrow().clone()
    }

    fn log(&self, line: String) {
        self.logs.borrow_mut().push(line);
    }

    // ── Движок пакетов ───────────────────────────────────────────────────

    /// Порт `_run_pkg_query`: ЕДИНЫЙ поход наружу для всех вопросов движка
    /// пакетов. `(None, "")` — спрашивать было нечем (плана нет, бинаря нет)
    /// либо команда не дала ответа. Это НЕ то же, что ненулевой код: у пробы
    /// существования цели «спросить не вышло» и «такого пакета нет» ведут к
    /// разным решениям.
    fn run_pkg_query(&self, query: Option<&plan::InstalledPkgQuery>) -> (Option<i32>, String) {
        let Some(q) = query else {
            return (None, String::new());
        };
        if !self.io.which(&q.binary) {
            return (None, String::new());
        }
        let out = self.io.run(&q.cmd.argv, q.cmd.timeout_secs, q.cmd.lang_c);
        match out.rc() {
            Some(rc) => (Some(rc), out.stdout),
            None => (None, String::new()),
        }
    }

    /// Семейство пакетного менеджера этой машины — считается один раз.
    pub fn family(&self) -> PkgFamily {
        if let Some(f) = self.family.get() {
            return f;
        }
        let detector = DistroDetector::from_os_release_files(
            self.io.read_text("/etc/os-release").as_deref(),
            self.io.read_text("/usr/lib/os-release").as_deref(),
        );
        let f = detector.family(&|b| self.io.which(b));
        self.family.set(Some(f));
        f
    }

    /// Порт `installed_packages`: имена ВСЕГО, что установлено. `[]` —
    /// спросить не вышло.
    pub fn installed_packages(&self, family: PkgFamily) -> Vec<String> {
        let query = plan::installed_packages_query(family);
        let (rc, out) = self.run_pkg_query(query.as_ref());
        plan::parse_installed_packages(rc == Some(0), &out)
    }

    /// Порт `_pacman_qi_stdout`: stdout вопроса «метаданные установленного
    /// пакета» либо `None`.
    pub fn pkg_meta_stdout(&self, package: &str, family: PkgFamily) -> Option<String> {
        let query = plan::installed_meta_query(family, package);
        let (rc, out) = self.run_pkg_query(query.as_ref());
        if rc == Some(0) {
            Some(out)
        } else {
            None
        }
    }

    /// Порт `_repo_target_exists`: есть ли такая цель в репозиториях.
    /// `Some(true)` есть, `Some(false)` менеджер внятно сказал «нет»,
    /// `None` — спросить не удалось (решение по `None` остаётся прежним,
    /// иначе на не-Arch мы отказывали бы на ровном месте).
    pub fn repo_target_exists(&self, package: &str, family: PkgFamily) -> Option<bool> {
        let query = plan::repo_version_query(family, package.trim());
        let (rc, out) = match query {
            None => (None, String::new()),
            Some(q) => {
                let ipq = plan::InstalledPkgQuery { binary: q.binary, cmd: q.cmd };
                self.run_pkg_query(Some(&ipq))
            }
        };
        plan::repo_target_exists(family, rc, &out)
    }

    /// Порт `PackageManager.is_installed` / `_is_pkg_installed`.
    /// Best-effort: `false` при любой неудаче.
    pub fn is_pkg_installed(&self, package: &str) -> bool {
        let Ok(argv) = plan::query_cmd(self.family(), package) else {
            return false;
        };
        let out = self.io.run(&argv, None, false);
        plan::pkg_installed_from_rc(out.rc())
    }

    // ── Polkit и привилегированные helper'ы ──────────────────────────────

    /// Порт `polkit_available`: есть ли `pkexec` — ЕДИНАЯ проверка polkit.
    ///
    /// Ответ запоминается на время жизни исполнителя, и ровно один раз в
    /// журнал уходит объяснение, чего не хватает.
    pub fn polkit_available(&self) -> bool {
        if let Some(known) = self.polkit.get() {
            return known;
        }
        let found = self.io.which("pkexec");
        self.polkit.set(Some(found));
        if !found {
            self.log(self.polkit_missing_message());
        }
        found
    }

    /// Порт `polkit_missing_message`: человеческое объяснение «нет pkexec» с
    /// именем пакета для ЭТОГО дистрибутива.
    pub fn polkit_missing_message(&self) -> String {
        system::polkit_missing_message(&utils::tool_package_hint(
            "pkexec",
            self.family().as_str(),
        ))
    }

    /// Порт `_helper_available`: развёрнут ли привилегированный helper и его
    /// polkit-политика. Один вопрос на все три helper'а Rexon (de/pkg/stage).
    ///
    /// ⭐ Бит запуска проверяется дополнительно к оригиналу — см. пункт 4 в
    /// шапке модуля. Мягкий откат (вызывающий идёт сырым `pkexec <pm>`)
    /// от этого только включается там, где раньше падал `exec`.
    pub fn helper_available(&self, helper_path: &str, policy_path: &str) -> bool {
        self.polkit_available()
            && self.io.is_file(helper_path)
            && self.io.is_executable(helper_path)
            && self.io.exists(policy_path)
    }

    /// Порт `_de_helper_available` (установка рабочих столов одним паролем).
    pub fn de_helper_available(&self) -> bool {
        self.helper_available(plan::de_helper_path(), plan::DE_POLICY_PATH)
    }

    /// Порт `_pkg_helper_available` (операции пакетного менеджера).
    pub fn pkg_helper_available(&self) -> bool {
        self.helper_available(plan::pkg_helper_path(), plan::PKG_POLICY_PATH)
    }

    /// Порт `_stage_helper_available` (раскладка файлов в /usr/share).
    pub fn stage_helper_available(&self) -> bool {
        self.helper_available(plan::stage_helper_path(), plan::STAGE_POLICY_PATH)
    }

    /// Порт `privileged_pkg_cmd`: argv `pkexec rexon-pkg-helper …` либо
    /// `None` — тогда вызывающий откатывается на сырой `pkexec <pm> …`.
    pub fn privileged_pkg_cmd(
        &self,
        family: PkgFamily,
        op: &str,
        packages: &[String],
    ) -> Option<Vec<String>> {
        plan::privileged_pkg_cmd(family, op, packages, self.pkg_helper_available())
    }

    /// Порт `privileged_stage_cmd`: argv `pkexec rexon-stage-helper …` либо
    /// `None` (мягкий откат на сырой путь).
    pub fn privileged_stage_cmd(
        &self,
        category: &str,
        name: &str,
        staged: &str,
    ) -> Option<Vec<String>> {
        let is_dir = !staged.is_empty() && self.io.is_dir(staged);
        let real = if staged.is_empty() {
            None
        } else {
            self.io.realpath(staged)
        };
        plan::privileged_stage_cmd(
            category,
            name,
            staged,
            is_dir,
            real.as_deref(),
            self.stage_helper_available(),
        )
    }

    // ── Экран входа (display manager) ────────────────────────────────────

    /// Порт `_dm_symlink_target`: `(realpath симлинка, жива ли цель)`.
    ///
    /// `(None, false)` — симлинка нет вовсе. `(Some(цель), false)` — симлинк
    /// ВИСЯЧИЙ: после `pacman -R sddm` (в том числе нашего `remove_de` для
    /// KDE и LXQt) он остаётся в `/etc`, а юнита за ним нет. Что с этим
    /// делать, решает ядро ([`plan::dm_symlink_unit`]); имя мёртвой цели
    /// отдаётся наружу, чтобы человеку было что сказать.
    pub fn dm_symlink_target(&self) -> (Option<String>, bool) {
        let link = plan::DM_SYMLINK;
        // `exists` разыменовывает симлинк и на висячем даёт false — поэтому
        // спрашиваем сразу realpath: он отвечает и по мёртвой цели.
        let Some(target) = self.io.realpath(link) else {
            return (None, false);
        };
        if target == link && !self.io.exists(link) {
            // realpath ничего не разрешил и файла нет — симлинка нет.
            return (None, false);
        }
        let alive = self.io.exists(&target);
        (Some(target), alive)
    }

    /// Порт `_dm_is_enabled`: stdout `systemctl is-enabled <unit>.service`.
    ///
    /// Только ЧТЕНИЕ состояния юнита — ничего не включает и не выключает.
    /// Код возврата намеренно не смотрится: `systemctl is-enabled` печатает
    /// `disabled` с ненулевым кодом, и по коду ответ терялся бы.
    pub fn dm_is_enabled(&self, unit: &str) -> Option<String> {
        let argv = vec![
            "systemctl".to_string(),
            "is-enabled".to_string(),
            format!("{unit}.service"),
        ];
        let out = self.io.run(&argv, Some(5), false);
        if out.no_answer() {
            return None;
        }
        Some(out.stdout.trim().to_string())
    }

    /// Порт `active_login_manager`: имя активного юнита экрана входа (без
    /// `.service`). Решение и таблицы юнитов — ядро.
    pub fn active_login_manager(&self) -> String {
        let (target, alive) = self.dm_symlink_target();
        plan::active_login_manager_from(
            target.as_deref(),
            alive,
            self.io.which("systemctl"),
            &|u| self.dm_is_enabled(u),
        )
    }

    /// Активный экран входа без подстановки `sddm`: `None` — определить
    /// нечем. Решение — [`plan::active_login_manager_opt`].
    pub fn active_login_manager_opt(&self) -> Option<String> {
        let (target, alive) = self.dm_symlink_target();
        plan::active_login_manager_opt(
            target.as_deref(),
            alive,
            self.io.which("systemctl"),
            &|u| self.dm_is_enabled(u),
        )
    }

    /// Юнит-файлы, которые могут быть экранами входа: `(путь, текст)` в
    /// порядке приоритета каталогов [`plan::DM_UNIT_DIRS`]. Только чтение.
    ///
    /// Замаскированный юнит (ссылка на `/dev/null`) пропускается — он не
    /// перекрывает пакетный файл в списке; маску увидит проверка готовности.
    pub fn dm_unit_files(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for dir in plan::DM_UNIT_DIRS {
            let Some(mut names) = self.io.list_dir(dir) else { continue };
            names.sort();
            for name in names {
                let Some(unit) = plan::unit_name_from_path(&name) else { continue };
                if !plan::dm_unit_name_listable(unit) {
                    continue;
                }
                let path = format!("{dir}/{name}");
                if self.io.realpath(&path).as_deref() == Some("/dev/null") {
                    continue;
                }
                if !self.io.is_file(&path) {
                    continue;
                }
                let Some(text) = self.io.read_text(&path) else { continue };
                if plan::dm_unit_file_candidate(unit, &text) {
                    out.push((path, text));
                }
            }
        }
        out
    }

    /// Установленные экраны входа (юниты без `.service`). Решение —
    /// [`plan::dm_units_from_unit_files`].
    pub fn installed_login_managers(&self) -> Vec<String> {
        plan::dm_units_from_unit_files(&self.dm_unit_files())
    }

    /// Пробы для выбора механизма переключения экрана входа. Только
    /// чтение файлов и PATH, без команд.
    pub fn dm_backend_probe(&self) -> plan::DmBackendProbe {
        // display-manager.service — настоящий юнит (openSUSE), если какой-то
        // из путей разрешается в файл с этим же именем, а не в юнит DM.
        let dm_service_is_regular_file = [plan::DM_SYMLINK, "/usr/lib/systemd/system/display-manager.service"]
            .iter()
            .any(|p| {
                self.io.is_file(p)
                    && self
                        .io
                        .realpath(p)
                        .is_some_and(|r| plan::unit_name_from_path(&r) == Some("display-manager"))
            });
        let systemd_running = self.io.is_dir("/run/systemd/system");
        plan::DmBackendProbe {
            systemd_running,
            has_systemctl: self.io.which("systemctl"),
            x11_default_dm_file: self.io.is_file("/etc/X11/default-display-manager"),
            dm_service_is_regular_file,
            suse_dm_alternative: self.io.exists("/etc/alternatives/default-displaymanager"),
            other_init: if systemd_running {
                String::new()
            } else {
                plan::other_init_name(&|p| self.io.exists(p), &|n| self.io.which(n))
            },
        }
    }

    /// Есть ли программа: абсолютный путь — исполняемый файл, иначе PATH.
    pub fn program_exists(&self, program: &str) -> bool {
        if program.starts_with('/') {
            self.io.is_executable(program)
        } else {
            !program.is_empty() && self.io.which(program)
        }
    }

    /// Установленные приветствия LightDM (`/usr/share/xgreeters`).
    pub fn lightdm_greeters(&self) -> Vec<plan::DmGreeter> {
        let dir = plan::XGREETERS_DIR;
        let Some(mut names) = self.io.list_dir(dir) else { return Vec::new() };
        names.sort();
        let files: Vec<(String, String)> = names
            .into_iter()
            .filter(|n| n.ends_with(".desktop"))
            .filter_map(|n| {
                let path = format!("{dir}/{n}");
                self.io.read_text(&path).map(|t| (path, t))
            })
            .collect();
        plan::xgreeters_from(&files, &|exec| self.program_exists(exec))
    }

    /// `greeter-session` по `lightdm --show-config`: команда только читает
    /// конфиги и работает без root. `None` — не задано или не ответила.
    pub fn lightdm_greeter_session(&self) -> Option<String> {
        if !self.io.which("lightdm") {
            return None;
        }
        let argv = vec!["lightdm".to_string(), "--show-config".to_string()];
        let out = self.io.run(&argv, Some(5), true);
        if out.no_answer() {
            return None;
        }
        // LightDM 1.33 (Arch) печатает конфиг в stderr; смотрим оба потока.
        plan::lightdm_greeter_from_show_config(&format!("{}\n{}", out.stdout, out.stderr))
    }

    /// Есть ли системный пользователь сейчас или его объявляет `sysusers.d`.
    fn dm_user_ok(&self, user: &str) -> bool {
        if self
            .io
            .read_text("/etc/passwd")
            .is_some_and(|t| plan::passwd_has_user(&t, user))
        {
            return true;
        }
        ["/usr/lib/sysusers.d", "/etc/sysusers.d"].iter().any(|dir| {
            self.io.list_dir(dir).is_some_and(|names| {
                names.iter().filter(|n| n.ends_with(".conf")).any(|n| {
                    self.io
                        .read_text(&format!("{dir}/{n}"))
                        .is_some_and(|t| plan::sysusers_declares(&t, user))
                })
            })
        })
    }

    /// Число сеансов (`*.desktop`) в каталогах.
    fn session_count(&self, dirs: &[&str]) -> usize {
        dirs.iter()
            .filter_map(|d| self.io.list_dir(d))
            .map(|names| names.iter().filter(|n| n.ends_with(".desktop")).count())
            .sum()
    }

    /// Тексты конфигов SDDM в порядке слияния: пакетные `conf.d`, свои
    /// `conf.d`, затем `/etc/sddm.conf`.
    fn sddm_conf_texts(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for dir in [plan::SDDM_DISTRO_CONF_D, plan::SDDM_CONF_D] {
            out.extend(self.sddm_conf_entries(dir).into_iter().map(|(_, t)| t));
        }
        if let Some(t) = self.io.read_text("/etc/sddm.conf") {
            out.push(t);
        }
        out
    }

    /// Факты для проверки готовности одного экрана входа. Только чтение
    /// файлов и PATH; единственная команда — `lightdm --show-config` (для
    /// LightDM), она тоже только читает.
    pub fn dm_facts(
        &self,
        unit: &str,
        spec: Option<&plan::DmSpec>,
        exec_start: &[String],
        family: PkgFamily,
    ) -> plan::DmFacts {
        let binary = exec_start.first().cloned().unwrap_or_default();
        let masked = self
            .io
            .realpath(&format!("/etc/systemd/system/{unit}.service"))
            .as_deref()
            == Some("/dev/null");
        let mut facts = plan::DmFacts {
            binary_exists: self.program_exists(&binary),
            masked,
            needs_xorg: spec.is_some_and(|s| s.needs_xorg),
            xorg: self.io.which("Xorg")
                || self.io.is_executable("/usr/bin/Xorg")
                || self.io.is_executable("/usr/lib/Xorg"),
            x_sessions: self.session_count(&["/usr/share/xsessions", "/usr/local/share/xsessions"]),
            wayland_sessions: self.session_count(&[
                "/usr/share/wayland-sessions",
                "/usr/local/share/wayland-sessions",
            ]),
            ..Default::default()
        };
        let mut user = spec.and_then(|s| s.user).map(String::from);
        if let Some(s) = spec {
            facts.pam = s.pam.to_string();
        }
        // greetd и всё, что запускается через него (cosmic-greeter): своё
        // приветствие и пользователь — из СВОЕГО конфига.
        if exec_start.first().is_some_and(|b| b.rsplit('/').next() == Some("greetd")) {
            let config = self.io.read_text(&plan::greetd_config_path(exec_start)).unwrap_or_default();
            let command = plan::greetd_command_kind(&config);
            let program_exists = match &command {
                plan::GreetdCommand::Graphical(p) => self.program_exists(p),
                _ => false,
            };
            if let Some(u) = plan::greetd_session_user(&config) {
                user = Some(u);
            }
            facts.greetd = Some(plan::GreetdFacts { command, program_exists });
        }
        match unit {
            "lightdm" => {
                facts.lightdm = Some(plan::LightdmFacts {
                    greeter_session: self.lightdm_greeter_session(),
                    default_greeter: plan::lightdm_default_greeter(family).map(String::from),
                    greeters: self.lightdm_greeters(),
                });
            }
            "sddm" => {
                let texts = self.sddm_conf_texts();
                let confs: Vec<&str> = texts.iter().map(String::as_str).collect();
                let theme_dir = plan::ini_last_value(&confs, "Theme", "ThemeDir")
                    .unwrap_or_else(|| plan::SDDM_THEMES_DIR.to_string());
                let current = plan::ini_last_value(&confs, "Theme", "Current");
                let meta = current.as_deref().and_then(|t| {
                    self.io.read_text(&format!("{}/{t}/metadata.desktop", theme_dir.trim_end_matches('/')))
                });
                let theme = plan::sddm_theme_ready(current.as_deref(), meta.as_deref(), &|g| {
                    self.program_exists(g)
                });
                let x11 = plan::sddm_uses_x11(&confs);
                facts.needs_xorg = x11;
                let wayland_compositor = (!x11).then(|| {
                    let c = plan::sddm_wayland_compositor(&confs);
                    let ok = self.program_exists(&c);
                    (c, ok)
                });
                facts.sddm = Some(plan::SddmFacts { theme, wayland_compositor });
            }
            _ => {}
        }
        if !facts.pam.is_empty() {
            facts.pam_exists = ["/etc/pam.d", "/usr/lib/pam.d"]
                .iter()
                .any(|d| self.io.is_file(&format!("{d}/{}", facts.pam)));
        }
        if let Some(u) = user {
            facts.user_ok = self.dm_user_ok(&u);
            facts.user = Some(u);
        }
        facts
    }

    /// Экраны входа на этой системе для раздела «Экран входа в систему»:
    /// установленные, какой основной, готов ли каждый стать основным.
    /// Только чтение — ничего не включает и не выключает.
    ///
    /// Без systemd список — по программам из таблицы, основной — по файлам
    /// выбора init, готовность — `Unsupported` (кнопки нет).
    pub fn login_managers(&self) -> Vec<plan::LoginManagerInfo> {
        let family = self.family();
        let backend = self.dm_switch_backend();
        let label = |unit: &str| {
            plan::login_manager_spec(unit).map_or(String::new(), |s| s.label.to_string())
        };
        if let plan::DmSwitchBackend::Unsupported(_) = backend {
            let active = plan::dm_from_init_files(
                self.io.read_text("/etc/conf.d/display-manager").as_deref(),
                self.io.read_text("/etc/X11/default-display-manager").as_deref(),
            );
            return plan::LOGIN_MANAGER_SPECS
                .iter()
                .filter_map(|s| {
                    let path = self.io.which_path(s.binary)?;
                    Some(plan::LoginManagerInfo {
                        unit: s.unit.to_string(),
                        label: s.label.to_string(),
                        binary: Some(path),
                        present: true,
                        active: active.as_deref() == Some(s.unit),
                        readiness: plan::DmReadiness::Unsupported,
                        warnings: Vec::new(),
                    })
                })
                .collect();
        }
        let files = self.dm_unit_files();
        let mut units = plan::dm_units_from_unit_files(&files);
        let active = self.active_login_manager_opt();
        if let Some(a) = &active {
            if !units.contains(a) {
                units.insert(0, a.clone());
            }
        }
        units
            .into_iter()
            .map(|unit| {
                let text = files
                    .iter()
                    .find(|(p, _)| plan::unit_name_from_path(p) == Some(unit.as_str()))
                    .map(|(_, t)| t.as_str())
                    .unwrap_or_default();
                let exec = plan::unit_exec_start(text).unwrap_or_default();
                let spec = plan::login_manager_spec(&unit);
                let facts = self.dm_facts(&unit, spec, &exec, family);
                plan::LoginManagerInfo {
                    label: label(&unit),
                    binary: exec.first().cloned(),
                    present: facts.binary_exists,
                    active: active.as_deref() == Some(unit.as_str()),
                    readiness: plan::dm_readiness(spec, &backend, &facts),
                    warnings: plan::dm_warnings(&facts),
                    unit,
                }
            })
            .collect()
    }

    /// Механизм выбора экрана входа на этой системе.
    pub fn dm_switch_backend(&self) -> plan::DmSwitchBackend {
        plan::dm_switch_backend(self.family(), &self.dm_backend_probe())
    }

    /// Неизменяемая система (Silverblue, Kinoite, Bazzite, SteamOS…): в
    /// первой версии Rexon экран входа там не переключает.
    fn distro_immutable(&self) -> bool {
        DistroDetector::from_os_release_files(
            self.io.read_text("/etc/os-release").as_deref(),
            self.io.read_text("/usr/lib/os-release").as_deref(),
        )
        .is_immutable()
    }

    /// Какие из `units` работают СЕЙЧАС (`systemctl is-active` одним
    /// вызовом, строка ответа на юнит). Пусто — спросить нечем.
    fn running_login_managers(&self, units: &[&str]) -> Vec<String> {
        if units.is_empty() || !self.io.which("systemctl") {
            return Vec::new();
        }
        let mut argv = vec!["systemctl".to_string(), "is-active".to_string()];
        argv.extend(units.iter().map(|u| format!("{u}.service")));
        let out = self.io.run(&argv, Some(5), true);
        if out.no_answer() {
            return Vec::new();
        }
        units
            .iter()
            .zip(out.stdout.lines())
            .filter(|(_, state)| state.trim() == "active")
            .map(|(u, _)| (*u).to_string())
            .collect()
    }

    /// Снимок раздела «Менеджер входа» для интерфейса: установленные экраны
    /// входа, основной и работающий сейчас, можно ли и с каким текстом
    /// предлагать «Сделать основным». Только чтение: команды — `lightdm
    /// --show-config` и `systemctl is-active`. Решение —
    /// [`login_screen_view`].
    pub fn login_screen_view(&self) -> LoginScreenView {
        let family = self.family();
        let backend = self.dm_switch_backend();
        let list = self.login_managers();
        let running = if backend.switch_supported() {
            let units: Vec<&str> = list.iter().map(|m| m.unit.as_str()).collect();
            self.running_login_managers(&units)
        } else {
            Vec::new()
        };
        let facts = LoginScreenFacts {
            family,
            helper: self.pkg_helper_available(),
            immutable: self.distro_immutable(),
            running,
            previous: self.io.read_text(plan::PREVIOUS_DM_FILE),
        };
        login_screen_view(&list, &backend, &facts)
    }

    /// «Сделать основным»: `(сделано, текст для человека)`.
    ///
    /// Защита от дурака на стороне ядра (правило #13): интерфейс кнопку
    /// уже запер, но и здесь всё проверяется заново по живой системе —
    /// механизм, неизменяемость, есть ли такой экран входа, не основной ли
    /// он уже, готов ли и выбрано ли приветствие из установленных. На любом
    /// отказе пароль не спрашивается. Дальше — ОДИН вызов `pkexec`
    /// помощника `switch-dm`, который повторяет проверки сам и при
    /// расхождении откатывает; сырого `pkexec systemctl` нет.
    pub fn switch_login_manager(&self, unit: &str, greeter: Option<&str>) -> (bool, String) {
        let unit = unit.trim();
        let greeter = greeter.map(str::trim).filter(|g| !g.is_empty());
        let backend = self.dm_switch_backend();
        if let Some(note) = plan::login_manager_unsupported_note(&backend, None) {
            return (false, note);
        }
        if self.distro_immutable() {
            return (false, login_manager_immutable_note());
        }
        let list = self.login_managers();
        let Some(info) = list.iter().find(|m| m.unit == unit) else {
            return (false, plan::login_manager_switch_failed(&login_manager_missing_reason()));
        };
        if info.active {
            return (false, login_manager_already_default(&info.label, unit));
        }
        let greeter = match &info.readiness {
            plan::DmReadiness::Blocked(reason) => {
                return (false, plan::login_manager_switch_failed(reason));
            }
            plan::DmReadiness::Unsupported => {
                return (false, plan::login_manager_switch_failed(&login_manager_cant_switch_reason()));
            }
            plan::DmReadiness::NeedsGreeterChoice(greeters) => match greeter {
                Some(g) if greeters.iter().any(|x| x.id == g) => Some(g),
                _ => return (false, login_manager_choose_greeter()),
            },
            // Готовый LightDM: приветствие можно сменить, но только на
            // установленное. Остальным приветствие не передаётся вовсе.
            plan::DmReadiness::Ready if unit == "lightdm" => match greeter {
                Some(g) if self.lightdm_greeters().iter().any(|x| x.id == g) => Some(g),
                Some(_) => return (false, login_manager_choose_greeter()),
                None => None,
            },
            plan::DmReadiness::Ready => None,
        };
        let cmd = plan::switch_dm_cmd(self.family(), unit, greeter, self.pkg_helper_available());
        let old = list.iter().find(|m| m.active).map(|m| m.unit.clone());
        // Сеанс прав окна (тот же один пароль); его нет — прежний помощник
        // пакетов. Сырого пути у переключения нет ни там, ни там (З5).
        let op = crate::privilege::op::PrivOp::SwitchDm {
            family: self.family(),
            unit: unit.to_string(),
            greeter: greeter.map(str::to_string),
        };
        let out = match (self.io.run_priv(&op, &[], None), &cmd) {
            (Some(out), _) => out,
            // Язык сообщений помощника — по локали человека: `lang_c` выключен.
            (None, Some(cmd)) => self.io.run(cmd, Some(300), false),
            (None, None) => return (false, plan::login_manager_needs_helper()),
        };
        // «Not authorized» — пароль не подтверждён, даже если код иной.
        let rc = if out.stderr.contains("Not authorized") { Some(126) } else { out.rc() };
        // Ручная команда — та же, что у помощника пакетов.
        let manual_argv = cmd.unwrap_or_else(|| {
            plan::switch_dm_cmd(self.family(), unit, greeter, true).unwrap_or_default()
        });
        let manual = format!("sudo {}", manual_argv.get(1..).unwrap_or_default().join(" "));
        let (ok, mut message) = plan::switch_dm_outcome(rc, &out.stdout, &out.stderr, unit, &manual);
        if ok {
            if let Some(hint) = old.and_then(|o| plan::login_manager_rollback_hint(&o, &backend)) {
                message.push_str("\n\n");
                message.push_str(&hint);
            }
        }
        (ok, message)
    }

    /// Порт `_display_manager_present`: ЧЕСТНЫЙ ответ «экран входа есть».
    ///
    /// Им пользуется защита от дурака `ensure_display_manager`: ставить SDDM
    /// ТОЛЬКО когда экрана входа реально нет. Висячий симлинк экраном входа
    /// не считается — иначе защита молчит ровно тогда, когда нужна.
    pub fn display_manager_present(&self) -> bool {
        let (target, alive) = self.dm_symlink_target();
        plan::display_manager_present_from(
            target.as_deref(),
            alive,
            self.io.which("systemctl"),
            &|u| self.dm_is_enabled(u),
        )
    }

    // ── Plymouth (загрузочная заставка) ──────────────────────────────────

    /// Порт `_plymouth_theme_from_default_link`: тема по ссылке
    /// `default.plymouth` (нужна там, где бинарь вырезан из пакета — Ubuntu).
    pub fn plymouth_theme_from_default_link(&self) -> Option<String> {
        let link = format!("{}/default.plymouth", plan::PLYMOUTH_THEMES_DIR);
        let target = self
            .io
            .realpath(&link)
            .filter(|t| self.io.exists(t));
        plan::plymouth_theme_from_link(target.as_deref())
    }

    /// Порт `detect_plymouth_theme`: текущая активная тема либо `None`.
    /// Ссылка читается ЛЕНИВО — только когда имени из бинаря не вышло.
    pub fn detect_plymouth_theme(&self) -> Option<String> {
        if !self.io.which("plymouth-set-default-theme") {
            return self.plymouth_theme_from_default_link();
        }
        let out = self
            .io
            .run(&["plymouth-set-default-theme".to_string()], None, false);
        plan::plymouth_theme_from_output(out.rc(), &out.stdout)
            .or_else(|| self.plymouth_theme_from_default_link())
    }

    /// Порт `list_plymouth_themes`: установленные темы. `[]` при любой
    /// неудаче. Темой подкаталог делает файл `<тема>.plymouth` внутри —
    /// ровно так их перечисляет сам plymouth.
    pub fn list_plymouth_themes(&self) -> Vec<String> {
        let dir = plan::PLYMOUTH_THEMES_DIR;
        let Some(names) = self.io.list_dir(dir) else {
            return Vec::new();
        };
        let entries: Vec<(String, bool)> = names
            .into_iter()
            .map(|n| {
                let is_dir = self.io.is_dir(&format!("{dir}/{n}"));
                (n, is_dir)
            })
            .collect();
        let probes: Vec<(String, bool)> = plan::plymouth_theme_candidates(&entries)
            .into_iter()
            .map(|t| {
                let file = plan::plymouth_theme_file_in(dir, &t);
                let present = self.io.is_file(&file);
                (t, present)
            })
            .collect();
        plan::plymouth_installed_themes(&probes)
    }

    /// Порт `_plymouth_package_default`: дефолт из `plymouthd.defaults`.
    pub fn plymouth_package_default(&self) -> Option<String> {
        let text = self.io.read_text(plan::PLYMOUTH_DEFAULTS);
        plan::plymouth_package_default(text.as_deref())
    }

    /// Порт `target_plymouth_theme`: родной дефолт ЭТОГО дистрибутива.
    /// Шаги ленивы в порядке оригинала: брендинг → `plymouthd.defaults` →
    /// запасные кандидаты.
    pub fn target_plymouth_theme(&self) -> Option<String> {
        let installed = self.list_plymouth_themes();
        plan::target_plymouth_theme(
            &installed,
            &|p| self.is_pkg_installed(p),
            &|| self.plymouth_package_default(),
        )
    }

    /// Порт `plymouth_reset_command`: человекочитаемая команда сброса (для
    /// показа). `None` — сбрасывать некуда.
    pub fn plymouth_reset_command(&self, target: Option<&str>) -> Option<String> {
        let owned;
        let target = match target {
            Some(t) => t,
            None => {
                owned = self.target_plymouth_theme()?;
                &owned
            }
        };
        if target.is_empty() {
            return None;
        }
        plan::plymouth_reset_command(target, self.io.which("plymouth-set-default-theme"))
    }

    /// Порт `reset_plymouth_to_default`: сброс заставки одним `pkexec`.
    ///
    /// Идемпотентно: текущая тема == целевой → ничего не запускается.
    /// ⚠️ `-R` пересобирает initramfs, эффект виден только после reboot.
    pub fn reset_plymouth_to_default(&self, target: Option<&str>) -> (bool, String) {
        let owned;
        let target: Option<&str> = match target {
            Some(t) => Some(t),
            None => {
                owned = self.target_plymouth_theme();
                owned.as_deref()
            }
        };
        // Ленивость оригинала: без цели тему не читаем, а при наличии
        // plymouth-set-default-theme про update-alternatives не спрашиваем.
        let has_target = target.map(|t| !t.is_empty()).unwrap_or(false);
        let current = if has_target {
            self.detect_plymouth_theme()
        } else {
            None
        };
        let has_tool = has_target && self.io.which("plymouth-set-default-theme");
        let has_alt = has_target && !has_tool && self.io.which("update-alternatives");

        let plan_ = plan::plymouth_reset_plan(target, current.as_deref(), has_tool, has_alt, &|p| {
            self.io.is_file(p)
        });
        if let Some(pre) = plan::plymouth_reset_precheck(&plan_) {
            return (pre.ok, pre.message);
        }
        let plan::PlymouthReset::Run { cmd, shell, target } = plan_ else {
            // precheck отдаёт None только для Run — сюда попасть нельзя.
            unreachable!("plymouth_reset_precheck: не Run без ответа");
        };
        let out = self.io.run(&cmd.argv, cmd.timeout_secs, cmd.lang_c);
        let answer = plan::plymouth_reset_outcome(
            &target,
            &shell,
            out.rc(),
            &out.stdout,
            &out.stderr,
        );
        (answer.ok, answer.message)
    }

    // ── SDDM (экран входа) ───────────────────────────────────────────────

    /// Порт `_sddm_conf_entries`: пары (имя, содержимое) `*.conf` каталога в
    /// порядке слияния SDDM (лексикографическом — поздние перекрывают
    /// ранние). Нечитаемый файл остаётся в списке с ПУСТЫМ содержимым: он
    /// может опознаться по одному имени (токен форк-темы).
    pub fn sddm_conf_entries(&self, conf_d: &str) -> Vec<(String, String)> {
        let Some(names) = self.io.list_dir(conf_d) else {
            return Vec::new();
        };
        let reads: Vec<(String, Option<String>)> = plan::sddm_conf_read_order(&names)
            .into_iter()
            .map(|name| {
                let text = self.io.read_text(&format!(
                    "{}/{name}",
                    conf_d.trim_end_matches('/')
                ));
                (name, text)
            })
            .collect();
        plan::sddm_conf_entries(&reads)
    }

    /// Порт `_sddm_kde_settings_text`: содержимое `<conf_d>/kde_settings.conf`.
    /// Его кладёт KDE/дистро — это «как из коробки», НЕ Rexon: читается
    /// отдельно от общего скана и никогда не удаляется.
    pub fn sddm_kde_settings_text(&self, conf_d: &str) -> Option<String> {
        self.io.read_text(&plan::sddm_kde_settings_path(conf_d))
    }

    /// Порт `_sddm_read_current_theme`: текущая тема SDDM из `*.conf`.
    pub fn sddm_read_current_theme(&self, conf_d: &str) -> Option<String> {
        let entries = self.sddm_conf_entries(conf_d);
        plan::sddm_current_theme_from(&entries)
    }

    /// Порт `sddm_distro_default_theme`: дистро-дефолтная тема, без хардкода
    /// Breeze.
    pub fn sddm_distro_default_theme(&self) -> Option<String> {
        let distro = self.sddm_conf_entries(plan::SDDM_DISTRO_CONF_D);
        let kde = self.sddm_kde_settings_text(plan::SDDM_CONF_D);
        plan::sddm_distro_default_theme(&distro, kde.as_deref())
    }

    /// Порт `sddm_reset_plan`: что удалим и к какой теме вернёмся — БЕЗ
    /// применения. Пустой `to_remove` = идемпотентный no-op.
    pub fn sddm_reset_plan(&self, conf_d: &str) -> plan::SddmResetPlan {
        let entries = self.sddm_conf_entries(conf_d);
        let distro = self.sddm_conf_entries(plan::SDDM_DISTRO_CONF_D);
        // kde_settings.conf берётся из ТОГО ЖЕ каталога, что сканируем:
        // иначе предпросмотр на подставленном каталоге читал бы живой /etc.
        let kde = self.sddm_kde_settings_text(conf_d);
        plan::sddm_reset_plan(&entries, &distro, kde.as_deref())
    }

    /// Исполнить УЖЕ посчитанный план сброса SDDM: один `pkexec sh -c
    /// "rm -f …"` по конкретным путям. `(ok, сообщение, удалённые)`.
    ///
    /// ⭐ Отдельно от [`Self::sddm_reset_plan`] намеренно — см. пункт 1 в
    /// шапке модуля: маршрут показывает человеку план и исполняет ровно его,
    /// а не пересчитанный второй скан каталога.
    pub fn apply_sddm_reset(
        &self,
        reset_plan: &plan::SddmResetPlan,
        conf_d: &str,
    ) -> (bool, String, Vec<String>) {
        match plan::sddm_reset_command(reset_plan, conf_d) {
            plan::SddmReset::AlreadyStock => (
                true,
                plan::sddm_already_stock_message(),
                Vec::new(),
            ),
            plan::SddmReset::Run { cmd, shell, removed, default_theme } => {
                let out = self.io.run(&cmd.argv, cmd.timeout_secs, cmd.lang_c);
                let answer = plan::sddm_reset_outcome(
                    &removed,
                    default_theme.as_deref(),
                    &shell,
                    out.rc(),
                    &out.stdout,
                    &out.stderr,
                );
                (answer.ok, answer.message, answer.removed)
            }
        }
    }

    /// Порт `reset_sddm_to_default`: сброс экрана входа к стоку дистро.
    /// Идемпотентно: удалять нечего → `(true, "уже на стоке", [])`.
    pub fn reset_sddm_to_default(&self, conf_d: &str) -> (bool, String, Vec<String>) {
        let reset_plan = self.sddm_reset_plan(conf_d);
        self.apply_sddm_reset(&reset_plan, conf_d)
    }

    // ── Тяжёлые форки Plasma (AeroThemePlasma / AeroShell) ───────────────

    /// Порт `detect_plasma_forks`: установленные форки Plasma. Только Arch —
    /// форки живут исключительно в AUR; на прочих семействах `[]`.
    pub fn detect_plasma_forks(&self) -> Vec<String> {
        plan::filter_plasma_forks(&self.installed_packages(PkgFamily::Arch))
    }

    /// Порт `plasma_fork_recovery_command`: человекочитаемая команда полного
    /// отката на сток (для показа). `None` — форков нет либо шагов не вышло.
    ///
    /// `memo` — общая память проб `pacman -Si`: один и тот же стоковый пакет
    /// всплывает у нескольких форков, а полный откат проходит по набору
    /// дважды.
    fn recovery_command_with(
        &self,
        forks: &[String],
        memo: &RefCell<plan::TargetExistsMemo>,
    ) -> Option<String> {
        plan::plasma_fork_recovery_command(
            forks,
            &|p| self.pkg_meta_stdout(p, PkgFamily::Arch),
            &|p| {
                memo.borrow_mut()
                    .answer(p, &|n| self.repo_target_exists(n, PkgFamily::Arch))
            },
        )
    }

    /// Порт `plasma_fork_recovery_command` с собственной памятью проб.
    /// `forks = None` — набор определяется живой пробой пакетов.
    pub fn plasma_fork_recovery_command(&self, forks: Option<&[String]>) -> Option<String> {
        let owned;
        let forks = match forks {
            Some(f) => f,
            None => {
                owned = self.detect_plasma_forks();
                &owned
            }
        };
        let memo = RefCell::new(plan::TargetExistsMemo::new());
        self.recovery_command_with(forks, &memo)
    }

    /// Порт `plasma_fork_reset_preview`: что будет сделано — БЕЗ транзакции.
    /// Пробы только на чтение (`pacman -Qi`/`-Si`), `pkexec` не трогается.
    pub fn plasma_fork_reset_preview(&self, forks: Option<&[String]>) -> (bool, String) {
        let owned;
        let forks = match forks {
            Some(f) => f,
            None => {
                owned = self.detect_plasma_forks();
                &owned
            }
        };
        let answer = plan::plasma_fork_preview(
            forks,
            &|p| self.pkg_meta_stdout(p, PkgFamily::Arch),
            &|p| self.repo_target_exists(p, PkgFamily::Arch),
        );
        (answer.ok, answer.message)
    }

    /// Порт `remove_plasma_forks`: снять форки и вернуть сток ОДНОЙ
    /// pkexec-транзакцией (`pacman -Rns`, затем `pacman -S <сток>`).
    ///
    /// ⛔ Ветка `Blocked` — отказ ДО pacman: удалять форк, не имея чем
    /// вернуть сток, и есть та самая полусломанная система. Порядок шагов
    /// менять нельзя (автономные форки зависят от провайдера).
    /// ⚠️ После успеха нужен logout/login.
    pub fn remove_plasma_forks(&self, forks: Option<&[String]>) -> (bool, String) {
        let owned;
        let forks = match forks {
            Some(f) => f,
            None => {
                owned = self.detect_plasma_forks();
                &owned
            }
        };
        let memo = RefCell::new(plan::TargetExistsMemo::new());
        let qi = |p: &str| self.pkg_meta_stdout(p, PkgFamily::Arch);
        let exists = |p: &str| {
            memo.borrow_mut()
                .answer(p, &|n| self.repo_target_exists(n, PkgFamily::Arch))
        };
        let reset = plan::plasma_fork_reset_plan(forks, &qi, &exists);
        let (cmd, shell) = match reset {
            plan::PlasmaForkReset::NoForks { ok, message }
            | plan::PlasmaForkReset::NoActions { ok, message }
            | plan::PlasmaForkReset::Blocked { ok, message, .. } => return (ok, message),
            plan::PlasmaForkReset::Run { cmd, shell, .. } => (cmd, shell),
        };
        let out = self.io.run(&cmd.argv, cmd.timeout_secs, cmd.lang_c);
        // Обе восстановительные команды считаются ЛЕНИВО и только в своей
        // ветке: «что собирались выполнить» — при отказе авторизации (система
        // не тронута), «что осталось» — пересчётом по ЖИВОМУ состоянию после
        // падения, ведь часть шагов могла пройти.
        let answer = plan::plasma_fork_reset_outcome(
            &shell,
            out.rc(),
            &out.stdout,
            &out.stderr,
            &|| self.recovery_command_with(forks, &memo),
            &|| self.plasma_fork_recovery_command(None),
        );
        (answer.ok, answer.message)
    }

    // ── Завершение сеанса ────────────────────────────────────────────────

    /// Порт `logout_session`: завершить текущую сессию (показать экран
    /// входа). НЕ выключает компьютер. Первая успешная команда выигрывает.
    ///
    /// ⭐ Таймаут назван таймаутом: у оригинала это была отдельная ветка
    /// `except`, а «не запустилось» пряталось в общий `except Exception`.
    pub fn logout_session(&self) -> (bool, String) {
        let cmds = plan::logout_cmds(
            &self.io.env("XDG_CURRENT_DESKTOP"),
            &self.io.env("DESKTOP_SESSION"),
            &self.io.env("XDG_SESSION_ID"),
        );
        let mut last_err = String::new();
        for cmd in &cmds {
            let out = self.io.run(&cmd.argv, cmd.timeout_secs, cmd.lang_c);
            let head = cmd.argv.first().cloned().unwrap_or_default();
            match out.outcome {
                RunOutcome::Ok => return (true, "ok".to_string()),
                RunOutcome::Failed => last_err = out.stderr.trim().to_string(),
                RunOutcome::Timeout => last_err = format!("{head}: timeout"),
                // TRANSLATORS: %1 is a command name.
                RunOutcome::Error => last_err = crate::i18n::trf("%1: didn't start", &[&head]),
            }
        }
        if last_err.is_empty() {
            (false, crate::i18n::tr("Couldn't log out"))
        } else {
            (false, last_err)
        }
    }

    // ── Аварийный откат оформления (`--recover-kde`) ─────────────────────
    //
    // Порт `rexon/app.py:682` и девятнадцати его помощников. Ярлык
    // «Восстановить оформление рабочего стола» и пункт меню окна ведут
    // сюда, и человек нажимает их тогда, когда оформление УЖЕ сломано —
    // значит маршрут обязан работать без Qt вовсе и не падать ни на одном
    // шаге. Отсюда его форма: каждый шаг best-effort, каждый говорит
    // словами, что сделал, и ни один не отменяет следующие.
    //
    // Решения и все тексты — [`crate::app`]; здесь только порядок и походы
    // наружу.

    /// Корень пользовательских конфигов ЭТОГО процесса.
    fn config_home(&self) -> String {
        let xdg = self.io.env("XDG_CONFIG_HOME");
        let home = self.io.env("HOME");
        utils::config_home(
            if xdg.is_empty() { None } else { Some(xdg.as_str()) },
            &home,
        )
    }

    /// Путь раскладки рабочих столов и панелей Plasma.
    fn appletsrc_path(&self) -> String {
        format!(
            "{}/plasma-org.kde.plasma.desktop-appletsrc",
            self.config_home()
        )
    }

    /// Маршрут `--recover-kde` целиком. → код возврата процесса (0/1).
    ///
    /// `out` — приёмник строк для человека (в продукте `println!`): порядок
    /// и слова ровно те же, что печатал оригинал.
    ///
    /// ⛔ ГРАНИЦА. Своими руками этот маршрут меняет
    /// `plasma-org.kde.plasma.desktop-appletsrc` (хирургия панели, с
    /// резервной копией рядом) и снимает слои шрифтов Rexon — только наши
    /// отмеченные блоки и наши файлы, по плану ядра
    /// (`qt_settings::font_layers_reset_plan`). Всё остальное меняется
    /// чужими командами:
    /// `plasma-apply-*`, `kwriteconfig`, `lookandfeeltool`, `systemctl
    /// --user`, и root-ное — только `pkexec` из планов ядра.
    pub fn recover_kde(&self, out: &mut dyn FnMut(&str)) -> i32 {
        // Защита от дурака: восстановление умеет только Plasma, и на чужом
        // рабочем столе мы говорим это вслух, а не делаем вид, что сработали.
        let xdg = self.io.env("XDG_CURRENT_DESKTOP");
        if !app::is_plasma_desktop(&xdg) {
            for line in app::not_plasma_lines(&xdg) {
                out(&line);
            }
            return 1;
        }

        // Адрес резервной копии темы — общий с окном
        // ([`crate::window_menu::recover_backup_path`]): диалог называет
        // человеку ту тему, которую этот маршрут и применит.
        let home = self.io.env("HOME");
        let xdg = self.io.env("XDG_CONFIG_HOME");
        let backup = crate::window_menu::recover_backup_path(
            &home,
            if xdg.is_empty() { None } else { Some(xdg.as_str()) },
        );
        let saved = self.io.read_text(&backup);
        let target = app::recover_theme_target(saved.as_deref());

        // Тяжёлые форки Plasma подменяют сами бинарники Plasma и сбросом
        // темы НЕ лечатся — предупреждаем ДО сброса и даём команду полного
        // отката. Проба только на чтение, `pkexec` здесь не трогается.
        let forks = self.detect_plasma_forks();
        if !forks.is_empty() {
            let (can_reset, preview) = self.plasma_fork_reset_preview(Some(&forks));
            for line in app::fork_warning_lines(&forks, can_reset, &preview) {
                out(&line);
            }
        }

        out(&app::recovering_line(&target));
        let applied = self.io.run(
            &app::lookandfeel_apply_argv(&target),
            Some(app::LOOKANDFEEL_TIMEOUT_SECS),
            false,
        );
        if applied.rc() != Some(0) {
            // ⭐ Починка при переносе: у оригинала отсутствующий
            // `plasma-apply-lookandfeel` поднимал FileNotFoundError прямо из
            // `main()` — аварийный маршрут падал трассировкой стека ровно
            // там, где обязан был объяснить, что происходит.
            if applied.no_answer() {
                out(app::LOOKANDFEEL_MISSING_LINE);
            } else {
                out(&app::apply_failed_line(&applied.stderr));
            }
            return 1;
        }
        self.io.run(
            &app::colorscheme_apply_argv(),
            Some(app::COLORSCHEME_TIMEOUT_SECS),
            false,
        );
        out(&app::restored_line(&target));

        self.recover_kde_config(&target, &forks, out);
        self.recover_kde_wallpaper(out);
        0
    }

    /// Порт `_recover_kde_config`: полный сброс конфига (часть 2).
    ///
    /// Сбросы к стоку — ВСЕГДА (шрифты, эффекты KWin, Kvantum, загрузочная
    /// заставка и экран входа): все они идемпотентны, и кнопка одним
    /// нажатием возвращает состояние чистой установки. Агрессивное —
    /// чистка раскладки панели, способная стереть пользовательские виджеты,
    /// — только при форках Plasma либо их следах в конфиге.
    pub fn recover_kde_config(
        &self,
        target: &str,
        forks: &[String],
        out: &mut dyn FnMut(&str),
    ) {
        self.reset_kde_fonts(out);
        self.reset_kwin_theme_effects(out);
        self.reset_kvantum_if_aero(out);
        self.reset_plymouth_step(out);
        self.reset_sddm_step(out);

        let aggressive = !forks.is_empty() || self.detect_plasma_fork_residue();
        if !aggressive {
            return;
        }
        out(app::PANEL_AGGRESSIVE_LINE);
        // Сперва ХИРУРГИЯ: убрать только битые форк-апплеты, сохранив
        // пользовательскую панель. Их не нашлось, а раскладка всё равно
        // «аэро» (подменён сам контейнмент) — тогда полный `--resetLayout`.
        if self.remove_fork_applets(out) == 0 && self.appletsrc_has_fork_residue() {
            self.reset_panel_layout(target, out);
        }
    }

    /// Порт `_reset_kde_fonts`: снять переопределения шрифтов в kdeglobals.
    /// `--delete` снимает ключ, после чего Plasma берёт собственный дефолт —
    /// надёжнее, чем вписывать конкретный шрифт.
    ///
    /// Вместе с ключами снимаются и слои, которые кладёт страница «Шрифты»
    /// ([`Self::reset_font_layers`]): иначе меню и панели GTK- и
    /// Qt-программ, сглаживание и DPI так и остались бы нашими.
    pub fn reset_kde_fonts(&self, out: &mut dyn FnMut(&str)) {
        self.reset_font_layers();
        let Some(kw) = self.io.which_path(app::KWRITECONFIG_TOOL) else {
            return;
        };
        for cmd in app::kde_font_reset_argv(&kw) {
            self.io
                .run(&cmd, Some(app::KWRITECONFIG_TIMEOUT_SECS), false);
        }
        out(app::FONTS_RESET_LINE);
    }

    /// Снять слои шрифтов Rexon (`qt_settings::font_layers_reset_plan`):
    /// наши блоки в `gtk.css`, `fonts.conf`, `~/.Xresources`, таблицу стилей
    /// qt6ct/qt5ct с её строкой в `.conf`, записанные нами шрифты COSMIC.
    /// Чужое содержимое файлов остаётся. → сколько файлов не удалось вернуть
    /// (каждый — строкой в журнал).
    pub fn reset_font_layers(&self) -> usize {
        struct Fs<'b, IO: SystemIo + ?Sized>(&'b IO);
        impl<IO: SystemIo + ?Sized> qt_settings::FontLayerFs for Fs<'_, IO> {
            fn read(&self, path: &str) -> Option<String> {
                self.0.read_text(path)
            }
            fn is_link(&self, path: &str) -> bool {
                // Ссылка — если сам путь разрешается не туда, куда его
                // каталог (каталог настроек сам может быть ссылкой).
                let Some((dir, name)) = path.rsplit_once('/') else { return false };
                match (self.0.realpath(path), self.0.realpath(if dir.is_empty() { "/" } else { dir })) {
                    (Some(real), Some(real_dir)) => {
                        real != format!("{}/{name}", real_dir.trim_end_matches('/'))
                    }
                    _ => false,
                }
            }
        }
        let home = self.io.env("HOME");
        let plan = qt_settings::font_layers_reset_plan(&self.config_home(), &home, &Fs(self.io));
        let mut failed = 0;
        for undo in plan {
            let done = match &undo {
                qt_settings::FontLayerUndo::Write { path, text } => self.io.write_text(path, text),
                qt_settings::FontLayerUndo::Remove { path } => self.io.unlink_file(path),
                qt_settings::FontLayerUndo::Relink { path, target } => {
                    self.io.replace_with_symlink(path, target)
                }
            };
            if let Err(e) = done {
                failed += 1;
                self.log(format!("font layer reset: {undo:?}: {e}"));
            }
        }
        failed
    }

    /// Порт `_reset_kwin_theme_effects`: снять поимённые эффекты форк-тем и
    /// заставить живой KWin перечитать конфиг.
    pub fn reset_kwin_theme_effects(&self, out: &mut dyn FnMut(&str)) {
        let Some(kw) = self.io.which_path(app::KWRITECONFIG_TOOL) else {
            return;
        };
        for cmd in app::kwin_effect_reset_argv(&kw) {
            self.io
                .run(&cmd, Some(app::KWRITECONFIG_TIMEOUT_SECS), false);
        }
        if !self.kwin_reconfigure() {
            self.log(app::KWIN_RECONFIGURE_FAILED_LOG.to_string());
        }
        out(app::KWIN_EFFECTS_RESET_LINE);
    }

    /// Порт `utils.kwin_reconfigure`: попросить живой KWin перечитать
    /// конфиг. → получилось ли.
    ///
    /// ⭐ Починка при переносе. У оригинала запасной путь был мёртвым:
    /// `dbus_call` брал ПЕРВЫЙ доступный бинарь (`dbus-send`, иначе
    /// `qdbus`), а `_reset_kwin_reconfigure_fallback` спрашивал тот же
    /// резолвер заново — и либо получал тот же `dbus-send` и уходил ни с
    /// чем, либо получал `qdbus` и повторял ДОСЛОВНО ту же команду, которая
    /// только что не прошла. Здесь перебираются оба бинаря по очереди:
    /// не ответил `dbus-send` — пробуем `qdbus`, и только потом сдаёмся.
    /// Без этого правка kwinrc ложилась в файл, а живой KWin продолжал
    /// рисовать снятые эффекты до следующего входа в систему.
    pub fn kwin_reconfigure(&self) -> bool {
        let (dest, obj, method) = utils::KWIN_RECONFIGURE;
        for (tool, kind) in utils::DBUS_TOOL_PREFERENCE {
            let Some(exe) = self.io.which_path(tool) else {
                continue;
            };
            let argv = utils::dbus_call_argv(kind, &exe, dest, obj, method, &[]);
            let out = self
                .io
                .run(&argv, Some(app::KWIN_RECONFIGURE_TIMEOUT_SECS), false);
            if out.rc() == Some(0) {
                return true;
            }
        }
        false
    }

    /// Порт `_reset_kvantum_if_aero`: тема Kvantum из Aero-семейства →
    /// стиль виджетов Breeze. Легитимный Kvantum (Dracula и родня) НЕ
    /// трогается.
    pub fn reset_kvantum_if_aero(&self, out: &mut dyn FnMut(&str)) {
        let path = format!("{}/{}", self.config_home(), app::KVANTUM_CONFIG_REL);
        if !self.io.is_file(&path) {
            return;
        }
        let Some(text) = self.io.read_text(&path) else {
            return;
        };
        // Разбор INI — общим читателем ядра, своей копии парсера нет.
        let cfg = crate::gtk_sanitizer::read_kde_ini(Some(&text));
        let theme = crate::gtk_sanitizer::kde_value(
            &cfg,
            app::KVANTUM_THEME_SECTION,
            app::KVANTUM_THEME_KEY,
        );
        if theme.is_empty() || !app::is_aero_kvantum_theme(&theme) {
            return;
        }
        let Some(kw) = self.io.which_path(app::KWRITECONFIG_TOOL) else {
            return;
        };
        self.io.run(
            &app::widget_style_breeze_argv(&kw),
            Some(app::KWRITECONFIG_TIMEOUT_SECS),
            false,
        );
        out(&app::kvantum_breeze_line(&theme));
    }

    /// Порт `_reset_plymouth`: вернуть РОДНУЮ загрузочную заставку дистро.
    ///
    /// Plymouth — системный уровень (root + initramfs), сбросом темы Plasma
    /// не лечится. Идемпотентно: тема уже стоковая — наружу не идёт ничего,
    /// initramfs не пересобирается.
    pub fn reset_plymouth_step(&self, out: &mut dyn FnMut(&str)) {
        let Some(current) = self.detect_plymouth_theme() else {
            return; // Plymouth не установлен — сбрасывать нечего.
        };
        let target = self.target_plymouth_theme().unwrap_or_default();
        if target.is_empty() {
            out(app::PLYMOUTH_TARGET_UNKNOWN_LINE);
            return;
        }
        if current == target {
            out(&app::plymouth_already_stock_line(&target));
            return;
        }
        out(&app::plymouth_switch_line(&current, &target));
        let (ok, message) = self.reset_plymouth_to_default(Some(&target));
        if ok {
            out(&app::step_ok_line(&message));
            return;
        }
        out(&app::plymouth_failed_line(&message));
        if let Some(cmd) = self.plymouth_reset_command(Some(&target)) {
            out(&app::manual_command_line(&cmd));
        }
    }

    /// Порт `_reset_sddm`: вернуть экран входа к стоку ДИСТРИБУТИВА.
    ///
    /// ⭐ Удаляется ровно то, что показано человеку. Оригинал считал план
    /// (`sddm_reset_plan`), печатал его, а потом звал
    /// `reset_sddm_to_default()`, который сканировал `/etc/sddm.conf.d`
    /// ЗАНОВО и удалял результат второго скана. Между сканами состояние
    /// могло измениться — и удалялось не то, что обещано. Здесь план
    /// считается один раз и исполняется как есть.
    ///
    /// Идемпотентно: удалять нечего — `pkexec` не запускается вовсе, и на
    /// чистой системе человека не спрашивают пароль впустую.
    pub fn reset_sddm_step(&self, out: &mut dyn FnMut(&str)) {
        let conf_d = plan::SDDM_CONF_D;
        let reset_plan = self.sddm_reset_plan(conf_d);
        if reset_plan.to_remove.is_empty() {
            // Дистро-дефолт берём из УЖЕ посчитанного плана, а не вторым
            // сканом тех же каталогов: ответ тот же, а походов в ФС вдвое
            // меньше — и разойтись двум ответам негде.
            out(&app::sddm_already_stock_line(
                reset_plan.default_theme.as_deref(),
            ));
            return;
        }
        out(&app::sddm_removing_line(
            &reset_plan.to_remove,
            reset_plan.default_theme.as_deref(),
        ));
        let (ok, message, _removed) = self.apply_sddm_reset(&reset_plan, conf_d);
        if ok {
            out(&app::step_ok_line(&message));
        } else {
            out(&app::sddm_failed_line(&message));
        }
    }

    /// Порт `_detect_plasma_fork_residue`: остались ли СЛЕДЫ форк-темы в
    /// конфиге, когда сами пакеты уже сняты.
    ///
    /// Смотрим раскладку панели и конфиги экрана входа. Что считать токеном
    /// форка — решает ядро ([`app::has_fork_token`]), здесь только чтение.
    pub fn detect_plasma_fork_residue(&self) -> bool {
        let (_, sddm_dir, sddm_conf) = app::fork_residue_files(&self.io.env("HOME"));
        let mut candidates = vec![self.appletsrc_path()];
        // Маска `/etc/sddm.conf.d/*.conf` раскрывается здесь; порядок
        // задаётся нами, а не произвольным обходом ФС.
        let mut confs: Vec<String> = self
            .io
            .list_dir(sddm_dir)
            .unwrap_or_default()
            .into_iter()
            .filter(|n| n.ends_with(".conf"))
            .map(|n| format!("{sddm_dir}/{n}"))
            .collect();
        confs.sort();
        candidates.extend(confs);
        candidates.push(sddm_conf.to_string());

        for path in candidates {
            if !self.io.is_file(&path) {
                continue;
            }
            let Some(text) = self.io.read_text(&path) else {
                continue;
            };
            if app::has_fork_token(&text) {
                return true;
            }
        }
        false
    }

    /// Порт `_remove_fork_applets`: хирургически убрать битые апплеты
    /// форк-темы из раскладки, НЕ снося панель. → сколько убрано.
    ///
    /// Перед перезаписью рядом кладётся резервная копия. Разбор и решение
    /// «что чужое» — ядро ([`app::strip_fork_applets`]).
    pub fn remove_fork_applets(&self, out: &mut dyn FnMut(&str)) -> usize {
        let appletsrc = self.appletsrc_path();
        if !self.io.is_file(&appletsrc) {
            return 0;
        }
        let Some(text) = self.io.read_text(&appletsrc) else {
            return 0;
        };
        let (new_text, removed) = app::strip_fork_applets(&text);
        if removed.is_empty() {
            return 0;
        }
        let backup = app::appletsrc_backup_path(&appletsrc);
        if let Err(e) = self.io.copy_file(&appletsrc, &backup) {
            out(&app::applets_rewrite_failed_line(&e));
            return 0;
        }
        if let Err(e) = self.io.write_text(&appletsrc, &new_text) {
            out(&app::applets_rewrite_failed_line(&e));
            return 0;
        }
        out(&app::applets_removed_line(&removed, &backup));
        removed.len()
    }

    /// Порт `_appletsrc_has_fork_residue`: остались ли токены форка в
    /// раскладке ПОСЛЕ хирургии (значит подменён сам контейнмент).
    pub fn appletsrc_has_fork_residue(&self) -> bool {
        let appletsrc = self.appletsrc_path();
        if !self.io.is_file(&appletsrc) {
            return false;
        }
        self.io
            .read_text(&appletsrc)
            .map(|t| app::has_fork_token(&t))
            .unwrap_or(false)
    }

    /// Порт `_reset_panel_layout`: вернуть стоковую раскладку панели
    /// целиком. ⚠️ Стирает ВСЕ пользовательские панели и виджеты — потому и
    /// зовётся только по следам форк-темы, когда хирургия не помогла.
    pub fn reset_panel_layout(&self, target: &str, out: &mut dyn FnMut(&str)) {
        let Some(tool) = self.io.which_path(app::LOOKANDFEELTOOL_BIN) else {
            out(app::LOOKANDFEELTOOL_MISSING_LINE);
            return;
        };
        self.io.run(
            &app::panel_reset_layout_argv(&tool, target),
            Some(app::PANEL_RESET_TIMEOUT_SECS),
            false,
        );
        out(app::PANEL_LAYOUT_RESET_LINE);
    }

    /// Порт `_recover_kde_wallpaper`: вернуть обои ВСЕХ рабочих столов к
    /// стандартным и перезапустить plasmashell.
    ///
    /// Останавливает любые живые и видео-обои (плагин возвращается на
    /// `org.kde.image`) — про это человека предупреждает диалог окна ещё до
    /// запуска, иначе пропавшие обои читаются как поломка Rexon.
    pub fn recover_kde_wallpaper(&self, out: &mut dyn FnMut(&str)) {
        let appletsrc = self.appletsrc_path();
        if !self.io.is_file(&appletsrc) {
            return;
        }
        // Пакеты обоев перебираем по алфавиту: обход каталога произволен, а
        // выбор «первого попавшегося» обязан быть воспроизводимым.
        let mut packages = self.io.list_dir(app::WALLPAPERS_ROOT).unwrap_or_default();
        packages.sort();
        let default_wp = app::default_wallpaper_dir(
            &|p| self.io.is_dir(p),
            &packages,
            &|p| self.io.exists(p),
        );

        let Some(text) = self.io.read_text(&appletsrc) else {
            return;
        };
        let mut desktops = app::desktop_containment_ids(&text);
        if desktops.is_empty() {
            desktops.push(app::FALLBACK_CONTAINMENT.to_string());
        }

        let Some(kw) = self.io.which_path(app::KWRITECONFIG_TOOL) else {
            return;
        };
        for cid in &desktops {
            self.io.run(
                &app::wallpaper_plugin_reset_argv(&kw, cid),
                Some(app::KWRITECONFIG_TIMEOUT_SECS),
                false,
            );
            if !default_wp.is_empty() {
                self.io.run(
                    &app::wallpaper_image_argv(&kw, cid, &default_wp),
                    Some(app::KWRITECONFIG_TIMEOUT_SECS),
                    false,
                );
            }
        }
        out(&app::wallpaper_reset_line(desktops.len(), &default_wp));

        // Перезапуск plasmashell — чтобы записанное подхватилось сейчас, а
        // не после следующего входа в систему.
        for (cmd, timeout) in app::plasmashell_restart_argv()
            .iter()
            .zip(app::PLASMASHELL_RESTART_TIMEOUTS)
        {
            self.io.run(cmd, Some(*timeout), false);
        }
    }
}

// ============================================================================
// Живой исполнитель
// ============================================================================

fn path_env() -> Option<String> {
    std::env::var("PATH").ok()
}

/// НЕстрогий `os.path.realpath`: разбирает путь по звеньям и следует за
/// симлинками, но НЕ требует, чтобы конечная цель существовала.
///
/// ⚠️ Зачем свой, а не `std::fs::canonicalize`. Тот строгий: на висячем
/// симлинке — а это штатное состояние `/etc/systemd/system/display-manager.service`
/// после `pacman -R sddm` — он возвращает ошибку, и мёртвая цель осталась бы
/// безымянной. Ровно на этой безымянности защита от дурака и молчала.
pub fn realpath_lenient(path: &str) -> String {
    use std::ffi::OsString;

    let start = Path::new(path);
    let mut out = if start.is_absolute() {
        PathBuf::from("/")
    } else {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"))
    };
    let split = |p: &Path| -> Vec<OsString> {
        let mut v: Vec<OsString> = p
            .components()
            .filter_map(|c| match c {
                std::path::Component::Normal(s) => Some(s.to_os_string()),
                std::path::Component::ParentDir => Some(OsString::from("..")),
                _ => None,
            })
            .collect();
        v.reverse(); // снимаем с хвоста → порядок остаётся прямым
        v
    };
    let mut stack = split(start);
    // Бюджет переходов — как ELOOP у ядра: кольцо симлинков не должно
    // подвесить исполнитель.
    let mut budget = 40;
    while let Some(name) = stack.pop() {
        if name == "" {
            continue;
        }
        if name == ".." {
            out.pop();
            continue;
        }
        out.push(&name);
        if budget == 0 {
            continue;
        }
        if let Ok(target) = std::fs::read_link(&out) {
            budget -= 1;
            out.pop();
            if target.is_absolute() {
                out = PathBuf::from("/");
            }
            stack.extend(split(&target));
        }
    }
    out.to_string_lossy().into_owned()
}

/// Настоящий исполнитель: живые ФС, `PATH` и подпроцессы.
///
/// ⛔ Всё, что он умеет ИЗМЕНИТЬ, изменяется единственным способом — запуском
/// готового `argv` из плана ядра. Своих команд у него нет; файлы он пишет,
/// удаляет и заменяет ссылкой только по путям, которые назвал план ядра.
pub struct LiveSystemIo;

impl SystemIo for LiveSystemIo {
    fn run_priv(
        &self,
        op: &crate::privilege::op::PrivOp,
        files: &[String],
        log_path: Option<&str>,
    ) -> Option<RunOut> {
        crate::privilege::run::run_live(op, files, log_path, &mut |_| {})
    }

    fn exists(&self, path: &str) -> bool {
        Path::new(path).exists()
    }

    fn is_file(&self, path: &str) -> bool {
        Path::new(path).is_file()
    }

    fn is_dir(&self, path: &str) -> bool {
        Path::new(path).is_dir()
    }

    fn is_executable(&self, path: &str) -> bool {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }

    fn realpath(&self, path: &str) -> Option<String> {
        if path.is_empty() {
            return None;
        }
        Some(realpath_lenient(path))
    }

    fn read_text(&self, path: &str) -> Option<String> {
        std::fs::read(path)
            .ok()
            .map(|raw| String::from_utf8_lossy(&raw).into_owned())
    }

    fn list_dir(&self, dir: &str) -> Option<Vec<String>> {
        let entries = std::fs::read_dir(dir).ok()?;
        Some(
            entries
                .flatten()
                .filter_map(|e| e.file_name().to_str().map(str::to_string))
                .collect(),
        )
    }

    fn which(&self, name: &str) -> bool {
        // Порт `safe_check_command`: у имён с синонимами ответ даётся по ВСЕМ
        // вариантам, иначе проверка и последующий запуск расходятся.
        let path = path_env();
        if utils::tool_has_variants(name) {
            return utils::tool_argv(name, &[], path.as_deref()).is_some();
        }
        utils::lookup_tool(&[name], &[], path.as_deref()).is_some()
    }

    fn run(&self, argv: &[String], timeout_secs: Option<u64>, lang_c: bool) -> RunOut {
        if argv.is_empty() {
            return RunOut::missing();
        }
        let spec = RunSpec {
            argv: argv.to_vec(),
            output: RunOutput::Capture,
            timeout_secs,
            lang_c,
            ..RunSpec::default()
        };
        let report = utils::run(&spec, path_env().as_deref());
        RunOut {
            outcome: report.outcome,
            returncode: report.returncode,
            stdout: utils::decode_text(&report.stdout),
            stderr: utils::decode_text(&report.stderr),
        }
    }

    fn env(&self, name: &str) -> String {
        std::env::var(name).unwrap_or_default()
    }

    fn which_path(&self, name: &str) -> Option<String> {
        utils::resolve_tool(name, &[], path_env().as_deref())
    }

    /// Запись целиком — ОБЩЕЙ дверью ядра, а не `fs::write`.
    ///
    /// Этой дверью в профиль покупателя уходит, в частности, эталон защиты
    /// приложений по умолчанию (`SystemDefaultsProtector.snapshot`), который
    /// с 19.09 снимается сам при первом запуске. Обычная запись сперва
    /// обрезает файл: закрытое в эти полсекунды окно оставляло на диске
    /// огрызок. Здесь на месте цели либо прежний файл, либо новый целиком.
    fn write_text(&self, path: &str, content: &str) -> Result<(), String> {
        utils::atomic_write_str_err(path, content)
    }

    fn copy_file(&self, src: &str, dst: &str) -> Result<(), String> {
        std::fs::copy(src, dst).map(|_| ()).map_err(|e| e.to_string())
    }

    fn unlink_file(&self, path: &str) -> Result<(), String> {
        std::fs::remove_file(path).map_err(|e| e.to_string())
    }

    fn replace_with_symlink(&self, path: &str, target: &str) -> Result<(), String> {
        let tmp = format!("{path}.rexon-link-{}", std::process::id());
        let _ = std::fs::remove_file(&tmp);
        std::os::unix::fs::symlink(target, &tmp).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, path).map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            e.to_string()
        })
    }
}

// ============================================================================
// Раздел «Менеджер входа»: снимок для интерфейса
// ============================================================================

/// Приветствие LightDM в строке раздела.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LoginGreeterRow {
    /// Имя файла без `.desktop` — то, что уйдёт помощнику.
    pub id: String,
    /// Имя для человека (`Name=`, пусто — `id`).
    pub name: String,
    /// Текст окна подтверждения с этим приветствием.
    pub confirm: String,
}

/// Строка раздела: один установленный экран входа.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginManagerRow {
    pub unit: String,
    /// Имя для человека (незнакомый — имя юнита).
    pub label: String,
    /// Основной: на него указывает ссылка, им начнётся следующая загрузка.
    pub active: bool,
    /// Работает сейчас (`systemctl is-active`).
    pub running: bool,
    /// `ready` | `needs_greeter` | `blocked` | `unsupported`.
    pub state: &'static str,
    /// Почему кнопки нет (серым под именем); пусто — причины нет.
    pub reason: String,
    /// Приветствия на выбор (только LightDM).
    pub greeters: Vec<LoginGreeterRow>,
    /// Предвыбранное приветствие (`id`); пусто — выбирать не из чего.
    pub greeter: String,
    /// Кнопка «Сделать основным» доступна.
    pub can_switch: bool,
    /// Заголовок окна подтверждения.
    pub confirm_title: String,
    /// Текст окна подтверждения без выбора приветствия.
    pub confirm: String,
    /// Предупреждения, не мешающие переключению.
    pub warnings: Vec<String>,
}

/// Весь раздел «Менеджер входа».
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginScreenView {
    pub managers: Vec<LoginManagerRow>,
    /// Механизм выбора: `systemd` | `debian` | `suse` | `nixos` | `other`.
    pub backend: &'static str,
    /// На этой системе Rexon переключает экран входа.
    pub switchable: bool,
    /// Почему кнопок нет на всей системе; пусто — есть.
    pub note: String,
    /// Основной уже другой, а работает ещё прежний: вступит после
    /// перезагрузки. Пусто — расхождения нет.
    pub pending: String,
    /// «Как вернуть прежний экран входа» — текст файла, который помощник
    /// оставил для консоли ([`plan::PREVIOUS_DM_FILE`]); пусто — файла нет.
    pub rollback: String,
}

/// Наблюдения для [`login_screen_view`] — собирает обвязка, только чтение.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginScreenFacts {
    pub family: PkgFamily,
    /// Помощник `rexon-pkg-helper` развёрнут вместе с политикой polkit.
    pub helper: bool,
    /// Неизменяемая система ([`DistroDetector::is_immutable`]).
    pub immutable: bool,
    /// Экраны входа, которые работают сейчас.
    pub running: Vec<String>,
    /// Текст [`plan::PREVIOUS_DM_FILE`], если файл есть.
    pub previous: Option<String>,
}

fn backend_key(backend: &plan::DmSwitchBackend) -> &'static str {
    match backend {
        plan::DmSwitchBackend::Systemd => "systemd",
        plan::DmSwitchBackend::SystemdDebian => "debian",
        plan::DmSwitchBackend::SystemdSuseAlternatives => "suse",
        plan::DmSwitchBackend::NixosDeclarative => "nixos",
        plan::DmSwitchBackend::Unsupported(_) => "other",
    }
}

fn row_label(info: &plan::LoginManagerInfo) -> String {
    if info.label.trim().is_empty() { info.unit.clone() } else { info.label.clone() }
}

/// Неизменяемая система: экран входа Rexon пока не трогает.
pub fn login_manager_immutable_note() -> String {
    crate::i18n::tr(
        "On this system the login screen is part of the read-only base, so Rexon doesn't change it yet.",
    )
}

/// Причина отказа: такого экрана входа на машине нет.
pub fn login_manager_missing_reason() -> String {
    crate::i18n::tr("this login screen isn't installed")
}

/// Причина отказа и подпись строки: этот экран входа Rexon основным не
/// делает (нет проверок готовности — Ly, LXDM, slim…).
pub fn login_manager_cant_switch_reason() -> String {
    crate::i18n::tr("Rexon can't make this login screen the default yet.")
}

/// Отказ: выбранный уже основной.
pub fn login_manager_already_default(label: &str, unit: &str) -> String {
    let name = if label.trim().is_empty() { unit } else { label };
    // TRANSLATORS: %1 is a login screen name (SDDM, LightDM, …).
    crate::i18n::trf("%1 is already the login screen.", &[&name])
}

/// Отказ: у LightDM не выбрано приветствие из установленных.
pub fn login_manager_choose_greeter() -> String {
    crate::i18n::tr("First choose which greeter LightDM should show.")
}

/// Основной и работающий расходятся — до перезагрузки.
pub fn login_manager_pending_note(new_label: &str, running_label: &str) -> String {
    crate::i18n::trf(
        // TRANSLATORS: %1 is the login screen chosen for the next boot, %2 the one running now.
        "%1 becomes the login screen after a restart. %2 is still running now.",
        &[&new_label, &running_label],
    )
}

/// Решение для раздела «Менеджер входа» — чистая функция над списком
/// [`SystemExec::login_managers`] и наблюдениями.
///
/// Кнопка есть только там, где её нажатие действительно сработает
/// (правило #13): systemd-механизм, который Rexon умеет, не неизменяемая
/// система, помощник на месте, строка не основная, программа есть и
/// готовность `Ready` либо «выбрать приветствие». Иначе — причина словами,
/// серым, ДО пароля, а не после него.
pub fn login_screen_view(
    list: &[plan::LoginManagerInfo],
    backend: &plan::DmSwitchBackend,
    facts: &LoginScreenFacts,
) -> LoginScreenView {
    let note = plan::login_manager_unsupported_note(backend, None)
        .or_else(|| facts.immutable.then(login_manager_immutable_note))
        .or_else(|| (!facts.helper).then(plan::login_manager_needs_helper))
        .unwrap_or_default();
    let switchable = note.is_empty();
    let old = list.iter().find(|m| m.active).map(|m| m.unit.as_str());
    let managers = list
        .iter()
        .map(|info| {
            let (state, mut reason, greeters) = match &info.readiness {
                plan::DmReadiness::Ready => ("ready", String::new(), Vec::new()),
                plan::DmReadiness::NeedsGreeterChoice(list) => ("needs_greeter", String::new(), list.clone()),
                plan::DmReadiness::Blocked(r) => ("blocked", r.clone(), Vec::new()),
                plan::DmReadiness::Unsupported => ("unsupported", String::new(), Vec::new()),
            };
            if state == "unsupported" && !info.active {
                reason = match backend {
                    // Без systemd — команда для этой init вместо кнопки.
                    plan::DmSwitchBackend::Unsupported(init) => plan::login_manager_unsupported_note(
                        backend,
                        plan::dm_manual_switch_command(init, facts.family, &info.unit).as_deref(),
                    )
                    .unwrap_or_default(),
                    _ if switchable => login_manager_cant_switch_reason(),
                    // Причина одна на всю систему — она в `note`.
                    _ => String::new(),
                };
            }
            let can_switch = switchable
                && !info.active
                && info.present
                && matches!(state, "ready" | "needs_greeter");
            let suggested = info.suggested_greeter().map(|g| g.id.clone()).unwrap_or_default();
            LoginManagerRow {
                unit: info.unit.clone(),
                label: row_label(info),
                active: info.active,
                running: facts.running.iter().any(|u| *u == info.unit),
                state,
                reason,
                greeters: greeters
                    .iter()
                    .map(|g| LoginGreeterRow {
                        id: g.id.clone(),
                        name: if g.name.trim().is_empty() { g.id.clone() } else { g.name.clone() },
                        confirm: plan::login_manager_switch_confirm(old, &info.unit, Some(g), backend),
                    })
                    .collect(),
                greeter: suggested,
                can_switch,
                confirm_title: plan::login_manager_switch_title(&info.unit),
                confirm: plan::login_manager_switch_confirm(old, &info.unit, None, backend),
                warnings: info.warnings.clone(),
            }
        })
        .collect::<Vec<_>>();
    let pending = match (
        managers.iter().find(|m| m.active),
        managers.iter().find(|m| m.running && !m.active),
    ) {
        (Some(main), Some(now)) if !main.running => login_manager_pending_note(&main.label, &now.label),
        _ => String::new(),
    };
    LoginScreenView {
        managers,
        backend: backend_key(backend),
        switchable,
        note,
        pending,
        rollback: facts.previous.as_deref().map(str::trim).unwrap_or_default().to_string(),
    }
}

/// Исполнитель поверх живой машины — то, чем пользуется продукт.
pub fn live() -> SystemExec<'static, LiveSystemIo> {
    SystemExec::new(&LiveSystemIo)
}
