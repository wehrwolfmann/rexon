//! Сверка оформления AeroShell с установленной Plasma — и команда пересборки.
//!
//! Пакеты `aeroshell-*` / `aerothemeplasma-*` из AUR не темы, а форки: они
//! собирают свой C++ против ЗАГОЛОВКОВ той Plasma, что стояла в момент
//! сборки. После `pacman -Syu` официальный стек уезжает вперёд, а форки
//! остаются собранными под старую версию — и отваливаются молча: у KWin
//! пропадают эффекты (панель задач, меню «Пуск», свечение кнопок, размытие),
//! у форка libplasma расходятся символы и перестаёт запускаться экран
//! блокировки. Лечится только пересборкой пакетов.
//!
//! Здесь — ЧИСТОЕ решение «расходится или нет», «чем пересобирать», состав
//! плана пересборки, порядок и тексты её шагов и разбор кода возврата
//! сборщика. Запуск pacman и самой пересборки — за вызывающим: в окне на
//! Rust это исполнитель ядра (`system_de_exec::DeOpsExec::aeroshell_survey` /
//! `rebuild_aeroshell`), в классическом окне — `rexon/system.py`; здесь
//! только чтение уже установленных файлов и разбор вывода. Версию пакета и список его файлов спрашивает общий движок пакетов
//! ([`crate::system_plan::installed_version_query`] и
//! [`crate::system_plan::package_files_query`]) — своих копий здесь нет.
//!
//! # Как определяется версия, под которую собран пакет
//!
//! 1. **Модуль эффекта KWin** — внутри `.so` лежит строка
//!    `org.kde.kwin.EffectPluginFactory<версия>`. Её вшивает при сборке сам
//!    KWin (макрос `KWIN_EFFECT_FACTORY`), и её же KWin ищет при загрузке
//!    модуля: не совпала — эффект молча выброшен. То есть мы сверяем ровно
//!    ту строку, по которой принимает решение загрузчик, — ни ложной
//!    тревоги, ни пропущенной поломки.
//! 2. **Форк libplasma** — библиотека ставится под именем
//!    `libPlasma.so.<версия>`, где версия та, под которую собран форк.
//!    Имя видно прямо в списке файлов пакета, читать сам `.so` не нужно.
//!
//! Отвергнутые способы (проверено на живой машине 10.08.2026):
//! * **Даты сборки пакетов.** kwin переустанавливался позже сборки форка,
//!   но версия при этом не менялась (поднялся только выпуск пакета,
//!   `6.7.4-3.1`) — по датам вышла бы ложная тревога.
//! * **Версия пакета из `pacman -Qi`.** У форков она из их СОБСТВЕННОГО
//!   репозитория: `aeroshell-kwin-components-git 6.7.0_61` — это версия
//!   проекта AeroShell, а не Plasma, и с версией системы несравнима.

use crate::i18n::{ntr, tr, trf};
use serde::Serialize;

/// Опознаватель фабрики эффектов KWin. Всё, что идёт следом до первого
/// не-цифры-не-точки, — версия KWin, под которую собран модуль.
const EFFECT_FACTORY_IID: &str = "org.kde.kwin.EffectPluginFactory";

/// Признак каталога модулей эффектов в списке файлов пакета. Соседний
/// `.../effects/configs/` (страницы настроек эффектов) сюда не попадает —
/// у него свой опознаватель и он не про совместимость с KWin.
const EFFECT_PLUGIN_DIR: &str = "/kwin/effects/plugins/";

/// Начало имени библиотеки форка libplasma: дальше идёт версия.
const PLASMA_LIB_PREFIX: &str = "libPlasma.so.";

/// Хелперы AUR, которые умеют пересобирать пакет по имени, в порядке
/// предпочтения. paru первым: только он собирает в чистом окружении
/// (`--chroot`), а именно чистое окружение и защищает от повторения беды —
/// в системной сборке пакет может подхватить уже наполовину обновлённый стек.
pub const REBUILD_HELPERS: &[&str] = &["paru", "yay"];

/// Ключ настройки «проверять оформление при запуске». Отсутствие ключа
/// означает ВКЛЮЧЕНО: выключенная проверка — это молча сломанный рабочий
/// стол после очередного обновления, такое не может быть значением по
/// умолчанию. Выключить можно только осознанно, в настройках.
pub const CHECK_ON_START_SETTING: &str = "aeroshell_check_on_start";

/// Включена ли проверка при запуске. `value` — то, что лежит в настройках
/// (None — ключа нет).
pub fn check_on_start(value: Option<bool>) -> bool {
    value.unwrap_or(true)
}

/// Версия из строки pacman: без эпохи (`1:`) и без выпуска пакета (`-3.1`).
/// `6.7.4-3.1` → `6.7.4`. Сравнивать выпуск нельзя: он растёт при
/// пересборке официального пакета, а совместимость от этого не меняется.
pub fn upstream_version(pacman_version: &str) -> String {
    let s = pacman_version.trim();
    let s = match s.split_once(':') {
        Some((epoch, rest)) if !epoch.is_empty() && epoch.bytes().all(|b| b.is_ascii_digit()) => {
            rest
        }
        _ => s,
    };
    match s.split_once('-') {
        Some((ver, _pkgrel)) => ver.to_string(),
        None => s.to_string(),
    }
}

/// Версия KWin из опознавателя внутри собранного модуля эффекта.
///
/// Ищем в сырых байтах `.so`: разбирать ELF ради одной строки незачем —
/// опознаватель лежит в данных как обычная строка, а поиск подстроки не
/// зависит ни от разрядности, ни от формата секций.
pub fn effect_iid_version(data: &[u8]) -> Option<String> {
    let needle = EFFECT_FACTORY_IID.as_bytes();
    let mut from = 0usize;
    while let Some(pos) = find_bytes(&data[from..], needle) {
        let start = from + pos + needle.len();
        let mut end = start;
        while end < data.len() && (data[end].is_ascii_digit() || data[end] == b'.') {
            end += 1;
        }
        let version = String::from_utf8_lossy(&data[start..end]).to_string();
        // Пустой хвост — это заголовок самого KWin, а не собранный модуль.
        if version.contains('.') {
            return Some(version);
        }
        from = start.max(from + pos + 1);
    }
    None
}

/// Поиск подстроки в байтах (без внешних зависимостей).
fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Пути файлов пакета из вывода `pacman -Ql <пакет>`.
///
/// Своего разбора здесь нет: список файлов установленного пакета — вопрос к
/// общему движку пакетов ([`crate::system_plan::package_file_paths`]), он же
/// знает, что у `dpkg -L` и `rpm -ql` форма ответа другая. Семейство здесь
/// всегда Arch: пакеты оформления живут только в AUR.
pub fn package_files(ql_output: &str) -> Vec<String> {
    crate::system_plan::package_file_paths(crate::system::PkgFamily::Arch, ql_output)
}

/// Модули эффектов KWin среди файлов пакета.
pub fn effect_plugin_paths(files: &[String]) -> Vec<String> {
    files
        .iter()
        .filter(|p| p.contains(EFFECT_PLUGIN_DIR) && p.ends_with(".so"))
        .cloned()
        .collect()
}

/// Версия Plasma, под которую собран форк libplasma, — из имени
/// `libPlasma.so.<версия>` в списке файлов пакета.
pub fn plasma_library_version(files: &[String]) -> Option<String> {
    for path in files {
        let name = path.rsplit('/').next().unwrap_or(path);
        let Some(rest) = name.strip_prefix(PLASMA_LIB_PREFIX) else {
            continue;
        };
        // Только полная версия `6.7.4`: `libPlasma.so.7` — это ссылка по
        // основному номеру, она не меняется от выпуска к выпуску.
        if rest.contains('.') && rest.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
            return Some(rest.to_string());
        }
    }
    None
}

// ── Модули Qt пакета: плазмоиды, QML-модули, плагины ───────────────────────
//
// У плазмоидов (SevenStart, seventasks, systemtray, notifications, volume) и
// QML-модулей набора НЕТ опознавателя версии, как у эффектов KWin, поэтому
// прежде они получали «метки нет» и молчали, даже когда уже не грузились.
// Их ломают две вещи, и обе видно без запуска Plasma:
//   1. **Не находится библиотека или символ** — `ldd -r` по модулю (смена
//      soname, например `libprocesscore.so.10 → .11`; расхождение с
//      libplasma — «undefined symbol»; модуль собран под Qt новее
//      установленного — «version `Qt_6.N' not found»).
//   2. **Приватный интерфейс Qt чужой минорной версии.** Модуль, которому
//      нужен `Qt_6_PRIVATE_API` (сейчас — notifications: AOT-код QML), держится
//      только на той минорной версии Qt, под которую собран: приватный
//      интерфейс между 6.11 и 6.12 не сохраняется, а символ версии тот же,
//      и `ldd` ничего не видит. Версию сборки выдаёт сам модуль: каждый
//      собранный с Qt файл ссылается на `qt_version_tag@Qt_6.N`, где N —
//      минорная версия Qt при сборке; её и сверяем с установленной qt6-base.

/// Признак модуля Qt в списке файлов пакета (плагины и QML-модули).
const QT_MODULE_DIR: &str = "/qt6/";

/// Символ версии приватного интерфейса Qt.
const QT_PRIVATE_API: &[u8] = b"Qt_6_PRIVATE_API";

/// Слова, которыми в бедах модуля названо «приватный интерфейс чужого Qt» —
/// по ним [`Report::at_risk_only`] (и сторож входа, [`crate::session_guard`])
/// отличает риск от свершившейся поломки.
pub const QT_PRIVATE_PROBLEM: &str = "private API";

/// Сколько бед одного модуля показывать в подробностях: дальше — повторы.
const MODULE_PROBLEMS_SHOWN: usize = 3;

/// Модули Qt среди файлов пакета: `.so` под `…/qt6/…`.
pub fn qt_module_paths(files: &[String]) -> Vec<String> {
    files
        .iter()
        .filter(|p| p.contains(QT_MODULE_DIR) && p.ends_with(".so"))
        .cloned()
        .collect()
}

/// Минорная версия Qt, под которую собран модуль: старшая из `Qt_6.N`.
pub fn qt_minor_built(data: &[u8]) -> Option<u32> {
    let needle = b"Qt_6.";
    let mut best: Option<u32> = None;
    let mut from = 0usize;
    while let Some(pos) = find_bytes(&data[from..], needle) {
        let start = from + pos + needle.len();
        let mut end = start;
        while end < data.len() && data[end].is_ascii_digit() {
            end += 1;
        }
        if let Ok(n) = std::str::from_utf8(&data[start..end]).unwrap_or("").parse::<u32>() {
            best = Some(best.map_or(n, |b| b.max(n)));
        }
        from = start;
    }
    best
}

/// Пользуется ли модуль приватным интерфейсом Qt.
pub fn uses_qt_private_api(data: &[u8]) -> bool {
    find_bytes(data, QT_PRIVATE_API).is_some()
}

/// Минорная версия из версии пакета Qt: `6.11.2-3` → 11.
pub fn qt_minor(version: &str) -> Option<u32> {
    let v = upstream_version(version);
    let mut parts = v.split('.');
    (parts.next()? == "6").then_some(())?;
    parts.next()?.parse().ok()
}

/// Беды модуля из вывода `ldd -r <модуль>` (stdout и stderr вместе).
///
/// `siblings` — имена файлов всего набора оформления. Модуль, который
/// требует соседа по набору по голому имени (так `smodglow.so` требует
/// `org.smod.smod.so`), находит его, только когда сосед уже загружен
/// хозяином, — `ldd` такого не знает и говорит «not found», а следом
/// «undefined symbol» на символы соседа. Это не поломка: такие строки
/// пропускаются, а «undefined symbol» в таком модуле не считается вовсе.
pub fn ldd_problems(output: &str, siblings: &[String]) -> Vec<String> {
    let sibling = |name: &str| siblings.iter().any(|s| s.rsplit('/').next() == Some(name));
    let mut problems = Vec::new();
    let mut sibling_missing = false;
    for line in output.lines() {
        let line = line.trim();
        if let Some((name, _)) = line.split_once(" => not found") {
            let name = name.trim();
            if sibling(name) {
                sibling_missing = true;
            } else {
                problems.push(format!("{name}: not found"));
            }
        } else if line.contains(": version `") && line.contains("' not found") {
            // «<модуль>: <библиотека>: version `Qt_6.12' not found (required
            // by …)» — хвост «required by» повторяет модуль, он не нужен.
            problems.push(line.split(" (required by").next().unwrap_or(line).to_string());
        } else if let Some((_, rest)) = line.split_once("error while loading shared libraries: ") {
            // Файл библиотеки есть, но загрузчик его не берёт (обрезан, не
            // того класса ELF): «<программа>: error while loading shared
            // libraries: /usr/lib/libQt6Quick.so.6: file too short» — замер
            // в ВМ SteamOS 25.10.2026; ldd тогда ничего не пишет про «not found».
            problems.push(rest.trim().to_string());
        }
    }
    if !sibling_missing {
        for line in output.lines() {
            if let Some(rest) = line.trim().strip_prefix("undefined symbol: ") {
                // «undefined symbol: _ZTI…, version Qt_6\t(…)» — без запятой.
                let symbol = rest.split(|c: char| c.is_whitespace() || c == ',').next().unwrap_or(rest);
                problems.push(format!("undefined symbol: {symbol}"));
            }
        }
    }
    problems
}

/// Пакет содержит собранный код (а значит, требует пересборки), а не одни
/// картинки со звуками.
pub fn is_compiled_package(files: &[String]) -> bool {
    files.iter().any(|p| p.ends_with(".so") || p.contains(".so."))
}

// ══════════════════════════════════════════════════════════════════════════
// «Одинокий форк»: замена libplasma осталась, а самой оболочки нет
// ══════════════════════════════════════════════════════════════════════════
//
// Форк libplasma ПОДМЕНЯЕТ системную библиотеку (`provides=libplasma` без
// версии), а пакеты Plasma зависят от `libplasma` тоже без версии. Поэтому
// при выходе следующей Plasma `pacman -Syu` обновит plasma-workspace и kwin,
// а форк оставит прежним — и падает уже ВЕСЬ стол (экран блокировки,
// plasmashell), а не одна тема. Пока стоит сама оболочка AeroThemePlasma,
// форк ей нужен, и лечение — пересборка. Без оболочки форк не нужен никому:
// пересобирать его — чинить то, что можно просто убрать. Правильный совет —
// снять остатки и вернуть штатный libplasma (откат форков уже есть:
// `system_exec::SystemExec::remove_plasma_forks`).

/// Начало имени форка libplasma (`aeroshell-libplasma-git` и варианты).
pub const LIBPLASMA_FORK_PREFIX: &str = "aeroshell-libplasma";

/// Начало имени пакета самой оболочки AeroThemePlasma (Wayland и X11:
/// `aerothemeplasma-desktop-git`, `aerothemeplasma-desktop-x11-git`) — тот
/// же пакет, по которому узнаётся комплект в [`crate::aur_install::AEROTHEMEPLASMA`].
pub const SHELL_PACKAGE_PREFIX: &str = "aerothemeplasma-desktop";

/// Стоит форк libplasma, а самой оболочки нет.
pub fn lonely_libplasma_fork(forks: &[String]) -> bool {
    forks.iter().any(|f| f.starts_with(LIBPLASMA_FORK_PREFIX))
        && !forks.iter().any(|f| f.starts_with(SHELL_PACKAGE_PREFIX))
}

/// Откуда взята версия сборки пакета.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Marker {
    /// Опознаватель внутри модуля эффекта KWin.
    KwinEffect,
    /// Имя библиотеки форка libplasma.
    PlasmaLibrary,
    /// Минорная версия Qt модуля с приватным интерфейсом Qt.
    QtVersion,
    /// Метки нет — о совместимости пакета сказать нечего.
    None,
}

/// Что известно про один установленный пакет оформления.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PackageVerdict {
    /// Имя пакета.
    pub package: String,
    /// Откуда взята версия сборки.
    pub marker: Marker,
    /// Версия, под которую пакет собран (None — метки не нашлось).
    pub built_for: Option<String>,
    /// Версия, установленная сейчас (None — узнать не удалось).
    pub installed: Option<String>,
    /// Пакет содержит собранный код.
    pub compiled: bool,
    /// Версии разошлись либо у модулей пакета есть беды — пакет сейчас не
    /// работает.
    pub stale: bool,
    /// Беды модулей Qt пакета: «модуль: что не так» (пусто — всё грузится).
    pub problems: Vec<String>,
}

/// Итог сверки для показа человеку.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Report {
    /// Разбор по каждому найденному пакету оформления.
    pub packages: Vec<PackageVerdict>,
    /// Имена пакетов, у которых версии разошлись.
    pub stale: Vec<String>,
    /// Что пересобирать (все собранные пакеты набора, а не только
    /// отвалившиеся: они собираются друг против друга и врозь разъедутся).
    pub rebuild: Vec<String>,
    /// Есть ли расхождение.
    pub needs_rebuild: bool,
    /// Версия KWin, установленная сейчас.
    pub kwin_version: Option<String>,
    /// Версия официального стека Plasma, установленная сейчас.
    pub plasma_version: Option<String>,
    /// Форк libplasma остался без оболочки ([`lonely_libplasma_fork`]).
    pub lonely_fork: bool,
    /// Что снять при возврате штатной Plasma (пусто, если форк не одинок):
    /// сам форк libplasma и все прочие СОБРАННЫЕ пакеты набора — без
    /// оболочки они так же бесполезны и так же ломаются. Значки и звуки
    /// остаются: они ничего не подменяют и ничего не ломают.
    pub restore: Vec<String>,
    /// Что сказал журнал сеанса ([`crate::plugin_journal`]): все модули,
    /// которые KWin и plasmashell не загрузили, — и набора, и сторонние.
    pub journal: Vec<crate::plugin_journal::Finding>,
}

impl Report {
    /// Расхождение есть, но пока только РИСК: единственная улика — модули,
    /// собранные под приватный интерфейс прежней минорной версии Qt. Они
    /// грузятся (ненайденного нет, журнал сеанса молчит, версии Plasma и
    /// KWin совпадают), а сломаться могут в любой момент. Такое состояние
    /// называется другими словами («нужно пересобрать», а не «перестало
    /// работать») и при запуске показывается один раз, а не каждый: окно,
    /// которое твердит о поломке при работающем столе, перестают читать
    /// (приёмка 10.10.2026).
    pub fn at_risk_only(&self) -> bool {
        self.needs_rebuild
            && !self.lonely_fork
            && self.journal.is_empty()
            && self.packages.iter().filter(|p| p.stale).all(|p| {
                let versions_differ = match (&p.built_for, &p.installed) {
                    (Some(b), Some(i)) => b != i,
                    _ => false,
                };
                !p.problems.is_empty()
                    && p.problems.iter().all(|x| x.contains(QT_PRIVATE_PROBLEM))
                    && (p.marker == Marker::QtVersion || !versions_differ)
            })
    }

    /// Пустой итог — «оформление AeroShell не установлено».
    pub fn empty() -> Self {
        Self {
            packages: Vec::new(),
            stale: Vec::new(),
            rebuild: Vec::new(),
            needs_rebuild: false,
            kwin_version: None,
            plasma_version: None,
            lonely_fork: false,
            restore: Vec::new(),
            journal: Vec::new(),
        }
    }

    /// Находки журнала по пакетам набора.
    pub fn journal_in_set(&self) -> Vec<&crate::plugin_journal::Finding> {
        self.journal
            .iter()
            .filter(|f| f.package().is_some_and(|p| self.packages.iter().any(|v| v.package == p)))
            .collect()
    }

    /// Находки журнала вне набора (сторонние эффекты и виджеты).
    pub fn journal_outside(&self) -> Vec<&crate::plugin_journal::Finding> {
        self.journal
            .iter()
            .filter(|f| !f.package().is_some_and(|p| self.packages.iter().any(|v| v.package == p)))
            .collect()
    }
}

/// Добавить к сверке по версиям то, что сказал журнал сеанса.
///
/// Журнал — точный источник: модуль пакета набора назван в журнале
/// несовместимым — пакет сломан, даже если по версиям в файлах сходится
/// (и версии берутся из журнала: их назвал сам KWin). Обратное НЕ верно:
/// журнал бывает пуст (отладка KWin до 6.8 выключена, журнал повёрнут,
/// сеанс без systemd), поэтому прежняя сверка по версиям остаётся в силе.
pub fn apply_journal(report: &mut Report, findings: Vec<crate::plugin_journal::Finding>) {
    use crate::plugin_journal::Cause;
    for f in &findings {
        let Some(pkg) = f.package() else { continue };
        let Some(v) = report.packages.iter_mut().find(|v| v.package == pkg) else { continue };
        if f.failure.cause == Cause::NotFound {
            continue;
        }
        v.stale = true;
        v.problems.push(format!("{}: {}", f.failure.id, f.failure.cause.technical()));
        if let Cause::KwinVersion { built_for: Some(b), expected: Some(e) } = &f.failure.cause {
            if matches!(v.marker, Marker::KwinEffect | Marker::None) {
                v.marker = Marker::KwinEffect;
                v.built_for = Some(b.clone());
                v.installed = Some(e.clone());
            }
        }
    }
    report.stale = report.packages.iter().filter(|p| p.stale).map(|p| p.package.clone()).collect();
    report.needs_rebuild = !report.stale.is_empty();
    report.journal = findings;
}

/// Сверить установленные пакеты оформления с текущей Plasma.
///
/// `forks` — имена установленных пакетов-форков (их отбирает
/// [`crate::system_plan::filter_plasma_forks`]).
/// `kwin_version` / `plasma_version` — версии из pacman, как есть (эпоха и
/// выпуск отрезаются здесь).
/// `file_list(pkg)` — вывод `pacman -Ql <pkg>` либо None.
/// `read_file(path)` — содержимое файла либо None.
pub fn check_with(
    forks: &[String],
    kwin_version: Option<&str>,
    plasma_version: Option<&str>,
    file_list: &dyn Fn(&str) -> Option<String>,
    read_file: &dyn Fn(&str) -> Option<Vec<u8>>,
) -> Report {
    check_modules_with(forks, kwin_version, plasma_version, None, file_list, read_file, &|_| None)
}

/// [`check_with`] вместе со сверкой модулей Qt (плазмоиды, QML-модули).
///
/// `qt_version` — версия qt6-base из pacman как есть; `ldd(path)` — вывод
/// `ldd -r <path>` (stdout и stderr вместе) либо None (тогда этот шаг
/// пропускается: «не смогли узнать» — не «сломано»).
pub fn check_modules_with(
    forks: &[String],
    kwin_version: Option<&str>,
    plasma_version: Option<&str>,
    qt_version: Option<&str>,
    file_list: &dyn Fn(&str) -> Option<String>,
    read_file: &dyn Fn(&str) -> Option<Vec<u8>>,
    ldd: &dyn Fn(&str) -> Option<String>,
) -> Report {
    let kwin = kwin_version.map(upstream_version);
    let plasma = plasma_version.map(upstream_version);
    let qt_installed = qt_version.and_then(qt_minor);
    // Файлы всех пакетов набора: по ним узнаются соседи ([`ldd_problems`]).
    let all_files: Vec<(String, Vec<String>)> = forks
        .iter()
        .map(|pkg| (pkg.clone(), file_list(pkg).map(|out| package_files(&out)).unwrap_or_default()))
        .collect();
    let siblings: Vec<String> = all_files
        .iter()
        .flat_map(|(_, f)| f.iter().filter(|p| p.ends_with(".so")).cloned())
        .collect();

    let mut report = Report {
        kwin_version: kwin.clone(),
        plasma_version: plasma.clone(),
        ..Report::empty()
    };

    for (pkg, files) in &all_files {
        let compiled = is_compiled_package(files);

        // Модуль эффекта KWin — самая точная метка, поэтому первая.
        let mut marker = Marker::None;
        let mut built_for = None;
        for path in effect_plugin_paths(files) {
            if let Some(v) = read_file(&path).as_deref().and_then(effect_iid_version) {
                marker = Marker::KwinEffect;
                built_for = Some(v);
                break;
            }
        }
        if built_for.is_none() {
            if let Some(v) = plasma_library_version(files) {
                marker = Marker::PlasmaLibrary;
                built_for = Some(v);
            }
        }

        // Модули Qt: ненайденное и приватный интерфейс чужого Qt.
        let mut problems = Vec::new();
        let mut qt_built: Option<u32> = None;
        for path in qt_module_paths(files) {
            let name = path.rsplit('/').next().unwrap_or(&path).to_string();
            if let Some(out) = ldd(&path) {
                for p in ldd_problems(&out, &siblings).into_iter().take(MODULE_PROBLEMS_SHOWN) {
                    problems.push(format!("{name}: {p}"));
                }
            }
            let Some(data) = read_file(&path) else { continue };
            if !uses_qt_private_api(&data) {
                continue;
            }
            if let (Some(built), Some(inst)) = (qt_minor_built(&data), qt_installed) {
                if built != inst {
                    problems.push(format!("{name}: Qt 6.{built} {QT_PRIVATE_PROBLEM}, installed Qt 6.{inst}"));
                    qt_built = Some(built);
                }
            }
        }
        if marker == Marker::None {
            if let (Some(b), Some(_)) = (qt_built, qt_installed) {
                marker = Marker::QtVersion;
                built_for = Some(format!("6.{b}"));
            }
        }

        let installed = match marker {
            Marker::KwinEffect => kwin.clone(),
            Marker::PlasmaLibrary => plasma.clone(),
            Marker::QtVersion => qt_installed.map(|i| format!("6.{i}")),
            Marker::None => None,
        };
        // Расхождение объявляем, только когда известны ОБЕ версии: «не
        // смогли узнать» — это не «сломано», пугать таким нельзя.
        let stale = match (&built_for, &installed) {
            (Some(b), Some(i)) => b != i,
            _ => false,
        } || !problems.is_empty();

        if stale {
            report.stale.push(pkg.clone());
        }
        if compiled {
            report.rebuild.push(pkg.clone());
        }
        report.packages.push(PackageVerdict {
            package: pkg.clone(),
            marker,
            built_for,
            installed,
            compiled,
            stale,
            problems,
        });
    }

    report.needs_rebuild = !report.stale.is_empty();
    report.lonely_fork = lonely_libplasma_fork(forks);
    if report.lonely_fork {
        report.restore = report
            .packages
            .iter()
            .filter(|p| p.compiled || p.package.starts_with(LIBPLASMA_FORK_PREFIX))
            .map(|p| p.package.clone())
            .collect();
    }
    report
}

/// Что посоветовать человеку по итогам сверки.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Advice {
    /// Всё в порядке либо сказать нечего — окна нет.
    Nothing,
    /// Пересобрать пакеты (оболочка стоит, сборка отстала от системы).
    Rebuild { packages: Vec<String> },
    /// Форк остался без оболочки: снять его и вернуть штатную Plasma.
    RestoreStock { forks: Vec<String> },
    /// Снять нужно, но вернуть штатный пакет сейчас нечем (его нет в
    /// репозиториях) — `message` объясняет почему, ничего не запускается.
    RestoreBlocked { message: String },
    /// Пересборка пока бесполезна: ветка, с которой собирается форк
    /// libplasma, ещё не догнала Plasma из репозиториев. `plasma` — версия
    /// Plasma, `fork` — версия форка в его ветке по умолчанию.
    WaitForUpstream { plasma: String, fork: String },
    /// Сейчас всё работает, но в репозиториях уже новая серия Plasma, до
    /// которой авторы набора не догнали: не обновляться. `plasma` — версия
    /// в репозиториях, `fork` — версия форка в его ветке по умолчанию.
    HoldUpdate { plasma: String, fork: String },
    /// Сейчас всё работает, в репозиториях точечный выпуск Plasma: после
    /// него понадобится пересборка (Rexon её предложит).
    RebuildAfterUpdate { from: String, to: String },
    /// Журнал сеанса назвал модули KWin или Plasma НЕ из набора (сторонние
    /// эффекты, виджеты, оформление окон), которые не загрузились.
    /// `rebuild` — AUR-пакеты, пересборка которых это чинит (пусто — Rexon
    /// только говорит, что делать).
    BrokenPlugins { rebuild: Vec<String> },
}

/// Совет по итогу сверки.
///
/// `restore_preview` — ответ предпросмотра отката
/// (`SystemExec::plasma_fork_reset_preview`) для [`Report::restore`]: `(ok,
/// команда либо объяснение отказа)`. Нужен только одинокому форку; `None` —
/// предпросмотр не делался (тогда откат всё равно предлагается: исполнитель
/// откажет вслух сам, если вернуть нечем).
///
/// ⭐ Одинокий форк важнее пересборки: даже отставший от системы форк без
/// оболочки лечится снятием, а пересборка оставила бы ту же мину до
/// следующего обновления.
pub fn advise(report: &Report, restore_preview: Option<(bool, String)>) -> Advice {
    advise_with_upstream(report, restore_preview, None)
}

/// [`advise`] со сверкой «догнали ли авторы форка» ([`upstream_lag`]).
/// `upstream` — `(версия Plasma, к которой идёт система; версия форка в его
/// ветке по умолчанию)`; `None` — узнать не удалось (нет сети, чужой
/// хостинг) — тогда совет прежний: молчаливый пропуск лучше ложной тревоги.
pub fn advise_with_upstream(
    report: &Report,
    restore_preview: Option<(bool, String)>,
    upstream: Option<(String, String)>,
) -> Advice {
    if report.lonely_fork && !report.restore.is_empty() {
        return match restore_preview {
            Some((false, message)) => Advice::RestoreBlocked { message },
            _ => Advice::RestoreStock {
                forks: report.restore.clone(),
            },
        };
    }
    if report.needs_rebuild && !report.rebuild.is_empty() {
        if let Some((plasma, fork)) = upstream {
            if upstream_lag(&fork, &plasma) {
                return Advice::WaitForUpstream { plasma, fork };
            }
        }
        return Advice::Rebuild {
            packages: report.rebuild.clone(),
        };
    }
    broken_plugins(report)
}

/// Совет по сторонним модулям из журнала: [`Advice::BrokenPlugins`] либо
/// [`Advice::Nothing`].
fn broken_plugins(report: &Report) -> Advice {
    let outside: Vec<crate::plugin_journal::Finding> = report.journal_outside().into_iter().cloned().collect();
    if outside.is_empty() {
        return Advice::Nothing;
    }
    Advice::BrokenPlugins {
        rebuild: crate::plugin_journal::rebuild_packages(&outside),
    }
}

/// Что именно сломано — строки для окна и уведомления: сначала модули
/// набора (из журнала), потом сторонние. Пусто — журнал ничего не назвал.
pub fn journal_lines(report: &Report) -> Vec<String> {
    let mut all: Vec<crate::plugin_journal::Finding> = report.journal_in_set().into_iter().cloned().collect();
    all.extend(report.journal_outside().into_iter().cloned());
    crate::plugin_journal::finding_lines(&all)
}

/// Текст окна о сторонних модулях (простой HTML; строки экранируются).
pub fn plugins_text(report: &Report, can_rebuild: bool) -> String {
    let outside: Vec<crate::plugin_journal::Finding> = report.journal_outside().into_iter().cloned().collect();
    let lines: Vec<String> = crate::plugin_journal::finding_lines(&outside)
        .iter()
        .map(|l| format!("• {}", crate::ui_base::escape_dialog_text(l)))
        .collect();
    let mut text = format!("{}<br><br>{}", crate::plugin_journal::plugins_intro(), lines.join("<br>"));
    if can_rebuild {
        text.push_str(&format!("<br><br>{}", crate::plugin_journal::plugins_rebuild_note()));
    }
    text
}

// ══════════════════════════════════════════════════════════════════════════
// Догнали ли авторы форка новую Plasma
// ══════════════════════════════════════════════════════════════════════════
//
// Сборщик AUR собирает `-git`-пакет с ветки ПО УМОЛЧАНИЮ репозитория (в
// PKGBUILD нет `#branch=`). Форк libplasma вливает каждый выпуск Plasma, а
// на большой переход заводит новую ветку (`Plasma/6.8`) и переключает на неё
// ветку по умолчанию. Пока этого не случилось, пересборка соберёт СТАРУЮ
// libplasma против новой Plasma — «пересобрал, а всё равно сломано», плюс
// полчаса работы машины. Поэтому до вопроса о пересборке смотрим:
//   1. ветку по умолчанию — `git ls-remote --symref <репозиторий> HEAD`;
//   2. её `PROJECT_VERSION` — из `CMakeLists.txt` той ветки (сырой файл);
//   3. версию Plasma, к которой идёт система, — старшую из установленной
//      plasma-workspace и libplasma в базе репозиториев (`pacman -Si`, без
//      прав администратора).
// «Догнал» = версия форка не младше версии Plasma. Сеть — с таймаутом; нет
// сети, git или curl, чужой хостинг — проверка молча пропускается.

/// Таймаут каждого сетевого шага сверки, секунды.
pub const UPSTREAM_TIMEOUT_SECS: u64 = 12;

/// Адрес репозитория для `git ls-remote` по адресу проекта из пакета
/// (`URL` в `pacman -Qi`). Только https: иной адрес — не наш случай.
pub fn upstream_git_url(project_url: &str) -> Option<String> {
    let url = project_url.trim().trim_end_matches('/');
    if !url.starts_with("https://") || url.contains(char::is_whitespace) {
        return None;
    }
    Some(if url.ends_with(".git") { url.to_string() } else { format!("{url}.git") })
}

/// Ветка по умолчанию из ответа `git ls-remote --symref <url> HEAD`
/// (`ref: refs/heads/Plasma/6.7\tHEAD`).
pub fn default_branch(ls_remote: &str) -> Option<String> {
    ls_remote.lines().find_map(|line| {
        let rest = line.strip_prefix("ref: refs/heads/")?;
        let (branch, what) = rest.split_once('\t')?;
        (what.trim() == "HEAD" && !branch.is_empty()).then(|| branch.to_string())
    })
}

/// Адрес сырого файла ветки. Знаем два вида хостинга: GitHub и GitLab (его
/// же движок у gitgud.io, где живёт AeroShell). Прочие — `None`.
pub fn raw_file_url(project_url: &str, branch: &str, file: &str) -> Option<String> {
    let url = project_url.trim().trim_end_matches('/').trim_end_matches(".git");
    let rest = url.strip_prefix("https://")?;
    let (host, path) = rest.split_once('/')?;
    if path.is_empty() || branch.is_empty() {
        return None;
    }
    if host == "github.com" {
        return Some(format!("https://raw.githubusercontent.com/{path}/{branch}/{file}"));
    }
    if host.contains("gitlab") || host == "gitgud.io" || host == "invent.kde.org" {
        return Some(format!("{url}/-/raw/{branch}/{file}"));
    }
    None
}

/// `PROJECT_VERSION` из `CMakeLists.txt`: `set(PROJECT_VERSION "6.7.5")`.
pub fn cmake_project_version(text: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let rest = line.trim().strip_prefix("set(PROJECT_VERSION")?;
        let version: String = rest
            .trim_start()
            .trim_start_matches('"')
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.')
            .collect();
        version.contains('.').then_some(version)
    })
}

/// Старшая из версий (сравнение по числам; пустые пропускаются).
pub fn newest_version(versions: &[Option<String>]) -> Option<String> {
    versions
        .iter()
        .flatten()
        .map(|v| upstream_version(v))
        .filter(|v| !crate::engine_update::ver_tuple(v).is_empty())
        .max_by(|a, b| crate::engine_update::ver_tuple(a).cmp(&crate::engine_update::ver_tuple(b)))
}

/// Отстаёт ли форк от Plasma: версия форка младше. Бета форка (`6.7.91`)
/// против выпуска (`6.8.0`) — отстаёт: так и есть, ветка ещё не выпущена.
pub fn upstream_lag(fork_version: &str, plasma_version: &str) -> bool {
    let (f, p) = (
        crate::engine_update::ver_tuple(fork_version),
        crate::engine_update::ver_tuple(plasma_version),
    );
    !f.is_empty() && !p.is_empty() && f < p
}

/// Заголовок окна «пересборка пока не поможет».
pub fn wait_title() -> String {
    tr("Repair Isn't Possible Yet")
}
/// Текст окна «пересборка пока не поможет».
pub fn wait_text(plasma: &str, fork: &str) -> String {
    // Большой переход называем коротко («Plasma 6.8»), точечный — полностью
    // («Plasma 6.7.6» при форке 6.7.5): иначе вышло бы «не догнали 6.7,
    // у них 6.7.5».
    let shown = if plasma_series(plasma) == plasma_series(fork) {
        plasma.to_string()
    } else {
        plasma_series(plasma)
    };
    trf(
        // TRANSLATORS: %1 is the new Plasma version (e.g. 6.8), %2 the version
        // the AeroThemePlasma authors currently have (e.g. 6.7.5).
        "Rebuilding the appearance won't help yet: the AeroThemePlasma authors \
         haven't caught up with Plasma %1 (their version is still %2). A rebuild \
         now would produce the same broken result.\n\nRexon will check again \
         on the next start and offer the repair as soon as their update is out.",
        &[&shown, &fork],
    )
}

// ══════════════════════════════════════════════════════════════════════════
// Предупреждение ДО обновления системы
// ══════════════════════════════════════════════════════════════════════════
//
// Сверка выше узнаёт о беде, когда она уже случилась: Plasma обновилась,
// «Пуск» и панель задач пропали. Новая Plasma видна в репозиториях раньше,
// чем человек нажмёт «Обновить», — и сказать «подождите» можно до поломки.
// Что придёт с обновлением, спрашиваем у `checkupdates` (pacman-contrib):
// он синхронизирует СВОЮ копию базы репозиториев во временном каталоге,
// без прав администратора и не трогая базу системы. Нет его — честно
// пропускаем: читать системную базу (`pacman -Si`) значит видеть её такой,
// какой она была при последнем `-Sy`, а синхронизировать её саму — root.
//
// Решение:
//   * переход на новую серию Plasma (6.7 → 6.8), а форк libplasma в ветке
//     по умолчанию ещё на старой ([`upstream_lag`] по серии) — «не
//     обновляйтесь»: после обновления пропадут «Пуск» и панель задач, а
//     пересобрать будет не из чего;
//   * точечный выпуск (6.7.4 → 6.7.5) — мягко: эффекты KWin набора после
//     него выключатся до пересборки, Rexon её предложит;
//   * авторы уже на новой серии либо узнать их версию не удалось (нет сети)
//     — так же мягко, как точечный выпуск: «не обновляйтесь» без данных было
//     бы ложной тревогой, а пересборка после обновления нужна в любом случае.
// ⛔ Только предупреждение: никаких IgnorePkg, хуков pacman и вмешательства
// в чужие помощники AUR.

/// Пакеты официального стека Plasma, по обновлению которых судим о новой
/// Plasma. libplasma у владельца набора заменён форком и в списке
/// обновлений не появится — тогда решают plasma-workspace и kwin.
pub const PLASMA_STACK_PACKAGES: &[&str] = &["plasma-workspace", "kwin", "libplasma"];

// Разбор `checkupdates` общий у всех проверок «до обновления» — в
// [`crate::update_guard`].
pub use crate::update_guard::{
    parse_pending_updates, PendingUpdate, PENDING_UPDATES_BINARY, PENDING_UPDATES_TIMEOUT_SECS,
};

/// Серия Plasma — два первых числа: `6.7.5` → `6.7`.
pub fn plasma_series(version: &str) -> String {
    version.split('.').take(2).collect::<Vec<_>>().join(".")
}

/// Ждущее обновление Plasma: `(версия сейчас, версия в репозиториях)` без
/// эпохи и выпуска пакета — по пакету стека с самой новой версией. None —
/// Plasma в списке обновлений нет (пересборка пакета без смены версии —
/// тоже нет: совместимость от неё не меняется).
pub fn pending_plasma(updates: &[PendingUpdate]) -> Option<(String, String)> {
    use crate::engine_update::ver_tuple;
    updates
        .iter()
        .filter(|u| PLASMA_STACK_PACKAGES.contains(&u.package.as_str()))
        .map(|u| (upstream_version(&u.installed), upstream_version(&u.available)))
        .filter(|(from, to)| !ver_tuple(to).is_empty() && ver_tuple(to) > ver_tuple(from))
        .max_by(|a, b| ver_tuple(&a.1).cmp(&ver_tuple(&b.1)))
}

/// Совет ДО обновления. Зовётся, когда сейчас всё в порядке
/// ([`Advice::Nothing`]): уже сломанное важнее того, что только придёт.
///
/// `updates` — разобранный вывод `checkupdates`; `fork` — версия форка
/// libplasma в его ветке по умолчанию (None — узнать не удалось).
pub fn advise_before_update(report: &Report, updates: &[PendingUpdate], fork: Option<&str>) -> Advice {
    // Нет собранных пакетов набора — ломаться нечему (значки и звуки от
    // версии Plasma не зависят).
    if report.rebuild.is_empty() {
        return Advice::Nothing;
    }
    let Some((from, to)) = pending_plasma(updates) else {
        return Advice::Nothing;
    };
    if plasma_series(&from) == plasma_series(&to) {
        return Advice::RebuildAfterUpdate { from, to };
    }
    match fork {
        Some(fork) if upstream_lag(&plasma_series(fork), &plasma_series(&to)) => Advice::HoldUpdate {
            plasma: to,
            fork: fork.to_string(),
        },
        // Авторы догнали либо их версию узнать не удалось: «не обновляйтесь»
        // говорить не с чего, но пересобирать после обновления придётся.
        _ => Advice::RebuildAfterUpdate { from, to },
    }
}

/// Заголовок предупреждения «не обновляйте систему».
pub fn hold_title() -> String {
    tr("Don't Update the System Yet")
}
/// Текст предупреждения «не обновляйте систему».
pub fn hold_text(plasma: &str, fork: &str) -> String {
    trf(
        // TRANSLATORS: %1 is the new Plasma version waiting in the repositories
        // (e.g. 6.8), %2 the newest Plasma that AeroThemePlasma supports (e.g. 6.7).
        "Plasma %1 is out, but AeroThemePlasma supports only Plasma %2 so far. \
         After the update, the Start menu and the taskbar will disappear.\n\n\
         Wait until the AeroThemePlasma authors release a version for Plasma %1 — \
         then it is safe to update.",
        &[&plasma_series(plasma), &plasma_series(fork)],
    )
}
/// Заголовок мягкого предупреждения о точечном выпуске.
pub fn rebuild_after_title() -> String {
    tr("A Plasma Update Is Waiting")
}
/// Текст мягкого предупреждения о точечном выпуске.
pub fn rebuild_after_text(to: &str) -> String {
    trf(
        // TRANSLATORS: %1 is the Plasma version waiting in the repositories (e.g. 6.7.5).
        "Plasma %1 is ready to install. After the update, the AeroThemePlasma \
         window effects will stay off until it is rebuilt for the new version — \
         Rexon will offer the rebuild right after the update.",
        &[&to],
    )
}

// ── Тексты окна «вернуть штатную Plasma» ─────────────────────────────────

/// Заголовок окна вопроса.
pub fn restore_title() -> String {
    tr("Leftover of AeroThemePlasma Found")
}
/// Главный текст вопроса (простой HTML).
pub fn restore_text() -> String {
    tr("<b>AeroThemePlasma has been removed, but its replacement for a part of \
        Plasma is still installed.</b>")
}
/// Объяснение: чем грозит и что сделает Rexon (простой HTML).
pub fn restore_informative() -> String {
    tr("Right now everything works, but the next Plasma update can break the \
        whole desktop: the panel and the lock screen may stop starting.\
        <br><br>Rexon will remove the leftover and put the standard Plasma part \
        back. You'll be asked for the password once; afterwards you'll need to \
        log out and log back in.")
}
/// Кнопка согласия.
pub fn restore_button() -> String {
    format!("🧹 {}", tr("Restore Standard Plasma"))
}
/// Заголовок окна хода.
pub fn restore_run_title() -> String {
    tr("Restoring Standard Plasma")
}
/// Первая отметка в окне хода.
pub fn restore_start_message() -> String {
    tr("🧹 Removing the leftover and restoring standard Plasma…")
}
/// Отметка в окне хода, когда всё получилось.
pub fn restore_done_message() -> String {
    tr("✅ Standard Plasma restored")
}
/// Итог под журналом при удаче (простой HTML).
pub fn restore_ok_text() -> String {
    tr("The leftover has been removed and the standard Plasma part is back.\
        <br><br><b>Log out and log back in</b> to finish.")
}
/// Заголовок отказа, когда вернуть штатный пакет нечем, — тот же, что у
/// окна про пакеты-замены в меню окна: случай один и тот же.
pub fn restore_blocked_title() -> String {
    crate::window_menu::forks_blocked_title()
}

/// То же, но файлы читаются с диска.
pub fn check(
    forks: &[String],
    kwin_version: Option<&str>,
    plasma_version: Option<&str>,
    file_list: &dyn Fn(&str) -> Option<String>,
) -> Report {
    check_with(forks, kwin_version, plasma_version, file_list, &|path| {
        std::fs::read(path).ok()
    })
}

/// Первый доступный хелпер AUR, умеющий пересборку.
pub fn pick_helper(exists: &dyn Fn(&str) -> bool) -> Option<String> {
    REBUILD_HELPERS
        .iter()
        .find(|h| exists(h))
        .map(|h| (*h).to_string())
}

/// Инструмент, без которого сборки в чистом окружении не бывает (пакет
/// `devtools`). paru зовёт его сам, и без него `--chroot` падает.
const CHROOT_TOOL: &str = "mkarchroot";

/// Можно ли собирать в чистом окружении: умеет только paru, и только когда
/// в системе есть чем его создать. Просить `--chroot` без devtools —
/// значит гарантированно уронить сборку на ровном месте.
pub fn chroot_available(helper: &str, exists: &dyn Fn(&str) -> bool) -> bool {
    helper == "paru" && exists(CHROOT_TOOL)
}

/// Команда пересборки пакетов выбранным хелпером.
///
/// * `--rebuild` — собрать заново, даже если версия в AUR не менялась
///   (у `-git`-пакетов она и не меняется, а пересобрать надо).
/// * `--chroot` — сборка в чистом окружении; просится только когда она
///   вообще возможна ([`chroot_available`]). У yay такой возможности нет
///   вовсе, а paru без devtools на этом флаге падает.
/// * `--sudo pkexec` — права запрашиваются графическим окном polkit. Rexon
///   запускается из меню приложений, где терминала нет: обычный sudo в такой
///   сборке просто не сможет спросить пароль и всё упадёт.
///
/// None — хелпер незнакомый либо пересобирать нечего.
pub fn rebuild_command(helper: &str, packages: &[String], chroot: bool) -> Option<Vec<String>> {
    if packages.is_empty() || !REBUILD_HELPERS.contains(&helper) {
        return None;
    }
    let mut argv = vec![
        helper.to_string(),
        "-S".to_string(),
        "--rebuild".to_string(),
    ];
    if chroot && helper == "paru" {
        argv.push("--chroot".to_string());
    }
    // Ни одного вопроса в никуда. Оба сборщика по умолчанию предлагают
    // посмотреть изменения в сборочных файлах и ждут ответа — а отвечать
    // некому: терминала у окна нет, и сборка встала бы навсегда.
    argv.push(match helper {
        "paru" => "--skipreview".to_string(),
        // ⚠️ Именно `--diffmenu=false`: `--nodiffmenu` yay не понимает
        // (проверено запуском 10.08.2026 — «invalid option»).
        _ => "--diffmenu=false".to_string(),
    });
    argv.push("--noconfirm".to_string());
    // Права: шим сеанса прав (пункт Б10, `privilege::aur`) — только без
    // чистого окружения: в нём paru зовёт через sudo `mkarchroot` и
    // `arch-nspawn`, а шим пропускает лишь pacman. Иначе — `pkexec`.
    let shim = if chroot && helper == "paru" {
        None
    } else {
        crate::privilege::aur::sudo_for(packages, false)
    };
    match shim {
        Some(sudo) => {
            if helper == "paru" {
                argv.extend(["--nochroot".to_string(), "--nolocalrepo".to_string()]);
            }
            argv.extend(["--sudo".to_string(), sudo.program, "--sudoflags".to_string(), sudo.flags]);
        }
        None => argv.extend(["--sudo".to_string(), "pkexec".to_string(), "--sudoflags".to_string(), String::new()]),
    }
    argv.extend(packages.iter().cloned());
    Some(argv)
}

// ══════════════════════════════════════════════════════════════════════════
// Пересборка: что решается ДО запуска сборщика, как читается её ход и итог
// ══════════════════════════════════════════════════════════════════════════
//
// Наружу (в `rexon/system.py`) остаются ровно три действия: спросить у
// системы, есть ли paru/yay/mkarchroot и агент polkit, дождаться замка
// пакетного менеджера и запустить сам сборщик, читая его вывод построчно.
// Всё остальное — здесь.

/// Сколько последних строк вывода держим на руках. Больше незачем: весь ход
/// сборки человек и так видит в окне прогресса, хвост нужен только для
/// итогового сообщения об ошибке.
pub const REBUILD_TAIL_LINES: usize = 40;
/// Сколько строк хвоста показать, когда сборка упала.
pub const REBUILD_FAILURE_LINES: usize = 15;
/// Стадия, под которой ход сборки идёт в окно прогресса.
pub const REBUILD_LINE_STAGE: &str = "package";
/// Стадия начала работы.
pub const REBUILD_START_STAGE: &str = "install";
/// Стадия успешного конца.
pub const REBUILD_DONE_STAGE: &str = "done";

/// Пересобирать нечего.
pub fn rebuild_nothing() -> String {
    tr("Nothing to rebuild")
}
/// В системе нет ни paru, ни yay.
pub fn rebuild_no_helper() -> String {
    tr("No AUR package builder found (paru or yay) — the appearance can't be rebuilt without it")
}
/// Команду собрать не удалось (незнакомый хелпер).
pub fn rebuild_no_command() -> String {
    tr("Couldn't put together the rebuild command")
}
/// Отметка в окне прогресса, когда сборка удалась.
pub fn rebuild_done_message() -> String {
    tr("✅ Appearance rebuilt")
}
/// Итог для человека, когда сборка удалась.
pub fn rebuild_success_message() -> String {
    tr("The appearance was rebuilt for the current system version.\nLog out and back in \
        for everything to take effect.")
}

/// Сообщение о начале пересборки.
pub fn rebuild_start_message(packages: usize) -> String {
    ntr(
        "🔧 Rebuilding %n appearance package…",
        "🔧 Rebuilding %n appearance packages…",
        packages as u64,
    )
}

/// Сборщик не удалось даже запустить (ошибка ОС).
pub fn rebuild_spawn_failure(helper: &str, error: &str) -> String {
    trf("Couldn't start %1: %2", &[&helper, &error])
}

/// Отказ, когда сборка завершилась не нулём и не сказала ни слова.
pub fn rebuild_exit_code_message(code: i32) -> String {
    trf("The build exited with code %1", &[&code])
}

/// Что делать с пересборкой: либо готовый отказ, либо команда и с чего начать.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RebuildPlan {
    /// Готовый отказ — наружу идти незачем.
    pub error: Option<String>,
    /// Команда сборщика (пусто, когда есть `error`).
    pub argv: Vec<String>,
    /// Стадия и текст первой отметки в окне прогресса.
    pub start_stage: String,
    pub start_message: String,
}

/// Решение «можно ли пересобирать и чем» — целиком по явным входам.
///
/// `helper` — найденный в системе сборщик AUR ([`pick_helper`]);
/// `chroot` — доступна ли сборка в чистом окружении ([`chroot_available`]);
/// `polkit_ok` — есть ли чем запросить права; `polkit_missing` — готовый
/// текст отказа на этот случай (его строит `system::polkit_missing_message`).
///
/// Порядок проверок тот же, что был в Python: нечего собирать → нечем
/// собирать → нечем спросить права → команда не сложилась.
pub fn rebuild_plan(
    packages: &[String],
    helper: Option<&str>,
    chroot: bool,
    polkit_ok: bool,
    polkit_missing: &str,
) -> RebuildPlan {
    let refuse = |text: &str| RebuildPlan {
        error: Some(text.to_string()),
        argv: Vec::new(),
        start_stage: REBUILD_START_STAGE.to_string(),
        start_message: String::new(),
    };
    if packages.is_empty() {
        return refuse(&rebuild_nothing());
    }
    let Some(helper) = helper else {
        return refuse(&rebuild_no_helper());
    };
    if !polkit_ok {
        return refuse(polkit_missing);
    }
    let Some(argv) = rebuild_command(helper, packages, chroot) else {
        return refuse(&rebuild_no_command());
    };
    RebuildPlan {
        error: None,
        argv,
        start_stage: REBUILD_START_STAGE.to_string(),
        start_message: rebuild_start_message(packages.len()),
    }
}

/// Строка вывода сборщика: что из неё показывать и запоминать.
/// None — строку пропустить (пустая либо одни пробелы).
pub fn rebuild_line(raw: &str) -> Option<String> {
    let line = raw.trim_end();
    if line.is_empty() {
        None
    } else {
        Some(line.to_string())
    }
}

/// Итог пересборки.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RebuildOutcome {
    pub ok: bool,
    /// Что вернуть вызывающему.
    pub message: String,
    /// Последняя отметка в окне прогресса (None — отмечать нечего).
    pub log_stage: Option<String>,
    pub log_message: Option<String>,
}

/// Разбор кода возврата сборщика вместе с хвостом его вывода.
///
/// `tail` — последние строки вывода (не длиннее [`REBUILD_TAIL_LINES`]).
/// При неудаче человеку показываются последние [`REBUILD_FAILURE_LINES`]
/// строк; если сборщик не сказал вовсе ничего — хотя бы код возврата.
pub fn rebuild_outcome(code: i32, tail: &[String]) -> RebuildOutcome {
    if code == 0 {
        return RebuildOutcome {
            ok: true,
            message: rebuild_success_message(),
            log_stage: Some(REBUILD_DONE_STAGE.to_string()),
            log_message: Some(rebuild_done_message()),
        };
    }
    let from = tail.len().saturating_sub(REBUILD_FAILURE_LINES);
    let shown = tail[from..].join("\n");
    RebuildOutcome {
        ok: false,
        message: if shown.is_empty() {
            rebuild_exit_code_message(code)
        } else {
            shown
        },
        log_stage: None,
        log_message: None,
    }
}

// ══════════════════════════════════════════════════════════════════════════
// Пересборка в окне на Rust: что добавляет исполнитель ядра
// ══════════════════════════════════════════════════════════════════════════
//
// Исполняет пересборку `system_de_exec::DeOpsExec::rebuild_aeroshell` — тем же
// порядком, что установку рабочего стола: ход дела в окно операции, ожидание
// замка пакетного менеджера, отмена пока система не тронута. Здесь — только
// решения и тексты, которых у Python-пути не было.

/// Проверка оформления уже идёт (вторая кнопка или проверка при запуске).
pub fn rebuild_busy_message() -> String {
    tr("⏳ The appearance is already being checked — wait until it finishes.")
}
/// Кнопка «Проверить сейчас» нажата — сверка ушла в фон.
pub fn survey_running_message() -> String {
    tr("🔍 Checking the appearance…")
}
/// Сборка не уложилась в отведённое время (класс «сборка», часы).
pub fn rebuild_timeout_message() -> String {
    tr("The appearance build took too long and was stopped. Try again later.")
}
/// Первая строка итога, когда сборка упала.
pub fn rebuild_failed_head() -> String {
    tr("Couldn't rebuild the appearance.")
}
/// Подпись над хвостом вывода сборщика в итоге неудачи.
pub fn rebuild_tail_head() -> String {
    tr("Last lines of the build:")
}

/// Окружение запуска сборщика: английская речь. Тот же приём, что у Python
/// (`LANG=C`, `LC_ALL=C`): по этой речи узнаётся отказ пароля, а на русской
/// локали окно пароля отвечало бы по-русски, и отказ читался бы как сбой
/// сборки.
pub fn rebuild_argv_c_locale(argv: &[String]) -> Vec<String> {
    let mut out: Vec<String> = ["env", "LANG=C", "LC_ALL=C"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    out.extend(argv.iter().cloned());
    out
}

/// Сборщик упал потому, что человек не подтвердил пароль администратора.
///
/// Права сборщик просит сам (`--sudo pkexec`), и код окна пароля до нас не
/// доходит — только его слова в выводе: «Request dismissed» (окно закрыли),
/// «Not authorized» (нет агента polkit) либо код 126 в строке ошибки
/// сборщика.
pub fn rebuild_auth_denied(output: &str) -> bool {
    crate::system_plan::pkexec_auth_denied(0, output)
        || output.contains("Request dismissed")
        || output.contains("exit status 126")
}

/// Итог отказа пароля: что случилось и что делать. Кнопка названа теми же
/// словами, что написаны на ней.
pub fn rebuild_denied_message() -> String {
    let button = crate::qt_settings::text("aero.check_now", &[]).unwrap_or_default();
    let button = button.trim_start_matches(|c: char| !c.is_alphanumeric());
    trf(
        // TRANSLATORS: %1 is the label of the “Check Now” button in Rexon settings.
        "The administrator password wasn't confirmed — the rebuild was stopped.\n\
         Click “%1” in Rexon settings when you're ready to enter the password.",
        &[&button],
    )
}

/// Итог неудачной сборки: сначала главное для человека (система цела), потом
/// хвост вывода сборщика — его разбор ([`rebuild_outcome`]) уже готов.
pub fn rebuild_failure_message(builder_output: &str) -> String {
    format!(
        "{} {}\n\n{}\n{builder_output}",
        rebuild_failed_head(),
        crate::de_progress::aeroshell_fail_note(),
        rebuild_tail_head(),
    )
}

/// Текст окна вопроса: главный текст и объяснение одним блоком (простой
/// HTML). Имена пакетов и версии в вопрос не идут — покупателю они ни о чём
/// не говорят; их видно в журнале хода ([`crate::de_progress::aeroshell_details`]).
pub fn confirm_text() -> String {
    format!(
        "{}<br><br>{}",
        crate::de_progress::aeroshell_text(),
        crate::de_progress::aeroshell_informative()
    )
}

/// Заголовок окна вопроса: при одном только риске ([`Report::at_risk_only`])
/// — «нужно пересобрать», иначе — «перестало работать».
pub fn confirm_title_for(report: &Report) -> String {
    if report.at_risk_only() {
        crate::de_progress::aeroshell_risk_title()
    } else {
        crate::de_progress::aeroshell_title()
    }
}

/// Подпись кнопки согласия в окне вопроса — под стать заголовку
/// ([`confirm_title_for`]): при одном только риске — «Пересобрать», при
/// поломке — «Восстановить». Действие у кнопки одно и то же.
pub fn confirm_button_for(report: &Report) -> String {
    if report.at_risk_only() {
        crate::de_progress::aeroshell_rebuild_button()
    } else {
        crate::de_progress::aeroshell_fix_button()
    }
}

/// [`confirm_text`] с тем, что назвал журнал сеанса: какой эффект или
/// виджет выключен и почему (простой HTML). Журнал пуст — как прежде.
pub fn confirm_text_for(report: &Report) -> String {
    if report.at_risk_only() {
        return format!(
            "{}<br><br>{}",
            crate::de_progress::aeroshell_risk_text(),
            crate::de_progress::aeroshell_informative()
        );
    }
    let lines = journal_lines(report);
    if lines.is_empty() {
        return confirm_text();
    }
    let list: Vec<String> = lines
        .iter()
        .map(|l| format!("• {}", crate::ui_base::escape_dialog_text(l)))
        .collect();
    format!(
        "{}<br><br>{}<br><br>{}",
        crate::de_progress::aeroshell_text(),
        list.join("<br>"),
        crate::de_progress::aeroshell_informative()
    )
}

/// Подробности сверки для журнала — из итога сверки.
pub fn report_details(report: &Report) -> String {
    let packages: Vec<crate::de_progress::AeroshellPackage> = report
        .packages
        .iter()
        .map(|p| crate::de_progress::AeroshellPackage {
            package: p.package.clone(),
            built_for: p.built_for.clone().unwrap_or_default(),
            installed: p.installed.clone().unwrap_or_default(),
            stale: p.stale,
        })
        .collect();
    let mut text = crate::de_progress::aeroshell_details(&packages);
    for p in &report.packages {
        for problem in &p.problems {
            text.push_str(&format!("\n  {}: {problem}", p.package));
        }
    }
    for f in &report.journal {
        text.push_str(&format!("\n{}", crate::plugin_journal::finding_technical(f)));
    }
    text
}

// ══════════════════════════════════════════════════════════════════════════
// Проверка вне запуска Rexon: после обновления пакетов и при входе в сеанс
// ══════════════════════════════════════════════════════════════════════════
//
// Сверка при запуске Rexon ничего не даёт тому, кто обновил систему и Rexon
// не открывал: стол ломается при следующем входе, и узнаёт он об этом по
// пустой панели. Поэтому — пользовательские юниты systemd (без root, без
// пароля). Юниты общие у всех проверок «до и после обновления» —
// [`crate::update_guard::watch_units`]: их `rexon --update-check` при
// стоящих форках Plasma зовёт и эту сверку (`rexon --aeroshell-check`):
// дождаться конца транзакции pacman, сверить (только чтение) и при беде
// показать уведомление с кнопкой, открывающей Rexon сразу на проверке.
// Пароль спрашивается только при самой пересборке или откате — в окне
// Rexon, один раз.

/// Флаг безоконного маршрута проверки (его зовёт `.service`).
pub const CHECK_FLAG: &str = "--aeroshell-check";
/// Добавка к [`CHECK_FLAG`]: проверка при входе — уведомлять даже о том,
/// о чём уже говорили (новый сеанс — новый человек у экрана).
pub const LOGIN_FLAG: &str = "--login";
/// Флаг окна, открытого из уведомления: проверить сразу, даже если
/// проверка при запуске выключена, и открыть «Настройки».
pub const OPEN_FLAG: &str = "--aeroshell-repair";

pub use crate::update_guard::{LOCK_WAIT_SECS, PACMAN_DB_LOCK, PACMAN_LOCAL_DB};
/// Сколько ждать нажатия в уведомлении, секунды.
pub const NOTIFY_WAIT_SECS: u64 = 6 * 60 * 60;

/// Отпечаток состояния для уведомления: о том же самом второй раз не
/// говорим (слежка срабатывает на каждый пакет обновления).
pub fn notify_fingerprint(report: &Report, advice: &Advice) -> String {
    // Для предупреждений до обновления важно, ЧТО ждёт: новая серия
    // («не обновляйтесь» до 6.8 — одно состояние, 6.8.0 и 6.8.1 не повод
    // повторять) и точный выпуск для мягкого предупреждения.
    let kind = match advice {
        Advice::Nothing => "nothing".to_string(),
        Advice::Rebuild { .. } if report.at_risk_only() => "rebuild-risk".to_string(),
        Advice::Rebuild { .. } => "rebuild".to_string(),
        Advice::RestoreStock { .. } => "restore".to_string(),
        Advice::RestoreBlocked { .. } => "blocked".to_string(),
        Advice::WaitForUpstream { .. } => "wait".to_string(),
        Advice::HoldUpdate { plasma, .. } => format!("hold:{}", plasma_series(plasma)),
        Advice::RebuildAfterUpdate { to, .. } => format!("pending:{to}"),
        Advice::BrokenPlugins { .. } => "plugins".to_string(),
    };
    let mut fp = format!(
        "{kind} kwin={} plasma={} stale={}",
        report.kwin_version.as_deref().unwrap_or("-"),
        report.plasma_version.as_deref().unwrap_or("-"),
        report.stale.join(","),
    );
    // Журнал — только когда он что-то назвал: прежние отпечатки остаются
    // прежними, и о том же состоянии второй раз не говорим.
    if !report.journal.is_empty() {
        let mut ids: Vec<&str> = report.journal.iter().map(|f| f.failure.id.as_str()).collect();
        ids.sort_unstable();
        fp.push_str(&format!(" journal={}", ids.join(",")));
    }
    fp
}

/// Уведомление: заголовок, текст (обычный) и подпись кнопки.
#[derive(Debug, Clone, PartialEq)]
pub struct Notice {
    pub summary: String,
    pub body: String,
    pub button: String,
}

/// Что сказать уведомлением по совету. None — говорить нечего.
pub fn notice_for(advice: &Advice) -> Option<Notice> {
    let (summary, body) = match advice {
        Advice::Nothing => return None,
        Advice::Rebuild { .. } => (
            crate::de_progress::aeroshell_title(),
            tr("After the system update, AeroThemePlasma no longer fits the new Plasma: \
                the taskbar, the Start menu and effects may be missing. Rexon can rebuild it."),
        ),
        Advice::RestoreStock { .. } => (
            restore_title(),
            tr("AeroThemePlasma is gone, but its replacement for a part of Plasma is still \
                installed. The next Plasma update can break the whole desktop — Rexon can \
                put the standard part back."),
        ),
        Advice::RestoreBlocked { message } => (restore_blocked_title(), message.clone()),
        Advice::WaitForUpstream { plasma, fork } => (wait_title(), wait_text(plasma, fork)),
        Advice::HoldUpdate { plasma, fork } => (hold_title(), hold_text(plasma, fork)),
        Advice::RebuildAfterUpdate { to, .. } => (rebuild_after_title(), rebuild_after_text(to)),
        Advice::BrokenPlugins { .. } => (crate::plugin_journal::plugins_title(), String::new()),
    };
    Some(Notice {
        summary,
        body,
        button: tr("Open Rexon"),
    })
}

/// [`notice_for`] с тем, что назвал журнал сеанса: строки «что сломано —
/// что делать» идут следом за общим текстом.
pub fn notice_for_report(advice: &Advice, report: &Report) -> Option<Notice> {
    let mut notice = notice_for(advice)?;
    // Один только риск — мягкие слова, как у окна ([`confirm_title_for`]):
    // стол работает, и «перестало работать» над ним читается как враньё.
    if matches!(advice, Advice::Rebuild { .. }) && report.at_risk_only() {
        notice.summary = crate::de_progress::aeroshell_risk_title();
        notice.body = tr(
            "After the system update, AeroThemePlasma is still built for the previous version \
             of the system. It works for now, but it can fail at any moment. Rexon can rebuild it.",
        );
    }
    let lines = match advice {
        Advice::BrokenPlugins { .. } => {
            let outside: Vec<crate::plugin_journal::Finding> =
                report.journal_outside().into_iter().cloned().collect();
            crate::plugin_journal::finding_lines(&outside)
        }
        Advice::Rebuild { .. } | Advice::WaitForUpstream { .. } => journal_lines(report),
        _ => Vec::new(),
    };
    if !lines.is_empty() {
        let list = lines.iter().map(|l| format!("• {l}")).collect::<Vec<_>>().join("\n");
        notice.body = if notice.body.is_empty() { list } else { format!("{}\n\n{list}", notice.body) };
    }
    Some(notice)
}

/// Уведомлять ли: есть что сказать, и это не то же, о чём уже говорили
/// (`last` — сохранённый отпечаток). При входе в сеанс говорим всегда.
pub fn should_notify(advice: &Advice, fingerprint: &str, last: Option<&str>, login: bool) -> bool {
    // Сторонние модули при входе не повторяем: это не поломка всего стола,
    // а сказанное один раз про снятый с диска виджет иначе звучало бы на
    // каждом входе.
    // Риск без поломки (отпечаток `rebuild-risk`) — тоже один раз.
    let login = login
        && !matches!(advice, Advice::BrokenPlugins { .. })
        && !fingerprint.starts_with("rebuild-risk ");
    *advice != Advice::Nothing && (login || last.map(str::trim) != Some(fingerprint))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upstream_version_strips_epoch_and_pkgrel() {
        assert_eq!(upstream_version("6.7.4-3.1"), "6.7.4");
        assert_eq!(upstream_version("6.7.4"), "6.7.4");
        assert_eq!(upstream_version("1:6.7.4-1"), "6.7.4");
        assert_eq!(upstream_version(" 6.8.0-1 "), "6.8.0");
    }

    #[test]
    fn effect_iid_version_reads_marker() {
        let blob = b"\x00\x00IID\x00org.kde.kwin.EffectPluginFactory6.7.4\x00rest";
        assert_eq!(effect_iid_version(blob).as_deref(), Some("6.7.4"));
        assert_eq!(effect_iid_version(b"no marker here"), None);
    }

    #[test]
    fn effect_iid_version_skips_marker_without_version() {
        // Голый опознаватель (заголовок KWin) пропускается, версия берётся
        // из следующего вхождения.
        let blob = b"org.kde.kwin.EffectPluginFactory\x00org.kde.kwin.EffectPluginFactory6.8.0";
        assert_eq!(effect_iid_version(blob).as_deref(), Some("6.8.0"));
    }

    fn ql(pkg: &str, paths: &[&str]) -> String {
        paths
            .iter()
            .map(|p| format!("{pkg} {p}"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn package_files_drops_directories() {
        let out = ql("p", &["/usr/lib/", "/usr/lib/a.so"]);
        assert_eq!(package_files(&out), vec!["/usr/lib/a.so".to_string()]);
    }

    #[test]
    fn effect_plugin_paths_ignores_config_pages() {
        let files = package_files(&ql(
            "p",
            &[
                "/usr/lib/qt6/plugins/kwin/effects/plugins/aeroglide.so",
                "/usr/lib/qt6/plugins/kwin/effects/configs/kwin_aeroglide_config.so",
                "/usr/share/kwin/effects/dimscreenaero/metadata.json",
            ],
        ));
        assert_eq!(
            effect_plugin_paths(&files),
            vec!["/usr/lib/qt6/plugins/kwin/effects/plugins/aeroglide.so".to_string()]
        );
    }

    #[test]
    fn plasma_library_version_takes_full_version_only() {
        let files = package_files(&ql(
            "p",
            &[
                "/usr/lib/libPlasma.so",
                "/usr/lib/libPlasma.so.7",
                "/usr/lib/libPlasma.so.6.7.4",
            ],
        ));
        assert_eq!(plasma_library_version(&files).as_deref(), Some("6.7.4"));
        assert_eq!(plasma_library_version(&[]), None);
    }

    /// Набор как на живой машине: модуль эффекта под старый KWin, форк
    /// libplasma под текущую Plasma, пакет с одними картинками.
    fn fixture(pkg: &str) -> Option<String> {
        match pkg {
            "aeroshell-kwin-components-git" => Some(ql(
                pkg,
                &["/usr/lib/qt6/plugins/kwin/effects/plugins/aeroglide.so"],
            )),
            "aeroshell-libplasma-git" => Some(ql(
                pkg,
                &["/usr/lib/libPlasma.so.6.7.4", "/usr/lib/libPlasma.so.7"],
            )),
            "aerothemeplasma-icons-git" => Some(ql(pkg, &["/usr/share/icons/a.png"])),
            _ => None,
        }
    }

    fn effect_blob(version: &str) -> Vec<u8> {
        format!("org.kde.kwin.EffectPluginFactory{version}\0").into_bytes()
    }

    #[test]
    fn check_finds_kwin_effect_mismatch() {
        let forks: Vec<String> = ["aeroshell-kwin-components-git", "aeroshell-libplasma-git"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let report = check_with(
            &forks,
            Some("6.7.5-1"),
            Some("6.7.4-1.1"),
            &fixture,
            &|_p| Some(effect_blob("6.7.4")),
        );
        assert!(report.needs_rebuild);
        assert_eq!(report.stale, vec!["aeroshell-kwin-components-git".to_string()]);
        // Пересобирается ВЕСЬ собранный набор, а не только отвалившийся пакет.
        assert_eq!(report.rebuild, forks);
        assert_eq!(report.packages[0].marker, Marker::KwinEffect);
        assert_eq!(report.packages[0].built_for.as_deref(), Some("6.7.4"));
        assert_eq!(report.packages[0].installed.as_deref(), Some("6.7.5"));
        assert_eq!(report.packages[1].marker, Marker::PlasmaLibrary);
        assert!(!report.packages[1].stale);
    }

    #[test]
    fn check_finds_libplasma_mismatch() {
        let forks = vec!["aeroshell-libplasma-git".to_string()];
        let report = check_with(&forks, Some("6.8.0"), Some("6.8.0"), &fixture, &|_p| None);
        assert!(report.needs_rebuild);
        assert_eq!(report.stale, forks);
    }

    #[test]
    fn check_is_quiet_when_versions_match() {
        let forks: Vec<String> = ["aeroshell-kwin-components-git", "aeroshell-libplasma-git"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let report = check_with(&forks, Some("6.7.4-3.1"), Some("6.7.4-1.1"), &fixture, &|_p| {
            Some(effect_blob("6.7.4"))
        });
        assert!(!report.needs_rebuild);
        assert!(report.stale.is_empty());
    }

    #[test]
    fn check_does_not_cry_without_evidence() {
        // Пакет без меток и пакет, чей список файлов не прочитался: молчим.
        let forks: Vec<String> = ["aerothemeplasma-icons-git", "aeroshell-unknown-git"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let report = check_with(&forks, Some("6.8.0"), Some("6.8.0"), &fixture, &|_p| None);
        assert!(!report.needs_rebuild);
        assert!(report.rebuild.is_empty());
        assert_eq!(report.packages[0].marker, Marker::None);
        assert!(!report.packages[0].compiled);
    }

    #[test]
    fn check_without_installed_versions_is_silent() {
        let forks = vec!["aeroshell-libplasma-git".to_string()];
        let report = check_with(&forks, None, None, &fixture, &|_p| None);
        assert!(!report.needs_rebuild);
        assert_eq!(report.packages[0].built_for.as_deref(), Some("6.7.4"));
        assert_eq!(report.packages[0].installed, None);
    }

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn lonely_fork_is_libplasma_without_shell() {
        assert!(lonely_libplasma_fork(&names(&["aeroshell-libplasma-git"])));
        assert!(lonely_libplasma_fork(&names(&[
            "aeroshell-kwin-components-git",
            "aeroshell-libplasma-git",
            "aerothemeplasma-icons-git",
        ])));
        // Оболочка стоит (любой из двух сеансов) — форк ей нужен.
        assert!(!lonely_libplasma_fork(&names(&[
            "aeroshell-libplasma-git",
            "aerothemeplasma-desktop-git",
        ])));
        assert!(!lonely_libplasma_fork(&names(&[
            "aeroshell-libplasma-git",
            "aerothemeplasma-desktop-x11-git",
        ])));
        // Форка libplasma нет — откатывать нечего.
        assert!(!lonely_libplasma_fork(&names(&["aerothemeplasma-icons-git"])));
        assert!(!lonely_libplasma_fork(&[]));
    }

    /// Машина как у владельца 04.10: от набора остался один форк libplasma,
    /// версии сходятся. Совет — вернуть штатную Plasma, а не «всё хорошо».
    #[test]
    fn lonely_fork_advises_restore_even_when_versions_match() {
        let forks = names(&["aeroshell-libplasma-git", "aerothemeplasma-icons-git"]);
        let report = check_with(&forks, Some("6.7.4"), Some("6.7.4-1.1"), &fixture, &|_p| None);
        assert!(!report.needs_rebuild);
        assert!(report.lonely_fork);
        // Значки остаются: они ничего не подменяют.
        assert_eq!(report.restore, names(&["aeroshell-libplasma-git"]));
        assert_eq!(
            advise(&report, None),
            Advice::RestoreStock { forks: names(&["aeroshell-libplasma-git"]) }
        );
        assert_eq!(
            advise(&report, Some((true, "cmd".into()))),
            Advice::RestoreStock { forks: names(&["aeroshell-libplasma-git"]) }
        );
    }

    /// Одинокий и уже отставший форк — всё равно откат, не пересборка.
    #[test]
    fn lonely_stale_fork_still_restores() {
        let forks = names(&["aeroshell-kwin-components-git", "aeroshell-libplasma-git"]);
        let report = check_with(&forks, Some("6.8.0"), Some("6.8.0"), &fixture, &|_p| {
            Some(effect_blob("6.7.4"))
        });
        assert!(report.needs_rebuild);
        assert_eq!(report.restore, forks);
        assert_eq!(advise(&report, None), Advice::RestoreStock { forks });
    }

    #[test]
    fn restore_without_stock_package_is_refused_in_words() {
        let forks = names(&["aeroshell-libplasma-git"]);
        let report = check_with(&forks, None, None, &fixture, &|_p| None);
        assert_eq!(
            advise(&report, Some((false, "no stock".into()))),
            Advice::RestoreBlocked { message: "no stock".into() }
        );
    }

    #[test]
    fn shell_installed_keeps_rebuild_advice() {
        let forks = names(&["aeroshell-libplasma-git", "aerothemeplasma-desktop-git"]);
        let report = check_with(&forks, Some("6.8.0"), Some("6.8.0"), &fixture, &|_p| None);
        assert!(!report.lonely_fork);
        assert!(report.restore.is_empty());
        assert_eq!(
            advise(&report, None),
            Advice::Rebuild { packages: names(&["aeroshell-libplasma-git"]) }
        );
        let fresh = check_with(&forks, Some("6.7.4"), Some("6.7.4"), &fixture, &|_p| None);
        assert_eq!(advise(&fresh, None), Advice::Nothing);
    }

    #[test]
    fn upstream_parsing() {
        assert_eq!(
            upstream_git_url("https://gitgud.io/aeroshell/libplasma").as_deref(),
            Some("https://gitgud.io/aeroshell/libplasma.git")
        );
        assert_eq!(upstream_git_url("https://x.org/a.git/").as_deref(), Some("https://x.org/a.git"));
        assert_eq!(upstream_git_url("git://x.org/a"), None);
        assert_eq!(
            default_branch("ref: refs/heads/Plasma/6.7\tHEAD\n6ed0921cf\tHEAD\n").as_deref(),
            Some("Plasma/6.7")
        );
        assert_eq!(default_branch("6ed0921cf\tHEAD\n"), None);
        assert_eq!(
            raw_file_url("https://gitgud.io/aeroshell/libplasma", "Plasma/6.7", "CMakeLists.txt").as_deref(),
            Some("https://gitgud.io/aeroshell/libplasma/-/raw/Plasma/6.7/CMakeLists.txt")
        );
        assert_eq!(
            raw_file_url("https://github.com/aeroshell-desktop/aerothemeplasma/", "main", "f").as_deref(),
            Some("https://raw.githubusercontent.com/aeroshell-desktop/aerothemeplasma/main/f")
        );
        assert_eq!(raw_file_url("https://example.org/a/b", "main", "f"), None);
        assert_eq!(
            cmake_project_version("cmake_minimum_required(VERSION 3.16)\nset(PROJECT_VERSION \"6.7.91\")\nproject(x)").as_deref(),
            Some("6.7.91")
        );
        assert_eq!(cmake_project_version("project(x VERSION 1.0)"), None);
        assert_eq!(
            newest_version(&[Some("6.7.5-1.1".into()), Some("6.8.0-1".into()), None]).as_deref(),
            Some("6.8.0")
        );
        assert!(upstream_lag("6.7.5", "6.8.0"));
        assert!(upstream_lag("6.7.91", "6.8.0"), "бета форка — ещё не догнал");
        assert!(upstream_lag("6.7.5", "6.7.6"));
        assert!(!upstream_lag("6.8.0", "6.8.0"));
        assert!(!upstream_lag("6.8.1", "6.8.0"));
        assert!(!upstream_lag("", "6.8.0"));
    }

    #[test]
    fn lagging_upstream_turns_rebuild_into_wait() {
        let forks = names(&["aeroshell-libplasma-git", "aerothemeplasma-desktop-git"]);
        let report = check_with(&forks, Some("6.8.0"), Some("6.8.0"), &fixture, &|_p| None);
        assert_eq!(
            advise_with_upstream(&report, None, Some(("6.8.0".into(), "6.7.5".into()))),
            Advice::WaitForUpstream { plasma: "6.8.0".into(), fork: "6.7.5".into() }
        );
        assert!(matches!(
            advise_with_upstream(&report, None, Some(("6.8.0".into(), "6.8.0".into()))),
            Advice::Rebuild { .. }
        ));
        // Ничего не сломано — сверка с авторами ничего не меняет.
        let fresh = check_with(&forks, Some("6.7.4"), Some("6.7.4"), &fixture, &|_p| None);
        assert_eq!(
            advise_with_upstream(&fresh, None, Some(("6.8.0".into(), "6.7.5".into()))),
            Advice::Nothing
        );
    }

    #[test]
    fn wait_text_names_versions_plainly() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let big = wait_text("6.8.0", "6.7.5");
        assert!(big.contains("Plasma 6.8 (their version is still 6.7.5)"), "{big}");
        let point = wait_text("6.7.6", "6.7.5");
        assert!(point.contains("Plasma 6.7.6 (their version"), "{point}");
    }

    #[test]
    fn qt_markers_in_module_bytes() {
        let blob = b"\0libQt6Core.so.6\0Qt_6\0Qt_6.11\0Qt_6.2\0Qt_6_PRIVATE_API\0";
        assert_eq!(qt_minor_built(blob), Some(11));
        assert!(uses_qt_private_api(blob));
        assert_eq!(qt_minor_built(b"Qt_6\0nothing"), None);
        assert!(!uses_qt_private_api(b"Qt_6.11"));
        assert_eq!(qt_minor("6.11.2-3"), Some(11));
        assert_eq!(qt_minor("5.15.2"), None);
    }

    /// Вывод `ldd -r` в той форме, что дают живые модули (проверено на
    /// собранных пакетах набора 04.10.2026).
    #[test]
    fn ldd_problems_reads_missing_and_undefined() {
        let out = "\tlinux-vdso.so.1 (0x00007ffd)\n\
                   \tlibprocesscore.so.10 => not found\n\
                   \tlibPlasma.so.7 => /usr/lib/libPlasma.so.7 (0x7f)\n\
                   /usr/lib/x/a.so: /usr/lib/libQt6Core.so.6: version `Qt_6.12' not found (required by /usr/lib/x/a.so)\n\
                   undefined symbol: _ZN6Plasma6Applet3fooEv\t(/usr/lib/x/a.so)\n";
        assert_eq!(
            ldd_problems(out, &[]),
            vec![
                "libprocesscore.so.10: not found".to_string(),
                "/usr/lib/x/a.so: /usr/lib/libQt6Core.so.6: version `Qt_6.12' not found".to_string(),
                "undefined symbol: _ZN6Plasma6Applet3fooEv".to_string(),
            ]
        );
        assert!(ldd_problems("\tlibc.so.6 => /usr/lib/libc.so.6 (0x7f)\n", &[]).is_empty());
        // С версией символа (живой `ldd -r /usr/bin/sddm`, ВМ SteamOS 25.10.2026).
        assert_eq!(
            ldd_problems("undefined symbol: _ZTI20QDBusAbstractAdaptor, version Qt_6\t(/usr/bin/sddm)\n", &[]),
            vec!["undefined symbol: _ZTI20QDBusAbstractAdaptor".to_string()]
        );
        // Файл библиотеки есть, но пуст (живой `ldd -r`, ВМ SteamOS 25.10.2026).
        assert_eq!(
            ldd_problems(
                "\n/usr/bin/sddm-greeter-qt6: error while loading shared libraries: /usr/lib/libQt6Quick.so.6: file too short\n",
                &[]
            ),
            vec!["/usr/lib/libQt6Quick.so.6: file too short".to_string()]
        );
    }

    /// smodglow.so требует декорацию по голому имени: это сосед по набору,
    /// его грузит хозяин — не поломка (живой `ldd -r` даёт ровно такое).
    #[test]
    fn ldd_problems_ignores_sibling_from_the_set() {
        let out = "\torg.smod.smod.so => not found\n\
                   undefined symbol: _ZN4SMOD10Decoration16staticMetaObjectE\t(/x/smodglow.so)\n";
        let siblings = vec!["/usr/lib/qt6/plugins/org.kde.kdecoration3/org.smod.smod.so".to_string()];
        assert!(ldd_problems(out, &siblings).is_empty());
        assert_eq!(ldd_problems(out, &[]).len(), 2);
    }

    fn desktop_fixture(pkg: &str) -> Option<String> {
        match pkg {
            "aerothemeplasma-desktop-git" => Some(ql(
                pkg,
                &[
                    "/usr/lib/qt6/plugins/plasma/applets/io.gitgud.wackyideas.SevenStart.so",
                    "/usr/lib/qt6/plugins/plasma/applets/io.gitgud.wackyideas.notifications.so",
                    "/usr/share/plasma/plasmoids/io.gitgud.wackyideas.SevenStart/metadata.json",
                ],
            )),
            _ => fixture(pkg),
        }
    }

    fn applet_blob(path: &str) -> Option<Vec<u8>> {
        Some(if path.ends_with("notifications.so") {
            b"Qt_6\0Qt_6.11\0Qt_6_PRIVATE_API\0".to_vec()
        } else {
            b"Qt_6\0Qt_6.11\0".to_vec()
        })
    }

    /// Плазмоиды больше не «без метки»: чужой минор Qt у модуля с приватным
    /// интерфейсом и ненайденная библиотека делают пакет отставшим.
    #[test]
    fn applets_checked_for_qt_minor_and_missing_libraries() {
        let forks = names(&["aeroshell-libplasma-git", "aerothemeplasma-desktop-git"]);
        let healthy = check_modules_with(
            &forks, Some("6.7.4"), Some("6.7.4"), Some("6.11.2-3"),
            &desktop_fixture, &applet_blob, &|_| Some(String::new()),
        );
        assert!(!healthy.needs_rebuild, "{healthy:?}");
        assert_eq!(healthy.packages[1].marker, Marker::None);

        // Qt ушёл на 6.12: notifications собран под 6.11 с приватным API.
        let qt_moved = check_modules_with(
            &forks, Some("6.7.4"), Some("6.7.4"), Some("6.12.0-1"),
            &desktop_fixture, &applet_blob, &|_| Some(String::new()),
        );
        assert_eq!(qt_moved.stale, names(&["aerothemeplasma-desktop-git"]));
        let p = &qt_moved.packages[1];
        assert_eq!(p.marker, Marker::QtVersion);
        assert_eq!((p.built_for.as_deref(), p.installed.as_deref()), (Some("6.11"), Some("6.12")));
        assert_eq!(
            p.problems,
            vec!["io.gitgud.wackyideas.notifications.so: Qt 6.11 private API, installed Qt 6.12".to_string()]
        );
        assert!(matches!(advise(&qt_moved, None), Advice::Rebuild { .. }));

        // Сменился soname библиотеки, на которую ссылается SevenStart.
        let lib_gone = check_modules_with(
            &forks, Some("6.7.4"), Some("6.7.4"), Some("6.11.2"),
            &desktop_fixture, &applet_blob,
            &|path| Some(if path.contains("SevenStart") {
                "\tlibprocesscore.so.10 => not found\n".to_string()
            } else {
                String::new()
            }),
        );
        assert!(lib_gone.needs_rebuild);
        assert_eq!(
            lib_gone.packages[1].problems,
            vec!["io.gitgud.wackyideas.SevenStart.so: libprocesscore.so.10: not found".to_string()]
        );
        let details = report_details(&lib_gone);
        assert!(details.contains("libprocesscore.so.10: not found"), "{details}");
    }

    // ── Предупреждение до обновления ─────────────────────────────────────

    const CHECKUPDATES_68: &str = "\u{1b}[1mkwin\u{1b}[0m 6.7.5-1.1 -> 6.8.0-1\n\
        plasma-workspace 6.7.5-1.1 -> 6.8.0-1\n\
        firefox 140.0-1 -> 141.0-1\n";
    const CHECKUPDATES_675: &str = "kwin 6.7.4-1.1 -> 6.7.5-1\nplasma-workspace 6.7.4-1.1 -> 6.7.5-1\n";

    #[test]
    fn checkupdates_output_parsed() {
        let list = parse_pending_updates(CHECKUPDATES_68);
        assert_eq!(list.len(), 3);
        assert_eq!(
            list[0],
            PendingUpdate { package: "kwin".into(), installed: "6.7.5-1.1".into(), available: "6.8.0-1".into() }
        );
        assert!(parse_pending_updates("junk\n\n==> error\n").is_empty());
        assert_eq!(pending_plasma(&list), Some(("6.7.5".into(), "6.8.0".into())));
        // Plasma не обновляется — None; пересборка пакета без смены версии — тоже.
        assert_eq!(pending_plasma(&parse_pending_updates("firefox 1-1 -> 2-1\n")), None);
        assert_eq!(pending_plasma(&parse_pending_updates("kwin 6.7.5-1 -> 6.7.5-2\n")), None);
        // Эпоха не путает.
        assert_eq!(
            pending_plasma(&parse_pending_updates("kwin 1:6.7.4-1 -> 1:6.7.5-1\n")),
            Some(("6.7.4".into(), "6.7.5".into()))
        );
    }

    fn working_set() -> Report {
        let forks = names(&["aeroshell-kwin-components-git", "aeroshell-libplasma-git", "aerothemeplasma-desktop-git"]);
        check_with(&forks, Some("6.7.4"), Some("6.7.4"), &fixture, &|_p| {
            Some(b"\0org.kde.kwin.EffectPluginFactory6.7.4\0".to_vec())
        })
    }

    #[test]
    fn new_plasma_series_with_lagging_authors_says_hold() {
        let report = working_set();
        assert_eq!(advise(&report, None), Advice::Nothing, "{report:?}");
        let updates = parse_pending_updates(CHECKUPDATES_68);
        assert_eq!(
            advise_before_update(&report, &updates, Some("6.7.5")),
            Advice::HoldUpdate { plasma: "6.8.0".into(), fork: "6.7.5".into() }
        );
        // Бета ветки форка — ещё не догнали.
        assert!(matches!(advise_before_update(&report, &updates, Some("6.7.91")), Advice::HoldUpdate { .. }));
    }

    #[test]
    fn point_release_warns_softly() {
        let report = working_set();
        let updates = parse_pending_updates(CHECKUPDATES_675);
        let soft = Advice::RebuildAfterUpdate { from: "6.7.4".into(), to: "6.7.5".into() };
        assert_eq!(advise_before_update(&report, &updates, None), soft);
        assert_eq!(advise_before_update(&report, &updates, Some("6.7.4")), soft);
    }

    /// Новая серия, а авторы догнали (в т.ч. 6.8.0 при ждущем 6.8.1) либо
    /// их версия неизвестна — мягко: «не обновляйтесь» не говорим, но
    /// пересборка после обновления понадобится.
    #[test]
    fn new_series_caught_up_or_unknown_warns_softly() {
        let report = working_set();
        let updates = parse_pending_updates(CHECKUPDATES_68);
        let soft = Advice::RebuildAfterUpdate { from: "6.7.5".into(), to: "6.8.0".into() };
        assert_eq!(advise_before_update(&report, &updates, Some("6.8.0")), soft);
        assert_eq!(advise_before_update(&report, &updates, None), soft);
        let later = parse_pending_updates("kwin 6.7.5-1 -> 6.8.1-1\n");
        assert_eq!(
            advise_before_update(&report, &later, Some("6.8.0")),
            Advice::RebuildAfterUpdate { from: "6.7.5".into(), to: "6.8.1".into() }
        );
    }

    #[test]
    fn quiet_without_set_or_plasma_update() {
        let updates = parse_pending_updates(CHECKUPDATES_68);
        // Набора нет.
        assert_eq!(advise_before_update(&Report::empty(), &updates, Some("6.7.5")), Advice::Nothing);
        // Только значки — ломаться нечему.
        let icons = check_with(&names(&["aerothemeplasma-icons-git"]), Some("6.7.4"), Some("6.7.4"), &fixture, &|_p| None);
        assert_eq!(advise_before_update(&icons, &updates, Some("6.7.5")), Advice::Nothing);
        let report = working_set();
        // Обновлений Plasma нет.
        assert_eq!(advise_before_update(&report, &[], Some("6.7.5")), Advice::Nothing);
    }

    #[test]
    fn before_update_notices_are_plain_and_once_per_state() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let report = working_set();
        let hold = Advice::HoldUpdate { plasma: "6.8.0".into(), fork: "6.7.5".into() };
        let n = notice_for(&hold).unwrap();
        assert_eq!(n.summary, hold_title());
        assert!(n.body.starts_with("Plasma 6.8 is out, but AeroThemePlasma supports only Plasma 6.7 so far."), "{}", n.body);
        assert!(n.body.contains("Start menu and the taskbar"), "{}", n.body);
        let soft = Advice::RebuildAfterUpdate { from: "6.7.4".into(), to: "6.7.5".into() };
        assert!(notice_for(&soft).unwrap().body.contains("Plasma 6.7.5 is ready"));
        for a in [&hold, &soft] {
            let body = notice_for(a).unwrap().body;
            for jargon in ["libplasma", "pacman", "checkupdates", "aeroshell-", "IgnorePkg"] {
                assert!(!body.contains(jargon), "жаргон «{jargon}»: {body}");
            }
        }
        // 6.8.0 → 6.8.1 в репозиториях — то же «не обновляйтесь», не повтор.
        let fp = notify_fingerprint(&report, &hold);
        let fp2 = notify_fingerprint(&report, &Advice::HoldUpdate { plasma: "6.8.1".into(), fork: "6.7.5".into() });
        assert_eq!(fp, fp2);
        assert!(!should_notify(&hold, &fp2, Some(&fp), false));
        // Авторы догнали, а ждёт точечный выпуск — новое состояние.
        assert_ne!(fp, notify_fingerprint(&report, &soft));
    }

    #[test]
    fn notify_once_per_state_but_always_at_login() {
        let forks = names(&["aeroshell-libplasma-git"]);
        let report = check_with(&forks, Some("6.7.5"), Some("6.7.5"), &fixture, &|_p| None);
        let advice = advise(&report, None);
        let fp = notify_fingerprint(&report, &advice);
        assert!(fp.starts_with("restore "), "{fp}");
        assert!(should_notify(&advice, &fp, None, false));
        assert!(!should_notify(&advice, &fp, Some(&format!("{fp}\n")), false));
        assert!(should_notify(&advice, &fp, Some(&fp), true));
        assert!(!should_notify(&Advice::Nothing, "x", None, true));
        let notice = notice_for(&advice).unwrap();
        assert_eq!(notice.summary, restore_title());
        assert!(notice_for(&Advice::Nothing).is_none());
        for a in [advice, Advice::Rebuild { packages: forks.clone() }] {
            let n = notice_for(&a).unwrap();
            for jargon in ["libplasma", "pacman", "pkexec", "aeroshell-"] {
                assert!(!n.body.contains(jargon), "жаргон «{jargon}»: {}", n.body);
            }
        }
    }

    #[test]
    fn restore_texts_are_plain() {
        for text in [restore_title(), restore_text(), restore_informative(), restore_ok_text()] {
            for jargon in ["libplasma", "pacman", "pkexec", "fork", "форк"] {
                assert!(!text.contains(jargon), "жаргон «{jargon}»: {text}");
            }
        }
    }

    #[test]
    fn rebuild_command_paru_builds_in_chroot() {
        let pkgs = vec!["aeroshell-libplasma-git".to_string()];
        assert_eq!(
            rebuild_command("paru", &pkgs, true).unwrap(),
            vec![
                "paru",
                "-S",
                "--rebuild",
                "--chroot",
                "--skipreview",
                "--noconfirm",
                "--sudo",
                "pkexec",
                "--sudoflags",
                "",
                "aeroshell-libplasma-git",
            ]
        );
    }

    #[test]
    fn rebuild_command_without_chroot_tooling_builds_in_system() {
        // Нет devtools — просить чистое окружение нельзя, иначе сборка
        // падает, не начавшись.
        let pkgs = vec!["aeroshell-libplasma-git".to_string()];
        let argv = rebuild_command("paru", &pkgs, false).unwrap();
        assert!(!argv.contains(&"--chroot".to_string()), "{argv:?}");
    }

    #[test]
    fn rebuild_command_yay_has_no_chroot() {
        let pkgs = vec!["aeroshell-libplasma-git".to_string()];
        // Даже когда чистое окружение доступно: у yay такого флага нет.
        let argv = rebuild_command("yay", &pkgs, true).unwrap();
        assert!(!argv.contains(&"--chroot".to_string()));
        assert_eq!(argv[0], "yay");
        // У yay свой выключатель показа изменений — не paru'шный.
        assert!(argv.contains(&"--diffmenu=false".to_string()), "{argv:?}");
        assert!(!argv.contains(&"--skipreview".to_string()), "{argv:?}");
    }

    #[test]
    fn rebuild_command_refuses_unknown_helper_and_empty_set() {
        assert_eq!(rebuild_command("trizen", &["p".to_string()], true), None);
        assert_eq!(rebuild_command("paru", &[], true), None);
    }

    #[test]
    fn chroot_only_with_paru_and_devtools() {
        assert!(chroot_available("paru", &|c| c == "mkarchroot"));
        assert!(!chroot_available("paru", &|_| false));
        assert!(!chroot_available("yay", &|_| true));
    }

    #[test]
    fn check_on_start_defaults_to_enabled() {
        // Ключа ещё нет — проверяем; выключено только явным False.
        assert!(check_on_start(None));
        assert!(check_on_start(Some(true)));
        assert!(!check_on_start(Some(false)));
    }

    #[test]
    fn rebuild_argv_speaks_english() {
        let argv = rebuild_argv_c_locale(&["yay".to_string(), "-S".to_string()]);
        assert_eq!(argv, ["env", "LANG=C", "LC_ALL=C", "yay", "-S"]);
    }

    #[test]
    fn rebuild_auth_denied_reads_builder_output() {
        assert!(rebuild_auth_denied(
            "Error executing command as another user: Request dismissed"
        ));
        assert!(rebuild_auth_denied("==> error: Not authorized"));
        assert!(rebuild_auth_denied(" -> error installing repo packages: exit status 126"));
        assert!(!rebuild_auth_denied("==> ERROR: A failure occurred in build()."));
        assert!(!rebuild_auth_denied(""));
    }

    #[test]
    fn rebuild_texts_are_plain() {
        let denied = rebuild_denied_message();
        // Подпись кнопки — из каталога «Настроек» на языке интерфейса.
        let button = crate::qt_settings::text("aero.check_now", &[]).unwrap();
        let button = button.trim_start_matches(|c: char| !c.is_alphanumeric());
        assert!(denied.contains(&format!("“{button}”")), "{denied}");
        let failed = rebuild_failure_message("tail");
        assert!(failed.starts_with(&rebuild_failed_head()), "{failed}");
        assert!(failed.ends_with("Last lines of the build:\ntail"), "{failed}");
        for text in [
            denied,
            failed,
            confirm_text(),
            rebuild_busy_message(),
            survey_running_message(),
            rebuild_timeout_message(),
            rebuild_success_message(),
        ] {
            for jargon in ["pkexec", "helper", "хелпер", "polkit", "/tmp", "--", "chroot"] {
                assert!(!text.contains(jargon), "жаргон «{jargon}»: {text}");
            }
        }
    }

    #[test]
    fn pick_helper_prefers_paru() {
        assert_eq!(pick_helper(&|_| true).as_deref(), Some("paru"));
        assert_eq!(pick_helper(&|h| h == "yay").as_deref(), Some("yay"));
        assert_eq!(pick_helper(&|_| false), None);
    }

    // ── Журнал сеанса ──────────────────────────────────────────────────

    fn journal_finding(id: &str, pkg: Option<(&str, bool)>, cause: crate::plugin_journal::Cause) -> crate::plugin_journal::Finding {
        use crate::plugin_journal::*;
        Finding {
            failure: Failure { kind: PluginKind::Effect, id: id.into(), cause, paths: vec![], time_us: 1 },
            name: id.to_uppercase(),
            origin: match pkg {
                Some((name, foreign)) => Origin::Package { name: name.into(), foreign },
                None => Origin::UserDir,
            },
            file: None,
        }
    }

    fn kwin_cause(b: &str, e: &str) -> crate::plugin_journal::Cause {
        crate::plugin_journal::Cause::KwinVersion { built_for: Some(b.into()), expected: Some(e.into()) }
    }

    /// По версиям в файлах всё сходится, а журнал говорит: эффект набора
    /// не загрузился. Верим журналу — и версии берём у него.
    #[test]
    fn journal_overrides_matching_versions() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let forks: Vec<String> = ["aeroshell-kwin-components-git"].iter().map(|s| s.to_string()).collect();
        let mut report = check_with(&forks, Some("6.7.4-1"), Some("6.7.4-1"), &fixture, &|p| {
            p.ends_with("aeroglide.so").then(|| effect_blob("6.7.4"))
        });
        assert!(!report.needs_rebuild);
        apply_journal(&mut report, vec![journal_finding("aeroglide", Some(("aeroshell-kwin-components-git", true)), kwin_cause("6.7.3", "6.7.4"))]);
        assert!(report.needs_rebuild);
        assert_eq!(report.stale, ["aeroshell-kwin-components-git"]);
        let v = &report.packages[0];
        assert_eq!((v.built_for.as_deref(), v.installed.as_deref()), (Some("6.7.3"), Some("6.7.4")));
        assert!(matches!(advise(&report, None), Advice::Rebuild { .. }));
        let text = confirm_text_for(&report);
        assert!(text.contains("The “AEROGLIDE” effect is off: it was built for KWin 6.7.3, but KWin 6.7.4 is installed."), "{text}");
        assert!(text.contains("Rebuild the AUR package aeroshell-kwin-components-git."), "{text}");
        assert!(report_details(&report).contains("journal: Effect aeroglide"));
        let notice = notice_for_report(&advice_rebuild(&report), &report).unwrap();
        assert!(notice.body.contains("• The “AEROGLIDE” effect is off"), "{}", notice.body);
    }

    fn advice_rebuild(report: &Report) -> Advice {
        advise(report, None)
    }

    /// Журнал пуст (повёрнут, отладка KWin < 6.8 выключена) — прежняя
    /// сверка по версиям не теряется.
    #[test]
    fn empty_journal_keeps_version_check() {
        let forks: Vec<String> = ["aeroshell-kwin-components-git"].iter().map(|s| s.to_string()).collect();
        let mut report = check_with(&forks, Some("6.7.5-1"), Some("6.7.5-1"), &fixture, &|p| {
            p.ends_with("aeroglide.so").then(|| effect_blob("6.7.4"))
        });
        apply_journal(&mut report, Vec::new());
        assert!(report.needs_rebuild);
        assert_eq!(confirm_text_for(&report), confirm_text());
    }

    /// Сторонний эффект из AUR и виджет из магазина: набора нет вовсе.
    #[test]
    fn third_party_findings_give_broken_plugins_advice() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let mut report = Report::empty();
        apply_journal(
            &mut report,
            vec![
                journal_finding("forceblur", Some(("kwin-effects-forceblur", true)), kwin_cause("6.7.4", "6.7.5")),
                journal_finding("seventasks", None, crate::plugin_journal::Cause::LoadFailed { detail: String::new() }),
            ],
        );
        assert!(!report.needs_rebuild);
        let advice = advise(&report, None);
        assert_eq!(advice, Advice::BrokenPlugins { rebuild: vec!["kwin-effects-forceblur".into()] });
        let text = plugins_text(&report, true);
        assert!(text.contains("Rebuild the AUR package kwin-effects-forceblur.") && text.contains("from the store"), "{text}");
        let fp = notify_fingerprint(&report, &advice);
        assert!(fp.starts_with("plugins ") && fp.ends_with("journal=forceblur,seventasks"), "{fp}");
        // При входе о стороннем не повторяем; новое состояние — говорим.
        assert!(!should_notify(&advice, &fp, Some(&fp), true));
        assert!(should_notify(&advice, &fp, None, true));
        let n = notice_for_report(&advice, &report).unwrap();
        assert_eq!(n.summary, crate::plugin_journal::plugins_title());
        assert!(n.body.starts_with("• The “FORCEBLUR” effect"), "{}", n.body);
    }

    /// Набор сломан и по журналу, и сторонний модуль тоже: совет — пересборка
    /// набора, а сторонний виден в строках окна.
    #[test]
    fn set_rebuild_wins_and_lists_third_party_too() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let forks: Vec<String> = ["aeroshell-kwin-components-git"].iter().map(|s| s.to_string()).collect();
        let mut report = check_with(&forks, Some("6.7.5-1"), Some("6.7.5-1"), &fixture, &|_| None);
        apply_journal(
            &mut report,
            vec![
                journal_finding("aeroglide", Some(("aeroshell-kwin-components-git", true)), kwin_cause("6.7.4", "6.7.5")),
                journal_finding("other", Some(("kwin", false)), crate::plugin_journal::Cause::LoadFailed { detail: "x".into() }),
            ],
        );
        assert!(matches!(advise(&report, None), Advice::Rebuild { .. }));
        let lines = journal_lines(&report);
        assert_eq!(lines.len(), 2);
        assert!(lines[1].contains("Update the whole system"), "{lines:?}");
    }

    /// Риск — не поломка: одни только модули под приватный интерфейс прежнего
    /// Qt дают мягкие слова и свой отпечаток; любая твёрдая улика — прежние.
    #[test]
    fn at_risk_only_is_just_qt_private_api() {
        let pkg = |marker, built: &str, inst: &str, problems: &[&str]| PackageVerdict {
            package: "aeroshell-libplasma-git".to_string(),
            marker,
            built_for: Some(built.to_string()),
            installed: Some(inst.to_string()),
            compiled: true,
            stale: true,
            problems: problems.iter().map(|p| p.to_string()).collect(),
        };
        let report = |p: PackageVerdict| Report {
            stale: vec![p.package.clone()],
            rebuild: vec![p.package.clone()],
            needs_rebuild: true,
            packages: vec![p],
            ..Report::empty()
        };
        let qt = "libcorebindingsplugin.so: Qt 6.11 private API, installed Qt 6.12";
        let soft = report(pkg(Marker::PlasmaLibrary, "6.7.5", "6.7.5", &[qt]));
        assert!(soft.at_risk_only());
        assert_eq!(confirm_title_for(&soft), crate::de_progress::aeroshell_risk_title());
        assert_eq!(confirm_button_for(&soft), crate::de_progress::aeroshell_rebuild_button());
        assert!(confirm_text_for(&soft).contains(&crate::de_progress::aeroshell_risk_text()));
        let advice = Advice::Rebuild { packages: soft.rebuild.clone() };
        assert!(notify_fingerprint(&soft, &advice).starts_with("rebuild-risk "));
        let fp = notify_fingerprint(&soft, &advice);
        assert_eq!(notice_for_report(&advice, &soft).unwrap().summary, crate::de_progress::aeroshell_risk_title());
        assert!(!should_notify(&advice, &fp, Some(&fp), true));
        assert!(should_notify(&advice, &fp, None, true));
        assert!(report(pkg(Marker::QtVersion, "6.11", "6.12", &[qt])).at_risk_only());

        // Версия Plasma разошлась — уже поломка.
        let hard = report(pkg(Marker::PlasmaLibrary, "6.7.4", "6.7.5", &[qt]));
        assert!(!hard.at_risk_only());
        assert_eq!(confirm_title_for(&hard), crate::de_progress::aeroshell_title());
        assert_eq!(confirm_button_for(&hard), crate::de_progress::aeroshell_fix_button());
        assert!(notify_fingerprint(&hard, &advice).starts_with("rebuild "));
        // Ненайденная библиотека — тоже.
        let missing = "x.so: libprocesscore.so.10 => not found";
        assert!(!report(pkg(Marker::PlasmaLibrary, "6.7.5", "6.7.5", &[qt, missing])).at_risk_only());
        // Расхождения нет вовсе — и риска нет.
        assert!(!Report::empty().at_risk_only());
    }
}
