//! Root-side checks (item B3 of the plan, see [`super`]).
//!
//! Helper "B" lives for the whole window session, so it cannot trust the
//! window in anything: everything that came over the socket is checked here,
//! BEFORE execution, and only here is "allowed or not" decided. The functions
//! are pure, without I/O: everything to know about the machine (the current
//! desktop, `pacman -Qi` answers, the rebuild recipe, `PKEXEC_UID`) comes as
//! parameters. The operation type (`PrivOp`, item B1) does not enter here:
//! the functions take simple arguments, and `PrivOp` calls them itself (item B8).
//!
//! # The tables are the core's, not our own
//!
//! There are no package lists here. The install allowlists are the same
//! tables the window builds plans from: [`system::DE_DATABASE`] (desktops),
//! [`plan::DM_PAIRS`] (login screens), [`crate::compliance`] (desktop
//! completion), [`crate::wallpaper_engine::install_deps_packages`] (live
//! wallpapers), shells, widget programs, Conky, MATE layout parts, theme
//! icons and cursors, «Distro Looks» looks: everything listed by
//! [`plan::pkg_helper_install_allowlist`] (check
//! `таблицы_сеанса_совпадают_с_белым_списком_помощника_пакетов`),
//! and the rebuild recipe ([`repo_rebuild::build_requirements`]). The
//! permanent removal list is [`plan::PKG_HELPER_VITAL`], the package name is
//! [`plan::is_safe_pkg_arg`], the dependency closure is
//! [`system::filter_removable_transitive`], the current desktop protection is
//! [`plan::remove_de_decision`]. This way the check of helper "B" and the
//! check of the current `rexon-*-helper` cannot diverge.
//!
//! # Prohibition to function
//!
//! | Plan row | Function |
//! |---|---|
//! | Z1 neither a flag nor a shell in an argument | [`check_operand`] |
//! | Z2 a name, not a path | [`check_package_name`], [`check_item_name`], [`check_de_id`] |
//! | Z2 a path only inside the user root | [`check_user_path`], [`check_built_package`] |
//! | Z2 the destination is a core constant | [`stage_destination_roots`] |
//! | Z3 a foreign process on the channel | [`check_peer`], [`parse_pkexec_uid`] |
//! | Z5 no raw pkexec after a refusal | [`raw_fallback_allowed`] |
//! | Z6 vital packages and the current desktop | [`check_not_vital`], [`check_remove_packages`], [`desktop_removal_set`] |
//! | Z7 code running as root from a theme | [`check_plymouth_module_name`], [`check_theme_file_not_code`] |
//! | Z8 bypassing someone else's polkit (PLM) | [`check_plm_key`], [`check_plm_wallpaper_name`] |
//! | install only from tables | [`check_install`], [`check_build_deps`], [`check_build_only`] |
//! | §1.4 item 5 immutable system | [`check_mutable_system`] |
//! | desktop as a set: desktop table, DKMS by the core rule, replacement from the replacement table | [`check_de_set`], [`check_de_set_preview`] |
//! | SteamOS desktop image | [`check_sysext`] |
//! | removing an orphan of the update guard | [`check_remove_packages`] + machine probe in `helper.rs` ([`Refusal::NotOrphan`]) |
//! | rolling back an update guard set from the pacman cache | [`check_cache_package`] + cache files (`openat2`, owner root) and machine check in `helper.rs` ([`Refusal::NotRollback`]) |
//! | fonts "for all users": font file name, only the core directory, no overwriting | [`check_font_install`], [`check_font_remove`] + `fc-query` on the root copy and destination probe in `helper.rs` ([`Refusal::NotAFont`], [`Refusal::FontExists`]) |
//! | SDDM login screen: theme background, «Show clock», carrying over the desktop look | [`check_sddm_look`] (+ theme name: [`check_item_name`]) + in `helper.rs`: the theme in the system directory with a settings file ([`Refusal::SddmTheme`]), PNG/JPEG (WebP with the WebP module in the theme greeter's Qt) by the signature of the root copy ([`Refusal::LoginImage`]), files in the `sddm` home as that user |
//!
//! Z4 (privileges living on after the window) is not an argument check, it is
//! in [`super::revoke`]. File system checks (`openat2`, links, `.plymouth`
//! contents) are in [`super::unpack`].

use std::collections::HashSet;

use crate::distro_compat::SteamOsBlock;
use crate::repo_rebuild::{self, SrcInfo};
use crate::system::{self, PkgFamily};
use crate::system_plan as plan;

/// The longest name (of a package, desktop, theme) the helper accepts.
/// No table has longer names; a long string is a sign of a non-name.
pub const MAX_NAME_LEN: usize = 128;

/// How many packages the helper accepts in one operation. The longest desktop
/// in the database is a few dozen packages; the margin is for groups and the AUR.
pub const MAX_PACKAGES: usize = 512;

/// The longest path the helper accepts (`PATH_MAX`).
pub const MAX_PATH_LEN: usize = 4096;

/// Why the helper refused. A refusal is not a failure: the operation did not
/// start, the system is untouched, and there will be no raw `pkexec` instead of it (Z5).
///
/// There is no text for a person here on purpose: the strings will come with
/// the refusal dialog (item B13), and the journal is fine with [`Refusal::code`] and the data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// An empty argument or an empty list where at least something is needed.
    Empty,
    /// An argument longer than [`MAX_NAME_LEN`] / [`MAX_PATH_LEN`].
    TooLong(String),
    /// More than [`MAX_PACKAGES`] names in one operation.
    TooMany(usize),
    /// Z1: a flag (`-oX`, `--config=…`), a space, a line break or a shell
    /// metacharacter in an argument.
    Operand(String),
    /// Z2: not a package name (a path, a package file, a foreign `@`).
    PackageName(String),
    /// Z2: not a theme, directory or category name.
    ItemName(String),
    /// A family on which the helper does not manage packages.
    Family(&'static str),
    /// The desktop is not in the Rexon database.
    UnknownDesktop(String),
    /// Install: the name is not in the table this operation goes by.
    NotInTable(String),
    /// Rebuild: the string is not from the recipe or not a dependency.
    BuildDep(String),
    /// Z6: a vital package.
    Vital(String),
    /// Z6: a package of the current desktop.
    CurrentDesktopPackage(String),
    /// Z6: removing the current desktop itself (the core's text:
    /// [`plan::remove_de_decision`]).
    CurrentDesktop(String),
    /// Z6: the current desktop is not determined: nothing to protect with, removal is not allowed.
    CurrentDesktopUnknown,
    /// Z2: the path is not absolute, has `.`/`..`/`//` or control characters.
    Path(String),
    /// Z2: the path is outside the allowed user root.
    OutsideRoot(String),
    /// The built file is not named like a makepkg file.
    PackageFile(String),
    /// The login screen is not from the helper table.
    DisplayManager(String),
    /// Z7: the theme carries its own code (`.so`, a foreign `ModuleName`).
    BootSplashCode(String),
    /// Z3: the wrong process on the channel.
    Peer,
    /// Z8: a PLM settings key outside the allowlist.
    PlmKey(String),
    /// §1.4 item 5: the system partition is read-only (SteamOS). The text is
    /// the core's ([`SteamOsBlock::reason`]).
    ReadOnlySystem(String),
    /// Desktop set: a replacement of a conflicting package not from the core's
    /// replacement table, or a conflict the consent does not cover (`--ask=4` would remove too much).
    Conflict(String),
    /// The desktop set does not match the core rule by the pacman answers of this
    /// machine (a member that does not install, a DKMS package not by the rule, the probe is silent).
    SetRule(String),
    /// A desktop image (sysext) is SteamOS only.
    NotSteamOs,
    /// Removing an orphan: the package is not an orphan on this machine (not
    /// installed, present in the repositories, or not a member of an update guard set).
    NotOrphan(String),
    /// Rollback from the cache: not exactly a rollback of the finding's set on this
    /// machine (the package is not from the set, the version is not lower than the
    /// installed one, the set is incomplete or exceeded).
    NotRollback(String),
    /// Fonts: the name is not a font file name (a path, a hidden file, an extension
    /// not from [`crate::qt_settings::FONT_INSTALL_EXTS`]), or the removal path is
    /// not a font file inside [`crate::qt_settings::SHARED_FONT_DIR`].
    FontFile(String),
    /// Fonts: fontconfig (`fc-query` on the root copy) does not consider the file
    /// a font.
    NotAFont(String),
    /// Fonts: the directory for all users already has a file (or a link) with
    /// this name; what is not ours is not overwritten.
    FontExists(String),
    /// SDDM login screen: there is no `sddm` account (or its home directory).
    NoSddmUser,
    /// SDDM login screen: the theme is not in the system directory, or it does not
    /// accept its own background (no settings file).
    SddmTheme(String),
    /// SDDM login screen: the background picture is neither PNG nor JPEG (nor WebP
    /// with the WebP module in the greeter's Qt) by its signature.
    LoginImage(String),
    /// Look carry-over: a file name not from [`plan::SDDM_LOOK_FILES`], a repeat,
    /// files with «Reset», or contents that are not text.
    LookFile(String),
    /// Look carry-over: a cursor, font or DPI value outside the rules.
    LookValue(String),
    /// Look carry-over: the value is overridden in a file Rexon does not
    /// edit.
    LookElsewhere,
    /// Look carry-over: nothing to write with as `sddm` (`setpriv`).
    NoSetpriv,
}

impl Refusal {
    /// Short name for the journal (not translated).
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
// Z1: neither a flag nor a shell
// ============================================================================

/// Characters that are in no name of the core tables and that mean something
/// to a shell or to argument parsing.
fn is_shell_meta(c: char) -> bool {
    c.is_whitespace()
        || c.is_control()
        || matches!(
            c,
            ';' | '|' | '&' | '$' | '`' | '\'' | '"' | '\\' | '<' | '>' | '(' | ')' | '{'
                | '}' | '[' | ']' | '*' | '?' | '~' | '!' | '#' | '%' | '^' | ','
        )
}

/// Z1: a string from the window that will become an argv word. Non-empty, no
/// longer than [`MAX_NAME_LEN`], not a flag (a leading `-`: `-oX`, `--config=…`,
/// `--root`) and without shell metacharacters and spaces. This is the FIRST
/// line of any name check; the second is the rule of the name kind itself.
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

/// For the journal: a long string from the window is not carried whole.
fn clip(s: &str) -> String {
    s.chars().take(MAX_NAME_LEN).collect()
}

// ============================================================================
// Z2: a name, not a path
// ============================================================================

/// A family on which the helper manages packages
/// ([`plan::PKG_HELPER_FAMILIES`]; Nix and unknown ones do not count).
pub fn check_family(family: PkgFamily) -> Check {
    if plan::PKG_HELPER_FAMILIES.contains(&family.as_str()) {
        Ok(())
    } else {
        Err(Refusal::Family(family.as_str()))
    }
}

/// Z2: a package name. [`check_operand`] + the core rule
/// [`plan::is_safe_pkg_arg`], the same as in `rexon-pkg-helper`: `/` only in
/// a Gentoo atom, `@` only for a dnf group, a package file (`x.deb`) is
/// refused even without `/`.
pub fn check_package_name(family: PkgFamily, name: &str) -> Check {
    check_operand(name)?;
    if plan::is_safe_pkg_arg(family, name) {
        Ok(())
    } else {
        Err(Refusal::PackageName(clip(name)))
    }
}

/// Z2: the list of names of one operation: non-empty, no longer than
/// [`MAX_PACKAGES`], each by [`check_package_name`].
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

/// Z2: a theme, staging directory or background name: the rule of
/// `rexon-system-helper` and `rexon-stage-helper` (and [`plan::privileged_stage_cmd`]):
/// `[A-Za-z0-9._-]+`, without a leading `-` or `.`, without `..`.
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

/// Z2: a desktop only from the core database ([`system::DE_DATABASE`]). This is
/// stricter than the character check of `rexon-de-helper`: an unknown name
/// does not pass even if it is "clean".
pub fn check_de_id(de_id: &str) -> Check {
    check_operand(de_id)?;
    if system::DE_DATABASE.iter().any(|(id, _)| *id == de_id) {
        Ok(())
    } else {
        Err(Refusal::UnknownDesktop(clip(de_id)))
    }
}

/// A file staging category from [`plan::STAGE_CATEGORIES`].
pub fn check_stage_category(category: &str) -> Check {
    if plan::STAGE_CATEGORIES.contains(&category) {
        Ok(())
    } else {
        Err(Refusal::ItemName(clip(category)))
    }
}

/// Z2: the staging destination is a core constant by category
/// ([`plan::STAGE_DIR_ROOT`] and the second webkit root
/// [`plan::WEB_GREETER_ROOT`]). Empty means the category's destination is a
/// single background file, whose path the helper also knows itself. There is
/// no path from the window at all.
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

/// Kind of a system theme (`rexon-system-helper`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemThemeKind {
    Sddm,
    Plymouth,
}

/// What to do with a system theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemThemeMode {
    Install,
    Apply,
    Remove,
}

/// A `rexon-system-helper` category (`sddm`, `apply-plymouth`,
/// `remove-sddm`, …): the same closed list as the helper's `case`.
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

/// Login screen for `enable-dm`, from [`plan::DE_HELPER_DISPLAY_MANAGERS`].
pub fn check_enable_dm_unit(unit: &str) -> Check {
    if plan::DE_HELPER_DISPLAY_MANAGERS.contains(&unit) {
        Ok(())
    } else {
        Err(Refusal::DisplayManager(clip(unit)))
    }
}

/// Login screen for `switch-dm`, from [`plan::dm_switch_units`].
pub fn check_switch_dm_unit(unit: &str) -> Check {
    if plan::dm_switch_units().contains(&unit) {
        Ok(())
    } else {
        Err(Refusal::DisplayManager(clip(unit)))
    }
}

// ============================================================================
// Z2: a path only inside the user root (without the FS)
// ============================================================================

/// A lexically normal absolute path: starts with `/`, without empty components
/// (`//`, a trailing `/`), without `.` and `..`, without control characters,
/// no longer than [`MAX_PATH_LEN`]. Links cannot be checked this way; that is
/// the job of [`super::unpack`] (`openat2` without links); here is what can be
/// seen from the string.
fn is_normal_abs_path(p: &str) -> bool {
    p.len() > 1
        && p.len() <= MAX_PATH_LEN
        && p.starts_with('/')
        && !p.chars().any(|c| c.is_control())
        && p[1..].split('/').all(|seg| !seg.is_empty() && seg != "." && seg != "..")
}

/// Z2: a path from the window strictly INSIDE `root` (`root` itself does not do).
/// `root` is given by the root side, not the window: from the home directory
/// by `PKEXEC_UID` from passwd ([`user_rebuild_root`]). A foreign absolute path
/// (`/tmp/x.deb`, `/etc/…`, `/var/cache/pacman/pkg/…`), `..` and a
/// "neighbouring" root (`/home/a/.cache/rexon/rebuild2/…`) are refused.
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
    // A wrong root is an error of the root side, but even then the answer is "no".
    if !is_normal_abs_path(root) {
        return Err(Refusal::OutsideRoot(clip(path)));
    }
    match path.strip_prefix(root).and_then(|rest| rest.strip_prefix('/')) {
        Some(rest) if !rest.is_empty() => Ok(rest),
        _ => Err(Refusal::OutsideRoot(clip(path))),
    }
}

// ============================================================================
// Fonts "for all users"
// ============================================================================

/// How many font files the helper accepts in one operation (a family with
/// styles is dozens of files; the margin is for collections).
pub const MAX_FONT_FILES: usize = 256;

/// Limit of one font file. The heaviest are CJK collections (`.ttc`) of
/// dozens of megabytes.
pub const MAX_FONT_BYTES: u64 = 256 * 1024 * 1024;

/// The longest file name (`NAME_MAX`, bytes).
const MAX_FILE_NAME_BYTES: usize = 255;

/// The font file name under which it goes into the directory for all: one name
/// (without `/`), not hidden and not looking like an option (`.`, `-` at the
/// start), without `..` and control characters, with an extension from
/// [`crate::qt_settings::FONT_INSTALL_EXTS`] (case-insensitive).
/// Spaces and non-Latin characters are allowed: the name goes neither into a
/// shell nor into argv; the helper only creates a file with it.
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
        // A bare extension without a name (`.ttf`) is cut off above; `ttf` without a
        // dot is not a font file.
        && name.contains('.');
    if ok {
        Ok(())
    } else {
        Err(Refusal::FontFile(clip(name)))
    }
}

/// Installing fonts for all: the name list is non-empty, no longer than
/// [`MAX_FONT_FILES`], each is [`check_font_file_name`], without repeats.
/// The destination directory is a core constant
/// ([`crate::qt_settings::SHARED_FONT_DIR`]): only names come from the window.
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

/// Removing fonts for all: each path is a normal absolute path strictly inside
/// [`crate::qt_settings::SHARED_FONT_DIR`] (subdirectories are allowed: an
/// administrator lays them out like that too), the last segment is a font file
/// name ([`check_font_file_name`]), each intermediate one is not hidden;
/// without repeats, no more than [`MAX_FONT_FILES`]. Package fonts
/// (`/usr/share/fonts`), the whole directory, foreign paths are refused. That
/// it is a regular file and not a link, the helper checks on the system itself.
/// Returns paths relative to the directory.
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

/// The user's rebuild root: `<home>/.cache/rexon/rebuild`, as in
/// `rexon-pkg-helper` (the home directory comes from passwd by `PKEXEC_UID`,
/// not from the window environment, so `XDG_CACHE_HOME` plays no part here).
pub fn user_rebuild_root(home: &str) -> String {
    repo_rebuild::rebuild_root(&format!("{}/.cache/rexon", home.trim_end_matches('/')))
}

/// makepkg file suffixes (`PKGEXT`): the same as in `install-built` of
/// `rexon-pkg-helper`.
const MAKEPKG_EXT: &[&str] = &[
    ".pkg.tar", ".pkg.tar.zst", ".pkg.tar.xz", ".pkg.tar.gz", ".pkg.tar.bz2",
    ".pkg.tar.lz4", ".pkg.tar.lzo", ".pkg.tar.lrz", ".pkg.tar.lz", ".pkg.tar.Z",
];

/// Z2: a built file for `pacman -U` only from the user's rebuild directory,
/// and not in the root itself but in a recipe subdirectory
/// (`<rebuild_root>/<recipe>/<file>`), the name is a makepkg file name. This
/// way `pacman -U` gets neither `/tmp/x.deb`, nor an old version from the
/// pacman cache, nor a system file. The owner, links and "is it a regular
/// file" are checked by the fd intake ([`super::unpack`]).
pub fn check_built_package<'a>(path: &'a str, rebuild_root: &str) -> Check<&'a str> {
    let rel = check_user_path(path, rebuild_root)?;
    let Some((_, name)) = rel.rsplit_once('/') else {
        return Err(Refusal::OutsideRoot(clip(path)));
    };
    check_package_file_name(name)
}

/// A file built by an AUR helper for `pacman -U` (`PrivOp::AurInstallBuilt`,
/// item B10): an absolute path without `.`/`..`, the name is a makepkg file
/// name. The location is not checked: the build directory is set by paru/yay
/// and the person's `PKGDEST`. What gets installed is decided by the `.PKGINFO`
/// of the root copy (the name is one of those the window asked to build); root
/// does not open the path.
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

/// A pacman cache file for a rollback (`PrivOp::CacheRollback`): a normal
/// absolute path not in the root, the name is a makepkg file name. → `(directory, name)`.
/// Whether the directory belongs to the machine's cache (`pacman-conf CacheDir`)
/// and whose file it is, the helper decides at execution: it opens it itself, without links.
pub fn check_cache_package(path: &str) -> Check<(&str, &str)> {
    let name = check_aur_package(path)?;
    match path.rsplit_once('/') {
        Some((dir, _)) if !dir.is_empty() => Ok((dir, name)),
        _ => Err(Refusal::Path(clip(path))),
    }
}

/// A makepkg file name: `<name>-<version>-<release>-<arch>` + a [`MAKEPKG_EXT`] suffix.
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
// Z3: only the window that raised the helper is on the channel
// ============================================================================

/// `PKEXEC_UID`: decimal digits only (`rexon-pkg-helper` requires the same).
/// `None` means the helper was not started through pkexec.
pub fn parse_pkexec_uid(s: &str) -> Option<u32> {
    if s.is_empty() || s.len() > 10 || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

/// Z3: the peer's `SO_PEERCRED` against whoever raised the helper: the UID is
/// `PKEXEC_UID`, the pid is the helper's parent (`getppid()`; pkexec replaces
/// itself with the helper through exec, the parent is the window's thread).
pub fn check_peer(peer_uid: u32, peer_pid: i32, pkexec_uid: u32, parent_pid: i32) -> Check {
    if peer_pid > 1 && peer_uid == pkexec_uid && peer_pid == parent_pid {
        Ok(())
    } else {
        Err(Refusal::Peer)
    }
}

// ============================================================================
// Z5: a helper refusal is not bypassed with a raw pkexec
// ============================================================================

/// Why the operation did not go through the helper.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelperFailure {
    /// There is no helper file.
    Missing,
    /// The helper exists, but the owner or permissions are wrong (it was replaced).
    BadOwner,
    /// The helper checked the argument and refused ([`Refusal`]).
    Refused,
    /// The session broke off or a frame is corrupted.
    SessionLost,
    /// The person did not confirm the password.
    NotAuthorized,
}

/// Z5: whether the "as now" path may be taken (a raw `pkexec` from the same
/// `PrivOp`). Only if there is no helper at all or it is not acceptable. After
/// a check refusal, a session break and a password refusal, never (lesson of
/// 78bcc84: the reference enshrined a bypass with a raw `pkexec pacman`).
pub fn raw_fallback_allowed(failure: HelperFailure) -> bool {
    matches!(failure, HelperFailure::Missing | HelperFailure::BadOwner)
}

// ============================================================================
// Z6: vital packages and the current desktop
// ============================================================================

/// A vital package by the core rule [`plan::is_vital_package`] (the exact names
/// of [`plan::PKG_HELPER_VITAL`] and the families of kernels, drivers, firmware
/// and boot); the same pattern is built into `rexon-pkg-helper`.
pub fn is_vital(name: &str) -> bool {
    plan::is_vital_package(name)
}

/// Z6: vital packages are never removed.
pub fn check_not_vital(name: &str) -> Check {
    if is_vital(name) {
        Err(Refusal::Vital(clip(name)))
    } else {
        Ok(())
    }
}

/// Desktop packages by the core table: [`system::desktop_packages`].
pub fn desktop_packages(family: PkgFamily, de_id: &str) -> Vec<String> {
    system::desktop_packages(de_id, family)
}

/// The current desktop is known to the database: otherwise there is nothing to protect with.
fn known_current_de(current_de: &str) -> Check<&str> {
    let id = current_de.trim();
    if id.is_empty() || id == "unknown" || check_de_id(id).is_err() {
        Err(Refusal::CurrentDesktopUnknown)
    } else {
        Ok(id)
    }
}

/// Z6: removal BY NAMES (`remove`, removing build packages): each name is a
/// name, not vital and not among the current desktop's packages
/// ([`desktop_packages`] for `current_de`; `lightdm` and `sddm`, shared by two
/// desktops, are protected too). The current desktop is not determined: refusal.
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

/// Z6: removing a whole DESKTOP: the root side builds the package set itself,
/// only `de_id` comes from the window.
///
/// 1. The core protection ([`plan::remove_de_decision`], `force = false`): the
///    current desktop itself is refused with the core's text.
/// 2. Vital packages and the current desktop's packages drop out of the set
///    (a shared login screen: `lightdm` for Cinnamon and Xfce).
/// 3. The core closure ([`system::filter_removable_transitive`]): a package
///    needed by something outside the set, and everything it needs, stays.
///    `qi` is the output of `pacman -Qi <package>` (not called on other families).
///
/// `Ok` is what to remove; it may be empty (everything turned out to be shared).
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
    // The current desktop protection is shared with the window's `remove_de`
    // ([`system::without_current_desktop`]); the root side determines the login
    // screen itself (helper "B", item B8), so it is not here.
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
// Install: only from the core tables
// ============================================================================

/// The yt-dlp package name is a core constant.
const YTDLP_PACKAGE: &str = crate::youtube_wallpaper::YTDLP_PACKAGE;

/// Which table the install goes by. The window names the table, the root side
/// knows its contents itself: a name outside the table does not pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallSource<'a> {
    /// A desktop from the database ([`system::DE_DATABASE`]): main and AUR packages.
    Desktop { de_id: &'a str },
    /// A login screen ([`plan::DM_PAIRS`]): the "foolproofing" of no DM → SDDM.
    DisplayManager,
    /// «Check My Desktop»: packages of the CURRENT desktop and the tables of
    /// [`crate::compliance`] (Ayatana, MATE).
    DesktopRepair { current_de: &'a str },
    /// Video dependencies of wallpapers
    /// ([`crate::wallpaper_engine::install_deps_packages`], both sessions).
    VideoDeps,
    /// yt-dlp for YouTube wallpapers.
    YtDlp,
    /// The Gentoo overlay tool ([`plan::GENTOO_OVERLAY_TOOL_PACKAGE`]).
    GentooTool,
    /// Desktop shells ([`crate::shell_bundle::all_packages`]).
    Shell,
    /// Conky widget programs and the 7z unpacker
    /// ([`crate::utils::tool_packages_for`]).
    Tools,
    /// Conky ([`crate::de_apply::conky_package`]).
    Conky,
    /// MATE layout parts ([`crate::mate_panel::all_part_packages`]).
    MatePanel,
    /// Theme icons and cursors ([`crate::themes::known_set_packages`]).
    ThemeSets,
    /// «Distro Looks» looks
    /// ([`crate::distro_looks::builtin_package_names`]).
    DistroLook,
}

/// The allowlist of one table. `Err` means there is no table (unknown desktop).
pub fn install_allowlist(family: PkgFamily, source: InstallSource<'_>) -> Check<Vec<String>> {
    let owned = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    Ok(match source {
        InstallSource::Desktop { de_id } => {
            check_de_id(de_id)?;
            desktop_packages(family, de_id)
        }
        InstallSource::DisplayManager => {
            let mut all: Vec<String> = plan::DM_PAIRS.iter().map(|(pkg, _)| pkg.to_string()).collect();
            // "No login screen → SDDM": the package name by family (Gentoo with a category).
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

/// Install: each name is a name ([`check_package_list`]) and is in the table
/// `source`.
pub fn check_install(family: PkgFamily, names: &[String], source: InstallSource<'_>) -> Check {
    check_package_list(family, names)?;
    let allowed: HashSet<String> = install_allowlist(family, source)?.into_iter().collect();
    match names.iter().find(|n| !allowed.contains(*n)) {
        Some(n) => Err(Refusal::NotInTable(clip(n))),
        None => Ok(()),
    }
}

/// Split a dependency string into the name and the version constraint
/// (`cmake>=3.20` → `cmake`, `>=3.20`), like `dep_ok` in `rexon-pkg-helper`.
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

/// Rebuild, `build-deps`: Arch only; each string is a name (the core rule)
/// with possibly a version constraint, and is in the recipe's
/// [`repo_rebuild::build_requirements`] (`depends` + `makedepends` +
/// `base-devel`). The root side parses the recipe itself from what it got
/// over the fd, not from the window's words.
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
        // `>`/`<` are legitimate in a version constraint, so Z1 is applied to the
        // name (inside check_package_name), the constraint gets its own rule.
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

/// Rebuild, the `install-built` tail: what to remove after the install is only
/// what is needed ONLY for building (`makedepends` or `base-devel`, but not
/// `depends`, the rule [`repo_rebuild::build_only`]), and never anything
/// vital. Empty is allowed.
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

/// Rolling back heavy Plasma forks: only forks are removed
/// ([`plan::filter_plasma_forks`]), only the stock package the core derives
/// itself from the fork metadata is installed ([`plan::fork_stock_candidates`];
/// `qi` is `pacman -Qi`).
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
// Z7: code running as root from a theme
// ============================================================================

/// Z7: the `ModuleName=` of a Plymouth theme is only a stock module
/// ([`crate::installer::PLYMOUTH_SPLASH_MODULES`]). Plymouth builds the path
/// `<module directory>/<name>.so` without checking `../`, and a foreign name
/// means a `.so` from the theme itself running as root at boot.
pub fn check_plymouth_module_name(value: &str) -> Check {
    if crate::installer::PLYMOUTH_SPLASH_MODULES.contains(&value.trim()) {
        Ok(())
    } else {
        Err(Refusal::BootSplashCode(clip(value)))
    }
}

/// Z7: a file inside a theme is not a library (`x.so`, `x.so.1`): the same as in
/// [`crate::installer::system_theme_problem`]. `rel` is the path inside the theme.
pub fn check_theme_file_not_code(rel: &str) -> Check {
    let base = rel.rsplit('/').next().unwrap_or(rel);
    if base.ends_with(".so") || base.contains(".so.") {
        Err(Refusal::BootSplashCode(clip(rel)))
    } else {
        Ok(())
    }
}

// ============================================================================
// Z8: PLM: the helper does not retell someone else's config
// ============================================================================

/// Z8: a `plasmalogin.conf` key the helper may change (plan §3.3):
/// `[Greeter] ShowClock`, `WallpaperPluginId` and
/// `[Greeter][Wallpaper][<plugin>] Image/Color/FillMode`, where the plugin is
/// from the PLM list (`plugins` is given by the root side). `[X11] ServerPath`,
/// `[Autologin] …`, `[Users] DefaultPath` and everything else are refused.
/// `groups` is the key's group path (`["Greeter", "Wallpaper", "org.kde.image"]`).
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

/// Z8: the name of the background file the helper puts into
/// `/var/lib/plasmalogin/wallpapers/`: a name, not a path.
pub fn check_plm_wallpaper_name(name: &str) -> Check {
    check_item_name(name)
}

// ============================================================================
// SDDM login screen: carrying over the desktop look
// ============================================================================

/// A string from the window that becomes a value in the SDDM settings (read by
/// the root daemon): non-empty, no longer than `max`, without control
/// characters (a line break would start a new group or key).
fn check_look_text(v: &str, max: usize) -> Check {
    if v.trim().is_empty() || v.len() > max || v.chars().any(|c| c.is_control()) {
        Err(Refusal::LookValue(clip(v)))
    } else {
        Ok(())
    }
}

/// Carrying the look over to the SDDM login screen ([`super::op::PrivOp::SddmLook`]):
/// * the cursor theme is a directory name (spaces are allowed, `/`, `\` and a
///   leading `.`/`-` are not), the size is 8…512, the font is a QFont string of
///   up to 256 characters, DPI is 48…480: this way only `-dpi <number>`,
///   assembled by the helper, goes into `ServerArguments`;
/// * the files are names from [`plan::SDDM_LOOK_FILES`] without repeats;
/// * «Reset» (`look: None`) comes without files.
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
// §1.4 item 5: immutable system
// ============================================================================

/// On SteamOS the system partition is read-only: the helper does not touch
/// packages, the login screen or the boot splash. The rule and the text are
/// the core's ([`SteamOsBlock`]); the root side finds out `steamos` itself.
pub fn check_mutable_system(block: SteamOsBlock, steamos: bool) -> Check {
    match block.on(steamos) {
        Some(reason) => Err(Refusal::ReadOnlySystem(reason)),
        None => Ok(()),
    }
}

// ============================================================================
// Desktop as a set (DKMS `--asdeps`, replacing a conflicting package, Deepin) and SteamOS
// ============================================================================

/// What a desktop set never contains: `deepin-kwin` (the Deepin X11 window
/// manager, [`crate::deepin_treeland::forbid_x11_wm`]) in no set at all; for
/// Deepin also the recipe-built `deepin-session` (it comes from our own build)
/// and the whole group.
fn forbidden_in_set(de_id: &str, name: &str) -> bool {
    use crate::deepin_treeland as dt;
    name == dt::X11_WM_PACKAGE || (de_id == dt::DE_ID && (name == dt::SESSION_PACKAGE || name == dt::GROUP))
}

/// A desktop as a set outside the DE helper table (`PrivOp::DeInstallSet`):
/// what is visible without asking the system:
/// * pacman only (DKMS modules, `--asdeps` and `--ask=4` are its things);
/// * a desktop from the database, names are names, without `deepin-kwin` (for
///   Deepin also without the recipe-built `deepin-session` and the whole group);
/// * `deps_only` is from the set itself;
/// * `replace` (removed by the same transaction) is not vital, not from the
///   set and not a package of the current desktop; the current desktop must
///   be known and must not be the one being installed (no replacement under a
///   running session).
///
/// Names outside the desktop table (members of its groups, a DKMS module,
/// kernel headers) and the replacement itself are checked against the core
/// rule by the pacman answers, [`check_de_set_preview`], before execution.
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

/// The core rule for a set by the probe the root side made ITSELF: `base` are
/// the set names from the desktop table and the members of its groups (by
/// `pacman -Sg` of this machine), `preview` is [`plan::install_preview`] over
/// `base` (groups, DKMS modules and headers for the installed kernels:
/// [`plan::kernel_module_extras`], conflicts with what is installed). `None`
/// means the probe did not answer: refusal, nothing unchecked may be installed.
///
/// * the set does not install (`blocked`, including `deepin-kwin` anywhere in
///   the plan): refusal;
/// * a set name outside `base` is only what the core rule itself added to the
///   probe (the DKMS variant, headers of the installed kernel);
/// * `deps_only` is only what the core rule considers a dependency;
/// * `replace` is exactly the replacements from the core table
///   ([`plan::conflict_swaps`]) covering ALL conflicts of the probe: `--ask=4`
///   answers "yes" to any conflict, so a foreign conflict with a replacement
///   means refusal; without conflicts there is no replacement.
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

/// A desktop image on SteamOS (`PrivOp::Sysext`): the same as
/// `rexon-sysext-helper` allows today: only on SteamOS (the root side finds
/// out itself), a desktop from the table [`crate::steamos_sysext::DESKTOPS`].
/// The desktop the person is working in right now cannot be removed (the
/// window's `remove_plan` decision, repeated here).
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
// Tests: one refusal per row of the prohibition table
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

    /// `pacman -Qi`: nothing is needed by anyone.
    fn qi_none(_: &str) -> Option<String> {
        Some("Depends On      : None\nRequired By     : None\n".into())
    }

    /// Some desktop that has packages on Arch, and a second, different one.
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

    // ── Z1 ──────────────────────────────────────────────────────────────

    #[test]
    fn з1_флаги_и_оболочка_отказ() {
        for bad in [
            "-oX", "--config=/tmp/evil.conf", "--root=/", "-", "-dd", "a b", "a;b", "a|b",
            "$(id)", "`id`", "a\nb", "a\0b", "a&b", "x>y", "~", "",
        ] {
            assert!(check_operand(bad).is_err(), "{bad:?} прошёл");
        }
        // a flag passes neither as a package name nor as a theme name
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

    // ── Z2: names ───────────────────────────────────────────────────────

    #[test]
    fn з2_путь_вместо_имени_отказ() {
        let arch = PkgFamily::Arch;
        for bad in ["/tmp/x.deb", "x.deb", "../x", "./x", "a/b", "x.pkg.tar.zst", ".hidden", "@world"] {
            assert!(check_package_name(arch, bad).is_err(), "{bad} прошёл");
        }
        // Gentoo: exactly one '/', no sets
        assert!(check_package_name(PkgFamily::Gentoo, "x11-misc/lightdm").is_ok());
        assert!(check_package_name(PkgFamily::Gentoo, "@world").is_err());
        assert!(check_package_name(PkgFamily::Gentoo, "/usr/bin/x").is_err());
        // Fedora: a dnf group is the only legitimate '@'
        assert!(check_package_name(PkgFamily::Fedora, "@cinnamon-desktop-environment").is_ok());
        assert!(check_package_name(arch, "lightdm-slick-greeter").is_ok());
        // an empty and a too long list
        assert_eq!(check_package_list(arch, &[]), Err(Refusal::Empty));
        let many = vec!["a".to_string(); MAX_PACKAGES + 1];
        assert_eq!(check_package_list(arch, &many), Err(Refusal::TooMany(MAX_PACKAGES + 1)));
        // a family without a helper
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

    // ── Z2: paths ───────────────────────────────────────────────────────

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
        // a broken root is also "no"
        assert!(check_user_path("/x/y", "/x/../").is_err());
        assert!(check_user_path("/x/y", "").is_err());
    }

    #[test]
    fn з2_собранный_пакет_только_из_рецепта() {
        let root = user_rebuild_root("/home/a");
        let ok = "/home/a/.cache/rexon/rebuild/pantheon/switchboard-8.0.2-3-x86_64.pkg.tar.zst";
        assert_eq!(check_built_package(ok, &root), Ok("switchboard-8.0.2-3-x86_64.pkg.tar.zst"));
        // a file right in the root, not a makepkg package, hidden, a flag
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

    // ── Z3 ──────────────────────────────────────────────────────────────

    #[test]
    fn з3_чужой_процесс_на_канале_отказ() {
        assert_eq!(parse_pkexec_uid("1000"), Some(1000));
        for bad in ["", "-1", "1000 ", "1e3", "0x10", "99999999999"] {
            assert_eq!(parse_pkexec_uid(bad), None, "{bad:?}");
        }
        assert!(check_peer(1000, 4242, 1000, 4242).is_ok());
        assert_eq!(check_peer(1001, 4242, 1000, 4242), Err(Refusal::Peer)); // foreign UID
        assert_eq!(check_peer(1000, 4243, 1000, 4242), Err(Refusal::Peer)); // not the parent
        assert_eq!(check_peer(1000, 1, 1000, 1), Err(Refusal::Peer)); // orphaned: the parent is init
        assert_eq!(check_peer(0, 0, 0, 0), Err(Refusal::Peer));
    }

    // ── Z5 ──────────────────────────────────────────────────────────────

    #[test]
    fn з5_после_отказа_сырого_pkexec_нет() {
        assert!(raw_fallback_allowed(HelperFailure::Missing));
        assert!(raw_fallback_allowed(HelperFailure::BadOwner));
        assert!(!raw_fallback_allowed(HelperFailure::Refused));
        assert!(!raw_fallback_allowed(HelperFailure::SessionLost));
        assert!(!raw_fallback_allowed(HelperFailure::NotAuthorized));
    }

    // ── Z6 ──────────────────────────────────────────────────────────────

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

    /// The permanent list (and its addition) does not touch a single desktop
    /// package: otherwise removing a desktop would silently leave half the packages.
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
        // the current desktop itself: the core's text
        let r = desktop_removal_set(arch, cur, cur, &qi_none);
        assert!(matches!(r, Err(Refusal::CurrentDesktop(ref t)) if !t.is_empty()), "{r:?}");
        // a package of the current desktop by name
        let p = system::get_packages(cur, arch)[0].clone();
        assert!(matches!(
            check_remove_packages(arch, std::slice::from_ref(&p), cur),
            Err(Refusal::CurrentDesktopPackage(_))
        ));
        // the current desktop is unknown: nothing may be removed
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
        // another desktop is allowed, but without the packages shared with the current one
        let set = desktop_removal_set(arch, other, cur, &qi_none).unwrap();
        let cur_pkgs = desktop_packages(arch, cur);
        assert!(set.iter().all(|p| !cur_pkgs.contains(p)));
        assert!(!set.is_empty());
        // an unrelated package name is removed
        assert!(check_remove_packages(arch, &s(&["some-app"]), cur).is_ok());
    }

    /// A shared login screen: Cinnamon and Xfce on Arch both carry `lightdm`.
    /// While in one, removing the other does not touch `lightdm`, although
    /// `pacman -Qi` says nobody needs it.
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

    /// The core closure: a package needed by something outside the set stays, and
    /// with it everything it needs.
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

    // ── Install: only from tables ───────────────────────────────────────

    #[test]
    fn установка_только_из_таблиц() {
        let arch = PkgFamily::Arch;
        let (de, other) = two_desktops();
        let pkgs = system::get_packages(de, arch);
        assert!(check_install(arch, &pkgs, InstallSource::Desktop { de_id: de }).is_ok());
        // an unrelated name, a name of another desktop, a flag, a path
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
        // login screen
        assert!(check_install(arch, &s(&["sddm"]), InstallSource::DisplayManager).is_ok());
        assert!(check_install(arch, &s(&["xorg-server"]), InstallSource::DisplayManager).is_err());
        // desktop completion: its own desktop and the compliance tables, but nothing foreign
        let repair = InstallSource::DesktopRepair { current_de: de };
        assert!(check_install(arch, &s(&[crate::compliance::AYATANA_PACKAGES[0]]), repair).is_ok());
        assert!(check_install(arch, &pkgs[..1], repair).is_ok());
        assert!(check_install(arch, &s(&["openssh"]), repair).is_err());
        assert_eq!(
            check_install(arch, &s(&["a"]), InstallSource::DesktopRepair { current_de: "unknown" }),
            Err(Refusal::CurrentDesktopUnknown)
        );
        // live wallpapers and yt-dlp
        assert!(check_install(arch, &s(&["mpv", "mpvpaper"]), InstallSource::VideoDeps).is_ok());
        assert!(check_install(PkgFamily::Debian, &s(&["mpvpaper"]), InstallSource::VideoDeps).is_err());
        assert!(check_install(arch, &s(&["yt-dlp"]), InstallSource::YtDlp).is_ok());
        assert!(check_install(arch, &s(&["youtube-dl"]), InstallSource::YtDlp).is_err());
    }

    /// Every name of every install table passes the name check: otherwise the
    /// helper would refuse an honest operation.
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

    /// ⛔ Guard B9: the privilege session tables are exactly the install allowlist
    /// of the package helper ([`plan::pkg_helper_install_allowlist`]). A table
    /// the package helper installs from but the session does not know is an
    /// honest install the session would refuse without a bypass (Z5): Conky,
    /// MATE applets, shells, icons and cursors, «Distro Looks».
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
            // The other way round: everything the session lets through, the package
            // helper lets through too (names that fail its name check are installed by nobody).
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
        // remove after building: only makedepends/base-devel, not depends
        assert!(check_build_only(arch, &s(&["meson", "vala", "base-devel"]), &r).is_ok());
        assert!(check_build_only(arch, &[], &r).is_ok());
        assert!(check_build_only(arch, &s(&["evolution-data-server"]), &r).is_err());
        assert!(check_build_only(arch, &s(&["polkit"]), &r).is_err());
        assert!(check_build_only(arch, &s(&["linux"]), &r).is_err());
    }

    /// The recipe itself requires something vital in makedepends: it still cannot
    /// be removed.
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

    // ── Z7 ──────────────────────────────────────────────────────────────

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

    // ── Z8 ──────────────────────────────────────────────────────────────

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

    // ── §1.4 item 5 ─────────────────────────────────────────────────────

    #[test]
    fn неизменяемая_система_отказ() {
        for b in [SteamOsBlock::Desktops, SteamOsBlock::LoginScreen, SteamOsBlock::BootSplash, SteamOsBlock::SystemPart] {
            assert!(matches!(check_mutable_system(b, true), Err(Refusal::ReadOnlySystem(ref t)) if !t.is_empty()));
            assert!(check_mutable_system(b, false).is_ok());
        }
    }

    // ── desktop as a set and the SteamOS desktop image ─────────────────

    fn swap(new: &str, installed: &str) -> plan::PkgConflict {
        plan::PkgConflict { new_package: new.into(), installed: installed.into(), ..plan::PkgConflict::default() }
    }

    #[test]
    fn набор_стола_без_пробы() {
        let arch = PkgFamily::Arch;
        let river = s(&["river-classic", "waybar"]);
        assert!(check_de_set(arch, "river", &river, &[], &s(&["river"]), "kde").is_ok());
        // pacman only; the desktop from the database; names are names.
        assert!(matches!(check_de_set(PkgFamily::Debian, "river", &river, &[], &[], "kde"), Err(Refusal::Family(_))));
        assert!(matches!(check_de_set(arch, "evil", &river, &[], &[], "kde"), Err(Refusal::UnknownDesktop(_))));
        assert!(matches!(check_de_set(arch, "river", &s(&["-oX"]), &[], &[], "kde"), Err(Refusal::Operand(_))));
        assert_eq!(check_de_set(arch, "river", &[], &[], &[], "kde"), Err(Refusal::Empty));
        // deepin-kwin in no set; for Deepin without the recipe-built session and the group.
        assert!(matches!(check_de_set(arch, "river", &s(&["deepin-kwin"]), &[], &[], "kde"), Err(Refusal::NotInTable(_))));
        for bad in ["deepin-session", "deepin", "deepin-kwin"] {
            assert!(matches!(check_de_set(arch, "deepin", &s(&["treeland", bad]), &[], &[], "kde"), Err(Refusal::NotInTable(_))), "{bad}");
        }
        assert!(check_de_set(arch, "deepin", &s(&["treeland", "ddm"]), &[], &[], "kde").is_ok());
        // --asdeps only from the set itself.
        assert!(matches!(check_de_set(arch, "river", &river, &s(&["linux-headers"]), &[], "kde"), Err(Refusal::NotInTable(_))));
        // Replacement: a permanent package, a current desktop package, a name from the set: no.
        let kde_pkg = desktop_packages(arch, "kde").into_iter().find(|p| !is_vital(p)).unwrap();
        assert!(matches!(check_de_set(arch, "river", &river, &[], &s(&["glibc"]), "kde"), Err(Refusal::Vital(_))));
        assert!(matches!(check_de_set(arch, "river", &river, &[], &[kde_pkg], "kde"), Err(Refusal::CurrentDesktopPackage(_))));
        assert!(matches!(check_de_set(arch, "river", &river, &[], &s(&["waybar"]), "kde"), Err(Refusal::Conflict(_))));
        // Replacement under this same running desktop and with an unknown desktop: no.
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
        // The probe is silent: nothing unchecked is installed.
        assert_eq!(check_de_set_preview(&all, &base, &[], &[], None), Err(Refusal::SetRule("pacman".into())));
        // An unrelated name: not from the root side's probe.
        let mut evil = all.clone();
        evil.push("nvidia-dkms".into());
        assert_eq!(check_de_set_preview(&evil, &base, &[], &[], Some(&p)), Err(Refusal::NotInTable("nvidia-dkms".into())));
        // --asdeps only for what the core rule considers a dependency.
        assert!(matches!(check_de_set_preview(&all, &base, &s(&["treeland"]), &[], Some(&p)), Err(Refusal::SetRule(_))));
        // deepin-kwin anywhere in the plan: refusal.
        let mut kwin = p.clone();
        kwin.new_packages.push("deepin-kwin".into());
        assert!(matches!(check_de_set_preview(&all, &base, &[], &[], Some(&kwin)), Err(Refusal::SetRule(_))));
        // A replacement without a conflict: no.
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
        // Without consent the conflict is not removed.
        assert_eq!(check_de_set_preview(&pkgs, &pkgs, &[], &[], Some(&p)), Err(Refusal::Conflict("river".into())));
        // An extra name for replacement: no, even if the conflict is covered.
        assert_eq!(
            check_de_set_preview(&pkgs, &pkgs, &[], &s(&["river", "kwin"]), Some(&p)),
            Err(Refusal::Conflict("kwin".into()))
        );
        // A conflict not from the replacement table (deepin-kwin against kwin) is not
        // removed by anyone's "consent": --ask=4 would remove KDE Plasma.
        let foreign = plan::InstallPreview {
            targets: pkgs.clone(),
            conflicts: vec![swap("river-classic", "river"), swap("mutter49", "mutter")],
            ..plan::InstallPreview::default()
        };
        assert!(matches!(
            check_de_set_preview(&pkgs, &pkgs, &[], &s(&["river", "mutter"]), Some(&foreign)),
            Err(Refusal::Conflict(_))
        ));
        // Something installed that someone requires is not removed either.
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

    /// What the helper is now allowed to do with fonts: PUT a font file under its
    /// own name into `/usr/local/share/fonts` and DELETE a font file from there
    /// (including from a subdirectory). Nothing more: no other directory, no path
    /// instead of a name, no hidden files, no non-fonts by extension, no package
    /// fonts.
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
        // List: empty, too long, a repeat: refusal.
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
            // package fonts, neighbours, the directory itself
            "/usr/share/fonts/noto/NotoSans-Regular.ttf",
            "/usr/local/share/fonts2/a.ttf",
            "/usr/local/share/fonts",
            "/usr/local/share/fonts/",
            "/home/u/.local/share/fonts/a.ttf",
            // leaving the directory and garbage in the path
            "/usr/local/share/fonts/../../../etc/shadow",
            "/usr/local/share/fonts/./a.ttf",
            "/usr/local/share/fonts//a.ttf",
            "usr/local/share/fonts/a.ttf",
            "/usr/local/share/fonts/a.ttf\n",
            // not a font, and fontconfig service files
            "/usr/local/share/fonts/fonts.dir",
            "/usr/local/share/fonts/fonts.scale",
            "/usr/local/share/fonts/.uuid",
            "/usr/local/share/fonts/sub",
            "/usr/local/share/fonts/run.sh",
            // a hidden subdirectory
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
