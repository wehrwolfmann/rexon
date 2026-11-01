//! Операции сеанса прав (пункт Б1 плана, см. [`super`]).
//!
//! [`PrivOp`] — закрытый перечень всего, что Rexon делает от root (сведён
//! по описи всех мест, где код звал root). Каждый вариант несёт
//! ДАННЫЕ операции (семейство, имена пакетов, юнит, категорию), а не строку
//! команды (запрет З1): команду строит тот, кто исполняет.
//!
//! Два исполнителя у одной операции:
//! * сеанс прав — операция уходит помощнику в `Request` (`proto.rs`), он
//!   проверяет её (`validate.rs`) и сам собирает команду из таблиц ядра;
//! * путь «как сейчас» — [`PrivOp::pkexec_argv`]: та же команда, что сегодня
//!   строят прежние сборщики (`system_plan`, `repo_rebuild`, `aur_install`,
//!   `de_apply`, `steamos_sysext`…). Здесь они только ВЫЗЫВАЮТСЯ: второй копии
//!   флагов и путей помощников быть не должно. Путь нужен, когда помощника
//!   сеанса нет вовсе (З5: не при отказе его проверки).
//!
//! # Локальные файлы (З2)
//!
//! Файл или каталог с машины пользователя (собранный пакет, тема, фон экрана
//! входа) — [`LocalFile`]: номер дескриптора в `SCM_RIGHTS` того же кадра и
//! метка. Метка — путь на стороне окна; помощник сеанса по ней ничего не
//! открывает (только текст журнала). Прежние помощники принимают путь —
//! поэтому метку в argv кладёт только путь «как сейчас».

use serde::{Deserialize, Serialize};

use crate::system::PkgFamily;
use crate::system_plan as plan;

/// `PkgFamily` в протоколе — строкой `as_str` (у самого типа есть только
/// `Serialize`; разбор — через `from_value`, чужое слово — ошибка кадра).
mod family_serde {
    use serde::{de::Error, Deserialize, Deserializer, Serialize, Serializer};

    use crate::system::PkgFamily;

    pub fn serialize<S: Serializer>(f: &PkgFamily, ser: S) -> Result<S::Ok, S::Error> {
        f.serialize(ser)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(de: D) -> Result<PkgFamily, D::Error> {
        let s = String::deserialize(de)?;
        PkgFamily::from_value(&s).ok_or_else(|| D::Error::custom(format!("unknown family: {s}")))
    }
}

/// Файл или каталог пользователя, переданный открытым дескриптором (З2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalFile {
    /// Номер дескриптора среди `SCM_RIGHTS` кадра с этой операцией.
    pub slot: u8,
    /// Путь на стороне окна: для журнала и для пути «как сейчас». Помощник
    /// сеанса по нему ничего не открывает.
    pub label: String,
}

/// Файл шрифта для установки «для всех пользователей»
/// ([`PrivOp::FontInstall`]): дескриптор и имя, под которым файл ляжет в
/// [`crate::qt_settings::SHARED_FONT_DIR`]. Имя — только имя файла шрифта
/// ([`super::validate::check_font_file_name`]), путь приёмника помощник
/// строит сам.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FontFile {
    pub source: LocalFile,
    pub name: String,
}

/// Файл настроек стола для экрана входа SDDM ([`PrivOp::SddmLook`]):
/// дескриптор и имя из [`plan::SDDM_LOOK_FILES`] — куда он ляжет в
/// `~sddm/.config`. Путь приёмника помощник строит сам.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LookFile {
    pub source: LocalFile,
    pub name: String,
}

/// По какой таблице ядра идёт установка пакетов ([`PrivOp::PkgInstall`]).
/// Окно называет таблицу, содержимое root-сторона берёт из своего бинаря
/// ([`super::validate::install_allowlist`]): имя вне названной таблицы не
/// пройдёт, даже если оно стоит в другой. Текущий стол для починки окно не
/// передаёт — помощник узнаёт его у logind сам.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum PkgSource {
    /// Набор стола из базы ([`crate::system::DE_DATABASE`]).
    Desktop { de_id: String },
    /// Экран входа ([`plan::DM_PAIRS`]).
    DisplayManager,
    /// «Проверить мой стол»: пакеты ТЕКУЩЕГО стола и таблицы `compliance`.
    DesktopRepair,
    /// Видеозависимости обоев.
    VideoDeps,
    /// yt-dlp для обоев с YouTube.
    YtDlp,
    /// Инструмент подключения оверлея Gentoo
    /// ([`plan::GENTOO_OVERLAY_TOOL_PACKAGE`]).
    GentooTool,
    /// Оболочки стола ([`crate::shell_bundle::all_packages`]).
    Shell,
    /// Программы виджетов Conky и распаковщик 7z
    /// ([`crate::utils::tool_packages_for`]).
    Tools,
    /// Сам Conky ([`crate::de_apply::conky_package`]).
    Conky,
    /// Части раскладок MATE ([`crate::mate_panel::all_part_packages`]).
    MatePanel,
    /// Значки и курсоры тем ([`crate::themes::known_set_packages`]).
    ThemeSets,
    /// Образы «Как в дистрибутиве»
    /// ([`crate::distro_looks::builtin_package_names`]).
    DistroLook,
}

impl PkgSource {
    /// Все таблицы без параметров (кроме [`PkgSource::Desktop`]) — для
    /// сверок полноты: каждая таблица белого списка помощника пакетов
    /// названа здесь.
    pub const PLAIN: &'static [PkgSource] = &[
        PkgSource::DisplayManager,
        PkgSource::DesktopRepair,
        PkgSource::VideoDeps,
        PkgSource::YtDlp,
        PkgSource::GentooTool,
        PkgSource::Shell,
        PkgSource::Tools,
        PkgSource::Conky,
        PkgSource::MatePanel,
        PkgSource::ThemeSets,
        PkgSource::DistroLook,
    ];

    /// Таблица проверки root-стороны; `current_de` — стол, который помощник
    /// узнал сам (для [`PkgSource::DesktopRepair`]).
    pub fn install_source<'a>(&'a self, current_de: &'a str) -> super::validate::InstallSource<'a> {
        use super::validate::InstallSource;
        match self {
            PkgSource::Desktop { de_id } => InstallSource::Desktop { de_id },
            PkgSource::DisplayManager => InstallSource::DisplayManager,
            PkgSource::DesktopRepair => InstallSource::DesktopRepair { current_de },
            PkgSource::VideoDeps => InstallSource::VideoDeps,
            PkgSource::YtDlp => InstallSource::YtDlp,
            PkgSource::GentooTool => InstallSource::GentooTool,
            PkgSource::Shell => InstallSource::Shell,
            PkgSource::Tools => InstallSource::Tools,
            PkgSource::Conky => InstallSource::Conky,
            PkgSource::MatePanel => InstallSource::MatePanel,
            PkgSource::ThemeSets => InstallSource::ThemeSets,
            PkgSource::DistroLook => InstallSource::DistroLook,
        }
    }
}

/// Вид системной темы (помощник `rexon-system-helper`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SystemThemeKind {
    Sddm,
    Plymouth,
}

impl SystemThemeKind {
    /// Слово помощника: `sddm`, `plymouth` (и `apply-…`/`remove-…`).
    pub fn as_str(self) -> &'static str {
        match self {
            SystemThemeKind::Sddm => "sddm",
            SystemThemeKind::Plymouth => "plymouth",
        }
    }
}

/// Семейство для системного помощника: он знает только два пути сборки
/// образа ядра (`mkinitcpio`/`dracut` и `update-initramfs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeFamily {
    Arch,
    Debian,
}

impl ThemeFamily {
    pub fn as_str(self) -> &'static str {
        match self {
            ThemeFamily::Arch => "arch",
            ThemeFamily::Debian => "debian",
        }
    }
}

/// Операции с системной темой (опись Б-1…Б-4, Б-7, Б-8).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum SystemThemeOp {
    /// Положить тему в `/usr/share/<вид>/themes/<theme>` (Plymouth — с
    /// проверкой `ModuleName` внутри помощника). `source` — каталог темы.
    Install {
        kind: SystemThemeKind,
        theme: String,
        source: LocalFile,
        family: ThemeFamily,
    },
    /// «Применить» уже стоящую тему.
    Apply {
        kind: SystemThemeKind,
        theme: String,
        family: ThemeFamily,
    },
    /// «Удалить» тему (не активную и не из пакета системы).
    Remove {
        kind: SystemThemeKind,
        theme: String,
        family: ThemeFamily,
    },
}

/// Операция образа стола на SteamOS (опись А-19).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SysextOp {
    Install,
    Remove,
}

impl SysextOp {
    fn helper_op(self) -> crate::steamos_sysext::HelperOp {
        match self {
            SysextOp::Install => crate::steamos_sysext::HelperOp::Install,
            SysextOp::Remove => crate::steamos_sysext::HelperOp::Remove,
        }
    }
}

/// Чем подключить оверлей Gentoo (опись А-9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GentooOverlayTool {
    Eselect,
    Layman,
}

/// Чем вернуть штатную заставку загрузки (опись А-18).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlymouthTool {
    /// `plymouth-set-default-theme <тема> -R`.
    SetDefaultTheme,
    /// `update-alternatives` + `update-initramfs` (Debian/Ubuntu).
    UpdateAlternatives,
}

/// Привилегированная операция Rexon. Номера в комментариях — места описи v3.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum PrivOp {
    /// Поставить пакеты из репозиториев (Я1/Я3: А-13, А-20, Б-10, Б-11,
    /// Б-14, пакет SDDM в А-6, инструмент оверлея Gentoo в А-9).
    PkgInstall {
        #[serde(with = "family_serde")]
        family: PkgFamily,
        packages: Vec<String>,
        /// Таблица, из которой имена (проверка помощника — только по ней).
        source: PkgSource,
    },
    /// Снять пакеты (А-7 удаление стола; Б-12 «Как в дистрибутиве»).
    PkgRemove {
        #[serde(with = "family_serde")]
        family: PkgFamily,
        packages: Vec<String>,
    },
    /// Поставить стол набором из таблицы DE-помощника (А-1); экран входа
    /// помощник решает сам. `log_label` — журнал пути «как сейчас»; сеанс
    /// шлёт ход операции по сокету.
    DeInstall {
        #[serde(with = "family_serde")]
        family: PkgFamily,
        de_id: String,
        log_label: String,
    },
    /// Поставить стол набором вне таблицы помощника (А-2): DKMS `--asdeps`,
    /// замена конфликтующего, Deepin. Путь «как сейчас» — скрипт обёртки,
    /// а не одна команда (см. [`PrivOp::pkexec_argv`]).
    DeInstallSet {
        #[serde(with = "family_serde")]
        family: PkgFamily,
        de_id: Option<String>,
        packages: Vec<String>,
        deps_only: Vec<String>,
        replace: Vec<String>,
        update_first: bool,
    },
    /// Включить экран входа после установки стола (А-5, вторая половина А-6).
    EnableDm {
        #[serde(with = "family_serde")]
        family: PkgFamily,
        unit: String,
    },
    /// «Сделать основным» экран входа с автооткатом (А-8). Сырого пути нет.
    SwitchDm {
        #[serde(with = "family_serde")]
        family: PkgFamily,
        unit: String,
        greeter: Option<String>,
    },
    /// Сделать сеансом по умолчанию у экрана входа пару X11-сеанса `x11`
    /// на Wayland ([`plan::X11_SESSION_SUCCESSORS`]): только ключи сеанса
    /// SDDM и Plasma Login Manager, указывающие на `x11`; автовход не
    /// включается и не выключается. Что пара установлена и что после правки
    /// экран входа сам `x11` не открывает, помощник от root проверяет сам.
    LoginSessionWayland { x11: String },
    /// Сборочные зависимости пересборки (Я4: А-14…А-16).
    BuildDeps { names: Vec<String> },
    /// Поставить собранный пользователем пакет и снять сборочные (Я4).
    InstallBuilt {
        package: LocalFile,
        build_only: Vec<String>,
        replace: bool,
    },
    /// Сборка из AUR (Я5: А-4, А-12, А-22, Б-10, Б-13). От root здесь только
    /// вызовы pacman у сборщика (пункт Б10); помощнику сеанса не уходит.
    AurInstall {
        helper: String,
        packages: Vec<String>,
        replace_conflicts: bool,
    },
    /// Пересборка AeroShell (А-23); то же, что [`PrivOp::AurInstall`].
    AeroShellRebuild {
        helper: String,
        packages: Vec<String>,
        chroot: bool,
    },
    /// Шим `rexon-sudo-shim` (пункт Б10, [`super::aur`]): зависимости сборки
    /// AUR из официальных репозиториев — `pacman -S --needed --asdeps`.
    /// Помощник сам сверяет каждое имя с sync-базой своей машины.
    AurDeps { names: Vec<String> },
    /// Шим: собранные paru/yay пакеты — `pacman -U` копий root. `allowed` —
    /// что окно просило собрать в этой операции: `pkgname` из `.PKGINFO`
    /// каждой копии обязан быть оттуда.
    AurInstallBuilt {
        packages: Vec<LocalFile>,
        allowed: Vec<String>,
        replace: bool,
    },
    /// Шим: снять сборочные зависимости после сборки (`--removemake`).
    /// Помощник снимает только сироты-зависимости (`pacman -Qqdt`).
    AurDropMake { names: Vec<String> },
    /// Разложить каталог в системный по категории (Я2: Б-5, Б-6, А-10, А-21).
    /// Приёмник — константа ядра по `category`.
    Stage {
        category: String,
        name: String,
        source: LocalFile,
    },
    /// Системные темы SDDM/Plymouth (Я6, Б-1…Б-4, Б-7, Б-8).
    SystemTheme(SystemThemeOp),
    /// Собрать/снять стол образом sysext на SteamOS (А-19).
    Sysext { op: SysextOp, de_id: String },
    /// `emerge --sync` одного оверлея (А-9).
    GentooSync { repo: String },
    /// Подключить оверлей Gentoo (А-9).
    GentooOverlay {
        overlay: String,
        tool: GentooOverlayTool,
    },
    /// Убрать старое системное переопределение схем MATE (А-11).
    MateGschemaCleanup,
    /// Снять форки Plasma и вернуть штатные пакеты (А-17): решение окна
    /// ([`plan::plasma_fork_split`]) — что снять и что поставить.
    PlasmaForksReset {
        removable: Vec<String>,
        stock: Vec<String>,
    },
    /// Вернуть штатную заставку загрузки (А-18).
    PlymouthReset { target: String, tool: PlymouthTool },
    /// Снять файлы Rexon из `/etc/sddm.conf.d` (А-18). `files` — имена файлов.
    SddmReset { files: Vec<String> },
    /// Удалить пакет-сироту из окна сторожа обновлений
    /// ([`crate::update_guard::remove_orphans_live`]): член набора, ушедший
    /// из репозиториев (`kwin-x11` 6.7.5 при Plasma 6.8). Только pacman.
    /// Что каждое имя — установленная сирота члена набора, помощник от root
    /// проверяет сам ([`crate::update_guard::orphan_candidate`], sync-база
    /// своей машины); окну не верит.
    OrphanRemove { names: Vec<String> },
    /// Вернуть прежние версии набора сторожа обновлений из кэша pacman
    /// ([`crate::update_guard::rollback_live`]). `files` — пути файлов кэша,
    /// как их нашло окно. Только pacman. Что каждый файл — пакет root в
    /// каталоге кэша этой машины, что версии (по `.PKGINFO`) строго меньше
    /// установленных и что это ровно откат набора находки, помощник от root
    /// проверяет сам; окну не верит.
    CacheRollback { files: Vec<String> },
    /// Поставить шрифты для всех пользователей («Управление шрифтами» →
    /// «Для всех пользователей», как группа «Системные» у KDE): копии root в
    /// [`crate::qt_settings::SHARED_FONT_DIR`], затем `fc-cache`. Что каждый
    /// файл — шрифт (`fc-query` по копии root), что имя — имя файла шрифта и
    /// что такого файла там ещё нет (чужое не перезаписывается), помощник
    /// проверяет сам.
    FontInstall { fonts: Vec<FontFile> },
    /// Удалить шрифты, поставленные для всех пользователей: только файлы
    /// внутри [`crate::qt_settings::SHARED_FONT_DIR`] (шрифты пакетов в
    /// `/usr/share/fonts` — нельзя), затем `fc-cache`.
    FontRemove { files: Vec<String> },
    /// Фон темы экрана входа SDDM («Изменить фон» модуля KDE): картинка
    /// ложится в каталог темы под именем [`plan::SDDM_BACKGROUND_STEM`],
    /// имя — в `<ConfigFile>.user` темы. `image: None` — вернуть фон самой
    /// темы. Что тема стоит в системном каталоге и принимает фон, что файл —
    /// PNG или JPEG (по сигнатуре копии root), помощник проверяет сам.
    SddmBackground { theme: String, image: Option<LocalFile> },
    /// Перенести оформление стола на экран входа SDDM («Применить параметры
    /// Plasma» модуля KDE): ключи [`plan::SDDM_LOOK_KEYS`] — в настройки SDDM
    /// по правилу [`plan::plan_sddm_look`], файлы — в `~sddm/.config` от имени
    /// `sddm`. `look: None` — «Сбросить» (файлов тогда нет).
    SddmLook { look: Option<plan::SddmLook>, files: Vec<LookFile> },
    /// «Показывать часы» у темы экрана входа SDDM (галочка модуля KDE):
    /// `showClock` — в `<ConfigFile>.user` темы. Что тема стоит в системном
    /// каталоге и принимает свои настройки, помощник проверяет сам, как у
    /// [`PrivOp::SddmBackground`].
    SddmClock { theme: String, show: bool },
}

/// Что из прежних помощников есть на машине — вход пути «как сейчас».
/// Заполняет вызывающий теми же пробами, что и сегодня
/// (`pkg_helper_available`, `stage_helper_available`, …).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Helpers {
    /// `rexon-pkg-helper` и его политика на месте.
    pub pkg: bool,
    /// `rexon-stage-helper`.
    pub stage: bool,
    /// `rexon-de-helper`.
    pub de: bool,
    /// `rexon-system-helper` (+ `pkexec`) — путь берётся из
    /// [`crate::installer::system_helper_path`].
    pub system: bool,
    /// Текст установленного `rexon-sysext-helper` (SteamOS), если есть.
    pub sysext_installed: Option<String>,
    /// Код языка интерфейса для sysext-помощника (`ru`).
    pub lang: String,
}

impl PrivOp {
    /// argv пути «как сейчас» — то, что сегодня строит соответствующее место
    /// кода (прежний именованный помощник, иначе сырой `pkexec`).
    ///
    /// `None` — одной команды нет:
    /// * сборщик отказал (пусто, имя не прошло проверку, помощника нет там,
    ///   где сырого пути нет по З5 — `switch-dm`, системные темы);
    /// * сырой путь сегодня — скрипт во временном файле (`DeInstallSet`,
    ///   `Stage` и `SystemTheme::Install` без помощника, `MateGschemaCleanup`):
    ///   его текст — [`PrivOp::fallback_script`] или прежний сборщик плана.
    ///
    /// У NixOS установка и удаление идут без `pkexec` (профиль пользователя) —
    /// как и сегодня.
    pub fn pkexec_argv(&self, helpers: &Helpers) -> Option<Vec<String>> {
        match self {
            PrivOp::PkgInstall { family, packages, .. } => plan::install_argv(*family, packages, helpers.pkg),
            PrivOp::PkgRemove { family, packages } => remove_argv(*family, packages, helpers.pkg),
            PrivOp::DeInstall { family, de_id, log_label } => {
                de_install_argv(*family, de_id, log_label, helpers.de)
            }
            PrivOp::DeInstallSet { .. } => None,
            PrivOp::EnableDm { family, unit } => {
                let (package, _) = plan::DM_PAIRS.iter().find(|(_, u)| u == unit)?;
                plan::dm_enable_step(*family, &[package.to_string()], false, helpers.pkg, false)
                    .map(|(_, cmd)| cmd.argv)
            }
            PrivOp::SwitchDm { family, unit, greeter } => {
                plan::switch_dm_cmd(*family, unit, greeter.as_deref(), helpers.pkg)
            }
            // Сырого пути нет: что меняется ровно ключ сеанса и что пара
            // установлена, без помощника сеанса проверить некому.
            PrivOp::LoginSessionWayland { .. } => None,
            // Пересборка: с помощником пакетов — его `build-deps`/`install-built`
            // (один пароль на оба шага), без него — `pkexec /bin/bash`.
            // `helpers.pkg` для `InstallBuilt` вызывающий ставит так же, как
            // `repo_rebuild`: помощник есть И файл лежит в каталоге пересборок.
            PrivOp::BuildDeps { names } => {
                if names.is_empty() {
                    return None;
                }
                Some(if helpers.pkg {
                    crate::repo_rebuild::helper_deps_argv(&crate::repo_rebuild::pkg_helper_prefix(), names)
                } else {
                    crate::repo_rebuild::deps_install_argv(names)
                })
            }
            PrivOp::InstallBuilt { package, build_only, replace } => Some(if helpers.pkg {
                crate::repo_rebuild::helper_install_argv(
                    &crate::repo_rebuild::pkg_helper_prefix(),
                    &package.label,
                    build_only,
                    *replace,
                )
            } else {
                crate::repo_rebuild::install_argv(&package.label, build_only, *replace)
            }),
            PrivOp::AurInstall { helper, packages, replace_conflicts } => {
                if packages.is_empty() {
                    return None;
                }
                Some(crate::aur_install::install_argv(helper, packages, *replace_conflicts))
            }
            PrivOp::AeroShellRebuild { helper, packages, chroot } => {
                crate::aeroshell::rebuild_command(helper, packages, *chroot)
            }
            // Прежний путь шима — `pkexec` той же строки pacman, что дал
            // сборщик (решает сам шим по ответу окна, [`super::aur`]).
            PrivOp::AurDeps { .. } | PrivOp::AurInstallBuilt { .. } | PrivOp::AurDropMake { .. } => None,
            // Прежнего пути нет намеренно: «сирота ли это» проверить от root
            // без помощника сеанса некому, а сырой `pacman -Rns` поверх
            // слова окна — ровно то, от чего проверка защищает.
            PrivOp::OrphanRemove { .. } => None,
            // То же: «откат ли это ровно набора» без помощника сеанса
            // проверить некому, сырой `pacman -U` поверх путей окна — то,
            // от чего проверка защищает.
            PrivOp::CacheRollback { .. } => None,
            // Прежнего пути у шрифтов «для всех» нет намеренно: что файл —
            // шрифт и что чужое не перезаписывается, без помощника сеанса
            // проверить некому, а сырой `pkexec cp` поверх пути окна — то,
            // от чего проверка защищает.
            PrivOp::FontInstall { .. } | PrivOp::FontRemove { .. } => None,
            // Экран входа SDDM: что картинка — картинка, что ключи — ровно
            // ключи переноса и что файлы в доме `sddm` пишутся от его имени,
            // без помощника сеанса проверить некому (З5).
            PrivOp::SddmBackground { .. } | PrivOp::SddmLook { .. } | PrivOp::SddmClock { .. } => None,
            PrivOp::Stage { category, name, source } => plan::privileged_stage_cmd(
                category,
                name,
                &source.label,
                true,
                Some(&source.label),
                helpers.stage,
            ),
            PrivOp::SystemTheme(op) => system_theme_argv(op, helpers.system),
            PrivOp::Sysext { op, de_id } => Some(crate::steamos_sysext::helper_argv(
                op.helper_op(),
                de_id,
                helpers.sysext_installed.as_deref(),
                &helpers.lang,
            )),
            PrivOp::GentooSync { repo } => plan::sync_repo_cmd(PkgFamily::Gentoo, repo),
            PrivOp::GentooOverlay { overlay, tool } => gentoo_overlay_argv(overlay, *tool),
            PrivOp::MateGschemaCleanup => None,
            PrivOp::PlasmaForksReset { removable, stock } => plasma_forks_reset_argv(removable, stock),
            PrivOp::PlymouthReset { target, tool } => {
                let theme_file_exists = |_: &str| true;
                match plan::plymouth_reset_plan(
                    Some(target),
                    None,
                    *tool == PlymouthTool::SetDefaultTheme,
                    *tool == PlymouthTool::UpdateAlternatives,
                    &theme_file_exists,
                ) {
                    plan::PlymouthReset::Run { cmd, .. } => Some(cmd.argv),
                    _ => None,
                }
            }
            PrivOp::SddmReset { files } => {
                let reset = plan::SddmResetPlan { to_remove: files.clone(), default_theme: None };
                match plan::sddm_reset_command(&reset, plan::SDDM_CONF_D) {
                    plan::SddmReset::Run { cmd, .. } => Some(cmd.argv),
                    plan::SddmReset::AlreadyStock => None,
                }
            }
        }
    }

    /// Скрипт сырого пути там, где он сегодня идёт `pkexec bash <файл>` и
    /// весь текст выводится из данных операции: `(префикс файла, текст)`.
    pub fn fallback_script(&self) -> Option<(&'static str, String)> {
        match self {
            PrivOp::MateGschemaCleanup => Some(("rexon_gschema_", plan::mate_gschema_cleanup_script())),
            _ => None,
        }
    }

    /// Файлы, которые клиент прикладывает к кадру `SCM_RIGHTS`, в порядке
    /// появления в операции.
    pub fn local_files(&self) -> Vec<&LocalFile> {
        match self {
            PrivOp::InstallBuilt { package, .. } => vec![package],
            PrivOp::AurInstallBuilt { packages, .. } => packages.iter().collect(),
            PrivOp::Stage { source, .. } => vec![source],
            PrivOp::SystemTheme(SystemThemeOp::Install { source, .. }) => vec![source],
            PrivOp::FontInstall { fonts } => fonts.iter().map(|f| &f.source).collect(),
            PrivOp::SddmBackground { image: Some(image), .. } => vec![image],
            PrivOp::SddmLook { files, .. } => files.iter().map(|f| &f.source).collect(),
            _ => Vec::new(),
        }
    }
}

/// Удаление: именованный путь помощника, иначе сырой `pkexec` поверх
/// [`plan::remove_cmd`]; NixOS — без `pkexec` (как в [`plan::remove_plan`]).
fn remove_argv(family: PkgFamily, packages: &[String], helper: bool) -> Option<Vec<String>> {
    if packages.is_empty() {
        return None;
    }
    let base = plan::remove_cmd(family, packages).ok()?;
    if family == PkgFamily::Nix {
        return Some(base);
    }
    if let Some(named) = plan::privileged_pkg_cmd(family, "remove", packages, helper) {
        return Some(named);
    }
    let mut argv = vec!["pkexec".to_string()];
    argv.extend(base);
    Some(argv)
}

/// Команда DE-помощника — из того же плана установки стола, что и сегодня
/// ([`plan::install_batch_plan_with_deps`] с набором из таблицы помощника).
/// Набор вне таблицы помощник не ставит — тогда `None`.
fn de_install_argv(family: PkgFamily, de_id: &str, log: &str, helper: bool) -> Option<Vec<String>> {
    let table = crate::system::de_entry_packages_for(de_id, family.as_str())?;
    let packages: Vec<String> = table.iter().map(|p| p.to_string()).collect();
    let detector = crate::system::DistroDetector::from_os_release_files(None, None);
    let batch = plan::install_batch_plan_with_deps(
        family,
        &detector,
        &packages,
        &[],
        &[],
        &[],
        None,
        false,
        Some(de_id),
        helper,
        "",
        log,
    )
    .ok()?;
    if !batch.used_de_helper {
        return None;
    }
    batch.steps.into_iter().find_map(|s| match s {
        plan::PlanStep::Run { id: plan::RunStepId::PkgMain, cmd, .. } => Some(cmd.argv),
        _ => None,
    })
}

/// Системный помощник. Сырого пути у «Применить»/«Удалить» нет; у установки
/// он — скрипт (`ext_manager::sddm_fallback_script`), не одна команда.
fn system_theme_argv(op: &SystemThemeOp, helper: bool) -> Option<Vec<String>> {
    if !helper {
        return None;
    }
    let path = crate::installer::system_helper_path();
    let lang = crate::system_l10n::lang_args(path);
    Some(match op {
        SystemThemeOp::Apply { kind, theme, family } => {
            crate::de_apply::system_apply_argv(path, &lang, kind.as_str(), theme, family.as_str())
        }
        SystemThemeOp::Remove { kind, theme, family } => {
            crate::de_apply::system_remove_argv(path, &lang, kind.as_str(), theme, family.as_str())
        }
        // ⚠️ Отдельного сборщика у этой строки нет: она набрана на месте в
        // `appearance_installer::plan_root_install_with` и дважды в
        // `ext_manager` (SDDM, Plymouth). Сверка — тест ниже.
        SystemThemeOp::Install { kind, theme, source, family } => {
            let mut v = vec!["pkexec".to_string(), path.to_string()];
            v.extend(lang);
            v.extend([
                kind.as_str().to_string(),
                theme.clone(),
                source.label.clone(),
                family.as_str().to_string(),
            ]);
            v
        }
    })
}

/// ⚠️ Литералы `system_de_exec::ensure_gentoo_overlay` — сборщика нет
/// (метод закрытый); сверка — тест ниже.
fn gentoo_overlay_argv(overlay: &str, tool: GentooOverlayTool) -> Option<Vec<String>> {
    // Только оверлеи из таблицы ядра — те же, что подключает установка стола.
    if !plan::GENTOO_OVERLAYS.iter().any(|(_, o)| *o == overlay) {
        return None;
    }
    let argv: &[&str] = match tool {
        GentooOverlayTool::Eselect => &["pkexec", "eselect", "repository", "enable"],
        GentooOverlayTool::Layman => &["pkexec", "layman", "-a"],
    };
    let mut v: Vec<String> = argv.iter().map(|s| s.to_string()).collect();
    v.push(overlay.to_string());
    Some(v)
}

/// Команда отката форков — из [`plan::plasma_fork_reset_plan`]: решение
/// окна (`removable`, `stock`) подаётся ему пробами. Сток приходит через
/// подставные форки `\0<сток>` с `Provides: <сток>`; снимаемые форки идут
/// своими именами без метаданных — у них, как и при решении окна, кандидатов
/// нет. Несогласованное решение план отвергнет (`Blocked`) — `None`.
fn plasma_forks_reset_argv(removable: &[String], stock: &[String]) -> Option<Vec<String>> {
    let forks: Vec<String> = removable
        .iter()
        .cloned()
        .chain(stock.iter().map(|s| format!("\0{s}")))
        .collect();
    let qi = |f: &str| f.strip_prefix('\0').map(|s| format!("Provides        : {s}\n"));
    let exists = |name: &str| Some(stock.iter().any(|s| s == name));
    match plan::plasma_fork_reset_plan(&forks, &qi, &exists) {
        plan::PlasmaForkReset::Run { cmd, removable: r, stock: s, .. }
            if r.as_slice() == removable && s.as_slice() == stock =>
        {
            Some(cmd.argv)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn file(slot: u8, label: &str) -> LocalFile {
        LocalFile { slot, label: label.to_string() }
    }

    fn helpers(on: bool) -> Helpers {
        Helpers { pkg: on, stage: on, de: on, system: on, sysext_installed: None, lang: "ru".into() }
    }

    /// По одному образцу на каждый вариант (и на каждый вид вложенных).
    fn samples() -> Vec<PrivOp> {
        vec![
            PrivOp::PkgInstall { family: PkgFamily::Arch, packages: names(&["sddm", "yt-dlp"]), source: PkgSource::DisplayManager },
            PrivOp::PkgInstall { family: PkgFamily::Arch, packages: names(&["mate-panel"]), source: PkgSource::Desktop { de_id: "mate".into() } },
            PrivOp::PkgInstall { family: PkgFamily::Arch, packages: names(&["xdg-utils"]), source: PkgSource::DesktopRepair },
            PrivOp::PkgInstall { family: PkgFamily::Arch, packages: names(&["ffmpeg"]), source: PkgSource::VideoDeps },
            PrivOp::PkgInstall { family: PkgFamily::Arch, packages: names(&["yt-dlp"]), source: PkgSource::YtDlp },
            PrivOp::PkgRemove { family: PkgFamily::Debian, packages: names(&["mate-desktop"]) },
            PrivOp::DeInstall { family: PkgFamily::Arch, de_id: "pantheon".into(), log_label: "/tmp/a.log".into() },
            PrivOp::DeInstallSet {
                family: PkgFamily::Arch,
                de_id: Some("deepin".into()),
                packages: names(&["deepin", "deepin-anything-dkms"]),
                deps_only: names(&["deepin-anything-dkms"]),
                replace: names(&["old"]),
                update_first: true,
            },
            PrivOp::DeInstallSet {
                family: PkgFamily::Nix,
                de_id: None,
                packages: vec![],
                deps_only: vec![],
                replace: vec![],
                update_first: false,
            },
            PrivOp::EnableDm { family: PkgFamily::Arch, unit: "plasmalogin".into() },
            PrivOp::SwitchDm { family: PkgFamily::Arch, unit: "lightdm".into(), greeter: Some("slick-greeter".into()) },
            PrivOp::SwitchDm { family: PkgFamily::Fedora, unit: "gdm".into(), greeter: None },
            PrivOp::LoginSessionWayland { x11: "plasmax11".into() },
            PrivOp::BuildDeps { names: names(&["vala", "cmake>=3.20"]) },
            PrivOp::InstallBuilt { package: file(0, "/home/u/.cache/rexon/rebuild/x.pkg.tar.zst"), build_only: names(&["vala"]), replace: true },
            PrivOp::AurInstall { helper: "paru".into(), packages: names(&["mate-tweak"]), replace_conflicts: false },
            PrivOp::AeroShellRebuild { helper: "yay".into(), packages: names(&["aeroshell-workspace-git"]), chroot: true },
            PrivOp::AurDeps { names: names(&["cmake", "ninja"]) },
            PrivOp::AurInstallBuilt {
                packages: vec![file(0, "/home/u/.cache/paru/clone/t/t-1-1-any.pkg.tar.zst"), file(1, "/home/u/.cache/paru/clone/t/u-1-1-any.pkg.tar.zst")],
                allowed: names(&["t", "u"]),
                replace: false,
            },
            PrivOp::AurDropMake { names: names(&["cmake"]) },
            PrivOp::Stage { category: "gdm-bg".into(), name: "rexon-login".into(), source: file(1, "/tmp/rexon_stage_x") },
            PrivOp::SystemTheme(SystemThemeOp::Install {
                kind: SystemThemeKind::Plymouth,
                theme: "spinner2".into(),
                source: file(0, "/tmp/t"),
                family: ThemeFamily::Debian,
            }),
            PrivOp::SystemTheme(SystemThemeOp::Apply { kind: SystemThemeKind::Sddm, theme: "breeze".into(), family: ThemeFamily::Arch }),
            PrivOp::SystemTheme(SystemThemeOp::Remove { kind: SystemThemeKind::Sddm, theme: "old".into(), family: ThemeFamily::Arch }),
            PrivOp::Sysext { op: SysextOp::Install, de_id: "xfce".into() },
            PrivOp::Sysext { op: SysextOp::Remove, de_id: "xfce".into() },
            PrivOp::GentooSync { repo: "mate".into() },
            PrivOp::GentooOverlay { overlay: "x".into(), tool: GentooOverlayTool::Eselect },
            PrivOp::GentooOverlay { overlay: "x".into(), tool: GentooOverlayTool::Layman },
            PrivOp::MateGschemaCleanup,
            PrivOp::PlasmaForksReset { removable: names(&["a"]), stock: names(&["libplasma"]) },
            PrivOp::PlymouthReset { target: "bgrt".into(), tool: PlymouthTool::SetDefaultTheme },
            PrivOp::PlymouthReset { target: "bgrt".into(), tool: PlymouthTool::UpdateAlternatives },
            PrivOp::SddmReset { files: names(&["zz-rexon-theme.conf"]) },
            PrivOp::OrphanRemove { names: names(&["kwin-x11"]) },
            PrivOp::CacheRollback { files: names(&["/var/cache/pacman/pkg/qt6-base-6.11.2-1-x86_64.pkg.tar.zst"]) },
            PrivOp::FontInstall {
                fonts: vec![
                    FontFile { source: file(0, "/home/u/Загрузки/Мой шрифт.ttf"), name: "Мой шрифт.ttf".into() },
                    FontFile { source: file(1, "/home/u/b.otf"), name: "b.otf".into() },
                ],
            },
            PrivOp::FontRemove { files: names(&["/usr/local/share/fonts/b.otf"]) },
            PrivOp::SddmBackground { theme: "breeze".into(), image: Some(file(0, "/home/u/Картинки/фон.jpg")) },
            PrivOp::SddmBackground { theme: "breeze".into(), image: None },
            PrivOp::SddmLook {
                look: Some(plan::SddmLook {
                    cursor_theme: Some("Bibata-Modern-Ice".into()),
                    cursor_size: Some(24),
                    font: Some("Noto Sans,10,-1,5,400,0,0,0,0,0,0,0,0,0,0,1".into()),
                    numlock: Some(plan::SddmNumlock::On),
                    dpi: Some(120),
                }),
                files: vec![
                    LookFile { source: file(0, "/home/u/.config/kdeglobals"), name: "kdeglobals".into() },
                    LookFile { source: file(1, "/home/u/.config/fontconfig/fonts.conf"), name: "fontconfig/fonts.conf".into() },
                ],
            },
            PrivOp::SddmLook { look: None, files: Vec::new() },
            PrivOp::SddmClock { theme: "breeze".into(), show: false },
        ]
    }

    #[test]
    fn every_variant_round_trips() {
        for op in samples() {
            let text = serde_json::to_string(&op).unwrap();
            let back: PrivOp = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{text}: {e}"));
            assert_eq!(back, op, "{text}");
        }
    }

    #[test]
    fn foreign_words_are_refused() {
        // Чужое семейство, лишнее поле, неизвестная операция — ошибка разбора.
        for bad in [
            r#"{"pkg_install":{"family":"windows","packages":["a"],"source":"yt_dlp"}}"#,
            r#"{"pkg_install":{"family":"arch","packages":["a"],"source":"yt_dlp","argv":["sh"]}}"#,
            // Без таблицы, с чужой таблицей, с текущим столом от окна.
            r#"{"pkg_install":{"family":"arch","packages":["a"]}}"#,
            r#"{"pkg_install":{"family":"arch","packages":["a"],"source":"anything"}}"#,
            r#"{"pkg_install":{"family":"arch","packages":["a"],"source":{"desktop_repair":{"current_de":"x"}}}}"#,
            r#"{"pkg_install":{"family":"arch","packages":["a"],"source":{"desktop":{"de_id":"x","extra":1}}}}"#,
            r#"{"build_deps":{"names":["a"],"path":"/tmp/x"}}"#,
            r#"{"stage":{"category":"gdm-bg","name":"n","source":{"slot":0,"label":"/tmp/x","path":"/etc"}}}"#,
            r#"{"run_shell":{"cmd":"id"}}"#,
            // Шрифты: путь приёмника от окна, лишнее поле у файла.
            r#"{"font_install":{"fonts":[{"source":{"slot":0,"label":"/tmp/a.ttf"},"name":"a.ttf"}],"dest":"/usr/share/fonts"}}"#,
            r#"{"font_install":{"fonts":[{"source":{"slot":0,"label":"/tmp/a.ttf"},"name":"a.ttf","dir":"/etc"}]}}"#,
            r#"{"font_remove":{"files":["/usr/local/share/fonts/a.ttf"],"root":"/"}}"#,
            // Экран входа SDDM: путь темы или приёмника от окна, чужой ключ
            // настроек, NumLock не из перечня.
            r#"{"sddm_background":{"theme":"breeze","image":null,"dir":"/usr/share/sddm/themes/breeze"}}"#,
            r#"{"sddm_look":{"look":{"server_path":"/tmp/x"},"files":[]}}"#,
            r#"{"sddm_look":{"look":{"numlock":"maybe"},"files":[]}}"#,
            r#"{"sddm_look":{"look":null,"files":[{"source":{"slot":0,"label":"/h/k"},"name":"kdeglobals","dest":"/etc"}]}}"#,
        ] {
            assert!(serde_json::from_str::<PrivOp>(bad).is_err(), "{bad}");
        }
    }

    fn run_argv(steps: &[plan::PlanStep], id: plan::RunStepId) -> Vec<String> {
        steps
            .iter()
            .find_map(|s| match s {
                plan::PlanStep::Run { id: i, cmd, .. } if *i == id => Some(cmd.argv.clone()),
                _ => None,
            })
            .expect("шаг есть")
    }

    #[test]
    fn pkg_install_matches_install_plan() {
        for fam in [PkgFamily::Arch, PkgFamily::Debian, PkgFamily::Fedora, PkgFamily::Nix] {
            for on in [true, false] {
                let pkgs = names(&["sddm", "yt-dlp"]);
                let plan = plan::install_plan(fam, &pkgs, false, None, on).unwrap();
                let op = PrivOp::PkgInstall { family: fam, packages: pkgs, source: PkgSource::DisplayManager };
                assert_eq!(op.pkexec_argv(&helpers(on)), Some(run_argv(&plan.steps, plan::RunStepId::PkgInstall)), "{fam:?} {on}");
            }
        }
        // Починка «Проверить мой стол» — та же команда.
        for on in [true, false] {
            let op = PrivOp::PkgInstall { family: PkgFamily::Arch, packages: names(&["xdg-utils"]), source: PkgSource::DesktopRepair };
            assert_eq!(op.pkexec_argv(&helpers(on)), crate::compliance::pkg_install_arch("xdg-utils", on));
        }
        let empty = PrivOp::PkgInstall { family: PkgFamily::Arch, packages: vec![], source: PkgSource::YtDlp };
        assert_eq!(empty.pkexec_argv(&helpers(true)), None);
    }

    #[test]
    fn pkg_remove_matches_remove_plan() {
        for fam in [PkgFamily::Arch, PkgFamily::Debian, PkgFamily::Nix] {
            for on in [true, false] {
                let pkgs = names(&["mate-desktop", "caja"]);
                let plan = plan::remove_plan(fam, &pkgs, &|_| None, &|_| None, &|_| true, on).unwrap();
                let op = PrivOp::PkgRemove { family: fam, packages: pkgs };
                assert_eq!(op.pkexec_argv(&helpers(on)), Some(run_argv(&plan.steps, plan::RunStepId::PkgRemove)), "{fam:?} {on}");
            }
        }
    }

    #[test]
    fn de_install_matches_batch_plan() {
        let det = crate::system::DistroDetector::from_os_release_files(Some("ID=arch\n"), None);
        let table = names(crate::system::de_entry_packages_for("pantheon", "arch").unwrap());
        let batch = plan::install_batch_plan_with_deps(
            PkgFamily::Arch, &det, &table, &[], &[], &[], None, true, Some("pantheon"), true, "/tmp/a.sh", "/tmp/a.log",
        )
        .unwrap();
        assert!(batch.used_de_helper);
        let op = PrivOp::DeInstall { family: PkgFamily::Arch, de_id: "pantheon".into(), log_label: "/tmp/a.log".into() };
        assert_eq!(op.pkexec_argv(&helpers(true)), Some(run_argv(&batch.steps, plan::RunStepId::PkgMain)));
        // Без помощника и для стола вне его таблицы (Deepin) одной команды нет.
        assert_eq!(op.pkexec_argv(&helpers(false)), None);
        let deepin = PrivOp::DeInstall { family: PkgFamily::Arch, de_id: "deepin".into(), log_label: "/tmp/a.log".into() };
        assert_eq!(deepin.pkexec_argv(&helpers(true)), None);
        let set = &samples()[7];
        assert_eq!(set.pkexec_argv(&helpers(true)), None);
    }

    #[test]
    fn enable_dm_matches_dm_enable_step_and_ensure_dm() {
        for on in [true, false] {
            for (pkg, unit) in plan::DM_PAIRS {
                let (_, cmd) = plan::dm_enable_step(PkgFamily::Arch, &[pkg.to_string()], false, on, false).unwrap();
                let op = PrivOp::EnableDm { family: PkgFamily::Arch, unit: unit.to_string() };
                assert_eq!(op.pkexec_argv(&helpers(on)), Some(cmd.argv), "{unit} {on}");
            }
            // «Нет экрана входа → SDDM»: обе команды плана.
            match plan::ensure_dm_plan(PkgFamily::Arch, false, true, on) {
                plan::EnsureDmPlan::Install { install, enable, .. } => {
                    let en = PrivOp::EnableDm { family: PkgFamily::Arch, unit: "sddm".into() };
                    assert_eq!(en.pkexec_argv(&helpers(on)), Some(enable.argv));
                    let inst = PrivOp::PkgInstall { family: PkgFamily::Arch, packages: names(&["sddm"]), source: PkgSource::DisplayManager };
                    assert_eq!(inst.pkexec_argv(&helpers(on)), Some(install.argv));
                }
                _ => panic!("ожидалась установка SDDM"),
            }
        }
        let unknown = PrivOp::EnableDm { family: PkgFamily::Arch, unit: "evil".into() };
        assert_eq!(unknown.pkexec_argv(&helpers(true)), None);
    }

    #[test]
    fn switch_dm_matches_switch_dm_cmd() {
        for on in [true, false] {
            for (unit, greeter) in [("lightdm", Some("slick-greeter")), ("sddm", None), ("gdm", Some("x"))] {
                let op = PrivOp::SwitchDm { family: PkgFamily::Arch, unit: unit.into(), greeter: greeter.map(String::from) };
                assert_eq!(op.pkexec_argv(&helpers(on)), plan::switch_dm_cmd(PkgFamily::Arch, unit, greeter, on));
            }
        }
        // Сырого пути нет (З5).
        let op = PrivOp::SwitchDm { family: PkgFamily::Arch, unit: "sddm".into(), greeter: None };
        assert_eq!(op.pkexec_argv(&helpers(false)), None);
    }

    #[test]
    fn rebuild_steps_match_repo_rebuild() {
        // Те же начала, что `repo_rebuild::rebuild_ctx_live` кладёт в
        // `RebuildCtx::pkg_helper`, и те же сборщики, что зовёт пересборка.
        let prefix = crate::repo_rebuild::pkg_helper_prefix();
        assert_eq!(&prefix[..2], &["pkexec".to_string(), plan::pkg_helper_path().to_string()]);
        let deps = names(&["vala", "cmake>=3.20"]);
        let op = PrivOp::BuildDeps { names: deps.clone() };
        assert_eq!(op.pkexec_argv(&helpers(true)), Some(crate::repo_rebuild::helper_deps_argv(&prefix, &deps)));
        assert_eq!(op.pkexec_argv(&helpers(false)), Some(crate::repo_rebuild::deps_install_argv(&deps)));
        assert_eq!(PrivOp::BuildDeps { names: vec![] }.pkexec_argv(&helpers(true)), None);
        let f = "/home/u/.cache/rexon/rebuild/x.pkg.tar.zst";
        for replace in [true, false] {
            let op = PrivOp::InstallBuilt { package: file(0, f), build_only: deps.clone(), replace };
            assert_eq!(
                op.pkexec_argv(&helpers(true)),
                Some(crate::repo_rebuild::helper_install_argv(&prefix, f, &deps, replace))
            );
            assert_eq!(
                op.pkexec_argv(&helpers(false)),
                Some(crate::repo_rebuild::install_argv(f, &deps, replace))
            );
        }
    }

    #[test]
    fn pkg_source_maps_to_validate_tables() {
        use super::super::validate::InstallSource;
        let cases = [
            (PkgSource::Desktop { de_id: "mate".into() }, InstallSource::Desktop { de_id: "mate" }),
            (PkgSource::DisplayManager, InstallSource::DisplayManager),
            (PkgSource::DesktopRepair, InstallSource::DesktopRepair { current_de: "xfce" }),
            (PkgSource::VideoDeps, InstallSource::VideoDeps),
            (PkgSource::YtDlp, InstallSource::YtDlp),
            (PkgSource::GentooTool, InstallSource::GentooTool),
            (PkgSource::Shell, InstallSource::Shell),
            (PkgSource::Tools, InstallSource::Tools),
            (PkgSource::Conky, InstallSource::Conky),
            (PkgSource::MatePanel, InstallSource::MatePanel),
            (PkgSource::ThemeSets, InstallSource::ThemeSets),
            (PkgSource::DistroLook, InstallSource::DistroLook),
        ];
        // Каждая таблица без параметров — в перечне сверок.
        for (src, _) in &cases[1..] {
            assert!(PkgSource::PLAIN.contains(src), "{src:?}");
        }
        assert_eq!(PkgSource::PLAIN.len(), cases.len() - 1);
        for (src, want) in cases {
            assert_eq!(src.install_source("xfce"), want);
        }
    }

    #[test]
    fn aur_matches_aur_install_and_aeroshell() {
        for helper in ["yay", "paru", "trizen"] {
            for replace in [true, false] {
                let pkgs = names(&["mate-tweak", "brisk-menu"]);
                let op = PrivOp::AurInstall { helper: helper.into(), packages: pkgs.clone(), replace_conflicts: replace };
                assert_eq!(op.pkexec_argv(&helpers(true)), Some(crate::aur_install::install_argv(helper, &pkgs, replace)));
            }
            let op = PrivOp::AurInstall { helper: helper.into(), packages: names(&["t"]), replace_conflicts: false };
            assert_eq!(op.pkexec_argv(&helpers(true)), Some(crate::themes::aur_install_cmd(helper, "t").argv));
            for chroot in [true, false] {
                let pkgs = names(&["aeroshell-workspace-git"]);
                let op = PrivOp::AeroShellRebuild { helper: helper.into(), packages: pkgs.clone(), chroot };
                assert_eq!(op.pkexec_argv(&helpers(true)), crate::aeroshell::rebuild_command(helper, &pkgs, chroot));
            }
        }
    }

    #[test]
    fn stage_matches_privileged_stage_cmd() {
        for on in [true, false] {
            for (cat, name) in [("gdm-bg", "rexon-login"), ("mate-layouts", "rexon"), ("bogus", "x"), ("gdm-bg", "../x")] {
                let op = PrivOp::Stage { category: cat.into(), name: name.into(), source: file(0, "/tmp/rexon_stage_a") };
                assert_eq!(
                    op.pkexec_argv(&helpers(on)),
                    plan::privileged_stage_cmd(cat, name, "/tmp/rexon_stage_a", true, Some("/tmp/rexon_stage_a"), on),
                    "{cat} {name} {on}"
                );
            }
        }
        // Метка вне /tmp — отказ того же сборщика.
        let op = PrivOp::Stage { category: "gdm-bg".into(), name: "n".into(), source: file(0, "/etc") };
        assert_eq!(op.pkexec_argv(&helpers(true)), None);
    }

    #[test]
    fn system_theme_matches_de_apply_and_root_install() {
        let path = crate::installer::system_helper_path();
        let lang = crate::system_l10n::lang_args(path);
        for kind in [SystemThemeKind::Sddm, SystemThemeKind::Plymouth] {
            for fam in [ThemeFamily::Arch, ThemeFamily::Debian] {
                let apply = PrivOp::SystemTheme(SystemThemeOp::Apply { kind, theme: "t".into(), family: fam });
                assert_eq!(
                    apply.pkexec_argv(&helpers(true)),
                    Some(crate::de_apply::system_apply_argv(path, &lang, kind.as_str(), "t", fam.as_str()))
                );
                let remove = PrivOp::SystemTheme(SystemThemeOp::Remove { kind, theme: "t".into(), family: fam });
                assert_eq!(
                    remove.pkexec_argv(&helpers(true)),
                    Some(crate::de_apply::system_remove_argv(path, &lang, kind.as_str(), "t", fam.as_str()))
                );
                assert_eq!(apply.pkexec_argv(&helpers(false)), None);

                // Установка — против плана «Оформления» (строка набрана там на месте).
                let root = "/tmp/rexon_x/mytheme";
                let fs = crate::installed_scan::FsSnapshot::from_json(&serde_json::json!({
                    "dirs": { root: ["mytheme.plymouth", "metadata.desktop"] },
                    "files": { format!("{root}/metadata.desktop"): "X-KDE-PluginInfo-Name=mytheme\n" },
                }));
                let mut cat = crate::models::ThemeCategory::new(0, "k", "n", "");
                cat.apply_kind = kind.as_str().to_string();
                cat.install_dir = format!("/usr/share/{}/themes", kind.as_str());
                let env = crate::appearance_installer::RootEnv {
                    is_plasmalogin: false,
                    has_pkexec: true,
                    system_helper_path: path,
                    system_helper_exists: true,
                    is_steamos_or_immutable: false,
                    is_steamos: false,
                    family_is_debian: fam == ThemeFamily::Debian,
                };
                let want = crate::appearance_installer::plan_root_install_with(&env, &fs, &cat, root, &|_| false)
                    .unwrap_or_else(|_| panic!("план установки"))
                    .argv;
                let install = PrivOp::SystemTheme(SystemThemeOp::Install {
                    kind,
                    theme: "mytheme".into(),
                    source: file(0, root),
                    family: fam,
                });
                assert_eq!(install.pkexec_argv(&helpers(true)), Some(want), "{kind:?} {fam:?}");
                assert_eq!(install.pkexec_argv(&helpers(false)), None);
            }
        }
    }

    #[test]
    fn sysext_matches_helper_argv() {
        let text = crate::steamos_sysext::helper_script();
        for installed in [None, Some(text.clone()), Some("old".to_string())] {
            for (op, hop) in [
                (SysextOp::Install, crate::steamos_sysext::HelperOp::Install),
                (SysextOp::Remove, crate::steamos_sysext::HelperOp::Remove),
            ] {
                let h = Helpers { sysext_installed: installed.clone(), lang: "ru".into(), ..Helpers::default() };
                let priv_op = PrivOp::Sysext { op, de_id: "xfce".into() };
                assert_eq!(
                    priv_op.pkexec_argv(&h),
                    Some(crate::steamos_sysext::helper_argv(hop, "xfce", installed.as_deref(), "ru"))
                );
            }
        }
    }

    #[test]
    fn gentoo_matches_sync_and_overlay_literals() {
        let op = PrivOp::GentooSync { repo: "mate".into() };
        assert_eq!(op.pkexec_argv(&helpers(true)), plan::sync_repo_cmd(PkgFamily::Gentoo, "mate"));
        let bad = PrivOp::GentooSync { repo: "--evil".into() };
        assert_eq!(bad.pkexec_argv(&helpers(true)), plan::sync_repo_cmd(PkgFamily::Gentoo, "--evil"));
        // Литералы `system_de_exec::ensure_gentoo_overlay` (метод закрытый —
        // сверка с его текстом, пока строки не вынесены в сборщик).
        let (_, overlay) = plan::GENTOO_OVERLAYS[0];
        let es = PrivOp::GentooOverlay { overlay: overlay.into(), tool: GentooOverlayTool::Eselect };
        assert_eq!(es.pkexec_argv(&helpers(true)), Some(names(&["pkexec", "eselect", "repository", "enable", overlay])));
        let la = PrivOp::GentooOverlay { overlay: overlay.into(), tool: GentooOverlayTool::Layman };
        assert_eq!(la.pkexec_argv(&helpers(true)), Some(names(&["pkexec", "layman", "-a", overlay])));
        let foreign = PrivOp::GentooOverlay { overlay: "not-in-table".into(), tool: GentooOverlayTool::Eselect };
        assert_eq!(foreign.pkexec_argv(&helpers(true)), None);
    }

    #[test]
    fn mate_gschema_is_a_script() {
        let op = PrivOp::MateGschemaCleanup;
        assert_eq!(op.pkexec_argv(&helpers(true)), None);
        assert_eq!(op.fallback_script(), Some(("rexon_gschema_", plan::mate_gschema_cleanup_script())));
        assert_eq!(samples()[0].fallback_script(), None);
    }

    #[test]
    fn plasma_forks_reset_matches_plan() {
        // Форк-провайдер (сток по Provides), форк по запасной карте и
        // автономный форк.
        let forks = names(&["aeroshell-kwin-git", "aeroshell-libplasma", "aeroshell-workspace-git"]);
        let qi = |f: &str| match f {
            "aeroshell-kwin-git" => Some("Provides        : kwin\n".to_string()),
            _ => None,
        };
        let exists = |n: &str| Some(matches!(n, "kwin" | "libplasma"));
        let plan::PlasmaForkReset::Run { cmd, removable, stock, .. } = plan::plasma_fork_reset_plan(&forks, &qi, &exists) else {
            panic!("ожидался откат");
        };
        assert!(!removable.is_empty() && !stock.is_empty(), "{removable:?} {stock:?}");
        let op = PrivOp::PlasmaForksReset { removable: removable.clone(), stock: stock.clone() };
        assert_eq!(op.pkexec_argv(&helpers(true)), Some(cmd.argv));
        // Только снять / только поставить.
        for (r, s) in [(removable.clone(), vec![]), (vec![], stock.clone())] {
            let op = PrivOp::PlasmaForksReset { removable: r, stock: s };
            assert!(op.pkexec_argv(&helpers(true)).is_some());
        }
        let nothing = PrivOp::PlasmaForksReset { removable: vec![], stock: vec![] };
        assert_eq!(nothing.pkexec_argv(&helpers(true)), None);
    }

    #[test]
    fn recovery_resets_match_plans() {
        for (tool, sdt, ua) in [
            (PlymouthTool::SetDefaultTheme, true, false),
            (PlymouthTool::UpdateAlternatives, false, true),
        ] {
            let plan::PlymouthReset::Run { cmd, .. } = plan::plymouth_reset_plan(Some("bgrt"), Some("other"), sdt, ua, &|_| true)
            else {
                panic!("ожидался запуск");
            };
            let op = PrivOp::PlymouthReset { target: "bgrt".into(), tool };
            assert_eq!(op.pkexec_argv(&helpers(true)), Some(cmd.argv));
        }
        let conf = vec![("zz-rexon-theme.conf".to_string(), "[Theme]\nCurrent=x\n".to_string())];
        let reset = plan::sddm_reset_plan(&conf, &[], None);
        let plan::SddmReset::Run { cmd, removed, .. } = plan::sddm_reset_command(&reset, plan::SDDM_CONF_D) else {
            panic!("ожидалось снятие файла Rexon");
        };
        let op = PrivOp::SddmReset { files: removed };
        assert_eq!(op.pkexec_argv(&helpers(true)), Some(cmd.argv));
        let none = PrivOp::SddmReset { files: vec![] };
        assert_eq!(none.pkexec_argv(&helpers(true)), None);
    }

    /// Шрифты «для всех» — только через помощника сеанса: сырого `pkexec`
    /// нет ни с прежними помощниками, ни без них (З5).
    #[test]
    fn fonts_have_no_raw_path() {
        for op in samples().iter().filter(|o| matches!(o, PrivOp::FontInstall { .. } | PrivOp::FontRemove { .. })) {
            assert_eq!(op.pkexec_argv(&helpers(true)), None, "{op:?}");
            assert_eq!(op.pkexec_argv(&helpers(false)), None, "{op:?}");
            assert_eq!(op.fallback_script(), None, "{op:?}");
        }
    }

    /// Экран входа SDDM — тоже только через помощника сеанса (З5).
    #[test]
    fn sddm_greeter_has_no_raw_path() {
        let ops: Vec<PrivOp> = samples()
            .into_iter()
            .filter(|o| matches!(o, PrivOp::SddmBackground { .. } | PrivOp::SddmLook { .. } | PrivOp::SddmClock { .. }))
            .collect();
        assert_eq!(ops.len(), 5);
        for op in &ops {
            assert_eq!(op.pkexec_argv(&helpers(true)), None, "{op:?}");
            assert_eq!(op.pkexec_argv(&helpers(false)), None, "{op:?}");
            assert_eq!(op.fallback_script(), None, "{op:?}");
        }
    }

    #[test]
    fn local_files_are_listed() {
        let with_files: Vec<usize> = samples().iter().map(|o| o.local_files().len()).collect();
        // InstallBuilt, Stage, SystemTheme::Install, SddmBackground с
        // картинкой — по файлу; AurInstallBuilt, FontInstall и SddmLook —
        // по два.
        assert_eq!(with_files.iter().sum::<usize>(), 10);
    }
}
