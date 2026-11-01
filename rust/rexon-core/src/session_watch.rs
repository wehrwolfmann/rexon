//! Сторож сеанса Plasma для AeroThemePlasma: падения KWin и plasmashell
//! сразу после входа.
//!
//! ЗАЧЕМ. Сторож до KWin ([`crate::session_guard`]) ловит расхождение по
//! версиям. Но бывает, что версии сходятся, а модуль всё равно роняет
//! процесс: KWin пересобран под новый Qt, у эффекта разошёлся приватный ABI.
//! Тогда `kwin_wayland_wrapper` перезапускает KWin до 10 раз и выкидывает к
//! экрану входа, а plasmashell после трёх падений за минуту systemd
//! оставляет мёртвым — чёрный стол с курсором.
//!
//! КАК. Пользовательский юнит [`UNIT_NAME`] после `graphical-session.target`
//! запускает `rexon --session-watch`. Первые [`WATCH_SECS`] секунд он раз в
//! [`POLL_SECS`] смотрит:
//!   * `systemctl --user show` у `plasma-plasmashell.service` (`NRestarts`,
//!     `Result=start-limit-hit`, `ActiveState`) и у `plasma-kwin_x11.service`
//!     (на X11 KWin перезапускает сам systemd — `NRestarts`);
//!   * журнал сеанса: записи `systemd-coredump` о падении `kwin_wayland`,
//!     `kwin_x11`, `plasmashell` (они лежат в журнале пользователя, без root,
//!     и несут стек упавшего потока — по нему видно, в чьём модуле упало), и
//!     сколько разных процессов `kwin_wayland` писало в журнал (на Wayland
//!     обёртка перезапускает KWin молча, без единой строки и без
//!     `NRestarts`: новый процесс — единственный след без дампа).
//!
//! Порог ([`KWIN_CRASH_THRESHOLD`] падения KWin или отказ plasmashell) ⇒
//! кто виноват ([`culprits`]): модуль набора в стеке упавшего потока или в
//! находках [`crate::plugin_journal`] этого входа. Виноват Aero:
//!   * KWin — эффекты Aero выключаются СРАЗУ ([`crate::session_guard::fall_back_to_stock`]
//!     с одними эффектами): следующий перезапуск KWin обёрткой уже читает
//!     новый `kwinrc`, и сеанс выживает;
//!   * plasmashell жив — раскладку посреди сеанса не тронуть (plasmashell
//!     держит её в памяти), поэтому отметка
//!     [`crate::session_guard::request_fallback`] на следующий вход;
//!   * plasmashell мёртв (systemd сдался) — раскладку держать некому, её
//!     можно заменить сейчас: откат целиком, потом plasmashell поднимается
//!     заново (`reset-failed` + `start`).
//!
//! После — уведомление с причиной (имя модуля).
//!
//! ЗАЩИТА ОТ ДУРАКА.
//!   * Не Plasma, нет AeroThemePlasma, проверка выключена в настройках —
//!     сторож молчит (юнит и не ставится).
//!   * Виноват не Aero или виновника не видно — только запись в журнал
//!     Rexon, оформление не трогается: чужая беда (драйвер, Qt, чужой
//!     виджет) откатом Aero не лечится.
//!   * Дамп обрабатывается секунды, а `NRestarts` растёт сразу — поэтому
//!     после порога сторож ждёт стек ещё [`LATE_TRACE_SECS`] секунд, а не
//!     решает «не Aero» по пустому журналу.
//!   * Один и тот же случай (сторона + модули + версии KWin/Plasma/Qt/набора)
//!     обрабатывается один раз ([`handled_path`]): человек, вернувший
//!     оформление руками без пересборки, не получит отката снова. После
//!     пересборки версии другие — и отпечаток другой.
//!   * Никакого root: только `~/.config`, `~/.local/state`, `systemctl --user`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::i18n::trf;
use crate::plugin_journal::{Finding, PluginKind};
use crate::session_guard::{self as guard, FallbackCause, FallbackPlan, GuardPaths, PendingNotice};

// ============================================================================
// Имена, пороги
// ============================================================================

/// Флаг маршрута сторожа (его зовёт юнит).
pub const WATCH_FLAG: &str = "--session-watch";
/// Пользовательский юнит сторожа.
pub const UNIT_NAME: &str = "rexon-session-watch.service";
pub const PLASMASHELL_UNIT: &str = "plasma-plasmashell.service";
pub const KWIN_WAYLAND_UNIT: &str = "plasma-kwin_wayland.service";
pub const KWIN_X11_UNIT: &str = "plasma-kwin_x11.service";
pub const SESSION_TARGET: &str = "graphical-session.target";

/// Сколько следить после входа, секунды. Падения из-за несовместимого
/// модуля случаются при загрузке — в первые секунды; позже падение уже не
/// «сразу после входа», и откатывать по нему оформление — ложная тревога.
pub const WATCH_SECS: u64 = 120;
/// Как часто смотреть, секунды. Обёртка KWin перезапускает его за доли
/// секунды; десять падений подряд — это 5–20 с, и откат обязан успеть до
/// того, как она сдастся.
pub const POLL_SECS: u64 = 1;
/// Сколько после порога ждать стек упавшего потока, секунды (дамп пишется
/// с задержкой).
pub const LATE_TRACE_SECS: u64 = 20;
/// Падения KWin: со второго подряд это уже не случайность — второй
/// перезапуск с тем же модулем падает так же, а на десятом обёртка сдаётся.
pub const KWIN_CRASH_THRESHOLD: u32 = 2;
/// Перезапуски plasmashell: systemd сдаётся на третьем падении за минуту
/// (`StartLimitBurst=3`); на втором ещё можно предупредить живым столом.
pub const SHELL_RESTART_THRESHOLD: u32 = 2;
/// Запас назад от начала сеанса, секунды (KWin стартует чуть раньше цели).
pub const SINCE_MARGIN_SECS: u64 = 30;
/// Сколько отпечатков помнить.
const HANDLED_KEEP: usize = 20;

/// Где помнить обработанные случаи.
pub fn handled_path(paths: &GuardPaths) -> PathBuf {
    paths.state_dir.join("watch-handled")
}

// ============================================================================
// systemctl --user show
// ============================================================================

/// Состояние одного юнита.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UnitState {
    pub id: String,
    pub active_state: String,
    pub result: String,
    pub n_restarts: u32,
    pub main_pid: u32,
    /// Когда юнит вышел из «неактивен», микросекунды эпохи.
    pub inactive_exit_us: Option<u64>,
}

impl UnitState {
    pub fn alive(&self) -> bool {
        self.active_state == "active"
    }
    /// systemd сдался: больше сам не перезапустит.
    pub fn gave_up(&self) -> bool {
        self.result == "start-limit-hit" || self.active_state == "failed"
    }
}

/// Команда опроса юнитов.
pub fn show_argv() -> Vec<String> {
    let mut argv: Vec<String> = ["systemctl", "--user", "show", "--timestamp=unix"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    for p in ["Id", "ActiveState", "Result", "NRestarts", "MainPID", "InactiveExitTimestamp"] {
        argv.push("-p".into());
        argv.push(p.into());
    }
    argv.extend([PLASMASHELL_UNIT, KWIN_WAYLAND_UNIT, KWIN_X11_UNIT, SESSION_TARGET].iter().map(|s| s.to_string()));
    argv
}

/// Метка времени `--timestamp=unix` (`@1729850000`) → микросекунды. Иной
/// вид (старый systemd без этого ключа) — не знаем.
fn unix_stamp_us(v: &str) -> Option<u64> {
    let secs: u64 = v.trim().strip_prefix('@')?.split('.').next()?.parse().ok()?;
    (secs > 0).then_some(secs * 1_000_000)
}

/// Разбор `systemctl show` по нескольким юнитам: блоки `Ключ=значение`
/// через пустую строку.
pub fn parse_show(text: &str) -> Vec<UnitState> {
    let mut out = Vec::new();
    let mut cur = UnitState::default();
    let mut any = false;
    let flush = |cur: &mut UnitState, any: &mut bool, out: &mut Vec<UnitState>| {
        if *any && !cur.id.is_empty() {
            out.push(std::mem::take(cur));
        } else {
            *cur = UnitState::default();
        }
        *any = false;
    };
    for line in text.lines() {
        if line.trim().is_empty() {
            flush(&mut cur, &mut any, &mut out);
            continue;
        }
        let Some((k, v)) = line.split_once('=') else { continue };
        any = true;
        match k.trim() {
            "Id" => cur.id = v.trim().to_string(),
            "ActiveState" => cur.active_state = v.trim().to_string(),
            "Result" => cur.result = v.trim().to_string(),
            "NRestarts" => cur.n_restarts = v.trim().parse().unwrap_or(0),
            "MainPID" => cur.main_pid = v.trim().parse().unwrap_or(0),
            "InactiveExitTimestamp" => cur.inactive_exit_us = unix_stamp_us(v),
            _ => {}
        }
    }
    flush(&mut cur, &mut any, &mut out);
    out
}

fn unit<'a>(units: &'a [UnitState], id: &str) -> Option<&'a UnitState> {
    units.iter().find(|u| u.id == id)
}

/// С какого времени читать журнал: начало сеанса (выход цели сеанса из
/// «неактивна») с запасом. None — systemd не назвал.
pub fn session_since_us(units: &[UnitState]) -> Option<u64> {
    let t = [SESSION_TARGET, KWIN_WAYLAND_UNIT, KWIN_X11_UNIT]
        .iter()
        .filter_map(|id| unit(units, id).and_then(|u| u.inactive_exit_us))
        .min()?;
    Some(t.saturating_sub(SINCE_MARGIN_SECS * 1_000_000))
}

// ============================================================================
// Журнал сеанса: дампы и процессы KWin
// ============================================================================

/// Команда чтения журнала с `since_us`.
pub fn journal_argv(since_us: u64) -> Vec<String> {
    let mut argv: Vec<String> = ["journalctl", "--user", "-b", "--no-pager", "-q", "-o", "json", "--all"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    argv.push(format!("--since=@{}", since_us / 1_000_000));
    argv.push("--output-fields=MESSAGE,_COMM,_PID,SYSLOG_IDENTIFIER,COREDUMP_COMM,COREDUMP_USER_UNIT".into());
    // Поля одного имени — «или», `+` — «или» между группами.
    argv.extend(["_COMM=kwin_wayland", "_COMM=kwin_x11", "+", "SYSLOG_IDENTIFIER=systemd-coredump"].iter().map(|s| s.to_string()));
    argv
}

/// Одно падение по записи `systemd-coredump`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Crash {
    /// Имя процесса (`kwin_wayland`, `plasmashell`).
    pub comm: String,
    pub time_us: u64,
    /// Модули в стеке упавшего (первого) потока, по порядку кадров.
    pub modules: Vec<String>,
}

/// Что увидели в журнале.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JournalFacts {
    pub crashes: Vec<Crash>,
    /// Разные процессы `kwin_wayland`, писавшие в журнал.
    pub kwin_pids: BTreeSet<u64>,
}

/// Процессы, чьи падения считаем.
const WATCHED_COMMS: &[&str] = &["kwin_wayland", "kwin_x11", "plasmashell"];

/// Модули стека первого потока: кадры `#3 0x… func (libfoo.so + 0x12)`.
pub fn crash_modules(message: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut in_first = false;
    for line in message.lines() {
        let t = line.trim();
        if t.starts_with("Stack trace of thread") {
            if in_first {
                break;
            }
            in_first = true;
            continue;
        }
        if !in_first || !t.starts_with('#') {
            continue;
        }
        let Some(open) = t.rfind('(') else { continue };
        let inner = &t[open + 1..];
        let Some((module, _)) = inner.split_once(" + ") else { continue };
        let module = module.trim();
        if !module.is_empty() && !out.iter().any(|m| m == module) {
            out.push(module.to_string());
        }
    }
    out
}

fn field(v: &Value, key: &str) -> Option<String> {
    match v.get(key)? {
        Value::String(s) => Some(s.clone()),
        Value::Array(bytes) => {
            let raw: Vec<u8> = bytes.iter().filter_map(|b| b.as_u64().map(|b| b as u8)).collect();
            Some(String::from_utf8_lossy(&raw).into_owned())
        }
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// Разбор `journalctl -o json` ([`journal_argv`]).
pub fn parse_journal(text: &str) -> JournalFacts {
    let mut facts = JournalFacts::default();
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line.trim()) else { continue };
        let time_us = field(&v, "__REALTIME_TIMESTAMP").and_then(|s| s.parse().ok()).unwrap_or(0);
        if field(&v, "SYSLOG_IDENTIFIER").as_deref() == Some("systemd-coredump") {
            let Some(comm) = field(&v, "COREDUMP_COMM") else { continue };
            if !WATCHED_COMMS.contains(&comm.as_str()) {
                continue;
            }
            let modules = field(&v, "MESSAGE").map(|m| crash_modules(&m)).unwrap_or_default();
            facts.crashes.push(Crash { comm, time_us, modules });
            continue;
        }
        if field(&v, "_COMM").as_deref() == Some("kwin_wayland") {
            if let Some(pid) = field(&v, "_PID").and_then(|p| p.parse().ok()) {
                facts.kwin_pids.insert(pid);
            }
        }
    }
    facts
}

// ============================================================================
// Наблюдение и порог
// ============================================================================

/// Что случилось с начала сеанса.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Observation {
    pub kwin_crashes: u32,
    pub shell_restarts: u32,
    /// systemd больше не поднимет plasmashell.
    pub shell_gave_up: bool,
    pub shell_alive: bool,
    pub crashes: Vec<Crash>,
}

impl Observation {
    pub fn kwin_failed(&self) -> bool {
        self.kwin_crashes >= KWIN_CRASH_THRESHOLD
    }
    pub fn shell_failed(&self) -> bool {
        self.shell_gave_up || self.shell_restarts >= SHELL_RESTART_THRESHOLD
    }
}

/// Свести юниты и журнал. Падения KWin — наибольшее из: дампов, лишних
/// процессов `kwin_wayland` (каждый перезапуск обёрткой — новый процесс),
/// `NRestarts` юнита KWin на X11. Источники пересекаются, поэтому не
/// складываются.
pub fn observe(units: &[UnitState], journal: &JournalFacts) -> Observation {
    let dumps = |pred: &dyn Fn(&str) -> bool| journal.crashes.iter().filter(|c| pred(&c.comm)).count() as u32;
    let kwin_dumps = dumps(&|c| c.starts_with("kwin_"));
    let respawns = (journal.kwin_pids.len() as u32).saturating_sub(1);
    let x11 = unit(units, KWIN_X11_UNIT).map(|u| u.n_restarts).unwrap_or(0);
    let shell = unit(units, PLASMASHELL_UNIT).cloned().unwrap_or_default();
    Observation {
        kwin_crashes: kwin_dumps.max(respawns).max(x11),
        shell_restarts: shell.n_restarts.max(dumps(&|c| c == "plasmashell")),
        shell_gave_up: shell.gave_up(),
        shell_alive: shell.alive(),
        crashes: journal.crashes.clone(),
    }
}

// ============================================================================
// Кто виноват
// ============================================================================

/// Сторона, которая падает.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Side {
    Kwin,
    Shell,
}

/// Что известно о наборе на этой машине.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AeroSet {
    /// Пакеты набора.
    pub packages: Vec<String>,
    /// Имена файлов модулей набора (`smodglow.so`) — их ищем в стеке.
    pub modules: Vec<String>,
    pub effect_ids: Vec<String>,
    pub applet_ids: Vec<String>,
}

/// Имена `.so` из файлов пакетов набора. Форк libplasma не в счёт: его
/// библиотеки зовутся как штатные (`libPlasma.so.6`) и есть в стеке любого
/// падения plasmashell — по ним виновника не назвать.
pub fn aero_modules(files: &[(String, Vec<String>)]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (pkg, list) in files {
        if pkg.starts_with(crate::aeroshell::LIBPLASMA_FORK_PREFIX) {
            continue;
        }
        for path in list {
            let name = path.rsplit('/').next().unwrap_or(path);
            if (name.ends_with(".so") || name.contains(".so.")) && !out.iter().any(|m| m == name) {
                out.push(name.to_string());
            }
        }
    }
    out
}

/// Виновник.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Culprit {
    pub side: Side,
    /// Имя для человека (из метаданных модуля либо имя файла).
    pub name: String,
    /// Техническая строка для журнала и «Подробнее».
    pub technical: String,
}

/// Модуль из стека — модуль набора (имя файла, а у библиотеки с версией —
/// и без хвоста `.so.N`).
fn is_aero_module(module: &str, aero: &AeroSet) -> bool {
    let stem = |m: &str| m.split(".so").next().unwrap_or(m).to_string();
    aero.modules.iter().any(|a| a == module || stem(a) == stem(module))
}

/// Виновники из набора: модуль набора в стеке упавшего потока или в
/// находках журнала этого входа ([`crate::plugin_journal`]). Чужие модули
/// сюда не попадают — по ним оформление не трогают.
pub fn culprits(obs: &Observation, aero: &AeroSet, findings: &[Finding]) -> Vec<Culprit> {
    let mut out: Vec<Culprit> = Vec::new();
    let mut push = |c: Culprit| {
        if !out.iter().any(|o| o.side == c.side && o.name == c.name) {
            out.push(c);
        }
    };
    for c in &obs.crashes {
        let side = if c.comm == "plasmashell" { Side::Shell } else { Side::Kwin };
        if let Some(m) = c.modules.iter().find(|m| is_aero_module(m, aero)) {
            push(Culprit { side, name: m.clone(), technical: format!("{}: crashed in {m}", c.comm) });
        }
    }
    for f in findings {
        let by_package = f.package().is_some_and(|p| aero.packages.iter().any(|a| a == p));
        let by_id = match f.failure.kind {
            PluginKind::Effect | PluginKind::KwinPlugin => aero.effect_ids.contains(&f.failure.id),
            PluginKind::Applet => aero.applet_ids.contains(&f.failure.id),
            PluginKind::Decoration => false,
        };
        if !(by_package || by_id) {
            continue;
        }
        let side = if f.failure.kind == PluginKind::Applet { Side::Shell } else { Side::Kwin };
        push(Culprit {
            side,
            name: f.name.clone(),
            technical: format!("{}: {}", f.failure.id, f.failure.cause.technical()),
        });
    }
    out
}

// ============================================================================
// Решение
// ============================================================================

/// Что делать с plasmashell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellAct {
    None,
    /// Жив — раскладка со следующего входа ([`guard::request_fallback`]).
    NextLogin,
    /// Мёртв — откат целиком сейчас и поднять заново.
    Revive,
}

/// Откат.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Action {
    /// Выключить эффекты Aero сейчас.
    pub kwin: bool,
    pub shell: ShellAct,
    /// Причина для уведомления (человеческая).
    pub reason: String,
    /// Технические строки.
    pub why: Vec<String>,
    pub fingerprint: String,
}

/// Итог одного взгляда.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchDecision {
    /// Порог не достигнут.
    Nothing,
    /// Порог достигнут, виновника пока не видно — ждать стек.
    Wait,
    /// Только запись в журнал Rexon.
    LogOnly { why: Vec<String> },
    Act(Action),
}

/// Причина для уведомления.
pub fn culprit_reason(c: &Culprit) -> String {
    match c.side {
        // TRANSLATORS: %1 is the name of an AeroThemePlasma module.
        Side::Kwin => trf("“%1” made KWin crash", &[&c.name]),
        // TRANSLATORS: %1 is the name of an AeroThemePlasma module.
        Side::Shell => trf("“%1” made the Plasma desktop crash", &[&c.name]),
    }
}

/// Решить по наблюдению.
///
/// `done` — стороны, по которым этот сторож уже откатил (второй раз не
/// трогаем); `versions` — версии KWin/Plasma/Qt/набора для отпечатка;
/// `handled` — отпечатки, обработанные прежними входами; `final_look` —
/// ждать стек дальше некуда.
pub fn decide(
    obs: &Observation,
    found: &[Culprit],
    done: &[Side],
    versions: &str,
    handled: &[String],
    final_look: bool,
) -> WatchDecision {
    let kwin_failed = obs.kwin_failed() && !done.contains(&Side::Kwin);
    let shell_failed = obs.shell_failed() && !done.contains(&Side::Shell);
    if !kwin_failed && !shell_failed {
        return WatchDecision::Nothing;
    }
    let kwin_c: Vec<&Culprit> = found.iter().filter(|c| kwin_failed && c.side == Side::Kwin).collect();
    let shell_c: Vec<&Culprit> = found.iter().filter(|c| shell_failed && c.side == Side::Shell).collect();
    let mut why = vec![format!(
        "KWin crashes {}, plasmashell restarts {}{}",
        obs.kwin_crashes,
        obs.shell_restarts,
        if obs.shell_gave_up { ", plasmashell gave up" } else { "" }
    )];
    if kwin_c.is_empty() && shell_c.is_empty() {
        if !final_look {
            return WatchDecision::Wait;
        }
        why.push("no AeroThemePlasma module is to blame: the look stays as it is".into());
        for c in &obs.crashes {
            why.push(format!("{} crashed in {}", c.comm, c.modules.iter().take(4).cloned().collect::<Vec<_>>().join(", ")));
        }
        return WatchDecision::LogOnly { why };
    }
    let shell = if shell_c.is_empty() {
        ShellAct::None
    } else if obs.shell_alive {
        ShellAct::NextLogin
    } else if obs.shell_gave_up {
        ShellAct::Revive
    } else if final_look {
        // Ни жив, ни сдался (systemd как раз перезапускает) — раскладку
        // сейчас не трогать.
        ShellAct::NextLogin
    } else {
        return WatchDecision::Wait;
    };
    let mut names: Vec<String> = kwin_c.iter().chain(shell_c.iter()).map(|c| format!("{:?}:{}", c.side, c.name)).collect();
    names.sort();
    let fingerprint = guard::fingerprint(&format!(
        "{}{}|{}|{versions}",
        if kwin_c.is_empty() { "" } else { "kwin" },
        if shell_c.is_empty() { "" } else { "+shell" },
        names.join(",")
    ));
    why.extend(kwin_c.iter().chain(shell_c.iter()).map(|c| c.technical.clone()));
    if handled.iter().any(|h| h == &fingerprint) {
        why.push("the same crash was handled at an earlier login: the look stays as it is".into());
        return WatchDecision::LogOnly { why };
    }
    let reason = kwin_c.iter().chain(shell_c.iter()).take(2).map(|c| culprit_reason(c)).collect::<Vec<_>>().join("; ");
    WatchDecision::Act(Action { kwin: !kwin_c.is_empty(), shell, reason, why, fingerprint })
}

/// Прочитать обработанные отпечатки.
pub fn load_handled(paths: &GuardPaths) -> Vec<String> {
    std::fs::read_to_string(handled_path(paths))
        .map(|t| t.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string).collect())
        .unwrap_or_default()
}

/// Запомнить отпечаток (последние [`HANDLED_KEEP`]).
pub fn remember_handled(paths: &GuardPaths, fingerprint: &str) -> Result<(), String> {
    let mut list = load_handled(paths);
    list.retain(|f| f != fingerprint);
    list.push(fingerprint.to_string());
    let from = list.len().saturating_sub(HANDLED_KEEP);
    guard::write_atomic(&handled_path(paths), &(list[from..].join("\n") + "\n"))
}

// ============================================================================
// Исполнение решения (общее для живого и проверок)
// ============================================================================

/// Что сделал откат.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ActDone {
    pub effects_off: bool,
    pub layout_replaced: bool,
    pub requested: bool,
    /// Поднять plasmashell заново.
    pub revive_shell: bool,
    pub notice: Option<PendingNotice>,
}

/// Исполнить [`Action`] над файлами. `shell_dead_now` — перепроверка прямо
/// перед правкой раскладки: plasmashell всё ещё мёртв (иначе только отметка
/// на следующий вход). Порядок: эффекты → раскладка/отметка → отпечаток.
pub fn apply(paths: &GuardPaths, action: &Action, parts: &guard::AeroParts, shell_dead_now: bool) -> Result<ActDone, String> {
    let cause = FallbackCause::Requested { reason: action.reason.clone() };
    let mut done = ActDone::default();
    let effects: Vec<String> = parts.effects.iter().map(|e| e.id.clone()).collect();
    if action.kwin && !effects.is_empty() {
        let plan = FallbackPlan { effects: effects.clone(), applets: Vec::new(), why: action.why.clone() };
        let d = guard::fall_back_to_stock(paths, &plan, cause.clone())?;
        done.effects_off = !d.effects_off.is_empty();
    }
    match action.shell {
        ShellAct::None => {}
        ShellAct::Revive if shell_dead_now => {
            let plan = FallbackPlan { effects, applets: parts.applets.clone(), why: action.why.clone() };
            let d = guard::fall_back_to_stock(paths, &plan, cause)?;
            done.effects_off |= !d.effects_off.is_empty();
            done.layout_replaced = d.layout_replaced;
            done.revive_shell = true;
        }
        ShellAct::Revive | ShellAct::NextLogin => {
            guard::request_fallback(paths, &action.reason)?;
            done.requested = true;
        }
    }
    remember_handled(paths, &action.fingerprint)?;
    done.notice = if done.effects_off || done.layout_replaced {
        Some(PendingNotice::TurnedOff {
            effects: done.effects_off,
            layout: done.layout_replaced,
            why: action.why.clone(),
            reason: Some(action.reason.clone()),
        })
    } else if done.requested {
        Some(PendingNotice::NextLogin { reason: action.reason.clone() })
    } else {
        None
    };
    Ok(done)
}

// ============================================================================
// Юнит
// ============================================================================

/// Путь юнита.
pub fn unit_path(config_home: &str) -> String {
    format!("{}/{UNIT_NAME}", crate::update_guard::user_unit_dir(config_home))
}

/// Текст юнита. Описание — по-английски и не из каталога: его читает
/// `systemctl`, а не покупатель.
pub fn unit_text(exec_start: &str, program: Option<&str>) -> String {
    let condition = program
        .filter(|p| p.starts_with('/'))
        .map(|p| format!("ConditionPathExists={p}\n"))
        .unwrap_or_default();
    format!(
        "[Unit]\n\
         Description=Rexon: turn AeroThemePlasma off if it crashes KWin or plasmashell right after login\n\
         After={SESSION_TARGET}\n\
         PartOf={SESSION_TARGET}\n\
         # Only in Plasma; Rexon checks this again itself.\n\
         ConditionEnvironment=XDG_CURRENT_DESKTOP=KDE\n\
         {condition}\n\
         [Service]\n\
         Type=simple\n\
         ExecStart={exec_start}\n\
         # The notice process outlives the watch.\n\
         KillMode=process\n\
         TimeoutStopSec=5\n\
         Nice=5\n\n\
         [Install]\n\
         WantedBy={SESSION_TARGET}\n"
    )
}

/// Что сделать с юнитом.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnitChange {
    Nothing,
    Install(String),
    Remove,
}

/// План: `wanted` — те же условия, что у сценария до KWin (Plasma-набор
/// стоит, проверка включена); `exec_start` — строка запуска (None —
/// заводить нечем, снимаем).
pub fn unit_change(wanted: bool, exec_start: Option<&str>, program: Option<&str>, existing: Option<&str>) -> UnitChange {
    match (wanted, exec_start) {
        (true, Some(exec)) => {
            let text = unit_text(exec, program);
            if existing == Some(text.as_str()) {
                UnitChange::Nothing
            } else {
                UnitChange::Install(text)
            }
        }
        _ if existing.is_some() => UnitChange::Remove,
        _ => UnitChange::Nothing,
    }
}

/// `systemctl --user …` по порядку. Включается без `--now`: сторож смотрит
/// первые минуты ВХОДА, а не сеанс, который уже идёт. Снятие: `disable`
/// (файл ещё на месте) → файл удаляется → `daemon-reload`.
pub fn unit_systemctl(change: &UnitChange) -> Vec<Vec<String>> {
    let v = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    match change {
        UnitChange::Nothing => Vec::new(),
        UnitChange::Install(_) => vec![v(&["daemon-reload"]), v(&["enable", UNIT_NAME])],
        UnitChange::Remove => vec![v(&["disable", UNIT_NAME]), v(&["daemon-reload"])],
    }
}

fn systemctl_live(args: &[String]) -> Result<(), String> {
    let mut argv = vec!["systemctl".to_string(), "--user".to_string()];
    argv.extend(args.iter().cloned());
    let r = crate::utils::run(
        &crate::utils::RunSpec {
            argv: argv.clone(),
            output: crate::utils::RunOutput::Capture,
            timeout_secs: Some(30),
            lang_c: true,
            ..Default::default()
        },
        None,
    );
    if r.returncode == Some(0) {
        Ok(())
    } else {
        Err(format!("{}: {}", argv.join(" "), String::from_utf8_lossy(&r.stderr).trim()))
    }
}

/// Поставить или снять юнит на живой машине. Зовёт
/// [`guard::sync_env_script_live`] — одна точка для обоих сторожей.
pub fn sync_unit_live(config_home: &str, wanted: bool, exec_start: Option<&str>, program: Option<&str>) -> Result<UnitChange, String> {
    let path = unit_path(config_home);
    let existing = std::fs::read_to_string(&path).ok();
    let change = unit_change(wanted, exec_start, program, existing.as_deref());
    if change == UnitChange::Nothing {
        return Ok(change);
    }
    if crate::utils::which("systemctl", None).is_none() {
        return Err("systemctl not found".into());
    }
    match &change {
        UnitChange::Install(text) => guard::write_atomic(Path::new(&path), text)?,
        UnitChange::Remove => {}
        UnitChange::Nothing => {}
    }
    for args in unit_systemctl(&change) {
        if change == UnitChange::Remove && args.first().map(String::as_str) == Some("daemon-reload") {
            let _ = std::fs::remove_file(&path);
        }
        systemctl_live(&args)?;
    }
    Ok(change)
}

// ============================================================================
// Живой сторож
// ============================================================================

fn run_capture(argv: Vec<String>, timeout: u64) -> Option<String> {
    let r = crate::utils::run(
        &crate::utils::RunSpec {
            argv,
            output: crate::utils::RunOutput::Capture,
            timeout_secs: Some(timeout),
            lang_c: true,
            ..Default::default()
        },
        None,
    );
    matches!(r.outcome, crate::utils::RunOutcome::Ok | crate::utils::RunOutcome::Failed)
        .then(|| String::from_utf8_lossy(&r.stdout).to_string())
}

fn units_live() -> Vec<UnitState> {
    let units = run_capture(show_argv(), 10).map(|t| parse_show(&t)).unwrap_or_default();
    if !units.is_empty() {
        return units;
    }
    // systemd старше 248 не знает `--timestamp=unix`: без него время
    // начала сеанса неизвестно, но счётчики перезапусков те же.
    let argv: Vec<String> = show_argv().into_iter().filter(|a| a != "--timestamp=unix").collect();
    run_capture(argv, 10).map(|t| parse_show(&t)).unwrap_or_default()
}

fn now_us() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_micros() as u64).unwrap_or(0)
}

/// Набор и версии на живой машине. None — набора нет.
fn aero_live() -> Option<(AeroSet, guard::AeroParts, String)> {
    let db = guard::LocalDb::read(Path::new(guard::PACMAN_LOCAL));
    if !guard::aero_installed(&db) {
        return None;
    }
    let forks = crate::system_plan::filter_plasma_forks(&db.names());
    let files: Vec<(String, Vec<String>)> = forks
        .iter()
        .map(|p| (p.clone(), db.file_list(p).map(|o| crate::aeroshell::package_files(&o)).unwrap_or_default()))
        .collect();
    let parts = guard::aero_parts(&files, &|p| std::fs::read(p).ok());
    let set = AeroSet {
        packages: forks.clone(),
        modules: aero_modules(&files),
        effect_ids: parts.effects.iter().map(|e| e.id.clone()).collect(),
        applet_ids: parts.applets.clone(),
    };
    let mut versions: Vec<String> = ["kwin", "plasma-workspace", "qt6-base"]
        .iter()
        .map(|p| format!("{p}={}", db.version(p).unwrap_or_default()))
        .collect();
    versions.extend(forks.iter().map(|p| format!("{p}={}", db.version(p).unwrap_or_default())));
    Some((set, parts, versions.join(";")))
}

/// Поднять plasmashell после отката (systemd уже сдался).
fn revive_shell_live() {
    let _ = systemctl_live(&["reset-failed".into(), PLASMASHELL_UNIT.into()]);
    let _ = systemctl_live(&["start".into(), "--no-block".into(), PLASMASHELL_UNIT.into()]);
}

/// `rexon --session-watch` — его зовёт юнит. Всегда 0.
pub fn watch_route_live(out: &mut dyn FnMut(&str)) -> i32 {
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    if !desktop.split(':').any(|p| p.eq_ignore_ascii_case("KDE")) {
        out("session-watch: not a Plasma session");
        return 0;
    }
    if !guard::check_enabled_live() {
        out("session-watch: disabled in settings");
        return 0;
    }
    let Some((aero, parts, versions)) = aero_live() else {
        out("session-watch: AeroThemePlasma is not installed");
        return 0;
    };
    let paths = GuardPaths::live();
    let started = std::time::Instant::now();
    let since = session_since_us(&units_live()).unwrap_or_else(|| now_us().saturating_sub(WATCH_SECS * 1_000_000));
    let mut log = |l: &str| {
        crate::log_info!("session-watch", "{l}");
        out(l);
    };
    let mut done: Vec<Side> = Vec::new();
    let mut threshold_at: Option<std::time::Instant> = None;
    let mut findings: Vec<Finding> = Vec::new();
    let mut findings_at: Option<std::time::Instant> = None;
    loop {
        let units = units_live();
        let journal = run_capture(journal_argv(since), 10).map(|t| parse_journal(&t)).unwrap_or_default();
        let obs = observe(&units, &journal);
        let reached = (obs.kwin_failed() && !done.contains(&Side::Kwin)) || (obs.shell_failed() && !done.contains(&Side::Shell));
        if reached && threshold_at.is_none() {
            threshold_at = Some(std::time::Instant::now());
        }
        let final_look = threshold_at.is_some_and(|t| t.elapsed().as_secs() >= LATE_TRACE_SECS);
        if reached {
            // Журнал модулей дороже (pacman -Qo) — не чаще раза в 5 с.
            if findings_at.is_none_or(|t| t.elapsed().as_secs() >= 5) || final_look {
                findings = crate::system_de_exec::plugin_findings_live()
                    .into_iter()
                    .filter(|f| f.failure.time_us == 0 || f.failure.time_us >= since)
                    .collect();
                findings_at = Some(std::time::Instant::now());
            }
            let found = culprits(&obs, &aero, &findings);
            match decide(&obs, &found, &done, &versions, &load_handled(&paths), final_look) {
                WatchDecision::Nothing | WatchDecision::Wait => {}
                WatchDecision::LogOnly { why } => {
                    for w in &why {
                        log(&format!("session-watch: {w}"));
                    }
                    break;
                }
                WatchDecision::Act(action) => {
                    for w in &action.why {
                        log(&format!("session-watch: {w}"));
                    }
                    let dead_now = action.shell == ShellAct::Revive && {
                        let u = units_live();
                        unit(&u, PLASMASHELL_UNIT).is_some_and(|s| s.gave_up() && s.main_pid == 0)
                    };
                    match apply(&paths, &action, &parts, dead_now) {
                        Ok(d) => {
                            log(&format!("session-watch: fallback {d:?}"));
                            if d.effects_off {
                                guard::reconfigure_kwin_live();
                            }
                            if d.revive_shell {
                                revive_shell_live();
                            }
                            if let Some(n) = &d.notice {
                                guard::queue_notice_live(&paths, n, &mut log);
                            }
                        }
                        Err(e) => log(&format!("session-watch: fallback failed: {e}")),
                    }
                    if action.kwin {
                        done.push(Side::Kwin);
                    }
                    if action.shell != ShellAct::None {
                        done.push(Side::Shell);
                    }
                    threshold_at = None;
                    if done.len() >= 2 {
                        break;
                    }
                }
            }
        }
        if threshold_at.is_none() && started.elapsed().as_secs() >= WATCH_SECS {
            break;
        }
        std::thread::sleep(std::time::Duration::from_secs(POLL_SECS));
    }
    0
}

// ============================================================================
// Удаление Rexon
// ============================================================================

/// `systemctl --user` для снятия юнита при удалении Rexon (файл удаляет
/// вызывающий между шагами, как у [`unit_systemctl`]).
pub fn uninstall_systemctl() -> Vec<Vec<String>> {
    unit_systemctl(&UnitChange::Remove)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin_journal::{Cause, Failure, Origin};
    use crate::session_guard::{AeroParts, EffectModule};

    struct Tmp(PathBuf);
    impl Tmp {
        fn new(tag: &str) -> Self {
            let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
            let p = std::env::temp_dir().join(format!("rexon-session-watch-{tag}-{}-{n}", std::process::id()));
            std::fs::create_dir_all(&p).unwrap();
            Tmp(p)
        }
        fn paths(&self) -> GuardPaths {
            GuardPaths::new(self.0.join("config"), self.0.join("state/rexon/session-guard"))
        }
    }
    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    const SHOW: &str = "Id=plasma-plasmashell.service\nActiveState=failed\nResult=start-limit-hit\nNRestarts=3\nMainPID=0\nInactiveExitTimestamp=@1792900010\n\n\
Id=plasma-kwin_wayland.service\nActiveState=active\nResult=success\nNRestarts=0\nMainPID=1234\nInactiveExitTimestamp=@1792900005\n\n\
Id=plasma-kwin_x11.service\nActiveState=inactive\nResult=success\nNRestarts=0\nMainPID=0\nInactiveExitTimestamp=\n\n\
Id=graphical-session.target\nActiveState=active\nResult=success\nNRestarts=0\nMainPID=0\nInactiveExitTimestamp=@1792900004\n";

    const TRACE: &str = "Process 4321 (kwin_wayland) of user 1000 dumped core.\n\nModule smodglow.so without build-id.\n\
Stack trace of thread 4321:\n#0  0x00007f n/a (libc.so.6 + 0x9a)\n#1  0x00007f _ZN4KWin6Effect (smodglow.so + 0x1d2)\n\
#2  0x00007f n/a (libkwin.so.6 + 0x33)\n\nStack trace of thread 4400:\n#0  0x00007f n/a (aeroblur.so + 0x1)\n";

    fn dump(comm: &str, message: &str, t: u64) -> String {
        serde_json::json!({
            "SYSLOG_IDENTIFIER": "systemd-coredump",
            "COREDUMP_COMM": comm,
            "MESSAGE": message,
            "__REALTIME_TIMESTAMP": t.to_string(),
            "_COMM": "systemd-coredum",
        })
        .to_string()
    }

    fn kwin_line(pid: u64) -> String {
        serde_json::json!({"_COMM": "kwin_wayland", "_PID": pid.to_string(), "MESSAGE": "kwin_core: hi", "__REALTIME_TIMESTAMP": "1"}).to_string()
    }

    fn aero() -> AeroSet {
        AeroSet {
            packages: vec!["aeroshell-kwin-components-git".into(), "aerothemeplasma-desktop-git".into()],
            modules: vec!["smodglow.so".into(), "aeroblur.so".into(), "libseventasks.so".into()],
            effect_ids: vec!["smodglow".into(), "aeroblur".into()],
            applet_ids: vec!["io.gitgud.wackyideas.seventasks".into()],
        }
    }

    fn parts() -> AeroParts {
        AeroParts {
            effects: vec![
                EffectModule { file_name: "smodglow.so".into(), id: "smodglow".into() },
                EffectModule { file_name: "aeroblur.so".into(), id: "aeroblur".into() },
            ],
            applets: vec!["io.gitgud.wackyideas.seventasks".into()],
        }
    }

    fn finding(kind: PluginKind, id: &str, pkg: Option<&str>) -> Finding {
        Finding {
            failure: Failure { kind, id: id.into(), cause: Cause::LoadFailed { detail: "undefined symbol: x".into() }, paths: vec![], time_us: 5 },
            name: format!("Name of {id}"),
            origin: match pkg {
                Some(p) => Origin::Package { name: p.into(), foreign: true },
                None => Origin::Unknown,
            },
            file: None,
        }
    }

    fn crash(comm: &str, modules: &[&str]) -> Crash {
        Crash { comm: comm.into(), time_us: 1, modules: modules.iter().map(|s| s.to_string()).collect() }
    }

    // ── разбор ──

    #[test]
    fn show_parses_blocks_and_unix_stamps() {
        let u = parse_show(SHOW);
        assert_eq!(u.len(), 4);
        let shell = unit(&u, PLASMASHELL_UNIT).unwrap();
        assert!(shell.gave_up() && !shell.alive());
        assert_eq!(shell.n_restarts, 3);
        assert_eq!(unit(&u, KWIN_WAYLAND_UNIT).unwrap().main_pid, 1234);
        assert_eq!(unit(&u, KWIN_X11_UNIT).unwrap().inactive_exit_us, None);
        assert_eq!(session_since_us(&u), Some((1_792_900_004 - SINCE_MARGIN_SECS) * 1_000_000));
        // Старый systemd без --timestamp=unix: время не знаем.
        assert_eq!(parse_show("Id=x.target\nInactiveExitTimestamp=Sun 2026-10-25 06:00:00 CET\n")[0].inactive_exit_us, None);
        assert!(parse_show("").is_empty());
    }

    #[test]
    fn show_and_journal_commands() {
        let s = show_argv();
        assert_eq!(&s[..4], &["systemctl", "--user", "show", "--timestamp=unix"]);
        assert!(s.contains(&PLASMASHELL_UNIT.to_string()) && s.contains(&KWIN_X11_UNIT.to_string()));
        let j = journal_argv(1_792_900_004_123_456);
        assert!(j.contains(&"--since=@1792900004".to_string()), "{j:?}");
        assert!(j.contains(&"--all".to_string()), "без --all длинный MESSAGE дампа приходит null");
        let plus = j.iter().position(|a| a == "+").unwrap();
        assert!(j[plus - 1].starts_with("_COMM=") && j[plus + 1] == "SYSLOG_IDENTIFIER=systemd-coredump");
    }

    #[test]
    fn crash_modules_take_only_the_crashing_thread() {
        assert_eq!(crash_modules(TRACE), vec!["libc.so.6", "smodglow.so", "libkwin.so.6"]);
        assert!(crash_modules("Process 1 (kwin_wayland) of user 1000 dumped core.").is_empty());
    }

    #[test]
    fn journal_counts_dumps_and_kwin_processes() {
        let text = [
            dump("kwin_wayland", TRACE, 10),
            dump("cinnamon", TRACE, 11),
            kwin_line(100),
            kwin_line(100),
            kwin_line(101),
            "not json".to_string(),
            serde_json::json!({"SYSLOG_IDENTIFIER": "systemd-coredump", "COREDUMP_COMM": "plasmashell",
                "MESSAGE": TRACE.bytes().map(|b| b as u64).collect::<Vec<_>>(), "__REALTIME_TIMESTAMP": "12"})
            .to_string(),
        ]
        .join("\n");
        let f = parse_journal(&text);
        assert_eq!(f.crashes.len(), 2, "чужой процесс (cinnamon) не в счёт");
        assert_eq!(f.crashes[0].comm, "kwin_wayland");
        assert_eq!(f.crashes[1].modules[1], "smodglow.so", "MESSAGE массивом байт тоже читается");
        assert_eq!(f.kwin_pids.len(), 2);
    }

    // ── порог ──

    #[test]
    fn kwin_crash_count_takes_the_largest_source_not_the_sum() {
        let mut j = JournalFacts { crashes: vec![crash("kwin_wayland", &[])], ..Default::default() };
        j.kwin_pids.extend([1, 2, 3]);
        let o = observe(&[], &j);
        assert_eq!(o.kwin_crashes, 2, "дамп и новый процесс — одно и то же падение");
        assert!(o.kwin_failed());
        let o = observe(&[], &JournalFacts { crashes: vec![crash("kwin_wayland", &[])], ..Default::default() });
        assert!(!o.kwin_failed(), "одно падение — ещё не повод");
        let x11 = UnitState { id: KWIN_X11_UNIT.into(), n_restarts: 2, ..Default::default() };
        assert!(observe(&[x11], &JournalFacts::default()).kwin_failed(), "X11: перезапуски systemd");
    }

    #[test]
    fn shell_threshold_is_two_restarts_or_giving_up() {
        let s = |n, res: &str, st: &str| UnitState { id: PLASMASHELL_UNIT.into(), n_restarts: n, result: res.into(), active_state: st.into(), ..Default::default() };
        assert!(!observe(&[s(1, "success", "active")], &JournalFacts::default()).shell_failed());
        let o = observe(&[s(2, "success", "active")], &JournalFacts::default());
        assert!(o.shell_failed() && o.shell_alive);
        let o = observe(&[s(0, "start-limit-hit", "failed")], &JournalFacts::default());
        assert!(o.shell_failed() && o.shell_gave_up && !o.shell_alive);
    }

    // ── кто виноват ──

    #[test]
    fn culprit_from_trace_and_from_journal_findings() {
        let obs = Observation { crashes: vec![crash("kwin_wayland", &["libc.so.6", "smodglow.so"]), crash("plasmashell", &["libQt6Core.so.6"])], ..Default::default() };
        let c = culprits(&obs, &aero(), &[]);
        assert_eq!(c.len(), 1);
        assert_eq!((c[0].side, c[0].name.as_str()), (Side::Kwin, "smodglow.so"));

        let f = [
            finding(PluginKind::Applet, "io.gitgud.wackyideas.seventasks", None),
            finding(PluginKind::Effect, "othereffect", Some("kwin-effects-forceblur")),
            finding(PluginKind::Effect, "x", Some("aeroshell-kwin-components-git")),
        ];
        let c = culprits(&Observation::default(), &aero(), &f);
        assert_eq!(c.iter().map(|c| (c.side, c.name.clone())).collect::<Vec<_>>(), vec![
            (Side::Shell, "Name of io.gitgud.wackyideas.seventasks".to_string()),
            (Side::Kwin, "Name of x".to_string()),
        ], "чужой эффект не виноват");
    }

    #[test]
    fn libplasma_fork_and_versioned_names() {
        let files = vec![
            ("aeroshell-libplasma-git".to_string(), vec!["/usr/lib/libPlasma.so.6".to_string()]),
            ("aeroshell-kwin-components-git".to_string(), vec!["/usr/lib/qt6/plugins/kwin/effects/plugins/smodglow.so".to_string(), "/usr/lib/libaero.so.1.2".to_string(), "/usr/share/x.qml".to_string()]),
        ];
        assert_eq!(aero_modules(&files), vec!["smodglow.so", "libaero.so.1.2"]);
        let set = AeroSet { modules: aero_modules(&files), ..Default::default() };
        assert!(is_aero_module("libaero.so.1", &set));
        assert!(!is_aero_module("libPlasma.so.6", &set), "форк libplasma по имени не опознать");
    }

    // ── решение ──

    fn kwin_obs() -> Observation {
        Observation { kwin_crashes: 2, shell_alive: true, crashes: vec![crash("kwin_wayland", &["smodglow.so"])], ..Default::default() }
    }

    #[test]
    fn nothing_below_threshold_wait_then_log_only_for_strangers() {
        let one = Observation { kwin_crashes: 1, ..Default::default() };
        assert_eq!(decide(&one, &[], &[], "v", &[], true), WatchDecision::Nothing);
        let o = Observation { kwin_crashes: 3, crashes: vec![crash("kwin_wayland", &["libnvidia-glcore.so.580"])], ..Default::default() };
        let found = culprits(&o, &aero(), &[]);
        assert_eq!(decide(&o, &found, &[], "v", &[], false), WatchDecision::Wait, "стек мог ещё не прийти");
        match decide(&o, &found, &[], "v", &[], true) {
            WatchDecision::LogOnly { why } => assert!(why.iter().any(|w| w.contains("libnvidia")), "{why:?}"),
            d => panic!("{d:?}"),
        }
    }

    #[test]
    fn kwin_culprit_turns_effects_off_now_without_touching_layout() {
        let o = kwin_obs();
        let found = culprits(&o, &aero(), &[]);
        match decide(&o, &found, &[], "v", &[], false) {
            WatchDecision::Act(a) => {
                assert!(a.kwin);
                assert_eq!(a.shell, ShellAct::None);
                assert_eq!(a.reason, "“smodglow.so” made KWin crash");
            }
            d => panic!("{d:?}"),
        }
    }

    #[test]
    fn shell_alive_waits_for_next_login_dead_is_revived() {
        let f = [finding(PluginKind::Applet, "io.gitgud.wackyideas.seventasks", None)];
        let alive = Observation { shell_restarts: 2, shell_alive: true, ..Default::default() };
        let found = culprits(&alive, &aero(), &f);
        assert!(matches!(decide(&alive, &found, &[], "v", &[], false), WatchDecision::Act(Action { shell: ShellAct::NextLogin, kwin: false, .. })));
        let dead = Observation { shell_restarts: 3, shell_gave_up: true, ..Default::default() };
        assert!(matches!(decide(&dead, &found, &[], "v", &[], false), WatchDecision::Act(Action { shell: ShellAct::Revive, .. })));
        // Перезапускается прямо сейчас: ни жив, ни сдался — подождать.
        let between = Observation { shell_restarts: 2, ..Default::default() };
        assert_eq!(decide(&between, &found, &[], "v", &[], false), WatchDecision::Wait);
        assert!(matches!(decide(&between, &found, &[], "v", &[], true), WatchDecision::Act(Action { shell: ShellAct::NextLogin, .. })));
    }

    #[test]
    fn same_fingerprint_is_not_handled_twice_new_versions_are() {
        let o = kwin_obs();
        let found = culprits(&o, &aero(), &[]);
        let WatchDecision::Act(a) = decide(&o, &found, &[], "kwin=6.8.1", &[], false) else { panic!() };
        assert!(matches!(decide(&o, &found, &[], "kwin=6.8.1", &[a.fingerprint.clone()], false), WatchDecision::LogOnly { .. }));
        let WatchDecision::Act(b) = decide(&o, &found, &[], "kwin=6.8.2", &[a.fingerprint.clone()], false) else { panic!() };
        assert_ne!(a.fingerprint, b.fingerprint);
        // Эту сторону сторож уже откатил в этот вход.
        assert_eq!(decide(&o, &found, &[Side::Kwin], "kwin=6.8.1", &[], false), WatchDecision::Nothing);
    }

    // ── исполнение ──

    fn put(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    const APPLETSRC: &str = "[Containments][1]\nformfactor=0\nlocation=0\nplugin=org.kde.desktopcontainment\n\n\
[Containments][2]\nformfactor=2\nlocation=4\nplugin=org.kde.panel\n\n[Containments][2][Applets][3]\nplugin=io.gitgud.wackyideas.seventasks\n";

    fn action(kwin: bool, shell: ShellAct) -> Action {
        Action { kwin, shell, reason: "“smodglow.so” made KWin crash".into(), why: vec!["w".into()], fingerprint: "fp1".into() }
    }

    #[test]
    fn apply_kwin_turns_effects_off_and_remembers() {
        let t = Tmp::new("kwin");
        let p = t.paths();
        put(&p.kwinrc(), "[Plugins]\nsmodglowEnabled=true\n");
        put(&p.appletsrc(), APPLETSRC);
        let d = apply(&p, &action(true, ShellAct::None), &parts(), false).unwrap();
        assert!(d.effects_off && !d.layout_replaced && !d.requested);
        let kwinrc = std::fs::read_to_string(p.kwinrc()).unwrap();
        assert!(kwinrc.contains("smodglowEnabled=false") && kwinrc.contains("aeroblurEnabled=false"), "{kwinrc}");
        assert_eq!(std::fs::read_to_string(p.appletsrc()).unwrap(), APPLETSRC, "раскладка не тронута");
        assert!(matches!(guard::load_saved(&p).unwrap().cause, FallbackCause::Requested { .. }));
        assert_eq!(load_handled(&p), vec!["fp1"]);
        assert!(matches!(d.notice, Some(PendingNotice::TurnedOff { effects: true, layout: false, .. })));
    }

    #[test]
    fn apply_shell_alive_only_requests_next_login() {
        let t = Tmp::new("next");
        let p = t.paths();
        put(&p.appletsrc(), APPLETSRC);
        let d = apply(&p, &action(false, ShellAct::NextLogin), &parts(), false).unwrap();
        assert!(d.requested && !d.layout_replaced);
        assert!(p.request_file().exists());
        assert_eq!(std::fs::read_to_string(p.appletsrc()).unwrap(), APPLETSRC);
        assert!(matches!(d.notice, Some(PendingNotice::NextLogin { .. })));
    }

    #[test]
    fn apply_shell_dead_replaces_layout_but_rechecks_first() {
        let t = Tmp::new("dead");
        let p = t.paths();
        put(&p.appletsrc(), APPLETSRC);
        // Перепроверка: plasmashell уже поднялся — только отметка.
        let d = apply(&p, &action(false, ShellAct::Revive), &parts(), false).unwrap();
        assert!(d.requested && !d.revive_shell);
        let _ = std::fs::remove_file(p.request_file());
        let d = apply(&p, &action(false, ShellAct::Revive), &parts(), true).unwrap();
        assert!(d.layout_replaced && d.revive_shell && d.effects_off);
        assert!(!std::fs::read_to_string(p.appletsrc()).unwrap().contains("seventasks"));
        assert_eq!(std::fs::read_to_string(p.saved_appletsrc()).unwrap(), APPLETSRC, "копия раскладки человека");
        assert!(!p.request_file().exists());
    }

    #[test]
    fn handled_list_is_bounded() {
        let t = Tmp::new("handled");
        let p = t.paths();
        for i in 0..30 {
            remember_handled(&p, &format!("fp{i}")).unwrap();
        }
        remember_handled(&p, "fp29").unwrap();
        let l = load_handled(&p);
        assert_eq!(l.len(), HANDLED_KEEP);
        assert_eq!(l.last().map(String::as_str), Some("fp29"));
    }

    // ── юнит ──

    #[test]
    fn unit_text_and_plan() {
        let text = unit_text("/usr/bin/rexon --session-watch", Some("/usr/bin/rexon"));
        for needle in [
            "After=graphical-session.target",
            "PartOf=graphical-session.target",
            "WantedBy=graphical-session.target",
            "ConditionEnvironment=XDG_CURRENT_DESKTOP=KDE",
            "ConditionPathExists=/usr/bin/rexon",
            "ExecStart=/usr/bin/rexon --session-watch",
            "KillMode=process",
        ] {
            assert!(text.contains(needle), "{needle}\n{text}");
        }
        assert!(!text.contains("--now"));
        assert_eq!(unit_change(true, Some("/usr/bin/rexon --session-watch"), Some("/usr/bin/rexon"), Some(&text)), UnitChange::Nothing);
        assert!(matches!(unit_change(true, Some("/x --session-watch"), None, Some(&text)), UnitChange::Install(_)));
        assert_eq!(unit_change(false, Some("/x"), None, Some(&text)), UnitChange::Remove);
        assert_eq!(unit_change(true, None, None, Some(&text)), UnitChange::Remove, "заводить нечем — снять");
        assert_eq!(unit_change(false, None, None, None), UnitChange::Nothing);
        assert_eq!(unit_systemctl(&UnitChange::Install(text)), vec![vec!["daemon-reload".to_string()], vec!["enable".to_string(), UNIT_NAME.to_string()]]);
        assert_eq!(uninstall_systemctl()[0], vec!["disable".to_string(), UNIT_NAME.to_string()]);
        assert_eq!(unit_path("/home/u/.config/"), format!("/home/u/.config/systemd/user/{UNIT_NAME}"));
    }
}
