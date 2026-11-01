//! Экран входа окончательно упал → понятный текст на текстовой консоли
//! вместо чёрного экрана.
//!
//! ЗАЧЕМ. 07–08.10.2026 обновление поставило Qt вразнобой, и Plasma Login
//! Manager после перезагрузки падал до `start-limit-hit`: человек видел
//! чёрный экран и мигающий курсор, без единого слова о том, что случилось и
//! что делать. [`crate::update_guard`] предупреждает ДО перезагрузки, а этот
//! модуль отвечает за случай «уже поздно».
//!
//! КАК УСТРОЕНО. Пока установлен Rexon, у ЛЮБОГО экрана входа есть drop-in
//! [`DROPIN_NAME`] с `OnFailure=` [`RESCUE_SERVICE`]: он лежит в
//! `display-manager.service.d/` (systemd читает drop-in'ы псевдонима у того
//! юнита, на который псевдоним указывает) и, на случай, если псевдонима
//! нет, — у каждого экрана входа из [`crate::system_plan::LOGIN_MANAGER_SPECS`].
//! Служба зовёт программу [`RESCUE_BIN_NAME`] с [`BOOT_FLAG`]:
//!   1. ждёт, пока systemd сдастся ([`decide`]): `OnFailure=` срабатывает и
//!      на падениях, после которых systemd сам перезапустит юнит (замер на
//!      стенде archlinux 20.10.2026, см. `system_plan::DM_FALLBACK_SCRIPT`);
//!   2. выясняет причину БЕЗ сети ([`diagnose`]): `ldd -r` того, что экран
//!      входа действительно запускает (`ExecStart` и, у SDDM, выбранное
//!      настройками приветствие — [`sddm_greeter`]), и прочих программ
//!      пакета, где «библиотеки нет» не списывается на необязательную
//!      зависимость ([`crate::aeroshell::ldd_problems`]); наборы пакетов
//!      вразнобой ([`crate::update_guard::evaluate`] без ждущих обновлений);
//!      последние строки журнала самого экрана входа — не systemd и не
//!      `ExecStartPre` ([`log_lines`]);
//!   3. пишет текст в [`ISSUE_PATH`] (его печатает `agetty` над приглашением)
//!      и поднимает `getty@` на консоли, которую ядро рисует ([`console_vt`]:
//!      обычно tty1 — экраны входа держат `Conflicts=getty@tty1.service`, и
//!      после их падения там не запущено ничего; на SteamOS — tty4).
//!
//! ЭКРАН ВХОДА ЖИВ, А ОКНА ВХОДА НЕТ. SDDM и Plasma Login Manager не
//! замечают приветствия, упавшего с «чужим» кодом (127 — нет библиотеки,
//! сигнал): служба остаётся `active`, и `OnFailure=` молчит. Этот случай
//! ловит сторож приветствия — таймер из того же drop-in'а
//! ([`decide_greeter`], раздел «Сторож приветствия»); текст и приглашение
//! — те же.
//!
//! ⛔ НИЧЕГО НЕ ЧИНИТ. Ни пакетов, ни экрана входа, ни `reset-failed`:
//! только объясняет и показывает приглашение входа. Чинит человек — по
//! подсказкам `rexon doctor` ([`doctor_report`]), которая тоже только
//! смотрит.
//!
//! СОГЛАСОВАНИЕ СО СЛУЖБОЙ-ОТКАТОМ (`rexon-dm-fallback`, ставится при
//! «Сделать основным»). Обе зовёт `OnFailure=` одного и того же юнита, и
//! эта служба упорядочена `After=` отката. Откат, вернув прежний экран входа,
//! запускает его — и к моменту нашей проверки `display-manager.service` уже
//! не `failed` (`inactive` с заданием запуска либо `activating`): мы ждём.
//! Если упадёт и прежний (у него drop-in отката нет, а наш есть — через
//! псевдоним), сработаем мы, и текст скажет, что откат уже был.
//! Проверяется всегда ТЕКУЩИЙ `display-manager.service`, а не тот юнит,
//! который нас позвал: упавший посторонний экран входа ни при чём.
//! `MONITOR_UNIT` для этого не нужен — и обычно его нет: служба стоит в
//! `OnFailure=` у нескольких юнитов (псевдоним и все экраны входа), и
//! systemd тогда переменную не передаёт («multiple trigger source
//! candidates», замер в ВМ SteamOS 25.10.2026).
//!
//! ЗАЩИТА ОТ ЗАЦИКЛИВАНИЯ: отметка [`STAMP_PATH`] в `/run` — один показ за
//! загрузку; `StartLimitIntervalSec=0` у службы — частые вызовы на
//! перезапусках экрана входа не исчерпают её собственный предел запусков
//! раньше того вызова, ради которого она существует.
//!
//! ЯЗЫК. На консоли при загрузке языка сеанса нет: берётся системная локаль
//! (`/etc/locale.conf`, её же systemd отдаёт службам в `LANG`), перевод — из
//! каталогов, вшитых при сборке ([`crate::system_l10n::translated`]), как у
//! остального системного текста Rexon: на диске при загрузке может не быть
//! ничего исправного. Шрифт консоли по умолчанию кириллицы и прочего не-
//! латинского не рисует — поэтому под переводом всегда идёт английский.

use crate::i18n::{fill, n_};
use crate::system::PkgFamily;
use crate::system_exec::SystemIo;
use crate::update_guard::{self, Effect, Finding, Survey};

// ══════════════════════════════════════════════════════════════════════════
// Файлы в системе
// ══════════════════════════════════════════════════════════════════════════

/// Программа службы (в пакете — [`RESCUE_BIN_PACKAGE`]). Без Qt: её зовут
/// именно тогда, когда Qt мог сломаться.
pub const RESCUE_BIN_NAME: &str = "rexon-login-rescue";
/// Путь программы в пакете.
pub const RESCUE_BIN_PACKAGE: &str = "/usr/lib/rexon/rexon-login-rescue";
/// Служба: её зовёт `OnFailure=` экрана входа.
pub const RESCUE_SERVICE: &str = "rexon-login-rescue.service";
/// Drop-in экрана входа с `OnFailure=`. Имя не совпадает с drop-in'ом
/// отката (`50-rexon-fallback.conf`): совпади оно, файл в `/etc` перекрыл бы
/// наш в `/usr/lib`, и одна из служб потерялась бы.
pub const DROPIN_NAME: &str = "50-rexon-login-rescue.conf";
/// Каталог юнитов пакета.
pub const UNIT_DIR_PACKAGE: &str = "/usr/lib/systemd/system";
/// Псевдоним текущего экрана входа.
pub const DM_ALIAS: &str = "display-manager.service";
/// Текст над приглашением `agetty` (`/run` — для созданного на лету; с
/// util-linux 2.41 читается всегда, раньше — только без `/etc/issue`).
pub const ISSUE_PATH: &str = "/run/issue.d/50-rexon-login-rescue.issue";
/// Отметка «в эту загрузку уже показали».
pub const STAMP_PATH: &str = "/run/rexon/login-rescue.done";
/// Командная строка ядра: из неё — какие консоли ядро рисует на экране.
pub const KERNEL_CMDLINE: &str = "/proc/cmdline";
/// Предел номеров консолей в ядре (`MAX_NR_CONSOLES`).
const MAX_VT: u32 = 63;

/// Номер консоли, которую человек УВИДИТ: первая из `fbcon=vc:<n1>-<n2>`
/// (номера с 1, как у `ttyN`), если первая консоль в этот промежуток не
/// входит, иначе 1. SteamOS грузится с `fbcon=vc:4-6`: на tty1 ядро не
/// рисует ничего, и приглашение там — тот же чёрный экран (замер в ВМ
/// SteamOS 3.9.5, 25.10.2026: текст виден только на tty4). Опции `fbcon=`
/// идут через запятую; повтор параметра — действует последний.
pub fn console_vt(cmdline: &str) -> u32 {
    let mut first = None;
    for arg in cmdline.split_whitespace() {
        let Some(opts) = arg.strip_prefix("fbcon=") else { continue };
        for opt in opts.split(',') {
            if let Some(range) = opt.strip_prefix("vc:") {
                first = range.split('-').next().and_then(|n| n.trim().parse::<u32>().ok());
            }
        }
    }
    match first {
        Some(n) if n > 1 && n <= MAX_VT => n,
        _ => 1,
    }
}

/// Управляющая последовательность консоли Linux «включить погашенный экран»
/// (`ESC [ 13 ]`, console_codes(4)).
pub const UNBLANK_SEQ: &str = "\x1b[13]";

/// Устройство консоли `vt`.
pub fn tty_path(vt: u32) -> String {
    format!("/dev/tty{vt}")
}

/// Служба приглашения входа на консоли `vt`.
pub fn getty_unit(vt: u32) -> String {
    format!("getty@tty{vt}.service")
}
/// Флаг запуска из службы.
pub const BOOT_FLAG: &str = "--boot";
/// Флаг одной проверки сторожа приветствия (из [`WATCH_SERVICE`]).
pub const WATCH_FLAG: &str = "--watch-greeter";
/// Служба и таймер сторожа приветствия.
pub const WATCH_SERVICE: &str = "rexon-greeter-watch.service";
pub const WATCH_TIMER: &str = "rexon-greeter-watch.timer";
/// Первая проверка — через столько секунд после старта экрана входа.
pub const WATCH_FIRST_SECS: u32 = 45;
/// Следующие — через столько секунд после конца предыдущей.
pub const WATCH_EVERY_SECS: u32 = 30;
/// Экран входа моложе этого — приветствие ещё может подниматься.
pub const GREETER_GRACE_SECS: u64 = 30;
/// Перепроверка перед показом: окна входа нет и через столько секунд.
pub const WATCH_RECHECK_SECS: u64 = 10;
/// Флаг отчёта для человека (`rexon doctor`).
pub const DOCTOR_FLAG: &str = "--doctor";
/// Команда, которую текст на консоли предлагает выполнить.
pub const DOCTOR_COMMAND: &str = "rexon doctor";
/// Слово маршрута `rexon doctor` (первый аргумент).
pub const DOCTOR_WORD: &str = "doctor";
/// Строк журнала в тексте.
pub const LOG_LINES: usize = 5;
/// Длина строки журнала в тексте, символы.
pub const LOG_LINE_MAX: usize = 160;

/// Юнит службы. `exec` — путь программы.
pub fn rescue_unit(exec: &str) -> String {
    format!(
        "[Unit]\n\
         Description=Rexon: explain on the text console why the login screen did not start\n\
         DefaultDependencies=no\n\
         After=sysinit.target {fallback}\n\
         StartLimitIntervalSec=0\n\
         \n\
         [Service]\n\
         Type=oneshot\n\
         ExecStart={exec} {BOOT_FLAG}\n",
        fallback = crate::system_plan::DM_FALLBACK_SERVICE,
    )
}

/// Drop-in экрана входа: `OnFailure=` — на падение самого экрана входа,
/// `Wants=` — сторож приветствия ([`WATCH_TIMER`]) на случай, когда экран
/// входа жив, а окна входа нет.
pub fn dropin() -> String {
    format!("[Unit]\nOnFailure={RESCUE_SERVICE}\nWants={WATCH_TIMER}\n")
}

/// Служба сторожа приветствия: одна проверка ([`WATCH_FLAG`]). Обычные
/// зависимости по умолчанию (`Conflicts=shutdown.target`): при выключении
/// systemd её не запустит.
pub fn watch_unit(exec: &str) -> String {
    format!(
        "[Unit]\n\
         Description=Rexon: explain on the text console why the login window is gone\n\
         \n\
         [Service]\n\
         Type=oneshot\n\
         ExecStart={exec} {WATCH_FLAG}\n"
    )
}

/// Таймер сторожа: первая проверка через [`WATCH_FIRST_SECS`] после старта
/// экрана входа, дальше — через [`WATCH_EVERY_SECS`] после конца
/// предыдущей. Повтор нужен автовходу: приветствие запускается только после
/// выхода из сеанса, и упасть оно может через час после загрузки.
pub fn watch_timer() -> String {
    format!(
        "[Unit]\n\
         Description=Rexon: check that the login screen still shows its login window\n\
         \n\
         [Timer]\n\
         OnActiveSec={WATCH_FIRST_SECS}s\n\
         OnUnitInactiveSec={WATCH_EVERY_SECS}s\n\
         AccuracySec=5s\n\
         Unit={WATCH_SERVICE}\n"
    )
}

/// Юниты, которым кладётся drop-in: псевдоним и все известные экраны входа.
/// Двойной `OnFailure=` на одну службу systemd сводит в одну зависимость.
pub fn dropin_units() -> Vec<String> {
    let mut units = vec![DM_ALIAS.to_string()];
    for spec in crate::system_plan::LOGIN_MANAGER_SPECS {
        let unit = format!("{}.service", spec.unit);
        if !units.contains(&unit) {
            units.push(unit);
        }
    }
    units
}

/// Все файлы страховки в каталоге юнитов `unit_dir`: (путь, текст). Их
/// печатает пакету `rexon-login-rescue --files`/`--file`, удаляет их
/// `pacman -R` — они в учёте пакета.
pub fn unit_files(unit_dir: &str, exec: &str) -> Vec<(String, String)> {
    let dir = unit_dir.trim_end_matches('/');
    let mut files = vec![
        (format!("{dir}/{RESCUE_SERVICE}"), rescue_unit(exec)),
        (format!("{dir}/{WATCH_SERVICE}"), watch_unit(exec)),
        (format!("{dir}/{WATCH_TIMER}"), watch_timer()),
    ];
    for unit in dropin_units() {
        files.push((format!("{dir}/{unit}.d/{DROPIN_NAME}"), dropin()));
    }
    files
}

// ══════════════════════════════════════════════════════════════════════════
// Состояние юнита и решение «ждать / показать»
// ══════════════════════════════════════════════════════════════════════════

/// Свойства [`DM_ALIAS`] из `systemctl show`.
pub const SHOW_PROPS: &[&str] =
    &["Id", "LoadState", "ActiveState", "SubState", "Result", "ExecStart", "ExecStartPre", "ExecStartPost",
      "ActiveEnterTimestampMonotonic", "Conflicts"];

/// Состояние юнита экрана входа.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UnitState {
    /// Настоящее имя (`sddm.service`), не псевдоним.
    pub id: String,
    pub load: String,
    pub active: String,
    pub sub: String,
    pub result: String,
    /// Программа из `ExecStart` (`path=`).
    pub exec_path: Option<String>,
    /// Имена программ `ExecStartPre=`/`ExecStartPost=` (см. [`aux_commands`]):
    /// их строки в журнале юнита — не голос экрана входа.
    pub aux_commands: Vec<String>,
    /// Когда юнит стал `active`, мкс от загрузки (`ActiveEnterTimestampMonotonic`;
    /// 0 — ещё ни разу).
    pub active_since_us: Option<u64>,
    /// `Conflicts=`: экран входа уходит, если запустить `getty@` его консоли.
    pub conflicts: Vec<String>,
}

impl UnitState {
    /// Имя без `.service` (`sddm`).
    pub fn short(&self) -> &str {
        self.id.strip_suffix(".service").unwrap_or(&self.id)
    }

    /// Имя для человека: из таблицы экранов входа, иначе юнит.
    pub fn label(&self) -> String {
        crate::system_plan::login_manager_spec(self.short())
            .map(|s| s.label.to_string())
            .unwrap_or_else(|| self.short().to_string())
    }

    /// systemd сдался: юнит `failed` и сам больше не перезапустится.
    pub fn gave_up(&self) -> bool {
        self.active == "failed" && !self.sub.starts_with("auto-restart")
    }
}

/// Разбор `systemctl show -p …` (строки `Ключ=значение`).
pub fn parse_show(text: &str) -> UnitState {
    let mut s = UnitState::default();
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else { continue };
        let value = value.trim().to_string();
        match key.trim() {
            "Id" => s.id = value,
            "LoadState" => s.load = value,
            "ActiveState" => s.active = value,
            "SubState" => s.sub = value,
            "Result" => s.result = value,
            "ExecStart" => s.exec_path = exec_path(&value),
            "ActiveEnterTimestampMonotonic" => s.active_since_us = value.parse().ok().filter(|v| *v > 0),
            "Conflicts" => s.conflicts = value.split_whitespace().map(str::to_string).collect(),
            "ExecStartPre" | "ExecStartPost" => {
                for name in aux_commands(&value) {
                    if !s.aux_commands.contains(&name) {
                        s.aux_commands.push(name);
                    }
                }
            }
            _ => {}
        }
    }
    // Своя программа юнита своей же «вспомогательной» не считается
    // (`ExecStartPre=/usr/bin/sddm --check` не глушит строки самого sddm).
    if let Some(main) = s.exec_path.as_deref().map(base_name) {
        s.aux_commands.retain(|n| n != main);
    }
    s
}

fn base_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Имена программ из значения `ExecStartPre=`/`ExecStartPost=` в выводе
/// `systemctl show`: у каждой команды `{ path=… ; argv[]=… ; … }` — имя
/// `path`, первого слова `argv[]` и, если это `sh -c '…'`, первого слова
/// сценария. Так в журнале узнаются строки и самой оболочки (её
/// `SYSLOG_IDENTIFIER`), и запущенной ею программы (`_COMM`): на SteamOS
/// `ExecStartPre` экрана входа удаляет временные настройки автовхода, и
/// `rm: cannot remove …zzt-*-temp-login.conf` (замер в ВМ, 25.10.2026)
/// заслонял настоящую ошибку.
pub fn aux_commands(value: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |name: &str| {
        let name = base_name(name.trim_matches(|c| c == '"' || c == '\'' || c == '-' || c == '@' || c == '!' || c == '+' || c == ':'));
        if !name.is_empty() && !out.iter().any(|n| n == name) {
            out.push(name.to_string());
        }
    };
    for block in value.split("path=").skip(1) {
        if let Some(path) = block.split([' ', ';']).next() {
            push(path);
        }
        let Some(args) = block.split("argv[]=").nth(1) else { continue };
        let args = args.split(" ; ").next().unwrap_or(args);
        let words: Vec<&str> = args.split_whitespace().collect();
        if let Some(first) = words.first() {
            push(first);
        }
        if let Some(pos) = words.iter().position(|w| *w == "-c") {
            if let Some(script) = words.get(pos + 1) {
                push(script);
            }
        }
    }
    out
}

/// Программа из значения `ExecStart`: `{ path=/usr/bin/sddm ; argv[]=… }`.
fn exec_path(value: &str) -> Option<String> {
    let rest = value.split("path=").nth(1)?;
    let path = rest.split([' ', ';']).next()?.trim();
    path.starts_with('/').then(|| path.to_string())
}

/// Что видно при вызове службы.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BootFacts {
    /// `systemctl is-system-running` = `stopping`.
    pub stopping: bool,
    /// [`STAMP_PATH`] уже есть.
    pub already_shown: bool,
    /// `MONITOR_UNIT` от systemd (кто упал); пусто — не передан.
    pub monitor_unit: String,
    pub dm: UnitState,
}

/// Решение службы.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Не наш случай — причина для журнала.
    Skip(&'static str),
    /// Экран входа ещё может подняться (перезапуск, откат запустил прежний).
    Wait,
    /// Экран входа окончательно упал: показать текст и приглашение.
    Rescue,
}

/// Ждать, пропустить или показать. Решает состояние ТЕКУЩЕГО экрана
/// входа; `monitor_unit` — лишь добавочный отсев, когда systemd его
/// передал (обычно нет — см. шапку модуля): упал посторонний, а текущий
/// жив — `Wait` и так; текущий сдался — показать, кто бы ни позвал.
pub fn decide(f: &BootFacts) -> Verdict {
    if f.stopping {
        return Verdict::Skip("system is shutting down");
    }
    if f.already_shown {
        return Verdict::Skip("already shown during this boot");
    }
    if f.dm.load != "loaded" || f.dm.id.is_empty() || f.dm.id == DM_ALIAS {
        return Verdict::Skip("no login screen is set up as display-manager.service");
    }
    if !f.monitor_unit.is_empty() && f.monitor_unit != f.dm.id && f.monitor_unit != DM_ALIAS {
        return Verdict::Skip("the failed unit is not the current login screen");
    }
    if !f.dm.gave_up() {
        return Verdict::Wait;
    }
    Verdict::Rescue
}

// ══════════════════════════════════════════════════════════════════════════
// Сторож приветствия: экран входа жив, а окна входа нет
// ══════════════════════════════════════════════════════════════════════════
//
// ЗАЧЕМ. SDDM и Plasma Login Manager после выхода приветствия смотрят только
// на три кода помощника (ошибка дисплея, консоли, сеанса — `HELPER_*` 4, 5,
// 2). Приветствие, которое не нашло библиотеку (127) или упало сигналом,
// даёт другой код: экран входа ничего не делает, его служба остаётся
// `active`, `OnFailure=` не срабатывает, а человек видит чёрный экран. При
// автовходе это проявляется только после выхода из сеанса.
//
// КАК УСТРОЕНО. Таймер [`WATCH_TIMER`] (его тянет `Wants=` из того же
// drop-in'а) раз в [`WATCH_EVERY_SECS`] зовёт [`WATCH_FLAG`]. Проверка
// дешёвая и идёт от дешёвого к дорогому ([`watch_facts`]): отметка показа →
// состояние экрана входа → сеансы logind на seat0 → журнал. Показ — только
// если ([`decide_greeter`]) экран входа `active` дольше
// [`GREETER_GRACE_SECS`], на seat0 нет ни приветствия, ни живого сеанса
// человека, а в журнале экрана входа после его запуска последним словом
// помощника стоит неудачный выход ([`greeter_crash_evidence`]); у Plasma
// Login Manager — ещё и когда сеанс приветствия есть, но упали его
// пользовательские юниты ([`PLASMALOGIN_GREETER_UNITS`]): там приветствие
// живёт службой systemd пользователя, и сеанс после её падения не
// закрывается. Перед показом — перепроверка через [`WATCH_RECHECK_SECS`].
//
// ⛔ Над экраном входа — никаких действий; консоль для приглашения
// выбирается так, чтобы `getty@` не попал в его `Conflicts=` ([`pick_vt`]).

/// Экраны входа, у которых сторож что-то понимает (журнал помощника).
pub const WATCHED_DMS: &[&str] = &["sddm", "plasmalogin"];
/// Пользователь приветствия Plasma Login Manager.
pub const PLASMALOGIN_GREETER_USER: &str = "plasmalogin";
/// Юниты systemd пользователя приветствия Plasma Login Manager: окно входа и
/// его композитор (цель держит оба через `BindsTo=`).
pub const PLASMALOGIN_GREETER_UNITS: &[&str] = &["plasma-login.service", "plasma-login-kwin_wayland.service"];
/// Программы приветствия Plasma Login Manager (их — в `ldd`).
const PLASMALOGIN_GREETER_PROGRAMS: &[&str] = &["startplasma-login-wayland", "plasma-login-greeter"];
/// Место, куда logind сажает графические сеансы.
pub const SEAT: &str = "seat0";
/// Активная консоль ядра.
pub const ACTIVE_VT_PATH: &str = "/sys/class/tty/tty0/active";

/// Программы приветствия, которые экран входа запускает помимо `ExecStart`
/// (у SDDM приветствие выбирает [`sddm_greeter`]).
pub fn greeter_programs(dm_short: &str, files: &[String]) -> Vec<String> {
    if dm_short != "plasmalogin" {
        return Vec::new();
    }
    files.iter().filter(|f| PLASMALOGIN_GREETER_PROGRAMS.contains(&base_name(f))).cloned().collect()
}

/// Сеанс logind (`loginctl show-session`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionInfo {
    pub id: String,
    pub seat: String,
    pub class: String,
    pub state: String,
}

impl SessionInfo {
    fn live_on_seat(&self) -> bool {
        self.seat == SEAT && self.state != "closing"
    }
    /// Сеанс приветствия на seat0.
    pub fn is_greeter(&self) -> bool {
        self.live_on_seat() && self.class == "greeter"
    }
    /// Человек на seat0 — графический сеанс или вход на консоли (его тоже
    /// не уводим [`pick_vt`]'ом). `closing` — сеанс, из которого вышли, а
    /// процессы ещё доживают: он не в счёт (автовход после выхода).
    pub fn is_person(&self) -> bool {
        self.live_on_seat() && (self.class.starts_with("user") || self.class == "lock-screen")
    }
}

/// Номера сеансов из `loginctl list-sessions -o json`; не JSON (старый
/// systemd) — первое слово каждой строки `--no-legend`.
pub fn parse_session_ids(text: &str) -> Vec<String> {
    if let Ok(serde_json::Value::Array(items)) = serde_json::from_str::<serde_json::Value>(text.trim()) {
        return items.iter().map(|v| json_field(v, "session")).filter(|s| !s.is_empty()).collect();
    }
    text.lines().filter_map(|l| l.split_whitespace().next()).map(str::to_string).collect()
}

/// Свойства сеанса для `loginctl show-session -p …`.
pub const SESSION_PROPS: &[&str] = &["Id", "Seat", "Class", "State"];

/// Разбор `loginctl show-session -p Id -p Seat -p Class -p State`.
pub fn parse_session_show(text: &str) -> SessionInfo {
    let mut s = SessionInfo::default();
    for line in text.lines() {
        let Some((k, v)) = line.split_once('=') else { continue };
        let v = v.trim().to_string();
        match k.trim() {
            "Id" => s.id = v,
            "Seat" => s.seat = v,
            "Class" => s.class = v,
            "State" => s.state = v,
            _ => {}
        }
    }
    s
}

/// Упавшие юниты из `systemctl show -p Id -p ActiveState` по нескольким
/// юнитам (блоки через пустую строку).
pub fn failed_units(show: &str) -> Vec<String> {
    show.split("\n\n")
        .map(parse_show)
        .filter(|u| !u.id.is_empty() && u.active == "failed")
        .map(|u| u.id)
        .collect()
}

/// Код из «Auth: sddm-helper exited with 127» (и `plasmalogin-helper`);
/// «exited successfully» — 0.
fn helper_exit_code(msg: &str) -> Option<i64> {
    if !msg.contains("-helper exited") {
        return None;
    }
    if msg.contains("-helper exited successfully") {
        return Some(0);
    }
    let rest = msg.split("-helper exited with ").nth(1)?.trim_start();
    let end = rest.char_indices().find(|(i, c)| !(c.is_ascii_digit() || (*i == 0 && *c == '-'))).map(|(i, _)| i).unwrap_or(rest.len());
    rest[..end].parse().ok()
}

/// Загрузчик не смог запустить программу.
fn is_loader_error(msg: &str) -> bool {
    msg.contains("symbol lookup error") || msg.contains("error while loading shared libraries")
}

/// Улика из журнала экрана входа (`journalctl -u … -o json`): последнее
/// слово помощника после `since_us` (запуск экрана входа) — неудачный выход
/// или его падение. Сообщение загрузчика (нет библиотеки, нет символа)
/// после последнего удачного выхода — тоже в улику: его строка понятнее
/// кода. Удачный выход (человек вышел из сеанса) улику снимает.
///
/// Код выхода помощника виден не всегда: когда приветствие идёт под
/// Wayland-композитором или под X от имени пользователя sddm (x11-user — так
/// на SteamOS, где weston нет и SDDM откатывается с wayland), между
/// помощником и приветствием стоит `sddm-helper-start-*`, и он выходит с 0
/// при любом коде приветствия. Тогда улика — «Greeter stopped.» без
/// предшествующего «Greeter stopping...»: остановку при входе человека SDDM
/// всегда начинает сам и пишет «stopping», а тут окно закрылось само (замер
/// в ВМ SteamOS 3.9.5, 25.10.2026). «Greeter starting...» начинает отсчёт
/// заново: новое окно входа — прежние слова не в счёт.
pub fn greeter_crash_evidence(entries: &[JournalEntry], since_us: Option<u64>) -> Option<String> {
    let mut last_bad: Option<String> = None;
    let mut loader: Option<String> = None;
    let mut stopping = false;
    for e in entries {
        if e.from_systemd() {
            continue;
        }
        if let (Some(since), Some(at)) = (since_us, e.mono) {
            if at < since {
                continue;
            }
        }
        let msg = e.message.trim();
        if msg.starts_with("Greeter starting") {
            last_bad = None;
            loader = None;
            stopping = false;
        } else if msg.starts_with("Greeter stopping") {
            stopping = true;
        } else if msg.starts_with("Greeter stopped") {
            if !stopping && last_bad.is_none() {
                last_bad = Some(msg.to_string());
            }
            stopping = false;
        } else if let Some(code) = helper_exit_code(msg) {
            if code == 0 {
                last_bad = None;
                loader = None;
            } else {
                last_bad = Some(msg.to_string());
            }
        } else if msg.contains("-helper (") && msg.contains("crashed") {
            last_bad = Some(msg.to_string());
        } else if is_loader_error(msg) {
            loader = Some(msg.to_string());
        }
    }
    last_bad.map(|bad| loader.unwrap_or(bad))
}

/// Что видит сторож приветствия.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WatchFacts {
    pub stopping: bool,
    pub already_shown: bool,
    pub dm: UnitState,
    /// Время от загрузки, мкс (`/proc/uptime`).
    pub uptime_us: Option<u64>,
    pub sessions: Vec<SessionInfo>,
    /// Упавшие юниты приветствия (только Plasma Login Manager).
    pub greeter_units_failed: Vec<String>,
    /// [`greeter_crash_evidence`].
    pub evidence: Option<String>,
}

/// Решение сторожа приветствия: `Rescue` — показать текст.
pub fn decide_greeter(f: &WatchFacts) -> Verdict {
    if f.stopping {
        return Verdict::Skip("system is shutting down");
    }
    if f.already_shown {
        return Verdict::Skip("already shown during this boot");
    }
    if f.dm.load != "loaded" || f.dm.id.is_empty() || f.dm.id == DM_ALIAS {
        return Verdict::Skip("no login screen is set up as display-manager.service");
    }
    if !WATCHED_DMS.contains(&f.dm.short()) {
        return Verdict::Skip("this login screen is not watched");
    }
    if f.dm.active != "active" {
        // Упавший экран входа — дело `OnFailure=` (`--boot`).
        return Verdict::Skip("the login screen is not running");
    }
    if let (Some(up), Some(since)) = (f.uptime_us, f.dm.active_since_us) {
        if up < since.saturating_add(GREETER_GRACE_SECS * 1_000_000) {
            return Verdict::Wait;
        }
    }
    if f.sessions.iter().any(SessionInfo::is_person) {
        return Verdict::Skip("someone is logged in on seat0");
    }
    if f.sessions.iter().any(SessionInfo::is_greeter) {
        return if f.greeter_units_failed.is_empty() {
            Verdict::Skip("the login window is there")
        } else {
            Verdict::Rescue
        };
    }
    if f.evidence.is_none() {
        return Verdict::Skip("no sign that the login window crashed");
    }
    Verdict::Rescue
}

/// Отметка [`STAMP_PATH`]: «юнит причина».
pub fn stamp_text(dm_id: &str, greeter: bool) -> String {
    format!("{dm_id} {}\n", if greeter { "greeter" } else { "failed" })
}

/// Отметка говорит «окна входа не было» у этого экрана входа.
pub fn stamp_says_greeter(stamp: Option<&str>, dm_id: &str) -> bool {
    stamp.is_some_and(|s| {
        let mut w = s.split_whitespace();
        w.next() == Some(dm_id) && w.next() == Some("greeter")
    })
}

/// Время от загрузки, мкс, из `/proc/uptime`.
pub fn parse_uptime_us(text: &str) -> Option<u64> {
    let secs: f64 = text.split_whitespace().next()?.parse().ok()?;
    (secs >= 0.0).then(|| (secs * 1_000_000.0) as u64)
}

/// Номер активной консоли из [`ACTIVE_VT_PATH`] (`tty1`).
pub fn parse_active_vt(text: &str) -> Option<u32> {
    text.trim().strip_prefix("tty")?.parse().ok()
}

/// Последняя консоль из `fbcon=vc:<n1>-<n2>`; нет — `None`.
fn drawn_last_vt(cmdline: &str) -> Option<u32> {
    let mut last = None;
    for arg in cmdline.split_whitespace() {
        let Some(opts) = arg.strip_prefix("fbcon=") else { continue };
        for opt in opts.split(',') {
            if let Some(range) = opt.strip_prefix("vc:") {
                last = range.split('-').nth(1).and_then(|n| n.trim().parse::<u32>().ok());
            }
        }
    }
    last
}

/// Консоль для приглашения, когда экран входа жив: начиная с
/// [`console_vt`], в пределах того, что ядро рисует, — не та, на которой
/// сейчас экран входа (`active_vt`), и не та, чей `getty@` у экрана входа в
/// `Conflicts=` (systemd остановил бы экран входа). `None` — подходящей нет.
pub fn pick_vt(cmdline: &str, active_vt: Option<u32>, conflicts: &[String]) -> Option<u32> {
    let first = console_vt(cmdline);
    let last = drawn_last_vt(cmdline).filter(|l| *l >= first).unwrap_or(first + 11).min(MAX_VT);
    (first..=last).find(|vt| Some(*vt) != active_vt && !conflicts.contains(&getty_unit(*vt)))
}

/// Упавшие службы окна входа Plasma Login Manager (`systemctl --user -M
/// plasmalogin@`); у других экранов входа — пусто.
fn greeter_units_failed_live<IO: SystemIo + ?Sized>(io: &IO, dm: &UnitState) -> Vec<String> {
    if dm.short() != "plasmalogin" {
        return Vec::new();
    }
    let mut a = argv(&["systemctl", "--user", "-M", &format!("{PLASMALOGIN_GREETER_USER}@"), "show"]);
    a.extend(PLASMALOGIN_GREETER_UNITS.iter().map(|u| u.to_string()));
    a.extend(argv(&["-p", "Id", "-p", "ActiveState"]));
    failed_units(&io.run(&a, Some(10), true).stdout)
}

/// Окно входа Plasma Login Manager пишет в журнал своей службы
/// пользователя, а не экрана входа: при упавших `units` разбор идёт по их
/// журналу. Общее у сторожа и `rexon doctor`.
fn use_greeter_journal<IO: SystemIo + ?Sized>(io: &IO, observed: &mut Observed, units: &[String]) {
    if units.is_empty() {
        return;
    }
    observed.journal = io.run(&greeter_journal_argv(units), Some(20), true).stdout;
    observed.greeter_units = units.to_vec();
}

/// Собрать [`WatchFacts`] (только чтение), от дешёвого к дорогому: дальше
/// не идёт, когда решение уже ясно. `stamp` — текст [`STAMP_PATH`].
pub fn watch_facts<IO: SystemIo + ?Sized>(io: &IO, stamp: Option<&str>) -> WatchFacts {
    let mut f = WatchFacts { already_shown: stamp.is_some(), ..Default::default() };
    if f.already_shown {
        return f;
    }
    f.stopping = io.run(&argv(&["systemctl", "is-system-running"]), Some(10), true).stdout.trim() == "stopping";
    f.dm = dm_state_live(io);
    f.uptime_us = io.read_text("/proc/uptime").as_deref().and_then(parse_uptime_us);
    if f.stopping || f.dm.active != "active" || !WATCHED_DMS.contains(&f.dm.short()) {
        return f;
    }
    let ids = parse_session_ids(&io.run(&argv(&["loginctl", "list-sessions", "-o", "json", "--no-pager"]), Some(10), true).stdout);
    for id in ids {
        let mut a = argv(&["loginctl", "show-session", &id]);
        for p in SESSION_PROPS {
            a.push("-p".into());
            a.push(p.to_string());
        }
        f.sessions.push(parse_session_show(&io.run(&a, Some(10), true).stdout));
    }
    if f.sessions.iter().any(SessionInfo::is_person) {
        return f;
    }
    if f.sessions.iter().any(SessionInfo::is_greeter) {
        f.greeter_units_failed = greeter_units_failed_live(io, &f.dm);
        return f;
    }
    let journal = io.run(&argv(&["journalctl", "-b", "-u", &f.dm.id, "-n", "300", "-o", "json", "--no-pager"]), Some(20), true).stdout;
    f.evidence = greeter_crash_evidence(&parse_journal_json(&journal), f.dm.active_since_us);
    f
}

// ══════════════════════════════════════════════════════════════════════════
// Откат на прежний экран входа уже был?
// ══════════════════════════════════════════════════════════════════════════

/// Откат (`rexon-dm-fallback`) в эту загрузку уже вернул `dm_short`, а тот
/// тоже упал → имя экрана входа, с которого откатились. `conf` — текст
/// `dm-fallback.conf`, `done_mtime`/`boot_time` — секунды эпохи отметки
/// `dm-fallback.done` и начала загрузки.
pub fn fallback_from(conf: Option<&str>, done_mtime: Option<u64>, boot_time: Option<u64>, dm_short: &str) -> Option<String> {
    let conf = conf?;
    let fresh = matches!((done_mtime, boot_time), (Some(d), Some(b)) if d >= b);
    let val = |k: &str| conf.lines().find_map(|l| l.strip_prefix(k)?.strip_prefix('=').map(str::trim));
    (fresh && val("old") == Some(dm_short)).then(|| val("new").unwrap_or_default().to_string())
}

/// Начало загрузки, секунды эпохи: строка `btime` из `/proc/stat`.
pub fn parse_btime(proc_stat: &str) -> Option<u64> {
    proc_stat.lines().find_map(|l| l.strip_prefix("btime ")?.trim().parse().ok())
}

// ══════════════════════════════════════════════════════════════════════════
// Причина
// ══════════════════════════════════════════════════════════════════════════

/// Что не так.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cause {
    /// `ldd -r`: библиотеки нет, версии символа нет, символа нет.
    Library { program: String, problem: String },
    /// Части набора пакетов разных версий ([`update_guard::PACKAGE_SETS`]).
    PackageMix(Finding),
}

/// Наблюдения для [`diagnose`] — всё, что собрано снаружи.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Observed {
    pub dm: UnitState,
    /// Пакет экрана входа (если узнали).
    pub package: Option<String>,
    /// Файлы пакета — соседи для [`crate::aeroshell::ldd_problems`].
    pub package_files: Vec<String>,
    /// (программа, вывод `ldd -r`).
    pub ldd: Vec<(String, String)>,
    /// Программы, которые экран входа действительно запускает
    /// ([`entry_programs`]): `ExecStart` и выбранное приветствие.
    pub entries: Vec<String>,
    /// Необязательные зависимости пакета: (имя, установлен ли) — `pacman
    /// -Qi`; `None` — не узнали (не Arch или pacman не ответил).
    pub optional_deps: Option<Vec<(String, bool)>>,
    /// `pacman -Q` и `pacman -Qg …` (только Arch).
    pub pacman_q: Option<String>,
    pub pacman_qg: Option<String>,
    /// `pacman -Qmq`: пакеты вне репозиториев ([`update_guard::foreign_packages`]).
    pub foreign: Option<std::collections::BTreeSet<String>>,
    /// `journalctl -b -u <юнит> -o json` (разбор — [`log_lines`]).
    pub journal: String,
    /// Откатились с этого экрана входа ([`fallback_from`]).
    pub fallback_from: Option<String>,
    /// Экран входа жив, а окна входа нет ([`decide_greeter`]).
    pub greeter_gone: bool,
    /// Упавшие службы окна входа: тогда `journal` — их журнал
    /// ([`greeter_journal_argv`]), а разбор — [`greeter_log_lines`].
    pub greeter_units: Vec<String>,
}

/// Итог разбора.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Diagnosis {
    pub dm: UnitState,
    pub package: Option<String>,
    pub causes: Vec<Cause>,
    pub log: Vec<String>,
    pub fallback_from: Option<String>,
    /// См. [`Observed::greeter_gone`].
    pub greeter_gone: bool,
}

/// Бед `ldd` на одну программу и всего в тексте.
const LDD_PER_PROGRAM: usize = 2;
const LDD_TOTAL: usize = 6;

/// Разобрать наблюдения. Чистая функция: решения здесь, походы наружу —
/// в [`observe_live`].
pub fn diagnose(o: &Observed) -> Diagnosis {
    let mut causes = Vec::new();
    // Пакеты вразнобой — первыми: это причина, а ldd показывает следствие.
    if let Some(q) = &o.pacman_q {
        let survey = Survey {
            installed: update_guard::parse_installed(q),
            groups: update_guard::parse_groups(o.pacman_qg.as_deref().unwrap_or("")),
            pending: Vec::new(),
            repo: None,
            foreign: o.foreign.clone(),
        };
        for f in update_guard::evaluate(&survey, update_guard::PACKAGE_SETS) {
            if f.set.effect == Effect::LoginScreen {
                causes.push(Cause::PackageMix(f));
            }
        }
    }
    let mut seen: Vec<String> = Vec::new();
    let mut libs = 0;
    // «Библиотеки нет» у программы, которую экран входа не запускает, —
    // чаще всего просто не поставленная необязательная зависимость (пакет
    // sddm несёт приветствие на Qt5, а qt5-declarative у него
    // необязательна): верим такой беде, только когда все необязательные
    // зависимости пакета стоят.
    let optional_all_installed = o.optional_deps.as_ref().is_some_and(|d| d.iter().all(|(_, installed)| *installed));
    for (program, out) in &o.ldd {
        let entry = o.entries.contains(program);
        let problems = crate::aeroshell::ldd_problems(out, &o.package_files)
            .into_iter()
            .filter(|p| entry || optional_all_installed || !p.ends_with(": not found"));
        for problem in problems.take(LDD_PER_PROGRAM) {
            if libs >= LDD_TOTAL || seen.contains(&problem) {
                continue;
            }
            seen.push(problem.clone());
            libs += 1;
            causes.push(Cause::Library { program: program.clone(), problem });
        }
    }
    Diagnosis {
        dm: o.dm.clone(),
        package: o.package.clone(),
        causes,
        log: if o.greeter_units.is_empty() { log_lines(&o.journal, &o.dm.aux_commands) } else { greeter_log_lines(&o.journal) },
        fallback_from: o.fallback_from.clone(),
        greeter_gone: o.greeter_gone,
    }
}

/// Запись журнала юнита (`journalctl -o json`): текст и кто его написал.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JournalEntry {
    pub message: String,
    /// `_PID`.
    pub pid: String,
    /// `SYSLOG_IDENTIFIER`.
    pub ident: String,
    /// `_COMM`.
    pub comm: String,
    /// `__MONOTONIC_TIMESTAMP`, мкс от загрузки.
    pub mono: Option<u64>,
}

impl JournalEntry {
    /// Сам systemd о юните («Scheduled restart job…», «start-limit-hit»,
    /// «Triggering OnFailure=…»): это одинаково при любой причине.
    pub fn from_systemd(&self) -> bool {
        self.pid == "1" || self.ident == "systemd"
    }
}

/// Строка поля записи: текст или (не UTF-8) массив байтов.
fn json_field(v: &serde_json::Value, key: &str) -> String {
    match v.get(key) {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Array(bytes)) => {
            let raw: Vec<u8> = bytes.iter().filter_map(|b| b.as_u64()).map(|b| b as u8).collect();
            String::from_utf8_lossy(&raw).into_owned()
        }
        Some(serde_json::Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

/// Разбор `journalctl -o json` (запись на строку). Строка не JSON — текст
/// неизвестного автора (старый вывод `-o cat`).
pub fn parse_journal_json(text: &str) -> Vec<JournalEntry> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| match serde_json::from_str::<serde_json::Value>(l) {
            Ok(v) if v.is_object() => JournalEntry {
                message: json_field(&v, "MESSAGE"),
                pid: json_field(&v, "_PID"),
                ident: json_field(&v, "SYSLOG_IDENTIFIER"),
                comm: json_field(&v, "_COMM"),
                mono: json_field(&v, "__MONOTONIC_TIMESTAMP").parse().ok(),
            },
            _ => JournalEntry { message: l.to_string(), ..Default::default() },
        })
        .collect()
}

/// Строки журнала для человека: голос самого экрана входа (и его
/// приветствия, помощников), без systemd и без программ
/// `ExecStartPre`/`ExecStartPost` (`aux` — [`UnitState::aux_commands`]).
/// Если экран входа не сказал ничего (упал молча, сигналом) — из строк
/// systemd только те, где сказано, как закончился процесс.
pub fn log_lines(journal_json: &str, aux: &[String]) -> Vec<String> {
    let entries = parse_journal_json(journal_json);
    let is_aux = |e: &JournalEntry| aux.iter().any(|a| *a == e.ident || *a == e.comm);
    let own: Vec<&str> = entries.iter().filter(|e| !e.from_systemd() && !is_aux(e)).map(|e| e.message.as_str()).collect();
    let picked: Vec<&str> = if own.iter().any(|m| !m.trim().is_empty()) {
        own
    } else {
        entries.iter().filter(|e| e.from_systemd() && e.message.contains("process exited")).map(|e| e.message.as_str()).collect()
    };
    log_tail(&picked.join("\n"))
}

/// Строки systemd о том, как кончилась служба.
fn is_exit_report(msg: &str) -> bool {
    msg.contains("process exited") || msg.contains("Failed with result")
}

/// Шум графического стека: окно входа на Qt пишет его при каждом запуске
/// (в ВМ SteamOS — «libEGL warning: …», «MESA: error: ZINK: …»), и о
/// падении он обычно ничего не говорит. Сообщение загрузчика — не шум.
fn is_render_noise(msg: &str) -> bool {
    const MARKS: &[&str] = &["EGL", "MESA", "Mesa", "ZINK", "zink", "libGL", "GLX", "glx", "dri2", "dri3", "DRI2", "DRI3"];
    !is_loader_error(msg) && MARKS.iter().any(|m| msg.contains(m))
}

/// Строк systemd о конце службы и строк самого окна входа в хвосте.
const GREETER_EXIT_LINES: usize = 2;
const GREETER_OWN_LINES: usize = 3;

/// Строки журнала упавшей службы окна входа ([`greeter_journal_argv`]):
/// как она кончилась (systemd пользователя: «Main process exited,
/// code=dumped, status=11/SEGV») и несколько последних слов самого процесса.
/// Шум графики ([`is_render_noise`]) отброшен, если процесс сказал что-то
/// ещё; сказал только его — лучше он, чем ничего.
pub fn greeter_log_lines(journal_json: &str) -> Vec<String> {
    let entries = parse_journal_json(journal_json);
    let said = |e: &JournalEntry| !e.from_systemd() && !e.message.trim().is_empty();
    let clear = entries.iter().any(|e| said(e) && !is_render_noise(&e.message));
    let exits: Vec<usize> = entries.iter().enumerate().filter(|(_, e)| e.from_systemd() && is_exit_report(&e.message)).map(|(i, _)| i).collect();
    let exits = &exits[exits.len().saturating_sub(GREETER_EXIT_LINES)..];
    let own: Vec<usize> = entries.iter().enumerate().filter(|(_, e)| said(e) && (!clear || !is_render_noise(&e.message))).map(|(i, _)| i).collect();
    let room = GREETER_OWN_LINES.min(LOG_LINES.saturating_sub(exits.len()));
    let own = &own[own.len().saturating_sub(room)..];
    let mut picked: Vec<usize> = own.iter().chain(exits).copied().collect();
    picked.sort_unstable();
    log_tail(&picked.iter().map(|&i| entries[i].message.as_str()).collect::<Vec<_>>().join("\n"))
}

/// `journalctl` для журнала служб окна входа `units`: строки процесса несут
/// `_SYSTEMD_USER_UNIT`, а строки systemd пользователя о службе («Main
/// process exited») — `USER_UNIT`; `+` у journalctl — «или». (`--user-unit`
/// не годится: он добавляет `_UID` того, кто спрашивает.)
pub fn greeter_journal_argv(units: &[String]) -> Vec<String> {
    let mut a = argv(&["journalctl", "-b", "-n", "200", "-o", "json", "--no-pager"]);
    a.extend(units.iter().map(|u| format!("_SYSTEMD_USER_UNIT={u}")));
    a.push("+".into());
    a.extend(units.iter().map(|u| format!("USER_UNIT={u}")));
    a
}

/// Последние строки текста: без пустых и управляющих символов (консоль
/// исполнила бы их), длинные — обрезаны.
pub fn log_tail(journal: &str) -> Vec<String> {
    let clean: Vec<String> = journal
        .lines()
        .map(|l| l.chars().filter(|c| !c.is_control()).collect::<String>())
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .map(|l| {
            if l.chars().count() > LOG_LINE_MAX {
                let mut cut: String = l.chars().take(LOG_LINE_MAX - 1).collect();
                cut.push('…');
                cut
            } else {
                l
            }
        })
        .collect();
    clean[clean.len().saturating_sub(LOG_LINES)..].to_vec()
}

/// Пакет экрана входа по юниту и семейству пакетного менеджера.
pub fn dm_package(dm_short: &str, family: PkgFamily) -> Option<String> {
    let spec = crate::system_plan::login_manager_spec(dm_short)?;
    if family == PkgFamily::Debian {
        if let Some((_, pkg)) = crate::system_plan::DM_DEBIAN_PKG_FOR_UNIT.iter().find(|(u, _)| *u == dm_short) {
            return Some(pkg.to_string());
        }
    }
    Some(spec.package.to_string())
}

/// Файлы пакета: argv по семейству. `None` — не умеем.
pub fn package_files_argv(family: PkgFamily, package: &str) -> Option<Vec<String>> {
    let argv: &[&str] = match family {
        PkgFamily::Arch => &["pacman", "-Qlq"],
        PkgFamily::Debian => &["dpkg-query", "-L"],
        PkgFamily::Fedora | PkgFamily::Suse | PkgFamily::Alt => &["rpm", "-ql"],
        _ => return None,
    };
    let mut v: Vec<String> = argv.iter().map(|s| s.to_string()).collect();
    v.push(package.to_string());
    Some(v)
}

/// Команда переустановки пакета для человека.
pub fn reinstall_command(family: PkgFamily, package: &str) -> Option<String> {
    Some(match family {
        PkgFamily::Arch => format!("sudo pacman -S {package}"),
        PkgFamily::Debian => format!("sudo apt install --reinstall {package}"),
        PkgFamily::Fedora => format!("sudo dnf reinstall {package}"),
        PkgFamily::Suse => format!("sudo zypper install -f {package}"),
        _ => return None,
    })
}

/// Кандидаты в `ldd`: сначала то, что экран входа действительно запускает
/// (`entries`, [`entry_programs`]), потом файлы пакета в каталогах программ
/// и библиотек, без каталогов, без `skip` (приветствия, которые при этих
/// настройках не запустятся вовсе) и без не-ELF (решает исполнитель).
pub fn ldd_candidates(entries: &[String], files: &[String], skip: &[String]) -> Vec<String> {
    const MAX: usize = 24;
    let mut out: Vec<String> = Vec::new();
    for e in entries {
        if !out.contains(e) {
            out.push(e.clone());
        }
    }
    let in_dirs = |p: &str| ["/usr/bin/", "/usr/sbin/", "/usr/lib", "/usr/libexec/"].iter().any(|d| p.starts_with(d));
    let program = |p: &str| p.starts_with("/usr/bin/") || p.starts_with("/usr/sbin/") || p.starts_with("/usr/libexec/");
    // Программы раньше библиотек: приветствие важнее его модулей.
    let mut rest: Vec<&String> = files.iter().filter(|p| in_dirs(p) && !p.ends_with('/') && !skip.contains(p)).collect();
    rest.sort_by_key(|p| !program(p));
    for p in rest {
        if out.len() >= MAX {
            break;
        }
        if !out.contains(p) {
            out.push(p.clone());
        }
    }
    out
}

/// Каталог тем SDDM по умолчанию (`[Theme] ThemeDir=`).
pub const SDDM_THEME_DIR_DEFAULT: &str = crate::system_plan::SDDM_THEMES_DIR;
/// Настройки SDDM в порядке чтения: каталог пакета, каталог `/etc`,
/// главный файл — поздние перекрывают ранние.
pub const SDDM_CONF_DIRS: &[&str] = &[crate::system_plan::SDDM_DISTRO_CONF_D, crate::system_plan::SDDM_CONF_D];
pub const SDDM_CONF_FILE: &str = "/etc/sddm.conf";
/// Имя программы приветствия SDDM (Qt5 — без суффикса, иначе `-qt<N>`).
const SDDM_GREETER: &str = "sddm-greeter";

/// Программа приветствия SDDM среди файлов пакета. Только из каталога
/// программ: в пакете есть и `/etc/pam.d/sddm-greeter`, и в списке
/// `pacman -Qlq sddm` он идёт ПЕРВЫМ — по нему каталог приветствия
/// выходил `/etc/pam.d`, и `ldd` приветствия не делался вовсе (ВМ SteamOS
/// 3.9.5, 25.10.2026).
fn is_sddm_greeter_program(path: &str) -> bool {
    base_name(path).starts_with(SDDM_GREETER) && path.rsplit_once('/').is_some_and(|(dir, _)| dir.ends_with("/bin") || dir.ends_with("/sbin"))
}

/// Значение `[section] key=` по текстам настроек в порядке чтения: последнее
/// выигрывает, в том числе пустое (так читает SDDM: пустое `Current=` в
/// `/etc/sddm.conf` — встроенная тема).
pub fn ini_value(texts: &[String], section: &str, key: &str) -> Option<String> {
    let mut found = None;
    for text in texts {
        let mut inside = false;
        for line in text.lines() {
            let l = line.trim();
            if l.starts_with('#') || l.starts_with(';') || l.is_empty() {
                continue;
            }
            if let Some(name) = l.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
                inside = name.trim() == section;
                continue;
            }
            if !inside {
                continue;
            }
            if let Some((k, v)) = l.split_once('=') {
                if k.trim() == key {
                    found = Some(v.trim().to_string());
                }
            }
        }
    }
    found
}

/// Какое приветствие запустит SDDM — так же, как решает сам SDDM
/// (`Display::findGreeterTheme`, `Greeter::start`): тема `[Theme] Current`
/// из каталога `[Theme] ThemeDir`; есть тема — версия Qt из её
/// `metadata.desktop` (`[SddmGreeterTheme] QtVersion`, по умолчанию 5),
/// нет темы (пусто или каталога нет) — версия Qt самого SDDM; приветствия
/// под эту версию нет — приветствие версии SDDM. Версия Qt самого SDDM —
/// по пакету: собранный на Qt6 SDDM ставит `sddm-greeter-qt6`.
/// `confs` — тексты настроек в порядке чтения, `files` — файлы пакета,
/// `is_dir`/`is_exec`/`read` — живые пробы.
pub fn sddm_greeter(
    confs: &[String],
    files: &[String],
    is_dir: &dyn Fn(&str) -> bool,
    is_exec: &dyn Fn(&str) -> bool,
    read: &dyn Fn(&str) -> Option<String>,
) -> String {
    let greeters: Vec<&String> = files.iter().filter(|f| is_sddm_greeter_program(f)).collect();
    let bin_dir = greeters.first().and_then(|g| g.rsplit_once('/')).map(|(d, _)| d.to_string()).unwrap_or_else(|| "/usr/bin".into());
    let path_for = |qt: i64| {
        if qt == 5 {
            format!("{bin_dir}/{SDDM_GREETER}")
        } else {
            format!("{bin_dir}/{SDDM_GREETER}-qt{qt}")
        }
    };
    let own_qt = if greeters.iter().any(|g| base_name(g) == "sddm-greeter-qt6") { 6 } else { 5 };
    let theme = ini_value(confs, "Theme", "Current").unwrap_or_default();
    let theme_dir = ini_value(confs, "Theme", "ThemeDir").unwrap_or_else(|| SDDM_THEME_DIR_DEFAULT.into());
    let theme_path = format!("{}/{theme}", theme_dir.trim_end_matches('/'));
    let qt = if theme.is_empty() || !is_dir(&theme_path) {
        own_qt
    } else {
        let meta = read(&format!("{theme_path}/metadata.desktop")).unwrap_or_default();
        // QSettings::toInt(): не число — 0, такого приветствия нет.
        ini_value(&[meta], "SddmGreeterTheme", "QtVersion").map(|v| v.parse().unwrap_or(0)).unwrap_or(5)
    };
    let chosen = path_for(qt);
    if is_exec(&chosen) {
        chosen
    } else {
        path_for(own_qt)
    }
}

/// Программы, которые экран входа запускает: `ExecStart` и, у SDDM,
/// выбранное приветствие ([`sddm_greeter`]). Второе — (`entries`, `skip`):
/// `skip` — приветствия того же пакета, которые при этих настройках не
/// запустятся (их бедам не место среди причин).
pub fn entry_programs(dm: &UnitState, greeter: Option<&str>, files: &[String]) -> (Vec<String>, Vec<String>) {
    let mut entries: Vec<String> = dm.exec_path.iter().cloned().collect();
    let mut skip = Vec::new();
    if let Some(g) = greeter {
        if !entries.iter().any(|e| e == g) {
            entries.push(g.to_string());
        }
        skip = files.iter().filter(|f| is_sddm_greeter_program(f) && f.as_str() != g).cloned().collect();
    }
    (entries, skip)
}

/// Необязательные зависимости из `pacman -Qi` (под `LANG=C`): строки поля
/// `Optional Deps` — «имя: зачем [installed]». `None` — поля нет (вывод не
/// тот); пустой список — `None` в поле.
pub fn parse_optional_deps(qi: &str) -> Option<Vec<(String, bool)>> {
    let mut out = Vec::new();
    let mut inside = false;
    let mut seen = false;
    for line in qi.lines() {
        let value = if let Some(rest) = line.strip_prefix("Optional Deps") {
            inside = true;
            seen = true;
            rest.trim_start().strip_prefix(':').unwrap_or(rest).trim()
        } else if inside && line.starts_with(char::is_whitespace) {
            line.trim()
        } else {
            inside = false;
            continue;
        };
        if value.is_empty() || value == "None" {
            continue;
        }
        let name = value.split([':', ' ']).next().unwrap_or(value).trim();
        // Имя может нести версию: «qt5-declarative>=5.15».
        let name = name.split(['<', '>', '=']).next().unwrap_or(name);
        if !name.is_empty() {
            out.push((name.to_string(), value.ends_with("[installed]")));
        }
    }
    seen.then_some(out)
}

// ══════════════════════════════════════════════════════════════════════════
// Текст
// ══════════════════════════════════════════════════════════════════════════

// TRANSLATORS: Shown on the text console when the graphical login screen failed to start. %1 is a login screen name (SDDM, Plasma Login Manager, …).
const T_HEADLINE: &str = n_("Rexon: the login screen %1 did not start, so the computer shows this text console instead.");
// TRANSLATORS: Shown on the text console when the login screen program still runs but its login window (the greeter) closed, so the screen stayed black. %1 is a login screen name (SDDM, Plasma Login Manager, …).
const T_HEADLINE_GREETER: &str = n_("Rexon: the login screen %1 is running, but its login window has closed, so the computer shows this text console instead.");
// TRANSLATORS: "rexon doctor" output. %1 is a login screen name, %2 its state as systemd reports it (active/running).
const T_DOCTOR_GREETER: &str = n_("The login screen %1 is running (%2), but its login window has closed.");
// TRANSLATORS: Text console message. %1 is a login screen name.
const T_FALLBACK: &str = n_("Rexon had already brought back the previous login screen after %1 failed, but it did not start either.");
// TRANSLATORS: Text console message, a list of causes follows.
const T_CAUSE_HEADER: &str = n_("Likely cause:");
// TRANSLATORS: Text console message. %1 is a product name (Qt, Plasma, KDE Frameworks, GNOME), %2 packages with versions, e.g. "qt6-base 6.12.0 with qt6-declarative 6.11.2".
const T_CAUSE_MIX: &str = n_("parts of %1 of different versions are installed (%2)");
// TRANSLATORS: Text console message. Two lists of packages with versions, e.g. "qt6-base 6.12.0" and "qt6-declarative 6.11.2".
const T_PAIR: &str = n_("%1 with %2");
// TRANSLATORS: Text console message.
const T_CAUSE_UNKNOWN: &str = n_("The cause could not be found without the network.");
// TRANSLATORS: Text console message, lines of the login screen's log follow.
const T_LOG_HEADER: &str = n_("Last lines of its log:");
// TRANSLATORS: Text console message. %1 is a command, keep it as is.
const T_ACTION: &str = n_("Log in below with your user name and password and run: %1");
// TRANSLATORS: Text console message.
const T_UNCHANGED: &str = n_("Rexon has not changed anything on this computer.");
// TRANSLATORS: "rexon doctor" output. %1 is a login screen name, %2 its state as systemd reports it (active/running).
const T_DOCTOR_OK: &str = n_("The login screen %1 is not broken (%2).");
// TRANSLATORS: "rexon doctor" output. display-manager.service is a service name, keep it as is.
const T_DOCTOR_NONE: &str = n_("No login screen is set up on this computer (display-manager.service).");
// TRANSLATORS: "rexon doctor" output. %1 is a login screen name, %2 its state as systemd reports it.
const T_DOCTOR_FAILED: &str = n_("The login screen %1 did not start (%2).");
// TRANSLATORS: "rexon doctor" output when the login screen works, a list of problems follows.
const T_NOTICED: &str = n_("Rexon also noticed:");
// TRANSLATORS: "rexon doctor" output, a list of suggestions follows.
const T_ADVICE_HEADER: &str = n_("What you can do:");
// TRANSLATORS: "rexon doctor" output. %1 is a command, keep it as is.
const T_ADVICE_UPDATE: &str = n_("When the network works, update the system so that all parts fit together: %1");
// TRANSLATORS: "rexon doctor" output. %1 is a command, keep it as is.
const T_ADVICE_ROLLBACK: &str = n_("If there is nothing to update yet, put the previous versions back from the package cache: %1");
// TRANSLATORS: "rexon doctor" output. %1 is a command, keep it as is.
const T_ADVICE_REINSTALL: &str = n_("Reinstall the login screen package: %1");
// TRANSLATORS: "rexon doctor" output. %1 is a command, keep it as is.
const T_ADVICE_LOG: &str = n_("See the whole log of the login screen: %1");
// TRANSLATORS: "rexon doctor" output, the text Rexon saved when it switched the login screen follows.
const T_ADVICE_PREVIOUS: &str = n_("How to go back to the previous login screen:");
// TRANSLATORS: "rexon doctor" output. %1 is a command, keep it as is.
const T_ADVICE_REBOOT: &str = n_("Then restart the computer: %1");
// TRANSLATORS: "rexon doctor" output.
const T_DOCTOR_FOOTER: &str = n_("This command only looks: it has not changed anything.");
// TRANSLATORS: "rexon doctor" output on SteamOS (Steam Deck, Steam Machine). %1 is a command, keep it as is.
const T_ADVICE_STEAMOS_UPDATE: &str = n_("When the network works, update SteamOS: %1");
// TRANSLATORS: "rexon doctor" output on SteamOS. "Steam Deck Recovery Instructions" is the title of Valve's help page, keep it in English; help.steampowered.com is a web address.
const T_ADVICE_STEAMOS_RECOVERY: &str = n_("On SteamOS the system part is read-only, so single packages cannot be reinstalled. If updating does not help, restore SteamOS with Valve's recovery image: see \"Steam Deck Recovery Instructions\" on help.steampowered.com.");
// TRANSLATORS: "rexon doctor" output. %1 and %2 are login screen names.
const T_ADVICE_BOTH_FAILED: &str = n_("Going back to the previous login screen will not help: %1 and %2 both fail to start.");

/// Все строки модуля (для сверки «каждая — через gettext и с вставками»).
pub const TEXTS: &[&str] = &[
    T_HEADLINE, T_FALLBACK, T_CAUSE_HEADER, T_CAUSE_MIX, T_PAIR, T_CAUSE_UNKNOWN, T_LOG_HEADER,
    T_ACTION, T_UNCHANGED, T_DOCTOR_OK, T_DOCTOR_NONE, T_DOCTOR_FAILED, T_ADVICE_HEADER,
    T_ADVICE_UPDATE, T_ADVICE_ROLLBACK, T_ADVICE_REINSTALL, T_ADVICE_LOG, T_ADVICE_PREVIOUS,
    T_ADVICE_REBOOT, T_DOCTOR_FOOTER, T_NOTICED, T_ADVICE_STEAMOS_UPDATE, T_ADVICE_STEAMOS_RECOVERY,
    T_ADVICE_BOTH_FAILED, T_HEADLINE_GREETER, T_DOCTOR_GREETER,
];

/// Переводчик: msgid → текст на нужном языке.
pub type Translate<'a> = &'a dyn Fn(&str) -> String;

fn mix_versions(f: &Finding, t: Translate) -> String {
    const SHOWN: usize = 3;
    let mut odd = f.odd.iter().take(SHOWN).map(|(n, v, _)| format!("{n} {v}")).collect::<Vec<_>>().join(", ");
    if f.odd.len() > SHOWN {
        odd.push_str(", …");
    }
    fill(&t(T_PAIR), &[&format!("{} {}", f.lead.0, f.lead.1), &odd])
}

fn cause_lines(d: &Diagnosis, t: Translate) -> Vec<String> {
    d.causes
        .iter()
        .map(|c| match c {
            Cause::Library { program, problem } => format!("  - {program}: {problem}"),
            Cause::PackageMix(f) if f.stage == update_guard::Stage::Orphaned => {
                format!("  - {}", update_guard::orphan_cause(f, t))
            }
            Cause::PackageMix(f) => format!("  - {}", fill(&t(T_CAUSE_MIX), &[&f.set.name, &mix_versions(f, t)])),
        })
        .collect()
}

/// Текст для консоли на одном языке.
pub fn console_text(d: &Diagnosis, t: Translate) -> String {
    let label = d.dm.label();
    let headline = if d.greeter_gone { T_HEADLINE_GREETER } else { T_HEADLINE };
    let mut lines = vec![fill(&t(headline), &[&label])];
    if let Some(from) = &d.fallback_from {
        let from = crate::system_plan::login_manager_spec(from).map(|s| s.label).unwrap_or(from.as_str());
        lines.push(fill(&t(T_FALLBACK), &[&from]));
    }
    lines.push(String::new());
    if d.causes.is_empty() {
        lines.push(t(T_CAUSE_UNKNOWN));
    } else {
        lines.push(t(T_CAUSE_HEADER));
        lines.extend(cause_lines(d, t));
    }
    if !d.log.is_empty() {
        lines.push(String::new());
        lines.push(t(T_LOG_HEADER));
        lines.extend(d.log.iter().map(|l| format!("  {l}")));
    }
    lines.push(String::new());
    lines.push(fill(&t(T_ACTION), &[&DOCTOR_COMMAND]));
    lines.push(t(T_UNCHANGED));
    lines.join("\n") + "\n"
}

/// Язык консоли: системная локаль (`locale.conf` — текст
/// `/etc/locale.conf`, `env` — окружение службы), если для неё есть
/// вшитый каталог. `None` — английский.
pub fn console_language(locale_conf: Option<&str>, env: &dyn Fn(&str) -> Option<String>) -> Option<&'static str> {
    let from_conf = |k: &str| {
        locale_conf?
            .lines()
            .find_map(|l| l.trim().strip_prefix(k)?.strip_prefix('=').map(|v| v.trim().trim_matches('"').to_string()))
            .filter(|v| !v.is_empty())
    };
    let lookup = |k: &str| env(k).filter(|v| !v.trim().is_empty()).or_else(|| from_conf(k));
    let langs = crate::system_l10n::languages();
    crate::i18n::language_candidates(None, &lookup)
        .into_iter()
        .find_map(|c| langs.iter().copied().find(|l| *l == c))
}

/// Перевод на язык консоли (вшитый каталог; нет перевода — английский).
pub fn console_translate(code: &str, msgid: &str) -> String {
    crate::system_l10n::translated(code, msgid).unwrap_or_else(|| msgid.to_string())
}

/// Перевод для `rexon doctor`: каталог на диске (`disk` — [`crate::i18n::tr`]),
/// а строки, которых в нём нет (или нет самого каталога), — из вшитого
/// каталога языка `lang`.
pub fn doctor_translate(disk: &dyn Fn(&str) -> String, lang: Option<&str>, msgid: &str) -> String {
    let from_disk = disk(msgid);
    if from_disk != msgid {
        return from_disk;
    }
    match lang {
        Some(code) if code != crate::i18n::SOURCE_LANGUAGE => console_translate(code, msgid),
        _ => from_disk,
    }
}

/// Полный текст на консоль: перевод, а под ним английский (шрифт консоли
/// может не рисовать букв языка).
pub fn console_message(d: &Diagnosis, lang: Option<&str>) -> String {
    let en = console_text(d, &|m| m.to_string());
    match lang {
        Some(code) if code != crate::i18n::SOURCE_LANGUAGE => {
            let local = console_text(d, &|m| console_translate(code, m));
            if local == en {
                en
            } else {
                format!("{local}\n{en}")
            }
        }
        _ => en,
    }
}

/// Текст для файла issue: `agetty` понимает `\` как начало вставки
/// (`\n` — имя машины), поэтому `\` удваивается. Рамка — чтобы текст
/// отделялся от приглашения.
pub fn issue_text(message: &str) -> String {
    let rule = "=".repeat(64);
    format!("\n{rule}\n{}{rule}\n\n", message.replace('\\', "\\\\"))
}

/// Версия util-linux из `agetty --version`: (2, 42).
pub fn parse_agetty_version(text: &str) -> Option<(u32, u32)> {
    let ver = text.split("util-linux").nth(1)?.split_whitespace().next()?;
    let mut parts = ver.split('.');
    let number = |p: &str| -> Option<u32> { p.split(|c: char| !c.is_ascii_digit()).next()?.parse().ok() };
    Some((number(parts.next()?)?, number(parts.next()?)?))
}

/// `agetty` этой версии сам покажет [`ISSUE_PATH`] при живом `/etc/issue`
/// (с 2.41). Старше — текст пишется на консоль напрямую.
pub fn agetty_reads_run_issue(version: Option<(u32, u32)>) -> bool {
    version.is_some_and(|v| v >= (2, 41))
}

/// Где запущен `rexon doctor`: от этого зависят советы.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DoctorHost {
    pub family: PkgFamily,
    /// SteamOS ([`crate::distro_compat::running_on_steamos`]): системный
    /// раздел только для чтения, пакетным менеджером не чинится — вместо
    /// `pacman` советуются обновление и восстановление SteamOS.
    pub steamos: bool,
}

/// Отчёт `rexon doctor`: что с экраном входа и что можно сделать. Только
/// советы: ни одной команды он не выполняет.
pub fn doctor_report(d: &Diagnosis, host: DoctorHost, rollback: &[Vec<String>], previous_dm: Option<&str>, t: Translate) -> String {
    let family = host.family;
    let label = d.dm.label();
    let state = format!("{}/{}", d.dm.active, d.dm.sub);
    let mut lines = Vec::new();
    if d.dm.load != "loaded" || d.dm.id.is_empty() || d.dm.id == DM_ALIAS {
        lines.push(t(T_DOCTOR_NONE));
    } else if d.dm.active == "failed" {
        lines.push(fill(&t(T_DOCTOR_FAILED), &[&label, &state]));
    } else if d.greeter_gone {
        lines.push(fill(&t(T_DOCTOR_GREETER), &[&label, &state]));
    } else {
        lines.push(fill(&t(T_DOCTOR_OK), &[&label, &state]));
    }
    if let Some(from) = &d.fallback_from {
        let from = crate::system_plan::login_manager_spec(from).map(|s| s.label).unwrap_or(from.as_str());
        lines.push(fill(&t(T_FALLBACK), &[&from]));
    }
    // Окна входа нет — для советов это та же поломка, что и упавший экран входа.
    let failed = d.dm.active == "failed" || d.greeter_gone;
    if !d.causes.is_empty() {
        lines.push(String::new());
        lines.push(t(if failed { T_CAUSE_HEADER } else { T_NOTICED }));
        lines.extend(cause_lines(d, t));
    }
    if failed && !d.log.is_empty() {
        lines.push(String::new());
        lines.push(t(T_LOG_HEADER));
        lines.extend(d.log.iter().map(|l| format!("  {l}")));
    }
    let mut advice: Vec<String> = Vec::new();
    let mixes: Vec<(usize, &Finding)> = d
        .causes
        .iter()
        .filter_map(|c| if let Cause::PackageMix(f) = c { Some(f) } else { None })
        .enumerate()
        .collect();
    // Сироте обновление не поможет — ей свой совет (удалить).
    let orphans: Vec<String> = mixes.iter().filter_map(|(_, f)| update_guard::orphan_advice(f, t)).collect();
    let mixes: Vec<(usize, &Finding)> = mixes.into_iter().filter(|(_, f)| f.stage != update_guard::Stage::Orphaned).collect();
    let libs = d.causes.iter().any(|c| matches!(c, Cause::Library { .. }));
    if host.steamos {
        // `pacman -S/-Syu/-U/-Rns` на SteamOS не сработают: /usr только для
        // чтения, а части системы ставит только обновление образа.
        if failed || !d.causes.is_empty() {
            advice.push(fill(&t(T_ADVICE_STEAMOS_UPDATE), &[&"sudo steamos-update"]));
            advice.push(t(T_ADVICE_STEAMOS_RECOVERY));
        }
    } else if family == PkgFamily::Arch {
        advice.extend(orphans.iter().cloned());
    }
    if !host.steamos && !mixes.is_empty() && family == PkgFamily::Arch {
        advice.push(fill(&t(T_ADVICE_UPDATE), &[&"sudo pacman -Syu"]));
        let paths: Vec<String> = mixes.iter().flat_map(|(i, _)| rollback.get(*i).cloned().unwrap_or_default()).collect();
        if !paths.is_empty() {
            advice.push(fill(&t(T_ADVICE_ROLLBACK), &[&format!("sudo pacman -U {}", paths.join(" "))]));
        }
    }
    if !host.steamos && libs && mixes.is_empty() && orphans.is_empty() {
        if let Some(cmd) = d.package.as_deref().and_then(|p| reinstall_command(family, p)) {
            advice.push(fill(&t(T_ADVICE_REINSTALL), &[&cmd]));
        }
    }
    if failed && !d.dm.id.is_empty() {
        advice.push(fill(&t(T_ADVICE_LOG), &[&format!("journalctl -b -u {}", d.dm.id)]));
    }
    if failed {
        if let Some(from) = &d.fallback_from {
            // Откат уже вернул прежний экран входа, и упал и он: совет
            // «вернуть прежний» вёл бы ровно на тот, что сейчас не стартует.
            let from = crate::system_plan::login_manager_spec(from).map(|s| s.label).unwrap_or(from.as_str());
            advice.push(fill(&t(T_ADVICE_BOTH_FAILED), &[&from, &label]));
        } else if let Some(prev) = previous_dm.map(str::trim).filter(|p| !p.is_empty()) {
            advice.push(format!("{}\n{}", t(T_ADVICE_PREVIOUS), prev.lines().map(|l| format!("    {l}")).collect::<Vec<_>>().join("\n")));
        }
        if !advice.is_empty() {
            advice.push(fill(&t(T_ADVICE_REBOOT), &[&"sudo reboot"]));
        }
    }
    if !advice.is_empty() {
        lines.push(String::new());
        lines.push(t(T_ADVICE_HEADER));
        lines.extend(advice.into_iter().map(|a| format!("  - {a}")));
    }
    lines.push(String::new());
    lines.push(t(T_DOCTOR_FOOTER));
    lines.join("\n") + "\n"
}

// ══════════════════════════════════════════════════════════════════════════
// Исполнение
// ══════════════════════════════════════════════════════════════════════════

fn argv(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

/// Состояние [`DM_ALIAS`] на живой машине.
pub fn dm_state_live<IO: SystemIo + ?Sized>(io: &IO) -> UnitState {
    let mut a = argv(&["systemctl", "show", DM_ALIAS]);
    for p in SHOW_PROPS {
        a.push("-p".into());
        a.push(p.to_string());
    }
    parse_show(&io.run(&a, Some(15), true).stdout)
}

/// Тексты настроек SDDM в порядке чтения самим SDDM: файлы каталогов
/// [`SDDM_CONF_DIRS`] по имени, затем [`SDDM_CONF_FILE`].
fn sddm_conf_texts<IO: SystemIo + ?Sized>(io: &IO) -> Vec<String> {
    let mut texts = Vec::new();
    for dir in SDDM_CONF_DIRS {
        let mut names = io.list_dir(dir).unwrap_or_default();
        names.sort();
        texts.extend(names.iter().filter_map(|n| io.read_text(&format!("{dir}/{n}"))));
    }
    texts.extend(io.read_text(SDDM_CONF_FILE));
    texts
}

/// Файл — ELF (по первым байтам).
fn is_elf(path: &str) -> bool {
    use std::io::Read;
    let mut magic = [0u8; 4];
    std::fs::File::open(path).and_then(|mut f| f.read_exact(&mut magic)).is_ok() && magic == *b"\x7fELF"
}

fn mtime_secs(path: &str) -> Option<u64> {
    let m = std::fs::metadata(path).ok()?.modified().ok()?;
    m.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs())
}

/// Собрать наблюдения (только чтение).
pub fn observe_live<IO: SystemIo + ?Sized>(io: &IO, family: PkgFamily, dm: UnitState) -> Observed {
    let package = dm_package(dm.short(), family);
    let package_files: Vec<String> = package
        .as_deref()
        .and_then(|p| package_files_argv(family, p))
        .map(|a| io.run(&a, Some(30), true))
        .filter(|r| r.rc() == Some(0))
        .map(|r| r.stdout.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect())
        .unwrap_or_default();
    let greeter = (dm.short() == "sddm").then(|| {
        sddm_greeter(&sddm_conf_texts(io), &package_files, &|p| io.is_dir(p), &|p| io.is_executable(p), &|p| io.read_text(p))
    });
    let (mut entries, skip) = entry_programs(&dm, greeter.as_deref(), &package_files);
    for p in greeter_programs(dm.short(), &package_files) {
        if !entries.contains(&p) {
            entries.push(p);
        }
    }
    let optional_deps = match (&package, family) {
        (Some(p), PkgFamily::Arch) if io.which("pacman") => {
            let r = io.run(&argv(&["pacman", "-Qi", p]), Some(30), true);
            if r.rc() == Some(0) {
                parse_optional_deps(&r.stdout)
            } else {
                None
            }
        }
        _ => None,
    };
    let mut ldd = Vec::new();
    if io.which("ldd") {
        for p in ldd_candidates(&entries, &package_files, &skip) {
            if !io.is_file(&p) || !is_elf(&p) {
                continue;
            }
            let r = io.run(&argv(&["ldd", "-r", &p]), Some(20), true);
            ldd.push((p, format!("{}\n{}", r.stdout, r.stderr)));
        }
    }
    let (pacman_q, pacman_qg) = if family == PkgFamily::Arch && io.which("pacman") {
        let q = io.run(&argv(&["pacman", "-Q"]), Some(60), true);
        let mut g = argv(&["pacman", "-Qg"]);
        g.extend(update_guard::groups_needed(update_guard::PACKAGE_SETS).iter().map(|s| s.to_string()));
        let qg = io.run(&g, Some(60), true);
        ((q.rc() == Some(0)).then_some(q.stdout), Some(qg.stdout))
    } else {
        (None, None)
    };
    let foreign = pacman_q.as_ref().and_then(|_| update_guard::foreign_packages(io, None));
    let journal = if dm.id.is_empty() {
        String::new()
    } else {
        io.run(&argv(&["journalctl", "-b", "-u", &dm.id, "-n", "200", "-o", "json", "--no-pager"]), Some(20), true).stdout
    };
    let conf = io.read_text(&format!("{}/dm-fallback.conf", crate::system_plan::DM_FALLBACK_DIR));
    let done = mtime_secs(&format!("{}/dm-fallback.done", crate::system_plan::DM_FALLBACK_DIR));
    let btime = io.read_text("/proc/stat").as_deref().and_then(parse_btime);
    let fallback_from = fallback_from(conf.as_deref(), done, btime, dm.short());
    Observed {
        dm,
        package,
        package_files,
        ldd,
        entries,
        optional_deps,
        pacman_q,
        pacman_qg,
        foreign,
        journal,
        fallback_from,
        greeter_gone: false,
        greeter_units: Vec::new(),
    }
}

/// Служба ([`BOOT_FLAG`]): решить, показать текст и приглашение входа.
/// Код возврата 0 всегда, кроме неудачи записи текста: падение службы
/// ничего не дало бы человеку.
pub fn boot_live(out: &mut dyn FnMut(&str)) -> i32 {
    let io = crate::system_exec::LiveSystemIo;
    let stopping = io.run(&argv(&["systemctl", "is-system-running"]), Some(10), true).stdout.trim() == "stopping";
    let facts = BootFacts {
        stopping,
        already_shown: std::path::Path::new(STAMP_PATH).exists(),
        monitor_unit: std::env::var("MONITOR_UNIT").unwrap_or_default(),
        dm: dm_state_live(&io),
    };
    match decide(&facts) {
        Verdict::Skip(why) => {
            out(&format!("{RESCUE_BIN_NAME}: nothing to do: {why}"));
            return 0;
        }
        Verdict::Wait => {
            out(&format!(
                "{RESCUE_BIN_NAME}: {} is {}/{}, waiting for systemd to give up",
                facts.dm.id, facts.dm.active, facts.dm.sub
            ));
            return 0;
        }
        Verdict::Rescue => {}
    }
    if let Some(dir) = std::path::Path::new(STAMP_PATH).parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(STAMP_PATH, stamp_text(&facts.dm.id, false));
    let family = crate::system::DistroDetector::detect_live().family_live();
    let diagnosis = diagnose(&observe_live(&io, family, facts.dm.clone()));
    let vt = console_vt(&io.read_text(KERNEL_CMDLINE).unwrap_or_default());
    present(&io, &diagnosis, Some(vt), out)
}

/// Показать текст: в [`ISSUE_PATH`] и, если `vt` есть, — `getty@` на этой
/// консоли, переключение на неё и включение экрана. Код возврата — как у
/// [`boot_live`].
fn present<IO: SystemIo + ?Sized>(io: &IO, diagnosis: &Diagnosis, vt: Option<u32>, out: &mut dyn FnMut(&str)) -> i32 {
    let locale_conf = io.read_text("/etc/locale.conf");
    let lang = console_language(locale_conf.as_deref(), &|k| std::env::var(k).ok());
    let message = console_message(diagnosis, lang);
    for line in message.lines() {
        out(line);
    }
    let mut rc = 0;
    if let Some(dir) = std::path::Path::new(ISSUE_PATH).parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Err(e) = std::fs::write(ISSUE_PATH, issue_text(&message)) {
        out(&format!("{RESCUE_BIN_NAME}: cannot write {ISSUE_PATH}: {e}"));
        rc = 1;
    }
    let Some(vt) = vt else {
        out(&format!("{RESCUE_BIN_NAME}: no free console to show the login prompt on"));
        return rc;
    };
    let getty = getty_unit(vt);
    let version = parse_agetty_version(&io.run(&argv(&["agetty", "--version"]), Some(5), true).stdout);
    if !agetty_reads_run_issue(version) {
        // Старый agetty файл в /run не покажет: текст — прямо на консоль
        // (`getty@` запускается с --noclear, текст останется над приглашением).
        if let Ok(mut tty) = std::fs::OpenOptions::new().write(true).open(tty_path(vt)) {
            use std::io::Write;
            let _ = tty.write_all(issue_text(&message).replace("\\\\", "\\").replace('\n', "\r\n").as_bytes());
        }
    }
    let getty_active = io.run(&argv(&["systemctl", "is-active", &getty]), Some(10), true).stdout.trim() == "active";
    if getty_active {
        // Приглашение уже стоит — пусть agetty перерисует его с новым текстом.
        let _ = io.run(&argv(&["agetty", "--reload"]), Some(10), true);
    } else {
        let r = io.run(&argv(&["systemctl", "start", "--no-block", &getty]), Some(30), true);
        if r.rc() != Some(0) {
            out(&format!("{RESCUE_BIN_NAME}: could not start {getty}: {}", r.stderr.trim()));
        }
    }
    if io.which("chvt") {
        let _ = io.run(&argv(&["chvt", &vt.to_string()]), Some(5), true);
    }
    // Консоль могла остаться погашенной: в ВМ SteamOS 25.10.2026 после
    // загрузки tty4 активна, текст на ней есть, а экран чёрный до первого
    // нажатия клавиши (а о нажатии человек не знает). Запись на консоль
    // экран не включает — включает только «ESC [ 13 ]» (console_codes(4)).
    if let Ok(mut tty) = std::fs::OpenOptions::new().write(true).open(tty_path(vt)) {
        use std::io::Write;
        let _ = tty.write_all(UNBLANK_SEQ.as_bytes());
    }
    rc
}

/// Сторож приветствия ([`WATCH_FLAG`]): проверить, при нужде — перепроверить
/// через [`WATCH_RECHECK_SECS`] и показать текст. Код возврата — как у
/// [`boot_live`].
pub fn watch_live(out: &mut dyn FnMut(&str)) -> i32 {
    let io = crate::system_exec::LiveSystemIo;
    let stamp = || std::fs::read_to_string(STAMP_PATH).ok();
    let facts = watch_facts(&io, stamp().as_deref());
    match decide_greeter(&facts) {
        Verdict::Rescue => {}
        // Тихо: таймер зовёт часто, журнал не засоряется.
        Verdict::Skip(_) | Verdict::Wait => return 0,
    }
    out(&format!("{RESCUE_BIN_NAME}: {} has no login window, checking again in {WATCH_RECHECK_SECS} s", facts.dm.id));
    std::thread::sleep(std::time::Duration::from_secs(WATCH_RECHECK_SECS));
    let facts = watch_facts(&io, stamp().as_deref());
    match decide_greeter(&facts) {
        Verdict::Rescue => {}
        Verdict::Skip(why) => {
            out(&format!("{RESCUE_BIN_NAME}: nothing to do: {why}"));
            return 0;
        }
        Verdict::Wait => return 0,
    }
    if let Some(dir) = std::path::Path::new(STAMP_PATH).parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(STAMP_PATH, stamp_text(&facts.dm.id, true));
    if let Some(e) = &facts.evidence {
        out(&format!("{RESCUE_BIN_NAME}: {e}"));
    }
    for u in &facts.greeter_units_failed {
        out(&format!("{RESCUE_BIN_NAME}: {u} of {PLASMALOGIN_GREETER_USER} failed"));
    }
    let family = crate::system::DistroDetector::detect_live().family_live();
    let mut observed = observe_live(&io, family, facts.dm.clone());
    observed.greeter_gone = true;
    use_greeter_journal(&io, &mut observed, &facts.greeter_units_failed);
    let diagnosis = diagnose(&observed);
    let cmdline = io.read_text(KERNEL_CMDLINE).unwrap_or_default();
    let active = io.read_text(ACTIVE_VT_PATH).as_deref().and_then(parse_active_vt);
    present(&io, &diagnosis, pick_vt(&cmdline, active, &facts.dm.conflicts), out)
}

/// `rexon doctor`: тот же разбор и советы. Ничего не меняет, пароля не
/// спрашивает (журнал системы без прав может быть не виден — тогда без него).
pub fn doctor_live(out: &mut dyn FnMut(&str)) -> i32 {
    let io = crate::system_exec::LiveSystemIo;
    let family = crate::system::DistroDetector::detect_live().family_live();
    let dm = dm_state_live(&io);
    let mut observed = observe_live(&io, family, dm);
    // Сторож в эту загрузку уже сказал «окна входа нет», а экран входа жив.
    observed.greeter_gone = observed.dm.active == "active"
        && stamp_says_greeter(std::fs::read_to_string(STAMP_PATH).ok().as_deref(), &observed.dm.id);
    // Журнал — тот же, что брал сторож: упавшей службы окна входа. Без прав
    // `systemctl -M` может не ответить — тогда журнал экрана входа.
    if observed.dm.active == "active" {
        let units = greeter_units_failed_live(&io, &observed.dm);
        use_greeter_journal(&io, &mut observed, &units);
    }
    let d = diagnose(&observed);
    let cache = if d.causes.iter().any(|c| matches!(c, Cause::PackageMix(f) if !f.rollback.is_empty())) {
        update_guard::cache_listing(&io)
    } else {
        Vec::new()
    };
    let rollback: Vec<Vec<String>> = d
        .causes
        .iter()
        .filter_map(|c| if let Cause::PackageMix(f) = c { Some(f) } else { None })
        .map(|f| update_guard::rollback_paths(f, &cache))
        .collect();
    let previous = io.read_text(crate::system_plan::PREVIOUS_DM_FILE);
    let host = DoctorHost { family, steamos: crate::distro_compat::running_on_steamos() };
    // Язык — как у всего Rexon (каталог `.mo` на диске); каталога нет
    // (пакет сломан, `/usr/share/locale` без нашего `.mo`, запуск из дерева
    // без сборки переводов) — вшитый, как у текста на консоли.
    let lang = match crate::i18n::current_language() {
        code if code != crate::i18n::SOURCE_LANGUAGE => Some(code),
        _ => {
            let locale_conf = io.read_text("/etc/locale.conf");
            console_language(locale_conf.as_deref(), &|k| std::env::var(k).ok()).map(str::to_string)
        }
    };
    let text = doctor_report(&d, host, &rollback, previous.as_deref(), &|m| doctor_translate(&crate::i18n::tr, lang.as_deref(), m));
    for line in text.lines() {
        out(line);
    }
    0
}

/// Аргументы `rexon doctor`: слово маршрута первым либо [`DOCTOR_FLAG`].
pub fn is_doctor_args(args: &[String]) -> bool {
    args.first().is_some_and(|a| a == DOCTOR_WORD) || args.iter().any(|a| a == DOCTOR_FLAG)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::update_guard::Stage;

    fn failed_sddm() -> UnitState {
        parse_show(
            "Id=sddm.service\nLoadState=loaded\nActiveState=failed\nSubState=failed\nResult=start-limit-hit\n\
             ExecStart={ path=/usr/bin/sddm ; argv[]=/usr/bin/sddm ; ignore_errors=no ; start_time=[n/a] ; stop_time=[n/a] ; pid=0 ; code=(null) ; status=0/0 }\n",
        )
    }

    const ARCH: DoctorHost = DoctorHost { family: PkgFamily::Arch, steamos: false };

    fn facts(dm: UnitState) -> BootFacts {
        BootFacts { dm, ..Default::default() }
    }

    #[test]
    fn show_output_parsed() {
        let s = failed_sddm();
        assert_eq!(s.id, "sddm.service");
        assert_eq!(s.short(), "sddm");
        assert_eq!(s.label(), "SDDM");
        assert_eq!(s.result, "start-limit-hit");
        assert_eq!(s.exec_path.as_deref(), Some("/usr/bin/sddm"));
        assert!(s.gave_up());
        assert_eq!(exec_path(""), None);
        assert_eq!(exec_path("{ path=relative ; }"), None);
        let unknown = parse_show("Id=foo.service\n");
        assert_eq!(unknown.label(), "foo");
    }

    #[test]
    fn waits_while_systemd_restarts() {
        for (active, sub) in [
            ("failed", "auto-restart"),
            ("activating", "auto-restart"),
            ("failed", "auto-restart-queued"),
            ("activating", "start"),
            ("active", "running"),
            // Откат вернул прежний экран входа и поставил его запуск.
            ("inactive", "dead"),
        ] {
            let mut dm = failed_sddm();
            dm.active = active.into();
            dm.sub = sub.into();
            assert_eq!(decide(&facts(dm)), Verdict::Wait, "{active}/{sub}");
        }
    }

    #[test]
    fn rescues_when_systemd_gave_up() {
        assert_eq!(decide(&facts(failed_sddm())), Verdict::Rescue);
        let mut f = facts(failed_sddm());
        f.monitor_unit = "sddm.service".into();
        assert_eq!(decide(&f), Verdict::Rescue);
        f.monitor_unit = DM_ALIAS.into();
        assert_eq!(decide(&f), Verdict::Rescue);
    }

    #[test]
    fn skips_foreign_and_repeated_cases() {
        let mut f = facts(failed_sddm());
        f.already_shown = true;
        assert!(matches!(decide(&f), Verdict::Skip(_)));
        let mut f = facts(failed_sddm());
        f.stopping = true;
        assert!(matches!(decide(&f), Verdict::Skip(_)));
        // Упал посторонний экран входа (человек пробовал gdm руками).
        let mut f = facts(failed_sddm());
        f.monitor_unit = "gdm.service".into();
        assert!(matches!(decide(&f), Verdict::Skip(_)));
        // Псевдонима нет вовсе.
        let none = parse_show("Id=display-manager.service\nLoadState=not-found\nActiveState=inactive\n");
        assert!(matches!(decide(&facts(none)), Verdict::Skip(_)));
    }

    #[test]
    fn units_text() {
        let unit = rescue_unit(RESCUE_BIN_PACKAGE);
        assert!(unit.contains("ExecStart=/usr/lib/rexon/rexon-login-rescue --boot\n"));
        assert!(unit.contains("Type=oneshot\n"));
        assert!(unit.contains("StartLimitIntervalSec=0\n"));
        assert!(unit.contains("DefaultDependencies=no\n"));
        // Ждёт службу-откат, если её тоже позвали.
        assert!(unit.contains(&format!("After=sysinit.target {}\n", crate::system_plan::DM_FALLBACK_SERVICE)));
        // Ничего не чинит: ни перезапусков, ни Restart=.
        assert!(!unit.contains("Restart="));
        assert_eq!(dropin(), "[Unit]\nOnFailure=rexon-login-rescue.service\nWants=rexon-greeter-watch.timer\n");
    }

    #[test]
    fn dropins_do_not_clash_with_fallback() {
        assert_ne!(DROPIN_NAME, crate::system_plan::DM_FALLBACK_DROPIN);
        assert_ne!(RESCUE_SERVICE, crate::system_plan::DM_FALLBACK_SERVICE);
        let units = dropin_units();
        assert_eq!(units[0], DM_ALIAS);
        for u in ["sddm.service", "plasmalogin.service", "gdm.service", "lightdm.service", "greetd.service"] {
            assert!(units.contains(&u.to_string()), "{u}");
        }
        let mut sorted = units.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), units.len());
        let files = unit_files("/usr/lib/systemd/system/", RESCUE_BIN_PACKAGE);
        assert_eq!(files[0].0, "/usr/lib/systemd/system/rexon-login-rescue.service");
        assert!(files.iter().any(|(p, _)| p == "/usr/lib/systemd/system/display-manager.service.d/50-rexon-login-rescue.conf"));
        // Файлы — только в каталоге пакета: в /etc (там drop-in отката) — ничего.
        assert!(files.iter().all(|(p, _)| p.starts_with("/usr/lib/systemd/system/")));
    }

    #[test]
    fn fallback_detected_only_this_boot() {
        let conf = "old=sddm\nold_real=/usr/lib/systemd/system/sddm.service\nnew=plasmalogin\n";
        assert_eq!(fallback_from(Some(conf), Some(200), Some(100), "sddm").as_deref(), Some("plasmalogin"));
        // Откат был в прошлую загрузку.
        assert_eq!(fallback_from(Some(conf), Some(50), Some(100), "sddm"), None);
        // Упал новый, а не тот, к которому откатились.
        assert_eq!(fallback_from(Some(conf), Some(200), Some(100), "plasmalogin"), None);
        assert_eq!(fallback_from(None, Some(200), Some(100), "sddm"), None);
        assert_eq!(fallback_from(Some(conf), None, Some(100), "sddm"), None);
        assert_eq!(parse_btime("cpu 1 2\nbtime 1761380000\nprocesses 5\n"), Some(1761380000));
    }

    /// Случай 08.10.2026: qt6-base 6.12.0 при qt6-declarative 6.11.2.
    fn observed_qt_mix() -> Observed {
        Observed {
            dm: failed_sddm(),
            package: Some("sddm".into()),
            package_files: vec!["/usr/bin/sddm".into(), "/usr/bin/sddm-greeter-qt6".into()],
            ldd: vec![
                ("/usr/bin/sddm".into(), "\tlinux-vdso.so.1 (0x7ffd)\n\tlibQt6Core.so.6 => /usr/lib/libQt6Core.so.6\n".into()),
                (
                    "/usr/bin/sddm-greeter-qt6".into(),
                    "\tlibQt6Qml.so.6 => /usr/lib/libQt6Qml.so.6\n\
                     undefined symbol: _ZN7QObject5eventEP6QEvent, version Qt_6_PRIVATE_API\t(/usr/lib/libQt6Qml.so.6)\n\
                     undefined symbol: _ZN7QObject5eventEP6QEvent, version Qt_6_PRIVATE_API\t(/usr/lib/libQt6Quick.so.6)\n\
                     undefined symbol: a\nundefined symbol: b\n\
                     \tlibfoo.so.3 => not found\n"
                        .into(),
                ),
            ],
            entries: vec!["/usr/bin/sddm".into(), "/usr/bin/sddm-greeter-qt6".into()],
            optional_deps: None,
            pacman_q: Some("qt6-base 6.12.0-1\nqt6-declarative 6.11.2-1\nqt6-svg 6.12.0-1\nsddm 0.21.0-6\n".into()),
            pacman_qg: Some(String::new()),
            foreign: None,
            journal: "\nsddm-greeter-qt6: symbol lookup error: undefined symbol QtPrivate_6_11_2\n\x1b[31mred\x1b[0m\n".into(),
            fallback_from: None,
            greeter_gone: false,
            greeter_units: Vec::new(),
        }
    }

    #[test]
    fn causes_found_offline() {
        let d = diagnose(&observed_qt_mix());
        let Cause::PackageMix(f) = &d.causes[0] else { panic!("{:?}", d.causes) };
        assert_eq!(f.set.id, "qt6");
        assert_eq!(f.stage, Stage::DontReboot);
        let libs: Vec<&Cause> = d.causes.iter().filter(|c| matches!(c, Cause::Library { .. })).collect();
        // Не больше двух на программу, без повторов.
        assert_eq!(libs.len(), 2, "{libs:?}");
        assert_eq!(
            libs[0],
            &Cause::Library { program: "/usr/bin/sddm-greeter-qt6".into(), problem: "libfoo.so.3: not found".into() }
        );
        assert_eq!(d.log.len(), 2);
        assert_eq!(d.log[1], "[31mred[0m");
    }

    #[test]
    fn console_text_reads_well() {
        let d = diagnose(&observed_qt_mix());
        let text = console_message(&d, None);
        assert!(text.starts_with("Rexon: the login screen SDDM did not start"), "{text}");
        assert!(text.contains("  - parts of Qt of different versions are installed (qt6-base 6.12.0 with qt6-declarative 6.11.2)\n"), "{text}");
        assert!(text.contains("  - /usr/bin/sddm-greeter-qt6: libfoo.so.3: not found\n"), "{text}");
        // Журнал отделён от причин пустой строкой.
        assert!(text.contains("_ZN7QObject5eventEP6QEvent\n\nLast lines of its log:\n"), "{text}");
        assert!(text.contains("and run: rexon doctor\n"));
        assert!(text.contains("Rexon has not changed anything"));
        assert!(!text.chars().any(|c| c.is_control() && c != '\n'));
        // Без причин — честно «не нашли».
        let mut o = observed_qt_mix();
        o.ldd.clear();
        o.pacman_q = None;
        o.fallback_from = Some("plasmalogin".into());
        let text = console_message(&diagnose(&o), None);
        assert!(text.contains("The cause could not be found without the network.\n\nLast lines of its log:\n"), "{text}");
        assert!(text.contains("after Plasma Login Manager failed, but it did not start either."), "{text}");
    }

    /// SteamOS: `fbcon=vc:4-6` — tty1 не рисуется, приглашение — на tty4.
    #[test]
    fn console_is_the_one_kernel_draws() {
        let steamos = "BOOT_IMAGE=/boot/vmlinuz-linux-neptune-72 console=tty1 quiet splash fbcon=vc:4-6 steamos.efi=x";
        assert_eq!(console_vt(steamos), 4);
        assert_eq!(getty_unit(console_vt(steamos)), "getty@tty4.service");
        assert_eq!(tty_path(4), "/dev/tty4");
        assert_eq!(console_vt("quiet splash"), 1);
        assert_eq!(console_vt(""), 1);
        assert_eq!(console_vt("fbcon=vc:1-6"), 1);
        assert_eq!(console_vt("fbcon=font:TER16x32,vc:2"), 2);
        // Действует последний параметр; мусор — первая консоль.
        assert_eq!(console_vt("fbcon=vc:4-6 fbcon=vc:3-6"), 3);
        assert_eq!(console_vt("fbcon=vc:x-6"), 1);
        assert_eq!(console_vt("fbcon=vc:99"), 1);
    }

    #[test]
    fn issue_escapes_backslash() {
        let t = issue_text("a \\n b\n");
        assert!(t.contains("a \\\\n b\n"));
        assert!(t.starts_with('\n') && t.ends_with("\n\n"));
    }

    #[test]
    fn agetty_version() {
        assert_eq!(parse_agetty_version("agetty from util-linux 2.42.4 (flow control, issue)"), Some((2, 42)));
        assert_eq!(parse_agetty_version("agetty from util-linux 2.38.1\n"), Some((2, 38)));
        assert_eq!(parse_agetty_version("agetty from util-linux 2.41-rc1"), Some((2, 41)));
        assert_eq!(parse_agetty_version(""), None);
        assert!(agetty_reads_run_issue(Some((2, 41))));
        assert!(!agetty_reads_run_issue(Some((2, 39))));
        assert!(!agetty_reads_run_issue(None));
    }

    #[test]
    fn console_language_from_system_locale() {
        let none = |_: &str| None;
        let langs = crate::system_l10n::languages();
        if langs.contains(&"ru") {
            assert_eq!(console_language(Some("LANG=ru_RU.UTF-8\n"), &none), Some("ru"));
            assert_eq!(console_language(Some("LANG=\"ru_RU.UTF-8\"\n"), &none), Some("ru"));
        }
        assert_eq!(console_language(Some("LANG=en_US.UTF-8\n"), &none), None);
        assert_eq!(console_language(None, &none), None);
        assert_eq!(console_language(Some("LANG=xx_YY.UTF-8\n"), &none), None);
        // Окружение службы (systemd отдаёт locale.conf в LANG) — раньше файла.
        let env = |k: &str| (k == "LANG").then(|| "C.UTF-8".to_string());
        assert_eq!(console_language(Some("LANG=ru_RU.UTF-8\n"), &env), None);
    }

    #[test]
    fn translation_keeps_english_below() {
        let d = diagnose(&observed_qt_mix());
        let en = console_message(&d, None);
        // Языка без каталога — ровно английский, без повтора.
        assert_eq!(console_message(&d, Some("xx")), en);
        assert_eq!(console_message(&d, Some("en")), en);
    }

    /// Сирота вне репозиториев (kwin-x11 6.7.5 при Plasma 6.8.1): на консоли
    /// и в `rexon doctor` — «ушёл из репозиториев», совет — удалить, а не
    /// «обновитесь».
    #[test]
    fn orphan_package_told_as_removed_from_repositories() {
        let mut o = observed_qt_mix();
        o.ldd.clear();
        o.pacman_q = Some("plasma-workspace 6.8.1-1.1\nkwin 6.8.1-2.1\nkwin-x11 6.7.5-3.1\n".into());
        o.pacman_qg = Some("plasma plasma-workspace\nplasma kwin\nplasma kwin-x11\n".into());
        o.foreign = Some(update_guard::parse_foreign("kwin-x11\n"));
        let d = diagnose(&o);
        let Cause::PackageMix(f) = &d.causes[0] else { panic!("{:?}", d.causes) };
        assert_eq!(f.stage, Stage::Orphaned);
        let en = |m: &str| m.to_string();
        let console = console_text(&d, &en);
        assert!(console.contains("kwin-x11 has been removed from the repositories"), "{console}");
        assert!(!console.contains("different versions"), "{console}");
        let doctor = doctor_report(&d, ARCH, &[Vec::new()], None, &en);
        assert!(doctor.contains("remove the package: sudo pacman -Rns kwin-x11"), "{doctor}");
        assert!(!doctor.contains("sudo pacman -Syu"), "{doctor}");
        // Без сведений о сиротах — прежнее «вразнобой».
        o.foreign = None;
        let d = diagnose(&o);
        assert!(console_text(&d, &en).contains("parts of Plasma of different versions"));
    }

    #[test]
    fn texts_are_sane() {
        for t in TEXTS {
            assert!(crate::system_l10n::translation_ok(t, t), "{t}");
        }
        let mut v = TEXTS.to_vec();
        v.sort_unstable();
        v.dedup();
        assert_eq!(v.len(), TEXTS.len());
    }

    #[test]
    fn doctor_advises_without_acting() {
        let d = diagnose(&observed_qt_mix());
        let rollback = vec![vec!["/var/cache/pacman/pkg/qt6-base-6.11.2-1-x86_64.pkg.tar.zst".to_string()]];
        let prev = "Rexon switched …\n    sudo systemctl enable --force sddm.service && sudo reboot\n";
        let text = doctor_report(&d, ARCH, &rollback, Some(prev), &|m| m.to_string());
        assert!(text.starts_with("The login screen SDDM did not start (failed/failed).\n"), "{text}");
        assert!(text.contains("sudo pacman -Syu"));
        assert!(text.contains("sudo pacman -U /var/cache/pacman/pkg/qt6-base-6.11.2-1-x86_64.pkg.tar.zst"));
        // Пакеты вразнобой — переустановка экрана входа не лечит, не советуем.
        assert!(!text.contains("Reinstall"));
        assert!(text.contains("journalctl -b -u sddm.service"));
        assert!(text.contains("How to go back to the previous login screen:"));
        assert!(text.contains("sudo reboot"));
        assert!(text.trim_end().ends_with("This command only looks: it has not changed anything."));

        let mut o = observed_qt_mix();
        o.pacman_q = None;
        let text = doctor_report(&diagnose(&o), ARCH, &[], None, &|m| m.to_string());
        assert!(text.contains("Reinstall the login screen package: sudo pacman -S sddm"), "{text}");

        let mut ok = observed_qt_mix();
        ok.dm.active = "active".into();
        ok.dm.sub = "running".into();
        ok.ldd.clear();
        ok.pacman_q = None;
        let text = doctor_report(&diagnose(&ok), ARCH, &[], Some(prev), &|m| m.to_string());
        assert!(text.starts_with("The login screen SDDM is not broken (active/running)."), "{text}");
        assert!(!text.contains("What you can do:"), "{text}");
        // Экран входа жив, а набор разошёлся — «заметил», а не «причина».
        let mut mix = observed_qt_mix();
        mix.dm.active = "active".into();
        let text = doctor_report(&diagnose(&mix), ARCH, &[], None, &|m| m.to_string());
        assert!(text.contains("Rexon also noticed:\n") && !text.contains("Likely cause"), "{text}");
    }

    #[test]
    fn package_lookup() {
        assert_eq!(dm_package("plasmalogin", PkgFamily::Debian).as_deref(), Some("plasma-login-manager"));
        assert_eq!(dm_package("sddm", PkgFamily::Arch).as_deref(), Some("sddm"));
        assert_eq!(dm_package("unknown-dm", PkgFamily::Arch), None);
        assert_eq!(package_files_argv(PkgFamily::Arch, "sddm"), Some(argv(&["pacman", "-Qlq", "sddm"])));
        assert_eq!(package_files_argv(PkgFamily::Nix, "sddm"), None);
        let c = ldd_candidates(
            &["/usr/bin/sddm".into()],
            &["/usr/".into(), "/usr/lib/qt6/qml/SddmComponents/libplugin.so".into(), "/usr/bin/sddm".into(),
              "/usr/bin/sddm-greeter-qt6".into(), "/usr/share/sddm/x.conf".into(), "/usr/bin/sddm-greeter".into()],
            &["/usr/bin/sddm-greeter".into()],
        );
        assert_eq!(c, vec!["/usr/bin/sddm", "/usr/bin/sddm-greeter-qt6", "/usr/lib/qt6/qml/SddmComponents/libplugin.so"]);
    }

    /// Пакет ставит программу по пути, на который смотрит юнит, и зовёт её
    /// для `rexon doctor` в обход Qt.
    #[test]
    fn package_installs_rescue() {
        let pkgbuild = include_str!("../../../packaging/aur/PKGBUILD");
        assert!(pkgbuild.contains(&format!("\"$pkgdir{RESCUE_BIN_PACKAGE}\"")));
        assert!(pkgbuild.contains(&format!("exec {RESCUE_BIN_PACKAGE} {DOCTOR_WORD}")));
        assert!(pkgbuild.contains("rexon-login-rescue\" --list-files"));
        assert!(matches!(crate::app::route(&["doctor".to_string()]), crate::app::Route::Doctor));
    }

    #[test]
    fn doctor_args() {
        assert!(is_doctor_args(&["doctor".to_string()]));
        assert!(is_doctor_args(&["--doctor".to_string()]));
        assert!(!is_doctor_args(&["--qt".to_string(), "doctor".to_string()]));
        assert!(!is_doctor_args(&[]));
    }

    fn sddm_files() -> Vec<String> {
        ["/etc/pam.d/sddm-greeter", "/usr/bin/sddm", "/usr/bin/sddm-greeter", "/usr/bin/sddm-greeter-qt6", "/usr/lib/sddm/sddm-helper"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    /// Приветствие выбирается так же, как его выбирает SDDM.
    #[test]
    fn sddm_greeter_follows_settings() {
        let files = sddm_files();
        let all = |_: &str| true;
        let themes = |p: &str| p == "/usr/share/sddm/themes/breeze" || p == "/usr/share/sddm/themes/maya" || p == "/opt/t/x";
        let meta = |p: &str| match p {
            "/usr/share/sddm/themes/breeze/metadata.desktop" => Some("[SddmGreeterTheme]\nName=Breeze\nQtVersion=6\n".to_string()),
            "/usr/share/sddm/themes/maya/metadata.desktop" => Some("[SddmGreeterTheme]\nName=Maya\n".to_string()),
            "/opt/t/x/metadata.desktop" => Some("[SddmGreeterTheme]\nQtVersion=6\n".to_string()),
            _ => None,
        };
        let conf = |t: &str| vec![format!("[General]\nNumlock=on\n\n[Theme]\n{t}\n")];
        let g = |confs: Vec<String>, exec: &dyn Fn(&str) -> bool| sddm_greeter(&confs, &files, &themes, exec, &meta);
        // Тема на Qt6.
        assert_eq!(g(conf("Current=breeze"), &all), "/usr/bin/sddm-greeter-qt6");
        // Тема без QtVersion — Qt5 (так считает SDDM).
        assert_eq!(g(conf("Current=maya"), &all), "/usr/bin/sddm-greeter");
        // …но Qt5-приветствия нет — приветствие версии самого SDDM.
        assert_eq!(g(conf("Current=maya"), &|p: &str| p != "/usr/bin/sddm-greeter"), "/usr/bin/sddm-greeter-qt6");
        // Темы нет вовсе или каталога темы нет — встроенная тема SDDM.
        assert_eq!(g(Vec::new(), &all), "/usr/bin/sddm-greeter-qt6");
        assert_eq!(g(conf("Current=gone"), &all), "/usr/bin/sddm-greeter-qt6");
        // Поздний файл перекрывает ранний, и пустое значение тоже.
        let mut confs = conf("Current=maya");
        confs.push("[Theme]\nCurrent=\n".into());
        assert_eq!(g(confs, &all), "/usr/bin/sddm-greeter-qt6");
        // Свой каталог тем.
        assert_eq!(g(conf("ThemeDir=/opt/t\nCurrent=x"), &all), "/usr/bin/sddm-greeter-qt6");
        // Ключ из другого раздела не в счёт.
        assert_eq!(g(vec!["[Users]\nCurrent=maya\n".into()], &all), "/usr/bin/sddm-greeter-qt6");
        // SDDM на Qt5 (приветствия Qt6 в пакете нет).
        let qt5: Vec<String> = vec!["/usr/bin/sddm".into(), "/usr/bin/sddm-greeter".into()];
        assert_eq!(sddm_greeter(&[], &qt5, &themes, &all, &meta), "/usr/bin/sddm-greeter");

        let dm = failed_sddm();
        let (entries, skip) = entry_programs(&dm, Some("/usr/bin/sddm-greeter-qt6"), &files);
        assert_eq!(entries, vec!["/usr/bin/sddm", "/usr/bin/sddm-greeter-qt6"]);
        assert_eq!(skip, vec!["/usr/bin/sddm-greeter"]);
        let (entries, skip) = entry_programs(&dm, None, &files);
        assert_eq!(entries, vec!["/usr/bin/sddm"]);
        assert!(skip.is_empty());
    }

    /// Замер в ВМ SteamOS 25.10.2026: `ExecStart=/bin/false`, а «причиной»
    /// названо Qt5-приветствие без qt5-declarative (необязательная
    /// зависимость sddm). То же — в `rexon doctor` на исправной системе.
    #[test]
    fn optional_qt5_greeter_is_not_a_cause() {
        let qt5_missing = "\tlibQt5Quick.so.5 => not found\n\tlibQt5Qml.so.5 => not found\n";
        let mut o = observed_qt_mix();
        o.pacman_q = None;
        o.dm.exec_path = Some("/bin/false".into());
        o.entries = vec!["/bin/false".into(), "/usr/bin/sddm-greeter-qt6".into()];
        o.ldd = vec![
            ("/bin/false".into(), "\tlibc.so.6 => /usr/lib/libc.so.6\n".into()),
            ("/usr/bin/sddm-greeter-qt6".into(), "\tlibQt6Quick.so.6 => /usr/lib/libQt6Quick.so.6\n".into()),
            ("/usr/lib/sddm/sddm-helper".into(), qt5_missing.into()),
        ];
        o.optional_deps = Some(vec![("qt5-declarative".into(), false)]);
        assert!(diagnose(&o).causes.is_empty());
        // Не узнали необязательные зависимости — тоже не верим.
        o.optional_deps = None;
        assert!(diagnose(&o).causes.is_empty());
        // Все необязательные стоят, а библиотеки нет — это беда.
        o.optional_deps = Some(vec![("qt5-declarative".into(), true)]);
        assert_eq!(diagnose(&o).causes.len(), 2);
        // Несовпадение символов у вспомогательной программы — всегда беда:
        // библиотека стоит, но не та.
        o.optional_deps = Some(vec![("qt5-declarative".into(), false)]);
        o.ldd[2].1 = "undefined symbol: _ZTI20QDBusAbstractAdaptor\t(/usr/lib/sddm/sddm-helper)\n".into();
        assert_eq!(diagnose(&o).causes.len(), 1);
        // То, что экран входа запускает, проверяется без скидок.
        o.entries.push("/usr/lib/sddm/sddm-helper".into());
        o.ldd[2].1 = qt5_missing.into();
        assert_eq!(diagnose(&o).causes.len(), 2);

        // `rexon doctor` на исправной системе — без «also noticed».
        let mut ok = observed_qt_mix();
        ok.pacman_q = None;
        ok.dm.active = "active".into();
        ok.dm.sub = "running".into();
        ok.ldd.push(("/usr/bin/sddm-greeter".into(), qt5_missing.into()));
        ok.ldd.retain(|(p, _)| p != "/usr/bin/sddm-greeter-qt6");
        ok.optional_deps = Some(vec![("qt5-declarative".into(), false)]);
        let text = doctor_report(&diagnose(&ok), ARCH, &[], None, &|m| m.to_string());
        assert!(!text.contains("noticed") && !text.contains("Qt5"), "{text}");
    }

    #[test]
    fn optional_deps_parsed() {
        let qi = "Name            : sddm\nVersion         : 0.21.0-6\n\
                  Optional Deps   : qt5-declarative: for using Qt5 themes\n\
                  \x20                 foo>=2: something [installed]\n\
                  Required By     : sddm-kcm\n";
        assert_eq!(
            parse_optional_deps(qi),
            Some(vec![("qt5-declarative".to_string(), false), ("foo".to_string(), true)])
        );
        assert_eq!(parse_optional_deps("Name : x\nOptional Deps   : None\nRequired By : y\n"), Some(Vec::new()));
        assert_eq!(parse_optional_deps("error: package 'sddm' was not found\n"), None);
    }

    #[test]
    fn aux_commands_from_show() {
        let s = parse_show(
            "Id=sddm.service\n\
             ExecStart={ path=/usr/bin/sddm ; argv[]=/usr/bin/sddm ; ignore_errors=no ; start_time=[n/a] ; stop_time=[n/a] ; pid=0 ; code=(null) ; status=0/0 }\n\
             ExecStartPre={ path=/bin/sh ; argv[]=/bin/sh -c \"rm /etc/sddm.conf.d/zzt-*-temp-login.conf\" ; ignore_errors=yes ; start_time=[n/a] ; stop_time=[n/a] ; pid=0 ; code=(null) ; status=0/0 }\n\
             ExecStartPre={ path=/usr/bin/sddm ; argv[]=/usr/bin/sddm --test ; ignore_errors=no ; start_time=[n/a] ; stop_time=[n/a] ; pid=0 ; code=(null) ; status=0/0 }\n\
             ExecStartPost={ path=/usr/lib/steamos/x-post ; argv[]=x-post ; ignore_errors=no ; start_time=[n/a] ; stop_time=[n/a] ; pid=0 ; code=(null) ; status=0/0 }\n",
        );
        assert_eq!(s.aux_commands, vec!["sh", "rm", "x-post"]);
        assert!(failed_sddm().aux_commands.is_empty());
        assert_eq!(aux_commands(""), Vec::<String>::new());
    }

    fn jline(pid: &str, ident: &str, comm: &str, msg: &str) -> String {
        serde_json::json!({ "_PID": pid, "SYSLOG_IDENTIFIER": ident, "_COMM": comm, "MESSAGE": msg, "_SYSTEMD_UNIT": "sddm.service" })
            .to_string()
    }

    /// Хвост журнала — голос экрана входа, а не systemd и не `ExecStartPre`
    /// (снимок ВМ SteamOS r8-boot-fixed.png: пять строк systemd поверх
    /// настоящей ошибки).
    #[test]
    fn log_shows_the_process_not_systemd() {
        let aux = vec!["sh".to_string(), "rm".to_string()];
        let journal = [
            jline("1", "systemd", "systemd", "Starting Simple Desktop Display Manager..."),
            jline("701", "sh", "rm", "rm: cannot remove '/etc/sddm.conf.d/zzt-*-temp-login.conf': No such file or directory"),
            jline("702", "sddm", "sddm", "/usr/bin/sddm: symbol lookup error: /usr/bin/sddm: undefined symbol: _ZTI20QDBusAbstractAdaptor"),
            jline("1", "systemd", "systemd", "sddm.service: Main process exited, code=exited, status=127/n/a"),
            jline("1", "systemd", "systemd", "sddm.service: Scheduled restart job, restart counter is at 2."),
            jline("1", "systemd", "systemd", "sddm.service: Start request repeated too quickly."),
            jline("1", "systemd", "systemd", "sddm.service: Failed with result 'start-limit-hit'."),
            jline("1", "systemd", "systemd", "Failed to start Simple Desktop Display Manager."),
            jline("1", "systemd", "systemd", "sddm.service: Triggering OnFailure= dependencies."),
        ]
        .join("\n");
        assert_eq!(
            log_lines(&journal, &aux),
            vec!["/usr/bin/sddm: symbol lookup error: /usr/bin/sddm: undefined symbol: _ZTI20QDBusAbstractAdaptor"]
        );
        // Без `ExecStartPre` в списке строка rm осталась бы — проверка, что
        // отсев идёт именно по нему.
        assert_eq!(log_lines(&journal, &[]).len(), 2);
        // Процесс промолчал (упал сигналом) — из systemd только «как кончился».
        let silent = [
            jline("1", "systemd", "systemd", "sddm.service: Main process exited, code=dumped, status=11/SEGV"),
            jline("1", "systemd", "systemd", "sddm.service: Failed with result 'core-dump'."),
        ]
        .join("\n");
        assert_eq!(log_lines(&silent, &aux), vec!["sddm.service: Main process exited, code=dumped, status=11/SEGV"]);
        // MESSAGE не в UTF-8 — массивом байтов.
        let bytes = r#"{"_PID":"9","SYSLOG_IDENTIFIER":"sddm-greeter-qt6","MESSAGE":[104,105,255]}"#;
        assert_eq!(log_lines(bytes, &aux), vec!["hi\u{fffd}"]);
        assert!(log_lines("", &aux).is_empty());
    }

    /// Упала служба окна входа Plasma Login Manager (снимок ВМ SteamOS
    /// g5-plasmalogin-rescue.png): в хвосте был только шум EGL/ZINK, а
    /// строка о падении systemd пользователя отсеивалась.
    #[test]
    fn greeter_unit_log_shows_the_crash() {
        let user = |msg: &str| {
            serde_json::json!({ "_PID": "812", "SYSLOG_IDENTIFIER": "systemd", "_COMM": "systemd", "MESSAGE": msg, "USER_UNIT": "plasma-login.service" })
                .to_string()
        };
        let own = |msg: &str| jline("901", "plasmalogin-greeter", "plasmalogin-gre", msg);
        let noise = [
            own("libEGL warning: failed to get driver name for fd -1"),
            own("MESA: error: ZINK: vkEnumeratePhysicalDevices failed (VK_ERROR_INITIALIZATION_FAILED)"),
            own("MESA: error: ZINK: failed to choose pdev"),
            own("libEGL warning: egl: failed to create dri2 screen"),
        ];
        let tail = [
            user("Started Plasma Login greeter."),
            user("plasma-login.service: Main process exited, code=dumped, status=11/SEGV"),
            user("plasma-login.service: Failed with result 'core-dump'."),
        ];
        let mut journal: Vec<String> = noise.to_vec();
        journal.push(own("QDBusConnection: name 'org.freedesktop.UPower' had owner '' but we thought it was ':1.31'"));
        journal.extend(tail.iter().cloned());
        assert_eq!(
            greeter_log_lines(&journal.join("\n")),
            vec![
                "QDBusConnection: name 'org.freedesktop.UPower' had owner '' but we thought it was ':1.31'",
                "plasma-login.service: Main process exited, code=dumped, status=11/SEGV",
                "plasma-login.service: Failed with result 'core-dump'.",
            ]
        );
        // Сказал только шум — лучше он, чем ничего: не больше трёх строк
        // процесса, строки о падении — всегда.
        let only_noise: Vec<String> = noise.iter().chain(tail.iter()).cloned().collect();
        let lines = greeter_log_lines(&only_noise.join("\n"));
        assert_eq!(lines.len(), 5, "{lines:?}");
        assert_eq!(lines[0], "MESA: error: ZINK: vkEnumeratePhysicalDevices failed (VK_ERROR_INITIALIZATION_FAILED)");
        assert!(lines[4].contains("core-dump"));
        // Сообщение загрузчика — не шум, даже с «EGL» в имени библиотеки.
        let loader = [noise[0].clone(), own("plasmalogin-greeter: error while loading shared libraries: libEGL.so.1: cannot open shared object file")];
        assert_eq!(greeter_log_lines(&loader.join("\n")).len(), 1);
        assert!(greeter_log_lines("").is_empty());
        // Разбор выбирается по упавшим службам.
        let mut o = Observed { journal: journal.join("\n"), ..Default::default() };
        assert_eq!(diagnose(&o).log.len(), 5);
        o.greeter_units = vec!["plasma-login.service".into()];
        assert_eq!(diagnose(&o).log.len(), 3);
    }

    #[test]
    fn greeter_journal_has_systemd_words_too() {
        let a = greeter_journal_argv(&["plasma-login.service".to_string()]);
        let tail: Vec<&str> = a.iter().skip_while(|x| !x.starts_with("_SYSTEMD_USER_UNIT=")).map(String::as_str).collect();
        assert_eq!(tail, ["_SYSTEMD_USER_UNIT=plasma-login.service", "+", "USER_UNIT=plasma-login.service"]);
        assert!(a.iter().any(|x| x == "-b") && !a.iter().any(|x| x.starts_with("--user-unit")));
    }

    /// systemd не передаёт `MONITOR_UNIT`, когда служба стоит в
    /// `OnFailure=` у нескольких юнитов: решение — по текущему экрану входа.
    #[test]
    fn decides_without_monitor_unit() {
        let f = facts(failed_sddm());
        assert!(f.monitor_unit.is_empty());
        assert_eq!(decide(&f), Verdict::Rescue);
        // Упал посторонний (gdm руками), а текущий sddm жив — не показываем.
        let mut alive = failed_sddm();
        alive.active = "active".into();
        alive.sub = "running".into();
        assert_eq!(decide(&facts(alive)), Verdict::Wait);
        // Текущий ещё перезапускается.
        let mut restarting = failed_sddm();
        restarting.sub = "auto-restart".into();
        assert_eq!(decide(&facts(restarting)), Verdict::Wait);
    }

    #[test]
    fn steamos_advice_is_not_pacman() {
        let steamos = DoctorHost { family: PkgFamily::Arch, steamos: true };
        let en = |m: &str| m.to_string();
        let mut o = observed_qt_mix();
        o.pacman_q = None;
        let text = doctor_report(&diagnose(&o), steamos, &[], None, &en);
        assert!(text.contains("update SteamOS: sudo steamos-update"), "{text}");
        assert!(text.contains("Steam Deck Recovery Instructions"), "{text}");
        assert!(!text.contains("pacman"), "{text}");
        // Пакеты вразнобой и сирота — тоже без pacman.
        let d = diagnose(&observed_qt_mix());
        let text = doctor_report(&d, steamos, &[vec!["/var/cache/pacman/pkg/x.pkg.tar.zst".into()]], None, &en);
        assert!(!text.contains("sudo pacman"), "{text}");
        // Исправный экран входа без находок — советов нет.
        let mut ok = observed_qt_mix();
        ok.pacman_q = None;
        ok.ldd.clear();
        ok.dm.active = "active".into();
        assert!(!doctor_report(&diagnose(&ok), steamos, &[], None, &en).contains("steamos-update"));
    }

    /// Откат уже вернул прежний экран входа, и тот тоже упал: «вернитесь к
    /// прежнему» не советуется.
    #[test]
    fn no_way_back_when_previous_failed_too() {
        let prev = "Rexon switched …\n    sudo systemctl enable --force sddm.service && sudo reboot\n";
        let mut o = observed_qt_mix();
        o.fallback_from = Some("plasmalogin".into());
        let text = doctor_report(&diagnose(&o), ARCH, &[], Some(prev), &|m| m.to_string());
        assert!(!text.contains("How to go back") && !text.contains("enable --force"), "{text}");
        assert!(
            text.contains("Going back to the previous login screen will not help: Plasma Login Manager and SDDM both fail to start."),
            "{text}"
        );
        // Без отката — прежний совет на месте.
        let text = doctor_report(&diagnose(&observed_qt_mix()), ARCH, &[], Some(prev), &|m| m.to_string());
        assert!(text.contains("How to go back to the previous login screen:"), "{text}");
    }

    /// `rexon doctor` без `.mo` на диске говорит вшитым каталогом.
    #[test]
    fn doctor_translates_without_mo() {
        let none = |m: &str| m.to_string();
        assert_eq!(doctor_translate(&none, None, T_DOCTOR_FOOTER), T_DOCTOR_FOOTER);
        assert_eq!(doctor_translate(&none, Some("en"), T_DOCTOR_FOOTER), T_DOCTOR_FOOTER);
        assert_eq!(doctor_translate(&none, Some("xx"), T_DOCTOR_FOOTER), T_DOCTOR_FOOTER);
        if crate::system_l10n::languages().contains(&"ru") {
            let ru = doctor_translate(&none, Some("ru"), T_DOCTOR_FOOTER);
            assert_ne!(ru, T_DOCTOR_FOOTER);
            assert_eq!(Some(ru), crate::system_l10n::translated("ru", T_DOCTOR_FOOTER));
        }
        // Перевод с диска — первым.
        let disk = |m: &str| format!("[{m}]");
        assert_eq!(doctor_translate(&disk, Some("ru"), "x"), "[x]");
    }

    // ── Сторож приветствия ───────────────────────────────────────────────

    /// Подставной исполнитель: argv через пробел, начинающийся с ключа, →
    /// вывод; файлы — из таблицы. Ничего настоящего не запускает.
    struct WatchIo {
        runs: Vec<(String, String)>,
        files: Vec<(String, String)>,
        calls: std::cell::RefCell<Vec<String>>,
    }
    impl SystemIo for WatchIo {
        fn exists(&self, _: &str) -> bool { false }
        fn is_file(&self, _: &str) -> bool { false }
        fn is_dir(&self, _: &str) -> bool { false }
        fn is_executable(&self, _: &str) -> bool { false }
        fn realpath(&self, _: &str) -> Option<String> { None }
        fn read_text(&self, path: &str) -> Option<String> {
            self.files.iter().find(|(p, _)| p == path).map(|(_, t)| t.clone())
        }
        fn list_dir(&self, _: &str) -> Option<Vec<String>> { None }
        fn which(&self, _: &str) -> bool { false }
        fn run(&self, argv: &[String], _: Option<u64>, _: bool) -> crate::system_exec::RunOut {
            let key = argv.join(" ");
            self.calls.borrow_mut().push(key.clone());
            let found = self.runs.iter().find(|(k, _)| key.starts_with(k.as_str()));
            crate::system_exec::RunOut {
                outcome: crate::utils::RunOutcome::Ok,
                returncode: Some(if found.is_some() { 0 } else { 1 }),
                stdout: found.map(|(_, o)| o.clone()).unwrap_or_default(),
                stderr: String::new(),
            }
        }
        fn env(&self, _: &str) -> String { String::new() }
        fn which_path(&self, _: &str) -> Option<String> { None }
        fn write_text(&self, _: &str, _: &str) -> Result<(), String> { Err("ro".into()) }
        fn copy_file(&self, _: &str, _: &str) -> Result<(), String> { Err("ro".into()) }
    }

    /// Экран входа запущен на 5-й секунде, сейчас 120-я.
    const DM_SINCE_US: u64 = 5_000_000;

    fn dm_show(id: &str, active: &str) -> String {
        format!(
            "Id={id}\nLoadState=loaded\nActiveState={active}\nSubState=running\nResult=success\n\
             ExecStart={{ path=/usr/bin/{short} ; argv[]=/usr/bin/{short} ; }}\n\
             ActiveEnterTimestampMonotonic={DM_SINCE_US}\nConflicts=getty@tty1.service shutdown.target\n",
            short = id.trim_end_matches(".service"),
        )
    }

    fn jm(mono: u64, ident: &str, msg: &str) -> String {
        serde_json::json!({ "__MONOTONIC_TIMESTAMP": mono.to_string(), "_PID": "700", "SYSLOG_IDENTIFIER": ident, "MESSAGE": msg })
            .to_string()
    }

    /// Сеансы: (номер, класс, состояние) на seat0.
    fn watch_io(dm: &str, sessions: &[(&str, &str, &str)], journal: &[String]) -> WatchIo {
        let mut runs = vec![
            ("systemctl is-system-running".to_string(), "running\n".to_string()),
            ("systemctl show display-manager.service".to_string(), dm_show(dm, "active")),
        ];
        let list: Vec<serde_json::Value> = sessions.iter().map(|(id, _, _)| serde_json::json!({ "session": id, "seat": "seat0" })).collect();
        runs.push(("loginctl list-sessions".into(), serde_json::Value::Array(list).to_string()));
        for (id, class, state) in sessions {
            runs.push((format!("loginctl show-session {id} "), format!("Id={id}\nSeat=seat0\nClass={class}\nState={state}\n")));
        }
        runs.push((format!("journalctl -b -u {dm} "), journal.join("\n")));
        WatchIo { runs, files: vec![("/proc/uptime".into(), "120.50 300.00\n".into())], calls: Default::default() }
    }

    fn greeter_crash_journal() -> Vec<String> {
        vec![
            jm(1_000_000, "sddm", "Auth: sddm-helper exited with 1"),
            jm(6_000_000, "sddm", "Greeter starting..."),
            jm(6_100_000, "sddm", "/usr/bin/sddm-greeter-qt6: symbol lookup error: /usr/bin/sddm-greeter-qt6: undefined symbol: _ZN5QQuick"),
            jm(6_200_000, "sddm", "Auth: sddm-helper exited with 127"),
            jm(6_200_100, "systemd", "this line is not the login screen"),
        ]
    }

    #[test]
    fn greeter_crash_is_shown() {
        let io = watch_io("sddm.service", &[], &greeter_crash_journal());
        let f = watch_facts(&io, None);
        assert_eq!(f.dm.active_since_us, Some(DM_SINCE_US));
        assert_eq!(f.uptime_us, Some(120_500_000));
        assert_eq!(decide_greeter(&f), Verdict::Rescue, "{f:?}");
        // Строка загрузчика понятнее кода — она и улика.
        assert!(f.evidence.as_deref().unwrap().contains("symbol lookup error"), "{f:?}");
        // Упал сигналом, без строки загрузчика.
        let io = watch_io("sddm.service", &[], &[jm(7_000_000, "sddm", "Auth: sddm-helper exited with 11")]);
        let f = watch_facts(&io, None);
        assert_eq!(f.evidence.as_deref(), Some("Auth: sddm-helper exited with 11"));
        assert_eq!(decide_greeter(&f), Verdict::Rescue);
    }

    #[test]
    fn greeter_present_means_nothing_to_do() {
        let io = watch_io("sddm.service", &[("c1", "greeter", "online")], &greeter_crash_journal());
        let f = watch_facts(&io, None);
        assert!(matches!(decide_greeter(&f), Verdict::Skip(_)), "{f:?}");
        // До журнала не дошли: решение ясно по сеансам.
        assert!(!io.calls.borrow().iter().any(|c| c.starts_with("journalctl")));
    }

    #[test]
    fn logged_in_person_means_nothing_to_do() {
        for (class, state) in [("user", "active"), ("user", "online"), ("user-early", "active"), ("lock-screen", "online")] {
            let io = watch_io("sddm.service", &[("2", class, state)], &greeter_crash_journal());
            let f = watch_facts(&io, None);
            assert!(matches!(decide_greeter(&f), Verdict::Skip(_)), "{class}/{state}");
        }
        // Сеанс без места (ssh) человека на экране не значит.
        let mut f = watch_facts(&watch_io("sddm.service", &[], &greeter_crash_journal()), None);
        f.sessions.push(SessionInfo { id: "5".into(), seat: String::new(), class: "user".into(), state: "active".into() });
        assert_eq!(decide_greeter(&f), Verdict::Rescue);
    }

    /// Автовход: человек вышел (сеанс доживает в `closing`), SDDM поднял
    /// приветствие, и оно упало.
    #[test]
    fn autologin_logout_then_greeter_crash_is_shown() {
        let journal = vec![
            jm(6_000_000, "sddm", "Auth: sddm-helper exited successfully"),
            jm(6_100_000, "sddm", "Session started true"),
            jm(90_000_000, "sddm", "Auth: sddm-helper exited successfully"),
            jm(90_500_000, "sddm", "/usr/bin/sddm-greeter-qt6: error while loading shared libraries: libQt6Quick.so.6: cannot open shared object file"),
            jm(90_600_000, "sddm", "Auth: sddm-helper exited with 127"),
        ];
        let io = watch_io("sddm.service", &[("2", "user", "closing")], &journal);
        let f = watch_facts(&io, None);
        assert_eq!(decide_greeter(&f), Verdict::Rescue, "{f:?}");
        assert!(f.evidence.as_deref().unwrap().contains("libQt6Quick.so.6"));
        // Только вышел, приветствие ещё не поднялось: удачный выход — не улика.
        let io = watch_io("sddm.service", &[("2", "user", "closing")], &journal[..3]);
        assert!(matches!(decide_greeter(&watch_facts(&io, None)), Verdict::Skip(_)));
        // Сеанс человека кончился с ошибкой, а приветствие поднялось.
        let journal = vec![jm(90_000_000, "sddm", "Auth: sddm-helper exited with 1")];
        let io = watch_io("sddm.service", &[("c3", "greeter", "online")], &journal);
        assert!(matches!(decide_greeter(&watch_facts(&io, None)), Verdict::Skip(_)));
    }

    #[test]
    fn watch_stays_quiet_when_it_should() {
        let crash = greeter_crash_journal();
        let base = watch_facts(&watch_io("sddm.service", &[], &crash), None);
        assert_eq!(decide_greeter(&base), Verdict::Rescue);
        // Уже показали — ни одного запуска.
        let io = watch_io("sddm.service", &[], &crash);
        let f = watch_facts(&io, Some("sddm.service failed\n"));
        assert!(matches!(decide_greeter(&f), Verdict::Skip(_)));
        assert!(io.calls.borrow().is_empty());
        // Выключение.
        let mut f = base.clone();
        f.stopping = true;
        assert!(matches!(decide_greeter(&f), Verdict::Skip(_)));
        // Экран входа только что запустился.
        let mut f = base.clone();
        f.uptime_us = Some(DM_SINCE_US + 10_000_000);
        assert_eq!(decide_greeter(&f), Verdict::Wait);
        // Экран входа упал — это дело `--boot`.
        let mut f = base.clone();
        f.dm.active = "failed".into();
        assert!(matches!(decide_greeter(&f), Verdict::Skip(_)));
        // Чужой экран входа — сторож не смотрит.
        let io = watch_io("gdm.service", &[], &crash);
        let f = watch_facts(&io, None);
        assert!(matches!(decide_greeter(&f), Verdict::Skip(_)));
        assert!(!io.calls.borrow().iter().any(|c| c.starts_with("loginctl")));
        // Падение до нынешнего запуска экрана входа — не в счёт.
        let old = vec![jm(1_000_000, "sddm", "Auth: sddm-helper exited with 127")];
        assert!(matches!(decide_greeter(&watch_facts(&watch_io("sddm.service", &[], &old), None)), Verdict::Skip(_)));
        // Улик нет вовсе.
        assert!(matches!(decide_greeter(&watch_facts(&watch_io("sddm.service", &[], &[]), None)), Verdict::Skip(_)));
    }

    /// Plasma Login Manager: окно входа — служба пользователя plasmalogin;
    /// она упала, а сеанс приветствия остался.
    #[test]
    fn plasmalogin_failed_greeter_unit_is_shown() {
        let mut io = watch_io("plasmalogin.service", &[("c1", "greeter", "online")], &[]);
        io.runs.push((
            "systemctl --user -M plasmalogin@ show plasma-login.service plasma-login-kwin_wayland.service".into(),
            "Id=plasma-login.service\nActiveState=failed\n\nId=plasma-login-kwin_wayland.service\nActiveState=active\n".into(),
        ));
        let f = watch_facts(&io, None);
        assert_eq!(f.greeter_units_failed, vec!["plasma-login.service"]);
        assert_eq!(decide_greeter(&f), Verdict::Rescue);
        // Живые — ничего.
        let mut io = watch_io("plasmalogin.service", &[("c1", "greeter", "online")], &[]);
        io.runs.push(("systemctl --user -M plasmalogin@ show".into(), "Id=plasma-login.service\nActiveState=active\n".into()));
        assert!(matches!(decide_greeter(&watch_facts(&io, None)), Verdict::Skip(_)));
        // Сама программа приветствия не запустилась — как у SDDM.
        let j = vec![jm(7_000_000, "plasmalogin", "Auth: plasmalogin-helper exited with 127")];
        assert_eq!(decide_greeter(&watch_facts(&watch_io("plasmalogin.service", &[], &j), None)), Verdict::Rescue);
        let files: Vec<String> = vec!["/usr/bin/plasmalogin".into(), "/usr/bin/startplasma-login-wayland".into(), "/usr/lib/plasma-login-greeter".into()];
        assert_eq!(greeter_programs("plasmalogin", &files), vec!["/usr/bin/startplasma-login-wayland", "/usr/lib/plasma-login-greeter"]);
        assert!(greeter_programs("sddm", &files).is_empty());
    }

    #[test]
    fn watch_parsers() {
        assert_eq!(helper_exit_code("Auth: sddm-helper exited with 127"), Some(127));
        assert_eq!(helper_exit_code("[08:05:12.1] (WW) DAEMON: Auth: sddm-helper exited with -1 x"), Some(-1));
        assert_eq!(helper_exit_code("Auth: sddm-helper exited successfully"), Some(0));
        assert_eq!(helper_exit_code("Greeter stopped."), None);
        let crashed = [JournalEntry { message: "Auth: sddm-helper (--socket x) crashed (exit code 1)".into(), ..Default::default() }];
        assert!(greeter_crash_evidence(&crashed, None).is_some());
        // SteamOS: weston нет → откат на x11-user; помощник выходит с 0, а
        // окно входа закрылось само (журнал ВМ SteamOS 3.9.5).
        let x11user = |tail: &[&str]| -> Vec<JournalEntry> {
            ["Greeter starting...", "Auth: sddm-helper exited with 4", "Greeter stopped. SDDM::Auth::HELPER_DISPLAYSERVER_ERROR",
             "Failed to launch the display server, falling back to DisplayServer=x11-user", "Greeter starting...",
             "Greeter session started successfully"]
                .iter()
                .chain(tail)
                .map(|m| JournalEntry { message: m.to_string(), ..Default::default() })
                .collect()
        };
        let died = x11user(&["Auth: sddm-helper exited successfully", "Greeter stopped. SDDM::Auth::HELPER_SUCCESS"]);
        assert_eq!(greeter_crash_evidence(&died, None).as_deref(), Some("Greeter stopped. SDDM::Auth::HELPER_SUCCESS"));
        // Человек вошёл: SDDM сам останавливает окно входа — не улика.
        let login = x11user(&["Greeter stopping...", "Auth: sddm-helper exited successfully", "Greeter stopped. SDDM::Auth::HELPER_SUCCESS"]);
        assert_eq!(greeter_crash_evidence(&login, None), None);
        // Окно входа упало, но SDDM поднял новое — прежнее не в счёт.
        let again = x11user(&["Auth: sddm-helper exited successfully", "Greeter stopped. SDDM::Auth::HELPER_SUCCESS", "Greeter starting..."]);
        assert_eq!(greeter_crash_evidence(&again, None), None);
        assert_eq!(parse_session_ids("[{\"session\":\"2\"},{\"session\":\"c1\"}]"), vec!["2", "c1"]);
        assert_eq!(parse_session_ids("     2 1000 alex seat0 tty2\n    c1  967 sddm seat0 tty1\n"), vec!["2", "c1"]);
        assert!(parse_session_ids("[]").is_empty());
        assert_eq!(parse_uptime_us("1.5 3.0"), Some(1_500_000));
        assert_eq!(parse_uptime_us(""), None);
        assert_eq!(parse_active_vt("tty1\n"), Some(1));
        assert_eq!(parse_active_vt(""), None);
        assert_eq!(stamp_text("sddm.service", true), "sddm.service greeter\n");
        assert!(stamp_says_greeter(Some("sddm.service greeter\n"), "sddm.service"));
        assert!(!stamp_says_greeter(Some("sddm.service failed\n"), "sddm.service"));
        assert!(!stamp_says_greeter(Some("sddm.service\n"), "sddm.service"));
        assert!(!stamp_says_greeter(None, "sddm.service"));
        let s = parse_show(&dm_show("sddm.service", "active"));
        assert_eq!(s.conflicts, vec!["getty@tty1.service", "shutdown.target"]);
        assert_eq!(parse_show("ActiveEnterTimestampMonotonic=0\n").active_since_us, None);
    }

    /// Экран входа жив: `getty@` его консоли остановил бы его (`Conflicts=`),
    /// а на его консоли (активной) приглашения не видно.
    #[test]
    fn prompt_console_avoids_the_login_screen() {
        let c = vec!["getty@tty1.service".to_string()];
        assert_eq!(pick_vt("quiet", Some(1), &c), Some(2));
        assert_eq!(pick_vt("quiet", Some(2), &c), Some(3));
        assert_eq!(pick_vt("quiet", Some(7), &[]), Some(1));
        // SteamOS: рисуются только 4–6.
        let steamos = "quiet fbcon=vc:4-6";
        assert_eq!(pick_vt(steamos, Some(4), &[]), Some(5));
        let all: Vec<String> = (4..=6).map(getty_unit).collect();
        assert_eq!(pick_vt(steamos, Some(1), &all), None);
    }

    #[test]
    fn greeter_gone_texts() {
        let mut o = observed_qt_mix();
        o.dm.active = "active".into();
        o.dm.sub = "running".into();
        o.greeter_gone = true;
        let d = diagnose(&o);
        let text = console_message(&d, None);
        assert!(text.starts_with("Rexon: the login screen SDDM is running, but its login window has closed"), "{text}");
        let doctor = doctor_report(&d, ARCH, &[], None, &|m| m.to_string());
        assert!(doctor.starts_with("The login screen SDDM is running (active/running), but its login window has closed."), "{doctor}");
        assert!(doctor.contains("Likely cause:") && doctor.contains("sudo reboot"), "{doctor}");
    }

    #[test]
    fn watch_units_text() {
        let unit = watch_unit(RESCUE_BIN_PACKAGE);
        assert!(unit.contains("ExecStart=/usr/lib/rexon/rexon-login-rescue --watch-greeter\n"));
        assert!(unit.contains("Type=oneshot\n"));
        // Обычные зависимости: при выключении служба не запускается.
        assert!(!unit.contains("DefaultDependencies=no"));
        assert!(!unit.contains("Restart="));
        let timer = watch_timer();
        assert!(timer.contains("OnActiveSec=45s\n") && timer.contains("OnUnitInactiveSec=30s\n"));
        assert!(timer.contains(&format!("Unit={WATCH_SERVICE}\n")));
        assert!(!timer.contains("[Install]"));
        let files = unit_files(UNIT_DIR_PACKAGE, RESCUE_BIN_PACKAGE);
        for name in [WATCH_SERVICE, WATCH_TIMER] {
            assert!(files.iter().any(|(p, _)| *p == format!("{UNIT_DIR_PACKAGE}/{name}")), "{name}");
        }
    }
}
