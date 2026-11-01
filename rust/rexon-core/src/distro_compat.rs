//! Совместимость с российскими/китайскими/индийскими дистрибутивами —
//! порт `rexon/distro_compat.py`.
//!
//! Чистые данные + логика: таблицы дистрибутивов (`DISTRO_INFO`), маппинги
//! пакетов DE для ALT/ROSA, фирменные апплеты ROSA и класс `DistroCompat`
//! (предупреждения, подбор пакетов, проверка возможности установки, бейдж).
//! Правильность транскрипции таблиц и всех веток логики проверяется
//! побайтовой сверкой с самим Python (golden-тест `distro_compat_parity.rs`).
//!
//! Побочные эффекты Python (`_is_zps_enabled` — lsmod+файл, `is_apt_rpm_system`
//! — which, `detect_alt_variant` — /etc/os-release) вынесены в тонкие обёртки,
//! а их детерминированная суть портирована чистыми функциями
//! (`zps_enabled_from`, `is_apt_rpm`, `detect_alt_variant_from`). Состояние
//! ЗПС в методы `DistroCompat` передаётся параметром `zps_enabled` — как
//! `settings.rs` выносит предикат `command_exists` параметром.
//!
//! Решение «можно ли ставить DE» живёт в ОДНОМ месте — `install_reason`. Из
//! него собираются обе формы: короткая строка для журнала
//! (`check_install_possible`, историческая, её сверяет записанный эталон) и
//! человеческое объяснение для окна установки (`InstallReason::title` /
//! `problem` / `advice` — на языке каталога перевода, 29.09.2026). Развести
//! их на две копии веток нельзя: они разошлись бы молча (правило #12).
//! Короткая строка для журнала переводу не подлежит и остаётся русской.

use serde::Serialize;

use crate::i18n::{n_, tr, trf};

// `PkgFamily` живёт в `system.rs` — как в Python: system.py определяет enum,
// distro_compat.py его импортирует. Реэкспорт сохраняет прежние пути
// (`distro_compat::PkgFamily`) для существующего кода и тестов.
pub use crate::system::PkgFamily;

/// Информация о дистрибутиве (значение `DISTRO_INFO[id]`).
///
/// Порядок полей совпадает с порядком ключей Python-словаря — от него зависит
/// порядок в `json.dumps(get_info())` (сверяется побайтово).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct DistroInfo {
    pub name: &'static str,
    pub family: PkgFamily,
    pub country: &'static str,
    pub notes: &'static str,
    pub has_de_repos: bool,
    pub warnings: &'static [&'static str],
}

/// `DISTRO_INFO`: os-release ID → информация. Порядок вставки важен для
/// `get_supported_*_distros` (итерируют `DISTRO_INFO.items()`).
pub const DISTRO_INFO: &[(&str, DistroInfo)] = &[
    ("astra", DistroInfo {
        name: "Astra Linux",
        family: PkgFamily::Debian,
        country: "RU",
        notes: "Debian-based. SE: ЗПС ограничивает установку неподписанных пакетов.",
        has_de_repos: false,
        warnings: &[
            n_("Astra Linux SE runs a closed software environment (ZPS)."),
            n_("Other desktop environments can only be installed on Astra Linux CE or with the closed software environment turned off."),
            n_("The stock desktop is Fly. GNOME/KDE/XFCE/MATE are not available in the standard repositories."),
        ],
    }),
    ("osnova", DistroInfo {
        name: "ОСнова (НППКТ)",
        family: PkgFamily::Debian,
        country: "RU",
        notes: "Debian 10 based. Закрытые репозитории, сертификация ФСТЭК.",
        has_de_repos: false,
        warnings: &[
            n_("OSnova is a closed distribution certified by FSTEC."),
            n_("Its repositories are not public."),
            n_("Rexon can manage packages, but the sources of desktop environments are limited."),
        ],
    }),
    ("alt", DistroInfo {
        name: "ALT Linux",
        family: PkgFamily::Alt,
        country: "RU",
        notes: "APT-RPM: команды apt-get, но пакеты RPM. Не путать с Debian!",
        has_de_repos: true,
        warnings: &[
            n_("ALT Linux uses APT-RPM: apt-get with RPM packages."),
            n_("Desktop environment package names differ from Debian and Fedora."),
        ],
    }),
    ("altlinux", DistroInfo {
        name: "ALT Linux",
        family: PkgFamily::Alt,
        country: "RU",
        notes: "Синоним alt.",
        has_de_repos: true,
        warnings: &[],
    }),
    ("simply", DistroInfo {
        name: "Simply Linux",
        family: PkgFamily::Alt,
        country: "RU",
        notes: "ALT-based, для домашних пользователей. XFCE по умолчанию.",
        has_de_repos: true,
        warnings: &[],
    }),
    ("rosa", DistroInfo {
        name: "ROSA Linux",
        family: PkgFamily::Fedora,
        country: "RU",
        notes: "dnf с собственными именами пакетов (task-kde, task-gnome).",
        has_de_repos: true,
        warnings: &[
            n_("ROSA Linux uses its own task-* metapackages."),
            n_("The standard Fedora comps groups (@kde-desktop) may not work."),
            n_("Rexon will try Fedora-compatible commands, but the results may differ."),
        ],
    }),
    ("redos", DistroInfo {
        name: "РЕД ОС",
        family: PkgFamily::Fedora,
        country: "RU",
        notes: "RHEL/CentOS-based. dnf, близкие к RHEL имена пакетов.",
        has_de_repos: true,
        warnings: &[
            n_("RED OS is certified by FSTEC. Adding third-party repositories may void the certification."),
        ],
    }),
    ("calculate", DistroInfo {
        name: "Calculate Linux",
        family: PkgFamily::Gentoo,
        country: "RU",
        notes: "Полная совместимость с Gentoo. Portage/emerge, те же имена пакетов.",
        has_de_repos: true,
        warnings: &[],
    }),
    ("ubuntukylin", DistroInfo {
        name: "Ubuntu Kylin",
        family: PkgFamily::Debian,
        country: "CN",
        notes: "Официальный флейвор Ubuntu. ID=ubuntu. Рабочий стол UKUI.",
        has_de_repos: true,
        warnings: &[],
    }),
    ("boss", DistroInfo {
        name: "BOSS Linux (Bharat OS)",
        family: PkgFamily::Debian,
        country: "IN",
        notes: "Debian-based. C-DAC India. Рабочий стол Cinnamon по умолчанию. apt/dpkg, стандартные Debian-пакеты.",
        has_de_repos: true,
        warnings: &[],
    }),
    ("nixos", DistroInfo {
        name: "NixOS",
        family: PkgFamily::Nix,
        country: "US",
        notes: "Декларативная система. Рабочий стол задаётся в /etc/nixos/configuration.nix, Rexon выдаёт готовый snippet и инструкцию nixos-rebuild switch.",
        has_de_repos: true,
        warnings: &[
            n_("NixOS uses a declarative model: Rexon doesn't install desktop environments with nix-env commands, it gives you a snippet for /etc/nixos/configuration.nix instead."),
            n_("After adding the snippet, run: sudo nixos-rebuild switch"),
        ],
    }),
    ("steamos", DistroInfo {
        name: "SteamOS",
        family: PkgFamily::Arch,
        country: "US",
        // Steam Deck и Steam Machine — одна и та же ОС (`ID=steamos`).
        // Обновление заменяет системный раздел целиком и стирает всё, что
        // поставлено поверх, поэтому столы ставятся не пакетами, а образами
        // systemd-sysext (`crate::steamos_sysext`, 25.10.2026).
        notes: "Arch-based, корень только для чтения, обновление заменяет образ целиком. Рабочие столы Rexon ставит образами-расширениями systemd-sysext.",
        has_de_repos: true,
        // Тексты для человека: английский — исходник каталога перевода,
        // переводятся на месте показа (`get_warnings`).
        warnings: &[STEAMOS_SUMMARY_WORKS, STEAMOS_SUMMARY_DESKTOPS],
    }),
];

/// Пакеты DE одного дистрибутива: список пакетов + дисплей-менеджер.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct DePackages {
    pub packages: &'static [&'static str],
    pub dm: &'static str,
}

/// `ALT_DE_PACKAGES`: имена пакетов DE для ALT Linux (собственная база).
/// Порядок важен для сообщения `', '.join(ALT_DE_PACKAGES.keys())`.
pub const ALT_DE_PACKAGES: &[(&str, DePackages)] = &[
    ("kde", DePackages { packages: &["kde5", "plasma5-workspace", "konsole", "dolphin", "sddm"], dm: "sddm" }),
    ("gnome", DePackages { packages: &["gnome-shell", "gnome-control-center", "gnome-tweaks", "nautilus", "gdm"], dm: "gdm" }),
    ("xfce", DePackages { packages: &["xfce4", "xfce4-goodies", "lightdm", "lightdm-gtk-greeter"], dm: "lightdm" }),
    ("mate", DePackages { packages: &["mate", "mate-terminal", "plank", "lightdm", "lightdm-gtk-greeter"], dm: "lightdm" }),
    ("cinnamon-mint", DePackages { packages: &["cinnamon", "nemo", "gnome-terminal", "lightdm", "lightdm-gtk-greeter"], dm: "lightdm" }),
    ("budgie", DePackages { packages: &["budgie-desktop", "nautilus", "lightdm", "lightdm-gtk-greeter"], dm: "lightdm" }),
    // Тайловый WM: DM не бандлим — экран входа общесистемный, его ставит
    // install_de → ensure_display_manager (Python). `dm` — справочный фолбэк.
    ("hyprland", DePackages { packages: &["hyprland", "waybar", "wofi", "kitty", "grim", "slurp", "wl-clipboard"], dm: "sddm" }),
    ("lxqt", DePackages { packages: &["lxqt-session", "lxqt-panel", "lxqt-config", "lxqt-policykit", "lxqt-runner", "lxqt-notificationd", "pcmanfm-qt", "qterminal", "openbox", "sddm"], dm: "sddm" }),
];

/// `ROSA_DE_PACKAGES`: собственные метапакеты task-* для ROSA Linux.
pub const ROSA_DE_PACKAGES: &[(&str, DePackages)] = &[
    ("kde", DePackages { packages: &["task-kde5", "sddm"], dm: "sddm" }),
    ("gnome", DePackages { packages: &["task-gnome", "gdm"], dm: "gdm" }),
    ("xfce", DePackages { packages: &["task-xfce", "lightdm", "lightdm-gtk-greeter"], dm: "lightdm" }),
    ("mate", DePackages { packages: &["task-mate", "lightdm", "lightdm-gtk-greeter"], dm: "lightdm" }),
    ("lxqt", DePackages { packages: &["task-lxqt", "sddm"], dm: "sddm" }),
];

/// `ROSA_BRANDING_PACKAGES`: опциональные фирменные апплеты ROSA поверх DE.
pub const ROSA_BRANDING_PACKAGES: &[(&str, &[&str])] = &[
    ("kde", &["simplewelcome", "klook"]),
];

/// Найти запись в упорядоченной таблице по ключу (аналог `dict.get`).
fn lookup<'a, V>(table: &'a [(&str, V)], key: &str) -> Option<&'a V> {
    table.iter().find(|(k, _)| *k == key).map(|(_, v)| v)
}

/// Входные данные детектора (то, что `DistroCompat` берёт из `DistroDetector`
/// в Python: `distro_id`, `family`, `pretty_name` и поля os-release `_info`).
#[derive(Debug, Clone)]
pub struct Detector {
    pub distro_id: String,
    pub family: PkgFamily,
    pub pretty_name: String,
    /// os-release VARIANT_ID (для детекта Astra SE).
    pub variant_id: String,
    /// os-release NAME (для детекта Astra SE).
    pub name: String,
}

/// Расширенная проверка совместимости дистрибутива — порт класса
/// `DistroCompat`. Держит данные детектора и найденную запись `DISTRO_INFO`.
pub struct DistroCompat {
    detector: Detector,
    info: Option<DistroInfo>,
}

impl DistroCompat {
    /// Создать из детектора: `info = DISTRO_INFO.get(distro_id)`.
    pub fn new(detector: Detector) -> Self {
        let info = lookup(DISTRO_INFO, &detector.distro_id).copied();
        Self { detector, info }
    }

    /// Является ли дистрибутив российским (`country == "RU"`).
    pub fn is_russian_distro(&self) -> bool {
        self.info.map(|i| i.country == "RU").unwrap_or(false)
    }

    /// Название дистрибутива (из базы или `pretty_name` детектора).
    pub fn distro_name(&self) -> String {
        match self.info {
            Some(i) => i.name.to_string(),
            None => self.detector.pretty_name.clone(),
        }
    }

    /// Есть ли дистрибутив в базе совместимости.
    pub fn is_known(&self) -> bool {
        self.info.is_some()
    }

    /// Доступны ли DE в штатных репозиториях (по умолчанию — да).
    pub fn has_de_repos(&self) -> bool {
        self.info.map(|i| i.has_de_repos).unwrap_or(true)
    }

    /// Полная информация о дистрибутиве.
    pub fn get_info(&self) -> Option<DistroInfo> {
        self.info
    }

    /// SteamOS ли это — Steam Deck ИЛИ Steam Machine. Решает только
    /// `ID=steamos` из os-release: у обеих машин он один (образ ОС общий), а
    /// `VARIANT_ID` у Steam Machine не подтверждён на устройстве, и на него
    /// полагаться нельзя. Родня по `ID_LIKE` (HoloISO, Bazzite) сюда не
    /// попадает: у них своё устройство системы.
    pub fn is_steamos(&self) -> bool {
        is_steamos_id(&self.detector.distro_id)
    }

    /// Является ли система Astra Linux SE (по os-release NAME/VARIANT_ID).
    ///
    /// Повторяет подстрочные проверки Python: `"special" in name`,
    /// `"smolensk" in variant`, `"se" in variant` (регистронезависимо).
    /// Публичен для моста rexon-py: по нему шим сохраняет ленивость проверки
    /// ЗПС 1-в-1 с Python (колбэк зовётся только на Astra SE).
    pub fn is_astra_se(&self) -> bool {
        let variant = self.detector.variant_id.to_lowercase();
        let name = self.detector.name.to_lowercase();
        name.contains("special") || variant.contains("smolensk") || variant.contains("se")
    }

    /// Предупреждения для пользователя. `zps_enabled` — состояние ЗПС
    /// (в Python `_is_zps_enabled()`; здесь параметр, т.к. это subprocess+fs).
    pub fn get_warnings(&self, zps_enabled: bool) -> Vec<String> {
        let mut warnings: Vec<String> = Vec::new();

        if let Some(i) = self.info {
            // Строки таблицы помечены `n_`: переводятся здесь, на месте показа.
            warnings.extend(i.warnings.iter().map(|s| tr(s)));
        }

        if self.detector.distro_id == "astra" && self.is_astra_se() {
            warnings.push(tr(
                "Astra Linux Special Edition detected. The closed software environment may block package installation.",
            ));
            if zps_enabled {
                warnings.push(format!(
                    "⚠️ {}",
                    tr("The closed software environment (ZPS) is ON. Installing unsigned packages is blocked.")
                ));
            }
        }

        if self.detector.family == PkgFamily::Alt {
            warnings.push(format!(
                "ℹ️ {}",
                tr("APT-RPM is in use. Desktop environment packages are adapted for ALT Linux.")
            ));
        }

        warnings
    }

    /// Скорректированный список пакетов DE (None → использовать общий PackageDB).
    pub fn get_de_packages(&self, de_id: &str) -> Option<Vec<String>> {
        if self.detector.family == PkgFamily::Alt {
            if let Some(p) = lookup(ALT_DE_PACKAGES, de_id) {
                return Some(p.packages.iter().map(|s| s.to_string()).collect());
            }
        }
        if self.detector.distro_id == "rosa" {
            if let Some(p) = lookup(ROSA_DE_PACKAGES, de_id) {
                return Some(p.packages.iter().map(|s| s.to_string()).collect());
            }
        }
        None
    }

    /// Опциональные фирменные апплеты ROSA поверх DE (пусто, если не ROSA).
    pub fn get_rosa_branding(&self, de_id: &str) -> Vec<String> {
        if self.detector.distro_id != "rosa" {
            return Vec::new();
        }
        lookup(ROSA_BRANDING_PACKAGES, de_id)
            .map(|v| v.iter().map(|s| s.to_string()).collect())
            .unwrap_or_default()
    }

    /// Бейдж совместимости для UI.
    pub fn get_compatibility_badge(&self) -> String {
        let Some(i) = self.info else {
            return format!("ℹ️ {}", self.detector.pretty_name);
        };
        let name = i.name;
        let family = i.family.as_str();
        if !i.has_de_repos {
            // TRANSLATORS: compatibility badge; %1 is the distribution name.
            return format!("⚠️ {}", trf("%1 (limited repositories)", &[&name]));
        }
        if !i.warnings.is_empty() {
            return format!("⚠️ {name} ({family})");
        }
        format!("✅ {name} ({family})")
    }

    /// Почему установка DE возможна/невозможна — ЕДИНСТВЕННОЕ место, где
    /// живёт это правило. И короткая строка `check_install_possible`, и
    /// человеческое объяснение для окна установки собираются из результата:
    /// две копии одних и тех же веток разошлись бы молча (правило #12).
    ///
    /// `zps_enabled` — состояние ЗПС (параметр, см. `get_warnings`).
    pub fn install_reason(&self, de_id: &str, zps_enabled: bool) -> InstallReason {
        // Astra SE с ЗПС.
        if self.detector.distro_id == "astra" && self.is_astra_se() {
            if zps_enabled {
                return InstallReason::AstraZps;
            }
            if !self.has_de_repos() {
                return InstallReason::AstraNoRepos { de_id: de_id.to_string() };
            }
        }

        // ОСнова — закрытые репозитории.
        if self.detector.distro_id == "osnova" {
            return InstallReason::OsnovaClosed;
        }

        // ALT — проверяем наличие пакетов.
        if self.detector.family == PkgFamily::Alt && lookup(ALT_DE_PACKAGES, de_id).is_none() {
            return InstallReason::AltUnsupportedDe { de_id: de_id.to_string() };
        }

        // COSMIC: ограниченный список семейств.
        if de_id == "cosmic"
            && !matches!(
                self.detector.family,
                PkgFamily::Arch | PkgFamily::Fedora | PkgFamily::Suse | PkgFamily::Gentoo | PkgFamily::Void | PkgFamily::Alpine
            )
        {
            return InstallReason::CosmicUnavailable;
        }

        // ROSA: только 4 DE имеют собственные task-*.
        if self.detector.distro_id == "rosa" && lookup(ROSA_DE_PACKAGES, de_id).is_none() {
            return InstallReason::RosaNoTaskPackage { de_id: de_id.to_string() };
        }

        // SteamOS (Steam Deck и Steam Machine): пакетами не ставим никогда —
        // обновление системы стёрло бы стол целиком (до 28.09.2026 здесь
        // было «можно, с предупреждением» через `steamos-readonly disable`).
        // С 25.10.2026 проверенные столы ставятся образами systemd-sysext
        // (`crate::steamos_sysext`, исполнитель сам уводит туда установку);
        // прочие — честный отказ с причиной.
        if self.is_steamos() {
            return match crate::steamos_sysext::support(de_id) {
                crate::steamos_sysext::Support::Supported(_) | crate::steamos_sysext::Support::Builtin => {
                    InstallReason::Ok
                }
                crate::steamos_sysext::Support::NotYet(_) => {
                    InstallReason::SteamOsNotYet { de_id: de_id.to_string() }
                }
            };
        }

        InstallReason::Ok
    }

    /// Проверить, возможна ли установка DE. Возвращает (ok, сообщение).
    /// `zps_enabled` — состояние ЗПС (параметр, см. `get_warnings`).
    ///
    /// Короткая форма для журнала и для кода, которому нужна одна строка;
    /// решение и текст берутся у [`DistroCompat::install_reason`].
    pub fn check_install_possible(&self, de_id: &str, zps_enabled: bool) -> (bool, String) {
        let reason = self.install_reason(de_id, zps_enabled);
        (reason.allowed(), reason.short_message())
    }
}

// ── Объяснение решения об установке (для окна установки DE) ────────────────

/// Прежний выбор «русский или английский» парами текстов в коде.
///
/// ⚠️ Уходит (29.09.2026): подписи переезжают на каталог перевода
/// (`crate::i18n`), язык там один на процесс. У `InstallReason` и мастера
/// установки параметр `Lang` уже ничего не решает и остался ради
/// вызывающих; новых `pick` не заводить. Разбор локали не свой: он уже есть
/// в [`crate::translator::user_lang_from_locale`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Ru,
    En,
}

impl Lang {
    /// Язык по строке локали (`ru_RU.UTF-8` → [`Lang::Ru`]). Всё, что не
    /// русский, получает английский: он понятнее «сырого» русского тому, кто
    /// русского не знает, а покрыть все языки мира мы не беремся.
    pub fn from_locale(locale_str: &str) -> Self {
        Self::from_code(&crate::translator::user_lang_from_locale(locale_str))
    }

    /// Язык по коду ISO 639-1 (`ru` → [`Lang::Ru`]).
    pub fn from_code(code: &str) -> Self {
        if code.eq_ignore_ascii_case("ru") {
            Lang::Ru
        } else {
            Lang::En
        }
    }

    /// Язык покупателя по переменным окружения — в том порядке, в каком их
    /// читает сама система локалей: `LC_ALL`, `LC_MESSAGES`, `LANG`.
    ///
    /// Сам обход переменных — у [`crate::translator::locale_from_env`]: к
    /// 07.09.2026 один и тот же цикл лежал двумя копиями в Qt-слое (значок в
    /// лотке и вкладка «Оформление»), а с приходом перевода описаний понадобилась
    /// не «русский или нет», а СТРОКА локали — правда о ней осталась одна
    /// (правило #12).
    pub fn from_env() -> Self {
        let locale = crate::translator::locale_from_env();
        if locale.is_empty() {
            return Lang::En;
        }
        Self::from_locale(&locale)
    }

    /// Код языка строкой — им же помечается ответ моста.
    pub fn code(self) -> &'static str {
        match self {
            Lang::Ru => "ru",
            Lang::En => "en",
        }
    }

    /// Выбор одного из двух вариантов текста. Для новых подписей — `tr`.
    pub fn pick(self, ru: &str, en: &str) -> String {
        match self {
            Lang::Ru => ru.to_string(),
            Lang::En => en.to_string(),
        }
    }
}

/// Исход проверки: ставим молча, ставим с предупреждением или не ставим.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallOutcome {
    /// Препятствий нет — спрашивать не о чем.
    Allowed,
    /// Поставить можно, но человек должен знать, на что идёт.
    Warning,
    /// Ставить нельзя: установка заведомо не пройдёт.
    Blocked,
}

impl InstallOutcome {
    /// Исход строкой — для моста и журнала.
    pub fn code(self) -> &'static str {
        match self {
            InstallOutcome::Allowed => "ok",
            InstallOutcome::Warning => "warning",
            InstallOutcome::Blocked => "blocked",
        }
    }
}

/// Причина решения. Тексты — производные от неё, поэтому новый случай
/// добавляется одним вариантом перечисления и одной строкой в каждом
/// `match` ниже; ветвление правила при этом не трогается.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallReason {
    /// Ограничений нет.
    Ok,
    /// Astra Linux SE с включённой замкнутой программной средой.
    AstraZps,
    /// Astra Linux: DE нет в штатных репозиториях.
    AstraNoRepos { de_id: String },
    /// ОСнова: публичных репозиториев нет.
    OsnovaClosed,
    /// ALT Linux: для этого DE не описано соответствие пакетов.
    AltUnsupportedDe { de_id: String },
    /// COSMIC не собран для семейства этого дистрибутива.
    CosmicUnavailable,
    /// ROSA: нет собственного метапакета `task-*` — ставим на свой страх.
    RosaNoTaskPackage { de_id: String },
    /// SteamOS (Steam Deck и Steam Machine): этот стол образом systemd-sysext
    /// ещё не проверен — установка ЗАПРЕЩЕНА (пакетами на SteamOS не ставим
    /// никогда: обновление ОС стирает всё поставленное поверх).
    SteamOsNotYet { de_id: String },
}

/// Перечень DE через запятую из таблицы пакетов.
fn de_list(table: &[(&str, DePackages)]) -> String {
    table
        .iter()
        .map(|(k, _)| *k)
        .collect::<Vec<&str>>()
        .join(", ")
}

impl InstallReason {
    /// Стабильный код причины — для журнала, тестов и моста.
    pub fn code(&self) -> &'static str {
        match self {
            InstallReason::Ok => "ok",
            InstallReason::AstraZps => "astra_zps",
            InstallReason::AstraNoRepos { .. } => "astra_no_repos",
            InstallReason::OsnovaClosed => "osnova_closed",
            InstallReason::AltUnsupportedDe { .. } => "alt_unsupported_de",
            InstallReason::CosmicUnavailable => "cosmic_unavailable",
            InstallReason::RosaNoTaskPackage { .. } => "rosa_no_task_package",
            InstallReason::SteamOsNotYet { .. } => "steamos_not_yet",
        }
    }

    /// Исход по причине.
    pub fn outcome(&self) -> InstallOutcome {
        match self {
            InstallReason::Ok => InstallOutcome::Allowed,
            InstallReason::RosaNoTaskPackage { .. } => InstallOutcome::Warning,
            _ => InstallOutcome::Blocked,
        }
    }

    /// Разрешена ли установка.
    pub fn allowed(&self) -> bool {
        self.outcome() != InstallOutcome::Blocked
    }

    /// Короткая строка для журнала — историческая форма
    /// `check_install_possible` (её сверяет записанный эталон).
    pub fn short_message(&self) -> String {
        match self {
            InstallReason::Ok => "OK".to_string(),
            InstallReason::AstraZps =>
                "Astra Linux SE: ЗПС включена. Установка неподписанных пакетов невозможна.\nОтключите ЗПС или используйте Astra Linux CE.".to_string(),
            InstallReason::AstraNoRepos { de_id } => format!(
                "Рабочий стол «{de_id}» недоступен в стандартных репозиториях Astra Linux.\nПодключите репозитории Debian для установки альтернативных рабочих столов."
            ),
            InstallReason::OsnovaClosed =>
                "ОСнова — закрытый дистрибутив. Публичные репозитории с рабочими столами недоступны.".to_string(),
            InstallReason::AltUnsupportedDe { de_id } => format!(
                "Рабочий стол «{de_id}» не поддерживается на ALT Linux.\nДоступные рабочие столы: {}",
                de_list(ALT_DE_PACKAGES)
            ),
            InstallReason::CosmicUnavailable =>
                "COSMIC доступен на Arch, Fedora, openSUSE, Gentoo (через cosmic-overlay), Void и Alpine.\nНа Debian/Ubuntu/Solus/Clear/ALT пакетов пока нет.".to_string(),
            InstallReason::RosaNoTaskPackage { de_id } => format!(
                "⚠️ Для «{de_id}» в ROSA Linux нет собственного метапакета task-*.\nRexon попробует использовать Fedora-группы, но они могут отсутствовать в штатных репозиториях ROSA.\nДоступны без предупреждения: {}.",
                de_list(ROSA_DE_PACKAGES)
            ),
            // Та же сводка, что покупатель читает в окне и на странице в
            // Steam.
            InstallReason::SteamOsNotYet { .. } => steamos_summary(),
        }
    }

    /// Заголовок окна — одной фразой, без жаргона (правило #13).
    pub fn title(&self, _lang: Lang) -> String {
        match self {
            InstallReason::Ok => String::new(),
            InstallReason::AstraZps => tr("Astra Linux SE security blocks the installation"),
            InstallReason::AstraNoRepos { .. } => tr("This desktop is not in the Astra Linux repositories"),
            InstallReason::OsnovaClosed => tr("OSnova repositories are not public"),
            InstallReason::AltUnsupportedDe { .. } => tr("ALT Linux: this desktop is not supported yet"),
            InstallReason::CosmicUnavailable => tr("COSMIC is not packaged for this distribution yet"),
            InstallReason::RosaNoTaskPackage { .. } => tr("ROSA Linux: installation is possible, but not guaranteed"),
            InstallReason::SteamOsNotYet { .. } => tr(STEAMOS_DESKTOPS_TITLE),
        }
    }

    /// Что мешает — человеческим языком, без имён конфигов и команд.
    pub fn problem(&self, _lang: Lang) -> String {
        match self {
            InstallReason::Ok => String::new(),
            InstallReason::AstraZps => tr(
                "Astra Linux Special Edition runs a closed software environment: it only \
                 starts software signed by the vendor. Desktop environment packages carry \
                 no such signature, so the installation cannot succeed — it would fail \
                 somewhere in the middle.",
            ),
            // TRANSLATORS: %1 is the id of a desktop environment, e.g. "gnome".
            InstallReason::AstraNoRepos { de_id } => trf(
                "The stock Astra Linux package sources only carry its own Fly desktop. \
                 There are no “%1” packages there, so there is nothing to download.",
                &[de_id],
            ),
            InstallReason::OsnovaClosed => tr(
                "OSnova is a certified distribution with closed package sources. There is \
                 no public place to download desktop environments from, so Rexon has \
                 nothing to fetch.",
            ),
            // TRANSLATORS: %1 is the id of a desktop environment, e.g. "gnome".
            InstallReason::AltUnsupportedDe { de_id } => trf(
                "ALT Linux builds its own packages under its own names. The mapping for \
                 “%1” is not described yet, so Rexon does not know what to install.",
                &[de_id],
            ),
            InstallReason::CosmicUnavailable => tr(
                "COSMIC is a young desktop and is not packaged everywhere yet. This \
                 distribution's package sources do not carry it.",
            ),
            // TRANSLATORS: %1 is the id of a desktop environment, e.g. "gnome".
            InstallReason::RosaNoTaskPackage { de_id } => trf(
                "ROSA installs desktops through its own task-* package sets, and there \
                 is none for “%1”. Rexon will try the regular Fedora groups, but \
                 ROSA's package sources may not have them.",
                &[de_id],
            ),
            InstallReason::SteamOsNotYet { de_id } => match crate::steamos_sysext::support(de_id) {
                crate::steamos_sysext::Support::NotYet(why) => why,
                _ => String::new(),
            },
        }
    }

    /// Что с этим делать. Пусто только тогда, когда делать нечего.
    pub fn advice(&self, _lang: Lang) -> String {
        match self {
            InstallReason::Ok => String::new(),
            InstallReason::AstraZps => tr(
                "What you can do: turn the closed software environment off (administrator \
                 rights and a reboot are required) — or install the desktop on Astra Linux \
                 CE, which has no such restriction.",
            ),
            InstallReason::AstraNoRepos { .. } => tr(
                "What you can do: add the Debian package sources — Astra is based on Debian, \
                 and the required packages will show up afterwards.",
            ),
            InstallReason::OsnovaClosed => tr(
                "What you can do: request the package source from your system vendor (it \
                 usually comes with the licence) and add it — Rexon will see the desktops \
                 afterwards.",
            ),
            // TRANSLATORS: %1 is a comma-separated list of desktop environment ids.
            InstallReason::AltUnsupportedDe { .. } => trf(
                "What you can do: pick one of the desktops verified on ALT Linux — %1.",
                &[&de_list(ALT_DE_PACKAGES)],
            ),
            InstallReason::CosmicUnavailable => tr(
                "What you can do: pick another desktop. COSMIC is packaged for Arch, Fedora, \
                 openSUSE, Gentoo (through cosmic-overlay), Void and Alpine.",
            ),
            // TRANSLATORS: %1 is a comma-separated list of desktop environment ids.
            InstallReason::RosaNoTaskPackage { .. } => trf(
                "You may continue: if the packages are missing, the installation simply \
                 stops and nothing breaks. Installed without a warning: %1.",
                &[&de_list(ROSA_DE_PACKAGES)],
            ),
            InstallReason::SteamOsNotYet { .. } => tr(STEAMOS_SUMMARY_DESKTOPS),
        }
    }

    /// Готовое объяснение одним куском: что мешает и что делать.
    pub fn explanation(&self, lang: Lang) -> String {
        let problem = self.problem(lang);
        let advice = self.advice(lang);
        match (problem.is_empty(), advice.is_empty()) {
            (true, true) => String::new(),
            (false, true) => problem,
            (true, false) => advice,
            (false, false) => format!("{problem}\n\n{advice}"),
        }
    }
}

/// Один элемент списка региональных дистрибутивов для UI.
///
/// Порядок полей = порядок ключей Python-словаря в `_get_regional_distros`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RegionalDistro {
    pub id: String,
    pub name: String,
    pub family: PkgFamily,
    pub country: String,
    pub has_de_repos: bool,
    pub has_warnings: bool,
}

/// Дистрибутивы по стране (порт `_get_regional_distros`): по одному на имя,
/// в порядке `DISTRO_INFO`, дубли имён отсеиваются.
pub fn get_regional_distros(country: &str) -> Vec<RegionalDistro> {
    let mut result = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    for (id, info) in DISTRO_INFO {
        if info.country == country && !seen.contains(&info.name) {
            seen.push(info.name);
            result.push(RegionalDistro {
                id: (*id).to_string(),
                name: info.name.to_string(),
                family: info.family,
                country: country.to_string(),
                has_de_repos: info.has_de_repos,
                has_warnings: !info.warnings.is_empty(),
            });
        }
    }
    result
}

/// Российские дистрибутивы для UI (порт `get_supported_russian_distros`).
pub fn get_supported_russian_distros() -> Vec<RegionalDistro> {
    get_regional_distros("RU")
}

/// Региональные дистрибутивы (порт `get_supported_regional_distros`).
/// `countries = None` → RU, CN, IN (в этом порядке).
pub fn get_supported_regional_distros(countries: Option<&[&str]>) -> Vec<RegionalDistro> {
    let default: [&str; 3] = ["RU", "CN", "IN"];
    let list = countries.unwrap_or(&default);
    let mut result = Vec::new();
    for c in list {
        result.extend(get_regional_distros(c));
    }
    result
}

// ── SteamOS (Steam Deck и Steam Machine): чего Rexon там не делает ──────────
//
// SteamOS — атомарная система: системный раздел `/usr` закрыт на запись, а
// обновление заменяет его целиком и стирает всё, что было поставлено поверх.
// Всё, что Rexon
// пишет в домашнюю папку — темы пользователя, живые обои, заставка при
// входе, — работает и переживает обновления. Всё, что пишет в систему, —
// нет, и честнее показать это заранее, чем дать нажать «Применить» и
// промолчать. Решение «что нельзя» живёт здесь ОДНО, а вкладки только
// спрашивают (правило #12).

/// Сводка для покупателя, первая половина: что работает.
pub const STEAMOS_SUMMARY_WORKS: &str =
    // TRANSLATORS: SteamOS runs on Steam Deck and Steam Machine. "Startup movie" is the short video Rexon plays right after you log in.
    n_("Themes, live wallpapers and the startup movie work on SteamOS and survive system updates.");
/// Сводка для покупателя, вторая половина: какие столы ставятся (список —
/// `steamos_sysext::DESKTOPS`, сверка `steamos_summary_names_the_built_desktops`).
pub const STEAMOS_SUMMARY_DESKTOPS: &str = n_(
    "Xfce, LXQt and Hyprland can be installed on SteamOS: Rexon builds them as system \
     extensions and rebuilds them by itself after every system update.",
);
/// Заголовок окна отказа в установке рабочего стола (стол не проверен).
pub const STEAMOS_DESKTOPS_TITLE: &str =
    n_("SteamOS: this desktop environment can't be installed yet");

/// Сводка целиком — для журнала, короткой формы и страницы в Steam.
pub fn steamos_summary() -> String {
    format!("{} {}", tr(STEAMOS_SUMMARY_WORKS), tr(STEAMOS_SUMMARY_DESKTOPS))
}

/// SteamOS ли это по `ID` из os-release. Одна проверка на весь продукт —
/// её зовут и [`DistroCompat::is_steamos`], и живой [`running_on_steamos`].
pub fn is_steamos_id(distro_id: &str) -> bool {
    distro_id.trim().eq_ignore_ascii_case("steamos")
}

/// Живая проверка: SteamOS ли под нами (Steam Deck или Steam Machine).
/// Читает `/etc/os-release` тем же детектором, что и остальной продукт.
pub fn running_on_steamos() -> bool {
    is_steamos_id(&crate::system::DistroDetector::detect_live().distro_id())
}

/// Что Rexon НЕ делает на SteamOS: всё, что пишет в системный раздел.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SteamOsBlock {
    /// Пакеты самой SteamOS: доустановка текущего стола, связки оболочек,
    /// смена экрана входа. Столы ставятся иначе — образами systemd-sysext
    /// (`crate::steamos_sysext`), и для их карточек причина своя
    /// ([`steamos_desktop_card_block`]).
    Desktops,
    /// Тема экрана входа. На SteamOS экран входа держит переход в игровой
    /// режим — его не трогаем вовсе.
    LoginScreen,
    /// Заставка загрузки: обновление SteamOS пересобирает загрузочный образ
    /// и возвращает свою.
    BootSplash,
    /// Прочая системная часть темы (фон экрана входа, системные плагины
    /// панелей, темы оболочки GNOME) — всё, что ставится с паролем
    /// администратора в системный раздел.
    SystemPart,
}

impl SteamOsBlock {
    /// Причина одной фразой — рядом с неактивной кнопкой или в окне отказа.
    pub fn reason(self) -> String {
        match self {
            SteamOsBlock::Desktops => tr(
                "On SteamOS, Rexon doesn't change the packages of the system itself: the next \
                 system update would undo it, so desktops are installed as system extensions instead.",
            ),
            SteamOsBlock::LoginScreen => tr(
                "On SteamOS the login screen can't be changed: switching to Game Mode depends on it.",
            ),
            SteamOsBlock::BootSplash => tr(
                "On SteamOS the boot splash can't be changed: every system update puts the \
                 original one back.",
            ),
            SteamOsBlock::SystemPart => tr(
                "On SteamOS this can't be installed: it has to be written to the system \
                 partition, which SteamOS keeps read-only.",
            ),
        }
    }

    /// Причина, если действие запрещено на этой системе; `None` — можно.
    pub fn on(self, steamos: bool) -> Option<String> {
        steamos.then(|| self.reason())
    }

    /// То же по живой системе.
    pub fn live(self) -> Option<String> {
        self.on(running_on_steamos())
    }
}

/// Причина неактивных кнопок установки и удаления на карточке стола
/// `de_id` на SteamOS; `None` — кнопки работают (стол ставится образом
/// systemd-sysext) или это не SteamOS.
pub fn steamos_desktop_card_block(de_id: &str, steamos: bool) -> Option<String> {
    if !steamos {
        return None;
    }
    match crate::steamos_sysext::support(de_id) {
        crate::steamos_sysext::Support::Supported(_) => None,
        crate::steamos_sysext::Support::Builtin => Some(SteamOsBlock::Desktops.reason()),
        crate::steamos_sysext::Support::NotYet(why) => Some(why),
    }
}

// ── Детерминированные части побочных функций (чистые) ──────────────────────

/// Признак APT-RPM (ALT Linux): есть apt-get и rpm, но нет dpkg.
/// Порт `is_apt_rpm_system` без обращения к PATH (предикаты — параметры).
pub fn is_apt_rpm(has_apt: bool, has_rpm: bool, has_dpkg: bool) -> bool {
    has_apt && has_rpm && !has_dpkg
}

/// Разбор варианта ALT Linux из содержимого /etc/os-release.
/// Порт логики `detect_alt_variant` (без чтения файла — эффект в обёртке).
pub fn detect_alt_variant_from(content: &str) -> &'static str {
    let c = content.to_lowercase();
    if c.contains("workstation") || c.contains("рабочая станция") {
        "workstation"
    } else if c.contains("education") || c.contains("образование") {
        "education"
    } else if c.contains("server") || c.contains("сервер") {
        "server"
    } else if c.contains("simply") {
        "simply"
    } else if c.contains("starterkit") {
        "starterkit"
    } else {
        "unknown"
    }
}

/// Включена ли ЗПС на Astra SE. Порт логики `_is_zps_enabled` без subprocess/fs:
/// `lsmod` (если снят) содержит `digsig_verif`, либо конфиг ЗПС содержит
/// `DIGSIG_ELF_MODE=1`. Эффекты (запуск lsmod, чтение файла) — в обёртке.
pub fn zps_enabled_from(lsmod_out: Option<&str>, conf: Option<&str>) -> bool {
    if let Some(out) = lsmod_out {
        if out.contains("digsig_verif") {
            return true;
        }
    }
    if let Some(c) = conf {
        if c.contains("DIGSIG_ELF_MODE=1") {
            return true;
        }
    }
    false
}

// ── Тесты объяснения об установке ──────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn detector(distro_id: &str, family: PkgFamily, name: &str, variant: &str) -> Detector {
        Detector {
            distro_id: distro_id.to_string(),
            family,
            pretty_name: distro_id.to_string(),
            variant_id: variant.to_string(),
            name: name.to_string(),
        }
    }

    fn astra_se() -> DistroCompat {
        DistroCompat::new(detector(
            "astra",
            PkgFamily::Debian,
            "Astra Linux Special Edition",
            "smolensk",
        ))
    }

    /// Все причины перебираются одним списком: новый вариант перечисления
    /// обязан получить и заголовок, и объяснение, и совет — иначе тест падает.
    fn all_reasons() -> Vec<InstallReason> {
        vec![
            InstallReason::AstraZps,
            InstallReason::AstraNoRepos { de_id: "kde".into() },
            InstallReason::OsnovaClosed,
            InstallReason::AltUnsupportedDe { de_id: "cosmic".into() },
            InstallReason::CosmicUnavailable,
            InstallReason::RosaNoTaskPackage { de_id: "budgie".into() },
            InstallReason::SteamOsNotYet { de_id: "gnome".into() },
        ]
    }

    /// Короткая строка не изменилась ни в одной ветке: её сверяет записанный
    /// эталон, поэтому расхождение здесь ловится раньше roundtrip-теста.
    ///
    /// ⚠️ Осознанное изменение 28.09.2026: у SteamOS строка другая — прежняя
    /// обещала установку через снятие защиты, а теперь установка запрещена
    /// (первое же обновление SteamOS стёрло бы поставленное). Сверяется
    /// английский исходник:
    /// русский живёт в каталоге перевода.
    #[test]
    fn short_message_matches_history() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let cases: Vec<(DistroCompat, &str, bool, &str)> = vec![
            (astra_se(), "kde", true, "Astra Linux SE: ЗПС включена. Установка неподписанных пакетов невозможна.\nОтключите ЗПС или используйте Astra Linux CE."),
            (astra_se(), "kde", false, "Рабочий стол «kde» недоступен в стандартных репозиториях Astra Linux.\nПодключите репозитории Debian для установки альтернативных рабочих столов."),
            (DistroCompat::new(detector("osnova", PkgFamily::Debian, "ОСнова", "")), "kde", false,
             "ОСнова — закрытый дистрибутив. Публичные репозитории с рабочими столами недоступны."),
            (DistroCompat::new(detector("alt", PkgFamily::Alt, "ALT", "")), "cosmic", false,
             "Рабочий стол «cosmic» не поддерживается на ALT Linux.\nДоступные рабочие столы: kde, gnome, xfce, mate, cinnamon-mint, budgie, hyprland, lxqt"),
            (DistroCompat::new(detector("debian", PkgFamily::Debian, "Debian", "")), "cosmic", false,
             "COSMIC доступен на Arch, Fedora, openSUSE, Gentoo (через cosmic-overlay), Void и Alpine.\nНа Debian/Ubuntu/Solus/Clear/ALT пакетов пока нет."),
            (DistroCompat::new(detector("rosa", PkgFamily::Fedora, "ROSA", "")), "budgie", false,
             "⚠️ Для «budgie» в ROSA Linux нет собственного метапакета task-*.\nRexon попробует использовать Fedora-группы, но они могут отсутствовать в штатных репозиториях ROSA.\nДоступны без предупреждения: kde, gnome, xfce, mate, lxqt."),
            // 25.10.2026: на SteamOS проверенные столы ставятся образами
            // systemd-sysext, отказ — только непроверенным.
            (DistroCompat::new(detector("steamos", PkgFamily::Arch, "SteamOS", "")), "gnome", false,
             "Themes, live wallpapers and the startup movie work on SteamOS and survive system updates. Xfce, LXQt and Hyprland can be installed on SteamOS: Rexon builds them as system extensions and rebuilds them by itself after every system update."),
            (DistroCompat::new(detector("steamos", PkgFamily::Arch, "SteamOS", "")), "xfce", false, "OK"),
            (DistroCompat::new(detector("cachyos", PkgFamily::Arch, "CachyOS", "")), "kde", false, "OK"),
        ];
        for (compat, de, zps, want) in cases {
            let (_, got) = compat.check_install_possible(de, zps);
            assert_eq!(got, want, "короткая строка для {de}");
        }
    }

    /// Решение «можно/нельзя» у короткой и подробной формы всегда одно.
    #[test]
    fn short_and_detailed_agree() {
        let compats = [
            astra_se(),
            DistroCompat::new(detector("osnova", PkgFamily::Debian, "ОСнова", "")),
            DistroCompat::new(detector("alt", PkgFamily::Alt, "ALT", "")),
            DistroCompat::new(detector("rosa", PkgFamily::Fedora, "ROSA", "")),
            DistroCompat::new(detector("steamos", PkgFamily::Arch, "SteamOS", "")),
            DistroCompat::new(detector("debian", PkgFamily::Debian, "Debian", "")),
            DistroCompat::new(detector("cachyos", PkgFamily::Arch, "CachyOS", "")),
        ];
        for compat in &compats {
            for de in ["kde", "gnome", "cosmic", "lxqt", "budgie", "нет-такого"] {
                for zps in [false, true] {
                    let reason = compat.install_reason(de, zps);
                    let (ok, msg) = compat.check_install_possible(de, zps);
                    assert_eq!(ok, reason.allowed());
                    assert_eq!(msg, reason.short_message());
                }
            }
        }
    }

    /// Ни одна причина не молчит: у каждой есть заголовок, объяснение и совет,
    /// и объяснение собрано из них. Русский слово в слово сверяет снимок
    /// `installer_parity::russian_installer_texts_as_before` (раздел
    /// `de_reason_*`) под закреплённым русским каталогом.
    #[test]
    fn every_reason_explains_itself() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        for reason in all_reasons() {
            assert!(!reason.title(Lang::En).is_empty(), "{}: заголовок", reason.code());
            assert!(!reason.problem(Lang::En).is_empty(), "{}: что мешает", reason.code());
            assert!(!reason.advice(Lang::En).is_empty(), "{}: что делать", reason.code());
            let explanation = reason.explanation(Lang::En);
            assert!(explanation.contains(&reason.problem(Lang::En)), "{}", reason.code());
            assert!(explanation.contains(&reason.advice(Lang::En)), "{}", reason.code());
        }
        // «Всё в порядке» не объясняет ничего — окну нечего показывать.
        assert!(InstallReason::Ok.explanation(Lang::En).is_empty());
        assert_eq!(InstallReason::Ok.outcome(), InstallOutcome::Allowed);
    }

    /// Английский текст не содержит кириллицы (типовая ошибка «забыл перевести»).
    #[test]
    fn english_has_no_cyrillic() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        for reason in all_reasons() {
            let text = format!(
                "{} {} {}",
                reason.title(Lang::En),
                reason.problem(Lang::En),
                reason.advice(Lang::En)
            );
            assert!(
                !text.chars().any(|c| ('\u{0400}'..='\u{04FF}').contains(&c)),
                "{}: в английском тексте кириллица",
                reason.code()
            );
        }
    }

    /// Язык берётся из локали тем же разбором, что и автоперевод описаний.
    #[test]
    fn lang_from_locale() {
        assert_eq!(Lang::from_locale("ru_RU.UTF-8"), Lang::Ru);
        assert_eq!(Lang::from_locale("ru"), Lang::Ru);
        assert_eq!(Lang::from_locale("en_US.UTF-8"), Lang::En);
        assert_eq!(Lang::from_locale(""), Lang::En);
        assert_eq!(Lang::from_locale("de_DE"), Lang::En);
        assert_eq!(Lang::Ru.code(), "ru");
    }

    /// os-release Steam Deck (снят с устройства) и Steam Machine (та же ОС;
    /// `VARIANT_ID` у неё не подтверждён, поэтому проверяется и чужой, и
    /// отсутствующий). Все три — SteamOS; похожие по родне — нет.
    const DECK_OS_RELEASE: &str = "NAME=\"SteamOS\"\nPRETTY_NAME=\"SteamOS\"\n\
        VERSION_CODENAME=holo\nID=steamos\nID_LIKE=arch\nVARIANT_ID=steamdeck\n\
        VERSION_ID=3.7.13\nLOGO=steamos\n";
    const MACHINE_OS_RELEASE: &str = "NAME=\"SteamOS\"\nPRETTY_NAME=\"SteamOS\"\n\
        ID=\"steamos\"\nID_LIKE=arch\nVARIANT_ID=steammachine\nVERSION_ID=3.8.28\n";
    const NO_VARIANT_OS_RELEASE: &str = "NAME=SteamOS\nID=steamos\nID_LIKE=arch\n";

    fn compat_from_os_release(text: &str) -> DistroCompat {
        let det = crate::system::DistroDetector::from_os_release_files(Some(text), None);
        DistroCompat::new(Detector {
            distro_id: det.distro_id(),
            family: PkgFamily::Arch,
            pretty_name: det.pretty_name(),
            variant_id: String::new(),
            name: det.name(),
        })
    }

    /// Steam Deck и Steam Machine опознаются одинаково — по `ID`, а не по
    /// `VARIANT_ID`; на обеих проверенные столы ставятся образами
    /// systemd-sysext, прочие запрещены с причиной.
    #[test]
    fn steam_deck_and_steam_machine_install_only_checked_desktops() {
        for text in [DECK_OS_RELEASE, MACHINE_OS_RELEASE, NO_VARIANT_OS_RELEASE] {
            let compat = compat_from_os_release(text);
            assert!(compat.is_steamos(), "не опознана SteamOS:\n{text}");
            for de in ["xfce", "lxqt", "hyprland"] {
                assert_eq!(compat.install_reason(de, false), InstallReason::Ok, "{de}");
            }
            for de in ["gnome", "cosmic", "mate"] {
                let reason = compat.install_reason(de, false);
                assert_eq!(reason, InstallReason::SteamOsNotYet { de_id: de.into() }, "{de}");
                assert_eq!(reason.outcome(), InstallOutcome::Blocked, "{de}: запрет, а не предупреждение");
                let (ok, _) = compat.check_install_possible(de, false);
                assert!(!ok, "{de}: короткая форма тоже запрещает");
            }
        }
        for other in ["ID=holoiso\nID_LIKE=arch steamos\n", "ID=bazzite\nID_LIKE=fedora\n", "ID=cachyos\n"] {
            assert!(!compat_from_os_release(other).is_steamos(), "ложное срабатывание: {other}");
        }
    }

    /// Тексты для покупателя: никаких команд и обещаний «снимем защиту»,
    /// сказано, что работает, и что установка окружений пока не
    /// поддерживается — без сроков.
    #[test]
    fn steamos_texts_are_honest() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let reason = InstallReason::SteamOsNotYet { de_id: "gnome".into() };
        let all = format!(
            "{} {} {} {}",
            reason.short_message(),
            reason.title(Lang::En),
            reason.explanation(Lang::En),
            compat_from_os_release(DECK_OS_RELEASE).get_warnings(false).join(" ")
        );
        for banned in ["steamos-readonly", "pacman", "--asexplicit", "unlock", "soon", "will be supported"] {
            assert!(!all.contains(banned), "в тексте осталось «{banned}»: {all}");
        }
        assert!(all.contains("live wallpapers"));
        assert!(all.contains("system extensions"));
        let warnings = compat_from_os_release(MACHINE_OS_RELEASE).get_warnings(false);
        assert_eq!(warnings, vec![tr(STEAMOS_SUMMARY_WORKS), tr(STEAMOS_SUMMARY_DESKTOPS)]);
    }

    /// Сводка называет ровно те столы, что собираются образами.
    #[test]
    fn steamos_summary_names_the_built_desktops() {
        let names: Vec<&str> = crate::steamos_sysext::DESKTOPS.iter().map(|d| d.name).collect();
        assert_eq!(names, ["Xfce", "LXQt", "Hyprland"]);
        assert!(STEAMOS_SUMMARY_DESKTOPS.starts_with("Xfce, LXQt and Hyprland can be installed"));
    }

    /// Карточки на SteamOS: проверенные столы — кнопки работают, Plasma и
    /// непроверенные — с причиной; вне SteamOS причин нет.
    #[test]
    fn steamos_card_blocks() {
        assert_eq!(steamos_desktop_card_block("xfce", true), None);
        assert_eq!(steamos_desktop_card_block("kde", true), Some(SteamOsBlock::Desktops.reason()));
        assert!(steamos_desktop_card_block("gnome", true).is_some());
        assert_eq!(steamos_desktop_card_block("gnome", false), None);
    }

    /// Каждое запрещённое на SteamOS действие называет причину одной фразой,
    /// и причины у разных действий разные; вне SteamOS запрета нет.
    #[test]
    fn steamos_blocks_explain_themselves() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let all = [
            SteamOsBlock::Desktops,
            SteamOsBlock::LoginScreen,
            SteamOsBlock::BootSplash,
            SteamOsBlock::SystemPart,
        ];
        let mut seen = Vec::new();
        for what in all {
            assert_eq!(what.on(false), None, "{what:?}: вне SteamOS запрета нет");
            let reason = what.on(true).expect("на SteamOS — запрет с причиной");
            assert!(reason.starts_with("On SteamOS"), "{what:?}: {reason}");
            assert_eq!(reason.matches(". ").count(), 0, "{what:?}: одна фраза: {reason}");
            assert!(!seen.contains(&reason), "{what:?}: причина повторяет чужую");
            seen.push(reason);
        }
        assert!(is_steamos_id("steamos") && is_steamos_id(" SteamOS "));
        assert!(!is_steamos_id("") && !is_steamos_id("holoiso"));
    }

    /// Astra SE с ЗПС — блокировка, и она не зависит от выбранного DE.
    #[test]
    fn astra_zps_blocks_every_desktop() {
        for de in ["kde", "gnome", "xfce", "hyprland"] {
            let reason = astra_se().install_reason(de, true);
            assert_eq!(reason, InstallReason::AstraZps);
            assert_eq!(reason.outcome(), InstallOutcome::Blocked);
            assert!(!reason.allowed());
        }
    }
}
