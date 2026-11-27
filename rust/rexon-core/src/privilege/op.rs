//! Privilege session operations (item B1 of the plan, see [`super`]).
//!
//! [`PrivOp`] is the closed list of everything Rexon does as root (compiled from
//! an inventory of all the places where the code called root). Each variant
//! carries the operation's DATA (family, package names, unit, category), not a
//! command string (prohibition Z1): the command is built by whoever executes it.
//!
//! One operation has two executors:
//! * the privilege session — the operation goes to the helper in a `Request`
//!   (`proto.rs`), which checks it (`validate.rs`) and builds the command itself
//!   from the core's tables;
//! * the "as now" path — [`PrivOp::pkexec_argv`]: the same command that the
//!   former builders build today (`system_plan`, `repo_rebuild`, `aur_install`,
//!   `de_apply`, `steamos_sysext`…). Here they are only CALLED: there must be no
//!   second copy of the helpers' flags and paths. The path is needed when there
//!   is no session helper at all (Z5: not when its check refuses).
//!
//! # Local files (Z2)
//!
//! A file or directory from the user's machine (a built package, a theme, a
//! login screen background) is a [`LocalFile`]: the descriptor number in the
//! `SCM_RIGHTS` of the same frame and a label. The label is the path on the
//! window's side; the session helper opens nothing by it (it is only log text).
//! The former helpers accept a path — so only the "as now" path puts the label
//! into argv.

use serde::{Deserialize, Serialize};

use crate::system::PkgFamily;
use crate::system_plan as plan;

/// `PkgFamily` in the protocol is a string `as_str` (the type itself has only
/// `Serialize`; parsing goes through `from_value`, a foreign word is a frame
/// error).
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

/// A user's file or directory passed as an open descriptor (Z2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalFile {
    /// Descriptor number among the `SCM_RIGHTS` of the frame with this operation.
    pub slot: u8,
    /// Path on the window's side: for the log and for the "as now" path. The
    /// session helper opens nothing by it.
    pub label: String,
}

/// A font file for installing "for all users" ([`PrivOp::FontInstall`]): the
/// descriptor and the name under which the file will lie in
/// [`crate::qt_settings::SHARED_FONT_DIR`]. The name is only a font file name
/// ([`super::validate::check_font_file_name`]); the helper builds the
/// destination path itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FontFile {
    pub source: LocalFile,
    pub name: String,
}

/// A desktop settings file for the SDDM login screen ([`PrivOp::SddmLook`]): the
/// descriptor and the name from [`plan::SDDM_LOOK_FILES`] — where it will lie in
/// `~sddm/.config`. The helper builds the destination path itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LookFile {
    pub source: LocalFile,
    pub name: String,
}

/// Which core table a package installation follows ([`PrivOp::PkgInstall`]).
/// The window names the table, the root side takes the contents from its own
/// binary ([`super::validate::install_allowlist`]): a name outside the named
/// table will not pass, even if it is in another one. The window does not pass
/// the current desktop for a repair — the helper learns it from logind itself.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum PkgSource {
    /// A desktop set from the database ([`crate::system::DE_DATABASE`]).
    Desktop { de_id: String },
    /// Login screen ([`plan::DM_PAIRS`]).
    DisplayManager,
    /// "Check my desktop": packages of the CURRENT desktop and the `compliance` table.
    DesktopRepair,
    /// Video dependencies of wallpapers.
    VideoDeps,
    /// yt-dlp for YouTube wallpapers.
    YtDlp,
    /// The Gentoo overlay connection tool ([`plan::GENTOO_OVERLAY_TOOL_PACKAGE`]).
    GentooTool,
    /// Desktop shells ([`crate::shell_bundle::all_packages`]).
    Shell,
    /// Conky widget programs and the 7z unpacker
    /// ([`crate::utils::tool_packages_for`]).
    Tools,
    /// Conky itself ([`crate::de_apply::conky_package`]).
    Conky,
    /// Parts of MATE layouts ([`crate::mate_panel::all_part_packages`]).
    MatePanel,
    /// Icons and cursors of themes ([`crate::themes::known_set_packages`]).
    ThemeSets,
    /// "Like in a distribution" looks ([`crate::distro_looks::builtin_package_names`]).
    DistroLook,
}

impl PkgSource {
    /// All tables without parameters (except [`PkgSource::Desktop`]) — for
    /// completeness checks: every table of the package helper's allow-list is named
    /// here.
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

    /// The root side's check table; `current_de` — the desktop the helper learned
    /// by itself (for [`PkgSource::DesktopRepair`]).
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

/// Kind of system theme (the `rexon-system-helper` helper).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SystemThemeKind {
    Sddm,
    Plymouth,
}

impl SystemThemeKind {
    /// The helper's word: `sddm`, `plymouth` (and `apply-…`/`remove-…`).
    pub fn as_str(self) -> &'static str {
        match self {
            SystemThemeKind::Sddm => "sddm",
            SystemThemeKind::Plymouth => "plymouth",
        }
    }
}

/// Family for the system helper: it knows only two ways of building the kernel
/// image (`mkinitcpio`/`dracut` and `update-initramfs`).
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

/// Operations with a system theme (inventory B-1…B-4, B-7, B-8).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum SystemThemeOp {
    /// Put the theme into `/usr/share/<kind>/themes/<theme>` (Plymouth — with a
    /// `ModuleName` check inside the helper). `source` — the theme directory.
    Install {
        kind: SystemThemeKind,
        theme: String,
        source: LocalFile,
        family: ThemeFamily,
    },
    /// "Apply" an already installed theme.
    Apply {
        kind: SystemThemeKind,
        theme: String,
        family: ThemeFamily,
    },
    /// "Remove" a theme (not the active one and not one from a system package).
    Remove {
        kind: SystemThemeKind,
        theme: String,
        family: ThemeFamily,
    },
}

/// A desktop image operation on SteamOS (inventory A-19).
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

/// How to connect a Gentoo overlay (inventory A-9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GentooOverlayTool {
    Eselect,
    Layman,
}

/// How to restore the stock boot splash (inventory A-18).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlymouthTool {
    /// `plymouth-set-default-theme <theme> -R`.
    SetDefaultTheme,
    /// `update-alternatives` + `update-initramfs` (Debian/Ubuntu).
    UpdateAlternatives,
}

/// A privileged Rexon operation. The numbers in comments are places in the v3
/// inventory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum PrivOp {
    /// Install packages from the repositories (Y1/Y3: A-13, A-20, B-10, B-11, B-14,
    /// the SDDM package in A-6, the Gentoo overlay tool in A-9).
    PkgInstall {
        #[serde(with = "family_serde")]
        family: PkgFamily,
        packages: Vec<String>,
        /// The table the names come from (the helper checks only against it).
        source: PkgSource,
    },
    /// Remove packages (A-7 desktop removal; B-12 "Like in a distribution").
    PkgRemove {
        #[serde(with = "family_serde")]
        family: PkgFamily,
        packages: Vec<String>,
    },
    /// Install a desktop as a set from the DE helper's table (A-1); the helper
    /// decides the login screen itself. `log_label` — the log of the "as now" path;
    /// the session sends the operation's progress over the socket.
    DeInstall {
        #[serde(with = "family_serde")]
        family: PkgFamily,
        de_id: String,
        log_label: String,
    },
    /// Install a desktop as a set outside the helper's table (A-2): DKMS
    /// `--asdeps`, replacing a conflicting package, Deepin. The "as now" path is a
    /// wrapper script, not a single command (see [`PrivOp::pkexec_argv`]).
    DeInstallSet {
        #[serde(with = "family_serde")]
        family: PkgFamily,
        de_id: Option<String>,
        packages: Vec<String>,
        deps_only: Vec<String>,
        replace: Vec<String>,
        update_first: bool,
    },
    /// Enable the login screen after installing a desktop (A-5, the second half of A-6).
    EnableDm {
        #[serde(with = "family_serde")]
        family: PkgFamily,
        unit: String,
    },
    /// "Make default" a login screen with automatic rollback (A-8). There is no raw path.
    SwitchDm {
        #[serde(with = "family_serde")]
        family: PkgFamily,
        unit: String,
        greeter: Option<String>,
    },
    /// Make the default session of the login screen the Wayland pair of the X11
    /// session `x11` ([`plan::X11_SESSION_SUCCESSORS`]): only SDDM and Plasma Login
    /// Manager session keys pointing to `x11`; autologin is neither enabled nor
    /// disabled. That the pair is installed and that after the edit the login
    /// screen itself does not open `x11`, the root helper checks itself.
    LoginSessionWayland { x11: String },
    /// Build dependencies of a rebuild (Y4: A-14…A-16).
    BuildDeps { names: Vec<String> },
    /// Install a package built by the user and remove the build dependencies (Y4).
    InstallBuilt {
        package: LocalFile,
        build_only: Vec<String>,
        replace: bool,
    },
    /// A build from AUR (Y5: A-4, A-12, A-22, B-10, B-13). Only the builder's pacman
    /// calls run as root here (item B10); it does not go to the session helper.
    AurInstall {
        helper: String,
        packages: Vec<String>,
        replace_conflicts: bool,
    },
    /// Rebuilding AeroShell (A-23); the same as [`PrivOp::AurInstall`].
    AeroShellRebuild {
        helper: String,
        packages: Vec<String>,
        chroot: bool,
    },
    /// The `rexon-sudo-shim` shim (item B10, [`super::aur`]): AUR build
    /// dependencies from the official repositories — `pacman -S --needed --asdeps`.
    /// The helper itself checks every name against its machine's sync database.
    AurDeps { names: Vec<String> },
    /// Shim: packages built by paru/yay — `pacman -U` of root's copies. `allowed` —
    /// what the window asked to build in this operation: the `pkgname` from the
    /// `.PKGINFO` of each copy must come from there.
    AurInstallBuilt {
        packages: Vec<LocalFile>,
        allowed: Vec<String>,
        replace: bool,
    },
    /// Shim: remove build dependencies after the build (`--removemake`). The helper
    /// removes only orphaned dependencies (`pacman -Qqdt`).
    AurDropMake { names: Vec<String> },
    /// Lay a directory out into a system one by category (Y2: B-5, B-6, A-10,
    /// A-21). The destination is a core constant by `category`.
    Stage {
        category: String,
        name: String,
        source: LocalFile,
    },
    /// SDDM/Plymouth system themes (Y6, B-1…B-4, B-7, B-8).
    SystemTheme(SystemThemeOp),
    /// Build/remove a desktop as a sysext image on SteamOS (A-19).
    Sysext { op: SysextOp, de_id: String },
    /// `emerge --sync` of a single overlay (A-9).
    GentooSync { repo: String },
    /// Connect a Gentoo overlay (A-9).
    GentooOverlay {
        overlay: String,
        tool: GentooOverlayTool,
    },
    /// Remove the old system override of MATE schemas (A-11).
    MateGschemaCleanup,
    /// Remove Plasma forks and restore the stock packages (A-17): the window's
    /// decision ([`plan::plasma_fork_split`]) — what to remove and what to install.
    PlasmaForksReset {
        removable: Vec<String>,
        stock: Vec<String>,
    },
    /// Restore the stock boot splash (A-18).
    PlymouthReset { target: String, tool: PlymouthTool },
    /// Remove Rexon's files from `/etc/sddm.conf.d` (A-18). `files` — file names.
    SddmReset { files: Vec<String> },
    /// Remove an orphan package from the update guard window
    /// ([`crate::update_guard::remove_orphans_live`]): a set member that left the
    /// repositories (`kwin-x11` 6.7.5 with Plasma 6.8). Only pacman. That each name
    /// is an installed orphan of a set member, the root helper checks itself
    /// ([`crate::update_guard::orphan_candidate`], its machine's sync database); it
    /// does not trust the window.
    OrphanRemove { names: Vec<String> },
    /// Restore previous versions of the update guard's set from the pacman cache
    /// ([`crate::update_guard::rollback_live`]). `files` — paths of cache files as
    /// the window found them. Only pacman. That each file is a root package in this
    /// machine's cache directory, that the versions (per `.PKGINFO`) are strictly
    /// lower than the installed ones and that this is exactly the rollback of the
    /// finding's set, the root helper checks itself; it does not trust the window.
    CacheRollback { files: Vec<String> },
    /// Install fonts for all users ("Font management" → "For all users", like the
    /// "System" group in KDE): root's copies into
    /// [`crate::qt_settings::SHARED_FONT_DIR`], then `fc-cache`. That each file is
    /// a font (`fc-query` on root's copy), that the name is a font file name and
    /// that such a file is not there yet (nothing foreign is overwritten), the
    /// helper checks itself.
    FontInstall { fonts: Vec<FontFile> },
    /// Remove fonts installed for all users: only files inside
    /// [`crate::qt_settings::SHARED_FONT_DIR`] (fonts of packages in
    /// `/usr/share/fonts` may not be removed), then `fc-cache`.
    FontRemove { files: Vec<String> },
    /// Background of the SDDM login screen theme (the KDE module's "Change
    /// background"): the picture goes into the theme directory under the name
    /// [`plan::SDDM_BACKGROUND_STEM`], the name into the theme's `<ConfigFile>.user`.
    /// `image: None` — restore the theme's own background. That the theme is in
    /// the system directory and accepts a background, and that the file is PNG or
    /// JPEG (by the signature of root's copy), the helper checks itself.
    SddmBackground { theme: String, image: Option<LocalFile> },
    /// Carry the desktop's appearance over to the SDDM login screen (the KDE
    /// module's "Apply Plasma Settings"): keys [`plan::SDDM_LOOK_KEYS`] go into the
    /// SDDM settings per the rule [`plan::plan_sddm_look`], files into
    /// `~sddm/.config` as the `sddm` user. `look: None` — "Reset" (there are no
    /// files then).
    SddmLook { look: Option<plan::SddmLook>, files: Vec<LookFile> },
    /// "Show clock" of the SDDM login screen theme (the KDE module's checkbox):
    /// `showClock` goes into the theme's `<ConfigFile>.user`. That the theme is in
    /// the system directory and accepts its settings, the helper checks itself, as
    /// for [`PrivOp::SddmBackground`].
    SddmClock { theme: String, show: bool },
}

/// Which of the former helpers are on the machine — the input of the "as now"
/// path. Filled in by the caller with the same probes as today
/// (`pkg_helper_available`, `stage_helper_available`, …).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Helpers {
    /// `rexon-pkg-helper` and its policy are in place.
    pub pkg: bool,
    /// `rexon-stage-helper`.
    pub stage: bool,
    /// `rexon-de-helper`.
    pub de: bool,
    /// `rexon-system-helper` (+ `pkexec`) — the path is taken from
    /// [`crate::installer::system_helper_path`].
    pub system: bool,
    /// Text of the installed `rexon-sysext-helper` (SteamOS), if any.
    pub sysext_installed: Option<String>,
    /// Interface language code for the sysext helper (`ru`).
    pub lang: String,
}

impl PrivOp {
    /// argv of the "as now" path — what the corresponding place in the code builds
    /// today (the former named helper, otherwise a raw `pkexec`).
    ///
    /// `None` — there is no single command:
    /// * the builder refused (empty, a name failed the check, no helper where there
    ///   is no raw path per Z5 — `switch-dm`, system themes);
    /// * today's raw path is a script in a temporary file (`DeInstallSet`, `Stage`
    ///   and `SystemTheme::Install` without a helper, `MateGschemaCleanup`): its
    ///   text is [`PrivOp::fallback_script`] or the plan's former builder.
    ///
    /// On NixOS installation and removal go without `pkexec` (the user's profile) —
    /// as today.
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
            // There is no raw path: without the session helper nobody can check that
            // exactly the session key changes and that the pair is installed.
            PrivOp::LoginSessionWayland { .. } => None,
            // Rebuild: with the package helper — its `build-deps`/`install-built`
            // (one password for both steps), without it — `pkexec /bin/bash`.
            // The caller sets `helpers.pkg` for `InstallBuilt` the same way as
            // `repo_rebuild` does: there is a helper AND the file lies in the rebuild
            // directory.
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
            // The shim's old path is `pkexec` of the same pacman command line the
            // builder gave (the shim itself decides by the window's answer,
            // [`super::aur`]).
            PrivOp::AurDeps { .. } | PrivOp::AurInstallBuilt { .. } | PrivOp::AurDropMake { .. } => None,
            // There is deliberately no old path: without the session helper nobody can
            // check as root "is this an orphan", and a raw `pacman -Rns` on the
            // window's word is exactly what the check protects against.
            PrivOp::OrphanRemove { .. } => None,
            // The same: without the session helper nobody can check "is this exactly
            // the rollback of the set", and a raw `pacman -U` on the window's paths is
            // what the check protects against.
            PrivOp::CacheRollback { .. } => None,
            // Fonts "for all" deliberately have no old path: without the session
            // helper nobody can check that the file is a font and that nothing foreign
            // is overwritten, and a raw `pkexec cp` on the window's path is what the
            // check protects against.
            PrivOp::FontInstall { .. } | PrivOp::FontRemove { .. } => None,
            // SDDM login screen: without the session helper nobody can check that the
            // picture is a picture, that the keys are exactly the carry-over keys and
            // that the files in the `sddm` home are written as that user (Z5).
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

    /// A raw-path script where today it goes as `pkexec bash <file>` and the whole
    /// text is derived from the operation's data: `(file prefix, text)`.
    pub fn fallback_script(&self) -> Option<(&'static str, String)> {
        match self {
            PrivOp::MateGschemaCleanup => Some(("rexon_gschema_", plan::mate_gschema_cleanup_script())),
            _ => None,
        }
    }

    /// Files the client attaches to the `SCM_RIGHTS` frame, in order of appearance
    /// in the operation.
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

/// Removal: the helper's named path, otherwise a raw `pkexec` over
/// [`plan::remove_cmd`]; NixOS — without `pkexec` (as in [`plan::remove_plan`]).
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

/// The DE helper's command — from the same desktop install plan as today
/// ([`plan::install_batch_plan_with_deps`] with a set from the helper's table).
/// The helper does not install a set outside its table — then `None`.
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

/// The system helper. "Apply"/"Remove" have no raw path; for installation it
/// is a script (`ext_manager::sddm_fallback_script`), not a single command.
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
        // Warning: this line has no builder of its own: it is assembled in place in
        // `appearance_installer::plan_root_install_with` and twice in
        // `ext_manager` (SDDM, Plymouth). The check is the test below.
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

/// Warning: the literals of `system_de_exec::ensure_gentoo_overlay` — there is
/// no builder (the method is private); the check is the test below.
fn gentoo_overlay_argv(overlay: &str, tool: GentooOverlayTool) -> Option<Vec<String>> {
    // Only overlays from the core table — the same ones the desktop install connects.
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

/// The fork rollback command — from [`plan::plasma_fork_reset_plan`]: the
/// window's decision (`removable`, `stock`) is fed to it as probes. The stock
/// comes through substitute forks `\0<stock>` with `Provides: <stock>`; the
/// forks being removed go under their own names without metadata — like in
/// the window's decision, they have no candidates. The plan rejects an
/// inconsistent decision (`Blocked`) — `None`.
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

    /// One sample for each variant (and for each kind of nested ones).
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
                    // i18n-exempt: non-ASCII user paths and font names must survive the round trip
                    FontFile { source: file(0, "/home/u/Загрузки/Мой шрифт.ttf"), name: "Мой шрифт.ttf".into() },
                    FontFile { source: file(1, "/home/u/b.otf"), name: "b.otf".into() },
                ],
            },
            PrivOp::FontRemove { files: names(&["/usr/local/share/fonts/b.otf"]) },
            PrivOp::SddmBackground { theme: "breeze".into(), image: Some(file(0, "/home/u/Картинки/фон.jpg")) }, // i18n-exempt: non-ASCII user path
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
        // A foreign family, an extra field, an unknown operation — a parsing error.
        for bad in [
            r#"{"pkg_install":{"family":"windows","packages":["a"],"source":"yt_dlp"}}"#,
            r#"{"pkg_install":{"family":"arch","packages":["a"],"source":"yt_dlp","argv":["sh"]}}"#,
            // Without a table, with a foreign table, with the current desktop from the window.
            r#"{"pkg_install":{"family":"arch","packages":["a"]}}"#,
            r#"{"pkg_install":{"family":"arch","packages":["a"],"source":"anything"}}"#,
            r#"{"pkg_install":{"family":"arch","packages":["a"],"source":{"desktop_repair":{"current_de":"x"}}}}"#,
            r#"{"pkg_install":{"family":"arch","packages":["a"],"source":{"desktop":{"de_id":"x","extra":1}}}}"#,
            r#"{"build_deps":{"names":["a"],"path":"/tmp/x"}}"#,
            r#"{"stage":{"category":"gdm-bg","name":"n","source":{"slot":0,"label":"/tmp/x","path":"/etc"}}}"#,
            r#"{"run_shell":{"cmd":"id"}}"#,
            // Fonts: the destination path from the window, an extra field in a file.
            r#"{"font_install":{"fonts":[{"source":{"slot":0,"label":"/tmp/a.ttf"},"name":"a.ttf"}],"dest":"/usr/share/fonts"}}"#,
            r#"{"font_install":{"fonts":[{"source":{"slot":0,"label":"/tmp/a.ttf"},"name":"a.ttf","dir":"/etc"}]}}"#,
            r#"{"font_remove":{"files":["/usr/local/share/fonts/a.ttf"],"root":"/"}}"#,
            // SDDM login screen: a theme or destination path from the window, a foreign
            // settings key, a NumLock value not from the list.
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
            .expect("the step exists")
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
        // The "Check my desktop" repair — the same command.
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
        // Without a helper, and for a desktop outside its table (Deepin), there is no
        // single command.
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
            // "No login screen → SDDM": both commands of the plan.
            match plan::ensure_dm_plan(PkgFamily::Arch, false, true, on) {
                plan::EnsureDmPlan::Install { install, enable, .. } => {
                    let en = PrivOp::EnableDm { family: PkgFamily::Arch, unit: "sddm".into() };
                    assert_eq!(en.pkexec_argv(&helpers(on)), Some(enable.argv));
                    let inst = PrivOp::PkgInstall { family: PkgFamily::Arch, packages: names(&["sddm"]), source: PkgSource::DisplayManager };
                    assert_eq!(inst.pkexec_argv(&helpers(on)), Some(install.argv));
                }
                _ => panic!("expected an SDDM install"),
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
        // There is no raw path (Z5).
        let op = PrivOp::SwitchDm { family: PkgFamily::Arch, unit: "sddm".into(), greeter: None };
        assert_eq!(op.pkexec_argv(&helpers(false)), None);
    }

    #[test]
    fn rebuild_steps_match_repo_rebuild() {
        // The same beginnings that `repo_rebuild::rebuild_ctx_live` puts into
        // `RebuildCtx::pkg_helper`, and the same builders the rebuild calls.
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
        // Every table without parameters is in the list of checks.
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
                let pkgs = names(&["mate-tweak", "mate-menu"]);
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
        // A label outside /tmp — the same builder refuses.
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

                // Installation — against the "Appearance" plan (the line is assembled in place there).
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
                    .unwrap_or_else(|_| panic!("install plan"))
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
        // The literals of `system_de_exec::ensure_gentoo_overlay` (the method is
        // private — checked against its text until the lines are moved into a builder).
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
        // A fork provider (stock via Provides), a fork via the fallback map and a
        // standalone fork.
        let forks = names(&["aeroshell-kwin-git", "aeroshell-libplasma", "aeroshell-workspace-git"]);
        let qi = |f: &str| match f {
            "aeroshell-kwin-git" => Some("Provides        : kwin\n".to_string()),
            _ => None,
        };
        let exists = |n: &str| Some(matches!(n, "kwin" | "libplasma"));
        let plan::PlasmaForkReset::Run { cmd, removable, stock, .. } = plan::plasma_fork_reset_plan(&forks, &qi, &exists) else {
            panic!("expected a rollback");
        };
        assert!(!removable.is_empty() && !stock.is_empty(), "{removable:?} {stock:?}");
        let op = PrivOp::PlasmaForksReset { removable: removable.clone(), stock: stock.clone() };
        assert_eq!(op.pkexec_argv(&helpers(true)), Some(cmd.argv));
        // Only remove / only install.
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
                panic!("expected a launch");
            };
            let op = PrivOp::PlymouthReset { target: "bgrt".into(), tool };
            assert_eq!(op.pkexec_argv(&helpers(true)), Some(cmd.argv));
        }
        let conf = vec![("zz-rexon-theme.conf".to_string(), "[Theme]\nCurrent=x\n".to_string())];
        let reset = plan::sddm_reset_plan(&conf, &[], None);
        let plan::SddmReset::Run { cmd, removed, .. } = plan::sddm_reset_command(&reset, plan::SDDM_CONF_D) else {
            panic!("expected removal of a Rexon file");
        };
        let op = PrivOp::SddmReset { files: removed };
        assert_eq!(op.pkexec_argv(&helpers(true)), Some(cmd.argv));
        let none = PrivOp::SddmReset { files: vec![] };
        assert_eq!(none.pkexec_argv(&helpers(true)), None);
    }

    /// Fonts "for all" — only through the session helper: there is no raw `pkexec`
    /// either with the former helpers or without them (Z5).
    #[test]
    fn fonts_have_no_raw_path() {
        for op in samples().iter().filter(|o| matches!(o, PrivOp::FontInstall { .. } | PrivOp::FontRemove { .. })) {
            assert_eq!(op.pkexec_argv(&helpers(true)), None, "{op:?}");
            assert_eq!(op.pkexec_argv(&helpers(false)), None, "{op:?}");
            assert_eq!(op.fallback_script(), None, "{op:?}");
        }
    }

    /// SDDM login screen — also only through the session helper (Z5).
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
        // InstallBuilt, Stage, SystemTheme::Install, SddmBackground with a picture —
        // one file each; AurInstallBuilt, FontInstall and SddmLook — two each.
        assert_eq!(with_files.iter().sum::<usize>(), 10);
    }
}
