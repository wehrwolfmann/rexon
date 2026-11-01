//! Пакет из репозитория Arch, который не ставится, потому что ждёт
//! пересборки, — и его сборка заново из официальных исходников Arch.
//!
//! ЗАЧЕМ. В Arch библиотека выходит новой версией раньше, чем пересобраны
//! все пакеты, которые с ней связаны. На это время такой пакет в
//! репозитории требует прежнюю версию библиотеки (`libcamel-1.2.so=67-64`),
//! а в репозитории уже только новая (`=68-64`), и pacman отказывает:
//! «unable to satisfy dependency». Живой случай 19.10.2026 — Pantheon:
//! `switchboard-plug-online-accounts` 8.0.2-3 ждал `evolution-data-server`
//! 3.62 (libcamel 67 → 68, libedataserver 27 → 28). Человек может ждать
//! неделями; а официальный рецепт Arch собирается против новой библиотеки
//! за минуты на его же машине.
//!
//! Здесь два решения:
//!
//! 1. **Признак** ([`classify`], [`find_candidates`]). Пересборку
//!    предлагаем ТОЛЬКО когда КАЖДАЯ неразрешимая зависимость пакета — версия
//!    библиотеки (`libX.so=N-64`), а в репозитории есть та же библиотека
//!    другой версии с той же разрядностью. Зависимость, которой в
//!    репозитории нет вовсе, пересборкой не лечится: сборка упадёт на ней же,
//!    и предлагать кнопку было бы обманом.
//! 2. **План и исполнение** (ниже, раздел «Пересборка»).
//!
//! ⛔ `pacman -d`/`-dd`/`--nodeps` не предлагаются и не звучат НИГДЕ: пакет,
//! поставленный мимо зависимостей, ломается молча, при первом же запуске.
//!
//! Только семейство Arch и только изменяемая система: на SteamOS и других
//! неизменяемых системах поставленное поверх стирается обновлением.

use crate::system::PkgFamily;
use crate::system_plan::{valid_pkg_name, Cmd, PreviewAnswer};

// ============================================================================
// Признак «ждёт пересборки против новой библиотеки»
// ============================================================================

/// Зависимость от версии библиотеки: `libcamel-1.2.so=67-64` → библиотека
/// `libcamel-1.2.so`, версия `67`, разрядность `64`.
///
/// Такие зависимости makepkg дописывает сам, по библиотекам, с которыми
/// пакет связан при сборке, — поэтому они и «застревают» на прежней версии.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SonameDep {
    pub lib: String,
    pub version: String,
    pub bits: String,
}

/// Разобрать зависимость вида `libX.so=N-64`. Всё прочее (обычные имена,
/// ограничения `>=`, `<`) — `None`.
pub fn parse_soname_dep(dep: &str) -> Option<SonameDep> {
    let (lib, value) = dep.trim().split_once('=')?;
    if lib.is_empty()
        || !lib.ends_with(".so")
        || lib.ends_with(['<', '>'])
        || !valid_pkg_name(lib)
    {
        return None;
    }
    let (version, bits) = value.rsplit_once('-')?;
    if version.is_empty() || !matches!(bits, "32" | "64") || !valid_pkg_name(version) {
        return None;
    }
    Some(SonameDep {
        lib: lib.to_string(),
        version: version.to_string(),
        bits: bits.to_string(),
    })
}

/// Неразрешимые зависимости из вывода pacman (под LANG=C):
/// `:: unable to satisfy dependency 'ЗАВИСИМОСТЬ' required by ПАКЕТ` →
/// (зависимость, пакет). Без повторов, порядок появления.
pub fn parse_unsatisfied(output: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for line in output.lines() {
        let Some((_, tail)) = line.split_once("unable to satisfy dependency '") else {
            continue;
        };
        let Some((dep, rest)) = tail.split_once('\'') else {
            continue;
        };
        let Some((_, by)) = rest.split_once("required by ") else {
            continue;
        };
        let by = by.trim();
        if dep.is_empty() || !valid_pkg_name(by) {
            continue;
        }
        let pair = (dep.to_string(), by.to_string());
        if !out.contains(&pair) {
            out.push(pair);
        }
    }
    out
}

/// Значения поля вывода `pacman -Si` (под LANG=C) КАК ЕСТЬ, с версиями:
/// `Provides : libcamel-1.2.so=68-64  libebook-1.2.so=21-64`. «None» —
/// пусто. Строки-продолжения (с отступом, без двоеточия) тоже читаются:
/// на узком терминале pacman переносит длинные списки.
pub fn si_field(stdout: &str, field: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut inside = false;
    for line in stdout.lines() {
        if let Some(rest) = line.strip_prefix(field) {
            if let Some((gap, values)) = rest.split_once(':') {
                if gap.trim().is_empty() {
                    inside = true;
                    out.extend(values.split_whitespace().map(str::to_string));
                    continue;
                }
            }
        }
        if inside {
            // Продолжение: строка с отступом и без «Имя :» впереди.
            let continued = line.starts_with(' ')
                && line.split_once(" : ").map(|(head, _)| head.trim().is_empty()).unwrap_or(true);
            if continued {
                out.extend(line.split_whitespace().map(str::to_string));
                continue;
            }
            break;
        }
    }
    out.retain(|v| v != "None");
    out
}

/// Одна библиотека, ради которой пакет ждёт пересборки.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibGap {
    /// `libcamel-1.2.so`.
    pub lib: String,
    /// Что требует пакет из репозитория: `67`.
    pub wanted: String,
    /// Что есть в репозитории: `68`.
    pub available: String,
    /// Чей это пакет: `evolution-data-server`.
    pub provider: String,
}

/// Пакет, который можно собрать заново против новых библиотек.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub package: String,
    pub gaps: Vec<LibGap>,
}

/// Признак «ждёт пересборки»: КАЖДАЯ неразрешимая зависимость `package`
/// (`unsatisfied`) — версия библиотеки, и та же библиотека с той же
/// разрядностью в репозитории есть другой версии.
///
/// `provider_of(lib)` — кто в репозиториях даёт эту библиотеку и его поле
/// Provides целиком; `None` — никто (или спросить не вышло).
pub fn classify(
    package: &str,
    unsatisfied: &[String],
    provider_of: &dyn Fn(&str) -> Option<(String, Vec<String>)>,
) -> Option<Candidate> {
    if unsatisfied.is_empty() {
        return None;
    }
    let mut gaps = Vec::new();
    for dep in unsatisfied {
        let want = parse_soname_dep(dep)?;
        let (provider, provides) = provider_of(&want.lib)?;
        let have = provides.iter().filter_map(|p| parse_soname_dep(p)).find(|p| {
            p.lib == want.lib && p.bits == want.bits
        })?;
        if have.version == want.version {
            // Та же версия есть, а pacman всё равно отказал — дело не в
            // пересборке; угадывать не будем.
            return None;
        }
        gaps.push(LibGap {
            lib: want.lib,
            wanted: want.version,
            available: have.version,
            provider,
        });
    }
    Some(Candidate {
        package: package.to_string(),
        gaps,
    })
}

fn query(argv: Vec<String>) -> Cmd {
    Cmd {
        argv,
        timeout_secs: None,
        lang_c: true,
    }
}

/// Пробная транзакция одного имени без прав root: что поставится, а при
/// отказе — неразрешимые зависимости. Без `--needed`: стоящий поставщик
/// библиотеки тоже должен попасть в ответ.
pub fn probe_cmd(name: &str) -> Cmd {
    query(vec![
        "pacman".into(),
        "-Sp".into(),
        "--noconfirm".into(),
        "--print-format".into(),
        "%n".into(),
        name.to_string(),
    ])
}

/// `pacman -Si ИМЯ` под LANG=C.
pub fn info_cmd(name: &str) -> Cmd {
    query(vec!["pacman".into(), "-Si".into(), name.to_string()])
}

/// Кто в репозиториях даёт библиотеку `lib`: pacman сам разрешает имя
/// библиотеки в пакет (`pacman -Sp libcamel-1.2.so` → evolution-data-server
/// и, на чистой системе, его зависимости). Поставщик — тот из ответа, у кого
/// библиотека есть в Provides; ищем с конца — цель транзакции печатается
/// после своих зависимостей.
fn provider_of(lib: &str, run: &dyn Fn(&Cmd) -> PreviewAnswer) -> Option<(String, Vec<String>)> {
    let (rc, stdout, _) = run(&probe_cmd(lib))?;
    if rc != 0 {
        return None;
    }
    let names: Vec<&str> = stdout.lines().map(str::trim).filter(|l| valid_pkg_name(l)).collect();
    for name in names.iter().rev() {
        let Some((0, info, _)) = run(&info_cmd(name)) else {
            continue;
        };
        let provides = si_field(&info, "Provides");
        let has = provides
            .iter()
            .filter_map(|p| parse_soname_dep(p))
            .any(|p| p.lib == lib);
        if has {
            return Some((name.to_string(), provides));
        }
    }
    None
}

/// Какие из `packages` (обычно — то, что предпросмотр установки назвал
/// неустановимым) ждут пересборки. Пусто — предлагать нечего.
///
/// Поломка может сидеть глубже: пакет набора ставится, а его зависимость —
/// нет. Тогда кандидат — та зависимость (на неё и указывает «required by»).
/// Кнопка предлагается, только если пересборка лечит ВСЕ найденные поломки:
/// иначе после сборки установка всё равно упала бы.
pub fn find_candidates(
    family: PkgFamily,
    immutable: bool,
    packages: &[String],
    run: &dyn Fn(&Cmd) -> PreviewAnswer,
) -> Vec<Candidate> {
    if family != PkgFamily::Arch || immutable {
        return Vec::new();
    }
    let mut out: Vec<Candidate> = Vec::new();
    for pkg in packages {
        if !valid_pkg_name(pkg) {
            return Vec::new();
        }
        let Some((rc, stdout, stderr)) = run(&probe_cmd(pkg)) else {
            return Vec::new();
        };
        if rc == 0 {
            continue;
        }
        // Как у предпросмотра: «error:» — в stderr, «:: unable to satisfy» —
        // в stdout; читаем оба.
        let pairs = parse_unsatisfied(&format!("{stdout}\n{stderr}"));
        if pairs.is_empty() {
            // «target not found» и прочее: не наш случай.
            return Vec::new();
        }
        let mut owners: Vec<String> = Vec::new();
        for (_, by) in &pairs {
            if !owners.contains(by) {
                owners.push(by.clone());
            }
        }
        for owner in owners {
            if out.iter().any(|c| c.package == owner) {
                continue;
            }
            let deps: Vec<String> =
                pairs.iter().filter(|(_, by)| *by == owner).map(|(d, _)| d.clone()).collect();
            match classify(&owner, &deps, &|lib| provider_of(lib, run)) {
                Some(c) => out.push(c),
                None => return Vec::new(),
            }
        }
    }
    out
}

/// [`find_candidates`] на живой машине (команды — через `utils::run`).
pub fn find_candidates_live(packages: &[String]) -> Vec<Candidate> {
    if packages.is_empty() || crate::utils::which("pacman", None).is_none() {
        return Vec::new();
    }
    let distro = crate::system::DistroDetector::detect_live();
    let family = distro.family_live();
    let immutable = distro.is_immutable() || distro.is_steamos();
    find_candidates(family, immutable, packages, &|cmd: &Cmd| {
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

// ============================================================================
// Пересборка: рецепт Arch → сборка от пользователя → установка
// ============================================================================
//
// Порядок и почему так:
//
// * **Рецепт** — официальный репозиторий упаковки Arch
//   (`gitlab.archlinux.org/archlinux/packaging/packages/<pkgbase>`), метка
//   ровно той версии, что лежит в репозитории (`8.0.2-3`; двоеточие эпохи
//   метка пишет дефисом). Метки нет — берётся последнее состояние, и журнал
//   говорит об этом вслух. Клон неглубокий, в кеш Rexon
//   (`~/.cache/rexon/rebuild/<pkgbase>`), каждый раз заново.
// * **Проверка исходников** — дело makepkg: суммы и подписи из рецепта он
//   сверяет сам. Ключи подписи (`validpgpkeys`), которых у человека нет,
//   берутся из самого рецепта (`keys/pgp/<отпечаток>.asc`), иначе с сервера
//   ключей; не получилось — честный отказ, а не сборка без проверки.
// * **Пароль** — один на всё: оба root-шага идут через помощник пакетов
//   (`rexon-pkg-helper`, операции `build-deps` и `install-built`) под
//   действием polkit `org.rexon.pkg` (`auth_admin_keep`) — тем же, что
//   ставит остальное (Deepin); пока держится подтверждение (около пяти
//   минут), второго окна нет. Сборка дольше — перед установкой пароль
//   спросят ещё раз. Помощника нет (запуск из дерева без пакета) — прежний
//   `pkexec /bin/bash`, по паролю на шаг.
// * **Зависимости сборки** — одним вопросом пароля, `pacman -S --needed
//   --asdeps`: только то, чего нет, и с пометкой «зависимость», чтобы оно
//   не числилось поставленным руками. Проверки (`check()`) пропускаются
//   (`--nocheck`): это работа Arch, а человеку они добавили бы и время, и
//   лишние пакеты.
// * **Сборка** — от пользователя, без root (makepkg от root не работает и
//   правильно делает), в срезе `rexon-build.slice`, nice 19 и ionice idle:
//   рабочий стол не должен заметить сборку.
// * **Версия** — `pkgrel` НЕ трогаем. Собранный пакет той же версии, что в
//   репозитории: pacman считает его «тем же», `--needed` его не трогает,
//   `-Syu` не пытается ни обновить, ни откатить, и не печатает «local is
//   newer». Своя пересборка Arch всегда повышает `pkgrel` (`8.0.2-4`), и она
//   придёт обычным обновлением, заменив нашу. Метка вида `pkgrel+0.1` дала
//   бы то же самое, но ценой правки рецепта (и его сумм), вечного «local is
//   newer than extra» и отката на сломанный пакет при любом `pacman -S` без
//   `--needed`.
// * **Установка** — `pacman -U` собранного файла (без `-debug`), вторым и
//   последним root-шагом; в том же вызове снимаются сборочные пакеты,
//   которые поставили мы (как `--removemake` у помощников AUR).
//
// ⛔ Ни в одной команде здесь нет `-d`/`-dd`/`--nodeps` — сверка
// `no_command_skips_dependency_checks`.

use std::sync::Arc;

use crate::de_progress::{STAGE_ERROR, STAGE_RAW, STAGE_WARNING};
use crate::i18n::{tr, trf};
use crate::system_de_exec::{cancelled_message, CancelToken, DeOpOutcome, DeOpSink};
use crate::privilege::op::{LocalFile, PrivOp};
use crate::system_exec::RunOut;

/// Адрес официального репозитория упаковки Arch для `pkgbase`.
pub fn packaging_url(pkgbase: &str) -> String {
    format!("https://gitlab.archlinux.org/archlinux/packaging/packages/{pkgbase}.git")
}

/// Метка версии в репозитории упаковки: `1:2.0-1` → `1-2.0-1`.
pub fn version_tag(version: &str) -> String {
    version.replace(':', "-")
}

/// `pkgbase` из записи базы синхронизации (`<имя>-<версия>/desc`):
/// строка после `%BASE%`.
pub fn parse_db_base(desc: &str) -> Option<String> {
    let mut lines = desc.lines();
    while let Some(line) = lines.next() {
        if line.trim() == "%BASE%" {
            return lines.next().map(str::trim).filter(|b| valid_pkg_name(b)).map(str::to_string);
        }
    }
    None
}

/// Нужное из `.SRCINFO` рецепта.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SrcInfo {
    pub pkgbase: String,
    pub pkgnames: Vec<String>,
    pub epoch: String,
    pub pkgver: String,
    pub pkgrel: String,
    /// `depends` всех пакетов рецепта (+ под архитектуру машины).
    pub depends: Vec<String>,
    /// `makedepends` (+ под архитектуру машины).
    pub makedepends: Vec<String>,
    pub validpgpkeys: Vec<String>,
}

impl SrcInfo {
    /// Полная версия: `[эпоха:]версия-выпуск`.
    pub fn version(&self) -> String {
        if self.epoch.is_empty() {
            format!("{}-{}", self.pkgver, self.pkgrel)
        } else {
            format!("{}:{}-{}", self.epoch, self.pkgver, self.pkgrel)
        }
    }
}

/// Разобрать `.SRCINFO`. `carch` — архитектура машины (`x86_64`): поля
/// `depends_x86_64` берутся, чужие — нет.
pub fn parse_srcinfo(text: &str, carch: &str) -> Option<SrcInfo> {
    let mut info = SrcInfo::default();
    let push = |list: &mut Vec<String>, value: &str| {
        if !value.is_empty() && !list.iter().any(|v| v == value) {
            list.push(value.to_string());
        }
    };
    for line in text.lines() {
        let Some((key, value)) = line.trim().split_once(" = ") else {
            continue;
        };
        let value = value.trim();
        let key = key.trim();
        let base_key = key
            .strip_suffix(carch)
            .and_then(|k| k.strip_suffix('_'))
            .unwrap_or(key);
        match base_key {
            "pkgbase" => info.pkgbase = value.to_string(),
            "pkgname" => push(&mut info.pkgnames, value),
            "epoch" => info.epoch = value.to_string(),
            "pkgver" => info.pkgver = value.to_string(),
            "pkgrel" => info.pkgrel = value.to_string(),
            "depends" => push(&mut info.depends, value),
            "makedepends" => push(&mut info.makedepends, value),
            "validpgpkeys" => push(&mut info.validpgpkeys, value),
            _ => {}
        }
    }
    (!info.pkgbase.is_empty() && !info.pkgnames.is_empty() && !info.pkgver.is_empty())
        .then_some(info)
}

/// Строка зависимости, которую можно отдать pacman аргументом: имя и,
/// возможно, ограничение версии. Ни ведущего `-`, ни пробелов, ни путей.
fn valid_dep(dep: &str) -> bool {
    dep.chars().next().is_some_and(|c| c.is_ascii_alphanumeric())
        && dep
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "@._+:<>=-".contains(c))
}

/// Что нужно для сборки: зависимости рецепта и `base-devel` (компилятор,
/// fakeroot — makepkg без них не соберёт ничего).
pub fn build_requirements(info: &SrcInfo) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for dep in info.depends.iter().chain(&info.makedepends).map(String::as_str).chain(["base-devel"]) {
        if valid_dep(dep) && !out.iter().any(|d| d == dep) {
            out.push(dep.to_string());
        }
    }
    out
}

/// Из недостающего — то, что нужно ТОЛЬКО для сборки: его снимаем после
/// установки. Зависимости самого пакета остаются — без них он не работает.
pub fn build_only(missing: &[String], info: &SrcInfo) -> Vec<String> {
    missing
        .iter()
        .filter(|m| !info.depends.contains(m))
        .filter(|m| info.makedepends.contains(m) || m.as_str() == "base-devel")
        .cloned()
        .collect()
}

/// `pacman -T`: чего из списка нет (код 127 и имена в stdout; 0 — всё есть).
pub fn deptest_argv(deps: &[String]) -> Vec<String> {
    let mut argv = vec!["pacman".to_string(), "-T".to_string(), "--".to_string()];
    argv.extend(deps.iter().cloned());
    argv
}

/// Разбор ответа [`deptest_argv`]: `None` — ответ непонятный.
pub fn parse_deptest(rc: i32, stdout: &str) -> Option<Vec<String>> {
    match rc {
        0 => Some(Vec::new()),
        127 => Some(
            stdout
                .lines()
                .map(str::trim)
                .filter(|l| valid_dep(l))
                .map(str::to_string)
                .collect(),
        ),
        _ => None,
    }
}

/// Неглубокий клон рецепта: метка версии либо (`tag = None`) последнее
/// состояние.
pub fn clone_argv(url: &str, dir: &str, tag: Option<&str>) -> Vec<String> {
    let mut argv: Vec<String> = ["git", "clone", "--quiet", "--depth", "1"].map(String::from).to_vec();
    if let Some(tag) = tag {
        argv.extend(["--branch".to_string(), tag.to_string()]);
    }
    argv.extend(["--".to_string(), url.to_string(), dir.to_string()]);
    argv
}

/// Скрипт root-шагов: язык вывода — C (его разбирает журнал), аргументы
/// приходят позиционно, не через текст скрипта.
const ROOT_DEPS_SCRIPT: &str = "export LC_ALL=C; exec pacman -S --needed --noconfirm --asdeps -- \"$@\"";
const ROOT_INSTALL_SCRIPT: &str = "export LC_ALL=C; f=\"$1\"; shift; \
pacman -U --noconfirm -- \"$f\" || exit $?; \
if [ $# -gt 0 ]; then pacman -Rns --noconfirm -- \"$@\" || echo \"rexon: build-only packages kept\"; fi";
/// То же с заменой конфликтующего установленного (`--ask=4`, см.
/// [`RecipeEdit::replace_conflicts`]).
const ROOT_INSTALL_REPLACING_SCRIPT: &str = "export LC_ALL=C; f=\"$1\"; shift; \
pacman -U --noconfirm --ask=4 -- \"$f\" || exit $?; \
if [ $# -gt 0 ]; then pacman -Rns --noconfirm -- \"$@\" || echo \"rexon: build-only packages kept\"; fi";
/// Имя «нулевого» аргумента `bash -c` — видно в списке процессов.
const ROOT_SCRIPT_NAME: &str = "rexon-rebuild";

/// Поставить недостающее для сборки — одним вопросом пароля (без
/// помощника пакетов; с ним — [`helper_deps_argv`]).
pub fn deps_install_argv(missing: &[String]) -> Vec<String> {
    let mut argv: Vec<String> = ["pkexec", "/bin/bash", "-c", ROOT_DEPS_SCRIPT, ROOT_SCRIPT_NAME]
        .map(String::from)
        .to_vec();
    argv.extend(missing.iter().cloned());
    argv
}

/// Поставить собранный файл и снять сборочные пакеты — вторым вопросом
/// (без помощника пакетов; с ним — [`helper_install_argv`]).
/// `replace` — заменить конфликтующий установленный пакет.
pub fn install_argv(file: &str, build_only: &[String], replace: bool) -> Vec<String> {
    let script = if replace { ROOT_INSTALL_REPLACING_SCRIPT } else { ROOT_INSTALL_SCRIPT };
    let mut argv: Vec<String> = ["pkexec", "/bin/bash", "-c", script, ROOT_SCRIPT_NAME]
        .map(String::from)
        .to_vec();
    argv.push(file.to_string());
    argv.extend(build_only.iter().cloned());
    argv
}

/// Начало команды помощника пакетов на живой машине: `pkexec <путь>
/// [--lang=…]`. `None` — помощника нет или он не годится этой пересборке
/// (каталог пересборок не `~/.cache/rexon/rebuild`: помощник ищет его по
/// домашнему каталогу из passwd, `$XDG_CACHE_HOME` ему не виден).
pub fn pkg_helper_prefix_live(cache_dir: &str) -> Option<Vec<String>> {
    let home = std::env::var("HOME").unwrap_or_default();
    if home.is_empty() || cache_dir != crate::appearance_cache::cache_dir_in(None, &home) {
        return None;
    }
    if !crate::system_exec::live().pkg_helper_available() {
        return None;
    }
    Some(pkg_helper_prefix())
}

/// Начало команды помощника пакетов: `pkexec <путь> [--lang=…]` — без
/// проверки, что он есть (её делает вызывающий).
pub fn pkg_helper_prefix() -> Vec<String> {
    let path = crate::system_plan::pkg_helper_path();
    let mut argv = vec!["pkexec".to_string(), path.to_string()];
    argv.extend(crate::system_l10n::lang_args(path));
    argv
}

/// Недостающее для сборки — через помощник пакетов (`build-deps`).
/// `helper` — начало команды ([`RebuildCtx::pkg_helper`]).
pub fn helper_deps_argv(helper: &[String], missing: &[String]) -> Vec<String> {
    let mut argv = helper.to_vec();
    argv.extend(["arch".to_string(), "build-deps".to_string()]);
    argv.extend(missing.iter().cloned());
    argv
}

/// Собранный файл и снятие сборочных — через помощник пакетов
/// (`install-built`). Файл помощник примет только из каталога пересборок
/// вызвавшего ([`rebuild_root`]).
pub fn helper_install_argv(helper: &[String], file: &str, build_only: &[String], replace: bool) -> Vec<String> {
    let mut argv = helper.to_vec();
    argv.extend(
        ["arch", "install-built", if replace { "replace" } else { "plain" }, file].map(String::from),
    );
    argv.extend(build_only.iter().cloned());
    argv
}

/// Снять установленные пакеты через помощник пакетов (`remove`, тот же
/// вечный список): пакеты правки рецепта ([`RecipeEdit::remove_after`]) —
/// `install-built` их не снимает, он снимает только поставленное своим
/// `build-deps`.
pub fn helper_remove_argv(helper: &[String], packages: &[String]) -> Vec<String> {
    let mut argv = helper.to_vec();
    argv.extend(["arch".to_string(), "remove".to_string()]);
    argv.extend(packages.iter().cloned());
    argv
}

/// Имена пакетов для `install-built` из строк зависимостей: без
/// ограничения версии (`cmake>=3.20` → `cmake`), без повторов. Снимет
/// помощник только то, что сам поставил в `build-deps`.
pub fn helper_build_only_names(deps: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for dep in deps {
        let name = dep.split(['<', '>', '=']).next().unwrap_or_default();
        if !name.is_empty() && !out.iter().any(|n| n == name) {
            out.push(name.to_string());
        }
    }
    out
}

/// Каталог пересборок в кеше Rexon: рецепты, журналы и собранные файлы.
pub fn rebuild_root(cache_dir: &str) -> String {
    format!("{cache_dir}/rebuild")
}

/// Срез, в котором идут сборки (`scripts/build-queue` и пересборки).
pub const BUILD_SLICE: &str = "rexon-build.slice";

/// Сборка от пользователя. `slice` — обернуть в `systemd-run --user
/// --scope` среза [`BUILD_SLICE`] (нужен пользовательский systemd).
/// `-C` — чистая сборка даже поверх прошлой неудачной. `PKGDEST` — каталог
/// рецепта (makepkg берёт его из окружения поверх `makepkg.conf`): файл
/// ложится в каталог пересборок, откуда его примет помощник пакетов, даже
/// если у человека в `makepkg.conf` свой `PKGDEST`.
pub fn build_argv(dir: &str, slice: bool) -> Vec<String> {
    let mut argv: Vec<String> = Vec::new();
    if slice {
        argv.extend(
            ["systemd-run", "--user", "--scope", "--quiet", &format!("--slice={BUILD_SLICE}"), "--"]
                .map(String::from),
        );
    }
    argv.extend(["env".to_string(), "LC_ALL=C".to_string(), format!("PKGDEST={dir}")]);
    argv.extend(
        [
            "nice", "-n", "19", "ionice", "-c", "3", "makepkg", "-D", dir, "-f", "-C",
            "--nocheck", "--noconfirm", "--noprogressbar",
        ]
        .map(String::from),
    );
    argv
}

/// Какие файлы makepkg собрал (`PKGDEST` — тот же, что у [`build_argv`]).
pub fn packagelist_argv(dir: &str) -> Vec<String> {
    vec![
        "env".to_string(),
        format!("PKGDEST={dir}"),
        "makepkg".to_string(),
        "-D".to_string(),
        dir.to_string(),
        "--packagelist".to_string(),
    ]
}

/// Файл пакета `pkgname` из ответа [`packagelist_argv`]: имя вида
/// `<pkgname>-<версия>-<выпуск>-<архитектура>.pkg.tar.*`. Отладочные
/// (`<pkgname>-debug-…`) и соседи по рецепту не берутся.
pub fn pick_package_file(list: &str, pkgname: &str) -> Option<String> {
    list.lines().map(str::trim).find(|path| {
        let base = path.rsplit('/').next().unwrap_or(path);
        let Some(rest) = base.strip_prefix(pkgname).and_then(|r| r.strip_prefix('-')) else {
            return false;
        };
        let Some((stem, _)) = rest.split_once(".pkg.tar") else {
            return false;
        };
        stem.split('-').count() == 3
    })
    .map(str::to_string)
}

/// Почему не собралось — по журналу makepkg (LC_ALL=C).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildFailure {
    /// Суммы исходников не сошлись.
    Checksum,
    /// Подпись исходников не подтвердилась.
    Signature,
    /// Исходник не скачался.
    Download,
    /// Прочее (ошибка компиляции и т. п.).
    Other,
}

pub fn build_failure(log: &str) -> BuildFailure {
    if log.contains("did not pass the validity check") {
        BuildFailure::Checksum
    } else if log.contains("PGP signatures could not be verified") {
        BuildFailure::Signature
    } else if log.contains("Failure while downloading") || log.contains("Failure while") {
        BuildFailure::Download
    } else {
        BuildFailure::Other
    }
}

/// Сколько последних строк журнала сборки показать в отказе.
pub const FAILURE_TAIL_LINES: usize = 8;

// ── Правка рецепта: свой пакет под другим именем ────────────────────────
//
// Обычная пересборка ставит пакет Arch как есть. Правка нужна, когда сам
// рецепт Arch не годится этой машине (живой случай — Deepin рядом с KDE
// Plasma: `deepin-session` жёстко тянет `deepin-kwin`, а тот конфликтует с
// KWin; см. `crate::deepin_treeland`). Тогда пакет собирается под СВОИМ
// именем, объявляет `provides`/`conflicts` прежнего и ничего больше в
// рецепте не меняет: исходники, суммы и подписи — те же, что у Arch.
//
// ⚠️ Почему своё имя, а не то же самое. Пакет с именем Arch первое же
// обновление Arch (`-Syu`) «обновил» бы обратно на рецепт Arch — и вернул бы
// ту самую зависимость: у Deepin это вопрос pacman «удалить kwin?», а
// `pacman -Syu --noconfirm` и вовсе обрывался бы целиком. Под своим именем
// `-Syu` наш пакет не трогает (обновлять нечего: такого имени в
// репозиториях нет), а то, что требует прежнее имя, довольствуется
// `provides`. Плата — сборка не обновляется сама; об этом говорит хук
// pacman в самом пакете и карточка Rexon.

/// Что поменять в рецепте перед сборкой.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecipeEdit {
    /// Имя собранного пакета.
    pub pkgname: String,
    /// Приписка к описанию пакета (`pkgdesc`): кто и зачем собрал.
    pub pkgdesc_note: String,
    /// Зависимости, переносимые в необязательные: (имя, пояснение).
    pub to_optdepends: Vec<(String, String)>,
    /// Файлы пакета (путь без ведущего `/`), которые не ставить.
    pub drop_files: Vec<String>,
    /// Свои файлы: (путь без ведущего `/`, права `0644`/`0755`, содержимое).
    pub add_files: Vec<(String, String, String)>,
    /// Собранный пакет заменяет установленный, с которым конфликтует
    /// (`conflicts` рецепта): `pacman -U --ask=4` — ответ «да» на вопрос
    /// «удалить конфликтующий?». Только после явного согласия человека.
    pub replace_conflicts: bool,
    /// Установленные пакеты, которые снять той же root-командой после
    /// установки собранного (`pacman -Rns`, вместе со сборочными).
    pub remove_after: Vec<String>,
}

/// Граница вставленного содержимого файла в тексте рецепта.
const HEREDOC_END: &str = "REXON_EOF";

/// Слово для bash в одинарных кавычках.
fn sh_word(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

/// Путь внутри пакета: относительный, без `..`, без пробелов и кавычек.
fn valid_pkg_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.split('/').any(|s| s.is_empty() || s == "." || s == "..")
        && path.chars().all(|c| c.is_ascii_alphanumeric() || "._+-/@".contains(c))
}

/// `pkgrel` из версии репозитория, если рецепт той же версии: собранный
/// пакет обязан называть ровно ту версию, что лежит в репозитории
/// (CachyOS пересобирает пакеты Arch с `pkgrel` вида `1.1`, а рецепт Arch
/// говорит `1`, — без этого своя сборка с первой минуты числилась бы
/// устаревшей). Другая версия — `None`: тогда остаётся `pkgrel` рецепта.
pub fn matching_pkgrel(info: &SrcInfo, repo_version: &str) -> Option<String> {
    let (epoch, rest) = match repo_version.split_once(':') {
        Some((e, r)) => (e, r),
        None => ("", repo_version),
    };
    let (ver, rel) = rest.rsplit_once('-')?;
    (epoch == info.epoch
        && ver == info.pkgver
        && !rel.is_empty()
        && rel.chars().all(|c| c.is_ascii_digit() || c == '.'))
    .then(|| rel.to_string())
}

/// Текст, дописываемый в конец PKGBUILD: bash читает рецепт сверху вниз,
/// и наши присваивания перекрывают присваивания Arch, не трогая их строк.
/// `None` — рецепт такой правке не поддаётся (несколько пакетов в одном
/// рецепте, неверное имя или путь) — собирать не берёмся.
///
/// `deferred` — зависимости пакета, которые сами (через свои зависимости)
/// тянут снятое (`deepin-shell` → … → `deepin-session` → `deepin-kwin`).
/// Поставить их ДО сборки нельзя — упрёмся в тот же конфликт, а с
/// `--assume-installed` система осталась бы с незакрытой зависимостью, если
/// сборка не удастся. Для сборки они не нужны (это не библиотеки, с
/// которыми связывается пакет), поэтому проверку makepkg перед сборкой они
/// не проходят, а в сам пакет записываются: `depends` переопределяется в
/// `package()` (как у пакетов, собранных из одного рецепта по нескольку), и
/// `pacman -U` ставит их вместе с пакетом одной транзакцией, где
/// снятое уже закрыто нашим `provides`.
pub fn recipe_appendix(edit: &RecipeEdit, info: &SrcInfo, repo_version: &str, deferred: &[String]) -> Option<String> {
    let [orig] = info.pkgnames.as_slice() else {
        return None;
    };
    if !valid_pkg_name(&edit.pkgname)
        || edit.pkgname == *orig
        || !edit.to_optdepends.iter().all(|(d, _)| valid_pkg_name(d))
        || !deferred.iter().all(|d| valid_pkg_name(d))
        || !edit.drop_files.iter().all(|p| valid_pkg_path(p))
        || !edit
            .add_files
            .iter()
            .all(|(p, mode, body)| valid_pkg_path(p) && (mode == "0644" || mode == "0755") && !body.lines().any(|l| l == HEREDOC_END))
    {
        return None;
    }
    let mut s = String::new();
    s.push_str("\n# ---- Rexon: local build, see pkgdesc ----\n");
    s.push_str(&format!("pkgname={}\n", edit.pkgname));
    if let Some(rel) = matching_pkgrel(info, repo_version) {
        s.push_str(&format!("pkgrel={rel}\n"));
    }
    if !edit.pkgdesc_note.is_empty() {
        s.push_str(&format!("pkgdesc+={}\n", sh_word(&format!(" ({})", edit.pkgdesc_note))));
    }
    if !edit.to_optdepends.is_empty() {
        let names: Vec<&str> = edit.to_optdepends.iter().map(|(d, _)| d.as_str()).collect();
        // Имя и имя с ограничением версии (`deepin-kwin>=6`).
        let pattern: Vec<String> = names.iter().map(|n| format!("{n}|{n}[\\<\\>=]*")).collect();
        s.push_str("_rexon_keep=()\n");
        s.push_str("for _rexon_dep in \"${depends[@]}\"; do\n");
        s.push_str(&format!("  case \"$_rexon_dep\" in {}) ;; *) _rexon_keep+=(\"$_rexon_dep\") ;; esac\n", pattern.join("|")));
        s.push_str("done\n");
        s.push_str("depends=(\"${_rexon_keep[@]}\")\n");
        s.push_str("unset _rexon_keep _rexon_dep\n");
        for (dep, why) in &edit.to_optdepends {
            s.push_str(&format!("optdepends+=({})\n", sh_word(&format!("{dep}: {why}"))));
        }
    }
    if !deferred.is_empty() {
        s.push_str("_rexon_package_depends=(\"${depends[@]}\")\n");
        s.push_str("_rexon_keep=()\n");
        s.push_str("for _rexon_dep in \"${depends[@]}\"; do\n");
        s.push_str(&format!("  case \"$_rexon_dep\" in {}) ;; *) _rexon_keep+=(\"$_rexon_dep\") ;; esac\n", deferred.join("|")));
        s.push_str("done\n");
        s.push_str("depends=(\"${_rexon_keep[@]}\")\n");
        s.push_str("unset _rexon_keep _rexon_dep\n");
    }
    s.push_str(&format!("provides+=(\"{orig}=${{epoch:+$epoch:}}$pkgver-$pkgrel\")\n"));
    s.push_str(&format!("conflicts+=({})\n", sh_word(orig)));
    if !edit.drop_files.is_empty() || !edit.add_files.is_empty() || !deferred.is_empty() {
        // Своя package() поверх рецептной: сначала рецепт ставит всё как
        // у Arch, затем — наши снятия и добавления.
        s.push_str("eval \"_rexon_package_recipe() $(declare -f package | tail -n +2)\"\n");
        s.push_str("package() {\n");
        if !deferred.is_empty() {
            s.push_str("  depends=(\"${_rexon_package_depends[@]}\")\n");
        }
        s.push_str("  _rexon_package_recipe\n");
        for path in &edit.drop_files {
            s.push_str(&format!("  rm -f -- \"$pkgdir/{path}\"\n"));
            // Опустевший каталог тоже не нужен (`usr/share/xsessions/`).
            if let Some((parent, _)) = path.rsplit_once('/') {
                s.push_str(&format!("  rmdir --ignore-fail-on-non-empty -- \"$pkgdir/{parent}\" 2>/dev/null || true\n"));
            }
        }
        for (path, mode, body) in &edit.add_files {
            s.push_str(&format!("  install -Dm{mode} /dev/stdin \"$pkgdir/{path}\" <<'{HEREDOC_END}'\n"));
            s.push_str(body);
            if !body.ends_with('\n') {
                s.push('\n');
            }
            s.push_str(HEREDOC_END);
            s.push('\n');
        }
        s.push_str("}\n");
    }
    Some(s)
}

/// `.SRCINFO` после правки: перенесённые в необязательные зависимости
/// больше не нужны ни для сборки, ни после неё.
fn apply_edit_to_srcinfo(info: &mut SrcInfo, edit: &RecipeEdit) {
    let dropped = |dep: &String| {
        edit.to_optdepends.iter().any(|(d, _)| {
            dep == d || dep.strip_prefix(d.as_str()).is_some_and(|rest| rest.starts_with(['<', '>', '=']))
        })
    };
    info.depends.retain(|d| !dropped(d));
}

/// Отложенные зависимости ([`recipe_appendix`]): из недостающих — те, чья
/// пробная установка (`pacman -Sp`, ответ `probes`) тянет снятое правкой.
/// Не ответила проба — зависимость не откладывается (тогда честно упрёмся
/// в конфликт на вопросе пароля, а не поставим что-то мимо проверки).
pub fn deferred_depends(missing: &[String], info: &SrcInfo, edit: &RecipeEdit, probes: &dyn Fn(&str) -> Option<String>) -> Vec<String> {
    missing
        .iter()
        .filter(|m| info.depends.contains(m) && !info.makedepends.contains(m))
        .filter(|m| {
            probes(m).is_some_and(|out| out.lines().map(str::trim).any(|l| edit.to_optdepends.iter().any(|(d, _)| d == l)))
        })
        .cloned()
        .collect()
}

// ── Тексты для человека ─────────────────────────────────────────────────

/// Кнопка в предупреждении про пакет, ждущий пересборки.
pub fn offer_button() -> String {
    tr("🛠 Rebuild from Arch sources")
}

/// Пояснение под кнопкой.
pub fn offer_hint() -> String {
    tr("The official Arch recipe, built on your computer in a few minutes. \
        When Arch releases its own rebuild, it will come with the regular update.")
}

/// Кнопку нажали, а собирать уже нечего (состав системы сменился).
pub fn offer_gone() -> String {
    tr("Nothing needs rebuilding any more: the packages have changed since the card was opened.")
}

/// Заголовок окна хода и вопроса.
pub fn operation_title(packages: &[String]) -> String {
    trf("Rebuilding %1 from Arch sources", &[&packages.join(", ")])
}

/// Вопрос перед пересборкой.
pub fn confirm_text(candidates: &[Candidate]) -> String {
    let mut libs: Vec<String> = Vec::new();
    for c in candidates {
        for g in &c.gaps {
            let item = format!("{} {} → {}", g.lib, g.wanted, g.available);
            if !libs.contains(&item) {
                libs.push(item);
            }
        }
    }
    let names: Vec<&str> = candidates.iter().map(|c| c.package.as_str()).collect();
    trf(
        "%1 in the repository was built against an older version of a library \
         (%2), so it can't be installed until Arch rebuilds it.\n\n\
         Rexon can build it right now with the official Arch recipe: the \
         recipe is downloaded from gitlab.archlinux.org, the sources are \
         checked against its checksums and signatures, and the build runs on \
         your computer without administrator rights. The password is asked \
         once. If the build takes longer than a few minutes, it may be asked \
         once more before installing.\n\n\
         When Arch releases its own rebuild, it will replace this one with \
         the regular update.",
        &[&names.join(", "), &libs.join(", ")],
    )
}

fn msg_fetch(pkg: &str) -> String {
    trf("📥 Downloading the official Arch recipe for %1…", &[&pkg])
}
fn msg_fetch_head(pkg: &str, tag: &str) -> String {
    trf(
        "The recipe for version %2 of %1 wasn't found; taking its latest state.",
        &[&pkg, &tag],
    )
}
fn msg_version_differs(recipe: &str, repo: &str) -> String {
    trf(
        "The recipe is for version %1, the repository has %2.",
        &[&recipe, &repo],
    )
}
fn msg_deps(list: &[String]) -> String {
    trf("📦 Installing what the build needs: %1", &[&list.join(", ")])
}
fn msg_key(fpr: &str) -> String {
    trf("🔑 Getting the signing key %1 to check the sources…", &[&fpr])
}
fn msg_build(pkg: &str) -> String {
    trf("🔨 Building %1 — this usually takes a few minutes…", &[&pkg])
}
fn msg_install(pkg: &str) -> String {
    trf("📦 Installing the rebuilt %1…", &[&pkg])
}
fn msg_done(pkgs: &[String]) -> String {
    trf(
        // Без «✅» впереди: значок ставит окно хода (`de_progress::on_done`).
        "%1 was rebuilt from Arch sources and installed. When Arch releases \
         its own rebuild, it will come with the regular update.",
        &[&pkgs.join(", ")],
    )
}
fn err_not_arch() -> String {
    tr("Rebuilding from sources is only available on Arch Linux and its \
        derivatives with a writable system.")
}
fn err_tools(tool: &str) -> String {
    trf(
        "The program %1 is missing, so the package can't be built. Install it \
         and try again.",
        &[&tool],
    )
}
fn err_not_in_repo(pkg: &str) -> String {
    trf("%1 wasn't found in the repositories.", &[&pkg])
}
fn err_fetch(pkg: &str) -> String {
    trf(
        "Couldn't download the build recipe for %1 from gitlab.archlinux.org. \
         Check your internet connection and try again.",
        &[&pkg],
    )
}
fn err_recipe(pkg: &str) -> String {
    trf("The build recipe for %1 can't be read.", &[&pkg])
}
fn err_deptest() -> String {
    tr("Couldn't find out what the build needs: the package manager didn't answer.")
}
fn err_deps(list: &[String]) -> String {
    trf(
        "Couldn't install what the build needs (%1). Details are in the log above.",
        &[&list.join(", ")],
    )
}
fn err_key(fpr: &str) -> String {
    trf(
        "Couldn't get the signing key %1, so the sources can't be checked. \
         The build was stopped: Rexon doesn't build unchecked sources.",
        &[&fpr],
    )
}
fn err_build(pkg: &str, cause: &BuildFailure, tail: &[String], log: &str) -> String {
    let why = match cause {
        BuildFailure::Checksum => tr(
            "The downloaded sources don't match the checksums in the recipe — \
             they may have been changed on the way. Nothing was installed.",
        ),
        BuildFailure::Signature => tr(
            "The signature of the sources couldn't be confirmed. Nothing was installed.",
        ),
        BuildFailure::Download => tr(
            "A source file couldn't be downloaded. Check your internet connection \
             and try again.",
        ),
        BuildFailure::Other => tr("The build stopped with an error."),
    };
    let mut text = trf("Building %1 failed. %2", &[&pkg, &why]);
    if !tail.is_empty() {
        text.push_str("\n\n");
        text.push_str(&tail.join("\n"));
    }
    text.push_str("\n\n");
    text.push_str(&trf("Full build log: %1", &[&log]));
    text
}
fn err_no_file(pkg: &str) -> String {
    trf("The build finished, but the package file for %1 wasn't found.", &[&pkg])
}
fn err_install(pkg: &str) -> String {
    trf(
        "The rebuilt %1 couldn't be installed. Details are in the log above.",
        &[&pkg],
    )
}
fn err_lock(timeout: u64) -> String {
    crate::system_plan::pkg_lock_timeout_message(timeout)
}

// ── Исполнение ──────────────────────────────────────────────────────────

/// Походы наружу пересборки. Ни одного решения: решает [`rebuild_with`].
pub trait RebuildIo {
    /// Короткая команда с выводом в память.
    fn run(&self, argv: &[String], timeout_secs: Option<u64>, lang_c: bool) -> RunOut;
    /// Долгая команда: вывод (stdout и stderr) — в журнал `log_path`,
    /// строки на лету — в `line`.
    fn run_logged(
        &self,
        argv: &[String],
        timeout_secs: Option<u64>,
        log_path: &str,
        line: Arc<dyn Fn(&str) + Send + Sync>,
    ) -> RunOut;
    fn read_text(&self, path: &str) -> Option<String>;
    /// Дописать текст в конец файла (правка рецепта, [`RecipeEdit`]).
    fn append_text(&self, path: &str, text: &str) -> Result<(), String>;
    fn exists(&self, path: &str) -> bool;
    fn which(&self, name: &str) -> bool;
    fn make_dirs(&self, path: &str) -> Result<(), String>;
    fn remove_tree(&self, path: &str);
    /// Замки пакетного менеджера.
    fn lock_paths(&self) -> Vec<String>;
    fn sleep_secs(&self, secs: u64);
    /// Root-шаг через сеанс прав окна ([`crate::privilege::run`]): `files` —
    /// пути файлов операции в порядке [`PrivOp::local_files`], ход — в
    /// журнал `log_path` и в `line`. `None` — сеанса прав нет или помощник
    /// этот шаг не исполняет: идти прежней командой ([`RebuildIo::run_logged`]).
    /// По умолчанию `None` — исполнители без сеанса прав (тесты, стенды).
    fn run_priv(
        &self,
        op: &PrivOp,
        files: &[String],
        log_path: &str,
        line: Arc<dyn Fn(&str) + Send + Sync>,
    ) -> Option<RunOut> {
        let _ = (op, files, log_path, line);
        None
    }
}

/// Где и как собирать.
#[derive(Debug, Clone)]
pub struct RebuildCtx {
    /// Кеш Rexon (`~/.cache/rexon`).
    pub cache_dir: String,
    /// Архитектура машины (`x86_64`).
    pub carch: String,
    /// Каталог базы pacman (`/var/lib/pacman`).
    pub db_path: String,
    /// Можно ли завернуть сборку в пользовательский срез systemd.
    pub slice: bool,
    /// Начало команды помощника пакетов (`pkexec <путь> [--lang=…]`,
    /// [`pkg_helper_prefix_live`]): оба root-шага идут через него, и пароль
    /// спрашивают один раз на всю установку. `None` — прежний `pkexec
    /// /bin/bash`, по паролю на шаг.
    pub pkg_helper: Option<Vec<String>>,
}

enum Stop {
    Failed(String),
    Cancelled,
}

struct Run<'a> {
    io: &'a dyn RebuildIo,
    sink: Arc<dyn DeOpSink>,
    cancel: &'a CancelToken,
    ctx: &'a RebuildCtx,
    /// Система уже тронута — отмена больше не честна.
    touched: bool,
}

/// Таймаут клона и ключей: сеть, а не машина.
const NET_TIMEOUT_SECS: u64 = 600;
/// Таймаут установки (`pkexec bash` и `pkexec rexon-pkg-helper arch
/// build-deps|install-built` по argv в класс не попадают).
const INSTALL_TIMEOUT_SECS: u64 = 1800;

fn denied(rc: Option<i32>) -> bool {
    matches!(rc, Some(126) | Some(127))
}

impl Run<'_> {
    fn emit(&self, stage: &str, message: &str, fraction: Option<f64>) {
        self.sink.progress(stage, message, fraction);
    }

    fn checkpoint(&self) -> Result<(), Stop> {
        if !self.touched && self.cancel.is_cancelled() {
            return Err(Stop::Cancelled);
        }
        Ok(())
    }

    fn line_sink(&self) -> Arc<dyn Fn(&str) + Send + Sync> {
        let sink = self.sink.clone();
        Arc::new(move |line: &str| sink.progress(STAGE_RAW, line, None))
    }

    fn wait_lock(&self) -> Result<(), Stop> {
        let paths = self.io.lock_paths();
        let mut waited = 0u64;
        let mut told = false;
        while paths.iter().any(|p| self.io.exists(p)) {
            if waited >= crate::system_plan::PKG_LOCK_TIMEOUT_SECS {
                return Err(Stop::Failed(err_lock(crate::system_plan::PKG_LOCK_TIMEOUT_SECS)));
            }
            if !told {
                self.emit(
                    crate::system_plan::PKG_LOCK_WAIT_STAGE,
                    &crate::system_plan::pkg_lock_wait_message(),
                    None,
                );
                told = true;
            }
            self.io.sleep_secs(2);
            waited += 2;
        }
        Ok(())
    }

    fn denied_message(&self, manual: &str) -> String {
        crate::system_plan::pkexec_denied_message(manual)
    }

    /// Собрать и поставить один пакет; `edit` — собрать его под своим
    /// именем по правке рецепта ([`RecipeEdit`]).
    fn one(&mut self, pkg: &str, edit: Option<&RecipeEdit>, share: (f64, f64)) -> Result<(), Stop> {
        let at = |f: f64| Some(share.0 + (share.1 - share.0) * f);
        // Имя того, что соберётся и встанет.
        let built_name = edit.map(|e| e.pkgname.as_str()).unwrap_or(pkg);
        self.checkpoint()?;
        self.emit("rebuild", &msg_fetch(pkg), at(0.0));

        // Что лежит в репозитории: имя репозитория, версия.
        let info = self.io.run(&info_cmd(pkg).argv, None, true);
        if info.rc() != Some(0) {
            return Err(Stop::Failed(err_not_in_repo(pkg)));
        }
        let repo = si_field(&info.stdout, "Repository").into_iter().next().unwrap_or_default();
        let version = si_field(&info.stdout, "Version").into_iter().next().unwrap_or_default();
        if !valid_pkg_name(&repo) || version.is_empty() {
            return Err(Stop::Failed(err_not_in_repo(pkg)));
        }
        // pkgbase — из записи базы синхронизации; не прочли — имя пакета
        // (у большинства пакетов они совпадают).
        let desc = self.io.run(
            &[
                "tar".to_string(),
                "-xOf".to_string(),
                format!("{}/sync/{repo}.db", self.ctx.db_path),
                format!("{pkg}-{version}/desc"),
            ],
            None,
            true,
        );
        let base = (desc.rc() == Some(0))
            .then(|| parse_db_base(&desc.stdout))
            .flatten()
            .unwrap_or_else(|| pkg.to_string());

        // Рецепт.
        let root = rebuild_root(&self.ctx.cache_dir);
        let dir = format!("{root}/{base}");
        self.io.make_dirs(&root).map_err(|_| Stop::Failed(err_fetch(pkg)))?;
        self.io.remove_tree(&dir);
        let url = packaging_url(&base);
        let tag = version_tag(&version);
        let cloned = self.io.run(&clone_argv(&url, &dir, Some(&tag)), Some(NET_TIMEOUT_SECS), true);
        if cloned.rc() != Some(0) {
            self.io.remove_tree(&dir);
            let head = self.io.run(&clone_argv(&url, &dir, None), Some(NET_TIMEOUT_SECS), true);
            if head.rc() != Some(0) {
                return Err(Stop::Failed(err_fetch(pkg)));
            }
            self.emit(STAGE_WARNING, &msg_fetch_head(pkg, &tag), None);
        }
        let mut srcinfo = self
            .io
            .read_text(&format!("{dir}/.SRCINFO"))
            .and_then(|t| parse_srcinfo(&t, &self.ctx.carch))
            .filter(|s| s.pkgnames.iter().any(|n| n == pkg))
            .ok_or_else(|| Stop::Failed(err_recipe(pkg)))?;
        if srcinfo.version() != version {
            self.emit(STAGE_WARNING, &msg_version_differs(&srcinfo.version(), &version), None);
        }
        if let Some(edit) = edit {
            apply_edit_to_srcinfo(&mut srcinfo, edit);
        }
        self.checkpoint()?;

        // Что нужно для сборки и чего нет.
        let wanted = build_requirements(&srcinfo);
        let test = self.io.run(&deptest_argv(&wanted), None, true);
        let mut missing = test
            .rc()
            .and_then(|rc| parse_deptest(rc, &test.stdout))
            .ok_or_else(|| Stop::Failed(err_deptest()))?;
        if let Some(edit) = edit {
            // Правка — дописанным в конец PKGBUILD текстом: строки Arch не
            // трогаются, суммы и подписи исходников остаются его.
            let probes = |dep: &str| {
                let r = self.io.run(&probe_cmd(dep).argv, None, true);
                (r.rc() == Some(0)).then(|| r.stdout.clone())
            };
            let deferred = deferred_depends(&missing, &srcinfo, edit, &probes);
            missing.retain(|m| !deferred.contains(m));
            let appendix = recipe_appendix(edit, &srcinfo, &version, &deferred)
                .ok_or_else(|| Stop::Failed(err_recipe(pkg)))?;
            self.io
                .append_text(&format!("{dir}/PKGBUILD"), &appendix)
                .map_err(|_| Stop::Failed(err_recipe(pkg)))?;
        }
        let build_only_list = build_only(&missing, &srcinfo);
        let edit_remove: Vec<String> = edit.map(|e| e.remove_after.clone()).unwrap_or_default();
        let mut remove_after = build_only_list.clone();
        for p in &edit_remove {
            if !remove_after.contains(p) {
                remove_after.push(p.clone());
            }
        }
        let replace = edit.is_some_and(|e| e.replace_conflicts);
        if !missing.is_empty() {
            self.emit("install", &msg_deps(&missing), at(0.15));
            self.wait_lock()?;
            self.checkpoint()?;
            self.touched = true;
            self.sink.cancellable(false);
            let log = format!("{root}/{base}-deps.log");
            // ⚠️ Таймаут свой: `pkexec bash …` по argv в класс установки не
            // попадает, и по умолчанию его ждали бы 30 с — замер в контейнере
            // 20.10.2026: отказ «не поставилось» при pacman, который ещё качал.
            // Сеанс прав окна (один пароль на всё время окна); его нет —
            // прежний путь: помощник пакетов, иначе `pkexec /bin/bash`.
            let op = PrivOp::BuildDeps { names: missing.clone() };
            let r = match self.io.run_priv(&op, &[], &log, self.line_sink()) {
                Some(r) => r,
                None => {
                    let argv = match &self.ctx.pkg_helper {
                        Some(helper) => helper_deps_argv(helper, &missing),
                        None => deps_install_argv(&missing),
                    };
                    self.io.run_logged(&argv, Some(INSTALL_TIMEOUT_SECS), &log, self.line_sink())
                }
            };
            if denied(r.rc()) {
                return Err(Stop::Failed(self.denied_message(&format!(
                    "sudo pacman -S --needed --asdeps {}",
                    missing.join(" ")
                ))));
            }
            if r.rc() != Some(0) {
                return Err(Stop::Failed(err_deps(&missing)));
            }
        }
        self.checkpoint()?;

        // Ключи подписи исходников.
        for fpr in &srcinfo.validpgpkeys {
            if !fpr.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(Stop::Failed(err_key(fpr)));
            }
            let has = |io: &dyn RebuildIo| {
                io.run(&["gpg".into(), "--list-keys".into(), fpr.clone()], None, true).rc() == Some(0)
            };
            if has(self.io) {
                continue;
            }
            self.emit("rebuild", &msg_key(fpr), None);
            let local = format!("{dir}/keys/pgp/{fpr}.asc");
            let argv: Vec<String> = if self.io.exists(&local) {
                vec!["gpg".into(), "--import".into(), local]
            } else {
                vec!["gpg".into(), "--recv-keys".into(), fpr.clone()]
            };
            let _ = self.io.run(&argv, Some(NET_TIMEOUT_SECS), true);
            if !has(self.io) {
                return Err(Stop::Failed(err_key(fpr)));
            }
        }

        // Сборка.
        self.emit("rebuild", &msg_build(built_name), at(0.3));
        let log = format!("{root}/{base}.log");
        let built = self.io.run_logged(
            &build_argv(&dir, self.ctx.slice),
            Some(crate::utils::BUILD_TIMEOUT_SECS),
            &log,
            self.line_sink(),
        );
        if built.rc() != Some(0) {
            let text = self.io.read_text(&log).unwrap_or_default();
            let lines: Vec<String> =
                text.lines().map(str::trim_end).filter(|l| !l.trim().is_empty()).map(str::to_string).collect();
            let tail = &lines[lines.len().saturating_sub(FAILURE_TAIL_LINES)..];
            return Err(Stop::Failed(err_build(built_name, &build_failure(&text), tail, &log)));
        }
        let list = self.io.run(&packagelist_argv(&dir), None, true);
        let file = pick_package_file(&list.stdout, built_name)
            .filter(|f| self.io.exists(f))
            .ok_or_else(|| Stop::Failed(err_no_file(built_name)))?;
        self.checkpoint()?;

        // Установка.
        self.emit("install", &msg_install(built_name), at(0.85));
        self.wait_lock()?;
        self.touched = true;
        self.sink.cancellable(false);
        let install_log = format!("{root}/{base}-install.log");
        // Помощник примет файл только из каталога пересборок; файл вне его
        // (так не бывает: `PKGDEST` задан явно) — прежний путь, а не отказ.
        // Через помощник снятие пакетов правки — отдельным `remove` после
        // установки: `install-built` снимает только поставленное `build-deps`.
        let (argv, then_remove) = match &self.ctx.pkg_helper {
            Some(helper) if file.starts_with(&format!("{root}/")) => (
                helper_install_argv(helper, &file, &helper_build_only_names(&build_only_list), replace),
                (!edit_remove.is_empty()).then(|| helper_remove_argv(helper, &edit_remove)),
            ),
            _ => (install_argv(&file, &remove_after, replace), None),
        };
        // Сеанс прав: файл уходит дескриптором, снимаются только сборочные,
        // поставленные его же `build-deps` (как у помощника пакетов). Файл
        // вне каталога пересборок помощник не примет — такой идёт прежним
        // путём, как и выше.
        let via_session = if file.starts_with(&format!("{root}/")) {
            let op = PrivOp::InstallBuilt {
                package: LocalFile { slot: 0, label: file.clone() },
                build_only: helper_build_only_names(&build_only_list),
                replace,
            };
            self.io.run_priv(&op, std::slice::from_ref(&file), &install_log, self.line_sink())
        } else {
            None
        };
        let session_used = via_session.is_some();
        let r = match via_session {
            Some(r) => r,
            None => self.io.run_logged(&argv, Some(INSTALL_TIMEOUT_SECS), &install_log, self.line_sink()),
        };
        if denied(r.rc()) {
            return Err(Stop::Failed(self.denied_message(&format!("sudo pacman -U {file}"))));
        }
        if r.rc() != Some(0) {
            return Err(Stop::Failed(err_install(built_name)));
        }
        // Не снялось — как и у прежнего пути, не ошибка: собранное стоит.
        let remove_log = format!("{root}/{base}-remove.log");
        if session_used && !edit_remove.is_empty() {
            let op = PrivOp::PkgRemove { family: PkgFamily::Arch, packages: edit_remove.clone() };
            if self.io.run_priv(&op, &[], &remove_log, self.line_sink()).is_some() {
                return Ok(());
            }
        }
        if let Some(argv) = then_remove {
            let _ = self.io.run_logged(&argv, Some(INSTALL_TIMEOUT_SECS), &remove_log, self.line_sink());
        }
        Ok(())
    }
}

/// Собрать заново и поставить `packages` (по одному, по порядку).
pub fn rebuild_with(
    io: &dyn RebuildIo,
    sink: Arc<dyn DeOpSink>,
    cancel: &CancelToken,
    ctx: &RebuildCtx,
    packages: &[String],
) -> DeOpOutcome {
    let jobs: Vec<(String, Option<RecipeEdit>)> = packages.iter().map(|p| (p.clone(), None)).collect();
    rebuild_jobs(io, sink, cancel, ctx, &jobs, msg_done(packages))
}

/// Собрать `pkg` по рецепту Arch с правкой `edit` и поставить под именем
/// `edit.pkgname`. `done` — итог для человека.
pub fn rebuild_edited_with(
    io: &dyn RebuildIo,
    sink: Arc<dyn DeOpSink>,
    cancel: &CancelToken,
    ctx: &RebuildCtx,
    pkg: &str,
    edit: &RecipeEdit,
    done: String,
) -> DeOpOutcome {
    rebuild_jobs(io, sink, cancel, ctx, &[(pkg.to_string(), Some(edit.clone()))], done)
}

fn rebuild_jobs(
    io: &dyn RebuildIo,
    sink: Arc<dyn DeOpSink>,
    cancel: &CancelToken,
    ctx: &RebuildCtx,
    jobs: &[(String, Option<RecipeEdit>)],
    done: String,
) -> DeOpOutcome {
    let packages: Vec<String> = jobs.iter().map(|(p, _)| p.clone()).collect();
    let mut run = Run {
        io,
        sink: sink.clone(),
        cancel,
        ctx,
        touched: false,
    };
    sink.cancellable(true);
    let result = (|| -> Result<(), Stop> {
        if packages.is_empty() || !packages.iter().all(|p| valid_pkg_name(p)) {
            return Err(Stop::Failed(err_recipe(&packages.join(", "))));
        }
        for tool in ["git", "makepkg", "pacman", "pkexec"] {
            if !io.which(tool) {
                return Err(Stop::Failed(err_tools(tool)));
            }
        }
        let n = jobs.len() as f64;
        for (i, (pkg, edit)) in jobs.iter().enumerate() {
            run.one(pkg, edit.as_ref(), (i as f64 / n, (i as f64 + 1.0) / n))?;
        }
        Ok(())
    })();
    sink.cancellable(false);
    match result {
        Ok(()) => {
            // Итог в журнал пишет окно хода по `DeOpOutcome` — второй строкой
            // «done» здесь он бы задвоился.
            DeOpOutcome {
                ok: true,
                message: done,
                cancelled: false,
                restart_needed: false,
            }
        }
        Err(Stop::Cancelled) => DeOpOutcome {
            ok: false,
            message: cancelled_message(),
            cancelled: true,
            restart_needed: false,
        },
        Err(Stop::Failed(message)) => {
            sink.progress(STAGE_ERROR, &message, None);
            DeOpOutcome {
                ok: false,
                message,
                cancelled: false,
                restart_needed: false,
            }
        }
    }
}

/// Исполнитель на живой машине.
pub struct LiveRebuildIo {
    io: crate::system_de_exec::LiveDeOpsIo,
}

impl LiveRebuildIo {
    pub fn new() -> Self {
        LiveRebuildIo {
            io: crate::system_de_exec::LiveDeOpsIo::new(),
        }
    }
}

impl Default for LiveRebuildIo {
    fn default() -> Self {
        Self::new()
    }
}

impl RebuildIo for LiveRebuildIo {
    fn run(&self, argv: &[String], timeout_secs: Option<u64>, lang_c: bool) -> RunOut {
        crate::system_exec::SystemIo::run(&self.io, argv, timeout_secs, lang_c)
    }

    fn run_logged(
        &self,
        argv: &[String],
        timeout_secs: Option<u64>,
        log_path: &str,
        line: Arc<dyn Fn(&str) + Send + Sync>,
    ) -> RunOut {
        use crate::system_de_exec::DeOpsIo;
        let _ = std::fs::write(log_path, b"");
        let mut tail = crate::system_plan::tail_log(
            log_path,
            crate::system_plan::LogTailKind::Raw,
            move |_stage: &str, message: &str| line(message),
        );
        let out = self.io.run_to_file(argv, timeout_secs, log_path);
        tail.stop();
        out
    }

    fn read_text(&self, path: &str) -> Option<String> {
        std::fs::read_to_string(path).ok()
    }

    fn append_text(&self, path: &str, text: &str) -> Result<(), String> {
        use std::io::Write;
        std::fs::OpenOptions::new()
            .append(true)
            .open(path)
            .and_then(|mut f| f.write_all(text.as_bytes()))
            .map_err(|e| e.to_string())
    }

    fn exists(&self, path: &str) -> bool {
        std::path::Path::new(path).exists()
    }

    fn which(&self, name: &str) -> bool {
        crate::utils::which(name, None).is_some()
    }

    fn make_dirs(&self, path: &str) -> Result<(), String> {
        std::fs::create_dir_all(path).map_err(|e| e.to_string())
    }

    fn remove_tree(&self, path: &str) {
        let _ = std::fs::remove_dir_all(path);
    }

    fn lock_paths(&self) -> Vec<String> {
        crate::system_plan::lock_paths_live(PkgFamily::Arch)
    }

    fn sleep_secs(&self, secs: u64) {
        std::thread::sleep(std::time::Duration::from_secs(secs));
    }

    fn run_priv(
        &self,
        op: &PrivOp,
        files: &[String],
        log_path: &str,
        line: Arc<dyn Fn(&str) + Send + Sync>,
    ) -> Option<RunOut> {
        // Журнал шага начинается заново, как у `run_logged`.
        let _ = std::fs::write(log_path, b"");
        crate::privilege::run::run_live(op, files, Some(log_path), &mut |l: &str| line(l))
    }
}

/// Где собирать на живой машине. `Err` — текст отказа: не Arch или
/// неизменяемая система (поставленное поверх сотрёт обновление).
pub fn rebuild_ctx_live() -> Result<RebuildCtx, String> {
    let distro = crate::system::DistroDetector::detect_live();
    if distro.family_live() != PkgFamily::Arch
        || distro.is_immutable()
        || distro.is_steamos()
        || crate::distro_compat::SteamOsBlock::Desktops.live().is_some()
    {
        return Err(err_not_arch());
    }
    let home = std::env::var("HOME").unwrap_or_default();
    let db_path = {
        let out = crate::system_exec::SystemIo::run(
            &crate::system_de_exec::LiveDeOpsIo::new(),
            &["pacman-conf".to_string(), "DBPath".to_string()],
            None,
            true,
        );
        let path = out.stdout.trim().trim_end_matches('/').to_string();
        if out.rc() == Some(0) && path.starts_with('/') {
            path
        } else {
            "/var/lib/pacman".to_string()
        }
    };
    let cache_dir = crate::appearance_cache::cache_dir(&home);
    Ok(RebuildCtx {
        pkg_helper: pkg_helper_prefix_live(&cache_dir),
        cache_dir,
        carch: std::env::consts::ARCH.to_string(),
        db_path,
        slice: crate::utils::which("systemd-run", None).is_some()
            && std::env::var("DBUS_SESSION_BUS_ADDRESS").is_ok_and(|v| !v.is_empty()),
    })
}

/// Отказ до первого побочного действия — в журнал окна и итогом.
pub fn refuse(sink: &Arc<dyn DeOpSink>, message: String) -> DeOpOutcome {
    sink.progress(STAGE_ERROR, &message, None);
    DeOpOutcome {
        ok: false,
        message,
        cancelled: false,
        restart_needed: false,
    }
}

/// Пересборка на живой машине: отказ на не-Arch и неизменяемых системах
/// — до первого побочного действия.
pub fn rebuild_live(sink: Arc<dyn DeOpSink>, cancel: CancelToken, packages: &[String]) -> DeOpOutcome {
    match rebuild_ctx_live() {
        Ok(ctx) => rebuild_with(&LiveRebuildIo::new(), sink, &cancel, &ctx, packages),
        Err(message) => refuse(&sink, message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Записанные ответы pacman, 19.10.2026 (CachyOS, extra) ───────────

    /// `LC_ALL=C pacman -Sp switchboard-plug-online-accounts` (stdout+stderr).
    const PROBE_SWITCHBOARD: &str = "error: failed to prepare transaction (could not satisfy dependencies)\n\
:: unable to satisfy dependency 'libcamel-1.2.so=67-64' required by switchboard-plug-online-accounts\n\
:: unable to satisfy dependency 'libedataserver-1.2.so=27-64' required by switchboard-plug-online-accounts\n";

    /// `LC_ALL=C pacman -Si evolution-data-server` (начало).
    const SI_EDS: &str = "Repository      : extra\n\
Name            : evolution-data-server\n\
Version         : 3.62.1-1\n\
Description     : Unified contacts, tasks and calendar backend\n\
Architecture    : x86_64\n\
Provides        : libcamel-1.2.so=68-64  libebackend-1.2.so=11-64  libebook-1.2.so=21-64  libebook-contacts-1.2.so=5-64  libecal-2.0.so=3-64  libedata-book-1.2.so=27-64  libedata-cal-2.0.so=2-64  libedataserver-1.2.so=28-64  libedataserverui-1.2.so=4-64\n\
Depends On      : cairo  dconf  glib2  glibc  gtk3\n\
Optional Deps   : None\n\
Conflicts With  : None\n";

    fn answers(table: Vec<(String, (i32, &'static str, &'static str))>) -> impl Fn(&Cmd) -> PreviewAnswer {
        move |cmd: &Cmd| {
            let key = cmd.argv.join(" ");
            table
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, (rc, out, err))| (*rc, out.to_string(), err.to_string()))
        }
    }

    const PROBE: &str = "pacman -Sp --noconfirm --print-format %n";

    fn probe(name: &str) -> String {
        format!("{PROBE} {name}")
    }

    #[test]
    fn soname_dep_is_recognised_and_plain_names_are_not() {
        assert_eq!(
            parse_soname_dep("libcamel-1.2.so=67-64"),
            Some(SonameDep {
                lib: "libcamel-1.2.so".into(),
                version: "67".into(),
                bits: "64".into()
            })
        );
        assert_eq!(parse_soname_dep("libfoo.so=1.2-32").map(|d| d.version), Some("1.2".into()));
        for plain in [
            "evolution-data-server",
            "glib2>=2.80",
            "libcamel-1.2.so",
            "libcamel-1.2.so>=67-64",
            "python=3.13",
            "libfoo.so=1-128",
            "libfoo.so=-64",
        ] {
            assert_eq!(parse_soname_dep(plain), None, "{plain}");
        }
    }

    #[test]
    fn unsatisfied_lines_are_read_from_recorded_output() {
        assert_eq!(
            parse_unsatisfied(PROBE_SWITCHBOARD),
            vec![
                ("libcamel-1.2.so=67-64".to_string(), "switchboard-plug-online-accounts".to_string()),
                ("libedataserver-1.2.so=27-64".to_string(), "switchboard-plug-online-accounts".to_string()),
            ]
        );
        assert!(parse_unsatisfied("error: target not found: nosuchpkg\n").is_empty());
    }

    #[test]
    fn si_field_keeps_versions_and_reads_wrapped_lines() {
        let provides = si_field(SI_EDS, "Provides");
        assert_eq!(provides.len(), 9);
        assert_eq!(provides[0], "libcamel-1.2.so=68-64");
        assert!(si_field(SI_EDS, "Optional Deps").is_empty());
        let wrapped = "Provides        : liba.so=1-64\n                  libb.so=2-64\nDepends On      : glibc\n";
        assert_eq!(si_field(wrapped, "Provides"), vec!["liba.so=1-64", "libb.so=2-64"]);
    }

    #[test]
    fn pantheon_plug_waits_for_rebuild_against_new_eds() {
        let run = answers(vec![
            (probe("switchboard-plug-online-accounts"), (1, "", PROBE_SWITCHBOARD)),
            (probe("libcamel-1.2.so"), (0, "evolution-data-server\n", "")),
            (probe("libedataserver-1.2.so"), (0, "evolution-data-server\n", "")),
            ("pacman -Si evolution-data-server".to_string(), (0, SI_EDS, "")),
        ]);
        let found = find_candidates(
            PkgFamily::Arch,
            false,
            &["switchboard-plug-online-accounts".to_string()],
            &run,
        );
        assert_eq!(found.len(), 1);
        let c = &found[0];
        assert_eq!(c.package, "switchboard-plug-online-accounts");
        assert_eq!(
            c.gaps,
            vec![
                LibGap {
                    lib: "libcamel-1.2.so".into(),
                    wanted: "67".into(),
                    available: "68".into(),
                    provider: "evolution-data-server".into(),
                },
                LibGap {
                    lib: "libedataserver-1.2.so".into(),
                    wanted: "27".into(),
                    available: "28".into(),
                    provider: "evolution-data-server".into(),
                },
            ]
        );
    }

    #[test]
    fn missing_dependency_is_not_offered_a_rebuild() {
        // Библиотеки нет в репозитории вовсе: пересборка упала бы на ней же.
        let out = ":: unable to satisfy dependency 'libgone.so=3-64' required by foo\n";
        let run = answers(vec![
            ("pacman -Sp --noconfirm --print-format %n foo".to_string(), (1, out, "")),
            ("pacman -Sp --noconfirm --print-format %n libgone.so".to_string(), (1, "", "error: target not found: libgone.so\n")),
        ]);
        assert!(find_candidates(PkgFamily::Arch, false, &["foo".to_string()], &run).is_empty());

        // Обычная (не библиотечная) зависимость, которой нет.
        let out = ":: unable to satisfy dependency 'python-gone' required by foo\n";
        let run = answers(vec![("pacman -Sp --noconfirm --print-format %n foo".to_string(), (1, out, ""))]);
        assert!(find_candidates(PkgFamily::Arch, false, &["foo".to_string()], &run).is_empty());

        // Одна зависимость лечится, другая нет — кнопки нет.
        let out = ":: unable to satisfy dependency 'libcamel-1.2.so=67-64' required by foo\n\
                   :: unable to satisfy dependency 'python-gone' required by foo\n";
        let run = answers(vec![
            ("pacman -Sp --noconfirm --print-format %n foo".to_string(), (1, out, "")),
            (probe("libcamel-1.2.so"), (0, "evolution-data-server\n", "")),
            ("pacman -Si evolution-data-server".to_string(), (0, SI_EDS, "")),
        ]);
        assert!(find_candidates(PkgFamily::Arch, false, &["foo".to_string()], &run).is_empty());
    }

    #[test]
    fn same_version_or_other_bits_is_not_a_rebuild_case() {
        let provider = |_: &str| Some(("eds".to_string(), vec!["libcamel-1.2.so=68-64".to_string()]));
        assert_eq!(classify("foo", &["libcamel-1.2.so=68-64".to_string()], &provider), None);
        assert_eq!(classify("foo", &["libcamel-1.2.so=67-32".to_string()], &provider), None);
        assert!(classify("foo", &["libcamel-1.2.so=67-64".to_string()], &provider).is_some());
        assert_eq!(classify("foo", &[], &provider), None);
    }

    #[test]
    fn deeper_breakage_names_the_dependency_as_candidate() {
        let out = ":: unable to satisfy dependency 'libcamel-1.2.so=67-64' required by plug\n";
        let run = answers(vec![
            ("pacman -Sp --noconfirm --print-format %n pantheon-settings".to_string(), (1, out, "")),
            (probe("libcamel-1.2.so"), (0, "nss\nevolution-data-server\n", "")),
            ("pacman -Si nss".to_string(), (0, "Name            : nss\nProvides        : None\n", "")),
            ("pacman -Si evolution-data-server".to_string(), (0, SI_EDS, "")),
        ]);
        let found = find_candidates(PkgFamily::Arch, false, &["pantheon-settings".to_string()], &run);
        assert_eq!(found.iter().map(|c| c.package.as_str()).collect::<Vec<_>>(), vec!["plug"]);
    }

    #[test]
    fn other_families_and_immutable_systems_get_nothing() {
        let run = |_: &Cmd| -> PreviewAnswer { panic!("спрашивать не должны") };
        let pkgs = vec!["switchboard-plug-online-accounts".to_string()];
        assert!(find_candidates(PkgFamily::Debian, false, &pkgs, &run).is_empty());
        assert!(find_candidates(PkgFamily::Arch, true, &pkgs, &run).is_empty());
    }

    #[test]
    fn installable_package_is_not_a_candidate() {
        let run = answers(vec![("pacman -Sp --noconfirm --print-format %n foo".to_string(), (0, "foo\n", ""))]);
        assert!(find_candidates(PkgFamily::Arch, false, &["foo".to_string()], &run).is_empty());
    }
}

#[cfg(test)]
mod rebuild_tests {
    use super::*;
    use crate::utils::RunOutcome;
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// `.SRCINFO` из gitlab.archlinux.org, метка 8.0.2-3 (снято 19.10.2026).
    const SRCINFO: &str = "pkgbase = switchboard-plug-online-accounts\n\
\tpkgdesc = Switchboard Online Accounts Plug\n\
\tpkgver = 8.0.2\n\
\tpkgrel = 3\n\
\turl = https://github.com/elementary/switchboard-plug-onlineaccounts\n\
\tarch = x86_64\n\
\tgroups = pantheon\n\
\tlicense = GPL-3.0-only\n\
\tmakedepends = git\n\
\tmakedepends = meson\n\
\tmakedepends = vala\n\
\tdepends = glib2\n\
\tdepends = glibc\n\
\tdepends = gtk4\n\
\tdepends = hicolor-icon-theme\n\
\tdepends = libadwaita-1.so\n\
\tdepends = libcamel-1.2.so\n\
\tdepends = libedataserver-1.2.so\n\
\tdepends = libgee\n\
\tdepends = libgranite-7.so\n\
\tdepends = libswitchboard-3.so\n\
\tsource = switchboard-plug-online-accounts::git+https://github.com/elementary/switchboard-plug-onlineaccounts.git#tag=0a0c0dfdf7ed56894b48cade54c19aaea8fccef9\n\
\tb2sums = 99e52de2cc7a0cee2210c776cfdc5cebffcb4620b7aadc6186ed9d6e26a4cd70d566e667af3f7ae1a94f3427b43ceb1f59c373e4b698d2f345df9844a5486d88\n\
\n\
pkgname = switchboard-plug-online-accounts\n";

    /// `makepkg --packagelist` в контейнере archlinux:latest (OPTIONS с debug).
    const PACKAGELIST: &str = "/home/u/.cache/rexon/rebuild/switchboard-plug-online-accounts/switchboard-plug-online-accounts-8.0.2-3-x86_64.pkg.tar.zst\n\
/home/u/.cache/rexon/rebuild/switchboard-plug-online-accounts/switchboard-plug-online-accounts-debug-8.0.2-3-x86_64.pkg.tar.zst\n";

    const PKG: &str = "switchboard-plug-online-accounts";
    const DIR: &str = "/home/u/.cache/rexon/rebuild/switchboard-plug-online-accounts";

    fn out(rc: i32, stdout: &str) -> RunOut {
        RunOut {
            outcome: if rc == 0 { RunOutcome::Ok } else { RunOutcome::Failed },
            returncode: Some(rc),
            stdout: stdout.to_string(),
            stderr: String::new(),
        }
    }

    #[derive(Default)]
    struct Fake {
        /// (начало командной строки, ответ) — берётся первый подходящий.
        answers: Vec<(String, RunOut)>,
        files: HashMap<String, String>,
        calls: RefCell<Vec<Vec<String>>>,
        /// Дописанное в файлы: (путь, текст).
        appended: RefCell<Vec<(String, String)>>,
        /// Таймауты долгих команд: (командная строка, таймаут).
        timeouts: RefCell<Vec<(String, Option<u64>)>>,
        /// Сеанс прав окна есть: root-шаги уходят в `run_priv`.
        session: bool,
        /// Операции сеанса прав: (операция, файлы).
        priv_ops: RefCell<Vec<(PrivOp, Vec<String>)>>,
    }

    impl Fake {
        fn answer(&mut self, prefix: &str, rc: i32, stdout: &str) {
            self.answers.push((prefix.to_string(), out(rc, stdout)));
        }
        fn reply(&self, argv: &[String]) -> RunOut {
            self.calls.borrow_mut().push(argv.to_vec());
            let line = argv.join(" ");
            self.answers
                .iter()
                .find(|(p, _)| line.starts_with(p.as_str()))
                .map(|(_, r)| r.clone())
                .unwrap_or_else(|| out(0, ""))
        }
        fn calls(&self) -> Vec<String> {
            self.calls.borrow().iter().map(|c| c.join(" ")).collect()
        }
    }

    impl RebuildIo for Fake {
        fn run(&self, argv: &[String], _: Option<u64>, _: bool) -> RunOut {
            self.reply(argv)
        }
        fn run_logged(&self, argv: &[String], timeout: Option<u64>, _: &str, line: Arc<dyn Fn(&str) + Send + Sync>) -> RunOut {
            self.timeouts.borrow_mut().push((argv.join(" "), timeout));
            let r = self.reply(argv);
            line("строка журнала");
            r
        }
        fn read_text(&self, path: &str) -> Option<String> {
            self.files.get(path).cloned()
        }
        fn append_text(&self, path: &str, text: &str) -> Result<(), String> {
            if !self.files.contains_key(path) {
                return Err("нет файла".into());
            }
            self.appended.borrow_mut().push((path.to_string(), text.to_string()));
            Ok(())
        }
        fn exists(&self, path: &str) -> bool {
            self.files.contains_key(path)
        }
        fn which(&self, _: &str) -> bool {
            true
        }
        fn make_dirs(&self, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn remove_tree(&self, _: &str) {}
        fn lock_paths(&self) -> Vec<String> {
            vec!["/var/lib/pacman/db.lck".to_string()]
        }
        fn sleep_secs(&self, _: u64) {}
        fn run_priv(&self, op: &PrivOp, files: &[String], _: &str, line: Arc<dyn Fn(&str) + Send + Sync>) -> Option<RunOut> {
            if !self.session {
                return None;
            }
            self.priv_ops.borrow_mut().push((op.clone(), files.to_vec()));
            line("строка помощника");
            Some(out(0, ""))
        }
    }

    #[derive(Default)]
    struct Sink(Mutex<Vec<(String, String)>>);

    impl DeOpSink for Sink {
        fn progress(&self, stage: &str, message: &str, _: Option<f64>) {
            self.0.lock().unwrap().push((stage.to_string(), message.to_string()));
        }
        fn cancellable(&self, _: bool) {}
    }

    fn ctx() -> RebuildCtx {
        RebuildCtx {
            cache_dir: "/home/u/.cache/rexon".to_string(),
            carch: "x86_64".to_string(),
            db_path: "/var/lib/pacman".to_string(),
            slice: true,
            pkg_helper: None,
        }
    }

    /// Помощник пакетов установлен (пакет Rexon): тот же стенд.
    const HELPER: &str = "/usr/lib/rexon-pkg-helper";

    fn helper_ctx() -> RebuildCtx {
        RebuildCtx {
            pkg_helper: Some(vec!["pkexec".to_string(), HELPER.to_string()]),
            ..ctx()
        }
    }

    /// Живой случай: в репозитории 8.0.2-3, рецепт той же метки, на машине
    /// нет granite и switchboard (зависимости самого пакета).
    fn pantheon_fake() -> Fake {
        let mut f = Fake::default();
        f.answer(
            &format!("pacman -Si {PKG}"),
            0,
            "Repository      : extra\nName            : switchboard-plug-online-accounts\nVersion         : 8.0.2-3\n",
        );
        f.answer("tar -xOf", 0, "%FILENAME%\nx.pkg.tar.zst\n\n%NAME%\nswitchboard-plug-online-accounts\n\n%BASE%\nswitchboard-plug-online-accounts\n");
        f.answer("pacman -T", 127, "libgranite-7.so\nlibswitchboard-3.so\n");
        f.answer("env PKGDEST=", 0, PACKAGELIST);
        f.files.insert(format!("{DIR}/.SRCINFO"), SRCINFO.to_string());
        f.files.insert(
            format!("{DIR}/switchboard-plug-online-accounts-8.0.2-3-x86_64.pkg.tar.zst"),
            String::new(),
        );
        f
    }

    fn rebuild(f: &Fake, cancel: &CancelToken) -> (DeOpOutcome, Vec<(String, String)>) {
        let sink = Arc::new(Sink::default());
        let outcome = rebuild_with(f, sink.clone(), cancel, &ctx(), &[PKG.to_string()]);
        let log = sink.0.lock().unwrap().clone();
        (outcome, log)
    }

    #[test]
    fn srcinfo_of_the_live_recipe_is_read() {
        let info = parse_srcinfo(SRCINFO, "x86_64").unwrap();
        assert_eq!(info.pkgbase, PKG);
        assert_eq!(info.pkgnames, vec![PKG]);
        assert_eq!(info.version(), "8.0.2-3");
        assert_eq!(info.makedepends, vec!["git", "meson", "vala"]);
        assert_eq!(info.depends.len(), 10);
        assert!(info.validpgpkeys.is_empty());
        let req = build_requirements(&info);
        assert_eq!(req.last().map(String::as_str), Some("base-devel"));
        assert_eq!(req.len(), 14);
    }

    #[test]
    fn srcinfo_takes_own_arch_and_epoch_and_split_packages() {
        let text = "pkgbase = foo\n\tpkgver = 2.0\n\tpkgrel = 1\n\tepoch = 1\n\
                    \tmakedepends_x86_64 = nasm\n\tmakedepends_aarch64 = armthing\n\
                    \tvalidpgpkeys = ABCDEF0123456789ABCDEF0123456789ABCDEF01\n\n\
                    pkgname = foo\n\tdepends = bar\n\npkgname = foo-docs\n";
        let info = parse_srcinfo(text, "x86_64").unwrap();
        assert_eq!(info.version(), "1:2.0-1");
        assert_eq!(version_tag(&info.version()), "1-2.0-1");
        assert_eq!(info.makedepends, vec!["nasm"]);
        assert_eq!(info.pkgnames, vec!["foo", "foo-docs"]);
        assert_eq!(info.depends, vec!["bar"]);
        assert_eq!(info.validpgpkeys.len(), 1);
        assert_eq!(parse_srcinfo("nonsense", "x86_64"), None);
    }

    #[test]
    fn pkgbase_comes_from_sync_db_entry() {
        assert_eq!(parse_db_base("%NAME%\nfoo-docs\n\n%BASE%\nfoo\n\n").as_deref(), Some("foo"));
        assert_eq!(parse_db_base("%NAME%\nfoo\n"), None);
        assert_eq!(
            packaging_url("switchboard-plug-online-accounts"),
            "https://gitlab.archlinux.org/archlinux/packaging/packages/switchboard-plug-online-accounts.git"
        );
    }

    #[test]
    fn package_file_skips_debug_and_siblings() {
        assert_eq!(
            pick_package_file(PACKAGELIST, PKG).as_deref(),
            Some(format!("{DIR}/switchboard-plug-online-accounts-8.0.2-3-x86_64.pkg.tar.zst").as_str())
        );
        assert_eq!(pick_package_file(PACKAGELIST, "switchboard-plug"), None);
        assert_eq!(pick_package_file("/x/foo-docs-1-1-any.pkg.tar.zst\n", "foo"), None);
        assert!(pick_package_file("/x/foo-1:2-1-any.pkg.tar.zst\n", "foo").is_some());
    }

    #[test]
    fn build_runs_as_user_in_build_slice_at_low_priority() {
        let argv = build_argv(DIR, true);
        let line = argv.join(" ");
        assert!(line.starts_with("systemd-run --user --scope --quiet --slice=rexon-build.slice -- "));
        assert!(line.contains(&format!("env LC_ALL=C PKGDEST={DIR} nice -n 19 ionice -c 3 makepkg -D")), "{line}");
        assert_eq!(packagelist_argv(DIR).join(" "), format!("env PKGDEST={DIR} makepkg -D {DIR} --packagelist"));
        assert!(!argv.iter().any(|a| a == "pkexec" || a == "sudo" || a == "-s" || a == "--syncdeps"));
        assert!(!build_argv(DIR, false).iter().any(|a| a == "systemd-run"));
    }

    /// Замена конфликтующего — только `--ask=4` у `pacman -U` собранного
    /// файла, снимаемое — тем же скриптом после установки.
    #[test]
    fn replacing_install_answers_only_the_conflict_question() {
        let plain = install_argv("/x/f.pkg.tar.zst", &["deepin-kwin".to_string()], false);
        let replacing = install_argv("/x/f.pkg.tar.zst", &["deepin-kwin".to_string()], true);
        assert!(!plain[3].contains("--ask"));
        assert!(replacing[3].contains("pacman -U --noconfirm --ask=4 -- \"$f\""), "{}", replacing[3]);
        assert_eq!(&replacing[5..], &["/x/f.pkg.tar.zst".to_string(), "deepin-kwin".to_string()]);
    }

    #[test]
    fn no_command_skips_dependency_checks() {
        let all = [
            build_argv(DIR, true),
            deps_install_argv(&["vala".to_string()]),
            install_argv("/x/f.pkg.tar.zst", &["vala".to_string()], false),
            install_argv("/x/f.pkg.tar.zst", &[], true),
            helper_deps_argv(&["pkexec".to_string(), HELPER.to_string()], &["vala".to_string()]),
            helper_install_argv(&["pkexec".to_string(), HELPER.to_string()], "/x/f.pkg.tar.zst", &["vala".to_string()], true),
            deptest_argv(&["vala".to_string()]),
            clone_argv("u", "d", Some("t")),
            packagelist_argv(DIR),
        ];
        for argv in &all {
            for a in argv {
                for bad in ["-d", "-dd", "--nodeps", "-Udd", "-Sdd"] {
                    assert!(
                        !a.split_whitespace().any(|w| w == bad),
                        "{bad} в {argv:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn pantheon_plug_is_cloned_built_and_installed() {
        let f = pantheon_fake();
        let (outcome, log) = rebuild(&f, &CancelToken::new());
        assert!(outcome.ok, "{outcome:?}");
        let calls = f.calls();
        let pos = |prefix: &str| {
            calls.iter().position(|c| c.starts_with(prefix)).unwrap_or_else(|| panic!("нет {prefix}: {calls:#?}"))
        };
        assert!(calls.contains(&format!(
            "git clone --quiet --depth 1 --branch 8.0.2-3 -- {} {DIR}",
            packaging_url(PKG)
        )));
        let deps = &calls[pos("pkexec /bin/bash -c export LC_ALL=C; exec pacman -S")];
        assert!(deps.ends_with("rexon-rebuild libgranite-7.so libswitchboard-3.so"), "{deps}");
        assert!(deps.contains("--asdeps"));
        // Зависимости — до сборки, сборка — до установки.
        assert!(pos("pkexec /bin/bash -c export LC_ALL=C; exec") < pos("systemd-run"));
        assert!(pos("systemd-run") < pos("pkexec /bin/bash -c export LC_ALL=C; f="));
        let install = &calls[pos("pkexec /bin/bash -c export LC_ALL=C; f=")];
        // Ставится сам пакет, не -debug; снимать нечего — недоставало
        // зависимостей самого пакета.
        assert!(install.ends_with(&format!(
            "rexon-rebuild {DIR}/switchboard-plug-online-accounts-8.0.2-3-x86_64.pkg.tar.zst"
        )), "{install}");
        assert!(log.iter().any(|(s, m)| s == STAGE_RAW && m == "строка журнала"));
        assert!(outcome.message.contains(PKG));
    }

    /// `pkexec bash …` таймаута по классу команды не получает (30 с опроса):
    /// оба шага root — со своим, длинным.
    #[test]
    fn root_steps_wait_as_long_as_an_install() {
        let f = pantheon_fake();
        let (outcome, _) = rebuild(&f, &CancelToken::new());
        assert!(outcome.ok);
        let timeouts = f.timeouts.borrow();
        let root: Vec<&Option<u64>> = timeouts.iter().filter(|(c, _)| c.starts_with("pkexec")).map(|(_, t)| t).collect();
        assert_eq!(root, vec![&Some(INSTALL_TIMEOUT_SECS), &Some(INSTALL_TIMEOUT_SECS)]);
    }

    /// Помощник пакетов есть — оба root-шага идут через него (одно действие
    /// polkit с `auth_admin_keep`: пароль один на всю установку), `pkexec
    /// /bin/bash` не зовётся вовсе; таймауты те же.
    #[test]
    fn root_steps_go_through_pkg_helper_when_installed() {
        let mut f = pantheon_fake();
        f.answers.retain(|(p, _)| p != "pacman -T");
        f.answer("pacman -T", 127, "libgranite-7.so\nvala\n");
        let sink = Arc::new(Sink::default());
        let outcome = rebuild_with(&f, sink, &CancelToken::new(), &helper_ctx(), &[PKG.to_string()]);
        assert!(outcome.ok, "{outcome:?}");
        let calls = f.calls();
        assert!(!calls.iter().any(|c| c.contains("/bin/bash")), "{calls:#?}");
        let root: Vec<&String> = calls.iter().filter(|c| c.starts_with("pkexec")).collect();
        assert_eq!(
            root,
            vec![
                &format!("pkexec {HELPER} arch build-deps libgranite-7.so vala"),
                &format!(
                    "pkexec {HELPER} arch install-built plain \
                     {DIR}/switchboard-plug-online-accounts-8.0.2-3-x86_64.pkg.tar.zst vala"
                ),
            ]
        );
        let pos = |prefix: &str| calls.iter().position(|c| c.starts_with(prefix)).unwrap();
        assert!(pos(&format!("pkexec {HELPER} arch build-deps")) < pos("systemd-run"));
        assert!(pos("systemd-run") < pos(&format!("pkexec {HELPER} arch install-built")));
        let timeouts = f.timeouts.borrow();
        let root_t: Vec<Option<u64>> =
            timeouts.iter().filter(|(c, _)| c.starts_with("pkexec")).map(|(_, t)| *t).collect();
        assert_eq!(root_t, vec![Some(INSTALL_TIMEOUT_SECS), Some(INSTALL_TIMEOUT_SECS)]);
    }

    /// Сеанс прав окна есть: оба root-шага уходят помощнику сеанса —
    /// операциями, а не командами; ни `pkexec`, ни помощника пакетов. Файл —
    /// дескриптором (путь в списке файлов), снимаемое — только сборочное.
    #[test]
    fn root_steps_go_through_privilege_session() {
        for ctx in [ctx(), helper_ctx()] {
            let mut f = pantheon_fake();
            f.session = true;
            f.answers.retain(|(p, _)| p != "pacman -T");
            f.answer("pacman -T", 127, "libgranite-7.so\nvala\n");
            let sink = Arc::new(Sink::default());
            let outcome = rebuild_with(&f, sink, &CancelToken::new(), &ctx, &[PKG.to_string()]);
            assert!(outcome.ok, "{outcome:?}");
            assert!(!f.calls().iter().any(|c| c.starts_with("pkexec")), "{:#?}", f.calls());
            let file = format!("{DIR}/switchboard-plug-online-accounts-8.0.2-3-x86_64.pkg.tar.zst");
            let ops = f.priv_ops.borrow();
            assert_eq!(
                ops.as_slice(),
                [
                    (PrivOp::BuildDeps { names: vec!["libgranite-7.so".into(), "vala".into()] }, vec![]),
                    (
                        PrivOp::InstallBuilt {
                            package: LocalFile { slot: 0, label: file.clone() },
                            build_only: vec!["vala".into()],
                            replace: false,
                        },
                        vec![file.clone()],
                    ),
                ]
            );
        }
    }

    /// Замена конфликтующего через помощник — режим `replace`, файл и
    /// снимаемое — позиционно после него.
    #[test]
    fn helper_install_argv_modes() {
        let h = ["pkexec".to_string(), HELPER.to_string(), "--lang=ru".to_string()];
        let plain = helper_install_argv(&h, "/x/f.pkg.tar.zst", &[], false);
        assert_eq!(plain.join(" "), format!("pkexec {HELPER} --lang=ru arch install-built plain /x/f.pkg.tar.zst"));
        let replacing = helper_install_argv(&h, "/x/f.pkg.tar.zst", &["deepin-kwin".to_string()], true);
        assert_eq!(&replacing[3..], &["arch", "install-built", "replace", "/x/f.pkg.tar.zst", "deepin-kwin"].map(String::from));
        assert_eq!(
            helper_deps_argv(&h, &["cmake>=3.20".to_string()]).join(" "),
            format!("pkexec {HELPER} --lang=ru arch build-deps cmake>=3.20")
        );
    }

    /// Файл вне каталога пересборок помощник не примет — такой случай идёт
    /// прежним путём, а не отказом; зависимости при этом всё равно через
    /// помощник.
    #[test]
    fn package_outside_rebuild_root_falls_back_to_bash() {
        let mut f = pantheon_fake();
        f.answers.retain(|(p, _)| p != "env PKGDEST=");
        f.answer("env PKGDEST=", 0, "/home/u/packages/switchboard-plug-online-accounts-8.0.2-3-x86_64.pkg.tar.zst\n");
        f.files.insert("/home/u/packages/switchboard-plug-online-accounts-8.0.2-3-x86_64.pkg.tar.zst".into(), String::new());
        let sink = Arc::new(Sink::default());
        let outcome = rebuild_with(&f, sink, &CancelToken::new(), &helper_ctx(), &[PKG.to_string()]);
        assert!(outcome.ok, "{outcome:?}");
        let calls = f.calls();
        assert!(calls.iter().any(|c| c.starts_with(&format!("pkexec {HELPER} arch build-deps"))));
        assert!(!calls.iter().any(|c| c.contains("install-built")), "{calls:#?}");
        assert!(calls.iter().any(|c| c.starts_with("pkexec /bin/bash -c export LC_ALL=C; f=")));
    }

    /// Окно пароля помощника закрыли — тот же честный отказ с ручной
    /// командой, сборка не начинается.
    #[test]
    fn closed_password_window_of_helper_says_so() {
        let mut f = pantheon_fake();
        f.answers.insert(0, (format!("pkexec {HELPER} arch build-deps"), out(126, "")));
        let sink = Arc::new(Sink::default());
        let outcome = rebuild_with(&f, sink, &CancelToken::new(), &helper_ctx(), &[PKG.to_string()]);
        assert!(!outcome.ok);
        assert!(outcome.message.contains("sudo pacman -S --needed --asdeps libgranite-7.so"));
        assert!(!f.calls().iter().any(|c| c.starts_with("systemd-run")));
    }

    #[test]
    fn build_only_packages_are_removed_after_install() {
        let mut f = pantheon_fake();
        f.answers.retain(|(p, _)| p != "pacman -T");
        f.answer("pacman -T", 127, "vala\nbase-devel\nlibgee\n");
        let (outcome, _) = rebuild(&f, &CancelToken::new());
        assert!(outcome.ok);
        let calls = f.calls();
        let install = calls.iter().find(|c| c.contains("; f=")).unwrap();
        assert!(install.ends_with("x86_64.pkg.tar.zst vala base-devel"), "{install}");
    }

    #[test]
    fn missing_tag_falls_back_to_latest_recipe_with_a_note() {
        let mut f = pantheon_fake();
        f.answers.insert(0, ("git clone --quiet --depth 1 --branch".to_string(), out(128, "")));
        let (outcome, log) = rebuild(&f, &CancelToken::new());
        assert!(outcome.ok);
        assert!(f.calls().iter().any(|c| c.starts_with("git clone --quiet --depth 1 -- ")));
        assert!(log.iter().any(|(s, _)| s == STAGE_WARNING));
    }

    #[test]
    fn unreachable_gitlab_is_an_honest_refusal_before_any_password() {
        let mut f = pantheon_fake();
        f.answers.insert(0, ("git clone".to_string(), out(128, "")));
        let (outcome, log) = rebuild(&f, &CancelToken::new());
        assert!(!outcome.ok);
        assert!(outcome.message.contains("gitlab.archlinux.org"));
        assert!(!f.calls().iter().any(|c| c.starts_with("pkexec")));
        assert!(log.iter().any(|(s, _)| s == STAGE_ERROR));
    }

    #[test]
    fn closed_password_window_says_so() {
        let mut f = pantheon_fake();
        f.answers.insert(0, ("pkexec /bin/bash -c export LC_ALL=C; exec".to_string(), out(126, "")));
        let (outcome, _) = rebuild(&f, &CancelToken::new());
        assert!(!outcome.ok);
        assert!(outcome.message.contains("sudo pacman -S --needed --asdeps libgranite-7.so"));
        assert!(!f.calls().iter().any(|c| c.starts_with("systemd-run")));
    }

    #[test]
    fn checksum_mismatch_is_explained_and_nothing_installed() {
        let mut f = pantheon_fake();
        f.answers.insert(0, ("systemd-run".to_string(), out(1, "")));
        f.files.insert(
            format!("/home/u/.cache/rexon/rebuild/{PKG}.log"),
            "==> Validating source files with b2sums...\n    x ... FAILED\n==> ERROR: One or more files did not pass the validity check!\n".to_string(),
        );
        let (outcome, _) = rebuild(&f, &CancelToken::new());
        assert!(!outcome.ok);
        assert!(outcome.message.contains("validity check"));
        assert!(outcome.message.contains(&format!("/home/u/.cache/rexon/rebuild/{PKG}.log")));
        assert!(!f.calls().iter().any(|c| c.contains("; f=")));
    }

    #[test]
    fn missing_signing_key_is_imported_from_the_recipe_or_refused() {
        let fpr = "ABCDEF0123456789ABCDEF0123456789ABCDEF01";
        let mut f = pantheon_fake();
        f.files.insert(
            format!("{DIR}/.SRCINFO"),
            SRCINFO.replace("\tmakedepends = git\n", &format!("\tmakedepends = git\n\tvalidpgpkeys = {fpr}\n")),
        );
        f.files.insert(format!("{DIR}/keys/pgp/{fpr}.asc"), String::new());
        f.answers.insert(0, (format!("gpg --list-keys {fpr}"), out(2, "")));
        let (outcome, _) = rebuild(&f, &CancelToken::new());
        // Ключ так и не появился — сборки без проверки нет.
        assert!(!outcome.ok);
        assert!(f.calls().contains(&format!("gpg --import {DIR}/keys/pgp/{fpr}.asc")));
        assert!(!f.calls().iter().any(|c| c.starts_with("systemd-run")));
        assert!(outcome.message.contains(fpr));
    }

    #[test]
    fn cancel_before_any_change_is_a_clean_cancel() {
        let f = pantheon_fake();
        let cancel = CancelToken::new();
        cancel.cancel();
        let (outcome, _) = rebuild(&f, &cancel);
        assert!(outcome.cancelled);
        assert!(f.calls().is_empty());
    }

    #[test]
    fn deptest_answers_are_read() {
        assert_eq!(parse_deptest(0, ""), Some(vec![]));
        assert_eq!(parse_deptest(127, "vala\nlibgee>=0.20\n"), Some(vec!["vala".into(), "libgee>=0.20".into()]));
        assert_eq!(parse_deptest(1, "error"), None);
    }

    // ── Правка рецепта (Deepin на Treeland) ─────────────────────────────

    /// `PKGBUILD` из gitlab.archlinux.org, метка 2.0.33-1 (снято 20.10.2026).
    const DEEPIN_SESSION_PKGBUILD: &str = r#"# Maintainer: Felix Yan <felixonmars@archlinux.org>

pkgname=deepin-session
pkgver=2.0.33
pkgrel=1
pkgdesc='New deepin session, based on systemd and existing projects'
arch=('x86_64')
url="https://github.com/linuxdeepin/dde-session"
license=('GPL-3.0-or-later' 'LGPL-3.0-or-later' 'CC0-1.0')
depends=('bash' 'glibc' 'glib2' 'libcap-ng' 'libgcc' 'libstdc++' 'libx11' 'libxcb' 'libsecret' 'qt6-base' 'dtk6core'
         'libxcursor' 'libxfixes' 'systemd-libs' 'deepin-session-ui' 'deepin-kwin' 'deepin-shell'
         'deepin-qt5platform-plugins' 'deepin-qt5integration'
         'deepin-qt6platform-plugins' 'deepin-qt6integration')
makedepends=('git' 'cmake' 'ninja')
groups=('deepin')
source=("git+https://github.com/linuxdeepin/dde-session.git#tag=$pkgver")
sha512sums=('8e1b9bcba132cf905871a8288356ea2f12413340a61bb8611277298e95b0afef97f96c78ecc1f4412e89232a1c6e0b79db527cfc5cd86ce7d2918660d3bcb588')

package() {
  cd dde-session
  DESTDIR="$pkgdir" ninja install
}
"#;

    /// `.SRCINFO` той же метки (сокращено до полей, которые читает разбор).
    const DEEPIN_SESSION_SRCINFO: &str = "pkgbase = deepin-session\n\
\tpkgver = 2.0.33\n\tpkgrel = 1\n\tarch = x86_64\n\tgroups = deepin\n\
\tmakedepends = git\n\tmakedepends = cmake\n\tmakedepends = ninja\n\
\tdepends = bash\n\tdepends = qt6-base\n\tdepends = deepin-session-ui\n\
\tdepends = deepin-kwin\n\tdepends = deepin-shell\n\n\
pkgname = deepin-session\n";

    fn treeland_edit() -> RecipeEdit {
        RecipeEdit {
            pkgname: "deepin-session-treeland".into(),
            pkgdesc_note: "Rexon build without deepin-kwin".into(),
            to_optdepends: vec![("deepin-kwin".into(), "X11 session (conflicts with kwin)".into())],
            drop_files: vec!["usr/share/xsessions/deepin.desktop".into()],
            add_files: vec![("usr/share/libalpm/scripts/check".into(), "0755".into(), "#!/bin/sh\necho 'it''s' \"$x\"\n".into())],
            ..RecipeEdit::default()
        }
    }

    #[test]
    fn pkgrel_follows_the_repository_only_for_the_same_version() {
        let info = parse_srcinfo(DEEPIN_SESSION_SRCINFO, "x86_64").unwrap();
        assert_eq!(matching_pkgrel(&info, "2.0.33-1.1").as_deref(), Some("1.1"));
        assert_eq!(matching_pkgrel(&info, "2.0.33-1").as_deref(), Some("1"));
        assert_eq!(matching_pkgrel(&info, "2.0.34-1"), None);
        assert_eq!(matching_pkgrel(&info, "1:2.0.33-1"), None);
        assert_eq!(matching_pkgrel(&info, "2.0.33-1;rm"), None);
    }

    #[test]
    fn appendix_is_refused_for_split_recipes_and_bad_paths() {
        let info = parse_srcinfo(DEEPIN_SESSION_SRCINFO, "x86_64").unwrap();
        let mut split = info.clone();
        split.pkgnames.push("second".into());
        assert!(recipe_appendix(&treeland_edit(), &split, "2.0.33-1", &[]).is_none());
        let mut bad = treeland_edit();
        bad.drop_files = vec!["../etc/passwd".into()];
        assert!(recipe_appendix(&bad, &info, "2.0.33-1", &[]).is_none());
        let mut same = treeland_edit();
        same.pkgname = "deepin-session".into();
        assert!(recipe_appendix(&same, &info, "2.0.33-1", &[]).is_none());
        let mut eof = treeland_edit();
        eof.add_files[0].2 = format!("a\n{HEREDOC_END}\nb\n");
        assert!(recipe_appendix(&eof, &info, "2.0.33-1", &[]).is_none());
    }

    /// Дописанный рецепт читает настоящий bash — так же, как makepkg:
    /// имя, версия, зависимости, provides/conflicts и обёрнутая package().
    #[test]
    fn edited_recipe_is_read_by_bash_as_intended() {
        let info = parse_srcinfo(DEEPIN_SESSION_SRCINFO, "x86_64").unwrap();
        let appendix = recipe_appendix(&treeland_edit(), &info, "2.0.33-1.1", &["deepin-shell".to_string()]).unwrap();
        let dir = std::env::temp_dir().join(format!("rexon-recipe-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("PKGBUILD");
        std::fs::write(&path, format!("{DEEPIN_SESSION_PKGBUILD}{appendix}")).unwrap();
        let script = r#"set -e; source "$1"
printf 'name=%s\n' "$pkgname"; printf 'rel=%s\n' "$pkgrel"; printf 'desc=%s\n' "$pkgdesc"
printf 'dep=%s\n' "${depends[@]}"; printf 'opt=%s\n' "${optdepends[@]}"
printf 'prov=%s\n' "${provides[@]}"; printf 'conf=%s\n' "${conflicts[@]}"
printf 'grp=%s\n' "${groups[@]}"; printf 'pkgdep=%s\n' "${_rexon_package_depends[@]}"
declare -f package"#;
        let out = std::process::Command::new("bash").args(["-c", script, "bash"]).arg(&path).output().unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        let lines: Vec<&str> = text.lines().collect();
        assert!(lines.contains(&"name=deepin-session-treeland"));
        assert!(lines.contains(&"rel=1.1"));
        assert!(lines.contains(&"desc=New deepin session, based on systemd and existing projects (Rexon build without deepin-kwin)"));
        assert!(!lines.contains(&"dep=deepin-kwin"), "{text}");
        // Отложенная зависимость не проверяется перед сборкой, но
        // записывается в пакет: package() возвращает полный список.
        assert!(!lines.contains(&"dep=deepin-shell") && lines.contains(&"dep=bash"), "{text}");
        assert_eq!(lines.iter().filter(|l| l.starts_with("dep=")).count(), 19);
        assert!(lines.contains(&"pkgdep=deepin-shell") && !lines.contains(&"pkgdep=deepin-kwin"));
        assert_eq!(lines.iter().filter(|l| l.starts_with("pkgdep=")).count(), 20);
        assert!(text.contains("depends=(\"${_rexon_package_depends[@]}\")"), "{text}");
        assert!(lines.contains(&"opt=deepin-kwin: X11 session (conflicts with kwin)"));
        assert!(lines.contains(&"prov=deepin-session=2.0.33-1.1"));
        assert!(lines.contains(&"conf=deepin-session"));
        assert!(lines.contains(&"grp=deepin"), "группа остаётся — по ней снимается стол");
        // package(): сначала рецепт Arch, затем снятие и свои файлы.
        assert!(text.contains("_rexon_package_recipe"));
        assert!(text.contains("rm -f -- \"$pkgdir/usr/share/xsessions/deepin.desktop\""));
        assert!(text.contains("install -Dm0755 /dev/stdin \"$pkgdir/usr/share/libalpm/scripts/check\""));
    }

    /// Пересборка с правкой: рецепт дописан, `deepin-kwin` не просится ни
    /// в сборку, ни в установку, ставится файл под своим именем.
    const DEEPIN_DIR: &str = "/home/u/.cache/rexon/rebuild/deepin-session";

    fn deepin_fake() -> Fake {
        const DIR: &str = DEEPIN_DIR;
        let mut f = Fake::default();
        f.answer(
            "pacman -Si deepin-session",
            0,
            "Repository      : cachyos-extra-znver4\nName            : deepin-session\nVersion         : 2.0.33-1.1\n",
        );
        f.answer("tar -xOf", 0, "%BASE%\ndeepin-session\n");
        f.answer("pacman -T", 127, "deepin-shell\ncmake\n");
        // deepin-shell тянет deepin-session, а тот — deepin-kwin (живой ответ
        // в контейнере 20.10.2026, сокращён): ставить его до сборки нельзя.
        f.answer("pacman -Sp --noconfirm --print-format %n deepin-shell", 0, "deepin-session-shell\ndeepin-session\ndeepin-kwin\ndeepin-shell\n");
        f.answer(
            "env PKGDEST=",
            0,
            &format!("{DIR}/deepin-session-treeland-2.0.33-1.1-x86_64.pkg.tar.zst\n{DIR}/deepin-session-treeland-debug-2.0.33-1.1-x86_64.pkg.tar.zst\n"),
        );
        f.files.insert(format!("{DIR}/.SRCINFO"), DEEPIN_SESSION_SRCINFO.to_string());
        f.files.insert(format!("{DIR}/PKGBUILD"), DEEPIN_SESSION_PKGBUILD.to_string());
        f.files.insert(format!("{DIR}/deepin-session-treeland-2.0.33-1.1-x86_64.pkg.tar.zst"), String::new());
        f
    }

    /// Переход с обычного Deepin через помощник: `install-built` снимает
    /// только сборочное (то, что поставил `build-deps`), а `deepin-kwin`
    /// правки — отдельным `remove` помощника после установки; пароль тот же.
    #[test]
    fn migration_via_helper_removes_x11_wm_with_remove_op() {
        let f = deepin_fake();
        let edit = RecipeEdit {
            replace_conflicts: true,
            remove_after: vec!["deepin-kwin".into()],
            ..treeland_edit()
        };
        let sink = Arc::new(Sink::default());
        let outcome = rebuild_edited_with(&f, sink, &CancelToken::new(), &helper_ctx(), "deepin-session", &edit, "готово".into());
        assert!(outcome.ok, "{outcome:?}");
        let root: Vec<String> = f.calls().into_iter().filter(|c| c.starts_with("pkexec")).collect();
        assert_eq!(
            root,
            vec![
                format!("pkexec {HELPER} arch build-deps cmake"),
                format!(
                    "pkexec {HELPER} arch install-built replace \
                     {DEEPIN_DIR}/deepin-session-treeland-2.0.33-1.1-x86_64.pkg.tar.zst cmake"
                ),
                format!("pkexec {HELPER} arch remove deepin-kwin"),
            ]
        );
    }

    #[test]
    fn build_only_names_drop_version_constraints() {
        let deps: Vec<String> = ["cmake>=3.20", "vala", "cmake", "libfoo.so=1-64"].map(String::from).to_vec();
        assert_eq!(helper_build_only_names(&deps), ["cmake", "vala", "libfoo.so"].map(String::from).to_vec());
    }

    #[test]
    fn edited_rebuild_installs_the_renamed_package() {
        const DIR: &str = DEEPIN_DIR;
        let f = deepin_fake();
        let sink = Arc::new(Sink::default());
        let outcome = rebuild_edited_with(&f, sink, &CancelToken::new(), &ctx(), "deepin-session", &treeland_edit(), "готово".into());
        assert!(outcome.ok, "{outcome:?}");
        assert_eq!(outcome.message, "готово");
        let appended = f.appended.borrow();
        assert_eq!(appended.len(), 1);
        assert_eq!(appended[0].0, format!("{DIR}/PKGBUILD"));
        assert!(appended[0].1.contains("pkgname=deepin-session-treeland\npkgrel=1.1\n"));
        let calls = f.calls();
        let deptest = calls.iter().find(|c| c.starts_with("pacman -T")).unwrap();
        assert!(!deptest.contains("deepin-kwin"), "{deptest}");
        assert!(deptest.contains("deepin-shell"));
        let deps = calls.iter().find(|c| c.contains("exec pacman -S")).unwrap();
        assert!(deps.ends_with("rexon-rebuild cmake"), "отложенная deepin-shell не ставится до сборки: {deps}");
        assert!(appended[0].1.contains("in deepin-shell) ;;"), "{}", appended[0].1);
        let install = calls.iter().find(|c| c.contains("; f=")).unwrap();
        assert!(install.ends_with(&format!("{DIR}/deepin-session-treeland-2.0.33-1.1-x86_64.pkg.tar.zst cmake")), "{install}");
        assert!(!calls.iter().any(|c| c.contains("deepin-kwin")), "{calls:#?}");
    }

    #[test]
    fn build_failure_causes() {
        assert_eq!(build_failure("==> ERROR: One or more PGP signatures could not be verified!"), BuildFailure::Signature);
        assert_eq!(build_failure("==> ERROR: Failure while downloading x"), BuildFailure::Download);
        assert_eq!(build_failure("error: meson failed"), BuildFailure::Other);
    }

    /// Живая машина до первого пароля: настоящие `pacman -Si`, база
    /// синхронизации, клон с gitlab.archlinux.org, `.SRCINFO`, `pacman -T`;
    /// окно пароля «закрывается» (126) — в систему ничего не ставится.
    /// Сеть и Arch нужны, поэтому только вручную:
    /// `cargo test -p rexon-core --lib live_host_flow_stops_at_password -- --ignored`.
    #[test]
    #[ignore]
    fn live_host_flow_stops_at_password() {
        struct NoRoot(LiveRebuildIo);
        impl RebuildIo for NoRoot {
            fn run(&self, argv: &[String], t: Option<u64>, c: bool) -> RunOut {
                assert_ne!(argv[0], "pkexec");
                self.0.run(argv, t, c)
            }
            fn run_logged(&self, argv: &[String], t: Option<u64>, log: &str, line: Arc<dyn Fn(&str) + Send + Sync>) -> RunOut {
                if argv[0] == "pkexec" {
                    return out(126, "");
                }
                self.0.run_logged(argv, t, log, line)
            }
            fn read_text(&self, p: &str) -> Option<String> { self.0.read_text(p) }
            fn append_text(&self, p: &str, t: &str) -> Result<(), String> { self.0.append_text(p, t) }
            fn exists(&self, p: &str) -> bool { self.0.exists(p) }
            fn which(&self, n: &str) -> bool { self.0.which(n) }
            fn make_dirs(&self, p: &str) -> Result<(), String> { self.0.make_dirs(p) }
            fn remove_tree(&self, p: &str) { self.0.remove_tree(p) }
            fn lock_paths(&self) -> Vec<String> { self.0.lock_paths() }
            fn sleep_secs(&self, s: u64) { self.0.sleep_secs(s) }
        }
        let cache = std::env::var("REXON_REBUILD_STAND")
            .unwrap_or_else(|_| format!("{}/research/rebuild-stand/cache", std::env::var("HOME").unwrap()));
        let ctx = RebuildCtx {
            cache_dir: cache.clone(),
            carch: std::env::consts::ARCH.to_string(),
            db_path: "/var/lib/pacman".to_string(),
            slice: false,
            pkg_helper: None,
        };
        let sink = Arc::new(Sink::default());
        let outcome = rebuild_with(&NoRoot(LiveRebuildIo::new()), sink.clone(), &CancelToken::new(), &ctx, &[PKG.to_string()]);
        for (stage, message) in sink.0.lock().unwrap().iter() {
            eprintln!("[{stage}] {message}");
        }
        eprintln!("ИТОГ: {outcome:?}");
        assert!(std::path::Path::new(&format!("{cache}/rebuild/{PKG}/.SRCINFO")).exists());
        assert!(!outcome.ok);
    }
}
