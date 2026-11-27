//! System layer: a port of `rexon/system.py` (pure logic ONLY).
//!
//! Ported:
//!   * `PkgFamily`: the package manager family (enum);
//!   * `DistroDetector.ID_MAP` + distribution detection: os-release parsing
//!     (the text is passed as a parameter, the file fallback /etc → /usr/lib
//!     is in `parse_os_release_files`; live reading is the thin wrapper
//!     `DistroDetector::detect_live`), binary-based detection is moved behind
//!     the `has_binary` predicate (like `command_exists` in settings.rs);
//!   * `PackageDB`: the whole built-in DE database (`DE_DATABASE`, 15 DEs,
//!     including LXQt, Wayfire and the tiling niri/sway/river, × package
//!     families) + UI metadata (`DE_META`) and the functions choosing
//!     packages for (de, family);
//!   * pure installer algorithms: `expand_groups` (expanding pacman groups),
//!     `parse_qi_field`/`qi_field` (parsing `pacman -Qi`),
//!     `required_by_outside`, `filter_removable_transitive` (forbidden set +
//!     transitive closure over `Depends On`). The pacman output is passed as
//!     a fixture closure; the live `pacman -Qg`/`-Qi` calls (under LANG=C: a
//!     Russian locale breaks field parsing, see the `russian-locale-*` test)
//!     stay on the executor's side.
//!
//! BOUNDARY. Everything that EXECUTES (runs commands, goes to pkexec, reads
//! the live file system) lives in two neighbours, not here:
//!   * decisions and argv (helper script texts, polkit policies, install
//!     plans, rollback of Plasma forks / Plymouth / SDDM, login screen
//!     detection): [`crate::system_plan`];
//!   * the trips outside themselves: [`crate::system_exec`]: `pkexec`
//!     helpers, `systemctl is-enabled`, package manager queries, the
//!     `/etc/sddm.conf.d` and `/usr/share/plymouth/themes` directories,
//!     `logout_session`. Side calls there are injected via the `SystemIo`
//!     trait, so the executor is tested without a live machine.
//!
//! STILL IN PYTHON (next round): installing, switching and removing desktops
//! (`install_de`/`switch_de`/`remove_de`,
//! `PackageManager.install/install_batch/remove/update_db/run_post_install`,
//! `ensure_display_manager`, the AeroShell rebuild) and live installation
//! checks of `SystemInfo`/`DesktopEnvironment`.
//!
//! Each ported function is checked against the Python oracle by the golden
//! tests `tests/system_parity.rs` (fixtures `tests/golden/system_*.json` were
//! taken from the live `rexon.system`).

use std::collections::HashSet;
use std::os::unix::fs::PermissionsExt;

use serde::{Serialize, Serializer};
use serde_json::{Map, Value};

use crate::i18n::n_;

// ============================================================================
// PkgFamily
// ============================================================================

/// Package manager family: a port of `PkgFamily(str, Enum)`.
///
/// The values match the string `PkgFamily.<X>.value`, so that serialization
/// (`family.value`) and os-release parsing give the same as Python.
/// `distro_compat.rs` re-exports this type (in Python the enum lives in
/// system.py, and distro_compat.py imports it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PkgFamily {
    Arch,
    Debian,
    Fedora,
    Suse,
    Gentoo,
    Alpine,
    Void,
    Nix,
    Solus,
    Clear,
    Alt,
    Unknown,
}

impl PkgFamily {
    /// String value of the enum, like `PkgFamily.<X>.value` in Python.
    pub fn as_str(self) -> &'static str {
        match self {
            PkgFamily::Arch => "arch",
            PkgFamily::Debian => "debian",
            PkgFamily::Fedora => "fedora",
            PkgFamily::Suse => "suse",
            PkgFamily::Gentoo => "gentoo",
            PkgFamily::Alpine => "alpine",
            PkgFamily::Void => "void",
            PkgFamily::Nix => "nix",
            PkgFamily::Solus => "solus",
            PkgFamily::Clear => "clear",
            PkgFamily::Alt => "alt",
            PkgFamily::Unknown => "unknown",
        }
    }

    /// Parse from the string value (the reverse of `as_str`); unknown → None.
    /// Named `from_value`, not `from_str`, to avoid confusion with
    /// `std::str::FromStr` (that returns `Result`, here the semantics are
    /// Python's `PkgFamily(value)`).
    pub fn from_value(s: &str) -> Option<Self> {
        Some(match s {
            "arch" => PkgFamily::Arch,
            "debian" => PkgFamily::Debian,
            "fedora" => PkgFamily::Fedora,
            "suse" => PkgFamily::Suse,
            "gentoo" => PkgFamily::Gentoo,
            "alpine" => PkgFamily::Alpine,
            "void" => PkgFamily::Void,
            "nix" => PkgFamily::Nix,
            "solus" => PkgFamily::Solus,
            "clear" => PkgFamily::Clear,
            "alt" => PkgFamily::Alt,
            "unknown" => PkgFamily::Unknown,
            _ => return None,
        })
    }
}

/// Serialized as the string value (the analogue of `PkgFamily.value` in dumps).
impl Serialize for PkgFamily {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_str(self.as_str())
    }
}

// ============================================================================
// DistroDetector
// ============================================================================

/// Parse one os-release text: a port of `DistroDetector._parse_os_release`
/// (the loop body): lines with `=` that are not comments; key/value are
/// trimmed, the value is stripped of surrounding `"` then `'` (exactly in
/// this order, like `strip('"').strip("'")`). A repeated key overwrites the
/// value, keeping the position (dict semantics).
pub fn parse_os_release(text: &str) -> Map<String, Value> {
    let mut result = Map::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.contains('=') && !line.starts_with('#') {
            let (k, v) = line.split_once('=').expect("contains('=') проверен");
            let v = v.trim().trim_matches('"').trim_matches('\'');
            result.insert(k.trim().to_string(), Value::from(v));
        }
    }
    result
}

/// File fallback: a port of the loop `for path in ("/etc/os-release",
/// "/usr/lib/os-release")`: the FIRST existing file is parsed and returned
/// (even if it is empty, the second is not looked at); neither exists → {}.
/// The parameters are file contents (None = FileNotFoundError).
pub fn parse_os_release_files(
    etc: Option<&str>,
    usr_lib: Option<&str>,
) -> Map<String, Value> {
    if let Some(text) = etc {
        return parse_os_release(text);
    }
    if let Some(text) = usr_lib {
        return parse_os_release(text);
    }
    Map::new()
}

/// Whether an executable is in PATH: the analogue of `shutil.which(name) is not None`.
/// A live wrapper for the `has_binary` predicate; tests pass a fixture predicate.
pub fn binary_in_path(name: &str) -> bool {
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    for dir in std::env::split_paths(&paths) {
        let p = dir.join(name);
        if let Ok(md) = std::fs::metadata(&p) {
            if md.is_file() && md.permissions().mode() & 0o111 != 0 {
                return true;
            }
        }
    }
    false
}

/// Family lookup in `ID_MAP` (linear over the const table).
pub fn id_map_lookup(id: &str) -> Option<PkgFamily> {
    ID_MAP.iter().find(|(k, _)| *k == id).map(|(_, f)| *f)
}

/// Detects the distribution and the package manager family: a port of the
/// `DistroDetector` class (without reading files in the constructor: `_info`
/// is built from the os-release text passed as a parameter).
pub struct DistroDetector {
    /// Parsed os-release: Python's `self._info`.
    pub info: Map<String, Value>,
}

impl DistroDetector {
    /// Constructor from the contents of the os-release files (None = no file).
    pub fn from_os_release_files(etc: Option<&str>, usr_lib: Option<&str>) -> Self {
        Self { info: parse_os_release_files(etc, usr_lib) }
    }

    /// Live detection: reads /etc/os-release, then /usr/lib/os-release, like
    /// `DistroDetector()` in Python (a read error = the file is missing).
    pub fn detect_live() -> Self {
        let etc = std::fs::read_to_string("/etc/os-release").ok();
        let usr_lib = std::fs::read_to_string("/usr/lib/os-release").ok();
        Self::from_os_release_files(etc.as_deref(), usr_lib.as_deref())
    }

    fn get(&self, key: &str) -> Option<&str> {
        self.info.get(key).and_then(Value::as_str)
    }

    /// Distribution / OS name: NAME → PRETTY_NAME → "Linux" (by the PRESENCE of
    /// the key, not by being non-empty: an empty NAME="" is returned as is).
    pub fn name(&self) -> String {
        if let Some(v) = self.get("NAME") {
            return v.to_string();
        }
        if let Some(v) = self.get("PRETTY_NAME") {
            return v.to_string();
        }
        "Linux".to_string()
    }

    /// Pretty name: PRETTY_NAME → name().
    pub fn pretty_name(&self) -> String {
        match self.get("PRETTY_NAME") {
            Some(v) => v.to_string(),
            None => self.name(),
        }
    }

    /// Distribution ID (from os-release), lowercase.
    pub fn distro_id(&self) -> String {
        self.get("ID").unwrap_or("").to_lowercase()
    }

    /// Parent distributions (ID_LIKE), lowercase.
    pub fn id_like(&self) -> Vec<String> {
        self.get("ID_LIKE")
            .unwrap_or("")
            .split_whitespace()
            .map(str::to_lowercase)
            .collect()
    }

    /// Distribution version (VERSION_ID) or "".
    pub fn version_id(&self) -> String {
        self.get("VERSION_ID").unwrap_or("").to_string()
    }

    /// Package manager family: a port of the `family` property.
    ///
    /// Order: a direct ID match → fallback to ID_LIKE → detection by binaries
    /// through the `has_binary` predicate (live: `binary_in_path`).
    /// Special case APT-RPM: ALT has apt-get + rpm, but NO dpkg.
    pub fn family(&self, has_binary: &dyn Fn(&str) -> bool) -> PkgFamily {
        if let Some(f) = id_map_lookup(&self.distro_id()) {
            return f;
        }
        for like in self.id_like() {
            if let Some(f) = id_map_lookup(&like) {
                return f;
            }
        }
        if has_binary("apt-get") {
            if has_binary("dpkg") {
                return PkgFamily::Debian;
            }
            if has_binary("rpm") {
                return PkgFamily::Alt;
            }
            return PkgFamily::Debian; // assume Debian
        }
        for (binary, family) in [
            ("pacman", PkgFamily::Arch),
            ("dnf", PkgFamily::Fedora),
            ("yum", PkgFamily::Fedora),
            ("zypper", PkgFamily::Suse),
            ("emerge", PkgFamily::Gentoo),
            ("apk", PkgFamily::Alpine),
            ("xbps-install", PkgFamily::Void),
            ("nix-env", PkgFamily::Nix),
            ("eopkg", PkgFamily::Solus),
            ("swupd", PkgFamily::Clear),
        ] {
            if has_binary(binary) {
                return family;
            }
        }
        PkgFamily::Unknown
    }

    /// Family with live binary detection (PATH).
    pub fn family_live(&self) -> PkgFamily {
        self.family(&binary_in_path)
    }

    /// Is this SteamOS?
    pub fn is_steamos(&self) -> bool {
        self.distro_id().contains("steamos") || self.name().to_lowercase().contains("steamos")
    }

    /// Immutable distribution?
    pub fn is_immutable(&self) -> bool {
        const IMMUTABLE_IDS: [&str; 7] = [
            "steamos", "nixos", "bazzite", "silverblue", "kinoite", "sericea", "onyx",
        ];
        let name = self.name().to_lowercase();
        IMMUTABLE_IDS.contains(&self.distro_id().as_str())
            || name.contains("atomic")
            || name.contains("silverblue")
            || self
                .get("VARIANT_ID")
                .unwrap_or("")
                .to_lowercase()
                .contains("ostree")
    }
}

// ============================================================================
// PackageDB: the BUILT-IN package database (port of the PackageDB class)
// ============================================================================

/// An entry of `_DE_DATABASE[de_id]`. Package lists are in the insertion order
/// of the Python dict (the family order does not matter for lookup, but is kept).
pub struct DeEntry {
    pub packages: &'static [(PkgFamily, &'static [&'static str])],
    pub aur: &'static [&'static str],
    /// PACKAGE name for asking the package manager "is the DE installed". Not a
    /// binary: the executable is named differently (`plasma-desktop` →
    /// `plasmashell`, `hyprland` → `Hyprland`). See [`get_check_binary`].
    pub check_package: &'static str,
    pub config_paths: &'static [&'static str],
}

/// CPU load of a desktop: an ATTRIBUTE, not a caption.
///
/// It used to be a Russian string (Low/Medium/High), and «Comparison»
/// coloured the cell by comparing its text with a list; with translation
/// each language has its own caption, so the colour decision relies on the
/// variant, and the text is only for display ([`CpuLoad::label`]). `Display`
/// and `Serialize` give the ready caption in the UI language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpuLoad {
    Low,
    Medium,
    High,
}

impl CpuLoad {
    /// Caption in the UI language.
    pub fn label(self) -> String {
        match self {
            CpuLoad::Low => crate::i18n::trc("CPU load", "Low"),
            CpuLoad::Medium => crate::i18n::trc("CPU load", "Medium"),
            CpuLoad::High => crate::i18n::trc("CPU load", "High"),
        }
    }
}

impl std::fmt::Display for CpuLoad {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label())
    }
}

impl Serialize for CpuLoad {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.label())
    }
}

/// Parity checks compare the ready cell caption with the desktop's attribute.
impl PartialEq<CpuLoad> for String {
    fn eq(&self, other: &CpuLoad) -> bool {
        *self == other.label()
    }
}

/// DE metadata for the UI: an element of `_DE_META`. The field order matches
/// the key order of the Python dict (important for dump comparison).
///
/// Human-facing texts (`description`, `compositor` for LXQt) are English
/// `msgid`s of the translation catalog; show them through
/// [`DeMeta::description_text`] and [`DeMeta::compositor_text`], not the field
/// directly. The other strings are program and technology names, they are
/// not translated.
#[derive(Serialize)]
pub struct DeMeta {
    pub id: &'static str,
    pub display_name: &'static str,
    pub description: &'static str,
    pub session_name: &'static str,
    pub ram_mb: u32,
    pub cpu_usage: CpuLoad,
    pub customizable: u8,
    pub gaming_friendly: u8,
    pub toolkit: &'static str,
    /// Desktop session. For desktops with an X11/Wayland choice at install time
    /// ([`SESSION_VARIANTS`]) this is the original meta entry (it drives the
    /// colour in «Comparison»); the showcase uses [`DeMeta::session_text`], and
    /// the default choice is [`SessionVariants::default_session`], both from the
    /// same table.
    pub session_type: &'static str,
    pub compositor: &'static str,
    pub panel_type: &'static str,
    pub file_manager: &'static str,
    pub terminal: &'static str,
}

impl DeMeta {
    /// Desktop description in the UI language.
    pub fn description_text(&self) -> String {
        crate::i18n::tr(self.description)
    }

    /// Compositor in the UI language (for most, the program name).
    pub fn compositor_text(&self) -> String {
        crate::i18n::tr(self.compositor)
    }

    /// Session in the UI language, for «Comparison» and the cards.
    ///
    /// If the desktop has a session choice ([`SESSION_VARIANTS`]), the caption is
    /// derived from the variants table instead of being written a second time by
    /// hand: this way the showcase does not diverge from the install dialog. The
    /// table is described for Arch, and the desktop metadata is written for it too.
    pub fn session_text(&self) -> String {
        match session_variants(self.id, PkgFamily::Arch).map(|v| v.maturity) {
            Some(WaylandMaturity::Mature) => crate::i18n::tr("X11 + Wayland"),
            Some(WaylandMaturity::Experimental) => crate::i18n::tr("X11 + Wayland (experimental)"),
            Some(WaylandMaturity::ExperimentalAur) => {
                crate::i18n::tr("X11 + Wayland (experimental, AUR)")
            }
            None => self.session_type.to_string(),
        }
    }
}

/// `DistroDetector.ID_MAP`: os-release ID → family, in insertion order.
pub const ID_MAP: &[(&str, PkgFamily)] = &[
    ("arch", PkgFamily::Arch),
    ("cachyos", PkgFamily::Arch),
    ("manjaro", PkgFamily::Arch),
    ("endeavouros", PkgFamily::Arch),
    ("garuda", PkgFamily::Arch),
    ("artix", PkgFamily::Arch),
    ("steamos", PkgFamily::Arch),
    ("archcraft", PkgFamily::Arch),
    ("blendos", PkgFamily::Arch),
    ("archbang", PkgFamily::Arch),
    ("crystal", PkgFamily::Arch),
    ("debian", PkgFamily::Debian),
    ("ubuntu", PkgFamily::Debian),
    ("linuxmint", PkgFamily::Debian),
    ("pop", PkgFamily::Debian),
    ("elementary", PkgFamily::Debian),
    ("zorin", PkgFamily::Debian),
    ("kali", PkgFamily::Debian),
    ("parrot", PkgFamily::Debian),
    ("mx", PkgFamily::Debian),
    ("mxlinux", PkgFamily::Debian),
    ("lmde", PkgFamily::Debian),
    ("raspbian", PkgFamily::Debian),
    ("peppermint", PkgFamily::Debian),
    ("tuxedo", PkgFamily::Debian),
    ("fedora", PkgFamily::Fedora),
    ("rhel", PkgFamily::Fedora),
    ("centos", PkgFamily::Fedora),
    ("almalinux", PkgFamily::Fedora),
    ("rocky", PkgFamily::Fedora),
    ("ol", PkgFamily::Fedora),
    ("nobara", PkgFamily::Fedora),
    ("ultramarine", PkgFamily::Fedora),
    ("bazzite", PkgFamily::Fedora),
    ("opensuse", PkgFamily::Suse),
    ("opensuse-leap", PkgFamily::Suse),
    ("opensuse-tumbleweed", PkgFamily::Suse),
    ("sle", PkgFamily::Suse),
    ("sles", PkgFamily::Suse),
    ("gentoo", PkgFamily::Gentoo),
    ("alpine", PkgFamily::Alpine),
    ("void", PkgFamily::Void),
    ("nixos", PkgFamily::Nix),
    ("solus", PkgFamily::Solus),
    ("clear-linux-os", PkgFamily::Clear),
    ("alt", PkgFamily::Alt),
    ("altlinux", PkgFamily::Alt),
    ("simply", PkgFamily::Alt),
    ("astra", PkgFamily::Debian),
    ("osnova", PkgFamily::Debian),
    ("boss", PkgFamily::Debian),
    ("redos", PkgFamily::Fedora),
    ("rosa", PkgFamily::Fedora),
    ("calculate", PkgFamily::Gentoo),
    // openKylin: `ID=openkylin` without `ID_LIKE` (openKylin base-files): the
    // family is written explicitly, without waiting for the binary probe. Kylin
    // OS (`ID=kylin`) is deliberately NOT here: the same ID is used by the
    // desktop edition on Debian and the server one on RPM, so the binaries
    // decide the family (`family`).
    ("openkylin", PkgFamily::Debian),
];

/// `_DE_DATABASE`: DE id → packages by family / AUR / check / configs. The
/// transcription was generated from live Python (see the header of
/// `tests/system_parity.rs`) and is checked in full by a golden test.
pub const DE_DATABASE: &[(&str, DeEntry)] = &[
    // cinnamon-mint
    ("cinnamon-mint", DeEntry {
        packages: &[
            (PkgFamily::Arch, &[
                "cinnamon", "cinnamon-translations", "nemo", "nemo-fileroller",
                "gnome-terminal", "lightdm", "lightdm-slick-greeter",
            ]),
            (PkgFamily::Debian, &[
                "cinnamon-desktop-environment", "lightdm",
                "lightdm-gtk-greeter", "nemo",
            ]),
            (PkgFamily::Fedora, &[
                "@cinnamon-desktop-environment", "cinnamon-themes",
                "mint-y-theme", "mint-y-icons", "mint-x-icons", "lightdm",
                "lightdm-gtk-greeter",
            ]),
            (PkgFamily::Suse, &[
                "patterns-cinnamon-cinnamon", "lightdm", "lightdm-gtk-greeter",
                "nemo",
            ]),
            (PkgFamily::Gentoo, &[
                "gnome-extra/cinnamon", "x11-misc/lightdm", "gnome-extra/nemo",
                "x11-terms/gnome-terminal",
            ]),
            (PkgFamily::Alpine, &[
                "cinnamon", "nemo", "gnome-terminal", "lightdm",
                "lightdm-gtk-greeter",
            ]),
            (PkgFamily::Solus, &[
                "cinnamon", "nemo", "gnome-terminal", "lightdm",
                "lightdm-gtk-greeter",
            ]),
            (PkgFamily::Void, &["cinnamon-all", "lightdm", "lightdm-gtk3-greeter"]),
            (PkgFamily::Alt, &[
                "cinnamon", "nemo", "gnome-terminal", "lightdm",
                "lightdm-gtk-greeter",
            ]),
        ],
        // ⚠️ `lightdm-slick-greeter` was removed from here: it is in extra and is
        // already in the Arch list above. A package from the official repository
        // marked as AUR is not cosmetic: on a machine WITHOUT yay/paru this whole
        // list is silently dropped (`aur_packages=… if has_aur_helper`), and the
        // desktop card lies to the user about "some packages from the AUR".
        //
        // ⛔ Do NOT bring `mint-artwork` back here: it declares
        // `conflicts=(lightdm-gtk-greeter lightdm-gtk-greeter-settings)` and itself
        // puts `/etc/lightdm/lightdm-gtk-greeter.conf`, while Rexon installs the GTK
        // login screen with XFCE and MATE: on such a machine pacman would refuse,
        // and without a refusal it would break login for the neighbours. The Mint
        // look comes from the themes, icons and wallpapers below; without it only
        // the /etc/skel files and the Bibata cursor are lost (AUR RPC, 19.10.2026).
        //
        // On CachyOS the Mint themes and icons are in the `cachyos` repository:
        // whatever from this list is in the enabled repositories, the install takes
        // from there as it is (`pacman -Si`), see `system_plan::split_repo_available`.
        aur: &[
            "mint-themes", "mint-x-icons", "mint-y-icons", "mint-backgrounds",
            "lightdm-settings",
        ],
        check_package: "cinnamon",
        config_paths: &[
            "~/.config/cinnamon", "~/.cinnamon", "~/.cinnamon/configs",
            "~/.local/share/cinnamon", "~/.config/cinnamon-session",
            "~/.config/dconf/user",
        ],
    }),
    // xfce
    ("xfce", DeEntry {
        packages: &[
            (PkgFamily::Arch, &["xfce4", "xfce4-goodies", "lightdm", "lightdm-gtk-greeter"]),
            (PkgFamily::Debian, &["xfce4", "xfce4-goodies", "lightdm", "lightdm-gtk-greeter"]),
            (PkgFamily::Fedora, &["@xfce-desktop-environment", "lightdm", "lightdm-gtk-greeter"]),
            (PkgFamily::Suse, &["patterns-xfce-xfce", "lightdm", "lightdm-gtk-greeter"]),
            (PkgFamily::Gentoo, &[
                "xfce-base/xfce4-meta", "x11-terms/xfce4-terminal",
                "app-editors/mousepad", "x11-misc/lightdm",
            ]),
            (PkgFamily::Alpine, &[
                "xfce4", "xfce4-terminal", "xfce4-screensaver", "lightdm",
                "lightdm-gtk-greeter",
            ]),
            (PkgFamily::Void, &["xfce4", "lightdm", "lightdm-gtk3-greeter"]),
            (PkgFamily::Solus, &["xfce4-desktop", "lightdm"]),
            (PkgFamily::Clear, &["desktop-xfce4"]),
            (PkgFamily::Alt, &["xfce4", "xfce4-goodies", "lightdm", "lightdm-gtk-greeter"]),
        ],
        aur: &[],
        check_package: "xfce4-session",
        config_paths: &["~/.config/xfce4", "~/.config/Thunar"],
    }),
    // mate
    ("mate", DeEntry {
        packages: &[
            // `appmenu-gtk-module` is the GTK program side of the global menu
            // (`vala-panel-appmenu-mate` from the AUR list), it is in extra.
            (PkgFamily::Arch, &[
                "mate", "mate-extra", "plank", "lightdm",
                "lightdm-gtk-greeter", "appmenu-gtk-module",
            ]),
            (PkgFamily::Debian, &[
                "mate-desktop-environment", "mate-desktop-environment-extras",
                "lightdm", "lightdm-gtk-greeter",
            ]),
            (PkgFamily::Fedora, &["@mate-desktop-environment", "lightdm", "lightdm-gtk-greeter"]),
            (PkgFamily::Suse, &["patterns-mate-mate", "lightdm", "lightdm-gtk-greeter"]),
            (PkgFamily::Gentoo, &["mate-base/mate", "x11-misc/lightdm"]),
            (PkgFamily::Alpine, &[
                "mate-desktop", "mate-session-manager", "mate-panel", "caja",
                "marco", "mate-terminal", "mate-utils", "lightdm",
                "lightdm-gtk-greeter",
            ]),
            (PkgFamily::Void, &["mate", "lightdm", "lightdm-gtk3-greeter"]),
            (PkgFamily::Alt, &[
                "mate", "mate-terminal", "plank", "lightdm",
                "lightdm-gtk-greeter",
            ]),
        ],
        aur: &[
            "mate-tweak", "brisk-menu", "mate-indicator-applet",
            "ayatana-indicator-application", "ayatana-indicator-session",
            "ayatana-indicator-datetime", "ayatana-indicator-sound",
            "ayatana-indicator-notifications",
            // ⛔ Not the common `vala-panel-appmenu`: it depends on `-budgie` (which
            // depends on all of budgie-desktop) and on `-valapanel` (vala-panel). MATE
            // only needs its own part and the menu registrar (AUR RPC, 19.10.2026).
            "vala-panel-appmenu-mate", "vala-panel-appmenu-registrar",
        ],
        check_package: "mate-session-manager",
        config_paths: &[
            "~/.config/mate", "~/.config/caja", "~/.config/mate-session",
            "~/.config/dconf/user",
        ],
    }),
    // gnome
    ("gnome", DeEntry {
        packages: &[
            (PkgFamily::Arch, &["gnome", "gnome-tweaks", "gdm"]),
            (PkgFamily::Debian, &["gnome", "gnome-tweaks", "gdm3"]),
            (PkgFamily::Fedora, &["@gnome-desktop", "gnome-tweaks", "gdm"]),
            (PkgFamily::Suse, &["patterns-gnome-gnome", "gdm"]),
            (PkgFamily::Gentoo, &["gnome-base/gnome", "gnome-base/gdm"]),
            (PkgFamily::Alpine, &[
                "gnome-desktop", "gnome-shell", "gnome-control-center",
                "nautilus", "gnome-console", "gdm",
            ]),
            (PkgFamily::Void, &["gnome", "gdm"]),
            (PkgFamily::Solus, &[
                "gnome-shell", "gnome-desktop-branding",
                "gnome-control-center", "gnome-console", "gdm",
            ]),
            (PkgFamily::Clear, &["desktop-gnome-apps"]),
            (PkgFamily::Alt, &[
                "gnome-shell", "gnome-control-center", "gnome-tweaks",
                "nautilus", "gnome-console", "gdm",
            ]),
        ],
        aur: &[],
        check_package: "gnome-shell",
        config_paths: &[
            "~/.config/gnome-session", "~/.config/dconf/user",
            "~/.local/share/gnome-shell/extensions",
            "~/.local/share/backgrounds",
        ],
    }),
    // kde
    ("kde", DeEntry {
        packages: &[
            (PkgFamily::Arch, &[
                "plasma-meta", "konsole", "dolphin", "kate", "gwenview", "ark",
                "spectacle", "plasma-login-manager",
            ]),
            (PkgFamily::Debian, &["kde-plasma-desktop", "konsole", "dolphin", "sddm"]),
            (PkgFamily::Fedora, &["@kde-desktop-environment", "sddm"]),
            (PkgFamily::Suse, &["patterns-kde-kde", "sddm"]),
            (PkgFamily::Gentoo, &["kde-plasma/plasma-meta", "x11-misc/sddm"]),
            (PkgFamily::Alpine, &["plasma-desktop-meta", "konsole", "dolphin", "sddm"]),
            (PkgFamily::Void, &["kde-plasma", "konsole", "dolphin", "sddm"]),
            (PkgFamily::Solus, &["plasma-desktop", "konsole", "sddm"]),
            (PkgFamily::Clear, &["desktop-kde-apps"]),
            (PkgFamily::Alt, &["kde5", "plasma5-workspace", "konsole", "dolphin", "sddm"]),
        ],
        aur: &[],
        check_package: "plasma-desktop",
        config_paths: &[
            "~/.config/plasma-org.kde.plasma.desktop-appletsrc",
            "~/.config/plasmashellrc", "~/.config/kdeglobals",
            "~/.config/kwinrc", "~/.config/kglobalshortcutsrc",
            "~/.config/kcminputrc", "~/.config/plasmarc",
            "~/.config/plasmanotifyrc", "~/.local/share/plasma",
            "~/.local/share/color-schemes",
        ],
    }),
    // budgie
    ("budgie", DeEntry {
        packages: &[
            // The program set is as in Ubuntu Budgie 26.04 on Budgie 10.10 (the
            // `ubuntu-budgie.resolute` seeds, the `ubuntu-budgie-desktop` metapackage
            // and its `/etc/budgie-desktop/defaults.list`): files: Nemo (with
            // `nemo-fileroller`, which pulls in the File Roller archiver), terminal:
            // `xfce4-terminal` (Tilix was dropped in 24.10), text: gedit, images:
            // gThumb, video: VLC, music: Lollypop, PDF: Atril, calculator and monitor
            // from MATE, disks: GNOME Disks. `budgie-desktop` itself pulls in none of
            // this: without the lines below, someone who installs Budgie alone is left
            // without a terminal and programs. We do not take Ubuntu Budgie's office,
            // games, browser, torrents and podcasts: they are not part of the desktop.
            // Arch: all packages in extra (03.11.2026); the other families were checked
            // against their package databases (Debian trixie, Fedora rawhide, openSUSE
            // Factory, Gentoo, Void, Solus, ALT Sisyphus) without installing.
            (PkgFamily::Arch, &[
                "budgie-desktop", "budgie-desktop-view", "budgie-extras",
                "network-manager-applet", "nemo", "nemo-fileroller",
                "xfce4-terminal", "gedit", "gthumb", "vlc", "lollypop", "atril",
                "mate-calc", "mate-system-monitor", "gnome-disk-utility",
                "lightdm", "lightdm-gtk-greeter",
            ]),
            (PkgFamily::Debian, &[
                "budgie-desktop", "budgie-extras", "nemo", "nemo-fileroller",
                "xfce4-terminal", "gedit", "gthumb", "vlc", "lollypop", "atril",
                "mate-calc", "mate-system-monitor", "gnome-disk-utility",
                "lightdm", "lightdm-gtk-greeter",
            ]),
            (PkgFamily::Fedora, &[
                "@budgie-desktop-environment", "nemo", "nemo-fileroller",
                "xfce4-terminal", "gedit", "gthumb", "vlc", "lollypop", "atril",
                "mate-calc", "mate-system-monitor", "gnome-disk-utility",
                "lightdm",
            ]),
            // openSUSE: the Nemo add-on for File Roller is `nemo-extension-fileroller`.
            (PkgFamily::Suse, &[
                "patterns-budgie-budgie", "nemo", "nemo-extension-fileroller",
                "xfce4-terminal", "gedit", "gthumb", "vlc", "lollypop", "atril",
                "mate-calc", "mate-system-monitor", "gnome-disk-utility",
                "lightdm",
            ]),
            (PkgFamily::Gentoo, &[
                "budgie-base/budgie-desktop",
                "budgie-base/budgie-desktop-view", "gnome-extra/nemo",
                "gnome-extra/nemo-fileroller", "x11-terms/xfce4-terminal",
                "app-editors/gedit", "media-gfx/gthumb", "media-video/vlc",
                "media-sound/lollypop", "app-text/atril", "mate-extra/mate-calc",
                "mate-extra/mate-system-monitor", "sys-apps/gnome-disk-utility",
                "x11-misc/lightdm",
            ]),
            (PkgFamily::Void, &[
                "budgie-desktop", "nemo", "nemo-fileroller", "xfce4-terminal",
                "gedit", "gthumb", "vlc", "lollypop", "atril", "mate-calc",
                "mate-system-monitor", "gnome-disk-utility", "lightdm",
                "lightdm-gtk3-greeter",
            ]),
            (PkgFamily::Solus, &[
                "budgie-desktop", "nemo", "nemo-fileroller", "xfce4-terminal",
                "gedit", "gthumb", "vlc", "lollypop", "atril", "mate-calc",
                "mate-system-monitor", "gnome-disk-utility", "lightdm",
            ]),
            // ALT: Atril is packaged as `mate-document-viewer`.
            (PkgFamily::Alt, &[
                "budgie-desktop", "nemo", "nemo-fileroller", "xfce4-terminal",
                "gedit", "gthumb", "vlc", "lollypop", "mate-document-viewer",
                "mate-calc", "mate-system-monitor", "gnome-disk-utility",
                "lightdm", "lightdm-gtk-greeter",
            ]),
        ],
        // `budgie-extras` is in extra: installed by pacman together with the rest,
        // no AUR helper is needed for it (and on a machine without a helper it no
        // longer goes missing).
        aur: &[],
        check_package: "budgie-desktop",
        config_paths: &[
            "~/.config/budgie-desktop", "~/.config/budgie-extras",
            "~/.config/dconf/user",
        ],
    }),
    // pantheon is the elementary OS desktop (Gala on Mutter, Wingpanel, Plank,
    // Switchboard). There are packages nowhere except Arch (the `pantheon`
    // group in extra, 02.10.2026) and NixOS (the
    // `services.desktopManager.pantheon` module): Debian/Ubuntu do not have it
    // at all (only elementary OS itself), Fedora has separate parts without a
    // session, and no desktop can be assembled from them.
    ("pantheon", DeEntry {
        packages: &[
            // The group pulls in the session (`pantheon-session` → gala, wingpanel,
            // plank, gnome-session), default settings, the theme, icons and elementary
            // apps. ⚠️ `lightdm-gtk-greeter` is a foolproof guard: lightdm in Arch is
            // built with lightdm-gtk-greeter as the default greeter, and the Pantheon
            // greeter from the group is not enabled without editing `greeter-session`
            // in /etc/lightdm/lightdm.conf, so without the gtk greeter the login
            // screen does not come up.
            (PkgFamily::Arch, &["pantheon", "lightdm", "lightdm-gtk-greeter"]),
        ],
        aur: &[],
        check_package: "pantheon-session",
        config_paths: &[
            "~/.config/dconf/user", "~/.config/plank", "~/.local/share/plank",
        ],
    }),
    // deepin is the Deepin desktop (DDE v25: deepin-session, dde-shell) ONLY on
    // Treeland, Deepin's Wayland compositor (Wehrwolfmann's decision of
    // 26.10.2026): Rexon never installs the X11 session with deepin-kwin. The
    // set below is raw material: the executor installs everything from the
    // group except deepin-session (replaced by our own build without
    // deepin-kwin), see `crate::deepin_treeland`, and this entry is not given
    // to the root helper. Only Arch was checked (the `deepin` group in extra,
    // 02.10.2026). ⚠️ In 2025 openSUSE removed Deepin from its repositories: a
    // service package included D-Bus services and polkit rules bypassing the
    // distribution's security review. Hence ONLY the distribution's stock
    // group and no services, rules or "enablers" of our own; where there are
    // no packages, the desktop is not offered at all.
    ("deepin", DeEntry {
        packages: &[
            // The group: session, settings, theme and apps (its `deepin-session` pulls
            // in deepin-kwin, which is why the group is not installed as a whole, see
            // above). Login screen: lightdm with the Deepin greeter: it is installed by
            // deepin-session-shell, and
            // `/usr/share/lightdm/lightdm.conf.d/60-deepin.conf` from deepin-daemon
            // itself sets `greeter-session=lightdm-deepin-greeter`. lightdm is named
            // explicitly so that the "no enabled login screen" guard knows which one to
            // enable (ddm from the group is early and has no X11).
            // `deepin-terminal` is in `deepin-extra`, not in the `deepin` group.
            (PkgFamily::Arch, &["deepin", "deepin-terminal", "lightdm"]),
        ],
        aur: &[],
        check_package: "deepin-session",
        config_paths: &[
            "~/.config/deepin", "~/.config/dsg", "~/.config/dconf/user",
        ],
    }),
    // cosmic
    ("cosmic", DeEntry {
        packages: &[
            (PkgFamily::Arch, &[
                "cosmic-session", "cosmic-comp", "cosmic-panel",
                "cosmic-applets", "cosmic-app-library", "cosmic-bg",
                "cosmic-greeter", "cosmic-idle", "cosmic-launcher",
                "cosmic-notifications", "cosmic-osd", "cosmic-randr",
                "cosmic-screenshot", "cosmic-settings",
                "cosmic-settings-daemon", "cosmic-workspaces",
                "cosmic-initial-setup", "xdg-desktop-portal-cosmic",
                "cosmic-files", "cosmic-terminal", "cosmic-text-editor",
                "cosmic-store", "cosmic-player", "cosmic-wallpapers",
                "cosmic-icon-theme", "gnome-keyring", "power-profiles-daemon",
                "packagekit", "flatpak",
            ]),
            (PkgFamily::Fedora, &["cosmic-desktop"]),
            (PkgFamily::Suse, &["patterns-cosmic-cosmic"]),
            (PkgFamily::Gentoo, &["cosmic-base/cosmic-session"]),
            (PkgFamily::Alpine, &[
                "cosmic-session", "cosmic-comp", "cosmic-panel",
                "cosmic-applets", "cosmic-app-library", "cosmic-bg",
                "cosmic-greeter", "cosmic-idle", "cosmic-launcher",
                "cosmic-notifications", "cosmic-osd", "cosmic-randr",
                "cosmic-screenshot", "cosmic-settings",
                "cosmic-settings-daemon", "cosmic-workspaces",
                "xdg-desktop-portal-cosmic", "cosmic-files", "cosmic-terminal",
                "cosmic-text-editor", "cosmic-store", "cosmic-wallpapers",
                "cosmic-icons",
            ]),
            (PkgFamily::Void, &["cosmic-desktop"]),
        ],
        aur: &[],
        check_package: "cosmic-session",
        config_paths: &["~/.config/cosmic", "~/.local/state/cosmic"],
    }),
    // hyprland
    // SDDM removed: the login screen is system-wide, it is guaranteed by
    // install_de → ensure_display_manager (the Python path), not by a per-WM
    // package list.
    ("hyprland", DeEntry {
        packages: &[
            (PkgFamily::Arch, &[
                "hyprland", "hyprpaper", "hyprlock", "hypridle",
                "xdg-desktop-portal-hyprland", "xdg-desktop-portal-gtk",
                "waybar", "wofi", "kitty",
                "thunar", "grim", "slurp", "wl-clipboard", "polkit-gnome",
                "otf-font-awesome",
            ]),
            (PkgFamily::Debian, &[
                "hyprland", "xdg-desktop-portal-hyprland", "waybar", "wofi",
                "kitty", "grim", "slurp", "wl-clipboard",
            ]),
            (PkgFamily::Fedora, &[
                "hyprland", "xdg-desktop-portal-hyprland", "waybar", "wofi",
                "kitty", "grim", "slurp", "wl-clipboard",
            ]),
            // hyprlock and hypridle (screen lock; the post-install step
            // writes the key binding and autostart) exist only in Arch and
            // openSUSE Tumbleweed. Debian has them only in 14/sid and
            // trixie-backports, Ubuntu since 26.04, Fedora up to 41, Alpine
            // in testing, ALT in Sisyphus (no hyprlock in p11), Gentoo and
            // Void not at all: a missing package would fail the whole
            // install. Without them the post-install step says plainly that
            // there is nothing to set up the lock with.
            (PkgFamily::Suse, &[
                "hyprland", "hyprlock", "hypridle", "xdg-desktop-portal-hyprland",
                "waybar", "wofi", "kitty", "grim", "slurp", "wl-clipboard",
            ]),
            (PkgFamily::Gentoo, &[
                "gui-wm/hyprland", "gui-apps/waybar", "gui-apps/wofi",
                "x11-terms/kitty", "gui-apps/grim", "gui-apps/slurp",
            ]),
            (PkgFamily::Void, &[
                "hyprland", "waybar", "wofi", "kitty", "grim", "slurp",
                "wl-clipboard",
            ]),
            (PkgFamily::Alpine, &[
                "hyprland", "waybar", "wofi", "kitty", "grim", "slurp",
                "wl-clipboard",
            ]),
            (PkgFamily::Alt, &[
                "hyprland", "waybar", "wofi", "kitty", "grim", "slurp",
                "wl-clipboard",
            ]),
        ],
        // All three are in extra and are already listed in the Arch list above:
        // duplicating them via the AUR helper meant building from source what is
        // already built, and on a machine without a helper losing them entirely.
        aur: &[],
        check_package: "hyprland",
        config_paths: &[
            "~/.config/hypr", "~/.config/waybar", "~/.config/wofi",
            "~/.config/kitty",
        ],
    }),
    // lxqt
    ("lxqt", DeEntry {
        packages: &[
            // `openbox` is named EXPLICITLY instead of being taken "somehow from the
            // group": lxqt-session's post_install prints "An X11 window manager is also
            // needed" with an unconditional cat, and on a user's clean machine one
            // cannot rely on a WM showing up by itself. `breeze-icons` is the icon
            // theme (also a hard dependency of lxqt-session, but it is here on purpose:
            // the list must cover both requests by itself).
            //
            // `xscreensaver` is the screen lock in X11. LXQt has no locker of its own:
            // «Lock» (and Ctrl+Alt+L) calls `xdg-screensaver lock`, which only knows a
            // running xscreensaver (it has no LXQt branch: exit code 1 and a
            // "screensaver error" window). lxqt-session starts it itself at login
            // (`lxqt-xscreensaver-autostart.desktop`, TryExec=xscreensaver). It is
            // neither in the `lxqt` group nor in the metapackages of other families.
            // For Wayland: swaylock in `SESSION_VARIANTS`; the commands for both
            // sessions are written by the post-install (`plan::lxqt_lock_edit`).
            (PkgFamily::Arch, &["lxqt", "openbox", "breeze-icons", "sddm", "xscreensaver"]),
            (PkgFamily::Debian, &["lxqt", "sddm", "xscreensaver"]),
            (PkgFamily::Fedora, &["@lxqt-desktop-environment", "sddm", "xscreensaver"]),
            (PkgFamily::Suse, &["patterns-lxqt-lxqt", "sddm", "xscreensaver"]),
            (PkgFamily::Gentoo, &["lxqt-base/lxqt-meta", "x11-misc/sddm", "x11-misc/xscreensaver"]),
            (PkgFamily::Alpine, &[
                "lxqt-desktop", "lximage-qt", "pavucontrol-qt", "font-dejavu",
                "sddm", "xscreensaver",
            ]),
            (PkgFamily::Void, &["lxqt", "sddm", "xscreensaver"]),
            (PkgFamily::Alt, &[
                "lxqt-session", "lxqt-panel", "lxqt-config", "lxqt-policykit",
                "lxqt-runner", "lxqt-notificationd", "pcmanfm-qt", "qterminal",
                "openbox", "sddm", "xscreensaver",
            ]),
        ],
        aur: &[],
        check_package: "lxqt-session",
        config_paths: &[
            "~/.config/lxqt", "~/.config/pcmanfm-qt",
            "~/.config/qterminal.org", "~/.config/openbox",
        ],
    }),
    // niri
    // Icon fonts for waybar (otherwise "tofu" instead of glyphs): Font Awesome
    // is waybar's default config; Nerd symbols (symbols-only) from the official
    // repos where available (Arch/openSUSE/Gentoo/Void/Alpine; not in
    // Debian/Fedora). SDDM removed: the login screen is system-wide, it is
    // guaranteed by install_de → ensure_display_manager (the Python path), not
    // by a per-WM package list.
    ("niri", DeEntry {
        packages: &[
            (PkgFamily::Arch, &[
                "niri", "swaybg", "swayidle", "swaylock", "waybar", "fuzzel",
                "alacritty", "mako", "thunar", "grim", "slurp", "wl-clipboard",
                "polkit-gnome", "xdg-desktop-portal-gnome",
                "xwayland-satellite",
                "otf-font-awesome", "ttf-nerd-fonts-symbols",
            ]),
            (PkgFamily::Fedora, &[
                "niri", "swaybg", "swayidle", "swaylock", "waybar", "fuzzel",
                "alacritty", "mako", "thunar", "grim", "slurp", "wl-clipboard",
                "xdg-desktop-portal-gnome", "fontawesome-fonts",
            ]),
            (PkgFamily::Suse, &[
                "niri", "swaybg", "swayidle", "swaylock", "waybar", "fuzzel",
                "alacritty", "mako", "grim", "slurp", "wl-clipboard",
                "xdg-desktop-portal-gnome", "fontawesome-fonts",
                "symbols-only-nerd-fonts",
            ]),
            (PkgFamily::Alpine, &[
                "niri", "swaybg", "swayidle", "swaylock", "waybar", "fuzzel",
                "alacritty", "mako", "grim", "slurp", "wl-clipboard",
                "xdg-desktop-portal-gnome", "font-awesome",
                "font-nerd-fonts-symbols",
            ]),
        ],
        aur: &[],
        check_package: "niri",
        config_paths: &["~/.config/niri", "~/.config/waybar", "~/.config/fuzzel"],
    }),
    // sway
    // Icon fonts for waybar (otherwise "tofu" instead of glyphs): Font Awesome
    // is waybar's default config; Nerd symbols (symbols-only) from the official
    // repos where available (Arch/openSUSE/Gentoo/Void/Alpine; not in
    // Debian/Fedora). SDDM removed: the login screen is system-wide, it is
    // guaranteed by install_de → ensure_display_manager (the Python path), not
    // by a per-WM package list.
    ("sway", DeEntry {
        packages: &[
            (PkgFamily::Arch, &[
                "sway", "swaybg", "swayidle", "swaylock", "waybar", "wofi",
                "wmenu", "foot", "mako", "thunar", "grim", "slurp",
                "wl-clipboard", "polkit-gnome", "xdg-desktop-portal-wlr",
                "xdg-desktop-portal-gtk", "xorg-xwayland",
                "otf-font-awesome", "ttf-nerd-fonts-symbols",
            ]),
            // Notifications in Debian/Ubuntu are `mako-notifier`: the package named
            // `mako` there is the source of a Python template engine, and `apt-get
            // install mako` broke the WHOLE sway install.
            (PkgFamily::Debian, &[
                "sway", "swaybg", "swayidle", "swaylock", "waybar", "wofi",
                "foot", "mako-notifier", "grim", "slurp", "wl-clipboard",
                "xdg-desktop-portal-wlr", "fonts-font-awesome",
            ]),
            (PkgFamily::Fedora, &[
                "sway", "swaybg", "swayidle", "swaylock", "waybar", "wofi",
                "foot", "mako", "grim", "slurp", "wl-clipboard",
                "xdg-desktop-portal-wlr", "fontawesome-fonts",
            ]),
            (PkgFamily::Suse, &[
                "sway", "swaybg", "swayidle", "swaylock", "waybar", "wofi",
                "foot", "mako", "grim", "slurp", "wl-clipboard",
                "xdg-desktop-portal-wlr", "fontawesome-fonts",
                "symbols-only-nerd-fonts",
            ]),
            (PkgFamily::Gentoo, &[
                "gui-wm/sway", "gui-apps/swaybg", "gui-apps/swayidle",
                "gui-apps/swaylock", "gui-apps/waybar",
                "gui-apps/wofi", "gui-apps/foot", "gui-apps/grim",
                "gui-apps/slurp", "media-fonts/fontawesome",
                "media-fonts/symbols-nerd-font",
            ]),
            (PkgFamily::Void, &[
                "sway", "swaybg", "swayidle", "swaylock", "waybar", "wofi",
                "foot", "mako", "grim", "slurp", "wl-clipboard", "font-awesome",
                "nerd-fonts-symbols-ttf",
            ]),
            (PkgFamily::Alpine, &[
                "sway", "swaybg", "swayidle", "swaylock", "waybar", "wofi",
                "foot", "mako", "grim", "slurp", "wl-clipboard", "font-awesome",
                "font-nerd-fonts-symbols",
            ]),
            (PkgFamily::Alt, &[
                "sway", "swaybg", "swayidle", "swaylock", "waybar", "wofi",
                "foot", "mako", "grim", "slurp", "wl-clipboard",
                "fonts-font-awesome",
            ]),
        ],
        aur: &[],
        check_package: "sway",
        config_paths: &[
            "~/.config/sway", "~/.config/waybar", "~/.config/wofi",
            "~/.config/foot",
        ],
    }),
    // river
    // ⚠️ river-classic is installed (the 0.3 branch: its own `rivertile`
    // tiling and `riverctl`), not river 0.4+. The new river is "non-monolithic":
    // windows in it are managed by a SEPARATE program over the
    // river-window-management-v1 protocol, and without one river by itself is
    // useless (river(1): "river is not useful without a compatible window
    // manager"). There is no such manager in the official repositories, and the
    // user would see bare river as a black screen without a single key binding
    // (rule #13). Package names differ per family: Arch, Fedora (since 43) and
    // Alpine (since 3.23) call the 0.3 branch `river-classic`, Void still calls
    // it `river`. Debian, openSUSE, Gentoo and ALT do not carry river in their
    // official repositories: there the desktop is honestly "not available on
    // this distribution". Without `~/.config/river/init` river-classic also
    // starts without key bindings: the starter init is put by the post-install
    // (`plan::river_starter_init`). Panel: waybar (started by the starter
    // init), launcher: fuzzel, terminal: foot (also called by the example init
    // from river-classic itself). Screen lock: swaylock (Super+Shift+Esc), on
    // idle and before sleep it is called by swayidle; both are started by the
    // same starter init. river-classic has no locker of its own.
    ("river", DeEntry {
        packages: &[
            (PkgFamily::Arch, &[
                "river-classic", "waybar", "fuzzel", "foot", "mako", "swaybg",
                "swaylock", "swayidle",
                "thunar", "grim", "slurp", "wl-clipboard", "polkit-gnome",
                "xdg-desktop-portal-wlr", "xdg-desktop-portal-gtk",
                "otf-font-awesome", "ttf-nerd-fonts-symbols",
            ]),
            (PkgFamily::Fedora, &[
                "river-classic", "waybar", "fuzzel", "foot", "mako", "swaybg",
                "swaylock", "swayidle",
                "grim", "slurp", "wl-clipboard", "xdg-desktop-portal-wlr",
                "fontawesome-fonts",
            ]),
            (PkgFamily::Void, &[
                "river", "waybar", "fuzzel", "foot", "mako", "swaybg",
                "swaylock", "swayidle", "grim",
                "slurp", "wl-clipboard", "xdg-desktop-portal-wlr",
                "font-awesome", "nerd-fonts-symbols-ttf",
            ]),
            (PkgFamily::Alpine, &[
                "river-classic", "waybar", "fuzzel", "foot", "mako", "swaybg",
                "swaylock", "swayidle",
                "grim", "slurp", "wl-clipboard", "xdg-desktop-portal-wlr",
                "font-awesome", "font-nerd-fonts-symbols",
            ]),
        ],
        aur: &[],
        check_package: "river-classic",
        config_paths: &["~/.config/river", "~/.config/waybar", "~/.config/fuzzel"],
    }),
    // wayfire
    // wf-shell (the wf-panel panel with an app menu and the wf-background
    // background) is a mandatory part of the set: Wayfire's autostart module
    // starts them at login itself (`autostart_wf_shell`, true by default), and
    // without them the session is a grey screen with no menu and not a single
    // key to launch programs (the module defaults have no command bindings),
    // rule #13. wcm is the Wayfire settings window, so wayfire.ini does not
    // have to be edited by hand. swaylock and swayidle are the screen lock:
    // Wayfire's defaults have no lock key at all, and the documentation example
    // calls swaylock; the binding and idle lock are added by the post-install
    // (`plan::wayfire_lock_edit`). Alpine has Wayfire only in testing, ALT only
    // in Sisyphus: there the desktop is "not available".
    ("wayfire", DeEntry {
        packages: &[
            (PkgFamily::Arch, &[
                "wayfire", "wf-shell", "wcm", "foot", "mako", "thunar", "grim",
                "slurp", "wl-clipboard", "polkit-gnome",
                "xdg-desktop-portal-wlr", "xdg-desktop-portal-gtk",
                "xorg-xwayland", "swaylock", "swayidle",
            ]),
            (PkgFamily::Debian, &[
                "wayfire", "wf-shell", "wcm", "foot", "mako-notifier", "grim",
                "slurp", "wl-clipboard", "xdg-desktop-portal-wlr", "swaylock",
                "swayidle",
            ]),
            (PkgFamily::Fedora, &[
                "wayfire", "wf-shell", "wayfire-config-manager", "foot", "mako",
                "grim", "slurp", "wl-clipboard", "xdg-desktop-portal-wlr",
                "swaylock", "swayidle",
            ]),
            (PkgFamily::Suse, &[
                "wayfire", "wf-shell", "wcm", "foot", "mako", "grim", "slurp",
                "wl-clipboard", "xdg-desktop-portal-wlr", "swaylock", "swayidle",
            ]),
            (PkgFamily::Gentoo, &[
                "gui-wm/wayfire", "gui-apps/wf-shell", "gui-apps/wcm",
                "gui-apps/foot", "gui-apps/grim", "gui-apps/slurp",
                "gui-apps/swaylock", "gui-apps/swayidle",
            ]),
            (PkgFamily::Void, &[
                "wayfire", "wf-shell", "wcm", "foot", "mako", "grim", "slurp",
                "wl-clipboard", "xdg-desktop-portal-wlr", "swaylock", "swayidle",
            ]),
        ],
        aur: &[],
        check_package: "wayfire",
        config_paths: &[
            "~/.config/wayfire.ini", "~/.config/wayfire", "~/.config/wf-shell.ini",
        ],
    }),
];

/// `_DE_META`: DE metadata for the UI, in the original order.
pub const DE_META: &[DeMeta] = &[
    DeMeta {
        id: "cinnamon-mint",
        display_name: "Cinnamon",
        description: n_("🍃 The Cinnamon desktop with the full set of Linux Mint components"),
        session_name: "cinnamon",
        ram_mb: 650,
        cpu_usage: CpuLoad::Medium,
        customizable: 5,
        gaming_friendly: 4,
        toolkit: "GTK3",
        session_type: "X11",
        compositor: "Muffin",
        panel_type: "Cinnamon Panel",
        file_manager: "Nemo",
        terminal: "gnome-terminal",
    },
    DeMeta {
        id: "xfce",
        display_name: "XFCE",
        description: n_("A light and fast desktop that uses few resources"),
        session_name: "xfce",
        ram_mb: 300,
        cpu_usage: CpuLoad::Low,
        customizable: 4,
        gaming_friendly: 5,
        toolkit: "GTK3",
        session_type: "X11",
        compositor: "Xfwm4",
        panel_type: "Xfce Panel",
        file_manager: "Thunar",
        terminal: "xfce4-terminal",
    },
    DeMeta {
        id: "mate",
        display_name: "MATE",
        description: n_("A traditional desktop that carries on GNOME 2"),
        session_name: "mate",
        ram_mb: 400,
        cpu_usage: CpuLoad::Low,
        customizable: 3,
        gaming_friendly: 4,
        toolkit: "GTK3",
        session_type: "X11",
        compositor: "Marco",
        panel_type: "MATE Panel",
        file_manager: "Caja",
        terminal: "mate-terminal",
    },
    DeMeta {
        id: "gnome",
        display_name: "GNOME",
        description: n_("A modern desktop with a minimalist design"),
        session_name: "gnome",
        ram_mb: 800,
        cpu_usage: CpuLoad::High,
        customizable: 3,
        gaming_friendly: 3,
        toolkit: "GTK4",
        session_type: "Wayland",
        compositor: "Mutter",
        panel_type: "GNOME Shell",
        file_manager: "Nautilus",
        terminal: "GNOME Console",
    },
    DeMeta {
        id: "kde",
        display_name: "KDE Plasma",
        description: n_("A powerful, customizable desktop with beautiful effects"),
        session_name: "plasma",
        ram_mb: 700,
        cpu_usage: CpuLoad::Medium,
        customizable: 5,
        gaming_friendly: 5,
        toolkit: "Qt6",
        session_type: "Wayland",
        compositor: "KWin",
        panel_type: "Plasma Panel",
        file_manager: "Dolphin",
        terminal: "Konsole",
    },
    DeMeta {
        id: "budgie",
        display_name: "Budgie",
        description: n_("An elegant desktop from Buddies of Budgie"),
        session_name: "budgie-desktop",
        ram_mb: 550,
        cpu_usage: CpuLoad::Medium,
        customizable: 3,
        gaming_friendly: 3,
        // Budgie 10.10 (Arch, 02.11.2026): Wayland-only session, windows are
        // managed by labwc (`startbudgielabwc`, `labwc` is a dependency of
        // `budgie-desktop`); the desktop itself is on GTK3 (`gtk3`, no GTK4 in the
        // dependencies).
        toolkit: "GTK3",
        session_type: "Wayland",
        compositor: "labwc",
        panel_type: "Budgie Panel",
        // Nemo, as in Ubuntu Budgie (the `budgie` package set above).
        file_manager: "Nemo",
        terminal: "xfce4-terminal",
    },
    DeMeta {
        id: "pantheon",
        display_name: "Pantheon",
        description: n_("The calm, macOS-like desktop of elementary OS"),
        session_name: "pantheon",
        ram_mb: 600,
        cpu_usage: CpuLoad::Medium,
        customizable: 2,
        gaming_friendly: 3,
        toolkit: "GTK3 + GTK4 (Granite)",
        session_type: "Wayland",
        compositor: "Gala (Mutter)",
        panel_type: "Wingpanel + Plank",
        file_manager: "Pantheon Files",
        terminal: "Pantheon Terminal",
    },
    DeMeta {
        id: "deepin",
        display_name: "Deepin",
        description: n_("The polished desktop of deepin with rounded, translucent windows, on its own Wayland compositor Treeland (experimental)"),
        session_name: "treeland",
        ram_mb: 900,
        cpu_usage: CpuLoad::High,
        customizable: 3,
        gaming_friendly: 3,
        toolkit: "Qt6 (DTK)",
        session_type: "Wayland",
        compositor: "Treeland",
        panel_type: "dde-shell",
        file_manager: "Deepin File Manager",
        terminal: "Deepin Terminal",
    },
    DeMeta {
        id: "cosmic",
        display_name: "COSMIC",
        description: n_("A new Wayland-native desktop from System76, written in Rust"),
        session_name: "cosmic",
        ram_mb: 500,
        cpu_usage: CpuLoad::Medium,
        customizable: 4,
        gaming_friendly: 4,
        toolkit: "iced (Rust)",
        session_type: "Wayland",
        compositor: "cosmic-comp",
        panel_type: "COSMIC Panel",
        file_manager: "COSMIC Files",
        terminal: "COSMIC Term",
    },
    DeMeta {
        id: "lxqt",
        display_name: "LXQt",
        description: n_("🪶 A lightweight Qt desktop, the successor to LXDE — the Qt counterpart of XFCE"),
        session_name: "lxqt",
        ram_mb: 350,
        cpu_usage: CpuLoad::Low,
        customizable: 4,
        gaming_friendly: 4,
        toolkit: "Qt6",
        session_type: "X11",
        compositor: n_("none (Openbox WM; labwc optional)"),
        panel_type: "LXQt Panel",
        file_manager: "PCManFM-Qt",
        terminal: "QTerminal",
    },
    // Wayfire is not tiling: windows float, as in Compiz. But it is not a full
    // desktop either: no programs of its own and no common system settings,
    // only the wf-shell panel and the compositor's own settings (WCM), so in
    // the list it has its own section «Compositors with effects»
    // (`qt_desktops::EFFECTS_IDS`).
    DeMeta {
        id: "wayfire",
        display_name: "Wayfire",
        description: n_("🔥 A Wayland window manager with Compiz-style effects (desktop cube, wobbly windows) and a simple wf-shell panel. It isn't a full desktop: it has no programs or system settings of its own — only settings for Wayfire itself."),
        session_name: "wayfire",
        ram_mb: 250,
        cpu_usage: CpuLoad::Low,
        customizable: 4,
        gaming_friendly: 3,
        toolkit: "GTK4 (wf-shell)",
        session_type: "Wayland",
        compositor: "Wayfire (wlroots)",
        panel_type: "wf-panel",
        file_manager: "Thunar",
        terminal: "foot",
    },
    // Tiling window managers come after the full DEs.
    DeMeta {
        id: "hyprland",
        display_name: "Hyprland",
        description: n_("🪟 A tiling Wayland compositor with animations and high performance"),
        session_name: "hyprland",
        ram_mb: 250,
        cpu_usage: CpuLoad::Low,
        customizable: 5,
        gaming_friendly: 5,
        toolkit: "—",
        session_type: "Wayland",
        compositor: "Hyprland",
        panel_type: "Waybar / Eww",
        file_manager: "Thunar / Dolphin",
        terminal: "kitty / foot",
    },
    DeMeta {
        id: "sway",
        display_name: "Sway",
        description: n_("🧩 A tiling Wayland compositor — a drop-in replacement for i3 that reads its config"),
        session_name: "sway",
        ram_mb: 150,
        cpu_usage: CpuLoad::Low,
        customizable: 5,
        gaming_friendly: 4,
        toolkit: "—",
        session_type: "Wayland",
        compositor: "Sway (wlroots)",
        panel_type: "Waybar",
        file_manager: "Thunar",
        terminal: "foot",
    },
    DeMeta {
        id: "niri",
        display_name: "niri",
        description: n_("🎞️ A scrollable tiling Wayland compositor — windows sit on an endless horizontal strip"),
        session_name: "niri",
        ram_mb: 200,
        cpu_usage: CpuLoad::Low,
        customizable: 4,
        gaming_friendly: 4,
        toolkit: "—",
        session_type: "Wayland",
        compositor: "niri (Smithay)",
        panel_type: "Waybar",
        file_manager: "Thunar",
        terminal: "Alacritty / foot",
    },
    DeMeta {
        id: "river",
        display_name: "River",
        description: n_("🏞️ A dynamic tiling Wayland compositor: windows are grouped by tags instead of workspaces"),
        session_name: "river",
        ram_mb: 120,
        cpu_usage: CpuLoad::Low,
        customizable: 4,
        gaming_friendly: 4,
        toolkit: "—",
        session_type: "Wayland",
        compositor: "River (wlroots)",
        panel_type: "Waybar",
        file_manager: "Thunar",
        terminal: "foot",
    },
];

fn de_entry(de_id: &str) -> Option<&'static DeEntry> {
    DE_DATABASE.iter().find(|(k, _)| *k == de_id).map(|(_, e)| e)
}

/// List of main packages for a DE and distribution: a port of
/// `PackageDB.get_packages`. Returns [] if the DE is not available on this
/// distribution (there is NO fallback to Debian: the names are incompatible).
pub fn get_packages(de_id: &str, family: PkgFamily) -> Vec<String> {
    let Some(entry) = de_entry(de_id) else {
        return Vec::new();
    };
    entry
        .packages
        .iter()
        .find(|(f, _)| *f == family)
        .map(|(_, pkgs)| pkgs.iter().map(|p| p.to_string()).collect())
        .unwrap_or_default()
}

/// Whether a DE is available on the given distribution: a port of `PackageDB.is_available`.
pub fn is_available(de_id: &str, family: PkgFamily) -> bool {
    de_entry(de_id)
        .map(|e| e.packages.iter().any(|(f, _)| *f == family))
        .unwrap_or(false)
}

/// AUR packages (returns [] for non-Arch): a port of `PackageDB.get_aur_packages`.
pub fn get_aur_packages(de_id: &str, family: PkgFamily) -> Vec<String> {
    if family != PkgFamily::Arch {
        return Vec::new();
    }
    de_entry(de_id)
        .map(|e| e.aur.iter().map(|p| p.to_string()).collect())
        .unwrap_or_default()
}

/// The PACKAGE name used to ask the package manager whether a DE is
/// installed: a port of `PackageDB.get_check_binary` (None for an unknown DE).
///
/// ⚠️ The function name is misleading and stays that way only for parity
/// with Python: it returns `check_package`, i.e. a PACKAGE name
/// (`plasma-desktop`, `mate-session-manager`), not an executable. No
/// executables with such names exist: `which("plasma-desktop")` finds
/// nothing. The "DE → executable" map (`plasmashell`, `mate-session`,
/// `Hyprland`) lives next to it: `de_installed_binaries`.
pub fn get_check_binary(de_id: &str) -> Option<&'static str> {
    de_entry(de_id).map(|e| e.check_package)
}

/// Executables that show the desktop is REALLY installed.
///
/// `check_package` from the database is a PACKAGE name (`plasma-desktop`,
/// `mate-session-manager`); looking for such a file in PATH is pointless, it
/// is not there. So each desktop has its own executable, and Cinnamon has
/// two: `cinnamon-session` gets pulled in by dependencies even without the
/// desktop itself, so we also confirm with the `nemo` file manager.
///
/// A new desktop cannot be forgotten here: an unknown `de_id` gives an empty
/// list rather than "a package name instead of a file", so an installed
/// desktop is no longer shown as forever missing (which is how the old
/// `_CHECK_BINARIES.get(name) or check_package` behaved).
pub fn de_installed_binaries(de_id: &str) -> &'static [&'static str] {
    match de_id {
        "cinnamon-mint" => &["cinnamon", "nemo"],
        "xfce" => &["xfce4-session"],
        "mate" => &["mate-session"],
        "gnome" => &["gnome-shell"],
        "kde" => &["plasmashell"],
        "budgie" => &["budgie-desktop"],
        "pantheon" => &["gala", "io.elementary.wingpanel"],
        "deepin" => &["dde-session"],
        "cosmic" => &["cosmic-session"],
        "hyprland" => &["Hyprland"],
        "lxqt" => &["lxqt-session"],
        "niri" => &["niri"],
        "sway" => &["sway"],
        // `rivertile` exists only in river-classic: bare river 0.4+ (also
        // `/usr/bin/river`, but without a window manager) must not look like an
        // installed desktop, since the user would not be able to log into it.
        "river" => &["river", "rivertile"],
        "wayfire" => &["wayfire"],
        _ => &[],
    }
}

/// DE config paths: a port of `PackageDB.get_config_paths`.
pub fn get_config_paths(de_id: &str) -> Vec<String> {
    de_entry(de_id)
        .map(|e| e.config_paths.iter().map(|p| p.to_string()).collect())
        .unwrap_or_default()
}

/// List of all known DEs: a port of `PackageDB.list_de_ids`.
pub fn list_de_ids() -> Vec<&'static str> {
    DE_DATABASE.iter().map(|(k, _)| *k).collect()
}

// ============================================================================
// Which environment is running
// ============================================================================
//
// ⚠️ ONE implementation for the whole product. There were THREE, and all
// three differed:
//   * `compliance` knew `plasma` but not sway, niri and LXQt: the tiling
//     ones silently got no compliance checks (a test explicitly pinned
//     `detect_current_de("sway") == None`);
//   * `appdefaults` knew sway and niri, but compared tokens case-sensitively
//     and did not know `plasma`;
//   * `SystemInfo.detect_current_de` in Python searched for substrings and
//     also knew neither sway nor niri, and its answer drives the ban on
//     removing the RUNNING desktop (rule #13): in a sway session the answer
//     was "unknown", and the guard did not fire.
// Three diverging tables are exactly what rule #12 forbids, so both former
// functions now call this one. On 30.09 `wallpaper_engine::detect_de` was
// folded into it too (a fourth table: substrings, Cinnamon as `cinnamon`,
// `plasma` in the session name was not recognised). Only
// `we_util::detect_desktop` lives separately: a port of the waywallen
// daemon's `Util::desktop`, it has its own list of answers.

/// `XDG_CURRENT_DESKTOP` token (lowercase) → Rexon DE identifier. Order
/// matters: `Budgie:GNOME` must give budgie, so the tokens of the string are
/// iterated first, and only inside that this table. The root package helper
/// also uses it to learn the graphical session's desktop from logind's
/// `Desktop=` (`system_plan::pkg_helper_script`).
pub const XDG_TOKEN_TO_DE: &[(&str, &str)] = &[
    ("kde", "kde"),
    ("plasma", "kde"),
    ("gnome", "gnome"),
    ("x-cinnamon", "cinnamon-mint"),
    ("cinnamon", "cinnamon-mint"),
    ("xfce", "xfce"),
    ("mate", "mate"),
    ("budgie", "budgie"),
    // Pantheon session: `DesktopNames=Pantheon` in both session files
    // (elementary/session-settings: xsessions/pantheon.desktop.in and
    // wayland-sessions/pantheon-wayland.desktop.in).
    ("pantheon", "pantheon"),
    // Deepin: DDE v23/v25 sets `XDG_CURRENT_DESKTOP=DDE` itself (dde-session
    // `environmentsmanager.cpp`), deepin 20 sets `Deepin` (startdde
    // `session.go`); the Wayland session declares `DesktopNames=Treeland`.
    // Session names are `deepin` (X11) and `treeland` (Wayland).
    ("dde", "deepin"),
    ("deepin", "deepin"),
    ("treeland", "deepin"),
    ("lxqt", "lxqt"),
    ("cosmic", "cosmic"),
    ("hyprland", "hyprland"),
    ("sway", "sway"),
    ("niri", "niri"),
    // Since 24.10 Rexon installs river and Wayfire itself. river-classic does
    // not set `XDG_CURRENT_DESKTOP` (the Rexon starter init sets it, and a
    // session from the login screen is recognised by `DESKTOP_SESSION=river`);
    // Wayfire sets `wayfire` itself.
    ("river", "river"),
    ("wayfire", "wayfire"),
    // Rexon does not install labwc but recognises it: the background on it is
    // put by an external daemon (`wallpaper_engine::WLROOTS_DE`). Previously such
    // compositors were known only to the wallpaper engine's separate table, and
    // here they passed only on Wayland "with the first token as is", and were
    // not recognised by session name at all.
    ("labwc", "labwc"),
    // UKUI (the Kylin desktop): `XDG_CURRENT_DESKTOP=UKUI` is set by
    // `ukui-session` itself (`sessionapplication.cpp`), Ubuntu Kylin gives
    // `ubuntukylin:UKUI`. Session names are `ukui` (X11, `ukui.desktop`) and
    // `kylin-wlcom` (Wayland, the session file of the kylin-wlcom compositor).
    // ⚠️ Recognition only: the desktop is not in the catalog (`DE_DATABASE`,
    // `de_customization::de_from_id`), and Rexon behaves as on any unknown
    // desktop, honestly saying it does not support it yet
    // ([`unsupported_desktop_notice`]). Without this line a UKUI session on X11
    // was not recognised at all, and on Wayland it passed only "with the first
    // token as is".
    ("ukui", "ukui"),
    ("kylin-wlcom", "ukui"),
];

/// Desktops Rexon recognises in a session but does not support yet: they
/// are not in the catalog (`DE_DATABASE`), so no install and no settings of
/// their own. Identifier → name for the person (a proper name, not translated).
pub const RECOGNIZED_UNSUPPORTED_DES: &[(&str, &str)] = &[("ukui", "UKUI")];

/// Name of a desktop Rexon recognises but does not support yet; `None`: the
/// desktop is supported or entirely unknown.
pub fn unsupported_desktop_name(de_id: &str) -> Option<&'static str> {
    RECOGNIZED_UNSUPPORTED_DES.iter().find(|(id, _)| *id == de_id).map(|(_, name)| *name)
}

/// An honest line for the person in a session of such a desktop; `None`:
/// nothing to say. Rexon works there as on any unknown desktop: wallpapers,
/// themes, cursors and fonts are applied the common way.
pub fn unsupported_desktop_notice(de_id: &str) -> Option<String> {
    let name = unsupported_desktop_name(de_id)?;
    Some(crate::i18n::trf(
        // TRANSLATORS: %1 is a desktop environment name (UKUI). Rexon recognizes the session but can't install this desktop or change its own settings yet.
        "Rexon doesn't support the %1 desktop yet: it can't install it or change its own settings. Wallpapers, themes, cursors and fonts work as on any other desktop.",
        &[&name],
    ))
}

/// Which environment is running NOW: by `XDG_CURRENT_DESKTOP`, failing that
/// by `DESKTOP_SESSION`, failing that the first token as is, if the session
/// is Wayland at all (an unknown compositor is better than "nothing":
/// wallpapers and fonts are still applied to it).
///
/// The environment values are passed as parameters: the function is pure
/// and is tested without patching `os.environ`.
pub fn detect_current_de(
    xdg_current_desktop: &str,
    desktop_session: &str,
    has_wayland: bool,
) -> Option<String> {
    let xdg = xdg_current_desktop.to_lowercase();
    for part in xdg.split(':') {
        let part = part.trim();
        if let Some((_, de)) = XDG_TOKEN_TO_DE.iter().find(|(k, _)| *k == part) {
            return Some((*de).to_string());
        }
    }
    // Fallback: the session name. We look for a substring: session names are
    // written every which way (`gnome-xorg`, `plasmawayland`, `cinnamon2d`).
    let session = desktop_session.to_lowercase();
    if !session.is_empty() {
        if let Some((_, de)) = XDG_TOKEN_TO_DE.iter().find(|(k, _)| session.contains(*k)) {
            return Some((*de).to_string());
        }
    }
    if has_wayland {
        let first = xdg.split(':').next().unwrap_or("").trim();
        if !first.is_empty() {
            return Some(first.to_string());
        }
    }
    None
}

// ============================================================================
// The "compositor + shell" bundle: who else lives in this session
// ============================================================================
//
// ⚠️ The "one environment = one string" model is incomplete. Under niri one
// can sit bare, or with a third-party shell (Noctalia, DankMaterialShell),
// and these are two DIFFERENT machines: in the second case the background
// is drawn not by our engine but by the shell, with its own fullscreen
// wlr-layer-shell surface on the Background layer, one per output (Noctalia
// uses the namespace `noctalia-wallpaper`, DMS sets no namespace at all).
// Our wallpaper surface ends up under it, and the user sees someone else's
// wallpaper with a green report. `detect_current_de` returns "niri" in both
// cases, so the bundle is recognised with a separate answer.
//
// The core asks the machine NOTHING: observations come in as parameters
// ([`ShellFacts`]) exactly as with `detect_current_de`, gathered by the
// caller. Otherwise the bundle could not be run through a parity check.
//
// ⚠️ Boundary of responsibility. A file in the session directory proves the
// shell WAS HERE: Noctalia removes its socket only in the destructor, after
// a crash the file stays. So "liveness" is the caller's job (flock on
// `noctalia-*.lock`, the `noctalia msg status` answer, `systemctl --user
// is-active`), and the core takes the already obtained fact. A bus name does
// not suffer from this flaw: it cannot be stale, and it is the most
// reliable sign.

/// A third-party desktop shell that draws the background itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopShell {
    /// Noctalia (native C++/GLES, MIT). It lives not only on wlroots but also on
    /// KDE Plasma, so it cannot be recognised by the compositor.
    ///
    /// ⚠️ Noctalia on Plasma is NOT a full integration: their own documentation
    /// (`docs/user/compositor-settings/kde.mdx`) says outright "not a
    /// first-class compositor integration", windows there go through a KWin
    /// script injected over D-Bus (they themselves call it "more fragile"). And
    /// one more trap specific to Plasma: on every change of ITS OWN wallpaper
    /// Noctalia calls `plasma-apply-wallpaperimage`, i.e. rewrites Plasma's own
    /// wallpaper, and does so regardless of `[wallpaper] enabled`.
    Noctalia,
    /// DankMaterialShell (on Quickshell, MIT).
    DankMaterialShell,
}

impl DesktopShell {
    /// Identifier for settings and the log.
    pub fn as_str(self) -> &'static str {
        match self {
            DesktopShell::Noctalia => "noctalia",
            DesktopShell::DankMaterialShell => "dms",
        }
    }

    /// Name for the user (an identifier has no place in on-screen text, rule #13).
    pub fn label(self) -> &'static str {
        match self {
            DesktopShell::Noctalia => "Noctalia",
            DesktopShell::DankMaterialShell => "DankMaterialShell",
        }
    }

    /// All known shells, so that iteration never diverges from the table.
    pub fn all() -> &'static [DesktopShell] {
        &[DesktopShell::Noctalia, DesktopShell::DankMaterialShell]
    }
}

impl Serialize for DesktopShell {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

/// Session observations by which a shell is recognised. All of it is data:
/// file names, unit names, bus names, process names, environment variables.
#[derive(Debug, Clone, Copy, Default)]
pub struct ShellFacts<'a> {
    /// Value of `$WAYLAND_DISPLAY` (empty: not Wayland or unknown).
    pub wayland_display: &'a str,
    /// File names (without path) in `$XDG_RUNTIME_DIR`.
    pub runtime_files: &'a [String],
    /// Active systemd user units, with the suffix, as `systemctl --user` prints
    /// them.
    pub active_user_units: &'a [String],
    /// Owned names on the D-Bus session bus.
    pub dbus_names: &'a [String],
    /// Names of the user's running processes (without path and arguments).
    pub process_names: &'a [String],
    /// Environment variables as name→value pairs (we only need those listed in
    /// [`SHELL_ENV_MARKERS`]; live gathering: [`SHELL_ENV_NAMES`]).
    pub env: &'a [(&'a str, &'a str)],
}

/// A recognised shell together with WHAT it was recognised by.
///
/// The list of confirmations is not decoration: when analysing a "wallpaper
/// did not apply" complaint it is the only thing answering why the core
/// decided the background is foreign.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ShellDetection {
    /// Who exactly.
    pub shell: DesktopShell,
    /// What confirmed it, in human language.
    pub evidence: Vec<String>,
}

/// Exact name of Noctalia's control socket in `$XDG_RUNTIME_DIR`.
///
/// ⚠️ With an empty `WAYLAND_DISPLAY` Noctalia itself substitutes
/// `wayland-0`, and with an empty `XDG_RUNTIME_DIR` it goes to `/tmp`: the
/// core has no reason to chase these fallback paths, the directory and the
/// name are obtained by the caller.
pub fn noctalia_socket_name(wayland_display: &str) -> String {
    format!("noctalia-{}.sock", noctalia_display(wayland_display))
}

/// Exact name of Noctalia's lock file (the shell holds a flock on it against
/// a second launch): the same formula as for the socket.
pub fn noctalia_lock_name(wayland_display: &str) -> String {
    format!("noctalia-{}.lock", noctalia_display(wayland_display))
}

/// Display in Noctalia's file names: the shell replaces an empty one with `wayland-0`.
fn noctalia_display(wayland_display: &str) -> &str {
    if wayland_display.is_empty() {
        "wayland-0"
    } else {
        wayland_display
    }
}

/// Session bus names the shell owns UNCONDITIONALLY.
///
/// Noctalia has no name of its own like `dev.noctalia.Noctalia` (that is
/// only the window app_id), but it always takes two service names.
/// Conditional ones are no good: `dev.noctalia.KWinActiveWindow` exists only
/// on KDE, `org.kde.StatusNotifierWatcher` only if the name is free, and
/// `org.freedesktop.Notifications` belongs to nobody in particular (it is
/// taken by Noctalia, DMS and stock shells alike).
const SHELL_DBUS_NAMES: &[(DesktopShell, &[&str])] = &[
    (DesktopShell::Noctalia, &["dev.noctalia.Debug", "dev.noctalia.Mpris"]),
    (DesktopShell::DankMaterialShell, &[]),
];

/// User units that start the shell.
///
/// ⚠️ Noctalia has NO stock unit (the package installs only the binary and a
/// `.desktop` with `Exec=noctalia --daemon`); `noctalia.service` is created
/// only by the NixOS module. So the absence of the unit proves nothing; only
/// its presence proves something.
///
/// ⚠️ The package installs `dms.service` with
/// `WantedBy=graphical-session.target`. If the person ran
/// `systemctl --user enable dms` (rather than `add-wants` to their
/// compositor, as the authors themselves advise), the unit comes up in ANY
/// session, including Plasma and GNOME. So DMS "under Plasma" can occur,
/// although officially it does not support Plasma: the sign is valid there too.
const SHELL_UNITS: &[(DesktopShell, &[&str])] = &[
    (DesktopShell::Noctalia, &["noctalia.service"]),
    (DesktopShell::DankMaterialShell, &["dms.service"]),
];

/// An environment marker: the variable name and, if it matters, its EXACT value.
///
/// The value is needed where the variable itself is set not only by our
/// shell: `QS_APP_ID` is set by the shared launcher module `dankgo`, and more
/// than just DMS uses it (each program on it has its own identifier).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnvMarker {
    /// Variable name.
    pub name: &'static str,
    /// Required value; `None`: any non-empty one will do.
    pub value: Option<&'static str>,
}

/// Environment variables the shell sets for its UI process and which are
/// inherited by programs launched FROM the shell, i.e. by us.
///
/// DankMaterialShell: the backend sets `DMS_SOCKET` and
/// `QS_APP_ID=com.danklinux.dms` for the `qs` process (the `dankgo` module,
/// `shellapp/env.go`), and `DMS_EXECUTABLE` is set by `dms` itself
/// (`core/cmd/dms/shellapp.go`). There is no environment cleanup before
/// launching programs in the shell (launching goes through
/// `systemd-run --user --scope`, which inherits the environment), so all
/// three reach us too. ⚠️ `QS_APP_ID` is indeed an environment VARIABLE; the
/// old note here ("the AppId pragma in QML, not in the environment") was wrong.
///
/// Noctalia has no variables of its own: `setenv` in its code occurs only in
/// hooks and screenshots.
pub const SHELL_ENV_MARKERS: &[(DesktopShell, &[EnvMarker])] = &[
    (DesktopShell::Noctalia, &[]),
    (
        DesktopShell::DankMaterialShell,
        &[
            EnvMarker { name: "DMS_SOCKET", value: None },
            EnvMarker { name: "DMS_EXECUTABLE", value: None },
            EnvMarker { name: "QS_APP_ID", value: Some("com.danklinux.dms") },
        ],
    ),
];

/// Names of all variables from [`SHELL_ENV_MARKERS`]: what the live caller
/// should gather. The match with the table is guarded by the
/// `tests/desktop_shell.rs` check, otherwise gathering and the table would
/// silently diverge.
pub const SHELL_ENV_NAMES: &[&str] = &["DMS_SOCKET", "DMS_EXECUTABLE", "QS_APP_ID"];

/// Suffixes of DankMaterialShell's service files.
///
/// The names are confirmed by the sources of the `dankgo` launcher module
/// (the commit pinned in `core/go.mod` of DMS 1.6.2): the socket
/// `danklinux-<backend pid>.sock` (`paths/paths.go`, name = the app `ID` +
/// `-<pid>.sock`, the `ID` of DMS is `danklinux`), next to it
/// `danklinux-<pid>.pid` (holding the PID of the `qs` process) and
/// `danklinux-<pid>.session` (holding `WAYLAND_DISPLAY`), see
/// `shellapp/pidfile.go`. On a regular exit both files are removed.
const DMS_RUNTIME_SUFFIXES: &[&str] = &[".sock", ".pid", ".session"];

/// Value from [`ShellFacts::env`] by variable name.
fn env_value<'a>(facts: &ShellFacts<'a>, name: &str) -> &'a str {
    facts
        .env
        .iter()
        .find(|(k, _)| *k == name)
        .map(|(_, v)| *v)
        .unwrap_or("")
}

/// The shell's names from a "shell → list of names" table.
fn names_for(table: &'static [(DesktopShell, &'static [&'static str])], shell: DesktopShell)
    -> &'static [&'static str]
{
    table
        .iter()
        .find(|(s, _)| *s == shell)
        .map(|(_, n)| *n)
        .unwrap_or(&[])
}

/// Confirmations of the presence of one shell.
///
/// Returns (number of PROOFS, list of confirmations). The difference between
/// a proof and a confirmation is fundamental: a process name is NOT a proof,
/// because the shell's own command line (`noctalia msg …`, `dms ipc call …`)
/// has the same name: it lives for milliseconds and does not mean the shell
/// is in the session. Such signs go into the list but do not make the decision.
fn shell_signs(shell: DesktopShell, facts: &ShellFacts) -> (usize, Vec<String>) {
    let mut proofs = 0usize;
    let mut evidence: Vec<String> = Vec::new();

    // 1. A service file in the session directory: created by the live shell ITSELF.
    let file = match shell {
        // The control socket `noctalia-$WAYLAND_DISPLAY.sock` and the lock file
        // `noctalia-$WAYLAND_DISPLAY.lock` (the shell holds a flock on it against
        // a second launch). First the EXACT names for our display: they are the
        // most precise confirmation. As a fallback, any form of
        // `noctalia-*.sock|.lock`: there is no reason to go blind because of an
        // empty or foreign `WAYLAND_DISPLAY`. The fallback also catches the
        // daemon's second socket, `noctalia-dmenu-$WAYLAND_DISPLAY.sock`; this
        // gives no false "shell present", since the daemon creates it too.
        DesktopShell::Noctalia => {
            let exact = [
                noctalia_socket_name(facts.wayland_display),
                noctalia_lock_name(facts.wayland_display),
            ];
            exact
                .iter()
                .find_map(|e| facts.runtime_files.iter().find(|f| *f == e))
                .or_else(|| {
                    facts.runtime_files.iter().find(|f| {
                        f.starts_with("noctalia-")
                            && (f.ends_with(".sock") || f.ends_with(".lock"))
                    })
                })
        }
        // `danklinux-<pid>.sock` / `.pid` / `.session`. The server never creates a
        // name WITHOUT the process number (`danklinux.sock`): it is only a fallback
        // path where the `dms` client looks for the socket
        // (`core/cmd/dms/server_client.go`). We do not take it as a sign.
        DesktopShell::DankMaterialShell => facts.runtime_files.iter().find(|f| {
            let stem_ok = f.starts_with("danklinux-");
            stem_ok && DMS_RUNTIME_SUFFIXES.iter().any(|s| f.ends_with(s))
        }),
    };
    if let Some(name) = file {
        proofs += 1;
        evidence.push(format!("файл {name} в каталоге сеанса"));
    }

    // 2. An owned name on the session bus. It is never stale: the most
    //    reliable sign of all.
    let bus = names_for(SHELL_DBUS_NAMES, shell);
    if let Some(name) = facts.dbus_names.iter().find(|n| bus.contains(&n.as_str())) {
        proofs += 1;
        evidence.push(format!("имя {name} на шине"));
    }

    // 3. A started user unit.
    let units = names_for(SHELL_UNITS, shell);
    if let Some(unit) = facts
        .active_user_units
        .iter()
        .find(|u| units.contains(&u.as_str()))
    {
        proofs += 1;
        evidence.push(format!("запущен юнит {unit}"));
    }

    // 4. A marker in our environment: it means we were launched FROM the shell.
    let markers = SHELL_ENV_MARKERS
        .iter()
        .find(|(s, _)| *s == shell)
        .map(|(_, m)| *m)
        .unwrap_or(&[]);
    for marker in markers {
        let value = env_value(facts, marker.name);
        let hit = match marker.value {
            Some(want) => value == want,
            None => !value.is_empty(),
        };
        if hit {
            proofs += 1;
            evidence.push(match marker.value {
                Some(want) => format!("переменная {}={want} в окружении", marker.name),
                None => format!("переменная {} в окружении", marker.name),
            });
            break;
        }
    }

    // 5. A process name is only a confirmation (see the doc comment). UI
    //    process names (`qs`, `quickshell`) are not taken at all: any Quickshell
    //    shell runs under them, and it can be started by hand.
    let procs: &[&str] = match shell {
        DesktopShell::Noctalia => &["noctalia"],
        DesktopShell::DankMaterialShell => &["dms"],
    };
    for p in procs {
        if facts.process_names.iter().any(|n| n == p) {
            evidence.push(format!("процесс {p}"));
        }
    }

    (proofs, evidence)
}

/// Which third-party shell draws on top of this compositor. `None`: no shell
/// (or it gave itself away in no way: then the background is ours, and that
/// is the right answer, nobody is there to draw over us).
///
/// Each proof is self-sufficient: ONE is enough, a service file in the
/// session directory, a bus name, a started unit or an environment marker.
/// That is how it should be: a shell is started by a unit, by `.desktop`
/// autostart, or by a line in the compositor config, and it takes the
/// wallpaper layer the same way.
pub fn detect_shell(facts: &ShellFacts) -> Option<ShellDetection> {
    let mut best: Option<(usize, ShellDetection)> = None;
    for shell in DesktopShell::all() {
        let (proofs, evidence) = shell_signs(*shell, facts);
        if proofs == 0 {
            continue;
        }
        // Two shells at once do not happen, but if signs were found for both
        // (stale files left from a previous session), we take the one with more
        // proofs; on a tie, the table order. For the main question, "is the
        // background ours", the answer is the same either way.
        let better = best.as_ref().map_or(true, |(p, _)| proofs > *p);
        if better {
            best = Some((proofs, ShellDetection { shell: *shell, evidence }));
        }
    }
    best.map(|(_, d)| d)
}

/// Iteration over the database for the helper table generators
/// (`system_plan.rs`): de_id → packages by family in insertion order.
pub fn de_database_packages(
) -> impl Iterator<Item = (&'static str, &'static [(PkgFamily, &'static [&'static str])])> {
    DE_DATABASE.iter().map(|(k, e)| (*k, e.packages))
}

/// Packages of a (de_id, family) pair by the family's string value, like
/// iterating `db["packages"].items()` in `_de_helper_supports`: None if the
/// DE or the family is missing.
pub fn de_entry_packages_for(
    de_id: &str,
    family_value: &str,
) -> Option<&'static [&'static str]> {
    de_entry(de_id).and_then(|e| {
        e.packages
            .iter()
            .find(|(f, _)| f.as_str() == family_value)
            .map(|(_, pkgs)| *pkgs)
    })
}

// ============================================================================
// polkit: without pkexec NOT A SINGLE privileged operation works
// ============================================================================
//
// ⚠️ There was exactly one `pkexec` presence check in the whole project (in
// the theme installer), and it told the person nothing. Meanwhile on
// openSUSE `pkexec` is a separate subpackage pulled in only via
// `Recommends`: when the system is installed with `--no-recommends` it is
// missing, and then installing desktops, system themes, login screens
// (EVERYTHING privileged) silently does not work. So there is one check,
// early and with a human explanation (rule #13).

/// What to show/log if there is no `pkexec` in the system. `hint` is the
/// "Install the package …" text from `utils::tool_package_hint("pkexec",
/// family)`; empty if no package name was found for the family.
pub fn polkit_missing_message(hint: &str) -> String {
    if hint.is_empty() {
        // TRANSLATORS: Shown when the pkexec program (polkit) is missing. pkexec is a program name.
        crate::i18n::tr(
            "pkexec was not found: Rexon won't be able to do anything that needs \
             administrator rights — installing desktop environments, system \
             themes or login screens.",
        )
    } else {
        // TRANSLATORS: Same as the previous message; %1 is a hint like "Install the polkit package."
        crate::i18n::trf(
            "pkexec was not found: Rexon won't be able to do anything that needs \
             administrator rights — installing desktop environments, system \
             themes or login screens. %1",
            &[&hint],
        )
    }
}

// ============================================================================
// Pure installer algorithms (PackageManager, parsing/filters only)
// ============================================================================
//
// The pacman output is passed as a closure: `Fn(&str) -> Option<String>` is
// the command's stdout for a package when returncode == 0, None on error.
// The live call (`pacman -Qg/-Qi` under LANG=C, LC_ALL=C: a Russian locale
// translates field names and breaks parsing) is executed when connected to
// live Python.

/// Members of a package group from the output of
/// [`crate::system_plan::package_group_query`].
///
/// `ok`: the command exited with zero (did not start at all → false). The
/// only parser of this output in the whole project.
///
/// `None` means "this is not a group": either asking failed or there is no
/// such group. `pacman -Qg` on a non-group prints "error: group … was not
/// found" to stderr, returns code 1 and an EMPTY stdout (checked on a live
/// machine on `bash`, on a nonexistent name and on `base-devel`, which has
/// long been gone from the Arch database). An empty output with a zero code
/// is treated the same: the name stays as is, and that is right: an ordinary
/// package does not become a group.
///
/// The output has one form: "group member" per line; lines shorter than two
/// words are dropped, which also filters out any garbage.
pub fn parse_group_members(ok: bool, stdout: &str) -> Option<Vec<String>> {
    if !ok || stdout.trim().is_empty() {
        return None;
    }
    Some(
        stdout
            .lines()
            .filter_map(|line| {
                let mut parts = line.split_whitespace();
                let (_group, member) = (parts.next()?, parts.next()?);
                Some(member.to_string())
            })
            .collect(),
    )
}

/// Expand package group names into the group's real packages: a port of
/// `PackageManager._expand_groups`.
///
/// In `DE_DATABASE` MATE has "mate" and "mate-extra": these are groups:
/// `pacman -Q mate` fails (group != package), and the engine's group
/// membership query gives "group member" lines. `qg_output(pkg)` is the
/// stdout of that query with a zero code, or None. A successful but empty
/// output leaves the package as is; the result is deduplicated keeping order.
///
/// Families without the notion of a "package group" are cut off by the
/// engine itself ([`crate::system_plan::package_group_query`] returns None
/// for them), but the early Arch exit is kept: without it non-Arch would pay
/// a callback call for every name.
pub fn expand_groups(
    family: PkgFamily,
    packages: &[String],
    qg_output: &dyn Fn(&str) -> Option<String>,
) -> Vec<String> {
    if family != PkgFamily::Arch {
        return packages.to_vec();
    }
    let mut expanded: Vec<String> = Vec::new();
    for p in packages {
        let out = qg_output(p);
        match parse_group_members(out.is_some(), out.as_deref().unwrap_or_default()) {
            Some(members) => expanded.extend(members),
            None => expanded.push(p.clone()),
        }
    }
    let mut seen: HashSet<String> = HashSet::new();
    expanded.into_iter().filter(|p| seen.insert(p.clone())).collect()
}

/// Read one field from the stdout of `pacman -Qi <pkg>`: the pure part of
/// `PackageManager._qi_field` (LANG=C: fields "Required By" | "Depends On" |
/// "Provides" | …). Returns [] if the field is empty ("None") or not found.
///
/// Stripping version constraints cuts the name at the LEFTMOST of the
/// separators ("=", ">", "<"). So "gtk3>=3.24.0" gives "gtk3".
///
/// Fixed a bug of the original: the loop `for sep in "=><"` took the first
/// separator present in the string at all (for "gtk3>=…" that is "=" at
/// pos. 5), not the leftmost one (">" at pos. 4), which gave "gtk3>". Because
/// of this the transitive DE removal protection did not recognise packages
/// with a versioned dependency (`foo>=1.2`: gtk3, qt6-base, lightdm), and the
/// safeguard against removing the active DE did not fire for them.
pub fn parse_qi_field(stdout: &str, field: &str) -> Vec<String> {
    for line in stdout.lines() {
        if !line.starts_with(field) {
            continue;
        }
        let Some((_, rest)) = line.split_once(':') else {
            continue;
        };
        let items: Vec<&str> = rest.split_whitespace().collect();
        if items.is_empty() || items == ["None"] {
            return Vec::new();
        }
        let mut cleaned: Vec<String> = Vec::new();
        for item in items {
            let it = match item.find(['=', '>', '<']) {
                Some(pos) => &item[..pos],
                None => item,
            };
            cleaned.push(it.to_string());
        }
        return cleaned;
    }
    Vec::new()
}

/// Port of `PackageManager._qi_field` in full: non-Arch → [], command error →
/// []. `qi_output(pkg)` is the stdout of `pacman -Qi pkg` or None.
pub fn qi_field(
    family: PkgFamily,
    package: &str,
    field: &str,
    qi_output: &dyn Fn(&str) -> Option<String>,
) -> Vec<String> {
    if family != PkgFamily::Arch {
        return Vec::new();
    }
    match qi_output(package) {
        Some(out) => parse_qi_field(&out, field),
        None => Vec::new(),
    }
}

/// Which installed packages OUTSIDE `candidate_set` require `package`: a port
/// of `PackageManager._required_by_outside`.
pub fn required_by_outside(
    family: PkgFamily,
    package: &str,
    candidate_set: &HashSet<String>,
    qi_output: &dyn Fn(&str) -> Option<String>,
) -> Vec<String> {
    qi_field(family, package, "Required By", qi_output)
        .into_iter()
        .filter(|d| !candidate_set.contains(d))
        .collect()
}

/// Desktop packages by the core's table: main and AUR, the same set that
/// `remove_de` removes (`system_plan::remove_de_decision`).
///
/// The Wayland session add-on ([`SESSION_VARIANTS`]) is included too: these
/// are also desktop packages: the same «Install» button installs them, the
/// privilege session checks them by the table, and while the desktop is the
/// current one, removing a neighbour will not remove them (the shared
/// `labwc` of Xfce and Budgie).
pub fn desktop_packages(de_id: &str, family: PkgFamily) -> Vec<String> {
    let mut all = get_packages(de_id, family);
    let extra = session_packages(de_id, family, SessionKind::Wayland)
        .into_iter()
        .chain(get_aur_packages(de_id, family))
        .chain(session_aur_packages(de_id, family, SessionKind::Wayland));
    for p in extra {
        if !all.contains(&p) {
            all.push(p);
        }
    }
    all
}

// ============================================================================
// X11 or Wayland session: the choice at install time
// ============================================================================
//
// Facts as of 02.11.2026 (Arch/CachyOS packages, archlinux.org file lists):
//   * KDE Plasma 6.8 is Wayland only: `plasma-workspace` puts a single
//     `wayland-sessions/plasma.desktop` and conflicts with
//     `plasma-x11-session`, which is no longer in the repositories;
//   * GNOME 51 is Wayland only (`gnome-session`: a single `gnome.desktop`);
//   * Budgie 10.10 is Wayland only (depends on labwc);
//   * Pantheon 8 (`pantheon-session` 8.1) has only `pantheon-wayland.desktop`;
//   * COSMIC and Deepin on Treeland are Wayland only.
// Three have a choice:
//   * Cinnamon 6.6: both sessions in the `cinnamon` package itself
//     (`cinnamon.desktop` and `cinnamon-wayland.desktop`); Wayland is still
//     experimental (Mint promises to drop the label with the next release);
//   * Xfce 4.20: `xfce-wayland.desktop` is in `xfce4-session`, but
//     `startxfce4 --wayland` does not start without the labwc compositor
//     (an optional dependency of the package: "recommended compositor for
//     the experimental Wayland session");
//   * LXQt 2.4: `lxqt-wayland-session` (extra) and a compositor;
//     `startlxqtwayland` takes labwc first. The project dropped the
//     "experimental" label from the Wayland session in LXQt 2.3.0
//     (lxqt-project.org, "How to Enable Wayland in Lubuntu 26.04",
//     01.05.2026): Wayland is mature.
// MATE 1.28 is installed only with the X11 session, without a switch
// (Wehrwolfmann's decision of 16.11.2026): its Wayland was only from the AUR
// (`mate-wayland-session` on Wayfire, one vote, flagged "out of date" since
// 17.04.2026). The "Wayland add-on from the AUR" mechanism
// ([`WaylandMaturity::ExperimentalAur`], [`SessionVariants::wayland_aur`])
// is kept for MATE (Polo), once a package appears.
// Described only for Arch: in other families the set is not verified, there
// is no choice there and things are installed as before.

/// Session on the login screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionKind {
    X11,
    Wayland,
}

impl SessionKind {
    /// Value for the question dialog's switch.
    pub fn as_str(self) -> &'static str {
        match self {
            SessionKind::X11 => "x11",
            SessionKind::Wayland => "wayland",
        }
    }

    /// Back to [`Self::as_str`]; unknown → `None`.
    pub fn from_value(s: &str) -> Option<Self> {
        match s {
            "x11" => Some(SessionKind::X11),
            "wayland" => Some(SessionKind::Wayland),
            _ => None,
        }
    }
}

/// What the person chose in the «Install?» dialog of a desktop with two sessions.
///
/// The packages differ only in the Wayland add-on ([`Self::install_kind`]):
/// the X11 session is in the desktop's main packages and is always
/// installed, so «Wayland» and «Both» install the same thing, and differ in
/// which session Rexon recommends picking on the login screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionPick {
    X11,
    Wayland,
    Both,
}

impl SessionPick {
    /// All variants, in display order.
    pub const ALL: [SessionPick; 3] = [SessionPick::X11, SessionPick::Wayland, SessionPick::Both];

    /// Value for the question dialog's switch.
    pub fn as_str(self) -> &'static str {
        match self {
            SessionPick::X11 => "x11",
            SessionPick::Wayland => "wayland",
            SessionPick::Both => "both",
        }
    }

    /// Back to [`Self::as_str`]; unknown → `None`.
    pub fn from_value(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.as_str() == s)
    }

    /// Which set the executor installs: with the Wayland add-on for «Wayland»
    /// and «Both».
    pub fn install_kind(self) -> SessionKind {
        match self {
            SessionPick::X11 => SessionKind::X11,
            SessionPick::Wayland | SessionPick::Both => SessionKind::Wayland,
        }
    }

    /// Whether the Wayland add-on is needed (it may also come from the AUR).
    pub fn needs_wayland(self) -> bool {
        self.install_kind() == SessionKind::Wayland
    }
}

/// How ready the desktop's Wayland session is. This drives the default
/// choice ([`SessionVariants::default_session`]) and the captions in the
/// dialog and in «Comparison». Revisit when the project drops the
/// "experimental" label from Wayland.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaylandMaturity {
    /// The project no longer calls the session experimental: it is also the
    /// default.
    Mature,
    /// From the distribution's repositories, but the project itself calls it
    /// experimental.
    Experimental,
    /// Experimental and only from the AUR (built on the machine). Currently no
    /// desktop in the table is marked this way: the former MATE on Wayfire was
    /// removed on 16.11.2026. Kept for MATE (Polo), once a package appears;
    /// under test via `test_session_variants` (tests only).
    ExperimentalAur,
}

/// Two sessions of one desktop: what to add for Wayland and what the
/// sessions are called on the login screen (the `Name=` field of their `.desktop`).
pub struct SessionVariants {
    pub family: PkgFamily,
    /// On top of the main set: for the Wayland session (from the repositories).
    pub wayland_packages: &'static [&'static str],
    /// On top of the main set: for the Wayland session from the AUR (currently
    /// empty for all; for MATE (Polo), once a package appears).
    pub wayland_aur: &'static [&'static str],
    /// What of the add-on is removed together with the desktop: the session
    /// bundle holds the desktop's packages as a dependency. Compositors (labwc,
    /// Wayfire) are not: other desktops may need them or they were installed
    /// on their own.
    pub remove_with_desktop: &'static [&'static str],
    pub maturity: WaylandMaturity,
    pub x11_session_name: &'static str,
    pub wayland_session_name: &'static str,
    /// The X11 session file (put by the desktop's main packages): together with
    /// [`X11_SERVER_BINARY`] it shows that X11 login is possible.
    pub x11_session_file: &'static str,
    /// Who manages windows in the Wayland session instead of the desktop's usual
    /// window manager (empty: the same as in X11).
    pub wayland_compositor: &'static str,
    /// The Wayland session file: together with [`Self::wayland_binaries`] it
    /// shows that Wayland login is already possible.
    pub wayland_session_file: &'static str,
    /// At least one of these executables is needed.
    pub wayland_binaries: &'static [&'static str],
}

pub const SESSION_VARIANTS: &[(&str, SessionVariants)] = &[
    ("cinnamon-mint", SessionVariants {
        family: PkgFamily::Arch,
        // Both sessions are in the `cinnamon` package; Xwayland is pulled in by muffin.
        wayland_packages: &[],
        wayland_aur: &[],
        remove_with_desktop: &[],
        maturity: WaylandMaturity::Experimental,
        x11_session_name: "Cinnamon",
        x11_session_file: "/usr/share/xsessions/cinnamon.desktop",
        wayland_session_name: "Cinnamon (Wayland)",
        wayland_compositor: "",
        wayland_session_file: "/usr/share/wayland-sessions/cinnamon-wayland.desktop",
        wayland_binaries: &["cinnamon-session-cinnamon"],
    }),
    ("xfce", SessionVariants {
        family: PkgFamily::Arch,
        wayland_packages: &["labwc", "xorg-xwayland"],
        wayland_aur: &[],
        remove_with_desktop: &[],
        maturity: WaylandMaturity::Experimental,
        x11_session_name: "Xfce Session",
        x11_session_file: "/usr/share/xsessions/xfce.desktop",
        wayland_session_name: "Xfce Session (Wayland)",
        wayland_compositor: "labwc",
        wayland_session_file: "/usr/share/wayland-sessions/xfce-wayland.desktop",
        // `startxfce4 --wayland` without arguments calls only labwc.
        wayland_binaries: &["labwc"],
    }),
    ("lxqt", SessionVariants {
        family: PkgFamily::Arch,
        // swaylock is the screen lock in Wayland: `lxqt-leave --lockscreen`
        // (Super+L in labwc) calls `lock_command_wayland`, which is empty by
        // default, and the screen did not lock at all.
        wayland_packages: &["lxqt-wayland-session", "labwc", "xorg-xwayland", "swaylock"],
        wayland_aur: &[],
        remove_with_desktop: &["lxqt-wayland-session"],
        maturity: WaylandMaturity::Mature,
        x11_session_name: "LXQt Desktop",
        x11_session_file: "/usr/share/xsessions/lxqt.desktop",
        wayland_session_name: "LXQt (Wayland)",
        wayland_compositor: "labwc",
        wayland_session_file: "/usr/share/wayland-sessions/lxqt-wayland.desktop",
        // Compositors `startlxqtwayland` supports by itself.
        wayland_binaries: &["labwc", "niri", "kwin_wayland", "wayfire", "Hyprland", "sway"],
    }),
];

impl SessionVariants {
    /// What is selected until the person touches the switch: mature Wayland
    /// means Wayland, experimental means the proven X11.
    pub fn default_session(&self) -> SessionKind {
        match self.maturity {
            WaylandMaturity::Mature => SessionKind::Wayland,
            WaylandMaturity::Experimental | WaylandMaturity::ExperimentalAur => SessionKind::X11,
        }
    }

    /// The desktop's Wayland is still experimental.
    pub fn wayland_experimental(&self) -> bool {
        self.maturity != WaylandMaturity::Mature
    }

    /// Recommended variant of the «Install?» dialog: the same maturity-based
    /// choice as [`Self::default_session`].
    pub fn default_pick(&self) -> SessionPick {
        match self.default_session() {
            SessionKind::X11 => SessionPick::X11,
            SessionKind::Wayland => SessionPick::Wayland,
        }
    }
}

/// X server without which the X11 session does not start (LightDM and SDDM
/// depend on it, but the login screen may be a different one).
pub const X11_SERVER_BINARY: &str = "Xorg";

/// Which sessions of an installed desktop already work: (X11, Wayland).
/// `None`: the desktop has no session choice.
pub fn installed_sessions(
    de_id: &str,
    family: PkgFamily,
    file_exists: &dyn Fn(&str) -> bool,
    in_path: &dyn Fn(&str) -> bool,
) -> Option<(bool, bool)> {
    let v = session_variants(de_id, family)?;
    let x11 = file_exists(v.x11_session_file) && in_path(X11_SERVER_BINARY);
    let wayland = wayland_session_ready(de_id, family, file_exists, in_path)?;
    Some((x11, wayland))
}

/// Which session can be added to an installed desktop: no Wayland, then
/// Wayland (as before, whatever the state of X11); only Wayland, then X11.
pub fn missing_session(sessions: (bool, bool)) -> Option<SessionKind> {
    match sessions {
        (_, false) => Some(SessionKind::Wayland),
        (false, true) => Some(SessionKind::X11),
        (true, true) => None,
    }
}

/// Whether the desktop has a session choice in this family.
pub fn session_variants(de_id: &str, family: PkgFamily) -> Option<&'static SessionVariants> {
    #[cfg(test)]
    let extra = test_session_variants::EXTRA.with(std::cell::Cell::get);
    #[cfg(not(test))]
    let extra: &'static [(&'static str, SessionVariants)] = &[];
    SESSION_VARIANTS
        .iter()
        .chain(extra)
        .find(|(id, v)| *id == de_id && v.family == family)
        .map(|(_, v)| v)
}

/// Test override of the session table: the table currently has no desktop
/// with Wayland from the AUR, and the mechanism (refusal without an AUR
/// helper, "from AUR" captions, building by the helper) must stay under test
/// until MATE (Polo).
#[cfg(test)]
pub(crate) mod test_session_variants {
    use super::*;
    use std::cell::Cell;

    thread_local! {
        pub(super) static EXTRA: Cell<&'static [(&'static str, SessionVariants)]> = const { Cell::new(&[]) };
    }

    /// Sample desktop with a Wayland session only from the AUR: the former MATE
    /// entry (`mate-wayland-session` on Wayfire), removed from the product on
    /// 16.11.2026.
    pub(crate) const AUR_SAMPLE: &[(&str, SessionVariants)] = &[
        ("mate", SessionVariants {
            family: PkgFamily::Arch,
            wayland_packages: &["wayfire", "xorg-xwayland"],
            wayland_aur: &["mate-wayland-session"],
            remove_with_desktop: &["mate-wayland-session"],
            maturity: WaylandMaturity::ExperimentalAur,
            x11_session_name: "MATE",
            x11_session_file: "/usr/share/xsessions/mate.desktop",
            wayland_session_name: "MATE (Wayland)",
            wayland_compositor: "Wayfire",
            wayland_session_file: "/usr/share/wayland-sessions/MATE.desktop",
            wayland_binaries: &["wayfire"],
        }),
    ];

    /// Run `f` while the session table contains [`AUR_SAMPLE`] (in this thread
    /// only).
    pub(crate) fn with_aur_sample<R>(f: impl FnOnce() -> R) -> R {
        struct Reset;
        impl Drop for Reset {
            fn drop(&mut self) {
                EXTRA.with(|e| e.set(&[]));
            }
        }
        EXTRA.with(|e| e.set(AUR_SAMPLE));
        let _reset = Reset;
        f()
    }
}

/// Packages on top of the main set for session `kind` (from the repositories).
pub fn session_packages(de_id: &str, family: PkgFamily, kind: SessionKind) -> Vec<String> {
    match (kind, session_variants(de_id, family)) {
        (SessionKind::Wayland, Some(v)) => v.wayland_packages.iter().map(|p| p.to_string()).collect(),
        _ => Vec::new(),
    }
}

/// AUR packages on top of the main set for session `kind`.
pub fn session_aur_packages(de_id: &str, family: PkgFamily, kind: SessionKind) -> Vec<String> {
    match (kind, session_variants(de_id, family)) {
        (SessionKind::Wayland, Some(v)) => v.wayland_aur.iter().map(|p| p.to_string()).collect(),
        _ => Vec::new(),
    }
}

/// What of the Wayland add-on is removed together with the desktop.
pub fn session_removal_packages(de_id: &str, family: PkgFamily) -> Vec<String> {
    session_variants(de_id, family)
        .map(|v| v.remove_with_desktop.iter().map(|p| p.to_string()).collect())
        .unwrap_or_default()
}

/// Whether the desktop's Wayland session can already be entered: the session
/// file exists and there is a compositor. `None`: the desktop has no choice.
pub fn wayland_session_ready(
    de_id: &str,
    family: PkgFamily,
    file_exists: &dyn Fn(&str) -> bool,
    in_path: &dyn Fn(&str) -> bool,
) -> Option<bool> {
    let v = session_variants(de_id, family)?;
    Some(file_exists(v.wayland_session_file) && v.wayland_binaries.iter().any(|b| in_path(b)))
}

/// Login screen package by its unit name: usually the same; the exceptions
/// are the core's tables (`plasmalogin` → `plasma-login-manager`, on Debian
/// `gdm` → `gdm3`).
pub fn dm_package_for_unit(family: PkgFamily, unit: &str) -> String {
    let table = if family == PkgFamily::Debian {
        crate::system_plan::DM_DEBIAN_PKG_FOR_UNIT
    } else {
        crate::system_plan::DM_PKG_FOR_UNIT
    };
    table
        .iter()
        .find(|(u, _)| *u == unit)
        .map(|(_, p)| p.to_string())
        .unwrap_or_else(|| unit.to_string())
}

/// A LightDM greeter (`lightdm-gtk-greeter`, `lightdm-slick-greeter`, …):
/// `lightdm` itself depends on none of them, and removing the desktop that
/// brought the selected greeter would leave the login screen without a
/// face: the service does not start. While the active login screen is
/// LightDM, no greeter is removed.
pub fn is_lightdm_greeter(package: &str) -> bool {
    package.starts_with("lightdm-") && package.contains("greeter")
}

/// What removing ANOTHER desktop never touches: the current desktop's
/// packages by the core's table (main and AUR) and the system's active login
/// screen: its unit, package ([`dm_package_for_unit`]) and, for LightDM, the
/// greeters ([`is_lightdm_greeter`]; checked by [`without_current_desktop`]).
///
/// ⛔ The hole this closed (28.10.2026): the `remove_de` guard compared only
/// desktop identifiers. Sitting in Xfce, removing Cinnamon removed `lightdm`
/// (the shared login screen of both), and after a reboot there was no
/// graphical login. The dependency closure did not hold it: nobody needs
/// `lightdm` by `Required By`.
///
/// `current_de` is unknown (`""`, `unknown`): there is no table, the login
/// screen remains; `active_dm_unit` is the `active_login_manager_opt` answer
/// (`None`: nothing to determine it with).
pub fn current_desktop_keep(
    family: PkgFamily,
    current_de: &str,
    active_dm_unit: Option<&str>,
) -> Vec<String> {
    let mut keep = desktop_packages(current_de.trim(), family);
    if let Some(unit) = active_dm_unit.map(str::trim).filter(|u| !u.is_empty()) {
        for p in [unit.to_string(), dm_package_for_unit(family, unit)] {
            if !keep.contains(&p) {
                keep.push(p);
            }
        }
    }
    keep
}

/// A desktop's removal set without what the current desktop holds: Arch
/// groups are expanded on BOTH sides (`qg` is the `pacman -Qg` output, as in
/// [`expand_groups`]), and a member of a shared group is protected too;
/// LightDM greeters stay while `keep` contains `lightdm`. Order as in `all`.
pub fn without_current_desktop(
    family: PkgFamily,
    all: &[String],
    keep: &[String],
    qg_output: &dyn Fn(&str) -> Option<String>,
) -> Vec<String> {
    // Compare by name without the Gentoo category: the `lightdm` unit also
    // holds `x11-misc/lightdm` from the table.
    let base = |p: &str| p.rsplit('/').next().unwrap_or(p).to_string();
    let mut held: HashSet<String> = keep.iter().map(|p| base(p)).collect();
    held.extend(expand_groups(family, keep, qg_output).iter().map(|p| base(p)));
    let greeters = held.contains("lightdm");
    expand_groups(family, all, qg_output)
        .into_iter()
        .filter(|p| {
            let b = base(p);
            !held.contains(&b) && !(greeters && is_lightdm_greeter(&b))
        })
        .collect()
}

/// Two-pass filter for safe removal (Arch only): a port of
/// `PackageManager._filter_removable_transitive`. Returns (safe, skipped)
/// keeping the original order of `packages`.
///
/// 1. Direct external deps: the package is required by someone OUTSIDE the
///    list → forbidden.
/// 2. Transitively: if forbidden package X depends on Y (Y in the list), Y is
///    forbidden too, otherwise pacman fails with "could not satisfy". The
///    closure runs to a fixed point; a dependency cycle does not loop
///    (verified by the golden case `dependency-cycle`).
pub fn filter_removable_transitive(
    family: PkgFamily,
    packages: &[String],
    qi_output: &dyn Fn(&str) -> Option<String>,
) -> (Vec<String>, Vec<String>) {
    if family != PkgFamily::Arch {
        return (packages.to_vec(), Vec::new());
    }
    let candidate: HashSet<String> = packages.iter().cloned().collect();
    let mut forbidden: HashSet<String> = HashSet::new();
    for p in packages {
        if !required_by_outside(family, p, &candidate, qi_output).is_empty() {
            forbidden.insert(p.clone());
        }
    }
    // Transitive closure: deps of forbidden packages that are in candidate
    let mut changed = true;
    while changed {
        changed = false;
        for p in forbidden.clone() {
            let deps: HashSet<String> =
                qi_field(family, &p, "Depends On", qi_output).into_iter().collect();
            for d in deps.intersection(&candidate) {
                if !forbidden.contains(d) {
                    forbidden.insert(d.clone());
                    changed = true;
                }
            }
        }
    }
    let safe = packages.iter().filter(|p| !forbidden.contains(*p)).cloned().collect();
    let skipped = packages.iter().filter(|p| forbidden.contains(*p)).cloned().collect();
    (safe, skipped)
}
