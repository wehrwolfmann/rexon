//! The `.rxn` version 5 manifest — model, validation, in-memory v3/v4 → v5
//! translation and the rules for choosing the payload per desktop.
//!
//! The idea (project decision 25.10.2026): a package stores not "files for a
//! desktop" but a description of the look and the author's native variants for
//! specific **consumers** (whoever reads the format: `aurorae`, `xfwm4`,
//! `openbox-3`, `gtk`…). What to install on the current desktop is decided by
//! Rexon: native variant → compatible → generated from the model (marked
//! "exact / approximate") → substitute by meaning → honest refusal.
//!
//! The module is data and pure functions only: no files, no processes, no
//! network. Install and rollback are built by the next layer on top of the
//! existing paths (`ext_manager`, `appearance_installer`, `rxn_v4_installer`).
//!
//! What is here:
//!   * [`Package`] and nested types — the serde model of the v5 manifest;
//!   * [`parse`] — parsing any supported manifest (3, 4, 5) into [`Package`]
//!     with full validation; v3/v4 are translated in memory, their schema is
//!     checked by the existing code (`constants::is_valid_type`,
//!     `ext_manager::check_rexon_compat`, `manifest_v4::validate_v4_manifest`);
//!   * [`choose`]/[`choose_all`] — payload choice rules per session;
//!   * dependencies from three sources ([`DepSource`]) with a fallback source
//!     ([`PackageDep::resolve`]) and a before/after snapshot check "no
//!     substitution by a provider, nothing forbidden appeared"
//!     ([`verify_dependency`], [`verify_package_change`]); package
//!     preconditions ([`unmet_preconditions`]);
//!   * classes of executable content ([`scan_executables`],
//!     [`check_contents`]) and the ban on native compositor configs;
//!   * licenses ([`license_class`], [`check_publishable`],
//!     [`derived_attribution`]);
//!   * scrubbing personal data on export ([`scrub_text`],
//!     [`find_secrets`], [`never_export`]);
//!   * an optional Steam Workshop section ([`Workshop`]) — without app_id and
//!     without uploading.
//!
//! Strings for the person are English source text via `i18n::tr/trf`.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::constants;
use crate::ext_manager;
use crate::i18n::{tr, trf};
use crate::system::PkgFamily;
use crate::system_plan;

/// `format_version` of the v5 manifest. `4` is taken by adaptive video
/// wallpapers (`manifest_v4::V4_FORMAT_VERSION`), `3` — the old single-type format.
pub const V5_FORMAT_VERSION: u32 = 5;

// ============================================================================
// Validation error
// ============================================================================

/// One manifest error: where (`items[2].native`) and what is wrong — as text
/// for the person in the interface language.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ManifestError {
    /// Path to the field in the manifest; empty — the manifest as a whole.
    pub field: String,
    pub message: String,
}

impl ManifestError {
    fn new(field: impl Into<String>, message: String) -> Self {
        Self { field: field.into(), message }
    }
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.field.is_empty() {
            f.write_str(&self.message)
        } else {
            write!(f, "{}: {}", self.field, self.message)
        }
    }
}

/// All errors, one line each — for the log and the error window.
pub fn errors_text(errors: &[ManifestError]) -> String {
    errors.iter().map(ToString::to_string).collect::<Vec<_>>().join("\n")
}

// ============================================================================
// Model
// ============================================================================

/// Text in several languages: `{"en": "…", "ru": "…"}` or just a string
/// (that is how v3 and short packages write it).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Localized {
    Plain(String),
    ByLang(BTreeMap<String, String>),
}

impl Default for Localized {
    fn default() -> Self {
        Localized::Plain(String::new())
    }
}

impl Localized {
    /// Text for language `lang` (`ru`, `pt_BR`): exact match → language without
    /// region → English → any non-empty.
    pub fn get(&self, lang: &str) -> &str {
        match self {
            Localized::Plain(s) => s,
            Localized::ByLang(m) => {
                let base = lang.split(['_', '-', '.']).next().unwrap_or(lang);
                [lang, base, "en"]
                    .iter()
                    .find_map(|k| m.get(*k).filter(|s| !s.trim().is_empty()))
                    .or_else(|| m.values().find(|s| !s.trim().is_empty()))
                    .map(String::as_str)
                    .unwrap_or("")
            }
        }
    }

    fn is_blank(&self) -> bool {
        self.get("en").trim().is_empty()
    }
}

/// Package author.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Author {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// A third-party part of the package (`sources`) or the source of a derivative (`derived_from`).
///
/// A source with an `id` is not just attribution but **pinned content**
/// («Distro Looks»): elements refer to its files by the path
/// `sources/<id>/…`, and on install Rexon puts there exactly what is pinned.
/// The pin is one of three:
///   * `rev` — a git commit (40 or 64 hex digits), `url` — the repository URL;
///   * `sha256` — hash of the archive or binary package at `url`; a mismatch
///     is a refusal, not "I'll download whatever is there";
///   * `package` — a dependency role from `requires.packages`: the files are
///     brought by a system package (BeautyLine icons from the repository), the
///     path `sources/<id>/usr/share/…` means a file of that package.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Source {
    /// What was taken: "Nord palette", "Arc frames".
    pub what: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// SPDX license expression of the source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    /// Source name for `sources/<id>/…` paths.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Pinned git commit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rev: Option<String>,
    /// SHA-256 hash of the downloaded archive or package.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    /// Dependency role that brings the source's files.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
}

/// Directory in the package under which the pinned sources' files lie.
pub const SOURCES_DIR: &str = "sources";

impl Source {
    /// The source is downloaded on install (git or an archive by hash).
    pub fn is_fetched(&self) -> bool {
        self.rev.is_some() || self.sha256.is_some()
    }
}

/// Kind of a setting Rexon draws itself (like Wallpaper Engine properties).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PropertyType {
    Color,
    Slider,
    Combo,
    Toggle,
    Text,
}

/// A package property. In the model it is referenced as `${prop.<name>}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Property {
    #[serde(rename = "type")]
    pub kind: PropertyType,
    pub default: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<Localized>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    /// List options: a string (`"vapor"`) or `{"value": "vgui", "label":
    /// {…}}` with a label for the person.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<PropOption>,
    /// Where else the value can be taken from (`from-wallpaper`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allow: Vec<String>,
}

/// An option of a list property. A bare string is the old form (the value
/// itself serves as the label), an object — the value and a label in languages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PropOption {
    Plain(String),
    Labeled { value: String, label: Localized },
}

impl PropOption {
    pub fn value(&self) -> &str {
        match self {
            PropOption::Plain(v) | PropOption::Labeled { value: v, .. } => v,
        }
    }

    /// Label for language `lang`; without a label — the value itself.
    pub fn label(&self, lang: &str) -> &str {
        match self {
            PropOption::Labeled { label, .. } if !label.is_blank() => label.get(lang),
            other => other.value(),
        }
    }
}

impl Property {
    /// Values of the list options in manifest order.
    pub fn option_values(&self) -> Vec<&str> {
        self.options.iter().map(PropOption::value).collect()
    }
}

/// Property values chosen by the person: name → value. What is missing is
/// taken from the property's `default`.
pub type PropValues = BTreeMap<String, Value>;

/// A condition on property values (`when`): property name → a value or a list
/// of allowed values; all pairs must match. Only for list properties and
/// toggles.
pub type When = BTreeMap<String, Value>;

/// A permission the element asks the person for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Permission {
    /// Run code or commands (Conky, JS, QML, scripts).
    Exec,
    /// Access the network.
    Network,
}

/// One package element: an abstract kind + variants for consumers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Item {
    /// Abstract kind from the closed list [`KINDS`].
    pub kind: String,
    /// Description file (level 2: generation), `model/decoration.json`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// The author's native variants: consumer → path in the archive.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub native: BTreeMap<String, String>,
    /// Whether the author allowed generation for the other consumers.
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub generate: bool,
    /// Without this element the whole package doesn't install.
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub required: bool,
    /// The element's meaning for substitution by meaning (`weather`, `clock`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// What to substitute on another desktop: consumer → path in the archive.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub substitute: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub permissions: Vec<Permission>,
    /// Kind `ref`: id of a Workshop item installed together with the set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workshop: Option<u64>,
    /// `plasmoid` only: showing it as a separate window outside Plasma
    /// (`plasmawindowed`). A model for the future, Rexon doesn't execute it yet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub windowed: Option<Windowed>,
    /// A warning for the card before installing ("on a laptop with two GPUs the
    /// screen may stay black").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<Localized>,
    /// The element only for these property values: `{"variant": "vgui"}`.
    /// No match — [`Choice::Off`], not "skipped".
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub when: When,
}

/// Session type for showing a plasmoid as a window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SessionKind {
    X11,
    Wayland,
}

/// Window layer of a plasmoid outside Plasma.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HintLayer {
    /// Below program windows, above the wallpaper.
    Below,
    Normal,
    /// A desktop window (`_NET_WM_WINDOW_TYPE_DESKTOP`).
    Desktop,
}

fn x11_only() -> Vec<SessionKind> {
    vec![SessionKind::X11]
}

/// The plasmoid's `windowed` block (plasmawindowed research 25.10): a native
/// plasmoid is Plasma only, a window outside it — by a flag in Rexon settings
/// and only when plasma-workspace is already installed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Windowed {
    #[serde(default)]
    pub allowed: bool,
    /// Where the author tested the window. Default — X11 only.
    #[serde(default = "x11_only")]
    pub sessions: Vec<SessionKind>,
    /// `[width, height]`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_size: Option<[u32; 2]>,
    /// `[x, y, width, height]`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_geometry: Option<[i32; 4]>,
    /// What is needed beyond libplasma (`plasma5support`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub needs: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint_layer: Option<HintLayer>,
}

/// Plasmoid roles meaningless outside Plasma: they control the shell itself
/// (tasks, virtual desktops, tray, menu). Outside Plasma — an honest refusal:
/// neither a window nor a mandatory substitute.
pub const PLASMA_ONLY_ROLES: &[&str] = &[
    "taskmanager",
    "pager",
    "systemtray",
    "appmenu",
    "kickoff",
    "kicker",
    "minimizeall",
    "showdesktop",
];

pub fn is_plasma_only_role(role: &str) -> bool {
    PLASMA_ONLY_ROLES.contains(&role)
}

fn yes() -> bool {
    true
}

fn is_true(b: &bool) -> bool {
    *b
}

/// Where a dependency is installed from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DepSource {
    /// Official distribution repositories (names per family).
    #[default]
    Repo,
    /// AUR — Arch only and only with an explicit `aur/` prefix.
    Aur,
    /// extensions.gnome.org: `uuid` and a GNOME Shell version check. Installed
    /// right away, by the shell's dialog (`gnome_ext_section::install_dependency`).
    GnomeExtensions,
}

impl DepSource {
    /// Whether Rexon installs such a dependency itself, without a "Yes/No"
    /// question (rule of 24.10: repositories — yes, the protection is the password
    /// dialog; AUR — never automatically; EGO — yes, decision of 25.10 for Dash to
    /// Panel/ArcMenu).
    pub fn installs_automatically(self) -> bool {
        !matches!(self, DepSource::Aur)
    }

    pub fn key(self) -> &'static str {
        match self {
            DepSource::Repo => "repo",
            DepSource::Aur => "aur",
            DepSource::GnomeExtensions => "gnome-extensions",
        }
    }
}

/// Key of AUR package names in [`PackageDep::names`].
pub const AUR_NAMES_KEY: &str = "aur";

/// A package dependency. Described by a role, names per distribution family:
/// `{"role": "gtk3-adwaita-base", "arch": ["adw-gtk-theme"],
/// "fedora": ["adw-gtk3-theme"], "debian": null}`. `null` — "not available in
/// this family", then the element is skipped there with a reason.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PackageDep {
    pub role: String,
    /// Which element kinds need it (empty — the whole package).
    #[serde(rename = "for", default, skip_serializing_if = "Vec::is_empty")]
    pub for_kinds: Vec<String>,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub required: bool,
    #[serde(default)]
    pub source: DepSource,
    /// `gnome-extensions`: extension uuid (`dash-to-panel@jderose9.github.com`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uuid: Option<String>,
    /// Packages that must NOT APPEAR as a result of installing this dependency
    /// (lesson of 25.10: `gamescope-session-steam-git` from AUR was substituted by
    /// a CachyOS package and pulled in `jupiter-hw-support`). What was installed
    /// before is not a violation: [`verify_dependency`] compares two snapshots.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub forbid: Vec<String>,
    /// Only for these consumers of the chosen variant (empty — for any):
    /// `user-theme` is needed by the `gnome-shell` theme, but not by the Plasma
    /// theme of the same `shell-theme` element.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub consumers: Vec<String>,
    /// Needed only for these property values: `oxygen-cursors` — only with
    /// `{"variant": "vgui"}` (the KDE_Classic cursor of the VGUI2 style).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub when: When,
    /// Fallback sources in order after `source`: `{"source": "repo",
    /// "fallback": ["aur"]}` — "the repository first, if not there — AUR"
    /// (BeautyLine is in the CachyOS repository, on plain Arch — only in AUR).
    /// Only `repo` → `aur` is allowed; AUR names go under the `aur` key. Which
    /// source is taken is decided by [`PackageDep::resolve`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fallback: Vec<DepSource>,
    /// Names per family (`arch`, `debian`…) and `aur`; the value is a list of
    /// names or `null`.
    #[serde(flatten)]
    pub names: BTreeMap<String, Value>,
}

/// A dependency with a chosen source ([`PackageDep::resolve`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResolvedDep {
    pub source: DepSource,
    /// Bare package names (empty for EGO, installed by `uuid`).
    pub names: Vec<String>,
}

impl ResolvedDep {
    /// Names for the install command: for AUR — `aur/<name>` (see
    /// [`PackageDep::install_names`]).
    pub fn install_names(&self) -> Vec<String> {
        match self.source {
            DepSource::Aur => self.names.iter().map(|n| format!("aur/{n}")).collect(),
            _ => self.names.clone(),
        }
    }

    /// Whether Rexon installs the choice itself ([`DepSource::installs_automatically`]):
    /// a fallback to AUR is never installed automatically.
    pub fn installs_automatically(&self) -> bool {
        self.source.installs_automatically()
    }
}

impl PackageDep {
    /// Package names for a family. `None` — the dependency is not available in
    /// this family (not specified or `null`), empty never happens (validation
    /// won't let it through). Only the main `source`; fallbacks — [`PackageDep::resolve`].
    pub fn names_for(&self, family: PkgFamily) -> Option<Vec<String>> {
        self.names_from(self.source, family)
    }

    fn names_from(&self, source: DepSource, family: PkgFamily) -> Option<Vec<String>> {
        let key = match source {
            DepSource::Repo => family.as_str(),
            DepSource::Aur if family == PkgFamily::Arch => AUR_NAMES_KEY,
            _ => return None,
        };
        let list = self.names.get(key)?.as_array()?;
        Some(list.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
    }

    /// Sources in order: `source`, then `fallback`.
    pub fn sources_in_order(&self) -> Vec<DepSource> {
        std::iter::once(self.source).chain(self.fallback.iter().copied()).collect()
    }

    /// Pick a source from repository data. `in_repos(name)` — whether the package
    /// is in the system's enabled repositories (the caller collects this up front,
    /// `pacman -Si` and the like; there are no processes here). A repository fits
    /// if it has ALL of the family's names; AUR — Arch only and unchecked
    /// (`in_repos` doesn't know about it); EGO — always. `None` — no source
    /// provides the package: the element is skipped with a reason rather than
    /// installing "whatever there is".
    pub fn resolve(&self, family: PkgFamily, in_repos: &dyn Fn(&str) -> bool) -> Option<ResolvedDep> {
        for source in self.sources_in_order() {
            match source {
                DepSource::Repo => {
                    if let Some(names) = self.names_from(source, family) {
                        if names.iter().all(|n| in_repos(n)) {
                            return Some(ResolvedDep { source, names });
                        }
                    }
                }
                DepSource::Aur => {
                    if let Some(names) = self.names_from(source, family) {
                        return Some(ResolvedDep { source, names });
                    }
                }
                DepSource::GnomeExtensions => return Some(ResolvedDep { source, names: Vec::new() }),
            }
        }
        None
    }

    /// Names for the install command. AUR — always `aur/<name>`: that way `paru`
    /// won't take a same-named package or a "Provides" from an enabled repository
    /// (lesson of 25.10, `gamescope-session-cachyos`).
    pub fn install_names(&self, family: PkgFamily) -> Option<Vec<String>> {
        let names = self.names_for(family)?;
        Some(match self.source {
            DepSource::Aur => names.into_iter().map(|n| format!("aur/{n}")).collect(),
            _ => names,
        })
    }
}

/// The `requires` section.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Requires {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub packages: Vec<PackageDep>,
    /// Consumer versions the NATIVE variants were written for:
    /// `{"cinnamon": ">=6.0"}`. Doesn't concern generation.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub targets: BTreeMap<String, String>,
    /// Packages that must not APPEAR as a result of installing the whole package
    /// (not just one dependency). What was installed before doesn't matter:
    /// `plasma-login-manager` may be the person's login screen, but the Garuda
    /// look doesn't bring it. Checked by [`verify_package_change`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub forbid: Vec<String>,
    /// What must hold on the system BEFORE installing ([`unmet_preconditions`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub preconditions: Vec<Precondition>,
}

impl Requires {
    fn is_empty(&self) -> bool {
        self.packages.is_empty() && self.targets.is_empty() && self.forbid.is_empty() && self.preconditions.is_empty()
    }
}

/// A package precondition: who on the system covers the name `name` (a package
/// or its Provides). `{"name": "libplasma", "owners": ["libplasma"]}` — if
/// libplasma is installed, it must be libplasma itself and not a fork
/// (`aeroshell-libplasma-git` substitutes it, and Plasma appearance doesn't
/// apply on it). At least one of `owners`, `not_owners`, `installed` is needed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Precondition {
    pub name: String,
    /// Which real packages may cover the name (empty — any).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub owners: Vec<String>,
    /// Which packages may not cover the name.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub not_owners: Vec<String>,
    /// `true` — the name must be on the system, `false` — it must not; no field —
    /// doesn't matter (only owners are checked, if the name is present).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installed: Option<bool>,
    /// Explanation for the person of why the package won't install.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<Localized>,
}

impl Precondition {
    /// Whether the condition is violated: `found` — who covers the name now (`None` — nothing).
    fn is_unmet(&self, found: Option<&str>) -> bool {
        match (self.installed, found) {
            (Some(true), None) | (Some(false), Some(_)) => true,
            (_, Some(owner)) => {
                (!self.owners.is_empty() && !self.owners.iter().any(|o| o == owner))
                    || self.not_owners.iter().any(|o| o == owner)
            }
            (_, None) => false,
        }
    }
}

/// A violated precondition and what was found on the system.
#[derive(Debug, Clone, PartialEq)]
pub struct UnmetPrecondition<'a> {
    pub precondition: &'a Precondition,
    /// Who covers the name (`None` — not installed).
    pub found: Option<String>,
}

impl UnmetPrecondition<'_> {
    /// Text for the person: the author's explanation, otherwise a generic one.
    pub fn text(&self, lang: &str) -> String {
        if let Some(m) = self.precondition.message.as_ref().filter(|m| !m.is_blank()) {
            return m.get(lang).to_string();
        }
        let name = &self.precondition.name;
        match (&self.found, self.precondition.installed) {
            (None, _) => trf("This package needs %1, and it is not installed.", &[name]),
            (Some(_), Some(false)) => trf("This package cannot be installed while %1 is installed.", &[name]),
            (Some(owner), _) => trf(
                "%1 is provided here by %2, and this package does not work with it.",
                &[name, owner],
            ),
        }
    }
}

/// The `privacy` section: what was scrubbed on «Save my desktop».
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Privacy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exported_by: Option<String>,
    #[serde(default)]
    pub scrubbed: bool,
    /// List of what was scrubbed — the person sees it before saving.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub removed: Vec<String>,
}

/// Workshop item visibility (as in Steam UGC).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Visibility {
    Public,
    FriendsOnly,
    #[default]
    Private,
    Unlisted,
}

/// Optional Steam Workshop section. There is no uploading, and app_id is never
/// written into the package (decision of 25.10: Rexon has no AppID yet, only
/// the program knows it). The section is laid down in advance so the format
/// doesn't have to change later.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Workshop {
    /// Id of the published item (PublishedFileId), if already uploaded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published_file_id: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// Preview for the Workshop (path in the archive); none — the common `preview`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
    #[serde(default)]
    pub visibility: Visibility,
}

/// A `.rxn` v5 package — after [`parse`] always in this form, whatever version
/// it came from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Package {
    pub format_version: u32,
    pub id: String,
    #[serde(default = "default_version")]
    pub version: String,
    pub name: Localized,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<Localized>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub authors: Vec<Author>,
    /// SPDX expression. Mandatory for publishing and export, not for installing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<Source>,
    /// Source of a derivative (import of a foreign theme → model → generation).
    /// Written by Rexon itself ([`derived_attribution`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub derived_from: Vec<Source>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rexon_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
    /// Previews per consumer or desktop: `{"xfce": "previews/xfce.png"}`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub previews: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// Properties in manifest order (the order is the display order).
    #[serde(default, with = "ordered_properties", skip_serializing_if = "Vec::is_empty")]
    pub properties: Vec<(String, Property)>,
    pub items: Vec<Item>,
    #[serde(default, skip_serializing_if = "Requires::is_empty")]
    pub requires: Requires,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub privacy: Option<Privacy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workshop: Option<Workshop>,
    /// Which format version the package was read from (3, 4 or 5). Not written to
    /// the file: after reading the package is always v5.
    #[serde(skip)]
    pub source_format: u32,
}

fn default_version() -> String {
    "0.0.0".to_string()
}

/// Properties — a JSON object with key order (`serde_json` with
/// `preserve_order`) ↔ a list of pairs.
mod ordered_properties {
    use super::Property;
    use serde::de::Error as _;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use serde_json::{Map, Value};

    pub fn serialize<S: Serializer>(v: &[(String, Property)], s: S) -> Result<S::Ok, S::Error> {
        let mut m = Map::new();
        for (k, p) in v {
            m.insert(k.clone(), serde_json::to_value(p).map_err(serde::ser::Error::custom)?);
        }
        m.serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<(String, Property)>, D::Error> {
        let m = Map::<String, Value>::deserialize(d)?;
        m.into_iter()
            .map(|(k, v)| {
                let p = Property::deserialize(v).map_err(|e| D::Error::custom(format!("{k}: {e}")))?;
                Ok((k, p))
            })
            .collect()
    }
}

impl Package {
    /// Property by name.
    pub fn property(&self, name: &str) -> Option<&Property> {
        self.properties.iter().find(|(k, _)| k == name).map(|(_, p)| p)
    }

    /// Dependencies needed by an element of kind `kind` (its own and
    /// package-wide) — all possible ones, ignoring `consumers` and `when`.
    pub fn deps_for(&self, kind: &str) -> Vec<&PackageDep> {
        self.requires
            .packages
            .iter()
            .filter(|d| d.for_kinds.is_empty() || d.for_kinds.iter().any(|k| k == kind))
            .collect()
    }

    /// Dependencies of element `kind` whose variant was chosen for consumer
    /// `consumer` (takes the `consumers` field into account), with default
    /// property values.
    pub fn deps_for_choice(&self, kind: &str, consumer: &str) -> Vec<&PackageDep> {
        self.deps_for_choice_with(kind, consumer, &PropValues::new())
    }

    /// The same with chosen property values: a dependency with `when` is
    /// included only if the condition matched.
    pub fn deps_for_choice_with(&self, kind: &str, consumer: &str, values: &PropValues) -> Vec<&PackageDep> {
        self.deps_for(kind)
            .into_iter()
            .filter(|d| d.consumers.is_empty() || d.consumers.iter().any(|c| c == consumer))
            .filter(|d| self.when_holds(&d.when, values))
            .collect()
    }

    /// Property value: the one chosen by the person, otherwise `default`.
    pub fn prop_value<'a>(&'a self, name: &str, values: &'a PropValues) -> Option<&'a Value> {
        values.get(name).or_else(|| self.property(name).map(|p| &p.default))
    }

    /// Whether the `when` condition matched (empty — always).
    pub fn when_holds(&self, when: &When, values: &PropValues) -> bool {
        when.iter().all(|(name, want)| {
            let Some(have) = self.prop_value(name, values) else { return false };
            match want {
                Value::Array(list) => list.contains(have),
                v => v == have,
            }
        })
    }

    /// Pinned source by `id`.
    pub fn source(&self, id: &str) -> Option<&Source> {
        self.sources.iter().find(|s| s.id.as_deref() == Some(id))
    }

    /// Whether the package installs anything executable at all (by declared kind
    /// and permissions; the actual content is checked by [`check_contents`]).
    pub fn has_executable(&self) -> bool {
        self.items
            .iter()
            .any(|i| kind_is_executable(&i.kind) || i.permissions.contains(&Permission::Exec))
    }
}

// ============================================================================
// Kinds and consumers
// ============================================================================

/// The closed list of v5 abstract kinds. A new kind is a line here (and, if it
/// has an old v3 type, a line in [`LEGACY_TYPES`]).
pub const KINDS: &[&str] = &[
    // look description (levels 1–2)
    "palette",
    "window-decoration",
    "panel-layout",
    "icons",
    "cursors",
    "sounds",
    "fonts",
    "gtk-theme",
    "qt-style",
    "shell-theme",
    "global-look",
    "wallpaper",
    "login-look",
    "lock-look",
    "boot-splash",
    "session-splash",
    "behaviour",
    // Rexon engine (level 3)
    "live-wallpaper-adaptive",
    "widget-conky",
    "widget-web",
    // code tied to a shell (level 4)
    "plasmoid",
    "wallpaper-plugin",
    "shell-extension",
    "cinnamon-applet",
    "cinnamon-desklet",
    "cinnamon-extension",
    "wm-script",
    "wm-effect",
    "window-switcher",
    "fm-action",
    "desktop-tool",
    "plugin",
    // a session on the login screen (Steam game mode): runs a program
    "game-session",
    // a reference to another Workshop item
    "ref",
];

/// Kinds that by their nature execute code: without `permissions: ["exec"]` a
/// v5 package is rejected (Rexon doesn't trust the field, but it doesn't let
/// silent code through either).
pub const EXECUTABLE_KINDS: &[&str] = &[
    "widget-conky",
    "widget-web",
    "plasmoid",
    "wallpaper-plugin",
    "shell-extension",
    "cinnamon-applet",
    "cinnamon-desklet",
    "cinnamon-extension",
    "wm-script",
    "wm-effect",
    "window-switcher",
    "fm-action",
    "desktop-tool",
    "plugin",
    "game-session",
];

pub fn is_kind(kind: &str) -> bool {
    KINDS.contains(&kind)
}

pub fn kind_is_executable(kind: &str) -> bool {
    EXECUTABLE_KINDS.contains(&kind)
}

/// Consumers of the common standard: present in any session (level 1 —
/// freedesktop directories and the Rexon wallpaper engine).
pub const UNIVERSAL_CONSUMERS: &[&str] = &["any", "freedesktop", "rexon-wallpaper"];

/// Consumer compatibility: `(who needs it, whose variant fits)`.
/// GTK-SSD (CSD frames in Mutter/Muffin/Gala) are drawn by the GTK theme.
/// `qt6ct` — Qt programs outside Plasma and LXQt whose style and palette are
/// chosen by qt6ct: a Kvantum theme is an ordinary Qt style plugin and paints
/// them the same way as in Plasma (`themes::apply_kvantum_plan`), and a KDE
/// color scheme is translated into a Qt palette
/// (`shell_palette::plan_scheme_layer`). `lxqt` — the same for the LXQt
/// palette (`lxqt.conf [Palette]`).
///
/// ⚠️ Over time the table will move into the `de_customization` matrix
/// together with a `consumer` column; until the matrix knows that column,
/// the table lives here.
pub const COMPATIBLE_CONSUMERS: &[(&str, &str)] =
    &[("gtk-ssd", "gtk"), ("qt6ct", "kvantum"), ("qt6ct", "kde-colors"), ("lxqt", "kde-colors")];

/// Compositor consumers: their native configs from a package are NEVER
/// accepted (decision of 25.10): only model → our own include file, without
/// `exec`/`spawn`/`bind` by construction.
pub const COMPOSITOR_CONSUMERS: &[&str] = &[
    "hyprland", "sway", "niri", "river", "wayfire", "labwc", "wlroots", "mango", "kwinrc",
];

/// Old v3 types → `(v5 kind, consumer)`. Order as in the matrix. A guard
/// test checks that every `constants::valid_types` type is here (except
/// `theme-bundle`, which is split into elements).
pub const LEGACY_TYPES: &[(&str, &str, &str)] = &[
    ("theme-gtk", "gtk-theme", "gtk"),
    ("theme-icons", "icons", "freedesktop"),
    ("theme-cursors", "cursors", "freedesktop"),
    ("theme-sounds", "sounds", "freedesktop"),
    ("theme-kvantum", "qt-style", "kvantum"),
    ("theme-login-screen", "login-look", "login-screen"),
    ("theme-plymouth", "boot-splash", "plymouth"),
    ("theme-conky", "widget-conky", "conky"),
    ("wallpaper-pack", "wallpaper", "any"),
    ("wallpaper-video-adaptive", "live-wallpaper-adaptive", "rexon-wallpaper"),
    ("font-pack", "fonts", "freedesktop"),
    ("theme-kde-global", "global-look", "kde-lnf"),
    ("theme-kde-colors", "palette", "kde-colors"),
    ("theme-kde-plasma", "shell-theme", "plasma"),
    ("theme-window-decoration", "window-decoration", "aurorae"),
    ("theme-plasmoid", "plasmoid", "plasma"),
    ("theme-kde-splash", "session-splash", "kde-splash"),
    ("theme-kde-lockscreen", "lock-look", "kde-lockscreen"),
    ("kde-kwin-script", "wm-script", "kwin"),
    ("kde-kwin-effect", "wm-effect", "kwin"),
    ("kde-window-switcher", "window-switcher", "kwin"),
    ("kde-wallpaper-plugin", "wallpaper-plugin", "plasma"),
    ("gnome-shell-extension", "shell-extension", "gnome-shell"),
    ("theme-gnome-shell", "shell-theme", "gnome-shell"),
    ("cinnamon-applet", "cinnamon-applet", "cinnamon"),
    ("cinnamon-desklet", "cinnamon-desklet", "cinnamon"),
    ("cinnamon-extension", "cinnamon-extension", "cinnamon"),
    ("theme-cinnamon", "shell-theme", "cinnamon"),
    ("nemo-action", "fm-action", "nemo"),
    ("theme-xfwm4", "window-decoration", "xfwm4"),
    ("theme-marco", "window-decoration", "metacity-1"),
    ("theme-lxqt", "shell-theme", "lxqt"),
    ("theme-openbox", "window-decoration", "openbox-3"),
    ("desktop-tool", "desktop-tool", "rexon-script"),
    ("plugin", "plugin", "rexon-script"),
];

/// v3 type → `(kind, consumer)`.
pub fn legacy_kind(v3_type: &str) -> Option<(&'static str, &'static str)> {
    LEGACY_TYPES.iter().find(|(t, _, _)| *t == v3_type).map(|(_, k, c)| (*k, *c))
}

/// Backwards: `(kind, consumer)` → the v3 type by which the element goes into
/// the existing install path (stage A of the plan).
pub fn legacy_type(kind: &str, consumer: &str) -> Option<&'static str> {
    LEGACY_TYPES
        .iter()
        .find(|(_, k, c)| *k == kind && *c == consumer)
        .map(|(t, _, _)| *t)
}

// ============================================================================
// Parsing and validation
// ============================================================================

/// Parse a manifest of any supported version into a v5 [`Package`].
///
/// * `format_version: 5` — the v5 model and [`validate`];
/// * `type: wallpaper-video-adaptive` (v4) — the `validate_v4_manifest`
///   schema, id/name derived as on install, one `live-wallpaper-adaptive` element;
/// * everything else — v3: type from the matrix, `theme-bundle` — one element per entry.
///
/// All errors are collected at once so the author fixes the package in one go.
pub fn parse(meta: &Value) -> Result<Package, Vec<ManifestError>> {
    let Value::Object(obj) = meta else {
        return Err(vec![ManifestError::new("", tr("manifest must be a JSON object (dict)"))]);
    };
    let fv = obj.get("format_version").and_then(Value::as_u64);
    let pkg = match fv {
        Some(5) => parse_v5(obj)?,
        Some(v) if v > u64::from(V5_FORMAT_VERSION) => {
            return Err(vec![ManifestError::new(
                "format_version",
                trf(
                    "the package uses format version %1, this Rexon reads up to %2 — update Rexon",
                    &[&v, &V5_FORMAT_VERSION],
                ),
            )])
        }
        _ if obj.get("type").and_then(Value::as_str) == Some(crate::manifest_v4::V4_TYPE) => {
            upgrade_v4(obj)?
        }
        _ => upgrade_v3(obj)?,
    };
    // Compatibility with the Rexon version — common to all format versions.
    if let Err(message) = ext_manager::check_rexon_compat(obj) {
        return Err(vec![ManifestError::new("rexon_version", message)]);
    }
    Ok(pkg)
}

fn parse_v5(obj: &Map<String, Value>) -> Result<Package, Vec<ManifestError>> {
    let mut errors = raw_checks(obj);
    let mut pkg: Package = match serde_json::from_value(Value::Object(obj.clone())) {
        Ok(p) => p,
        Err(e) => {
            errors.push(ManifestError::new(
                "",
                trf("manifest.json does not match format 5: %1", &[&e]),
            ));
            return Err(errors);
        }
    };
    pkg.source_format = V5_FORMAT_VERSION;
    errors.extend(validate(&pkg));
    if errors.is_empty() {
        Ok(pkg)
    } else {
        Err(errors)
    }
}

/// Checks visible only in the raw JSON (serde would silently drop them).
fn raw_checks(obj: &Map<String, Value>) -> Vec<ManifestError> {
    let mut errors = Vec::new();
    if let Some(Value::Object(ws)) = obj.get("workshop") {
        for key in ["app_id", "appid", "consumer_app_id", "creator_app_id"] {
            if ws.contains_key(key) {
                errors.push(ManifestError::new(
                    format!("workshop.{key}"),
                    tr("the Steam app id must not be stored in the package: Rexon knows it itself"),
                ));
            }
        }
    }
    errors
}

/// Validate the v5 model. An empty list — the package is fine.
pub fn validate(pkg: &Package) -> Vec<ManifestError> {
    let mut e = Vec::new();
    if pkg.format_version != V5_FORMAT_VERSION {
        e.push(ManifestError::new(
            "format_version",
            trf("format_version=%1, expected %2", &[&pkg.format_version, &V5_FORMAT_VERSION]),
        ));
    }
    if !ext_manager::is_safe_sys_name(&pkg.id) {
        e.push(ManifestError::new(
            "id",
            trf(
                "id '%1' may only contain Latin letters, digits, '.', '_' and '-' and must not start with '.' or '-'",
                &[&pkg.id],
            ),
        ));
    }
    if pkg.name.is_blank() {
        e.push(ManifestError::new("name", tr("the package needs a name")));
    }
    for (field, path) in [("preview", pkg.preview.as_deref())]
        .into_iter()
        .chain(pkg.previews.iter().map(|(_, p)| ("previews", Some(p.as_str()))))
    {
        if let Some(p) = path {
            check_path(&mut e, field, p);
        }
    }
    validate_properties(&mut e, &pkg.properties);
    if pkg.items.is_empty() {
        e.push(ManifestError::new("items", tr("the package has no items")));
    }
    // A package translated from v3/v4: rules the old format didn't know
    // (a substitute for a plasmoid) don't apply to it. 0 — the model was built
    // not through `parse`, it is validated strictly as v5.
    let legacy = (1..V5_FORMAT_VERSION).contains(&pkg.source_format);
    for (i, item) in pkg.items.iter().enumerate() {
        validate_item(&mut e, &format!("items[{i}]"), item, legacy);
        validate_when(&mut e, &format!("items[{i}].when"), &item.when, &pkg.properties);
    }
    for (i, dep) in pkg.requires.packages.iter().enumerate() {
        validate_dep(&mut e, &format!("requires.packages[{i}]"), dep);
    }
    for (i, dep) in pkg.requires.packages.iter().enumerate() {
        validate_when(&mut e, &format!("requires.packages[{i}].when"), &dep.when, &pkg.properties);
    }
    check_pkg_names(&mut e, "requires.forbid", &pkg.requires.forbid);
    for (i, p) in pkg.requires.preconditions.iter().enumerate() {
        let field = format!("requires.preconditions[{i}]");
        check_pkg_names(&mut e, &format!("{field}.name"), std::slice::from_ref(&p.name));
        check_pkg_names(&mut e, &format!("{field}.owners"), &p.owners);
        check_pkg_names(&mut e, &format!("{field}.not_owners"), &p.not_owners);
        if p.owners.is_empty() && p.not_owners.is_empty() && p.installed.is_none() {
            e.push(ManifestError::new(
                field.clone(),
                tr("a precondition needs owners, not_owners or installed"),
            ));
        }
        if p.message.as_ref().is_some_and(Localized::is_blank) {
            e.push(ManifestError::new(format!("{field}.message"), tr("the explanation is empty")));
        }
    }
    for (consumer, req) in &pkg.requires.targets {
        if parse_constraint(req).is_none() {
            e.push(ManifestError::new(
                format!("requires.targets.{consumer}"),
                trf("cannot read the version requirement '%1' (expected e.g. '>=6.0')", &[req]),
            ));
        }
    }
    validate_sources(&mut e, pkg);
    validate_licenses(&mut e, pkg);
    if let Some(ws) = &pkg.workshop {
        validate_workshop(&mut e, ws);
    }
    e
}

fn is_hex(s: &str, lens: &[usize]) -> bool {
    lens.contains(&s.len()) && s.chars().all(|c| c.is_ascii_hexdigit())
}

/// Pinned sources (`sources[].id`) and references to them from elements.
fn validate_sources(e: &mut Vec<ManifestError>, pkg: &Package) {
    let mut seen: Vec<&str> = Vec::new();
    for (i, src) in pkg.sources.iter().enumerate() {
        let field = format!("sources[{i}]");
        let pins = [src.rev.is_some(), src.sha256.is_some(), src.package.is_some()]
            .iter()
            .filter(|b| **b)
            .count();
        let Some(id) = src.id.as_deref() else {
            if pins > 0 {
                e.push(ManifestError::new(
                    format!("{field}.id"),
                    tr("a pinned source needs an id: items refer to its files as sources/<id>/…"),
                ));
            }
            continue;
        };
        if !ext_manager::is_safe_sys_name(id) || id.contains('/') {
            e.push(ManifestError::new(
                format!("{field}.id"),
                trf("source id '%1' may only contain Latin letters, digits, '.', '_' and '-'", &[&id]),
            ));
        }
        if seen.contains(&id) {
            e.push(ManifestError::new(format!("{field}.id"), trf("source id '%1' is used twice", &[&id])));
        }
        seen.push(id);
        if pins != 1 {
            e.push(ManifestError::new(
                field.clone(),
                tr("a source with an id needs exactly one of: rev (git commit), sha256 (archive) or package (dependency role)"),
            ));
        }
        if let Some(rev) = &src.rev {
            if !is_hex(rev, &[40, 64]) {
                e.push(ManifestError::new(
                    format!("{field}.rev"),
                    tr("rev must be a full git commit hash, not a branch or a tag"),
                ));
            }
        }
        if let Some(sum) = &src.sha256 {
            if !is_hex(sum, &[64]) {
                e.push(ManifestError::new(format!("{field}.sha256"), tr("sha256 must be 64 hexadecimal characters")));
            }
        }
        if src.is_fetched() && !src.url.as_deref().is_some_and(|u| u.starts_with("https://")) {
            e.push(ManifestError::new(
                format!("{field}.url"),
                tr("a downloaded source needs an https:// address"),
            ));
        }
        if let Some(role) = &src.package {
            if !pkg.requires.packages.iter().any(|d| &d.role == role) {
                e.push(ManifestError::new(
                    format!("{field}.package"),
                    trf("no dependency with the role '%1' in requires.packages", &[role]),
                ));
            }
        }
        if license_class(src.license.as_deref()) == LicenseClass::Missing {
            e.push(ManifestError::new(
                format!("{field}.license"),
                trf("source '%1' needs its license", &[&src.what]),
            ));
        }
    }
    for (i, item) in pkg.items.iter().enumerate() {
        for (section, map) in [("native", &item.native), ("substitute", &item.substitute)] {
            for (consumer, path) in map {
                let Some(rest) = path.strip_prefix(SOURCES_DIR).and_then(|r| r.strip_prefix('/')) else {
                    continue;
                };
                let id = rest.split('/').next().unwrap_or_default();
                if pkg.source(id).is_none() {
                    e.push(ManifestError::new(
                        format!("items[{i}].{section}.{consumer}"),
                        trf("path '%1' refers to a source that is not declared in sources", &[path]),
                    ));
                }
            }
        }
    }
    for (i, dep) in pkg.requires.packages.iter().enumerate() {
        for c in &dep.consumers {
            if !is_consumer_name(c) {
                e.push(ManifestError::new(
                    format!("requires.packages[{i}].consumers"),
                    trf("'%1' is not a valid consumer name", &[c]),
                ));
            }
        }
    }
}

/// A path inside the archive: relative, no `..`, no `\`.
fn check_path(e: &mut Vec<ManifestError>, field: &str, path: &str) {
    let ok = !path.trim().is_empty()
        && !path.contains('\\')
        && !path.contains('\0')
        && crate::appearance_installer::safe_extract_entry("/rxn", path);
    if !ok {
        e.push(ManifestError::new(
            field,
            trf("path '%1' must stay inside the package", &[&path]),
        ));
    }
}

fn validate_properties(e: &mut Vec<ManifestError>, props: &[(String, Property)]) {
    for (name, p) in props {
        let field = format!("properties.{name}");
        if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') || name.is_empty() {
            e.push(ManifestError::new(
                field.clone(),
                tr("property names may only contain Latin letters, digits and '_'"),
            ));
        }
        let bad_default = match p.kind {
            PropertyType::Color => !p.default.as_str().is_some_and(is_hex_color),
            PropertyType::Toggle => !p.default.is_boolean(),
            PropertyType::Text => !p.default.is_string(),
            PropertyType::Combo => {
                if p.options.is_empty() {
                    e.push(ManifestError::new(field.clone(), tr("a list property needs options")));
                }
                let values = p.option_values();
                for (i, v) in values.iter().enumerate() {
                    if values[..i].contains(v) {
                        e.push(ManifestError::new(
                            format!("{field}.options"),
                            trf("option '%1' is listed twice", &[v]),
                        ));
                    }
                }
                for o in &p.options {
                    if let PropOption::Labeled { label, .. } = o {
                        if label.is_blank() {
                            e.push(ManifestError::new(format!("{field}.options"), tr("the option label is empty")));
                        }
                    }
                }
                !p.default.as_str().is_some_and(|d| values.contains(&d))
            }
            PropertyType::Slider => match (p.min, p.max, p.default.as_f64()) {
                (Some(lo), Some(hi), Some(d)) if lo < hi => !(lo..=hi).contains(&d),
                _ => {
                    e.push(ManifestError::new(
                        field.clone(),
                        tr("a slider needs numeric min < max and a numeric default"),
                    ));
                    false
                }
            },
        };
        if bad_default {
            e.push(ManifestError::new(
                field,
                tr("the default value does not fit the property type"),
            ));
        }
    }
}

/// The `when` condition: the property is declared, it is a list or a toggle,
/// and every value fits it.
fn validate_when(e: &mut Vec<ManifestError>, field: &str, when: &When, props: &[(String, Property)]) {
    for (name, want) in when {
        let f = format!("{field}.{name}");
        let Some((_, p)) = props.iter().find(|(k, _)| k == name) else {
            e.push(ManifestError::new(f, trf("unknown property '%1'", &[name])));
            continue;
        };
        let wants: Vec<&Value> = match want {
            Value::Array(list) => list.iter().collect(),
            v => vec![v],
        };
        if wants.is_empty() {
            e.push(ManifestError::new(f.clone(), tr("expected a value or a list of values")));
        }
        let fits = |v: &Value| match p.kind {
            PropertyType::Combo => v.as_str().is_some_and(|s| p.option_values().contains(&s)),
            PropertyType::Toggle => v.is_boolean(),
            _ => false,
        };
        if !matches!(p.kind, PropertyType::Combo | PropertyType::Toggle) {
            e.push(ManifestError::new(f, tr("a condition can only test a list or toggle property")));
        } else if let Some(bad) = wants.into_iter().find(|v| !fits(v)) {
            e.push(ManifestError::new(
                f,
                trf("'%1' is not a value of property '%2'", &[&bad.to_string(), name]),
            ));
        }
    }
}

fn is_hex_color(s: &str) -> bool {
    let Some(hex) = s.strip_prefix('#') else { return false };
    matches!(hex.len(), 6 | 8) && hex.chars().all(|c| c.is_ascii_hexdigit())
}

fn validate_item(e: &mut Vec<ManifestError>, field: &str, item: &Item, legacy: bool) {
    if !is_kind(&item.kind) {
        e.push(ManifestError::new(
            format!("{field}.kind"),
            trf("unknown item kind '%1'", &[&item.kind]),
        ));
        return;
    }
    if item.kind == "ref" {
        if !item.workshop.is_some_and(|id| id > 0) {
            e.push(ManifestError::new(
                format!("{field}.workshop"),
                tr("a reference item needs the id of a Steam Workshop item"),
            ));
        }
        return;
    }
    if let Some(m) = &item.model {
        check_path(e, &format!("{field}.model"), m);
    }
    for (section, map) in [("native", &item.native), ("substitute", &item.substitute)] {
        for (consumer, path) in map {
            let f = format!("{field}.{section}.{consumer}");
            if is_compositor_consumer(consumer) {
                e.push(ManifestError::new(f.clone(), compositor_refusal()));
            } else if !is_consumer_name(consumer) {
                e.push(ManifestError::new(
                    f.clone(),
                    trf("'%1' is not a valid consumer name", &[consumer]),
                ));
            }
            check_path(e, &f, path);
            if is_compositor_config_path(path) {
                e.push(ManifestError::new(f, compositor_refusal()));
            }
        }
    }
    if item.native.is_empty() && item.model.is_none() && item.substitute.is_empty() {
        e.push(ManifestError::new(
            field,
            tr("the item has neither its own files nor a description to build them from"),
        ));
    }
    if !item.generate && item.native.is_empty() && item.substitute.is_empty() {
        e.push(ManifestError::new(
            format!("{field}.generate"),
            tr("generation is forbidden, but the item has no files of its own"),
        ));
    }
    if !item.substitute.is_empty() && item.role.as_deref().is_none_or(|r| r.trim().is_empty()) {
        e.push(ManifestError::new(
            format!("{field}.role"),
            tr("a substitute needs a role: what the item means (weather, clock…)"),
        ));
    }
    validate_plasmoid(e, field, item, legacy);
    if kind_is_executable(&item.kind) && !item.permissions.contains(&Permission::Exec) {
        e.push(ManifestError::new(
            format!("{field}.permissions"),
            trf(
                "items of kind '%1' run code: the package must declare the 'exec' permission",
                &[&item.kind],
            ),
        ));
    }
}

/// A plasmoid is native only to Plasma (plasmawindowed research 25.10):
/// outside it, it is optional and must carry a substitute, except for the shell's own roles.
fn validate_plasmoid(e: &mut Vec<ManifestError>, field: &str, item: &Item, legacy: bool) {
    if item.kind != "plasmoid" {
        if item.windowed.is_some() {
            e.push(ManifestError::new(
                format!("{field}.windowed"),
                tr("only Plasma widgets can be shown in a window"),
            ));
        }
        return;
    }
    if item.required {
        e.push(ManifestError::new(
            format!("{field}.required"),
            tr("a Plasma widget works natively only in Plasma, so it must be optional (\"required\": false)"),
        ));
    }
    let plasma_only = item.role.as_deref().is_some_and(is_plasma_only_role);
    if item.substitute.is_empty() && !plasma_only && !legacy {
        e.push(ManifestError::new(
            format!("{field}.substitute"),
            tr("a Plasma widget needs a substitute for other desktops (for example a Conky widget)"),
        ));
    }
    let Some(w) = &item.windowed else { return };
    let wf = format!("{field}.windowed");
    if plasma_only && w.allowed {
        e.push(ManifestError::new(
            format!("{wf}.allowed"),
            trf(
                "a widget with the role '%1' only makes sense inside Plasma and cannot be shown in a window",
                &[&item.role.as_deref().unwrap_or_default()],
            ),
        ));
    }
    if w.sessions.is_empty() {
        e.push(ManifestError::new(format!("{wf}.sessions"), tr("list at least one session type: x11 or wayland")));
    }
    if w.min_size.is_some_and(|[mw, mh]| mw == 0 || mh == 0) {
        e.push(ManifestError::new(format!("{wf}.min_size"), tr("the minimum size must be above zero")));
    }
    if let Some([_, _, gw, gh]) = w.default_geometry {
        let [mw, mh] = w.min_size.unwrap_or([1, 1]);
        if gw < mw as i32 || gh < mh as i32 || gw <= 0 || gh <= 0 {
            e.push(ManifestError::new(
                format!("{wf}.default_geometry"),
                tr("the window size must be positive and not smaller than the minimum size"),
            ));
        }
    }
    for n in &w.needs {
        if !is_consumer_name(n) {
            e.push(ManifestError::new(format!("{wf}.needs"), trf("'%1' is not a valid component name", &[n])));
        }
    }
}

fn compositor_refusal() -> String {
    tr("compositor configuration files are never taken from a package: Rexon writes its own from the description")
}

/// Consumer name: lowercase Latin, digits, `-`, `.`.
fn is_consumer_name(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with(['-', '.'])
        && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '.'))
}

/// The consumer is a compositor (its config from a package is not accepted).
pub fn is_compositor_consumer(consumer: &str) -> bool {
    COMPOSITOR_CONSUMERS.contains(&consumer)
}

/// The path looks like a native compositor config (`hypr/hyprland.conf`,
/// `sway/config`, `niri/config.kdl`, `river/init`, `wayfire.ini`,
/// `labwc/rc.xml`, `labwc/autostart`). Openbox/labwc themes (`themerc`) are
/// appearance, they don't fall in here.
pub fn is_compositor_config_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    let parts: Vec<&str> = lower.split('/').filter(|p| !p.is_empty()).collect();
    let Some(base) = parts.last().copied() else { return false };
    if matches!(base, "hyprland.conf" | "hyprland.lua" | "wayfire.ini" | "config.kdl") {
        return true;
    }
    let parent = parts.len().checked_sub(2).map(|i| parts[i]);
    matches!(
        (parent, base),
        (Some("hypr"), _)
            | (Some("sway"), "config")
            | (Some("river"), "init")
            | (Some("labwc"), "rc.xml" | "autostart" | "environment" | "menu.xml")
            | (Some("niri"), _)
    )
}

fn validate_dep(e: &mut Vec<ManifestError>, field: &str, dep: &PackageDep) {
    if dep.role.trim().is_empty() {
        e.push(ManifestError::new(format!("{field}.role"), tr("a dependency needs a role")));
    }
    for kind in &dep.for_kinds {
        if !is_kind(kind) {
            e.push(ManifestError::new(
                format!("{field}.for"),
                trf("unknown item kind '%1'", &[kind]),
            ));
        }
    }
    check_pkg_names(e, &format!("{field}.forbid"), &dep.forbid);
    let aur_fallback = dep.fallback.contains(&DepSource::Aur);
    if !dep.fallback.is_empty() && (dep.source != DepSource::Repo || dep.fallback != [DepSource::Aur]) {
        e.push(ManifestError::new(
            format!("{field}.fallback"),
            tr("only a repository dependency can have a fallback, and only to AUR: \"fallback\": [\"aur\"]"),
        ));
    }
    match dep.source {
        DepSource::Repo => {
            if !dep.names.keys().any(|k| k != AUR_NAMES_KEY) {
                e.push(ManifestError::new(
                    field,
                    tr("a repository dependency needs package names for at least one distribution family"),
                ));
            }
            if aur_fallback && !dep.names.get(AUR_NAMES_KEY).is_some_and(Value::is_array) {
                e.push(ManifestError::new(
                    format!("{field}.{AUR_NAMES_KEY}"),
                    tr("an AUR fallback needs package names under \"aur\""),
                ));
            }
            for (key, value) in &dep.names {
                let f = format!("{field}.{key}");
                match PkgFamily::from_value(key).filter(|f| *f != PkgFamily::Unknown) {
                    Some(family) => check_names(e, &f, value, |n| system_plan::is_safe_pkg_arg(family, n)),
                    None if key == AUR_NAMES_KEY && aur_fallback => {
                        check_names(e, &f, value, |n| system_plan::is_safe_pkg_arg(PkgFamily::Arch, n))
                    }
                    None if key == AUR_NAMES_KEY => e.push(ManifestError::new(
                        f,
                        tr("AUR packages go into a separate dependency with \"source\": \"aur\""),
                    )),
                    None => e.push(ManifestError::new(
                        f,
                        trf("unknown distribution family '%1'", &[key]),
                    )),
                }
            }
        }
        DepSource::Aur => {
            for (key, value) in &dep.names {
                let f = format!("{field}.{key}");
                if key == AUR_NAMES_KEY {
                    check_names(e, &f, value, |n| {
                        system_plan::is_safe_pkg_arg(PkgFamily::Arch, n)
                    });
                    if value.is_null() {
                        e.push(ManifestError::new(f, tr("an AUR dependency needs package names")));
                    }
                } else {
                    e.push(ManifestError::new(
                        f,
                        tr("an AUR dependency lists its packages under \"aur\" only"),
                    ));
                }
            }
            if !dep.names.contains_key(AUR_NAMES_KEY) {
                e.push(ManifestError::new(
                    field,
                    tr("an AUR dependency needs package names"),
                ));
            }
        }
        DepSource::GnomeExtensions => {
            match dep.uuid.as_deref() {
                Some(u) if is_extension_uuid(u) => {}
                _ => e.push(ManifestError::new(
                    format!("{field}.uuid"),
                    tr("a GNOME extension dependency needs the extension uuid (name@author)"),
                )),
            }
            if !dep.names.is_empty() {
                e.push(ManifestError::new(
                    field,
                    tr("a GNOME extension dependency has no package names, only the uuid"),
                ));
            }
        }
    }
    if dep.source != DepSource::GnomeExtensions && dep.uuid.is_some() {
        e.push(ManifestError::new(
            format!("{field}.uuid"),
            tr("uuid is only for dependencies from extensions.gnome.org"),
        ));
    }
}

/// List of bare package names (`forbid`, preconditions).
fn check_pkg_names(e: &mut Vec<ManifestError>, field: &str, names: &[String]) {
    for name in names {
        if !system_plan::valid_pkg_name(name) {
            e.push(ManifestError::new(field, trf("'%1' is not a package name", &[name])));
        }
    }
}

fn check_names(e: &mut Vec<ManifestError>, field: &str, value: &Value, ok: impl Fn(&str) -> bool) {
    match value {
        Value::Null => {}
        Value::Array(list) if !list.is_empty() => {
            for v in list {
                match v.as_str() {
                    Some(n) if ok(n) => {}
                    _ => e.push(ManifestError::new(
                        field,
                        trf("'%1' is not a package name", &[&v.as_str().unwrap_or(&v.to_string())]),
                    )),
                }
            }
        }
        _ => e.push(ManifestError::new(
            field,
            tr("expected a list of package names or null"),
        )),
    }
}

/// GNOME Shell extension uuid: `name@domain`, no paths or spaces.
pub fn is_extension_uuid(uuid: &str) -> bool {
    let Some((name, domain)) = uuid.split_once('@') else { return false };
    let part_ok = |s: &str| {
        !s.is_empty()
            && !s.starts_with(['.', '-'])
            && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    };
    part_ok(name) && part_ok(domain) && !uuid.contains("..")
}

fn validate_workshop(e: &mut Vec<ManifestError>, ws: &Workshop) {
    if ws.published_file_id == Some(0) {
        e.push(ManifestError::new(
            "workshop.published_file_id",
            tr("the Workshop item id cannot be 0"),
        ));
    }
    for tag in &ws.tags {
        if tag.trim().is_empty() || tag.chars().count() > 255 || tag.contains(',') {
            e.push(ManifestError::new(
                "workshop.tags",
                trf("Workshop tag '%1' must be 1–255 characters without commas", &[tag]),
            ));
        }
    }
    if let Some(p) = &ws.preview {
        check_path(e, "workshop.preview", p);
    }
}

// ── v3 / v4 → v5 ────────────────────────────────────────────────────────

fn install_err(field: &str, err: ext_manager::InstallError) -> Vec<ManifestError> {
    vec![ManifestError::new(field, err.message)]
}

/// Fields common to v3 and v4: the package description without elements.
fn legacy_header(obj: &Map<String, Value>, id: String, source_format: u32) -> Package {
    let text = |k: &str| obj.get(k).and_then(Value::as_str).filter(|s| !s.trim().is_empty()).map(str::to_string);
    let name = obj
        .get("name")
        .and_then(|v| serde_json::from_value::<Localized>(v.clone()).ok())
        .filter(|n| !n.is_blank())
        .unwrap_or_else(|| Localized::Plain(id.clone()));
    Package {
        format_version: V5_FORMAT_VERSION,
        version: text("version").unwrap_or_else(default_version),
        name,
        description: text("description").map(Localized::Plain),
        authors: text("author").map(|n| vec![Author { name: n, url: None }]).unwrap_or_default(),
        license: text("license"),
        homepage: text("homepage"),
        sources: Vec::new(),
        derived_from: Vec::new(),
        rexon_version: text("rexon_version"),
        preview: text("preview"),
        previews: BTreeMap::new(),
        tags: obj
            .get("tags")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|t| t.as_str().map(str::to_string)).collect())
            .unwrap_or_default(),
        properties: Vec::new(),
        items: Vec::new(),
        requires: Requires::default(),
        privacy: None,
        workshop: None,
        id,
        source_format,
    }
}

/// A v5 element from a v3 type and a directory in the archive. Old packages
/// didn't know the `permissions` field: executable types get it here, and the
/// permission is asked by the old path (the Conky dialog, script types).
fn legacy_item(v3_type: &str, dir: &str) -> Option<Item> {
    let (kind, consumer) = legacy_kind(v3_type)?;
    Some(Item {
        kind: kind.to_string(),
        model: None,
        native: BTreeMap::from([(consumer.to_string(), dir.to_string())]),
        generate: false,
        // A plasmoid is native only to Plasma: in an old package it must not block
        // the set on other desktops — there the entry is skipped.
        required: kind != "plasmoid",
        role: None,
        substitute: BTreeMap::new(),
        permissions: if kind_is_executable(kind) { vec![Permission::Exec] } else { Vec::new() },
        workshop: None,
        windowed: None,
        note: None,
        when: BTreeMap::new(),
    })
}

fn upgrade_v4(obj: &Map<String, Value>) -> Result<Package, Vec<ManifestError>> {
    let mut meta = obj.clone();
    ext_manager::v4_fill_id_and_name(&mut meta).map_err(|e| install_err("", e))?;
    let id = ext_manager::manifest_ext_id(&meta).map_err(|e| install_err("id", e))?;
    let mut pkg = legacy_header(&meta, id, crate::manifest_v4::V4_FORMAT_VERSION as u32);
    if let Some(item) = legacy_item(crate::manifest_v4::V4_TYPE, ".") {
        pkg.items.push(item);
    }
    Ok(pkg)
}

fn upgrade_v3(obj: &Map<String, Value>) -> Result<Package, Vec<ManifestError>> {
    let id = ext_manager::manifest_ext_id(obj).map_err(|e| install_err("id", e))?;
    let ext_type = obj.get("type").and_then(Value::as_str).unwrap_or_default();
    if !constants::is_valid_type(ext_type) {
        return Err(vec![ManifestError::new(
            "type",
            trf("Unknown extension type: '%1'", &[&ext_type]),
        )]);
    }
    let mut pkg = legacy_header(obj, id, constants::FORMAT_VERSION);
    if ext_type == "theme-bundle" {
        // Like `install_bundle_plan`: entries with an invalid type and nested sets
        // are skipped (that is exactly how old packages were installed).
        let entries = obj.get("bundle").and_then(Value::as_object);
        for (sub_type, dir) in entries.into_iter().flatten() {
            if sub_type == "theme-bundle" || !constants::is_valid_type(sub_type) {
                continue;
            }
            let Some(dir) = dir.as_str() else { continue };
            let mut e = Vec::new();
            check_path(&mut e, &format!("bundle.{sub_type}"), dir);
            if !e.is_empty() {
                return Err(e);
            }
            if let Some(item) = legacy_item(sub_type, dir) {
                pkg.items.push(item);
            }
        }
        if pkg.items.is_empty() {
            return Err(vec![ManifestError::new(
                "bundle",
                tr("the theme bundle lists no parts Rexon knows"),
            )]);
        }
    } else if let Some(item) = legacy_item(ext_type, "data/") {
        pkg.items.push(item);
    }
    Ok(pkg)
}

// ============================================================================
// Consumer versions
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Ge,
    Gt,
    Le,
    Lt,
    Eq,
}

fn parse_version(s: &str) -> Option<Vec<u64>> {
    let parts: Option<Vec<u64>> = s.trim().split('.').map(|p| p.parse().ok()).collect();
    parts.filter(|p| !p.is_empty())
}

fn parse_constraint(s: &str) -> Option<(Op, Vec<u64>)> {
    let s = s.trim();
    let (op, rest) = [(">=", Op::Ge), ("<=", Op::Le), ("==", Op::Eq), (">", Op::Gt), ("<", Op::Lt), ("=", Op::Eq)]
        .iter()
        .find_map(|(p, op)| s.strip_prefix(p).map(|r| (*op, r)))
        .unwrap_or((Op::Eq, s));
    Some((op, parse_version(rest)?))
}

fn cmp_versions(a: &[u64], b: &[u64]) -> std::cmp::Ordering {
    let n = a.len().max(b.len());
    (0..n)
        .map(|i| a.get(i).unwrap_or(&0).cmp(b.get(i).unwrap_or(&0)))
        .find(|o| o.is_ne())
        .unwrap_or(std::cmp::Ordering::Equal)
}

/// Whether version `have` satisfies requirement `req` (`>=6.0`, `<0.56`, `47`).
/// An unreadable requirement or version — `None`.
pub fn version_satisfies(have: &str, req: &str) -> Option<bool> {
    use std::cmp::Ordering::*;
    let (op, want) = parse_constraint(req)?;
    let have = parse_version(have)?;
    let ord = cmp_versions(&have, &want);
    Some(match op {
        Op::Ge => ord != Less,
        Op::Gt => ord == Greater,
        Op::Le => ord != Greater,
        Op::Lt => ord == Less,
        Op::Eq => ord == Equal,
    })
}

// ============================================================================
// Payload choice rules
// ============================================================================

/// How close a generated variant is to the intended one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Accuracy {
    Exact,
    /// Installed marked "approximate" with a preview of the GENERATED one
    /// (decision of 25.10, question 3).
    Approximate,
}

impl Accuracy {
    /// Label for the card.
    pub fn label(self) -> String {
        match self {
            // TRANSLATORS: card mark — Rexon built this part itself and it matches the design.
            Accuracy::Exact => tr("Made by Rexon: exact"),
            // TRANSLATORS: card mark — Rexon built this part itself, close to the design.
            Accuracy::Approximate => tr("Made by Rexon: approximate"),
        }
    }
}

/// Why the element won't install on this desktop.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RefuseReason {
    /// No own files for this desktop, and no description for generation either.
    NoVariant,
    /// The author forbade generation (`generate: false`).
    GenerationForbidden,
    /// There is a description, but Rexon has no generator for the session's consumers yet.
    NoGenerator,
}

impl RefuseReason {
    /// Text for the person. Details from the matrix (`Support`, `note`) are
    /// added by the caller — it knows the desktop.
    pub fn text(&self) -> String {
        match self {
            RefuseReason::NoVariant => {
                tr("The package has no version of this part for your desktop and nothing to build one from.")
            }
            RefuseReason::GenerationForbidden => tr(
                "The author only allows their own versions of this part, and none of them is for your desktop.",
            ),
            RefuseReason::NoGenerator => {
                tr("Rexon can't build this part for your desktop yet.")
            }
        }
    }
}

/// What to install for one element.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "choice", rename_all = "kebab-case")]
pub enum Choice {
    /// The author's native variant for a session consumer.
    Native { consumer: String, path: String },
    /// No native one; the variant of compatible consumer `from` fit.
    Compatible { consumer: String, from: String, path: String },
    /// Rexon builds the variant from the model. Preview — from the generated one.
    Generate { consumer: String, model: String, accuracy: Accuracy },
    /// Substitute by meaning, marked "substitute: <role>".
    Substitute { consumer: String, path: String, role: String },
    Refuse { reason: RefuseReason },
    /// The element is turned off by a property value (`when` didn't match): not
    /// installed and not counted as skipped.
    Off,
}

impl Choice {
    pub fn is_refused(&self) -> bool {
        matches!(self, Choice::Refuse { .. })
    }
}

/// The current session as seen by the format.
#[derive(Debug, Clone, Default)]
pub struct Session {
    /// Session consumers in order of preference (from the matrix): Budgie 10.10
    /// Wayland → `openbox-3`, `gtk`, `budgie-panel`. Universal ones
    /// ([`UNIVERSAL_CONSUMERS`]) need not be added.
    pub consumers: Vec<String>,
    /// Known consumer versions (`cinnamon` → `6.4.8`).
    pub versions: BTreeMap<String, String>,
}

impl Session {
    fn all_consumers(&self) -> Vec<&str> {
        let mut v: Vec<&str> = self.consumers.iter().map(String::as_str).collect();
        for u in UNIVERSAL_CONSUMERS {
            if !v.contains(u) {
                v.push(u);
            }
        }
        v
    }

    /// Whether a native variant fits a consumer per `requires.targets`.
    /// Unknown version or unreadable requirement — it fits (as with Spices).
    fn native_fits(&self, targets: &BTreeMap<String, String>, consumer: &str) -> bool {
        match (targets.get(consumer), self.versions.get(consumer)) {
            (Some(req), Some(have)) => version_satisfies(have, req).unwrap_or(true),
            _ => true,
        }
    }
}

/// Choose the payload for an element. `generator(kind, consumer)` — whether
/// Rexon can build the variant and how exactly (generators are the next layer).
///
/// Order: native for any session consumer → compatible → generation (unless
/// the author forbade it) → substitute → refusal.
pub fn choose(
    item: &Item,
    targets: &BTreeMap<String, String>,
    session: &Session,
    generator: &dyn Fn(&str, &str) -> Option<Accuracy>,
) -> Choice {
    let consumers = session.all_consumers();
    for c in &consumers {
        if let Some(path) = item.native.get(*c) {
            if session.native_fits(targets, c) {
                return Choice::Native { consumer: c.to_string(), path: path.clone() };
            }
        }
    }
    for c in &consumers {
        for (want, from) in COMPATIBLE_CONSUMERS {
            if want != c {
                continue;
            }
            if let Some(path) = item.native.get(*from) {
                if session.native_fits(targets, from) {
                    return Choice::Compatible {
                        consumer: c.to_string(),
                        from: from.to_string(),
                        path: path.clone(),
                    };
                }
            }
        }
    }
    if let (Some(model), true) = (&item.model, item.generate) {
        for c in &consumers {
            if let Some(accuracy) = generator(&item.kind, c) {
                return Choice::Generate { consumer: c.to_string(), model: model.clone(), accuracy };
            }
        }
    }
    for c in &consumers {
        if let Some(path) = item.substitute.get(*c) {
            return Choice::Substitute {
                consumer: c.to_string(),
                path: path.clone(),
                role: item.role.clone().unwrap_or_default(),
            };
        }
    }
    let reason = if item.model.is_some() && !item.generate {
        RefuseReason::GenerationForbidden
    } else if item.model.is_some() {
        RefuseReason::NoGenerator
    } else if !item.native.is_empty() && !item.generate {
        RefuseReason::GenerationForbidden
    } else {
        RefuseReason::NoVariant
    };
    Choice::Refuse { reason }
}

/// Choice for the whole package.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PackagePlan {
    /// The choice for each element, in `items` order.
    pub choices: Vec<Choice>,
    /// Index of the first mandatory element that won't install: then the whole
    /// package is not installed ("all or nothing").
    pub blocked_by: Option<usize>,
}

impl PackagePlan {
    pub fn installable(&self) -> bool {
        self.blocked_by.is_none()
    }

    /// Optional elements that will be skipped: `(index, reason)` — for the
    /// summary line "skipped: … (why)".
    pub fn skipped(&self) -> Vec<(usize, &RefuseReason)> {
        self.choices
            .iter()
            .enumerate()
            .filter_map(|(i, c)| match c {
                Choice::Refuse { reason } => Some((i, reason)),
                _ => None,
            })
            .collect()
    }
}

/// Choose the payload for each package element with default property values
/// ([`choose_all_with`]).
pub fn choose_all(
    pkg: &Package,
    session: &Session,
    generator: &dyn Fn(&str, &str) -> Option<Accuracy>,
) -> PackagePlan {
    choose_all_with(pkg, session, &PropValues::new(), generator)
}

/// Choose the payload for each package element. `ref` elements are installed
/// by Steam (an item dependency), here they are neither resolved nor blocking;
/// an element whose `when` didn't match `values` — [`Choice::Off`].
pub fn choose_all_with(
    pkg: &Package,
    session: &Session,
    values: &PropValues,
    generator: &dyn Fn(&str, &str) -> Option<Accuracy>,
) -> PackagePlan {
    let mut blocked_by = None;
    let choices: Vec<Choice> = pkg
        .items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            if !pkg.when_holds(&item.when, values) {
                return Choice::Off;
            }
            if item.kind == "ref" {
                return Choice::Native { consumer: "steam-workshop".into(), path: String::new() };
            }
            let c = choose(item, &pkg.requires.targets, session, generator);
            if c.is_refused() && item.required && blocked_by.is_none() {
                blocked_by = Some(i);
            }
            c
        })
        .collect();
    PackagePlan { choices, blocked_by }
}

// ============================================================================
// Dependencies: post-install check
// ============================================================================

/// A system snapshot: which REAL package covers a name (`pacman -Qq <name>`
/// → package name, or a Provides lookup; `None` — not installed).
pub type OwnerOf<'a> = &'a dyn Fn(&str) -> Option<String>;

/// Whether the name appeared because of the install: before — absent, after — present.
fn appeared(name: &str, before: OwnerOf, after: OwnerOf) -> bool {
    after(name).is_some() && before(name).is_none()
}

/// The "exactly what was asked for got installed" check from two snapshots —
/// before and after installing a dependency whose source was chosen by
/// [`PackageDep::resolve`].
///
/// Each `resolved` name must be installed as itself: substitution by a
/// provider (`gamescope-session-steam-git` covered by
/// `gamescope-session-cachyos`) is an error, even if the substitute was
/// installed beforehand. A package from `forbid` is an error only if it
/// APPEARED: one installed before the install wasn't brought by Rexon. After
/// an error the caller rolls back the install ("all or nothing").
pub fn verify_dependency(
    dep: &PackageDep,
    resolved: &ResolvedDep,
    before: OwnerOf,
    after: OwnerOf,
) -> Result<(), String> {
    for name in &resolved.names {
        match after(name) {
            None => return Err(trf("Package %1 did not get installed.", &[&name])),
            Some(real) if real != *name => {
                return Err(trf(
                    "Package %1 was replaced by %2 from another source — Rexon does not accept substitutes.",
                    &[name, &real],
                ))
            }
            Some(_) => {}
        }
    }
    forbid_appeared(&dep.forbid, before, after)
}

fn forbid_appeared(forbid: &[String], before: OwnerOf, after: OwnerOf) -> Result<(), String> {
    match forbid.iter().find(|bad| appeared(bad, before, after)) {
        Some(bad) => Err(trf("Installing pulled in %1, which this package must not bring along.", &[bad])),
        None => Ok(()),
    }
}

/// Package-wide bans (`requires.forbid`) from snapshots before and after
/// installing ALL of its dependencies.
pub fn verify_package_change(pkg: &Package, before: OwnerOf, after: OwnerOf) -> Result<(), String> {
    forbid_appeared(&pkg.requires.forbid, before, after)
}

/// Package preconditions violated on the system BEFORE installing. Empty —
/// can install; otherwise the package is not installed at all, and the person
/// sees [`UnmetPrecondition::text`].
pub fn unmet_preconditions<'a>(pkg: &'a Package, owner_of: OwnerOf) -> Vec<UnmetPrecondition<'a>> {
    pkg.requires
        .preconditions
        .iter()
        .filter_map(|p| {
            let found = owner_of(&p.name);
            p.is_unmet(found.as_deref()).then_some(UnmetPrecondition { precondition: p, found })
        })
        .collect()
}

/// Whether a GNOME Shell extension supports the running shell version.
/// `declared` — the `shell-version` field from extensions.gnome.org metadata
/// (`["45", "46"]`, old ones `["3.38"]`); `running` — `gnome-shell
/// --version` without the prefix (`47.2`). Since GNOME 40 the major number is
/// compared, before it — the `3.xx` pair.
pub fn gnome_shell_compatible(declared: &[String], running: &str) -> bool {
    let parts: Vec<&str> = running.trim().split('.').collect();
    let Some(major) = parts.first().filter(|m| !m.is_empty()) else { return false };
    let key = if *major == "3" {
        match parts.get(1) {
            Some(minor) => format!("3.{minor}"),
            None => return false,
        }
    } else {
        major.to_string()
    };
    declared.iter().any(|d| {
        let d = d.trim();
        d == key || d.split('.').take(if *major == "3" { 2 } else { 1 }).collect::<Vec<_>>().join(".") == key
    })
}

// ============================================================================
// Executable content
// ============================================================================

/// Class of executable content. Everything else is data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExecClass {
    /// A Conky config file (can `exec*`, `lua_load`).
    Conky,
    Lua,
    /// JavaScript: web widgets, Cinnamon applets, GNOME extensions, KWin.
    JavaScript,
    /// HTML with embedded scripts.
    WebPage,
    Qml,
    /// A launcher or file manager action with `Exec=`.
    DesktopExec,
    /// A shell or interpreter script (`#!`, `.sh`, `.py`…).
    Script,
    /// An executable file (ELF).
    Binary,
}

/// A file from the archive for the content check.
#[derive(Debug, Clone, Copy)]
pub struct ArchiveFile<'a> {
    /// Path in the archive.
    pub path: &'a str,
    /// Beginning of the content (the first few KiB are enough).
    pub head: &'a [u8],
    /// The "executable" bit in the archive.
    pub executable: bool,
}

/// The file's class, or `None` — data.
pub fn classify_file(f: &ArchiveFile) -> Option<ExecClass> {
    let lower = f.path.to_ascii_lowercase();
    let base = lower.rsplit('/').next().unwrap_or(&lower);
    let ext = base.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
    let text = String::from_utf8_lossy(f.head);
    if f.head.starts_with(b"\x7fELF") {
        return Some(ExecClass::Binary);
    }
    if f.head.starts_with(b"#!") {
        return Some(ExecClass::Script);
    }
    match ext {
        "lua" => return Some(ExecClass::Lua),
        "js" | "mjs" | "cjs" => return Some(ExecClass::JavaScript),
        "qml" => return Some(ExecClass::Qml),
        "sh" | "bash" | "py" | "pl" | "rb" | "so" => return Some(ExecClass::Script),
        "html" | "htm" | "xhtml" if text.to_ascii_lowercase().contains("<script") => {
            return Some(ExecClass::WebPage)
        }
        "desktop" | "nemo_action" | "kdesktop"
            if text.lines().any(|l| l.trim_start().starts_with("Exec=")) =>
        {
            return Some(ExecClass::DesktopExec)
        }
        _ => {}
    }
    if base.contains("conkyrc") || base == "conky.conf" || text.contains("conky.config") || text.contains("conky.text") {
        return Some(ExecClass::Conky);
    }
    if f.executable {
        return Some(ExecClass::Script);
    }
    None
}

/// What dangerous things a plasmoid's QML/JS does beyond drawing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PlasmoidRisk {
    /// `DataSource { engine: "executable" }` — runs shell commands.
    RunsCommands,
    /// `Qt.openUrlExternally` — opens URLs and files.
    OpensUrls,
    /// `org.kde.plasma.private.*` — shell internals (doesn't work outside Plasma,
    /// in Plasma — no guarantees).
    PrivateShellApi,
}

/// Analysis of a plasmoid's QML/JS for the permission dialog (`permissions: ["exec"]`).
pub fn plasmoid_risks(source: &str) -> Vec<PlasmoidRisk> {
    let compact: String = source.split_whitespace().collect();
    let mut out = Vec::new();
    if compact.contains("engine:\"executable\"") || compact.contains("engine:'executable'") {
        out.push(PlasmoidRisk::RunsCommands);
    }
    if compact.contains("Qt.openUrlExternally") {
        out.push(PlasmoidRisk::OpensUrls);
    }
    if compact.contains("org.kde.plasma.private.") {
        out.push(PlasmoidRisk::PrivateShellApi);
    }
    out
}

/// A found executable file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExecFinding {
    pub path: String,
    pub class: ExecClass,
}

/// Find everything executable in the archive (Rexon's scanner; the
/// `permissions` field has no say over it).
pub fn scan_executables(files: &[ArchiveFile]) -> Vec<ExecFinding> {
    files
        .iter()
        .filter_map(|f| classify_file(f).map(|class| ExecFinding { path: f.path.to_string(), class }))
        .collect()
}

/// What is wrong with an SVG.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SvgProblem {
    Script,
    ForeignObject,
    /// An outbound link (`href="http…"`, `//…`) or `javascript:`.
    ExternalLink,
}

/// SVG check: as a preview it may end up in a web engine.
pub fn svg_problems(data: &[u8]) -> Vec<SvgProblem> {
    let text = String::from_utf8_lossy(data).to_ascii_lowercase();
    let compact: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out = Vec::new();
    if compact.contains("<script") {
        out.push(SvgProblem::Script);
    }
    if compact.contains("<foreignobject") {
        out.push(SvgProblem::ForeignObject);
    }
    let external = ["href=\"http", "href='http", "href=\"//", "href='//", "javascript:", "url(http", "url(\"http", "url('http"]
        .iter()
        .any(|p| compact.replace("= ", "=").replace(" =", "=").contains(p));
    if external {
        out.push(SvgProblem::ExternalLink);
    }
    out
}

/// Which element (index) owns an archive path: under its `native`,
/// `substitute` or `model`.
fn owning_item(pkg: &Package, path: &str) -> Option<usize> {
    let under = |root: &str| {
        let root = root.trim_end_matches('/');
        root.is_empty() || root == "." || path == root || path.starts_with(&format!("{root}/"))
    };
    pkg.items.iter().position(|item| {
        item.native.values().chain(item.substitute.values()).chain(item.model.iter()).any(|r| under(r))
    })
}

/// Check the archive's content against the manifest: executable content —
/// only in elements with the `exec` permission; SVG — without scripts and
/// external links; native compositor configs — never.
pub fn check_contents(pkg: &Package, files: &[ArchiveFile]) -> Vec<ManifestError> {
    let mut e = Vec::new();
    for f in files {
        if is_compositor_config_path(f.path) {
            e.push(ManifestError::new(f.path, compositor_refusal()));
            continue;
        }
        if f.path.to_ascii_lowercase().ends_with(".svg") || f.path.to_ascii_lowercase().ends_with(".svgz") {
            let problems = svg_problems(f.head);
            if !problems.is_empty() {
                e.push(ManifestError::new(
                    f.path,
                    tr("the SVG image contains scripts or links to the internet"),
                ));
            }
        }
        if let Some(class) = classify_file(f) {
            let allowed = owning_item(pkg, f.path)
                .map(|i| &pkg.items[i])
                .is_some_and(|item| item.permissions.contains(&Permission::Exec));
            if !allowed {
                e.push(ManifestError::new(
                    f.path,
                    trf(
                        "the file runs code (%1), but its item does not declare the 'exec' permission",
                        &[&exec_class_label(class)],
                    ),
                ));
            }
        }
    }
    e
}

/// Class label for the person.
pub fn exec_class_label(class: ExecClass) -> String {
    match class {
        ExecClass::Conky => tr("Conky widget settings"),
        ExecClass::Lua => tr("Lua script"),
        ExecClass::JavaScript => tr("JavaScript"),
        ExecClass::WebPage => tr("web page with scripts"),
        ExecClass::Qml => tr("QML"),
        ExecClass::DesktopExec => tr("shortcut that runs a command"),
        ExecClass::Script => tr("script"),
        ExecClass::Binary => tr("program"),
    }
}

// ============================================================================
// Licenses
// ============================================================================

/// What a license permits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum LicenseClass {
    /// Free: may be distributed and modified.
    Free,
    /// "NC" — non-commercial only; not allowed in the Workshop.
    NonCommercial,
    /// "ND" — no derivatives.
    NoDerivatives,
    /// Explicitly non-free (`LicenseRef-Proprietary`, "All rights reserved").
    NonFree,
    /// No license.
    Missing,
    /// Rexon doesn't know this license and can't say what it permits.
    Unknown,
}

/// Free licenses Rexon recognises (SPDX, with `-only`/`-or-later`).
const FREE_LICENSES: &[&str] = &[
    "GPL-2.0", "GPL-3.0", "LGPL-2.0", "LGPL-2.1", "LGPL-3.0", "AGPL-3.0", "MIT", "MIT-0", "BSD-2-Clause",
    "BSD-3-Clause", "Apache-2.0", "MPL-2.0", "ISC", "Zlib", "Unlicense", "CC0-1.0", "CC-BY-3.0",
    "CC-BY-4.0", "CC-BY-SA-3.0", "CC-BY-SA-4.0", "OFL-1.1", "Artistic-2.0", "WTFPL", "EUPL-1.2",
];

fn single_license_class(id: &str) -> LicenseClass {
    let id = id.trim().trim_matches(|c| c == '(' || c == ')');
    let base = id.split(" WITH ").next().unwrap_or(id).trim();
    let base = base.trim_end_matches('+');
    let base = base.strip_suffix("-only").or_else(|| base.strip_suffix("-or-later")).unwrap_or(base);
    let upper = base.to_ascii_uppercase();
    if base.is_empty() || upper == "NONE" || upper == "NOASSERTION" {
        return LicenseClass::Missing;
    }
    if upper.starts_with("CC-") && upper.contains("-NC") {
        return LicenseClass::NonCommercial;
    }
    if upper.starts_with("CC-") && upper.contains("-ND") {
        return LicenseClass::NoDerivatives;
    }
    if upper.contains("PROPRIETARY") || upper.contains("ALL-RIGHTS-RESERVED") || upper == "LICENSEREF-NONFREE" {
        return LicenseClass::NonFree;
    }
    if FREE_LICENSES.iter().any(|f| f.eq_ignore_ascii_case(base)) {
        LicenseClass::Free
    } else {
        LicenseClass::Unknown
    }
}

/// Class of an SPDX expression. `A OR B` — the best of the options (the
/// recipient chooses), `A AND B` — the worst (both are needed). Parentheses
/// don't nest — such expressions don't occur in theme packages.
pub fn license_class(expr: Option<&str>) -> LicenseClass {
    let Some(expr) = expr.map(str::trim).filter(|s| !s.is_empty()) else {
        return LicenseClass::Missing;
    };
    let rank = |c: LicenseClass| match c {
        LicenseClass::Free => 0,
        LicenseClass::Unknown => 1,
        LicenseClass::NoDerivatives => 2,
        LicenseClass::NonCommercial => 3,
        LicenseClass::NonFree => 4,
        LicenseClass::Missing => 5,
    };
    expr.split(" OR ")
        .map(|alt| {
            alt.split(" AND ")
                .map(single_license_class)
                .max_by_key(|c| rank(*c))
                .unwrap_or(LicenseClass::Missing)
        })
        .min_by_key(|c| rank(*c))
        .unwrap_or(LicenseClass::Missing)
}

/// Individual licenses of an expression (`GPL-3.0-or-later AND CC-BY-SA-4.0` → two).
fn license_terms(expr: &str) -> Vec<String> {
    expr.split([' ', '(', ')'])
        .filter(|t| !t.is_empty() && !matches!(*t, "AND" | "OR" | "WITH"))
        .map(str::to_string)
        .collect()
}

/// License checks common to any package: a derivative only from a free one,
/// and it inherits the source's license (decision of 25.10, question 6).
fn validate_licenses(e: &mut Vec<ManifestError>, pkg: &Package) {
    for (i, src) in pkg.derived_from.iter().enumerate() {
        let field = format!("derived_from[{i}]");
        if license_class(src.license.as_deref()) != LicenseClass::Free {
            e.push(ManifestError::new(
                field.clone(),
                trf(
                    "'%1' is not under a free license, so a derived package cannot be made from it",
                    &[&src.what],
                ),
            ));
            continue;
        }
        let want = license_terms(src.license.as_deref().unwrap_or_default());
        let have = license_terms(pkg.license.as_deref().unwrap_or_default());
        if !want.iter().all(|w| have.contains(w)) {
            e.push(ManifestError::new(
                "license",
                trf(
                    "the package is derived from '%1' and must keep its license %2",
                    &[&src.what, &src.license.as_deref().unwrap_or_default()],
                ),
            ));
        }
    }
}

/// The `derived_from` record for a derivative (import of a foreign theme):
/// attribution of the source with its license. Non-free — an error with a reason.
pub fn derived_attribution(source: &Source) -> Result<Source, String> {
    match license_class(source.license.as_deref()) {
        LicenseClass::Free => Ok(source.clone()),
        LicenseClass::Missing => Err(trf(
            "'%1' has no license, so Rexon cannot make a derived package from it.",
            &[&source.what],
        )),
        _ => Err(trf(
            "The license of '%1' does not allow derived works to be shared, so Rexon cannot make a derived package from it.",
            &[&source.what],
        )),
    }
}

/// Whether the package may be published (Workshop, export): a license is
/// mandatory, both its own and those of third-party parts must allow
/// distribution; NC, ND, no license and unknown ones — not allowed (Rexon
/// warns and doesn't publish).
pub fn check_publishable(pkg: &Package) -> Vec<ManifestError> {
    let mut e = Vec::new();
    let mut check = |field: String, what: &str, lic: Option<&str>| {
        let msg = match license_class(lic) {
            LicenseClass::Free => return,
            LicenseClass::Missing => trf("%1 has no license: it can be installed here, but not shared", &[&what]),
            LicenseClass::NonCommercial => trf(
                "%1 is licensed for non-commercial use only — the Workshop does not allow that",
                &[&what],
            ),
            LicenseClass::NoDerivatives => trf(
                "%1 forbids changes (ND) — Rexon does not share such content",
                &[&what],
            ),
            LicenseClass::NonFree => trf("%1 is not free to share", &[&what]),
            LicenseClass::Unknown => trf(
                "Rexon does not know the license %1 of %2 and cannot tell whether it allows sharing",
                &[&lic.unwrap_or_default(), &what],
            ),
        };
        e.push(ManifestError::new(field, msg));
    };
    check("license".into(), &tr("The package"), pkg.license.as_deref());
    for (i, s) in pkg.sources.iter().chain(pkg.derived_from.iter()).enumerate() {
        check(format!("sources[{i}]"), &format!("'{}'", s.what), s.license.as_deref());
    }
    // Workshop — embedded files only, no downloads.
    for (i, s) in pkg.sources.iter().enumerate() {
        if s.is_fetched() {
            e.push(ManifestError::new(
                format!("sources[{i}]"),
                trf("'%1' is downloaded during installation — a shared package must contain its files", &[&s.what]),
            ));
        }
    }
    e
}

// ============================================================================
// Personal data on export
// ============================================================================

/// What kind of personal data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PrivateKind {
    HomePath,
    UserName,
    HostName,
    Email,
    Password,
    ApiKey,
    Coordinates,
    WifiName,
}

impl PrivateKind {
    /// Label for the list in `privacy.removed` and the dialog before saving.
    pub fn label(self) -> String {
        match self {
            PrivateKind::HomePath => tr("path to your home folder"),
            PrivateKind::UserName => tr("your user name"),
            PrivateKind::HostName => tr("the computer name"),
            PrivateKind::Email => tr("email address"),
            PrivateKind::Password => tr("password or mail login"),
            PrivateKind::ApiKey => tr("access key of an online service"),
            PrivateKind::Coordinates => tr("location coordinates"),
            PrivateKind::WifiName => tr("Wi-Fi network name"),
        }
    }
}

/// Found personal data: what and on which line (from 1). The value itself is
/// not repeated — it is visible in the file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PrivateFinding {
    pub kind: PrivateKind,
    pub line: usize,
}

/// What to scrub.
#[derive(Debug, Clone, Default)]
pub struct ScrubContext {
    /// `/home/<name>` without a trailing `/`.
    pub home: String,
    pub user: String,
    pub host: String,
}

/// Scrub result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scrubbed {
    pub text: String,
    /// What was replaced (once per kind).
    pub replaced: Vec<PrivateKind>,
}

/// In the text, replace the home path with `${HOME}`, the user name with
/// `${USER}`, the machine name with `${HOSTNAME}`. Names — only as whole
/// words and at least three letters long: a short name would match anything.
pub fn scrub_text(text: &str, ctx: &ScrubContext) -> Scrubbed {
    let mut out = text.to_string();
    let mut replaced = Vec::new();
    let home = ctx.home.trim_end_matches('/');
    if !home.is_empty() && out.contains(home) {
        out = replace_path_prefix(&out, home, "${HOME}");
        replaced.push(PrivateKind::HomePath);
    }
    for (word, var, kind) in [
        (ctx.user.as_str(), "${USER}", PrivateKind::UserName),
        (ctx.host.as_str(), "${HOSTNAME}", PrivateKind::HostName),
    ] {
        if word.chars().count() < 3 {
            continue;
        }
        let (next, n) = replace_word(&out, word, var);
        if n > 0 {
            out = next;
            replaced.push(kind);
        }
    }
    Scrubbed { text: out, replaced }
}

/// `home` as a path prefix: `/home/al` doesn't touch `/home/alex`.
pub(crate) fn replace_path_prefix(text: &str, home: &str, var: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(pos) = rest.find(home) {
        let after = &rest[pos + home.len()..];
        let boundary = after.chars().next().is_none_or(|c| !(c.is_alphanumeric() || c == '_' || c == '-' || c == '.'));
        out.push_str(&rest[..pos]);
        out.push_str(if boundary { var } else { home });
        rest = after;
    }
    out.push_str(rest);
    out
}

fn replace_word(text: &str, word: &str, var: &str) -> (String, usize) {
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let mut out = String::with_capacity(text.len());
    let mut n = 0;
    let mut rest = text;
    let mut prev: Option<char> = None;
    while let Some(pos) = rest.find(word) {
        let before = rest[..pos].chars().last().or(if pos == 0 { prev } else { None });
        let after = rest[pos + word.len()..].chars().next();
        out.push_str(&rest[..pos]);
        if before.is_none_or(|c| !is_word(c)) && after.is_none_or(|c| !is_word(c)) {
            out.push_str(var);
            n += 1;
        } else {
            out.push_str(word);
        }
        prev = word.chars().last();
        rest = &rest[pos + word.len()..];
    }
    out.push_str(rest);
    (out, n)
}

/// Find in widget settings (Conky, Cinnamon applets, `appletsrc`) what must
/// not be published: email addresses, IMAP/POP passwords and accounts, keys
/// for weather and other APIs, coordinates, Wi-Fi network names. Anything
/// found blocks exporting the widget until it is edited; it is not fixed
/// automatically. Only for widget settings: GNOME extension uuids look like
/// email addresses.
pub fn find_secrets(text: &str) -> Vec<PrivateFinding> {
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let lower = line.to_ascii_lowercase();
        let mut push = |kind| {
            if !out.iter().any(|f: &PrivateFinding| f.kind == kind && f.line == i + 1) {
                out.push(PrivateFinding { kind, line: i + 1 });
            }
        };
        if has_email(line) {
            push(PrivateKind::Email);
        }
        if has_assignment(&lower, &["password", "passwd", "pass"], 1)
            || ["${imap", "${pop3", "imap =", "pop3 =", "imap=", "pop3="].iter().any(|p| lower.contains(p))
        {
            push(PrivateKind::Password);
        }
        if has_assignment(&lower, &["api_key", "apikey", "api-key", "appid", "token", "secret", "key"], 16) {
            push(PrivateKind::ApiKey);
        }
        if has_assignment(&lower, &["lat", "lon", "lng", "latitude", "longitude"], 3)
            && lower.chars().any(|c| c.is_ascii_digit())
        {
            push(PrivateKind::Coordinates);
        }
        if has_assignment(&lower, &["ssid", "essid", "wifi", "wlan_name"], 1) && !lower.contains("${wireless_essid") {
            push(PrivateKind::WifiName);
        }
    }
    out
}

/// The line has `<key>` (as a whole word) followed by `=`/`:`/`&…=` and a
/// value at least `min_len` characters long.
fn has_assignment(lower: &str, keys: &[&str], min_len: usize) -> bool {
    let bytes = lower.as_bytes();
    for key in keys {
        let mut from = 0;
        while let Some(rel) = lower[from..].find(key) {
            let start = from + rel;
            let end = start + key.len();
            from = end;
            let before_ok = start == 0 || !(bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_');
            if !before_ok {
                continue;
            }
            let rest = lower[end..].trim_start_matches([' ', '\t', '"', '\'']);
            let Some(value) = rest.strip_prefix(['=', ':']) else { continue };
            let value = value.trim_start_matches([' ', '\t', '"', '\'']);
            let len = value
                .chars()
                .take_while(|c| !c.is_whitespace() && !matches!(c, '&' | '"' | '\'' | ',' | ';' | '}' | ')'))
                .count();
            if len >= min_len && !value.starts_with("${") {
                return true;
            }
        }
    }
    false
}

/// Whether the line has an email address (`name@domain.tld`). GNOME extension
/// uuids (`dash-to-panel@jderose9.github.com`) look the same, so
/// [`find_secrets`] is called only for widget settings, not for extension
/// lists.
fn has_email(line: &str) -> bool {
    let is_local = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '+' | '-');
    for (pos, _) in line.match_indices('@') {
        let local: String = line[..pos].chars().rev().take_while(|c| is_local(*c)).collect();
        let domain: String = line[pos + 1..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-'))
            .collect();
        let domain = domain.trim_end_matches('.');
        let tld_ok = domain
            .rsplit_once('.')
            .is_some_and(|(_, tld)| tld.len() >= 2 && tld.chars().all(|c| c.is_ascii_alphabetic()));
        if !local.is_empty() && tld_ok {
            return true;
        }
    }
    false
}

/// Files never taken on «Save my desktop»: history, recent files, bookmarks,
/// one's own launchers with scripts. `rel` — path from the home directory.
pub fn never_export(rel: &str) -> bool {
    let rel = rel.trim_start_matches("./").trim_start_matches('/');
    let lower = rel.to_ascii_lowercase();
    let base = lower.rsplit('/').next().unwrap_or(&lower);
    base.contains("history")
        || base.starts_with("recently-used")
        || base.contains("bookmarks")
        || base == "places.xbel"
        || base.starts_with("user-places")
        || lower.starts_with(".local/share/applications/")
        || lower.starts_with(".local/share/recently-used")
        || lower.contains("/recent")
        || lower.starts_with(".ssh/")
        || lower.starts_with(".gnupg/")
        || lower.contains("keyrings/")
        || lower.contains("kwallet")
}


#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// An example "the whole desktop look", with `exec` declared for a Cinnamon
    /// applet — without it the package must fail.
    fn full_look() -> Value {
        json!({
            "format_version": 5,
            "id": "nord-desk",
            "version": "1.2.0",
            "name": {"en": "Nord Desk", "ru": "Норд"},
            "description": {"en": "Calm arctic look for any desktop"},
            "authors": [{"name": "Wehrwolfmann", "url": "https://example.org"}],
            "license": "GPL-3.0-or-later",
            "sources": [{"what": "Nord palette", "url": "https://www.nordtheme.com", "license": "MIT"}],
            "rexon_version": ">=0.0.1",
            "preview": "preview.png",
            "tags": ["dark", "minimal"],
            "properties": {
                "radius": {"type": "slider", "min": 0, "max": 16, "default": 8},
                "accent": {"type": "color", "default": "#88c0d0", "allow": ["from-wallpaper"]},
                "panel_edge": {"type": "combo", "options": ["bottom", "top"], "default": "bottom"}
            },
            "items": [
                {"kind": "palette", "model": "model/palette.json", "native": {"kde-colors": "native/kde-colors/Nord.colors"}},
                {"kind": "window-decoration", "model": "model/decoration.json",
                 "native": {"aurorae": "native/aurorae/NordDesk", "xfwm4": "native/xfwm4/NordDesk"}},
                {"kind": "panel-layout", "model": "model/panel.json", "required": false},
                {"kind": "icons", "native": {"freedesktop": "native/icons/Nordzy"}},
                {"kind": "gtk-theme", "native": {"gtk": "native/gtk/Nordic"}, "required": false},
                {"kind": "widget-conky", "native": {"conky": "widgets/clock"}, "permissions": ["exec"], "required": false},
                {"kind": "cinnamon-applet", "native": {"cinnamon": "native/cinnamon/weather@x"},
                 "role": "weather", "substitute": {"conky": "widgets/weather"}, "permissions": ["exec"], "required": false}
            ],
            "requires": {
                "packages": [
                    {"role": "gtk3-adwaita-base", "for": ["gtk-theme"], "required": false,
                     "arch": ["adw-gtk-theme"], "fedora": ["adw-gtk3-theme"], "debian": null},
                    {"role": "dash-to-panel", "for": ["panel-layout"], "source": "gnome-extensions",
                     "uuid": "dash-to-panel@jderose9.github.com"},
                    {"role": "game-session", "source": "aur", "required": false,
                     "aur": ["gamescope-session-steam-git"], "forbid": ["jupiter-hw-support"]}
                ],
                "targets": {"cinnamon": ">=6.0"}
            },
            "privacy": {"exported_by": "rexon", "scrubbed": true, "removed": ["path to your home folder"]},
            "workshop": {"published_file_id": 3_100_000_000u64, "tags": ["Look", "KDE"], "visibility": "unlisted"}
        })
    }

    fn with(mut v: Value, f: impl FnOnce(&mut Value)) -> Value {
        f(&mut v);
        v
    }

    fn errs(v: &Value) -> Vec<ManifestError> {
        parse(v).expect_err("пакет должен быть отвергнут")
    }

    fn has_err(v: &Value, field: &str) -> bool {
        errs(v).iter().any(|e| e.field.starts_with(field))
    }

    fn session(consumers: &[&str]) -> Session {
        Session { consumers: consumers.iter().map(|s| s.to_string()).collect(), versions: BTreeMap::new() }
    }

    fn no_gen(_: &str, _: &str) -> Option<Accuracy> {
        None
    }

    // ── valid manifests ──────────────────────────────────────────────────

    #[test]
    fn full_look_parses() {
        let pkg = parse(&full_look()).unwrap_or_else(|e| panic!("{}", errors_text(&e)));
        assert_eq!(pkg.source_format, 5);
        assert_eq!(pkg.name.get("ru"), "Норд");
        assert_eq!(pkg.name.get("de_DE"), "Nord Desk");
        assert_eq!(pkg.items.len(), 7);
        assert!(pkg.items[0].generate && pkg.items[0].required);
        assert!(!pkg.items[2].required);
        // property order — as in the file, not alphabetical
        let names: Vec<&str> = pkg.properties.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(names, ["radius", "accent", "panel_edge"]);
        assert_eq!(pkg.property("accent").unwrap().kind, PropertyType::Color);
        assert_eq!(pkg.workshop.as_ref().unwrap().visibility, Visibility::Unlisted);
        assert_eq!(pkg.deps_for("gtk-theme").len(), 2, "своя + общая на пакет (AUR без for)");
        assert!(pkg.has_executable());
    }

    #[test]
    fn roundtrip_keeps_model() {
        let pkg = parse(&full_look()).unwrap();
        let again = parse(&serde_json::to_value(&pkg).unwrap()).unwrap();
        assert_eq!(pkg, again);
    }

    #[test]
    fn minimal_single_item_package() {
        let v = json!({"format_version": 5, "id": "cursors", "name": "Cursors",
                       "items": [{"kind": "cursors", "native": {"freedesktop": "native/cursors/X"}}]});
        let pkg = parse(&v).unwrap();
        assert_eq!(pkg.version, "0.0.0");
        assert!(pkg.workshop.is_none() && pkg.license.is_none());
    }

    // ── invalid manifests ────────────────────────────────────────────────

    #[test]
    fn rejects_not_object_and_future_format() {
        assert_eq!(errs(&json!([1])).len(), 1);
        let e = errs(&with(full_look(), |v| v["format_version"] = json!(6)));
        assert_eq!(e[0].field, "format_version");
    }

    #[test]
    fn rejects_bad_id_and_name() {
        assert!(has_err(&with(full_look(), |v| v["id"] = json!("../evil")), "id"));
        assert!(has_err(&with(full_look(), |v| v["id"] = json!("-x")), "id"));
        assert!(has_err(&with(full_look(), |v| v["name"] = json!({"en": "  "})), "name"));
    }

    #[test]
    fn collects_all_errors_at_once() {
        let v = with(full_look(), |v| {
            v["id"] = json!("a/b");
            v["items"][0]["kind"] = json!("toaster");
            v["items"][3]["native"]["freedesktop"] = json!("../../etc");
        });
        let e = errs(&v);
        assert!(e.len() >= 3, "{}", errors_text(&e));
    }

    #[test]
    fn rejects_paths_outside_package() {
        for bad in ["/etc/passwd", "../x", "a/../../x", "a\\b", ""] {
            let v = with(full_look(), |v| v["items"][0]["model"] = json!(bad));
            assert!(has_err(&v, "items[0].model"), "{bad:?}");
        }
        assert!(has_err(&with(full_look(), |v| v["preview"] = json!("/tmp/p.png")), "preview"));
    }

    #[test]
    fn rejects_unknown_kind_and_empty_items() {
        assert!(has_err(&with(full_look(), |v| v["items"][1]["kind"] = json!("theme-xfwm4")), "items[1].kind"));
        assert!(has_err(&with(full_look(), |v| v["items"] = json!([])), "items"));
    }

    #[test]
    fn rejects_item_without_payload_or_forbidden_generation_without_files() {
        let v = with(full_look(), |v| v["items"][2] = json!({"kind": "panel-layout"}));
        assert!(has_err(&v, "items[2]"));
        let v = with(full_look(), |v| v["items"][2]["generate"] = json!(false));
        assert!(has_err(&v, "items[2].generate"));
        let v = with(full_look(), |v| v["items"][6].as_object_mut().unwrap().remove("role").map(|_| ()).unwrap());
        assert!(has_err(&v, "items[6].role"));
    }

    #[test]
    fn executable_kind_needs_exec_permission() {
        let v = with(full_look(), |v| v["items"][6]["permissions"] = json!([]));
        assert!(has_err(&v, "items[6].permissions"));
    }

    #[test]
    fn compositor_configs_never_accepted() {
        let v = with(full_look(), |v| v["items"][1]["native"]["hyprland"] = json!("native/hypr/look.conf"));
        assert!(has_err(&v, "items[1].native.hyprland"));
        let v = with(full_look(), |v| v["items"][1]["native"]["xfwm4"] = json!("native/sway/config"));
        assert!(has_err(&v, "items[1].native.xfwm4"));
        for p in ["x/hypr/hyprland.conf", "niri/config.kdl", "labwc/rc.xml", "river/init", "a/wayfire.ini"] {
            assert!(is_compositor_config_path(p), "{p}");
        }
        for p in ["native/openbox-3/Nord/openbox-3/themerc", "native/labwc/themerc-override", "native/gtk/Nordic/gtk.css"] {
            assert!(!is_compositor_config_path(p), "{p}");
        }
    }

    #[test]
    fn rejects_bad_properties() {
        let v = with(full_look(), |v| v["properties"]["radius"]["default"] = json!(40));
        assert!(has_err(&v, "properties.radius"));
        let v = with(full_look(), |v| v["properties"]["accent"]["default"] = json!("blue"));
        assert!(has_err(&v, "properties.accent"));
        let v = with(full_look(), |v| v["properties"]["panel_edge"]["default"] = json!("left"));
        assert!(has_err(&v, "properties.panel_edge"));
        let v = with(full_look(), |v| v["properties"]["bad name"] = json!({"type": "toggle", "default": true}));
        assert!(has_err(&v, "properties.bad name"));
    }

    #[test]
    fn rejects_bad_targets() {
        let v = with(full_look(), |v| v["requires"]["targets"]["cinnamon"] = json!("new enough"));
        assert!(has_err(&v, "requires.targets.cinnamon"));
    }

    // ── plasmoids outside Plasma ─────────────────────────────────────────

    fn plasmoid() -> Value {
        json!({"kind": "plasmoid", "native": {"plasma": "native/plasmoids/org.example.clock"},
               "role": "clock", "substitute": {"conky": "widgets/clock"},
               "windowed": {"allowed": true, "min_size": [200, 200], "default_geometry": [40, 40, 260, 260],
                            "needs": ["plasma5support"], "hint_layer": "below"},
               "permissions": ["exec"], "required": false})
    }

    fn with_item(it: Value) -> Value {
        with(full_look(), |v| v["items"][2] = it)
    }

    #[test]
    fn plasmoid_with_window_block_parses() {
        let pkg = parse(&with_item(plasmoid())).unwrap_or_else(|e| panic!("{}", errors_text(&e)));
        let w = pkg.items[2].windowed.as_ref().unwrap();
        assert_eq!(w.sessions, [SessionKind::X11], "по умолчанию — только X11");
        assert_eq!(w.hint_layer, Some(HintLayer::Below));
        // Outside Plasma — the Conky substitute, in Plasma — the native one.
        let it = &pkg.items[2];
        assert!(matches!(choose(it, &BTreeMap::new(), &session(&["conky", "xfwm4"]), &no_gen), Choice::Substitute { .. }));
        assert!(matches!(choose(it, &BTreeMap::new(), &session(&["plasma"]), &no_gen), Choice::Native { .. }));
    }

    #[test]
    fn plasmoid_must_be_optional_with_substitute() {
        assert!(has_err(&with_item(with(plasmoid(), |v| v["required"] = json!(true))), "items[2].required"));
        let no_sub = with(plasmoid(), |v| {
            v.as_object_mut().unwrap().remove("substitute");
        });
        assert!(has_err(&with_item(no_sub), "items[2].substitute"));
        assert!(has_err(&with_item(with(plasmoid(), |v| v["permissions"] = json!([]))), "items[2].permissions"));
        // The same plasmoid without a substitute but declared as v5 is rejected.
        let strict = with_item(json!({"kind": "plasmoid", "native": {"plasma": "p"},
                                      "permissions": ["exec"], "required": false}));
        assert!(has_err(&strict, "items[2].substitute"));
    }

    #[test]
    fn legacy_plasmoid_is_optional_and_skipped_outside_plasma() {
        let single = parse(&json!({"id": "w", "type": "theme-plasmoid", "name": "W"})).unwrap();
        let bundle = parse(&json!({"id": "b", "type": "theme-bundle", "name": "B",
                                   "bundle": {"theme-gtk": "gtk", "theme-plasmoid": "plasmoids"}}))
        .unwrap();
        for pkg in [&single, &bundle] {
            let it = pkg.items.iter().find(|i| i.kind == "plasmoid").unwrap();
            assert!(!it.required && it.substitute.is_empty());
            assert!(validate(pkg).is_empty(), "{}", errors_text(&validate(pkg)));
            let plan = choose_all(pkg, &session(&["xfwm4", "gtk"]), &no_gen);
            assert!(plan.installable(), "вне Plasma пункт пропускается, пакет не блокируется");
            assert_eq!(plan.skipped().len(), 1);
            let plan = choose_all(pkg, &session(&["plasma", "gtk"]), &no_gen);
            assert!(plan.skipped().is_empty());
        }
        // Re-saved as v5 (as a model without `parse`) — strict rules.
        let mut resaved = single.clone();
        resaved.source_format = 0;
        assert!(!validate(&resaved).is_empty());
    }

    #[test]
    fn plasma_only_roles() {
        let tm = with(plasmoid(), |v| {
            v["role"] = json!("taskmanager");
            v.as_object_mut().unwrap().remove("substitute");
        });
        assert!(has_err(&with_item(tm.clone()), "items[2].windowed.allowed"));
        let tm = with(tm, |v| v["windowed"]["allowed"] = json!(false));
        assert!(parse(&with_item(tm.clone())).is_ok(), "роль оболочки: замена не нужна, окна нет");
        let pkg = parse(&with_item(tm)).unwrap();
        assert!(choose(&pkg.items[2], &BTreeMap::new(), &session(&["xfwm4"]), &no_gen).is_refused());
        for r in ["pager", "systemtray", "appmenu", "kickoff", "kicker", "minimizeall", "showdesktop"] {
            assert!(is_plasma_only_role(r));
        }
        assert!(!is_plasma_only_role("clock"));
    }

    #[test]
    fn windowed_block_checks() {
        let bad = |f: &str, val: Value| with_item(with(plasmoid(), |v| v["windowed"][f] = val));
        assert!(has_err(&bad("sessions", json!([])), "items[2].windowed.sessions"));
        assert!(has_err(&bad("min_size", json!([0, 10])), "items[2].windowed.min_size"));
        assert!(has_err(&bad("default_geometry", json!([0, 0, 100, 100])), "items[2].windowed.default_geometry"));
        assert!(has_err(&bad("needs", json!(["Bad Name"])), "items[2].windowed.needs"));
        assert!(errs(&bad("hint_layer", json!("top"))).len() == 1);
        let not_plasmoid = with(full_look(), |v| v["items"][3]["windowed"] = json!({"allowed": true}));
        assert!(has_err(&not_plasmoid, "items[3].windowed"));
    }

    #[test]
    fn plasmoid_qml_risks() {
        let qml = r#"
            import org.kde.plasma.private.taskmanager
            P5Support.DataSource { engine: "executable"; connectedSources: ["uptime"] }
            onClicked: Qt.openUrlExternally("https://example.org")"#;
        assert_eq!(
            plasmoid_risks(qml),
            [PlasmoidRisk::RunsCommands, PlasmoidRisk::OpensUrls, PlasmoidRisk::PrivateShellApi]
        );
        assert!(plasmoid_risks("Text { text: Qt.formatTime(new Date()) }").is_empty());
    }

    // ── Workshop ─────────────────────────────────────────────────────────

    #[test]
    fn workshop_never_carries_app_id() {
        let v = with(full_look(), |v| v["workshop"]["app_id"] = json!(480));
        assert!(has_err(&v, "workshop.app_id"));
        let v = with(full_look(), |v| v["workshop"]["published_file_id"] = json!(0));
        assert!(has_err(&v, "workshop.published_file_id"));
        let v = with(full_look(), |v| v["workshop"]["tags"] = json!(["a,b"]));
        assert!(has_err(&v, "workshop.tags"));
        let v = with(full_look(), |v| v["workshop"] = json!({}));
        assert_eq!(parse(&v).unwrap().workshop.unwrap().visibility, Visibility::Private);
    }

    #[test]
    fn workshop_ref_item() {
        let ok = with(full_look(), |v| v["items"].as_array_mut().unwrap().push(json!({"kind": "ref", "workshop": 12345, "required": false})));
        let pkg = parse(&ok).unwrap();
        let plan = choose_all(&pkg, &session(&["aurorae", "kde-colors", "gtk"]), &no_gen);
        assert!(!plan.choices[7].is_refused());
        assert!(has_err(&with(full_look(), |v| v["items"][0] = json!({"kind": "ref"})), "items[0].workshop"));
    }

    // ── v3 / v4 translation ──────────────────────────────────────────────

    #[test]
    fn v3_single_type_becomes_one_item() {
        let v = json!({"id": "nordic", "type": "theme-gtk", "name": "Nordic", "version": "2.0",
                       "author": "EliverLara", "license": "GPL-3.0", "tags": ["dark"]});
        let pkg = parse(&v).unwrap();
        assert_eq!(pkg.source_format, constants::FORMAT_VERSION);
        assert_eq!(pkg.format_version, V5_FORMAT_VERSION);
        assert_eq!(pkg.items.len(), 1);
        let item = &pkg.items[0];
        assert_eq!(item.kind, "gtk-theme");
        assert_eq!(item.native.get("gtk").map(String::as_str), Some("data/"));
        assert!(!item.generate, "у v3 нет модели — генерировать не из чего");
        assert_eq!(pkg.authors[0].name, "EliverLara");
        assert!(validate(&pkg).is_empty(), "{}", errors_text(&validate(&pkg)));
        assert_eq!(legacy_type(&item.kind, "gtk"), Some("theme-gtk"));
    }

    #[test]
    fn v3_executable_type_gets_exec() {
        let pkg = parse(&json!({"id": "clock", "type": "theme-conky", "name": "Clock"})).unwrap();
        assert_eq!(pkg.items[0].permissions, [Permission::Exec]);
        assert!(validate(&pkg).is_empty());
    }

    #[test]
    fn v3_errors_reuse_old_checks() {
        let e = errs(&json!({"type": "theme-gtk"}));
        assert_eq!(e[0].field, "id");
        let e = errs(&json!({"id": "a/b", "type": "theme-gtk"}));
        assert!(e[0].message.contains("a/b"));
        let e = errs(&json!({"id": "x", "type": "toaster"}));
        assert_eq!(e[0].field, "type");
        let e = errs(&json!({"id": "x", "type": "theme-gtk", "rexon_version": ">=99.0.0"}));
        assert_eq!(e[0].field, "rexon_version");
    }

    #[test]
    fn v3_bundle_becomes_items_and_skips_like_installer() {
        let v = json!({"id": "pack", "type": "theme-bundle", "name": "Pack", "bundle": {
            "theme-gtk": "gtk", "theme-icons": "icons", "theme-bundle": "nested", "toaster": "x", "theme-xfwm4": "xfwm"}});
        let pkg = parse(&v).unwrap();
        let kinds: Vec<(&str, &str)> = pkg
            .items
            .iter()
            .map(|i| (i.kind.as_str(), i.native.values().next().unwrap().as_str()))
            .collect();
        assert_eq!(kinds, [("gtk-theme", "gtk"), ("icons", "icons"), ("window-decoration", "xfwm")]);
        assert!(validate(&pkg).is_empty());
        assert!(errs(&json!({"id": "p", "type": "theme-bundle", "bundle": {"theme-gtk": "../x"}}))[0]
            .field
            .starts_with("bundle"));
        assert_eq!(errs(&json!({"id": "p", "type": "theme-bundle", "bundle": {}}))[0].field, "bundle");
    }

    fn v4() -> Value {
        json!({"format_version": 4, "type": "wallpaper-video-adaptive", "title": "Rainy Night",
               "videos": {"1080p": "v/1080.mp4"}, "fallback_video": "v/1080.mp4", "default_locale": "en"})
    }

    #[test]
    fn v4_becomes_adaptive_item_with_derived_id() {
        let pkg = parse(&v4()).unwrap();
        assert_eq!(pkg.source_format, 4);
        assert_eq!(pkg.id, "v4-rainy-night");
        assert_eq!(pkg.name.get("en"), "Rainy Night");
        assert_eq!(pkg.items[0].kind, "live-wallpaper-adaptive");
        assert_eq!(pkg.items[0].native.get("rexon-wallpaper").map(String::as_str), Some("."));
        // the archive root is a valid path
        assert!(validate(&pkg).is_empty(), "{}", errors_text(&validate(&pkg)));
    }

    #[test]
    fn v4_schema_errors_come_from_validate_v4() {
        let e = errs(&with(v4(), |v| v["default_locale"] = json!("english")));
        assert!(e[0].message.contains("default_locale"), "{}", e[0]);
    }

    /// Guard: every matrix type and the script types translate to v5, and the
    /// kinds in the table come from the closed list.
    #[test]
    fn legacy_table_covers_every_valid_type() {
        for t in constants::valid_types() {
            if t == "theme-bundle" {
                continue;
            }
            assert!(legacy_kind(t).is_some(), "тип {t} не переводится в v5 — строка в LEGACY_TYPES");
        }
        for (t, kind, consumer) in LEGACY_TYPES {
            assert!(constants::is_valid_type(t), "{t} — нет такого типа");
            assert!(is_kind(kind), "{kind}");
            assert!(is_consumer_name(consumer), "{consumer}");
            assert_eq!(constants::is_scripted(t) || kind_is_executable(kind), kind_is_executable(kind), "{t}");
        }
        for k in EXECUTABLE_KINDS {
            assert!(is_kind(k));
        }
    }

    // ── choice rules ─────────────────────────────────────────────────────

    fn item(v: Value) -> Item {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn choose_native_first() {
        let it = item(json!({"kind": "window-decoration", "model": "m.json",
                             "native": {"aurorae": "a", "xfwm4": "x"}}));
        let c = choose(&it, &BTreeMap::new(), &session(&["xfwm4", "gtk"]), &|_, _| Some(Accuracy::Exact));
        assert_eq!(c, Choice::Native { consumer: "xfwm4".into(), path: "x".into() });
    }

    #[test]
    fn choose_compatible_then_generate() {
        let it = item(json!({"kind": "window-decoration", "model": "m.json", "native": {"gtk": "g"}}));
        let c = choose(&it, &BTreeMap::new(), &session(&["gtk-ssd"]), &no_gen);
        assert_eq!(c, Choice::Compatible { consumer: "gtk-ssd".into(), from: "gtk".into(), path: "g".into() });
        let c = choose(&it, &BTreeMap::new(), &session(&["metacity-1"]), &|k, c| {
            (k == "window-decoration" && c == "metacity-1").then_some(Accuracy::Approximate)
        });
        assert_eq!(
            c,
            Choice::Generate { consumer: "metacity-1".into(), model: "m.json".into(), accuracy: Accuracy::Approximate }
        );
    }

    #[test]
    fn choose_universal_consumer_always_present() {
        let it = item(json!({"kind": "icons", "native": {"freedesktop": "i"}}));
        assert!(matches!(choose(&it, &BTreeMap::new(), &session(&[]), &no_gen), Choice::Native { .. }));
    }

    #[test]
    fn choose_substitute_then_refuse_with_reason() {
        let it = item(json!({"kind": "cinnamon-applet", "native": {"cinnamon": "c"}, "role": "weather",
                             "substitute": {"conky": "w"}, "permissions": ["exec"]}));
        let c = choose(&it, &BTreeMap::new(), &session(&["conky", "aurorae"]), &no_gen);
        assert_eq!(c, Choice::Substitute { consumer: "conky".into(), path: "w".into(), role: "weather".into() });
        let c = choose(&it, &BTreeMap::new(), &session(&["aurorae"]), &no_gen);
        assert_eq!(c, Choice::Refuse { reason: RefuseReason::NoVariant });

        let forbidden = item(json!({"kind": "window-decoration", "model": "m", "generate": false, "native": {"aurorae": "a"}}));
        let c = choose(&forbidden, &BTreeMap::new(), &session(&["xfwm4"]), &|_, _| Some(Accuracy::Exact));
        assert_eq!(c, Choice::Refuse { reason: RefuseReason::GenerationForbidden });

        let no_generator = item(json!({"kind": "panel-layout", "model": "m"}));
        let c = choose(&no_generator, &BTreeMap::new(), &session(&["xfce4-panel"]), &no_gen);
        assert_eq!(c, Choice::Refuse { reason: RefuseReason::NoGenerator });
        assert!(!RefuseReason::NoGenerator.text().is_empty());
    }

    #[test]
    fn native_respects_target_version() {
        let it = item(json!({"kind": "shell-theme", "model": "m", "native": {"cinnamon": "c"}}));
        let targets = BTreeMap::from([("cinnamon".to_string(), ">=6.0".to_string())]);
        let mut s = session(&["cinnamon"]);
        s.versions.insert("cinnamon".into(), "5.8.4".into());
        let c = choose(&it, &targets, &s, &|_, _| Some(Accuracy::Exact));
        assert!(matches!(c, Choice::Generate { .. }), "старый Cinnamon — родной вариант не годится: {c:?}");
        s.versions.insert("cinnamon".into(), "6.4.8".into());
        assert!(matches!(choose(&it, &targets, &s, &no_gen), Choice::Native { .. }));
        s.versions.clear();
        assert!(matches!(choose(&it, &targets, &s, &no_gen), Choice::Native { .. }), "версия неизвестна — годен");
    }

    #[test]
    fn package_all_or_nothing() {
        let pkg = parse(&full_look()).unwrap();
        // KDE: palette and frames are native, the panel without a generator is optional.
        let kde = session(&["kde-colors", "aurorae", "plasma", "gtk"]);
        let plan = choose_all(&pkg, &kde, &no_gen);
        assert!(plan.installable(), "{plan:?}");
        let skipped: Vec<usize> = plan.skipped().iter().map(|(i, _)| *i).collect();
        assert_eq!(skipped, [2, 5, 6]);
        // XFCE without a palette generator: the mandatory palette doesn't install — so neither does the package.
        let xfce = session(&["xfwm4", "xfce4-panel", "gtk"]);
        let plan = choose_all(&pkg, &xfce, &no_gen);
        assert_eq!(plan.blocked_by, Some(0));
        // With a palette generator — it installs, the palette is "approximate".
        let plan = choose_all(&pkg, &xfce, &|k, _| (k == "palette").then_some(Accuracy::Approximate));
        assert!(plan.installable());
        assert!(matches!(plan.choices[0], Choice::Generate { accuracy: Accuracy::Approximate, .. }));
        assert!(!Accuracy::Approximate.label().is_empty());
    }

    #[test]
    fn version_compare() {
        assert_eq!(version_satisfies("6.4.8", ">=6.0"), Some(true));
        assert_eq!(version_satisfies("0.55.9", "<0.56"), Some(true));
        assert_eq!(version_satisfies("47", "47"), Some(true));
        assert_eq!(version_satisfies("47.1", "=47"), Some(false));
        assert_eq!(version_satisfies("6", ">6.0"), Some(false));
        assert_eq!(version_satisfies("x", ">=1"), None);
    }

    // ── dependencies ─────────────────────────────────────────────────────

    #[test]
    fn dependency_names_and_sources() {
        let pkg = parse(&full_look()).unwrap();
        let [repo, ego, aur] = &pkg.requires.packages[..] else { panic!() };
        assert_eq!(repo.names_for(PkgFamily::Arch).unwrap(), ["adw-gtk-theme"]);
        assert_eq!(repo.names_for(PkgFamily::Debian), None, "null — в семействе нет");
        assert_eq!(repo.names_for(PkgFamily::Suse), None);
        assert_eq!(aur.install_names(PkgFamily::Arch).unwrap(), ["aur/gamescope-session-steam-git"]);
        assert_eq!(aur.install_names(PkgFamily::Fedora), None);
        assert_eq!(ego.uuid.as_deref(), Some("dash-to-panel@jderose9.github.com"));
        assert!(DepSource::Repo.installs_automatically());
        assert!(DepSource::GnomeExtensions.installs_automatically());
        assert!(!DepSource::Aur.installs_automatically());
    }

    #[test]
    fn dependency_validation() {
        let dep = |d: Value| with(full_look(), |v| v["requires"]["packages"] = json!([d]));
        assert!(has_err(&dep(json!({"role": "x", "arch": ["--overwrite=*"]})), "requires.packages[0].arch"));
        assert!(has_err(&dep(json!({"role": "x", "arch": ["/tmp/evil.pkg.tar.zst"]})), "requires.packages[0].arch"));
        assert!(has_err(&dep(json!({"role": "x", "aur": ["foo"]})), "requires.packages[0].aur"));
        assert!(has_err(&dep(json!({"role": "x", "ubuntu": ["foo"]})), "requires.packages[0].ubuntu"));
        assert!(has_err(&dep(json!({"role": "x"})), "requires.packages[0]"));
        assert!(has_err(&dep(json!({"role": "x", "source": "aur", "arch": ["foo"]})), "requires.packages[0]"));
        assert!(has_err(&dep(json!({"role": "x", "source": "gnome-extensions", "uuid": "no-at-sign"})), "requires.packages[0].uuid"));
        assert!(has_err(&dep(json!({"role": "x", "source": "gnome-extensions", "uuid": "a@b/../c"})), "requires.packages[0].uuid"));
        assert!(has_err(&dep(json!({"role": "x", "arch": ["a"], "uuid": "a@b"})), "requires.packages[0].uuid"));
        assert!(has_err(&dep(json!({"role": "", "arch": ["a"]})), "requires.packages[0].role"));
        assert!(has_err(&dep(json!({"role": "x", "for": ["toaster"], "arch": ["a"]})), "requires.packages[0].for"));
        assert!(parse(&dep(json!({"role": "x", "fedora": ["@gnome-desktop"]}))).is_ok());
    }

    #[test]
    fn provider_substitution_is_refused() {
        let pkg = parse(&full_look()).unwrap();
        let aur = &pkg.requires.packages[2];
        let r = aur.resolve(PkgFamily::Arch, &|_| false).unwrap();
        let empty = |_: &str| None;
        let exact = |n: &str| (n == "gamescope-session-steam-git").then(|| n.to_string());
        assert!(verify_dependency(aur, &r, &empty, &exact).is_ok());
        let provider = |n: &str| (n == "gamescope-session-steam-git").then(|| "gamescope-session-cachyos".to_string());
        assert!(verify_dependency(aur, &r, &empty, &provider).unwrap_err().contains("gamescope-session-cachyos"));
        // The substitute was installed beforehand — still a substitution.
        assert!(verify_dependency(aur, &r, &provider, &provider).is_err());
        let pulled = |n: &str| Some(n.to_string());
        assert!(verify_dependency(aur, &r, &empty, &pulled).unwrap_err().contains("jupiter-hw-support"));
        // jupiter-hw-support was installed before the install — we didn't bring it.
        let had = |n: &str| (n == "jupiter-hw-support").then(|| n.to_string());
        assert!(verify_dependency(aur, &r, &had, &pulled).is_ok());
        assert!(verify_dependency(aur, &r, &empty, &empty).is_err());
    }

    #[test]
    fn package_forbid_and_preconditions() {
        let v = with(full_look(), |v| {
            v["requires"]["forbid"] = json!(["plasma-login-manager"]);
            v["requires"]["preconditions"] = json!([
                {"name": "libplasma", "owners": ["libplasma"],
                 "message": {"en": "Fork of libplasma", "ru": "Форк libplasma"}},
                {"name": "steam", "installed": true},
                {"name": "jupiter-hw-support", "installed": false}
            ]);
        });
        let pkg = parse(&v).unwrap_or_else(|e| panic!("{}", errors_text(&e)));
        let again = parse(&serde_json::to_value(&pkg).unwrap()).unwrap();
        assert_eq!(pkg, again);

        // The "must not appear" ban: present before — fine, appeared — refusal.
        let plm = |n: &str| (n == "plasma-login-manager").then(|| n.to_string());
        let none = |_: &str| None;
        assert!(verify_package_change(&pkg, &plm, &plm).is_ok());
        assert!(verify_package_change(&pkg, &none, &plm).unwrap_err().contains("plasma-login-manager"));
        assert!(verify_package_change(&pkg, &none, &none).is_ok());

        // Preconditions.
        let stock = |n: &str| matches!(n, "libplasma" | "steam").then(|| n.to_string());
        assert!(unmet_preconditions(&pkg, &stock).is_empty());
        let fork = |n: &str| match n {
            "libplasma" => Some("aeroshell-libplasma-git".to_string()),
            "steam" => Some("steam".to_string()),
            _ => None,
        };
        let unmet = unmet_preconditions(&pkg, &fork);
        assert_eq!(unmet.len(), 1);
        assert_eq!(unmet[0].found.as_deref(), Some("aeroshell-libplasma-git"));
        assert_eq!(unmet[0].text("ru"), "Форк libplasma");
        // No steam, jupiter-hw-support present; no libplasma — doesn't matter.
        let other = |n: &str| (n == "jupiter-hw-support").then(|| n.to_string());
        let unmet = unmet_preconditions(&pkg, &other);
        let names: Vec<&str> = unmet.iter().map(|u| u.precondition.name.as_str()).collect();
        assert_eq!(names, ["steam", "jupiter-hw-support"]);
        assert!(unmet.iter().all(|u| !u.text("en").is_empty()));

        let bad = |f: &dyn Fn(&mut Value)| with(v.clone(), |v| f(v));
        assert!(has_err(&bad(&|v| v["requires"]["forbid"] = json!(["a b"])), "requires.forbid"));
        assert!(has_err(&bad(&|v| v["requires"]["preconditions"][0] = json!({"name": "libplasma"})), "requires.preconditions[0]"));
        assert!(has_err(&bad(&|v| v["requires"]["preconditions"][0]["not_owners"] = json!(["a b"])), "requires.preconditions[0].not_owners"));
        assert!(has_err(&bad(&|v| v["requires"]["preconditions"][0]["message"] = json!({"en": " "})), "requires.preconditions[0].message"));
        // An old manifest without the new fields is written as before.
        let old = serde_json::to_value(parse(&full_look()).unwrap()).unwrap();
        assert!(!old["requires"].as_object().unwrap().contains_key("forbid"));
        assert!(!old["requires"].as_object().unwrap().contains_key("preconditions"));
    }

    #[test]
    fn repo_first_then_aur_fallback() {
        let dep = |d: Value| with(full_look(), |v| v["requires"]["packages"] = json!([d]));
        let v = dep(json!({"role": "icons", "required": false, "arch": ["beautyline"], "fedora": null,
                           "fallback": ["aur"], "aur": ["beautyline"]}));
        let pkg = parse(&v).unwrap_or_else(|e| panic!("{}", errors_text(&e)));
        let d = &pkg.requires.packages[0];
        assert_eq!(d.sources_in_order(), [DepSource::Repo, DepSource::Aur]);
        assert_eq!(d.names_for(PkgFamily::Arch).unwrap(), ["beautyline"], "основной источник — репозиторий");
        // CachyOS: the package is in the repository — installed from there, automatically.
        let repo = d.resolve(PkgFamily::Arch, &|n| n == "beautyline").unwrap();
        assert_eq!((repo.source, repo.install_names()), (DepSource::Repo, vec!["beautyline".to_string()]));
        assert!(repo.installs_automatically());
        // Plain Arch: not in the repositories — AUR, with the prefix and not automatically.
        let aur = d.resolve(PkgFamily::Arch, &|_| false).unwrap();
        assert_eq!((aur.source, aur.install_names()), (DepSource::Aur, vec!["aur/beautyline".to_string()]));
        assert!(!aur.installs_automatically());
        // Fedora: null, and AUR doesn't exist outside Arch — no source.
        assert_eq!(d.resolve(PkgFamily::Fedora, &|_| true), None);
        // Without a fallback: not in the repository — no source.
        let plain = &parse(&full_look()).unwrap().requires.packages[0];
        assert_eq!(plain.resolve(PkgFamily::Arch, &|_| false), None);
        assert_eq!(plain.resolve(PkgFamily::Arch, &|_| true).unwrap().source, DepSource::Repo);
        // The old form of manifests doesn't change on write.
        assert!(!serde_json::to_value(plain).unwrap().as_object().unwrap().contains_key("fallback"));

        assert!(has_err(&dep(json!({"role": "x", "arch": ["a"], "fallback": ["aur"]})), "requires.packages[0].aur"));
        assert!(has_err(&dep(json!({"role": "x", "arch": ["a"], "fallback": ["aur"], "aur": ["--x"]})), "requires.packages[0].aur"));
        assert!(has_err(&dep(json!({"role": "x", "arch": ["a"], "fallback": ["gnome-extensions"]})), "requires.packages[0].fallback"));
        assert!(has_err(&dep(json!({"role": "x", "source": "aur", "aur": ["a"], "fallback": ["repo"]})), "requires.packages[0].fallback"));
        assert!(has_err(&dep(json!({"role": "x", "fallback": ["aur"], "aur": ["a"]})), "requires.packages[0]"));
    }

    #[test]
    fn gnome_shell_version_check() {
        let d = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(gnome_shell_compatible(&d(&["46", "47"]), "47.2"));
        assert!(!gnome_shell_compatible(&d(&["45", "46"]), "47.0"));
        assert!(gnome_shell_compatible(&d(&["3.36", "3.38"]), "3.38.4"));
        assert!(!gnome_shell_compatible(&d(&["3.36"]), "3.38.1"));
        assert!(!gnome_shell_compatible(&d(&["47"]), ""));
    }

    // ── executable content ───────────────────────────────────────────────

    fn file<'a>(path: &'a str, head: &'a [u8]) -> ArchiveFile<'a> {
        ArchiveFile { path, head, executable: false }
    }

    #[test]
    fn classify_files() {
        assert_eq!(classify_file(&file("w/clock.lua", b"")), Some(ExecClass::Lua));
        assert_eq!(classify_file(&file("w/applet.js", b"")), Some(ExecClass::JavaScript));
        assert_eq!(classify_file(&file("p/main.qml", b"")), Some(ExecClass::Qml));
        assert_eq!(classify_file(&file("x/run", b"#!/bin/sh\n")), Some(ExecClass::Script));
        assert_eq!(classify_file(&file("x/tool", b"\x7fELF\x02")), Some(ExecClass::Binary));
        assert_eq!(classify_file(&file("a/open.nemo_action", b"[Nemo Action]\nExec=rm -rf %F\n")), Some(ExecClass::DesktopExec));
        assert_eq!(classify_file(&file("w/index.html", b"<html><SCRIPT>x</SCRIPT>")), Some(ExecClass::WebPage));
        assert_eq!(classify_file(&file("w/clock.conkyrc", b"")), Some(ExecClass::Conky));
        assert_eq!(classify_file(&file("w/x.conf", b"conky.config = {}")), Some(ExecClass::Conky));
        assert_eq!(classify_file(&ArchiveFile { path: "w/data", head: b"", executable: true }), Some(ExecClass::Script));
        for data in ["a/gtk.css", "a/themerc", "a/x.png", "a/index.html", "a/x.desktop"] {
            assert_eq!(classify_file(&file(data, b"[Desktop Entry]\nName=x")), None, "{data}");
        }
        assert!(!exec_class_label(ExecClass::Qml).is_empty());
    }

    #[test]
    fn svg_checks() {
        assert!(svg_problems(br#"<svg><path d="M0 0"/></svg>"#).is_empty());
        assert!(svg_problems(br##"<svg><use xlink:href="#a"/></svg>"##).is_empty());
        assert_eq!(svg_problems(b"<svg><script>alert(1)</script></svg>"), [SvgProblem::Script]);
        assert_eq!(svg_problems(b"<svg><foreignObject/></svg>"), [SvgProblem::ForeignObject]);
        assert_eq!(svg_problems(br#"<svg><image xlink:href = "https://x/t.png"/></svg>"#), [SvgProblem::ExternalLink]);
    }

    #[test]
    fn contents_must_match_declared_permissions() {
        let pkg = parse(&full_look()).unwrap();
        let ok = [
            file("native/gtk/Nordic/gtk-3.0/gtk.css", b""),
            file("widgets/clock/clock.lua", b""),
            file("native/cinnamon/weather@x/applet.js", b""),
            file("native/aurorae/NordDesk/decoration.svg", b"<svg/>"),
        ];
        assert!(check_contents(&pkg, &ok).is_empty(), "{:?}", check_contents(&pkg, &ok));
        // A script in an element without exec (a GTK theme) and outside any element.
        let bad = [file("native/gtk/Nordic/install.sh", b""), file("stray/run.py", b"")];
        assert_eq!(check_contents(&pkg, &bad).len(), 2);
        let svg = [file("native/aurorae/NordDesk/close.svg", b"<svg><script/></svg>")];
        assert_eq!(check_contents(&pkg, &svg).len(), 1);
        let comp = [file("native/hypr/hyprland.conf", b"exec-once = evil")];
        assert_eq!(check_contents(&pkg, &comp)[0].message, compositor_refusal());
    }

    // ── licenses ─────────────────────────────────────────────────────────

    #[test]
    fn license_classes() {
        use LicenseClass::*;
        assert_eq!(license_class(Some("GPL-3.0-or-later")), Free);
        assert_eq!(license_class(Some("GPL-2.0+")), Free);
        // Breeze, Oxygen and most of KDE — LGPL-2.0-or-later.
        assert_eq!(license_class(Some("LGPL-2.0-or-later")), Free);
        assert_eq!(license_class(Some("MIT OR CC-BY-NC-4.0")), Free);
        assert_eq!(license_class(Some("MIT AND CC-BY-NC-SA-4.0")), NonCommercial);
        assert_eq!(license_class(Some("CC-BY-ND-4.0")), NoDerivatives);
        assert_eq!(license_class(Some("LicenseRef-Proprietary")), NonFree);
        assert_eq!(license_class(Some("LicenseRef-MyOwn")), Unknown);
        assert_eq!(license_class(None), Missing);
        assert_eq!(license_class(Some(" ")), Missing);
    }

    #[test]
    fn derived_inherits_free_license() {
        let src = Source { what: "Arc".into(), author: Some("horst3180".into()), license: Some("GPL-3.0-or-later".into()), ..Default::default() };
        assert_eq!(derived_attribution(&src).unwrap(), src);
        let nd = Source { license: Some("CC-BY-ND-4.0".into()), ..src.clone() };
        assert!(derived_attribution(&nd).is_err());
        let none = Source { license: None, ..src.clone() };
        assert!(derived_attribution(&none).is_err());

        let derived = |pkg_license: &str, src: &Source| {
            with(full_look(), |v| {
                v["license"] = json!(pkg_license);
                v["derived_from"] = serde_json::to_value(vec![src]).unwrap();
            })
        };
        assert!(parse(&derived("GPL-3.0-or-later", &src)).is_ok());
        assert!(parse(&derived("GPL-3.0-or-later AND CC-BY-SA-4.0", &src)).is_ok());
        assert!(has_err(&derived("MIT", &src), "license"), "производное обязано сохранить лицензию источника");
        let nc = Source { license: Some("CC-BY-NC-SA-4.0".into()), ..src };
        assert!(has_err(&derived("CC-BY-NC-SA-4.0", &nc), "derived_from[0]"), "от несвободного производного нет");
    }

    #[test]
    fn publishing_needs_shareable_licenses() {
        let pkg = parse(&full_look()).unwrap();
        assert!(check_publishable(&pkg).is_empty());
        let mut p = pkg.clone();
        p.license = None;
        assert_eq!(check_publishable(&p)[0].field, "license");
        let mut p = pkg.clone();
        p.sources[0].license = Some("CC-BY-NC-4.0".into());
        assert_eq!(check_publishable(&p)[0].field, "sources[0]");
        let mut p = pkg;
        p.license = Some("LicenseRef-Custom".into());
        assert!(check_publishable(&p)[0].message.contains("LicenseRef-Custom"));
    }

    // ── personal data ────────────────────────────────────────────────────

    #[test]
    fn scrub_home_user_host() {
        let ctx = ScrubContext { home: "/home/user".into(), user: "user".into(), host: "laptop".into() };
        let s = scrub_text("image=/home/user/Pictures/a.png\nother=/home/username/x\nby user@laptop, users\n", &ctx);
        assert_eq!(
            s.text,
            "image=${HOME}/Pictures/a.png\nother=/home/username/x\nby ${USER}@${HOSTNAME}, users\n"
        );
        assert_eq!(s.replaced, [PrivateKind::HomePath, PrivateKind::UserName, PrivateKind::HostName]);
        let short = ScrubContext { home: String::new(), user: "al".into(), host: String::new() };
        assert!(scrub_text("al value", &short).replaced.is_empty(), "короткие имена не трогаем");
    }

    #[test]
    fn secrets_block_widget_export() {
        let conf = "\
conky.text = [[
${imap_unseen imap.mail.ru me secret}
${execi 600 curl 'https://api.openweathermap.org/data/2.5/weather?lat=55.75&lon=37.61&appid=0123456789abcdef0123'}
mail: someone@example.com
ssid = \"HomeNet\"
${wireless_essid wlan0}
]]";
        let f = find_secrets(conf);
        let kinds: Vec<(PrivateKind, usize)> = f.iter().map(|x| (x.kind, x.line)).collect();
        assert!(kinds.contains(&(PrivateKind::Password, 2)), "{kinds:?}");
        assert!(kinds.contains(&(PrivateKind::ApiKey, 3)), "{kinds:?}");
        assert!(kinds.contains(&(PrivateKind::Coordinates, 3)), "{kinds:?}");
        assert!(kinds.contains(&(PrivateKind::Email, 4)), "{kinds:?}");
        assert!(kinds.contains(&(PrivateKind::WifiName, 5)), "{kinds:?}");
        assert!(!kinds.iter().any(|(_, l)| *l == 6), "живое чтение имени сети — не утечка");
        assert!(find_secrets("color = '#88c0d0'\nfont = Sans:size=10\n${key_value}").is_empty());
        assert!(!PrivateKind::ApiKey.label().is_empty());
    }

    #[test]
    fn never_exported_files() {
        for p in [
            ".local/share/recently-used.xbel",
            ".bash_history",
            ".config/gtk-3.0/bookmarks",
            ".local/share/applications/my-script.desktop",
            ".local/share/user-places.xbel",
            ".ssh/id_ed25519",
        ] {
            assert!(never_export(p), "{p}");
        }
        for p in [".config/conky/clock.conkyrc", ".config/xfce4/panel/whiskermenu-1.rc", ".themes/Nordic/gtk-3.0/gtk.css"] {
            assert!(!never_export(p), "{p}");
        }
    }

    // ── pinned sources («Distro Looks») ──────────────────────────────────

    fn pinned(sources: Value, native: &str) -> Value {
        with(full_look(), |v| {
            v["sources"] = sources;
            v["items"][3] = json!({"kind": "icons", "native": {"freedesktop": native}});
        })
    }

    #[test]
    fn pinned_sources_parse_and_resolve() {
        let rev = "1280f411dc307847332ecab2f578ed984d33498c";
        let sum = "b593a4a7617b3c5e37f5b3fde440d17ff928db5dcbaadf391827e506e2582f72";
        let v = pinned(
            json!([
                {"what": "git", "id": "git-src", "url": "https://example.org/r.git", "rev": rev, "license": "GPL-3.0-only"},
                {"what": "tar", "id": "tar-src", "url": "https://example.org/a.tar.xz", "sha256": sum, "license": "GPL-2.0-only"},
                {"what": "pkg", "id": "pkg-src", "package": "gtk3-adwaita-base", "license": "LGPL-2.1-only"}
            ]),
            "sources/pkg-src/usr/share/icons/X",
        );
        let pkg = parse(&v).unwrap_or_else(|e| panic!("{}", errors_text(&e)));
        assert!(pkg.source("git-src").is_some_and(Source::is_fetched));
        assert!(!pkg.source("pkg-src").unwrap().is_fetched());
        // Downloadable content is not published to the Workshop, a package source can be.
        let pubs = check_publishable(&pkg);
        assert!(pubs.iter().any(|e| e.field == "sources[0]"));
        assert!(pubs.iter().any(|e| e.field == "sources[1]"));
        assert!(!pubs.iter().any(|e| e.field == "sources[2]"));
    }

    #[test]
    fn pinned_sources_rules() {
        let ok = json!({"what": "w", "id": "s", "url": "https://e.org/r.git", "rev": "a".repeat(40), "license": "MIT"});
        let one = |f: &dyn Fn(&mut Value)| pinned(json!([with(ok.clone(), |s| f(s))]), "sources/s/icons");
        assert!(parse(&one(&|_| {})).is_ok());
        assert!(has_err(&one(&|s| s["rev"] = json!("nova")), "sources[0].rev"));
        assert!(has_err(&one(&|s| s["url"] = json!("http://e.org/r.git")), "sources[0].url"));
        assert!(has_err(&one(&|s| s["sha256"] = json!("b".repeat(64))), "sources[0]"));
        assert!(has_err(&one(&|s| { s.as_object_mut().unwrap().remove("license"); }), "sources[0].license"));
        assert!(has_err(&one(&|s| s["id"] = json!("../x")), "sources[0].id"));
        assert!(has_err(&one(&|s| { s.as_object_mut().unwrap().remove("id"); }), "sources[0].id"));
        assert!(has_err(&one(&|s| { s.as_object_mut().unwrap().remove("rev"); s["package"] = json!("no-such-role"); }), "sources[0].package"));
        // A path to an undeclared source.
        assert!(has_err(&pinned(json!([ok.clone()]), "sources/other/icons"), "items[3].native.freedesktop"));
        // The same id twice.
        assert!(has_err(&pinned(json!([ok.clone(), ok]), "sources/s/icons"), "sources[1].id"));
    }

    #[test]
    fn dependency_scoped_to_consumer() {
        let v = with(full_look(), |v| {
            v["requires"]["packages"] = json!([
                {"role": "user-theme", "for": ["shell-theme"], "consumers": ["gnome-shell"], "arch": ["gnome-shell-extensions"]},
                {"role": "any-shell", "for": ["shell-theme"], "arch": ["foo"]}
            ]);
        });
        let pkg = parse(&v).unwrap_or_else(|e| panic!("{}", errors_text(&e)));
        let roles = |c: &str| pkg.deps_for_choice("shell-theme", c).iter().map(|d| d.role.clone()).collect::<Vec<_>>();
        assert_eq!(roles("gnome-shell"), ["user-theme", "any-shell"]);
        assert_eq!(roles("plasma"), ["any-shell"]);
        let bad = with(v, |v| v["requires"]["packages"][0]["consumers"] = json!(["Bad Name"]));
        assert!(has_err(&bad, "requires.packages[0].consumers"));
    }

    #[test]
    fn items_and_deps_depend_on_property_values() {
        let v = with(full_look(), |v| {
            v["properties"]["panel_edge"]["options"] = json!([
                {"value": "bottom", "label": {"en": "Bottom", "ru": "Снизу"}},
                "top"
            ]);
            v["properties"]["dock"] = json!({"type": "toggle", "default": false});
            v["items"][2]["when"] = json!({"panel_edge": "top"});
            v["requires"]["packages"] = json!([
                {"role": "dock", "for": ["panel-layout"], "when": {"dock": true, "panel_edge": ["top", "bottom"]},
                 "arch": ["plank"]},
                {"role": "always", "for": ["panel-layout"], "arch": ["foo"]}
            ]);
        });
        let pkg = parse(&v).unwrap_or_else(|e| panic!("{}", errors_text(&e)));
        assert_eq!(pkg, parse(&serde_json::to_value(&pkg).unwrap()).unwrap());
        let edge = pkg.property("panel_edge").unwrap();
        assert_eq!(edge.option_values(), ["bottom", "top"]);
        assert_eq!(edge.options[0].label("ru"), "Снизу");
        assert_eq!(edge.options[0].label("de"), "Bottom");
        assert_eq!(edge.options[1].label("ru"), "top", "без подписи — само значение");

        let vals = |pairs: &[(&str, Value)]| pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect::<PropValues>();
        let roles = |values: &PropValues| {
            pkg.deps_for_choice_with("panel-layout", "kde-layout", values).iter().map(|d| d.role.clone()).collect::<Vec<_>>()
        };
        assert_eq!(roles(&PropValues::new()), ["always"], "по умолчанию дока нет");
        assert_eq!(pkg.deps_for_choice("panel-layout", "kde-layout").len(), 1);
        assert_eq!(roles(&vals(&[("dock", json!(true))])), ["dock", "always"]);
        assert_eq!(pkg.deps_for("panel-layout").len(), 2, "deps_for — все возможные");

        // The panel-layout element — only with panel_edge = top.
        let gen = |_: &str, c: &str| (c == "kde-layout").then_some(Accuracy::Exact);
        let s = session(&["kde-layout", "kde-colors", "aurorae", "gtk"]);
        let plan = choose_all(&pkg, &s, &gen);
        assert_eq!(plan.choices[2], Choice::Off);
        assert!(!plan.skipped().iter().any(|(i, _)| *i == 2), "выключенное — не «пропущено»");
        let plan = choose_all_with(&pkg, &s, &vals(&[("panel_edge", json!("top"))]), &gen);
        assert!(matches!(plan.choices[2], Choice::Generate { .. }));

        let bad = |f: &dyn Fn(&mut Value)| with(v.clone(), |v| f(v));
        assert!(has_err(&bad(&|v| v["items"][2]["when"] = json!({"nope": "x"})), "items[2].when.nope"));
        assert!(has_err(&bad(&|v| v["items"][2]["when"] = json!({"panel_edge": "left"})), "items[2].when.panel_edge"));
        assert!(has_err(&bad(&|v| v["items"][2]["when"] = json!({"radius": 8})), "items[2].when.radius"));
        assert!(has_err(&bad(&|v| v["items"][2]["when"] = json!({"panel_edge": []})), "items[2].when.panel_edge"));
        assert!(has_err(&bad(&|v| v["requires"]["packages"][0]["when"] = json!({"dock": "yes"})), "requires.packages[0].when.dock"));
        assert!(has_err(&bad(&|v| v["properties"]["panel_edge"]["options"] = json!(["top", "top"])), "properties.panel_edge.options"));
        assert!(has_err(&bad(&|v| v["properties"]["panel_edge"]["options"][0]["label"] = json!({"en": ""})), "properties.panel_edge.options"));
        // The old form — strings — is written as before.
        let old = serde_json::to_value(parse(&full_look()).unwrap()).unwrap();
        assert_eq!(old["properties"]["panel_edge"]["options"], json!(["bottom", "top"]));
        assert!(old["items"][0].get("when").is_none());
    }

    #[test]
    fn game_session_runs_code_and_item_note() {
        let it = json!({"kind": "game-session", "native": {"wayland-sessions": "s/x.desktop"}, "required": false,
                        "note": {"en": "Hybrid graphics", "ru": "Гибридная графика"}});
        assert!(has_err(&with_item(it.clone()), "items[2].permissions"));
        let pkg = parse(&with_item(with(it, |i| i["permissions"] = json!(["exec"])))).unwrap();
        assert_eq!(pkg.items[2].note.as_ref().unwrap().get("ru"), "Гибридная графика");
    }
}
