//! Checking the AeroShell look against the installed Plasma — and the rebuild command.
//!
//! The `aeroshell-*` / `aerothemeplasma-*` packages from the AUR are not themes but forks: they
//! build their own C++ against the HEADERS of the Plasma that was installed at build
//! time. After `pacman -Syu` the official stack moves ahead, while the forks
//! stay built for the old version — and break silently: KWin
//! loses effects (the taskbar, the Start menu, button glow, blur),
//! the libplasma fork's symbols diverge and the lock screen stops
//! starting. Only rebuilding the packages fixes it.
//!
//! Here is the PURE decision "diverged or not", "what to rebuild with", the contents of the
//! rebuild plan, the order and texts of its steps and parsing the builder's
//! return code. Running pacman and the rebuild itself is up to the caller: in the Rust
//! window that is the core executor (`system_de_exec::DeOpsExec::aeroshell_survey` /
//! `rebuild_aeroshell`), in the classic window — `rexon/system.py`; here
//! there is only reading already installed files and parsing output. The package version and its file list are asked of the shared package engine
//! ([`crate::system_plan::installed_version_query`] and
//! [`crate::system_plan::package_files_query`]) — there are no copies of them here.
//!
//! # How the version a package was built for is determined
//!
//! 1. **KWin effect module** — inside the `.so` there is the string
//!    `org.kde.kwin.EffectPluginFactory<version>`. KWin itself embeds it at build time
//!    (the `KWIN_EFFECT_FACTORY` macro), and KWin looks for the same string when loading
//!    the module: if it does not match, the effect is silently dropped. So we check exactly
//!    the string the loader decides by — no false
//!    alarms, no missed breakage.
//! 2. **libplasma fork** — the library is installed under the name
//!    `libPlasma.so.<version>`, where the version is the one the fork was built for.
//!    The name is visible right in the package file list, no need to read the `.so` itself.
//!
//! Rejected approaches (checked on a live machine on 10.08.2026):
//! * **Package build dates.** kwin was reinstalled after the fork was built,
//!   but its version did not change (only the package release went up,
//!   `6.7.4-3.1`) — dates would give a false alarm.
//! * **Package version from `pacman -Qi`.** For forks it comes from their OWN
//!   repository: `aeroshell-kwin-components-git 6.7.0_61` is the version of the
//!   AeroShell project, not of Plasma, and cannot be compared with the system version.

use crate::i18n::{ntr, tr, trf};
use serde::Serialize;

/// The KWin effect factory identifier. Everything that follows it up to the first
/// non-digit-non-dot is the KWin version the module was built for.
const EFFECT_FACTORY_IID: &str = "org.kde.kwin.EffectPluginFactory";

/// Marker of the effect modules directory in a package file list. The neighboring
/// `.../effects/configs/` (effect settings pages) does not match —
/// it has its own identifier and is not about KWin compatibility.
const EFFECT_PLUGIN_DIR: &str = "/kwin/effects/plugins/";

/// The start of the libplasma fork library name: the version follows.
const PLASMA_LIB_PREFIX: &str = "libPlasma.so.";

/// AUR helpers that can rebuild a package by name, in order of
/// preference. paru first: only it builds in a clean environment
/// (`--chroot`), and it is the clean environment that protects against a repeat —
/// in a system build the package may pick up an already half-updated stack.
pub const REBUILD_HELPERS: &[&str] = &["paru", "yay"];

/// The setting key "check the look at startup". A missing key
/// means ENABLED: a disabled check means a silently broken desktop
/// after the next update, and that cannot be the default.
/// It can only be turned off deliberately, in the settings.
pub const CHECK_ON_START_SETTING: &str = "aeroshell_check_on_start";

/// Whether the startup check is enabled. `value` — what is stored in the settings
/// (None — no key).
pub fn check_on_start(value: Option<bool>) -> bool {
    value.unwrap_or(true)
}

/// Version from a pacman string: without the epoch (`1:`) and without the package release (`-3.1`).
/// `6.7.4-3.1` → `6.7.4`. The release must not be compared: it goes up when the
/// official package is rebuilt, and compatibility does not change with it.
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

/// KWin version from the identifier inside a built effect module.
///
/// We search the raw bytes of the `.so`: there is no point parsing ELF for one string —
/// the identifier sits in the data as a plain string, and a substring search does not
/// depend on bitness or section format.
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
        // An empty tail is KWin's own header, not a built module.
        if version.contains('.') {
            return Some(version);
        }
        from = start.max(from + pos + 1);
    }
    None
}

/// Substring search in bytes (without external dependencies).
fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Package file paths from `pacman -Ql <package>` output.
///
/// There is no parsing of our own here: the file list of an installed package is a question for
/// the shared package engine ([`crate::system_plan::package_file_paths`]), which also
/// knows that `dpkg -L` and `rpm -ql` answer in a different form. The family here
/// is always Arch: look packages live only in the AUR.
pub fn package_files(ql_output: &str) -> Vec<String> {
    crate::system_plan::package_file_paths(crate::system::PkgFamily::Arch, ql_output)
}

/// KWin effect modules among the package files.
pub fn effect_plugin_paths(files: &[String]) -> Vec<String> {
    files
        .iter()
        .filter(|p| p.contains(EFFECT_PLUGIN_DIR) && p.ends_with(".so"))
        .cloned()
        .collect()
}

/// The Plasma version the libplasma fork was built for — from the name
/// `libPlasma.so.<version>` in the package file list.
pub fn plasma_library_version(files: &[String]) -> Option<String> {
    for path in files {
        let name = path.rsplit('/').next().unwrap_or(path);
        let Some(rest) = name.strip_prefix(PLASMA_LIB_PREFIX) else {
            continue;
        };
        // Only a full version `6.7.4`: `libPlasma.so.7` is a link by
        // major number, it does not change from release to release.
        if rest.contains('.') && rest.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
            return Some(rest.to_string());
        }
    }
    None
}

// ── The package's Qt modules: plasmoids, QML modules, plugins ─────────────────
//
// Plasmoids (SevenStart, seventasks, systemtray, notifications, volume) and
// the set's QML modules have NO version identifier like KWin effects do, so
// they used to get "no marker" and stayed silent even when they no longer loaded.
// Two things break them, and both are visible without running Plasma:
//   1. **A library or symbol is not found** — `ldd -r` on the module (a soname
//      change, e.g. `libprocesscore.so.10 → .11`; a mismatch with
//      libplasma — "undefined symbol"; a module built for a Qt newer than the
//      installed one — "version `Qt_6.N' not found").
//   2. **A private Qt interface of a different minor version.** A module that
//      needs `Qt_6_PRIVATE_API` (currently notifications: AOT QML code) holds
//      only on the Qt minor version it was built for: the private
//      interface is not preserved between 6.11 and 6.12, while the version symbol is the same,
//      and `ldd` sees nothing. The module itself reveals its build version: every
//      file built with Qt references `qt_version_tag@Qt_6.N`, where N is
//      the Qt minor version at build time; that is what we compare with the installed qt6-base.

/// Marker of a Qt module in a package file list (plugins and QML modules).
const QT_MODULE_DIR: &str = "/qt6/";

/// The version symbol of the private Qt interface.
const QT_PRIVATE_API: &[u8] = b"Qt_6_PRIVATE_API";

/// The words used in a module's problems for "a private interface of another Qt" —
/// by them [`Report::at_risk_only`] (and the login guard, [`crate::session_guard`])
/// tells a risk from breakage that has already happened.
pub const QT_PRIVATE_PROBLEM: &str = "private API";

/// How many problems of one module to show in detail: beyond that are repeats.
const MODULE_PROBLEMS_SHOWN: usize = 3;

/// Qt modules among the package files: `.so` under `…/qt6/…`.
pub fn qt_module_paths(files: &[String]) -> Vec<String> {
    files
        .iter()
        .filter(|p| p.contains(QT_MODULE_DIR) && p.ends_with(".so"))
        .cloned()
        .collect()
}

/// The Qt minor version the module was built for: the highest of `Qt_6.N`.
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

/// Whether the module uses the private Qt interface.
pub fn uses_qt_private_api(data: &[u8]) -> bool {
    find_bytes(data, QT_PRIVATE_API).is_some()
}

/// Minor version from a Qt package version: `6.11.2-3` → 11.
pub fn qt_minor(version: &str) -> Option<u32> {
    let v = upstream_version(version);
    let mut parts = v.split('.');
    (parts.next()? == "6").then_some(())?;
    parts.next()?.parse().ok()
}

/// A module's problems from `ldd -r <module>` output (stdout and stderr together).
///
/// `siblings` — file names of the whole look set. A module that
/// requires a sibling from the set by bare name (as `smodglow.so` requires
/// `org.smod.smod.so`) finds it only when the sibling is already loaded by
/// the host — `ldd` does not know that and says "not found", followed by
/// "undefined symbol" for the sibling's symbols. This is not breakage: such lines
/// are skipped, and "undefined symbol" in such a module does not count at all.
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
            // "<module>: <library>: version `Qt_6.12' not found (required
            // by …)" — the "required by" tail repeats the module, it is not needed.
            problems.push(line.split(" (required by").next().unwrap_or(line).to_string());
        } else if let Some((_, rest)) = line.split_once("error while loading shared libraries: ") {
            // The library file exists, but the loader does not take it (truncated, wrong
            // ELF class): "<program>: error while loading shared
            // libraries: /usr/lib/libQt6Quick.so.6: file too short" — measured
            // in a SteamOS VM on 25.10.2026; ldd then says nothing about "not found".
            problems.push(rest.trim().to_string());
        }
    }
    if !sibling_missing {
        for line in output.lines() {
            if let Some(rest) = line.trim().strip_prefix("undefined symbol: ") {
                // "undefined symbol: _ZTI…, version Qt_6\t(…)" — without a comma.
                let symbol = rest.split(|c: char| c.is_whitespace() || c == ',').next().unwrap_or(rest);
                problems.push(format!("undefined symbol: {symbol}"));
            }
        }
    }
    problems
}

/// The package contains built code (and therefore needs rebuilding), not just
/// pictures and sounds.
pub fn is_compiled_package(files: &[String]) -> bool {
    files.iter().any(|p| p.ends_with(".so") || p.contains(".so."))
}

// ══════════════════════════════════════════════════════════════════════════
// "Lonely fork": the libplasma replacement stayed, but the shell itself is gone
// ══════════════════════════════════════════════════════════════════════════
//
// The libplasma fork REPLACES the system library (`provides=libplasma` without
// a version), and Plasma packages depend on `libplasma` also without a version. So
// when the next Plasma comes out, `pacman -Syu` updates plasma-workspace and kwin
// but leaves the fork as it was — and then the WHOLE desktop crashes (the lock screen,
// plasmashell), not just one theme. While the AeroThemePlasma shell itself is installed,
// it needs the fork, and the cure is a rebuild. Without the shell nobody needs the fork:
// rebuilding it means repairing what can simply be removed. The right advice is
// to remove the leftovers and bring back the stock libplasma (fork rollback already exists:
// `system_exec::SystemExec::remove_plasma_forks`).

/// The start of the libplasma fork name (`aeroshell-libplasma-git` and variants).
pub const LIBPLASMA_FORK_PREFIX: &str = "aeroshell-libplasma";

/// The start of the AeroThemePlasma shell package name itself (Wayland and X11:
/// `aerothemeplasma-desktop-git`, `aerothemeplasma-desktop-x11-git`) — the same
/// package by which the bundle is recognized in [`crate::aur_install::AEROTHEMEPLASMA`].
pub const SHELL_PACKAGE_PREFIX: &str = "aerothemeplasma-desktop";

/// The libplasma fork is installed, but the shell itself is not.
pub fn lonely_libplasma_fork(forks: &[String]) -> bool {
    forks.iter().any(|f| f.starts_with(LIBPLASMA_FORK_PREFIX))
        && !forks.iter().any(|f| f.starts_with(SHELL_PACKAGE_PREFIX))
}

/// Where the package build version was taken from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Marker {
    /// The identifier inside a KWin effect module.
    KwinEffect,
    /// The libplasma fork library name.
    PlasmaLibrary,
    /// The Qt minor version of a module with the private Qt interface.
    QtVersion,
    /// No marker — nothing to say about the package's compatibility.
    None,
}

/// What is known about one installed look package.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PackageVerdict {
    /// Package name.
    pub package: String,
    /// Where the build version was taken from.
    pub marker: Marker,
    /// The version the package was built for (None — no marker found).
    pub built_for: Option<String>,
    /// The version installed now (None — could not find out).
    pub installed: Option<String>,
    /// The package contains built code.
    pub compiled: bool,
    /// The versions diverged or the package's modules have problems — the package does not
    /// work right now.
    pub stale: bool,
    /// Problems of the package's Qt modules: "module: what is wrong" (empty — everything loads).
    pub problems: Vec<String>,
}

/// The check result to show to the person.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Report {
    /// Breakdown for each look package found.
    pub packages: Vec<PackageVerdict>,
    /// Names of packages whose versions diverged.
    pub stale: Vec<String>,
    /// What to rebuild (all built packages of the set, not only the
    /// broken ones: they are built against each other and would drift apart separately).
    pub rebuild: Vec<String>,
    /// Whether there is a divergence.
    pub needs_rebuild: bool,
    /// The KWin version installed now.
    pub kwin_version: Option<String>,
    /// The version of the official Plasma stack installed now.
    pub plasma_version: Option<String>,
    /// The libplasma fork was left without the shell ([`lonely_libplasma_fork`]).
    pub lonely_fork: bool,
    /// What to remove when bringing back stock Plasma (empty if the fork is not lonely):
    /// the libplasma fork itself and all other BUILT packages of the set — without
    /// the shell they are just as useless and break just the same. Icons and sounds
    /// stay: they replace nothing and break nothing.
    pub restore: Vec<String>,
    /// What the session journal said ([`crate::plugin_journal`]): all modules
    /// KWin and plasmashell did not load — both from the set and third-party.
    pub journal: Vec<crate::plugin_journal::Finding>,
}

impl Report {
    /// There is a divergence, but so far only a RISK: the only evidence is modules
    /// built against the private interface of the previous Qt minor version. They
    /// load (nothing is missing, the session journal is silent, the Plasma and
    /// KWin versions match), but may break at any moment. This state
    /// is described in different words ("needs rebuilding" rather than "stopped
    /// working") and at startup is shown once, not every time: people stop reading a window
    /// that keeps claiming breakage while the desktop works
    /// (acceptance on 10.10.2026).
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

    /// An empty result — "the AeroShell look is not installed".
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

    /// Journal findings for the set's packages.
    pub fn journal_in_set(&self) -> Vec<&crate::plugin_journal::Finding> {
        self.journal
            .iter()
            .filter(|f| f.package().is_some_and(|p| self.packages.iter().any(|v| v.package == p)))
            .collect()
    }

    /// Journal findings outside the set (third-party effects and widgets).
    pub fn journal_outside(&self) -> Vec<&crate::plugin_journal::Finding> {
        self.journal
            .iter()
            .filter(|f| !f.package().is_some_and(|p| self.packages.iter().any(|v| v.package == p)))
            .collect()
    }
}

/// Add what the session journal said to the version check.
///
/// The journal is an exact source: if a module of a set package is named
/// incompatible in the journal, the package is broken even if the versions in the files match
/// (and the versions are taken from the journal: KWin itself named them). The reverse is NOT true:
/// the journal may be empty (KWin debugging before 6.8 is off, the journal was rotated,
/// a session without systemd), so the old version check stays in force.
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

/// Check the installed look packages against the current Plasma.
///
/// `forks` — names of the installed fork packages (selected by
/// [`crate::system_plan::filter_plasma_forks`]).
/// `kwin_version` / `plasma_version` — versions from pacman, as is (the epoch and
/// the release are cut off here).
/// `file_list(pkg)` — the output of `pacman -Ql <pkg>` or None.
/// `read_file(path)` — the file contents or None.
pub fn check_with(
    forks: &[String],
    kwin_version: Option<&str>,
    plasma_version: Option<&str>,
    file_list: &dyn Fn(&str) -> Option<String>,
    read_file: &dyn Fn(&str) -> Option<Vec<u8>>,
) -> Report {
    check_modules_with(forks, kwin_version, plasma_version, None, file_list, read_file, &|_| None)
}

/// [`check_with`] together with the Qt module check (plasmoids, QML modules).
///
/// `qt_version` — the qt6-base version from pacman as is; `ldd(path)` — the output of
/// `ldd -r <path>` (stdout and stderr together) or None (then this step
/// is skipped: "could not find out" is not "broken").
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
    // Files of all the set's packages: siblings are recognized by them ([`ldd_problems`]).
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

        // The KWin effect module is the most precise marker, so it goes first.
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

        // Qt modules: anything not found and the private interface of another Qt.
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
        // We declare a divergence only when BOTH versions are known: "could not
        // find out" is not "broken", and must not be used to scare.
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

/// What to advise the person based on the check.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Advice {
    /// Everything is fine or there is nothing to say — no window.
    Nothing,
    /// Rebuild the packages (the shell is installed, the build fell behind the system).
    Rebuild { packages: Vec<String> },
    /// The fork was left without the shell: remove it and bring back stock Plasma.
    RestoreStock { forks: Vec<String> },
    /// It needs removing, but there is nothing to bring back the stock package from right now (it is not in
    /// the repositories) — `message` explains why, nothing is run.
    RestoreBlocked { message: String },
    /// A rebuild is useless for now: the branch the libplasma fork is built from
    /// has not caught up with Plasma from the repositories yet. `plasma` — the
    /// Plasma version, `fork` — the fork version in its default branch.
    WaitForUpstream { plasma: String, fork: String },
    /// Everything works now, but the repositories already have a new Plasma series
    /// the set's authors have not caught up with: do not update. `plasma` — the version
    /// in the repositories, `fork` — the fork version in its default branch.
    HoldUpdate { plasma: String, fork: String },
    /// Everything works now, the repositories have a Plasma point release: after
    /// it a rebuild will be needed (Rexon will offer it).
    RebuildAfterUpdate { from: String, to: String },
    /// The session journal named KWin or Plasma modules NOT from the set (third-party
    /// effects, widgets, window decorations) that failed to load.
    /// `rebuild` — AUR packages whose rebuild fixes this (empty — Rexon
    /// only says what to do).
    BrokenPlugins { rebuild: Vec<String> },
}

/// Advice based on the check result.
///
/// `restore_preview` — the rollback preview answer
/// (`SystemExec::plasma_fork_reset_preview`) for [`Report::restore`]: `(ok,
/// command or the refusal explanation)`. Needed only for a lonely fork; `None` —
/// no preview was made (then the rollback is still offered: the executor
/// will refuse out loud itself if there is nothing to bring back).
///
/// ⭐ A lonely fork matters more than a rebuild: even a fork without the shell that fell
/// behind the system is cured by removal, while a rebuild would leave the same mine until
/// the next update.
pub fn advise(report: &Report, restore_preview: Option<(bool, String)>) -> Advice {
    advise_with_upstream(report, restore_preview, None)
}

/// [`advise`] with the "have the fork authors caught up" check ([`upstream_lag`]).
/// `upstream` — `(the Plasma version the system is heading to; the fork version in its
/// default branch)`; `None` — could not find out (no network, a foreign
/// host) — then the advice is the same as before: a silent skip is better than a false alarm.
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

/// Advice about third-party modules from the journal: [`Advice::BrokenPlugins`] or
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

/// What exactly is broken — lines for the window and the notification: first the set's
/// modules (from the journal), then third-party ones. Empty — the journal named nothing.
pub fn journal_lines(report: &Report) -> Vec<String> {
    let mut all: Vec<crate::plugin_journal::Finding> = report.journal_in_set().into_iter().cloned().collect();
    all.extend(report.journal_outside().into_iter().cloned());
    crate::plugin_journal::finding_lines(&all)
}

/// Window text about third-party modules (simple HTML; lines are escaped).
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
// Have the fork authors caught up with the new Plasma
// ══════════════════════════════════════════════════════════════════════════
//
// An AUR builder builds a `-git` package from the repository's DEFAULT branch (the
// PKGBUILD has no `#branch=`). The libplasma fork merges every Plasma release, and
// for a major transition it creates a new branch (`Plasma/6.8`) and switches the
// default branch to it. Until that happens, a rebuild builds the OLD
// libplasma against the new Plasma — "rebuilt it, still broken", plus
// half an hour of machine time. So before asking about a rebuild we look at:
//   1. the default branch — `git ls-remote --symref <repository> HEAD`;
//   2. its `PROJECT_VERSION` — from that branch's `CMakeLists.txt` (the raw file);
//   3. the Plasma version the system is heading to — the higher of the installed
//      plasma-workspace and libplasma in the repository database (`pacman -Si`, without
//      administrator rights).
// "Caught up" = the fork version is not lower than the Plasma version. The network has a timeout; no
// network, git or curl, a foreign host — the check is silently skipped.

/// Timeout of each network step of the check, seconds.
pub const UPSTREAM_TIMEOUT_SECS: u64 = 12;

/// Repository address for `git ls-remote` from the project address in the package
/// (`URL` in `pacman -Qi`). Only https: any other address is not our case.
pub fn upstream_git_url(project_url: &str) -> Option<String> {
    let url = project_url.trim().trim_end_matches('/');
    if !url.starts_with("https://") || url.contains(char::is_whitespace) {
        return None;
    }
    Some(if url.ends_with(".git") { url.to_string() } else { format!("{url}.git") })
}

/// Default branch from the answer of `git ls-remote --symref <url> HEAD`
/// (`ref: refs/heads/Plasma/6.7\tHEAD`).
pub fn default_branch(ls_remote: &str) -> Option<String> {
    ls_remote.lines().find_map(|line| {
        let rest = line.strip_prefix("ref: refs/heads/")?;
        let (branch, what) = rest.split_once('\t')?;
        (what.trim() == "HEAD" && !branch.is_empty()).then(|| branch.to_string())
    })
}

/// Address of a branch's raw file. We know two kinds of hosting: GitHub and GitLab (its
/// engine also runs gitgud.io, where AeroShell lives). Others — `None`.
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

/// `PROJECT_VERSION` from `CMakeLists.txt`: `set(PROJECT_VERSION "6.7.5")`.
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

/// The highest of the versions (numeric comparison; empty ones are skipped).
pub fn newest_version(versions: &[Option<String>]) -> Option<String> {
    versions
        .iter()
        .flatten()
        .map(|v| upstream_version(v))
        .filter(|v| !crate::engine_update::ver_tuple(v).is_empty())
        .max_by(|a, b| crate::engine_update::ver_tuple(a).cmp(&crate::engine_update::ver_tuple(b)))
}

/// Whether the fork lags behind Plasma: the fork version is lower. A fork beta (`6.7.91`)
/// vs a release (`6.8.0`) lags: that is how it is, the branch is not released yet.
pub fn upstream_lag(fork_version: &str, plasma_version: &str) -> bool {
    let (f, p) = (
        crate::engine_update::ver_tuple(fork_version),
        crate::engine_update::ver_tuple(plasma_version),
    );
    !f.is_empty() && !p.is_empty() && f < p
}

/// Title of the "a rebuild will not help yet" window.
pub fn wait_title() -> String {
    tr("Repair Isn't Possible Yet")
}
/// Text of the "a rebuild will not help yet" window.
pub fn wait_text(plasma: &str, fork: &str) -> String {
    // A major transition is named briefly ("Plasma 6.8"), a point one in full
    // ("Plasma 6.7.6" with fork 6.7.5): otherwise it would read "have not caught up with 6.7,
    // they have 6.7.5".
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
// Warning BEFORE the system update
// ══════════════════════════════════════════════════════════════════════════
//
// The check above learns about trouble when it has already happened: Plasma updated,
// "Start" and the taskbar vanished. A new Plasma shows up in the repositories before
// the person presses "Update" — and "wait" can be said before the breakage.
// What will come with the update is asked of `checkupdates` (pacman-contrib):
// it syncs ITS OWN copy of the repository database in a temporary directory,
// without administrator rights and without touching the system database. If it is missing, we honestly
// skip: reading the system database (`pacman -Si`) means seeing it as
// it was at the last `-Sy`, and syncing it itself needs root.
//
// Decision:
//   * a transition to a new Plasma series (6.7 → 6.8) while the libplasma fork in the default
//     branch is still on the old one ([`upstream_lag`] by series) — "do not
//     update": after the update "Start" and the taskbar will vanish, and
//     there will be nothing to rebuild from;
//   * a point release (6.7.4 → 6.7.5) — softly: the set's KWin effects will turn off after
//     it until a rebuild, Rexon will offer it;
//   * the authors are already on the new series or their version could not be found out (no network)
//     — as softly as a point release: "do not update" without data would
//     be a false alarm, and a rebuild after the update is needed anyway.
// ⛔ A warning only: no IgnorePkg, pacman hooks or interference
// with other people's AUR helpers.

/// Packages of the official Plasma stack whose update tells us about a new
/// Plasma. The set owner's libplasma is replaced by the fork and will not appear in the
/// update list — then plasma-workspace and kwin decide.
pub const PLASMA_STACK_PACKAGES: &[&str] = &["plasma-workspace", "kwin", "libplasma"];

// Parsing `checkupdates` is shared by all "before the update" checks — in
// [`crate::update_guard`].
pub use crate::update_guard::{
    parse_pending_updates, PendingUpdate, PENDING_UPDATES_BINARY, PENDING_UPDATES_TIMEOUT_SECS,
};

/// Plasma series — the first two numbers: `6.7.5` → `6.7`.
pub fn plasma_series(version: &str) -> String {
    version.split('.').take(2).collect::<Vec<_>>().join(".")
}

/// A pending Plasma update: `(version now, version in the repositories)` without
/// the epoch and package release — from the stack package with the newest version. None —
/// Plasma is not in the update list (a package rebuild without a version change is
/// not either: compatibility does not change with it).
pub fn pending_plasma(updates: &[PendingUpdate]) -> Option<(String, String)> {
    use crate::engine_update::ver_tuple;
    updates
        .iter()
        .filter(|u| PLASMA_STACK_PACKAGES.contains(&u.package.as_str()))
        .map(|u| (upstream_version(&u.installed), upstream_version(&u.available)))
        .filter(|(from, to)| !ver_tuple(to).is_empty() && ver_tuple(to) > ver_tuple(from))
        .max_by(|a, b| ver_tuple(&a.1).cmp(&ver_tuple(&b.1)))
}

/// Advice BEFORE the update. Called when everything is fine now
/// ([`Advice::Nothing`]): what is already broken matters more than what is only coming.
///
/// `updates` — the parsed `checkupdates` output; `fork` — the libplasma fork version
/// in its default branch (None — could not find out).
pub fn advise_before_update(report: &Report, updates: &[PendingUpdate], fork: Option<&str>) -> Advice {
    // No built packages of the set — nothing to break (icons and sounds do not
    // depend on the Plasma version).
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
        // The authors caught up or their version could not be found out: there is no ground to say
        // "do not update", but a rebuild after the update will be needed.
        _ => Advice::RebuildAfterUpdate { from, to },
    }
}

/// Title of the "do not update the system" warning.
pub fn hold_title() -> String {
    tr("Don't Update the System Yet")
}
/// Text of the "do not update the system" warning.
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
/// Title of the soft warning about a point release.
pub fn rebuild_after_title() -> String {
    tr("A Plasma Update Is Waiting")
}
/// Text of the soft warning about a point release.
pub fn rebuild_after_text(to: &str) -> String {
    trf(
        // TRANSLATORS: %1 is the Plasma version waiting in the repositories (e.g. 6.7.5).
        "Plasma %1 is ready to install. After the update, the AeroThemePlasma \
         window effects will stay off until it is rebuilt for the new version — \
         Rexon will offer the rebuild right after the update.",
        &[&to],
    )
}

// ── Texts of the "bring back stock Plasma" window ────────────────────────

/// Question window title.
pub fn restore_title() -> String {
    tr("Leftover of AeroThemePlasma Found")
}
/// Main question text (simple HTML).
pub fn restore_text() -> String {
    tr("<b>AeroThemePlasma has been removed, but its replacement for a part of \
        Plasma is still installed.</b>")
}
/// Explanation: what the risk is and what Rexon will do (simple HTML).
pub fn restore_informative() -> String {
    tr("Right now everything works, but the next Plasma update can break the \
        whole desktop: the panel and the lock screen may stop starting.\
        <br><br>Rexon will remove the leftover and put the standard Plasma part \
        back. You'll be asked for the password once; afterwards you'll need to \
        log out and log back in.")
}
/// The consent button.
pub fn restore_button() -> String {
    format!("🧹 {}", tr("Restore Standard Plasma"))
}
/// Progress window title.
pub fn restore_run_title() -> String {
    tr("Restoring Standard Plasma")
}
/// The first mark in the progress window.
pub fn restore_start_message() -> String {
    tr("🧹 Removing the leftover and restoring standard Plasma…")
}
/// The mark in the progress window when everything worked.
pub fn restore_done_message() -> String {
    tr("✅ Standard Plasma restored")
}
/// The result under the log on success (simple HTML).
pub fn restore_ok_text() -> String {
    tr("The leftover has been removed and the standard Plasma part is back.\
        <br><br><b>Log out and log back in</b> to finish.")
}
/// Refusal title when there is nothing to bring back the stock package from — the same as
/// the window about replacement packages in the window menu: it is the same case.
pub fn restore_blocked_title() -> String {
    crate::window_menu::forks_blocked_title()
}

/// The same, but the files are read from disk.
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

/// The first available AUR helper that can rebuild.
pub fn pick_helper(exists: &dyn Fn(&str) -> bool) -> Option<String> {
    REBUILD_HELPERS
        .iter()
        .find(|h| exists(h))
        .map(|h| (*h).to_string())
}

/// The tool without which there is no clean-environment build (the
/// `devtools` package). paru calls it itself, and without it `--chroot` fails.
const CHROOT_TOOL: &str = "mkarchroot";

/// Whether a clean-environment build is possible: only paru can do it, and only when
/// the system has something to create it with. Asking for `--chroot` without devtools
/// means a guaranteed build failure out of nowhere.
pub fn chroot_available(helper: &str, exists: &dyn Fn(&str) -> bool) -> bool {
    helper == "paru" && exists(CHROOT_TOOL)
}

/// The command to rebuild packages with the chosen helper.
///
/// * `--rebuild` — build again even if the version in the AUR did not change
///   (for `-git` packages it does not change, yet a rebuild is needed).
/// * `--chroot` — a clean-environment build; requested only when it is
///   possible at all ([`chroot_available`]). yay has no such option
///   at all, and paru without devtools fails on this flag.
/// * `--sudo pkexec` — rights are requested via the graphical polkit window. Rexon
///   is started from the application menu, where there is no terminal: plain sudo in such
///   a build simply cannot ask for the password and everything fails.
///
/// None — an unknown helper or nothing to rebuild.
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
    // Not a single question into the void. By default both builders offer
    // to review changes in the build files and wait for an answer — and there is nobody
    // to answer: the window has no terminal, and the build would hang forever.
    argv.push(match helper {
        "paru" => "--skipreview".to_string(),
        // ⚠️ Specifically `--diffmenu=false`: yay does not understand `--nodiffmenu`
        // (checked by running it on 10.08.2026 — "invalid option").
        _ => "--diffmenu=false".to_string(),
    });
    argv.push("--noconfirm".to_string());
    // Rights: the privilege session shim (item B10, `privilege::aur`) — only without
    // a clean environment: in it paru calls `mkarchroot` and
    // `arch-nspawn` via sudo, while the shim lets only pacman through. Otherwise — `pkexec`.
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
// Rebuild: what is decided BEFORE the builder starts, how its progress and result are read
// ══════════════════════════════════════════════════════════════════════════
//
// Exactly three actions remain outside (in `rexon/system.py`): ask the
// system whether paru/yay/mkarchroot and a polkit agent exist, wait for the
// package manager lock and run the builder itself, reading its output line by line.
// Everything else is here.

/// How many last output lines we keep at hand. No need for more: the person
/// sees the whole build progress in the progress window anyway, the tail is only needed for
/// the final error message.
pub const REBUILD_TAIL_LINES: usize = 40;
/// How many tail lines to show when the build failed.
pub const REBUILD_FAILURE_LINES: usize = 15;
/// The stage under which build progress goes to the progress window.
pub const REBUILD_LINE_STAGE: &str = "package";
/// The start-of-work stage.
pub const REBUILD_START_STAGE: &str = "install";
/// The successful-end stage.
pub const REBUILD_DONE_STAGE: &str = "done";

/// Nothing to rebuild.
pub fn rebuild_nothing() -> String {
    tr("Nothing to rebuild")
}
/// The system has neither paru nor yay.
pub fn rebuild_no_helper() -> String {
    tr("No AUR package builder found (paru or yay) — the appearance can't be rebuilt without it")
}
/// Could not build the command (unknown helper).
pub fn rebuild_no_command() -> String {
    tr("Couldn't put together the rebuild command")
}
/// The mark in the progress window when the build succeeded.
pub fn rebuild_done_message() -> String {
    tr("✅ Appearance rebuilt")
}
/// The result for the person when the build succeeded.
pub fn rebuild_success_message() -> String {
    tr("The appearance was rebuilt for the current system version.\nLog out and back in \
        for everything to take effect.")
}

/// The rebuild start message.
pub fn rebuild_start_message(packages: usize) -> String {
    ntr(
        "🔧 Rebuilding %n appearance package…",
        "🔧 Rebuilding %n appearance packages…",
        packages as u64,
    )
}

/// The builder could not even be started (OS error).
pub fn rebuild_spawn_failure(helper: &str, error: &str) -> String {
    trf("Couldn't start %1: %2", &[&helper, &error])
}

/// Refusal when the build finished with non-zero and said not a word.
pub fn rebuild_exit_code_message(code: i32) -> String {
    trf("The build exited with code %1", &[&code])
}

/// What to do with the rebuild: either a ready refusal, or the command and where to start.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RebuildPlan {
    /// A ready refusal — no need to go outside.
    pub error: Option<String>,
    /// The builder command (empty when there is an `error`).
    pub argv: Vec<String>,
    /// Stage and text of the first mark in the progress window.
    pub start_stage: String,
    pub start_message: String,
}

/// The decision "can it be rebuilt and with what" — entirely from explicit inputs.
///
/// `helper` — the AUR builder found on the system ([`pick_helper`]);
/// `chroot` — whether a clean-environment build is available ([`chroot_available`]);
/// `polkit_ok` — whether there is a way to request rights; `polkit_missing` — the ready
/// refusal text for that case (built by `system::polkit_missing_message`).
///
/// The order of checks is the same as in Python: nothing to build → nothing to
/// build with → nothing to ask rights with → the command did not come together.
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

/// A builder output line: what of it to show and remember.
/// None — skip the line (empty or whitespace only).
pub fn rebuild_line(raw: &str) -> Option<String> {
    let line = raw.trim_end();
    if line.is_empty() {
        None
    } else {
        Some(line.to_string())
    }
}

/// The rebuild result.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RebuildOutcome {
    pub ok: bool,
    /// What to return to the caller.
    pub message: String,
    /// The last mark in the progress window (None — nothing to mark).
    pub log_stage: Option<String>,
    pub log_message: Option<String>,
}

/// Parsing the builder's return code together with the tail of its output.
///
/// `tail` — the last output lines (no longer than [`REBUILD_TAIL_LINES`]).
/// On failure the person is shown the last [`REBUILD_FAILURE_LINES`]
/// lines; if the builder said nothing at all — at least the return code.
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
// Rebuild in the Rust window: what the core executor adds
// ══════════════════════════════════════════════════════════════════════════
//
// The rebuild is executed by `system_de_exec::DeOpsExec::rebuild_aeroshell` — in the same
// order as a desktop installation: progress into the operation window, waiting for
// the package manager lock, cancellation while the system is untouched. Here are only
// the decisions and texts the Python path did not have.

/// A look check is already running (a second button press or the startup check).
pub fn rebuild_busy_message() -> String {
    tr("⏳ The appearance is already being checked — wait until it finishes.")
}
/// The "Check now" button was pressed — the check went to the background.
pub fn survey_running_message() -> String {
    tr("🔍 Checking the appearance…")
}
/// The build did not fit into the allotted time (the "build" class, hours).
pub fn rebuild_timeout_message() -> String {
    tr("The appearance build took too long and was stopped. Try again later.")
}
/// The first line of the result when the build failed.
pub fn rebuild_failed_head() -> String {
    tr("Couldn't rebuild the appearance.")
}
/// The caption above the builder output tail in the failure result.
pub fn rebuild_tail_head() -> String {
    tr("Last lines of the build:")
}

/// The builder's launch environment: English speech. The same trick as in Python
/// (`LANG=C`, `LC_ALL=C`): a password refusal is recognized by this speech, while with a Russian
/// locale the password window would answer in Russian, and the refusal would read as a build
/// failure.
pub fn rebuild_argv_c_locale(argv: &[String]) -> Vec<String> {
    let mut out: Vec<String> = ["env", "LANG=C", "LC_ALL=C"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    out.extend(argv.iter().cloned());
    out
}

/// The builder failed because the person did not confirm the administrator password.
///
/// The builder asks for rights itself (`--sudo pkexec`), and the password window's code does not
/// reach us — only its words in the output: "Request dismissed" (the window was closed),
/// "Not authorized" (no polkit agent) or code 126 in the builder's
/// error line.
pub fn rebuild_auth_denied(output: &str) -> bool {
    crate::system_plan::pkexec_auth_denied(0, output)
        || output.contains("Request dismissed")
        || output.contains("exit status 126")
}

/// Result of a password refusal: what happened and what to do. The button is named with the same
/// words that are written on it.
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

/// Result of a failed build: first the main thing for the person (the system is intact), then
/// the builder output tail — its parse ([`rebuild_outcome`]) is already done.
pub fn rebuild_failure_message(builder_output: &str) -> String {
    format!(
        "{} {}\n\n{}\n{builder_output}",
        rebuild_failed_head(),
        crate::de_progress::aeroshell_fail_note(),
        rebuild_tail_head(),
    )
}

/// Question window text: the main text and the explanation as one block (simple
/// HTML). Package names and versions do not go into the question — they mean nothing
/// to the buyer; they are visible in the progress log ([`crate::de_progress::aeroshell_details`]).
pub fn confirm_text() -> String {
    format!(
        "{}<br><br>{}",
        crate::de_progress::aeroshell_text(),
        crate::de_progress::aeroshell_informative()
    )
}

/// Question window title: with only a risk ([`Report::at_risk_only`])
/// — "needs rebuilding", otherwise — "stopped working".
pub fn confirm_title_for(report: &Report) -> String {
    if report.at_risk_only() {
        crate::de_progress::aeroshell_risk_title()
    } else {
        crate::de_progress::aeroshell_title()
    }
}

/// Label of the consent button in the question window — matching the title
/// ([`confirm_title_for`]): with only a risk — "Rebuild", with
/// breakage — "Restore". The button's action is the same either way.
pub fn confirm_button_for(report: &Report) -> String {
    if report.at_risk_only() {
        crate::de_progress::aeroshell_rebuild_button()
    } else {
        crate::de_progress::aeroshell_fix_button()
    }
}

/// [`confirm_text`] with what the session journal named: which effect or
/// widget is turned off and why (simple HTML). An empty journal — as before.
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

/// Check details for the log — from the check result.
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
// Checking outside a Rexon launch: after a package update and at session login
// ══════════════════════════════════════════════════════════════════════════
//
// The check at Rexon startup does nothing for someone who updated the system and did not open
// Rexon: the desktop breaks at the next login, and they find out from an
// empty panel. Hence — systemd user units (no root, no
// password). The units are shared by all "before and after the update" checks —
// [`crate::update_guard::watch_units`]: with Plasma forks
// installed, their `rexon --update-check` also calls this check (`rexon --aeroshell-check`):
// wait for the pacman transaction to end, check (read-only) and on trouble
// show a notification with a button that opens Rexon right on the check.
// The password is asked only for the rebuild or rollback itself — in the Rexon
// window, once.

/// The flag of the windowless check route (called by the `.service`).
pub const CHECK_FLAG: &str = "--aeroshell-check";
/// An addition to [`CHECK_FLAG`]: the login check — notify even about what
/// was already said (a new session — a new person at the screen).
pub const LOGIN_FLAG: &str = "--login";
/// The flag of a window opened from a notification: check right away, even if
/// the startup check is disabled, and open "Settings".
pub const OPEN_FLAG: &str = "--aeroshell-repair";

pub use crate::update_guard::{LOCK_WAIT_SECS, PACMAN_DB_LOCK, PACMAN_LOCAL_DB};
/// How long to wait for a click in the notification, seconds.
pub const NOTIFY_WAIT_SECS: u64 = 6 * 60 * 60;

/// State fingerprint for the notification: we do not say the same thing a second
/// time (the watcher fires on every package of an update).
pub fn notify_fingerprint(report: &Report, advice: &Advice) -> String {
    // For warnings before an update, WHAT is pending matters: a new series
    // ("do not update" until 6.8 is one state, 6.8.0 and 6.8.1 are no reason
    // to repeat) and the exact release for the soft warning.
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
    // The journal — only when it named something: old fingerprints stay
    // the same, and we do not talk about the same state a second time.
    if !report.journal.is_empty() {
        let mut ids: Vec<&str> = report.journal.iter().map(|f| f.failure.id.as_str()).collect();
        ids.sort_unstable();
        fp.push_str(&format!(" journal={}", ids.join(",")));
    }
    fp
}

/// Notification: title, text (plain) and button label.
#[derive(Debug, Clone, PartialEq)]
pub struct Notice {
    pub summary: String,
    pub body: String,
    pub button: String,
}

/// What to say in a notification based on the advice. None — nothing to say.
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

/// [`notice_for`] with what the session journal named: the "what is broken —
/// what to do" lines follow the general text.
pub fn notice_for_report(advice: &Advice, report: &Report) -> Option<Notice> {
    let mut notice = notice_for(advice)?;
    // Only a risk — soft words, as in the window ([`confirm_title_for`]):
    // the desktop works, and "stopped working" over it reads as a lie.
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

/// Whether to notify: there is something to say, and it is not what was already said
/// (`last` — the saved fingerprint). At session login we always speak.
pub fn should_notify(advice: &Advice, fingerprint: &str, last: Option<&str>, login: bool) -> bool {
    // Third-party modules are not repeated at login: this is not breakage of the whole desktop,
    // and something said once about a widget removed from disk would otherwise sound at
    // every login.
    // A risk without breakage (fingerprint `rebuild-risk`) — also once.
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
        // A bare identifier (the KWin header) is skipped, the version is taken
        // from the next occurrence.
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

    /// A set as on a live machine: an effect module for an old KWin, a libplasma
    /// fork for the current Plasma, a package with pictures only.
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
        // The WHOLE built set is rebuilt, not only the package that broke.
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
        // A package without markers and a package whose file list could not be read: we stay silent.
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
        // The shell is installed (either of the two sessions) — it needs the fork.
        assert!(!lonely_libplasma_fork(&names(&[
            "aeroshell-libplasma-git",
            "aerothemeplasma-desktop-git",
        ])));
        assert!(!lonely_libplasma_fork(&names(&[
            "aeroshell-libplasma-git",
            "aerothemeplasma-desktop-x11-git",
        ])));
        // No libplasma fork — nothing to roll back.
        assert!(!lonely_libplasma_fork(&names(&["aerothemeplasma-icons-git"])));
        assert!(!lonely_libplasma_fork(&[]));
    }

    /// A machine like the owner's on 04.10: only the libplasma fork is left of the set,
    /// the versions match. The advice is to bring back stock Plasma, not "all is well".
    #[test]
    fn lonely_fork_advises_restore_even_when_versions_match() {
        let forks = names(&["aeroshell-libplasma-git", "aerothemeplasma-icons-git"]);
        let report = check_with(&forks, Some("6.7.4"), Some("6.7.4-1.1"), &fixture, &|_p| None);
        assert!(!report.needs_rebuild);
        assert!(report.lonely_fork);
        // Icons stay: they replace nothing.
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

    /// A lonely fork that already fell behind — still a rollback, not a rebuild.
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
        // Nothing is broken — the check against the authors changes nothing.
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

    /// `ldd -r` output in the form live modules give (checked on
    /// built packages of the set on 04.10.2026).
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
        // With a symbol version (live `ldd -r /usr/bin/sddm`, SteamOS VM, 25.10.2026).
        assert_eq!(
            ldd_problems("undefined symbol: _ZTI20QDBusAbstractAdaptor, version Qt_6\t(/usr/bin/sddm)\n", &[]),
            vec!["undefined symbol: _ZTI20QDBusAbstractAdaptor".to_string()]
        );
        // The library file exists but is empty (live `ldd -r`, SteamOS VM, 25.10.2026).
        assert_eq!(
            ldd_problems(
                "\n/usr/bin/sddm-greeter-qt6: error while loading shared libraries: /usr/lib/libQt6Quick.so.6: file too short\n",
                &[]
            ),
            vec!["/usr/lib/libQt6Quick.so.6: file too short".to_string()]
        );
    }

    /// smodglow.so requires the decoration by bare name: it is a sibling from the set,
    /// the host loads it — not breakage (live `ldd -r` gives exactly this).
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

    /// Plasmoids are no longer "without a marker": a foreign Qt minor in a module with the private
    /// interface and a missing library make the package lag behind.
    #[test]
    fn applets_checked_for_qt_minor_and_missing_libraries() {
        let forks = names(&["aeroshell-libplasma-git", "aerothemeplasma-desktop-git"]);
        let healthy = check_modules_with(
            &forks, Some("6.7.4"), Some("6.7.4"), Some("6.11.2-3"),
            &desktop_fixture, &applet_blob, &|_| Some(String::new()),
        );
        assert!(!healthy.needs_rebuild, "{healthy:?}");
        assert_eq!(healthy.packages[1].marker, Marker::None);

        // Qt moved to 6.12: notifications was built for 6.11 with the private API.
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

        // The soname of a library SevenStart links to changed.
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

    // ── Warning before an update ─────────────────────────────────────────

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
        // Plasma is not updated — None; a package rebuild without a version change — too.
        assert_eq!(pending_plasma(&parse_pending_updates("firefox 1-1 -> 2-1\n")), None);
        assert_eq!(pending_plasma(&parse_pending_updates("kwin 6.7.5-1 -> 6.7.5-2\n")), None);
        // The epoch does not confuse it.
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
        // A beta of the fork branch — not caught up yet.
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

    /// A new series, and the authors caught up (including 6.8.0 with 6.8.1 pending) or
    /// their version is unknown — softly: we do not say "do not update", but a
    /// rebuild after the update will be needed.
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
        // No set.
        assert_eq!(advise_before_update(&Report::empty(), &updates, Some("6.7.5")), Advice::Nothing);
        // Only icons — nothing to break.
        let icons = check_with(&names(&["aerothemeplasma-icons-git"]), Some("6.7.4"), Some("6.7.4"), &fixture, &|_p| None);
        assert_eq!(advise_before_update(&icons, &updates, Some("6.7.5")), Advice::Nothing);
        let report = working_set();
        // No Plasma updates.
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
        // 6.8.0 → 6.8.1 in the repositories — the same "do not update", not a repeat.
        let fp = notify_fingerprint(&report, &hold);
        let fp2 = notify_fingerprint(&report, &Advice::HoldUpdate { plasma: "6.8.1".into(), fork: "6.7.5".into() });
        assert_eq!(fp, fp2);
        assert!(!should_notify(&hold, &fp2, Some(&fp), false));
        // The authors caught up, and a point release is pending — a new state.
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
        // No devtools — a clean environment must not be requested, otherwise the build
        // fails before it starts.
        let pkgs = vec!["aeroshell-libplasma-git".to_string()];
        let argv = rebuild_command("paru", &pkgs, false).unwrap();
        assert!(!argv.contains(&"--chroot".to_string()), "{argv:?}");
    }

    #[test]
    fn rebuild_command_yay_has_no_chroot() {
        let pkgs = vec!["aeroshell-libplasma-git".to_string()];
        // Even when a clean environment is available: yay has no such flag.
        let argv = rebuild_command("yay", &pkgs, true).unwrap();
        assert!(!argv.contains(&"--chroot".to_string()));
        assert_eq!(argv[0], "yay");
        // yay has its own switch for showing changes — not paru's.
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
        // No key yet — we check; disabled only by an explicit False.
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
        // The button label — from the "Settings" catalog in the interface language.
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

    // ── Session journal ────────────────────────────────────────────────

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

    /// By the versions in the files everything matches, but the journal says: an effect of the set
    /// did not load. We trust the journal — and take the versions from it.
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

    /// The journal is empty (rotated, KWin < 6.8 debugging off) — the old
    /// version check is not lost.
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

    /// A third-party effect from the AUR and a widget from the store: no set at all.
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
        // At login we do not repeat third-party ones; a new state — we speak.
        assert!(!should_notify(&advice, &fp, Some(&fp), true));
        assert!(should_notify(&advice, &fp, None, true));
        let n = notice_for_report(&advice, &report).unwrap();
        assert_eq!(n.summary, crate::plugin_journal::plugins_title());
        assert!(n.body.starts_with("• The “FORCEBLUR” effect"), "{}", n.body);
    }

    /// The set is broken per the journal, and a third-party module too: the advice is to rebuild
    /// the set, and the third-party one is visible in the window lines.
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

    /// A risk is not breakage: modules built only against the private interface of the previous
    /// Qt give soft words and their own fingerprint; any hard evidence — the old ones.
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

        // The Plasma version diverged — that is already breakage.
        let hard = report(pkg(Marker::PlasmaLibrary, "6.7.4", "6.7.5", &[qt]));
        assert!(!hard.at_risk_only());
        assert_eq!(confirm_title_for(&hard), crate::de_progress::aeroshell_title());
        assert_eq!(confirm_button_for(&hard), crate::de_progress::aeroshell_fix_button());
        assert!(notify_fingerprint(&hard, &advice).starts_with("rebuild "));
        // A missing library — likewise.
        let missing = "x.so: libprocesscore.so.10 => not found";
        assert!(!report(pkg(Marker::PlasmaLibrary, "6.7.5", "6.7.5", &[qt, missing])).at_risk_only());
        // No divergence at all — and no risk.
        assert!(!Report::empty().at_risk_only());
    }
}
