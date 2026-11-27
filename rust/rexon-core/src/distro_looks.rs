//! «As in the distribution» looks: apply a whole `.rxn` v5 set and restore
//! it as it was.
//!
//! WHY. «Like SteamOS» and «Like Garuda Dr460nized» are data
//! (`data/distro-looks/<id>/manifest.json` + `native/plasma-settings.json`,
//! schema `kde-settings/1`), not code. Here is what executes them:
//!
//! 1. [`plan`] — a pure plan build: preconditions
//!    ([`rxn::unmet_preconditions`]) → choosing the payload for the desktop
//!    ([`rxn::choose_all_with`]) → dependencies of ONLY the chosen elements
//!    ([`Package::deps_for_choice_with`]). The owner's rule of 24.10: packages
//!    from the official repositories are installed right away (the protection is the password window),
//!    AUR ones — never automatically: such packages go to the person as a list
//!    ([`LookPlan::aur`]). If something mandatory does not install, the look is not applied
//!    at all ("all or nothing").
//! 2. [`apply`] — execution on a substitutable machine ([`Machine`]): sources
//!    into the cache with verification (`rev` for git — git itself verifies the contents by object
//!    hashes, an archive — by `sha256`), installing dependencies and checking "exactly that
//!    got installed" ([`rxn::verify_dependency`], [`rxn::verify_package_change`];
//!    on refusal what was installed is removed), preparing files (`copy_only`,
//!    `exclude`, declared `edits` — only from a closed set),
//!    checking the contents ([`rxn::check_contents`]), then ONE run of
//!    the shared executor [`crate::install_plan`]: the elements' files (with an owner
//!    mark, foreign things are not touched) and enabling with [`ApplyStep`] steps —
//!    in Plasma by `plasma-settings.json` (`plasma-apply-lookandfeel`,
//!    `plasma-apply-*`, `kwriteconfig6` keys, `kwin reconfigure`), on other
//!    desktops — with the «Appearance» press plans
//!    ([`crate::appearance_installer::press_plan_with`]: GTK, icons,
//!    cursors) and the native keys of frames, the shell and the font.
//! 3. Recording: a "what it was" snapshot (the run's rollback is moved to
//!    [`journal::applied_snapshot_dir`], a [`journal::Applied`] record of the section
//!    [`section_of`]) and the look's record in the shared log
//!    ([`journal::Source::Look`]: placed paths with an owner mark,
//!    v5 elements with the choice, commands restoring values that are kept not by
//!    a file but by the desktop itself — xfconf, gsettings).
//! 4. [`revert`] — «Restore as it was» for the whole look: remove what was placed per
//!    the log (only our own), restore files from the snapshot
//!    ([`journal::revert_applied_at`]), restore the desktop's previous values
//!    with commands. Packages installed by the look are NOT removed — their list
//!    is given to the person ([`Reverted::packages`]).
//!
//! The module starts no processes itself: commands go through [`Ctx::run`], network,
//! packages and probes — through [`Machine`]; the live implementations are `*_live`.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{json, Map, Value};

use crate::appearance_installer::{press_plan_with, ApplyAction, PressFacts, PressPlan};
use crate::ext_manager::{ExtStep, OWNER_MARK};
use crate::i18n::{tr, trf};
use crate::install_journal as journal;
use crate::install_plan::{exec_undoable, Ctx, Step, Undo};
use crate::rxn_manifest::{self as rxn, Accuracy, Choice, DepSource, OwnerOf, Package, PackageDep, PropValues, ResolvedDep, Session};
use crate::system::PkgFamily;
use crate::themes::{ApplyStep, Cmd};

// ============================================================================
// Looks
// ============================================================================

/// The schema of the look's Plasma part description (`native/plasma-settings.json`).
pub const SETTINGS_SCHEMA: &str = "kde-settings/1";

/// The consumer the Plasma description serves (`behaviour` →
/// `{"kconfig": "native/plasma-settings.json"}`).
pub const SETTINGS_CONSUMER: &str = "kconfig";

/// Kinds installed only at the person's wish ([`Request::enable`]):
/// the SteamOS game mode.
pub const OPT_IN_KINDS: &[&str] = &["game-session"];

/// The schema of the panels and behavior description for other desktops
/// (`native/desktop-model.json`).
pub const MODEL_SCHEMA: &str = "rexon-desktop-model/1";

/// A built-in look: id, manifest, Plasma description (`None` — the look has
/// none: it is made for other desktops), description for other desktops.
type BuiltinLook = (&'static str, &'static str, Option<&'static str>, Option<&'static str>);

const BUILTIN: &[BuiltinLook] = &[
    (
        "steamos",
        include_str!("../../../data/distro-looks/steamos/manifest.json"),
        Some(include_str!("../../../data/distro-looks/steamos/native/plasma-settings.json")),
        None,
    ),
    (
        "garuda-dr460nized",
        include_str!("../../../data/distro-looks/garuda-dr460nized/manifest.json"),
        Some(include_str!("../../../data/distro-looks/garuda-dr460nized/native/plasma-settings.json")),
        Some(include_str!("../../../data/distro-looks/garuda-dr460nized/native/desktop-model.json")),
    ),
];

/// Looks that are ready as data and tested but get no tile yet: their cards
/// in «Distro Looks» are a separate step. [`builtin`] knows them;
/// [`builtin_ids`] (the tiles, the package helper's allowlist) does not.
const STAGED: &[BuiltinLook] = &[
    (
        "windows-xp",
        include_str!("../../../data/distro-looks/windows-xp/manifest.json"),
        None,
        Some(include_str!("../../../data/distro-looks/windows-xp/native/desktop-model.json")),
    ),
    (
        "windows-7",
        include_str!("../../../data/distro-looks/windows-7/manifest.json"),
        None,
        Some(include_str!("../../../data/distro-looks/windows-7/native/desktop-model.json")),
    ),
];

fn all_builtin() -> impl Iterator<Item = &'static BuiltinLook> {
    BUILTIN.iter().chain(STAGED)
}

/// A look: the v5 manifest, the Plasma description and the description for other desktops.
#[derive(Debug, Clone, PartialEq)]
pub struct Look {
    pub pkg: Package,
    /// `native/plasma-settings.json` (`None` — the look does not have it).
    pub settings: Option<Value>,
    /// `native/desktop-model.json` — panels and behavior as roles, from which
    /// Rexon builds the native setup of other desktops (`None` — the look does not have it).
    pub model: Option<Value>,
}

/// Ids of the built-in looks shown to the person, in order.
pub fn builtin_ids() -> Vec<&'static str> {
    BUILTIN.iter().map(|(id, ..)| *id).collect()
}

/// Ids of all built-in looks, the staged ones ([`STAGED`]) included.
pub fn all_builtin_ids() -> Vec<&'static str> {
    all_builtin().map(|(id, ..)| *id).collect()
}

/// The built-in look `id` (a staged one too); `None` — there is no such look.
pub fn builtin(id: &str) -> Option<Result<Look, String>> {
    all_builtin().find(|(i, ..)| *i == id).map(|(_, m, s, d)| parse_look(m, *s, *d))
}

/// All packages the built-in looks may install in the family
/// `family`: the family's repository names and, on Arch, AUR names (on CachyOS
/// some of them are in the repository and go through the package helper). The helper's
/// install allowlist (`system_plan::pkg_helper_install_allowlist`).
pub fn builtin_package_names(family: PkgFamily) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for id in builtin_ids() {
        let Some(Ok(look)) = builtin(id) else { continue };
        for dep in &look.pkg.requires.packages {
            for source in [DepSource::Repo, DepSource::Aur] {
                for n in names_of(dep, source, family).unwrap_or_default() {
                    if !out.contains(&n) {
                        out.push(n);
                    }
                }
            }
        }
    }
    out
}

/// Parse a look: the manifest (checking the v5 model), the Plasma description and
/// the description for other desktops.
pub fn parse_look(manifest: &str, settings: Option<&str>, model: Option<&str>) -> Result<Look, String> {
    let meta: Value = serde_json::from_str(manifest).map_err(|e| e.to_string())?;
    let pkg = rxn::parse(&meta).map_err(|e| rxn::errors_text(&e))?;
    let errors = rxn::validate(&pkg);
    if !errors.is_empty() {
        return Err(rxn::errors_text(&errors));
    }
    let settings = match settings {
        None => None,
        Some(text) => {
            let v: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
            if v.get("schema").and_then(Value::as_str) != Some(SETTINGS_SCHEMA) {
                return Err(trf("the Plasma description of the look is not in the %1 format", &[&SETTINGS_SCHEMA]));
            }
            Some(v)
        }
    };
    let model = match model {
        None => None,
        Some(text) => {
            let v: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
            if v.get("schema").and_then(Value::as_str) != Some(MODEL_SCHEMA) {
                return Err(trf("the description of the look for other desktops is not in the %1 format", &[&MODEL_SCHEMA]));
            }
            Some(v)
        }
    };
    Ok(Look { pkg, settings, model })
}

/// The section of the look's «Apply» record in the shared log.
pub fn section_of(id: &str) -> String {
    format!("look:{id}")
}

/// The key of the look's record in the shared log.
pub fn journal_key(id: &str) -> String {
    journal::key_of(journal::Source::Look, id)
}

/// The owner mark of directories placed by the look.
fn owner_of_look(id: &str) -> String {
    format!("look:{id}")
}

// ============================================================================
// The machine: the desktop, the person's request, probes
// ============================================================================

/// Session consumers per Rexon desktop (there is no `consumer` column in the
/// `de_customization` matrix yet — the table is here).
///
/// Outside Plasma: `qt6ct` — Qt programs whose style and palette are chosen by
/// qt6ct (`themes::qt_style_way` = `PlatformTheme`); in LXQt the desktop
/// itself drives them (`kvantum` is native, the palette is `lxqt`). `marco`, `muffin`, `mutter` —
/// window managers to which Rexon carries the behavior from the look description.
/// Deepin is deliberately without `qt6ct` here: a session-wide variable would take
/// the Chameleon style away from Deepin programs.
pub fn session_consumers(de: &str) -> Vec<String> {
    let list: &[&str] = match de {
        "kde" | "plasma" => &["kde-lnf", "kde-colors", "plasma", "aurorae", "kvantum", "kde-layout", "kconfig", "kwin", "gtk"],
        "xfce" => &["xfwm4", "gtk", "xfce4-panel", "qt6ct"],
        "gnome" => &["gtk-ssd", "gnome-shell", "gtk", "mutter", "qt6ct"],
        "cinnamon" => &["gtk-ssd", "cinnamon", "cinnamon-panel", "muffin", "gtk", "qt6ct"],
        "mate" => &["metacity-1", "gtk", "mate-panel", "marco", "qt6ct"],
        "budgie" => &["gtk-ssd", "gtk", "qt6ct"],
        "lxqt" => &["gtk", "kvantum", "lxqt"],
        // Tiling: Rexon mirrors the `environment.d` variables into their session itself
        // (`compositor_autostart::sync_session_env`; for sway and river — an honest
        // caveat in the matrix).
        "sway" | "hyprland" | "niri" | "river" => &["gtk", "qt6ct"],
        _ => &["gtk"],
    };
    list.iter().map(|s| s.to_string()).collect()
}

/// The session of the desktop `de` with known consumer versions.
pub fn session_for(de: &str, versions: BTreeMap<String, String>) -> Session {
    Session { consumers: session_consumers(de), versions }
}

/// Where and for which desktop it is applied — all captured in advance.
#[derive(Debug, Clone)]
pub struct Target {
    /// The Rexon desktop: `kde`, `xfce`, `gnome`, `cinnamon`, `mate`…
    pub de: String,
    pub session: Session,
    pub family: PkgFamily,
    pub home: String,
    /// `$XDG_CONFIG_HOME` (or `~/.config`).
    pub config: String,
    /// `$XDG_DATA_HOME` (or `~/.local/share`).
    pub data: String,
    /// `$XDG_CACHE_HOME` (or `~/.cache`).
    pub cache: String,
    pub lang: String,
    /// A Treeland session (Deepin on Wayland): the flag goes into
    /// [`crate::install_plan::Ctx::wayland`] and means exactly that.
    pub wayland: bool,
    /// The cursor size now (the cursor plan outside Plasma writes it entirely).
    pub cursor_size: Option<u32>,
    /// An AUR helper that can ask for the password with a window (`yay`/`paru`,
    /// [`crate::aur_install::FLAG_AWARE_HELPERS`]); `None` — there is none.
    pub aur_helper: Option<String>,
}

impl Target {
    /// The Rexon config directory (the shared log).
    pub fn rexon_dir(&self) -> PathBuf {
        Path::new(&self.config).join("rexon")
    }

    /// A Plasma session: the `plasma-settings.json` description is executed.
    pub fn plasma(&self) -> bool {
        self.session.consumers.iter().any(|c| c == SETTINGS_CONSUMER)
    }

    fn looks_cache(&self) -> PathBuf {
        Path::new(&self.cache).join("rexon/distro-looks")
    }

    /// The directory of the look's prepared files (ours, outside the profile).
    fn prepared_dir(&self, id: &str) -> PathBuf {
        self.looks_cache().join("prepared").join(id)
    }

    /// Substitute `${HOME}`, `${CONFIG}`, `${DATA}`.
    fn subst(&self, s: &str) -> String {
        s.replace("${HOME}", &self.home).replace("${CONFIG}", &self.config).replace("${DATA}", &self.data)
    }

    /// The live machine: the desktop `de`, the process's XDG directories, the package family.
    pub fn live(de: &str) -> Target {
        let env = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        let home = env("HOME").unwrap_or_default();
        Target {
            de: de.to_string(),
            session: session_for(de, BTreeMap::new()),
            family: crate::system::DistroDetector::detect_live().family_live(),
            config: crate::utils::config_home(env("XDG_CONFIG_HOME").as_deref(), &home),
            data: crate::utils::data_home(env("XDG_DATA_HOME").as_deref(), &home),
            cache: crate::utils::cache_home(env("XDG_CACHE_HOME").as_deref(), &home),
            lang: crate::i18n::current_language(),
            wayland: crate::dde::treeland_session_live(),
            cursor_size: None,
            aur_helper: crate::aur_install::FLAG_AWARE_HELPERS
                .iter()
                .find(|h| crate::utils::which(h, None).is_some())
                .map(|h| h.to_string()),
            home,
        }
    }
}

/// What the person chose.
#[derive(Debug, Clone, Default)]
pub struct Request {
    /// Property values (`variant` for SteamOS); empty — defaults.
    pub values: PropValues,
    /// Enable "at your wish" kinds ([`OPT_IN_KINDS`]).
    pub enable: Vec<String>,
    /// Do not install optional kinds (mandatory ones cannot be turned off this way).
    pub disable: Vec<String>,
}

/// System probes for building the plan.
pub struct Probes<'a> {
    /// Which REAL package provides the name (taking Provides into account).
    pub owner_of: OwnerOf<'a>,
    /// Whether the package is in the enabled repositories.
    pub in_repos: &'a dyn Fn(&str) -> bool,
    /// Whether a GNOME Shell extension is installed (uuid).
    pub extension_installed: &'a dyn Fn(&str) -> bool,
}

// ============================================================================
// Plan
// ============================================================================

/// An element that will be installed.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlannedItem {
    pub index: usize,
    pub kind: String,
    pub consumer: String,
    /// The payload path in the package (`sources/<id>/…` or its own file).
    pub path: String,
    pub choice: Choice,
}

/// An element that will not be installed, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Skipped {
    pub kind: String,
    pub reason: String,
}

/// A dependency Rexon will install itself (an official repository).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DepInstall {
    pub role: String,
    pub resolved: ResolvedDep,
}

/// A GNOME Shell extension Rexon will install itself (a dependency
/// `"source": "gnome-extensions"`): through the shell (its confirmation window),
/// without it — the release zip for the GNOME version
/// ([`crate::gnome_ext_section::install_dependency`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExtNeed {
    pub role: String,
    pub uuid: String,
    /// For which kinds.
    pub kinds: Vec<String>,
}

/// Packages from the AUR the person can install themselves.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AurNeed {
    pub role: String,
    /// Names for the install command — with the `aur/` prefix.
    pub packages: Vec<String>,
    /// For which kinds.
    pub kinds: Vec<String>,
    /// Without them the kind does not install (rather than just losing a part).
    pub needed: bool,
}

/// Source pinning.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "pin", content = "value", rename_all = "kebab-case")]
pub enum Pin {
    /// A git commit.
    Git(String),
    /// The archive's SHA-256.
    Sha256(String),
}

impl Pin {
    fn text(&self) -> &str {
        match self {
            Pin::Git(s) | Pin::Sha256(s) => s,
        }
    }
}

/// Download a source into the cache.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FetchJob {
    pub id: String,
    pub url: String,
    pub pin: Pin,
    /// The contents directory in the cache (`…/sources/<id>-<pin>`).
    pub dir: PathBuf,
}

/// A file in the cache that says "exactly the pinned thing is here".
pub const PIN_MARK: &str = ".rexon-pin";

impl FetchJob {
    /// The contents are already in the cache and the pin is the same.
    pub fn cached(&self) -> bool {
        std::fs::read_to_string(self.dir.join(PIN_MARK)).is_ok_and(|t| t.trim() == self.pin.text())
    }
}

/// The look's apply plan.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LookPlan {
    pub id: String,
    pub name: String,
    pub items: Vec<PlannedItem>,
    pub skipped: Vec<Skipped>,
    /// Installed right away (the password window).
    pub install: Vec<DepInstall>,
    /// For the person: can be installed from the AUR themselves.
    pub aur: Vec<AurNeed>,
    /// Roles that will be on the system (installed or being installed).
    pub roles: BTreeSet<String>,
    pub fetch: Vec<FetchJob>,
    /// The Plasma description is executed.
    pub uses_settings: bool,
    pub values: PropValues,
    /// Parts that are not installed separately here because they are carried by
    /// other parts being installed (a global theme outside Plasma is a set of these same
    /// parts; the MATE and XFCE panel is drawn by the GTK theme), and how exactly.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub covered: Vec<Skipped>,
    /// What of the parts being installed will not be on this desktop, with a reason.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub limits: Vec<Skipped>,
    /// AUR packages Rexon will build itself with an AUR helper (the password window) —
    /// only the [`AUTO_AUR_ROLES`] roles.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aur_build: Vec<AurNeed>,
    /// GNOME Shell extensions installed right away (the shell window).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extensions: Vec<ExtNeed>,
}

/// The result of building the plan.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum Prepared {
    /// The look is not applied: the reason for the person (and what can be installed
    /// from the AUR, if that is the issue).
    Refused { reason: String, aur: Vec<AurNeed> },
    Ready(LookPlan),
}

/// The reason for skipping a kind the person did not choose ([`Request::enable`],
/// [`Request::disable`]).
pub fn not_chosen_reason() -> String {
    tr("not chosen")
}

/// The kind label for the person.
pub fn kind_label(kind: &str) -> String {
    match kind {
        "global-look" => tr("global theme"),
        "palette" => tr("colours"),
        "window-decoration" => tr("window frames"),
        "shell-theme" => tr("desktop shell theme"),
        "qt-style" => tr("Qt application style"),
        "gtk-theme" => tr("GTK theme"),
        "icons" => tr("icons"),
        "cursors" => tr("cursors"),
        "fonts" => tr("fonts"),
        "wallpaper" => tr("wallpaper"),
        "panel-layout" => tr("panel layout"),
        "behaviour" => tr("desktop behaviour"),
        "game-session" => tr("game mode"),
        other => other.to_string(),
    }
}

/// Whether Rexon can itself build the kind `kind` for the consumer `consumer` from
/// the look description (the element's `model`) — the generator for
/// [`rxn::choose_all_with`]. What is built is "approximate": native
/// parts of the desktop close to the intent, not the author's files.
///
/// * the panel layout — from the description's roles (menu, windows, clock, tray,
///   launchers): mate-panel ([`crate::mate_panel::own_layout_text`]) and
///   Cinnamon panels (the `org.cinnamon` keys);
/// * behavior — what the window manager itself can do: window buttons, smoothing
///   (compositing), Cinnamon effects; the rest is named in [`LookPlan::limits`].
pub fn look_generator(kind: &str, consumer: &str) -> Option<Accuracy> {
    match (kind, consumer) {
        ("panel-layout", "mate-panel" | "cinnamon-panel")
        | ("behaviour", "marco" | "xfwm4" | "muffin" | "mutter") => Some(Accuracy::Approximate),
        _ => None,
    }
}

/// The desktop name for the person (in reasons).
fn desktop_name(de: &str) -> &'static str {
    match de {
        "mate" => "MATE",
        "xfce" => "Xfce",
        "cinnamon" => "Cinnamon",
        "gnome" => "GNOME",
        "budgie" => "Budgie",
        "lxqt" => "LXQt",
        "sway" => "Sway",
        "hyprland" => "Hyprland",
        "niri" => "niri",
        "river" => "River",
        _ => "",
    }
}

/// The window manager the behavior is carried to (the behavior
/// consumer) — the name for the person.
fn wm_name(consumer: &str) -> &'static str {
    match consumer {
        "marco" => "Marco",
        "xfwm4" => "xfwm4",
        "muffin" => "Muffin",
        "mutter" => "Mutter",
        _ => "",
    }
}

/// The refusal reason for the person: specific where possible (what exactly is wrong with this
/// desktop), otherwise — the format's generic wording.
fn refusal_text(kind: &str, t: &Target, reason: &rxn::RefuseReason, look_settings: Option<&Value>) -> String {
    if t.plasma() {
        return reason.text();
    }
    match (kind, t.de.as_str()) {
        ("global-look", _) => tr("a global theme is a Plasma package, and this desktop has nothing like it"),
        ("panel-layout", "gnome") => tr("GNOME has no panels to lay out: its top bar and dash belong to GNOME Shell"),
        ("panel-layout", "xfce") => tr("Rexon can't lay out Xfce panels yet, so your panels stay as they are"),
        ("behaviour", _) => behaviour_refusal(look_settings, t),
        ("window-decoration", "sway" | "hyprland" | "niri" | "river") => {
            trf("%1 draws window borders itself, and there are no themes for them", &[&desktop_name(&t.de)])
        }
        ("panel-layout", "sway" | "hyprland" | "niri" | "river") => trf(
            "%1 has no panel of its own: a bar such as Waybar is chosen separately, in the Shells block of its card in Desktops",
            &[&desktop_name(&t.de)],
        ),
        ("shell-theme", "sway" | "hyprland" | "niri" | "river") => trf(
            "%1 has no shell theme of its own. If you use Noctalia or DankMaterialShell, press “%2” in Color Schemes after applying: they take the colours of this look",
            &[&desktop_name(&t.de), &crate::shell_palette::push_label()],
        ),
        ("palette", "deepin") | ("qt-style", "deepin") => {
            tr("Deepin programs are drawn by its own Chameleon style, and Rexon does not replace it")
        }
        _ => reason.text(),
    }
}

/// Why the look's behavior is not carried over here: by WHAT the look changes in
/// Plasma (the description's `kconfig` files), not with a generic phrase.
fn behaviour_refusal(settings: Option<&Value>, t: &Target) -> String {
    let mut what: Vec<String> = Vec::new();
    for k in settings.and_then(|s| s.get("kconfig")).and_then(Value::as_array).into_iter().flatten() {
        let label = match k.get("file").and_then(Value::as_str).unwrap_or_default() {
            "kwinrc" => tr("KWin window effects"),
            "ksplashrc" => tr("the Plasma splash screen after login"),
            "kscreenlockerrc" => tr("the Plasma lock screen picture"),
            "krunnerrc" => tr("KRunner in the middle of the screen"),
            // Fonts (`kdeglobals`) and the cursor (`kcminputrc`) are carried by their own parts
            // of the look — we do not name them here.
            _ => continue,
        };
        if !what.contains(&label) {
            what.push(label);
        }
    }
    if what.is_empty() {
        return tr("these settings exist only in Plasma");
    }
    let de = desktop_name(&t.de);
    if de.is_empty() {
        trf("it changes only things Plasma has: %1", &[&what.join(", ")])
    } else {
        trf("it changes only things Plasma has: %1 — %2 has none of them", &[&what.join(", "), &de])
    }
}

/// Refused parts that on this desktop are carried by other parts being installed —
/// moved from `skipped` to "covered".
fn split_covered(skipped: Vec<Skipped>, items: &[PlannedItem], t: &Target) -> (Vec<Skipped>, Vec<Skipped>) {
    if t.plasma() {
        return (Vec::new(), skipped);
    }
    let has = |k: &str| items.iter().any(|i| i.kind == k);
    let mut covered = Vec::new();
    let mut rest = Vec::new();
    for s in skipped {
        let why = match s.kind.as_str() {
            "global-look" if !items.is_empty() => {
                Some(tr("in Plasma it is one package of the parts listed here; on this desktop Rexon applies them one by one"))
            }
            "shell-theme" if has("gtk-theme") && matches!(t.de.as_str(), "mate" | "xfce") => Some(trf(
                "the %1 panel is drawn by the GTK theme, and the GTK theme of this look is applied",
                &[&desktop_name(&t.de)],
            )),
            _ => None,
        };
        match why {
            Some(reason) => covered.push(Skipped { kind: s.kind, reason }),
            None => rest.push(s),
        }
    }
    (covered, rest)
}

/// A KWin effect from the look description — for the person.
fn effect_label(effect: &str) -> String {
    match effect {
        "wobbly-windows" => tr("wobbly windows"),
        "magic-lamp" => tr("magic lamp minimise"),
        "fade" => tr("fading windows"),
        "cover-switch" => tr("cover switch window switcher"),
        "desktop-change-osd" => tr("desktop name pop-up when switching desktops"),
        "borderless-maximized" => tr("maximised windows without title bars"),
        other => other.to_string(),
    }
}

/// What of the look's behavior the window manager `consumer` carries over (`compiz`
/// — Compiz takes its place, [`compiz_replaces`]).
fn behaviour_carried(consumer: &str, what: &str, compiz: bool) -> bool {
    if compiz {
        return matches!(what, "wobbly-windows" | "magic-lamp" | "fade" | "cover-switch" | "desktop-change-osd");
    }
    matches!(
        (consumer, what),
        ("xfwm4", "borderless-maximized") | ("muffin", "fade" | "desktop-change-osd")
    )
}

/// The look's behavior description (`behaviour` of the description for other desktops).
fn model_behaviour(look: &Look) -> Option<&Value> {
    look.model.as_ref()?.get("behaviour")
}

/// The look's panels description.
fn model_panels(look: &Look) -> Vec<&Value> {
    look.model.as_ref().and_then(|m| m.get("panels")).and_then(Value::as_array).map(|a| a.iter().collect()).unwrap_or_default()
}

/// What of the part being installed `it` will not be on this desktop.
fn item_limits(look: &Look, it: &PlannedItem, t: &Target, roles: &BTreeSet<String>) -> Vec<Skipped> {
    if t.plasma() {
        return Vec::new();
    }
    let lim = |reason: String| Skipped { kind: it.kind.clone(), reason };
    let mut out = Vec::new();
    match (it.kind.as_str(), it.consumer.as_str()) {
        ("behaviour", consumer) => {
            let Some(b) = model_behaviour(look) else { return out };
            let mut wanted: Vec<String> =
                b.get("effects").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect();
            if b.get("borderless_maximized").and_then(Value::as_bool) == Some(true) {
                wanted.push("borderless-maximized".into());
            }
            let compiz = compiz_replaces(consumer) && roles.contains(COMPIZ_ROLE);
            let lost: Vec<String> =
                wanted.iter().filter(|w| !behaviour_carried(consumer, w, compiz)).map(|w| effect_label(w)).collect();
            if !lost.is_empty() {
                out.push(lim(if compiz {
                    trf("not carried over: %1 — Compiz has no such setting", &[&lost.join(", ")])
                } else {
                    trf(
                        "not carried over: %1 — %2 has no such settings, these are KWin effects",
                        &[&lost.join(", "), &wm_name(consumer)],
                    )
                }));
            }
            if compiz && wanted.iter().any(|w| w == "desktop-change-osd") {
                out.push(lim(tr(
                    "instead of the desktop name pop-up, Compiz briefly shows its map of desktops when you switch",
                )));
            }
        }
        ("panel-layout", "mate-panel") => {
            let panels = model_panels(look);
            let item_with = |k: &str, v: &str| {
                panels.iter().any(|p| {
                    p.get("items").and_then(Value::as_array).into_iter().flatten().any(|i| i.get(k).and_then(Value::as_str) == Some(v))
                })
            };
            if item_with("align", "center") {
                out.push(lim(tr("the clock sits at the right edge: the MATE panel can't put applets in the middle")));
            }
            if panels.iter().any(|p| p.get("hide").is_some()) {
                out.push(lim(tr("the dock does not move out of the way of windows: the MATE panel can only hide automatically")));
            }
        }
        // Only a look with a dock (a panel fitted to its contents) loses something here.
        ("panel-layout", "cinnamon-panel")
            if model_panels(look).iter().any(|p| p.get("expand").and_then(Value::as_bool) == Some(false)) =>
        {
            out.push(lim(tr(
                "the dock spans the whole width of the screen and keeps Cinnamon's own pinned programs: Cinnamon panels can't be shorter than the screen",
            )));
        }
        ("qt-style", _) | ("palette", QT6CT_ROLE | "lxqt") => {
            out.push(lim(tr("Qt programs pick it up after you log in again")));
        }
        _ => {}
    }
    out
}

/// The source id of the path `sources/<id>/…`.
fn source_id_of(path: &str) -> Option<&str> {
    path.strip_prefix(rxn::SOURCES_DIR)?.strip_prefix('/')?.split('/').next().filter(|s| !s.is_empty())
}

/// The path inside the source: `sources/<id>/<rel>` → `<rel>`.
fn rel_in_source(path: &str) -> &str {
    let rest = path.strip_prefix(rxn::SOURCES_DIR).and_then(|r| r.strip_prefix('/')).unwrap_or(path);
    rest.split_once('/').map(|(_, r)| r).unwrap_or("")
}

/// The state of a dependency's role.
#[derive(Debug, Clone)]
enum RoleState {
    Present,
    Install(ResolvedDep),
    Aur(ResolvedDep),
    /// The GNOME Shell extension is not installed — Rexon will install it itself (uuid).
    Extension(String),
    Missing(String),
}

/// The dependency's names for the source.
fn names_of(dep: &PackageDep, source: DepSource, family: PkgFamily) -> Option<Vec<String>> {
    let key = match source {
        DepSource::Repo => family.as_str(),
        DepSource::Aur if family == PkgFamily::Arch => rxn::AUR_NAMES_KEY,
        _ => return None,
    };
    let list = dep.names.get(key)?.as_array()?;
    Some(list.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
}

fn role_state(dep: &PackageDep, family: PkgFamily, probes: &Probes) -> RoleState {
    if dep.source == DepSource::GnomeExtensions {
        let uuid = dep.uuid.clone().unwrap_or_default();
        return if (probes.extension_installed)(&uuid) {
            RoleState::Present
        } else {
            RoleState::Extension(uuid)
        };
    }
    for source in dep.sources_in_order() {
        if let Some(names) = names_of(dep, source, family) {
            if !names.is_empty() && names.iter().all(|n| (probes.owner_of)(n).as_deref() == Some(n.as_str())) {
                return RoleState::Present;
            }
        }
    }
    match dep.resolve(family, probes.in_repos) {
        Some(r) if r.source == DepSource::Aur => RoleState::Aur(r),
        Some(r) if r.source == DepSource::Repo => RoleState::Install(r),
        _ => RoleState::Missing(tr("the programs it needs are not in this system's repositories")),
    }
}

/// The MATE Menu role: the menu a look's description prefers (`"prefer":
/// "mate-menu"`); without the package the built-in menu button takes its place.
pub const MATE_MENU_ROLE: &str = "mate-menu";

/// The Compiz role — the window manager that on MATE and XFCE (X11) gives
/// window effects "like in KWin" (see [`compiz_subplan`]).
pub const COMPIZ_ROLE: &str = "compiz";

/// Window managers whose place Compiz can take: Marco (MATE) and
/// xfwm4 (XFCE). Not Muffin and Mutter: they are also the desktop shell.
pub fn compiz_replaces(consumer: &str) -> bool {
    matches!(consumer, "marco" | "xfwm4")
}

/// Which window manager the plan replaces with Compiz — the name for the person;
/// `None` — it does not replace any.
pub fn compiz_replaced_wm(plan: &LookPlan) -> Option<&'static str> {
    if !plan.roles.contains(COMPIZ_ROLE) {
        return None;
    }
    plan.items.iter().find(|i| i.kind == "behaviour" && compiz_replaces(&i.consumer)).map(|i| wm_name(&i.consumer))
}

/// AUR roles Rexon builds itself rather than only naming them to the person.
/// The owner's decision of 31.10.2026 — an exception to the rule of 24.10 "AUR — never
/// automatically": for Garuda's window effects Compiz is installed by an AUR helper the
/// same way as other AUR packages in Rexon (`aur_install::install_argv`,
/// the password — with a window). No helper — the package is named in a list, as before.
pub const AUTO_AUR_ROLES: &[&str] = &[COMPIZ_ROLE];

/// The qt6ct role (and consumer).
pub const QT6CT_ROLE: &str = "qt6ct";

/// The state of the implicit qt6ct dependency: installed, being installed from the repository, or
/// nowhere to get it from.
fn qt6ct_state(family: PkgFamily, probes: &Probes) -> RoleState {
    let name = crate::utils::tool_package_name(crate::themes::QT_STYLE_TOOL, family.as_str());
    if name.is_empty() {
        return RoleState::Missing(String::new());
    }
    if (probes.owner_of)(&name).as_deref() == Some(name.as_str()) {
        return RoleState::Present;
    }
    if (probes.in_repos)(&name) {
        return RoleState::Install(ResolvedDep { source: DepSource::Repo, names: vec![name] });
    }
    RoleState::Missing(String::new())
}

/// Build the apply plan — without a single launch or write.
pub fn plan(look: &Look, t: &Target, req: &Request, probes: &Probes) -> Prepared {
    let pkg = &look.pkg;
    let name = pkg.name.get(&t.lang).to_string();
    let unmet = rxn::unmet_preconditions(pkg, probes.owner_of);
    if !unmet.is_empty() {
        let reason = unmet.iter().map(|u| u.text(&t.lang)).collect::<Vec<_>>().join("\n");
        return Prepared::Refused { reason, aur: Vec::new() };
    }
    let pc = rxn::choose_all_with(pkg, &t.session, &req.values, &look_generator);
    if let Some(i) = pc.blocked_by {
        let why = match &pc.choices[i] {
            Choice::Refuse { reason } => reason.text(),
            _ => String::new(),
        };
        return Prepared::Refused {
            reason: trf(
                "“%1” can't be applied on this desktop: its required part “%2” is missing. %3",
                &[&name, &kind_label(&pkg.items[i].kind), &why],
            ),
            aur: Vec::new(),
        };
    }

    let mut states: BTreeMap<String, RoleState> = BTreeMap::new();
    let mut items = Vec::new();
    let mut skipped = Vec::new();
    let mut install: Vec<DepInstall> = Vec::new();
    let mut aur: Vec<AurNeed> = Vec::new();
    let mut aur_build: Vec<AurNeed> = Vec::new();
    let mut extensions: Vec<ExtNeed> = Vec::new();
    let mut roles = BTreeSet::new();

    for (i, (item, choice)) in pkg.items.iter().zip(&pc.choices).enumerate() {
        let (consumer, path) = match choice {
            Choice::Off => continue,
            Choice::Refuse { reason } => {
                skipped.push(Skipped { kind: item.kind.clone(), reason: refusal_text(&item.kind, t, reason, look.settings.as_ref()) });
                continue;
            }
            // Rexon builds it ([`look_generator`]); the path is the description it is
            // built from.
            Choice::Generate { consumer, model, .. } => (consumer.clone(), model.clone()),
            Choice::Native { consumer, path }
            | Choice::Compatible { consumer, path, .. }
            | Choice::Substitute { consumer, path, .. } => (consumer.clone(), path.clone()),
        };
        let opt_in = OPT_IN_KINDS.contains(&item.kind.as_str()) && !req.enable.contains(&item.kind);
        if !item.required && (opt_in || req.disable.contains(&item.kind)) {
            skipped.push(Skipped { kind: item.kind.clone(), reason: not_chosen_reason() });
            continue;
        }
        let src_role = source_id_of(&path).and_then(|sid| pkg.source(sid)).and_then(|s| s.package.clone());
        let mut block: Option<String> = None;
        let mut my_install = Vec::new();
        let mut my_ext: Vec<(String, String)> = Vec::new();
        let mut my_roles = Vec::new();
        for dep in pkg.deps_for_choice_with(&item.kind, &consumer, &req.values) {
            let st = states.entry(dep.role.clone()).or_insert_with(|| role_state(dep, t.family, probes)).clone();
            let needed = dep.required || src_role.as_deref() == Some(dep.role.as_str());
            match st {
                RoleState::Present => my_roles.push(dep.role.clone()),
                RoleState::Install(r) => {
                    my_roles.push(dep.role.clone());
                    my_install.push(DepInstall { role: dep.role.clone(), resolved: r });
                }
                RoleState::Extension(uuid) => {
                    my_roles.push(dep.role.clone());
                    my_ext.push((dep.role.clone(), uuid));
                }
                RoleState::Aur(r) if AUTO_AUR_ROLES.contains(&dep.role.as_str()) && t.aur_helper.is_some() && !t.wayland => {
                    my_roles.push(dep.role.clone());
                    if !aur_build.iter().any(|a| a.role == dep.role) {
                        aur_build.push(AurNeed {
                            role: dep.role.clone(),
                            packages: r.names.clone(),
                            kinds: vec![item.kind.clone()],
                            needed: false,
                        });
                    }
                }
                RoleState::Aur(r) => {
                    match aur.iter_mut().find(|a| a.role == dep.role) {
                        Some(a) => {
                            if !a.kinds.contains(&item.kind) {
                                a.kinds.push(item.kind.clone());
                            }
                            a.needed |= needed;
                        }
                        None => aur.push(AurNeed {
                            role: dep.role.clone(),
                            packages: r.install_names(),
                            kinds: vec![item.kind.clone()],
                            needed,
                        }),
                    }
                    if needed && block.is_none() {
                        block = Some(trf(
                            "it needs %1 from the AUR, and Rexon does not build AUR packages by itself",
                            &[&r.names.join(", ")],
                        ));
                    }
                }
                RoleState::Missing(why) => {
                    if needed && block.is_none() {
                        block = Some(why);
                    }
                }
            }
        }
        if let Some(why) = block {
            if item.required {
                return Prepared::Refused {
                    reason: trf("“%1” was not applied: %2 — %3.", &[&name, &kind_label(&item.kind), &why]),
                    aur,
                };
            }
            skipped.push(Skipped { kind: item.kind.clone(), reason: why });
            continue;
        }
        // The `qt6ct` consumer does not work without qt6ct itself — this is
        // Rexon's knowledge, not the look's, so the dependency is implicit and shared by all
        // looks. If there is nothing to install it with, the Qt style takes the fallback path
        // (`QT_STYLE_OVERRIDE`), the Qt palette will not be set.
        if consumer == QT6CT_ROLE {
            match states.entry(QT6CT_ROLE.into()).or_insert_with(|| qt6ct_state(t.family, probes)).clone() {
                RoleState::Present => my_roles.push(QT6CT_ROLE.into()),
                RoleState::Install(r) => {
                    my_roles.push(QT6CT_ROLE.into());
                    my_install.push(DepInstall { role: QT6CT_ROLE.into(), resolved: r });
                }
                _ => {}
            }
        }
        for d in my_install {
            if !install.iter().any(|x| x.role == d.role) {
                install.push(d);
            }
        }
        for (role, uuid) in my_ext {
            match extensions.iter_mut().find(|e| e.role == role) {
                Some(e) => {
                    if !e.kinds.contains(&item.kind) {
                        e.kinds.push(item.kind.clone());
                    }
                }
                None => extensions.push(ExtNeed { role, uuid, kinds: vec![item.kind.clone()] }),
            }
        }
        roles.extend(my_roles);
        items.push(PlannedItem { index: i, kind: item.kind.clone(), consumer, path, choice: choice.clone() });
    }

    let (covered, skipped) = split_covered(skipped, &items, t);
    let limits: Vec<Skipped> = items.iter().flat_map(|it| item_limits(look, it, t, &roles)).collect();
    let uses_settings = t.plasma() && look.settings.is_some();
    let mut fetch: Vec<FetchJob> = Vec::new();
    for path in needed_paths(look, &items, &roles, uses_settings) {
        let Some(sid) = source_id_of(&path) else { continue };
        let Some(src) = pkg.source(sid) else { continue };
        if !src.is_fetched() || fetch.iter().any(|f| f.id == sid) {
            continue;
        }
        let pin = match (&src.rev, &src.sha256) {
            (Some(rev), _) => Pin::Git(rev.clone()),
            (None, Some(sha)) => Pin::Sha256(sha.clone()),
            _ => continue,
        };
        let short: String = pin.text().chars().take(12).collect();
        fetch.push(FetchJob {
            id: sid.to_string(),
            url: src.url.clone().unwrap_or_default(),
            dir: t.looks_cache().join("sources").join(format!("{sid}-{short}")),
            pin,
        });
    }

    Prepared::Ready(LookPlan {
        id: pkg.id.clone(),
        name,
        items,
        skipped,
        install,
        aur,
        roles,
        fetch,
        uses_settings,
        values: req.values.clone(),
        covered,
        limits,
        aur_build,
        extensions,
    })
}

/// The `sources/…` paths applying needs: the elements' payload and the Plasma
/// description files whose role will be on the system.
fn needed_paths(look: &Look, items: &[PlannedItem], roles: &BTreeSet<String>, uses_settings: bool) -> Vec<String> {
    let mut out: Vec<String> = items.iter().filter(|i| source_id_of(&i.path).is_some()).map(|i| i.path.clone()).collect();
    if uses_settings {
        for f in settings_files(look, roles) {
            out.push(f.0);
        }
    }
    out
}

/// Plasma description files (`files`): `(from, to)` with a role on the system.
fn settings_files(look: &Look, roles: &BTreeSet<String>) -> Vec<(String, String)> {
    let Some(s) = &look.settings else { return Vec::new() };
    s.get("files")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|f| role_ok(f, roles))
        .filter_map(|f| Some((f.get("from")?.as_str()?.to_string(), f.get("to")?.as_str()?.to_string())))
        .collect()
}

/// A description record's `needs_role` — the role will be on the system (or is not needed).
fn role_ok(v: &Value, roles: &BTreeSet<String>) -> bool {
    v.get("needs_role").and_then(Value::as_str).is_none_or(|r| roles.contains(r))
}

// ============================================================================
// Execution
// ============================================================================

/// The externals for [`apply`]: commands — through `ctx`, the rest — here.
pub struct Machine<'a> {
    pub ctx: &'a Ctx<'a>,
    /// Download a source into `job.dir` verifying the pin ([`fetch_live`]).
    pub fetch: &'a dyn Fn(&FetchJob) -> Result<(), String>,
    /// Install packages from the official repositories (the password window).
    pub install_packages: &'a dyn Fn(&[String]) -> Result<(), String>,
    /// Remove packages installed by this application (a verification refusal).
    pub remove_packages: &'a dyn Fn(&[String]) -> Result<(), String>,
    /// A system snapshot: which package provides the name — called before and after.
    pub owner_of: OwnerOf<'a>,
    /// Whether a Plasma package is installed: (`plasmoids`/`wallpapers`, id).
    pub kpackage_installed: &'a dyn Fn(&str, &str) -> bool,
    /// Whether there is a `<name>.desktop` launcher in the application directories.
    pub desktop_installed: &'a dyn Fn(&str) -> bool,
    /// The full path of the `<name>.desktop` launcher (panel launchers outside
    /// Plasma); `None` — the program is missing.
    pub desktop_path: &'a dyn Fn(&str) -> Option<String>,
    /// Build and install AUR packages with a helper (the password window) —
    /// [`LookPlan::aur_build`].
    pub install_aur: &'a dyn Fn(&[String]) -> Result<(), String>,
    /// Install a GNOME Shell extension by uuid ([`LookPlan::extensions`];
    /// live — [`crate::gnome_ext_section::live_install_dependency`]).
    pub install_extension: &'a dyn Fn(&str) -> Result<crate::gnome_ext_section::DepOutcome, crate::gnome_shell_ext::ShellExtError>,
}

/// The result of applying.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AppliedLook {
    pub id: String,
    /// The result for the person.
    pub message: String,
    pub notes: Vec<String>,
    pub skipped: Vec<Skipped>,
    pub aur: Vec<AurNeed>,
    /// Packages installed by the application.
    pub installed: Vec<String>,
    pub after_apply: Option<String>,
}

/// Apply the look by the plan `plan` (built by [`plan`] on the same machine).
/// `Err` — the look was not applied, the reason for the person; everything the run managed
/// to write to the profile has been restored.
pub fn apply(look: &Look, plan: &LookPlan, t: &Target, m: &Machine) -> Result<AppliedLook, String> {
    let name = &plan.name;
    let fail = |why: String| trf("“%1” was not applied: %2", &[name, &why]);

    // A repeated application — starting from what was there before the first one.
    if is_applied(&plan.id, &t.rexon_dir()) {
        revert(&plan.id, t, m.ctx).map_err(&fail)?;
    }

    for job in &plan.fetch {
        if !job.cached() {
            (m.fetch)(job).map_err(&fail)?;
        }
    }

    // Dependencies: all or nothing.
    let mut watch: BTreeSet<String> = look.pkg.requires.forbid.iter().cloned().collect();
    for d in &plan.install {
        watch.extend(d.resolved.names.iter().cloned());
        if let Some(dep) = look.pkg.requires.packages.iter().find(|p| p.role == d.role) {
            watch.extend(dep.forbid.iter().cloned());
        }
    }
    let before: BTreeMap<String, Option<String>> = watch.iter().map(|n| (n.clone(), (m.owner_of)(n))).collect();
    let mut installed = Vec::new();
    if !plan.install.is_empty() {
        let mut names: Vec<String> = Vec::new();
        for d in &plan.install {
            for n in d.resolved.install_names() {
                if !names.contains(&n) {
                    names.push(n);
                }
            }
        }
        if let Err(e) = (m.install_packages)(&names) {
            return Err(fail(trf("the programs it needs were not installed (%1). %2", &[&names.join(", "), &e])));
        }
        let after: BTreeMap<String, Option<String>> = watch.iter().map(|n| (n.clone(), (m.owner_of)(n))).collect();
        let b = |n: &str| before.get(n).cloned().flatten();
        let a = |n: &str| after.get(n).cloned().flatten();
        let mut verdict = rxn::verify_package_change(&look.pkg, &b, &a);
        for d in &plan.install {
            if verdict.is_err() {
                break;
            }
            if let Some(dep) = look.pkg.requires.packages.iter().find(|p| p.role == d.role) {
                verdict = rxn::verify_dependency(dep, &d.resolved, &b, &a);
            }
        }
        installed = watch.iter().filter(|n| b(n).is_none() && a(n).is_some()).cloned().collect();
        if let Err(why) = verdict {
            let removed = if installed.is_empty() {
                String::new()
            } else {
                match (m.remove_packages)(&installed) {
                    Ok(()) => trf("Rexon removed what it had installed: %1.", &[&installed.join(", ")]),
                    Err(e) => trf("Rexon could not remove what it had installed (%1): %2", &[&installed.join(", "), &e]),
                }
            };
            return Err(fail(format!("{why} {removed}").trim().to_string()));
        }
    }

    // Building from the AUR (only [`AUTO_AUR_ROLES`]): if it failed, the look is still
    // applied, without what these packages give (the reason goes to the person).
    let mut early_notes: Vec<String> = Vec::new();
    if !plan.aur_build.is_empty() {
        let names: Vec<String> = plan.aur_build.iter().flat_map(|a| a.packages.iter().cloned()).collect();
        let before: Vec<bool> = names.iter().map(|n| (m.owner_of)(n).is_some()).collect();
        match (m.install_aur)(&names) {
            Ok(()) => {
                for (n, had) in names.iter().zip(before) {
                    if !had && (m.owner_of)(n).is_some() && !installed.contains(n) {
                        installed.push(n.clone());
                    }
                }
            }
            Err(e) => early_notes.push(trf(
                "%1 was not built from the AUR (%2), so what it gives is left out.",
                &[&names.join(", "), &e],
            )),
        }
    }

    // GNOME Shell extensions — right away, without a Rexon question: the protection is the shell's
    // confirmation window. A refusal in it, no release for this GNOME version,
    // a network failure — the look is still applied, and the person is honestly told
    // what will be missing without the extension.
    let mut ext_lines: Vec<String> = Vec::new();
    for e in &plan.extensions {
        let without = e.kinds.iter().map(|k| kind_label(k)).collect::<Vec<_>>().join(", ");
        let r = (m.install_extension)(&e.uuid);
        if let Some(line) = crate::gnome_ext_section::dependency_note(&e.uuid, &without, &r) {
            ext_lines.push(line);
        }
    }

    // Files: prepare, check, build the steps.
    let prepared = prepare(look, plan, t, m).map_err(&fail)?;
    let owner = owner_of_look(&plan.id);
    let known: BTreeSet<String> = BTreeSet::new();
    let mut file_steps: Vec<ExtStep> = Vec::new();
    let mut notes: Vec<String> = early_notes;
    let mut item_paths: BTreeMap<usize, Vec<String>> = BTreeMap::new();
    for it in &plan.items {
        for (src, dest) in units_of(look, it, &prepared, t) {
            if put_unit(&src, &dest, &owner, &known, &mut file_steps, &mut notes) {
                item_paths.entry(it.index).or_default().push(dest);
            }
        }
    }
    if plan.uses_settings {
        for (from, to) in settings_files(look, &plan.roles) {
            let src = prepared.join(&from);
            let dest = t.subst(&to);
            if !src.exists() {
                continue;
            }
            if !Path::new(&dest).starts_with(&t.home) {
                return Err(fail(trf("the look wants to write outside your home folder: %1", &[&dest])));
            }
            put_unit(&src, &dest, &owner, &known, &mut file_steps, &mut notes);
        }
    }
    let existed = |p: &str| std::fs::symlink_metadata(p).is_ok();
    let placed = journal::placed_from_steps(&file_steps, &existed);

    // Enabling and restore commands.
    let mut restore: Vec<Vec<String>> = Vec::new();
    let subplans = if plan.uses_settings {
        plasma_subplans(look, plan, t, m, &mut restore)
    } else {
        desktop_subplans(look, plan, t, m, &prepared, &mut restore, &mut notes)
    };

    // One rollback for the whole run; it captures the desktop's previous values (gsettings, dconf,
    // xfconf) and the `kwriteconfig6` files itself before the commands.
    let mut undo = Undo::new().keeping_values();
    let mut pre = Vec::new();
    let files: Vec<Step> = file_steps.iter().cloned().map(Step::from).collect();
    let mut outcome = exec_undoable(&files, &mut pre, m.ctx, &mut undo).map(|r| {
        notes.extend(r.notes);
    });
    if outcome.is_ok() {
        for sp in &subplans {
            match exec_undoable(&sp.steps, &mut pre, m.ctx, &mut undo) {
                Err(f) => {
                    outcome = Err(f);
                    break;
                }
                Ok(r) => {
                    notes.extend(r.notes);
                    if !sp.must_ok && !r.apply.ok {
                        if let Some(n) = &sp.fail_note {
                            notes.push(n.clone());
                        }
                    }
                    if sp.must_ok && !r.apply.ok {
                        let why = r.apply.reason.unwrap_or_else(|| trf("%1 did not apply", &[&sp.what]));
                        outcome = Err(crate::install_plan::Failure::Text(why));
                        break;
                    }
                }
            }
        }
    }
    if let Err(f) = outcome {
        undo.finish(false);
        let mut why = crate::install_plan::failure_text(&f);
        if !installed.is_empty() {
            why.push(' ');
            why.push_str(&trf("The programs Rexon installed for it stay: %1.", &[&installed.join(", ")]));
        }
        return Err(fail(why));
    }

    record(look, plan, t, undo, placed, &item_paths, &restore, &installed);

    // The look changed the theme and colors: terminals that take the theme colors get a
    // readable pair, under the same look record in the shared log.
    let term_env = crate::terminal_guard::Env {
        de: t.de.clone(),
        home: t.home.clone(),
        config: t.config.clone(),
        data: t.data.clone(),
    };
    let terminal_line =
        crate::terminal_guard::guard(&term_env, m.ctx, &section_of(&plan.id), &plan.id, Some(journal_key(&plan.id)));

    let after_apply = look
        .settings
        .as_ref()
        .filter(|_| plan.uses_settings)
        .and_then(|s| s.get("after_apply"))
        .map(|v| localized(v, &t.lang));
    let mut lines = vec![trf("“%1” applied.", &[name])];
    lines.extend(ext_lines);
    for s in &plan.skipped {
        lines.push(trf("Skipped: %1 — %2", &[&kind_label(&s.kind), &s.reason]));
    }
    if !plan.aur.is_empty() {
        lines.push(aur_text(&plan.aur));
    }
    if let Some(l) = terminal_line {
        lines.push(l);
    }
    if let Some(a) = &after_apply {
        lines.push(a.clone());
    }
    Ok(AppliedLook {
        id: plan.id.clone(),
        message: lines.join("\n"),
        notes,
        skipped: plan.skipped.clone(),
        aur: plan.aur.clone(),
        installed,
        after_apply,
    })
}

/// The line "can be installed from the AUR yourself".
pub fn aur_text(aur: &[AurNeed]) -> String {
    let names: Vec<String> = aur.iter().flat_map(|a| a.packages.iter().cloned()).collect();
    trf(
        "Rexon does not build AUR packages by itself. If you want the missing parts, install them yourself and apply the look again: %1",
        &[&names.join(" ")],
    )
}

/// The text `{en, ru, de}` in the language `lang` (missing — English).
fn localized(v: &Value, lang: &str) -> String {
    v.get(lang).or_else(|| v.get("en")).and_then(Value::as_str).unwrap_or_default().to_string()
}

/// Write the "what it was" snapshot and the look into the shared log.
#[allow(clippy::too_many_arguments)]
fn record(
    look: &Look,
    plan: &LookPlan,
    t: &Target,
    mut undo: Undo,
    placed: Vec<journal::Placed>,
    item_paths: &BTreeMap<usize, Vec<String>>,
    restore: &[Vec<String>],
    installed: &[String],
) {
    let rexon = t.rexon_dir();
    let section = section_of(&plan.id);
    let dir = journal::applied_snapshot_dir(&rexon, &section);
    let values = undo.take_values();
    let touched = undo.keep_into(&dir, 1);
    let now = now_iso();
    let text = |p: &Path| p.to_string_lossy().into_owned();
    // A key the snapshot will restore is not restored a second time with our own
    // restore commands.
    let covered: Vec<&str> = values.iter().map(|v| v.key.as_str()).collect();
    let restore: Vec<&Vec<String>> = restore
        .iter()
        .filter(|argv| crate::install_plan::value_key_of(argv).is_none_or(|k| !covered.contains(&k.as_str())))
        .collect();
    if !touched.is_empty() || !values.is_empty() {
        let applied = journal::Applied {
            section: section.clone(),
            theme: plan.id.clone(),
            item: Some(journal_key(&plan.id)),
            applied_at: Some(now.clone()),
            snapshot: text(&dir),
            files: touched
                .iter()
                .map(|t| journal::AppliedFile { path: text(&t.path), before: t.before.as_deref().map(text), dir: t.dir })
                .collect(),
        };
        let values = values.into_iter().map(journal::AppliedValue::from).collect();
        if let Err(e) = journal::record_applied_values_at(&rexon, applied, values) {
            crate::log_warn!("install", "образ {}: «что было» не записано в журнал — {e}", plan.id);
        }
    }
    let elements = plan
        .items
        .iter()
        .map(|it| {
            let chosen = journal::ElementChoice::from_choice(&it.choice).map(|(_, c)| c);
            journal::Element {
                kind: it.kind.clone(),
                role: look.pkg.items[it.index].role.clone(),
                consumer: Some(it.consumer.clone()),
                chosen,
                paths: item_paths.get(&it.index).cloned().unwrap_or_default(),
            }
        })
        .collect();
    let mut card = Map::new();
    card.insert("look".into(), json!(plan.id));
    card.insert("version".into(), json!(look.pkg.version));
    card.insert("values".into(), Value::Object(plan.values.clone().into_iter().collect()));
    card.insert("restore".into(), json!(restore));
    card.insert("packages".into(), json!(installed));
    let entry = journal::Entry {
        source: journal::Source::Look,
        id: plan.id.clone(),
        name: plan.name.clone(),
        kind: "distro-look".into(),
        section: Some(section),
        enabled: None,
        installed_at: Some(now),
        package_copy: None,
        placed_known: true,
        placed,
        elements,
        card: Some(card),
    };
    if let Err(e) = journal::record_look_at(&rexon, entry) {
        crate::log_warn!("install", "образ {}: не записан в общий журнал — {e}", plan.id);
    }
}

fn now_iso() -> String {
    match crate::qt_app::Timestamp::now_local() {
        Some(now) => crate::qt_app::Timestamp { microsecond: 0, ..now }.isoformat(),
        None => {
            let secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            crate::engine_update::now_iso(secs)
        }
    }
}

// ── Preparing files ─────────────────────────────────────────────────────────

/// The path is safe: relative, without `..`.
fn safe_rel(p: &str) -> bool {
    !p.starts_with('/') && !p.split('/').any(|c| c == "..")
}

/// Prepare the payload in our own cache directory (`prepared/<id>/sources/…`):
/// copy what is needed from the downloaded sources (`copy_only`), remove
/// what is excluded (`exclude`), perform the edits (`edits`), check
/// the contents. The directory is a copy of the same shape as the package paths.
///
/// The lists come from the Plasma description (when it is executed) and from the
/// description for other desktops (always: a look made for MATE or XFCE has no
/// Plasma description, and its themes still must not bring their Cinnamon parts).
fn prepare(look: &Look, plan: &LookPlan, t: &Target, m: &Machine) -> Result<PathBuf, String> {
    let root = t.prepared_dir(&plan.id);
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let settings = look.settings.as_ref().filter(|_| plan.uses_settings);
    let descriptions: Vec<&Value> = settings.into_iter().chain(look.model.as_ref()).collect();
    let values = |key: &str| -> Vec<Value> {
        descriptions.iter().filter_map(|d| d.get(key)).filter_map(Value::as_array).flatten().cloned().collect()
    };
    let list = |key: &str| -> Vec<String> { values(key).iter().filter_map(|v| v.as_str().map(str::to_string)).collect() };
    let copy_only = list("copy_only");
    let exclude = list("exclude");

    for path in needed_paths(look, &plan.items, &plan.roles, plan.uses_settings) {
        let Some(job) = source_id_of(&path).and_then(|sid| plan.fetch.iter().find(|f| f.id == sid)) else {
            continue; // the source is a system package: nothing to copy
        };
        if !safe_rel(&path) {
            return Err(trf("unsafe path in the look: %1", &[&path]));
        }
        let under = |p: &str| p == path || p.starts_with(&format!("{path}/"));
        let only: Vec<&String> = copy_only.iter().filter(|c| under(c)).collect();
        let picks: Vec<String> = if only.is_empty() { vec![path.clone()] } else { only.into_iter().cloned().collect() };
        for p in picks {
            let src = job.dir.join(rel_in_source(&p));
            let dest = root.join(&p);
            if std::fs::symlink_metadata(&src).is_err() {
                return Err(trf("the downloaded source “%1” has no %2", &[&job.id, &rel_in_source(&p)]));
            }
            copy_any(&src, &dest).map_err(|e| trf("couldn't prepare %1: %2", &[&p, &e]))?;
        }
        // The whole source is the unit (a git repository whose root is the theme):
        // our download marker is not part of it.
        if rel_in_source(&path).is_empty() {
            let _ = std::fs::remove_file(root.join(&path).join(PIN_MARK));
        }
    }
    for x in &exclude {
        if safe_rel(x) {
            remove_any(&root.join(x));
        }
    }
    for e in values("edits") {
        run_edit(&e, &root, t, m)?;
    }
    strip_exec_bits(&look.pkg, &root);
    let problems = check_prepared(&look.pkg, &root);
    if !problems.is_empty() {
        return Err(problems);
    }
    Ok(root)
}

fn copy_any(src: &Path, dest: &Path) -> std::io::Result<()> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let md = std::fs::symlink_metadata(src)?;
    if md.is_dir() {
        crate::install_plan::copy_tree(src, dest, &mut Undo::off())
    } else {
        crate::install_plan::copy_file(src, dest)
    }
}

fn remove_any(p: &Path) {
    match std::fs::symlink_metadata(p) {
        Ok(m) if m.is_dir() => {
            let _ = std::fs::remove_dir_all(p);
        }
        Ok(_) => {
            let _ = std::fs::remove_file(p);
        }
        Err(_) => {}
    }
}

/// All files under `dir` (without links), in order.
fn walk_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = rd.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let p = e.path();
        match std::fs::symlink_metadata(&p) {
            Ok(m) if m.is_dir() => walk_files(&p, out),
            Ok(m) if m.is_file() => out.push(p),
            _ => {}
        }
    }
}

/// Theme repositories often keep every picture with the executable bit (the
/// B00merang themes: thousands of PNG files with mode 755). For an element
/// without the `exec` permission the payload is data: the bit is dropped from
/// our prepared copy, so nothing of it can be run from the profile. What runs
/// code by its contents (a shebang, ELF, `.sh`) is still refused by
/// [`check_prepared`].
fn strip_exec_bits(pkg: &Package, root: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let mut files = Vec::new();
    walk_files(root, &mut files);
    for f in files {
        let rel = f.strip_prefix(root).map(|r| r.to_string_lossy().into_owned()).unwrap_or_default();
        let Ok(md) = std::fs::metadata(&f) else { continue };
        let mode = md.permissions().mode();
        if mode & 0o111 != 0 && !rxn::path_allows_exec(pkg, &rel) {
            let _ = std::fs::set_permissions(&f, std::fs::Permissions::from_mode(mode & !0o111));
        }
    }
}

/// [`rxn::check_contents`] over the prepared files: executables only for
/// elements with the `exec` permission, SVG without scripts.
fn check_prepared(pkg: &Package, root: &Path) -> String {
    use std::io::Read;
    use std::os::unix::fs::PermissionsExt;
    let mut files = Vec::new();
    walk_files(root, &mut files);
    let mut heads: Vec<(String, Vec<u8>, bool)> = Vec::new();
    for f in files {
        let rel = f.strip_prefix(root).map(|r| r.to_string_lossy().into_owned()).unwrap_or_default();
        let mut head = vec![0u8; 8192];
        let n = std::fs::File::open(&f).and_then(|mut h| h.read(&mut head)).unwrap_or(0);
        head.truncate(n);
        let exec = std::fs::metadata(&f).is_ok_and(|m| m.permissions().mode() & 0o111 != 0);
        heads.push((rel, head, exec));
    }
    let archive: Vec<rxn::ArchiveFile> =
        heads.iter().map(|(p, h, x)| rxn::ArchiveFile { path: p, head: h, executable: *x }).collect();
    let errors = rxn::check_contents(pkg, &archive);
    if errors.is_empty() {
        String::new()
    } else {
        rxn::errors_text(&errors)
    }
}

/// A declared `edits` edit of the description (a closed set, no code).
fn run_edit(e: &Value, root: &Path, t: &Target, m: &Machine) -> Result<(), String> {
    let Some(file) = e.get("file").and_then(Value::as_str) else { return Ok(()) };
    if !safe_rel(file) {
        return Err(trf("unsafe path in the look: %1", &[&file]));
    }
    let path = root.join(file);
    if let Some(filter) = e.get("applets_filter") {
        let ids: Vec<String> = filter
            .get("remove_when_missing")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str().map(str::to_string))
            .filter(|id| !(m.kpackage_installed)("plasmoids", id))
            .collect();
        let mut files = Vec::new();
        if path.is_dir() {
            walk_files(&path, &mut files);
            files.retain(|f| f.extension().is_some_and(|x| x == "js"));
        } else if path.is_file() {
            files.push(path.clone());
        }
        for f in files {
            edit_file(&f, |text| drop_applets(text, &ids))?;
        }
        return Ok(());
    }
    if !path.is_file() {
        return Ok(()); // the element is not installed — nothing to edit
    }
    if let Some(needle) = e.get("drop_lines_containing").and_then(Value::as_str) {
        edit_file(&path, |text| keep_lines(text, |l| !l.contains(needle)))?;
    }
    if let Some(r) = e.get("replace") {
        let from = r.get("from").and_then(Value::as_str).unwrap_or_default();
        let to = t.subst(r.get("to").and_then(Value::as_str).unwrap_or_default());
        if !from.is_empty() {
            edit_file(&path, |text| text.replace(from, &to))?;
        }
    }
    if let Some(s) = e.get("ini_set") {
        let header: String = s
            .get("group")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(|g| format!("[{g}]"))
            .collect();
        let key = s.get("key").and_then(Value::as_str).unwrap_or_default();
        let value = t.subst(s.get("value").and_then(Value::as_str).unwrap_or_default());
        if !header.is_empty() && !key.is_empty() {
            edit_file(&path, |text| crate::defaults::set_ini_value(Some(text), &header, key, &value))?;
        }
    }
    if let Some(f) = e.get("launchers_filter") {
        let drop: Vec<String> = f
            .get("drop")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect();
        let only_installed = f.get("keep_only_installed").and_then(Value::as_bool).unwrap_or(false);
        edit_file(&path, |text| filter_launchers(text, &drop, only_installed, m.desktop_installed))?;
    }
    Ok(())
}

fn edit_file(path: &Path, f: impl FnOnce(&str) -> String) -> Result<(), String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let new = f(&text);
    if new != text {
        std::fs::write(path, new).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(())
}

fn keep_lines(text: &str, keep: impl Fn(&str) -> bool) -> String {
    text.split_inclusive('\n').filter(|l| keep(l)).collect()
}

/// Remove the applets `ids` from a layout script: the `addWidget("<id>")` line and
/// the lines that follow it and configure its variable (`w.…`).
pub fn drop_applets(text: &str, ids: &[String]) -> String {
    if ids.is_empty() {
        return text.to_string();
    }
    let mut out = String::new();
    let mut var: Option<String> = None;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if let Some(v) = &var {
            if trimmed.starts_with(&format!("{v}.")) || trimmed.starts_with(&format!("{v}[")) {
                continue;
            }
            var = None;
        }
        let hit = ids.iter().any(|id| {
            line.contains(&format!("addWidget(\"{id}\"")) || line.contains(&format!("addWidget('{id}'"))
        });
        if hit {
            // `var w = panel.addWidget(…)` / `w = …` — remember the variable.
            let lhs = trimmed.split('=').next().unwrap_or("").trim();
            let name = lhs.trim_start_matches("var ").trim_start_matches("let ").trim_start_matches("const ").trim();
            if trimmed.contains('=') && !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                var = Some(name.to_string());
            }
            continue;
        }
        out.push_str(line);
    }
    out
}

/// Filter the dock launchers in lines with `launchers`: remove `drop` and (with
/// `only_installed`) launchers of programs that are missing. `preferred://…` stay.
pub fn filter_launchers(text: &str, drop: &[String], only_installed: bool, installed: &dyn Fn(&str) -> bool) -> String {
    let keep = |el: &str| -> bool {
        let el = el.trim();
        if el.is_empty() || el.starts_with("preferred://") {
            return !el.is_empty();
        }
        let base = el.rsplit(['/', ':']).next().unwrap_or(el);
        if drop.iter().any(|d| d == base) {
            return false;
        }
        if only_installed && base.ends_with(".desktop") {
            return installed(base);
        }
        true
    };
    let mut out = String::new();
    for line in text.split_inclusive('\n') {
        if !line.contains("launchers") {
            out.push_str(line);
            continue;
        }
        out.push_str(&filter_quoted(line, &keep));
    }
    out
}

/// In the line `line` replace each quoted literal with the list of its
/// elements (comma-separated) that passed `keep`; a literal that became empty and
/// was a single array element is removed together with its comma.
fn filter_quoted(line: &str, keep: &dyn Fn(&str) -> bool) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '"' || c == '\'' {
            if let Some(end) = (i + 1..chars.len()).find(|&j| chars[j] == c) {
                let inner: String = chars[i + 1..end].iter().collect();
                let looks_like = inner.contains(".desktop") || inner.contains("://") || inner.contains("applications:");
                if !looks_like {
                    out.extend(&chars[i..=end]);
                    i = end + 1;
                    continue;
                }
                let kept: Vec<&str> = inner.split(',').filter(|e| keep(e)).collect();
                if kept.is_empty() && !inner.contains(',') {
                    // A whole array element: remove it with the comma after (or before).
                    let mut j = end + 1;
                    while j < chars.len() && chars[j] == ' ' {
                        j += 1;
                    }
                    if j < chars.len() && chars[j] == ',' {
                        i = j + 1;
                        while i < chars.len() && chars[i] == ' ' {
                            i += 1;
                        }
                    } else {
                        let t = out.trim_end().to_string();
                        out = t.strip_suffix(',').map(str::to_string).unwrap_or(t);
                        i = end + 1;
                    }
                    continue;
                }
                out.push(c);
                out.push_str(&kept.join(","));
                out.push(c);
                i = end + 1;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

// ── Where files go ──────────────────────────────────────────────────────────

/// "Container" directories under `usr/share` and where they go in the profile.
const CONTAINERS: &[(&str, &str, &str)] = &[
    ("plasma/look-and-feel", "data", "plasma/look-and-feel"),
    ("plasma/desktoptheme", "data", "plasma/desktoptheme"),
    ("plasma/layout-templates", "data", "plasma/layout-templates"),
    ("plasma/plasmoids", "data", "plasma/plasmoids"),
    ("plasma/wallpapers", "data", "plasma/wallpapers"),
    ("aurorae/themes", "data", "aurorae/themes"),
    ("color-schemes", "data", "color-schemes"),
    ("themes", "data", "themes"),
    ("icons", "data", "icons"),
    ("wallpapers", "data", "wallpapers"),
    ("fonts", "data", "fonts"),
    ("Kvantum", "config", "Kvantum"),
];

fn base_dir(t: &Target, which: &str, rel: &str) -> String {
    let root = if which == "config" { &t.config } else { &t.data };
    format!("{}/{rel}", root.trim_end_matches('/'))
}

/// The profile directory for an element of the kind `kind` whose path is not under `usr/share`
/// (a git repository, a release archive).
fn kind_dir(t: &Target, kind: &str, consumer: &str) -> Option<String> {
    let rel = match (kind, consumer) {
        ("palette", _) => ("data", "color-schemes"),
        ("window-decoration", "aurorae") => ("data", "aurorae/themes"),
        ("window-decoration", _) | ("shell-theme", "gnome-shell" | "cinnamon") | ("gtk-theme", _) => ("data", "themes"),
        ("shell-theme", "plasma") => ("data", "plasma/desktoptheme"),
        ("icons" | "cursors", _) => ("data", "icons"),
        ("wallpaper", _) => ("data", "wallpapers"),
        ("fonts", _) => ("data", "fonts"),
        ("qt-style", _) => ("config", "Kvantum"),
        ("global-look", _) => ("data", "plasma/look-and-feel"),
        ("panel-layout", _) => ("data", "plasma/layout-templates"),
        _ => return None,
    };
    Some(base_dir(t, rel.0, rel.1))
}

/// An element's copy units: `(prepared path, path in the profile)`.
/// A container directory (`…/look-and-feel`) is laid out by its children.
fn units_of(look: &Look, it: &PlannedItem, prepared: &Path, t: &Target) -> Vec<(PathBuf, String)> {
    // What Rexon builds is installed by its own steps, the description is not put into the profile.
    if matches!(it.choice, Choice::Generate { .. }) {
        return Vec::new();
    }
    let Some(sid) = source_id_of(&it.path) else { return Vec::new() };
    let src_fetched = look.pkg.source(sid).is_some_and(|s| s.is_fetched());
    if !src_fetched {
        return Vec::new();
    }
    let src = prepared.join(&it.path);
    let rel = rel_in_source(&it.path);
    let children = |dir: &Path, base: &str| -> Vec<(PathBuf, String)> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .map(|rd| rd.flatten().filter_map(|e| e.file_name().to_str().map(str::to_string)).collect())
            .unwrap_or_default();
        names.sort();
        names.into_iter().map(|n| (dir.join(&n), format!("{base}/{n}"))).collect()
    };
    if let Some(rest) = rel.strip_prefix("usr/share/") {
        // The shared icon set (`hicolor`, `locolor`) from a package is not put into the profile:
        // all programs share its directory, and the package's `index.theme`
        // would shadow the system one (defect of 09.11.2026, [`crate::icon_hygiene`]).
        let shared = |prefix: &str, unit: &str| prefix == "icons" && crate::icon_hygiene::is_reserved_icon_set(unit);
        for (prefix, which, to) in CONTAINERS {
            let base = base_dir(t, which, to);
            if rest == *prefix {
                let mut units = children(&src, &base);
                units.retain(|(p, _)| !p.file_name().is_some_and(|n| shared(prefix, &n.to_string_lossy())));
                return units;
            }
            if let Some(tail) = rest.strip_prefix(&format!("{prefix}/")) {
                let unit = tail.split('/').next().unwrap_or(tail);
                if shared(prefix, unit) {
                    return Vec::new();
                }
                let unit_src = prepared.join(format!("{}/{sid}/usr/share/{prefix}/{unit}", rxn::SOURCES_DIR));
                return vec![(unit_src, format!("{base}/{unit}"))];
            }
        }
        return Vec::new();
    }
    let Some(base) = kind_dir(t, &it.kind, &it.consumer) else { return Vec::new() };
    // `sources/<id>` itself — the repository root is the theme, named by the source id
    // (the same name [`desktop_subplans`] enables it by).
    let name = if rel.is_empty() {
        sid.to_string()
    } else {
        Path::new(rel).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
    };
    if name.is_empty() {
        return Vec::new();
    }
    vec![(src, format!("{base}/{name}"))]
}

/// Put the unit `src` → `dest` on behalf of the look. If the place is taken by something foreign —
/// do not touch it (a note); `false` — not placed.
fn put_unit(
    src: &Path,
    dest: &str,
    owner: &str,
    known: &BTreeSet<String>,
    steps: &mut Vec<ExtStep>,
    notes: &mut Vec<String>,
) -> bool {
    if steps.iter().any(|s| matches!(s, ExtStep::CopyTree { dest: d, .. } | ExtStep::CopyFile { dest: d, .. } if d == dest)) {
        return false;
    }
    let is_dir = src.is_dir();
    let present = std::fs::symlink_metadata(dest);
    if let Ok(md) = &present {
        let ours = known.contains(dest)
            || (md.is_dir()
                && std::fs::read_to_string(Path::new(dest).join(OWNER_MARK)).is_ok_and(|t| t.trim() == owner));
        // A file inside our own directory (the Sweet-Darkrc frame on top of a copy) is
        // ours: the directory was placed by this run.
        let inside_ours = steps.iter().any(|s| match s {
            ExtStep::CopyTree { dest: d, .. } => dest.starts_with(&format!("{d}/")),
            _ => false,
        });
        if !ours && !inside_ours {
            let name = Path::new(dest).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            notes.push(trf("%1 is already there and is not Rexon's — it was left as it is.", &[&name]));
            return false;
        }
        if ours {
            steps.push(if md.is_dir() {
                ExtStep::RemoveTree { path: dest.to_string(), safe: true, ignore_errors: true }
            } else {
                ExtStep::RemoveFile { path: dest.to_string() }
            });
        }
    }
    let src_s = src.to_string_lossy().into_owned();
    if is_dir {
        steps.push(ExtStep::CopyTree { src: src_s, dest: dest.to_string() });
        steps.push(ExtStep::WriteFile { path: format!("{dest}/{OWNER_MARK}"), content: format!("{owner}\n") });
    } else {
        steps.push(ExtStep::CopyFile { src: src_s, dest: dest.to_string() });
    }
    true
}

// ── Enabling: Plasma ────────────────────────────────────────────────────────

/// Part of the run: the steps and whether their success is required.
struct SubPlan {
    what: String,
    steps: Vec<Step>,
    must_ok: bool,
    /// A note to the person if an optional part failed.
    fail_note: Option<String>,
}

fn cmd(argv: Vec<String>) -> Cmd {
    Cmd { argv, timeout: None }
}

fn sv(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| s.to_string()).collect()
}

/// argv for "KWin, reread the config".
pub fn kwin_reconfigure_argv() -> Vec<String> {
    let (dest, obj, method) = crate::utils::KWIN_RECONFIGURE;
    sv(&["qdbus", dest, obj, method])
}

/// Files the Plasma part's commands will rewrite (their "what it was" is captured by
/// [`ApplyStep::Guard`]).
fn plasma_guard(settings: &Value, t: &Target) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut add = |p: String| {
        if !p.ends_with('/') && !out.contains(&p) {
            out.push(p);
        }
    };
    for p in crate::backup::FULL_CONFIGS {
        if let Some(rest) = p.strip_prefix("~/.config/") {
            add(format!("{}/{rest}", t.config));
        }
    }
    for extra in ["ksplashrc", "gtk-3.0/settings.ini", "gtk-4.0/settings.ini", "gtkrc", "gtkrc-2.0", "xsettingsd/xsettingsd.conf", "Kvantum/kvantum.kvconfig"] {
        add(format!("{}/{extra}", t.config));
    }
    add(format!("{}/.gtkrc-2.0", t.home));
    for v in settings.get("backup_extra").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
        add(t.subst(v));
    }
    for k in settings.get("kconfig").and_then(Value::as_array).into_iter().flatten() {
        if let Some(f) = k.get("file").and_then(Value::as_str) {
            add(format!("{}/{f}", t.config));
        }
    }
    out
}

/// The value of an `apply` step: `${prop.<name>}` — the variant record by the property
/// value, otherwise — the value itself.
fn step_value<'v>(look: &'v Look, settings: &'v Value, key: &str, v: &'v Value, values: &PropValues) -> Option<&'v Value> {
    if let Some(s) = v.as_str() {
        if let Some(prop) = s.strip_prefix("${prop.").and_then(|r| r.strip_suffix('}')) {
            let chosen = look.pkg.prop_value(prop, values)?.as_str()?;
            return settings.get("variants")?.get(chosen)?.get(key);
        }
    }
    Some(v)
}

/// The variant by property value (for `needs_role`).
fn variant<'v>(look: &'v Look, settings: &'v Value, values: &PropValues) -> Option<&'v Value> {
    for (name, _) in &look.pkg.properties {
        if let Some(v) = look.pkg.prop_value(name, values).and_then(Value::as_str) {
            if let Some(var) = settings.get("variants").and_then(|vs| vs.get(v)) {
                return Some(var);
            }
        }
    }
    None
}

fn plasma_subplans(look: &Look, plan: &LookPlan, t: &Target, m: &Machine, restore: &mut Vec<Vec<String>>) -> Vec<SubPlan> {
    let Some(settings) = &look.settings else { return Vec::new() };
    let has = |kind: &str| plan.items.iter().any(|i| i.kind == kind);
    // The variant asks for a role that will not exist — without the variant's cursor.
    let cursor_ok = variant(look, settings, &plan.values).is_none_or(|v| role_ok(v, &plan.roles));
    let mut steps: Vec<ApplyStep> = vec![ApplyStep::Guard { paths: plasma_guard(settings, t) }];
    let mut started = false;
    let mut after: Vec<ApplyStep> = Vec::new();
    for entry in settings.get("apply").and_then(Value::as_array).into_iter().flatten() {
        let Some((key, raw)) = entry.as_object().and_then(|o| o.iter().next()) else { continue };
        let Some(val) = step_value(look, settings, key, raw, &plan.values) else { continue };
        let name = val.as_str().unwrap_or_default().to_string();
        match key.as_str() {
            "lookandfeel" if has("global-look") && !name.is_empty() => {
                let mut argv = sv(&["plasma-apply-lookandfeel", "-a", &name]);
                if entry.get("reset_layout").and_then(Value::as_bool).unwrap_or(false) {
                    argv.push("--resetLayout".into());
                }
                steps.push(ApplyStep::Primary(cmd(argv)));
                started = true;
            }
            "colorscheme" if has("palette") && !name.is_empty() => {
                after.push(ApplyStep::BestEffort(cmd(sv(&["plasma-apply-colorscheme", &name]))))
            }
            "desktoptheme" if has("shell-theme") && !name.is_empty() => {
                after.push(ApplyStep::BestEffort(cmd(sv(&["plasma-apply-desktoptheme", &name]))))
            }
            "cursortheme" if has("cursors") && cursor_ok && !name.is_empty() => {
                after.push(ApplyStep::BestEffort(cmd(sv(&["plasma-apply-cursortheme", &name]))))
            }
            "kvantum" if has("qt-style") && !name.is_empty() => after.push(ApplyStep::IniSetIfOk {
                path: format!("{}/Kvantum/kvantum.kvconfig", t.config),
                section: "[General]".into(),
                key: "theme".into(),
                value: name,
            }),
            "gtk" if has("gtk-theme") => {
                let spec = val;
                for ver in ["gtk-3.0", "gtk-4.0"] {
                    let path = format!("{}/{ver}/settings.ini", t.config);
                    let mut set = |key: &str, value: String| {
                        after.push(ApplyStep::GtkIni { path: path.clone(), key: key.into(), value })
                    };
                    let get = |k: &str| spec.get(k).and_then(Value::as_str).map(str::to_string);
                    if let Some(v) = get("theme") {
                        set("gtk-theme-name", v);
                    }
                    if let (Some(v), true) = (get("icons"), has("icons")) {
                        set("gtk-icon-theme-name", v);
                    }
                    if let (Some(v), true) = (get("cursor"), has("cursors") && cursor_ok) {
                        set("gtk-cursor-theme-name", v);
                    }
                    if let (Some(v), true) = (get("font"), has("fonts")) {
                        set("gtk-font-name", v);
                    }
                    if spec.get("prefer_dark").and_then(Value::as_bool) == Some(true) {
                        set("gtk-application-prefer-dark-theme", "1".into());
                    }
                    if let Some(v) = get("decoration_layout") {
                        set("gtk-decoration-layout", v);
                    }
                }
            }
            "wallpaper" if has("wallpaper") => {
                let plugin = val.get("plugin").and_then(Value::as_str).unwrap_or("org.kde.image");
                let key = val.get("key").and_then(Value::as_str).unwrap_or("Image");
                let value = t.subst(val.get("value").and_then(Value::as_str).unwrap_or_default());
                let plugin = if (m.kpackage_installed)("wallpapers", plugin) {
                    plugin.to_string()
                } else {
                    val.get("fallback_plugin").and_then(Value::as_str).unwrap_or("org.kde.image").to_string()
                };
                let script = format!(
                    "var all = desktops(); for (var i = 0; i < all.length; i++) {{ var d = all[i]; d.wallpaperPlugin = {p}; d.currentConfigGroup = [\"Wallpaper\", {p}, \"General\"]; d.writeConfig({k}, {v}); }}",
                    p = Value::from(plugin.as_str()),
                    k = Value::from(key),
                    v = Value::from(value.as_str()),
                );
                let (dest, obj, method) = crate::utils::PLASMA_EVALUATE_SCRIPT;
                after.push(ApplyStep::IfOk(cmd(sv(&["qdbus", dest, obj, method, &script]))));
            }
            "kwin_reconfigure" => {}
            _ => {}
        }
    }
    if !started {
        steps.push(ApplyStep::ForceOk);
    }
    steps.extend(after);
    if has("behaviour") {
        for k in settings.get("kconfig").and_then(Value::as_array).into_iter().flatten() {
            if !role_ok(k, &plan.roles) {
                continue;
            }
            let (Some(file), Some(key), Some(value)) = (
                k.get("file").and_then(Value::as_str),
                k.get("key").and_then(Value::as_str),
                k.get("value").and_then(Value::as_str),
            ) else {
                continue;
            };
            let mut argv = sv(&["kwriteconfig6", "--file", &format!("{}/{file}", t.config)]);
            for g in k.get("group").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
                argv.push("--group".into());
                argv.push(g.into());
            }
            argv.push("--key".into());
            argv.push(key.into());
            argv.push(t.subst(value));
            steps.push(ApplyStep::BestEffort(cmd(argv)));
        }
    }
    steps.push(ApplyStep::IfOk(Cmd { argv: kwin_reconfigure_argv(), timeout: Some(10) }));
    restore.push(kwin_reconfigure_argv());
    vec![SubPlan { what: plan.name.clone(), steps: steps.into_iter().map(Step::from).collect(), must_ok: true, fail_note: None }]
}

// ── Enabling: other desktops ────────────────────────────────────────────────

/// Commands of a press plan (without file steps — the snapshot restores them).
fn plan_commands(steps: &[Step]) -> Vec<Vec<String>> {
    steps
        .iter()
        .filter_map(|s| match s {
            Step::Apply(ApplyStep::Primary(c) | ApplyStep::BestEffort(c) | ApplyStep::IfOk(c) | ApplyStep::IfNotOk(c)) => {
                Some(c.argv.clone())
            }
            _ => None,
        })
        .collect()
}

/// Read a value with a command; empty — it was not read.
fn read_value(ctx: &Ctx, de: &str, argv: &[String]) -> String {
    let r = (ctx.run)(argv, Some(5));
    if r.returncode != Some(0) {
        return String::new();
    }
    crate::themes::parse_native_role_output(de, &crate::utils::decode_text(&r.stdout))
}

/// The desktop's native key: what to read and what to write the value with.
fn native_key(de: &str, kind: &str, consumer: &str) -> Option<(Vec<String>, Vec<String>)> {
    let gs = |schema: &str, key: &str| Some((sv(&["gsettings", "get", schema, key]), sv(&["gsettings", "set", schema, key])));
    match (de, kind, consumer) {
        ("xfce", "window-decoration", _) => Some((
            sv(&["xfconf-query", "-c", "xfwm4", "-p", "/general/theme"]),
            sv(&["xfconf-query", "-c", "xfwm4", "-p", "/general/theme", "-s"]),
        )),
        ("xfce", "fonts", _) => Some((
            sv(&["xfconf-query", "-c", "xsettings", "-p", "/Gtk/FontName"]),
            sv(&["xfconf-query", "-c", "xsettings", "-p", "/Gtk/FontName", "-s"]),
        )),
        ("gnome", "window-decoration", _) => gs("org.gnome.desktop.wm.preferences", "theme"),
        ("gnome", "shell-theme", "gnome-shell") => gs("org.gnome.shell.extensions.user-theme", "name"),
        ("gnome", "fonts", _) => gs("org.gnome.desktop.interface", "font-name"),
        ("cinnamon", "window-decoration", _) => gs("org.cinnamon.desktop.wm.preferences", "theme"),
        ("cinnamon", "shell-theme", "cinnamon") => gs("org.cinnamon.theme", "name"),
        ("cinnamon", "fonts", _) => gs("org.cinnamon.desktop.interface", "font-name"),
        ("mate", "window-decoration", _) => gs("org.mate.Marco.general", "theme"),
        ("mate", "fonts", _) => gs("org.mate.interface", "font-name"),
        // There is no font key of its own: GTK programs read the GNOME key (on
        // Wayland — directly; the «Fonts» section does the same).
        ("budgie" | "sway" | "hyprland" | "niri" | "river", "fonts", _) => gs("org.gnome.desktop.interface", "font-name"),
        _ => None,
    }
}

/// The look's GTK font from the Plasma description (`apply` → `gtk.font`) — look
/// data usable outside Plasma too.
fn gtk_font(look: &Look) -> Option<String> {
    let s = look.settings.as_ref()?;
    s.get("apply")?.as_array()?.iter().find_map(|e| e.get("gtk")?.get("font")?.as_str().map(str::to_string))
}

fn desktop_subplans(
    look: &Look,
    plan: &LookPlan,
    t: &Target,
    m: &Machine,
    prepared: &Path,
    restore: &mut Vec<Vec<String>>,
    notes: &mut Vec<String>,
) -> Vec<SubPlan> {
    let ctx = m.ctx;
    let de = t.de.as_str();
    let facts = PressFacts {
        de: de.to_string(),
        home: t.home.clone(),
        xdg_config_home: Some(t.config.clone()),
        icons_valid: true,
        cursor_safe: true,
        cursor_size: t.cursor_size,
        ..PressFacts::default()
    };
    // The look's icons, if they are installed: the qt6ct config gets them too (otherwise
    // Qt programs under it would be left with `hicolor`).
    let icon_theme = plan
        .items
        .iter()
        .find(|i| i.kind == "icons")
        .and_then(|i| Path::new(&i.path).file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut out = Vec::new();
    for it in &plan.items {
        let name = Path::new(&it.path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let generated = matches!(it.choice, Choice::Generate { .. });
        match (it.kind.as_str(), generated) {
            // A KDE scheme outside Plasma — as a Qt palette (qt6ct or LXQt); without qt6ct
            // there is nobody to read it.
            ("palette", false)
                if it.consumer == "lxqt" || (it.consumer == QT6CT_ROLE && plan.roles.contains(QT6CT_ROLE)) =>
            {
                if let Some(steps) = palette_steps(look, plan, t, prepared, it, &icon_theme, crate::themes::own_qt_desktop_installed(m.ctx.have)) {
                    out.push(SubPlan { what: kind_label(&it.kind), steps, must_ok: false, fail_note: None });
                }
                continue;
            }
            ("qt-style", false) => {
                let qt6ct = it.consumer == QT6CT_ROLE && plan.roles.contains(QT6CT_ROLE);
                let env = crate::themes::KvantumEnv {
                    de_id: de,
                    home: &t.home,
                    xdg_config_home: &t.config,
                    platform_theme: if qt6ct { crate::themes::QT_STYLE_TOOL } else { "" },
                    icon_theme: icon_theme.clone(),
                    qt_desktop_installed: crate::themes::own_qt_desktop_installed(m.ctx.have),
                };
                let p = crate::themes::apply_kvantum_plan(&name, &env);
                if !p.steps.is_empty() {
                    // `kvantummanager` itself writes the theme choice — its file is under the rollback.
                    let guard = ApplyStep::Guard { paths: vec![format!("{}/Kvantum/kvantum.kvconfig", t.config)] };
                    let steps = std::iter::once(guard).chain(p.steps).map(Step::from).collect();
                    out.push(SubPlan { what: kind_label(&it.kind), steps, must_ok: false, fail_note: None });
                }
                continue;
            }
            ("panel-layout", true) => {
                let steps = panel_steps(look, plan, t, m, it, restore);
                if !steps.is_empty() {
                    out.push(SubPlan { what: kind_label(&it.kind), steps, must_ok: false, fail_note: None });
                }
                continue;
            }
            ("behaviour", true) => {
                let steps: Vec<Step> = behaviour_commands(look, &it.consumer)
                    .into_iter()
                    .map(|argv| Step::from(ApplyStep::BestEffort(cmd(argv))))
                    .collect();
                if !steps.is_empty() {
                    out.push(SubPlan { what: kind_label(&it.kind), steps, must_ok: false, fail_note: None });
                }
                if compiz_replaces(&it.consumer) && plan.roles.contains(COMPIZ_ROLE) && (m.owner_of)(COMPIZ_ROLE).is_some() {
                    match compiz_blocker(ctx) {
                        Some(why) => {
                            notes.push(trf("Compiz was not switched on: %1. %2 stays.", &[&why, &wm_name(&it.consumer)]))
                        }
                        None => {
                            let (sp, back) = compiz_subplan(look, plan, t, &it.consumer);
                            restore.push(back);
                            out.push(sp);
                        }
                    }
                }
                continue;
            }
            _ => {}
        }
        let press = match it.kind.as_str() {
            "gtk-theme" => Some((ApplyAction::Gtk, "gtk")),
            "icons" => Some((ApplyAction::Icons, "icons")),
            "cursors" => Some((ApplyAction::Cursor, "cursor")),
            _ => None,
        };
        if let Some((action, role)) = press {
            let mut f = facts.clone();
            f.gtk_dark = crate::themes::gtk_theme_is_dark(&name, None);
            if let PressPlan::Steps { mut steps, .. } = press_plan_with(&action, &name, &f) {
                // In Plasma the press writes KDE files itself (`kwriteconfig6` →
                // `kdeglobals`, `plasma-apply-cursortheme` → `kcminputrc`): their
                // "what it was" goes under the look's rollback, as in the Plasma path.
                if t.plasma() {
                    let guard = ApplyStep::Guard { paths: vec![format!("{}/kdeglobals", t.config), format!("{}/kcminputrc", t.config)] };
                    steps.insert(0, Step::from(guard));
                }
                if let Some(read) = crate::themes::native_role_read_cmd(de, role) {
                    let old = read_value(ctx, de, &read.argv);
                    if !old.is_empty() && old != name {
                        let mut f_old = f.clone();
                        f_old.gtk_dark = crate::themes::gtk_theme_is_dark(&old, None);
                        if let PressPlan::Steps { steps: back, .. } = press_plan_with(&action, &old, &f_old) {
                            restore.extend(plan_commands(&back));
                        }
                    }
                }
                out.push(SubPlan { what: kind_label(&it.kind), steps, must_ok: true, fail_note: None });
            }
            continue;
        }
        let value = match it.kind.as_str() {
            "fonts" => gtk_font(look),
            "window-decoration" | "shell-theme" => Some(name.clone()),
            "wallpaper" => {
                notes.push(trf(
                    "The wallpaper “%1” was put into your wallpaper folder; choose it in your desktop settings.",
                    &[&name],
                ));
                None
            }
            _ => None,
        };
        let Some(value) = value else { continue };
        let Some((read, write)) = native_key(de, &it.kind, &it.consumer) else { continue };
        let old = read_value(ctx, de, &read);
        if !old.is_empty() && old != value {
            let mut back = write.clone();
            back.push(old);
            restore.push(back);
        }
        let mut argv = write;
        argv.push(value);
        out.push(SubPlan {
            what: kind_label(&it.kind),
            steps: vec![Step::from(ApplyStep::BestEffort(cmd(argv)))],
            must_ok: false,
            fail_note: None,
        });
    }
    out
}

/// The Qt palette from the look's KDE scheme outside Plasma: the same layer as the
/// «Color schemes» section's (`shell_palette::plan_scheme_layer`: `kdeglobals` for
/// KDE programs, the qt6ct palette or `lxqt.conf`, the session variable), but
/// as records of this run — their "what it was" is captured by the look's shared rollback.
fn palette_steps(look: &Look, plan: &LookPlan, t: &Target, prepared: &Path, it: &PlannedItem, icon_theme: &str, qt_desktop_installed: bool) -> Option<Vec<Step>> {
    use crate::shell_palette as sp;
    // The payload is a scheme file or a schemes directory (SteamOS: Vapor and VGUI); from
    // a directory — the scheme the look enables in Plasma (the description's
    // `colorscheme`, with the variant).
    let src = prepared.join(&it.path);
    let file = if src.is_dir() {
        let settings = look.settings.as_ref()?;
        let name = settings
            .get("apply")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .find_map(|e| {
                let raw = e.get("colorscheme")?;
                step_value(look, settings, "colorscheme", raw, &plan.values)?.as_str().map(str::to_string)
            })?;
        src.join(format!("{name}.colors"))
    } else {
        src
    };
    let bytes = std::fs::read(&file).ok()?;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    let id = file.file_stem()?.to_string_lossy().into_owned();
    let home = sp::PaletteHome {
        home: t.home.clone(),
        xdg_config_home: t.config.clone(),
        xdg_data_home: t.data.clone(),
        xdg_cache_home: t.cache.clone(),
        ..sp::PaletteHome::default()
    };
    let ctx = sp::SchemeCtx { de_id: t.de.clone(), home, data_dirs: Vec::new(), session_platform_theme: String::new(), qt_desktop_installed };
    let lxqt = it.consumer == "lxqt";
    let facts = sp::LayerFacts {
        qt_tools: if lxqt { Vec::new() } else { vec![crate::themes::QT_STYLE_TOOL.to_string()] },
        icon_theme: icon_theme.to_string(),
    };
    let read = |p: &Path| std::fs::read_to_string(p).ok();
    let layer = sp::plan_scheme_layer(&ctx, lxqt, &id, &text, &sp::scheme_hash(&bytes), &facts, &read, &|_| false);
    let mut steps = Vec::new();
    for w in layer.writes {
        let path = w.path.to_string_lossy().into_owned();
        if w.kind == "env" {
            // The variables file — as a desktop step: after it Rexon mirrors
            // the variables into sessions that do not read `environment.d`
            // (`.xprofile` of MATE/XFCE/Cinnamon on X11, Hyprland, labwc).
            let dir = w.path.parent().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default();
            steps.push(Step::from(ApplyStep::ForceOk));
            steps.push(Step::from(ApplyStep::WriteIfOk { dir, path, content: w.text }));
        } else {
            steps.push(Step::from(ExtStep::WriteFile { path, content: w.text }));
        }
    }
    Some(steps)
}

/// Program launchers by the launcher role — the first one present on the system.
/// The desktop's native programs first.
fn launcher_candidates(role: &str, de: &str) -> Vec<&'static str> {
    let native: &[&str] = match (role, de) {
        ("terminal", "mate") => &["mate-terminal"],
        ("terminal", "xfce") => &["xfce4-terminal"],
        ("terminal", "cinnamon" | "gnome") => &["org.gnome.Terminal", "org.gnome.Console"],
        // Budgie — the Ubuntu Budgie set (`system::DE_DATABASE`).
        ("terminal", "budgie") => &["xfce4-terminal"],
        ("file-manager", "mate") => &["caja-browser", "caja"],
        ("file-manager", "xfce") => &["thunar"],
        ("file-manager", "cinnamon" | "budgie") => &["nemo"],
        ("system-monitor", "mate" | "budgie") => &["mate-system-monitor"],
        ("system-monitor", "xfce") => &["xfce4-taskmanager"],
        ("settings", "mate") => &["matecc"],
        ("settings", "xfce") => &["xfce-settings-manager"],
        ("settings", "cinnamon") => &["cinnamon-settings"],
        _ => &[],
    };
    let common: &[&str] = match role {
        "terminal" => &["org.gnome.Terminal", "org.gnome.Console", "org.kde.konsole", "kitty", "Alacritty", "xterm"],
        "browser" => &["firefox", "chromium", "google-chrome", "brave-browser", "librewolf", "vivaldi-stable", "org.mozilla.firefox"],
        "file-manager" => &["org.gnome.Nautilus", "nemo", "thunar", "caja-browser", "pcmanfm-qt", "org.kde.dolphin"],
        "system-monitor" => &["gnome-system-monitor", "org.gnome.SystemMonitor", "mate-system-monitor", "xfce4-taskmanager"],
        "settings" => &[],
        _ => &[],
    };
    let mut out: Vec<&'static str> = native.to_vec();
    for c in common {
        if !out.contains(c) {
            out.push(c);
        }
    }
    out
}

/// The MATE panel snapshot file before applying the look (the Rexon directory: it does not get
/// into the person's profile; restoring deletes it).
fn mate_panel_dump(t: &Target, id: &str) -> String {
    t.rexon_dir().join("looks").join(id).join("mate-panel.dconf").to_string_lossy().into_owned()
}

/// The panel layout from the look description: mate-panel — with its own layout
/// file, Cinnamon — with the `org.cinnamon` keys (their "what it was" is captured by
/// the shared value rollback).
fn panel_steps(look: &Look, plan: &LookPlan, t: &Target, m: &Machine, it: &PlannedItem, restore: &mut Vec<Vec<String>>) -> Vec<Step> {
    let panels = model_panels(look);
    let roles_of = |p: &Value| -> Vec<(String, String, Vec<String>)> {
        p.get("items")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|i| {
                let s = |k: &str| i.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
                let apps = i.get("apps").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect();
                (s("role"), if s("align").is_empty() { "start".into() } else { s("align") }, apps)
            })
            .collect()
    };
    // The menu item of the description: which menu it prefers and its caption.
    let menu_item = panels
        .iter()
        .flat_map(|p| p.get("items").and_then(Value::as_array).into_iter().flatten())
        .find(|i| i.get("role").and_then(Value::as_str) == Some("menu"));
    let has_role = |role: &str| {
        panels.iter().flat_map(|p| p.get("items").and_then(Value::as_array).into_iter().flatten()).any(|i| i.get("role").and_then(Value::as_str) == Some(role))
    };
    let launchers = |apps: &[String]| -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for app in apps {
            if let Some(p) = launcher_candidates(app, &t.de).into_iter().find_map(|c| (m.desktop_path)(&format!("{c}.desktop"))) {
                if !out.contains(&p) {
                    out.push(p);
                }
            }
        }
        out
    };
    match it.consumer.as_str() {
        "mate-panel" => {
            use crate::mate_panel::{OwnObject, OwnPanel};
            // The MATE Menu applet only when its package is (or is being) installed:
            // a missing applet would greet the person with "failed to load".
            let mate_menu = menu_item.and_then(|i| i.get("prefer")).and_then(Value::as_str) == Some(MATE_MENU_ROLE)
                && plan.roles.contains(MATE_MENU_ROLE);
            let mut own: Vec<OwnPanel> = Vec::new();
            for p in &panels {
                let top = p.get("edge").and_then(Value::as_str) == Some("top");
                let mut panel = OwnPanel {
                    id: if top { "top".into() } else { "dock".into() },
                    top,
                    size: p.get("size").and_then(Value::as_u64).unwrap_or(30) as u32,
                    expand: p.get("expand").and_then(Value::as_bool).unwrap_or(true),
                    centered: p.get("center").and_then(Value::as_bool).unwrap_or(false),
                    start: Vec::new(),
                    end: Vec::new(),
                };
                for (role, align, apps) in roles_of(p) {
                    let objects = match role.as_str() {
                        "menu" if mate_menu => vec![OwnObject::MateMenu],
                        "menu" => vec![OwnObject::MainMenu],
                        "show-desktop" => vec![OwnObject::ShowDesktop],
                        "windows" => vec![OwnObject::WindowList],
                        "clock" => vec![OwnObject::Clock],
                        "tray" => vec![OwnObject::NotificationArea],
                        "launchers" => launchers(&apps).into_iter().map(OwnObject::Launcher).collect(),
                        _ => Vec::new(),
                    };
                    // mate-panel has no center: the clock goes to the right edge.
                    if align == "start" {
                        panel.start.extend(objects);
                    } else {
                        // The "from the end" list goes right to left.
                        for o in objects {
                            panel.end.insert(0, o);
                        }
                    }
                }
                if !panel.start.is_empty() || !panel.end.is_empty() {
                    own.push(panel);
                }
            }
            if own.is_empty() {
                return Vec::new();
            }
            let layout = format!("{}/mate-panel/layouts/rexon-{}.layout", t.data.trim_end_matches('/'), plan.id);
            let dump = mate_panel_dump(t, &plan.id);
            restore.push(crate::mate_panel::own_restore_argv(&dump));
            let mut steps = vec![
                Step::from(ExtStep::WriteFile { path: layout.clone(), content: crate::mate_panel::own_layout_text(&own) }),
                Step::from(ApplyStep::BestEffort(cmd(crate::mate_panel::own_apply_argv(&layout, &dump)))),
            ];
            // The caption of the menu button ("Start" in the person's language); the
            // previous value comes back with the shared value rollback.
            let label = menu_item.and_then(|i| i.get("label")).map(|l| localized(l, &t.lang)).unwrap_or_default();
            if mate_menu && !label.is_empty() {
                steps.push(Step::from(ApplyStep::BestEffort(cmd(crate::mate_panel::mate_menu_label_argv(&label)))));
            }
            steps
        }
        "cinnamon-panel" => {
            let mut enabled: Vec<String> = Vec::new();
            let mut heights: Vec<String> = Vec::new();
            let mut panel_list: Vec<String> = Vec::new();
            let mut hide: Vec<String> = Vec::new();
            let mut next = 0usize;
            for (n, p) in panels.iter().enumerate() {
                let n = n + 1;
                let edge = p.get("edge").and_then(Value::as_str).unwrap_or("bottom");
                panel_list.push(format!("'{n}:0:{edge}'"));
                heights.push(format!("'{n}:{}'", p.get("size").and_then(Value::as_u64).unwrap_or(30)));
                let dodge = p.get("hide").and_then(Value::as_str) == Some("dodge-windows");
                hide.push(format!("'{n}:{}'", if dodge { "intel" } else { "false" }));
                let mut order: BTreeMap<&str, usize> = BTreeMap::new();
                for (role, align, _) in roles_of(p) {
                    let zone = match align.as_str() {
                        "center" => "center",
                        "end" => "right",
                        _ => "left",
                    };
                    // The Garuda dock is launchers and windows together: in Cinnamon it is
                    // the window list with pinned programs, in the middle.
                    let (zone, applets): (&str, &[&str]) = match role.as_str() {
                        "menu" => (zone, &["menu@cinnamon.org"]),
                        "clock" => (zone, &["calendar@cinnamon.org"]),
                        "tray" => (zone, &["systray@cinnamon.org", "xapp-status@cinnamon.org", "notifications@cinnamon.org", "network@cinnamon.org", "sound@cinnamon.org"]),
                        "user" => (zone, &["user@cinnamon.org"]),
                        "show-desktop" => (zone, &["show-desktop@cinnamon.org"]),
                        // A look with a dock already lists windows in it (above); a
                        // classic taskbar gets the window list of its own.
                        "windows" if !has_role("launchers") => (zone, &["grouped-window-list@cinnamon.org"]),
                        "launchers" => ("center", &["grouped-window-list@cinnamon.org"]),
                        _ => (zone, &[]),
                    };
                    for a in applets {
                        let pos = order.entry(zone).or_insert(0);
                        enabled.push(format!("'panel{n}:{zone}:{pos}:{a}:{next}'"));
                        *pos += 1;
                        next += 1;
                    }
                }
            }
            if enabled.is_empty() {
                return Vec::new();
            }
            let list = |v: &[String]| format!("[{}]", v.join(", "));
            [
                ("panels-enabled", list(&panel_list)),
                ("panels-height", list(&heights)),
                ("panels-autohide", list(&hide)),
                ("enabled-applets", list(&enabled)),
            ]
            .into_iter()
            .map(|(k, v)| Step::from(ApplyStep::BestEffort(cmd(sv(&["gsettings", "set", "org.cinnamon", k, &v])))))
            .collect()
        }
        _ => Vec::new(),
    }
}

// ── Compiz instead of Marco and xfwm4 (behavior "like in KWin") ─────────────
//
// The owner's decision of 31.10.2026: for Garuda's window effects (wobbly windows,
// «Magic Lamp», fading, the «Cover» switcher) on MATE and XFCE
// (X11) the desktop's window manager (Marco, xfwm4) is replaced with Compiz 0.9. Where
// things come from (the Compiz 0.9.14.2 sources, git.launchpad.net/compiz;
// xfce4-session and xfwm4 4.20, gitlab.xfce.org):
//
// * settings — the profile of libcompizconfig's `ini` backend: the file
//   `~/.config/compiz-1/compizconfig/config` (`[general] backend`, `profile`)
//   and `<profile>.ini` with plugin groups and `s0_<parameter>` keys;
//   lists — separated by `;` (`libcompizconfig/src/ini.c`);
// * frames are drawn by `gtk-window-decorator` (the `command` parameter of the `decor` plugin);
//   it takes the theme and button order on MATE from `org.mate.Marco.general` — the
//   same keys the look already sets for Marco — and on any other desktop, XFCE
//   included, from `org.gnome.desktop.wm.preferences` `theme`/`button-layout`
//   (`gtk/window-decorator/gwd-settings-storage.c`, chosen by
//   `XDG_CURRENT_DESKTOP`): on XFCE Rexon writes the look's frame theme there (in
//   the theme directory next to `xfwm4/` there is also `metacity-1/`) and buttons on the left;
// * «Magic Lamp» — `animation` `minimize_effects = animation:Magic Lamp`,
//   «Cover» — `shift` `mode = 0` (Cover), wobbly — `wobbly`, fading —
//   `fade`; Compiz has no desktop switch hint — the closest is the `wall`
//   with its desktop map on switching.
//
// How the desktop starts the window manager at login:
// * MATE — the launcher from `org.mate.session.required-components windowmanager`;
// * XFCE — xfce4-session brings up the window manager as a regular session
//   client: from the saved session (`~/.cache/sessions/xfce4-session-*`),
//   and if there is none — from `/sessions/Failsafe/Client0_Command` of the
//   `xfce4-session` channel (a string array, `xfwm4` by default). An xfwm4 displaced by
//   `--replace` sets "do not restart" for itself and drops out of the session;
//   the session does not watch for a window manager being present. So `Client0_Command`
//   is no good (the saved session overrides it), and Compiz is started by
//   autostart `~/.config/autostart` (xfce4-session reads it at every
//   login): the session brings up xfwm4, autostart replaces it with Compiz, and if
//   Compiz crashed — `exec xfwm4 --replace`. Compiz is NOT a session client then
//   (`env -u SESSION_MANAGER`): otherwise the session would restart a crashed Compiz
//   on top of the returned xfwm4 and would remember it in the saved session after
//   «Restore as it was». The xfwm4 that Rexon brings back is a session
//   client again.
//
// Safety: (1) before the replacement — a 3D probe: a virtual machine or software
// rendering (llvmpipe) — Compiz is not installed, the reason is named; (2) after
// `compiz --replace` a guard waits ~12 s: if Compiz crashed or there are no frames (no
// `gtk-window-decorator`) — the previous manager comes back (`--replace`), the reason
// is named; (3) «Restore as it was» restores the session key (autostart) and
// files with the look's shared rollback and starts the previous manager; (4) the
// MATE session key and the XFCE autostart are written ONLY after Compiz has passed
// the guard, and they themselves roll back to the previous manager if Compiz did not start.

/// The name of our Compiz profile (`<profile>.ini`).
pub const COMPIZ_PROFILE: &str = "Rexon";

/// The launcher with which the MATE session starts Compiz (`org.mate.session
/// .required-components windowmanager` = the launcher name without `.desktop`; it is also looked up
/// in `~/.local/share/applications` — `gsm_util_get_app_dirs`).
pub const COMPIZ_SESSION_DESKTOP: &str = "rexon-compiz";

/// The compizconfig `config` file: the `ini` backend and our profile.
pub fn compiz_config_text() -> String {
    format!("[general]\nbackend = ini\nprofile = {COMPIZ_PROFILE}\nplugin_list_autosort = true\n")
}

/// The Compiz profile with Garuda's effects. `core` first, the rest is sorted by
/// compizconfig itself (`plugin_list_autosort`).
pub fn compiz_profile_text() -> String {
    [
        "[core]",
        "s0_active_plugins = core;composite;opengl;decor;resize;place;move;mousepoll;compiztoolbox;regex;text;session;workarounds;wobbly;animation;fade;shift;wall;",
        "",
        "[decor]",
        "s0_command = gtk-window-decorator",
        "",
        "[animation]",
        "s0_minimize_effects = animation:Magic Lamp;",
        "s0_unminimize_effects = animation:Magic Lamp;",
        "",
        "[shift]",
        "s0_mode = 0",
        "s0_next_key = <Alt>Tab",
        "s0_prev_key = <Shift><Alt>Tab",
        "",
        "[wall]",
        "s0_show_switcher = true",
        "",
    ]
    .join("\n")
}

/// How Compiz takes the place of `wm`: on XFCE — not as a session client (see
/// the section above).
fn compiz_run(wm: &str) -> &'static str {
    match wm {
        "xfwm4" => "env -u SESSION_MANAGER compiz --replace ccp",
        _ => "compiz --replace ccp",
    }
}

/// The launcher with which the desktop starts Compiz at login: on MATE — the session's
/// window manager, on XFCE — autostart. If Compiz did not start or crashed — the previous
/// manager `wm` (login will not be left without a window manager).
pub fn compiz_session_desktop_text(wm: &str) -> String {
    match wm {
        "xfwm4" => format!(
            "[Desktop Entry]\nType=Application\nName=Compiz (Rexon)\nExec=sh -c \"{} || exec xfwm4 --replace\"\nOnlyShowIn=XFCE;\nTerminal=false\n",
            compiz_run(wm)
        ),
        _ => "[Desktop Entry]\nType=Application\nName=Compiz (Rexon)\nExec=sh -c \"compiz --replace ccp || exec marco --replace\"\nNoDisplay=true\nX-MATE-WMName=Compiz\nX-MATE-Autostart-Phase=WindowManager\nX-MATE-Provides=windowmanager\nX-MATE-Autostart-Notify=false\n".to_string(),
    }
}

/// Start Compiz instead of `wm` and guard it for ~12 s: Compiz is alive and there are frames
/// (`gtk-window-decorator`) — code 0; otherwise — back to `wm`, code 1.
/// Detached (`setsid -f`): the window manager outlives Rexon, and its output does not
/// hold the launch pipe. Frames are looked up by process name (`-x`; the kernel keeps
/// it truncated to 15 characters — `gtk-window-deco`): a search by command
/// line (`-f`) would find this very `sh -c`, whose text has the same name.
pub fn compiz_start_argv(wm: &str) -> Vec<String> {
    let script = format!(
        "u=$(id -u); setsid -f {} >/dev/null 2>&1 </dev/null; i=0; \
         while [ $i -lt 12 ]; do sleep 1; i=$((i+1)); pgrep -u \"$u\" -x compiz >/dev/null || break; done; \
         if pgrep -u \"$u\" -x compiz >/dev/null && pgrep -u \"$u\" -x gtk-window-deco >/dev/null; then exit 0; fi; \
         setsid -f {wm} --replace >/dev/null 2>&1 </dev/null; exit 1",
        compiz_run(wm)
    );
    sv(&["sh", "-c", &script])
}

/// Bring back the previous window manager `wm` (the «Restore as it was» command).
pub fn wm_back_argv(wm: &str) -> Vec<String> {
    sv(&["sh", "-c", &format!("setsid -f {wm} --replace >/dev/null 2>&1 </dev/null")])
}

/// Why Compiz must not be enabled here; `None` — it may. The probe is `glxinfo -B`
/// (no direct rendering or it is software — not allowed); without `glxinfo`
/// a virtual machine (`systemd-detect-virt --vm`) counts as a machine without 3D.
fn compiz_blocker(ctx: &Ctx) -> Option<String> {
    let glx = (ctx.run)(&sv(&["glxinfo", "-B"]), Some(10));
    if glx.returncode == Some(0) {
        let out = crate::utils::decode_text(&glx.stdout).to_lowercase();
        if out.contains("direct rendering: no") {
            return Some(tr("the graphics here work without 3D acceleration, and Compiz needs it"));
        }
        if ["llvmpipe", "softpipe", "swrast"].iter().any(|w| out.contains(w)) {
            return Some(tr("3D is drawn here by the processor (llvmpipe), and Compiz would be slow and unstable"));
        }
        return None;
    }
    let virt = (ctx.run)(&sv(&["systemd-detect-virt", "--vm"]), Some(5));
    if virt.returncode == Some(0) {
        let name = crate::utils::decode_text(&virt.stdout).trim().to_string();
        return Some(trf("this is a virtual machine (%1), and Rexon could not check its 3D support", &[&name]));
    }
    None
}

/// The "Compiz instead of `wm`" subplan and the command restoring `wm`.
fn compiz_subplan(look: &Look, plan: &LookPlan, t: &Target, wm: &str) -> (SubPlan, Vec<String>) {
    let config = t.config.trim_end_matches('/');
    let dir = format!("{config}/compiz-1/compizconfig");
    let mut steps = vec![
        Step::from(ExtStep::WriteFile { path: format!("{dir}/config"), content: compiz_config_text() }),
        Step::from(ExtStep::WriteFile { path: format!("{dir}/{COMPIZ_PROFILE}.ini"), content: compiz_profile_text() }),
    ];
    let start = Step::from(ApplyStep::Primary(Cmd { argv: compiz_start_argv(wm), timeout: Some(40) }));
    // The session — only after the guard: otherwise login would start something that already
    // failed to come up here.
    if wm == "xfwm4" {
        // Frames: outside MATE gtk-window-decorator reads the GNOME keys.
        let gs = |key: &str, value: &str| {
            Step::from(ApplyStep::BestEffort(cmd(sv(&["gsettings", "set", "org.gnome.desktop.wm.preferences", key, value]))))
        };
        let frames = plan
            .items
            .iter()
            .find(|i| i.kind == "window-decoration")
            .and_then(|i| Path::new(&i.path).file_name())
            .map(|n| n.to_string_lossy().into_owned());
        if let Some(theme) = frames {
            steps.push(gs("theme", &theme));
        }
        let buttons = model_behaviour(look).and_then(|b| b.get("window_buttons")).and_then(Value::as_str).unwrap_or_default();
        if !buttons.is_empty() {
            steps.push(gs("button-layout", buttons));
        }
        // Autostart is placed as a copy of the ready launcher after the guard; whatever
        // the copy creates is captured by the rollback (`Guard`).
        let staged = format!("{config}/compiz-1/{COMPIZ_SESSION_DESKTOP}.desktop");
        let autostart_dir = format!("{config}/autostart");
        let autostart = format!("{autostart_dir}/{COMPIZ_SESSION_DESKTOP}.desktop");
        steps.push(Step::from(ExtStep::WriteFile { path: staged.clone(), content: compiz_session_desktop_text(wm) }));
        steps.push(Step::from(ExtStep::MakeDirs { path: autostart_dir }));
        steps.push(Step::from(ApplyStep::Guard { paths: vec![autostart.clone()] }));
        steps.push(start);
        steps.push(Step::from(ApplyStep::IfOk(cmd(sv(&["cp", &staged, &autostart])))));
    } else {
        let desktop = format!("{}/applications/{COMPIZ_SESSION_DESKTOP}.desktop", t.data.trim_end_matches('/'));
        steps.push(Step::from(ExtStep::WriteFile { path: desktop, content: compiz_session_desktop_text(wm) }));
        steps.push(start);
        steps.push(Step::from(ApplyStep::IfOk(cmd(sv(&[
            "gsettings",
            "set",
            "org.mate.session.required-components",
            "windowmanager",
            COMPIZ_SESSION_DESKTOP,
        ])))));
    }
    let sp = SubPlan {
        what: "Compiz".into(),
        steps,
        must_ok: false,
        fail_note: Some(trf(
            "Compiz did not start or drew windows without frames, so Rexon switched back to %1. The rest of the look is applied.",
            &[&wm_name(wm)],
        )),
    };
    (sp, wm_back_argv(wm))
}

/// Title buttons in GTK/Metacity notation (`close,maximize,minimize:`) —
/// as xfwm4 letters (`CMH|`: `|` — the title).
pub fn xfwm4_button_layout(gtk: &str) -> String {
    let side = |s: &str| -> String {
        s.split(',')
            .filter_map(|b| match b.trim() {
                "close" => Some('C'),
                "maximize" => Some('M'),
                "minimize" => Some('H'),
                "menu" => Some('O'),
                "stick" | "sticky" => Some('T'),
                "shade" => Some('S'),
                _ => None,
            })
            .collect()
    };
    let (left, right) = gtk.split_once(':').unwrap_or((gtk, ""));
    format!("{}|{}", side(left), side(right))
}

/// Behavior commands from the look description for the window manager `consumer`
/// (what it can do; the rest — [`LookPlan::limits`]).
fn behaviour_commands(look: &Look, consumer: &str) -> Vec<Vec<String>> {
    let Some(b) = model_behaviour(look) else { return Vec::new() };
    let buttons = b.get("window_buttons").and_then(Value::as_str).unwrap_or_default();
    let compositing = b.get("compositing").and_then(Value::as_bool) == Some(true);
    let borderless = b.get("borderless_maximized").and_then(Value::as_bool) == Some(true);
    let effects: Vec<&str> = b.get("effects").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).collect();
    let gs = |schema: &str, key: &str, value: &str| sv(&["gsettings", "set", schema, key, value]);
    let xf = |prop: &str, ty: &str, value: &str| sv(&["xfconf-query", "-c", "xfwm4", "-p", prop, "-n", "-t", ty, "-s", value]);
    let mut out = Vec::new();
    match consumer {
        "marco" => {
            if compositing {
                out.push(gs("org.mate.Marco.general", "compositing-manager", "true"));
            }
            if !buttons.is_empty() {
                out.push(gs("org.mate.Marco.general", "button-layout", buttons));
            }
        }
        "xfwm4" => {
            if compositing {
                out.push(xf("/general/use_compositing", "bool", "true"));
            }
            if !buttons.is_empty() {
                out.push(xf("/general/button_layout", "string", &xfwm4_button_layout(buttons)));
            }
            if borderless {
                out.push(xf("/general/borderless_maximize", "bool", "true"));
            }
        }
        "muffin" => {
            if !buttons.is_empty() {
                out.push(gs("org.cinnamon.desktop.wm.preferences", "button-layout", buttons));
            }
            if effects.contains(&"fade") {
                out.push(gs("org.cinnamon", "desktop-effects", "true"));
                out.push(gs("org.cinnamon", "desktop-effects-map", "fade"));
            }
            if effects.contains(&"desktop-change-osd") {
                out.push(gs("org.cinnamon", "workspace-osd-visible", "true"));
            }
        }
        "mutter" => {
            if !buttons.is_empty() {
                out.push(gs("org.gnome.desktop.wm.preferences", "button-layout", buttons));
            }
        }
        _ => {}
    }
    out
}

// ============================================================================
// «Restore as it was»
// ============================================================================

/// Whether the look `id` is applied (there is a log record or a "what it was" snapshot).
pub fn is_applied(id: &str, rexon_dir: &Path) -> bool {
    let view = journal::view_at(rexon_dir);
    view.items.contains_key(&journal_key(id)) || view.applied.contains_key(&section_of(id))
}

/// The result of «Restore as it was».
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Reverted {
    /// There was something to restore.
    pub reverted: bool,
    /// Files that could not be restored (copies remained in the snapshot).
    pub not_restored: Vec<String>,
    /// Things placed by the look that were left (they became foreign).
    pub kept: Vec<String>,
    /// Packages installed by the look: Rexon does not remove them, the person decides
    /// themselves (`pacman -Rs …`).
    pub packages: Vec<String>,
    /// Plasma: the shell should be restarted for the panels to come back right away.
    pub restart_shell: bool,
    pub message: String,
}

/// «Restore as it was» for the look `id`: remove what was placed per the log (only
/// our own), restore files from the snapshot, restore desktop values with commands.
pub fn revert(id: &str, t: &Target, ctx: &Ctx) -> Result<Reverted, String> {
    let rexon = t.rexon_dir();
    let key = journal_key(id);
    let view = journal::view_at(&rexon);
    let entry = view.items.get(&key).cloned();
    let mut out = Reverted::default();
    let quiet = Ctx {
        install: false,
        steamos: ctx.steamos,
        privileged: None,
        run: ctx.run,
        have: ctx.have,
        spawn: ctx.spawn,
        wayland: ctx.wayland,
        sync_env: ctx.sync_env,
    };
    if let Some(entry) = &entry {
        out.reverted = true;
        let removal = journal::removal_plan(entry, &journal::probe_live);
        let steps: Vec<Step> = removal.steps.iter().cloned().map(Step::from).collect();
        crate::install_plan::exec(&steps, &mut Vec::new(), &quiet).map_err(|f| crate::install_plan::failure_text(&f))?;
        journal::prune(&removal);
        out.kept = removal.kept.iter().map(|(p, _)| p.clone()).collect();
    }
    // Desktop values — with the same runner as the run's (in checks —
    // a fake one), not with the live machine.
    match journal::revert_applied_with(&rexon, &section_of(id), &|_| false, ctx.run) {
        Ok(Some(failed)) => {
            out.reverted = true;
            out.not_restored = failed;
        }
        Ok(None) => {}
        Err(e) => return Err(e.to_string()),
    }
    if let Some(card) = entry.as_ref().and_then(|e| e.card.as_ref()) {
        for argv in card.get("restore").and_then(Value::as_array).into_iter().flatten() {
            let argv: Vec<String> = argv.as_array().into_iter().flatten().filter_map(|a| a.as_str().map(str::to_string)).collect();
            if !argv.is_empty() {
                let _ = (ctx.run)(&argv, Some(10));
            }
        }
        out.packages = card
            .get("packages")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect();
    }
    if entry.is_some() {
        journal::forget_at(&rexon, &key).map_err(|e| e.to_string())?;
    }
    out.restart_shell = out.reverted && t.plasma();
    let name = entry.as_ref().map(|e| e.name.clone()).unwrap_or_else(|| id.to_string());
    let mut lines = vec![if out.reverted {
        trf("“%1”: everything is back as it was.", &[&name])
    } else {
        trf("“%1” was not applied — there is nothing to put back.", &[&name])
    }];
    if !out.not_restored.is_empty() {
        lines.push(trf("Could not put back: %1", &[&out.not_restored.join(", ")]));
    }
    if !out.packages.is_empty() {
        lines.push(trf(
            "The programs installed for it stay; remove them yourself if you don't need them: %1",
            &[&out.packages.join(" ")],
        ));
    }
    out.message = lines.join("\n");
    Ok(out)
}

// ============================================================================
// The live machine
// ============================================================================

thread_local! {
    static PACMAN_DB: RefCell<Option<BTreeMap<String, String>>> = const { RefCell::new(None) };
}

/// Name owners from the local pacman database (`/var/lib/pacman/local/*/desc`):
/// the package name and its Provides → the package. File reading only.
pub fn pacman_owners(db: &Path) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    let Ok(rd) = std::fs::read_dir(db) else { return map };
    for e in rd.flatten() {
        let Ok(text) = std::fs::read_to_string(e.path().join("desc")) else { continue };
        let mut section = "";
        let mut name = String::new();
        let mut provides = Vec::new();
        for line in text.lines() {
            if line.starts_with('%') && line.ends_with('%') {
                section = line;
                continue;
            }
            if line.is_empty() {
                continue;
            }
            match section {
                "%NAME%" => name = line.to_string(),
                "%PROVIDES%" => provides.push(line.split(['=', '<', '>']).next().unwrap_or(line).to_string()),
                _ => {}
            }
        }
        if name.is_empty() {
            continue;
        }
        for p in provides {
            map.entry(p).or_insert_with(|| name.clone());
        }
        map.insert(name.clone(), name);
    }
    map
}

/// Which package provides a name on the live machine. Arch — by the local
/// pacman database (reread after [`forget_owner_cache_live`]); other
/// families — with a package manager query.
pub fn owner_of_live(name: &str) -> Option<String> {
    let family = crate::system::DistroDetector::detect_live().family_live();
    if family == PkgFamily::Arch {
        return PACMAN_DB.with(|c| {
            let mut c = c.borrow_mut();
            if c.is_none() {
                *c = Some(pacman_owners(Path::new("/var/lib/pacman/local")));
            }
            c.as_ref().and_then(|m| m.get(name).cloned())
        });
    }
    let argv = match family {
        PkgFamily::Fedora | PkgFamily::Suse => sv(&["rpm", "-q", "--whatprovides", name, "--qf", "%{NAME}\n"]),
        PkgFamily::Debian => sv(&["dpkg-query", "-W", "-f", "${Package}\n", name]),
        _ => return None,
    };
    let r = crate::install_plan::run_live(&argv, Some(10));
    (r.returncode == Some(0))
        .then(|| crate::utils::decode_text(&r.stdout).lines().next().unwrap_or("").trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Forget the read pacman database (after installation — the "after" snapshot).
pub fn forget_owner_cache_live() {
    PACMAN_DB.with(|c| *c.borrow_mut() = None);
}

/// Whether the package is in the enabled repositories (Arch — `pacman -Si`; others —
/// assumed to be there: installation will say if it is not).
pub fn in_repos_live(name: &str) -> bool {
    if crate::system::DistroDetector::detect_live().family_live() != PkgFamily::Arch {
        return true;
    }
    crate::install_plan::run_live(&sv(&["pacman", "-Si", name]), Some(30)).returncode == Some(0)
}

/// Whether a GNOME Shell extension is installed (a directory in the profile or in the system).
pub fn extension_installed_live(uuid: &str) -> bool {
    let t = Target::live("");
    [format!("{}/gnome-shell/extensions/{uuid}", t.data), format!("/usr/share/gnome-shell/extensions/{uuid}")]
        .iter()
        .any(|p| Path::new(p).is_dir())
}

/// The Plasma package `id` in `plasma/<sub>` of the profile or the system.
pub fn kpackage_installed_live(sub: &str, id: &str) -> bool {
    let t = Target::live("");
    [format!("{}/plasma/{sub}/{id}", t.data), format!("/usr/share/plasma/{sub}/{id}")]
        .iter()
        .any(|p| Path::new(p).is_dir())
}

/// A program launcher in the XDG directories.
pub fn desktop_installed_live(name: &str) -> bool {
    desktop_path_live(name).is_some()
}

/// The full path of a program launcher in the XDG directories (our own first).
pub fn desktop_path_live(name: &str) -> Option<String> {
    let t = Target::live("");
    let dirs = std::env::var("XDG_DATA_DIRS").ok().filter(|v| !v.is_empty()).unwrap_or("/usr/local/share:/usr/share".into());
    std::iter::once(t.data.clone())
        .chain(dirs.split(':').map(str::to_string))
        .map(|d| Path::new(&d).join("applications").join(name))
        .find(|p| p.is_file())
        .map(|p| p.to_string_lossy().into_owned())
}

/// Install packages from the official repositories (the password window; the AUR is not
/// built) — Rexon's shared path.
pub fn install_packages_live(names: &[String]) -> Result<(), String> {
    let r = crate::appearance_installer::live_install_repo_packages(names, crate::privilege::op::PkgSource::DistroLook);
    forget_owner_cache_live();
    r
}

/// Build and install AUR packages with a helper (`yay`/`paru`; the password — with a polkit
/// window, `aur_install::install_argv`).
pub fn install_aur_live(names: &[String]) -> Result<(), String> {
    let Some(helper) = Target::live("").aur_helper else {
        return Err(tr("there is no AUR helper (yay or paru)"));
    };
    let argv = crate::aur_install::install_argv(&helper, names, false);
    let r = crate::install_plan::run_live(&argv, Some(3600));
    forget_owner_cache_live();
    if r.returncode == Some(0) {
        Ok(())
    } else {
        let log = format!("{}\n{}", crate::utils::decode_text(&r.stdout), crate::utils::decode_text(&r.stderr));
        Err(crate::aur_install::failure_cause(&log).map(|c| crate::aur_install::failure_text(&c)).unwrap_or_else(|| {
            crate::aur_install::tail_lines(&log, 1).join(" ")
        }))
    }
}

/// Remove packages (the password window) — only those installed by this application.
pub fn remove_packages_live(names: &[String]) -> Result<(), String> {
    use crate::system_plan as sp;
    let family = crate::system::DistroDetector::detect_live().family_live();
    let full = remove_argv(family, names, sp::is_exec_file(sp::pkg_helper_path()) && Path::new(sp::PKG_POLICY_PATH).exists())?;
    // The window's privilege session (one password; root checks the vital ones and the current desktop);
    // without it — the old command. A helper refusal is not repeated with the old
    // command (Z5).
    let op = crate::privilege::op::PrivOp::PkgRemove { family, packages: names.to_vec() };
    if let Some(out) = crate::privilege::run::run_live(&op, &[], None, &mut |_| {}) {
        forget_owner_cache_live();
        return if out.returncode == Some(0) {
            Ok(())
        } else {
            // The helper sent the explanation as a progress line (the last one), other refusals — to stderr.
            let why = [out.stderr.as_str(), out.stdout.as_str()]
                .iter()
                .find_map(|t| t.lines().map(str::trim).rfind(|l| !l.is_empty()).map(str::to_string))
                .unwrap_or_default();
            Err(why)
        };
    }
    let r = crate::install_plan::run_live(&full, Some(600));
    forget_owner_cache_live();
    if r.returncode == Some(0) {
        Ok(())
    } else {
        Err(crate::utils::decode_text(&r.stderr).trim().to_string())
    }
}

/// The command removing the look's packages. The path is the Rexon package helper: it itself, as
/// root, does not allow removing something vital (`is_vital_package`). Previously there
/// was a raw `pkexec pacman -Rns` here bypassing this check — only
/// the user side held it. Without the helper (a run from the tree) the raw
/// path remains, but vital names are rejected on it too.
fn remove_argv(family: crate::system::PkgFamily, names: &[String], helper_ok: bool) -> Result<Vec<String>, String> {
    use crate::system_plan as sp;
    // The vital rule is one for all paths (`is_vital_package`: exact
    // names and families of kernels, drivers, firmware).
    if let Some(vital) = names.iter().find(|n| sp::is_vital_package(n)) {
        return Err(sp::vital_remove_error(vital));
    }
    if let Some(argv) = sp::privileged_pkg_cmd(family, "remove", names, helper_ok) {
        return Ok(argv);
    }
    let mut full = vec!["pkexec".to_string()];
    full.extend(sp::remove_cmd(family, names)?);
    Ok(full)
}

/// A file of a Pling product: `https://www.pling.com/p/<id>/#<file name>` →
/// `(id, file name)`. Pling hands out only short-lived signed links, so the
/// look names the product page and the file, and the link is asked from the
/// OCS API right before the download; the pinned `sha256` still decides.
pub fn pling_file_ref(url: &str) -> Option<(String, String)> {
    let rest = url.strip_prefix("https://www.pling.com/p/")?;
    let (id, file) = rest.split_once("/#")?;
    let ok_id = !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit());
    let ok_file = !file.is_empty() && !file.contains('/') && file != "..";
    (ok_id && ok_file).then(|| (id.to_string(), file.to_string()))
}

/// The current download link of the file `file` from an OCS product card
/// (`content/data/<id>`); only an `https://` one.
pub fn pling_download_link(detail_xml: &[u8], file: &str) -> Option<String> {
    let content = crate::ocs::parse_item_detail(detail_xml).ok()??;
    content.item.download_links.iter().find_map(|l| {
        let name = l.get("name").and_then(Value::as_str)?;
        let link = l.get("url").and_then(Value::as_str)?;
        (name == file && link.starts_with("https://")).then(|| link.to_string())
    })
}

/// Download a source into the cache with verification: an archive — by SHA-256 (no match —
/// a refusal), git — exactly the pinned commit (`git fetch` by hash, checking
/// `rev-parse HEAD`; git itself checks the contents with object hashes — on
/// GitLab the whole archive is not hashed, hence a clone, not an archive).
pub fn fetch_live(job: &FetchJob) -> Result<(), String> {
    use sha2::{Digest, Sha256};
    let tmp = job.dir.with_extension(format!("part-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    let run = |argv: Vec<String>, secs: u64| crate::install_plan::run_live(&argv, Some(secs));
    let tmp_s = tmp.to_string_lossy().into_owned();
    let result = (|| -> Result<(), String> {
        match &job.pin {
            Pin::Sha256(want) => {
                let mut net = crate::net::UreqNet::new();
                let url = match pling_file_ref(&job.url) {
                    Some((id, file)) => {
                        let detail = crate::net::NetIo::get(&mut net, &crate::ocs::item_url(&id), crate::ocs::USER_AGENT, 60)
                            .map_err(|e| e.human())?;
                        pling_download_link(&detail, &file).ok_or_else(|| trf("couldn't download %1", &[&job.url]))?
                    }
                    None => job.url.clone(),
                };
                let body = crate::net::NetIo::get(&mut net, &url, crate::ocs::USER_AGENT, 300)
                    .map_err(|e| e.human())?;
                let got: String = Sha256::digest(&body).iter().map(|b| format!("{b:02x}")).collect();
                if !got.eq_ignore_ascii_case(want) {
                    return Err(trf("the file from %1 does not match its pinned checksum — Rexon won't use it", &[&job.url]));
                }
                let archive = tmp.join(".rexon-archive");
                std::fs::write(&archive, &body).map_err(|e| e.to_string())?;
                let r = run(sv(&["tar", "-xf", &archive.to_string_lossy(), "-C", &tmp_s]), 300);
                let _ = std::fs::remove_file(&archive);
                if r.returncode != Some(0) {
                    return Err(trf("couldn't unpack %1", &[&job.url]));
                }
            }
            Pin::Git(rev) => {
                let git = |args: &[&str], secs| {
                    let mut argv = sv(&["git", "-C", &tmp_s]);
                    argv.extend(args.iter().map(|s| s.to_string()));
                    run(argv, secs).returncode == Some(0)
                };
                let ok = git(&["init", "-q"], 30)
                    && (git(&["fetch", "-q", "--depth", "1", &job.url, rev], 600)
                        || git(&["fetch", "-q", &job.url], 1200))
                    && git(&["-c", "advice.detachedHead=false", "checkout", "-q", rev], 120);
                if !ok {
                    return Err(trf("couldn't download %1", &[&job.url]));
                }
                let head = run(sv(&["git", "-C", &tmp_s, "rev-parse", "HEAD"]), 30);
                if crate::utils::decode_text(&head.stdout).trim() != rev {
                    return Err(trf("the download of %1 is not the pinned version — Rexon won't use it", &[&job.url]));
                }
                let _ = std::fs::remove_dir_all(tmp.join(".git"));
            }
        }
        std::fs::write(tmp.join(PIN_MARK), format!("{}\n", job.pin.text())).map_err(|e| e.to_string())?;
        let _ = std::fs::remove_dir_all(&job.dir);
        if let Some(parent) = job.dir.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::rename(&tmp, &job.dir).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&tmp);
    }
    result
}

/// Live machine probes for [`plan`].
pub fn probes_live() -> Probes<'static> {
    Probes { owner_of: &owner_of_live, in_repos: &in_repos_live, extension_installed: &extension_installed_live }
}

// ============================================================================
// The offer to the person: two plans and what will be restored before applying
// ============================================================================

/// The plans of one combination of the look's list-property values (the SteamOS style:
/// Vapor or VGUI2).
#[derive(Debug, Clone)]
pub struct OfferVariant {
    /// List-property values; empty — the look has none.
    pub values: PropValues,
    /// The plan without "at your wish" kinds ([`OPT_IN_KINDS`]).
    pub base: Prepared,
    /// The plan with "at your wish" kinds; `None` — the look has no such kinds.
    pub opt_in: Option<Prepared>,
}

/// What to show the person before «Apply»: plans for each combination of
/// variants (without and with "at your wish" kinds) and the looks that are currently
/// applied and will be restored first. All plans are built in advance — the question
/// window's switches change the text immediately, without new system probes.
#[derive(Debug, Clone)]
pub struct Offer {
    pub look: Look,
    /// First — the combination of default values.
    pub variants: Vec<OfferVariant>,
    /// Other looks applied now: (id, name). Before this look
    /// they are restored as they were — two looks on top of each other would mix up
    /// the "what it was" snapshots, and «Restore as it was» would restore the wrong thing.
    pub replaces: Vec<(String, String)>,
}

/// Whether the look has "at your wish" kinds.
pub fn has_opt_in(look: &Look) -> bool {
    look.pkg.items.iter().any(|i| OPT_IN_KINDS.contains(&i.kind.as_str()))
}

/// The look's warning for the kind `kind` ("on a laptop with two graphics cards
/// the screen may stay black") in the language `lang`; empty — there is none.
pub fn kind_note(look: &Look, kind: &str, lang: &str) -> String {
    look.pkg
        .items
        .iter()
        .find(|i| i.kind == kind)
        .and_then(|i| i.note.as_ref())
        .map(|n| n.get(lang).to_string())
        .unwrap_or_default()
}

/// The look's list properties the person chooses from in the question window:
/// (name, property) in manifest order; fewer than two variants — nothing
/// to choose.
pub fn choice_properties(look: &Look) -> Vec<(&str, &rxn::Property)> {
    look.pkg
        .properties
        .iter()
        .filter(|(_, p)| p.kind == rxn::PropertyType::Combo && p.options.len() > 1)
        .map(|(n, p)| (n.as_str(), p))
        .collect()
}

/// All combinations of list-property values; the default values first.
fn value_combinations(look: &Look) -> Vec<PropValues> {
    let mut out: Vec<PropValues> = vec![PropValues::new()];
    for (name, prop) in choice_properties(look) {
        let default = prop.default.as_str().unwrap_or_default().to_string();
        let mut values: Vec<String> = prop.option_values().iter().map(|v| v.to_string()).collect();
        // The default value first, so that the first combination is the default.
        values.sort_by_key(|v| *v != default);
        out = out
            .into_iter()
            .flat_map(|base| {
                values.iter().map(move |v| {
                    let mut m = base.clone();
                    m.insert(name.to_string(), Value::String(v.clone()));
                    m
                })
            })
            .collect();
    }
    out
}

impl Offer {
    /// The plans of the combination `values` (missing ones — defaults); not found —
    /// the default combination.
    pub fn variant(&self, values: &PropValues) -> &OfferVariant {
        let want = |v: &OfferVariant| v.values.iter().all(|(k, x)| values.get(k).is_none_or(|y| y == x));
        self.variants.iter().find(|v| want(v)).unwrap_or(&self.variants[0])
    }
}

/// Build the offer without a single launch: all plans are [`plan`] with the same
/// probes. `applied` — whether a look with this id is applied now.
pub fn offer(look: Look, t: &Target, probes: &Probes, applied: &dyn Fn(&str) -> bool) -> Offer {
    let variants = value_combinations(&look)
        .into_iter()
        .map(|values| {
            let req = Request { values: values.clone(), ..Request::default() };
            let base = plan(&look, t, &req, probes);
            let opt_in = has_opt_in(&look).then(|| {
                let req = Request { enable: OPT_IN_KINDS.iter().map(|s| s.to_string()).collect(), ..req.clone() };
                plan(&look, t, &req, probes)
            });
            OfferVariant { values, base, opt_in }
        })
        .collect();
    let replaces = builtin_ids()
        .into_iter()
        .filter(|id| *id != look.pkg.id && applied(id))
        .map(|id| {
            let name = builtin(id)
                .and_then(Result::ok)
                .map(|l| l.pkg.name.get(&t.lang).to_string())
                .unwrap_or_else(|| id.to_string());
            (id.to_string(), name)
        })
        .collect();
    Offer { look, variants, replaces }
}

/// The Rexon config directory of the live machine (the shared log is there).
pub fn rexon_dir_live() -> PathBuf {
    Target::live("").rexon_dir()
}

/// The offer for the built-in look `id` on the desktop `de` of the live machine.
/// The probes read the package database and query the repositories (`pacman -Si`) —
/// call it in a worker thread. `Err` — the look does not exist or did not parse.
pub fn offer_live(id: &str, de: &str) -> Result<Offer, String> {
    let look = builtin(id).ok_or_else(|| trf("Rexon doesn't know the look “%1”.", &[&id]))??;
    let t = Target::live(de);
    let rexon = t.rexon_dir();
    Ok(offer(look, &t, &probes_live(), &|other| is_applied(other, &rexon)))
}

/// [`apply`] on the live machine of the desktop `de`: first restore the looks
/// from `replaces` as they were, then apply. `report(stage, message)` — progress for
/// the window (the `de_progress` stages: `download`, `install`, `post_install`).
pub fn apply_live(
    look: &Look,
    plan: &LookPlan,
    replaces: &[(String, String)],
    de: &str,
    report: &dyn Fn(&str, &str),
) -> Result<AppliedLook, String> {
    let t = Target::live(de);
    let steamos = crate::distro_compat::running_on_steamos();
    for (id, name) in replaces {
        report("", &trf("Putting “%1” back as it was…", &[name]));
        let ctx = Ctx::live(false, steamos, None, t.wayland);
        let r = revert(id, &t, &ctx).map_err(|e| trf("“%1” was not applied: %2", &[&plan.name, &e]))?;
        report("", &r.message);
    }
    let ctx = Ctx::live(true, steamos, None, t.wayland);
    let fetch = |job: &FetchJob| {
        report("download", &trf("Downloading %1…", &[&job.url]));
        fetch_live(job)
    };
    let install = |names: &[String]| {
        report("install", &trf("Installing: %1", &[&names.join(", ")]));
        install_packages_live(names)
    };
    let remove = |names: &[String]| {
        report("remove", &trf("Removing: %1", &[&names.join(", ")]));
        remove_packages_live(names)
    };
    let install_aur = |names: &[String]| {
        report("install", &trf("Building from the AUR (this takes several minutes): %1", &[&names.join(", ")]));
        install_aur_live(names)
    };
    let install_extension = |uuid: &str| {
        report("install", &trf("Installing the GNOME extension %1…", &[&uuid]));
        crate::gnome_ext_section::live_install_dependency(uuid)
    };
    report("post_install", &trf("Applying “%1”…", &[&plan.name]));
    let m = Machine {
        ctx: &ctx,
        fetch: &fetch,
        install_packages: &install,
        remove_packages: &remove,
        owner_of: &owner_of_live,
        kpackage_installed: &kpackage_installed_live,
        desktop_installed: &desktop_installed_live,
        desktop_path: &desktop_path_live,
        install_aur: &install_aur,
        install_extension: &install_extension,
    };
    apply(look, plan, &t, &m)
}

/// [`revert`] on the live machine of the desktop `de`. Plasma: the shell restarts
/// itself (the same way as «Restore panel»), so the panels come back
/// right away, not after logging in again.
pub fn revert_live(id: &str, de: &str) -> Result<Reverted, String> {
    let t = Target::live(de);
    let ctx = Ctx::live(false, crate::distro_compat::running_on_steamos(), None, t.wayland);
    let r = revert(id, &t, &ctx)?;
    if r.restart_shell {
        let _ = crate::backup::plasma_restart_shell();
    }
    Ok(r)
}

// ============================================================================
// Checks
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_looks_parse() {
        for id in all_builtin_ids() {
            let look = builtin(id).unwrap().unwrap();
            assert_eq!(look.pkg.id, id);
            assert!(look.settings.is_some() || look.model.is_some(), "{id}: nothing beyond the manifest");
        }
    }

    /// Removing the look's packages goes through the package helper (it has its own
    /// vital check as root), not raw `pkexec pacman`.
    #[test]
    fn remove_goes_through_pkg_helper() {
        use crate::system::PkgFamily;
        let names = vec!["kvantum".to_string()];
        let argv = remove_argv(PkgFamily::Arch, &names, true).unwrap();
        assert_eq!(argv[0], "pkexec");
        assert_eq!(argv[1], crate::system_plan::pkg_helper_path());
        assert!(argv.iter().any(|a| a == "remove"));
        assert!(!argv.iter().any(|a| a == "pacman"));
        assert_eq!(argv.last().unwrap(), "kvantum");
    }

    /// Without the helper — the raw path, as before.
    #[test]
    fn remove_without_helper_is_raw_pkexec() {
        use crate::system::PkgFamily;
        let argv = remove_argv(PkgFamily::Arch, &["kvantum".to_string()], false).unwrap();
        assert_eq!(&argv[..3], &["pkexec", "pacman", "-Rns"]);
    }

    /// Something vital is not removed by any path.
    #[test]
    fn remove_refuses_vital_on_both_paths() {
        use crate::system::PkgFamily;
        for helper in [true, false] {
            for vital in ["linux", "polkit", "systemd", "pacman", "rexon"] {
                let names = vec!["kvantum".to_string(), vital.to_string()];
                assert!(remove_argv(PkgFamily::Arch, &names, helper).is_err(), "{vital} helper={helper}");
            }
        }
        assert!(remove_argv(PkgFamily::Arch, &["-dd".to_string()], false).is_err());
    }

    #[test]
    fn applets_filter_drops_widget_and_its_config_lines() {
        let js = "var p = new Panel;\nvar w = p.addWidget(\"org.kde.windowtitle\");\nw.currentConfigGroup = [\"General\"];\nw.writeConfig(\"a\", 1);\np.addWidget(\"org.kde.plasma.kickoff\");\n";
        let out = drop_applets(js, &["org.kde.windowtitle".into()]);
        assert_eq!(out, "var p = new Panel;\np.addWidget(\"org.kde.plasma.kickoff\");\n");
    }

    #[test]
    fn launchers_filter_keeps_preferred_and_installed() {
        let js = "t.writeConfig(\"launchers\", \"preferred://browser,applications:garuda-toolbox.desktop,applications:org.kde.konsole.desktop,applications:org.kde.plasma-systemmonitor.desktop\");\n";
        let installed = |n: &str| n == "org.kde.konsole.desktop";
        let out = filter_launchers(js, &["garuda-toolbox.desktop".into()], true, &installed);
        assert_eq!(out, "t.writeConfig(\"launchers\", \"preferred://browser,applications:org.kde.konsole.desktop\");\n");
        let arr = "launchers: [\"applications:octopi.desktop\", \"preferred://filemanager\"],\n";
        let out = filter_launchers(arr, &["octopi.desktop".into()], false, &installed);
        assert_eq!(out, "launchers: [\"preferred://filemanager\"],\n");
    }

    #[test]
    fn pacman_owners_reads_names_and_provides() {
        let dir = std::env::temp_dir().join(format!("rexon-pacdb-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("aeroshell-libplasma-1-1")).unwrap();
        std::fs::write(
            dir.join("aeroshell-libplasma-1-1/desc"),
            "%NAME%\naeroshell-libplasma\n\n%PROVIDES%\nlibplasma=6.8\n\n",
        )
        .unwrap();
        let map = pacman_owners(&dir);
        assert_eq!(map.get("libplasma").map(String::as_str), Some("aeroshell-libplasma"));
        assert_eq!(map.get("aeroshell-libplasma").map(String::as_str), Some("aeroshell-libplasma"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The 3D probe for Compiz: software rendering and a VM without `glxinfo` —
    /// not allowed, a graphics card with direct rendering — allowed.
    #[test]
    fn compiz_needs_real_3d() {
        use crate::utils::RunReport;
        let probe = |glx: Option<&'static str>, vm: Option<&'static str>| {
            let run = move |argv: &[String], _: Option<u64>| -> RunReport {
                let (rc, out) = match argv[0].as_str() {
                    "glxinfo" => glx.map_or((1, ""), |o| (0, o)),
                    "systemd-detect-virt" => vm.map_or((1, "none"), |o| (0, o)),
                    _ => (127, ""),
                };
                RunReport { returncode: Some(rc), stdout: out.as_bytes().to_vec(), ..Default::default() }
            };
            let have = |_: &str| true;
            let spawn = |_: &[String]| {};
            let ctx = Ctx {
                install: true,
                steamos: false,
                privileged: None,
                run: &run,
                have: &have,
                spawn: &spawn,
                wayland: false,
                sync_env: &crate::install_plan::no_sync_env,
            };
            compiz_blocker(&ctx)
        };
        assert!(probe(Some("direct rendering: Yes\nOpenGL renderer string: AMD Radeon 780M\n"), None).is_none());
        assert!(probe(Some("direct rendering: Yes\nOpenGL renderer string: llvmpipe (LLVM 20)\n"), None).is_some());
        assert!(probe(Some("direct rendering: No\n"), None).is_some());
        assert!(probe(None, Some("kvm")).is_some(), "ВМ без пробы 3D");
        assert!(probe(None, None).is_none(), "железо без glxinfo — решает сторож после запуска");
    }

    #[test]
    fn compiz_profile_has_garuda_effects() {
        let p = compiz_profile_text();
        for want in ["wobbly;", "animation;", "fade;", "shift;", "animation:Magic Lamp;", "s0_mode = 0", "<Alt>Tab"] {
            assert!(p.contains(want), "{want}");
        }
        assert!(compiz_config_text().contains("backend = ini"));
        assert!(compiz_session_desktop_text("marco").contains("|| exec marco --replace"));
        let s = compiz_start_argv("marco").join(" ");
        assert!(s.contains("-x gtk-window-deco ") && s.contains("marco --replace") && s.contains("exit 1"));
        let x = compiz_session_desktop_text("xfwm4");
        assert!(x.contains("env -u SESSION_MANAGER compiz --replace ccp || exec xfwm4 --replace") && x.contains("OnlyShowIn=XFCE;"));
        let s = compiz_start_argv("xfwm4").join(" ");
        assert!(s.contains("setsid -f env -u SESSION_MANAGER compiz") && s.contains("setsid -f xfwm4 --replace"));
    }

    #[test]
    fn source_paths_split() {
        assert_eq!(source_id_of("sources/valve/usr/share/x"), Some("valve"));
        assert_eq!(rel_in_source("sources/valve/usr/share/x"), "usr/share/x");
        assert_eq!(source_id_of("native/plasma-settings.json"), None);
    }

    /// A Pling file is named by the product page and the file name; anything
    /// else is an ordinary address.
    #[test]
    fn pling_file_reference() {
        assert_eq!(
            pling_file_ref("https://www.pling.com/p/2370808/#Windows-7-v1.1.1.tar.xz"),
            Some(("2370808".to_string(), "Windows-7-v1.1.1.tar.xz".to_string()))
        );
        for url in [
            "https://www.pling.com/p/2370808/",
            "https://www.pling.com/p/abc/#x.tar.xz",
            "https://www.pling.com/p/1/#a/b.tar.xz",
            "http://www.pling.com/p/1/#x.tar.xz",
            "https://github.com/x/y/releases/download/final/x.tar.gz",
        ] {
            assert_eq!(pling_file_ref(url), None, "{url}");
        }
    }

    /// The fresh link is taken by the file name from the product card, only https.
    #[test]
    fn pling_link_by_file_name() {
        let xml = br#"<?xml version="1.0"?><ocs><meta><status>ok</status></meta><data><content>
<id>2370808</id><name>win7-icon-theme</name>
<downloadlink1>https://files06.pling.com/api/files/download/j/a/SHA256SUMS</downloadlink1><downloadname1>SHA256SUMS</downloadname1>
<downloadlink2>http://files06.pling.com/api/files/download/j/b/Windows-7-v1.1.1.tar.gz</downloadlink2><downloadname2>Windows-7-v1.1.1.tar.gz</downloadname2>
<downloadlink3>https://files06.pling.com/api/files/download/j/c/Windows-7-v1.1.1.tar.xz</downloadlink3><downloadname3>Windows-7-v1.1.1.tar.xz</downloadname3>
</content></data></ocs>"#;
        assert_eq!(
            pling_download_link(xml, "Windows-7-v1.1.1.tar.xz").as_deref(),
            Some("https://files06.pling.com/api/files/download/j/c/Windows-7-v1.1.1.tar.xz")
        );
        assert_eq!(pling_download_link(xml, "Windows-7-v1.1.1.tar.gz"), None, "not https");
        assert_eq!(pling_download_link(xml, "missing.tar.xz"), None);
        assert_eq!(pling_download_link(b"not xml", "x"), None);
    }
}
