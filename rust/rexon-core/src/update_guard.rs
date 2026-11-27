//! Protection against an update that installs a set of packages mismatched.
//!
//! WHY. On 07.10.2026 `pacman -Syu` on CachyOS installed `qt6-base` 6.12.0 from
//! Arch `extra`, while `qt6-declarative` stayed at 6.11.2: CachyOS puts its own
//! builds of some Qt modules into a repository that pacman looks at before
//! `extra`, and publishes them later. `libQt6Qml` 6.11.2 requires the private
//! interface of exactly the Qt version it was built against
//! (`undefined symbol … QtPrivate_6_11_2`) — after a restart the login screen
//! crashed, and the person saw a black screen. This is a recurring class (CachyOS
//! distribution #453, #549, #605; with another library — #604:
//! `gdk-pixbuf2` 2.44.8 with an old `glycin`, Firefox did not start), and
//! it can be learned about BEFORE the update: `checkupdates` tells what will come.
//!
//! What the module does:
//!   * set rules — as DATA ([`PACKAGE_SETS`]): which packages must
//!     go at one version and with what precision; a new set — a new entry;
//!   * the "installed + pending update" check ([`evaluate`]): whether
//!     the set will diverge after the update, whether it has already diverged and whether what is
//!     pending fixes it;
//!   * an orphan package: a set member that is no longer in the repositories
//!     (`pacman -Qm`) does not "lag" — it will never be updated (25.10.2026:
//!     `kwin-x11` 6.7.5 with Plasma 6.8.1, the X11 session was removed from Plasma). Such a
//!     package is taken out of the set check, it gets its own finding
//!     [`Stage::Orphaned`] with the advice to remove it, not to "wait";
//!   * texts for a human, the state fingerprint ("this has already been said");
//!   * user units of the background check — shared by all the
//!     "before/after update" checks ([`watch_units`]); the AeroThemePlasma check
//!     also runs in them, it no longer has units of its own (see below).
//!
//! ⛔ Only a warning: no IgnorePkg, pacman hooks or holding
//! packages. The check never asks for a password. Actions — two window buttons
//! through the window's privilege session: removing an orphan ([`remove_orphans_live`]) and
//! bringing back the previous versions of a set from the pacman cache ([`rollback_live`]); "is it
//! an orphan" and "is this a rollback of exactly the finding's set" the root side checks
//! itself. There are no commands in the window and the notification; the rescue console with a crashed
//! login screen keeps the commands ([`orphan_advice`], rollback —
//! [`crate::login_rescue`]).

use std::collections::{BTreeMap, BTreeSet};

use crate::engine_update::ver_tuple;
use crate::i18n::{fill, fill_plain, n_, tr, trc, trf};
use crate::system::PkgFamily;
use crate::system_de_exec::DeOpsIo;
use crate::system_exec::SystemIo;

// ══════════════════════════════════════════════════════════════════════════
// What will come with the update: `checkupdates`
// ══════════════════════════════════════════════════════════════════════════
//
// `checkupdates` (pacman-contrib) synchronises ITS OWN copy of the repository
// database in a temporary directory, without administrator rights and without touching
// the system database. If it is missing — an honest "we don't know": reading the system database
// (`pacman -Si`) means seeing it as it was at the last
// `-Sy`, and synchronising it itself needs root.

/// The program that safely (without root) tells what will come with the update.
pub const PENDING_UPDATES_BINARY: &str = "checkupdates";

/// `checkupdates` timeout, seconds: it downloads the repository databases.
pub const PENDING_UPDATES_TIMEOUT_SECS: u64 = 180;

/// One pending update: the package, the version now, the version in the repositories.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingUpdate {
    pub package: String,
    pub installed: String,
    pub available: String,
}

/// Parsing the `checkupdates` output: lines `name old -> new`. Color
/// (in case it was turned on after all) and lines of other forms are skipped.
pub fn parse_pending_updates(text: &str) -> Vec<PendingUpdate> {
    let plain = strip_ansi(text);
    plain
        .lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            let (package, installed, arrow, available) =
                (words.next()?, words.next()?, words.next()?, words.next()?);
            (arrow == "->").then(|| PendingUpdate {
                package: package.to_string(),
                installed: installed.to_string(),
                available: available.to_string(),
            })
        })
        .collect()
}

/// Strip terminal color escape sequences.
fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

/// What will come with the system update: the parsed output of `checkupdates`.
/// None — not Arch, the program is missing or it failed (no network) — an honest
/// "we don't know". Exit code 2 of `checkupdates` — "no updates".
pub fn pending_updates<IO: SystemIo + ?Sized>(io: &IO, family: PkgFamily) -> Option<Vec<PendingUpdate>> {
    if family != PkgFamily::Arch || !io.which(PENDING_UPDATES_BINARY) {
        return None;
    }
    let r = io.run(&[PENDING_UPDATES_BINARY.to_string()], Some(PENDING_UPDATES_TIMEOUT_SECS), true);
    match r.rc() {
        Some(0) => Some(parse_pending_updates(&r.stdout)),
        Some(2) => Some(Vec::new()),
        _ => None,
    }
}

/// The directory of the database that `checkupdates` synchronises: `CHECKUPDATES_DB`
/// or `${TMPDIR:-/tmp}/checkup-db-${UID}/` — as it computes it itself.
pub fn checkupdates_db(env_db: &str, tmpdir: &str, uid: u32) -> String {
    if !env_db.trim().is_empty() {
        return env_db.trim().to_string();
    }
    let tmp = if tmpdir.trim().is_empty() { "/tmp" } else { tmpdir.trim() };
    format!("{}/checkup-db-{uid}/", tmp.trim_end_matches('/'))
}

/// Parsing `pacman -Q`: `(name, version)`.
pub fn parse_installed(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            Some((words.next()?.to_string(), words.next()?.to_string()))
        })
        .collect()
}

/// Parsing `pacman -Qg <groups>`: `(group, package)`.
pub fn parse_groups(text: &str) -> Vec<(String, String)> {
    parse_installed(text)
}

/// Parsing `pacman -Sl` (under `LANG=C`): all versions of each package in all
/// enabled repositories — `name → [version…]`.
pub fn parse_sync_list(text: &str) -> BTreeMap<String, Vec<String>> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for line in text.lines() {
        let mut words = line.split_whitespace();
        let (Some(_repo), Some(name), Some(version)) = (words.next(), words.next(), words.next()) else {
            continue;
        };
        out.entry(name.to_string()).or_default().push(version.to_string());
    }
    out
}

/// Parsing `pacman -Qmq`: packages that are in no enabled
/// repository (third-party ones from the AUR and those gone from the repositories).
pub fn parse_foreign(text: &str) -> BTreeSet<String> {
    text.lines().filter_map(|l| l.split_whitespace().next()).map(str::to_string).collect()
}

/// Packages outside the repositories: `pacman -Qmq` (read-only, without network — by
/// the synchronised databases). `dbpath` — the `checkupdates` database (it has
/// fresh repository databases and a `local` link to the installed ones), without it —
/// the system database. None — unknown (then we do not judge orphans).
pub fn foreign_packages<IO: SystemIo + ?Sized>(io: &IO, dbpath: Option<&str>) -> Option<BTreeSet<String>> {
    let mut argv = vec!["pacman".to_string(), "-Qmq".to_string()];
    if let Some(db) = dbpath {
        argv.push("--dbpath".to_string());
        argv.push(db.to_string());
    }
    let r = io.run(&argv, Some(60), true);
    match r.rc() {
        Some(0) => Some(parse_foreign(&r.stdout)),
        // Code 1 without output — there are no third-party packages at all.
        Some(1) if r.stdout.trim().is_empty() && r.stderr.trim().is_empty() => Some(BTreeSet::new()),
        _ => None,
    }
}

/// A version without the epoch and the package release: `1:6.30.0-1.1` → `6.30.0`.
pub fn upstream_version(pacman_version: &str) -> String {
    crate::aeroshell::upstream_version(pacman_version)
}

// ══════════════════════════════════════════════════════════════════════════
// Set rules — data
// ══════════════════════════════════════════════════════════════════════════

/// With what precision the versions of a set must match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Precision {
    /// Only the first number (`51` for GNOME).
    Major,
    /// Two numbers (`6.30` for KDE Frameworks).
    Minor,
    /// Three numbers (`6.12.0` for Qt).
    Patch,
}

impl Precision {
    fn parts(self) -> usize {
        match self {
            Precision::Major => 1,
            Precision::Minor => 2,
            Precision::Patch => 3,
        }
    }
}

/// Who belongs to the set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Members {
    /// All installed packages with such a name prefix.
    Prefix(&'static str),
    /// Installed members of a pacman group (`pacman -Qg`).
    Group(&'static str),
    /// Exactly these packages (among the installed ones).
    Names(&'static [&'static str]),
}

/// How the set must move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rule {
    /// All members — at the same version as `anchor`, with precision `precision`.
    /// `may_lag` — members that may lag behind (but not get ahead).
    /// `Prefix`/`Group` members with a different first version number are not part of
    /// the set: it is someone else's numbering scheme (a third-party `qt6-…` from the AUR).
    SameVersion {
        anchor: &'static str,
        precision: Precision,
        may_lag: &'static [&'static str],
        /// Members with THEIR OWN numbering starting from the given version (`qt6-webengine`
        /// from 6.140.0): from it on the number is not compared with the anchor at all — it is not
        /// a Qt version. Below it the member is checked as usual (taking `may_lag` into account).
        own_numbering: &'static [OwnNumbering],
    },
    /// The numbers differ, but they must move together: one member is updated,
    /// and for the other the repositories already have a newer one than pacman installs
    /// (a priority repository holds an old build) — they will diverge.
    MoveTogether,
}

/// A set member that from some version on is numbered on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OwnNumbering {
    pub package: &'static str,
    /// The first version of its own numbering (the first numbers: `[6, 140]`).
    pub from: &'static [u128],
}

impl OwnNumbering {
    /// This package version is already in its own numbering.
    fn covers(&self, name: &str, version: &str) -> bool {
        name == self.package && ver_tuple(&upstream_version(version)).as_slice() >= self.from
    }
}

/// The member's version is in its own numbering, there is nothing to compare with the anchor.
fn own_scheme(own: &[OwnNumbering], name: &str, version: &str) -> bool {
    own.iter().any(|o| o.covers(name, version))
}

/// What a diverged set threatens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    /// The login screen / the desktop after a restart.
    LoginScreen,
    /// Programs that use the set.
    Programs,
}

/// One set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackageSet {
    /// Identifier for the fingerprint.
    pub id: &'static str,
    /// Name of the set for a human — a proper name, not translated.
    pub name: &'static str,
    pub members: Members,
    pub rule: Rule,
    pub effect: Effect,
}

/// Set rules. A new set — a new entry, no code needed.
///
/// * **Qt** — up to the third number: Qt modules use the private interface of
///   `qt6-base`, bound to the EXACT version (`QtPrivate_6_12_0`; measured on
///   08.10.2026 with `objdump -T`: 16 `qt6-*` packages require this symbol). The exception
///   is `qt6-webengine`: its libraries do not use the private interface with such
///   a binding (measured on 14.11.2026 with `objdump -T`: 6.140.0 has
///   not a single `QtPrivate_6_*`, `qt6-declarative` 6.12.0 has 57,
///   `qt6-wayland` — 4). While it was numbered like Qt, it was allowed to
///   lag (6.11.2 with `qt6-base` 6.12.0), but not to get ahead. Since Qt 6.12
///   Qt WebEngine (together with Qt PDF and the Qt WebView engine) is a separate product
///   with ITS OWN numbering: the second number is the base Chromium version (6.140.0 —
///   Chromium 140, released alongside Qt 6.12.0; qt.io blog "Qt WebEngine will
///   release separately"). The number 6.140 says nothing about the Qt version, and
///   it must not be compared with `qt6-base` (a false alarm on 14.11.2026: "qt6-base
///   6.12.0 with qt6-webengine 6.140.0", a package from Arch `extra`, `ldd` clean).
///   ⚠️ The boundary: the requirement "Qt not older than the one it was built against" is not visible
///   from its own numbering — in Arch its `depends` is just `qt6-base`.
/// * **KDE Frameworks** and **Plasma** — up to the second number (pacman groups
///   `kf6` and `plasma`: point releases of individual packages are the norm).
/// * **GNOME** — `gnome-shell` works only with its own `mutter` (the library
///   number = the first number).
/// * **gdk-pixbuf2 + glycin** — the CachyOS #604 case: the numbers differ, they must
///   move together.
pub const PACKAGE_SETS: &[PackageSet] = &[
    PackageSet {
        id: "qt6",
        name: "Qt",
        members: Members::Prefix("qt6-"),
        rule: Rule::SameVersion {
            anchor: "qt6-base",
            precision: Precision::Patch,
            may_lag: &["qt6-webengine"],
            own_numbering: &[OwnNumbering { package: "qt6-webengine", from: &[6, 140] }],
        },
        effect: Effect::LoginScreen,
    },
    PackageSet {
        id: "kf6",
        name: "KDE Frameworks",
        members: Members::Group("kf6"),
        rule: Rule::SameVersion { anchor: "kcoreaddons", precision: Precision::Minor, may_lag: &[], own_numbering: &[] },
        effect: Effect::LoginScreen,
    },
    PackageSet {
        id: "plasma",
        name: "Plasma",
        members: Members::Group("plasma"),
        rule: Rule::SameVersion { anchor: "plasma-workspace", precision: Precision::Minor, may_lag: &[], own_numbering: &[] },
        effect: Effect::LoginScreen,
    },
    PackageSet {
        id: "gnome",
        name: "GNOME",
        members: Members::Names(&["mutter", "gnome-shell"]),
        rule: Rule::SameVersion { anchor: "mutter", precision: Precision::Major, may_lag: &[], own_numbering: &[] },
        effect: Effect::LoginScreen,
    },
    PackageSet {
        id: "pixbuf",
        name: "GTK",
        members: Members::Names(&["gdk-pixbuf2", "glycin", "glycin-gtk4"]),
        rule: Rule::MoveTogether,
        effect: Effect::Programs,
    },
];

/// The pacman groups that need to be queried (`pacman -Qg …`).
pub fn groups_needed(sets: &[PackageSet]) -> Vec<&'static str> {
    sets.iter()
        .filter_map(|s| match s.members {
            Members::Group(g) => Some(g),
            _ => None,
        })
        .collect()
}

// ══════════════════════════════════════════════════════════════════════════
// Check
// ══════════════════════════════════════════════════════════════════════════

/// What is visible on the machine: the installed packages, groups, pending updates, versions in the repositories.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Survey {
    /// `pacman -Q`.
    pub installed: Vec<(String, String)>,
    /// `pacman -Qg`: `(group, package)`.
    pub groups: Vec<(String, String)>,
    /// `checkupdates`.
    pub pending: Vec<PendingUpdate>,
    /// `pacman -Sl` by the `checkupdates` database: all versions in all repositories.
    /// None — not asked or failed (then we do not judge "move together":
    /// without this there would be a false alarm on every gdk-pixbuf2 update).
    pub repo: Option<BTreeMap<String, Vec<String>>>,
    /// `pacman -Qmq`: packages that are not in the repositories. None — not
    /// asked or failed: then such packages are checked like all others
    /// (the former behaviour — "mismatched").
    pub foreign: Option<BTreeSet<String>>,
}

/// Where the set diverged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// Everything matches now, the update will split it: "don't update".
    Hold,
    /// Already diverged, and the update does not fix it: "don't restart".
    DontReboot,
    /// Already diverged, and the pending update fixes it: "update once more".
    UpdateNow,
    /// A set member left the repositories and remained from the previous version: it will never
    /// be updated — "remove it if you don't use it". One finding —
    /// one such package (`odd[0]`).
    Orphaned,
}

impl Stage {
    fn key(self) -> &'static str {
        match self {
            Stage::Hold => "hold",
            Stage::DontReboot => "noreboot",
            Stage::UpdateNow => "update",
            Stage::Orphaned => "orphan",
        }
    }
}

/// Where to roll a package back to: the exact version (the one installed now) or any from
/// the cache with such first numbers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RollbackTarget {
    Exact(String),
    Series(Precision, Vec<u128>),
}

/// A finding for one set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub set: PackageSet,
    pub stage: Stage,
    /// The leading package and its version (without the epoch and release) in the diverged
    /// state: the anchor package or (for "move together") the one being updated.
    pub lead: (String, String),
    /// Those diverged from it: name, version, and (for "move together") the version
    /// that is already in another repository.
    pub odd: Vec<(String, String, Option<String>)>,
    /// What to bring back from the cache so that the set matches again.
    pub rollback: Vec<(String, RollbackTarget)>,
}

fn key_of(version: &str, precision: Precision) -> Vec<u128> {
    ver_tuple(&upstream_version(version)).into_iter().take(precision.parts()).collect()
}

fn major_of(version: &str) -> Option<u128> {
    ver_tuple(&upstream_version(version)).first().copied()
}

/// Set members among the installed packages.
fn members_of<'a>(set: &PackageSet, survey: &'a Survey) -> Vec<&'a (String, String)> {
    let in_group = |group: &str, name: &str| survey.groups.iter().any(|(g, p)| g == group && p == name);
    survey
        .installed
        .iter()
        .filter(|(name, _)| match set.members {
            Members::Prefix(p) => name.starts_with(p),
            Members::Group(g) => in_group(g, name),
            Members::Names(list) => list.contains(&name.as_str()),
        })
        .collect()
}

/// Check all sets. Empty — nothing to talk about.
pub fn evaluate(survey: &Survey, sets: &[PackageSet]) -> Vec<Finding> {
    sets.iter().flat_map(|set| evaluate_set(set, survey)).collect()
}

fn evaluate_set(set: &PackageSet, survey: &Survey) -> Vec<Finding> {
    match set.rule {
        Rule::SameVersion { anchor, precision, may_lag, own_numbering } => {
            same_version(set, survey, anchor, precision, may_lag, own_numbering)
        }
        Rule::MoveTogether => move_together(set, survey).into_iter().collect(),
    }
}

/// Those that diverged from the anchor package in one state (`version(name)`).
fn mismatched(
    names: &[&str],
    anchor: &str,
    precision: Precision,
    may_lag: &[&str],
    own: &[OwnNumbering],
    version: &dyn Fn(&str) -> String,
) -> Vec<String> {
    let anchor_key = key_of(&version(anchor), precision);
    names
        .iter()
        .filter(|n| **n != anchor)
        .filter(|n| !own_scheme(own, n, &version(n)))
        .filter(|n| {
            let k = key_of(&version(n), precision);
            if may_lag.contains(n) {
                k > anchor_key
            } else {
                k != anchor_key
            }
        })
        .map(|n| n.to_string())
        .collect()
}

fn same_version(
    set: &PackageSet,
    survey: &Survey,
    anchor: &str,
    precision: Precision,
    may_lag: &[&str],
    own: &[OwnNumbering],
) -> Vec<Finding> {
    let members = members_of(set, survey);
    let Some(anchor_installed) = members.iter().find(|(n, _)| n == anchor).map(|(_, v)| v.clone()) else {
        return Vec::new();
    };
    let anchor_major = major_of(&anchor_installed);
    let all_names: Vec<&str> = members
        .iter()
        .filter(|(_, v)| matches!(set.members, Members::Names(_)) || major_of(v) == anchor_major)
        .map(|(n, _)| n.as_str())
        .collect();
    // Those gone from the repositories will never be updated: they are not in the set check,
    // they get their own finding. The anchor package is not considered an orphan.
    let orphan = |n: &str| n != anchor && survey.foreign.as_ref().is_some_and(|f| f.contains(n));
    let (orphans, names): (Vec<&str>, Vec<&str>) = all_names.iter().copied().partition(|n| orphan(n));
    let installed = |name: &str| -> String {
        survey.installed.iter().find(|(n, _)| n == name).map(|(_, v)| v.clone()).unwrap_or_default()
    };
    let pending = |name: &str| survey.pending.iter().find(|u| u.package == name);
    let after = |name: &str| -> String { pending(name).map(|u| u.available.clone()).unwrap_or_else(|| installed(name)) };

    // An orphan against the anchor package AFTER the pending update: it will not
    // move by itself, and the set will move ahead.
    let anchor_after = key_of(&after(anchor), precision);
    let mut out: Vec<Finding> = orphans
        .iter()
        .filter(|n| !own_scheme(own, n, &installed(n)))
        .filter(|n| {
            let k = key_of(&installed(n), precision);
            if may_lag.contains(n) {
                k > anchor_after
            } else {
                k != anchor_after
            }
        })
        .map(|n| Finding {
            set: *set,
            stage: Stage::Orphaned,
            lead: (anchor.to_string(), upstream_version(&after(anchor))),
            odd: vec![(n.to_string(), upstream_version(&installed(n)), None)],
            rollback: Vec::new(),
        })
        .collect();

    let before_odd = mismatched(&names, anchor, precision, may_lag, own, &installed);
    let after_odd = mismatched(&names, anchor, precision, may_lag, own, &after);
    let (stage, odd, version): (Stage, Vec<String>, &dyn Fn(&str) -> String) =
        match (before_odd.is_empty(), after_odd.is_empty()) {
            (true, true) => return out,
            (true, false) => (Stage::Hold, after_odd, &after),
            (false, false) => (Stage::DontReboot, after_odd, &after),
            (false, true) => (Stage::UpdateNow, before_odd, &installed),
        };

    let rollback = match stage {
        // Bring back everything the update will touch in the set: before it the set matched.
        Stage::Hold => names
            .iter()
            .filter_map(|n| pending(n).map(|u| (n.to_string(), RollbackTarget::Exact(u.installed.clone()))))
            .collect(),
        // Already diverged: bring those that got ahead back to the oldest series of the set.
        Stage::DontReboot => {
            let strict: Vec<&str> = names.iter().copied().filter(|n| !may_lag.contains(n)).collect();
            let Some(oldest) = strict.iter().map(|n| key_of(&installed(n), precision)).min() else {
                return out;
            };
            strict
                .iter()
                .filter(|n| key_of(&installed(n), precision) > oldest)
                .map(|n| (n.to_string(), RollbackTarget::Series(precision, oldest.clone())))
                .collect()
        }
        Stage::UpdateNow | Stage::Orphaned => Vec::new(),
    };
    // A mismatched set — first: it is more important than an orphan.
    out.insert(
        0,
        Finding {
            set: *set,
            stage,
            lead: (anchor.to_string(), upstream_version(&version(anchor))),
            odd: odd.into_iter().map(|n| { let v = upstream_version(&version(&n)); (n, v, None) }).collect(),
            rollback,
        },
    );
    out
}

fn move_together(set: &PackageSet, survey: &Survey) -> Option<Finding> {
    let repo = survey.repo.as_ref()?;
    let members = members_of(set, survey);
    let pending = |name: &str| {
        survey.pending.iter().find(|u| {
            u.package == name && ver_tuple(&upstream_version(&u.available)) > ver_tuple(&upstream_version(&u.installed))
        })
    };
    let moved: Vec<&PendingUpdate> = members.iter().filter_map(|(n, _)| pending(n)).collect();
    let lead = moved.first()?;
    // They stay in place, although some repository already has a newer one.
    let stale: Vec<(String, String, Option<String>)> = members
        .iter()
        .filter(|(n, _)| survey.pending.iter().all(|u| &u.package != n))
        .filter_map(|(n, v)| {
            let have = ver_tuple(&upstream_version(v));
            let newest = repo
                .get(n)?
                .iter()
                .map(|r| upstream_version(r))
                .filter(|r| ver_tuple(r) > have)
                .max_by(|a, b| ver_tuple(a).cmp(&ver_tuple(b)))?;
            Some((n.clone(), upstream_version(v), Some(newest)))
        })
        .collect();
    if stale.is_empty() {
        return None;
    }
    Some(Finding {
        set: *set,
        stage: Stage::Hold,
        lead: (lead.package.clone(), upstream_version(&lead.available)),
        odd: stale,
        rollback: moved
            .iter()
            .map(|u| (u.package.clone(), RollbackTarget::Exact(u.installed.clone())))
            .collect(),
    })
}

// ══════════════════════════════════════════════════════════════════════════
// Rollback from the pacman cache
// ══════════════════════════════════════════════════════════════════════════

/// The default pacman package cache.
pub const PACKAGE_CACHE_DIR: &str = "/var/cache/pacman/pkg";

/// A rollback command longer than this is unreadable — then only words. The limit
/// of the rescue CONSOLE ([`crate::login_rescue`]): there the person types it themselves.
pub const ROLLBACK_MAX_PACKAGES: usize = 8;

/// The limit of the «Put Previous Versions Back» button: how many packages of one set
/// the window brings back in one transaction (Plasma — up to ~50 packages of the group). The
/// root helper checks the same limit.
pub const ROLLBACK_BUTTON_MAX: usize = 64;

impl RollbackTarget {
    /// Whether the version of a cache file (full, `6.11.2-1`) fits the target.
    pub fn accepts(&self, version: &str) -> bool {
        match self {
            RollbackTarget::Exact(want) => version == want,
            RollbackTarget::Series(p, key) => &key_of(version, *p) == key,
        }
    }
}

/// Parsing a cache file name: `qt6-base-6.11.2-1-x86_64.pkg.tar.zst` →
/// `("qt6-base", "6.11.2-1")`. Signatures (`.sig`) and everything else — None.
pub fn parse_cache_file(file: &str) -> Option<(String, String)> {
    if file.ends_with(".sig") {
        return None;
    }
    let stem = &file[..file.find(".pkg.tar")?];
    let mut parts = stem.rsplitn(4, '-');
    let (_arch, rel, ver, name) = (parts.next()?, parts.next()?, parts.next()?, parts.next()?);
    (!name.is_empty()).then(|| (name.to_string(), format!("{ver}-{rel}")))
}

/// Cache directories from the output of `pacman-conf CacheDir` (one per line, with a trailing
/// `/`) plus [`PACKAGE_CACHE_DIR`]; only normal absolute paths, without
/// the trailing `/`, without duplicates. The root helper has the same parsing (it
/// calls `pacman-conf` itself).
pub fn parse_cache_dirs(pacman_conf: &str) -> Vec<String> {
    let mut out = vec![PACKAGE_CACHE_DIR.to_string()];
    for line in pacman_conf.lines() {
        let dir = line.trim().trim_end_matches('/');
        let normal = dir.starts_with('/')
            && dir[1..].split('/').all(|seg| !seg.is_empty() && seg != "." && seg != "..")
            && !dir.chars().any(char::is_control);
        if normal && !out.iter().any(|d| d == dir) {
            out.push(dir.to_string());
        }
    }
    out
}

/// The machine's package cache: `(directory, file names)` for each directory of
/// [`parse_cache_dirs`] (`pacman-conf` can be read without privileges).
pub fn cache_listing<IO: SystemIo + ?Sized>(io: &IO) -> Vec<(String, Vec<String>)> {
    let conf = io.run(&["pacman-conf".to_string(), "CacheDir".to_string()], Some(30), true);
    let text = if conf.rc() == Some(0) { conf.stdout } else { String::new() };
    parse_cache_dirs(&text)
        .into_iter()
        .map(|d| {
            let files = io.list_dir(&d).unwrap_or_default();
            (d, files)
        })
        .collect()
}

/// One rollback package: what is installed now and what will come back from the cache.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RollbackItem {
    pub name: String,
    /// The installed version (full, as in `pacman -Q`); empty — we do not know.
    pub from: String,
    /// The version of the cache file (full).
    pub to: String,
    /// The path of the cache file.
    pub path: String,
}

/// Rollback of a finding from the cache: for each package — the newest fitting
/// version from all `caches` directories. Empty — at least one package is not in the cache
/// (a half rollback is worse than none) or there are more than `max` of them. `installed` —
/// `pacman -Q` (for "was").
pub fn rollback_items(
    finding: &Finding,
    installed: &[(String, String)],
    caches: &[(String, Vec<String>)],
    max: usize,
) -> Vec<RollbackItem> {
    if finding.rollback.is_empty() || finding.rollback.len() > max {
        return Vec::new();
    }
    let parsed: Vec<(String, String, String)> = caches
        .iter()
        .flat_map(|(dir, files)| {
            files.iter().filter_map(move |f| {
                parse_cache_file(f).map(|(n, v)| (n, v, format!("{}/{f}", dir.trim_end_matches('/'))))
            })
        })
        .collect();
    let mut out = Vec::new();
    for (name, target) in &finding.rollback {
        let best = parsed
            .iter()
            .filter(|(n, v, _)| n == name && target.accepts(v))
            .max_by(|a, b| ver_tuple(&a.1).cmp(&ver_tuple(&b.1)));
        match best {
            Some((_, to, path)) => out.push(RollbackItem {
                name: name.clone(),
                from: installed.iter().find(|(n, _)| n == name).map(|(_, v)| v.clone()).unwrap_or_default(),
                to: to.clone(),
                path: path.clone(),
            }),
            None => return Vec::new(),
        }
    }
    out
}

/// Cache file paths for the console rollback command (no more than
/// [`ROLLBACK_MAX_PACKAGES`]). Empty — at least one package is not in the cache.
pub fn rollback_paths(finding: &Finding, caches: &[(String, Vec<String>)]) -> Vec<String> {
    rollback_items(finding, &[], caches, ROLLBACK_MAX_PACKAGES).into_iter().map(|i| i.path).collect()
}

// ══════════════════════════════════════════════════════════════════════════
// Texts
// ══════════════════════════════════════════════════════════════════════════

/// Title of the warning by the most important finding.
pub fn title(findings: &[Finding]) -> String {
    let stage = |s: Stage| findings.iter().any(|f| f.stage == s);
    if stage(Stage::DontReboot) {
        tr("Don't Restart the Computer Yet")
    } else if stage(Stage::Hold) {
        tr("Don't Update the System Yet")
    } else if stage(Stage::UpdateNow) {
        tr("Update the System Once More")
    } else {
        tr("A Package No Longer Gets Updates")
    }
}

// Texts about an orphan are constants: they are shown both by the Rexon window (the session language,
// [`tr`]) and by the console when the login screen has crashed (the embedded catalog,
// [`crate::login_rescue`]), each with its own translator.

// TRANSLATORS: %1 is a package name, %2 packages with versions, e.g. "kwin-x11 6.7.5 with plasma-workspace 6.8.1".
const T_ORPHAN: &str = n_(
    "%1 has been removed from the repositories and no longer gets updates: it is left over from an earlier version (%2).",
);
// TRANSLATORS: Two lists of packages with versions, e.g. "kwin-x11 6.7.5" and "plasma-workspace 6.8.1".
const T_ORPHAN_PAIR: &str = n_("%1 with %2");
// TRANSLATORS: %1 is a command line, keep it as is.
const T_ORPHAN_REMOVE: &str = n_("If you don't use it, remove the package: %1");

/// Why a package left the repositories — for known cases: (package,
/// explanation). A new case — a new entry.
pub const ORPHAN_NOTES: &[(&str, &str)] = &[(
    "kwin-x11",
    // TRANSLATORS: Explains why the package kwin-x11 left the repositories. "Plasma (X11)" is a session name.
    n_("The Plasma (X11) session is not supported on this version of Plasma."),
)];

/// All the orphan strings (for the check "each one goes through gettext and with substitutions").
pub const ORPHAN_TEXTS: &[&str] = &[T_ORPHAN, T_ORPHAN_PAIR, T_ORPHAN_REMOVE, ORPHAN_NOTES[0].1];

/// The orphan removal command (`pacman -Rns`: with dependencies that became
/// unneeded, and without saved settings). None — the finding is not about an orphan.
pub fn orphan_remove_command(f: &Finding) -> Option<String> {
    if f.stage != Stage::Orphaned {
        return None;
    }
    let (name, _, _) = f.odd.first()?;
    Some(format!("sudo pacman -Rns {name}"))
}

/// What is up with the orphan: "left the repositories, remained from the previous version" and,
/// if the cause is known, an explanation. `t` — the translator (msgid → text).
pub fn orphan_cause(f: &Finding, t: &dyn Fn(&str) -> String) -> String {
    orphan_cause_with(f, t, fill)
}

/// [`orphan_cause`] for the text console: without right-to-left script isolates
/// (the kernel console does not understand them, see [`crate::i18n::fill_plain`]).
pub fn orphan_cause_plain(f: &Finding, t: &dyn Fn(&str) -> String) -> String {
    orphan_cause_with(f, t, fill_plain)
}

fn orphan_cause_with(
    f: &Finding,
    t: &dyn Fn(&str) -> String,
    fill: fn(&str, &[&dyn std::fmt::Display]) -> String,
) -> String {
    let (name, version, _) = f.odd.first().cloned().unwrap_or_default();
    let pair = fill(&t(T_ORPHAN_PAIR), &[&format!("{name} {version}"), &format!("{} {}", f.lead.0, f.lead.1)]);
    let mut text = fill(&t(T_ORPHAN), &[&name, &pair]);
    if let Some((_, note)) = ORPHAN_NOTES.iter().find(|(n, _)| *n == name) {
        text.push(' ');
        text.push_str(&t(note));
    }
    text
}

/// Advice about an orphan for the CONSOLE: "remove it if you don't use it: command".
/// A person at the rescue console types the command themselves — it belongs here.
/// For the Rexon window and the notification — [`finding_text_for`]: there is no command there.
pub fn orphan_advice(f: &Finding, t: &dyn Fn(&str) -> String) -> Option<String> {
    orphan_remove_command(f).map(|cmd| fill_plain(&t(T_ORPHAN_REMOVE), &[&cmd]))
}

/// Who the finding text is for. The rescue console does not come here — it has its own text with
/// the command ([`orphan_advice`], [`crate::login_rescue`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Audience {
    /// The Rexon window. `removable` — the window has an orphan removal button
    /// ([`removable_orphans`]); if not — we stay silent about what to do with the orphan
    /// (on SteamOS a package cannot be removed at all). `rollback` — the window
    /// has the «Put Previous Versions Back» button ([`rollback_offer`]); if not —
    /// we stay silent about the rollback.
    Window { removable: bool, rollback: bool },
    /// The background check notification: no button — "open Rexon".
    Notification,
}

/// Advice about an orphan for the Rexon window or the notification — without commands.
fn orphan_advice_for(f: &Finding, audience: Audience) -> Option<String> {
    if f.stage != Stage::Orphaned {
        return None;
    }
    match audience {
        // TRANSLATORS: About a package that no longer gets updates; the window has a button to remove it.
        Audience::Window { removable: true, .. } => Some(tr("If you don't use it, Rexon can remove it.")),
        Audience::Window { removable: false, .. } => None,
        // The window with the «Remove» button does not open by itself at startup
        // ([`window_should_show`]) — the notification names the button with which
        // the person will ask for it themselves, in the same words as on it.
        Audience::Notification => Some(trf(
            // TRANSLATORS: In a desktop notification about a package that no longer gets updates; %1 is the label of the “Check Now” button in Rexon settings.
            "If you don't use it, remove it in Rexon settings with “%1”.",
            &[&check_now_label()],
        )),
    }
}

/// The label of the «Check Now» button of the Rexon settings without the icon — as
/// the person sees it.
fn check_now_label() -> String {
    let button = crate::qt_settings::text("aero.check_now", &[]).unwrap_or_default();
    button.trim_start_matches(|c: char| !c.is_alphanumeric()).to_string()
}

/// Orphans of the findings — package names, in order, without duplicates.
pub fn orphan_names(findings: &[Finding]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for f in findings.iter().filter(|f| f.stage == Stage::Orphaned) {
        if let Some((name, _, _)) = f.odd.first() {
            if !out.contains(name) {
                out.push(name.clone());
            }
        }
    }
    out
}

/// Which orphans the window offers to remove with the button: on SteamOS — none
/// (`/usr` is read-only, parts of the system are installed by the image update).
pub fn removable_orphans(findings: &[Finding], steamos: bool) -> Vec<String> {
    if steamos {
        Vec::new()
    } else {
        orphan_names(findings)
    }
}

// ── An orphan the session runs on ───────────────────────────────────────
//
// Foolproofing: the person is in a Plasma (X11) session and removes kwin-x11
// — the next login into this session will not work. The same if the login screen
// opens such a session by itself (autologin, the last chosen one). The decision is made
// by data: package → the session that needs it. A new case — a new entry.

/// A session that depends on an orphan package.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OrphanSession {
    /// The orphan package.
    pub package: &'static str,
    /// logind session type (`Type=`, `XDG_SESSION_TYPE`) where the package is needed.
    pub session_type: &'static str,
    /// The session's desktop by the Rexon database ([`crate::system::detect_current_de`]).
    pub desktop: &'static str,
    /// Session file names (`/usr/share/xsessions/<name>.desktop`) — what
    /// the login screen writes into `Session=`.
    pub sessions: &'static [&'static str],
    /// Session name for a human — as in the login screen list (not translated).
    pub session_name: &'static str,
    /// A session from which the package can be removed safely.
    pub instead: &'static str,
}

/// Orphan packages a session runs on.
pub const ORPHAN_SESSIONS: &[OrphanSession] = &[OrphanSession {
    package: "kwin-x11",
    session_type: "x11",
    desktop: "kde",
    sessions: &["plasmax11"],
    session_name: "Plasma (X11)",
    instead: "Plasma (Wayland)",
}];

/// A table entry by package name.
pub fn orphan_session_of(package: &str) -> Option<&'static OrphanSession> {
    ORPHAN_SESSIONS.iter().find(|s| s.package == package)
}

/// The removal will take away a package that session `session_type` of desktop
/// `current_de` runs on (the one the person is in now).
pub fn orphan_in_use(names: &[String], session_type: &str, current_de: &str) -> Option<&'static OrphanSession> {
    let kind = session_type.trim().to_ascii_lowercase();
    ORPHAN_SESSIONS
        .iter()
        .find(|s| names.iter().any(|n| n == s.package) && s.session_type == kind && s.desktop == current_de.trim())
}

/// Session name from the login screen's `Session=` value: `plasmax11`,
/// `plasmax11.desktop`, `/usr/share/xsessions/plasmax11.desktop` → `plasmax11`.
pub fn session_file_name(value: &str) -> String {
    let base = value.trim().rsplit('/').next().unwrap_or_default();
    base.strip_suffix(".desktop").unwrap_or(base).to_string()
}

/// The removal will take away a package of a session that the login screen opens by itself
/// (`defaults` — [`login_default_sessions`]).
pub fn orphan_login_default(names: &[String], defaults: &[String]) -> Option<&'static OrphanSession> {
    ORPHAN_SESSIONS.iter().find(|s| {
        names.iter().any(|n| n == s.package) && defaults.iter().any(|d| s.sessions.contains(&session_file_name(d).as_str()))
    })
}

/// Sessions that the login screen opens or offers by itself: SDDM and Plasma Login
/// Manager autologin, the last chosen one in SDDM, the preselected one
/// in Plasma Login Manager (sources and order —
/// [`crate::system_plan::LOGIN_SESSION_SOURCES`]). `read` — file text,
/// `list` — names in a directory.
pub fn login_default_sessions(read: &dyn Fn(&str) -> Option<String>, list: &dyn Fn(&str) -> Vec<String>) -> Vec<String> {
    crate::system_plan::login_session_values(read, list)
}

const T_ORPHAN_IN_USE: &str =
    // TRANSLATORS: %1 and %2 are session names as shown on the login screen, e.g. "Plasma (X11)" and "Plasma (Wayland)".
    n_("You are using %1 right now, and it runs on this package. Sign in to %2 and remove it from there.");
const T_ORPHAN_LOGIN_DEFAULT: &str = n_(
    // TRANSLATORS: %1 and %2 are session names as shown on the login screen, e.g. "Plasma (X11)" and "Plasma (Wayland)".
    "The login screen starts %1 by itself, and it runs on this package. Sign in to %2 once, so the login \
     screen remembers it, and remove the package from there.",
);

/// Lines of the "the session depends on the package" refusals — templates with `%1`/`%2` (for
/// checking translations: the finished text substitutes the session names).
pub const ORPHAN_SESSION_TEXTS: &[&str] = &[T_ORPHAN_IN_USE, T_ORPHAN_LOGIN_DEFAULT, T_ORPHAN_NOW_REMOVABLE];

// TRANSLATORS: Said after the login screen was switched to another session; the package is the one Rexon offered to remove.
const T_ORPHAN_NOW_REMOVABLE: &str = n_("Now the package can be removed.");

/// Refusal: the person is currently in a session that depends on the package.
pub fn orphan_in_use_text(s: &OrphanSession) -> String {
    trf(T_ORPHAN_IN_USE, &[&s.session_name, &s.instead])
}

/// Refusal: the login screen itself opens a session that depends on the package.
pub fn orphan_login_default_text(s: &OrphanSession) -> String {
    trf(T_ORPHAN_LOGIN_DEFAULT, &[&s.session_name, &s.instead])
}

/// The helper's refusal `why` when removing `names` — "the login screen itself opens
/// a session on the package": then instead of a bare refusal the window offers to make
/// the Wayland counterpart of the session the default session (its entry in
/// [`crate::system_plan::X11_SESSION_SUCCESSORS`]). The refusal arrives as ready
/// text in the same language ([`crate::privilege::client::key_text`]), so it
/// is recognised by comparing with the same text.
pub fn login_default_successor(names: &[String], why: &str) -> Option<&'static crate::system_plan::SessionSuccessor> {
    ORPHAN_SESSIONS
        .iter()
        .filter(|s| names.iter().any(|n| n == s.package))
        .find(|s| why.trim() == orphan_login_default_text(s))
        .and_then(|s| s.sessions.iter().find_map(|x| crate::system_plan::session_successor(x)))
}

/// A note after changing the default session: now the orphan can be removed.
pub fn orphan_now_removable() -> String {
    crate::i18n::tr(T_ORPHAN_NOW_REMOVABLE)
}

/// What the guard window offers for orphans.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrphanOffer {
    /// The «Remove …» button.
    Remove(Vec<String>),
    /// Removing from here is not possible — this text instead of the button.
    Blocked(String),
    /// No orphans, or removal is impossible altogether (SteamOS) — neither a button nor advice.
    Nothing,
}

/// The window's decision. `session_type` and `current_de` — the window's session (the same one
/// the root helper learns from logind and checks itself).
pub fn orphan_offer(findings: &[Finding], steamos: bool, session_type: &str, current_de: &str) -> OrphanOffer {
    let names = removable_orphans(findings, steamos);
    if names.is_empty() {
        return OrphanOffer::Nothing;
    }
    match orphan_in_use(&names, session_type, current_de) {
        Some(s) => OrphanOffer::Blocked(orphan_in_use_text(s)),
        None => OrphanOffer::Remove(names),
    }
}

/// [`orphan_offer`] on the live machine (the window session — from the environment).
pub fn orphan_offer_live(findings: &[Finding]) -> OrphanOffer {
    let env = |n: &str| std::env::var(n).unwrap_or_default();
    let steamos = crate::system::DistroDetector::detect_live().is_steamos();
    let de = crate::system::detect_current_de(&env("XDG_CURRENT_DESKTOP"), &env("DESKTOP_SESSION"), !env("WAYLAND_DISPLAY").is_empty())
        .unwrap_or_default();
    orphan_offer(findings, steamos, &env("XDG_SESSION_TYPE"), &de)
}

/// A list of names for the label: «kwin-x11» or «kwin-x11, foo».
fn names_list(names: &[String]) -> String {
    names.join(", ")
}

/// The window button: «Remove kwin-x11».
pub fn orphan_remove_button(names: &[String]) -> String {
    // TRANSLATORS: Button. %1 is a package name (or several, comma-separated), e.g. "kwin-x11".
    trf("Remove %1", &[&names_list(names)])
}

/// Title of the removal progress window.
pub fn orphan_remove_title(names: &[String]) -> String {
    // TRANSLATORS: Window title while packages are being removed. %1 is a package name (or several).
    trf("Removing %1", &[&names_list(names)])
}

/// Result of the removal. `rest_clean` — the repeated check found nothing more.
pub fn orphan_removed_text(names: &[String], rest_clean: bool) -> String {
    // TRANSLATORS: Result of removing packages. %1 is a package name (or several).
    let done = trf("Removed: %1.", &[&names_list(names)]);
    if rest_clean {
        // TRANSLATORS: Follows "Removed: <package>." after Rexon checked the packages again.
        format!("{done} {}", tr("Everything else is in order."))
    } else {
        done
    }
}

/// The removal failed, and there is no explanation.
pub fn orphan_remove_failed(names: &[String]) -> String {
    // TRANSLATORS: %1 is a package name (or several).
    trf("Rexon couldn't remove %1.", &[&names_list(names)])
}

/// Remove orphans through the window's privilege session ([`crate::privilege`]). That the package
/// really is an orphan (installed, in no repository, a set member), is not
/// vital and not part of the current desktop — the root helper checks
/// itself; it does not trust the window. `run` — the operation executor
/// ([`crate::privilege::run::run_live`] on the live machine). This operation deliberately has
/// no former path through `pkexec`: without the helper there is nobody to check the
/// "orphan" as root.
///
/// `Err` — ready text for a human.
pub fn remove_orphans_with(
    names: &[String],
    run: &mut dyn FnMut(&crate::privilege::op::PrivOp) -> Option<crate::system_exec::RunOut>,
) -> Result<(), String> {
    if names.is_empty() {
        return Err(orphan_remove_failed(names));
    }
    let op = crate::privilege::op::PrivOp::OrphanRemove { names: names.to_vec() };
    let Some(out) = run(&op) else {
        // There is no session helper at all.
        return Err(crate::privilege::client::ClientError::HelperMissing.human_text());
    };
    if out.returncode == Some(0) {
        return Ok(());
    }
    // A check refusal — ready text in stderr; a pacman failure — its lines in
    // stdout (they are already in the window's log), the person gets a general result.
    let why = out.stderr.lines().map(str::trim).rfind(|l| !l.is_empty()).map(str::to_string);
    Err(why.unwrap_or_else(|| orphan_remove_failed(names)))
}

/// [`remove_orphans_with`] on the live machine: progress lines go to `line`.
pub fn remove_orphans_live(names: &[String], line: &mut dyn FnMut(&str)) -> Result<(), String> {
    remove_orphans_with(names, &mut |op| crate::privilege::run::run_live(op, &[], None, &mut *line))
}

/// A set member that can become an orphan: a package of a "single version" set,
/// but not its anchor (an anchor is not considered an orphan, see [`evaluate`]).
/// `groups` — `(group, package)` from `pacman -Qg` ([`groups_needed`]). This
/// check is also for the root helper: it agrees to remove only such packages.
pub fn orphan_candidate(name: &str, groups: &[(String, String)]) -> bool {
    PACKAGE_SETS.iter().any(|set| {
        let Rule::SameVersion { anchor, .. } = set.rule else {
            return false;
        };
        name != anchor
            && match set.members {
                Members::Prefix(p) => name.starts_with(p),
                Members::Group(g) => groups.iter().any(|(gr, p)| gr == g && p == name),
                Members::Names(list) => list.contains(&name),
            }
    })
}

fn versions_list(odd: &[(String, String, Option<String>)]) -> String {
    const SHOWN: usize = 3;
    let mut list = odd
        .iter()
        .take(SHOWN)
        .map(|(n, v, _)| format!("{n} {v}"))
        .collect::<Vec<_>>()
        .join(", ");
    if odd.len() > SHOWN {
        list.push_str(", …");
    }
    list
}

/// A paragraph about the set for the Rexon window with buttons (removing orphans, rollback).
/// `rollback` — the previous versions of all rollback packages are in the cache
/// ([`rollback_items`]).
pub fn finding_text(f: &Finding, rollback: bool) -> String {
    finding_text_for(f, rollback, Audience::Window { removable: true, rollback: true })
}

/// A paragraph about the set for the window or the notification ([`Audience`]).
pub fn finding_text_for(f: &Finding, rollback: bool, audience: Audience) -> String {
    if f.stage == Stage::Orphaned {
        let translate = |m: &str| tr(m);
        return [Some(orphan_cause(f, &translate)), orphan_advice_for(f, audience)]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join("\n\n");
    }
    let lead = format!("{} {}", f.lead.0, f.lead.1);
    let headline = match (f.set.rule, f.stage) {
        (Rule::MoveTogether, _) => {
            let (name, version, newer) = f.odd.first().cloned().unwrap_or_default();
            trf(
                // TRANSLATORS: %1 is a package with its new version (e.g. "gdk-pixbuf2 2.44.8"),
                // %2 a package it needs, %3 the version of %2 that stays installed,
                // %4 the newer version of %2 that already exists in another repository.
                "The update would install %1, but %2, which it needs, would stay at version %3: \
                 the repository your system prefers doesn't have version %4 yet.",
                &[&lead, &name, &version, &newer.unwrap_or_default()],
            )
        }
        (_, Stage::Hold) => trf(
            // TRANSLATORS: %1 is a product name (Qt, Plasma, KDE Frameworks, GNOME),
            // %2 packages with versions, e.g. "qt6-base 6.12.0 with qt6-declarative 6.11.2".
            "The update would install parts of %1 of different versions (%2).",
            &[&f.set.name, &pair_text(&lead, &f.odd)],
        ),
        (_, Stage::DontReboot) => trf(
            // TRANSLATORS: %1 is a product name (Qt, Plasma, KDE Frameworks, GNOME),
            // %2 packages with versions, e.g. "qt6-base 6.12.0 with qt6-declarative 6.11.2".
            "The last update installed parts of %1 of different versions (%2).",
            &[&f.set.name, &pair_text(&lead, &f.odd)],
        ),
        (_, Stage::UpdateNow) => trf(
            // TRANSLATORS: %1 is a product name (Qt, Plasma, KDE Frameworks, GNOME),
            // %2 packages with versions, e.g. "qt6-base 6.12.0 with qt6-declarative 6.11.2".
            "Parts of %1 are of different versions now (%2), and the waiting update brings them together.",
            &[&f.set.name, &pair_text(&lead, &f.odd)],
        ),
        (_, Stage::Orphaned) => unreachable!("сирота — выше"),
    };
    let effect = match f.set.effect {
        Effect::LoginScreen => tr("After a restart, the login screen may not start — only a black screen."),
        Effect::Programs => tr("Programs that use it (for example, Firefox) may stop starting."),
    };
    let action = match f.stage {
        Stage::Hold => tr(
            "Don't update right now. This usually passes within a few hours, when the repository \
             catches up — then the update arrives complete.",
        ),
        Stage::DontReboot => tr(
            "Don't restart the computer yet. The repository usually catches up within a few hours: \
             update the system again then, and everything fits together.",
        ),
        Stage::UpdateNow => tr("Update the system now, before restarting the computer."),
        Stage::Orphaned => unreachable!("сирота — выше"),
    };
    let mut paragraphs = vec![format!("{headline} {effect}"), action];
    if f.stage != Stage::UpdateNow {
        paragraphs.push(match f.set.effect {
            Effect::LoginScreen => tr(
                "If you have already updated and see a black screen after a restart: press \
                 Ctrl+Alt+F3, log in with your user name and password and run \
                 “sudo pacman -Syu”, then “sudo reboot”.",
            ),
            Effect::Programs => tr(
                "If you have already updated and programs stop starting: update the system \
                 again once the repository has caught up.",
            ),
        });
    }
    // Rollback — only when the new mismatched versions are already installed and the previous ones
    // are in the cache: in "don't update" there is nothing to put back (the previous
    // ones are still installed; if the person updates anyway, the next check will say "don't
    // restart" with a button). No command: the window button puts things back.
    if f.stage == Stage::DontReboot && rollback {
        match audience {
            Audience::Window { rollback: true, .. } => paragraphs.push(tr(
                // TRANSLATORS: The window has a button that puts the previous package versions back.
                "If there is nothing to update yet, Rexon can put the previous versions back: they are still \
                 on this computer.",
            )),
            Audience::Window { rollback: false, .. } => {}
            Audience::Notification => paragraphs.push(tr(
                // TRANSLATORS: In a desktop notification; Rexon's window has a button that puts the previous versions back.
                "If there is nothing to update yet, open Rexon to put the previous versions back.",
            )),
        }
    }
    paragraphs.join("\n\n")
}

fn pair_text(lead: &str, odd: &[(String, String, Option<String>)]) -> String {
    trf(
        // TRANSLATORS: two lists of packages with versions, e.g.
        // "qt6-base 6.12.0" and "qt6-declarative 6.11.2, qt6-svg 6.11.2".
        "%1 with %2",
        &[&lead, &versions_list(odd)],
    )
}

/// The whole warning text. `rollback(finding)` — the previous versions are in the
/// cache.
pub fn advice_text(findings: &[Finding], rollback: &dyn Fn(&Finding) -> bool) -> String {
    findings
        .iter()
        .map(|f| finding_text(f, rollback(f)))
        .collect::<Vec<_>>()
        .join("\n\n")
}

// ══════════════════════════════════════════════════════════════════════════
// "This has already been said"
// ══════════════════════════════════════════════════════════════════════════

/// A state fingerprint: we do not say the same thing twice (the Rexon window and
/// the notification share it — what the window showed is not repeated by a notification).
pub fn fingerprint(findings: &[Finding]) -> String {
    findings
        .iter()
        .map(|f| {
            let odd: BTreeSet<String> = f.odd.iter().map(|(n, v, _)| format!("{n}={v}")).collect();
            format!(
                "{}:{}:{}={}:{}",
                f.stage.key(),
                f.set.id,
                f.lead.0,
                f.lead.1,
                odd.into_iter().collect::<Vec<_>>().join(",")
            )
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Whether to speak: there is something to say, and it is not what has already been said
/// (`last` — the saved fingerprint). At session login — always.
pub fn should_notify(findings: &[Finding], fingerprint: &str, last: Option<&str>, login: bool) -> bool {
    !findings.is_empty() && (login || last.map(str::trim) != Some(fingerprint))
}

/// The fingerprint file in the user's cache.
pub fn stamp_path(cache_home: &str) -> String {
    format!("{}/rexon/update-guard-notified", cache_home.trim_end_matches('/'))
}

// ══════════════════════════════════════════════════════════════════════════
// Background check: systemd user units
// ══════════════════════════════════════════════════════════════════════════
//
// One set of units for all the "before and after an update" checks — both this one and
// the AeroThemePlasma check (its former `rexon-aeroshell-check.*` units
// are removed: two timers would run `checkupdates` twice).
//   * `.path` watches `/var/lib/pacman/local` — the directory of the installed
//     packages database. For each package pacman creates a directory
//     `<name>-<version>` there and removes the previous one, and `PathChanged=` on a directory
//     fires on entries appearing, being renamed and removed in it
//     (verified 04.10.2026 with a separate `systemd --user` in a sandbox,
//     systemd 262). The directory is world-readable, no privileges needed. Needed to say
//     "don't restart" right after an update that split a set;
//   * `.timer` — after session login and then every 6 hours: new things in
//     the repositories appear without any transaction on the machine;
//   * `.service` — `rexon --update-check`: wait for the end of the pacman
//     transaction, check (read-only), on trouble — a notification.
// ⛔ No pacman hooks as root: a hook runs inside the transaction, it cannot reach
// the user's session, and installing a hook means a password.

/// Flag of the windowless check route (called by the `.service`).
pub const CHECK_FLAG: &str = "--update-check";

/// Unit names.
pub const WATCH_PATH_UNIT: &str = "rexon-update-check.path";
pub const WATCH_TIMER_UNIT: &str = "rexon-update-check.timer";
pub const WATCH_SERVICE_UNIT: &str = "rexon-update-check.service";
/// The former AeroThemePlasma check units — removed when the shared ones are installed.
pub const LEGACY_UNITS: &[&str] = &[
    "rexon-aeroshell-check.path",
    "rexon-aeroshell-check.timer",
    "rexon-aeroshell-check.service",
];

/// The pacman installed packages database and its lock.
pub const PACMAN_LOCAL_DB: &str = "/var/lib/pacman/local";
pub const PACMAN_DB_LOCK: &str = "/var/lib/pacman/db.lck";
/// How long to wait for someone else's transaction to finish, seconds (a large update
/// with an initramfs rebuild takes minutes).
pub const LOCK_WAIT_SECS: u64 = 45 * 60;

/// The user units directory.
pub fn user_unit_dir(config_home: &str) -> String {
    format!("{}/systemd/user", config_home.trim_end_matches('/'))
}

/// Check units: (name, text). `exec_start` — the ready Rexon launch line
/// for `ExecStart=` ([`crate::app::unit_launch_command`] with
/// [`CHECK_FLAG`]); `program` — the path to its binary: if Rexon was removed, the check
/// is quietly skipped rather than failing every 6 hours. Descriptions are in English and
/// not from the catalog: they are read by `systemctl`, not by the customer.
pub fn watch_units(exec_start: &str, program: Option<&str>) -> Vec<(&'static str, String)> {
    let condition = program
        .filter(|p| p.starts_with('/'))
        .map(|p| format!("ConditionPathExists={p}\n"))
        .unwrap_or_default();
    vec![
        (
            WATCH_PATH_UNIT,
            format!(
                "[Unit]\n\
                 Description=Rexon: check the system after package updates\n\n\
                 [Path]\n\
                 PathChanged={PACMAN_LOCAL_DB}\n\
                 Unit={WATCH_SERVICE_UNIT}\n\
                 # A big update gives hundreds of events within seconds: with\n\
                 # the default limit (200 in 2 s) the watch would switch itself\n\
                 # off. Repeats are harmless: while a check is running, new\n\
                 # triggers merge into it.\n\
                 TriggerLimitIntervalSec=0\n\n\
                 [Install]\n\
                 WantedBy=default.target\n"
            ),
        ),
        (
            WATCH_TIMER_UNIT,
            format!(
                "[Unit]\n\
                 Description=Rexon: check pending updates after login and every 6 hours\n\n\
                 [Timer]\n\
                 OnStartupSec=90s\n\
                 # Repeat every 6 hours: a dangerous update may appear in the\n\
                 # repositories before the update — warn while it is not too late.\n\
                 OnUnitActiveSec=6h\n\
                 Unit={WATCH_SERVICE_UNIT}\n\n\
                 [Install]\n\
                 WantedBy=timers.target\n"
            ),
        ),
        (
            WATCH_SERVICE_UNIT,
            format!(
                "[Unit]\n\
                 Description=Rexon: warn before an update that would break the system\n\
                 {condition}\n\
                 [Service]\n\
                 Type=oneshot\n\
                 ExecStart={exec_start}\n\
                 # Waits for the end of the pacman transaction and for a press in the notification.\n\
                 TimeoutStartSec=infinity\n\
                 # Rexon, opened by the notification button, survives the check.\n\
                 KillMode=process\n\
                 Nice=10\n"
            ),
        ),
    ]
}

/// What to do with the check units.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WatchPlan {
    /// (path, text) — write.
    pub write: Vec<(String, String)>,
    /// Paths — delete (before the first `daemon-reload`).
    pub remove: Vec<String>,
    /// Arguments of `systemctl --user …` in order.
    pub systemctl: Vec<Vec<String>>,
}

/// The plan: `wanted` — whether watching is needed (Arch); `exec_start` — the launch line
/// (None — nothing to launch, watching is removed); `existing(path)` — the current
/// text of the unit file or None. The former AeroThemePlasma units are removed in
/// any case.
///
/// Idempotent: everything matches — the plan is empty, `systemctl` is not called.
pub fn watch_plan(
    config_home: &str,
    wanted: bool,
    exec_start: Option<&str>,
    program: Option<&str>,
    existing: &dyn Fn(&str) -> Option<String>,
) -> WatchPlan {
    let dir = user_unit_dir(config_home);
    let path = |name: &str| format!("{dir}/{name}");
    let mut plan = WatchPlan::default();
    let enable_names = [WATCH_PATH_UNIT, WATCH_TIMER_UNIT];
    let mut disable: Vec<&str> = Vec::new();

    // The former units — remove.
    for name in LEGACY_UNITS {
        if existing(&path(name)).is_some() {
            plan.remove.push(path(name));
            if !name.ends_with(".service") {
                disable.push(name);
            }
        }
    }
    let mut enable: Vec<&str> = Vec::new();
    match (wanted, exec_start) {
        (true, Some(exec)) => {
            for (name, text) in watch_units(exec, program) {
                if existing(&path(name)).as_deref() != Some(text.as_str()) {
                    plan.write.push((path(name), text));
                }
            }
            if !plan.write.is_empty() {
                enable.extend(enable_names);
            }
        }
        _ => {
            let present: Vec<String> = [WATCH_PATH_UNIT, WATCH_TIMER_UNIT, WATCH_SERVICE_UNIT]
                .iter()
                .map(|n| path(n))
                .filter(|p| existing(p).is_some())
                .collect();
            if !present.is_empty() {
                disable.extend(enable_names);
                plan.remove.extend(present);
            }
        }
    }
    if !disable.is_empty() {
        let mut args = vec!["disable".to_string(), "--now".to_string()];
        args.extend(disable.iter().map(|n| n.to_string()));
        plan.systemctl.push(args);
    }
    if !plan.write.is_empty() || !plan.remove.is_empty() {
        plan.systemctl.push(vec!["daemon-reload".into()]);
    }
    if !enable.is_empty() {
        let mut args = vec!["enable".to_string(), "--now".to_string()];
        args.extend(enable.iter().map(|n| n.to_string()));
        plan.systemctl.push(args);
    }
    plan
}

/// Execute the units plan. Without root: only `~/.config/systemd/user` and
/// `systemctl --user`. An empty plan — nothing is done.
pub fn apply_watch_plan<IO: DeOpsIo + ?Sized>(io: &IO, plan: &WatchPlan) -> Result<(), String> {
    if let Some((first, _)) = plan.write.first() {
        if let Some((dir, _)) = first.rsplit_once('/') {
            io.make_dirs(dir)?;
        }
    }
    for (path, text) in &plan.write {
        io.write_file_mode(path, text, 0o644)?;
    }
    if plan.systemctl.is_empty() {
        return Ok(());
    }
    if !io.which("systemctl") {
        return Err("systemctl not found".to_string());
    }
    for args in &plan.systemctl {
        // Removal: first disable (the units are still in place), then the files.
        if args.first().map(String::as_str) == Some("daemon-reload") {
            for path in &plan.remove {
                io.remove_file(path);
            }
        }
        let mut argv = vec!["systemctl".to_string(), "--user".to_string()];
        argv.extend(args.iter().cloned());
        let r = io.run(&argv, Some(30), true);
        if r.rc() != Some(0) {
            return Err(format!("{}: {}", argv.join(" "), r.stderr.trim()));
        }
    }
    Ok(())
}

// ══════════════════════════════════════════════════════════════════════════
// Execution
// ══════════════════════════════════════════════════════════════════════════

/// Build a [`Survey`]. None — not Arch, no pacman/checkupdates, or
/// `checkupdates` failed (no network): "we don't know" — silent.
pub fn survey<IO: SystemIo + ?Sized>(io: &IO, family: PkgFamily, uid: u32, sets: &[PackageSet]) -> Option<Survey> {
    if family != PkgFamily::Arch || !io.which("pacman") {
        return None;
    }
    let pending = pending_updates(io, family)?;
    let q = io.run(&["pacman".to_string(), "-Q".to_string()], Some(60), true);
    if q.rc() != Some(0) {
        return None;
    }
    let installed = parse_installed(&q.stdout);
    let groups = groups_needed(sets);
    let groups = if groups.is_empty() {
        Vec::new()
    } else {
        // Code 1 if some group does not exist at all — the found ones are in the output anyway.
        let mut argv = vec!["pacman".to_string(), "-Qg".to_string()];
        argv.extend(groups.iter().map(|g| g.to_string()));
        parse_groups(&io.run(&argv, Some(60), true).stdout)
    };
    let db = checkupdates_db(&io.env("CHECKUPDATES_DB"), &io.env("TMPDIR"), uid);
    // Orphans — by the fresh `checkupdates` database (it has a `local` link to
    // the installed ones), without it — by the system database.
    let fresh = io.exists(&format!("{}/local", db.trim_end_matches('/'))).then_some(db.as_str());
    let foreign = foreign_packages(io, fresh).or_else(|| fresh.and_then(|_| foreign_packages(io, None)));
    let mut survey = Survey { installed, groups, pending, repo: None, foreign };
    // Repository versions are needed only for "move together", and only when
    // someone from such a set is updated: `pacman -Sl` is tens of thousands of lines.
    let needs_repo = sets.iter().any(|s| {
        s.rule == Rule::MoveTogether
            && members_of(s, &survey).iter().any(|(n, _)| survey.pending.iter().any(|u| &u.package == n))
    });
    if needs_repo {
        if io.is_dir(&db) {
            let r = io.run(
                &["pacman".to_string(), "-Sl".to_string(), "--dbpath".to_string(), db.clone()],
                Some(60),
                true,
            );
            if r.rc() == Some(0) {
                survey.repo = Some(parse_sync_list(&r.stdout));
            }
        }
    }
    Some(survey)
}

/// Result of the check: the findings and the ready rollback paths for each.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Advice {
    pub findings: Vec<Finding>,
    /// Rollback of each finding from the cache ([`rollback_items`], no more than
    /// [`ROLLBACK_BUTTON_MAX`]); empty — nothing to roll back or nothing to roll back with.
    pub rollback: Vec<Vec<RollbackItem>>,
    /// Installed Plasma forks (AeroThemePlasma): the background check route
    /// leads to their check only when they are present.
    pub plasma_forks: Vec<String>,
}

impl Advice {
    pub fn title(&self) -> String {
        title(&self.findings)
    }
    /// Text of the Rexon window with the orphan removal and rollback buttons.
    pub fn text(&self) -> String {
        self.text_for(Audience::Window { removable: true, rollback: true })
    }
    /// Text for the window or the notification ([`Audience`]).
    pub fn text_for(&self, audience: Audience) -> String {
        self.findings
            .iter()
            .zip(&self.rollback)
            .map(|(f, r)| finding_text_for(f, !r.is_empty(), audience))
            .collect::<Vec<_>>()
            .join("\n\n")
    }
    /// Orphans among the findings ([`orphan_names`]).
    pub fn orphans(&self) -> Vec<String> {
        orphan_names(&self.findings)
    }
    pub fn fingerprint(&self) -> String {
        fingerprint(&self.findings)
    }
}

/// The whole check: [`survey`] → [`evaluate`] → rollback from the cache.
pub fn advise<IO: SystemIo + ?Sized>(io: &IO, family: PkgFamily, uid: u32) -> Advice {
    let Some(survey) = survey(io, family, uid, PACKAGE_SETS) else {
        return Advice::default();
    };
    let names: Vec<String> = survey.installed.iter().map(|(n, _)| n.clone()).collect();
    let plasma_forks = crate::system_plan::filter_plasma_forks(&names);
    let findings = evaluate(&survey, PACKAGE_SETS);
    let cache = if findings.iter().any(|f| !f.rollback.is_empty()) { cache_listing(io) } else { Vec::new() };
    let rollback =
        findings.iter().map(|f| rollback_items(f, &survey.installed, &cache, ROLLBACK_BUTTON_MAX)).collect();
    Advice { findings, rollback, plasma_forks }
}

// ── The «Put Previous Versions Back» button ─────────────────────────────

/// What the window offers to bring back with one button: the previous versions of ONE set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RollbackOffer {
    /// Name of the set for a human (Qt, Plasma, …).
    pub set_name: String,
    pub items: Vec<RollbackItem>,
}

/// The window's decision about a rollback. Only "don't restart": the new
/// mismatched versions are already installed, and the cache has the previous ones for ALL
/// rollback packages. "Don't update" — there is nothing to put back (the
/// previous ones are still installed), "update now" — the update fixes it, an orphan has its own button.
/// On SteamOS — never (`/usr` is read-only). Several sets — the
/// first: the others will be shown by the repeated check after the rollback.
pub fn rollback_offer(advice: &Advice, steamos: bool) -> Option<RollbackOffer> {
    if steamos {
        return None;
    }
    advice
        .findings
        .iter()
        .zip(&advice.rollback)
        .find(|(f, items)| f.stage == Stage::DontReboot && !items.is_empty())
        .map(|(f, items)| RollbackOffer { set_name: f.set.name.to_string(), items: items.clone() })
}

/// [`rollback_offer`] on the live machine.
pub fn rollback_offer_live(advice: &Advice) -> Option<RollbackOffer> {
    rollback_offer(advice, crate::system::DistroDetector::detect_live().is_steamos())
}

/// A version for a human: without the epoch and release, if "was" and "will be" are
/// distinguishable without them; otherwise the full one.
fn shown_versions(from: &str, to: &str) -> (String, String) {
    let (f, t) = (upstream_version(from), upstream_version(to));
    if f == t || f.is_empty() {
        (from.to_string(), to.to_string())
    } else {
        (f, t)
    }
}

/// How many "was → will be" lines the confirmation window shows.
const ROLLBACK_SHOWN: usize = 10;

/// The guard window button.
pub fn rollback_button() -> String {
    // TRANSLATORS: Button. Puts back the package versions that were installed before the last update.
    tr("Put Previous Versions Back")
}

/// Title of the confirmation window: «Put Previous Qt Versions Back?».
pub fn rollback_confirm_title(offer: &RollbackOffer) -> String {
    // TRANSLATORS: Window title. %1 is a product name (Qt, Plasma, KDE Frameworks, GNOME).
    trf("Put Back the Previous Versions of %1?", &[&offer.set_name])
}

/// Text of the confirmation window: the "was → will be" list.
pub fn rollback_confirm_text(offer: &RollbackOffer) -> String {
    let mut lines: Vec<String> = offer
        .items
        .iter()
        .take(ROLLBACK_SHOWN)
        .map(|i| {
            let (from, to) = shown_versions(&i.from, &i.to);
            if from.is_empty() {
                format!("{} → {to}", i.name)
            } else {
                format!("{} {from} → {to}", i.name)
            }
        })
        .collect();
    if offer.items.len() > ROLLBACK_SHOWN {
        let more = (offer.items.len() - ROLLBACK_SHOWN).to_string();
        // TRANSLATORS: Ends a list of packages; %1 is how many more are not shown.
        lines.push(trf("…and %1 more", &[&more]));
    }
    // TRANSLATORS: Before putting the previous versions of packages back. A list of packages follows.
    let lead = tr("Rexon will replace these packages with the versions you had before the update:");
    // TRANSLATORS: After the list of packages that Rexon puts back.
    let tail = tr("Then Rexon checks the packages again and tells you the result.");
    format!("{lead}\n\n{}\n\n{tail}", lines.join("\n"))
}

/// The "yes" button of the confirmation window.
pub fn rollback_confirm_button() -> String {
    // Its own context: without it «Put Back» shares a translation with another button.
    // TRANSLATORS: Button in the window that confirms putting previous package versions back.
    trc("update guard rollback", "Put Back")
}

/// Title of the progress window.
pub fn rollback_title(offer: &RollbackOffer) -> String {
    // TRANSLATORS: Window title while Rexon puts previous package versions back. %1 is a product name (Qt, Plasma, …).
    trf("Putting Back the Previous Versions of %1", &[&offer.set_name])
}

/// Result of the rollback. `rest_clean` — the repeated check found nothing more.
pub fn rollback_done_text(offer: &RollbackOffer, rest_clean: bool) -> String {
    // TRANSLATORS: Result. %1 is a product name (Qt, Plasma, KDE Frameworks, GNOME).
    let done = trf("The previous versions of %1 are back. Restarting the computer is safe again.", &[&offer.set_name]);
    if rest_clean {
        format!("{done} {}", tr("Everything else is in order."))
    } else {
        done
    }
}

/// The rollback went through, but the set still does not match — too early to restart.
pub fn rollback_still_mixed_text(offer: &RollbackOffer) -> String {
    // TRANSLATORS: Result. %1 is a product name (Qt, Plasma, KDE Frameworks, GNOME).
    trf("Rexon put the previous versions of %1 back, but the packages still don't fit together.", &[&offer.set_name])
}

/// The rollback failed, and there is no explanation.
pub fn rollback_failed(offer: &RollbackOffer) -> String {
    // TRANSLATORS: %1 is a product name (Qt, Plasma, KDE Frameworks, GNOME).
    trf("Rexon couldn't put the previous versions of %1 back.", &[&offer.set_name])
}

/// Put the previous versions back through the window's privilege session ([`crate::privilege`]).
/// That each file is a package from the root cache, that it is exactly the rollback of the finding's set
/// (strictly lower versions, nothing extra) — the root helper checks itself;
/// it does not trust the window. The former `pkexec` path is deliberately absent.
///
/// `Err` — ready text for a human.
pub fn rollback_with(
    offer: &RollbackOffer,
    run: &mut dyn FnMut(&crate::privilege::op::PrivOp) -> Option<crate::system_exec::RunOut>,
) -> Result<(), String> {
    if offer.items.is_empty() {
        return Err(rollback_failed(offer));
    }
    let files = offer.items.iter().map(|i| i.path.clone()).collect();
    let op = crate::privilege::op::PrivOp::CacheRollback { files };
    let Some(out) = run(&op) else {
        return Err(crate::privilege::client::ClientError::HelperMissing.human_text());
    };
    if out.returncode == Some(0) {
        return Ok(());
    }
    let why = out.stderr.lines().map(str::trim).rfind(|l| !l.is_empty()).map(str::to_string);
    Err(why.unwrap_or_else(|| rollback_failed(offer)))
}

/// [`rollback_with`] on the live machine: progress lines go to `line`.
pub fn rollback_live(offer: &RollbackOffer, line: &mut dyn FnMut(&str)) -> Result<(), String> {
    rollback_with(offer, &mut |op| crate::privilege::run::run_live(op, &[], None, &mut *line))
}

/// The check on the live machine (read-only, without a password).
pub fn advise_live() -> Advice {
    let io = crate::system_exec::LiveSystemIo;
    let family = crate::system::DistroDetector::detect_live().family_live();
    // SAFETY: getuid has no preconditions and cannot fail.
    let uid = unsafe { libc::getuid() };
    advise(&io, family, uid)
}

fn cache_live() -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    crate::utils::cache_home(std::env::var("XDG_CACHE_HOME").ok().as_deref().filter(|v| !v.is_empty()), &home)
}

fn stamp_live() -> String {
    stamp_path(&cache_live())
}

/// Whether this state has already been reported (the Rexon window and the notification).
pub fn notice_seen_live(fingerprint: &str) -> bool {
    std::fs::read_to_string(stamp_live()).is_ok_and(|last| last.trim() == fingerprint)
}

/// Remember that this state has been reported.
pub fn notice_mark_live(fingerprint: &str) {
    write_stamp(&stamp_live(), fingerprint);
}

fn write_stamp(stamp: &str, fingerprint: &str) {
    if let Some(dir) = std::path::Path::new(stamp).parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(stamp, fingerprint);
}

/// The file of the fingerprint shown specifically by the WINDOW (next to [`stamp_path`]).
/// The orphan notification says "open Rexon to remove it" — so the
/// window with the button must show up even after a notification about the same thing; the shared
/// fingerprint is not enough for that.
pub fn window_stamp_path(cache_home: &str) -> String {
    format!("{}/rexon/update-guard-shown", cache_home.trim_end_matches('/'))
}

fn window_stamp_live() -> String {
    window_stamp_path(&cache_live())
}

/// Whether the Rexon window has shown this state.
pub fn window_seen_live(fingerprint: &str) -> bool {
    std::fs::read_to_string(window_stamp_live()).is_ok_and(|last| last.trim() == fingerprint)
}

/// Remember that the window showed this state (and the notification has nothing to repeat).
pub fn window_mark_live(fingerprint: &str) {
    write_stamp(&window_stamp_live(), fingerprint);
    notice_mark_live(fingerprint);
}

/// Whether to show the window (`manual` — by the «Check Now» button, always).
///
/// Without the person's request — only:
///   * rollback (`rollback`): the set has ALREADY diverged, after a restart a
///     black screen threatens — by the WINDOW fingerprint (after the notification "open Rexon
///     to put back" the window must show up);
///   * a message without a button — by the fingerprint shared with the notification.
///
/// ⛔ An offer to REMOVE a package (`removal`, an orphan like `kwin-x11`) never
/// pops up by itself: on 08.11.2026 the «Remove kwin-x11» window popped up on the test bench
/// when «Appearance» was first opened, without any action by the person. The orphan
/// is reported by the notification, which names the settings button; the window comes from that button.
pub fn window_should_show(manual: bool, rollback: bool, removal: bool, notified: bool, window_seen: bool) -> bool {
    if manual {
        return true;
    }
    if rollback {
        return !window_seen;
    }
    !removal && !notified
}

/// argv of the notification: `notify-send` (important — does not fade by itself), without it —
/// the notification service via `dbus-send`. None — nothing to show it with.
pub fn notify_argv(which: &dyn Fn(&str) -> bool, summary: &str, body: &str) -> Option<Vec<String>> {
    let icon = "rexon";
    if which("notify-send") {
        Some(vec![
            "notify-send".into(),
            format!("--app-name={}", crate::utils::NOTIFY_APP_NAME),
            format!("--icon={icon}"),
            "--urgency=critical".into(),
            summary.to_string(),
            body.to_string(),
        ])
    } else if which("dbus-send") {
        Some(crate::utils::notify_dbus_argv("dbus-send", summary, body, icon, 0))
    } else {
        None
    }
}

/// Wait for the pacman transaction to finish: checking a half-updated
/// system means scaring for nothing. false — we never waited it out.
fn wait_pacman_lock_live(out: &mut dyn FnMut(&str)) -> bool {
    let started = std::time::Instant::now();
    while std::path::Path::new(PACMAN_DB_LOCK).exists() {
        if started.elapsed().as_secs() > LOCK_WAIT_SECS {
            out("update-check: package manager still busy, giving up");
            return false;
        }
        std::thread::sleep(std::time::Duration::from_secs(3));
    }
    true
}

/// The windowless check (`rexon --update-check [--login]`), called by the
/// user unit. Wait for the pacman transaction to finish → check the sets
/// (read-only) → on trouble a notification, once per state →
/// if Plasma forks are installed, their check ([`crate::system_de_exec::aeroshell_check_route_live`]).
/// The password is never asked. → the process exit code (0 — the check
/// ran, whatever it found).
pub fn check_route_live(login: bool, out: &mut dyn FnMut(&str)) -> i32 {
    if !wait_pacman_lock_live(out) {
        return 0;
    }
    let advice = advise_live();
    let fp = advice.fingerprint();
    out(&format!("update-check: {}", if fp.is_empty() { "ok" } else { fp.as_str() }));
    let last = std::fs::read_to_string(stamp_live()).ok();
    if should_notify(&advice.findings, &fp, last.as_deref(), login) {
        notice_mark_live(&fp);
        let io = crate::system_exec::LiveSystemIo;
        // On SteamOS a package can be neither removed nor put back — "open Rexon"
        // is out of place there.
        let audience = if crate::system::DistroDetector::detect_live().is_steamos() {
            Audience::Window { removable: false, rollback: false }
        } else {
            Audience::Notification
        };
        if let Some(argv) = notify_argv(&|n| io.which(n), &advice.title(), &advice.text_for(audience)) {
            let _ = io.run(&argv, Some(10), false);
        }
    } else if advice.findings.is_empty() && last.is_some_and(|l| !l.trim().is_empty()) {
        // The set matches — forget the previous state: if it diverges the same way again,
        // say it again.
        notice_mark_live("");
    }
    if !advice.plasma_forks.is_empty() {
        return crate::system_de_exec::aeroshell_check_route_live(login, out);
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn installed(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter().map(|(n, v)| (n.to_string(), v.to_string())).collect()
    }

    /// Qt on the machine on 07.10.2026 before the update: everything 6.11.2, webengine too.
    fn qt_611() -> Vec<(String, String)> {
        installed(&[
            ("qt6-base", "6.11.2-1"),
            ("qt6-declarative", "6.11.2-1.1"),
            ("qt6-svg", "6.11.2-1.1"),
            ("qt6-wayland", "6.11.2-1.1"),
            ("qt6-webengine", "6.11.2-2"),
            ("qt6ct", "0.11-1"),
            ("firefox", "141.0-1"),
        ])
    }

    fn survey_with(installed: Vec<(String, String)>, checkupdates: &str) -> Survey {
        Survey { installed, groups: Vec::new(), pending: parse_pending_updates(checkupdates), repo: None, foreign: None }
    }

    #[test]
    fn checkupdates_output_parsed() {
        let list = parse_pending_updates("\u{1b}[1mqt6-base\u{1b}[0m 6.11.2-1 -> 6.12.0-2\njunk\n\n==> error\n");
        assert_eq!(
            list,
            vec![PendingUpdate { package: "qt6-base".into(), installed: "6.11.2-1".into(), available: "6.12.0-2".into() }]
        );
    }

    /// The case of 07.10: qt6-base 6.11.2 → 6.12.0 pending, qt6-declarative 6.11.2
    /// not pending (CachyOS has not published it yet) — "don't update".
    #[test]
    fn qt_base_ahead_of_declarative_says_hold() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let survey = survey_with(qt_611(), "qt6-base 6.11.2-1 -> 6.12.0-2\n");
        let found = evaluate(&survey, PACKAGE_SETS);
        assert_eq!(found.len(), 1, "{found:?}");
        let f = &found[0];
        assert_eq!(f.set.id, "qt6");
        assert_eq!(f.stage, Stage::Hold);
        assert_eq!(f.lead, ("qt6-base".into(), "6.12.0".into()));
        let odd: Vec<&str> = f.odd.iter().map(|(n, _, _)| n.as_str()).collect();
        // webengine 6.11.2 lags legitimately; qt6ct is not a Qt module.
        assert_eq!(odd, ["qt6-declarative", "qt6-svg", "qt6-wayland"]);
        assert_eq!(f.rollback, vec![("qt6-base".into(), RollbackTarget::Exact("6.11.2-1".into()))]);
        let text = finding_text(f, true);
        assert!(text.contains("qt6-base 6.12.0 with qt6-declarative 6.11.2"), "{text}");
        assert!(text.contains("Ctrl+Alt+F3"), "{text}");
        // "Don't update": the previous versions are still installed — there is nothing
        // to put back, neither a command nor a button.
        assert!(!text.contains("pacman -U") && !text.contains("previous versions"), "{text}");
        assert_eq!(title(&found), "Don't Update the System Yet");
    }

    /// All of Qt arrives at once — we stay silent.
    #[test]
    fn whole_qt_at_once_is_quiet() {
        let survey = survey_with(
            qt_611(),
            "qt6-base 6.11.2-1 -> 6.12.0-2\nqt6-declarative 6.11.2-1.1 -> 6.12.0-1.1\n\
             qt6-svg 6.11.2-1.1 -> 6.12.0-1.1\nqt6-wayland 6.11.2-1.1 -> 6.12.0-1.1\nfirefox 141.0-1 -> 142.0-1\n",
        );
        assert!(evaluate(&survey, PACKAGE_SETS).is_empty());
        // No updates at all — likewise.
        assert!(evaluate(&survey_with(qt_611(), ""), PACKAGE_SETS).is_empty());
    }

    /// webengine lags one series behind (as on Arch now) — we stay silent; it must not
    /// get ahead of qt6-base.
    #[test]
    fn webengine_may_lag_but_not_lead() {
        let all_612 = installed(&[
            ("qt6-base", "6.12.0-2"),
            ("qt6-declarative", "6.12.0-1.1"),
            ("qt6-webengine", "6.11.2-2"),
        ]);
        assert!(evaluate(&survey_with(all_612.clone(), ""), PACKAGE_SETS).is_empty());
        let found = evaluate(&survey_with(all_612, "qt6-webengine 6.11.2-2 -> 6.13.0-1\n"), PACKAGE_SETS);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].odd[0].0, "qt6-webengine");
    }

    /// The case of 14.11.2026: Arch published qt6-webengine 6.140.0 — Qt WebEngine has
    /// its own numbering (the second number is Chromium), this is not "Qt parts of different
    /// versions". We stay silent both when it is already installed and when it is only arriving.
    #[test]
    fn webengine_own_numbering_is_not_mixed_qt() {
        let qt_612 = installed(&[
            ("qt6-base", "6.12.0-5"),
            ("qt6-declarative", "6.12.0-4.1"),
            ("qt6-wayland", "6.12.0-1.1"),
            ("qt6-webview", "6.12.0-1.1"),
            ("qt6-webengine", "6.140.0-1"),
        ]);
        assert!(evaluate(&survey_with(qt_612.clone(), ""), PACKAGE_SETS).is_empty());
        let mut old_web = qt_612.clone();
        old_web[4].1 = "6.11.2-2".into();
        assert!(evaluate(&survey_with(old_web, "qt6-webengine 6.11.2-2 -> 6.140.0-1\n"), PACKAGE_SETS).is_empty());
        // The next release of its own numbering — also silent.
        assert!(evaluate(&survey_with(qt_612.clone(), "qt6-webengine 6.140.0-1 -> 6.146.0-1\n"), PACKAGE_SETS).is_empty());
        // All of Qt goes to 6.13, webengine stays 6.140 — silent.
        let whole = "qt6-base 6.12.0-5 -> 6.13.0-1\nqt6-declarative 6.12.0-4.1 -> 6.13.0-1\n\
                     qt6-wayland 6.12.0-1.1 -> 6.13.0-1\nqt6-webview 6.12.0-1.1 -> 6.13.0-1\n";
        assert!(evaluate(&survey_with(qt_612, whole), PACKAGE_SETS).is_empty());
    }

    /// A real partial update next to a webengine with its own numbering:
    /// there is an alarm, and the text has only the modules with Qt numbering.
    #[test]
    fn real_mixed_qt_next_to_webengine_own_numbering() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let mixed = installed(&[
            ("qt6-base", "6.12.0-5"),
            ("qt6-declarative", "6.11.2-1.1"),
            ("qt6-webengine", "6.140.0-1"),
        ]);
        let found = evaluate(&survey_with(mixed.clone(), ""), PACKAGE_SETS);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].stage, Stage::DontReboot);
        let odd: Vec<&str> = found[0].odd.iter().map(|(n, _, _)| n.as_str()).collect();
        assert_eq!(odd, ["qt6-declarative"]);
        // webengine must not be rolled back to "the 6.11 series": its number is not Qt's.
        assert_eq!(found[0].rollback.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(), ["qt6-base"]);
        let text = finding_text(&found[0], true);
        assert!(text.contains("qt6-base 6.12.0 with qt6-declarative 6.11.2"), "{text}");
        assert!(!text.contains("webengine"), "{text}");
        // Before the update: qt6-base goes to 6.13, declarative stays — "don't update".
        let qt_612 = installed(&[
            ("qt6-base", "6.12.0-5"),
            ("qt6-declarative", "6.12.0-4.1"),
            ("qt6-webengine", "6.140.0-1"),
        ]);
        let hold = evaluate(&survey_with(qt_612, "qt6-base 6.12.0-5 -> 6.13.0-1\n"), PACKAGE_SETS);
        assert_eq!(hold.len(), 1, "{hold:?}");
        assert_eq!(hold[0].stage, Stage::Hold);
        assert_eq!(hold[0].odd.iter().map(|(n, _, _)| n.as_str()).collect::<Vec<_>>(), ["qt6-declarative"]);
    }

    /// The boundary of its own numbering is exactly 6.140: 6.14.0 is still Qt numbering (and
    /// gets ahead of qt6-base 6.12 — alarm), 6.139.x too; 6.140.0 itself is already its own.
    #[test]
    fn webengine_own_numbering_boundary() {
        let qt = |web: &str| {
            installed(&[("qt6-base", "6.12.0-5"), ("qt6-declarative", "6.12.0-4.1"), ("qt6-webengine", web)])
        };
        for web in ["6.14.0-1", "6.139.0-1", "1:6.13.0-1"] {
            let found = evaluate(&survey_with(qt(web), ""), PACKAGE_SETS);
            assert_eq!(found.len(), 1, "{web}: {found:?}");
            assert_eq!(found[0].odd[0].0, "qt6-webengine", "{web}");
        }
        for web in ["6.140.0-1", "6.140.1-2", "1:6.150.0-1"] {
            assert!(evaluate(&survey_with(qt(web), ""), PACKAGE_SETS).is_empty(), "{web}");
        }
        // Only webengine has its own numbering: qt6-webview 6.140 — alarm.
        let mut odd_view = qt("6.140.0-1");
        odd_view.push(("qt6-webview".into(), "6.140.0-1".into()));
        let found = evaluate(&survey_with(odd_view, ""), PACKAGE_SETS);
        assert_eq!(found[0].odd.iter().map(|(n, _, _)| n.as_str()).collect::<Vec<_>>(), ["qt6-webview"]);
    }

    /// The update has already split Qt and does not fix it — "don't restart", rollback from
    /// the cache to the old series; a pending update fixes it — "update once more".
    #[test]
    fn already_mixed_qt_dont_reboot_or_update_now() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let mixed = installed(&[
            ("qt6-base", "6.12.0-2"),
            ("qt6-declarative", "6.11.2-1.1"),
            ("qt6-webengine", "6.11.2-2"),
        ]);
        let found = evaluate(&survey_with(mixed.clone(), ""), PACKAGE_SETS);
        assert_eq!(found[0].stage, Stage::DontReboot);
        let cache: Vec<String> = [
            "qt6-base-6.11.1-1-x86_64.pkg.tar.zst",
            "qt6-base-6.11.2-1-x86_64.pkg.tar.zst",
            "qt6-base-6.11.2-1-x86_64.pkg.tar.zst.sig",
            "qt6-base-6.12.0-2-x86_64.pkg.tar.zst",
            "qt6-declarative-6.11.2-1.1-x86_64_v3.pkg.tar.zst",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let caches = |files: &[String]| vec![(PACKAGE_CACHE_DIR.to_string(), files.to_vec())];
        assert_eq!(
            rollback_paths(&found[0], &caches(&cache)),
            vec!["/var/cache/pacman/pkg/qt6-base-6.11.2-1-x86_64.pkg.tar.zst".to_string()]
        );
        // The needed version is not in the cache — no command, only words.
        assert!(rollback_paths(&found[0], &caches(&cache[3..])).is_empty());
        assert_eq!(title(&found), "Don't Restart the Computer Yet");

        let fixed = evaluate(&survey_with(mixed, "qt6-declarative 6.11.2-1.1 -> 6.12.0-1.1\n"), PACKAGE_SETS);
        assert_eq!(fixed[0].stage, Stage::UpdateNow);
        assert!(!finding_text(&fixed[0], false).contains("Ctrl+Alt+F3"));
    }

    /// A point difference in Qt is also a mismatch (the private marker goes up to the third
    /// number); for Plasma and KF6 point releases are the norm.
    #[test]
    fn precision_per_set() {
        let mut survey = survey_with(
            installed(&[
                ("qt6-base", "6.12.0-2"),
                ("qt6-declarative", "6.12.0-1"),
                ("plasma-workspace", "6.7.5-3.1"),
                ("plasma-login-manager", "6.7.4-3"),
                ("kwin", "6.7.5-3.1"),
                ("plasma-wayland-protocols", "1.23.0-1"),
                ("kcoreaddons", "6.30.0-1.1"),
                ("kio", "6.30.0-1.1"),
                ("oxygen-icons", "1:6.30.0-1"),
            ]),
            "qt6-base 6.12.0-2 -> 6.12.1-1\n",
        );
        survey.groups = [
            ("plasma", "plasma-workspace"),
            ("plasma", "plasma-login-manager"),
            ("plasma", "kwin"),
            ("kf6", "kcoreaddons"),
            ("kf6", "kio"),
            ("kf6", "oxygen-icons"),
        ]
        .iter()
        .map(|(g, p)| (g.to_string(), p.to_string()))
        .collect();
        let found = evaluate(&survey, PACKAGE_SETS);
        assert_eq!(found.iter().map(|f| f.set.id).collect::<Vec<_>>(), ["qt6"]);
        // Plasma: kwin 6.8 with plasma-workspace 6.7 — a mismatch.
        survey.pending = parse_pending_updates("kwin 6.7.5-3.1 -> 6.8.0-1\n");
        let found = evaluate(&survey, PACKAGE_SETS);
        assert_eq!(found.iter().map(|f| f.set.id).collect::<Vec<_>>(), ["plasma"]);
        assert_eq!(found[0].lead, ("plasma-workspace".into(), "6.7.5".into()));
        assert_eq!(found[0].odd[0].0, "kwin");
    }

    /// CachyOS #604: gdk-pixbuf2 is updated, glycin stays 2.1.5, although
    /// extra already has 2.2.2 — a mismatch. glycin fresh everywhere — silent.
    #[test]
    fn pixbuf_without_new_glycin_says_hold() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let base = installed(&[("gdk-pixbuf2", "2.44.7-1.1"), ("glycin", "2.1.5-2.1"), ("firefox", "141.0-1")]);
        let mut survey = survey_with(base, "gdk-pixbuf2 2.44.7-1.1 -> 2.44.8-1.1\n");
        survey.repo = Some(parse_sync_list(
            "cachyos-extra-znver4 gdk-pixbuf2 2.44.8-1.1 [installed: 2.44.7-1.1]\n\
             cachyos-extra-znver4 glycin 2.1.5-2.1 [installed]\n\
             extra gdk-pixbuf2 2.44.8-1 [installed: 2.44.7-1.1]\n\
             extra glycin 2.2.2-1 [installed: 2.1.5-2.1]\n",
        ));
        let found = evaluate(&survey, PACKAGE_SETS);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].set.id, "pixbuf");
        assert_eq!(found[0].stage, Stage::Hold);
        assert_eq!(found[0].odd, vec![("glycin".into(), "2.1.5".into(), Some("2.2.2".into()))]);
        let text = finding_text(&found[0], false);
        assert!(text.contains("gdk-pixbuf2 2.44.8"), "{text}");
        assert!(text.contains("version 2.1.5") && text.contains("version 2.2.2"), "{text}");
        assert!(text.contains("Firefox"), "{text}");
        assert!(!text.contains("Ctrl+Alt+F3"), "{text}");

        // glycin in the repositories is not newer than the installed one — an ordinary update.
        survey.repo = Some(parse_sync_list("extra glycin 2.1.5-2 [installed]\n"));
        assert!(evaluate(&survey, PACKAGE_SETS).is_empty());
        // The repository versions are unknown — we do not judge.
        survey.repo = None;
        assert!(evaluate(&survey, PACKAGE_SETS).is_empty());
    }

    /// GNOME: gnome-shell 52 with mutter 51.
    #[test]
    fn gnome_shell_needs_its_mutter() {
        let survey = survey_with(
            installed(&[("mutter", "51.0-1"), ("gnome-shell", "1:51.0-1.1")]),
            "gnome-shell 1:51.0-1.1 -> 1:52.0-1\n",
        );
        let found = evaluate(&survey, PACKAGE_SETS);
        assert_eq!(found[0].set.id, "gnome");
        assert!(evaluate(&survey_with(installed(&[("mutter", "51.0-1"), ("gnome-shell", "1:51.1-1")]), ""), PACKAGE_SETS)
            .is_empty());
    }

    #[test]
    fn route_flag_leads_to_update_check() {
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(crate::app::route(&args(&[CHECK_FLAG])), crate::app::Route::UpdateCheck { login: false });
        assert_eq!(
            crate::app::route(&args(&[CHECK_FLAG, "--login"])),
            crate::app::Route::UpdateCheck { login: true }
        );
    }

    #[test]
    fn cache_file_names_parsed() {
        assert_eq!(
            parse_cache_file("qt6-base-6.11.2-1-x86_64.pkg.tar.zst"),
            Some(("qt6-base".into(), "6.11.2-1".into()))
        );
        assert_eq!(
            parse_cache_file("gtk3-1:3.24.52-2.1-x86_64_v3.pkg.tar.zst"),
            Some(("gtk3".into(), "1:3.24.52-2.1".into()))
        );
        assert_eq!(parse_cache_file("qt6-base-6.11.2-1-x86_64.pkg.tar.zst.sig"), None);
        assert_eq!(parse_cache_file("download-abc"), None);
    }

    #[test]
    fn checkupdates_db_like_checkupdates() {
        assert_eq!(checkupdates_db("", "", 1000), "/tmp/checkup-db-1000/");
        assert_eq!(checkupdates_db("", "/var/tmp/", 1000), "/var/tmp/checkup-db-1000/");
        assert_eq!(checkupdates_db("/x/db", "/var/tmp", 1000), "/x/db");
    }

    /// About one state — once; a new state — again; at login —
    /// always. Texts without the jargon of system utilities.
    #[test]
    fn once_per_state_and_plain_texts() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let a = evaluate(&survey_with(qt_611(), "qt6-base 6.11.2-1 -> 6.12.0-2\n"), PACKAGE_SETS);
        let b = evaluate(&survey_with(qt_611(), "qt6-base 6.11.2-1 -> 6.12.1-1\n"), PACKAGE_SETS);
        let (fa, fb) = (fingerprint(&a), fingerprint(&b));
        assert_ne!(fa, fb);
        assert!(should_notify(&a, &fa, None, false));
        assert!(!should_notify(&a, &fa, Some(&format!("{fa}\n")), false));
        assert!(should_notify(&b, &fb, Some(&fa), false));
        assert!(should_notify(&a, &fa, Some(&fa), true));
        assert!(!should_notify(&[], "", None, true));
        let text = finding_text(&a[0], false);
        for jargon in ["checkupdates", "IgnorePkg", "QtPrivate", "cachyos"] {
            assert!(!text.contains(jargon), "жаргон «{jargon}»: {text}");
        }
    }

    #[test]
    fn notify_argv_prefers_notify_send() {
        let argv = notify_argv(&|n| n == "notify-send", "T", "B").unwrap();
        assert_eq!(argv[0], "notify-send");
        assert!(argv.contains(&"--urgency=critical".to_string()));
        assert_eq!(&argv[argv.len() - 2..], ["T", "B"]);
        assert_eq!(notify_argv(&|n| n == "dbus-send", "T", "B").unwrap()[0], "dbus-send");
        assert!(notify_argv(&|_| false, "T", "B").is_none());
    }

    #[test]
    fn watch_units_watch_pacman_db_without_rate_limit() {
        let exec = "/usr/lib/rexon/rexon-main-window --update-check";
        let units = watch_units(exec, Some("/usr/lib/rexon/rexon-main-window"));
        let text = |name: &str| units.iter().find(|(n, _)| *n == name).unwrap().1.clone();
        let path = text(WATCH_PATH_UNIT);
        assert!(path.contains("\nPathChanged=/var/lib/pacman/local\n"), "{path}");
        assert!(path.contains("\nUnit=rexon-update-check.service\n"), "{path}");
        assert!(path.contains("\nTriggerLimitIntervalSec=0\n"), "{path}");
        assert!(text(WATCH_TIMER_UNIT).contains("\nOnStartupSec=90s\n"));
        assert!(text(WATCH_TIMER_UNIT).contains("\nOnUnitActiveSec=6h\n"));
        let service = text(WATCH_SERVICE_UNIT);
        assert!(service.contains("\nType=oneshot\n"), "{service}");
        assert!(service.contains("\nKillMode=process\n"), "{service}");
        assert!(service.contains(&format!("\nExecStart={exec}\n")), "{service}");
        assert!(service.contains("\nConditionPathExists=/usr/lib/rexon/rexon-main-window\n"), "{service}");
        assert!(!watch_units(exec, None)[2].1.contains("ConditionPathExists"));
        // Neither root nor a password: the units are user units only.
        for (_, t) in &units {
            for word in ["pkexec", "sudo", "User=", "/etc/"] {
                assert!(!t.contains(word), "«{word}» в юните: {t}");
            }
        }
    }

    #[test]
    fn watch_plan_is_idempotent_removable_and_drops_legacy_units() {
        let exec = "/x/rexon --update-check";
        let none = |_: &str| None;
        let fresh = watch_plan("/h/.config", true, Some(exec), Some("/x/rexon"), &none);
        assert_eq!(fresh.write.len(), 3);
        assert!(fresh.write[0].0.starts_with("/h/.config/systemd/user/"));
        assert_eq!(fresh.systemctl[0], vec!["daemon-reload".to_string()]);
        assert_eq!(fresh.systemctl[1][..2], ["enable".to_string(), "--now".to_string()]);
        let written: std::collections::HashMap<String, String> = fresh.write.iter().cloned().collect();
        let have = |p: &str| written.get(p).cloned();
        assert_eq!(watch_plan("/h/.config", true, Some(exec), Some("/x/rexon"), &have), WatchPlan::default());
        // The launch line changed — rewrite.
        assert_eq!(watch_plan("/h/.config", true, Some("/y/rexon --update-check"), Some("/x/rexon"), &have).write.len(), 1);
        // Not needed or nothing to launch — remove.
        let off = watch_plan("/h/.config", false, Some(exec), None, &have);
        assert_eq!(off.remove.len(), 3);
        assert_eq!(off.systemctl[0][0], "disable");
        assert_eq!(watch_plan("/h/.config", true, None, None, &have).remove.len(), 3);
        assert_eq!(watch_plan("/h/.config", false, None, None, &none), WatchPlan::default());
        // The old AeroThemePlasma units are removed, the new ones are installed.
        let current = |p: &str| written.get(p).cloned().or_else(|| p.contains("rexon-aeroshell-check").then(String::new));
        let migrate = watch_plan("/h/.config", true, Some(exec), Some("/x/rexon"), &current);
        assert_eq!(migrate.remove.len(), 3);
        assert!(migrate.write.is_empty());
        assert_eq!(
            migrate.systemctl,
            vec![
                vec!["disable".to_string(), "--now".into(), "rexon-aeroshell-check.path".into(), "rexon-aeroshell-check.timer".into()],
                vec!["daemon-reload".to_string()],
            ]
        );
    }

    /// A fake pacman: argv (space-separated) → (code, output). What is not in
    /// the table — code 1 without output.
    struct PacmanIo {
        answers: Vec<(&'static str, i32, &'static str)>,
        /// Cache directories: directory → files.
        dirs: Vec<(&'static str, &'static [&'static str])>,
    }
    impl SystemIo for PacmanIo {
        fn exists(&self, _: &str) -> bool { false }
        fn is_file(&self, _: &str) -> bool { false }
        fn is_dir(&self, _: &str) -> bool { false }
        fn is_executable(&self, _: &str) -> bool { false }
        fn realpath(&self, _: &str) -> Option<String> { None }
        fn read_text(&self, _: &str) -> Option<String> { None }
        fn list_dir(&self, d: &str) -> Option<Vec<String>> {
            self.dirs.iter().find(|(k, _)| *k == d).map(|(_, f)| f.iter().map(|s| s.to_string()).collect())
        }
        fn which(&self, n: &str) -> bool { matches!(n, "pacman" | "checkupdates") }
        fn run(&self, argv: &[String], _: Option<u64>, _: bool) -> crate::system_exec::RunOut {
            let key = argv.join(" ");
            let (rc, out) = self.answers.iter().find(|(k, _, _)| *k == key).map(|(_, rc, o)| (*rc, *o)).unwrap_or((1, ""));
            crate::system_exec::RunOut {
                outcome: crate::utils::RunOutcome::Ok,
                returncode: Some(rc),
                stdout: out.into(),
                stderr: String::new(),
            }
        }
        fn env(&self, _: &str) -> String { String::new() }
        fn which_path(&self, _: &str) -> Option<String> { None }
        fn write_text(&self, _: &str, _: &str) -> Result<(), String> { Err("ro".into()) }
        fn copy_file(&self, _: &str, _: &str) -> Result<(), String> { Err("ro".into()) }
    }

    const PLASMA_QG: &str = "plasma plasma-workspace\nplasma kwin\nplasma kwin-x11\n";

    fn plasma_io(q: &'static str, checkupdates: (i32, &'static str), qm: (i32, &'static str)) -> PacmanIo {
        PacmanIo {
            answers: vec![
                ("checkupdates", checkupdates.0, checkupdates.1),
                ("pacman -Q", 0, q),
                ("pacman -Qg kf6 plasma", 0, PLASMA_QG),
                ("pacman -Qmq", qm.0, qm.1),
            ],
            dirs: Vec::new(),
        }
    }

    /// The «Put Previous Versions Back» button: Plasma mismatched (plasma-workspace and
    /// kwin 6.8.1 with kwin-x11 6.7.5 from the repositories) — rollback to 6.7 from TWO
    /// cache directories (`pacman-conf CacheDir`), "was → will be", texts without
    /// commands; without a full cache, on SteamOS and in other stages — no button.
    #[test]
    fn rollback_button_only_when_mixed_and_cache_complete() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let q = "plasma-workspace 6.8.1-1.1\nkwin 6.8.1-2.1\nkwin-x11 6.7.5-3.1\nfirefox 141.0-1\n";
        let mut io = plasma_io(q, (2, ""), (0, "decky-loader\n"));
        io.answers.push(("pacman-conf CacheDir", 0, "/var/cache/pacman/pkg/\n/mnt/cache/\n"));
        io.dirs = vec![
            ("/var/cache/pacman/pkg", &["plasma-workspace-6.7.5-3.1-x86_64_v4.pkg.tar.zst", "kwin-6.8.1-2.1-x86_64.pkg.tar.zst"]),
            ("/mnt/cache", &["kwin-6.7.4-1-x86_64.pkg.tar.zst", "kwin-6.7.5-3.1-x86_64.pkg.tar.zst", "kwin-6.7.5-3.1-x86_64.pkg.tar.zst.sig"]),
        ];
        let advice = advise(&io, PkgFamily::Arch, 1000);
        assert_eq!(advice.findings[0].stage, Stage::DontReboot);
        let offer = rollback_offer(&advice, false).expect("кнопка");
        assert_eq!(offer.set_name, "Plasma");
        assert_eq!(
            offer.items,
            vec![
                RollbackItem {
                    name: "plasma-workspace".into(),
                    from: "6.8.1-1.1".into(),
                    to: "6.7.5-3.1".into(),
                    path: "/var/cache/pacman/pkg/plasma-workspace-6.7.5-3.1-x86_64_v4.pkg.tar.zst".into(),
                },
                RollbackItem {
                    name: "kwin".into(),
                    from: "6.8.1-2.1".into(),
                    to: "6.7.5-3.1".into(),
                    path: "/mnt/cache/kwin-6.7.5-3.1-x86_64.pkg.tar.zst".into(),
                },
            ]
        );
        assert_eq!(rollback_offer(&advice, true), None, "SteamOS");
        let window = advice.text();
        assert!(window.contains("Rexon can put the previous versions back"), "{window}");
        let notice = advice.text_for(Audience::Notification);
        assert!(notice.contains("open Rexon to put the previous versions back"), "{notice}");
        let quiet = advice.text_for(Audience::Window { removable: false, rollback: false });
        assert!(!quiet.contains("previous versions"), "{quiet}");
        let confirm = rollback_confirm_text(&offer);
        assert!(confirm.contains("plasma-workspace 6.8.1 → 6.7.5\nkwin 6.8.1 → 6.7.5"), "{confirm}");
        assert_eq!(rollback_confirm_title(&offer), "Put Back the Previous Versions of Plasma?");
        // The console paragraph («Ctrl+Alt+F3 … sudo pacman -Syu») is for a person at
        // the console with a black screen, it is not about the rollback; there is no rollback command anywhere.
        for t in [&window, &notice, &confirm] {
            for jargon in ["pacman -U", "/var/cache", "cache", ".pkg.tar"] {
                assert!(!t.contains(jargon), "жаргон «{jargon}»: {t}");
            }
        }

        // The previous kwin is not in the cache — neither a button nor words about a rollback.
        io.dirs.truncate(1);
        let partial = advise(&io, PkgFamily::Arch, 1000);
        assert_eq!(rollback_offer(&partial, false), None);
        assert!(!partial.text().contains("previous versions"), "{}", partial.text());
        assert!(!partial.text_for(Audience::Notification).contains("previous versions"));

        // A pending update fixes it — "update", no button.
        let pending = advise(&plasma_io(q, (0, "kwin-x11 6.7.5-3.1 -> 6.8.1-1\n"), (1, "")), PkgFamily::Arch, 1000);
        assert_eq!(pending.findings[0].stage, Stage::UpdateNow);
        assert_eq!(rollback_offer(&pending, false), None);
    }

    /// "Don't update": the previous version is in the cache (exactly the installed one),
    /// but there is nothing to put back — no button; an orphan is not a rollback either.
    #[test]
    fn no_rollback_button_before_update_or_for_orphan() {
        let survey = survey_with(qt_611(), "qt6-base 6.11.2-1 -> 6.12.0-2\n");
        let findings = evaluate(&survey, PACKAGE_SETS);
        assert_eq!(findings[0].stage, Stage::Hold);
        let caches = vec![(PACKAGE_CACHE_DIR.to_string(), vec!["qt6-base-6.11.2-1-x86_64.pkg.tar.zst".to_string()])];
        let rollback: Vec<Vec<RollbackItem>> =
            findings.iter().map(|f| rollback_items(f, &survey.installed, &caches, ROLLBACK_BUTTON_MAX)).collect();
        assert!(!rollback[0].is_empty(), "файл в кэше есть");
        let advice = Advice { findings, rollback, plasma_forks: Vec::new() };
        assert_eq!(rollback_offer(&advice, false), None);
        assert!(!advice.text().contains("previous versions"));
        assert!(!advice.text_for(Audience::Notification).contains("previous versions"));
        // The limits of the button and the console differ: the console — 8, the button — 64.
        const _: () = assert!(ROLLBACK_MAX_PACKAGES < ROLLBACK_BUTTON_MAX);
    }

    /// Cache directories: from `pacman-conf`, without duplicates and odd paths.
    #[test]
    fn cache_dirs_from_pacman_conf() {
        assert_eq!(parse_cache_dirs(""), [PACKAGE_CACHE_DIR]);
        assert_eq!(
            parse_cache_dirs("/var/cache/pacman/pkg/\n/mnt/c/\nrel/\n/a/../b\n/\n/mnt/c\n"),
            [PACKAGE_CACHE_DIR, "/mnt/c"]
        );
        let t = RollbackTarget::Series(Precision::Minor, vec![6, 7]);
        assert!(t.accepts("6.7.5-3.1") && t.accepts("1:6.7.0-1") && !t.accepts("6.8.1-1"));
        assert!(RollbackTarget::Exact("6.11.2-1".into()).accepts("6.11.2-1"));
        assert!(!RollbackTarget::Exact("6.11.2-1".into()).accepts("6.11.2-2"));
    }

    /// Rollback: a privilege session operation with cache paths, the result in words.
    #[test]
    fn rollback_goes_through_privilege_session() {
        use crate::privilege::op::PrivOp;
        use crate::system_exec::RunOut;
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let item = |n: &str| RollbackItem {
            name: n.into(),
            from: "6.12.0-2".into(),
            to: "6.11.2-1".into(),
            path: format!("/var/cache/pacman/pkg/{n}-6.11.2-1-x86_64.pkg.tar.zst"),
        };
        let offer = RollbackOffer { set_name: "Qt".into(), items: vec![item("qt6-base")] };
        let out = |rc: i32, stderr: &str| RunOut {
            outcome: crate::utils::RunOutcome::Ok,
            returncode: Some(rc),
            stdout: String::new(),
            stderr: stderr.into(),
        };
        let mut seen = Vec::new();
        assert_eq!(rollback_with(&offer, &mut |op| { seen.push(op.clone()); Some(out(0, "")) }), Ok(()));
        assert_eq!(
            seen,
            vec![PrivOp::CacheRollback { files: vec!["/var/cache/pacman/pkg/qt6-base-6.11.2-1-x86_64.pkg.tar.zst".into()] }]
        );
        assert_eq!(rollback_with(&offer, &mut |_| Some(out(1, "x\nRexon didn't: y"))), Err("Rexon didn't: y".into()));
        assert_eq!(rollback_with(&offer, &mut |_| Some(out(1, ""))), Err("Rexon couldn't put the previous versions of Qt back.".into()));
        let missing = rollback_with(&offer, &mut |_| None).unwrap_err();
        assert!(!missing.contains("pkexec") && !missing.is_empty(), "{missing}");
        let empty = RollbackOffer { set_name: "Qt".into(), items: Vec::new() };
        let mut called = false;
        assert!(rollback_with(&empty, &mut |_| { called = true; None }).is_err());
        assert!(!called);
        assert_eq!(rollback_title(&offer), "Putting Back the Previous Versions of Qt");
        assert_eq!(
            rollback_done_text(&offer, true),
            "The previous versions of Qt are back. Restarting the computer is safe again. Everything else is in order."
        );
        // A long list — the first ten and "more".
        let many = RollbackOffer { set_name: "Plasma".into(), items: (0..13).map(|i| item(&format!("p{i}"))).collect() };
        let text = rollback_confirm_text(&many);
        assert!(text.contains("p9 6.12.0 → 6.11.2\n…and 3 more") && !text.contains("p10 "), "{text}");
    }

    /// Three Plasma 6.8 cases on fake pacman outputs: a mismatched set
    /// (all in the repositories, no updates), a lagging one waiting for an update, and
    /// an orphan package outside the repositories (Wehrwolfmann's machine 25.10.2026:
    /// `kwin-x11` 6.7.5, `pacman -Si kwin-x11` empty, `pacman -Qm`
    /// names it).
    #[test]
    fn plasma_mix_pending_and_orphan_told_apart() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let q = "plasma-workspace 6.8.1-1.1\nkwin 6.8.1-2.1\nkwin-x11 6.7.5-3.1\nfirefox 141.0-1\n";

        // 1. Mismatched: kwin-x11 is in the repositories (no orphans), nothing pending.
        let mix = advise(&plasma_io(q, (2, ""), (0, "decky-loader\n")), PkgFamily::Arch, 1000);
        assert_eq!(mix.findings.len(), 1, "{:?}", mix.findings);
        assert_eq!(mix.findings[0].stage, Stage::DontReboot);
        assert!(mix.text().contains("parts of Plasma of different versions"), "{}", mix.text());

        // 2. Waiting for an update: kwin-x11 6.8.1 is already in the repositories.
        let pending = advise(&plasma_io(q, (0, "kwin-x11 6.7.5-3.1 -> 6.8.1-1\n"), (1, "")), PkgFamily::Arch, 1000);
        assert_eq!(pending.findings.len(), 1, "{:?}", pending.findings);
        assert_eq!(pending.findings[0].stage, Stage::UpdateNow);
        assert_eq!(pending.title(), "Update the System Once More");

        // 3. Orphan: kwin-x11 is not in the repositories.
        let orphan = advise(&plasma_io(q, (2, ""), (0, "decky-loader\nkwin-x11\n")), PkgFamily::Arch, 1000);
        assert_eq!(orphan.findings.len(), 1, "{:?}", orphan.findings);
        let f = &orphan.findings[0];
        assert_eq!(f.stage, Stage::Orphaned);
        assert_eq!(f.lead, ("plasma-workspace".into(), "6.8.1".into()));
        assert_eq!(f.odd, vec![("kwin-x11".into(), "6.7.5".into(), None)]);
        assert!(f.rollback.is_empty());
        let text = orphan.text();
        assert!(
            text.contains(
                "kwin-x11 has been removed from the repositories and no longer gets updates: it is left over \
                 from an earlier version (kwin-x11 6.7.5 with plasma-workspace 6.8.1)."
            ),
            "{text}"
        );
        assert!(text.contains("The Plasma (X11) session is not supported"), "{text}");
        // For the window — without a command: the «Remove kwin-x11» button removes it.
        assert!(text.contains("If you don't use it, Rexon can remove it."), "{text}");
        assert_eq!(orphan.orphans(), ["kwin-x11"]);
        assert!(removable_orphans(&orphan.findings, true).is_empty(), "SteamOS: удалить нельзя");
        assert_eq!(orphan_remove_button(&orphan.orphans()), "Remove kwin-x11");
        // For the notification — which settings button removes it; on SteamOS (no button) — silent.
        let notice = orphan.text_for(Audience::Notification);
        assert!(notice.contains("remove it in Rexon settings with “Check Now”"), "{notice}");
        let steamos = orphan.text_for(Audience::Window { removable: false, rollback: false });
        assert!(steamos.contains("kwin-x11 has been removed from the repositories") && !steamos.contains("remove it"), "{steamos}");
        for t in [&text, &notice, &steamos] {
            for jargon in ["sudo", "pacman", "-Rns"] {
                assert!(!t.contains(jargon), "жаргон «{jargon}»: {t}");
            }
        }
        // The rescue console keeps the command.
        let console = orphan_advice(f, &|m: &str| m.to_string()).unwrap();
        assert_eq!(console, "If you don't use it, remove the package: sudo pacman -Rns kwin-x11");
        assert!(!text.contains("different versions") && !text.contains("Ctrl+Alt+F3"), "{text}");
        assert_eq!(orphan.title(), "A Package No Longer Gets Updates");
        assert_ne!(orphan.fingerprint(), mix.fingerprint());

        // Orphans are unknown (pacman -Qm failed) — the former "mismatched".
        let unknown = advise(&plasma_io(q, (2, ""), (3, "")), PkgFamily::Arch, 1000);
        assert_eq!(unknown.findings, mix.findings);
    }

    /// A Plasma update is pending, but the orphan will not follow it: not "don't
    /// update" forever, but "remove"; a mismatch of the others — separately
    /// and first.
    #[test]
    fn orphan_does_not_hold_the_update() {
        let mut survey = survey_with(
            installed(&[("plasma-workspace", "6.7.5-3.1"), ("kwin", "6.7.5-3.1"), ("kwin-x11", "6.7.5-3.1")]),
            "plasma-workspace 6.7.5-3.1 -> 6.8.1-1.1\nkwin 6.7.5-3.1 -> 6.8.1-2.1\n",
        );
        survey.groups = ["plasma-workspace", "kwin", "kwin-x11"].iter().map(|p| ("plasma".into(), p.to_string())).collect();
        // Without information about orphans — the former "don't update".
        assert_eq!(evaluate(&survey, PACKAGE_SETS)[0].stage, Stage::Hold);
        survey.foreign = Some(parse_foreign("kwin-x11\n"));
        let found = evaluate(&survey, PACKAGE_SETS);
        assert_eq!(found.iter().map(|f| f.stage).collect::<Vec<_>>(), [Stage::Orphaned]);
        // An orphan of the same series, no update — silent.
        survey.pending.clear();
        assert!(evaluate(&survey, PACKAGE_SETS).is_empty());
        // kwin (in the repositories) lags too — the mismatch first, the orphan second.
        survey.installed[0].1 = "6.8.1-1.1".into();
        let found = evaluate(&survey, PACKAGE_SETS);
        assert_eq!(found.iter().map(|f| f.stage).collect::<Vec<_>>(), [Stage::DontReboot, Stage::Orphaned]);
        assert_eq!(found[0].odd.iter().map(|(n, _, _)| n.as_str()).collect::<Vec<_>>(), ["kwin"]);
        for t in ORPHAN_TEXTS {
            assert!(crate::system_l10n::translation_ok(t, t), "{t}");
        }
    }

    /// Whom the root helper agrees to remove: a member of a "single version" set,
    /// not an anchor; groups — by `pacman -Qg`.
    #[test]
    fn orphan_candidate_only_set_members() {
        let groups = parse_groups(PLASMA_QG);
        assert!(orphan_candidate("kwin-x11", &groups));
        assert!(orphan_candidate("qt6-svg", &[]));
        assert!(orphan_candidate("gnome-shell", &[]));
        // Anchors are not considered orphans.
        for anchor in ["plasma-workspace", "qt6-base", "mutter"] {
            assert!(!orphan_candidate(anchor, &groups), "{anchor}");
        }
        // Not a set member; a member of a "move together" set; the wrong group.
        for other in ["firefox", "glycin", "decky-loader"] {
            assert!(!orphan_candidate(other, &groups), "{other}");
        }
        assert!(!orphan_candidate("kwin-x11", &[]));
    }

    /// An orphan the session depends on: the window session and the default login screen
    /// session; the window says in advance where to remove it from.
    #[test]
    fn orphan_session_in_use_and_login_default() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let k = vec!["kwin-x11".to_string()];
        assert_eq!(orphan_in_use(&k, "X11", "kde").map(|s| s.package), Some("kwin-x11"));
        assert!(orphan_in_use(&k, "wayland", "kde").is_none());
        assert!(orphan_in_use(&k, "x11", "xfce").is_none());
        assert!(orphan_in_use(&["qt6-svg".to_string()], "x11", "kde").is_none());
        for v in ["plasmax11", "plasmax11.desktop", "/usr/share/xsessions/plasmax11.desktop", " plasmax11 "] {
            assert_eq!(session_file_name(v), "plasmax11", "{v}");
        }
        assert!(orphan_login_default(&k, &["/usr/share/xsessions/plasmax11.desktop".into()]).is_some());
        assert!(orphan_login_default(&k, &["plasma".into(), "plasma.desktop".into()]).is_none());

        // Reading order: the directory by name, then the file; the later one overrides.
        let files: std::collections::HashMap<&str, &str> = [
            ("/usr/lib/sddm/sddm.conf.d/default.conf", "[Autologin]\nSession=plasmax11\n"),
            ("/etc/sddm.conf.d/kde_settings.conf", "[Autologin]\nSession=plasma\n"),
            ("/etc/plasmalogin.conf", "[Autologin]\nSession=\n"),
            ("/var/lib/sddm/state.conf", "[Last]\nSession=/usr/share/wayland-sessions/plasma.desktop\n"),
        ]
        .into_iter()
        .collect();
        let read = |p: &str| files.get(p).map(|t| t.to_string());
        let list = |d: &str| {
            files.keys().filter_map(|p| p.strip_prefix(&format!("{d}/")).map(str::to_string)).collect::<Vec<_>>()
        };
        assert_eq!(login_default_sessions(&read, &list), ["plasma", "/usr/share/wayland-sessions/plasma.desktop"]);

        // The window's decision: a Plasma X11 session — text instead of the button.
        let f = Finding {
            set: PACKAGE_SETS[2],
            stage: Stage::Orphaned,
            lead: ("plasma-workspace".into(), "6.8.1".into()),
            odd: vec![("kwin-x11".into(), "6.7.5".into(), None)],
            rollback: Vec::new(),
        };
        let blocked = orphan_offer(std::slice::from_ref(&f), false, "x11", "kde");
        let OrphanOffer::Blocked(text) = &blocked else { panic!("{blocked:?}") };
        assert_eq!(
            text,
            "You are using Plasma (X11) right now, and it runs on this package. Sign in to Plasma (Wayland) and remove it from there."
        );
        assert_eq!(orphan_offer(std::slice::from_ref(&f), false, "wayland", "kde"), OrphanOffer::Remove(k.clone()));
        assert_eq!(orphan_offer(std::slice::from_ref(&f), true, "x11", "kde"), OrphanOffer::Nothing);
        assert_eq!(orphan_offer(&[], false, "x11", "kde"), OrphanOffer::Nothing);
        // The helper's refusal texts are the same, by package name.
        let s = orphan_session_of("kwin-x11").unwrap();
        assert_eq!(crate::privilege::client::key_text("refused.orphan-in-use.kwin-x11"), orphan_in_use_text(s));
        assert_eq!(crate::privilege::client::key_text("refused.orphan-login-default.kwin-x11"), orphan_login_default_text(s));
        // The "the login screen opens the session" refusal is recognised by the window — and only it.
        let why = crate::privilege::client::key_text("refused.orphan-login-default.kwin-x11");
        assert_eq!(login_default_successor(&k, &why).map(|p| p.wayland), Some("plasma"));
        assert!(login_default_successor(&k, &orphan_in_use_text(s)).is_none());
        assert!(login_default_successor(&["qt6-svg".to_string()], &why).is_none());
        // Every table entry is a member of a guard set, and each of its sessions
        // has a Wayland counterpart (the «Make … the default session» button).
        for s in ORPHAN_SESSIONS {
            assert!(orphan_candidate(s.package, &parse_groups(PLASMA_QG)), "{}", s.package);
            for x11 in s.sessions {
                let pair = crate::system_plan::session_successor(x11).unwrap_or_else(|| panic!("{x11}"));
                assert_eq!((pair.x11_name, pair.wayland_name), (s.session_name, s.instead), "{x11}");
            }
        }
    }

    /// When the window shows up: by the button — always; rollback — by the window
    /// fingerprint (after the "open Rexon" notification — without fail); a message without
    /// a button — by the shared one; an offer to remove an orphan by itself — never.
    #[test]
    fn window_after_notice_only_when_it_can_act() {
        assert!(window_should_show(true, false, true, true, true));
        assert!(window_should_show(true, false, false, true, true));
        // Rollback: after the notification — yes, already shown — no.
        assert!(window_should_show(false, true, false, true, false));
        assert!(!window_should_show(false, true, false, true, true));
        // A message without a button — once, shared with the notification.
        assert!(!window_should_show(false, false, false, true, false));
        assert!(window_should_show(false, false, false, false, true));
        // Removing an orphan without a request — neither before nor after the notification.
        assert!(!window_should_show(false, false, true, false, false));
        assert!(!window_should_show(false, false, true, true, false));
        // Rollback and orphan together — the rollback window (the orphan — after its result).
        assert!(window_should_show(false, true, true, true, false));
        assert_eq!(window_stamp_path("/h/.cache/"), "/h/.cache/rexon/update-guard-shown");
        assert_ne!(window_stamp_path("/h/.cache"), stamp_path("/h/.cache"));
    }

    /// Removing an orphan: a privilege session operation, the result in words.
    #[test]
    fn remove_orphans_goes_through_privilege_session() {
        use crate::privilege::op::PrivOp;
        use crate::system_exec::RunOut;
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let names = vec!["kwin-x11".to_string()];
        let out = |rc: i32, stdout: &str, stderr: &str| RunOut {
            outcome: crate::utils::RunOutcome::Ok,
            returncode: Some(rc),
            stdout: stdout.into(),
            stderr: stderr.into(),
        };
        let mut seen = Vec::new();
        let ok = remove_orphans_with(&names, &mut |op| {
            seen.push(op.clone());
            Some(out(0, "removing kwin-x11...\n", ""))
        });
        assert_eq!(ok, Ok(()));
        assert_eq!(seen, vec![PrivOp::OrphanRemove { names: names.clone() }]);
        // A helper refusal — its ready text.
        let refused = remove_orphans_with(&names, &mut |_| Some(out(1, "", "Rexon didn't remove the package: x")));
        assert_eq!(refused, Err("Rexon didn't remove the package: x".to_string()));
        // pacman failed — a general result without its lines.
        let failed = remove_orphans_with(&names, &mut |_| Some(out(1, "error: failed to commit transaction\n", "")));
        assert_eq!(failed, Err("Rexon couldn't remove kwin-x11.".to_string()));
        // No helper — there is no former path: say so, without pkexec.
        let missing = remove_orphans_with(&names, &mut |_| None).unwrap_err();
        assert!(!missing.contains("pkexec") && !missing.is_empty(), "{missing}");
        // Empty — not a single operation.
        let mut called = false;
        assert!(remove_orphans_with(&[], &mut |_| { called = true; None }).is_err());
        assert!(!called);
        // Window labels.
        assert_eq!(orphan_remove_title(&names), "Removing kwin-x11");
        assert_eq!(orphan_removed_text(&names, true), "Removed: kwin-x11. Everything else is in order.");
        assert_eq!(orphan_removed_text(&names, false), "Removed: kwin-x11.");
        assert_eq!(removable_orphans(&[], false), Vec::<String>::new());
    }

    /// The check through fake I/O: which commands, without root.
    #[test]
    fn survey_asks_pacman_and_checkupdates_only() {
        use crate::system_exec::RunOut;
        use std::cell::RefCell;
        struct Io {
            calls: RefCell<Vec<Vec<String>>>,
        }
        impl SystemIo for Io {
            fn exists(&self, _: &str) -> bool { false }
            fn is_file(&self, _: &str) -> bool { false }
            fn is_dir(&self, p: &str) -> bool { p == "/tmp/checkup-db-1000/" }
            fn is_executable(&self, _: &str) -> bool { false }
            fn realpath(&self, _: &str) -> Option<String> { None }
            fn read_text(&self, _: &str) -> Option<String> { None }
            fn list_dir(&self, _: &str) -> Option<Vec<String>> { None }
            fn which(&self, n: &str) -> bool { matches!(n, "pacman" | "checkupdates") }
            fn run(&self, argv: &[String], _: Option<u64>, _: bool) -> RunOut {
                self.calls.borrow_mut().push(argv.to_vec());
                let stdout = match argv.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
                    ["checkupdates"] => "gdk-pixbuf2 2.44.7-1.1 -> 2.44.8-1.1\n",
                    ["pacman", "-Q"] => "gdk-pixbuf2 2.44.7-1.1\nglycin 2.1.5-2.1\n",
                    ["pacman", "-Qg", ..] => "",
                    ["pacman", "-Sl", "--dbpath", _] => "extra glycin 2.2.2-1 [installed: 2.1.5-2.1]\n",
                    _ => "",
                };
                RunOut { outcome: crate::utils::RunOutcome::Ok, returncode: Some(0), stdout: stdout.into(), stderr: String::new() }
            }
            fn env(&self, _: &str) -> String { String::new() }
            fn which_path(&self, _: &str) -> Option<String> { None }
            fn write_text(&self, _: &str, _: &str) -> Result<(), String> { Err("ro".into()) }
            fn copy_file(&self, _: &str, _: &str) -> Result<(), String> { Err("ro".into()) }
        }
        let io = Io { calls: RefCell::new(Vec::new()) };
        let advice = advise(&io, PkgFamily::Arch, 1000);
        assert_eq!(advice.findings.len(), 1);
        assert_eq!(advice.findings[0].set.id, "pixbuf");
        for call in io.calls.borrow().iter() {
            // `pacman-conf CacheDir` — read-only: where the cache for the rollback is.
            assert!(matches!(call[0].as_str(), "pacman" | "checkupdates" | "pacman-conf"), "{call:?}");
            assert!(!call.iter().any(|a| a == "-S" || a == "-Sy" || a == "-Syu" || a == "-U"), "{call:?}");
        }
        // Not Arch — not a single command.
        let io = Io { calls: RefCell::new(Vec::new()) };
        assert_eq!(advise(&io, PkgFamily::Debian, 1000), Advice::default());
        assert!(io.calls.borrow().is_empty());
    }
}
