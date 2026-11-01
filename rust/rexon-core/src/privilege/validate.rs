//! Проверки root-стороны (пункт Б3 плана, см. [`super`]).
//!
//! Помощник «Б» живёт весь сеанс окна, поэтому верить окну он не может ни в
//! чём: всё, что пришло по сокету, проверяется здесь, ПЕРЕД исполнением, и
//! только здесь решается «можно или нет». Функции чистые — без ввода-вывода:
//! всё, что надо знать о машине (текущий стол, ответы `pacman -Qi`, рецепт
//! пересборки, `PKEXEC_UID`), приходит параметрами. Тип операции
//! (`PrivOp`, пункт Б1) сюда не заходит: функции на простых аргументах,
//! `PrivOp` зовёт их сам (пункт Б8).
//!
//! # Таблицы — ядра, а не свои
//!
//! Списков пакетов здесь нет. Белые списки установки — те же таблицы, по
//! которым окно строит планы: [`system::DE_DATABASE`] (столы),
//! [`plan::DM_PAIRS`] (экраны входа), [`crate::compliance`] (доустановка
//! стола), [`crate::wallpaper_engine::install_deps_packages`] (видеообои),
//! оболочки, программы виджетов, Conky, части раскладок MATE, значки и
//! курсоры тем, образы «Как в дистрибутиве» — всё, что перечисляет
//! [`plan::pkg_helper_install_allowlist`] (сверка
//! `таблицы_сеанса_совпадают_с_белым_списком_помощника_пакетов`),
//! рецепт пересборки ([`repo_rebuild::build_requirements`]). Вечный список
//! удаления — [`plan::PKG_HELPER_VITAL`], имя пакета —
//! [`plan::is_safe_pkg_arg`], замыкание зависимостей —
//! [`system::filter_removable_transitive`], защита текущего стола —
//! [`plan::remove_de_decision`]. Так проверка помощника «Б» и проверка
//! нынешних `rexon-*-helper` не могут разойтись.
//!
//! # Запрет — функция
//!
//! | Строка плана | Функция |
//! |---|---|
//! | З1 ни флага, ни оболочки в аргументе | [`check_operand`] |
//! | З2 имя, а не путь | [`check_package_name`], [`check_item_name`], [`check_de_id`] |
//! | З2 путь только внутри корня пользователя | [`check_user_path`], [`check_built_package`] |
//! | З2 приёмник — константа ядра | [`stage_destination_roots`] |
//! | З3 чужой процесс на канале | [`check_peer`], [`parse_pkexec_uid`] |
//! | З5 никакого сырого pkexec после отказа | [`raw_fallback_allowed`] |
//! | З6 жизненно важное и текущий стол | [`check_not_vital`], [`check_remove_packages`], [`desktop_removal_set`] |
//! | З7 код от root из темы | [`check_plymouth_module_name`], [`check_theme_file_not_code`] |
//! | З8 обход чужого polkit (PLM) | [`check_plm_key`], [`check_plm_wallpaper_name`] |
//! | установка только из таблиц | [`check_install`], [`check_build_deps`], [`check_build_only`] |
//! | §1.4 п.5 неизменяемая система | [`check_mutable_system`] |
//! | стол набором: таблица стола, DKMS по правилу ядра, замена из таблицы замен | [`check_de_set`], [`check_de_set_preview`] |
//! | образ стола SteamOS | [`check_sysext`] |
//! | удаление сироты сторожа обновлений | [`check_remove_packages`] + проба машины в `helper.rs` ([`Refusal::NotOrphan`]) |
//! | откат набора сторожа из кэша pacman | [`check_cache_package`] + файлы кэша (`openat2`, владелец root) и сверка машины в `helper.rs` ([`Refusal::NotRollback`]) |
//! | шрифты «для всех пользователей»: имя файла шрифта, только каталог ядра, без перезаписи | [`check_font_install`], [`check_font_remove`] + `fc-query` по копии root и проба приёмника в `helper.rs` ([`Refusal::NotAFont`], [`Refusal::FontExists`]) |
//! | экран входа SDDM: фон темы, «Показывать часы», перенос оформления стола | [`check_sddm_look`] (+ имя темы — [`check_item_name`]) + в `helper.rs`: тема в системном каталоге с файлом настроек ([`Refusal::SddmTheme`]), PNG/JPEG (WebP — при модуле WebP у Qt приветствия темы) по сигнатуре копии root ([`Refusal::LoginImage`]), файлы в доме `sddm` — от его имени |
//!
//! З4 (жизнь прав после окна) — не проверка аргумента, она в
//! [`super::revoke`]. Проверки по файловой системе (`openat2`, ссылки,
//! содержимое `.plymouth`) — в [`super::unpack`].

use std::collections::HashSet;

use crate::distro_compat::SteamOsBlock;
use crate::repo_rebuild::{self, SrcInfo};
use crate::system::{self, PkgFamily};
use crate::system_plan as plan;

/// Самое длинное имя (пакета, стола, темы), которое помощник примет.
/// Имён длиннее нет ни в одной таблице; длинная строка — признак не имени.
pub const MAX_NAME_LEN: usize = 128;

/// Сколько пакетов помощник примет в одной операции. Самый длинный стол
/// базы — несколько десятков пакетов; запас — на группы и AUR.
pub const MAX_PACKAGES: usize = 512;

/// Самый длинный путь, который помощник примет (`PATH_MAX`).
pub const MAX_PATH_LEN: usize = 4096;

/// Почему помощник отказал. Отказ — не сбой: операция не началась, система
/// не тронута, и сырого `pkexec` вместо неё не будет (З5).
///
/// Текста человеку здесь нет намеренно: строки появятся вместе с окном
/// отказа (пункт Б13), а журналу хватает [`Refusal::code`] и данных.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// Пустой аргумент или пустой список там, где нужно хоть что-то.
    Empty,
    /// Аргумент длиннее [`MAX_NAME_LEN`] / [`MAX_PATH_LEN`].
    TooLong(String),
    /// Больше [`MAX_PACKAGES`] имён в одной операции.
    TooMany(usize),
    /// З1: флаг (`-oX`, `--config=…`), пробел, перевод строки или
    /// метасимвол оболочки в аргументе.
    Operand(String),
    /// З2: не имя пакета (путь, файл пакета, чужое `@`).
    PackageName(String),
    /// З2: не имя темы, каталога или категории.
    ItemName(String),
    /// Семейство, на котором помощник пакетами не управляет.
    Family(&'static str),
    /// Стола нет в базе Rexon.
    UnknownDesktop(String),
    /// Установка: имени нет в таблице, по которой идёт эта операция.
    NotInTable(String),
    /// Пересборка: строка не из рецепта или не зависимость.
    BuildDep(String),
    /// З6: жизненно важный пакет.
    Vital(String),
    /// З6: пакет текущего стола.
    CurrentDesktopPackage(String),
    /// З6: удаление самого текущего стола (текст ядра —
    /// [`plan::remove_de_decision`]).
    CurrentDesktop(String),
    /// З6: текущий стол не определён — защищать нечем, удалять нельзя.
    CurrentDesktopUnknown,
    /// З2: путь не абсолютный, с `.`/`..`/`//` или управляющими символами.
    Path(String),
    /// З2: путь вне разрешённого корня пользователя.
    OutsideRoot(String),
    /// Собранный файл назван не как файл makepkg.
    PackageFile(String),
    /// Экран входа не из таблицы помощников.
    DisplayManager(String),
    /// З7: тема несёт свой код (`.so`, чужой `ModuleName`).
    BootSplashCode(String),
    /// З3: на канале не тот процесс.
    Peer,
    /// З8: ключ настроек PLM вне белого списка.
    PlmKey(String),
    /// §1.4 п.5: системный раздел только для чтения (SteamOS). Текст —
    /// ядра ([`SteamOsBlock::reason`]).
    ReadOnlySystem(String),
    /// Набор стола: замена конфликтующего не из таблицы замен ядра или
    /// конфликт, которого согласие не покрывает (`--ask=4` снял бы лишнее).
    Conflict(String),
    /// Набор стола не сходится с правилом ядра по ответам pacman этой
    /// машины (неставящийся член, DKMS-пакет не по правилу, проба молчит).
    SetRule(String),
    /// Образ стола (sysext) — только на SteamOS.
    NotSteamOs,
    /// Удаление сироты: пакет не сирота по этой машине (не установлен, есть
    /// в репозиториях либо не член набора сторожа обновлений).
    NotOrphan(String),
    /// Откат из кэша: не ровно откат набора находки по этой машине (пакет не
    /// из набора, версия не меньше установленной, набор неполный или сверх).
    NotRollback(String),
    /// Шрифты: имя — не имя файла шрифта (путь, скрытый файл, расширение
    /// не из [`crate::qt_settings::FONT_INSTALL_EXTS`]) либо путь удаления
    /// не файл шрифта внутри [`crate::qt_settings::SHARED_FONT_DIR`].
    FontFile(String),
    /// Шрифты: fontconfig (`fc-query` по копии root) не считает файл
    /// шрифтом.
    NotAFont(String),
    /// Шрифты: в каталоге для всех пользователей уже лежит файл (или
    /// ссылка) с этим именем — чужое не перезаписывается.
    FontExists(String),
    /// Экран входа SDDM: учётной записи `sddm` (или её домашнего каталога)
    /// нет.
    NoSddmUser,
    /// Экран входа SDDM: темы нет в системном каталоге или она не принимает
    /// свой фон (нет файла настроек).
    SddmTheme(String),
    /// Экран входа SDDM: картинка фона — не PNG и не JPEG (и не WebP при
    /// модуле WebP у Qt приветствия) по сигнатуре.
    LoginImage(String),
    /// Перенос оформления: имя файла не из [`plan::SDDM_LOOK_FILES`], повтор,
    /// файлы при «Сбросить» или содержимое — не текст.
    LookFile(String),
    /// Перенос оформления: значение курсора, шрифта или DPI вне правил.
    LookValue(String),
    /// Перенос оформления: значение перекрыто в файле, который Rexon не
    /// правит.
    LookElsewhere,
    /// Перенос оформления: нечем писать от имени `sddm` (`setpriv`).
    NoSetpriv,
}

impl Refusal {
    /// Короткое имя для журнала (не переводится).
    pub fn code(&self) -> &'static str {
        match self {
            Refusal::Empty => "empty",
            Refusal::TooLong(_) => "too-long",
            Refusal::TooMany(_) => "too-many",
            Refusal::Operand(_) => "operand",
            Refusal::PackageName(_) => "package-name",
            Refusal::ItemName(_) => "item-name",
            Refusal::Family(_) => "family",
            Refusal::UnknownDesktop(_) => "unknown-desktop",
            Refusal::NotInTable(_) => "not-in-table",
            Refusal::BuildDep(_) => "build-dep",
            Refusal::Vital(_) => "vital",
            Refusal::CurrentDesktopPackage(_) => "current-desktop-package",
            Refusal::CurrentDesktop(_) => "current-desktop",
            Refusal::CurrentDesktopUnknown => "current-desktop-unknown",
            Refusal::Path(_) => "path",
            Refusal::OutsideRoot(_) => "outside-root",
            Refusal::PackageFile(_) => "package-file",
            Refusal::DisplayManager(_) => "display-manager",
            Refusal::BootSplashCode(_) => "boot-splash-code",
            Refusal::Peer => "peer",
            Refusal::PlmKey(_) => "plm-key",
            Refusal::ReadOnlySystem(_) => "read-only-system",
            Refusal::Conflict(_) => "conflict",
            Refusal::SetRule(_) => "set-rule",
            Refusal::NotSteamOs => "not-steamos",
            Refusal::NotOrphan(_) => "not-orphan",
            Refusal::NotRollback(_) => "not-rollback",
            Refusal::FontFile(_) => "font-file",
            Refusal::NotAFont(_) => "not-a-font",
            Refusal::FontExists(_) => "font-exists",
            Refusal::NoSddmUser => "no-sddm-user",
            Refusal::SddmTheme(_) => "sddm-theme",
            Refusal::LoginImage(_) => "login-image",
            Refusal::LookFile(_) => "look-file",
            Refusal::LookValue(_) => "look-value",
            Refusal::LookElsewhere => "look-elsewhere",
            Refusal::NoSetpriv => "no-setpriv",
        }
    }
}

type Check<T = ()> = Result<T, Refusal>;

// ============================================================================
// З1 — ни флага, ни оболочки
// ============================================================================

/// Символы, которых нет ни в одном имени таблиц ядра и которые что-то
/// значат для оболочки или разбора аргументов.
fn is_shell_meta(c: char) -> bool {
    c.is_whitespace()
        || c.is_control()
        || matches!(
            c,
            ';' | '|' | '&' | '$' | '`' | '\'' | '"' | '\\' | '<' | '>' | '(' | ')' | '{'
                | '}' | '[' | ']' | '*' | '?' | '~' | '!' | '#' | '%' | '^' | ','
        )
}

/// З1: строка от окна, которая станет словом argv. Непустая, не длиннее
/// [`MAX_NAME_LEN`], не флаг (ведущий `-` — `-oX`, `--config=…`, `--root`)
/// и без метасимволов оболочки и пробелов. Это ПЕРВЫЙ рубеж любой
/// именной проверки; второй — правило самого вида имени.
pub fn check_operand(s: &str) -> Check {
    if s.is_empty() {
        return Err(Refusal::Empty);
    }
    if s.len() > MAX_NAME_LEN {
        return Err(Refusal::TooLong(clip(s)));
    }
    if s.starts_with('-') || s.chars().any(is_shell_meta) {
        return Err(Refusal::Operand(clip(s)));
    }
    Ok(())
}

/// Для журнала: длинную строку от окна целиком не носим.
fn clip(s: &str) -> String {
    s.chars().take(MAX_NAME_LEN).collect()
}

// ============================================================================
// З2 — имя, а не путь
// ============================================================================

/// Семейство, на котором помощник управляет пакетами
/// ([`plan::PKG_HELPER_FAMILIES`]; Nix и неизвестное — нет).
pub fn check_family(family: PkgFamily) -> Check {
    if plan::PKG_HELPER_FAMILIES.contains(&family.as_str()) {
        Ok(())
    } else {
        Err(Refusal::Family(family.as_str()))
    }
}

/// З2: имя пакета. [`check_operand`] + правило ядра
/// [`plan::is_safe_pkg_arg`] — то же, что у `rexon-pkg-helper`: `/` только в
/// атоме Gentoo, `@` только у группы dnf, файл пакета (`x.deb`) — отказ
/// даже без `/`.
pub fn check_package_name(family: PkgFamily, name: &str) -> Check {
    check_operand(name)?;
    if plan::is_safe_pkg_arg(family, name) {
        Ok(())
    } else {
        Err(Refusal::PackageName(clip(name)))
    }
}

/// З2: список имён одной операции — непустой, не длиннее
/// [`MAX_PACKAGES`], каждое по [`check_package_name`].
pub fn check_package_list(family: PkgFamily, names: &[String]) -> Check {
    check_family(family)?;
    if names.is_empty() {
        return Err(Refusal::Empty);
    }
    if names.len() > MAX_PACKAGES {
        return Err(Refusal::TooMany(names.len()));
    }
    names.iter().try_for_each(|n| check_package_name(family, n))
}

/// З2: имя темы, каталога раскладки, фона — правило `rexon-system-helper`
/// и `rexon-stage-helper` (и [`plan::privileged_stage_cmd`]):
/// `[A-Za-z0-9._-]+`, без ведущих `-` и `.`, без `..`.
pub fn check_item_name(name: &str) -> Check {
    check_operand(name)?;
    let ok = !name.starts_with('.')
        && !name.contains("..")
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if ok {
        Ok(())
    } else {
        Err(Refusal::ItemName(clip(name)))
    }
}

/// З2: стол — только из базы ядра ([`system::DE_DATABASE`]). Это строже,
/// чем символьная проверка `rexon-de-helper`: незнакомое имя не пройдёт,
/// даже если оно «чистое».
pub fn check_de_id(de_id: &str) -> Check {
    check_operand(de_id)?;
    if system::DE_DATABASE.iter().any(|(id, _)| *id == de_id) {
        Ok(())
    } else {
        Err(Refusal::UnknownDesktop(clip(de_id)))
    }
}

/// Категория раскладки файлов — из [`plan::STAGE_CATEGORIES`].
pub fn check_stage_category(category: &str) -> Check {
    if plan::STAGE_CATEGORIES.contains(&category) {
        Ok(())
    } else {
        Err(Refusal::ItemName(clip(category)))
    }
}

/// З2: приёмник раскладки — константа ядра по категории
/// ([`plan::STAGE_DIR_ROOT`] и второй корень webkit
/// [`plan::WEB_GREETER_ROOT`]). Пусто — у категории приёмник один файл
/// фона, его путь помощник тоже знает сам. Пути от окна нет вовсе.
pub fn stage_destination_roots(category: &str) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = plan::STAGE_DIR_ROOT
        .iter()
        .filter(|(c, _)| *c == category)
        .map(|(_, root)| *root)
        .collect();
    if category == "lightdm-webkit" {
        out.push(plan::WEB_GREETER_ROOT);
    }
    out
}

/// Вид системной темы (`rexon-system-helper`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemThemeKind {
    Sddm,
    Plymouth,
}

/// Что сделать с системной темой.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemThemeMode {
    Install,
    Apply,
    Remove,
}

/// Категория `rexon-system-helper` (`sddm`, `apply-plymouth`,
/// `remove-sddm`, …) — тот же закрытый список, что `case` помощника.
pub fn parse_system_category(category: &str) -> Check<(SystemThemeMode, SystemThemeKind)> {
    let (mode, kind) = match category.split_once('-') {
        Some(("apply", k)) => (SystemThemeMode::Apply, k),
        Some(("remove", k)) => (SystemThemeMode::Remove, k),
        _ => (SystemThemeMode::Install, category),
    };
    match kind {
        "sddm" => Ok((mode, SystemThemeKind::Sddm)),
        "plymouth" => Ok((mode, SystemThemeKind::Plymouth)),
        _ => Err(Refusal::ItemName(clip(category))),
    }
}

/// Экран входа для `enable-dm` — из [`plan::DE_HELPER_DISPLAY_MANAGERS`].
pub fn check_enable_dm_unit(unit: &str) -> Check {
    if plan::DE_HELPER_DISPLAY_MANAGERS.contains(&unit) {
        Ok(())
    } else {
        Err(Refusal::DisplayManager(clip(unit)))
    }
}

/// Экран входа для `switch-dm` — из [`plan::dm_switch_units`].
pub fn check_switch_dm_unit(unit: &str) -> Check {
    if plan::dm_switch_units().contains(&unit) {
        Ok(())
    } else {
        Err(Refusal::DisplayManager(clip(unit)))
    }
}

// ============================================================================
// З2 — путь только внутри корня пользователя (без ФС)
// ============================================================================

/// Лексически нормальный абсолютный путь: начинается с `/`, без пустых
/// компонентов (`//`, хвостовой `/`), без `.` и `..`, без управляющих
/// символов, не длиннее [`MAX_PATH_LEN`]. Ссылки так не проверить — это
/// дело [`super::unpack`] (`openat2` без ссылок); здесь — то, что видно по
/// строке.
fn is_normal_abs_path(p: &str) -> bool {
    p.len() > 1
        && p.len() <= MAX_PATH_LEN
        && p.starts_with('/')
        && !p.chars().any(|c| c.is_control())
        && p[1..].split('/').all(|seg| !seg.is_empty() && seg != "." && seg != "..")
}

/// З2: путь от окна — строго ВНУТРИ `root` (сам `root` не годится).
/// `root` даёт root-сторона, не окно: из домашнего каталога по
/// `PKEXEC_UID` из passwd ([`user_rebuild_root`]). Чужой абсолютный путь
/// (`/tmp/x.deb`, `/etc/…`, `/var/cache/pacman/pkg/…`), `..` и
/// «соседний» корень (`/home/a/.cache/rexon/rebuild2/…`) — отказ.
pub fn check_user_path<'a>(path: &'a str, root: &str) -> Check<&'a str> {
    if path.is_empty() {
        return Err(Refusal::Empty);
    }
    if path.len() > MAX_PATH_LEN {
        return Err(Refusal::TooLong(clip(path)));
    }
    if !is_normal_abs_path(path) {
        return Err(Refusal::Path(clip(path)));
    }
    // Неправильный корень — ошибка root-стороны, но и тогда ответ «нет».
    if !is_normal_abs_path(root) {
        return Err(Refusal::OutsideRoot(clip(path)));
    }
    match path.strip_prefix(root).and_then(|rest| rest.strip_prefix('/')) {
        Some(rest) if !rest.is_empty() => Ok(rest),
        _ => Err(Refusal::OutsideRoot(clip(path))),
    }
}

// ============================================================================
// Шрифты «для всех пользователей»
// ============================================================================

/// Сколько файлов шрифтов помощник примет в одной операции (семейство с
/// начертаниями — десятки файлов; запас — на сборники).
pub const MAX_FONT_FILES: usize = 256;

/// Предел одного файла шрифта. Самые тяжёлые — сборники CJK (`.ttc`) на
/// десятки мегабайт.
pub const MAX_FONT_BYTES: u64 = 256 * 1024 * 1024;

/// Самое длинное имя файла (`NAME_MAX`, байты).
const MAX_FILE_NAME_BYTES: usize = 255;

/// Имя файла шрифта, под которым он ляжет в каталог для всех: одно имя (без
/// `/`), не скрытое и не похожее на ключ (`.`, `-` в начале), без `..` и
/// управляющих символов, с расширением из
/// [`crate::qt_settings::FONT_INSTALL_EXTS`] (без учёта регистра).
/// Пробелы и не-латиница можно: имя не уходит ни в оболочку, ни в argv —
/// помощник только создаёт файл с ним.
pub fn check_font_file_name(name: &str) -> Check {
    if name.is_empty() {
        return Err(Refusal::Empty);
    }
    if name.len() > MAX_FILE_NAME_BYTES {
        return Err(Refusal::TooLong(clip(name)));
    }
    let ok = !name.starts_with('.')
        && !name.starts_with('-')
        && !name.contains('/')
        && !name.contains("..")
        && !name.chars().any(|c| c.is_control())
        && crate::qt_settings::font_install_ext_ok(name)
        // Одно расширение без имени (`.ttf`) отсечено выше; `ttf` без точки —
        // не файл шрифта.
        && name.contains('.');
    if ok {
        Ok(())
    } else {
        Err(Refusal::FontFile(clip(name)))
    }
}

/// Установка шрифтов для всех: список имён непуст, не длиннее
/// [`MAX_FONT_FILES`], каждое — [`check_font_file_name`], без повторов.
/// Каталог приёмника — константа ядра
/// ([`crate::qt_settings::SHARED_FONT_DIR`]): от окна только имена.
pub fn check_font_install(names: &[&str]) -> Check {
    if names.is_empty() {
        return Err(Refusal::Empty);
    }
    if names.len() > MAX_FONT_FILES {
        return Err(Refusal::TooMany(names.len()));
    }
    let mut seen = HashSet::new();
    for n in names {
        check_font_file_name(n)?;
        if !seen.insert(*n) {
            return Err(Refusal::FontFile(clip(n)));
        }
    }
    Ok(())
}

/// Удаление шрифтов для всех: каждый путь — нормальный абсолютный путь
/// строго внутри [`crate::qt_settings::SHARED_FONT_DIR`] (подкаталоги
/// можно: так раскладывает и администратор), последний сегмент — имя
/// файла шрифта ([`check_font_file_name`]), каждый промежуточный — не
/// скрытый; без повторов, не больше [`MAX_FONT_FILES`]. Шрифты пакетов
/// (`/usr/share/fonts`), каталог целиком, чужие пути — отказ. Что это
/// обычный файл, а не ссылка, помощник проверяет по системе сам.
/// Возвращает пути относительно каталога.
pub fn check_font_remove(files: &[String]) -> Check<Vec<&str>> {
    if files.is_empty() {
        return Err(Refusal::Empty);
    }
    if files.len() > MAX_FONT_FILES {
        return Err(Refusal::TooMany(files.len()));
    }
    let mut seen = HashSet::new();
    let mut out = Vec::with_capacity(files.len());
    for f in files {
        let rest = check_user_path(f, crate::qt_settings::SHARED_FONT_DIR)?;
        let (dirs, name) = match rest.rsplit_once('/') {
            Some((d, n)) => (Some(d), n),
            None => (None, rest),
        };
        check_font_file_name(name)?;
        if dirs.is_some_and(|d| d.split('/').any(|seg| seg.starts_with('.') || seg.starts_with('-'))) {
            return Err(Refusal::FontFile(clip(f)));
        }
        if !seen.insert(rest) {
            return Err(Refusal::FontFile(clip(f)));
        }
        out.push(rest);
    }
    Ok(out)
}

/// Корень пересборок пользователя: `<home>/.cache/rexon/rebuild` — как у
/// `rexon-pkg-helper` (домашний каталог — из passwd по `PKEXEC_UID`, не из
/// окружения окна, поэтому `XDG_CACHE_HOME` здесь не участвует).
pub fn user_rebuild_root(home: &str) -> String {
    repo_rebuild::rebuild_root(&format!("{}/.cache/rexon", home.trim_end_matches('/')))
}

/// Хвосты файла makepkg (`PKGEXT`) — те же, что у `install-built`
/// `rexon-pkg-helper`.
const MAKEPKG_EXT: &[&str] = &[
    ".pkg.tar", ".pkg.tar.zst", ".pkg.tar.xz", ".pkg.tar.gz", ".pkg.tar.bz2",
    ".pkg.tar.lz4", ".pkg.tar.lzo", ".pkg.tar.lrz", ".pkg.tar.lz", ".pkg.tar.Z",
];

/// З2: собранный файл для `pacman -U` — только из каталога пересборок
/// пользователя, и не в самом корне, а в подкаталоге рецепта
/// (`<rebuild_root>/<рецепт>/<файл>`), имя — имя файла makepkg. Так
/// `pacman -U` не получит ни `/tmp/x.deb`, ни старую версию из кэша pacman,
/// ни системный файл. Владельца, ссылки и «обычный ли файл» проверяет
/// приём по fd ([`super::unpack`]).
pub fn check_built_package<'a>(path: &'a str, rebuild_root: &str) -> Check<&'a str> {
    let rel = check_user_path(path, rebuild_root)?;
    let Some((_, name)) = rel.rsplit_once('/') else {
        return Err(Refusal::OutsideRoot(clip(path)));
    };
    check_package_file_name(name)
}

/// Собранный AUR-помощником файл для `pacman -U` (`PrivOp::AurInstallBuilt`,
/// пункт Б10): абсолютный путь без `.`/`..`, имя — имя файла makepkg. Места
/// не проверяем: каталог сборки задают paru/yay и `PKGDEST` человека. Что
/// ставится, решает `.PKGINFO` копии root (имя — из тех, что окно просило
/// собрать); путь root не открывает.
pub fn check_aur_package(path: &str) -> Check<&str> {
    if path.is_empty() {
        return Err(Refusal::Empty);
    }
    if !is_normal_abs_path(path) {
        return Err(Refusal::Path(clip(path)));
    }
    let name = path.rsplit_once('/').map_or(path, |(_, n)| n);
    check_package_file_name(name)
}

/// Файл кэша pacman для отката (`PrivOp::CacheRollback`): нормальный
/// абсолютный путь не в корне, имя — имя файла makepkg. → `(каталог, имя)`.
/// Входит ли каталог в кэш машины (`pacman-conf CacheDir`) и чей файл,
/// решает помощник при исполнении: открывает его сам, без ссылок.
pub fn check_cache_package(path: &str) -> Check<(&str, &str)> {
    let name = check_aur_package(path)?;
    match path.rsplit_once('/') {
        Some((dir, _)) if !dir.is_empty() => Ok((dir, name)),
        _ => Err(Refusal::Path(clip(path))),
    }
}

/// Имя файла makepkg: `<имя>-<версия>-<выпуск>-<арх>` + хвост [`MAKEPKG_EXT`].
fn check_package_file_name(name: &str) -> Check<&str> {
    let name_ok = name
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphanumeric() || matches!(c, '@' | '_' | '+'))
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '@' | '.' | '_' | '+' | ':' | '-'))
        && MAKEPKG_EXT.iter().any(|e| name.ends_with(e));
    if name_ok {
        Ok(name)
    } else {
        Err(Refusal::PackageFile(clip(name)))
    }
}

// ============================================================================
// З3 — на канале только окно, которое подняло помощника
// ============================================================================

/// `PKEXEC_UID` — только десятичные цифры (так же требует
/// `rexon-pkg-helper`). `None` — помощник запущен не через pkexec.
pub fn parse_pkexec_uid(s: &str) -> Option<u32> {
    if s.is_empty() || s.len() > 10 || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

/// З3: `SO_PEERCRED` собеседника против того, кто поднял помощника: UID —
/// `PKEXEC_UID`, pid — родитель помощника (`getppid()`; pkexec заменяет
/// себя помощником через exec, родитель — поток окна).
pub fn check_peer(peer_uid: u32, peer_pid: i32, pkexec_uid: u32, parent_pid: i32) -> Check {
    if peer_pid > 1 && peer_uid == pkexec_uid && peer_pid == parent_pid {
        Ok(())
    } else {
        Err(Refusal::Peer)
    }
}

// ============================================================================
// З5 — отказ помощника не обходится сырым pkexec
// ============================================================================

/// Почему операция не прошла через помощник.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelperFailure {
    /// Файла помощника нет.
    Missing,
    /// Помощник есть, но владелец или права не те (подменён).
    BadOwner,
    /// Помощник проверил аргумент и отказал ([`Refusal`]).
    Refused,
    /// Сеанс оборвался или кадр испорчен.
    SessionLost,
    /// Человек не подтвердил пароль.
    NotAuthorized,
}

/// З5: можно ли идти путём «как сейчас» (сырой `pkexec` из того же
/// `PrivOp`). Только если помощника нет вовсе или он недопустим. После
/// отказа по проверке, обрыва сеанса и отказа в пароле — никогда (урок
/// 78bcc84: эталон закреплял обход сырым `pkexec pacman`).
pub fn raw_fallback_allowed(failure: HelperFailure) -> bool {
    matches!(failure, HelperFailure::Missing | HelperFailure::BadOwner)
}

// ============================================================================
// З6 — жизненно важное и текущий стол
// ============================================================================

/// Жизненно важный пакет — правило ядра [`plan::is_vital_package`] (точные
/// имена [`plan::PKG_HELPER_VITAL`] и семейства ядер, драйверов, прошивок и
/// загрузки); тот же шаблон вшит в `rexon-pkg-helper`.
pub fn is_vital(name: &str) -> bool {
    plan::is_vital_package(name)
}

/// З6: жизненно важное не удаляется никогда.
pub fn check_not_vital(name: &str) -> Check {
    if is_vital(name) {
        Err(Refusal::Vital(clip(name)))
    } else {
        Ok(())
    }
}

/// Пакеты стола по таблице ядра — [`system::desktop_packages`].
pub fn desktop_packages(family: PkgFamily, de_id: &str) -> Vec<String> {
    system::desktop_packages(de_id, family)
}

/// Текущий стол известен базе: иначе защищать нечем.
fn known_current_de(current_de: &str) -> Check<&str> {
    let id = current_de.trim();
    if id.is_empty() || id == "unknown" || check_de_id(id).is_err() {
        Err(Refusal::CurrentDesktopUnknown)
    } else {
        Ok(id)
    }
}

/// З6: удаление ПО ИМЕНАМ (`remove`, снятие сборочных пакетов): каждое
/// имя — имя, не жизненно важное и не из пакетов текущего стола
/// ([`desktop_packages`] для `current_de`; общие у двух столов `lightdm`,
/// `sddm` тоже под защитой). Текущий стол не определён — отказ.
pub fn check_remove_packages(family: PkgFamily, names: &[String], current_de: &str) -> Check {
    check_package_list(family, names)?;
    let current = known_current_de(current_de)?;
    let protected: HashSet<String> = desktop_packages(family, current).into_iter().collect();
    for n in names {
        check_not_vital(n)?;
        if protected.contains(n) {
            return Err(Refusal::CurrentDesktopPackage(clip(n)));
        }
    }
    Ok(())
}

/// З6: удаление СТОЛА целиком — набор пакетов root-сторона строит сама,
/// от окна приходит только `de_id`.
///
/// 1. Защита ядра ([`plan::remove_de_decision`], `force = false`): сам
///    текущий стол — отказ с текстом ядра.
/// 2. Из набора выпадают жизненно важные и пакеты текущего стола (общий
///    экран входа: `lightdm` у Cinnamon и Xfce).
/// 3. Замыкание ядра ([`system::filter_removable_transitive`]): пакет, нужный
///    кому-то вне набора, и всё, что нужно ему, остаётся. `qi` — вывод
///    `pacman -Qi <пакет>` (на прочих семействах не зовётся).
///
/// `Ok` — что удалять; может быть пустым (всё оказалось общим).
pub fn desktop_removal_set(
    family: PkgFamily,
    de_id: &str,
    current_de: &str,
    qi: &dyn Fn(&str) -> Option<String>,
) -> Check<Vec<String>> {
    check_family(family)?;
    check_de_id(de_id)?;
    let current = known_current_de(current_de)?;
    let display = system::DE_META
        .iter()
        .find(|m| m.id == de_id)
        .map(|m| m.display_name)
        .unwrap_or(de_id);
    let packages = system::get_packages(de_id, family);
    let aur = system::get_aur_packages(de_id, family);
    let all = match plan::remove_de_decision(de_id, display, current, false, &packages, &aur) {
        plan::RemoveDeDecision::Refused(text) => {
            return Err(if de_id == current {
                Refusal::CurrentDesktop(text)
            } else {
                Refusal::NotInTable(text)
            })
        }
        plan::RemoveDeDecision::Proceed(all) => all,
    };
    // Защита текущего стола — общая с `remove_de` окна
    // ([`system::without_current_desktop`]); экран входа root-сторона
    // определяет сама (помощник «Б», пункт Б8) — здесь его нет.
    let keep = system::current_desktop_keep(family, current, None);
    let candidates: Vec<String> = system::without_current_desktop(family, &all, &keep, &|_| None)
        .into_iter()
        .filter(|p| !is_vital(p))
        .collect();
    for p in &candidates {
        check_package_name(family, p)?;
    }
    let (safe, _kept) = system::filter_removable_transitive(family, &candidates, qi);
    Ok(safe)
}

// ============================================================================
// Установка — только из таблиц ядра
// ============================================================================

/// Имя пакета yt-dlp — константа ядра.
const YTDLP_PACKAGE: &str = crate::youtube_wallpaper::YTDLP_PACKAGE;

/// По какой таблице идёт установка. Окно называет таблицу, root-сторона
/// сама знает её содержимое — имя вне таблицы не пройдёт.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallSource<'a> {
    /// Стол из базы ([`system::DE_DATABASE`]): основные и AUR-пакеты.
    Desktop { de_id: &'a str },
    /// Экран входа ([`plan::DM_PAIRS`]): «защита от дурака» — нет DM → SDDM.
    DisplayManager,
    /// «Проверить мой стол»: пакеты ТЕКУЩЕГО стола и таблицы
    /// [`crate::compliance`] (Ayatana, MATE).
    DesktopRepair { current_de: &'a str },
    /// Видеозависимости обоев
    /// ([`crate::wallpaper_engine::install_deps_packages`], обе сессии).
    VideoDeps,
    /// yt-dlp для обоев с YouTube.
    YtDlp,
    /// Инструмент оверлея Gentoo ([`plan::GENTOO_OVERLAY_TOOL_PACKAGE`]).
    GentooTool,
    /// Оболочки стола ([`crate::shell_bundle::all_packages`]).
    Shell,
    /// Программы виджетов Conky и распаковщик 7z
    /// ([`crate::utils::tool_packages_for`]).
    Tools,
    /// Conky ([`crate::de_apply::conky_package`]).
    Conky,
    /// Части раскладок MATE ([`crate::mate_panel::all_part_packages`]).
    MatePanel,
    /// Значки и курсоры тем ([`crate::themes::known_set_packages`]).
    ThemeSets,
    /// Образы «Как в дистрибутиве»
    /// ([`crate::distro_looks::builtin_package_names`]).
    DistroLook,
}

/// Белый список одной таблицы. `Err` — таблицы нет (стол неизвестен).
pub fn install_allowlist(family: PkgFamily, source: InstallSource<'_>) -> Check<Vec<String>> {
    let owned = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    Ok(match source {
        InstallSource::Desktop { de_id } => {
            check_de_id(de_id)?;
            desktop_packages(family, de_id)
        }
        InstallSource::DisplayManager => {
            let mut all: Vec<String> = plan::DM_PAIRS.iter().map(|(pkg, _)| pkg.to_string()).collect();
            // «Нет экрана входа → SDDM»: имя пакета по семейству (Gentoo — с категорией).
            if let Some((_, p)) = plan::SDDM_PKG_FOR_FAMILY.iter().find(|(f, _)| *f == family.as_str()) {
                if !all.iter().any(|a| a == p) {
                    all.push(p.to_string());
                }
            }
            all
        }
        InstallSource::DesktopRepair { current_de } => {
            let current = known_current_de(current_de)?;
            let mut all = desktop_packages(family, current);
            all.extend(owned(crate::compliance::AYATANA_PACKAGES));
            all.extend(owned(crate::compliance::MATE_AUR_PACKAGES));
            all.extend(owned(crate::compliance::FIX_PACKAGES));
            all
        }
        InstallSource::VideoDeps => {
            let mut all = Vec::new();
            for session in ["wayland", "x11"] {
                for p in crate::wallpaper_engine::install_deps_packages(family.as_str(), session) {
                    if !all.contains(&p) {
                        all.push(p);
                    }
                }
            }
            all
        }
        InstallSource::YtDlp => vec![YTDLP_PACKAGE.to_string()],
        InstallSource::GentooTool => match family {
            PkgFamily::Gentoo => vec![plan::GENTOO_OVERLAY_TOOL_PACKAGE.to_string()],
            _ => Vec::new(),
        },
        InstallSource::Shell => owned(&crate::shell_bundle::all_packages(family)),
        InstallSource::Tools => owned(&crate::utils::tool_packages_for(family.as_str())),
        InstallSource::Conky => crate::de_apply::conky_package(family.as_str()).into_iter().map(str::to_string).collect(),
        InstallSource::MatePanel => owned(&crate::mate_panel::all_part_packages(family.as_str())),
        InstallSource::ThemeSets => owned(&crate::themes::known_set_packages(family.as_str())),
        InstallSource::DistroLook => crate::distro_looks::builtin_package_names(family),
    })
}

/// Установка: каждое имя — имя ([`check_package_list`]) и стоит в таблице
/// `source`.
pub fn check_install(family: PkgFamily, names: &[String], source: InstallSource<'_>) -> Check {
    check_package_list(family, names)?;
    let allowed: HashSet<String> = install_allowlist(family, source)?.into_iter().collect();
    match names.iter().find(|n| !allowed.contains(*n)) {
        Some(n) => Err(Refusal::NotInTable(clip(n))),
        None => Ok(()),
    }
}

/// Разбить строку зависимости на имя и ограничение версии (`cmake>=3.20`
/// → `cmake`, `>=3.20`) — как `dep_ok` в `rexon-pkg-helper`.
fn split_dep(dep: &str) -> (&str, &str) {
    let at = dep.find(['<', '>', '=']).unwrap_or(dep.len());
    dep.split_at(at)
}

fn version_constraint_ok(v: &str) -> bool {
    if v.is_empty() {
        return true;
    }
    let body = ["<=", ">=", "=", "<", ">"].iter().find_map(|op| v.strip_prefix(op));
    match body {
        Some(b) => {
            b.chars().next().is_some_and(|c| c.is_ascii_alphanumeric())
                && b.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '+' | ':' | '~' | '-'))
        }
        None => false,
    }
}

/// Пересборка, `build-deps`: только Arch; каждая строка — имя (правило
/// ядра) с, возможно, ограничением версии, и стоит в
/// [`repo_rebuild::build_requirements`] рецепта (`depends` + `makedepends` +
/// `base-devel`). Рецепт root-сторона разбирает сама из того, что
/// получила по fd, — не со слов окна.
pub fn check_build_deps(family: PkgFamily, deps: &[String], recipe: &SrcInfo) -> Check {
    if family != PkgFamily::Arch {
        return Err(Refusal::Family(family.as_str()));
    }
    if deps.is_empty() {
        return Err(Refusal::Empty);
    }
    if deps.len() > MAX_PACKAGES {
        return Err(Refusal::TooMany(deps.len()));
    }
    let wanted = repo_rebuild::build_requirements(recipe);
    for dep in deps {
        // `>`/`<` в ограничении версии законны, поэтому З1 — на имени
        // (внутри check_package_name), ограничение — своим правилом.
        if dep.len() > MAX_NAME_LEN {
            return Err(Refusal::TooLong(clip(dep)));
        }
        let (name, ver) = split_dep(dep);
        check_package_name(family, name)?;
        if !version_constraint_ok(ver) || !wanted.contains(dep) {
            return Err(Refusal::BuildDep(clip(dep)));
        }
    }
    Ok(())
}

/// Пересборка, хвост `install-built`: что снять после установки — только
/// то, что нужно ТОЛЬКО для сборки (`makedepends` или `base-devel`, но не
/// `depends`, правило [`repo_rebuild::build_only`]), и никогда не жизненно
/// важное. Пусто — можно.
pub fn check_build_only(family: PkgFamily, names: &[String], recipe: &SrcInfo) -> Check {
    if family != PkgFamily::Arch {
        return Err(Refusal::Family(family.as_str()));
    }
    if names.len() > MAX_PACKAGES {
        return Err(Refusal::TooMany(names.len()));
    }
    let allowed = repo_rebuild::build_only(names, recipe);
    for n in names {
        check_package_name(family, n)?;
        check_not_vital(n)?;
        if !allowed.contains(n) {
            return Err(Refusal::BuildDep(clip(n)));
        }
    }
    Ok(())
}

/// Откат тяжёлых форков Plasma: снимаются только форки
/// ([`plan::filter_plasma_forks`]), ставится только сток, который ядро само
/// выводит из метаданных форка ([`plan::fork_stock_candidates`]; `qi` —
/// `pacman -Qi`).
pub fn check_fork_rollback(
    forks: &[String],
    stock: &[String],
    qi: &dyn Fn(&str) -> Option<String>,
) -> Check {
    check_package_list(PkgFamily::Arch, forks)?;
    if plan::filter_plasma_forks(forks).len() != forks.len() {
        let bad = forks
            .iter()
            .find(|f| plan::filter_plasma_forks(std::slice::from_ref(f)).is_empty());
        return Err(Refusal::NotInTable(bad.map(|s| clip(s)).unwrap_or_default()));
    }
    if stock.is_empty() {
        return Ok(());
    }
    check_package_list(PkgFamily::Arch, stock)?;
    let allowed: HashSet<String> =
        forks.iter().flat_map(|f| plan::fork_stock_candidates(f, qi)).collect();
    match stock.iter().find(|s| !allowed.contains(*s)) {
        Some(s) => Err(Refusal::NotInTable(clip(s))),
        None => Ok(()),
    }
}

// ============================================================================
// З7 — код от root из темы
// ============================================================================

/// З7: `ModuleName=` темы Plymouth — только штатный модуль
/// ([`crate::installer::PLYMOUTH_SPLASH_MODULES`]). Plymouth строит путь
/// `<каталог модулей>/<имя>.so` без проверки `../`, и чужое имя — это `.so`
/// из самой темы от root при загрузке.
pub fn check_plymouth_module_name(value: &str) -> Check {
    if crate::installer::PLYMOUTH_SPLASH_MODULES.contains(&value.trim()) {
        Ok(())
    } else {
        Err(Refusal::BootSplashCode(clip(value)))
    }
}

/// З7: файл внутри темы — не библиотека (`x.so`, `x.so.1`): так же, как у
/// [`crate::installer::system_theme_problem`]. `rel` — путь внутри темы.
pub fn check_theme_file_not_code(rel: &str) -> Check {
    let base = rel.rsplit('/').next().unwrap_or(rel);
    if base.ends_with(".so") || base.contains(".so.") {
        Err(Refusal::BootSplashCode(clip(rel)))
    } else {
        Ok(())
    }
}

// ============================================================================
// З8 — PLM: помощник не пересказывает чужой конфиг
// ============================================================================

/// З8: ключ `plasmalogin.conf`, который помощник вправе поменять (план
/// §3.3): `[Greeter] ShowClock`, `WallpaperPluginId` и
/// `[Greeter][Wallpaper][<плагин>] Image/Color/FillMode`, где плагин — из
/// списка PLM (`plugins` даёт root-сторона). `[X11] ServerPath`,
/// `[Autologin] …`, `[Users] DefaultPath` и всё прочее — отказ.
/// `groups` — путь групп ключа (`["Greeter", "Wallpaper", "org.kde.image"]`).
pub fn check_plm_key(groups: &[&str], key: &str, plugins: &[&str]) -> Check {
    let ok = match groups {
        ["Greeter"] => matches!(key, "ShowClock" | "WallpaperPluginId"),
        ["Greeter", "Wallpaper", plugin] => {
            plugins.contains(plugin) && matches!(key, "Image" | "Color" | "FillMode")
        }
        _ => false,
    };
    if ok {
        Ok(())
    } else {
        Err(Refusal::PlmKey(clip(&format!("[{}] {key}", groups.join("][")))))
    }
}

/// З8: имя файла фона, который помощник положит в
/// `/var/lib/plasmalogin/wallpapers/` — имя, а не путь.
pub fn check_plm_wallpaper_name(name: &str) -> Check {
    check_item_name(name)
}

// ============================================================================
// Экран входа SDDM: перенос оформления стола
// ============================================================================

/// Строка от окна, которая ляжет значением в настройки SDDM (их читает
/// root-демон): непустая, не длиннее `max`, без управляющих символов
/// (перевод строки начал бы новую группу или ключ).
fn check_look_text(v: &str, max: usize) -> Check {
    if v.trim().is_empty() || v.len() > max || v.chars().any(|c| c.is_control()) {
        Err(Refusal::LookValue(clip(v)))
    } else {
        Ok(())
    }
}

/// Перенос оформления на экран входа SDDM ([`super::op::PrivOp::SddmLook`]):
/// * тема курсора — имя каталога (пробелы можно, `/`, `\` и ведущие `.`/`-`
///   — нет), размер — 8…512, шрифт — строка QFont до 256 символов, DPI —
///   48…480: так в `ServerArguments` уходит только `-dpi <число>`,
///   собранное помощником;
/// * файлы — имена из [`plan::SDDM_LOOK_FILES`] без повторов;
/// * «Сбросить» (`look: None`) — без файлов.
pub fn check_sddm_look(look: Option<&plan::SddmLook>, files: &[&str]) -> Check {
    let Some(look) = look else {
        return match files.first() {
            Some(f) => Err(Refusal::LookFile(clip(f))),
            None => Ok(()),
        };
    };
    if let Some(t) = &look.cursor_theme {
        check_look_text(t, MAX_NAME_LEN)?;
        if t.contains('/') || t.contains('\\') || t.starts_with('.') || t.starts_with('-') {
            return Err(Refusal::LookValue(clip(t)));
        }
    }
    if let Some(n) = look.cursor_size {
        if !(8..=512).contains(&n) {
            return Err(Refusal::LookValue(n.to_string()));
        }
    }
    if let Some(f) = &look.font {
        check_look_text(f, 256)?;
    }
    if let Some(d) = look.dpi {
        if !(48..=480).contains(&d) {
            return Err(Refusal::LookValue(d.to_string()));
        }
    }
    let mut seen = HashSet::new();
    for f in files {
        if !plan::SDDM_LOOK_FILES.contains(f) || !seen.insert(*f) {
            return Err(Refusal::LookFile(clip(f)));
        }
    }
    Ok(())
}

// ============================================================================
// §1.4 п.5 — неизменяемая система
// ============================================================================

/// На SteamOS системный раздел только для чтения: пакеты, экран входа,
/// заставку загрузки помощник не трогает. Правило и текст — ядра
/// ([`SteamOsBlock`]); `steamos` root-сторона узнаёт сама.
pub fn check_mutable_system(block: SteamOsBlock, steamos: bool) -> Check {
    match block.on(steamos) {
        Some(reason) => Err(Refusal::ReadOnlySystem(reason)),
        None => Ok(()),
    }
}

// ============================================================================
// Стол набором (DKMS `--asdeps`, замена конфликтующего, Deepin) и SteamOS
// ============================================================================

/// Чего в наборе стола не бывает никогда: `deepin-kwin` (оконный менеджер
/// Deepin X11, [`crate::deepin_treeland::forbid_x11_wm`]) — ни в каком
/// наборе; у Deepin ещё рецептный `deepin-session` (его даёт своя сборка) и
/// группа целиком.
fn forbidden_in_set(de_id: &str, name: &str) -> bool {
    use crate::deepin_treeland as dt;
    name == dt::X11_WM_PACKAGE || (de_id == dt::DE_ID && (name == dt::SESSION_PACKAGE || name == dt::GROUP))
}

/// Стол набором вне таблицы DE-помощника (`PrivOp::DeInstallSet`) — то, что
/// видно без вопросов к системе:
/// * только pacman (DKMS-модули, `--asdeps` и `--ask=4` — его);
/// * стол из базы, имена — имена, без `deepin-kwin` (у Deepin — и без
///   рецептного `deepin-session` и группы целиком);
/// * `deps_only` — из самого набора;
/// * `replace` (снимется той же транзакцией) — не жизненно важное, не из
///   набора и не пакет текущего стола; текущий стол должен быть известен и
///   не быть тем, что ставится (замена под работающим сеансом — нет).
///
/// Имена вне таблицы стола (члены её групп, DKMS-модуль, заголовки ядра) и
/// сама замена сверяются с правилом ядра по ответам pacman —
/// [`check_de_set_preview`], перед исполнением.
pub fn check_de_set(
    family: PkgFamily,
    de_id: &str,
    packages: &[String],
    deps_only: &[String],
    replace: &[String],
    current_de: &str,
) -> Check {
    if family != PkgFamily::Arch {
        return Err(Refusal::Family(family.as_str()));
    }
    check_de_id(de_id)?;
    check_package_list(family, packages)?;
    if let Some(bad) = packages.iter().find(|p| forbidden_in_set(de_id, p)) {
        return Err(Refusal::NotInTable(clip(bad)));
    }
    for list in [deps_only, replace] {
        if list.len() > MAX_PACKAGES {
            return Err(Refusal::TooMany(list.len()));
        }
    }
    if let Some(d) = deps_only.iter().find(|d| !packages.contains(*d)) {
        return Err(Refusal::NotInTable(clip(d)));
    }
    if replace.is_empty() {
        return Ok(());
    }
    let current = known_current_de(current_de)?;
    if current == de_id {
        return Err(Refusal::CurrentDesktop(clip(de_id)));
    }
    let protected: HashSet<String> = desktop_packages(family, current).into_iter().collect();
    for r in replace {
        check_package_name(family, r)?;
        check_not_vital(r)?;
        if protected.contains(r) {
            return Err(Refusal::CurrentDesktopPackage(clip(r)));
        }
        if packages.contains(r) {
            return Err(Refusal::Conflict(clip(r)));
        }
    }
    Ok(())
}

/// Правило ядра для набора по пробе, которую root-сторона сделала САМА:
/// `base` — имена набора из таблицы стола и члены её групп (по `pacman -Sg`
/// этой машины), `preview` — [`plan::install_preview`] по `base` (группы,
/// DKMS-модули и заголовки под стоящие ядра — [`plan::kernel_module_extras`],
/// конфликты с установленным). `None` — проба не ответила: отказ, ставить
/// непроверенное нельзя.
///
/// * набор не ставится (`blocked`, в том числе `deepin-kwin` где угодно в
///   плане) — отказ;
/// * имя набора вне `base` — только то, что правило ядра само добавило к
///   пробе (DKMS-вариант, заголовки стоящего ядра);
/// * `deps_only` — только то, что правило ядра считает зависимостью;
/// * `replace` — ровно замены из таблицы ядра ([`plan::conflict_swaps`]),
///   покрывающие ВСЕ конфликты пробы: `--ask=4` отвечает «да» на любой
///   конфликт, поэтому чужой конфликт при замене — отказ; без конфликтов
///   замены нет.
pub fn check_de_set_preview(
    packages: &[String],
    base: &[String],
    deps_only: &[String],
    replace: &[String],
    preview: Option<&plan::InstallPreview>,
) -> Check {
    let Some(p) = preview else {
        return Err(Refusal::SetRule("pacman".into()));
    };
    let mut p = p.clone();
    crate::deepin_treeland::forbid_x11_wm(&mut p);
    if let Some(b) = p.blocked.first() {
        return Err(Refusal::SetRule(clip(b)));
    }
    if let Some(extra) = packages.iter().find(|n| !base.contains(n) && !p.targets.contains(n)) {
        return Err(Refusal::NotInTable(clip(extra)));
    }
    if let Some(d) = deps_only.iter().find(|d| !p.deps_only.contains(d)) {
        return Err(Refusal::SetRule(clip(d)));
    }
    if p.conflicts.is_empty() {
        return match replace.first() {
            Some(r) => Err(Refusal::Conflict(clip(r))),
            None => Ok(()),
        };
    }
    let swaps = plan::conflict_swaps(&p);
    if !plan::conflict_swap_approved(&p, replace) {
        let first = p.conflicts.iter().find(|c| !replace.contains(&c.installed)).unwrap_or(&p.conflicts[0]);
        return Err(Refusal::Conflict(clip(&first.installed)));
    }
    match replace.iter().find(|r| !swaps.iter().any(|c| &c.installed == *r)) {
        Some(r) => Err(Refusal::Conflict(clip(r))),
        None => Ok(()),
    }
}

/// Образ стола на SteamOS (`PrivOp::Sysext`) — то же, что разрешает сегодня
/// `rexon-sysext-helper`: только на SteamOS (root-сторона узнаёт сама), стол
/// из таблицы [`crate::steamos_sysext::DESKTOPS`]. Снять стол, в котором
/// человек сейчас работает, нельзя (решение окна `remove_plan` — повторено
/// здесь).
pub fn check_sysext(de_id: &str, remove: bool, steamos: bool, current_de: &str) -> Check {
    if !steamos {
        return Err(Refusal::NotSteamOs);
    }
    if crate::steamos_sysext::desktop(de_id).is_none() {
        return Err(Refusal::UnknownDesktop(clip(de_id)));
    }
    if remove && current_de.trim() == de_id {
        return Err(Refusal::CurrentDesktop(clip(de_id)));
    }
    Ok(())
}

// ============================================================================
// Тесты — по отказу на строку таблицы запретов
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_FAMILIES: &[PkgFamily] = &[
        PkgFamily::Arch, PkgFamily::Debian, PkgFamily::Fedora, PkgFamily::Suse,
        PkgFamily::Gentoo, PkgFamily::Alpine, PkgFamily::Void, PkgFamily::Nix,
        PkgFamily::Solus, PkgFamily::Clear, PkgFamily::Alt,
    ];

    fn s(list: &[&str]) -> Vec<String> {
        list.iter().map(|x| x.to_string()).collect()
    }

    /// `pacman -Qi`: ничего никому не нужно.
    fn qi_none(_: &str) -> Option<String> {
        Some("Depends On      : None\nRequired By     : None\n".into())
    }

    /// Какой-нибудь стол, у которого на Arch есть пакеты, и второй, другой.
    fn two_desktops() -> (&'static str, &'static str) {
        let ids: Vec<&str> = system::DE_DATABASE
            .iter()
            .map(|(id, _)| *id)
            .filter(|id| !system::get_packages(id, PkgFamily::Arch).is_empty())
            .collect();
        let a = if ids.contains(&"xfce") { "xfce" } else { ids[0] };
        let b = if ids.contains(&"mate") && a != "mate" {
            "mate"
        } else {
            *ids.iter().find(|i| **i != a).unwrap()
        };
        (a, b)
    }

    // ── З1 ──────────────────────────────────────────────────────────────

    #[test]
    fn з1_флаги_и_оболочка_отказ() {
        for bad in [
            "-oX", "--config=/tmp/evil.conf", "--root=/", "-", "-dd", "a b", "a;b", "a|b",
            "$(id)", "`id`", "a\nb", "a\0b", "a&b", "x>y", "~", "",
        ] {
            assert!(check_operand(bad).is_err(), "{bad:?} прошёл");
        }
        // флаг не проходит и как имя пакета, и как имя темы
        assert!(matches!(check_package_name(PkgFamily::Arch, "-oX"), Err(Refusal::Operand(_))));
        assert!(matches!(
            check_package_name(PkgFamily::Arch, "--config=x"),
            Err(Refusal::Operand(_))
        ));
        assert!(check_item_name("--help").is_err());
        assert!(check_de_id("-x").is_err());
        assert!(matches!(check_operand(&"a".repeat(MAX_NAME_LEN + 1)), Err(Refusal::TooLong(_))));
        assert!(check_operand("xfce4-panel").is_ok());
    }

    // ── З2: имена ───────────────────────────────────────────────────────

    #[test]
    fn з2_путь_вместо_имени_отказ() {
        let arch = PkgFamily::Arch;
        for bad in ["/tmp/x.deb", "x.deb", "../x", "./x", "a/b", "x.pkg.tar.zst", ".hidden", "@world"] {
            assert!(check_package_name(arch, bad).is_err(), "{bad} прошёл");
        }
        // Gentoo: ровно один '/', наборов нет
        assert!(check_package_name(PkgFamily::Gentoo, "x11-misc/lightdm").is_ok());
        assert!(check_package_name(PkgFamily::Gentoo, "@world").is_err());
        assert!(check_package_name(PkgFamily::Gentoo, "/usr/bin/x").is_err());
        // Fedora: группа dnf — единственное законное '@'
        assert!(check_package_name(PkgFamily::Fedora, "@cinnamon-desktop-environment").is_ok());
        assert!(check_package_name(arch, "lightdm-slick-greeter").is_ok());
        // пустой и слишком длинный список
        assert_eq!(check_package_list(arch, &[]), Err(Refusal::Empty));
        let many = vec!["a".to_string(); MAX_PACKAGES + 1];
        assert_eq!(check_package_list(arch, &many), Err(Refusal::TooMany(MAX_PACKAGES + 1)));
        // семейство без помощника
        assert!(matches!(check_package_list(PkgFamily::Nix, &s(&["a"])), Err(Refusal::Family(_))));
        assert!(matches!(check_package_list(PkgFamily::Unknown, &s(&["a"])), Err(Refusal::Family(_))));
    }

    #[test]
    fn з2_имена_тем_и_столов() {
        for bad in ["", ".", "..", "a/b", "../x", ".x", "-x", "a..b", "a b", "тема"] {
            assert!(check_item_name(bad).is_err(), "{bad:?} прошёл");
        }
        assert!(check_item_name("breeze-2.0_x").is_ok());
        let (de, _) = two_desktops();
        assert!(check_de_id(de).is_ok());
        assert!(matches!(check_de_id("evil-de"), Err(Refusal::UnknownDesktop(_))));
        assert!(check_de_id("../kde").is_err());
    }

    #[test]
    fn з2_категории_и_приёмник_ядра() {
        for c in plan::STAGE_CATEGORIES {
            assert!(check_stage_category(c).is_ok());
        }
        assert!(check_stage_category("/usr/share").is_err());
        assert!(check_stage_category("etc").is_err());
        assert_eq!(
            stage_destination_roots("lightdm-webkit"),
            vec!["/usr/share/lightdm-webkit/themes", plan::WEB_GREETER_ROOT]
        );
        assert!(stage_destination_roots("gdm-bg").is_empty());
        assert_eq!(
            parse_system_category("apply-plymouth"),
            Ok((SystemThemeMode::Apply, SystemThemeKind::Plymouth))
        );
        assert_eq!(parse_system_category("sddm"), Ok((SystemThemeMode::Install, SystemThemeKind::Sddm)));
        assert!(parse_system_category("grub").is_err());
        assert!(parse_system_category("apply-grub").is_err());
        assert!(check_enable_dm_unit("sddm").is_ok());
        assert!(check_enable_dm_unit("sshd").is_err());
        assert!(check_switch_dm_unit("sshd").is_err());
    }

    // ── З2: пути ────────────────────────────────────────────────────────

    #[test]
    fn з2_пути_вне_корня_отказ() {
        let root = user_rebuild_root("/home/a");
        assert_eq!(root, "/home/a/.cache/rexon/rebuild");
        assert_eq!(user_rebuild_root("/home/a/"), root);
        for bad in [
            "/tmp/x.deb",
            "/var/cache/pacman/pkg/linux-6.0-1-x86_64.pkg.tar.zst",
            "/home/a/.cache/rexon/rebuild",
            "/home/a/.cache/rexon/rebuild/",
            "/home/a/.cache/rexon/rebuild2/r/x-1-1-x86_64.pkg.tar.zst",
            "/home/a/.cache/rexon/rebuild/../../../../etc/shadow",
            "/home/a/.cache/rexon/rebuild/r/./x",
            "/home/a/.cache/rexon/rebuild//r/x",
            "home/a/.cache/rexon/rebuild/r/x",
            "/home/a/.cache/rexon/rebuild/r/x\n",
            "",
        ] {
            assert!(check_user_path(bad, &root).is_err(), "{bad:?} прошёл");
        }
        assert_eq!(check_user_path("/home/a/.cache/rexon/rebuild/r/f", &root), Ok("r/f"));
        // испорченный корень — тоже «нет»
        assert!(check_user_path("/x/y", "/x/../").is_err());
        assert!(check_user_path("/x/y", "").is_err());
    }

    #[test]
    fn з2_собранный_пакет_только_из_рецепта() {
        let root = user_rebuild_root("/home/a");
        let ok = "/home/a/.cache/rexon/rebuild/pantheon/switchboard-8.0.2-3-x86_64.pkg.tar.zst";
        assert_eq!(check_built_package(ok, &root), Ok("switchboard-8.0.2-3-x86_64.pkg.tar.zst"));
        // файл прямо в корне, не пакет makepkg, скрытый, флаг
        for bad in [
            "/tmp/x.deb",
            "/home/a/.cache/rexon/rebuild/x-1-1-x86_64.pkg.tar.zst",
            "/home/a/.cache/rexon/rebuild/r/x.deb",
            "/home/a/.cache/rexon/rebuild/r/.x.pkg.tar.zst",
            "/home/a/.cache/rexon/rebuild/r/-x.pkg.tar.zst",
            "/home/a/.cache/rexon/rebuild/r/x.pkg.tar.zst.sig",
            "/home/a/.cache/rexon/rebuild/r/x y.pkg.tar.zst",
        ] {
            assert!(check_built_package(bad, &root).is_err(), "{bad} прошёл");
        }
    }

    // ── З3 ──────────────────────────────────────────────────────────────

    #[test]
    fn з3_чужой_процесс_на_канале_отказ() {
        assert_eq!(parse_pkexec_uid("1000"), Some(1000));
        for bad in ["", "-1", "1000 ", "1e3", "0x10", "99999999999"] {
            assert_eq!(parse_pkexec_uid(bad), None, "{bad:?}");
        }
        assert!(check_peer(1000, 4242, 1000, 4242).is_ok());
        assert_eq!(check_peer(1001, 4242, 1000, 4242), Err(Refusal::Peer)); // чужой UID
        assert_eq!(check_peer(1000, 4243, 1000, 4242), Err(Refusal::Peer)); // не родитель
        assert_eq!(check_peer(1000, 1, 1000, 1), Err(Refusal::Peer)); // осиротел — родитель init
        assert_eq!(check_peer(0, 0, 0, 0), Err(Refusal::Peer));
    }

    // ── З5 ──────────────────────────────────────────────────────────────

    #[test]
    fn з5_после_отказа_сырого_pkexec_нет() {
        assert!(raw_fallback_allowed(HelperFailure::Missing));
        assert!(raw_fallback_allowed(HelperFailure::BadOwner));
        assert!(!raw_fallback_allowed(HelperFailure::Refused));
        assert!(!raw_fallback_allowed(HelperFailure::SessionLost));
        assert!(!raw_fallback_allowed(HelperFailure::NotAuthorized));
    }

    // ── З6 ──────────────────────────────────────────────────────────────

    #[test]
    fn з6_жизненно_важное_отказ() {
        for v in [
            "linux", "linux-cachyos", "linux-cachyos-lts", "linux-firmware", "linux-image-amd64",
            "kernel-core", "sys-kernel/gentoo-kernel", "systemd", "systemd-libs", "polkit",
            "polkit-qt6", "pacman", "pacman-mirrorlist", "glibc", "sudo", "sudo-rs",
            "nvidia-utils", "nvidia-dkms", "lib32-nvidia-utils", "mesa", "vulkan-radeon",
            "xf86-video-amdgpu", "amd-ucode", "intel-ucode", "sof-firmware", "firmware-linux",
            "mkinitcpio", "dracut", "grub", "limine", "rexon", "@base",
        ] {
            assert!(is_vital(v), "{v} не в вечном списке");
            assert!(
                matches!(
                    check_remove_packages(PkgFamily::Arch, &s(&[v]), "kde"),
                    Err(Refusal::Vital(_) | Refusal::PackageName(_))
                ),
                "{v} удаляется"
            );
        }
        for ok in ["xfce4-panel", "lightdm", "polkit-gnome", "network-manager-applet"] {
            assert!(!is_vital(ok), "{ok} в вечном списке");
        }
    }

    /// Вечное (и добавка к нему) не задевает ни одного пакета столов: иначе
    /// удаление стола молча оставляло бы половину пакетов.
    #[test]
    fn вечное_не_задевает_столы() {
        for (id, _) in system::DE_DATABASE {
            for fam in ALL_FAMILIES {
                for p in desktop_packages(*fam, id) {
                    assert!(!is_vital(&p), "пакет стола {id}/{} {p} — вечный", fam.as_str());
                }
            }
        }
    }

    #[test]
    fn з6_текущий_стол_отказ() {
        let (cur, other) = two_desktops();
        let arch = PkgFamily::Arch;
        // сам текущий стол — текст ядра
        let r = desktop_removal_set(arch, cur, cur, &qi_none);
        assert!(matches!(r, Err(Refusal::CurrentDesktop(ref t)) if !t.is_empty()), "{r:?}");
        // пакет текущего стола по имени
        let p = system::get_packages(cur, arch)[0].clone();
        assert!(matches!(
            check_remove_packages(arch, std::slice::from_ref(&p), cur),
            Err(Refusal::CurrentDesktopPackage(_))
        ));
        // текущий стол неизвестен — удалять нельзя ничего
        for unknown in ["", "unknown", "evil"] {
            assert_eq!(
                check_remove_packages(arch, &s(&["foo"]), unknown),
                Err(Refusal::CurrentDesktopUnknown)
            );
            assert_eq!(
                desktop_removal_set(arch, other, unknown, &qi_none),
                Err(Refusal::CurrentDesktopUnknown)
            );
        }
        // другой стол — можно, но без общих с текущим пакетов
        let set = desktop_removal_set(arch, other, cur, &qi_none).unwrap();
        let cur_pkgs = desktop_packages(arch, cur);
        assert!(set.iter().all(|p| !cur_pkgs.contains(p)));
        assert!(!set.is_empty());
        // чужое имя пакета — удаляется
        assert!(check_remove_packages(arch, &s(&["some-app"]), cur).is_ok());
    }

    /// Общий экран входа: Cinnamon и Xfce на Arch оба несут `lightdm`. Сидя
    /// в одном, удаление другого `lightdm` не трогает, хотя `pacman -Qi`
    /// говорит, что он никому не нужен.
    #[test]
    fn з6_общий_пакет_текущего_стола_остаётся() {
        let arch = PkgFamily::Arch;
        let with_lightdm: Vec<&str> = system::DE_DATABASE
            .iter()
            .map(|(id, _)| *id)
            .filter(|id| desktop_packages(arch, id).iter().any(|p| p == "lightdm"))
            .collect();
        assert!(with_lightdm.len() >= 2, "{with_lightdm:?}");
        let set = desktop_removal_set(arch, with_lightdm[1], with_lightdm[0], &qi_none).unwrap();
        assert!(!set.iter().any(|p| p == "lightdm"), "{set:?}");
    }

    /// Замыкание ядра: пакет, нужный кому-то вне набора, остаётся, и с ним
    /// всё, что нужно ему.
    #[test]
    fn з6_замыкание_зависимостей() {
        let (cur, other) = two_desktops();
        let arch = PkgFamily::Arch;
        let pkgs: Vec<String> = desktop_packages(arch, other)
            .into_iter()
            .filter(|p| !desktop_packages(arch, cur).contains(p))
            .collect();
        assert!(pkgs.len() >= 2, "{pkgs:?}");
        let (held, dep) = (pkgs[0].clone(), pkgs[1].clone());
        let qi = move |p: &str| -> Option<String> {
            if p == held {
                Some(format!("Depends On      : {dep}\nRequired By     : someone-else\n"))
            } else {
                Some("Depends On      : None\nRequired By     : None\n".into())
            }
        };
        let set = desktop_removal_set(arch, other, cur, &qi).unwrap();
        assert!(!set.contains(&pkgs[0]) && !set.contains(&pkgs[1]), "{set:?}");
    }

    // ── Установка — только из таблиц ────────────────────────────────────

    #[test]
    fn установка_только_из_таблиц() {
        let arch = PkgFamily::Arch;
        let (de, other) = two_desktops();
        let pkgs = system::get_packages(de, arch);
        assert!(check_install(arch, &pkgs, InstallSource::Desktop { de_id: de }).is_ok());
        // чужое имя, имя чужого стола, флаг, путь
        for bad in ["openssh", "-oX", "/tmp/x.deb", "x.deb"] {
            assert!(check_install(arch, &s(&[bad]), InstallSource::Desktop { de_id: de }).is_err(), "{bad}");
        }
        let foreign: Vec<String> = system::get_packages(other, arch)
            .into_iter()
            .filter(|p| !pkgs.contains(p))
            .take(1)
            .collect();
        assert!(matches!(
            check_install(arch, &foreign, InstallSource::Desktop { de_id: de }),
            Err(Refusal::NotInTable(_))
        ));
        assert!(matches!(
            check_install(arch, &s(&["a"]), InstallSource::Desktop { de_id: "evil" }),
            Err(Refusal::UnknownDesktop(_))
        ));
        // экран входа
        assert!(check_install(arch, &s(&["sddm"]), InstallSource::DisplayManager).is_ok());
        assert!(check_install(arch, &s(&["xorg-server"]), InstallSource::DisplayManager).is_err());
        // доустановка стола: свой стол и таблицы compliance, но не чужое
        let repair = InstallSource::DesktopRepair { current_de: de };
        assert!(check_install(arch, &s(&[crate::compliance::AYATANA_PACKAGES[0]]), repair).is_ok());
        assert!(check_install(arch, &pkgs[..1], repair).is_ok());
        assert!(check_install(arch, &s(&["openssh"]), repair).is_err());
        assert_eq!(
            check_install(arch, &s(&["a"]), InstallSource::DesktopRepair { current_de: "unknown" }),
            Err(Refusal::CurrentDesktopUnknown)
        );
        // видеообои и yt-dlp
        assert!(check_install(arch, &s(&["mpv", "mpvpaper"]), InstallSource::VideoDeps).is_ok());
        assert!(check_install(PkgFamily::Debian, &s(&["mpvpaper"]), InstallSource::VideoDeps).is_err());
        assert!(check_install(arch, &s(&["yt-dlp"]), InstallSource::YtDlp).is_ok());
        assert!(check_install(arch, &s(&["youtube-dl"]), InstallSource::YtDlp).is_err());
    }

    /// Каждое имя каждой таблицы установки проходит именную проверку: иначе
    /// помощник отказал бы честной операции.
    #[test]
    fn таблицы_установки_проходят_проверку_имён() {
        for fam in ALL_FAMILIES {
            if check_family(*fam).is_err() {
                continue;
            }
            for (id, _) in system::DE_DATABASE {
                for p in desktop_packages(*fam, id) {
                    assert!(check_package_name(*fam, &p).is_ok(), "{id}/{}: {p}", fam.as_str());
                }
            }
            let (de, _) = two_desktops();
            for src in super::super::op::PkgSource::PLAIN {
                let src = src.install_source(de);
                for p in install_allowlist(*fam, src).unwrap() {
                    assert!(check_package_name(*fam, &p).is_ok(), "{src:?}: {p}");
                }
            }
        }
    }

    /// ⛔ Сторож Б9: таблицы сеанса прав — ровно белый список установки
    /// помощника пакетов ([`plan::pkg_helper_install_allowlist`]). Таблица,
    /// которую помощник пакетов ставит, а сеанс не знает, — честная
    /// установка, которую сеанс откажет без обхода (З5): Conky, апплеты
    /// MATE, оболочки, значки и курсоры, «Как в дистрибутиве».
    #[test]
    fn таблицы_сеанса_совпадают_с_белым_списком_помощника_пакетов() {
        use super::super::op::PkgSource;
        for fam in ALL_FAMILIES {
            if check_family(*fam).is_err() {
                continue;
            }
            let mut ours: HashSet<String> = HashSet::new();
            for (id, _) in system::DE_DATABASE {
                ours.extend(install_allowlist(*fam, InstallSource::Desktop { de_id: id }).unwrap());
                for src in PkgSource::PLAIN {
                    ours.extend(install_allowlist(*fam, src.install_source(id)).unwrap());
                }
            }
            let helper: HashSet<String> = plan::pkg_helper_install_allowlist(*fam).into_iter().collect();
            let missing: Vec<&String> = helper.iter().filter(|p| !ours.contains(*p)).collect();
            assert!(missing.is_empty(), "{}: нет источника сеанса для {missing:?}", fam.as_str());
            // Обратно: всё, что сеанс пропустит, пропускает и помощник пакетов
            // (имена, не прошедшие его проверку имени, не ставит никто).
            let extra: Vec<&String> = ours
                .iter()
                .filter(|p| !helper.contains(*p) && plan::is_safe_pkg_arg(*fam, p))
                .collect();
            assert!(extra.is_empty(), "{}: сеанс шире помощника пакетов: {extra:?}", fam.as_str());
        }
    }

    fn recipe() -> SrcInfo {
        SrcInfo {
            pkgbase: "switchboard-plug-online-accounts".into(),
            pkgnames: vec!["switchboard-plug-online-accounts".into()],
            pkgver: "8.0.2".into(),
            pkgrel: "3".into(),
            depends: s(&["evolution-data-server", "glib2>=2.76"]),
            makedepends: s(&["meson", "vala", "cmake>=3.20"]),
            ..SrcInfo::default()
        }
    }

    #[test]
    fn сборочные_зависимости_только_из_рецепта() {
        let arch = PkgFamily::Arch;
        let r = recipe();
        assert!(check_build_deps(arch, &s(&["meson", "cmake>=3.20", "glib2>=2.76", "base-devel"]), &r).is_ok());
        for bad in ["openssh", "cmake>=3.21", "-oX", "--config=/tmp/x", "/tmp/x.pkg.tar.zst", "cmake>=", "linux"] {
            assert!(check_build_deps(arch, &s(&[bad]), &r).is_err(), "{bad}");
        }
        assert!(matches!(check_build_deps(PkgFamily::Debian, &s(&["meson"]), &r), Err(Refusal::Family(_))));
        assert_eq!(check_build_deps(arch, &[], &r), Err(Refusal::Empty));
        // снять после сборки: только makedepends/base-devel, не depends
        assert!(check_build_only(arch, &s(&["meson", "vala", "base-devel"]), &r).is_ok());
        assert!(check_build_only(arch, &[], &r).is_ok());
        assert!(check_build_only(arch, &s(&["evolution-data-server"]), &r).is_err());
        assert!(check_build_only(arch, &s(&["polkit"]), &r).is_err());
        assert!(check_build_only(arch, &s(&["linux"]), &r).is_err());
    }

    /// Рецепт сам требует жизненно важное в makedepends: снять его всё
    /// равно нельзя.
    #[test]
    fn сборочное_вечное_не_снимается() {
        let mut r = recipe();
        r.makedepends.push("linux-headers".into());
        assert!(check_build_deps(PkgFamily::Arch, &s(&["linux-headers"]), &r).is_ok());
        assert_eq!(
            check_build_only(PkgFamily::Arch, &s(&["linux-headers"]), &r),
            Err(Refusal::Vital("linux-headers".into()))
        );
    }

    #[test]
    fn откат_форков_только_форки_и_их_сток() {
        let qi = |p: &str| -> Option<String> {
            (p == "aeroshell-libplasma").then(|| "Provides        : libplasma=6.4\nReplaces        : plasma-framework\n".into())
        };
        assert!(check_fork_rollback(&s(&["aeroshell-libplasma"]), &s(&["libplasma"]), &qi).is_ok());
        assert!(check_fork_rollback(&s(&["aeroshell-libplasma"]), &[], &qi).is_ok());
        assert!(check_fork_rollback(&s(&["plasma-desktop"]), &[], &qi).is_err());
        assert!(check_fork_rollback(&s(&["aeroshell-libplasma"]), &s(&["openssh"]), &qi).is_err());
        assert!(check_fork_rollback(&s(&["aeroshell-libplasma"]), &s(&["-oX"]), &qi).is_err());
    }

    // ── З7 ──────────────────────────────────────────────────────────────

    #[test]
    fn з7_код_из_темы_plymouth_отказ() {
        for m in crate::installer::PLYMOUTH_SPLASH_MODULES {
            assert!(check_plymouth_module_name(m).is_ok());
        }
        for bad in ["../../../share/plymouth/themes/t/evil", "evil", "/usr/lib/x", "script.so", ""] {
            assert!(matches!(check_plymouth_module_name(bad), Err(Refusal::BootSplashCode(_))), "{bad}");
        }
        for bad in ["evil.so", "lib/evil.so.1", "a/b/x.so"] {
            assert!(check_theme_file_not_code(bad).is_err(), "{bad}");
        }
        assert!(check_theme_file_not_code("images/solar.png").is_ok());
        assert!(check_theme_file_not_code("t.script").is_ok());
    }

    // ── З8 ──────────────────────────────────────────────────────────────

    #[test]
    fn з8_ключи_plm_вне_списка_отказ() {
        let plugins = ["org.kde.image", "org.kde.color"];
        assert!(check_plm_key(&["Greeter"], "ShowClock", &plugins).is_ok());
        assert!(check_plm_key(&["Greeter"], "WallpaperPluginId", &plugins).is_ok());
        assert!(check_plm_key(&["Greeter", "Wallpaper", "org.kde.image"], "Image", &plugins).is_ok());
        for (g, k) in [
            (&["X11"][..], "ServerPath"),
            (&["X11"][..], "ServerArguments"),
            (&["X11"][..], "SessionLogFile"),
            (&["Users"][..], "DefaultPath"),
            (&["Autologin"][..], "User"),
            (&["Greeter"][..], "Image"),
            (&["Greeter", "Wallpaper", "evil.plugin"][..], "Image"),
            (&["Greeter", "Wallpaper", "org.kde.image"][..], "ServerPath"),
        ] {
            assert!(matches!(check_plm_key(g, k, &plugins), Err(Refusal::PlmKey(_))), "{g:?} {k}");
        }
        assert!(check_plm_wallpaper_name("bg.png").is_ok());
        assert!(check_plm_wallpaper_name("../../etc/x").is_err());
    }

    // ── §1.4 п.5 ────────────────────────────────────────────────────────

    #[test]
    fn неизменяемая_система_отказ() {
        for b in [SteamOsBlock::Desktops, SteamOsBlock::LoginScreen, SteamOsBlock::BootSplash, SteamOsBlock::SystemPart] {
            assert!(matches!(check_mutable_system(b, true), Err(Refusal::ReadOnlySystem(ref t)) if !t.is_empty()));
            assert!(check_mutable_system(b, false).is_ok());
        }
    }

    // ── стол набором и образ стола SteamOS ─────────────────────────────

    fn swap(new: &str, installed: &str) -> plan::PkgConflict {
        plan::PkgConflict { new_package: new.into(), installed: installed.into(), ..plan::PkgConflict::default() }
    }

    #[test]
    fn набор_стола_без_пробы() {
        let arch = PkgFamily::Arch;
        let river = s(&["river-classic", "waybar"]);
        assert!(check_de_set(arch, "river", &river, &[], &s(&["river"]), "kde").is_ok());
        // Только pacman; стол — из базы; имена — имена.
        assert!(matches!(check_de_set(PkgFamily::Debian, "river", &river, &[], &[], "kde"), Err(Refusal::Family(_))));
        assert!(matches!(check_de_set(arch, "evil", &river, &[], &[], "kde"), Err(Refusal::UnknownDesktop(_))));
        assert!(matches!(check_de_set(arch, "river", &s(&["-oX"]), &[], &[], "kde"), Err(Refusal::Operand(_))));
        assert_eq!(check_de_set(arch, "river", &[], &[], &[], "kde"), Err(Refusal::Empty));
        // deepin-kwin — ни в каком наборе; у Deepin — без рецептного сеанса и группы.
        assert!(matches!(check_de_set(arch, "river", &s(&["deepin-kwin"]), &[], &[], "kde"), Err(Refusal::NotInTable(_))));
        for bad in ["deepin-session", "deepin", "deepin-kwin"] {
            assert!(matches!(check_de_set(arch, "deepin", &s(&["treeland", bad]), &[], &[], "kde"), Err(Refusal::NotInTable(_))), "{bad}");
        }
        assert!(check_de_set(arch, "deepin", &s(&["treeland", "ddm"]), &[], &[], "kde").is_ok());
        // --asdeps — только из самого набора.
        assert!(matches!(check_de_set(arch, "river", &river, &s(&["linux-headers"]), &[], "kde"), Err(Refusal::NotInTable(_))));
        // Замена: вечное, пакет текущего стола, имя из набора — нет.
        let kde_pkg = desktop_packages(arch, "kde").into_iter().find(|p| !is_vital(p)).unwrap();
        assert!(matches!(check_de_set(arch, "river", &river, &[], &s(&["glibc"]), "kde"), Err(Refusal::Vital(_))));
        assert!(matches!(check_de_set(arch, "river", &river, &[], &[kde_pkg], "kde"), Err(Refusal::CurrentDesktopPackage(_))));
        assert!(matches!(check_de_set(arch, "river", &river, &[], &s(&["waybar"]), "kde"), Err(Refusal::Conflict(_))));
        // Замена под работающим этим же столом и при неизвестном столе — нет.
        assert!(matches!(check_de_set(arch, "river", &river, &[], &s(&["river"]), "river"), Err(Refusal::CurrentDesktop(_))));
        assert_eq!(check_de_set(arch, "river", &river, &[], &s(&["river"]), ""), Err(Refusal::CurrentDesktopUnknown));
    }

    #[test]
    fn набор_стола_по_правилу_ядра() {
        let base = s(&["deepin-anything", "treeland"]);
        let extras = s(&["deepin-anything-dkms", "linux-cachyos-headers"]);
        let mut all = base.clone();
        all.extend(extras.iter().cloned());
        let p = plan::InstallPreview {
            targets: all.clone(),
            deps_only: extras.clone(),
            new_packages: all.clone(),
            ..plan::InstallPreview::default()
        };
        assert!(check_de_set_preview(&all, &base, &extras, &[], Some(&p)).is_ok());
        // Проба молчит — непроверенное не ставится.
        assert_eq!(check_de_set_preview(&all, &base, &[], &[], None), Err(Refusal::SetRule("pacman".into())));
        // Постороннее имя — не из пробы root-стороны.
        let mut evil = all.clone();
        evil.push("nvidia-dkms".into());
        assert_eq!(check_de_set_preview(&evil, &base, &[], &[], Some(&p)), Err(Refusal::NotInTable("nvidia-dkms".into())));
        // --asdeps — только то, что правило ядра считает зависимостью.
        assert!(matches!(check_de_set_preview(&all, &base, &s(&["treeland"]), &[], Some(&p)), Err(Refusal::SetRule(_))));
        // deepin-kwin где угодно в плане — отказ.
        let mut kwin = p.clone();
        kwin.new_packages.push("deepin-kwin".into());
        assert!(matches!(check_de_set_preview(&all, &base, &[], &[], Some(&kwin)), Err(Refusal::SetRule(_))));
        // Замена без конфликта — нет.
        assert!(matches!(check_de_set_preview(&all, &base, &[], &s(&["river"]), Some(&p)), Err(Refusal::Conflict(_))));
    }

    #[test]
    fn замена_только_из_таблицы_замен() {
        let pkgs = s(&["river-classic", "waybar"]);
        let p = plan::InstallPreview {
            targets: pkgs.clone(),
            conflicts: vec![swap("river-classic", "river")],
            ..plan::InstallPreview::default()
        };
        assert!(check_de_set_preview(&pkgs, &pkgs, &[], &s(&["river"]), Some(&p)).is_ok());
        // Без согласия конфликт не снимается.
        assert_eq!(check_de_set_preview(&pkgs, &pkgs, &[], &[], Some(&p)), Err(Refusal::Conflict("river".into())));
        // Лишнее имя к замене — нет, даже если конфликт покрыт.
        assert_eq!(
            check_de_set_preview(&pkgs, &pkgs, &[], &s(&["river", "kwin"]), Some(&p)),
            Err(Refusal::Conflict("kwin".into()))
        );
        // Конфликт не из таблицы замен (deepin-kwin против kwin) — не снимается
        // ничьим «согласием»: --ask=4 снял бы KDE Plasma.
        let foreign = plan::InstallPreview {
            targets: pkgs.clone(),
            conflicts: vec![swap("river-classic", "river"), swap("mutter49", "mutter")],
            ..plan::InstallPreview::default()
        };
        assert!(matches!(
            check_de_set_preview(&pkgs, &pkgs, &[], &s(&["river", "mutter"]), Some(&foreign)),
            Err(Refusal::Conflict(_))
        ));
        // Установленное, которое кто-то требует, тоже не снимается.
        let mut required = p.clone();
        required.conflicts[0].required_by = s(&["river-tools"]);
        assert!(matches!(check_de_set_preview(&pkgs, &pkgs, &[], &s(&["river"]), Some(&required)), Err(Refusal::Conflict(_))));
    }

    #[test]
    fn образ_стола_только_на_steamos() {
        assert!(check_sysext("xfce", false, true, "kde").is_ok());
        assert!(check_sysext("xfce", true, true, "kde").is_ok());
        assert_eq!(check_sysext("xfce", false, false, "kde"), Err(Refusal::NotSteamOs));
        assert!(matches!(check_sysext("mate", false, true, "kde"), Err(Refusal::UnknownDesktop(_))));
        assert!(matches!(check_sysext("xfce;id", false, true, "kde"), Err(Refusal::UnknownDesktop(_))));
        assert!(matches!(check_sysext("xfce", true, true, "xfce"), Err(Refusal::CurrentDesktop(_))));
    }

    #[test]
    fn коды_отказов_различны() {
        let all = [
            Refusal::Empty, Refusal::TooLong(String::new()), Refusal::TooMany(0),
            Refusal::Operand(String::new()), Refusal::PackageName(String::new()),
            Refusal::ItemName(String::new()), Refusal::Family(""),
            Refusal::UnknownDesktop(String::new()), Refusal::NotInTable(String::new()),
            Refusal::BuildDep(String::new()), Refusal::Vital(String::new()),
            Refusal::CurrentDesktopPackage(String::new()), Refusal::CurrentDesktop(String::new()),
            Refusal::CurrentDesktopUnknown, Refusal::Path(String::new()),
            Refusal::OutsideRoot(String::new()), Refusal::PackageFile(String::new()),
            Refusal::DisplayManager(String::new()), Refusal::BootSplashCode(String::new()),
            Refusal::Peer, Refusal::PlmKey(String::new()), Refusal::ReadOnlySystem(String::new()),
            Refusal::Conflict(String::new()), Refusal::SetRule(String::new()), Refusal::NotSteamOs,
            Refusal::NotOrphan(String::new()), Refusal::NotRollback(String::new()),
            Refusal::FontFile(String::new()), Refusal::NotAFont(String::new()),
            Refusal::FontExists(String::new()),
            Refusal::NoSddmUser, Refusal::SddmTheme(String::new()), Refusal::LoginImage(String::new()),
            Refusal::LookFile(String::new()), Refusal::LookValue(String::new()), Refusal::LookElsewhere,
            Refusal::NoSetpriv,
        ];
        let codes: HashSet<&str> = all.iter().map(Refusal::code).collect();
        assert_eq!(codes.len(), all.len());
    }

    #[test]
    fn файл_сборщика_aur() {
        assert_eq!(check_aur_package("/home/u/.cache/yay/t/t-1-1-x86_64.pkg.tar.zst"), Ok("t-1-1-x86_64.pkg.tar.zst"));
        assert_eq!(check_aur_package("/srv/pkgdest/t-1-1-any.pkg.tar"), Ok("t-1-1-any.pkg.tar"));
        for bad in ["", "t-1-1-any.pkg.tar", "/a/../t-1-1-any.pkg.tar", "/a//t-1-1-any.pkg.tar", "/tmp/x.deb", "/a/-x.pkg.tar", "/a/t\n.pkg.tar"] {
            assert!(check_aur_package(bad).is_err(), "{bad:?}");
        }
    }

    /// Что помощнику теперь разрешено со шрифтами: ПОЛОЖИТЬ файл шрифта под
    /// своим именем в `/usr/local/share/fonts` и УДАЛИТЬ файл шрифта оттуда
    /// же (в том числе из подкаталога). Ничего больше: ни другого каталога,
    /// ни пути вместо имени, ни скрытых файлов, ни не-шрифтов по
    /// расширению, ни шрифтов пакетов.
    #[test]
    fn шрифты_для_всех_имя_файла() {
        for ok in ["Inter.ttf", "Мой шрифт.TTF", "a b (1).otf", "NotoSansCJK.ttc", "x.woff2", "t.pcf.gz", "fixed.bdf", "Type1.pfb"] {
            assert_eq!(check_font_file_name(ok), Ok(()), "{ok:?}");
        }
        for bad in [
            "", ".ttf", ".hidden.ttf", "-x.ttf", "a/b.ttf", "../a.ttf", "a..ttf", "ttf", "a.ttf.sh", "a.so", "a.conf",
            "fonts.dir", "a\n.ttf", "a\0.ttf", "a\t.ttf",
        ] {
            assert!(check_font_file_name(bad).is_err(), "{bad:?} прошёл");
        }
        let long = format!("{}.ttf", "а".repeat(200));
        assert!(matches!(check_font_file_name(&long), Err(Refusal::TooLong(_))));
        // Список: пустой, слишком длинный, повтор — отказ.
        assert_eq!(check_font_install(&["a.ttf", "b.otf"]), Ok(()));
        assert_eq!(check_font_install(&[]), Err(Refusal::Empty));
        assert!(matches!(check_font_install(&["a.ttf", "a.ttf"]), Err(Refusal::FontFile(_))));
        assert!(matches!(check_font_install(&["a.ttf", "evil.sh"]), Err(Refusal::FontFile(_))));
        let many: Vec<String> = (0..=MAX_FONT_FILES).map(|i| format!("f{i}.ttf")).collect();
        let many: Vec<&str> = many.iter().map(String::as_str).collect();
        assert_eq!(check_font_install(&many), Err(Refusal::TooMany(MAX_FONT_FILES + 1)));
    }

    #[test]
    fn шрифты_для_всех_удаление_только_из_своего_каталога() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(
            check_font_remove(&s(&["/usr/local/share/fonts/a.ttf", "/usr/local/share/fonts/sub/Мой.otf"])),
            Ok(vec!["a.ttf", "sub/Мой.otf"])
        );
        for bad in [
            // шрифты пакетов, соседи, сам каталог
            "/usr/share/fonts/noto/NotoSans-Regular.ttf",
            "/usr/local/share/fonts2/a.ttf",
            "/usr/local/share/fonts",
            "/usr/local/share/fonts/",
            "/home/u/.local/share/fonts/a.ttf",
            // выход за каталог и мусор в пути
            "/usr/local/share/fonts/../../../etc/shadow",
            "/usr/local/share/fonts/./a.ttf",
            "/usr/local/share/fonts//a.ttf",
            "usr/local/share/fonts/a.ttf",
            "/usr/local/share/fonts/a.ttf\n",
            // не шрифт и служебные файлы fontconfig
            "/usr/local/share/fonts/fonts.dir",
            "/usr/local/share/fonts/fonts.scale",
            "/usr/local/share/fonts/.uuid",
            "/usr/local/share/fonts/sub",
            "/usr/local/share/fonts/run.sh",
            // скрытый подкаталог
            "/usr/local/share/fonts/.cache/a.ttf",
        ] {
            assert!(check_font_remove(&s(&[bad])).is_err(), "{bad:?} прошёл");
        }
        assert_eq!(check_font_remove(&[]), Err(Refusal::Empty));
        assert!(matches!(
            check_font_remove(&s(&["/usr/local/share/fonts/a.ttf", "/usr/local/share/fonts/a.ttf"])),
            Err(Refusal::FontFile(_))
        ));
        let many: Vec<String> = (0..=MAX_FONT_FILES).map(|i| format!("/usr/local/share/fonts/f{i}.ttf")).collect();
        assert_eq!(check_font_remove(&many), Err(Refusal::TooMany(MAX_FONT_FILES + 1)));
        assert_eq!(crate::qt_settings::SHARED_FONT_DIR, "/usr/local/share/fonts");
    }
}
