//! Compatibility with Russian/Chinese/Indian distributions, a port of
//! `rexon/distro_compat.py`.
//!
//! Pure data + logic: distribution tables (`DISTRO_INFO`), DE package
//! mappings for ALT/ROSA, ROSA branded applets and the `DistroCompat` class
//! (warnings, package selection, installability check, badge). The
//! correctness of the table transcription and of every logic branch is
//! verified by a byte-for-byte comparison with Python itself (the golden
//! test `distro_compat_parity.rs`).
//!
//! Python's side effects (`_is_zps_enabled`: lsmod + a file,
//! `is_apt_rpm_system`: which, `detect_alt_variant`: /etc/os-release) are
//! moved into thin wrappers, while their deterministic core is ported as
//! pure functions (`zps_enabled_from`, `is_apt_rpm`,
//! `detect_alt_variant_from`). The ZPS (closed software environment) state
//! is passed to the `DistroCompat` methods as the `zps_enabled` parameter,
//! just as `settings.rs` takes the `command_exists` predicate as a
//! parameter.
//!
//! The decision "can a DE be installed" lives in ONE place:
//! `install_reason`. Both forms are built from it: the short string for the
//! log (`check_install_possible`, historical, checked against a recorded
//! golden file) and the human explanation for the install window
//! (`InstallReason::title` / `problem` / `advice`, in the language of the
//! translation catalog, 29.09.2026). Splitting them into two copies of the
//! branches is not allowed: they would drift apart silently (rule #12).
//! The short log string is not subject to translation and stays Russian.

use serde::Serialize;

use crate::i18n::{n_, tr, trf};

// `PkgFamily` lives in `system.rs`, as in Python: system.py defines the
// enum, distro_compat.py imports it. The re-export keeps the old paths
// (`distro_compat::PkgFamily`) for existing code and tests.
pub use crate::system::PkgFamily;

/// Information about a distribution (the value of `DISTRO_INFO[id]`).
///
/// The field order matches the key order of the Python dictionary: the
/// order in `json.dumps(get_info())` depends on it (compared byte for
/// byte).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct DistroInfo {
    pub name: &'static str,
    pub family: PkgFamily,
    pub country: &'static str,
    pub notes: &'static str,
    pub has_de_repos: bool,
    pub warnings: &'static [&'static str],
}

/// Warning for distributions with their own repository (openKylin, Kylin OS).
const OWN_REPOS_WARNING: &str =
    n_("This distribution builds its own packages: some desktop environments may be missing from its repositories.");

/// `DISTRO_INFO`: os-release ID -> information. Insertion order matters
/// for `get_supported_*_distros` (they iterate `DISTRO_INFO.items()`).
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
    // A key not from os-release: Ubuntu Kylin has `ID=ubuntu` and is
    // recognised by `NAME="Ubuntu Kylin"` (written by the
    // ubuntukylin-default-settings package), see [`info_key`]. Until
    // 13.11.2026 the entry was looked up by ID and was unreachable.
    ("ubuntukylin", DistroInfo {
        name: "Ubuntu Kylin",
        family: PkgFamily::Debian,
        country: "CN",
        notes: "Официальный флейвор Ubuntu. ID=ubuntu, узнаётся по NAME=\"Ubuntu Kylin\". Рабочий стол UKUI.",
        has_de_repos: true,
        warnings: &[],
    }),
    ("openkylin", DistroInfo {
        name: "openKylin",
        family: PkgFamily::Debian,
        country: "CN",
        notes: "ID=openkylin без ID_LIKE. apt/dpkg, но репозиторий свой (не Debian). Рабочий стол UKUI.",
        has_de_repos: true,
        warnings: &[OWN_REPOS_WARNING],
    }),
    // Kylin OS (银河麒麟): `ID=kylin` for the desktop edition on Debian/Ubuntu
    // and for the server one on RPM, so the family is not set in the table
    // (`Unknown`): the machine's package manager decides it
    // (`system::DistroDetector::family` by binaries).
    ("kylin", DistroInfo {
        name: "Kylin OS",
        family: PkgFamily::Unknown,
        country: "CN",
        notes: "ID=kylin у двух редакций: настольная на Debian/Ubuntu, серверная на RPM. Семейство — по менеджеру пакетов. Рабочий стол UKUI.",
        has_de_repos: true,
        warnings: &[OWN_REPOS_WARNING],
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
        // Steam Deck and Steam Machine are one and the same OS (`ID=steamos`).
        // An update replaces the system partition entirely and erases everything
        // installed on top, so desktops are installed not as packages but as
        // systemd-sysext images (`crate::steamos_sysext`, 25.10.2026).
        notes: "Arch-based, корень только для чтения, обновление заменяет образ целиком. Рабочие столы Rexon ставит образами-расширениями systemd-sysext.",
        has_de_repos: true,
        // Texts for the person: English is the source of the translation catalog;
        // they are translated where shown (`get_warnings`).
        warnings: &[STEAMOS_SUMMARY_WORKS, STEAMOS_SUMMARY_DESKTOPS],
    }),
];

/// DE packages of one distribution: a package list + the display manager.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct DePackages {
    pub packages: &'static [&'static str],
    pub dm: &'static str,
}

/// `ALT_DE_PACKAGES`: DE package names for ALT Linux (its own database).
/// The order matters for the message `', '.join(ALT_DE_PACKAGES.keys())`.
pub const ALT_DE_PACKAGES: &[(&str, DePackages)] = &[
    ("kde", DePackages { packages: &["kde5", "plasma5-workspace", "konsole", "dolphin", "sddm"], dm: "sddm" }),
    ("gnome", DePackages { packages: &["gnome-shell", "gnome-control-center", "gnome-tweaks", "nautilus", "gdm"], dm: "gdm" }),
    ("xfce", DePackages { packages: &["xfce4", "xfce4-goodies", "lightdm", "lightdm-gtk-greeter"], dm: "lightdm" }),
    ("mate", DePackages { packages: &["mate", "mate-terminal", "plank", "lightdm", "lightdm-gtk-greeter"], dm: "lightdm" }),
    ("cinnamon-mint", DePackages { packages: &["cinnamon", "nemo", "gnome-terminal", "lightdm", "lightdm-gtk-greeter"], dm: "lightdm" }),
    ("budgie", DePackages { packages: &["budgie-desktop", "nautilus", "lightdm", "lightdm-gtk-greeter"], dm: "lightdm" }),
    // A tiling WM: we do not bundle a DM: the login screen is system-wide and
    // is installed by install_de -> ensure_display_manager (Python). `dm` is a
    // reference fallback.
    ("hyprland", DePackages { packages: &["hyprland", "waybar", "wofi", "kitty", "grim", "slurp", "wl-clipboard"], dm: "sddm" }),
    ("lxqt", DePackages { packages: &["lxqt-session", "lxqt-panel", "lxqt-config", "lxqt-policykit", "lxqt-runner", "lxqt-notificationd", "pcmanfm-qt", "qterminal", "openbox", "sddm"], dm: "sddm" }),
];

/// `ROSA_DE_PACKAGES`: ROSA Linux's own task-* metapackages.
pub const ROSA_DE_PACKAGES: &[(&str, DePackages)] = &[
    ("kde", DePackages { packages: &["task-kde5", "sddm"], dm: "sddm" }),
    ("gnome", DePackages { packages: &["task-gnome", "gdm"], dm: "gdm" }),
    ("xfce", DePackages { packages: &["task-xfce", "lightdm", "lightdm-gtk-greeter"], dm: "lightdm" }),
    ("mate", DePackages { packages: &["task-mate", "lightdm", "lightdm-gtk-greeter"], dm: "lightdm" }),
    ("lxqt", DePackages { packages: &["task-lxqt", "sddm"], dm: "sddm" }),
];

/// `ROSA_BRANDING_PACKAGES`: optional ROSA branded applets on top of a DE.
pub const ROSA_BRANDING_PACKAGES: &[(&str, &[&str])] = &[
    ("kde", &["simplewelcome", "klook"]),
];

/// Find an entry in an ordered table by key (the analogue of `dict.get`).
fn lookup<'a, V>(table: &'a [(&str, V)], key: &str) -> Option<&'a V> {
    table.iter().find(|(k, _)| *k == key).map(|(_, v)| v)
}

/// Detector input data (what `DistroCompat` takes from `DistroDetector` in
/// Python: `distro_id`, `family`, `pretty_name` and the os-release fields
/// `_info`).
#[derive(Debug, Clone)]
pub struct Detector {
    pub distro_id: String,
    pub family: PkgFamily,
    pub pretty_name: String,
    /// os-release VARIANT_ID (for detecting Astra SE).
    pub variant_id: String,
    /// os-release NAME (for detecting Astra SE).
    pub name: String,
}

/// The `DISTRO_INFO` entry key for a detector. Usually the os-release `ID`.
/// The exception is a flavour that carries its parent's ID: Ubuntu Kylin
/// has `ID=ubuntu`, and only `NAME="Ubuntu Kylin"` tells it apart (the
/// ubuntukylin-default-settings package rewrites NAME and PRETTY_NAME).
pub fn info_key(detector: &Detector) -> &str {
    let named = |needle: &str| {
        detector.name.to_lowercase().contains(needle) || detector.pretty_name.to_lowercase().contains(needle)
    };
    if detector.distro_id == "ubuntu" && named("kylin") {
        return "ubuntukylin";
    }
    &detector.distro_id
}

/// Extended distribution compatibility check, a port of the `DistroCompat`
/// class. Holds the detector data and the `DISTRO_INFO` entry found.
pub struct DistroCompat {
    detector: Detector,
    info: Option<DistroInfo>,
}

impl DistroCompat {
    /// Create from a detector: the `DISTRO_INFO` entry by [`info_key`].
    pub fn new(detector: Detector) -> Self {
        let info = lookup(DISTRO_INFO, info_key(&detector)).copied();
        Self { detector, info }
    }

    /// Whether the distribution is Russian (`country == "RU"`).
    pub fn is_russian_distro(&self) -> bool {
        self.info.map(|i| i.country == "RU").unwrap_or(false)
    }

    /// Distribution name (from the database or the detector's `pretty_name`).
    pub fn distro_name(&self) -> String {
        match self.info {
            Some(i) => i.name.to_string(),
            None => self.detector.pretty_name.clone(),
        }
    }

    /// Whether the distribution is in the compatibility database.
    pub fn is_known(&self) -> bool {
        self.info.is_some()
    }

    /// Whether DEs are available in the regular repositories (yes by default).
    pub fn has_de_repos(&self) -> bool {
        self.info.map(|i| i.has_de_repos).unwrap_or(true)
    }

    /// Full information about the distribution.
    pub fn get_info(&self) -> Option<DistroInfo> {
        self.info
    }

    /// Whether this is SteamOS: Steam Deck OR Steam Machine. Only `ID=steamos`
    /// from os-release decides: both machines share it (the OS image is
    /// common), while the Steam Machine's `VARIANT_ID` has not been confirmed
    /// on the device and cannot be relied on. Relatives by `ID_LIKE` (HoloISO,
    /// Bazzite) do not count: their systems are built differently.
    pub fn is_steamos(&self) -> bool {
        is_steamos_id(&self.detector.distro_id)
    }

    /// Whether the system is Astra Linux SE (by os-release NAME/VARIANT_ID).
    ///
    /// Repeats Python's substring checks: `"special" in name`,
    /// `"smolensk" in variant`, `"se" in variant` (case-insensitive). Public
    /// for the rexon-py bridge: through it the shim keeps the ZPS check lazy
    /// 1:1 with Python (the callback is called only on Astra SE).
    pub fn is_astra_se(&self) -> bool {
        let variant = self.detector.variant_id.to_lowercase();
        let name = self.detector.name.to_lowercase();
        name.contains("special") || variant.contains("smolensk") || variant.contains("se")
    }

    /// Warnings for the user. `zps_enabled` is the ZPS state (in Python
    /// `_is_zps_enabled()`; a parameter here, since it is subprocess+fs).
    pub fn get_warnings(&self, zps_enabled: bool) -> Vec<String> {
        let mut warnings: Vec<String> = Vec::new();

        if let Some(i) = self.info {
            // The table strings are marked `n_`: they are translated here, where shown.
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

    /// The adjusted DE package list (None -> use the common PackageDB).
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

    /// Optional ROSA branded applets on top of a DE (empty if not ROSA).
    pub fn get_rosa_branding(&self, de_id: &str) -> Vec<String> {
        if self.detector.distro_id != "rosa" {
            return Vec::new();
        }
        lookup(ROSA_BRANDING_PACKAGES, de_id)
            .map(|v| v.iter().map(|s| s.to_string()).collect())
            .unwrap_or_default()
    }

    /// Compatibility badge for the UI.
    pub fn get_compatibility_badge(&self) -> String {
        let Some(i) = self.info else {
            return format!("ℹ️ {}", self.detector.pretty_name);
        };
        let name = i.name;
        // The family is not set by the table (Kylin OS: one ID for the Debian and
        // RPM editions): take what the detector found on the machine.
        let family = if i.family == PkgFamily::Unknown { self.detector.family } else { i.family };
        let family = family.as_str();
        if !i.has_de_repos {
            // TRANSLATORS: compatibility badge; %1 is the distribution name.
            return format!("⚠️ {}", trf("%1 (limited repositories)", &[&name]));
        }
        if !i.warnings.is_empty() {
            return format!("⚠️ {name} ({family})");
        }
        format!("✅ {name} ({family})")
    }

    /// Why installing a DE is possible/impossible: the ONLY place where this
    /// rule lives. Both the short `check_install_possible` string and the human
    /// explanation for the install window are built from the result: two
    /// copies of the same branches would drift apart silently (rule #12).
    ///
    /// `zps_enabled` is the ZPS state (a parameter, see `get_warnings`).
    pub fn install_reason(&self, de_id: &str, zps_enabled: bool) -> InstallReason {
        // Astra SE with ZPS.
        if self.detector.distro_id == "astra" && self.is_astra_se() {
            if zps_enabled {
                return InstallReason::AstraZps;
            }
            if !self.has_de_repos() {
                return InstallReason::AstraNoRepos { de_id: de_id.to_string() };
            }
        }

        // OSnova: closed repositories.
        if self.detector.distro_id == "osnova" {
            return InstallReason::OsnovaClosed;
        }

        // ALT: check that the packages exist.
        if self.detector.family == PkgFamily::Alt && lookup(ALT_DE_PACKAGES, de_id).is_none() {
            return InstallReason::AltUnsupportedDe { de_id: de_id.to_string() };
        }

        // COSMIC: a limited list of families.
        if de_id == "cosmic"
            && !matches!(
                self.detector.family,
                PkgFamily::Arch | PkgFamily::Fedora | PkgFamily::Suse | PkgFamily::Gentoo | PkgFamily::Void | PkgFamily::Alpine
            )
        {
            return InstallReason::CosmicUnavailable;
        }

        // ROSA: only 4 DEs have their own task-*.
        if self.detector.distro_id == "rosa" && lookup(ROSA_DE_PACKAGES, de_id).is_none() {
            return InstallReason::RosaNoTaskPackage { de_id: de_id.to_string() };
        }

        // SteamOS (Steam Deck and Steam Machine): never install as packages: a
        // system update would erase the whole desktop (until 28.09.2026 this was
        // "allowed, with a warning" through `steamos-readonly disable`). Since
        // 25.10.2026 verified desktops are installed as systemd-sysext images
        // (`crate::steamos_sysext`; the executor redirects the installation there
        // itself); the others get an honest refusal with a reason.
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

    /// Check whether installing a DE is possible. Returns (ok, message).
    /// `zps_enabled` is the ZPS state (a parameter, see `get_warnings`).
    ///
    /// The short form for the log and for code that needs one string; the
    /// decision and the text come from [`DistroCompat::install_reason`].
    pub fn check_install_possible(&self, de_id: &str, zps_enabled: bool) -> (bool, String) {
        let reason = self.install_reason(de_id, zps_enabled);
        (reason.allowed(), reason.short_message())
    }
}

// ── Explanation of the install decision (for the DE install window) ────────

/// The old "Russian or English" choice via text pairs in the code.
///
/// ⚠️ Being phased out (29.09.2026): the labels are moving to the
/// translation catalog (`crate::i18n`), where there is one language per
/// process. In `InstallReason` and the install wizard the `Lang` parameter
/// no longer decides anything and stays only for the callers; do not add
/// new `pick`s. Locale parsing is not our own: it already exists in
/// [`crate::translator::user_lang_from_locale`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Ru,
    En,
}

impl Lang {
    /// Language from a locale string (`ru_RU.UTF-8` -> [`Lang::Ru`]). Anything
    /// not Russian gets English: it is clearer than "raw" Russian to someone
    /// who does not know Russian, and we do not undertake to cover every
    /// language in the world.
    pub fn from_locale(locale_str: &str) -> Self {
        Self::from_code(&crate::translator::user_lang_from_locale(locale_str))
    }

    /// Language from an ISO 639-1 code (`ru` -> [`Lang::Ru`]).
    pub fn from_code(code: &str) -> Self {
        if code.eq_ignore_ascii_case("ru") {
            Lang::Ru
        } else {
            Lang::En
        }
    }

    /// The customer's language from environment variables, in the order the
    /// locale system itself reads them: `LC_ALL`, `LC_MESSAGES`, `LANG`.
    ///
    /// The walk over the variables itself is in
    /// [`crate::translator::locale_from_env`]: by 07.09.2026 the same loop lay
    /// in two copies in the Qt layer (the tray icon and the "Appearance" tab),
    /// and with the arrival of description translation what was needed was not
    /// "Russian or not" but the locale STRING; only one truth about it remained
    /// (rule #12).
    pub fn from_env() -> Self {
        let locale = crate::translator::locale_from_env();
        if locale.is_empty() {
            return Lang::En;
        }
        Self::from_locale(&locale)
    }

    /// The language code as a string; the bridge answer is tagged with it too.
    pub fn code(self) -> &'static str {
        match self {
            Lang::Ru => "ru",
            Lang::En => "en",
        }
    }

    /// Pick one of two text variants. For new labels use `tr`.
    pub fn pick(self, ru: &str, en: &str) -> String {
        match self {
            Lang::Ru => ru.to_string(),
            Lang::En => en.to_string(),
        }
    }
}

/// Outcome of the check: install silently, install with a warning, or do not install.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallOutcome {
    /// No obstacles: nothing to ask about.
    Allowed,
    /// Installing is possible, but the person must know what they are getting into.
    Warning,
    /// Must not install: the installation is certain to fail.
    Blocked,
}

impl InstallOutcome {
    /// The outcome as a string, for the bridge and the log.
    pub fn code(self) -> &'static str {
        match self {
            InstallOutcome::Allowed => "ok",
            InstallOutcome::Warning => "warning",
            InstallOutcome::Blocked => "blocked",
        }
    }
}

/// The reason for the decision. The texts derive from it, so a new case is
/// added as one enum variant and one line in each `match` below; the
/// rule's branching is not touched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallReason {
    /// No restrictions.
    Ok,
    /// Astra Linux SE with the closed software environment enabled.
    AstraZps,
    /// Astra Linux: no DE in the regular repositories.
    AstraNoRepos { de_id: String },
    /// OSnova: there are no public repositories.
    OsnovaClosed,
    /// ALT Linux: no package mapping is described for this DE.
    AltUnsupportedDe { de_id: String },
    /// COSMIC is not built for this distribution's family.
    CosmicUnavailable,
    /// ROSA: no own `task-*` metapackage; installing at one's own risk.
    RosaNoTaskPackage { de_id: String },
    /// SteamOS (Steam Deck and Steam Machine): this desktop has not yet been
    /// verified as a systemd-sysext image, so installation is FORBIDDEN (we
    /// never install packages on SteamOS: an OS update erases everything
    /// installed on top).
    SteamOsNotYet { de_id: String },
}

/// Comma-separated list of DEs from the package table.
fn de_list(table: &[(&str, DePackages)]) -> String {
    table
        .iter()
        .map(|(k, _)| *k)
        .collect::<Vec<&str>>()
        .join(", ")
}

impl InstallReason {
    /// Stable reason code, for the log, tests and the bridge.
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

    /// Outcome by reason.
    pub fn outcome(&self) -> InstallOutcome {
        match self {
            InstallReason::Ok => InstallOutcome::Allowed,
            InstallReason::RosaNoTaskPackage { .. } => InstallOutcome::Warning,
            _ => InstallOutcome::Blocked,
        }
    }

    /// Whether installation is allowed.
    pub fn allowed(&self) -> bool {
        self.outcome() != InstallOutcome::Blocked
    }

    /// Short string for the log, the historical form of
    /// `check_install_possible` (checked against a recorded golden file).
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
            // The same summary the customer reads in the window and on the Steam
            // page.
            InstallReason::SteamOsNotYet { .. } => steamos_summary(),
        }
    }

    /// Window title: one phrase, no jargon (rule #13).
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

    /// What is in the way, in plain language, without config or command names.
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

    /// What to do about it. Empty only when there is nothing to do.
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

    /// A ready explanation in one piece: what is in the way and what to do.
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

/// One item of the regional distributions list for the UI.
///
/// Field order = key order of the Python dictionary in `_get_regional_distros`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RegionalDistro {
    pub id: String,
    pub name: String,
    pub family: PkgFamily,
    pub country: String,
    pub has_de_repos: bool,
    pub has_warnings: bool,
}

/// Distributions by country (a port of `_get_regional_distros`): one per
/// name, in `DISTRO_INFO` order, duplicate names filtered out.
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

/// Russian distributions for the UI (a port of `get_supported_russian_distros`).
pub fn get_supported_russian_distros() -> Vec<RegionalDistro> {
    get_regional_distros("RU")
}

/// Regional distributions (a port of `get_supported_regional_distros`).
/// `countries = None` -> RU, CN, IN (in this order).
pub fn get_supported_regional_distros(countries: Option<&[&str]>) -> Vec<RegionalDistro> {
    let default: [&str; 3] = ["RU", "CN", "IN"];
    let list = countries.unwrap_or(&default);
    let mut result = Vec::new();
    for c in list {
        result.extend(get_regional_distros(c));
    }
    result
}

// ── SteamOS (Steam Deck and Steam Machine): what Rexon does not do there ───
//
// SteamOS is an atomic system: the `/usr` system partition is read-only,
// and an update replaces it entirely and erases everything installed on
// top. Everything Rexon
// writes to the home folder (user themes, live wallpapers, the login
// splash) works and survives updates. Everything it writes to the system
// does not, and it is more honest to show this up front than to let the
// person press "Apply" and stay silent. The decision "what is not allowed"
// lives here, in ONE place, and the tabs only ask (rule #12).

/// Summary for the customer, first half: what works.
pub const STEAMOS_SUMMARY_WORKS: &str =
    // TRANSLATORS: SteamOS runs on Steam Deck and Steam Machine. "Startup movie" is the short video Rexon plays right after you log in.
    n_("Themes, live wallpapers and the startup movie work on SteamOS and survive system updates.");
/// Summary for the customer, second half: which desktops can be installed
/// (the list is `steamos_sysext::DESKTOPS`, checked by
/// `steamos_summary_names_the_built_desktops`).
pub const STEAMOS_SUMMARY_DESKTOPS: &str = n_(
    "Xfce, LXQt and Hyprland can be installed on SteamOS: Rexon builds them as system \
     extensions and rebuilds them by itself after every system update.",
);
/// Title of the window refusing to install a desktop (the desktop is not verified).
pub const STEAMOS_DESKTOPS_TITLE: &str =
    n_("SteamOS: this desktop environment can't be installed yet");

/// The whole summary, for the log, the short form and the Steam page.
pub fn steamos_summary() -> String {
    format!("{} {}", tr(STEAMOS_SUMMARY_WORKS), tr(STEAMOS_SUMMARY_DESKTOPS))
}

/// Whether this is SteamOS by the `ID` from os-release. One check for the
/// whole product: both [`DistroCompat::is_steamos`] and the live
/// [`running_on_steamos`] call it.
pub fn is_steamos_id(distro_id: &str) -> bool {
    distro_id.trim().eq_ignore_ascii_case("steamos")
}

/// Live check: whether we run on SteamOS (Steam Deck or Steam Machine).
/// Reads `/etc/os-release` with the same detector as the rest of the
/// product.
pub fn running_on_steamos() -> bool {
    is_steamos_id(&crate::system::DistroDetector::detect_live().distro_id())
}

/// What Rexon does NOT do on SteamOS: anything that writes to the system partition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SteamOsBlock {
    /// SteamOS's own packages: adding to the current desktop, shell bundles,
    /// changing the login screen. Desktops are installed differently, as
    /// systemd-sysext images (`crate::steamos_sysext`), and their cards have a
    /// reason of their own ([`steamos_desktop_card_block`]).
    Desktops,
    /// Login screen theme. On SteamOS the login screen handles the switch to
    /// game mode, so we do not touch it at all.
    LoginScreen,
    /// Boot splash: a SteamOS update rebuilds the boot image and brings its own
    /// back.
    BootSplash,
    /// The rest of a theme's system part (the login screen background, system
    /// panel plugins, GNOME shell themes): everything installed with the
    /// administrator password into the system partition.
    SystemPart,
}

impl SteamOsBlock {
    /// The reason in one phrase, next to a disabled button or in the refusal window.
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

    /// The reason if the action is forbidden on this system; `None` means allowed.
    pub fn on(self, steamos: bool) -> Option<String> {
        steamos.then(|| self.reason())
    }

    /// The same for the live system.
    pub fn live(self) -> Option<String> {
        self.on(running_on_steamos())
    }
}

/// The reason for disabled install and remove buttons on the card of
/// desktop `de_id` on SteamOS; `None` means the buttons work (the desktop
/// is installed as a systemd-sysext image) or this is not SteamOS.
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

// ── Deterministic parts of the side-effect functions (pure) ────────────────

/// The APT-RPM sign (ALT Linux): apt-get and rpm exist, but dpkg does not.
/// A port of `is_apt_rpm_system` without consulting PATH (the predicates
/// are parameters).
pub fn is_apt_rpm(has_apt: bool, has_rpm: bool, has_dpkg: bool) -> bool {
    has_apt && has_rpm && !has_dpkg
}

/// Parse the ALT Linux variant from the contents of /etc/os-release.
/// A port of the `detect_alt_variant` logic (without reading the file: the
/// effect is in the wrapper).
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

/// Whether ZPS is enabled on Astra SE. A port of the `_is_zps_enabled`
/// logic without subprocess/fs: `lsmod` (if captured) contains
/// `digsig_verif`, or the ZPS config contains `DIGSIG_ELF_MODE=1`. The
/// effects (running lsmod, reading the file) are in the wrapper.
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

// ── Tests of the install explanation ───────────────────────────────────────

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

    /// All reasons are enumerated in one list: a new enum variant must get a
    /// title, an explanation and advice, otherwise the test fails.
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

    /// The short string did not change in any branch: a recorded golden file
    /// checks it, so a divergence is caught here before the roundtrip test.
    ///
    /// ⚠️ A deliberate change on 28.09.2026: SteamOS has a different string:
    /// the old one promised installation by lifting the protection, and now
    /// installation is forbidden (the very first SteamOS update would erase
    /// what was installed). The English source is checked:
    /// the Russian lives in the translation catalog.
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
            // 25.10.2026: on SteamOS verified desktops are installed as
            // systemd-sysext images; only unverified ones are refused.
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

    /// The short and the detailed form always share the same "allowed/not allowed" decision.
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

    /// No reason stays silent: each has a title, an explanation and advice,
    /// and the explanation is built from them. The Russian is checked word for
    /// word by the snapshot `installer_parity::russian_installer_texts_as_before`
    /// (the `de_reason_*` section) under a pinned Russian catalog.
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
        // "All is fine" explains nothing: the window has nothing to show.
        assert!(InstallReason::Ok.explanation(Lang::En).is_empty());
        assert_eq!(InstallReason::Ok.outcome(), InstallOutcome::Allowed);
    }

    /// The English text contains no Cyrillic (the typical "forgot to translate" mistake).
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

    /// The language comes from the locale via the same parsing as the automatic description translation.
    #[test]
    fn lang_from_locale() {
        assert_eq!(Lang::from_locale("ru_RU.UTF-8"), Lang::Ru);
        assert_eq!(Lang::from_locale("ru"), Lang::Ru);
        assert_eq!(Lang::from_locale("en_US.UTF-8"), Lang::En);
        assert_eq!(Lang::from_locale(""), Lang::En);
        assert_eq!(Lang::from_locale("de_DE"), Lang::En);
        assert_eq!(Lang::Ru.code(), "ru");
    }

    /// os-release of the Steam Deck (captured from the device) and the Steam
    /// Machine (the same OS; its `VARIANT_ID` is not confirmed, so both a
    /// foreign and a missing one are checked). All three are SteamOS; lookalike
    /// relatives are not.
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

    /// Steam Deck and Steam Machine are recognised the same way, by `ID`, not
    /// by `VARIANT_ID`; on both, verified desktops are installed as
    /// systemd-sysext images, the others are forbidden with a reason.
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

    /// Texts for the customer: no commands and no "we will lift the
    /// protection" promises; they say what works and that installing desktop
    /// environments is not supported yet, without any timeline.
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

    /// The summary names exactly the desktops that are built as images.
    #[test]
    fn steamos_summary_names_the_built_desktops() {
        let names: Vec<&str> = crate::steamos_sysext::DESKTOPS.iter().map(|d| d.name).collect();
        assert_eq!(names, ["Xfce", "LXQt", "Hyprland"]);
        assert!(STEAMOS_SUMMARY_DESKTOPS.starts_with("Xfce, LXQt and Hyprland can be installed"));
    }

    /// Cards on SteamOS: verified desktops have working buttons, Plasma and
    /// unverified ones have a reason; outside SteamOS there are no reasons.
    #[test]
    fn steamos_card_blocks() {
        assert_eq!(steamos_desktop_card_block("xfce", true), None);
        assert_eq!(steamos_desktop_card_block("kde", true), Some(SteamOsBlock::Desktops.reason()));
        assert!(steamos_desktop_card_block("gnome", true).is_some());
        assert_eq!(steamos_desktop_card_block("gnome", false), None);
    }

    /// Each action forbidden on SteamOS names its reason in one phrase, and
    /// different actions have different reasons; outside SteamOS nothing is
    /// forbidden.
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

    /// Astra SE with ZPS is a block, and it does not depend on the chosen DE.
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
