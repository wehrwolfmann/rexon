//! Общий план установки, применения и удаления — и один его исполнитель.
//!
//! ЗАЧЕМ. В Rexon было три вида шагов и три исполнителя:
//!   * [`ExtStep`] (`ext_manager`) — установка и удаление `.rxn`: файлы,
//!     раскладка под `/tmp`, системная часть через pkexec, заметки человеку;
//!     исполнитель — `ext_exec::exec_plan_noted`;
//!   * `appearance_installer::InstallStep` — установка товара магазина
//!     «Оформления»: те же файловые шаги, только беднее (подмножество
//!     [`ExtStep`]); исполнитель — `exec_install_steps`. С шага 2 своего
//!     типа у магазина нет: его планировщики строят [`ExtStep`];
//!   * [`ApplyStep`] (`themes`) — «Применить» темы раздела: команды с флагом
//!     `ok`, записи ini, конфиг композитора, сверка D-Bus Deepin; исполнитель —
//!     `exec_apply_plan_report`.
//!
//! Формат `.rxn` v5 ставит пакет ОДНИМ планом: «положить файлы» и «включить».
//! Здесь первый шаг слияния: один тип шага
//! [`Step`] (файловый слой — [`ExtStep`], он покрывает шаги магазина целиком;
//! слой включения — [`ApplyStep`]) и один исполнитель [`exec`]. Старые
//! исполнители стали тонкими обёртками над ним, их планы не менялись.
//!
//! Различие путей, которое пока сохранено, — есть ли у плана системная
//! часть ([`Ctx::privileged`]). Копирование у всех путей одно (шаг 2): см.
//! [`copy_tree`] и [`copy_file`]; запуск команд тоже один — [`Ctx::run`],
//! [`Ctx::have`], [`Ctx::spawn`], живая машина — [`Ctx::live`]. Текст ошибки каждый путь собирает сам из [`Failure`] — так,
//! как собирал раньше.
//!
//! Откат при сбое посреди установки (шаг 4) тоже один: [`Undo`] — его
//! пишут копирование и прочие файловые шаги, исполняет [`exec`] (или
//! установка, у которой после плана есть свои шаги, — [`exec_undoable`]).

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::ext_manager::ExtStep;
use crate::ext_exec::PrivOutcome;
use crate::themes::{ApplyStep, CursorReport};

/// Шаг общего плана.
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    /// Положить, убрать, запустить: файловый и системный слой.
    File(ExtStep),
    /// Включить: шаг применения с флагом `ok` (см. [`ApplyStep`]).
    Apply(ApplyStep),
}

impl From<ExtStep> for Step {
    fn from(step: ExtStep) -> Self {
        Step::File(step)
    }
}

impl From<ApplyStep> for Step {
    fn from(step: ApplyStep) -> Self {
        Step::Apply(step)
    }
}

/// Чем кончился неудавшийся шаг. Текст для человека из него собирает
/// вызывающий — у каждого пути свой (см. обёртки).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// Текст уже готов (системная часть, SteamOS, проверка темы до пароля).
    Text(String),
    MakeDirs { path: String, err: String },
    Copy { src: String, dest: String, err: String },
    Remove { path: String, err: String },
    Symlink { link: String, err: String },
    Write { path: String, err: String },
    Unpack { archive: String, err: String },
    Prepare { source: String, err: String },
}

/// Внешнее для прогона плана.
pub struct Ctx<'a> {
    /// План установки (`true`) или удаления — от этого зависит системная
    /// часть на SteamOS.
    pub install: bool,
    /// Мы на SteamOS: системный раздел закрыт.
    pub steamos: bool,
    /// Исполнитель системных шагов (pkexec). `None` — системной части у
    /// плана не бывает (магазин): `Run` с `pkexec` идёт обычной командой.
    pub privileged: Option<&'a dyn Fn(&ExtStep) -> PrivOutcome>,
    /// ЕДИНСТВЕННЫЙ запуск команд плана с ожиданием: и файловых шагов
    /// (`Run`, `RunNoted`), и шагов включения ([`ApplyStep`] — команды,
    /// пробы, сверка Deepin, проверка конфига композитора). argv, таймаут
    /// шага (`None` — по классу команды, [`crate::utils::run_duration`]) →
    /// итог с выводом. Живой — [`run_live`].
    pub run: &'a dyn Fn(&[String], Option<u64>) -> crate::utils::RunReport,
    /// Есть ли программа — по тому же правилу поиска, что у [`Self::run`]
    /// ([`have_live`]).
    pub have: &'a dyn Fn(&str) -> bool,
    /// Запуск без ожидания ([`ExtStep::Spawn`]: `conky`, перезапуск
    /// оболочки). Живой — [`spawn_live`]; в проверках — ничего не запускать.
    pub spawn: &'a dyn Fn(&[String]),
    /// Идёт ли сеанс Treeland — причина остановки на
    /// [`ApplyStep::DdeTreelandGate`] ([`crate::dde::refusal`]). Имя поля
    /// прежнее: живые вызовы передают сюда
    /// [`crate::dde::treeland_session_live`].
    pub wayland: bool,
    /// План записал переменные в `environment.d` — отзеркалить их в
    /// сеансы, которые этот каталог не читают; записи — под откатом
    /// прогона. Живой — [`sync_env_live`]; в проверках — ничего: живая
    /// синхронизация пишет в конфиги композиторов настоящего профиля.
    pub sync_env: &'a dyn Fn(&mut Undo),
}

impl<'a> Ctx<'a> {
    /// Живая машина: команды — [`run_live`], поиск программ — [`have_live`],
    /// запуск без ожидания — [`spawn_live`].
    pub fn live(
        install: bool,
        steamos: bool,
        privileged: Option<&'a dyn Fn(&ExtStep) -> PrivOutcome>,
        treeland: bool,
    ) -> Self {
        Ctx { install, steamos, privileged, run: &run_live, have: &have_live, spawn: &spawn_live, wayland: treeland, sync_env: &sync_env_live }
    }
}

/// Отзеркалить переменные `environment.d` в сеансы живой машины
/// ([`crate::compositor_autostart::sync_session_env_live`]) под откатом
/// `undo`: файлы сеансов (`labwc/environment`, `.xprofile`, конфиг
/// Hyprland) — копией до записи, заведённое — отметкой. Прежде эта запись
/// шла мимо отката, и «вернуть как было» после курсора оставляло блок
/// переменных в `~/.config/xfce4/labwc/environment` (найдено 25.10.2026).
pub fn sync_env_live(undo: &mut Undo) {
    let paths: Vec<std::path::PathBuf> =
        crate::compositor_autostart::session_env_paths_live().into_iter().map(Into::into).collect();
    let missing = undo.guard_external(&paths);
    let _ = crate::compositor_autostart::sync_session_env_live();
    undo.after_external(missing);
}

/// Ничего не синхронизировать (проверки, подменные контексты).
pub fn no_sync_env(_: &mut Undo) {}

/// `$PATH` процесса — одно правило поиска программ у всех шагов плана.
fn path_env() -> Option<String> {
    std::env::var("PATH").ok()
}

/// Запустить команду плана и дождаться — через единственную точку запуска
/// продукта ([`crate::utils::run`]): синонимы бинаря (`kwriteconfig6` →
/// `kwriteconfig5`, `qdbus` → `qdbus6`, каталоги вне PATH), таймаут по классу
/// команды, если шаг не задал свой, журнал запуска; вывод забирается.
///
/// Магазин оформления системного исполнителя не имеет ([`Ctx::privileged`]
/// = `None`): его шаг помощника тем и раскладок (`pkexec <помощник> …`)
/// сперва идёт сеансу прав окна
/// ([`crate::ext_exec::run_system_argv_session`]), сеанса нет — прежней
/// командой. Отказ помощника ею не повторяется (З5).
pub fn run_live(argv: &[String], timeout_secs: Option<u64>) -> crate::utils::RunReport {
    if let Some(report) = crate::ext_exec::run_system_argv_session(argv, timeout_secs) {
        return report;
    }
    crate::utils::run(
        &crate::utils::RunSpec {
            argv: argv.to_vec(),
            output: crate::utils::RunOutput::Capture,
            timeout_secs,
            ..Default::default()
        },
        path_env().as_deref(),
    )
}

/// Есть ли программа `binary` — с той же подменой синонимов, что у
/// [`run_live`]: проба и запуск не расходятся во мнении.
pub fn have_live(binary: &str) -> bool {
    let path = path_env();
    let argv = crate::utils::resolved_argv(&[binary.to_string()], path.as_deref());
    argv.first().is_some_and(|b| crate::utils::which(b, path.as_deref()).is_some())
}

/// Запустить программу и не ждать (stdin/stdout/stderr — в никуда).
pub fn spawn_live(argv: &[String]) {
    if let Some(program) = argv.first() {
        let _ = Command::new(program)
            .args(&argv[1..])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
    }
}

/// Итог прогона.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    /// Что из плана сделано не было и почему ([`ExtStep::Notice`],
    /// неудавшийся [`ExtStep::RunNoted`]).
    pub notes: Vec<String>,
    /// Итог слоя включения (флаг `ok` и подробности курсора).
    pub apply: CursorReport,
}

/// Исполнить общий план.
///
/// Файловый шаг с ошибкой останавливает план (`Err`). Шаги включения не
/// падают: они ведут флаг `ok` ([`Report::apply`]); стоп Treeland прекращает
/// план целиком. `pre_ran` — пробы, исполненные ещё при сборке плана: их
/// одноимённые шаги `Run` пропускаются по очереди.
///
/// ОТКАТ (шаг 4 слияния установщиков). План установки (`ctx.install`) при
/// ошибке снимает всё, что положил ЭТОТ прогон, и возвращает отложенное
/// ([`Undo`]); при успехе отложенные копии стираются. Одно правило у `.rxn`
/// и магазина. Планы удаления (`ctx.install == false`) не откатываются: им
/// возвращать нечего, кроме того, что человек и просил снять.
pub fn exec(steps: &[Step], pre_ran: &mut Vec<Vec<String>>, ctx: &Ctx) -> Result<Report, Failure> {
    let mut undo = if ctx.install { Undo::new() } else { Undo::off() };
    let outcome = exec_undoable(steps, pre_ran, ctx, &mut undo);
    undo.finish(outcome.is_ok());
    outcome
}

/// То же, что [`exec`], но откат копится в `undo` вызывающего и НЕ
/// исполняется: установка, у которой после плана есть свои шаги (копия
/// пакета `.rxn`, запись состояния), решает сама — [`Undo::finish`].
/// Исключение — прежние значения стола ([`Undo::keeping_values`]): при
/// сбое они возвращаются здесь же.
pub fn exec_undoable(
    steps: &[Step],
    pre_ran: &mut Vec<Vec<String>>,
    ctx: &Ctx,
    undo: &mut Undo,
) -> Result<Report, Failure> {
    let mut notes = Vec::new();
    let mut apply = crate::appearance_installer::ApplyRun::new(ctx.wayland, ctx.run, ctx.have, ctx.sync_env);

    // Отказ установки целиком (всё основное пропущено — на месте чужое):
    // ответ причиной до первого шага, ничего не тронуто.
    if let Some(text) = steps.iter().find_map(|s| match s {
        Step::File(ExtStep::Refuse { text }) => Some(text),
        _ => None,
    }) {
        return Err(Failure::Text(text.clone()));
    }

    if ctx.privileged.is_some() {
        if ctx.steamos && ctx.install {
            let first = steps.iter().find_map(|s| match s {
                Step::File(f) if is_privileged(f) => Some(f),
                _ => None,
            });
            if let Some(step) = first {
                return Err(Failure::Text(steamos_block_of(step).reason()));
            }
        }
    }

    // Последняя раскладка (`Stage`) перед системным шагом — её содержимое он
    // унесёт в систему.
    let mut last_stage: Option<&str> = None;
    let run = run_steps(steps, pre_ran, ctx, &mut notes, &mut apply, undo, &mut last_stage);
    if let Err(f) = run {
        // Файлы, заведённые командами после `Guard`, — тоже под откат.
        apply.settle(undo);
        // Значения стола, которые успел сменить прогон, — назад сразу:
        // команде нужен запуск, а у отката файлов его нет.
        let _ = restore_values(&undo.take_values(), ctx.run);
        return Err(f);
    }
    Ok(Report { notes, apply: apply.finish(undo) })
}

/// Шаги плана по порядку — тело [`exec_undoable`].
fn run_steps<'s>(
    steps: &'s [Step],
    pre_ran: &mut Vec<Vec<String>>,
    ctx: &Ctx,
    notes: &mut Vec<String>,
    apply: &mut crate::appearance_installer::ApplyRun,
    undo: &mut Undo,
    last_stage: &mut Option<&'s str>,
) -> Result<(), Failure> {
    for step in steps {
        let file = match step {
            Step::Apply(a) => {
                if !apply.step(a, undo)? {
                    break;
                }
                continue;
            }
            Step::File(f) => f,
        };
        if let Some(privileged) = ctx.privileged {
            if is_privileged(file) {
                if ctx.steamos {
                    continue; // удаление на SteamOS: системной части не касаемся
                }
                if ctx.install {
                    if let Some(problem) = system_theme_problem(file, *last_stage) {
                        return Err(Failure::Text(problem.message()));
                    }
                }
                if let Some(text) = crate::ext_exec::privileged_failure_text(&privileged(file)) {
                    return Err(Failure::Text(text));
                }
                continue;
            }
        }
        exec_file_step(file, pre_ran, ctx, notes, undo)?;
        if let ExtStep::Stage { staged, .. } = file {
            *last_stage = Some(staged.as_str());
        }
    }
    Ok(())
}

fn io(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// Текст неудавшегося шага для человека (строчная буква: идёт после
/// вступления вызывающего).
pub fn failure_text(f: &Failure) -> String {
    use crate::i18n::trf;
    match f {
        Failure::Text(text) => text.clone(),
        Failure::MakeDirs { path, err } => trf("couldn't create the folder %1: %2", &[path, err]),
        Failure::Copy { src, dest, err } => trf("couldn't copy %1 → %2: %3", &[src, dest, err]),
        Failure::Remove { path, err } => trf("couldn't remove %1: %2", &[path, err]),
        Failure::Symlink { link, err } => trf("couldn't create the link %1: %2", &[link, err]),
        Failure::Write { path, err } => trf("couldn't write the file %1: %2", &[path, err]),
        Failure::Unpack { archive, err } => trf("couldn't unpack %1: %2", &[archive, err]),
        Failure::Prepare { source, err } => trf("couldn't prepare %1: %2", &[source, err]),
    }
}

fn exec_file_step(
    step: &ExtStep,
    pre_ran: &mut Vec<Vec<String>>,
    ctx: &Ctx,
    notes: &mut Vec<String>,
    undo: &mut Undo,
) -> Result<(), Failure> {
    match step {
        ExtStep::MakeDirs { path } => make_dirs(Path::new(path), undo, false)
            .map_err(|e| Failure::MakeDirs { path: path.clone(), err: io(e) })?,
        ExtStep::CopyTree { src, dest } => copy_tree(Path::new(src), Path::new(dest), undo)
            .map_err(|e| Failure::Copy { src: src.clone(), dest: dest.clone(), err: io(e) })?,
        ExtStep::CopyFile { src, dest } => copy_file_undo(Path::new(src), Path::new(dest), undo)
            .map_err(|e| Failure::Copy { src: src.clone(), dest: dest.clone(), err: io(e) })?,
        ExtStep::RemoveTree { path, safe, ignore_errors } => {
            if *safe {
                // Те же границы, что у `safe_rmtree`: ссылка — сама, каталог —
                // только под домом или временным каталогом; иначе — ничего.
                let p = Path::new(path);
                if !(crate::ext_utils::safe_rmtree_removes(p) && undo.set_aside(p)) {
                    crate::ext_utils::safe_rmtree(p);
                }
            } else {
                // `remove_dir_all` снимает каталог и ссылку; файл — ошибка.
                let p = Path::new(path);
                let tree_or_link = std::fs::symlink_metadata(p).is_ok_and(|m| m.is_dir() || m.file_type().is_symlink());
                if !(tree_or_link && undo.set_aside(p)) {
                    let r = std::fs::remove_dir_all(path);
                    if !*ignore_errors {
                        r.map_err(|e| Failure::Remove { path: path.clone(), err: io(e) })?;
                    }
                }
            }
        }
        ExtStep::RemoveFile { path } => {
            let p = Path::new(path);
            let non_dir = std::fs::symlink_metadata(p).is_ok_and(|m| !m.is_dir());
            if !(non_dir && undo.set_aside(p)) {
                std::fs::remove_file(path).map_err(|e| Failure::Remove { path: path.clone(), err: io(e) })?
            }
        }
        ExtStep::Symlink { target, link } => {
            std::os::unix::fs::symlink(target, link).map_err(|e| Failure::Symlink { link: link.clone(), err: io(e) })?;
            undo.created(Path::new(link));
        }
        ExtStep::WriteFile { path, content } => {
            let p = Path::new(path);
            if let Some(parent) = p.parent() {
                let _ = make_dirs(parent, undo, false);
            }
            let fresh = std::fs::symlink_metadata(p).is_err();
            if !fresh {
                undo.keep_copy(p);
            }
            std::fs::write(path, content).map_err(|e| Failure::Write { path: path.clone(), err: io(e) })?;
            if fresh {
                undo.created(p);
            }
        }
        ExtStep::ExtractZip { archive, dest } => {
            let d = Path::new(dest);
            let unpack = |into: &Path| {
                crate::ext_utils::safe_zip_extract(Path::new(archive), into)
                    .map_err(|e| Failure::Unpack { archive: archive.clone(), err: io(e) })
            };
            if undo.enabled && d.is_dir() {
                // Поверх существующего каталога: распаковать рядом и
                // перенести общим копированием — так откат знает каждый
                // положенный и заменённый файл.
                let side = undo.side_path(d, "unpack");
                let r = unpack(&side).and_then(|()| {
                    copy_tree(&side, d, undo)
                        .map_err(|e| Failure::Unpack { archive: archive.clone(), err: io(e) })
                });
                let _ = std::fs::remove_dir_all(&side);
                r?
            } else {
                make_dirs(d, undo, true).map_err(|e| Failure::MakeDirs { path: dest.clone(), err: io(e) })?;
                unpack(d)?
            }
        }
        ExtStep::Stage { staged, sources } => {
            // `_staged_under_tmp`: содержимое каталогов — в корень staged,
            // файлы — под своим именем.
            make_dirs(Path::new(staged), undo, false)
                .map_err(|e| Failure::MakeDirs { path: staged.clone(), err: io(e) })?;
            for p in sources {
                let prepare = |e: std::io::Error| Failure::Prepare { source: p.clone(), err: io(e) };
                let src = Path::new(p);
                if src.is_dir() {
                    for name in sorted_names(src) {
                        let from = src.join(&name);
                        let to = Path::new(staged).join(&name);
                        let r = if from.is_dir() { copy_tree(&from, &to, undo) } else { copy_file_undo(&from, &to, undo) };
                        r.map_err(prepare)?;
                    }
                } else {
                    let to = match src.file_name() {
                        Some(name) => Path::new(staged).join(name),
                        None => Path::new(staged).to_path_buf(),
                    };
                    copy_file_undo(src, &to, undo).map_err(prepare)?;
                }
            }
        }
        // Без системного исполнителя скрипту-обёртке взяться неоткуда; с
        // ним он исполнен выше вместе с прочими системными шагами.
        ExtStep::PkexecScript { .. } => {}
        ExtStep::Run { argv, timeout_secs } => {
            if pre_ran.first().map(|a| a == argv).unwrap_or(false) {
                pre_ran.remove(0); // уже исполнено пробой при сборке плана
                return Ok(());
            }
            let _ = undo.run_kept(ctx.run, argv, *timeout_secs);
        }
        ExtStep::RunNoted { argv, timeout_secs, failed } => {
            if undo.run_kept(ctx.run, argv, *timeout_secs).returncode != Some(0) {
                notes.push(failed.clone());
            }
        }
        ExtStep::Notice { text } => notes.push(text.clone()),
        ExtStep::Refuse { text } => return Err(Failure::Text(text.clone())),
        ExtStep::Spawn { argv } => {
            if argv.first().map(|a| a == "conky").unwrap_or(false) {
                // оригинал: pkill → time.sleep(0.5) → Popen
                std::thread::sleep(Duration::from_millis(500));
            }
            (ctx.spawn)(argv);
        }
    }
    Ok(())
}

/// Имена в каталоге по алфавиту (порядок от файловой системы не зависит);
/// имена не в UTF-8 пропускаются — как у снимка `ext_exec`.
fn sorted_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map(|rd| rd.flatten().filter_map(|e| e.file_name().to_str().map(str::to_string)).collect())
        .unwrap_or_default();
    names.sort();
    names
}

// ============================================================================
// Системная часть плана
// ============================================================================

/// Привилегированный ли шаг: пишет в систему через pkexec.
pub(crate) fn is_privileged(step: &ExtStep) -> bool {
    match step {
        ExtStep::PkexecScript { .. } => true,
        ExtStep::Run { argv, .. } => argv.first().map(String::as_str) == Some("pkexec"),
        _ => false,
    }
}

/// Аргументы помощника после `pkexec <помощник>` без необязательного
/// `--lang=…`: категория — первым словом.
///
/// ⭐ Починка 09.10.2026: прежде категория бралась третьим словом argv, а при
/// русском интерфейсе там стоит `--lang=ru` — проверка темы до окна пароля
/// молча не срабатывала, а на SteamOS причина отказа выходила общей.
pub(crate) fn helper_args(argv: &[String]) -> &[String] {
    let rest = argv.get(2..).unwrap_or(&[]);
    match rest.first() {
        Some(a) if a.starts_with("--lang=") => &rest[1..],
        _ => rest,
    }
}

/// Что именно пишет привилегированный шаг — для причины на SteamOS. Метка
/// берётся из самого шага: у помощников это категория третьим словом argv
/// (`pkexec <помощник> sddm …`), у скриптов-обёрток — их префикс
/// (`rexon_sddm`, `rexon_plymouth`, `rexon_gdm-bg`…).
pub(crate) fn steamos_block_of(step: &ExtStep) -> crate::distro_compat::SteamOsBlock {
    use crate::distro_compat::SteamOsBlock;
    let tag = match step {
        ExtStep::PkexecScript { prefix, .. } => prefix.as_str(),
        ExtStep::Run { argv, .. } => helper_args(argv).first().map(String::as_str).unwrap_or(""),
        _ => "",
    };
    if tag.contains("plymouth") {
        SteamOsBlock::BootSplash
    } else if ["sddm", "gdm", "lightdm", "greeter", "login"].iter().any(|k| tag.contains(k)) {
        SteamOsBlock::LoginScreen
    } else {
        SteamOsBlock::SystemPart
    }
}

/// Тема экрана входа или загрузки, которую системный шаг `step` сейчас унесёт
/// в `/usr/share`: проверка ДО окна пароля ([`crate::installer::system_theme_problem`]).
///
/// Содержимое — последний шаг раскладки (`Stage`) перед системным
/// (`last_stage`): его копирует и помощник, и запасной скрипт (тот берёт
/// исходный каталог, а раскладка — его копия, сделанная только что). На
/// запасном пути (сырой `pkexec bash`, помощника нет) эта проверка —
/// единственная: код от root через `ModuleName=../…` и `.so` в теме Plymouth
/// не проходит и там.
fn system_theme_problem(
    step: &ExtStep,
    last_stage: Option<&str>,
) -> Option<crate::installer::SystemThemeProblem> {
    let (kind, theme_id) = match step {
        ExtStep::Run { argv, .. } => {
            let args = helper_args(argv);
            (args.first()?.as_str(), args.get(1).map(String::as_str))
        }
        ExtStep::PkexecScript { prefix, .. } => (prefix.strip_prefix("rexon_")?, None),
        _ => return None,
    };
    if kind != "sddm" && kind != "plymouth" {
        return None;
    }
    crate::installer::system_theme_problem(kind, Path::new(last_stage?), theme_id)
}

// ============================================================================
// Копирование
// ============================================================================

/// Перенести дерево `src` в `dest` — одно поведение у всех путей установки
/// (шаг 2 слияния; прежде у `.rxn` и у магазина были разные).
///
/// * Символьная ссылка ПЕРЕСОЗДАЁТСЯ ссылкой с той же целью (иконочные наборы
///   вроде Buuf состоят из них почти целиком, и раскрытие раздуло бы тему в
///   разы; к тому же раскрытие ссылки наружу скопировало бы чужой файл).
///   Висячая ссылка тоже переносится: относительная цель (`../hicolor/…`)
///   под временным каталогом распаковки висит, а на месте установки может
///   быть жива. Если повторить висячую ссылку не удалось, она пропускается
///   — не повод ронять установку целиком; живая ссылка, которую не удалось
///   повторить, — ошибка.
/// * Файл или ссылка, уже лежащие на месте ссылки, заменяются (повторная
///   установка поверх); каталог — нет.
/// * Обычный файл не пишется СКВОЗЬ ссылку, оставшуюся на его месте: ссылка
///   снимается, файл кладётся на её место (иначе копия ушла бы туда, куда
///   ссылка указывает, — за пределы темы).
///
/// С записью в откат `undo` ([`Undo::off`] — без неё): каталог, которого не было, —
/// заведён целиком (внутри него поштучно не записывается); в существующий
/// каталог — поштучно, заменённое откладывается в сторону.
///
/// Имена обходятся по порядку (сравнение байтов имени): порядок файловой
/// системы от машины к машине разный, а с ним — что успело лечь до сбоя.
pub(crate) fn copy_tree(src: &Path, dest: &Path, undo: &mut Undo) -> std::io::Result<()> {
    if undo.enabled && std::fs::symlink_metadata(dest).is_err() {
        make_dirs(dest, undo, true)?;
        return copy_tree(src, dest, &mut Undo::off());
    }
    std::fs::create_dir_all(dest)?;
    let mut entries: Vec<std::fs::DirEntry> = std::fs::read_dir(src)?.collect::<Result<_, _>>()?;
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let from = entry.path();
        let to = dest.join(entry.file_name());
        let md = std::fs::symlink_metadata(&from)?;
        if md.file_type().is_symlink() {
            let target = std::fs::read_link(&from)?;
            undo.clear_non_dir(&to);
            match std::os::unix::fs::symlink(&target, &to) {
                Ok(()) => undo.created(&to),
                Err(e) => {
                    if from.exists() {
                        return Err(e);
                    }
                }
            }
        } else if md.is_dir() {
            copy_tree(&from, &to, undo)?;
        } else {
            copy_regular(&from, &to, undo)?;
        }
    }
    Ok(())
}

/// Копия файла `src` строго в файл `dest` — одно поведение у всех путей:
/// каталог назначения создаётся, ссылка на месте `dest` снимается (копия не
/// пишется сквозь неё). `dest` — существующий каталог — ошибка: план обязан
/// назвать файл явно (прежде у `.rxn` каталог означал «положить внутрь»,
/// а у магазина — ошибку, и один и тот же шаг значил разное).
pub(crate) fn copy_file(src: &Path, dest: &Path) -> std::io::Result<()> {
    copy_file_undo(src, dest, &mut Undo::off())
}

/// [`copy_file`] с записью в откат `undo`.
pub(crate) fn copy_file_undo(src: &Path, dest: &Path, undo: &mut Undo) -> std::io::Result<()> {
    if let Some(parent) = dest.parent() {
        make_dirs(parent, undo, false)?;
    }
    copy_regular(src, dest, undo)
}

fn copy_regular(src: &Path, dest: &Path, undo: &mut Undo) -> std::io::Result<()> {
    let before = std::fs::symlink_metadata(dest).ok();
    match &before {
        // Файл или ссылка на месте: с откатом — в сторону (вернётся при
        // сбое), без него ссылка снимается, файл перезаписывается.
        Some(m) if !m.is_dir() => {
            if !undo.set_aside(dest) && m.file_type().is_symlink() {
                std::fs::remove_file(dest)?;
            }
        }
        _ => {}
    }
    std::fs::copy(src, dest)?;
    if before.is_none() {
        undo.created(dest);
    }
    Ok(())
}

/// Завести каталог `path` с недостающими родителями. С откатом каждый
/// заведённый записывается: родители — снимаются, только если пусты; сам
/// `path` при `tree` — целиком (всё внутри положил этот прогон).
pub(crate) fn make_dirs(path: &Path, undo: &mut Undo, tree: bool) -> std::io::Result<()> {
    if !undo.enabled {
        return std::fs::create_dir_all(path);
    }
    let mut missing = Vec::new();
    let mut cur = Some(path);
    while let Some(p) = cur {
        if p.as_os_str().is_empty() || std::fs::symlink_metadata(p).is_ok() {
            break;
        }
        missing.push(p.to_path_buf());
        cur = p.parent();
    }
    for dir in missing.iter().rev() {
        match std::fs::create_dir(dir) {
            Ok(()) => {
                let whole = tree && dir.as_path() == path;
                undo.done.push(if whole { Done::CreatedTree(dir.clone()) } else { Done::Created(dir.clone()) });
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && dir.is_dir() => {}
            Err(e) => return Err(e),
        }
    }
    if path.is_dir() {
        Ok(())
    } else {
        // Тот же ответ, что дал бы `create_dir_all` (файл на месте каталога).
        std::fs::create_dir_all(path)
    }
}

// ============================================================================
// Откат установки
// ============================================================================

/// Откат одного прогона плана установки: что прогон положил и что отложил.
///
/// Записывается по ходу копирования, а не выводится из плана заранее
/// ([`crate::install_journal::placed_from_steps`] служит журналу): в
/// существующий каталог (чужой или базовый вроде `~/.local/share/fonts`)
/// план кладёт файлы поштучно, и снять при сбое надо именно их — и только
/// их, не трогая того, что лежало там до установки.
///
/// * Заведённое прогоном (файл, ссылка, каталог) — снимается; каталог,
///   заведённый под копию целиком, — целиком; родительский — только пустой.
/// * Замещаемое (файл, ссылка, прежняя копия темы под `RemoveTree`)
///   откладывается переименованием рядом (`.<имя>.rexon-undo-<pid>-<n>`, та
///   же файловая система) и при сбое возвращается на место. Файл, который
///   `WriteFile` пишет по месту, — копией. Снимок нужен: повторная установка
///   своей темы сначала снимает прежнюю копию, и без него сбой оставил бы
///   человека без темы вовсе; так же для конфигов, которые план пишет.
/// * Успех — отложенное стирается ([`Undo::finish`]).
///
/// Файловые записи слоя включения ([`ApplyStep`]: ini, наши файлы, конфиг
/// композитора) с шага 5а идут под тем же откатом
/// ([`Undo::write_text`], [`Undo::remove_file`]): сбой записи посреди
/// нажатия «Применить» возвращает всё, что нажатие успело записать.
///
/// Не откатываются: системная часть (pkexec — снять её можно только
/// помощником от root) и прочие команды плана (кеши значков, `fc-cache`).
///
/// Значения стола, которые хранит не файл, а служба (`gsettings`/`dconf`,
/// `xfconf-query`), откат с [`Undo::keeping_values`] снимает ДО команды,
/// которая их меняет ([`Undo::run_kept`]): прежнее значение или «ключ не
/// был задан» ложится в [`ValueBefore`] — командой возврата. Файл KDE,
/// который пишет `kwriteconfig6`, там же снимается обычной копией. Сбой посреди
/// плана возвращает их сразу ([`exec_undoable`]), удача отдаёт их в снимок
/// «что было» ([`Undo::take_values`]).
#[derive(Debug, Default)]
pub struct Undo {
    enabled: bool,
    done: Vec<Done>,
    seq: u32,
    /// Прежние значения стола в порядке записи; `None` — не снимаются.
    values: Option<Vec<ValueBefore>>,
}

/// Путь, тронутый прогоном ([`Undo::keep_into`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Touched {
    pub path: std::path::PathBuf,
    /// Копия прежнего содержимого; `None` — путь заведён прогоном.
    pub before: Option<std::path::PathBuf>,
    /// Заведённый прогоном каталог.
    pub dir: bool,
}

#[derive(Debug)]
enum Done {
    /// Заведено прогоном: файл, ссылка или каталог (снимается, только если пуст).
    Created(std::path::PathBuf),
    /// Каталог заведён прогоном под копию целиком.
    CreatedTree(std::path::PathBuf),
    /// Прежнее содержимое `path` отложено в `backup`.
    Saved { path: std::path::PathBuf, backup: std::path::PathBuf },
}

impl Undo {
    /// Откат, который записывает.
    pub fn new() -> Self {
        Undo { enabled: true, ..Default::default() }
    }

    /// Без отката: ничего не записывается и не откладывается (удаление,
    /// копирование вне плана).
    pub fn off() -> Self {
        Undo::default()
    }

    /// Снимать и прежние значения стола (`gsettings`/`dconf`, `xfconf`) до
    /// команд, которые их меняют, — для нажатий «Применить», у которых
    /// есть «Вернуть как было». Без отката ничего не снимается.
    pub fn keeping_values(mut self) -> Self {
        if self.enabled {
            self.values = Some(Vec::new());
        }
        self
    }

    /// Запустить команду плана через `run`; меняет она значение стола —
    /// прежнее снять до неё (у ключа — один раз за прогон: первое и есть
    /// «что было»). Команда не удалась — значение не менялось, снятое
    /// забывается.
    pub(crate) fn run_kept(
        &mut self,
        run: &dyn Fn(&[String], Option<u64>) -> crate::utils::RunReport,
        argv: &[String],
        timeout_secs: Option<u64>,
    ) -> crate::utils::RunReport {
        let mut fresh = false;
        if let Some(values) = self.values.as_mut() {
            if let Some(target) = value_target(argv) {
                if !values.iter().any(|v| v.key == target.key) {
                    match value_before(&target, run) {
                        Some(v) => {
                            values.push(v);
                            fresh = true;
                        }
                        None => crate::log_warn!("install", "{}: прежнее значение не прочитано — вернуть его будет нечем", target.key),
                    }
                }
            }
        }
        // Файл KDE, который перепишет `kwriteconfig6`, — копией до команды
        // (заведённое ею — отметкой после): возврат — файлом.
        let guarded = match (&self.values, kconfig_written_file(argv, &crate::utils::config_home_env())) {
            (Some(_), Some(file)) => Some(self.guard_external(&[file])),
            _ => None,
        };
        let report = run(argv, timeout_secs);
        if let Some(missing) = guarded {
            self.after_external(missing);
        }
        if fresh && report.returncode != Some(0) {
            if let Some(values) = self.values.as_mut() {
                values.pop();
            }
        }
        report
    }

    /// Снятые прежние значения стола (в порядке записи); у отката их больше
    /// нет.
    pub fn take_values(&mut self) -> Vec<ValueBefore> {
        self.values.as_mut().map(std::mem::take).unwrap_or_default()
    }

    /// Путь `path` заведён прогоном.
    fn created(&mut self, path: &Path) {
        if self.enabled {
            self.done.push(Done::Created(path.to_path_buf()));
        }
    }

    /// Пути `paths` сейчас перепишет чужая запись (не через этот откат):
    /// у существующих файлов снять копию, для отсутствующих запомнить
    /// первого недостающего предка — после записи [`Self::after_external`]
    /// отметит заведённое.
    pub(crate) fn guard_external(&mut self, paths: &[std::path::PathBuf]) -> Vec<(std::path::PathBuf, std::path::PathBuf)> {
        let mut missing = Vec::new();
        for p in paths {
            match std::fs::symlink_metadata(p) {
                Ok(m) if m.is_dir() => {}
                Ok(_) => self.keep_copy(p),
                Err(_) => {
                    let mut top = p.clone();
                    while let Some(parent) = top.parent() {
                        if parent.as_os_str().is_empty() || std::fs::symlink_metadata(parent).is_ok() {
                            break;
                        }
                        top = parent.to_path_buf();
                    }
                    missing.push((p.clone(), top));
                }
            }
        }
        missing
    }

    /// Чужая запись прошла: отметить заведённое ею (каталоги от предка
    /// вниз, затем файл) — откат и снимок «что было» снимут их.
    pub(crate) fn after_external(&mut self, missing: Vec<(std::path::PathBuf, std::path::PathBuf)>) {
        let mut seen: Vec<std::path::PathBuf> = Vec::new();
        for (path, top) in missing {
            let mut chain: Vec<std::path::PathBuf> = Vec::new();
            for a in path.ancestors() {
                chain.push(a.to_path_buf());
                if a == top {
                    break;
                }
            }
            for p in chain.into_iter().rev() {
                if std::fs::symlink_metadata(&p).is_ok() && !seen.contains(&p) {
                    self.created(&p);
                    seen.push(p);
                }
            }
        }
    }

    /// Свободное имя рядом с `path` для служебной копии.
    fn side_path(&mut self, path: &Path, what: &str) -> std::path::PathBuf {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let dir = path.parent().unwrap_or(Path::new("."));
        loop {
            self.seq += 1;
            let side = dir.join(format!(".{name}.rexon-{what}-{}-{}", std::process::id(), self.seq));
            if std::fs::symlink_metadata(&side).is_err() {
                return side;
            }
        }
    }

    /// Отложить то, что лежит на `path` (файл, ссылку, каталог), в сторону
    /// вместо удаления. `false` — откат выключен или переименовать не
    /// удалось: вызывающий поступает как без отката.
    fn set_aside(&mut self, path: &Path) -> bool {
        if !self.enabled {
            return false;
        }
        let backup = self.side_path(path, "undo");
        match std::fs::rename(path, &backup) {
            Ok(()) => {
                self.done.push(Done::Saved { path: path.to_path_buf(), backup });
                true
            }
            Err(e) => {
                crate::log_warn!("install", "{}: не отложено для отката — {e}", path.display());
                false
            }
        }
    }

    /// Файл или ссылку на месте `path` убрать: с откатом — в сторону, без
    /// него — снять; каталог не трогается.
    fn clear_non_dir(&mut self, path: &Path) {
        if let Ok(md) = std::fs::symlink_metadata(path) {
            if !md.is_dir() && !self.set_aside(path) {
                let _ = std::fs::remove_file(path);
            }
        }
    }

    /// Завести каталог `path` с недостающими родителями под откатом
    /// (родители снимаются при сбое, только если пусты).
    pub(crate) fn make_dirs(&mut self, path: &Path) -> std::io::Result<()> {
        make_dirs(path, self, false)
    }

    /// Записать `content` в файл `path` под откатом — общий путь файловых
    /// записей слоя включения («Применить», шаг 5а): каталог заводится,
    /// прежнее содержимое снимается копией (сквозь ссылку — копия её цели,
    /// сама ссылка остаётся), заведённый файл при сбое снимается. `atomic` —
    /// запись подменой имени ([`crate::utils::atomic_write`]), иначе по месту.
    pub(crate) fn write_text(&mut self, path: &Path, content: &str, atomic: bool) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                self.make_dirs(parent)?;
            }
        }
        let fresh = match std::fs::symlink_metadata(path) {
            Err(_) => true,
            // Висячая ссылка: запись заменит её файлом — откладываем саму ссылку.
            Ok(m) if m.file_type().is_symlink() && !path.exists() => !self.set_aside(path),
            Ok(_) => {
                self.keep_copy(path);
                false
            }
        };
        if atomic {
            crate::utils::atomic_write(&path.to_string_lossy(), content)?;
        } else {
            std::fs::write(path, content)?;
        }
        if fresh {
            self.created(path);
        }
        Ok(())
    }

    /// Убрать файл или ссылку `path` под откатом (в сторону, при сбое
    /// вернётся). Нет файла — не ошибка.
    pub(crate) fn remove_file(&mut self, path: &Path) -> std::io::Result<()> {
        match std::fs::symlink_metadata(path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
            Ok(m) if m.is_dir() => Err(std::io::Error::other("на месте файла каталог")),
            Ok(_) => {
                if self.set_aside(path) {
                    Ok(())
                } else {
                    std::fs::remove_file(path)
                }
            }
        }
    }

    /// Файл `path` будет переписан по месту: сохранить копию (той файловой
    /// системы, где он лежит; сквозь ссылку — копию её цели).
    fn keep_copy(&mut self, path: &Path) {
        if !self.enabled {
            return;
        }
        let Ok(real) = std::fs::canonicalize(path) else { return };
        if !real.is_file() {
            return;
        }
        let backup = self.side_path(&real, "undo");
        match std::fs::copy(&real, &backup) {
            Ok(_) => self.done.push(Done::Saved { path: real, backup }),
            Err(e) => crate::log_warn!("install", "{}: копия для отката не снята — {e}", real.display()),
        }
    }

    /// Итог прогона: успех — отложенное стереть; сбой — снять положенное и
    /// вернуть отложенное, в обратном порядке.
    pub fn finish(self, ok: bool) {
        if ok {
            self.commit()
        } else {
            self.rollback()
        }
    }

    /// Успех с сохранением «что было» (шаг 5а, «Применить»): отложенные
    /// копии не стираются, а переносятся в каталог снимка `dir` (имена —
    /// номера с `first`). Отдаёт каждый тронутый путь в порядке прогона, по
    /// разу: первое появление пути — правда о том, что было до прогона
    /// (позднейшие копии того же пути — промежуточные, стираются).
    pub fn keep_into(self, dir: &Path, first: usize) -> Vec<Touched> {
        let mut out: Vec<Touched> = Vec::new();
        let mut n = first;
        for d in self.done {
            let (path, backup, is_dir) = match d {
                Done::Created(p) => {
                    let is_dir = std::fs::symlink_metadata(&p).is_ok_and(|m| m.is_dir());
                    (p, None, is_dir)
                }
                Done::CreatedTree(p) => (p, None, true),
                Done::Saved { path, backup } => (path, Some(backup), false),
            };
            if out.iter().any(|t| t.path == path) {
                if let Some(b) = backup {
                    remove_any(&b);
                }
                continue;
            }
            let before = backup.map(|b| {
                let to = dir.join(format!("{n:04}"));
                n += 1;
                let moved = std::fs::create_dir_all(dir).is_ok()
                    && (std::fs::rename(&b, &to).is_ok()
                        || (b.is_file() && std::fs::copy(&b, &to).is_ok() && std::fs::remove_file(&b).is_ok()));
                if moved {
                    to
                } else {
                    // Не перенеслось — копия остаётся рядом с файлом.
                    crate::log_warn!("install", "{}: копия «что было» не перенесена в снимок", b.display());
                    b
                }
            });
            out.push(Touched { path, before, dir: is_dir });
        }
        out
    }

    fn commit(self) {
        for d in self.done {
            if let Done::Saved { backup, .. } = d {
                remove_any(&backup);
            }
        }
    }

    fn rollback(self) {
        for d in self.done.into_iter().rev() {
            match d {
                Done::Created(p) => match std::fs::symlink_metadata(&p) {
                    Ok(m) if m.is_dir() => {
                        let _ = std::fs::remove_dir(&p); // не пуст — значит, не только наше
                    }
                    Ok(_) => {
                        let _ = std::fs::remove_file(&p);
                    }
                    Err(_) => {}
                },
                Done::CreatedTree(p) => remove_any(&p),
                Done::Saved { path, backup } => {
                    // На месте — то, что положил этот прогон (его записи уже
                    // сняты выше); каталог не пуст — не наш, отложенное
                    // остаётся рядом, а не затирает чужое.
                    let free = match std::fs::symlink_metadata(&path) {
                        Ok(m) if m.is_dir() => std::fs::remove_dir(&path).is_ok(),
                        Ok(_) => std::fs::remove_file(&path).is_ok(),
                        Err(_) => true,
                    };
                    let back = free && std::fs::rename(&backup, &path).is_ok();
                    if !back {
                        crate::log_warn!(
                            "install",
                            "{}: прежнее не вернулось на место, оно лежит в {}",
                            path.display(),
                            backup.display()
                        );
                    }
                }
            }
        }
    }
}

/// Снять путь целиком: каталог — с содержимым, файл и ссылку — сами.
fn remove_any(path: &Path) {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() => {
            let _ = std::fs::remove_dir_all(path);
        }
        Ok(_) => {
            let _ = std::fs::remove_file(path);
        }
        Err(_) => {}
    }
}

// ============================================================================
// Значения стола под снимком «что было»
// ============================================================================

/// Прежнее значение ключа стола, которое переписала команда плана: чем его
/// вернуть. Ключ не был задан — возврат сбрасывает его (`reset`, `-r`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValueBefore {
    /// Какой ключ (`gsettings org.gnome.desktop.interface gtk-theme`,
    /// `dconf /org/x/y`, `xfconf xsettings /Net/ThemeName`).
    pub key: String,
    /// Команда возврата.
    pub restore: Vec<String>,
}

/// Ключ, который меняет команда, и как прочитать его нынешнее значение.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ValueTarget {
    key: String,
    kind: TargetKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TargetKind {
    /// `base` — `[env …] gsettings [--schemadir D]`; `assigns` — переменные
    /// `env` перед ним, `tool` — `gsettings [--schemadir D]` (из них
    /// собирается чтение умолчания схемы).
    Gsettings { base: Vec<String>, assigns: Vec<String>, tool: Vec<String>, schema: String, key: String },
    /// `base` — `[env …] dconf`.
    Dconf { base: Vec<String>, key: String },
    /// `base` — `[env …] xfconf-query -c CH -p PROP`; `create` — `-n -t T`
    /// команды (одно значение одного типа): чем завести свойство заново.
    Xfconf { base: Vec<String>, create: Vec<String> },
}

/// Программа команды под префиксом `env VAR=…`: `(префикс, переменные,
/// имя программы, её аргументы)`. Опции самого `env` — не разбираем.
fn split_env(argv: &[String]) -> Option<(&[String], &[String], &str, &[String])> {
    let mut i = 0;
    if argv.first().map(String::as_str) == Some("env") {
        i = 1;
        while argv.get(i).is_some_and(|a| a.contains('=') && !a.starts_with('-')) {
            i += 1;
        }
        if argv.get(i).is_some_and(|a| a.starts_with('-')) {
            return None;
        }
    }
    let program = argv.get(i)?;
    let name = Path::new(program).file_name()?.to_str()?;
    let assigns = if i > 0 { &argv[1..i] } else { &argv[..0] };
    Some((&argv[..=i], assigns, name, &argv[i + 1..]))
}

/// Меняет ли команда значение стола — и какое: `gsettings set|reset`,
/// `dconf write|reset` (один ключ), `xfconf-query … -s|-r` (одно свойство).
/// Остальное (`reset -f`, `-R`, сценарии `bash -c`) не разбирается —
/// снимать такое нечем.
fn value_target(argv: &[String]) -> Option<ValueTarget> {
    let (head, assigns, name, args) = split_env(argv)?;
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    match name {
        "gsettings" => {
            let mut tool = vec![head[head.len() - 1].clone()];
            let mut rest = &a[..];
            if rest.first() == Some(&"--schemadir") {
                tool.extend(rest.iter().take(2).map(|s| s.to_string()));
                rest = rest.get(2..)?;
            }
            let mut base = head[..head.len() - 1].to_vec();
            base.extend(tool.iter().cloned());
            let (schema, key) = match rest {
                ["set", schema, key, _] | ["reset", schema, key] => (*schema, *key),
                _ => return None,
            };
            Some(ValueTarget {
                key: format!("gsettings {schema} {key}"),
                kind: TargetKind::Gsettings { base, assigns: assigns.to_vec(), tool, schema: schema.into(), key: key.into() },
            })
        }
        "dconf" => {
            let key = match &a[..] {
                ["write", key, _] | ["reset", key] => *key,
                _ => return None,
            };
            if !key.starts_with('/') || key.ends_with('/') {
                return None;
            }
            Some(ValueTarget { key: format!("dconf {key}"), kind: TargetKind::Dconf { base: head.to_vec(), key: key.into() } })
        }
        "xfconf-query" => {
            let (mut channel, mut prop) = (None, None);
            let (mut sets, mut reset, mut create) = (0usize, false, false);
            let mut types: Vec<&str> = Vec::new();
            let mut i = 0;
            while i < a.len() {
                let (opt, inline) = match a[i].split_once('=') {
                    Some((o, v)) if o.starts_with("--") => (o, Some(v)),
                    _ => (a[i], None),
                };
                let takes = matches!(opt, "-c" | "--channel" | "-p" | "--property" | "-s" | "--set" | "-t" | "--type");
                let value = if takes {
                    match inline {
                        Some(v) => Some(v),
                        None => {
                            i += 1;
                            Some(*a.get(i)?)
                        }
                    }
                } else {
                    None
                };
                match opt {
                    "-c" | "--channel" => channel = value,
                    "-p" | "--property" => prop = value,
                    "-s" | "--set" => sets += 1,
                    "-t" | "--type" => types.extend(value),
                    "-n" | "--create" => create = true,
                    "-r" | "--reset" => reset = true,
                    // Массив, ветка целиком, переключатель — не снимаем.
                    _ => return None,
                }
                i += 1;
            }
            let (channel, prop) = (channel?, prop?);
            if sets == 0 && !reset {
                return None;
            }
            let mut base = head.to_vec();
            base.extend(["-c", channel, "-p", prop].map(String::from));
            let create = if create && sets == 1 && types.len() == 1 {
                vec!["-n".to_string(), "-t".to_string(), types[0].to_string()]
            } else {
                Vec::new()
            };
            Some(ValueTarget { key: format!("xfconf {channel} {prop}"), kind: TargetKind::Xfconf { base, create } })
        }
        _ => None,
    }
}

/// Файл, который перепишет `kwriteconfig6`/`kwriteconfig5`: `--file` (имя —
/// в каталоге конфигов `config_home`, путь — как есть), без него —
/// `kdeglobals`. `None` — команда не она.
fn kconfig_written_file(argv: &[String], config_home: &str) -> Option<std::path::PathBuf> {
    let (_, _, name, args) = split_env(argv)?;
    if !matches!(name, "kwriteconfig6" | "kwriteconfig5") {
        return None;
    }
    let mut file = "kdeglobals";
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--file" => {
                file = args.get(i + 1)?.as_str();
                i += 1;
            }
            a => {
                if let Some(f) = a.strip_prefix("--file=") {
                    file = f;
                }
            }
        }
        i += 1;
    }
    if file.is_empty() {
        return None;
    }
    Some(if file.starts_with('/') { file.into() } else { Path::new(config_home).join(file) })
}

/// Ключ стола, который меняет команда (как [`ValueBefore::key`]), — чтобы
/// свои команды возврата не возвращали второй раз то, что вернёт снимок.
pub fn value_key_of(argv: &[String]) -> Option<String> {
    value_target(argv).map(|t| t.key)
}

/// Команда только читает значение стола (`gsettings get`, `dconf read`,
/// `xfconf-query` без записи) — так снимается «что было»
/// ([`Undo::run_kept`]). Неудача такого чтения — не причина неудачи нажатия.
pub fn is_value_read(argv: &[String]) -> bool {
    let Some((_, _, name, args)) = split_env(argv) else { return false };
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    match name {
        "gsettings" => {
            let rest = if a.first() == Some(&"--schemadir") { a.get(2..).unwrap_or(&[]) } else { &a[..] };
            rest.first() == Some(&"get")
        }
        "dconf" => a.first() == Some(&"read"),
        "xfconf-query" => !a.iter().any(|o| {
            matches!(*o, "-s" | "--set" | "-r" | "--reset" | "-n" | "--create" | "-T" | "--toggle" | "-l" | "--list")
                || o.starts_with("--set=")
        }),
        _ => false,
    }
}

/// Ответ чтения без перевода строки в конце.
fn read_text(r: &crate::utils::RunReport) -> String {
    crate::utils::decode_text(&r.stdout).trim_end_matches(['\n', '\r']).to_string()
}

/// Прочитать нынешнее значение `target` и собрать команду, которая его
/// вернёт. `None` — прочитать не удалось (вернуть будет нечем).
fn value_before(target: &ValueTarget, run: &dyn Fn(&[String], Option<u64>) -> crate::utils::RunReport) -> Option<ValueBefore> {
    let with = |base: &[String], tail: &[&str]| -> Vec<String> {
        base.iter().cloned().chain(tail.iter().map(|s| s.to_string())).collect()
    };
    let restore = match &target.kind {
        TargetKind::Gsettings { base, assigns, tool, schema, key } => {
            let now = run(&with(base, &["get", schema, key]), Some(5));
            if now.returncode != Some(0) {
                return None;
            }
            let now = read_text(&now);
            // Умолчание схемы (с поправками дистрибутива) — мимо записанных
            // значений: совпало — возвращаем сбросом. У gsettings нет вопроса
            // «задан ли ключ», а сброс к тому же значению стол не отличит.
            let mut default = vec!["env".to_string()];
            default.extend(assigns.iter().filter(|a| !a.starts_with("GSETTINGS_BACKEND=")).cloned());
            default.push("GSETTINGS_BACKEND=memory".into());
            default.extend(tool.iter().cloned());
            default.extend(["get", schema, key].map(|s| s.to_string()));
            let d = run(&default, Some(5));
            if d.returncode == Some(0) && read_text(&d) == now {
                with(base, &["reset", schema, key])
            } else {
                with(base, &["set", schema, key, &now])
            }
        }
        TargetKind::Dconf { base, key } => {
            let now = run(&with(base, &["read", key]), Some(5));
            if now.returncode != Some(0) {
                return None;
            }
            let now = read_text(&now);
            if now.trim().is_empty() {
                with(base, &["reset", key])
            } else {
                with(base, &["write", key, &now])
            }
        }
        TargetKind::Xfconf { base, create } => {
            let now = run(base, Some(5));
            if now.returncode != Some(0) {
                // Свойства нет. (Нет и службы — тогда не удастся и сама
                // запись, и снятое забудется: см. [`Undo::run_kept`].)
                with(base, &["-r"])
            } else {
                let now = read_text(&now);
                if now.contains('\n') {
                    return None; // массив: «Value is an array with N items…»
                }
                let mut out = base.clone();
                out.extend(create.iter().cloned());
                out.extend(["-s".to_string(), now]);
                out
            }
        }
    };
    Some(ValueBefore { key: target.key.clone(), restore })
}

/// Вернуть прежние значения стола — в обратном порядке записи. Отдаёт
/// ключи, которые вернуть не вышло.
pub fn restore_values(values: &[ValueBefore], run: &dyn Fn(&[String], Option<u64>) -> crate::utils::RunReport) -> Vec<String> {
    let mut failed = Vec::new();
    for v in values.iter().rev() {
        if v.restore.is_empty() || run(&v.restore, Some(10)).returncode != Some(0) {
            failed.push(v.key.clone());
        }
    }
    failed
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("rexon-install-plan-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn s(p: &Path) -> String {
        p.to_string_lossy().into_owned()
    }

    fn no_run(_: &[String], _: Option<u64>) -> crate::utils::RunReport {
        crate::utils::RunReport::default()
    }

    fn nothing(_: &str) -> bool {
        false
    }

    fn no_spawn(_: &[String]) {}

    fn ctx() -> Ctx<'static> {
        Ctx { install: true, steamos: false, privileged: None, run: &no_run, have: &nothing, spawn: &no_spawn, wayland: false, sync_env: &no_sync_env }
    }

    /// Один план — файлы и включение: файлы кладутся, `ok` ведёт слой
    /// включения, заметки собираются.
    #[test]
    fn mixed_plan_puts_files_and_keeps_ok_flag() {
        let dir = scratch("mixed");
        let file = dir.join("theme/index.theme");
        let steps = vec![
            Step::from(ExtStep::WriteFile { path: s(&file), content: "[Icon Theme]\n".into() }),
            Step::from(ExtStep::Notice { text: "часть пропущена".into() }),
            Step::from(ApplyStep::ForceOk),
        ];
        let report = exec(&steps, &mut Vec::new(), &ctx()).expect("план исполнен");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "[Icon Theme]\n");
        assert_eq!(report.notes, vec!["часть пропущена".to_string()]);
        assert!(report.apply.ok);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Ошибка файлового шага останавливает план и говорит, какой шаг и где.
    #[test]
    fn file_failure_stops_plan() {
        let dir = scratch("fail");
        let missing = s(&dir.join("нет-такого"));
        let after = dir.join("after");
        let steps = vec![
            Step::from(ExtStep::RemoveFile { path: missing.clone() }),
            Step::from(ExtStep::MakeDirs { path: s(&after) }),
        ];
        let r = exec(&steps, &mut Vec::new(), &ctx());
        assert!(matches!(r, Err(Failure::Remove { ref path, .. }) if *path == missing), "{r:?}");
        assert!(!after.exists(), "шаг после ошибки исполнен");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Копия файла — строго файл в файл: каталог назначения создаётся,
    /// существующий каталог на месте файла — ошибка, ссылка на месте файла
    /// снимается, а не пишется насквозь.
    #[test]
    fn copy_file_is_file_to_file() {
        let dir = scratch("copy");
        let src = dir.join("a.txt");
        std::fs::write(&src, "a").unwrap();
        let into = dir.join("into");
        std::fs::create_dir_all(&into).unwrap();
        let step = [Step::from(ExtStep::CopyFile { src: s(&src), dest: s(&into) })];
        let r = exec(&step, &mut Vec::new(), &ctx());
        assert!(matches!(r, Err(Failure::Copy { .. })), "{r:?}");
        assert!(!into.join("a.txt").exists());

        let outside = dir.join("outside.txt");
        std::fs::write(&outside, "чужой").unwrap();
        let dest = dir.join("new/sub/b.txt");
        std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&outside, &dest).unwrap();
        let step = [Step::from(ExtStep::CopyFile { src: s(&src), dest: s(&dest) })];
        exec(&step, &mut Vec::new(), &ctx()).expect("файл в файл");
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "a");
        assert!(!std::fs::symlink_metadata(&dest).unwrap().file_type().is_symlink());
        assert_eq!(std::fs::read_to_string(&outside).unwrap(), "чужой", "копия прошла сквозь ссылку");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Дерево: ссылки переносятся ссылками (и висячие тоже), поверх старой
    /// ссылки ложится новая.
    #[test]
    fn copy_tree_keeps_links() {
        let dir = scratch("tree");
        let src = dir.join("src");
        std::fs::create_dir_all(src.join("sub")).unwrap();
        std::fs::write(src.join("sub/f.svg"), "<svg/>").unwrap();
        std::os::unix::fs::symlink("sub/f.svg", src.join("alias.svg")).unwrap();
        std::os::unix::fs::symlink("../нет-такого", src.join("dangling.svg")).unwrap();
        let dest = dir.join("dest");
        std::fs::create_dir_all(&dest).unwrap();
        std::os::unix::fs::symlink("старая", dest.join("alias.svg")).unwrap();
        let step = [Step::from(ExtStep::CopyTree { src: s(&src), dest: s(&dest) })];
        exec(&step, &mut Vec::new(), &ctx()).expect("дерево перенесено");
        assert_eq!(std::fs::read_link(dest.join("alias.svg")).unwrap(), Path::new("sub/f.svg"));
        assert_eq!(std::fs::read_link(dest.join("dangling.svg")).unwrap(), Path::new("../нет-такого"));
        assert_eq!(std::fs::read_to_string(dest.join("alias.svg")).unwrap(), "<svg/>");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Команды и запуск без ожидания идут ТОЛЬКО через контекст: и файловые
    /// шаги, и шаги включения; подделка ничего настоящего не запускает
    /// (25.10.2026 тест с подделкой запустил `cinnamon --replace` на живом
    /// экране — здесь такой путь закрыт).
    #[test]
    fn commands_and_spawn_go_through_ctx() {
        use crate::themes::Cmd;
        let ran = std::cell::RefCell::new(Vec::<(Vec<String>, Option<u64>)>::new());
        let spawned = std::cell::RefCell::new(Vec::<Vec<String>>::new());
        let run = |argv: &[String], t: Option<u64>| {
            ran.borrow_mut().push((argv.to_vec(), t));
            crate::utils::RunReport { returncode: Some(0), ..Default::default() }
        };
        let spawn = |argv: &[String]| spawned.borrow_mut().push(argv.to_vec());
        let have = |b: &str| b == "есть-такая";
        let c = Ctx { run: &run, spawn: &spawn, have: &have, ..ctx() };
        let v = |a: &[&str]| a.iter().map(|x| x.to_string()).collect::<Vec<String>>();
        let steps = vec![
            Step::from(ExtStep::Run { argv: v(&["кеш"]), timeout_secs: None }),
            Step::from(ExtStep::RunNoted { argv: v(&["заметка"]), timeout_secs: Some(7), failed: "нет".into() }),
            Step::from(ExtStep::Spawn { argv: v(&["cinnamon", "--replace"]) }),
            Step::from(ApplyStep::Primary(Cmd { argv: v(&["включить"]), timeout: Some(5) })),
            Step::from(ApplyStep::Live(Cmd { argv: v(&["нет-такой"]), timeout: None })),
            Step::from(ApplyStep::Live(Cmd { argv: v(&["есть-такая"]), timeout: None })),
        ];
        let report = exec(&steps, &mut Vec::new(), &c).expect("план исполнен");
        assert!(report.apply.ok);
        assert_eq!(report.apply.live, Some(true));
        assert!(report.notes.is_empty());
        assert_eq!(
            *ran.borrow(),
            vec![
                (v(&["кеш"]), None),
                (v(&["заметка"]), Some(7)),
                (v(&["включить"]), Some(5)),
                (v(&["есть-такая"]), None),
            ]
        );
        assert_eq!(*spawned.borrow(), vec![v(&["cinnamon", "--replace"])]);
    }

    /// Без системного исполнителя (магазин) проверка SteamOS не срабатывает,
    /// а `pkexec` идёт обычной командой.
    #[test]
    fn plan_without_privileged_runner_runs_pkexec_as_command() {
        let seen = std::cell::RefCell::new(Vec::<Vec<String>>::new());
        let run = |argv: &[String], _: Option<u64>| {
            seen.borrow_mut().push(argv.to_vec());
            crate::utils::RunReport { returncode: Some(0), ..Default::default() }
        };
        let c = Ctx { steamos: true, run: &run, ..ctx() };
        let argv = vec!["pkexec".to_string(), "true".to_string()];
        let steps = [Step::from(ExtStep::Run { argv: argv.clone(), timeout_secs: Some(5) })];
        exec(&steps, &mut Vec::new(), &c).expect("план исполнен");
        assert_eq!(*seen.borrow(), vec![argv]);
    }

    /// Всё под `root` побайтно: путь, вид, содержимое файла или цель ссылки.
    fn snapshot(root: &Path) -> Vec<(String, String)> {
        let mut out = Vec::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).unwrap().flatten() {
                let p = e.path();
                let rel = p.strip_prefix(root).unwrap().to_string_lossy().into_owned();
                let md = std::fs::symlink_metadata(&p).unwrap();
                let what = if md.file_type().is_symlink() {
                    format!("link → {}", std::fs::read_link(&p).unwrap().display())
                } else if md.is_dir() {
                    stack.push(p.clone());
                    "dir".to_string()
                } else {
                    format!("file {:?}", std::fs::read(&p).unwrap())
                };
                out.push((rel, what));
            }
        }
        out.sort();
        out
    }

    /// Сбой посреди установки: всё, что успел положить прогон, снято, всё
    /// замещённое (файл, ссылка, прежняя копия темы, конфиг, переписанный по
    /// месту) возвращено — профиль побайтно как до установки.
    #[test]
    fn failed_install_rolls_back_to_byte_identical_profile() {
        let dir = scratch("rollback");
        let src = dir.join("src");
        std::fs::create_dir_all(src.join("Theme/sub")).unwrap();
        std::fs::write(src.join("Theme/index.theme"), "новый").unwrap();
        std::fs::write(src.join("Theme/sub/a.svg"), "<svg/>").unwrap();
        std::os::unix::fs::symlink("sub/a.svg", src.join("Theme/alias.svg")).unwrap();
        std::fs::write(src.join("one.colors"), "новая схема").unwrap();

        let home = dir.join("home");
        // Чужой каталог на пути копии — с чужим файлом и файлом того же имени.
        std::fs::create_dir_all(home.join("icons/Foreign")).unwrap();
        std::fs::write(home.join("icons/Foreign/index.theme"), "чужой").unwrap();
        std::fs::write(home.join("icons/Foreign/own.txt"), "чужой свой").unwrap();
        // Прежняя копия своей темы (повторная установка снимает её).
        std::fs::create_dir_all(home.join("themes/Mine/gtk-3.0")).unwrap();
        std::fs::write(home.join("themes/Mine/gtk-3.0/gtk.css"), "старая").unwrap();
        // Файл и ссылка, которые план заменит, и конфиг, который перепишет.
        std::fs::create_dir_all(home.join("schemes")).unwrap();
        std::fs::write(home.join("schemes/one.colors"), "старая схема").unwrap();
        std::os::unix::fs::symlink("/нет/такого", home.join("schemes/two.colors")).unwrap();
        std::fs::write(home.join("app.conf"), "[a]\nkey=old\n").unwrap();
        let before = snapshot(&home);

        let h = |p: &str| s(&home.join(p));
        let steps: Vec<Step> = vec![
            ExtStep::MakeDirs { path: h("new/base") },
            ExtStep::CopyTree { src: s(&src.join("Theme")), dest: h("icons/Foreign") },
            ExtStep::RemoveTree { path: h("themes/Mine"), safe: false, ignore_errors: true },
            ExtStep::CopyTree { src: s(&src.join("Theme")), dest: h("themes/Mine") },
            ExtStep::CopyTree { src: s(&src.join("Theme")), dest: h("new/base/Fresh") },
            ExtStep::CopyFile { src: s(&src.join("one.colors")), dest: h("schemes/one.colors") },
            ExtStep::CopyFile { src: s(&src.join("one.colors")), dest: h("schemes/two.colors") },
            ExtStep::WriteFile { path: h("app.conf"), content: "[a]\nkey=new\n".into() },
            ExtStep::WriteFile { path: h("deep/new/rexon.conf"), content: "x".into() },
            ExtStep::Symlink { target: "themes/Mine".into(), link: h("link") },
            ExtStep::RemoveFile { path: h("нет-такого") },
        ]
        .into_iter()
        .map(Step::from)
        .collect();
        let r = exec(&steps, &mut Vec::new(), &ctx());
        assert!(matches!(r, Err(Failure::Remove { .. })), "{r:?}");
        assert_eq!(snapshot(&home), before, "сбой оставил профиль не таким, как до установки");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Шаг 5а: «Применить» — записи слоя включения под тем же откатом. Сбой
    /// записи посреди нажатия: все конфиги нажатия побайтно прежние (правка
    /// ini по месту, ini сквозь ссылку, наш новый файл, снятый файл, ключ,
    /// убранный из `settings.ini`); шаги после сбоя не исполнены.
    #[test]
    fn failed_apply_press_rolls_back_config_writes() {
        use crate::themes::Cmd;
        let dir = scratch("press");
        let home = dir.join("home");
        let cfg = home.join(".config");
        std::fs::create_dir_all(cfg.join("gtk-3.0")).unwrap();
        std::fs::write(cfg.join("gtk-3.0/settings.ini"), "[Settings]\ngtk-theme-name=Old\ngtk-cursor-theme-name=Old\n").unwrap();
        std::fs::write(cfg.join("kdeglobals"), "[General]\nfoo=1\n[Icons]\nTheme=Old\n").unwrap();
        // Конфиг человека, разложенный ссылкой из его хранилища.
        std::fs::create_dir_all(dir.join("store")).unwrap();
        std::fs::write(dir.join("store/lxqt.conf"), "[General]\nicon_theme=Old\n").unwrap();
        std::fs::create_dir_all(cfg.join("lxqt")).unwrap();
        std::os::unix::fs::symlink(dir.join("store/lxqt.conf"), cfg.join("lxqt/lxqt.conf")).unwrap();
        std::fs::write(cfg.join("ours.conf"), "наш прежний").unwrap();
        // На месте каталога — файл: запись под ним не удастся.
        std::fs::write(cfg.join("gtk-4.0"), "не каталог").unwrap();
        let before = (snapshot(&home), snapshot(&dir.join("store")));

        let c = |p: &str| s(&cfg.join(p));
        let ran = std::cell::RefCell::new(Vec::<Vec<String>>::new());
        let run = |argv: &[String], _: Option<u64>| {
            ran.borrow_mut().push(argv.to_vec());
            crate::utils::RunReport { returncode: Some(0), ..Default::default() }
        };
        let ctx = Ctx { run: &run, ..ctx() };
        let v = |a: &[&str]| a.iter().map(|x| x.to_string()).collect::<Vec<String>>();
        let steps: Vec<Step> = vec![
            ApplyStep::Primary(Cmd { argv: v(&["включить"]), timeout: None }),
            ApplyStep::GtkIni { path: c("gtk-3.0/settings.ini"), key: "gtk-theme-name".into(), value: "New".into() },
            ApplyStep::GtkIniUnsetIfOk { path: c("gtk-3.0/settings.ini"), key: "gtk-cursor-theme-name".into() },
            ApplyStep::IniSetIfOk { path: c("kdeglobals"), section: "[Icons]".into(), key: "Theme".into(), value: "New".into() },
            ApplyStep::IniSetIfOk { path: c("lxqt/lxqt.conf"), section: "[General]".into(), key: "icon_theme".into(), value: "New".into() },
            ApplyStep::WriteIfOk { dir: c("rexon-new/sub"), path: c("rexon-new/sub/cursor.conf"), content: "x".into() },
            ApplyStep::WriteSetsOk { dir: c(""), path: c("ours.conf"), content: "наш новый".into() },
            ApplyStep::RemoveIfOk { path: c("kdeglobals") },
            ApplyStep::GtkIni { path: c("gtk-4.0/settings.ini"), key: "gtk-theme-name".into(), value: "New".into() },
            ApplyStep::IfOk(Cmd { argv: v(&["после-сбоя"]), timeout: None }),
        ]
        .into_iter()
        .map(Step::from)
        .collect();
        let r = exec(&steps, &mut Vec::new(), &ctx);
        assert!(matches!(r, Err(Failure::Write { ref path, .. }) if path.ends_with("gtk-4.0/settings.ini")), "{r:?}");
        assert_eq!((snapshot(&home), snapshot(&dir.join("store"))), before, "сбой посреди «Применить» оставил конфиги не прежними");
        assert_eq!(*ran.borrow(), vec![v(&["включить"])], "шаг после сбоя исполнен");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Шаг 5а: удачное нажатие пишет конфиги и не оставляет служебных копий;
    /// ссылка на конфиг человека остаётся ссылкой.
    #[test]
    fn apply_press_writes_and_leaves_no_undo_copies() {
        let dir = scratch("press-ok");
        let cfg = dir.join("cfg");
        std::fs::create_dir_all(cfg.join("gtk-3.0")).unwrap();
        std::fs::write(cfg.join("gtk-3.0/settings.ini"), "[Settings]\ngtk-theme-name=Old\n").unwrap();
        std::fs::write(dir.join("real.conf"), "[General]\nicon_theme=Old\n").unwrap();
        std::os::unix::fs::symlink(dir.join("real.conf"), cfg.join("lxqt.conf")).unwrap();
        let c = |p: &str| s(&cfg.join(p));
        let steps: Vec<Step> = vec![
            ApplyStep::ForceOk,
            ApplyStep::GtkIni { path: c("gtk-3.0/settings.ini"), key: "gtk-theme-name".into(), value: "New".into() },
            ApplyStep::IniSetIfOk { path: c("lxqt.conf"), section: "[General]".into(), key: "icon_theme".into(), value: "New".into() },
            ApplyStep::WriteSetsOk { dir: c("new"), path: c("new/ours.conf"), content: "наш".into() },
        ]
        .into_iter()
        .map(Step::from)
        .collect();
        let report = exec(&steps, &mut Vec::new(), &ctx()).expect("нажатие удалось");
        assert!(report.apply.ok);
        assert!(std::fs::read_to_string(cfg.join("gtk-3.0/settings.ini")).unwrap().contains("gtk-theme-name=New"));
        assert!(std::fs::symlink_metadata(cfg.join("lxqt.conf")).unwrap().file_type().is_symlink(), "ссылка стала файлом");
        assert!(std::fs::read_to_string(dir.join("real.conf")).unwrap().contains("icon_theme=New"));
        let names: Vec<String> = snapshot(&cfg).into_iter().map(|(p, _)| p).collect();
        assert_eq!(names, ["gtk-3.0", "gtk-3.0/settings.ini", "lxqt.conf", "new", "new/ours.conf"], "остались служебные копии");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Сбой внутри чужого каталога (закрыт на запись): чужое цело, своё
    /// снято, а то, что легло до сбоя, — тоже.
    #[test]
    fn failure_inside_foreign_dir_keeps_foreign() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("foreign");
        let src = dir.join("src/Theme");
        std::fs::create_dir_all(src.join("xfwm4")).unwrap();
        std::fs::write(src.join("index.theme"), "наш").unwrap();
        std::fs::write(src.join("xfwm4/themerc"), "наш").unwrap();
        let home = dir.join("home");
        let foreign = home.join("themes/Theme");
        std::fs::create_dir_all(foreign.join("xfwm4")).unwrap();
        std::fs::write(foreign.join("index.theme"), "чужой").unwrap();
        std::fs::write(foreign.join("xfwm4/keep"), "чужой").unwrap();
        std::fs::set_permissions(foreign.join("xfwm4"), std::fs::Permissions::from_mode(0o555)).unwrap();
        let before = snapshot(&home);
        let steps = [
            Step::from(ExtStep::CopyTree { src: s(&dir.join("src/Theme")), dest: s(&home.join("icons/Fresh")) }),
            Step::from(ExtStep::CopyTree { src: s(&src), dest: s(&foreign) }),
        ];
        let r = exec(&steps, &mut Vec::new(), &ctx());
        let after = snapshot(&home);
        std::fs::set_permissions(foreign.join("xfwm4"), std::fs::Permissions::from_mode(0o755)).unwrap();
        if std::fs::write(foreign.join("xfwm4/probe"), "").is_ok() && r.is_ok() {
            // Под root права на запись не действуют — сбоя не будет.
            let _ = std::fs::remove_dir_all(&dir);
            return;
        }
        assert!(matches!(r, Err(Failure::Copy { .. })), "{r:?}");
        assert_eq!(after, before, "сбой в чужом каталоге задел чужое или оставил своё");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Успех: отложенные для отката копии не остаются рядом; удаление
    /// (`install = false`) не откатывается.
    #[test]
    fn success_leaves_no_undo_copies_and_removal_is_not_rolled_back() {
        let dir = scratch("commit");
        let src = dir.join("a.txt");
        std::fs::write(&src, "новый").unwrap();
        let dest = dir.join("home/a.txt");
        std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
        std::fs::write(&dest, "старый").unwrap();
        std::fs::create_dir_all(dir.join("home/Old")).unwrap();
        let steps = [
            Step::from(ExtStep::RemoveTree { path: s(&dir.join("home/Old")), safe: false, ignore_errors: false }),
            Step::from(ExtStep::CopyFile { src: s(&src), dest: s(&dest) }),
        ];
        exec(&steps, &mut Vec::new(), &ctx()).expect("план исполнен");
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "новый");
        let names: Vec<String> = snapshot(&dir.join("home")).into_iter().map(|(p, _)| p).collect();
        assert_eq!(names, vec!["a.txt".to_string()], "остались служебные копии");

        std::fs::create_dir_all(dir.join("home/Gone")).unwrap();
        let removal = [
            Step::from(ExtStep::RemoveTree { path: s(&dir.join("home/Gone")), safe: false, ignore_errors: false }),
            Step::from(ExtStep::RemoveFile { path: s(&dir.join("home/нет-такого")) }),
        ];
        let c = Ctx { install: false, ..ctx() };
        assert!(exec(&removal, &mut Vec::new(), &c).is_err());
        assert!(!dir.join("home/Gone").exists(), "удаление откатилось");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Подменный стол: хранилища `gsettings` (заданное человеком поверх
    /// умолчаний схем), `dconf` и `xfconf` в памяти и журнал запусков.
    /// Отвечает на те же формы команд, что настоящие программы; настоящие
    /// не запускаются.
    #[derive(Default)]
    pub(crate) struct FakeDesk {
        pub defaults: std::collections::BTreeMap<String, String>,
        pub gsettings: std::cell::RefCell<std::collections::BTreeMap<String, String>>,
        pub dconf: std::cell::RefCell<std::collections::BTreeMap<String, String>>,
        pub xfconf: std::cell::RefCell<std::collections::BTreeMap<String, String>>,
        pub ran: std::cell::RefCell<Vec<Vec<String>>>,
    }

    impl FakeDesk {
        pub(crate) fn run(&self, argv: &[String]) -> crate::utils::RunReport {
            self.ran.borrow_mut().push(argv.to_vec());
            let a: Vec<&str> = argv.iter().map(String::as_str).collect();
            let answer = |ok: bool, out: String| crate::utils::RunReport {
                returncode: Some(if ok { 0 } else { 1 }),
                stdout: out.into_bytes(),
                ..Default::default()
            };
            let id = |s: &str, k: &str| format!("{s} {k}");
            match &a[..] {
                ["env", "GSETTINGS_BACKEND=memory", "gsettings", "get", s, k] => match self.defaults.get(&id(s, k)) {
                    Some(v) => answer(true, format!("{v}\n")),
                    None => answer(false, String::new()),
                },
                ["gsettings", "get", s, k] => match self.defaults.get(&id(s, k)) {
                    Some(d) => answer(true, format!("{}\n", self.gsettings.borrow().get(&id(s, k)).unwrap_or(d))),
                    None => answer(false, String::new()),
                },
                ["gsettings", "set", s, k, v] if self.defaults.contains_key(&id(s, k)) => {
                    self.gsettings.borrow_mut().insert(id(s, k), v.to_string());
                    answer(true, String::new())
                }
                ["gsettings", "reset", s, k] if self.defaults.contains_key(&id(s, k)) => {
                    self.gsettings.borrow_mut().remove(&id(s, k));
                    answer(true, String::new())
                }
                ["dconf", "read", k] => answer(true, self.dconf.borrow().get(*k).map(|v| format!("{v}\n")).unwrap_or_default()),
                ["dconf", "write", k, v] => {
                    self.dconf.borrow_mut().insert(k.to_string(), v.to_string());
                    answer(true, String::new())
                }
                ["dconf", "reset", k] => {
                    self.dconf.borrow_mut().remove(*k);
                    answer(true, String::new())
                }
                ["xfconf-query", "-c", c, "-p", p, rest @ ..] => {
                    let key = id(c, p);
                    let exists = self.xfconf.borrow().contains_key(&key);
                    match rest {
                        [] => answer(exists, self.xfconf.borrow().get(&key).map(|v| format!("{v}\n")).unwrap_or_default()),
                        ["-r"] => {
                            self.xfconf.borrow_mut().remove(&key);
                            answer(true, String::new())
                        }
                        ["-s", v] if exists => {
                            self.xfconf.borrow_mut().insert(key, v.to_string());
                            answer(true, String::new())
                        }
                        ["-n", "-t", _, "-s", v] => {
                            self.xfconf.borrow_mut().insert(key, v.to_string());
                            answer(true, String::new())
                        }
                        _ => answer(false, String::new()),
                    }
                }
                _ => answer(false, String::new()),
            }
        }

        /// Всё, что хранит стол, — для сверки «как было».
        pub(crate) fn state(&self) -> String {
            format!("{:?} {:?} {:?}", self.gsettings.borrow(), self.dconf.borrow(), self.xfconf.borrow())
        }
    }

    pub(crate) fn v(a: &[&str]) -> Vec<String> {
        a.iter().map(|x| x.to_string()).collect()
    }

    /// Разбор команд, меняющих значения стола: какой ключ и чем он
    /// читается; чего не разбираем — не трогаем.
    #[test]
    fn value_targets_of_commands() {
        let key = |a: &[&str]| value_target(&v(a)).map(|t| t.key);
        assert_eq!(key(&["gsettings", "set", "org.a", "k", "'v'"]).as_deref(), Some("gsettings org.a k"));
        assert_eq!(key(&["/usr/bin/gsettings", "reset", "org.a", "k"]).as_deref(), Some("gsettings org.a k"));
        assert_eq!(key(&["env", "X=1", "gsettings", "--schemadir", "/d", "set", "org.a", "k", "1"]).as_deref(), Some("gsettings org.a k"));
        assert_eq!(key(&["gsettings", "get", "org.a", "k"]), None);
        assert_eq!(key(&["gsettings", "reset-recursively", "org.a"]), None);
        assert_eq!(key(&["dconf", "write", "/org/a/k", "'v'"]).as_deref(), Some("dconf /org/a/k"));
        assert_eq!(key(&["dconf", "reset", "-f", "/org/a/"]), None, "ветку целиком не снимаем");
        assert_eq!(key(&["xfconf-query", "-c", "xsettings", "-p", "/Net/ThemeName", "-s", "X"]).as_deref(), Some("xfconf xsettings /Net/ThemeName"));
        assert_eq!(key(&["xfconf-query", "--channel=xfwm4", "--property", "/general/theme", "--set=X"]).as_deref(), Some("xfconf xfwm4 /general/theme"));
        assert_eq!(key(&["xfconf-query", "-c", "c", "-p", "/p", "-r", "-R"]), None, "ветку целиком не снимаем");
        assert_eq!(key(&["xfconf-query", "-c", "c", "-p", "/p"]), None, "чтение не запись");
        assert_eq!(key(&["bash", "-c", "gsettings set org.a k v"]), None);
        assert_eq!(key(&["env", "-i", "gsettings", "set", "org.a", "k", "v"]), None);
        // Чтение умолчания схемы — с теми же переменными и каталогом схем.
        let desk = FakeDesk::default();
        let run = |a: &[String], _: Option<u64>| desk.run(a);
        let t = value_target(&v(&["env", "X=1", "gsettings", "--schemadir", "/d", "set", "org.a", "k", "1"])).unwrap();
        let _ = value_before(&t, &run);
        assert_eq!(
            desk.ran.borrow().clone(),
            [
                v(&["env", "X=1", "gsettings", "--schemadir", "/d", "get", "org.a", "k"]),
            ],
            "чтение не удалось — умолчание не спрашивается"
        );
        for read in [
            v(&["gsettings", "get", "org.a", "k"]),
            v(&["env", "GSETTINGS_BACKEND=memory", "gsettings", "get", "org.a", "k"]),
            v(&["dconf", "read", "/org/a/k"]),
            v(&["xfconf-query", "-c", "c", "-p", "/p"]),
        ] {
            assert!(is_value_read(&read), "{read:?}");
        }
        for write in [v(&["gsettings", "set", "org.a", "k", "v"]), v(&["xfconf-query", "-c", "c", "-p", "/p", "-s", "v"]), v(&["dconf", "write", "/a", "1"])] {
            assert!(!is_value_read(&write), "{write:?}");
        }
    }

    /// Сбой файлового шага посреди нажатия: значения стола, которые успели
    /// смениться, возвращаются сразу (вместе с файлами), снятого не остаётся.
    #[test]
    fn failed_press_puts_values_back() {
        use crate::themes::Cmd;
        let dir = scratch("values-fail");
        let desk = FakeDesk {
            defaults: [("org.a gtk-theme".to_string(), "'Adwaita'".to_string())].into(),
            ..Default::default()
        };
        desk.gsettings.borrow_mut().insert("org.a gtk-theme".into(), "'Old'".into());
        desk.xfconf.borrow_mut().insert("xsettings /Net/ThemeName".into(), "Old".into());
        let before = desk.state();
        let run = |a: &[String], _: Option<u64>| desk.run(a);
        let c = Ctx { run: &run, ..ctx() };
        let blocker = dir.join("файл");
        std::fs::write(&blocker, "").unwrap();
        let steps = vec![
            Step::from(ApplyStep::Primary(Cmd { argv: v(&["gsettings", "set", "org.a", "gtk-theme", "'New'"]), timeout: None })),
            Step::from(ExtStep::Run { argv: v(&["xfconf-query", "-c", "xsettings", "-p", "/Net/ThemeName", "-s", "New"]), timeout_secs: None }),
            Step::from(ExtStep::Run { argv: v(&["xfconf-query", "-c", "xsettings", "-p", "/Net/IconThemeName", "-n", "-t", "string", "-s", "New"]), timeout_secs: None }),
            // Запись под файлом, как под каталогом, — сбой.
            Step::from(ExtStep::WriteFile { path: s(&blocker.join("x.ini")), content: "x".into() }),
        ];
        let mut undo = Undo::new().keeping_values();
        assert!(exec_undoable(&steps, &mut Vec::new(), &c, &mut undo).is_err());
        assert_eq!(desk.state(), before, "значения стола не вернулись после сбоя");
        assert!(undo.take_values().is_empty(), "снятое осталось в откате");
        undo.finish(false);
        // Без `keeping_values` чтений нет: прочие планы идут как прежде.
        desk.ran.borrow_mut().clear();
        let mut plain = Undo::new();
        let _ = exec_undoable(&steps[..1], &mut Vec::new(), &c, &mut plain);
        assert_eq!(desk.ran.borrow().clone(), [v(&["gsettings", "set", "org.a", "gtk-theme", "'New'"])]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `kwriteconfig6`: файл, который она перепишет, снимается копией до
    /// команды — сбой посреди нажатия возвращает его, удача кладёт копию в
    /// снимок «что было». Имя без пути — в каталоге конфигов, без `--file` —
    /// `kdeglobals`.
    #[test]
    fn kwriteconfig_file_is_kept() {
        let cfg = "/h/.config";
        let f = |a: &[&str]| kconfig_written_file(&v(a), cfg);
        assert_eq!(f(&["kwriteconfig6", "--file", "kwinrc", "--group", "G", "--key", "K", "v"]), Some("/h/.config/kwinrc".into()));
        assert_eq!(f(&["kwriteconfig5", "--group", "Icons", "--key", "Theme", "x"]), Some("/h/.config/kdeglobals".into()));
        assert_eq!(f(&["env", "A=1", "kwriteconfig6", "--file=/abs/rc", "--key", "K", "v"]), Some("/abs/rc".into()));
        assert_eq!(f(&["kreadconfig6", "--file", "kwinrc", "--key", "K"]), None);

        let dir = scratch("kconfig");
        let rc = dir.join("kdeglobals");
        std::fs::write(&rc, "[Icons]\nTheme=Old\n").unwrap();
        let fresh = dir.join("sub/kcminputrc");
        let run = |a: &[String], _: Option<u64>| {
            // Подделка kwriteconfig6: пишет файл из `--file`.
            if a[0] == "kwriteconfig6" {
                let p = Path::new(&a[2]);
                std::fs::create_dir_all(p.parent().unwrap()).unwrap();
                std::fs::write(p, "[Icons]\nTheme=New\n").unwrap();
            }
            crate::utils::RunReport { returncode: Some(0), ..Default::default() }
        };
        let c = Ctx { run: &run, ..ctx() };
        let blocker = dir.join("файл");
        std::fs::write(&blocker, "").unwrap();
        let steps = vec![
            Step::from(ExtStep::Run { argv: v(&["kwriteconfig6", "--file", &s(&rc), "--group", "Icons", "--key", "Theme", "New"]), timeout_secs: None }),
            Step::from(ExtStep::Run { argv: v(&["kwriteconfig6", "--file", &s(&fresh), "--key", "K", "v"]), timeout_secs: None }),
        ];
        // Сбой после команд: файл прежний, заведённое снято.
        let mut failing = steps.clone();
        failing.push(Step::from(ExtStep::WriteFile { path: s(&blocker.join("x")), content: "x".into() }));
        let mut undo = Undo::new().keeping_values();
        assert!(exec_undoable(&failing, &mut Vec::new(), &c, &mut undo).is_err());
        undo.finish(false);
        assert_eq!(std::fs::read_to_string(&rc).unwrap(), "[Icons]\nTheme=Old\n");
        assert!(!fresh.exists() && !dir.join("sub").exists(), "заведённое командой осталось");
        // Удача: копия — в снимок, заведённое — отметкой.
        let mut undo = Undo::new().keeping_values();
        exec_undoable(&steps, &mut Vec::new(), &c, &mut undo).unwrap();
        let touched = undo.keep_into(&dir.join("snap"), 1);
        let paths: Vec<&Path> = touched.iter().map(|t| t.path.as_path()).collect();
        assert_eq!(paths, [std::fs::canonicalize(&rc).unwrap().as_path(), dir.join("sub").as_path(), fresh.as_path()]);
        assert_eq!(std::fs::read_to_string(touched[0].before.as_ref().unwrap()).unwrap(), "[Icons]\nTheme=Old\n");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
