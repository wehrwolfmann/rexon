//! Защита от обновления, которое поставит набор пакетов вразнобой.
//!
//! ЗАЧЕМ. 07.10.2026 `pacman -Syu` на CachyOS поставил `qt6-base` 6.12.0 из
//! Arch `extra`, а `qt6-declarative` остался 6.11.2: CachyOS кладёт свои
//! сборки части модулей Qt в репозиторий, который pacman смотрит раньше
//! `extra`, и выкладывает их позже. `libQt6Qml` 6.11.2 требует приватный
//! интерфейс ровно той версии Qt, под которую собран
//! (`undefined symbol … QtPrivate_6_11_2`), — после перезагрузки экран входа
//! падал, и человек видел чёрный экран. Это повторяющийся класс (CachyOS
//! distribution #453, #549, #605; с другой библиотекой — #604:
//! `gdk-pixbuf2` 2.44.8 при старом `glycin`, Firefox не запускался), и
//! узнать о нём можно ДО обновления: `checkupdates` говорит, что придёт.
//!
//! Что делает модуль:
//!   * правила наборов — ДАННЫМИ ([`PACKAGE_SETS`]): какие пакеты должны
//!     идти одной версией и с какой точностью; новый набор — новая запись;
//!   * сверка «установленное + ждущее обновление» ([`evaluate`]): разойдётся
//!     ли набор после обновления, разошёлся ли он уже и чинит ли его то, что
//!     ждёт;
//!   * пакет-сирота: член набора, которого больше нет в репозиториях
//!     (`pacman -Qm`), не «отстаёт» — он не обновится никогда (25.10.2026:
//!     `kwin-x11` 6.7.5 при Plasma 6.8.1, сеанс X11 из Plasma убран). Такой
//!     пакет из сверки набора выводится, о нём — своя находка
//!     [`Stage::Orphaned`] с советом удалить, а не «подождать»;
//!   * тексты для человека, отпечаток состояния («об этом уже говорили»);
//!   * пользовательские юниты фоновой проверки — общие для всех проверок
//!     «до/после обновления» ([`watch_units`]); в них же идёт и сверка
//!     AeroThemePlasma, своих юнитов у неё больше нет (см. ниже).
//!
//! ⛔ Только предупреждение: никаких IgnorePkg, хуков pacman, удержания
//! пакетов. Сверка пароля не спрашивает никогда. Действия — две кнопки окна
//! через сеанс прав окна: удаление сироты ([`remove_orphans_live`]) и
//! возврат прежних версий набора из кэша pacman ([`rollback_live`]); «сирота
//! ли это» и «откат ли это ровно набора находки» root-сторона проверяет
//! сама. Команд в окне и уведомлении нет; консоли спасения при упавшем
//! экране входа команды остаются ([`orphan_advice`], откат —
//! [`crate::login_rescue`]).

use std::collections::{BTreeMap, BTreeSet};

use crate::engine_update::ver_tuple;
use crate::i18n::{fill, n_, tr, trc, trf};
use crate::system::PkgFamily;
use crate::system_de_exec::DeOpsIo;
use crate::system_exec::SystemIo;

// ══════════════════════════════════════════════════════════════════════════
// Что придёт с обновлением: `checkupdates`
// ══════════════════════════════════════════════════════════════════════════
//
// `checkupdates` (pacman-contrib) синхронизирует СВОЮ копию базы
// репозиториев во временном каталоге, без прав администратора и не трогая
// базу системы. Нет его — честно «не знаем»: читать системную базу
// (`pacman -Si`) значит видеть её такой, какой она была при последнем
// `-Sy`, а синхронизировать её саму — root.

/// Программа, которая безопасно (без root) говорит, что придёт с обновлением.
pub const PENDING_UPDATES_BINARY: &str = "checkupdates";

/// Таймаут `checkupdates`, секунды: он скачивает базы репозиториев.
pub const PENDING_UPDATES_TIMEOUT_SECS: u64 = 180;

/// Одно ждущее обновление: пакет, версия сейчас, версия в репозиториях.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingUpdate {
    pub package: String,
    pub installed: String,
    pub available: String,
}

/// Разбор вывода `checkupdates`: строки `имя старая -> новая`. Цвет
/// (на случай, если его всё же включили) и строки иной формы пропускаются.
pub fn parse_pending_updates(text: &str) -> Vec<PendingUpdate> {
    let plain = strip_ansi(text);
    plain
        .lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            let (package, installed, arrow, available) =
                (words.next()?, words.next()?, words.next()?, words.next()?);
            (arrow == "->").then(|| PendingUpdate {
                package: package.to_string(),
                installed: installed.to_string(),
                available: available.to_string(),
            })
        })
        .collect()
}

/// Убрать управляющие последовательности цвета терминала.
fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

/// Что придёт с обновлением системы: разобранный вывод `checkupdates`.
/// None — не Arch, программы нет или она не смогла (нет сети) — честно
/// «не знаем». Код 2 у `checkupdates` — «обновлений нет».
pub fn pending_updates<IO: SystemIo + ?Sized>(io: &IO, family: PkgFamily) -> Option<Vec<PendingUpdate>> {
    if family != PkgFamily::Arch || !io.which(PENDING_UPDATES_BINARY) {
        return None;
    }
    let r = io.run(&[PENDING_UPDATES_BINARY.to_string()], Some(PENDING_UPDATES_TIMEOUT_SECS), true);
    match r.rc() {
        Some(0) => Some(parse_pending_updates(&r.stdout)),
        Some(2) => Some(Vec::new()),
        _ => None,
    }
}

/// Каталог базы, которую синхронизирует `checkupdates`: `CHECKUPDATES_DB`
/// либо `${TMPDIR:-/tmp}/checkup-db-${UID}/` — так считает он сам.
pub fn checkupdates_db(env_db: &str, tmpdir: &str, uid: u32) -> String {
    if !env_db.trim().is_empty() {
        return env_db.trim().to_string();
    }
    let tmp = if tmpdir.trim().is_empty() { "/tmp" } else { tmpdir.trim() };
    format!("{}/checkup-db-{uid}/", tmp.trim_end_matches('/'))
}

/// Разбор `pacman -Q`: `(имя, версия)`.
pub fn parse_installed(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            Some((words.next()?.to_string(), words.next()?.to_string()))
        })
        .collect()
}

/// Разбор `pacman -Qg <группы>`: `(группа, пакет)`.
pub fn parse_groups(text: &str) -> Vec<(String, String)> {
    parse_installed(text)
}

/// Разбор `pacman -Sl` (под `LANG=C`): все версии каждого пакета во всех
/// включённых репозиториях — `имя → [версия…]`.
pub fn parse_sync_list(text: &str) -> BTreeMap<String, Vec<String>> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for line in text.lines() {
        let mut words = line.split_whitespace();
        let (Some(_repo), Some(name), Some(version)) = (words.next(), words.next(), words.next()) else {
            continue;
        };
        out.entry(name.to_string()).or_default().push(version.to_string());
    }
    out
}

/// Разбор `pacman -Qmq`: пакеты, которых нет ни в одном включённом
/// репозитории (сторонние из AUR и ушедшие из репозиториев).
pub fn parse_foreign(text: &str) -> BTreeSet<String> {
    text.lines().filter_map(|l| l.split_whitespace().next()).map(str::to_string).collect()
}

/// Пакеты вне репозиториев: `pacman -Qmq` (только чтение, без сети — по
/// синхронизированным базам). `dbpath` — база `checkupdates` (в ней
/// свежие базы репозиториев и ссылка `local` на установленное), без неё —
/// база системы. None — не узнали (тогда сирот не судим).
pub fn foreign_packages<IO: SystemIo + ?Sized>(io: &IO, dbpath: Option<&str>) -> Option<BTreeSet<String>> {
    let mut argv = vec!["pacman".to_string(), "-Qmq".to_string()];
    if let Some(db) = dbpath {
        argv.push("--dbpath".to_string());
        argv.push(db.to_string());
    }
    let r = io.run(&argv, Some(60), true);
    match r.rc() {
        Some(0) => Some(parse_foreign(&r.stdout)),
        // Код 1 без вывода — сторонних пакетов нет вовсе.
        Some(1) if r.stdout.trim().is_empty() && r.stderr.trim().is_empty() => Some(BTreeSet::new()),
        _ => None,
    }
}

/// Версия без эпохи и выпуска пакета: `1:6.30.0-1.1` → `6.30.0`.
pub fn upstream_version(pacman_version: &str) -> String {
    crate::aeroshell::upstream_version(pacman_version)
}

// ══════════════════════════════════════════════════════════════════════════
// Правила наборов — данные
// ══════════════════════════════════════════════════════════════════════════

/// С какой точностью версии набора обязаны совпадать.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Precision {
    /// Только первое число (`51` у GNOME).
    Major,
    /// Два числа (`6.30` у KDE Frameworks).
    Minor,
    /// Три числа (`6.12.0` у Qt).
    Patch,
}

impl Precision {
    fn parts(self) -> usize {
        match self {
            Precision::Major => 1,
            Precision::Minor => 2,
            Precision::Patch => 3,
        }
    }
}

/// Кто входит в набор.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Members {
    /// Все установленные пакеты с таким началом имени.
    Prefix(&'static str),
    /// Установленные члены группы pacman (`pacman -Qg`).
    Group(&'static str),
    /// Ровно эти пакеты (из установленных).
    Names(&'static [&'static str]),
}

/// Как набор должен двигаться.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rule {
    /// Все члены — той же версии, что `anchor`, с точностью `precision`.
    /// `may_lag` — члены, которым можно отставать (но не обгонять).
    /// Члены `Prefix`/`Group` с другим первым числом версии в набор не
    /// входят: это чужая схема нумерации (сторонний `qt6-…` из AUR).
    SameVersion {
        anchor: &'static str,
        precision: Precision,
        may_lag: &'static [&'static str],
    },
    /// Номера разные, но двигаться обязаны вместе: один член обновляется,
    /// а у другого в репозиториях уже есть новее, чем ставит pacman
    /// (репозиторий с приоритетом держит старую сборку), — разойдутся.
    MoveTogether,
}

/// Чем грозит разошедшийся набор.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    /// Экран входа / рабочий стол после перезагрузки.
    LoginScreen,
    /// Программы, которые пользуются набором.
    Programs,
}

/// Один набор.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackageSet {
    /// Опознаватель для отпечатка.
    pub id: &'static str,
    /// Имя набора для человека — собственное имя, без перевода.
    pub name: &'static str,
    pub members: Members,
    pub rule: Rule,
    pub effect: Effect,
}

/// Правила наборов. Новый набор — новая запись, кода не нужно.
///
/// * **Qt** — до третьего числа: модули Qt пользуются приватным интерфейсом
///   `qt6-base`, привязанным к ТОЧНОЙ версии (`QtPrivate_6_12_0`; замер
///   08.10.2026 `objdump -T`: 16 пакетов `qt6-*` требуют этот знак). Исключение
///   — `qt6-webengine`: его библиотеки приватным интерфейсом с такой
///   привязкой не пользуются, и в Arch он штатно отстаёт (сейчас 6.11.2 при
///   `qt6-base` 6.12.0, `ldd -r` чисто). Отставать ему можно, обгонять — нет.
/// * **KDE Frameworks** и **Plasma** — до второго числа (группы pacman
///   `kf6` и `plasma`: точечные выпуски отдельных пакетов — норма).
/// * **GNOME** — `gnome-shell` работает только со своим `mutter` (номер
///   библиотеки = первое число).
/// * **gdk-pixbuf2 + glycin** — случай CachyOS #604: номера разные, двигаться
///   обязаны вместе.
pub const PACKAGE_SETS: &[PackageSet] = &[
    PackageSet {
        id: "qt6",
        name: "Qt",
        members: Members::Prefix("qt6-"),
        rule: Rule::SameVersion { anchor: "qt6-base", precision: Precision::Patch, may_lag: &["qt6-webengine"] },
        effect: Effect::LoginScreen,
    },
    PackageSet {
        id: "kf6",
        name: "KDE Frameworks",
        members: Members::Group("kf6"),
        rule: Rule::SameVersion { anchor: "kcoreaddons", precision: Precision::Minor, may_lag: &[] },
        effect: Effect::LoginScreen,
    },
    PackageSet {
        id: "plasma",
        name: "Plasma",
        members: Members::Group("plasma"),
        rule: Rule::SameVersion { anchor: "plasma-workspace", precision: Precision::Minor, may_lag: &[] },
        effect: Effect::LoginScreen,
    },
    PackageSet {
        id: "gnome",
        name: "GNOME",
        members: Members::Names(&["mutter", "gnome-shell"]),
        rule: Rule::SameVersion { anchor: "mutter", precision: Precision::Major, may_lag: &[] },
        effect: Effect::LoginScreen,
    },
    PackageSet {
        id: "pixbuf",
        name: "GTK",
        members: Members::Names(&["gdk-pixbuf2", "glycin", "glycin-gtk4"]),
        rule: Rule::MoveTogether,
        effect: Effect::Programs,
    },
];

/// Группы pacman, которые нужно спросить (`pacman -Qg …`).
pub fn groups_needed(sets: &[PackageSet]) -> Vec<&'static str> {
    sets.iter()
        .filter_map(|s| match s.members {
            Members::Group(g) => Some(g),
            _ => None,
        })
        .collect()
}

// ══════════════════════════════════════════════════════════════════════════
// Сверка
// ══════════════════════════════════════════════════════════════════════════

/// Что видно на машине: установленное, группы, ждущее, версии в репозиториях.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Survey {
    /// `pacman -Q`.
    pub installed: Vec<(String, String)>,
    /// `pacman -Qg`: `(группа, пакет)`.
    pub groups: Vec<(String, String)>,
    /// `checkupdates`.
    pub pending: Vec<PendingUpdate>,
    /// `pacman -Sl` по базе `checkupdates`: все версии во всех репозиториях.
    /// None — не спрашивали либо не удалось (тогда «двигаться вместе» не
    /// судим: без этого ложная тревога на каждое обновление gdk-pixbuf2).
    pub repo: Option<BTreeMap<String, Vec<String>>>,
    /// `pacman -Qmq`: пакеты, которых нет в репозиториях. None — не
    /// спрашивали либо не удалось: тогда такие пакеты сверяются как все
    /// (прежнее поведение — «вразнобой»).
    pub foreign: Option<BTreeSet<String>>,
}

/// Где набор разошёлся.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// Сейчас всё сходится, обновление разведёт: «не обновляйтесь».
    Hold,
    /// Уже разошёлся, и обновление не чинит: «не перезагружайтесь».
    DontReboot,
    /// Уже разошёлся, а ждущее обновление чинит: «обновитесь ещё раз».
    UpdateNow,
    /// Член набора ушёл из репозиториев и остался от прежней версии: он не
    /// обновится никогда — «удалите, если не пользуетесь». Одна находка —
    /// один такой пакет (`odd[0]`).
    Orphaned,
}

impl Stage {
    fn key(self) -> &'static str {
        match self {
            Stage::Hold => "hold",
            Stage::DontReboot => "noreboot",
            Stage::UpdateNow => "update",
            Stage::Orphaned => "orphan",
        }
    }
}

/// Куда откатить пакет: точная версия (она стоит сейчас) либо любая из
/// кэша с такими первыми числами.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RollbackTarget {
    Exact(String),
    Series(Precision, Vec<u128>),
}

/// Находка по одному набору.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub set: PackageSet,
    pub stage: Stage,
    /// Ведущий пакет и его версия (без эпохи и выпуска) в разошедшемся
    /// состоянии: опорный пакет либо (для «двигаться вместе») обновляемый.
    pub lead: (String, String),
    /// Разошедшиеся с ним: имя, версия, и (для «двигаться вместе») версия,
    /// которая уже есть в другом репозитории.
    pub odd: Vec<(String, String, Option<String>)>,
    /// Что вернуть из кэша, чтобы набор снова сошёлся.
    pub rollback: Vec<(String, RollbackTarget)>,
}

fn key_of(version: &str, precision: Precision) -> Vec<u128> {
    ver_tuple(&upstream_version(version)).into_iter().take(precision.parts()).collect()
}

fn major_of(version: &str) -> Option<u128> {
    ver_tuple(&upstream_version(version)).first().copied()
}

/// Члены набора среди установленного.
fn members_of<'a>(set: &PackageSet, survey: &'a Survey) -> Vec<&'a (String, String)> {
    let in_group = |group: &str, name: &str| survey.groups.iter().any(|(g, p)| g == group && p == name);
    survey
        .installed
        .iter()
        .filter(|(name, _)| match set.members {
            Members::Prefix(p) => name.starts_with(p),
            Members::Group(g) => in_group(g, name),
            Members::Names(list) => list.contains(&name.as_str()),
        })
        .collect()
}

/// Сверить все наборы. Пусто — говорить не о чем.
pub fn evaluate(survey: &Survey, sets: &[PackageSet]) -> Vec<Finding> {
    sets.iter().flat_map(|set| evaluate_set(set, survey)).collect()
}

fn evaluate_set(set: &PackageSet, survey: &Survey) -> Vec<Finding> {
    match set.rule {
        Rule::SameVersion { anchor, precision, may_lag } => same_version(set, survey, anchor, precision, may_lag),
        Rule::MoveTogether => move_together(set, survey).into_iter().collect(),
    }
}

/// Разошедшиеся с опорным пакетом в одном состоянии (`version(имя)`).
fn mismatched(
    names: &[&str],
    anchor: &str,
    precision: Precision,
    may_lag: &[&str],
    version: &dyn Fn(&str) -> String,
) -> Vec<String> {
    let anchor_key = key_of(&version(anchor), precision);
    names
        .iter()
        .filter(|n| **n != anchor)
        .filter(|n| {
            let k = key_of(&version(n), precision);
            if may_lag.contains(n) {
                k > anchor_key
            } else {
                k != anchor_key
            }
        })
        .map(|n| n.to_string())
        .collect()
}

fn same_version(
    set: &PackageSet,
    survey: &Survey,
    anchor: &str,
    precision: Precision,
    may_lag: &[&str],
) -> Vec<Finding> {
    let members = members_of(set, survey);
    let Some(anchor_installed) = members.iter().find(|(n, _)| n == anchor).map(|(_, v)| v.clone()) else {
        return Vec::new();
    };
    let anchor_major = major_of(&anchor_installed);
    let all_names: Vec<&str> = members
        .iter()
        .filter(|(_, v)| matches!(set.members, Members::Names(_)) || major_of(v) == anchor_major)
        .map(|(n, _)| n.as_str())
        .collect();
    // Ушедшие из репозиториев не обновятся никогда: в сверке набора их нет,
    // о них — своя находка. Опорный пакет сиротой не считаем.
    let orphan = |n: &str| n != anchor && survey.foreign.as_ref().is_some_and(|f| f.contains(n));
    let (orphans, names): (Vec<&str>, Vec<&str>) = all_names.iter().copied().partition(|n| orphan(n));
    let installed = |name: &str| -> String {
        survey.installed.iter().find(|(n, _)| n == name).map(|(_, v)| v.clone()).unwrap_or_default()
    };
    let pending = |name: &str| survey.pending.iter().find(|u| u.package == name);
    let after = |name: &str| -> String { pending(name).map(|u| u.available.clone()).unwrap_or_else(|| installed(name)) };

    // Сирота против опорного пакета ПОСЛЕ ждущего обновления: сама она
    // не сдвинется, а набор уйдёт вперёд.
    let anchor_after = key_of(&after(anchor), precision);
    let mut out: Vec<Finding> = orphans
        .iter()
        .filter(|n| {
            let k = key_of(&installed(n), precision);
            if may_lag.contains(n) {
                k > anchor_after
            } else {
                k != anchor_after
            }
        })
        .map(|n| Finding {
            set: *set,
            stage: Stage::Orphaned,
            lead: (anchor.to_string(), upstream_version(&after(anchor))),
            odd: vec![(n.to_string(), upstream_version(&installed(n)), None)],
            rollback: Vec::new(),
        })
        .collect();

    let before_odd = mismatched(&names, anchor, precision, may_lag, &installed);
    let after_odd = mismatched(&names, anchor, precision, may_lag, &after);
    let (stage, odd, version): (Stage, Vec<String>, &dyn Fn(&str) -> String) =
        match (before_odd.is_empty(), after_odd.is_empty()) {
            (true, true) => return out,
            (true, false) => (Stage::Hold, after_odd, &after),
            (false, false) => (Stage::DontReboot, after_odd, &after),
            (false, true) => (Stage::UpdateNow, before_odd, &installed),
        };

    let rollback = match stage {
        // Вернуть всё, что обновление тронет в наборе: до него набор сходился.
        Stage::Hold => names
            .iter()
            .filter_map(|n| pending(n).map(|u| (n.to_string(), RollbackTarget::Exact(u.installed.clone()))))
            .collect(),
        // Уже разошлось: вернуть обогнавших к самой старой серии набора.
        Stage::DontReboot => {
            let strict: Vec<&str> = names.iter().copied().filter(|n| !may_lag.contains(n)).collect();
            let Some(oldest) = strict.iter().map(|n| key_of(&installed(n), precision)).min() else {
                return out;
            };
            strict
                .iter()
                .filter(|n| key_of(&installed(n), precision) > oldest)
                .map(|n| (n.to_string(), RollbackTarget::Series(precision, oldest.clone())))
                .collect()
        }
        Stage::UpdateNow | Stage::Orphaned => Vec::new(),
    };
    // Набор вразнобой — первым: он важнее сироты.
    out.insert(
        0,
        Finding {
            set: *set,
            stage,
            lead: (anchor.to_string(), upstream_version(&version(anchor))),
            odd: odd.into_iter().map(|n| { let v = upstream_version(&version(&n)); (n, v, None) }).collect(),
            rollback,
        },
    );
    out
}

fn move_together(set: &PackageSet, survey: &Survey) -> Option<Finding> {
    let repo = survey.repo.as_ref()?;
    let members = members_of(set, survey);
    let pending = |name: &str| {
        survey.pending.iter().find(|u| {
            u.package == name && ver_tuple(&upstream_version(&u.available)) > ver_tuple(&upstream_version(&u.installed))
        })
    };
    let moved: Vec<&PendingUpdate> = members.iter().filter_map(|(n, _)| pending(n)).collect();
    let lead = moved.first()?;
    // Остаются на месте, хотя в каком-то репозитории уже есть новее.
    let stale: Vec<(String, String, Option<String>)> = members
        .iter()
        .filter(|(n, _)| survey.pending.iter().all(|u| &u.package != n))
        .filter_map(|(n, v)| {
            let have = ver_tuple(&upstream_version(v));
            let newest = repo
                .get(n)?
                .iter()
                .map(|r| upstream_version(r))
                .filter(|r| ver_tuple(r) > have)
                .max_by(|a, b| ver_tuple(a).cmp(&ver_tuple(b)))?;
            Some((n.clone(), upstream_version(v), Some(newest)))
        })
        .collect();
    if stale.is_empty() {
        return None;
    }
    Some(Finding {
        set: *set,
        stage: Stage::Hold,
        lead: (lead.package.clone(), upstream_version(&lead.available)),
        odd: stale,
        rollback: moved
            .iter()
            .map(|u| (u.package.clone(), RollbackTarget::Exact(u.installed.clone())))
            .collect(),
    })
}

// ══════════════════════════════════════════════════════════════════════════
// Откат из кэша pacman
// ══════════════════════════════════════════════════════════════════════════

/// Кэш пакетов pacman по умолчанию.
pub const PACKAGE_CACHE_DIR: &str = "/var/cache/pacman/pkg";

/// Длиннее этого команда отката нечитаема — тогда только словами. Предел
/// КОНСОЛИ спасения ([`crate::login_rescue`]): там человек набирает её сам.
pub const ROLLBACK_MAX_PACKAGES: usize = 8;

/// Предел кнопки «Вернуть прежние версии»: сколько пакетов одного набора
/// окно возвращает одной транзакцией (Plasma — до ~50 пакетов группы). Тот
/// же предел проверяет помощник от root.
pub const ROLLBACK_BUTTON_MAX: usize = 64;

impl RollbackTarget {
    /// Подходит ли версия файла кэша (полная, `6.11.2-1`) под цель.
    pub fn accepts(&self, version: &str) -> bool {
        match self {
            RollbackTarget::Exact(want) => version == want,
            RollbackTarget::Series(p, key) => &key_of(version, *p) == key,
        }
    }
}

/// Разбор имени файла кэша: `qt6-base-6.11.2-1-x86_64.pkg.tar.zst` →
/// `("qt6-base", "6.11.2-1")`. Подписи (`.sig`) и прочее — None.
pub fn parse_cache_file(file: &str) -> Option<(String, String)> {
    if file.ends_with(".sig") {
        return None;
    }
    let stem = &file[..file.find(".pkg.tar")?];
    let mut parts = stem.rsplitn(4, '-');
    let (_arch, rel, ver, name) = (parts.next()?, parts.next()?, parts.next()?, parts.next()?);
    (!name.is_empty()).then(|| (name.to_string(), format!("{ver}-{rel}")))
}

/// Каталоги кэша из вывода `pacman-conf CacheDir` (по строке, с `/` на
/// конце) плюс [`PACKAGE_CACHE_DIR`]; только нормальные абсолютные пути, без
/// хвостового `/`, без повторов. Тот же разбор — у помощника от root (он
/// зовёт `pacman-conf` сам).
pub fn parse_cache_dirs(pacman_conf: &str) -> Vec<String> {
    let mut out = vec![PACKAGE_CACHE_DIR.to_string()];
    for line in pacman_conf.lines() {
        let dir = line.trim().trim_end_matches('/');
        let normal = dir.starts_with('/')
            && dir[1..].split('/').all(|seg| !seg.is_empty() && seg != "." && seg != "..")
            && !dir.chars().any(char::is_control);
        if normal && !out.iter().any(|d| d == dir) {
            out.push(dir.to_string());
        }
    }
    out
}

/// Кэш пакетов машины: `(каталог, имена файлов)` по каждому каталогу
/// [`parse_cache_dirs`] (`pacman-conf` читается и без прав).
pub fn cache_listing<IO: SystemIo + ?Sized>(io: &IO) -> Vec<(String, Vec<String>)> {
    let conf = io.run(&["pacman-conf".to_string(), "CacheDir".to_string()], Some(30), true);
    let text = if conf.rc() == Some(0) { conf.stdout } else { String::new() };
    parse_cache_dirs(&text)
        .into_iter()
        .map(|d| {
            let files = io.list_dir(&d).unwrap_or_default();
            (d, files)
        })
        .collect()
}

/// Один пакет отката: что стоит сейчас и что вернётся из кэша.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RollbackItem {
    pub name: String,
    /// Установленная версия (полная, как у `pacman -Q`); пусто — не знаем.
    pub from: String,
    /// Версия файла кэша (полная).
    pub to: String,
    /// Путь файла кэша.
    pub path: String,
}

/// Откат находки по кэшу: для каждого пакета — самая новая подходящая
/// версия из всех каталогов `caches`. Пусто — хоть одного пакета нет в кэше
/// (половинчатый откат хуже никакого) либо их больше `max`. `installed` —
/// `pacman -Q` (для «было»).
pub fn rollback_items(
    finding: &Finding,
    installed: &[(String, String)],
    caches: &[(String, Vec<String>)],
    max: usize,
) -> Vec<RollbackItem> {
    if finding.rollback.is_empty() || finding.rollback.len() > max {
        return Vec::new();
    }
    let parsed: Vec<(String, String, String)> = caches
        .iter()
        .flat_map(|(dir, files)| {
            files.iter().filter_map(move |f| {
                parse_cache_file(f).map(|(n, v)| (n, v, format!("{}/{f}", dir.trim_end_matches('/'))))
            })
        })
        .collect();
    let mut out = Vec::new();
    for (name, target) in &finding.rollback {
        let best = parsed
            .iter()
            .filter(|(n, v, _)| n == name && target.accepts(v))
            .max_by(|a, b| ver_tuple(&a.1).cmp(&ver_tuple(&b.1)));
        match best {
            Some((_, to, path)) => out.push(RollbackItem {
                name: name.clone(),
                from: installed.iter().find(|(n, _)| n == name).map(|(_, v)| v.clone()).unwrap_or_default(),
                to: to.clone(),
                path: path.clone(),
            }),
            None => return Vec::new(),
        }
    }
    out
}

/// Пути файлов кэша для команды отката консоли (не больше
/// [`ROLLBACK_MAX_PACKAGES`]). Пусто — хоть одного пакета нет в кэше.
pub fn rollback_paths(finding: &Finding, caches: &[(String, Vec<String>)]) -> Vec<String> {
    rollback_items(finding, &[], caches, ROLLBACK_MAX_PACKAGES).into_iter().map(|i| i.path).collect()
}

// ══════════════════════════════════════════════════════════════════════════
// Тексты
// ══════════════════════════════════════════════════════════════════════════

/// Заголовок предупреждения по самой важной находке.
pub fn title(findings: &[Finding]) -> String {
    let stage = |s: Stage| findings.iter().any(|f| f.stage == s);
    if stage(Stage::DontReboot) {
        tr("Don't Restart the Computer Yet")
    } else if stage(Stage::Hold) {
        tr("Don't Update the System Yet")
    } else if stage(Stage::UpdateNow) {
        tr("Update the System Once More")
    } else {
        tr("A Package No Longer Gets Updates")
    }
}

// Тексты о сироте — константами: их показывает и окно Rexon (язык сеанса,
// [`tr`]), и консоль при упавшем экране входа (вшитый каталог,
// [`crate::login_rescue`]), каждый своим переводчиком.

// TRANSLATORS: %1 is a package name, %2 packages with versions, e.g. "kwin-x11 6.7.5 with plasma-workspace 6.8.1".
const T_ORPHAN: &str = n_(
    "%1 has been removed from the repositories and no longer gets updates: it is left over from an earlier version (%2).",
);
// TRANSLATORS: Two lists of packages with versions, e.g. "kwin-x11 6.7.5" and "plasma-workspace 6.8.1".
const T_ORPHAN_PAIR: &str = n_("%1 with %2");
// TRANSLATORS: %1 is a command line, keep it as is.
const T_ORPHAN_REMOVE: &str = n_("If you don't use it, remove the package: %1");

/// Почему пакет ушёл из репозиториев — для известных случаев: (пакет,
/// пояснение). Новый случай — новая запись.
pub const ORPHAN_NOTES: &[(&str, &str)] = &[(
    "kwin-x11",
    // TRANSLATORS: Explains why the package kwin-x11 left the repositories. "Plasma (X11)" is a session name.
    n_("The Plasma (X11) session is not supported on this version of Plasma."),
)];

/// Все строки о сироте (для сверки «каждая — через gettext и с вставками»).
pub const ORPHAN_TEXTS: &[&str] = &[T_ORPHAN, T_ORPHAN_PAIR, T_ORPHAN_REMOVE, ORPHAN_NOTES[0].1];

/// Команда удаления сироты (`pacman -Rns`: с зависимостями, ставшими
/// ненужными, и без сохранённых настроек). None — находка не о сироте.
pub fn orphan_remove_command(f: &Finding) -> Option<String> {
    if f.stage != Stage::Orphaned {
        return None;
    }
    let (name, _, _) = f.odd.first()?;
    Some(format!("sudo pacman -Rns {name}"))
}

/// Что с сиротой: «ушёл из репозиториев, остался от прежней версии» и,
/// если причина известна, — пояснение. `t` — переводчик (msgid → текст).
pub fn orphan_cause(f: &Finding, t: &dyn Fn(&str) -> String) -> String {
    let (name, version, _) = f.odd.first().cloned().unwrap_or_default();
    let pair = fill(&t(T_ORPHAN_PAIR), &[&format!("{name} {version}"), &format!("{} {}", f.lead.0, f.lead.1)]);
    let mut text = fill(&t(T_ORPHAN), &[&name, &pair]);
    if let Some((_, note)) = ORPHAN_NOTES.iter().find(|(n, _)| *n == name) {
        text.push(' ');
        text.push_str(&t(note));
    }
    text
}

/// Совет о сироте для КОНСОЛИ: «удалите, если не пользуетесь: команда».
/// Человек за консолью спасения команду набирает сам — она здесь к месту.
/// Окну Rexon и уведомлению — [`finding_text_for`]: там команды нет.
pub fn orphan_advice(f: &Finding, t: &dyn Fn(&str) -> String) -> Option<String> {
    orphan_remove_command(f).map(|cmd| fill(&t(T_ORPHAN_REMOVE), &[&cmd]))
}

/// Кому текст находки. Консоль спасения сюда не ходит — у неё свой текст с
/// командой ([`orphan_advice`], [`crate::login_rescue`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Audience {
    /// Окно Rexon. `removable` — в окне есть кнопка удаления сироты
    /// ([`removable_orphans`]); нет её — о том, что делать с сиротой,
    /// молчим (на SteamOS удалить пакет нельзя вовсе). `rollback` — в окне
    /// есть кнопка «Вернуть прежние версии» ([`rollback_offer`]); нет её —
    /// об откате молчим.
    Window { removable: bool, rollback: bool },
    /// Уведомление фоновой проверки: кнопки нет — «откройте Rexon».
    Notification,
}

/// Совет о сироте окну Rexon либо уведомлению — без команд.
fn orphan_advice_for(f: &Finding, audience: Audience) -> Option<String> {
    if f.stage != Stage::Orphaned {
        return None;
    }
    match audience {
        // TRANSLATORS: About a package that no longer gets updates; the window has a button to remove it.
        Audience::Window { removable: true, .. } => Some(tr("If you don't use it, Rexon can remove it.")),
        Audience::Window { removable: false, .. } => None,
        // TRANSLATORS: In a desktop notification about a package that no longer gets updates.
        Audience::Notification => Some(tr("If you don't use it, open Rexon to remove it.")),
    }
}

/// Сироты находок — имена пакетов, по порядку, без повторов.
pub fn orphan_names(findings: &[Finding]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for f in findings.iter().filter(|f| f.stage == Stage::Orphaned) {
        if let Some((name, _, _)) = f.odd.first() {
            if !out.contains(name) {
                out.push(name.clone());
            }
        }
    }
    out
}

/// Каких сирот окно предлагает удалить кнопкой: на SteamOS — никаких
/// (`/usr` только для чтения, части системы ставит обновление образа).
pub fn removable_orphans(findings: &[Finding], steamos: bool) -> Vec<String> {
    if steamos {
        Vec::new()
    } else {
        orphan_names(findings)
    }
}

// ── Сирота, на которой работает сеанс ───────────────────────────────────
//
// Защита от дурака: человек сидит в сеансе Plasma (X11) и удаляет kwin-x11
// — следующий вход в этот сеанс не сработает. То же, если экран входа
// открывает такой сеанс сам (автовход, последний выбранный). Решение —
// данными: пакет → сеанс, которому он нужен. Новый случай — новая запись.

/// Сеанс, который держится на пакете-сироте.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OrphanSession {
    /// Пакет-сирота.
    pub package: &'static str,
    /// Тип сеанса logind (`Type=`, `XDG_SESSION_TYPE`), где пакет нужен.
    pub session_type: &'static str,
    /// Стол сеанса по базе Rexon ([`crate::system::detect_current_de`]).
    pub desktop: &'static str,
    /// Имена файлов сеанса (`/usr/share/xsessions/<имя>.desktop`) — то, что
    /// экран входа пишет в `Session=`.
    pub sessions: &'static [&'static str],
    /// Имя сеанса для человека — как в списке экрана входа (не переводится).
    pub session_name: &'static str,
    /// Сеанс, из которого пакет удалять безопасно.
    pub instead: &'static str,
}

/// Пакеты-сироты, на которых работает сеанс.
pub const ORPHAN_SESSIONS: &[OrphanSession] = &[OrphanSession {
    package: "kwin-x11",
    session_type: "x11",
    desktop: "kde",
    sessions: &["plasmax11"],
    session_name: "Plasma (X11)",
    instead: "Plasma (Wayland)",
}];

/// Запись таблицы по имени пакета.
pub fn orphan_session_of(package: &str) -> Option<&'static OrphanSession> {
    ORPHAN_SESSIONS.iter().find(|s| s.package == package)
}

/// Удаление унесёт пакет, на котором работает сеанс `session_type` стола
/// `current_de` (тот, где человек сейчас).
pub fn orphan_in_use(names: &[String], session_type: &str, current_de: &str) -> Option<&'static OrphanSession> {
    let kind = session_type.trim().to_ascii_lowercase();
    ORPHAN_SESSIONS
        .iter()
        .find(|s| names.iter().any(|n| n == s.package) && s.session_type == kind && s.desktop == current_de.trim())
}

/// Имя сеанса из значения `Session=` экрана входа: `plasmax11`,
/// `plasmax11.desktop`, `/usr/share/xsessions/plasmax11.desktop` → `plasmax11`.
pub fn session_file_name(value: &str) -> String {
    let base = value.trim().rsplit('/').next().unwrap_or_default();
    base.strip_suffix(".desktop").unwrap_or(base).to_string()
}

/// Удаление унесёт пакет сеанса, который экран входа открывает сам
/// (`defaults` — [`login_default_sessions`]).
pub fn orphan_login_default(names: &[String], defaults: &[String]) -> Option<&'static OrphanSession> {
    ORPHAN_SESSIONS.iter().find(|s| {
        names.iter().any(|n| n == s.package) && defaults.iter().any(|d| s.sessions.contains(&session_file_name(d).as_str()))
    })
}

/// Сеансы, которые экран входа открывает или предлагает сам: автовход
/// SDDM и Plasma Login Manager, последний выбранный в SDDM, предвыбранный
/// у Plasma Login Manager (источники и порядок —
/// [`crate::system_plan::LOGIN_SESSION_SOURCES`]). `read` — текст файла,
/// `list` — имена в каталоге.
pub fn login_default_sessions(read: &dyn Fn(&str) -> Option<String>, list: &dyn Fn(&str) -> Vec<String>) -> Vec<String> {
    crate::system_plan::login_session_values(read, list)
}

const T_ORPHAN_IN_USE: &str =
    // TRANSLATORS: %1 and %2 are session names as shown on the login screen, e.g. "Plasma (X11)" and "Plasma (Wayland)".
    n_("You are using %1 right now, and it runs on this package. Sign in to %2 and remove it from there.");
const T_ORPHAN_LOGIN_DEFAULT: &str = n_(
    // TRANSLATORS: %1 and %2 are session names as shown on the login screen, e.g. "Plasma (X11)" and "Plasma (Wayland)".
    "The login screen starts %1 by itself, and it runs on this package. Sign in to %2 once, so the login \
     screen remembers it, and remove the package from there.",
);

/// Строки отказов «сеанс держится на пакете» — шаблоны с `%1`/`%2` (для
/// сверки переводов: готовый текст подставляет имена сеансов).
pub const ORPHAN_SESSION_TEXTS: &[&str] = &[T_ORPHAN_IN_USE, T_ORPHAN_LOGIN_DEFAULT, T_ORPHAN_NOW_REMOVABLE];

// TRANSLATORS: Said after the login screen was switched to another session; the package is the one Rexon offered to remove.
const T_ORPHAN_NOW_REMOVABLE: &str = n_("Now the package can be removed.");

/// Отказ: человек сейчас в сеансе, который держится на пакете.
pub fn orphan_in_use_text(s: &OrphanSession) -> String {
    trf(T_ORPHAN_IN_USE, &[&s.session_name, &s.instead])
}

/// Отказ: экран входа сам открывает сеанс, который держится на пакете.
pub fn orphan_login_default_text(s: &OrphanSession) -> String {
    trf(T_ORPHAN_LOGIN_DEFAULT, &[&s.session_name, &s.instead])
}

/// Отказ помощника `why` при удалении `names` — «экран входа сам открывает
/// сеанс на пакете»: тогда окно вместо голого отказа предлагает сделать
/// пару сеанса на Wayland сеансом по умолчанию (её запись в
/// [`crate::system_plan::X11_SESSION_SUCCESSORS`]). Отказ приходит готовым
/// текстом того же языка ([`crate::privilege::client::key_text`]), поэтому
/// узнаётся сверкой с тем же текстом.
pub fn login_default_successor(names: &[String], why: &str) -> Option<&'static crate::system_plan::SessionSuccessor> {
    ORPHAN_SESSIONS
        .iter()
        .filter(|s| names.iter().any(|n| n == s.package))
        .find(|s| why.trim() == orphan_login_default_text(s))
        .and_then(|s| s.sessions.iter().find_map(|x| crate::system_plan::session_successor(x)))
}

/// Приписка после смены сеанса по умолчанию: теперь сироту можно снять.
pub fn orphan_now_removable() -> String {
    crate::i18n::tr(T_ORPHAN_NOW_REMOVABLE)
}

/// Что окно сторожа предлагает с сиротами.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrphanOffer {
    /// Кнопка «Удалить …».
    Remove(Vec<String>),
    /// Удалять отсюда нельзя — вместо кнопки этот текст.
    Blocked(String),
    /// Сирот нет либо удалить нельзя вовсе (SteamOS) — ни кнопки, ни совета.
    Nothing,
}

/// Решение окна. `session_type` и `current_de` — сеанс окна (тот же, что
/// помощник от root узнает у logind и сверит сам).
pub fn orphan_offer(findings: &[Finding], steamos: bool, session_type: &str, current_de: &str) -> OrphanOffer {
    let names = removable_orphans(findings, steamos);
    if names.is_empty() {
        return OrphanOffer::Nothing;
    }
    match orphan_in_use(&names, session_type, current_de) {
        Some(s) => OrphanOffer::Blocked(orphan_in_use_text(s)),
        None => OrphanOffer::Remove(names),
    }
}

/// [`orphan_offer`] на живой машине (сеанс окна — по окружению).
pub fn orphan_offer_live(findings: &[Finding]) -> OrphanOffer {
    let env = |n: &str| std::env::var(n).unwrap_or_default();
    let steamos = crate::system::DistroDetector::detect_live().is_steamos();
    let de = crate::system::detect_current_de(&env("XDG_CURRENT_DESKTOP"), &env("DESKTOP_SESSION"), !env("WAYLAND_DISPLAY").is_empty())
        .unwrap_or_default();
    orphan_offer(findings, steamos, &env("XDG_SESSION_TYPE"), &de)
}

/// Список имён для подписи: «kwin-x11» либо «kwin-x11, foo».
fn names_list(names: &[String]) -> String {
    names.join(", ")
}

/// Кнопка окна: «Удалить kwin-x11».
pub fn orphan_remove_button(names: &[String]) -> String {
    // TRANSLATORS: Button. %1 is a package name (or several, comma-separated), e.g. "kwin-x11".
    trf("Remove %1", &[&names_list(names)])
}

/// Заголовок окна хода удаления.
pub fn orphan_remove_title(names: &[String]) -> String {
    // TRANSLATORS: Window title while packages are being removed. %1 is a package name (or several).
    trf("Removing %1", &[&names_list(names)])
}

/// Итог удаления. `rest_clean` — повторная сверка ничего больше не нашла.
pub fn orphan_removed_text(names: &[String], rest_clean: bool) -> String {
    // TRANSLATORS: Result of removing packages. %1 is a package name (or several).
    let done = trf("Removed: %1.", &[&names_list(names)]);
    if rest_clean {
        // TRANSLATORS: Follows "Removed: <package>." after Rexon checked the packages again.
        format!("{done} {}", tr("Everything else is in order."))
    } else {
        done
    }
}

/// Удаление не удалось, а пояснения нет.
pub fn orphan_remove_failed(names: &[String]) -> String {
    // TRANSLATORS: %1 is a package name (or several).
    trf("Rexon couldn't remove %1.", &[&names_list(names)])
}

/// Удалить сирот через сеанс прав окна ([`crate::privilege`]). Что пакет и
/// правда сирота (установлен, нет ни в одном репозитории, член набора), не
/// жизненно важен и не часть текущего стола — проверяет помощник от root
/// сам; окну он не верит. `run` — исполнитель операции
/// ([`crate::privilege::run::run_live`] на живой машине). Прежнего пути
/// через `pkexec` у этой операции нет намеренно: без помощника проверить
/// «сироту» от root некому.
///
/// `Err` — готовый текст для человека.
pub fn remove_orphans_with(
    names: &[String],
    run: &mut dyn FnMut(&crate::privilege::op::PrivOp) -> Option<crate::system_exec::RunOut>,
) -> Result<(), String> {
    if names.is_empty() {
        return Err(orphan_remove_failed(names));
    }
    let op = crate::privilege::op::PrivOp::OrphanRemove { names: names.to_vec() };
    let Some(out) = run(&op) else {
        // Помощника сеанса нет вовсе.
        return Err(crate::privilege::client::ClientError::HelperMissing.human_text());
    };
    if out.returncode == Some(0) {
        return Ok(());
    }
    // Отказ проверки — готовый текст в stderr; провал pacman — его строки в
    // stdout (они уже в журнале окна), человеку — общий итог.
    let why = out.stderr.lines().map(str::trim).rfind(|l| !l.is_empty()).map(str::to_string);
    Err(why.unwrap_or_else(|| orphan_remove_failed(names)))
}

/// [`remove_orphans_with`] на живой машине: строки хода — в `line`.
pub fn remove_orphans_live(names: &[String], line: &mut dyn FnMut(&str)) -> Result<(), String> {
    remove_orphans_with(names, &mut |op| crate::privilege::run::run_live(op, &[], None, &mut *line))
}

/// Член набора, который может стать сиротой: пакет набора «одной версией»,
/// но не его опорный (опорный сиротой не считается, см. [`evaluate`]).
/// `groups` — `(группа, пакет)` из `pacman -Qg` ([`groups_needed`]). Эта
/// проверка — и для помощника от root: удалять он берётся только такие.
pub fn orphan_candidate(name: &str, groups: &[(String, String)]) -> bool {
    PACKAGE_SETS.iter().any(|set| {
        let Rule::SameVersion { anchor, .. } = set.rule else {
            return false;
        };
        name != anchor
            && match set.members {
                Members::Prefix(p) => name.starts_with(p),
                Members::Group(g) => groups.iter().any(|(gr, p)| gr == g && p == name),
                Members::Names(list) => list.contains(&name),
            }
    })
}

fn versions_list(odd: &[(String, String, Option<String>)]) -> String {
    const SHOWN: usize = 3;
    let mut list = odd
        .iter()
        .take(SHOWN)
        .map(|(n, v, _)| format!("{n} {v}"))
        .collect::<Vec<_>>()
        .join(", ");
    if odd.len() > SHOWN {
        list.push_str(", …");
    }
    list
}

/// Абзац о наборе для окна Rexon с кнопками (удаление сирот, откат).
/// `rollback` — прежние версии всех пакетов отката есть в кэше
/// ([`rollback_items`]).
pub fn finding_text(f: &Finding, rollback: bool) -> String {
    finding_text_for(f, rollback, Audience::Window { removable: true, rollback: true })
}

/// Абзац о наборе для окна либо уведомления ([`Audience`]).
pub fn finding_text_for(f: &Finding, rollback: bool, audience: Audience) -> String {
    if f.stage == Stage::Orphaned {
        let translate = |m: &str| tr(m);
        return [Some(orphan_cause(f, &translate)), orphan_advice_for(f, audience)]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join("\n\n");
    }
    let lead = format!("{} {}", f.lead.0, f.lead.1);
    let headline = match (f.set.rule, f.stage) {
        (Rule::MoveTogether, _) => {
            let (name, version, newer) = f.odd.first().cloned().unwrap_or_default();
            trf(
                // TRANSLATORS: %1 is a package with its new version (e.g. "gdk-pixbuf2 2.44.8"),
                // %2 a package it needs, %3 the version of %2 that stays installed,
                // %4 the newer version of %2 that already exists in another repository.
                "The update would install %1, but %2, which it needs, would stay at version %3: \
                 the repository your system prefers doesn't have version %4 yet.",
                &[&lead, &name, &version, &newer.unwrap_or_default()],
            )
        }
        (_, Stage::Hold) => trf(
            // TRANSLATORS: %1 is a product name (Qt, Plasma, KDE Frameworks, GNOME),
            // %2 packages with versions, e.g. "qt6-base 6.12.0 with qt6-declarative 6.11.2".
            "The update would install parts of %1 of different versions (%2).",
            &[&f.set.name, &pair_text(&lead, &f.odd)],
        ),
        (_, Stage::DontReboot) => trf(
            // TRANSLATORS: %1 is a product name (Qt, Plasma, KDE Frameworks, GNOME),
            // %2 packages with versions, e.g. "qt6-base 6.12.0 with qt6-declarative 6.11.2".
            "The last update installed parts of %1 of different versions (%2).",
            &[&f.set.name, &pair_text(&lead, &f.odd)],
        ),
        (_, Stage::UpdateNow) => trf(
            // TRANSLATORS: %1 is a product name (Qt, Plasma, KDE Frameworks, GNOME),
            // %2 packages with versions, e.g. "qt6-base 6.12.0 with qt6-declarative 6.11.2".
            "Parts of %1 are of different versions now (%2), and the waiting update brings them together.",
            &[&f.set.name, &pair_text(&lead, &f.odd)],
        ),
        (_, Stage::Orphaned) => unreachable!("сирота — выше"),
    };
    let effect = match f.set.effect {
        Effect::LoginScreen => tr("After a restart, the login screen may not start — only a black screen."),
        Effect::Programs => tr("Programs that use it (for example, Firefox) may stop starting."),
    };
    let action = match f.stage {
        Stage::Hold => tr(
            "Don't update right now. This usually passes within a few hours, when the repository \
             catches up — then the update arrives complete.",
        ),
        Stage::DontReboot => tr(
            "Don't restart the computer yet. The repository usually catches up within a few hours: \
             update the system again then, and everything fits together.",
        ),
        Stage::UpdateNow => tr("Update the system now, before restarting the computer."),
        Stage::Orphaned => unreachable!("сирота — выше"),
    };
    let mut paragraphs = vec![format!("{headline} {effect}"), action];
    if f.stage != Stage::UpdateNow {
        paragraphs.push(match f.set.effect {
            Effect::LoginScreen => tr(
                "If you have already updated and see a black screen after a restart: press \
                 Ctrl+Alt+F3, log in with your user name and password and run \
                 “sudo pacman -Syu”, then “sudo reboot”.",
            ),
            Effect::Programs => tr(
                "If you have already updated and programs stop starting: run “sudo pacman -Syu” \
                 in a terminal once the repository has caught up.",
            ),
        });
    }
    // Откат — только когда новые несогласованные версии уже стоят и прежние
    // есть в кэше: в «не обновляйтесь» ставить обратно нечего (стоят ещё
    // прежние; обновится человек всё же — следующая сверка скажет «не
    // перезагружайтесь» с кнопкой). Команды нет: возвращает кнопка окна.
    if f.stage == Stage::DontReboot && rollback {
        match audience {
            Audience::Window { rollback: true, .. } => paragraphs.push(tr(
                // TRANSLATORS: The window has a button that puts the previous package versions back.
                "If there is nothing to update yet, Rexon can put the previous versions back: they are still \
                 on this computer.",
            )),
            Audience::Window { rollback: false, .. } => {}
            Audience::Notification => paragraphs.push(tr(
                // TRANSLATORS: In a desktop notification; Rexon's window has a button that puts the previous versions back.
                "If there is nothing to update yet, open Rexon to put the previous versions back.",
            )),
        }
    }
    paragraphs.join("\n\n")
}

fn pair_text(lead: &str, odd: &[(String, String, Option<String>)]) -> String {
    trf(
        // TRANSLATORS: two lists of packages with versions, e.g.
        // "qt6-base 6.12.0" and "qt6-declarative 6.11.2, qt6-svg 6.11.2".
        "%1 with %2",
        &[&lead, &versions_list(odd)],
    )
}

/// Весь текст предупреждения. `rollback(находка)` — прежние версии есть в
/// кэше.
pub fn advice_text(findings: &[Finding], rollback: &dyn Fn(&Finding) -> bool) -> String {
    findings
        .iter()
        .map(|f| finding_text(f, rollback(f)))
        .collect::<Vec<_>>()
        .join("\n\n")
}

// ══════════════════════════════════════════════════════════════════════════
// «Об этом уже говорили»
// ══════════════════════════════════════════════════════════════════════════

/// Отпечаток состояния: о том же самом второй раз не говорим (окно Rexon и
/// уведомление делят его — показанное окном уведомлением не повторяется).
pub fn fingerprint(findings: &[Finding]) -> String {
    findings
        .iter()
        .map(|f| {
            let odd: BTreeSet<String> = f.odd.iter().map(|(n, v, _)| format!("{n}={v}")).collect();
            format!(
                "{}:{}:{}={}:{}",
                f.stage.key(),
                f.set.id,
                f.lead.0,
                f.lead.1,
                odd.into_iter().collect::<Vec<_>>().join(",")
            )
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Говорить ли: есть что сказать, и это не то же, о чём уже говорили
/// (`last` — сохранённый отпечаток). При входе в сеанс — всегда.
pub fn should_notify(findings: &[Finding], fingerprint: &str, last: Option<&str>, login: bool) -> bool {
    !findings.is_empty() && (login || last.map(str::trim) != Some(fingerprint))
}

/// Файл отпечатка в кэше пользователя.
pub fn stamp_path(cache_home: &str) -> String {
    format!("{}/rexon/update-guard-notified", cache_home.trim_end_matches('/'))
}

// ══════════════════════════════════════════════════════════════════════════
// Фоновая проверка: пользовательские юниты systemd
// ══════════════════════════════════════════════════════════════════════════
//
// Одни юниты на все проверки «до и после обновления» — и на эту, и на
// сверку AeroThemePlasma (её прежние юниты `rexon-aeroshell-check.*`
// снимаются: два таймера гоняли бы `checkupdates` дважды).
//   * `.path` следит за `/var/lib/pacman/local` — каталогом базы
//     установленных пакетов. pacman на каждый пакет заводит там каталог
//     `<имя>-<версия>` и убирает прежний, а `PathChanged=` на каталоге
//     срабатывает на появление, переименование и удаление записей в нём
//     (проверено 04.10.2026 отдельным `systemd --user` в песочнице,
//     systemd 262). Каталог читаем всем, прав не надо. Нужен, чтобы сразу
//     после обновления, разведшего набор, сказать «не перезагружайтесь»;
//   * `.timer` — после входа в сеанс и дальше раз в 6 часов: новое в
//     репозиториях появляется без всякой транзакции на машине;
//   * `.service` — `rexon --update-check`: дождаться конца транзакции
//     pacman, сверить (только чтение), при беде — уведомление.
// ⛔ Никаких хуков pacman от root: хук работает внутри транзакции, до
// сеанса пользователя ему не достучаться, а установка хука — пароль.

/// Флаг безоконного маршрута проверки (его зовёт `.service`).
pub const CHECK_FLAG: &str = "--update-check";

/// Имена юнитов.
pub const WATCH_PATH_UNIT: &str = "rexon-update-check.path";
pub const WATCH_TIMER_UNIT: &str = "rexon-update-check.timer";
pub const WATCH_SERVICE_UNIT: &str = "rexon-update-check.service";
/// Прежние юниты сверки AeroThemePlasma — снимаются при установке общих.
pub const LEGACY_UNITS: &[&str] = &[
    "rexon-aeroshell-check.path",
    "rexon-aeroshell-check.timer",
    "rexon-aeroshell-check.service",
];

/// База установленных пакетов pacman и её замок.
pub const PACMAN_LOCAL_DB: &str = "/var/lib/pacman/local";
pub const PACMAN_DB_LOCK: &str = "/var/lib/pacman/db.lck";
/// Сколько ждать конца чужой транзакции, секунды (большое обновление
/// с пересборкой initramfs идёт минутами).
pub const LOCK_WAIT_SECS: u64 = 45 * 60;

/// Каталог пользовательских юнитов.
pub fn user_unit_dir(config_home: &str) -> String {
    format!("{}/systemd/user", config_home.trim_end_matches('/'))
}

/// Юниты проверки: (имя, текст). `exec_start` — готовая строка запуска
/// Rexon для `ExecStart=` ([`crate::app::unit_launch_command`] с
/// [`CHECK_FLAG`]); `program` — путь к его бинарю: Rexon удалили — проверка
/// тихо пропускается, а не падает каждые 6 часов. Описания — по-английски и
/// не из каталога: их читает `systemctl`, а не покупатель.
pub fn watch_units(exec_start: &str, program: Option<&str>) -> Vec<(&'static str, String)> {
    let condition = program
        .filter(|p| p.starts_with('/'))
        .map(|p| format!("ConditionPathExists={p}\n"))
        .unwrap_or_default();
    vec![
        (
            WATCH_PATH_UNIT,
            format!(
                "[Unit]\n\
                 Description=Rexon: check the system after package updates\n\n\
                 [Path]\n\
                 PathChanged={PACMAN_LOCAL_DB}\n\
                 Unit={WATCH_SERVICE_UNIT}\n\
                 # Большое обновление даёт сотни событий за секунды: с\n\
                 # ограничением по умолчанию (200 за 2 с) слежка отключилась бы\n\
                 # сама. Повторы безвредны: пока проверка идёт, новые\n\
                 # срабатывания сливаются с ней.\n\
                 TriggerLimitIntervalSec=0\n\n\
                 [Install]\n\
                 WantedBy=default.target\n"
            ),
        ),
        (
            WATCH_TIMER_UNIT,
            format!(
                "[Unit]\n\
                 Description=Rexon: check pending updates after login and every 6 hours\n\n\
                 [Timer]\n\
                 OnStartupSec=90s\n\
                 # Повтор раз в 6 часов: опасное обновление может появиться в\n\
                 # репозиториях до обновления — предупредить, пока не поздно.\n\
                 OnUnitActiveSec=6h\n\
                 Unit={WATCH_SERVICE_UNIT}\n\n\
                 [Install]\n\
                 WantedBy=timers.target\n"
            ),
        ),
        (
            WATCH_SERVICE_UNIT,
            format!(
                "[Unit]\n\
                 Description=Rexon: warn before an update that would break the system\n\
                 {condition}\n\
                 [Service]\n\
                 Type=oneshot\n\
                 ExecStart={exec_start}\n\
                 # Ждёт конца транзакции pacman и нажатия в уведомлении.\n\
                 TimeoutStartSec=infinity\n\
                 # Rexon, открытый кнопкой уведомления, переживает проверку.\n\
                 KillMode=process\n\
                 Nice=10\n"
            ),
        ),
    ]
}

/// Что сделать с юнитами проверки.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WatchPlan {
    /// (путь, текст) — записать.
    pub write: Vec<(String, String)>,
    /// Пути — удалить (перед первым `daemon-reload`).
    pub remove: Vec<String>,
    /// Аргументы `systemctl --user …` по порядку.
    pub systemctl: Vec<Vec<String>>,
}

/// План: `wanted` — нужна ли слежка (Arch); `exec_start` — строка запуска
/// (None — заводить нечем, слежку снимаем); `existing(путь)` — текущий
/// текст файла юнита либо None. Прежние юниты AeroThemePlasma снимаются в
/// любом случае.
///
/// Идемпотентно: всё совпадает — план пуст, `systemctl` не зовётся.
pub fn watch_plan(
    config_home: &str,
    wanted: bool,
    exec_start: Option<&str>,
    program: Option<&str>,
    existing: &dyn Fn(&str) -> Option<String>,
) -> WatchPlan {
    let dir = user_unit_dir(config_home);
    let path = |name: &str| format!("{dir}/{name}");
    let mut plan = WatchPlan::default();
    let enable_names = [WATCH_PATH_UNIT, WATCH_TIMER_UNIT];
    let mut disable: Vec<&str> = Vec::new();

    // Прежние юниты — снять.
    for name in LEGACY_UNITS {
        if existing(&path(name)).is_some() {
            plan.remove.push(path(name));
            if !name.ends_with(".service") {
                disable.push(name);
            }
        }
    }
    let mut enable: Vec<&str> = Vec::new();
    match (wanted, exec_start) {
        (true, Some(exec)) => {
            for (name, text) in watch_units(exec, program) {
                if existing(&path(name)).as_deref() != Some(text.as_str()) {
                    plan.write.push((path(name), text));
                }
            }
            if !plan.write.is_empty() {
                enable.extend(enable_names);
            }
        }
        _ => {
            let present: Vec<String> = [WATCH_PATH_UNIT, WATCH_TIMER_UNIT, WATCH_SERVICE_UNIT]
                .iter()
                .map(|n| path(n))
                .filter(|p| existing(p).is_some())
                .collect();
            if !present.is_empty() {
                disable.extend(enable_names);
                plan.remove.extend(present);
            }
        }
    }
    if !disable.is_empty() {
        let mut args = vec!["disable".to_string(), "--now".to_string()];
        args.extend(disable.iter().map(|n| n.to_string()));
        plan.systemctl.push(args);
    }
    if !plan.write.is_empty() || !plan.remove.is_empty() {
        plan.systemctl.push(vec!["daemon-reload".into()]);
    }
    if !enable.is_empty() {
        let mut args = vec!["enable".to_string(), "--now".to_string()];
        args.extend(enable.iter().map(|n| n.to_string()));
        plan.systemctl.push(args);
    }
    plan
}

/// Исполнить план юнитов. Без root: только `~/.config/systemd/user` и
/// `systemctl --user`. Пустой план — ничего не делается.
pub fn apply_watch_plan<IO: DeOpsIo + ?Sized>(io: &IO, plan: &WatchPlan) -> Result<(), String> {
    if let Some((first, _)) = plan.write.first() {
        if let Some((dir, _)) = first.rsplit_once('/') {
            io.make_dirs(dir)?;
        }
    }
    for (path, text) in &plan.write {
        io.write_file_mode(path, text, 0o644)?;
    }
    if plan.systemctl.is_empty() {
        return Ok(());
    }
    if !io.which("systemctl") {
        return Err("systemctl not found".to_string());
    }
    for args in &plan.systemctl {
        // Снятие: сначала disable (юниты ещё на месте), потом файлы.
        if args.first().map(String::as_str) == Some("daemon-reload") {
            for path in &plan.remove {
                io.remove_file(path);
            }
        }
        let mut argv = vec!["systemctl".to_string(), "--user".to_string()];
        argv.extend(args.iter().cloned());
        let r = io.run(&argv, Some(30), true);
        if r.rc() != Some(0) {
            return Err(format!("{}: {}", argv.join(" "), r.stderr.trim()));
        }
    }
    Ok(())
}

// ══════════════════════════════════════════════════════════════════════════
// Исполнение
// ══════════════════════════════════════════════════════════════════════════

/// Собрать [`Survey`]. None — не Arch, нет pacman/checkupdates, либо
/// `checkupdates` не смог (нет сети): «не знаем» — молчим.
pub fn survey<IO: SystemIo + ?Sized>(io: &IO, family: PkgFamily, uid: u32, sets: &[PackageSet]) -> Option<Survey> {
    if family != PkgFamily::Arch || !io.which("pacman") {
        return None;
    }
    let pending = pending_updates(io, family)?;
    let q = io.run(&["pacman".to_string(), "-Q".to_string()], Some(60), true);
    if q.rc() != Some(0) {
        return None;
    }
    let installed = parse_installed(&q.stdout);
    let groups = groups_needed(sets);
    let groups = if groups.is_empty() {
        Vec::new()
    } else {
        // Код 1, если какой-то группы нет вовсе, — найденные всё равно в выводе.
        let mut argv = vec!["pacman".to_string(), "-Qg".to_string()];
        argv.extend(groups.iter().map(|g| g.to_string()));
        parse_groups(&io.run(&argv, Some(60), true).stdout)
    };
    let db = checkupdates_db(&io.env("CHECKUPDATES_DB"), &io.env("TMPDIR"), uid);
    // Сироты — по свежей базе `checkupdates` (в ней есть ссылка `local` на
    // установленное), без неё — по базе системы.
    let fresh = io.exists(&format!("{}/local", db.trim_end_matches('/'))).then_some(db.as_str());
    let foreign = foreign_packages(io, fresh).or_else(|| fresh.and_then(|_| foreign_packages(io, None)));
    let mut survey = Survey { installed, groups, pending, repo: None, foreign };
    // Версии в репозиториях нужны только «двигаться вместе», и только когда
    // кто-то из такого набора обновляется: `pacman -Sl` — десятки тысяч строк.
    let needs_repo = sets.iter().any(|s| {
        s.rule == Rule::MoveTogether
            && members_of(s, &survey).iter().any(|(n, _)| survey.pending.iter().any(|u| &u.package == n))
    });
    if needs_repo {
        if io.is_dir(&db) {
            let r = io.run(
                &["pacman".to_string(), "-Sl".to_string(), "--dbpath".to_string(), db.clone()],
                Some(60),
                true,
            );
            if r.rc() == Some(0) {
                survey.repo = Some(parse_sync_list(&r.stdout));
            }
        }
    }
    Some(survey)
}

/// Итог сверки: находки и готовые пути отката для каждой.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Advice {
    pub findings: Vec<Finding>,
    /// Откат каждой находки из кэша ([`rollback_items`], не больше
    /// [`ROLLBACK_BUTTON_MAX`]); пусто — откатывать нечего или нечем.
    pub rollback: Vec<Vec<RollbackItem>>,
    /// Установленные форки Plasma (AeroThemePlasma): маршрут фоновой
    /// проверки ведёт к их сверке только при них.
    pub plasma_forks: Vec<String>,
}

impl Advice {
    pub fn title(&self) -> String {
        title(&self.findings)
    }
    /// Текст окна Rexon с кнопками удаления сирот и отката.
    pub fn text(&self) -> String {
        self.text_for(Audience::Window { removable: true, rollback: true })
    }
    /// Текст для окна либо уведомления ([`Audience`]).
    pub fn text_for(&self, audience: Audience) -> String {
        self.findings
            .iter()
            .zip(&self.rollback)
            .map(|(f, r)| finding_text_for(f, !r.is_empty(), audience))
            .collect::<Vec<_>>()
            .join("\n\n")
    }
    /// Сироты находок ([`orphan_names`]).
    pub fn orphans(&self) -> Vec<String> {
        orphan_names(&self.findings)
    }
    pub fn fingerprint(&self) -> String {
        fingerprint(&self.findings)
    }
}

/// Сверка целиком: [`survey`] → [`evaluate`] → откат из кэша.
pub fn advise<IO: SystemIo + ?Sized>(io: &IO, family: PkgFamily, uid: u32) -> Advice {
    let Some(survey) = survey(io, family, uid, PACKAGE_SETS) else {
        return Advice::default();
    };
    let names: Vec<String> = survey.installed.iter().map(|(n, _)| n.clone()).collect();
    let plasma_forks = crate::system_plan::filter_plasma_forks(&names);
    let findings = evaluate(&survey, PACKAGE_SETS);
    let cache = if findings.iter().any(|f| !f.rollback.is_empty()) { cache_listing(io) } else { Vec::new() };
    let rollback =
        findings.iter().map(|f| rollback_items(f, &survey.installed, &cache, ROLLBACK_BUTTON_MAX)).collect();
    Advice { findings, rollback, plasma_forks }
}

// ── Кнопка «Вернуть прежние версии» ─────────────────────────────────────

/// Что окно предлагает вернуть одной кнопкой: прежние версии ОДНОГО набора.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RollbackOffer {
    /// Имя набора для человека (Qt, Plasma, …).
    pub set_name: String,
    pub items: Vec<RollbackItem>,
}

/// Решение окна об откате. Только «не перезагружайтесь»: новые
/// несогласованные версии уже стоят, а в кэше есть прежние для ВСЕХ
/// пакетов отката. «Не обновляйтесь» — ставить обратно нечего (стоят ещё
/// прежние), «обновитесь сейчас» — чинит обновление, сирота — своя кнопка.
/// На SteamOS — никогда (`/usr` только для чтения). Несколько наборов —
/// первый: остальные покажет повторная сверка после отката.
pub fn rollback_offer(advice: &Advice, steamos: bool) -> Option<RollbackOffer> {
    if steamos {
        return None;
    }
    advice
        .findings
        .iter()
        .zip(&advice.rollback)
        .find(|(f, items)| f.stage == Stage::DontReboot && !items.is_empty())
        .map(|(f, items)| RollbackOffer { set_name: f.set.name.to_string(), items: items.clone() })
}

/// [`rollback_offer`] на живой машине.
pub fn rollback_offer_live(advice: &Advice) -> Option<RollbackOffer> {
    rollback_offer(advice, crate::system::DistroDetector::detect_live().is_steamos())
}

/// Версия для человека: без эпохи и выпуска, если по ним «было» и «станет»
/// различимы; иначе полная.
fn shown_versions(from: &str, to: &str) -> (String, String) {
    let (f, t) = (upstream_version(from), upstream_version(to));
    if f == t || f.is_empty() {
        (from.to_string(), to.to_string())
    } else {
        (f, t)
    }
}

/// Сколько строк «было → станет» показывает окно подтверждения.
const ROLLBACK_SHOWN: usize = 10;

/// Кнопка окна сторожа.
pub fn rollback_button() -> String {
    // TRANSLATORS: Button. Puts back the package versions that were installed before the last update.
    tr("Put Previous Versions Back")
}

/// Заголовок окна подтверждения: «Вернуть прежние версии Qt?».
pub fn rollback_confirm_title(offer: &RollbackOffer) -> String {
    // TRANSLATORS: Window title. %1 is a product name (Qt, Plasma, KDE Frameworks, GNOME).
    trf("Put Back the Previous Versions of %1?", &[&offer.set_name])
}

/// Текст окна подтверждения: перечень «было → станет».
pub fn rollback_confirm_text(offer: &RollbackOffer) -> String {
    let mut lines: Vec<String> = offer
        .items
        .iter()
        .take(ROLLBACK_SHOWN)
        .map(|i| {
            let (from, to) = shown_versions(&i.from, &i.to);
            if from.is_empty() {
                format!("{} → {to}", i.name)
            } else {
                format!("{} {from} → {to}", i.name)
            }
        })
        .collect();
    if offer.items.len() > ROLLBACK_SHOWN {
        let more = (offer.items.len() - ROLLBACK_SHOWN).to_string();
        // TRANSLATORS: Ends a list of packages; %1 is how many more are not shown.
        lines.push(trf("…and %1 more", &[&more]));
    }
    // TRANSLATORS: Before putting the previous versions of packages back. A list of packages follows.
    let lead = tr("Rexon will replace these packages with the versions you had before the update:");
    // TRANSLATORS: After the list of packages that Rexon puts back.
    let tail = tr("Then Rexon checks the packages again and tells you the result.");
    format!("{lead}\n\n{}\n\n{tail}", lines.join("\n"))
}

/// Кнопка «да» окна подтверждения.
pub fn rollback_confirm_button() -> String {
    // Свой контекст: «Put Back» без него делит перевод с другой кнопкой.
    // TRANSLATORS: Button in the window that confirms putting previous package versions back.
    trc("update guard rollback", "Put Back")
}

/// Заголовок окна хода.
pub fn rollback_title(offer: &RollbackOffer) -> String {
    // TRANSLATORS: Window title while Rexon puts previous package versions back. %1 is a product name (Qt, Plasma, …).
    trf("Putting Back the Previous Versions of %1", &[&offer.set_name])
}

/// Итог отката. `rest_clean` — повторная сверка ничего больше не нашла.
pub fn rollback_done_text(offer: &RollbackOffer, rest_clean: bool) -> String {
    // TRANSLATORS: Result. %1 is a product name (Qt, Plasma, KDE Frameworks, GNOME).
    let done = trf("The previous versions of %1 are back. Restarting the computer is safe again.", &[&offer.set_name]);
    if rest_clean {
        format!("{done} {}", tr("Everything else is in order."))
    } else {
        done
    }
}

/// Откат прошёл, а набор всё ещё не сходится — перезагружаться рано.
pub fn rollback_still_mixed_text(offer: &RollbackOffer) -> String {
    // TRANSLATORS: Result. %1 is a product name (Qt, Plasma, KDE Frameworks, GNOME).
    trf("Rexon put the previous versions of %1 back, but the packages still don't fit together.", &[&offer.set_name])
}

/// Откат не удался, а пояснения нет.
pub fn rollback_failed(offer: &RollbackOffer) -> String {
    // TRANSLATORS: %1 is a product name (Qt, Plasma, KDE Frameworks, GNOME).
    trf("Rexon couldn't put the previous versions of %1 back.", &[&offer.set_name])
}

/// Вернуть прежние версии через сеанс прав окна ([`crate::privilege`]).
/// Что каждый файл — пакет кэша root, что это ровно откат набора находки
/// (строго меньшие версии, ничего сверх) — проверяет помощник от root сам;
/// окну он не верит. Прежнего пути через `pkexec` нет намеренно.
///
/// `Err` — готовый текст для человека.
pub fn rollback_with(
    offer: &RollbackOffer,
    run: &mut dyn FnMut(&crate::privilege::op::PrivOp) -> Option<crate::system_exec::RunOut>,
) -> Result<(), String> {
    if offer.items.is_empty() {
        return Err(rollback_failed(offer));
    }
    let files = offer.items.iter().map(|i| i.path.clone()).collect();
    let op = crate::privilege::op::PrivOp::CacheRollback { files };
    let Some(out) = run(&op) else {
        return Err(crate::privilege::client::ClientError::HelperMissing.human_text());
    };
    if out.returncode == Some(0) {
        return Ok(());
    }
    let why = out.stderr.lines().map(str::trim).rfind(|l| !l.is_empty()).map(str::to_string);
    Err(why.unwrap_or_else(|| rollback_failed(offer)))
}

/// [`rollback_with`] на живой машине: строки хода — в `line`.
pub fn rollback_live(offer: &RollbackOffer, line: &mut dyn FnMut(&str)) -> Result<(), String> {
    rollback_with(offer, &mut |op| crate::privilege::run::run_live(op, &[], None, &mut *line))
}

/// Сверка на живой машине (только чтение, без пароля).
pub fn advise_live() -> Advice {
    let io = crate::system_exec::LiveSystemIo;
    let family = crate::system::DistroDetector::detect_live().family_live();
    // SAFETY: getuid не имеет предусловий и не может провалиться.
    let uid = unsafe { libc::getuid() };
    advise(&io, family, uid)
}

fn cache_live() -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    crate::utils::cache_home(std::env::var("XDG_CACHE_HOME").ok().as_deref().filter(|v| !v.is_empty()), &home)
}

fn stamp_live() -> String {
    stamp_path(&cache_live())
}

/// Говорили ли уже об этом состоянии (окно Rexon и уведомление).
pub fn notice_seen_live(fingerprint: &str) -> bool {
    std::fs::read_to_string(stamp_live()).is_ok_and(|last| last.trim() == fingerprint)
}

/// Запомнить, что об этом состоянии сказали.
pub fn notice_mark_live(fingerprint: &str) {
    write_stamp(&stamp_live(), fingerprint);
}

fn write_stamp(stamp: &str, fingerprint: &str) {
    if let Some(dir) = std::path::Path::new(stamp).parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(stamp, fingerprint);
}

/// Файл отпечатка, показанного именно ОКНОМ (рядом с [`stamp_path`]).
/// Уведомление о сироте зовёт «откройте Rexon, чтобы удалить» — значит,
/// окно с кнопкой обязано показаться и после уведомления о том же; общего
/// отпечатка для этого мало.
pub fn window_stamp_path(cache_home: &str) -> String {
    format!("{}/rexon/update-guard-shown", cache_home.trim_end_matches('/'))
}

fn window_stamp_live() -> String {
    window_stamp_path(&cache_live())
}

/// Показывало ли окно Rexon это состояние.
pub fn window_seen_live(fingerprint: &str) -> bool {
    std::fs::read_to_string(window_stamp_live()).is_ok_and(|last| last.trim() == fingerprint)
}

/// Запомнить, что окно показало это состояние (и уведомлению повторять нечего).
pub fn window_mark_live(fingerprint: &str) {
    write_stamp(&window_stamp_live(), fingerprint);
    notice_mark_live(fingerprint);
}

/// Показывать ли окно при запуске (`manual` — по кнопке, всегда). В окне
/// есть кнопка (`actionable`: удалить сироту либо вернуть прежние версии) —
/// смотрим отпечаток ОКНА: после уведомления «откройте Rexon» окно обязано
/// показаться. Иначе — общий с уведомлением: показанное уведомлением окно
/// не повторяет.
pub fn window_should_show(manual: bool, actionable: bool, notified: bool, window_seen: bool) -> bool {
    manual || if actionable { !window_seen } else { !notified }
}

/// argv уведомления: `notify-send` (важное — не гаснет само), без него —
/// служба уведомлений через `dbus-send`. None — показать нечем.
pub fn notify_argv(which: &dyn Fn(&str) -> bool, summary: &str, body: &str) -> Option<Vec<String>> {
    let icon = "rexon";
    if which("notify-send") {
        Some(vec![
            "notify-send".into(),
            format!("--app-name={}", crate::utils::NOTIFY_APP_NAME),
            format!("--icon={icon}"),
            "--urgency=critical".into(),
            summary.to_string(),
            body.to_string(),
        ])
    } else if which("dbus-send") {
        Some(crate::utils::notify_dbus_argv("dbus-send", summary, body, icon, 0))
    } else {
        None
    }
}

/// Дождаться конца транзакции pacman: сверять наполовину обновлённую
/// систему значит пугать зря. false — так и не дождались.
fn wait_pacman_lock_live(out: &mut dyn FnMut(&str)) -> bool {
    let started = std::time::Instant::now();
    while std::path::Path::new(PACMAN_DB_LOCK).exists() {
        if started.elapsed().as_secs() > LOCK_WAIT_SECS {
            out("update-check: package manager still busy, giving up");
            return false;
        }
        std::thread::sleep(std::time::Duration::from_secs(3));
    }
    true
}

/// Безоконная проверка (`rexon --update-check [--login]`), её зовёт
/// пользовательский юнит. Дождаться конца транзакции pacman → сверить наборы
/// (только чтение) → при беде уведомление, один раз на одно состояние →
/// если стоят форки Plasma, их сверка ([`crate::system_de_exec::aeroshell_check_route_live`]).
/// Пароль не спрашивается никогда. → код возврата процесса (0 — проверка
/// прошла, что бы она ни нашла).
pub fn check_route_live(login: bool, out: &mut dyn FnMut(&str)) -> i32 {
    if !wait_pacman_lock_live(out) {
        return 0;
    }
    let advice = advise_live();
    let fp = advice.fingerprint();
    out(&format!("update-check: {}", if fp.is_empty() { "ok" } else { fp.as_str() }));
    let last = std::fs::read_to_string(stamp_live()).ok();
    if should_notify(&advice.findings, &fp, last.as_deref(), login) {
        notice_mark_live(&fp);
        let io = crate::system_exec::LiveSystemIo;
        // На SteamOS ни удалить, ни вернуть пакет нельзя — «откройте Rexon»
        // там не к месту.
        let audience = if crate::system::DistroDetector::detect_live().is_steamos() {
            Audience::Window { removable: false, rollback: false }
        } else {
            Audience::Notification
        };
        if let Some(argv) = notify_argv(&|n| io.which(n), &advice.title(), &advice.text_for(audience)) {
            let _ = io.run(&argv, Some(10), false);
        }
    } else if advice.findings.is_empty() && last.is_some_and(|l| !l.trim().is_empty()) {
        // Набор сошёлся — забыть прежнее: если он разойдётся так же снова,
        // сказать снова.
        notice_mark_live("");
    }
    if !advice.plasma_forks.is_empty() {
        return crate::system_de_exec::aeroshell_check_route_live(login, out);
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn installed(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter().map(|(n, v)| (n.to_string(), v.to_string())).collect()
    }

    /// Qt на машине 07.10.2026 до обновления: всё 6.11.2, webengine тоже.
    fn qt_611() -> Vec<(String, String)> {
        installed(&[
            ("qt6-base", "6.11.2-1"),
            ("qt6-declarative", "6.11.2-1.1"),
            ("qt6-svg", "6.11.2-1.1"),
            ("qt6-wayland", "6.11.2-1.1"),
            ("qt6-webengine", "6.11.2-2"),
            ("qt6ct", "0.11-1"),
            ("firefox", "141.0-1"),
        ])
    }

    fn survey_with(installed: Vec<(String, String)>, checkupdates: &str) -> Survey {
        Survey { installed, groups: Vec::new(), pending: parse_pending_updates(checkupdates), repo: None, foreign: None }
    }

    #[test]
    fn checkupdates_output_parsed() {
        let list = parse_pending_updates("\u{1b}[1mqt6-base\u{1b}[0m 6.11.2-1 -> 6.12.0-2\njunk\n\n==> error\n");
        assert_eq!(
            list,
            vec![PendingUpdate { package: "qt6-base".into(), installed: "6.11.2-1".into(), available: "6.12.0-2".into() }]
        );
    }

    /// Случай 07.10: qt6-base 6.11.2 → 6.12.0 ждёт, qt6-declarative 6.11.2
    /// не ждёт (CachyOS ещё не выложил) — «не обновляйтесь».
    #[test]
    fn qt_base_ahead_of_declarative_says_hold() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let survey = survey_with(qt_611(), "qt6-base 6.11.2-1 -> 6.12.0-2\n");
        let found = evaluate(&survey, PACKAGE_SETS);
        assert_eq!(found.len(), 1, "{found:?}");
        let f = &found[0];
        assert_eq!(f.set.id, "qt6");
        assert_eq!(f.stage, Stage::Hold);
        assert_eq!(f.lead, ("qt6-base".into(), "6.12.0".into()));
        let odd: Vec<&str> = f.odd.iter().map(|(n, _, _)| n.as_str()).collect();
        // webengine 6.11.2 отстаёт законно; qt6ct — не модуль Qt.
        assert_eq!(odd, ["qt6-declarative", "qt6-svg", "qt6-wayland"]);
        assert_eq!(f.rollback, vec![("qt6-base".into(), RollbackTarget::Exact("6.11.2-1".into()))]);
        let text = finding_text(f, true);
        assert!(text.contains("qt6-base 6.12.0 with qt6-declarative 6.11.2"), "{text}");
        assert!(text.contains("Ctrl+Alt+F3"), "{text}");
        // «Не обновляйтесь»: стоят ещё прежние версии — ставить обратно
        // нечего, ни команды, ни кнопки.
        assert!(!text.contains("pacman -U") && !text.contains("previous versions"), "{text}");
        assert_eq!(title(&found), "Don't Update the System Yet");
    }

    /// Всё Qt приходит разом — молчим.
    #[test]
    fn whole_qt_at_once_is_quiet() {
        let survey = survey_with(
            qt_611(),
            "qt6-base 6.11.2-1 -> 6.12.0-2\nqt6-declarative 6.11.2-1.1 -> 6.12.0-1.1\n\
             qt6-svg 6.11.2-1.1 -> 6.12.0-1.1\nqt6-wayland 6.11.2-1.1 -> 6.12.0-1.1\nfirefox 141.0-1 -> 142.0-1\n",
        );
        assert!(evaluate(&survey, PACKAGE_SETS).is_empty());
        // Нет обновлений вовсе — тоже.
        assert!(evaluate(&survey_with(qt_611(), ""), PACKAGE_SETS).is_empty());
    }

    /// webengine отстаёт на серию (как у Arch сейчас) — молчим; обогнать
    /// qt6-base ему нельзя.
    #[test]
    fn webengine_may_lag_but_not_lead() {
        let all_612 = installed(&[
            ("qt6-base", "6.12.0-2"),
            ("qt6-declarative", "6.12.0-1.1"),
            ("qt6-webengine", "6.11.2-2"),
        ]);
        assert!(evaluate(&survey_with(all_612.clone(), ""), PACKAGE_SETS).is_empty());
        let found = evaluate(&survey_with(all_612, "qt6-webengine 6.11.2-2 -> 6.13.0-1\n"), PACKAGE_SETS);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].odd[0].0, "qt6-webengine");
    }

    /// Обновление уже развело Qt и не чинит — «не перезагружайтесь», откат из
    /// кэша к старой серии; ждущее обновление чинит — «обновитесь ещё раз».
    #[test]
    fn already_mixed_qt_dont_reboot_or_update_now() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let mixed = installed(&[
            ("qt6-base", "6.12.0-2"),
            ("qt6-declarative", "6.11.2-1.1"),
            ("qt6-webengine", "6.11.2-2"),
        ]);
        let found = evaluate(&survey_with(mixed.clone(), ""), PACKAGE_SETS);
        assert_eq!(found[0].stage, Stage::DontReboot);
        let cache: Vec<String> = [
            "qt6-base-6.11.1-1-x86_64.pkg.tar.zst",
            "qt6-base-6.11.2-1-x86_64.pkg.tar.zst",
            "qt6-base-6.11.2-1-x86_64.pkg.tar.zst.sig",
            "qt6-base-6.12.0-2-x86_64.pkg.tar.zst",
            "qt6-declarative-6.11.2-1.1-x86_64_v3.pkg.tar.zst",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let caches = |files: &[String]| vec![(PACKAGE_CACHE_DIR.to_string(), files.to_vec())];
        assert_eq!(
            rollback_paths(&found[0], &caches(&cache)),
            vec!["/var/cache/pacman/pkg/qt6-base-6.11.2-1-x86_64.pkg.tar.zst".to_string()]
        );
        // Нужной версии в кэше нет — команды нет, только слова.
        assert!(rollback_paths(&found[0], &caches(&cache[3..])).is_empty());
        assert_eq!(title(&found), "Don't Restart the Computer Yet");

        let fixed = evaluate(&survey_with(mixed, "qt6-declarative 6.11.2-1.1 -> 6.12.0-1.1\n"), PACKAGE_SETS);
        assert_eq!(fixed[0].stage, Stage::UpdateNow);
        assert!(!finding_text(&fixed[0], false).contains("Ctrl+Alt+F3"));
    }

    /// Точечная разница Qt — тоже разнобой (приватный знак — до третьего
    /// числа); у Plasma и KF6 точечные выпуски — норма.
    #[test]
    fn precision_per_set() {
        let mut survey = survey_with(
            installed(&[
                ("qt6-base", "6.12.0-2"),
                ("qt6-declarative", "6.12.0-1"),
                ("plasma-workspace", "6.7.5-3.1"),
                ("plasma-login-manager", "6.7.4-3"),
                ("kwin", "6.7.5-3.1"),
                ("plasma-wayland-protocols", "1.23.0-1"),
                ("kcoreaddons", "6.30.0-1.1"),
                ("kio", "6.30.0-1.1"),
                ("oxygen-icons", "1:6.30.0-1"),
            ]),
            "qt6-base 6.12.0-2 -> 6.12.1-1\n",
        );
        survey.groups = [
            ("plasma", "plasma-workspace"),
            ("plasma", "plasma-login-manager"),
            ("plasma", "kwin"),
            ("kf6", "kcoreaddons"),
            ("kf6", "kio"),
            ("kf6", "oxygen-icons"),
        ]
        .iter()
        .map(|(g, p)| (g.to_string(), p.to_string()))
        .collect();
        let found = evaluate(&survey, PACKAGE_SETS);
        assert_eq!(found.iter().map(|f| f.set.id).collect::<Vec<_>>(), ["qt6"]);
        // Plasma: kwin 6.8 при plasma-workspace 6.7 — разнобой.
        survey.pending = parse_pending_updates("kwin 6.7.5-3.1 -> 6.8.0-1\n");
        let found = evaluate(&survey, PACKAGE_SETS);
        assert_eq!(found.iter().map(|f| f.set.id).collect::<Vec<_>>(), ["plasma"]);
        assert_eq!(found[0].lead, ("plasma-workspace".into(), "6.7.5".into()));
        assert_eq!(found[0].odd[0].0, "kwin");
    }

    /// CachyOS #604: gdk-pixbuf2 обновляется, glycin остаётся 2.1.5, хотя в
    /// extra уже 2.2.2 — разнобой. glycin свежий везде — молчим.
    #[test]
    fn pixbuf_without_new_glycin_says_hold() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let base = installed(&[("gdk-pixbuf2", "2.44.7-1.1"), ("glycin", "2.1.5-2.1"), ("firefox", "141.0-1")]);
        let mut survey = survey_with(base, "gdk-pixbuf2 2.44.7-1.1 -> 2.44.8-1.1\n");
        survey.repo = Some(parse_sync_list(
            "cachyos-extra-znver4 gdk-pixbuf2 2.44.8-1.1 [installed: 2.44.7-1.1]\n\
             cachyos-extra-znver4 glycin 2.1.5-2.1 [installed]\n\
             extra gdk-pixbuf2 2.44.8-1 [installed: 2.44.7-1.1]\n\
             extra glycin 2.2.2-1 [installed: 2.1.5-2.1]\n",
        ));
        let found = evaluate(&survey, PACKAGE_SETS);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].set.id, "pixbuf");
        assert_eq!(found[0].stage, Stage::Hold);
        assert_eq!(found[0].odd, vec![("glycin".into(), "2.1.5".into(), Some("2.2.2".into()))]);
        let text = finding_text(&found[0], false);
        assert!(text.contains("gdk-pixbuf2 2.44.8"), "{text}");
        assert!(text.contains("version 2.1.5") && text.contains("version 2.2.2"), "{text}");
        assert!(text.contains("Firefox"), "{text}");
        assert!(!text.contains("Ctrl+Alt+F3"), "{text}");

        // glycin в репозиториях не новее установленного — обычное обновление.
        survey.repo = Some(parse_sync_list("extra glycin 2.1.5-2 [installed]\n"));
        assert!(evaluate(&survey, PACKAGE_SETS).is_empty());
        // Версий репозиториев не узнали — не судим.
        survey.repo = None;
        assert!(evaluate(&survey, PACKAGE_SETS).is_empty());
    }

    /// GNOME: gnome-shell 52 при mutter 51.
    #[test]
    fn gnome_shell_needs_its_mutter() {
        let survey = survey_with(
            installed(&[("mutter", "51.0-1"), ("gnome-shell", "1:51.0-1.1")]),
            "gnome-shell 1:51.0-1.1 -> 1:52.0-1\n",
        );
        let found = evaluate(&survey, PACKAGE_SETS);
        assert_eq!(found[0].set.id, "gnome");
        assert!(evaluate(&survey_with(installed(&[("mutter", "51.0-1"), ("gnome-shell", "1:51.1-1")]), ""), PACKAGE_SETS)
            .is_empty());
    }

    #[test]
    fn route_flag_leads_to_update_check() {
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(crate::app::route(&args(&[CHECK_FLAG])), crate::app::Route::UpdateCheck { login: false });
        assert_eq!(
            crate::app::route(&args(&[CHECK_FLAG, "--login"])),
            crate::app::Route::UpdateCheck { login: true }
        );
    }

    #[test]
    fn cache_file_names_parsed() {
        assert_eq!(
            parse_cache_file("qt6-base-6.11.2-1-x86_64.pkg.tar.zst"),
            Some(("qt6-base".into(), "6.11.2-1".into()))
        );
        assert_eq!(
            parse_cache_file("gtk3-1:3.24.52-2.1-x86_64_v3.pkg.tar.zst"),
            Some(("gtk3".into(), "1:3.24.52-2.1".into()))
        );
        assert_eq!(parse_cache_file("qt6-base-6.11.2-1-x86_64.pkg.tar.zst.sig"), None);
        assert_eq!(parse_cache_file("download-abc"), None);
    }

    #[test]
    fn checkupdates_db_like_checkupdates() {
        assert_eq!(checkupdates_db("", "", 1000), "/tmp/checkup-db-1000/");
        assert_eq!(checkupdates_db("", "/var/tmp/", 1000), "/var/tmp/checkup-db-1000/");
        assert_eq!(checkupdates_db("/x/db", "/var/tmp", 1000), "/x/db");
    }

    /// Об одном состоянии — один раз; новое состояние — снова; при входе —
    /// всегда. Тексты без жаргона служебных программ.
    #[test]
    fn once_per_state_and_plain_texts() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let a = evaluate(&survey_with(qt_611(), "qt6-base 6.11.2-1 -> 6.12.0-2\n"), PACKAGE_SETS);
        let b = evaluate(&survey_with(qt_611(), "qt6-base 6.11.2-1 -> 6.12.1-1\n"), PACKAGE_SETS);
        let (fa, fb) = (fingerprint(&a), fingerprint(&b));
        assert_ne!(fa, fb);
        assert!(should_notify(&a, &fa, None, false));
        assert!(!should_notify(&a, &fa, Some(&format!("{fa}\n")), false));
        assert!(should_notify(&b, &fb, Some(&fa), false));
        assert!(should_notify(&a, &fa, Some(&fa), true));
        assert!(!should_notify(&[], "", None, true));
        let text = finding_text(&a[0], false);
        for jargon in ["checkupdates", "IgnorePkg", "QtPrivate", "cachyos"] {
            assert!(!text.contains(jargon), "жаргон «{jargon}»: {text}");
        }
    }

    #[test]
    fn notify_argv_prefers_notify_send() {
        let argv = notify_argv(&|n| n == "notify-send", "T", "B").unwrap();
        assert_eq!(argv[0], "notify-send");
        assert!(argv.contains(&"--urgency=critical".to_string()));
        assert_eq!(&argv[argv.len() - 2..], ["T", "B"]);
        assert_eq!(notify_argv(&|n| n == "dbus-send", "T", "B").unwrap()[0], "dbus-send");
        assert!(notify_argv(&|_| false, "T", "B").is_none());
    }

    #[test]
    fn watch_units_watch_pacman_db_without_rate_limit() {
        let exec = "/usr/lib/rexon/rexon-main-window --update-check";
        let units = watch_units(exec, Some("/usr/lib/rexon/rexon-main-window"));
        let text = |name: &str| units.iter().find(|(n, _)| *n == name).unwrap().1.clone();
        let path = text(WATCH_PATH_UNIT);
        assert!(path.contains("\nPathChanged=/var/lib/pacman/local\n"), "{path}");
        assert!(path.contains("\nUnit=rexon-update-check.service\n"), "{path}");
        assert!(path.contains("\nTriggerLimitIntervalSec=0\n"), "{path}");
        assert!(text(WATCH_TIMER_UNIT).contains("\nOnStartupSec=90s\n"));
        assert!(text(WATCH_TIMER_UNIT).contains("\nOnUnitActiveSec=6h\n"));
        let service = text(WATCH_SERVICE_UNIT);
        assert!(service.contains("\nType=oneshot\n"), "{service}");
        assert!(service.contains("\nKillMode=process\n"), "{service}");
        assert!(service.contains(&format!("\nExecStart={exec}\n")), "{service}");
        assert!(service.contains("\nConditionPathExists=/usr/lib/rexon/rexon-main-window\n"), "{service}");
        assert!(!watch_units(exec, None)[2].1.contains("ConditionPathExists"));
        // Ни root, ни пароля: юниты только пользовательские.
        for (_, t) in &units {
            for word in ["pkexec", "sudo", "User=", "/etc/"] {
                assert!(!t.contains(word), "«{word}» в юните: {t}");
            }
        }
    }

    #[test]
    fn watch_plan_is_idempotent_removable_and_drops_legacy_units() {
        let exec = "/x/rexon --update-check";
        let none = |_: &str| None;
        let fresh = watch_plan("/h/.config", true, Some(exec), Some("/x/rexon"), &none);
        assert_eq!(fresh.write.len(), 3);
        assert!(fresh.write[0].0.starts_with("/h/.config/systemd/user/"));
        assert_eq!(fresh.systemctl[0], vec!["daemon-reload".to_string()]);
        assert_eq!(fresh.systemctl[1][..2], ["enable".to_string(), "--now".to_string()]);
        let written: std::collections::HashMap<String, String> = fresh.write.iter().cloned().collect();
        let have = |p: &str| written.get(p).cloned();
        assert_eq!(watch_plan("/h/.config", true, Some(exec), Some("/x/rexon"), &have), WatchPlan::default());
        // Сменилась строка запуска — переписать.
        assert_eq!(watch_plan("/h/.config", true, Some("/y/rexon --update-check"), Some("/x/rexon"), &have).write.len(), 1);
        // Не нужна либо заводить нечем — снять.
        let off = watch_plan("/h/.config", false, Some(exec), None, &have);
        assert_eq!(off.remove.len(), 3);
        assert_eq!(off.systemctl[0][0], "disable");
        assert_eq!(watch_plan("/h/.config", true, None, None, &have).remove.len(), 3);
        assert_eq!(watch_plan("/h/.config", false, None, None, &none), WatchPlan::default());
        // Прежние юниты AeroThemePlasma снимаются, новые ставятся.
        let current = |p: &str| written.get(p).cloned().or_else(|| p.contains("rexon-aeroshell-check").then(String::new));
        let migrate = watch_plan("/h/.config", true, Some(exec), Some("/x/rexon"), &current);
        assert_eq!(migrate.remove.len(), 3);
        assert!(migrate.write.is_empty());
        assert_eq!(
            migrate.systemctl,
            vec![
                vec!["disable".to_string(), "--now".into(), "rexon-aeroshell-check.path".into(), "rexon-aeroshell-check.timer".into()],
                vec!["daemon-reload".to_string()],
            ]
        );
    }

    /// Подставной pacman: argv (через пробел) → (код, вывод). Чего нет в
    /// таблице — код 1 без вывода.
    struct PacmanIo {
        answers: Vec<(&'static str, i32, &'static str)>,
        /// Каталоги кэша: каталог → файлы.
        dirs: Vec<(&'static str, &'static [&'static str])>,
    }
    impl SystemIo for PacmanIo {
        fn exists(&self, _: &str) -> bool { false }
        fn is_file(&self, _: &str) -> bool { false }
        fn is_dir(&self, _: &str) -> bool { false }
        fn is_executable(&self, _: &str) -> bool { false }
        fn realpath(&self, _: &str) -> Option<String> { None }
        fn read_text(&self, _: &str) -> Option<String> { None }
        fn list_dir(&self, d: &str) -> Option<Vec<String>> {
            self.dirs.iter().find(|(k, _)| *k == d).map(|(_, f)| f.iter().map(|s| s.to_string()).collect())
        }
        fn which(&self, n: &str) -> bool { matches!(n, "pacman" | "checkupdates") }
        fn run(&self, argv: &[String], _: Option<u64>, _: bool) -> crate::system_exec::RunOut {
            let key = argv.join(" ");
            let (rc, out) = self.answers.iter().find(|(k, _, _)| *k == key).map(|(_, rc, o)| (*rc, *o)).unwrap_or((1, ""));
            crate::system_exec::RunOut {
                outcome: crate::utils::RunOutcome::Ok,
                returncode: Some(rc),
                stdout: out.into(),
                stderr: String::new(),
            }
        }
        fn env(&self, _: &str) -> String { String::new() }
        fn which_path(&self, _: &str) -> Option<String> { None }
        fn write_text(&self, _: &str, _: &str) -> Result<(), String> { Err("ro".into()) }
        fn copy_file(&self, _: &str, _: &str) -> Result<(), String> { Err("ro".into()) }
    }

    const PLASMA_QG: &str = "plasma plasma-workspace\nplasma kwin\nplasma kwin-x11\n";

    fn plasma_io(q: &'static str, checkupdates: (i32, &'static str), qm: (i32, &'static str)) -> PacmanIo {
        PacmanIo {
            answers: vec![
                ("checkupdates", checkupdates.0, checkupdates.1),
                ("pacman -Q", 0, q),
                ("pacman -Qg kf6 plasma", 0, PLASMA_QG),
                ("pacman -Qmq", qm.0, qm.1),
            ],
            dirs: Vec::new(),
        }
    }

    /// Кнопка «Вернуть прежние версии»: Plasma вразнобой (plasma-workspace и
    /// kwin 6.8.1 при kwin-x11 6.7.5 из репозиториев) — откат к 6.7 из ДВУХ
    /// каталогов кэша (`pacman-conf CacheDir`), «было → станет», тексты без
    /// команд; без полного кэша, на SteamOS и в прочих стадиях — кнопки нет.
    #[test]
    fn rollback_button_only_when_mixed_and_cache_complete() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let q = "plasma-workspace 6.8.1-1.1\nkwin 6.8.1-2.1\nkwin-x11 6.7.5-3.1\nfirefox 141.0-1\n";
        let mut io = plasma_io(q, (2, ""), (0, "decky-loader\n"));
        io.answers.push(("pacman-conf CacheDir", 0, "/var/cache/pacman/pkg/\n/mnt/cache/\n"));
        io.dirs = vec![
            ("/var/cache/pacman/pkg", &["plasma-workspace-6.7.5-3.1-x86_64_v4.pkg.tar.zst", "kwin-6.8.1-2.1-x86_64.pkg.tar.zst"]),
            ("/mnt/cache", &["kwin-6.7.4-1-x86_64.pkg.tar.zst", "kwin-6.7.5-3.1-x86_64.pkg.tar.zst", "kwin-6.7.5-3.1-x86_64.pkg.tar.zst.sig"]),
        ];
        let advice = advise(&io, PkgFamily::Arch, 1000);
        assert_eq!(advice.findings[0].stage, Stage::DontReboot);
        let offer = rollback_offer(&advice, false).expect("кнопка");
        assert_eq!(offer.set_name, "Plasma");
        assert_eq!(
            offer.items,
            vec![
                RollbackItem {
                    name: "plasma-workspace".into(),
                    from: "6.8.1-1.1".into(),
                    to: "6.7.5-3.1".into(),
                    path: "/var/cache/pacman/pkg/plasma-workspace-6.7.5-3.1-x86_64_v4.pkg.tar.zst".into(),
                },
                RollbackItem {
                    name: "kwin".into(),
                    from: "6.8.1-2.1".into(),
                    to: "6.7.5-3.1".into(),
                    path: "/mnt/cache/kwin-6.7.5-3.1-x86_64.pkg.tar.zst".into(),
                },
            ]
        );
        assert_eq!(rollback_offer(&advice, true), None, "SteamOS");
        let window = advice.text();
        assert!(window.contains("Rexon can put the previous versions back"), "{window}");
        let notice = advice.text_for(Audience::Notification);
        assert!(notice.contains("open Rexon to put the previous versions back"), "{notice}");
        let quiet = advice.text_for(Audience::Window { removable: false, rollback: false });
        assert!(!quiet.contains("previous versions"), "{quiet}");
        let confirm = rollback_confirm_text(&offer);
        assert!(confirm.contains("plasma-workspace 6.8.1 → 6.7.5\nkwin 6.8.1 → 6.7.5"), "{confirm}");
        assert_eq!(rollback_confirm_title(&offer), "Put Back the Previous Versions of Plasma?");
        // Абзац консоли («Ctrl+Alt+F3 … sudo pacman -Syu») — для человека за
        // консолью при чёрном экране, он не про откат; команды отката нет нигде.
        for t in [&window, &notice, &confirm] {
            for jargon in ["pacman -U", "/var/cache", "cache", ".pkg.tar"] {
                assert!(!t.contains(jargon), "жаргон «{jargon}»: {t}");
            }
        }

        // Прежней kwin в кэше нет — ни кнопки, ни слов об откате.
        io.dirs.truncate(1);
        let partial = advise(&io, PkgFamily::Arch, 1000);
        assert_eq!(rollback_offer(&partial, false), None);
        assert!(!partial.text().contains("previous versions"), "{}", partial.text());
        assert!(!partial.text_for(Audience::Notification).contains("previous versions"));

        // Ждущее обновление чинит — «обновитесь», кнопки нет.
        let pending = advise(&plasma_io(q, (0, "kwin-x11 6.7.5-3.1 -> 6.8.1-1\n"), (1, "")), PkgFamily::Arch, 1000);
        assert_eq!(pending.findings[0].stage, Stage::UpdateNow);
        assert_eq!(rollback_offer(&pending, false), None);
    }

    /// «Не обновляйтесь»: прежняя версия в кэше есть (ровно та, что стоит),
    /// но ставить обратно нечего — кнопки нет; сирота — тоже не откат.
    #[test]
    fn no_rollback_button_before_update_or_for_orphan() {
        let survey = survey_with(qt_611(), "qt6-base 6.11.2-1 -> 6.12.0-2\n");
        let findings = evaluate(&survey, PACKAGE_SETS);
        assert_eq!(findings[0].stage, Stage::Hold);
        let caches = vec![(PACKAGE_CACHE_DIR.to_string(), vec!["qt6-base-6.11.2-1-x86_64.pkg.tar.zst".to_string()])];
        let rollback: Vec<Vec<RollbackItem>> =
            findings.iter().map(|f| rollback_items(f, &survey.installed, &caches, ROLLBACK_BUTTON_MAX)).collect();
        assert!(!rollback[0].is_empty(), "файл в кэше есть");
        let advice = Advice { findings, rollback, plasma_forks: Vec::new() };
        assert_eq!(rollback_offer(&advice, false), None);
        assert!(!advice.text().contains("previous versions"));
        assert!(!advice.text_for(Audience::Notification).contains("previous versions"));
        // Предел кнопки и консоли разный: консоли — 8, кнопке — 64.
        const _: () = assert!(ROLLBACK_MAX_PACKAGES < ROLLBACK_BUTTON_MAX);
    }

    /// Каталоги кэша: из `pacman-conf`, без повторов и странных путей.
    #[test]
    fn cache_dirs_from_pacman_conf() {
        assert_eq!(parse_cache_dirs(""), [PACKAGE_CACHE_DIR]);
        assert_eq!(
            parse_cache_dirs("/var/cache/pacman/pkg/\n/mnt/c/\nrel/\n/a/../b\n/\n/mnt/c\n"),
            [PACKAGE_CACHE_DIR, "/mnt/c"]
        );
        let t = RollbackTarget::Series(Precision::Minor, vec![6, 7]);
        assert!(t.accepts("6.7.5-3.1") && t.accepts("1:6.7.0-1") && !t.accepts("6.8.1-1"));
        assert!(RollbackTarget::Exact("6.11.2-1".into()).accepts("6.11.2-1"));
        assert!(!RollbackTarget::Exact("6.11.2-1".into()).accepts("6.11.2-2"));
    }

    /// Откат: операция сеанса прав с путями кэша, итог — словами.
    #[test]
    fn rollback_goes_through_privilege_session() {
        use crate::privilege::op::PrivOp;
        use crate::system_exec::RunOut;
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let item = |n: &str| RollbackItem {
            name: n.into(),
            from: "6.12.0-2".into(),
            to: "6.11.2-1".into(),
            path: format!("/var/cache/pacman/pkg/{n}-6.11.2-1-x86_64.pkg.tar.zst"),
        };
        let offer = RollbackOffer { set_name: "Qt".into(), items: vec![item("qt6-base")] };
        let out = |rc: i32, stderr: &str| RunOut {
            outcome: crate::utils::RunOutcome::Ok,
            returncode: Some(rc),
            stdout: String::new(),
            stderr: stderr.into(),
        };
        let mut seen = Vec::new();
        assert_eq!(rollback_with(&offer, &mut |op| { seen.push(op.clone()); Some(out(0, "")) }), Ok(()));
        assert_eq!(
            seen,
            vec![PrivOp::CacheRollback { files: vec!["/var/cache/pacman/pkg/qt6-base-6.11.2-1-x86_64.pkg.tar.zst".into()] }]
        );
        assert_eq!(rollback_with(&offer, &mut |_| Some(out(1, "x\nRexon didn't: y"))), Err("Rexon didn't: y".into()));
        assert_eq!(rollback_with(&offer, &mut |_| Some(out(1, ""))), Err("Rexon couldn't put the previous versions of Qt back.".into()));
        let missing = rollback_with(&offer, &mut |_| None).unwrap_err();
        assert!(!missing.contains("pkexec") && !missing.is_empty(), "{missing}");
        let empty = RollbackOffer { set_name: "Qt".into(), items: Vec::new() };
        let mut called = false;
        assert!(rollback_with(&empty, &mut |_| { called = true; None }).is_err());
        assert!(!called);
        assert_eq!(rollback_title(&offer), "Putting Back the Previous Versions of Qt");
        assert_eq!(
            rollback_done_text(&offer, true),
            "The previous versions of Qt are back. Restarting the computer is safe again. Everything else is in order."
        );
        // Длинный перечень — первые десять и «ещё».
        let many = RollbackOffer { set_name: "Plasma".into(), items: (0..13).map(|i| item(&format!("p{i}"))).collect() };
        let text = rollback_confirm_text(&many);
        assert!(text.contains("p9 6.12.0 → 6.11.2\n…and 3 more") && !text.contains("p10 "), "{text}");
    }

    /// Три случая Plasma 6.8 на подставных выводах pacman: набор вразнобой
    /// (все в репозиториях, обновлений нет), отставший ждёт обновления, и
    /// пакет-сирота вне репозиториев (машина Wehrwolfmann 25.10.2026:
    /// `kwin-x11` 6.7.5, `pacman -Si kwin-x11` пусто, `pacman -Qm` его
    /// называет).
    #[test]
    fn plasma_mix_pending_and_orphan_told_apart() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let q = "plasma-workspace 6.8.1-1.1\nkwin 6.8.1-2.1\nkwin-x11 6.7.5-3.1\nfirefox 141.0-1\n";

        // 1. Вразнобой: kwin-x11 в репозиториях есть (сирот нет), ждущего нет.
        let mix = advise(&plasma_io(q, (2, ""), (0, "decky-loader\n")), PkgFamily::Arch, 1000);
        assert_eq!(mix.findings.len(), 1, "{:?}", mix.findings);
        assert_eq!(mix.findings[0].stage, Stage::DontReboot);
        assert!(mix.text().contains("parts of Plasma of different versions"), "{}", mix.text());

        // 2. Ждёт обновления: kwin-x11 6.8.1 уже в репозиториях.
        let pending = advise(&plasma_io(q, (0, "kwin-x11 6.7.5-3.1 -> 6.8.1-1\n"), (1, "")), PkgFamily::Arch, 1000);
        assert_eq!(pending.findings.len(), 1, "{:?}", pending.findings);
        assert_eq!(pending.findings[0].stage, Stage::UpdateNow);
        assert_eq!(pending.title(), "Update the System Once More");

        // 3. Сирота: kwin-x11 нет в репозиториях.
        let orphan = advise(&plasma_io(q, (2, ""), (0, "decky-loader\nkwin-x11\n")), PkgFamily::Arch, 1000);
        assert_eq!(orphan.findings.len(), 1, "{:?}", orphan.findings);
        let f = &orphan.findings[0];
        assert_eq!(f.stage, Stage::Orphaned);
        assert_eq!(f.lead, ("plasma-workspace".into(), "6.8.1".into()));
        assert_eq!(f.odd, vec![("kwin-x11".into(), "6.7.5".into(), None)]);
        assert!(f.rollback.is_empty());
        let text = orphan.text();
        assert!(
            text.contains(
                "kwin-x11 has been removed from the repositories and no longer gets updates: it is left over \
                 from an earlier version (kwin-x11 6.7.5 with plasma-workspace 6.8.1)."
            ),
            "{text}"
        );
        assert!(text.contains("The Plasma (X11) session is not supported"), "{text}");
        // Окну — без команды: удаляет кнопка «Удалить kwin-x11».
        assert!(text.contains("If you don't use it, Rexon can remove it."), "{text}");
        assert_eq!(orphan.orphans(), ["kwin-x11"]);
        assert!(removable_orphans(&orphan.findings, true).is_empty(), "SteamOS: удалить нельзя");
        assert_eq!(orphan_remove_button(&orphan.orphans()), "Remove kwin-x11");
        // Уведомлению — «откройте Rexon»; на SteamOS (кнопки нет) — молчим.
        let notice = orphan.text_for(Audience::Notification);
        assert!(notice.contains("open Rexon to remove it"), "{notice}");
        let steamos = orphan.text_for(Audience::Window { removable: false, rollback: false });
        assert!(steamos.contains("kwin-x11 has been removed from the repositories") && !steamos.contains("remove it"), "{steamos}");
        for t in [&text, &notice, &steamos] {
            for jargon in ["sudo", "pacman", "-Rns"] {
                assert!(!t.contains(jargon), "жаргон «{jargon}»: {t}");
            }
        }
        // Консоли спасения команда остаётся.
        let console = orphan_advice(f, &|m: &str| m.to_string()).unwrap();
        assert_eq!(console, "If you don't use it, remove the package: sudo pacman -Rns kwin-x11");
        assert!(!text.contains("different versions") && !text.contains("Ctrl+Alt+F3"), "{text}");
        assert_eq!(orphan.title(), "A Package No Longer Gets Updates");
        assert_ne!(orphan.fingerprint(), mix.fingerprint());

        // Сирот не узнали (pacman -Qm не смог) — прежнее «вразнобой».
        let unknown = advise(&plasma_io(q, (2, ""), (3, "")), PkgFamily::Arch, 1000);
        assert_eq!(unknown.findings, mix.findings);
    }

    /// Обновление Plasma ждёт, а сирота за ним не пойдёт: не «не
    /// обновляйтесь» навсегда, а «удалите»; вразнобой у остальных — отдельно
    /// и первым.
    #[test]
    fn orphan_does_not_hold_the_update() {
        let mut survey = survey_with(
            installed(&[("plasma-workspace", "6.7.5-3.1"), ("kwin", "6.7.5-3.1"), ("kwin-x11", "6.7.5-3.1")]),
            "plasma-workspace 6.7.5-3.1 -> 6.8.1-1.1\nkwin 6.7.5-3.1 -> 6.8.1-2.1\n",
        );
        survey.groups = ["plasma-workspace", "kwin", "kwin-x11"].iter().map(|p| ("plasma".into(), p.to_string())).collect();
        // Без сведений о сиротах — прежний «не обновляйтесь».
        assert_eq!(evaluate(&survey, PACKAGE_SETS)[0].stage, Stage::Hold);
        survey.foreign = Some(parse_foreign("kwin-x11\n"));
        let found = evaluate(&survey, PACKAGE_SETS);
        assert_eq!(found.iter().map(|f| f.stage).collect::<Vec<_>>(), [Stage::Orphaned]);
        // Сирота той же серии, обновления нет — молчим.
        survey.pending.clear();
        assert!(evaluate(&survey, PACKAGE_SETS).is_empty());
        // Отстал и kwin (в репозиториях) — вразнобой первым, сирота вторым.
        survey.installed[0].1 = "6.8.1-1.1".into();
        let found = evaluate(&survey, PACKAGE_SETS);
        assert_eq!(found.iter().map(|f| f.stage).collect::<Vec<_>>(), [Stage::DontReboot, Stage::Orphaned]);
        assert_eq!(found[0].odd.iter().map(|(n, _, _)| n.as_str()).collect::<Vec<_>>(), ["kwin"]);
        for t in ORPHAN_TEXTS {
            assert!(crate::system_l10n::translation_ok(t, t), "{t}");
        }
    }

    /// Кого помощник от root берётся удалять: член набора «одной версией»,
    /// не опорный; группы — по `pacman -Qg`.
    #[test]
    fn orphan_candidate_only_set_members() {
        let groups = parse_groups(PLASMA_QG);
        assert!(orphan_candidate("kwin-x11", &groups));
        assert!(orphan_candidate("qt6-svg", &[]));
        assert!(orphan_candidate("gnome-shell", &[]));
        // Опорные сиротой не считаются.
        for anchor in ["plasma-workspace", "qt6-base", "mutter"] {
            assert!(!orphan_candidate(anchor, &groups), "{anchor}");
        }
        // Не член набора; член набора «двигаться вместе»; группа не та.
        for other in ["firefox", "glycin", "decky-loader"] {
            assert!(!orphan_candidate(other, &groups), "{other}");
        }
        assert!(!orphan_candidate("kwin-x11", &[]));
    }

    /// Сирота, на которой держится сеанс: сеанс окна и сеанс экрана входа
    /// по умолчанию; окно заранее говорит, откуда удалить.
    #[test]
    fn orphan_session_in_use_and_login_default() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let k = vec!["kwin-x11".to_string()];
        assert_eq!(orphan_in_use(&k, "X11", "kde").map(|s| s.package), Some("kwin-x11"));
        assert!(orphan_in_use(&k, "wayland", "kde").is_none());
        assert!(orphan_in_use(&k, "x11", "xfce").is_none());
        assert!(orphan_in_use(&["qt6-svg".to_string()], "x11", "kde").is_none());
        for v in ["plasmax11", "plasmax11.desktop", "/usr/share/xsessions/plasmax11.desktop", " plasmax11 "] {
            assert_eq!(session_file_name(v), "plasmax11", "{v}");
        }
        assert!(orphan_login_default(&k, &["/usr/share/xsessions/plasmax11.desktop".into()]).is_some());
        assert!(orphan_login_default(&k, &["plasma".into(), "plasma.desktop".into()]).is_none());

        // Порядок чтения: каталог по имени, затем файл; поздний перекрывает.
        let files: std::collections::HashMap<&str, &str> = [
            ("/usr/lib/sddm/sddm.conf.d/default.conf", "[Autologin]\nSession=plasmax11\n"),
            ("/etc/sddm.conf.d/kde_settings.conf", "[Autologin]\nSession=plasma\n"),
            ("/etc/plasmalogin.conf", "[Autologin]\nSession=\n"),
            ("/var/lib/sddm/state.conf", "[Last]\nSession=/usr/share/wayland-sessions/plasma.desktop\n"),
        ]
        .into_iter()
        .collect();
        let read = |p: &str| files.get(p).map(|t| t.to_string());
        let list = |d: &str| {
            files.keys().filter_map(|p| p.strip_prefix(&format!("{d}/")).map(str::to_string)).collect::<Vec<_>>()
        };
        assert_eq!(login_default_sessions(&read, &list), ["plasma", "/usr/share/wayland-sessions/plasma.desktop"]);

        // Решение окна: X11-сеанс Plasma — текст вместо кнопки.
        let f = Finding {
            set: PACKAGE_SETS[2],
            stage: Stage::Orphaned,
            lead: ("plasma-workspace".into(), "6.8.1".into()),
            odd: vec![("kwin-x11".into(), "6.7.5".into(), None)],
            rollback: Vec::new(),
        };
        let blocked = orphan_offer(std::slice::from_ref(&f), false, "x11", "kde");
        let OrphanOffer::Blocked(text) = &blocked else { panic!("{blocked:?}") };
        assert_eq!(
            text,
            "You are using Plasma (X11) right now, and it runs on this package. Sign in to Plasma (Wayland) and remove it from there."
        );
        assert_eq!(orphan_offer(std::slice::from_ref(&f), false, "wayland", "kde"), OrphanOffer::Remove(k.clone()));
        assert_eq!(orphan_offer(std::slice::from_ref(&f), true, "x11", "kde"), OrphanOffer::Nothing);
        assert_eq!(orphan_offer(&[], false, "x11", "kde"), OrphanOffer::Nothing);
        // Тексты отказов помощника — те же, по имени пакета.
        let s = orphan_session_of("kwin-x11").unwrap();
        assert_eq!(crate::privilege::client::key_text("refused.orphan-in-use.kwin-x11"), orphan_in_use_text(s));
        assert_eq!(crate::privilege::client::key_text("refused.orphan-login-default.kwin-x11"), orphan_login_default_text(s));
        // Отказ «экран входа открывает сеанс» опознаётся окном — и только он.
        let why = crate::privilege::client::key_text("refused.orphan-login-default.kwin-x11");
        assert_eq!(login_default_successor(&k, &why).map(|p| p.wayland), Some("plasma"));
        assert!(login_default_successor(&k, &orphan_in_use_text(s)).is_none());
        assert!(login_default_successor(&["qt6-svg".to_string()], &why).is_none());
        // Каждая запись таблицы — член набора сторожа, и у каждого её сеанса
        // есть пара на Wayland (кнопка «Сделать … сеансом по умолчанию»).
        for s in ORPHAN_SESSIONS {
            assert!(orphan_candidate(s.package, &parse_groups(PLASMA_QG)), "{}", s.package);
            for x11 in s.sessions {
                let pair = crate::system_plan::session_successor(x11).unwrap_or_else(|| panic!("{x11}"));
                assert_eq!((pair.x11_name, pair.wayland_name), (s.session_name, s.instead), "{x11}");
            }
        }
    }

    /// Когда окно показывается: сирота с кнопкой — по отпечатку окна (после
    /// уведомления «откройте Rexon» — непременно), прочее — по общему.
    #[test]
    fn window_after_notice_only_when_it_can_act() {
        assert!(window_should_show(true, false, true, true));
        assert!(window_should_show(false, true, true, false));
        assert!(!window_should_show(false, true, true, true));
        assert!(!window_should_show(false, false, true, false));
        assert!(window_should_show(false, false, false, true));
        // Кнопка отката — тоже «окну есть что сделать»: после уведомления
        // «откройте Rexon, чтобы вернуть» окно показывается.
        assert!(window_should_show(false, true, true, false));
        assert_eq!(window_stamp_path("/h/.cache/"), "/h/.cache/rexon/update-guard-shown");
        assert_ne!(window_stamp_path("/h/.cache"), stamp_path("/h/.cache"));
    }

    /// Удаление сироты: операция сеанса прав, итог — словами.
    #[test]
    fn remove_orphans_goes_through_privilege_session() {
        use crate::privilege::op::PrivOp;
        use crate::system_exec::RunOut;
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let names = vec!["kwin-x11".to_string()];
        let out = |rc: i32, stdout: &str, stderr: &str| RunOut {
            outcome: crate::utils::RunOutcome::Ok,
            returncode: Some(rc),
            stdout: stdout.into(),
            stderr: stderr.into(),
        };
        let mut seen = Vec::new();
        let ok = remove_orphans_with(&names, &mut |op| {
            seen.push(op.clone());
            Some(out(0, "removing kwin-x11...\n", ""))
        });
        assert_eq!(ok, Ok(()));
        assert_eq!(seen, vec![PrivOp::OrphanRemove { names: names.clone() }]);
        // Отказ помощника — его готовый текст.
        let refused = remove_orphans_with(&names, &mut |_| Some(out(1, "", "Rexon didn't remove the package: x")));
        assert_eq!(refused, Err("Rexon didn't remove the package: x".to_string()));
        // pacman не смог — общий итог без его строк.
        let failed = remove_orphans_with(&names, &mut |_| Some(out(1, "error: failed to commit transaction\n", "")));
        assert_eq!(failed, Err("Rexon couldn't remove kwin-x11.".to_string()));
        // Помощника нет — прежнего пути нет: сказать, без pkexec.
        let missing = remove_orphans_with(&names, &mut |_| None).unwrap_err();
        assert!(!missing.contains("pkexec") && !missing.is_empty(), "{missing}");
        // Пусто — ни одной операции.
        let mut called = false;
        assert!(remove_orphans_with(&[], &mut |_| { called = true; None }).is_err());
        assert!(!called);
        // Подписи окна.
        assert_eq!(orphan_remove_title(&names), "Removing kwin-x11");
        assert_eq!(orphan_removed_text(&names, true), "Removed: kwin-x11. Everything else is in order.");
        assert_eq!(orphan_removed_text(&names, false), "Removed: kwin-x11.");
        assert_eq!(removable_orphans(&[], false), Vec::<String>::new());
    }

    /// Сверка через подставной ввод-вывод: какие команды, без root.
    #[test]
    fn survey_asks_pacman_and_checkupdates_only() {
        use crate::system_exec::RunOut;
        use std::cell::RefCell;
        struct Io {
            calls: RefCell<Vec<Vec<String>>>,
        }
        impl SystemIo for Io {
            fn exists(&self, _: &str) -> bool { false }
            fn is_file(&self, _: &str) -> bool { false }
            fn is_dir(&self, p: &str) -> bool { p == "/tmp/checkup-db-1000/" }
            fn is_executable(&self, _: &str) -> bool { false }
            fn realpath(&self, _: &str) -> Option<String> { None }
            fn read_text(&self, _: &str) -> Option<String> { None }
            fn list_dir(&self, _: &str) -> Option<Vec<String>> { None }
            fn which(&self, n: &str) -> bool { matches!(n, "pacman" | "checkupdates") }
            fn run(&self, argv: &[String], _: Option<u64>, _: bool) -> RunOut {
                self.calls.borrow_mut().push(argv.to_vec());
                let stdout = match argv.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
                    ["checkupdates"] => "gdk-pixbuf2 2.44.7-1.1 -> 2.44.8-1.1\n",
                    ["pacman", "-Q"] => "gdk-pixbuf2 2.44.7-1.1\nglycin 2.1.5-2.1\n",
                    ["pacman", "-Qg", ..] => "",
                    ["pacman", "-Sl", "--dbpath", _] => "extra glycin 2.2.2-1 [installed: 2.1.5-2.1]\n",
                    _ => "",
                };
                RunOut { outcome: crate::utils::RunOutcome::Ok, returncode: Some(0), stdout: stdout.into(), stderr: String::new() }
            }
            fn env(&self, _: &str) -> String { String::new() }
            fn which_path(&self, _: &str) -> Option<String> { None }
            fn write_text(&self, _: &str, _: &str) -> Result<(), String> { Err("ro".into()) }
            fn copy_file(&self, _: &str, _: &str) -> Result<(), String> { Err("ro".into()) }
        }
        let io = Io { calls: RefCell::new(Vec::new()) };
        let advice = advise(&io, PkgFamily::Arch, 1000);
        assert_eq!(advice.findings.len(), 1);
        assert_eq!(advice.findings[0].set.id, "pixbuf");
        for call in io.calls.borrow().iter() {
            // `pacman-conf CacheDir` — только чтение: где кэш для отката.
            assert!(matches!(call[0].as_str(), "pacman" | "checkupdates" | "pacman-conf"), "{call:?}");
            assert!(!call.iter().any(|a| a == "-S" || a == "-Sy" || a == "-Syu" || a == "-U"), "{call:?}");
        }
        // Не Arch — ни одной команды.
        let io = Io { calls: RefCell::new(Vec::new()) };
        assert_eq!(advise(&io, PkgFamily::Debian, 1000), Advice::default());
        assert!(io.calls.borrow().is_empty());
    }
}
