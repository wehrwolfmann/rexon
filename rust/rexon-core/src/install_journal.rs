//! The shared install journal: what a plan placed and what to remove on uninstall.
//!
//! WHY. Before step 3 of the installer merge Rexon had two journals, and
//! neither said "what is on disk from this install":
//!   * `.rxn` — `extensions_state.json` (only "is it enabled") and a copy of
//!     the package in the extensions directory; what to remove was derived
//!     anew by uninstall from the plan, using the package copy
//!     (`ext_exec::uninstall`);
//!   * the "Appearance" store — `ocs_installed.json`: the item card and a bare
//!     list of paths with no kind and no owner (`ocs_store`,
//!     `appearance_installer::uninstall_from_log`).
//!
//! Here there is one record per installed item ([`Entry`]): where it came
//! from ([`Source`]), the placed paths with kind and owner mark ([`Placed`]),
//! the section, whether it is enabled, the whole store card and — groundwork
//! for `.rxn` v5 — package elements with the payload choice ([`Element`],
//! [`ElementChoice`]).
//!
//! TRANSITION ORDER (moving to the shared `.rxn` v5 journal). Steps 3–4: the
//! old files were the source for their readers, the shared journal was
//! written alongside (double write). Step 5b: the source of truth is the
//! shared journal. Writers (store purchase, "Remove" on a tile, lifting
//! nested themes, Cinnamon spices, installing, removing and toggling `.rxn`)
//! write ONLY it ([`put_store_log`], [`record_rxn_at`], [`set_rxn_enabled_at`]),
//! and the old `ocs_installed.json` and `extensions_state.json` get its mirror
//! from one function (`mirror_legacy`) — the same text the old logic wrote:
//! someone who goes back to an older Rexon version (it reads only the old
//! files) loses no install. The old files are not deleted.
//!
//! The old files are read only for migration ([`reconcile`]): for installs
//! made before the journal, and for edits made by an older Rexon version
//! after a downgrade. How that is detected — [`LegacySeen`]: the journal
//! remembers a fingerprint of each old file as it last saw it; if it matches,
//! only we wrote the file. `.rxn` packages are always reconciled against the
//! extensions directory (the "Extensions" cards read it too). The journal
//! learns the exact `.rxn` paths from the install itself
//! ([`placed_from_steps`]); packages installed before the journal don't have
//! them ([`Entry::placed_known`] = `false`) — such ones are removed the old
//! way, by the plan.
//!
//! Removal by journal ([`removal_plan`], [`remove`]) is shared by both sources
//! and removes ONLY what was recorded and only what is ours: a directory with
//! an owner mark — if the mark is still ours; a link — if there is still a
//! link there; a directory that existed before the install is not removed at all.
//!
//! Step 3g: every removal takes this path to remove files — `.rxn`
//! (`ext_exec::uninstall`, [`rxn_removal`]: disabling and restarting the
//! shell remain steps of its plan around the file removal), a store purchase
//! (`appearance_installer::uninstall_from_log`), its theme from a tile
//! (`theme_removal::commit`, [`part_removal`]) and a Cinnamon spice
//! (`cinnamon_spices::uninstall_with`, [`merge_into_plan`]).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::ext_manager::{ExtStep, OWNER_MARK};

/// Journal file name in the Rexon config directory (`~/.config/rexon`).
pub const JOURNAL_NAME: &str = "install_journal.json";

/// Journal format version.
pub const JOURNAL_FORMAT: u32 = 1;

/// Old journals next to the new one (their names belong to their owners).
const STORE_LOG_NAME: &str = "ocs_installed.json";
const EXT_STATE_NAME: &str = "extensions_state.json";
const EXT_DIR_NAME: &str = "extensions";

// ============================================================================
// Model
// ============================================================================

/// The whole journal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Journal {
    pub format: u32,
    /// Key — [`Entry::key`]: `rxn:<id>` or `store:<content_id>`.
    #[serde(default)]
    pub items: BTreeMap<String, Entry>,
    /// Order of store purchases (`content_id`) — as in `ocs_installed.json`:
    /// readers get the cards in the old order ([`store_log_of`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub store_order: Vec<String>,
    /// Fingerprints of the old journals as the shared journal saw them in sync
    /// with itself ([`LegacySeen`]).
    #[serde(default, skip_serializing_if = "LegacySeen::is_empty")]
    pub legacy_seen: LegacySeen,
    /// The last "Apply" per section: where the "what was there" snapshot lies
    /// ([`Applied`], step 5a). Key — the section ([`Applied::section`]).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub applied: BTreeMap<String, Applied>,
    /// Previous desktop values (`gsettings`/`dconf`, `xfconf`) overwritten by the
    /// section's last "Apply", in write order ([`AppliedValue`]). Key — the
    /// section, as in [`Self::applied`]; they live and are removed together with
    /// its record. Kept apart from [`Applied`] so that old journals and records
    /// without values read as before.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub applied_values: BTreeMap<String, Vec<AppliedValue>>,
}

impl Default for Journal {
    fn default() -> Self {
        Journal {
            format: JOURNAL_FORMAT,
            items: BTreeMap::new(),
            store_order: Vec::new(),
            legacy_seen: LegacySeen::default(),
            applied: BTreeMap::new(),
            applied_values: BTreeMap::new(),
        }
    }
}

/// Fingerprints ([`fingerprint`]) of the old journals — `ocs_installed.json`
/// and `extensions_state.json` — as the shared journal last saw them in sync
/// with itself (step 5b).
///
/// The fingerprint matches the file — nobody but us has written the file
/// since, and the source of truth is the shared journal. It doesn't match (or
/// there is no fingerprint) — the file was written by an older Rexon version
/// (downgrade and back) or the shared journal didn't exist yet: the records
/// of this part are migrated from the old file ([`reconcile`]).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegacySeen {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub store: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
}

impl LegacySeen {
    fn is_empty(&self) -> bool {
        self.store.is_none() && self.state.is_none()
    }
}

/// Who installed the item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Source {
    /// A `.rxn` package (the «Extensions» tab, Windows theme import).
    Rxn,
    /// The «Appearance» store (KDE Store, Cinnamon spices).
    Store,
    /// A «Distro Looks» look (`distro_looks`): a `.rxn` v5 set applied as a
    /// whole. It has no old journals — the record lives only here and is carried
    /// over as is during reconciliation.
    Look,
}

impl Source {
    fn prefix(self) -> &'static str {
        match self {
            Source::Rxn => "rxn",
            Source::Store => "store",
            Source::Look => "look",
        }
    }
}

/// One install.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub source: Source,
    /// `.rxn` package id or the store item's `content_id`.
    pub id: String,
    pub name: String,
    /// Kind on the old path: `.rxn` package type (`theme-icons`) or the store's
    /// `rexon_type`.
    pub kind: String,
    /// Section the item belongs to: `appearance`/`extensions` for `.rxn`,
    /// `rexon_type` for the store.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
    /// Whether it is enabled (`None` — the source has no such notion: the store).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installed_at: Option<String>,
    /// Copy of the `.rxn` package in the extensions directory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package_copy: Option<String>,
    /// Whether the placed paths are known. `false` — the record was reconciled
    /// from the old `.rxn` journal, which had no paths: remove the old way, by the plan.
    #[serde(default = "yes")]
    pub placed_known: bool,
    /// What was placed, in install order (removed in reverse).
    #[serde(default)]
    pub placed: Vec<Placed>,
    /// Elements of a `.rxn` v5 package (empty for older formats).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub elements: Vec<Element>,
    /// The store item card — the `ocs_installed.json` record as is (nothing is
    /// lost during reconciliation).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub card: Option<Map<String, Value>>,
}

fn yes() -> bool {
    true
}

impl Entry {
    pub fn key(&self) -> String {
        key_of(self.source, &self.id)
    }
}

/// Record key.
pub fn key_of(source: Source, id: &str) -> String {
    format!("{}:{id}", source.prefix())
}

/// One placed path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Placed {
    pub path: String,
    /// What was placed. `None` — a path from the old store journal that was not
    /// on disk at reconciliation: kind unknown, removal looks at the disk.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub what: Option<What>,
    /// Text of the owner mark ([`OWNER_MARK`]) at the directory root, if there is
    /// one: the directory is removed only while the mark is the same.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    /// The directory already existed before the install (the plan put files into
    /// someone else's directory): removal does not touch it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pre_existed: bool,
    /// The record path this one serves: the link `~/.themes/<theme>` serves the
    /// theme in `~/.local/share/themes`. Removed together with it (and by
    /// «Remove» on the theme tile too, [`part_removal`]); reconciliation rebuilds
    /// such paths anew.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub of: Option<String>,
}

/// Kind of a placed path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum What {
    File,
    Dir,
    Link,
    /// A directory created by the plan along a path (a base like
    /// `~/.local/share/fonts/rexon-<id>`): removed only if empty.
    CreatedDir,
}

/// «Apply» in a section (step 5a of the installer merge): what the press
/// overwrote and where the snapshot of what was there before it lies.
///
/// Recorded after a SUCCESSFUL press executed by the shared plan
/// (`appearance_installer::LiveInstaller::apply`): the press rollback
/// (`install_plan::Undo`) doesn't erase the stashed copies but moves them into
/// the snapshot ([`applied_snapshot_dir`]). A repeated press in the same
/// section adds only new paths — the earlier ones keep the earliest "what was
/// there". Restore — [`revert_applied_at`].
///
/// The cursor (and its size change) and colors outside Plasma are recorded
/// here the same way. Colors also keep their own layer memo
/// (`shell_palette::read_layer_memo`, restore via `revert_scheme_layer`): the
/// memo itself is the first path of the snapshot, so restoring via `applied`
/// restores it together with the files, and restoring via the memo leaves the
/// `applied` record, which later restores the same thing (checked by
/// `rxn_live_roundtrip_parity`).
///
/// Desktop values stored not in a file but by a service (`gsettings`/`dconf`,
/// `xfconf-query`) are captured before the press command
/// (`install_plan::Undo::keeping_values`) and lie next to the record —
/// [`Journal::applied_values`]; restore brings them back too.
///
/// Groundwork: restores that have their own path (`shell_palette::revert_flow`
/// — the shell palette, `.rexon-bak` copies of animations and the compositor
/// config, `backup` snapshots) are not folded in here yet. A KDE file written
/// by the `kwriteconfig6` command is captured as a copy before it — like any
/// other file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Applied {
    /// «Appearance» section (category key: `gtk`, `icons`…).
    pub section: String,
    /// The applied theme (last press).
    pub theme: String,
    /// The shared journal record the theme belongs to (`store:<id>`), if it is
    /// there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub applied_at: Option<String>,
    /// Directory of the "what was there" snapshot.
    pub snapshot: String,
    /// Touched paths in write order (restored in reverse).
    #[serde(default)]
    pub files: Vec<AppliedFile>,
}

/// A path overwritten by an «Apply» press.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppliedFile {
    pub path: String,
    /// Copy of the previous content in the snapshot. `None` — the path didn't
    /// exist before the press (restore removes it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<String>,
    /// A directory created by the press (removed only if empty).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub dir: bool,
}

/// The previous value of a desktop key overwritten by «Apply»
/// ([`Journal::applied_values`]): which key, and the command that restores
/// the previous value (the key was unset — a reset: `gsettings reset`,
/// `dconf reset`, `xfconf-query … -r`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppliedValue {
    /// `gsettings <schema> <key>`, `dconf <path>`, `xfconf <channel> <property>`.
    pub key: String,
    pub restore: Vec<String>,
}

/// Something [`revert_applied_with`] failed to restore — a desktop setting
/// key ([`AppliedValue::key`]), not a file path.
pub fn is_value_key(failed: &str) -> bool {
    !failed.starts_with('/')
}

/// A desktop setting key for the person — without the storage program name
/// (`gsettings org.a k` → `org.a k`).
pub fn value_key_label(key: &str) -> &str {
    key.split_once(' ').map_or(key, |(_, rest)| rest)
}

impl From<crate::install_plan::ValueBefore> for AppliedValue {
    fn from(v: crate::install_plan::ValueBefore) -> Self {
        AppliedValue { key: v.key, restore: v.restore }
    }
}

/// Directory of the "what was there" snapshot of section `section` in the Rexon config directory.
pub fn applied_snapshot_dir(rexon_dir: &Path, section: &str) -> PathBuf {
    let safe: String = section
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    rexon_dir.join("applied").join(if safe.is_empty() { "_".to_string() } else { safe })
}

/// An element of a `.rxn` v5 package (`rxn_manifest::Item`) — what exactly
/// got installed. A model for the future: Rexon doesn't execute v5 installs yet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Element {
    /// Abstract kind (`window-decoration`, `icons`…).
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// The consumer it was placed for (`aurorae`, `xfwm4`, `gtk`…).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consumer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chosen: Option<ElementChoice>,
    /// Paths from [`Entry::placed`] placed by this element.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<String>,
}

/// Which variant of the element was installed (`rxn_manifest::Choice` without refusals).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "choice", rename_all = "kebab-case")]
pub enum ElementChoice {
    Native,
    Compatible { from: String },
    Generate { model: String, accuracy: String },
    Substitute { role: String },
}

impl ElementChoice {
    /// A choice from the `.rxn` v5 rules and its consumer; a refusal and "off"
    /// install nothing — there is no record.
    pub fn from_choice(choice: &crate::rxn_manifest::Choice) -> Option<(String, ElementChoice)> {
        use crate::rxn_manifest::{Accuracy, Choice};
        match choice {
            Choice::Native { consumer, .. } => Some((consumer.clone(), ElementChoice::Native)),
            Choice::Compatible { consumer, from, .. } => {
                Some((consumer.clone(), ElementChoice::Compatible { from: from.clone() }))
            }
            Choice::Generate { consumer, model, accuracy } => Some((
                consumer.clone(),
                ElementChoice::Generate {
                    model: model.clone(),
                    accuracy: match accuracy {
                        Accuracy::Exact => "exact",
                        Accuracy::Approximate => "approximate",
                    }
                    .to_string(),
                },
            )),
            Choice::Substitute { consumer, role, .. } => {
                Some((consumer.clone(), ElementChoice::Substitute { role: role.clone() }))
            }
            Choice::Refuse { .. } | Choice::Off => None,
        }
    }
}

// ============================================================================
// File text
// ============================================================================

/// Parse the journal. `None` — no text, or it isn't a journal (broken JSON,
/// a foreign format version).
pub fn parse(text: Option<&str>) -> Option<Journal> {
    let j: Journal = serde_json::from_str(text?).ok()?;
    (j.format == JOURNAL_FORMAT).then_some(j)
}

/// File text: indented JSON with a trailing newline.
pub fn serialize(journal: &Journal) -> String {
    let mut text = serde_json::to_string_pretty(journal).expect("журнал сериализуется");
    text.push('\n');
    text
}

// ============================================================================
// What is on disk
// ============================================================================

/// What lies at a path (a link is not dereferenced).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Probe {
    Missing,
    File,
    Link,
    /// A directory and the text of its owner mark, if there is one.
    Dir { owner: Option<String> },
}

/// Live probe of a path.
pub fn probe_live(path: &str) -> Probe {
    let p = Path::new(path);
    match std::fs::symlink_metadata(p) {
        Err(_) => Probe::Missing,
        Ok(m) if m.file_type().is_symlink() => Probe::Link,
        Ok(m) if m.is_dir() => Probe::Dir {
            owner: std::fs::read_to_string(p.join(OWNER_MARK)).ok().map(|t| t.trim().to_string()),
        },
        Ok(_) => Probe::File,
    }
}

impl Probe {
    fn exists(&self) -> bool {
        !matches!(self, Probe::Missing)
    }
}

// ============================================================================
// Reconciliation "old journals → new"
// ============================================================================

/// A `.rxn` package lying in the extensions directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyExt {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub enabled: bool,
    pub dir: String,
}

/// Old journals: the text of `ocs_installed.json` and installed `.rxn` packages
/// (the extensions directory, read with the `extensions_state.json` map).
#[derive(Debug, Clone, Default)]
pub struct Legacy {
    pub store_log: Option<String>,
    pub extensions: Vec<LegacyExt>,
    /// Our links `~/.themes/<name>` to store themes from `~/.local/share/themes`
    /// (installed by `installed_scan::sync_theme_links`): theme path → link path
    /// ([`theme_links_of`]). Written into the purchase record with kind "link"
    /// right after the theme itself — removal by journal removes it too.
    pub theme_links: BTreeMap<String, String>,
}

/// Our `~/.themes` links to themes from records of the store journal `store`:
/// theme path in `~/.local/share/themes` → link path. A link is "ours" only if
/// it points exactly where `sync_theme_links` puts it
/// (`installed_scan::is_theme_link`): a name taken by the person doesn't count.
pub fn theme_links_of(store: &Map<String, Value>) -> BTreeMap<String, String> {
    theme_links_of_paths(
        store
            .values()
            .filter_map(|r| r.get("paths")?.as_array())
            .flatten()
            .filter_map(Value::as_str),
    )
}

/// The same for `.rxn` records of journal `journal`: their directories that
/// landed in `~/.local/share/themes` (as of 25.10.2026 no `.rxn` type puts
/// anything there — a GTK theme goes to `~/.themes` — but `sync_theme_links`
/// will link such a theme, and removing the package must remove the link).
pub fn rxn_theme_links_of(journal: &Journal) -> BTreeMap<String, String> {
    theme_links_of_paths(
        journal
            .items
            .values()
            .filter(|e| e.source == Source::Rxn)
            .flat_map(|e| e.placed.iter())
            .filter(|p| p.what == Some(What::Dir) && p.of.is_none() && !p.pre_existed)
            .map(|p| p.path.as_str()),
    )
}

/// Our `~/.themes` links to themes from paths `paths` (see [`theme_links_of`]).
pub fn theme_links_of_paths<'a>(paths: impl Iterator<Item = &'a str>) -> BTreeMap<String, String> {
    const XDG_THEMES: &str = "/.local/share/themes";
    let mut out = BTreeMap::new();
    for path in paths {
        let Some((dir, name)) = path.trim_end_matches('/').rsplit_once('/') else { continue };
        let Some(home) = dir.strip_suffix(XDG_THEMES) else { continue };
        if home.is_empty() || name.is_empty() || !crate::installed_scan::is_theme_link(home, name) {
            continue;
        }
        let link = format!("{}/{name}", crate::installed_scan::legacy_themes_dir(home));
        out.insert(path.to_string(), link);
    }
    out
}

fn section_of_rxn(kind: &str) -> Option<String> {
    if crate::constants::is_appearance(kind) {
        Some("appearance".into())
    } else if crate::constants::is_extension(kind) {
        Some("extensions".into())
    } else {
        None
    }
}

fn placed_of_probe(path: &str, probe: &Probe) -> Placed {
    let (what, owner) = match probe {
        Probe::Missing => (None, None),
        Probe::File => (Some(What::File), None),
        Probe::Link => (Some(What::Link), None),
        Probe::Dir { owner } => (Some(What::Dir), owner.clone()),
    };
    Placed { path: path.to_string(), what, owner, pre_existed: false, of: None }
}

/// The `~/.themes` link to theme `theme` (if it is ours, [`Legacy::theme_links`])
/// — into `placed` right after the theme: removal takes it out earlier.
fn push_theme_link(placed: &mut Vec<Placed>, theme: &str, legacy: &Legacy) {
    if let Some(link) = legacy.theme_links.get(theme) {
        if !placed.iter().any(|q| &q.path == link) {
            placed.push(Placed {
                path: link.clone(),
                what: Some(What::Link),
                owner: None,
                pre_existed: false,
                of: Some(theme.to_string()),
            });
        }
    }
}

/// Reconcile the journal with the old ones: nothing from the old ones is lost,
/// and what left the old ones leaves here too. `journal` — the new journal as
/// read (`None` — it doesn't exist yet: first read).
///
/// * Store: records are rebuilt from `ocs_installed.json` in full (the card —
///   as is, paths — with kind and mark from the disk); v5 elements of the
///   previous record are kept.
/// * `.rxn`: name, kind, "enabled" and the package copy — from the extensions
///   directory; placed paths of the previous record are kept, a new record
///   has none (`placed_known = false`).
pub fn reconcile(journal: Option<Journal>, legacy: &Legacy, probe: &dyn Fn(&str) -> Probe) -> Journal {
    let old = journal.unwrap_or_default();
    let mut items: BTreeMap<String, Entry> = BTreeMap::new();

    let mut store_order = Vec::new();
    let store = crate::ocs_store::parse_installed_log(legacy.store_log.as_deref());
    for (cid, rec) in &store {
        let Some(card) = rec.as_object() else { continue };
        let key = key_of(Source::Store, cid);
        let kind = card.get("rexon_type").and_then(Value::as_str).unwrap_or_default().to_string();
        let mut placed: Vec<Placed> = Vec::new();
        for p in card.get("paths").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
            if p.is_empty() {
                continue;
            }
            placed.push(placed_of_probe(p, &probe(p)));
            push_theme_link(&mut placed, p, legacy);
        }
        store_order.push(cid.clone());
        let elements = old.items.get(&key).map(|e| e.elements.clone()).unwrap_or_default();
        items.insert(
            key,
            Entry {
                source: Source::Store,
                id: cid.clone(),
                name: card.get("name").and_then(Value::as_str).unwrap_or(cid).to_string(),
                section: (!kind.is_empty()).then(|| kind.clone()),
                kind,
                enabled: None,
                installed_at: card.get("installed_at").and_then(Value::as_str).map(str::to_string),
                package_copy: None,
                placed_known: true,
                placed,
                elements,
                card: Some(card.clone()),
            },
        );
    }

    for ext in &legacy.extensions {
        let key = key_of(Source::Rxn, &ext.id);
        let mut entry = match old.items.get(&key) {
            Some(prev) if prev.source == Source::Rxn => prev.clone(),
            _ => Entry {
                source: Source::Rxn,
                id: ext.id.clone(),
                name: String::new(),
                kind: String::new(),
                section: None,
                enabled: None,
                installed_at: None,
                package_copy: None,
                placed_known: false,
                placed: Vec::new(),
                elements: Vec::new(),
                card: None,
            },
        };
        entry.name = ext.name.clone();
        entry.kind = ext.kind.clone();
        entry.section = section_of_rxn(&ext.kind);
        entry.enabled = Some(ext.enabled);
        entry.package_copy = Some(ext.dir.clone());
        // `~/.themes` links to the package's themes — anew from the disk, as for the store.
        let own: Vec<Placed> = entry.placed.drain(..).filter(|p| p.of.is_none()).collect();
        for p in own {
            let theme = (p.what == Some(What::Dir) && !p.pre_existed).then(|| p.path.clone());
            entry.placed.push(p);
            if let Some(theme) = theme {
                push_theme_link(&mut entry.placed, &theme, legacy);
            }
        }
        items.insert(key, entry);
    }

    // Looks have no old journals — carried over as is.
    for (key, entry) in &old.items {
        if entry.source == Source::Look {
            items.insert(key.clone(), entry.clone());
        }
    }

    // «Apply» has nothing to do with installs — carried over as is.
    Journal {
        format: JOURNAL_FORMAT,
        items,
        store_order,
        legacy_seen: old.legacy_seen,
        applied: old.applied,
        applied_values: old.applied_values,
    }
}

/// The store journal as seen through the shared journal: `content_id` →
/// purchase card (the `ocs_installed.json` record as is), in
/// [`Journal::store_order`] order. This way readers of the installed set
/// (cards, "is it installed", removal decisions) take data from the shared
/// journal but see the same as before.
pub fn store_log_of(journal: &Journal) -> Map<String, Value> {
    let mut out = Map::new();
    let ordered = journal.store_order.iter().map(|cid| key_of(Source::Store, cid));
    let rest = journal.items.keys().cloned();
    for key in ordered.chain(rest) {
        let Some(e) = journal.items.get(&key) else { continue };
        if e.source != Source::Store || out.contains_key(&e.id) {
            continue;
        }
        if let Some(card) = &e.card {
            out.insert(e.id.clone(), Value::Object(card.clone()));
        }
    }
    out
}

// ============================================================================
// What the plan placed
// ============================================================================

/// Placed paths according to the install file plan — before it runs.
///
/// `existed` — whether the path was on disk BEFORE the plan. Steps that put
/// something into the profile count: `CopyTree`/`ExtractZip` (directory),
/// `CopyFile` (file), `Symlink` (link), `WriteFile` (file; the owner mark
/// belongs to the directory's owner, not a separate path), `MakeDirs` and
/// missing parents (created directories — removed only if empty). Staging
/// under `/tmp` (`Stage`), commands and notes leave nothing in the profile.
/// Whatever the plan removed earlier (`RemoveTree` of its own previous copy)
/// counts as absent.
pub fn placed_from_steps(steps: &[ExtStep], existed: &dyn Fn(&str) -> bool) -> Vec<Placed> {
    let mut out: Vec<Placed> = Vec::new();
    // Path state over the course of the plan, on top of `existed`.
    let mut now: BTreeMap<String, bool> = BTreeMap::new();
    let present = |now: &BTreeMap<String, bool>, p: &str| -> bool {
        if let Some(v) = now.get(p) {
            return *v;
        }
        let mut cur = Path::new(p).parent();
        while let Some(a) = cur {
            if now.get(a.to_string_lossy().as_ref()) == Some(&false) {
                return false;
            }
            cur = a.parent();
        }
        existed(p)
    };

    fn put(out: &mut Vec<Placed>, now: &mut BTreeMap<String, bool>, path: &str, what: What, pre: bool) {
        now.insert(path.to_string(), true);
        if let Some(p) = out.iter_mut().find(|p| p.path == path) {
            // A repeated step on the same path: kind — the last one, "existed
            // before the install" — from the first.
            if what != What::CreatedDir {
                p.what = Some(what);
            }
            return;
        }
        out.push(Placed { path: path.to_string(), what: Some(what), owner: None, pre_existed: pre, of: None });
    }

    // Missing parents of `path` (top down) — created directories.
    let parents = |now: &BTreeMap<String, bool>, path: &str| -> Vec<String> {
        let mut missing = Vec::new();
        let mut cur = Path::new(path).parent();
        while let Some(a) = cur {
            let a_s = a.to_string_lossy().into_owned();
            if a_s.is_empty() || a_s == "/" || present(now, &a_s) {
                break;
            }
            missing.push(a_s);
            cur = a.parent();
        }
        missing.reverse();
        missing
    };

    for step in steps {
        let (path, what) = match step {
            ExtStep::CopyTree { dest, .. } | ExtStep::ExtractZip { dest, .. } => (dest, What::Dir),
            ExtStep::CopyFile { dest, .. } => (dest, What::File),
            ExtStep::Symlink { link, .. } => (link, What::Link),
            ExtStep::MakeDirs { path } => (path, What::CreatedDir),
            ExtStep::WriteFile { path, content } => {
                let p = Path::new(path);
                if p.file_name().is_some_and(|n| n == OWNER_MARK) {
                    let dir = p.parent().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default();
                    if let Some(placed) = out.iter_mut().find(|x| x.path == dir) {
                        placed.owner = Some(content.trim().to_string());
                        continue;
                    }
                }
                (path, What::File)
            }
            ExtStep::RemoveTree { path, .. } | ExtStep::RemoveFile { path } => {
                now.insert(path.clone(), false);
                continue;
            }
            ExtStep::Stage { .. }
            | ExtStep::PkexecScript { .. }
            | ExtStep::Run { .. }
            | ExtStep::Spawn { .. }
            | ExtStep::RunNoted { .. }
            | ExtStep::Notice { .. }
            | ExtStep::Refuse { .. } => continue,
        };
        let pre = present(&now, path);
        if what == What::CreatedDir && pre {
            continue; // the base already existed — it isn't ours
        }
        if !pre {
            for parent in parents(&now, path) {
                put(&mut out, &mut now, &parent, What::CreatedDir, false);
            }
        }
        put(&mut out, &mut now, path, what, pre && what == What::Dir);
    }
    // A file placed INSIDE its own directory is removed together with it.
    let dirs: Vec<String> = out
        .iter()
        .filter(|p| p.what == Some(What::Dir) && !p.pre_existed)
        .map(|p| p.path.clone())
        .collect();
    out.retain(|p| !dirs.iter().any(|d| p.path.starts_with(&format!("{d}/"))));
    out
}

/// Record of an installed `.rxn` package with the paths from its plan.
pub fn rxn_entry(ext: &LegacyExt, placed: Vec<Placed>, placed_known: bool, installed_at: &str) -> Entry {
    Entry {
        source: Source::Rxn,
        id: ext.id.clone(),
        name: ext.name.clone(),
        kind: ext.kind.clone(),
        section: section_of_rxn(&ext.kind),
        enabled: Some(ext.enabled),
        installed_at: Some(installed_at.to_string()),
        package_copy: Some(ext.dir.clone()),
        placed_known,
        placed,
        elements: Vec::new(),
        card: None,
    }
}

// ============================================================================
// Removal by journal
// ============================================================================

/// Why a recorded path is not removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeptWhy {
    /// The owner mark is not ours (or is gone): the directory belongs to someone else now.
    NotOwner,
    /// What lies there is not what was placed (it was a link — now a directory…).
    Changed,
    /// The directory existed before the install.
    PreExisted,
}

/// Removal plan of one record.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Removal {
    /// File steps for the shared executor ([`crate::install_plan::exec`]).
    pub steps: Vec<ExtStep>,
    /// Directories created by the plan — removed after the steps, only if empty.
    pub prune: Vec<String>,
    /// What of the recorded set is kept, and why.
    pub kept: Vec<(String, KeptWhy)>,
}

/// What to remove for record `entry` — only what was recorded and only what is ours.
///
/// Paths are removed in reverse install order, the `.rxn` package copy — at
/// the end. A record without known paths (`placed_known = false`) has no file
/// steps: it is removed the old way, by the plan.
pub fn removal_plan(entry: &Entry, probe: &dyn Fn(&str) -> Probe) -> Removal {
    let mut r = Removal::default();
    for p in entry.placed.iter().rev() {
        let now = probe(&p.path);
        if !now.exists() {
            continue;
        }
        let tree = |path: &str| ExtStep::RemoveTree { path: path.to_string(), safe: true, ignore_errors: false };
        let file = |path: &str| ExtStep::RemoveFile { path: path.to_string() };
        if let Some(owner) = &p.owner {
            match &now {
                Probe::Dir { owner: Some(o) } if o == owner => r.steps.push(tree(&p.path)),
                _ => r.kept.push((p.path.clone(), KeptWhy::NotOwner)),
            }
            continue;
        }
        match (p.what, &now) {
            (Some(What::Dir), Probe::Dir { .. }) if p.pre_existed => {
                r.kept.push((p.path.clone(), KeptWhy::PreExisted))
            }
            (Some(What::Dir), Probe::Dir { .. }) => r.steps.push(tree(&p.path)),
            (Some(What::File), Probe::File) | (Some(What::Link), Probe::Link) => r.steps.push(file(&p.path)),
            (Some(What::CreatedDir), Probe::Dir { .. }) => r.prune.push(p.path.clone()),
            // Kind unknown (a path from the old store journal): as before —
            // by what is on disk.
            (None, Probe::Dir { .. }) => r.steps.push(tree(&p.path)),
            (None, Probe::File | Probe::Link) => r.steps.push(file(&p.path)),
            _ => r.kept.push((p.path.clone(), KeptWhy::Changed)),
        }
    }
    if let Some(copy) = &entry.package_copy {
        if matches!(probe(copy), Probe::Dir { .. }) {
            r.steps.push(ExtStep::RemoveTree { path: copy.clone(), safe: true, ignore_errors: false });
        }
    }
    r
}

/// Whether path `path` is recorded in record `entry`: itself, or inside a
/// recorded directory of ours (not one that existed before the install and
/// not a created one — those own only what is recorded by name). What is
/// recorded is removed by the journal — which also decides whether to remove
/// it (foreign and pre-existing stuff stays).
pub fn records(entry: &Entry, path: &str) -> bool {
    let path = path.trim_end_matches('/');
    entry.placed.iter().any(|p| {
        let own = p.path.trim_end_matches('/');
        own == path
            || (matches!(p.what, Some(What::Dir) | None)
                && !p.pre_existed
                && path.starts_with(own)
                && path[own.len()..].starts_with('/'))
    })
}

/// Fold the journal's file removal into the removal plan `plan` of the old path.
///
/// Plan steps other than file steps (`path_of` → `Some`) stay as and where
/// they were: disabling (gsettings, `kpackagetool -r`, Conky…) — before the
/// files, shell restart and caches (`qdbus … reconfigure`, `fc-cache`) —
/// after. A file step on a recorded path ([`records`]) goes away: the journal
/// removes it with its own `ours` steps, in place of the plan's first file
/// step. A file step on a NON-recorded path stays — that is how things not
/// placed by the install itself (a program from the plan) are removed, as before.
pub fn merge_into_plan<S: Clone>(
    plan: &[S],
    path_of: &dyn Fn(&S) -> Option<String>,
    entry: &Entry,
    ours: Vec<S>,
) -> Vec<S> {
    let mut out = Vec::with_capacity(plan.len() + ours.len());
    let mut ours = Some(ours);
    for step in plan {
        let Some(path) = path_of(step) else {
            out.push(step.clone());
            continue;
        };
        if let Some(first) = ours.take() {
            out.extend(first);
        }
        if !records(entry, &path) {
            out.push(step.clone());
        }
    }
    if let Some(rest) = ours {
        out.extend(rest);
    }
    out
}

/// Path of a removal file step (`RemoveTree`/`RemoveFile`).
pub fn removal_path(step: &ExtStep) -> Option<String> {
    match step {
        ExtStep::RemoveTree { path, .. } | ExtStep::RemoveFile { path } => Some(path.clone()),
        _ => None,
    }
}

/// Removal of a `.rxn` package by journal within its removal plan `plan`
/// ([`merge_into_plan`]); the caller removes the package copy, as before.
/// `None` — the record has no paths (`placed_known = false`, the package was
/// installed before the journal): remove by the old plan in full.
pub fn rxn_removal(plan: &[ExtStep], entry: &Entry, probe: &dyn Fn(&str) -> Probe) -> Option<Removal> {
    if !entry.placed_known {
        return None;
    }
    let bare = Entry { package_copy: None, ..entry.clone() };
    let mut r = removal_plan(&bare, probe);
    r.steps = merge_into_plan(plan, &removal_path, &bare, std::mem::take(&mut r.steps));
    Some(r)
}

/// Removal of one path `path` of record `entry` and whatever serves it (the
/// `~/.themes` link to a theme, [`Placed::of`]) — «Remove» on a theme tile.
/// `None` — the path is not recorded in the record.
pub fn part_removal(entry: &Entry, path: &str, probe: &dyn Fn(&str) -> Probe) -> Option<Removal> {
    let path = path.trim_end_matches('/');
    let mine = |p: &Placed| p.path.trim_end_matches('/') == path || p.of.as_deref().map(|o| o.trim_end_matches('/')) == Some(path);
    if !entry.placed.iter().any(|p| p.path.trim_end_matches('/') == path) {
        return None;
    }
    let part = Entry {
        package_copy: None,
        placed: entry.placed.iter().filter(|p| mine(p)).cloned().collect(),
        ..entry.clone()
    };
    Some(removal_plan(&part, probe))
}

/// Remove empty plan-created directories of removal `removal`.
pub fn prune(removal: &Removal) {
    for dir in &removal.prune {
        let _ = std::fs::remove_dir(dir); // not empty — so not only ours
    }
}

/// Remove record `key`: the [`removal_plan`] plan via the shared executor with
/// context `ctx`, then the empty created directories; the record is forgotten
/// once everything of ours is removed. A step error — the record stays
/// («Remove» can be pressed again).
pub fn remove(
    journal: &mut Journal,
    key: &str,
    probe: &dyn Fn(&str) -> Probe,
    ctx: &crate::install_plan::Ctx,
) -> Result<Removal, crate::install_plan::Failure> {
    use crate::install_plan::{exec, Failure, Step};
    let Some(entry) = journal.items.get(key) else {
        return Err(Failure::Text(format!("{key}: нет в журнале")));
    };
    let removal = removal_plan(entry, probe);
    let steps: Vec<Step> = removal.steps.iter().cloned().map(Step::from).collect();
    exec(&steps, &mut Vec::new(), ctx)?;
    prune(&removal);
    journal.items.remove(key);
    Ok(removal)
}

// ============================================================================
// File on disk
// ============================================================================

/// Journal path in the Rexon config directory `rexon_dir`.
pub fn journal_path(rexon_dir: &Path) -> PathBuf {
    rexon_dir.join(JOURNAL_NAME)
}

/// Where the journals live: the Rexon config directory and the store journal
/// in it (in checks it may have its own name — the shared journal then sits next to it).
struct Files {
    dir: PathBuf,
    store_log: PathBuf,
}

impl Files {
    fn of_dir(dir: &Path) -> Self {
        Files { dir: dir.to_path_buf(), store_log: dir.join(STORE_LOG_NAME) }
    }

    fn of_store_log(store_log: &str) -> Option<Self> {
        let dir = rexon_dir_of_store_log(store_log)?;
        Some(Files { dir, store_log: PathBuf::from(store_log) })
    }

    fn state(&self) -> PathBuf {
        self.dir.join(EXT_STATE_NAME)
    }

    fn ext_dir(&self) -> PathBuf {
        self.dir.join(EXT_DIR_NAME)
    }
}

/// Texts of the old journals on disk (`None` — no file, or it is unreadable).
struct Disk {
    store: Option<String>,
    state: Option<String>,
}

fn disk_of(files: &Files) -> Disk {
    Disk {
        store: read_lossy(&files.store_log),
        state: std::fs::read_to_string(files.state()).ok(),
    }
}

/// Fingerprint of an old journal's text for [`LegacySeen`]: FNV-1a (64 bits)
/// and length; for a missing file — `none`. The algorithm is pinned here
/// rather than taken from `std` (its hash is not promised across Rust releases).
pub fn fingerprint(text: Option<&str>) -> String {
    let Some(text) = text else { return "none".to_string() };
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}-{}", text.len())
}

fn seen_of(disk: &Disk) -> LegacySeen {
    LegacySeen {
        store: Some(fingerprint(disk.store.as_deref())),
        state: Some(fingerprint(disk.state.as_deref())),
    }
}

/// Since journal `journal` last saw it, the store journal `disk` has been
/// written only by us (see [`LegacySeen`]).
fn trusts_store(journal: Option<&Journal>, disk: &Disk) -> bool {
    journal.and_then(|j| j.legacy_seen.store.as_deref()) == Some(fingerprint(disk.store.as_deref()).as_str())
}

/// The same for the `.rxn` state map.
fn trusts_state(journal: Option<&Journal>, disk: &Disk) -> bool {
    journal.and_then(|j| j.legacy_seen.state.as_deref()) == Some(fingerprint(disk.state.as_deref()).as_str())
}

/// The journal's "is it enabled" map of `.rxn` packages — in the
/// `extensions_state.json` format.
fn state_map_of(journal: &Journal) -> Map<String, Value> {
    journal
        .items
        .values()
        .filter(|e| e.source == Source::Rxn)
        .filter_map(|e| Some((e.id.clone(), Value::Bool(e.enabled?))))
        .collect()
}

/// The store journal text that store records are reconciled against: our own
/// (the shared journal) if the old file has since been written only by us,
/// otherwise — the old file (migration).
fn store_source(journal: Option<&Journal>, disk: &Disk) -> Option<String> {
    match journal {
        Some(j) if trusts_store(journal, disk) => {
            Some(crate::ocs_store::serialize_installed_log(&store_log_of(j)))
        }
        _ => disk.store.clone(),
    }
}

/// Reconciliation sources ([`Legacy`]) for journal `journal` of directory
/// `files`: store records and `.rxn` "enabled" — from the shared journal or,
/// if the old file changed without us, from it ([`LegacySeen`]); `.rxn`
/// packages — always from the extensions directory (the "Extensions" cards
/// read it).
fn sources(files: &Files, journal: Option<&Journal>, disk: &Disk) -> Legacy {
    let store_log = store_source(journal, disk);
    let theme_links = theme_links_of(&crate::ocs_store::parse_installed_log(store_log.as_deref()));
    let state = match journal {
        Some(j) if trusts_state(journal, disk) => state_map_of(j),
        _ => crate::ext_manager::load_state(disk.state.as_deref()),
    };
    let extensions = crate::qt_extensions::installed_in(&files.ext_dir(), &state).iter().map(legacy_ext).collect();
    Legacy { store_log, extensions, theme_links }
}

/// The old journals in `rexon_dir` as is — the same files their owners read
/// (without the shared journal).
pub fn legacy_at(rexon_dir: &Path) -> Legacy {
    let files = Files::of_dir(rexon_dir);
    sources(&files, None, &disk_of(&files))
}

/// File text; not UTF-8 — with replacement (that is how the store journal's
/// readers read it: a broken byte doesn't make the journal empty).
fn read_lossy(path: &Path) -> Option<String> {
    std::fs::read(path).ok().map(|raw| String::from_utf8_lossy(&raw).into_owned())
}

/// The shared journal of config directory `rexon_dir` for a reader: read and
/// reconciled ([`reconcile`]; old files — only if written without us), but
/// NOT written (see [`store_log_view`]).
pub fn view_at(rexon_dir: &Path) -> Journal {
    let files = Files::of_dir(rexon_dir);
    let journal = read_lossy(&journal_path(rexon_dir)).and_then(|t| parse(Some(&t)));
    let disk = disk_of(&files);
    let legacy = legacy_with_rxn_links(sources(&files, journal.as_ref(), &disk), journal.as_ref());
    reconcile(journal, &legacy, &probe_live)
}

/// Old journals plus `~/.themes` links to themes of the journal's `.rxn` packages.
fn legacy_with_rxn_links(mut legacy: Legacy, journal: Option<&Journal>) -> Legacy {
    if let Some(j) = journal {
        legacy.theme_links.extend(rxn_theme_links_of(j));
    }
    legacy
}

/// Store journal `log_path` for a reader — through the shared journal next to
/// it (the same config directory), reconciled with it ([`reconcile`],
/// [`store_log_of`]). Read only: neither the shared nor the old journal is
/// written — a reader on a foreign or substitute HOME leaves nothing behind;
/// install and removal write the journal ([`put_store_log`]).
pub fn store_log_view(log_path: &str) -> Map<String, Value> {
    store_log_of(&store_view(log_path))
}

/// Store records of the shared journal next to store journal `log_path`,
/// reconciled with it ([`reconcile`]) — paths with kind from the disk and
/// `~/.themes` links. Read only (see [`store_log_view`]); store removals take
/// from here what to remove.
pub fn store_view(log_path: &str) -> Journal {
    let disk = Disk { store: read_lossy(Path::new(log_path)), state: None };
    let journal = rexon_dir_of_store_log(log_path)
        .and_then(|dir| read_lossy(&journal_path(&dir)))
        .and_then(|t| parse(Some(&t)));
    let store_log = store_source(journal.as_ref(), &disk);
    let theme_links = theme_links_of(&crate::ocs_store::parse_installed_log(store_log.as_deref()));
    let legacy = Legacy { store_log, extensions: Vec::new(), theme_links };
    reconcile(journal, &legacy, &probe_live)
}

/// A package from the extensions directory as seen by the journal.
pub fn legacy_ext(ext: &crate::extension::Extension) -> LegacyExt {
    let id = ext.ext_id().to_string();
    LegacyExt {
        name: ext.manifest.get("name").and_then(Value::as_str).unwrap_or(&id).to_string(),
        kind: ext.ext_type().to_string(),
        enabled: ext.enabled,
        dir: ext.dir.to_string_lossy().into_owned(),
        id,
    }
}

/// Write the journal atomically: on failure the previous file is intact and there is no half-file.
pub fn save_at(rexon_dir: &Path, journal: &Journal) -> std::io::Result<()> {
    std::fs::create_dir_all(rexon_dir)?;
    crate::utils::atomic_write(&journal_path(rexon_dir).to_string_lossy(), &serialize(journal))
}

// ── Writing: only the shared journal, the old files are its mirror ─────────

/// A journal as read by a writer: the file text, the reconciled journal and
/// what it was reconciled from.
struct Loaded {
    files: Files,
    text: Option<String>,
    journal: Journal,
    legacy: Legacy,
    disk: Disk,
}

/// Read the journal for writing. An unreadable journal is set aside next to it
/// (`.broken`) and rebuilt from the old files.
fn load_files(files: Files) -> Loaded {
    let path = journal_path(&files.dir);
    let text = std::fs::read_to_string(&path).ok();
    let parsed = parse(text.as_deref());
    if text.is_some() && parsed.is_none() {
        let _ = std::fs::rename(&path, files.dir.join(format!("{JOURNAL_NAME}.broken")));
    }
    let disk = disk_of(&files);
    let legacy = legacy_with_rxn_links(sources(&files, parsed.as_ref(), &disk), parsed.as_ref());
    let journal = reconcile(parsed, &legacy, &probe_live);
    Loaded { files, text, journal, legacy, disk }
}

/// Which old journals are affected.
#[derive(Debug, Clone, Copy, Default)]
struct Parts {
    store: bool,
    state: bool,
}

/// Whether two maps match including record order (the order is part of the
/// old journal's text).
fn same_entries(a: &Map<String, Value>, b: &Map<String, Value>) -> bool {
    a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| x == y)
}

/// Pairs of the `.rxn` state map from the journal — in the order the old
/// writer wrote them (`ext_exec::save_state_with`): extensions directory
/// packages in directory read order, then recorded ones not (yet) visible in
/// the directory.
fn state_pairs_of(files: &Files, journal: &Journal) -> Vec<(String, bool)> {
    let state = state_map_of(journal);
    let mut pairs: Vec<(String, bool)> = crate::qt_extensions::installed_in(&files.ext_dir(), &state)
        .iter()
        .map(|e| (e.ext_id().to_string(), e.enabled))
        .collect();
    for e in journal.items.values().filter(|e| e.source == Source::Rxn) {
        if let Some(on) = e.enabled {
            if !pairs.iter().any(|(id, _)| id == &e.id) {
                pairs.push((e.id.clone(), on));
            }
        }
    }
    pairs
}

/// Mirror: the old journals from the shared one — with the same text the old
/// logic wrote (`ocs_store::serialize_installed_log`,
/// `ext_manager::save_state_text`), atomically. Only parts `parts` are
/// written. They matter exactly to whoever goes back to an older Rexon
/// version: it reads only the old files. Returns which parts were written; a
/// failure goes to the event log (the shared journal is already written, the
/// next write will retry the mirror).
fn mirror_legacy(files: &Files, journal: &Journal, parts: Parts) -> Parts {
    let mut wrote = Parts::default();
    let write = |path: &Path, text: &str| -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        crate::utils::atomic_write(&path.to_string_lossy(), text)
    };
    if parts.store {
        let text = crate::ocs_store::serialize_installed_log(&store_log_of(journal));
        match write(&files.store_log, &text) {
            Ok(()) => wrote.store = true,
            Err(e) => crate::log_warn!("install", "зеркало журнала магазина не записано ({}) — {e}", files.store_log.display()),
        }
    }
    if parts.state {
        let text = crate::ext_manager::save_state_text(&state_pairs_of(files, journal));
        match write(&files.state(), &text) {
            Ok(()) => wrote.state = true,
            Err(e) => crate::log_warn!("install", "зеркало карты состояний не записано ({}) — {e}", files.state().display()),
        }
    }
    wrote
}

/// Write journal `after` (read as `loaded`) and its mirror.
///
/// The part the writer edited (`touched`) is mirrored if the old file has
/// drifted from it (or is missing) — as the old writer always did; so is a
/// part nobody edited but whose mirror didn't land on the previous write (the
/// file is the one we saw, yet it disagrees with the journal). An old file
/// written without us is not rewritten on its own: it has already been migrated.
///
/// The order protects against interruption midway: first the journal (with
/// fingerprints of the old files it was built from), then the mirror, then
/// the fingerprints of the written mirror. An error is reported only for
/// writing the journal itself.
fn commit(loaded: Loaded, mut after: Journal, touched: Parts) -> std::io::Result<Journal> {
    let Loaded { files, text, journal: before, disk, .. } = loaded;
    let store_want = store_log_of(&after);
    let store_differs = disk.store.is_none()
        || !same_entries(&store_want, &crate::ocs_store::parse_installed_log(disk.store.as_deref()));
    let state_pairs = state_pairs_of(&files, &after);
    let state_want: Map<String, Value> = state_pairs.iter().map(|(id, on)| (id.clone(), Value::Bool(*on))).collect();
    let state_differs = disk.state.is_none()
        || !same_entries(&state_want, &crate::ext_manager::load_state(disk.state.as_deref()));
    let stale_store = trusts_store(Some(&before), &disk) && store_differs && !(disk.store.is_none() && store_want.is_empty());
    let stale_state = trusts_state(Some(&before), &disk) && state_differs && !(disk.state.is_none() && state_want.is_empty());
    let parts = Parts {
        store: (touched.store && store_differs) || stale_store,
        state: (touched.state && state_differs) || stale_state,
    };

    after.legacy_seen = seen_of(&disk);
    if text.as_deref() != Some(serialize(&after).as_str()) {
        save_at(&files.dir, &after)?;
    }
    let wrote = mirror_legacy(&files, &after, parts);
    if wrote.store || wrote.state {
        after.legacy_seen = seen_of(&disk_of(&files));
        if let Err(e) = save_at(&files.dir, &after) {
            crate::log_warn!("install", "отпечатки зеркала не записаны в общий журнал ({}) — {e}", files.dir.display());
        }
    }
    Ok(after)
}

/// Read the journal, reconcile it ([`reconcile`]) and write it if it changed
/// (on the first read — always); the mirror — only what the previous write
/// didn't finish ([`commit`]). Old files written without us are read once —
/// for migration — and are not deleted: they are insurance for a downgrade.
pub fn load_at(rexon_dir: &Path) -> std::io::Result<Journal> {
    let loaded = load_files(Files::of_dir(rexon_dir));
    let journal = loaded.journal.clone();
    commit(loaded, journal, Parts::default())
}

/// Record the install of a `.rxn` package (after it succeeded).
pub fn record_rxn_at(rexon_dir: &Path, entry: Entry) -> std::io::Result<()> {
    let loaded = load_files(Files::of_dir(rexon_dir));
    let mut journal = loaded.journal.clone();
    journal.items.insert(entry.key(), entry);
    commit(loaded, journal, Parts { state: true, ..Parts::default() }).map(drop)
}

/// Record an applied «Distro Looks» look (after success).
pub fn record_look_at(rexon_dir: &Path, entry: Entry) -> std::io::Result<()> {
    let loaded = load_files(Files::of_dir(rexon_dir));
    let mut journal = loaded.journal.clone();
    journal.items.insert(entry.key(), entry);
    commit(loaded, journal, Parts::default()).map(drop)
}

/// Forget record `key` (the look was removed via «Restore as it was»).
pub fn forget_at(rexon_dir: &Path, key: &str) -> std::io::Result<()> {
    let loaded = load_files(Files::of_dir(rexon_dir));
    let mut journal = loaded.journal.clone();
    journal.items.remove(key);
    commit(loaded, journal, Parts::default()).map(drop)
}

/// Mark `.rxn` package `id` enabled or disabled (`Some`) or forget it (`None`
/// — the package was removed). A package not (yet) visible in the extensions
/// directory is recorded anyway — as with the old state map.
pub fn set_rxn_enabled_at(rexon_dir: &Path, id: &str, enabled: Option<bool>) -> std::io::Result<()> {
    let loaded = load_files(Files::of_dir(rexon_dir));
    let mut journal = loaded.journal.clone();
    let key = key_of(Source::Rxn, id);
    match enabled {
        None => {
            journal.items.remove(&key);
        }
        Some(on) => {
            journal
                .items
                .entry(key)
                .or_insert_with(|| Entry {
                    source: Source::Rxn,
                    id: id.to_string(),
                    name: id.to_string(),
                    kind: String::new(),
                    section: None,
                    enabled: None,
                    installed_at: None,
                    package_copy: None,
                    placed_known: false,
                    placed: Vec::new(),
                    elements: Vec::new(),
                    card: None,
                })
                .enabled = Some(on);
        }
    }
    commit(loaded, journal, Parts { state: true, ..Parts::default() }).map(drop)
}

/// Record store purchases: `data` is the whole store journal in the old format
/// (`content_id` → card), as the writer produced it from [`store_log_view`].
/// The shared journal is written (store records rebuilt, `.rxn` as they were)
/// together with the `log_path` mirror.
pub fn put_store_log(log_path: &str, data: &Map<String, Value>) -> std::io::Result<()> {
    let files = Files::of_store_log(log_path)
        .ok_or_else(|| std::io::Error::other(format!("{log_path}: у пути нет каталога")))?;
    let loaded = load_files(files);
    let legacy = legacy_with_rxn_links(
        Legacy {
            store_log: Some(crate::ocs_store::serialize_installed_log(data)),
            extensions: loaded.legacy.extensions.clone(),
            theme_links: theme_links_of(data),
        },
        Some(&loaded.journal),
    );
    let journal = reconcile(Some(loaded.journal.clone()), &legacy, &probe_live);
    commit(loaded, journal, Parts { store: true, ..Parts::default() }).map(drop)
}

/// Record a section's «Apply» (after the press succeeded). If the section
/// already has a record, only new paths are added (their early snapshot is the
/// truth about what was there before Rexon); extra copies in the new snapshot are erased.
pub fn record_applied_at(rexon_dir: &Path, applied: Applied) -> std::io::Result<()> {
    record_applied_values_at(rexon_dir, applied, Vec::new())
}

/// [`record_applied_at`] together with the previous desktop values `values`:
/// the value of a key recorded earlier is not replaced — the early one is
/// exactly "what was there before Rexon".
pub fn record_applied_values_at(rexon_dir: &Path, applied: Applied, values: Vec<AppliedValue>) -> std::io::Result<()> {
    let loaded = load_files(Files::of_dir(rexon_dir));
    let mut journal = loaded.journal.clone();
    // Values without a section record are orphans (their record was removed
    // not by us): start from a clean slate.
    if !journal.applied.contains_key(&applied.section) {
        journal.applied_values.remove(&applied.section);
    }
    if !values.is_empty() {
        let kept = journal.applied_values.entry(applied.section.clone()).or_default();
        for v in values {
            if !kept.iter().any(|k| k.key == v.key) {
                kept.push(v);
            }
        }
    }
    let merged = match journal.applied.remove(&applied.section) {
        None => applied,
        Some(mut prev) => {
            for f in applied.files {
                if prev.files.iter().any(|p| p.path == f.path) {
                    if let Some(copy) = &f.before {
                        let _ = std::fs::remove_file(copy);
                    }
                } else {
                    prev.files.push(f);
                }
            }
            prev.theme = applied.theme;
            prev.item = applied.item;
            prev.applied_at = applied.applied_at;
            prev
        }
    };
    journal.applied.insert(merged.section.clone(), merged);
    commit(loaded, journal, Parts::default()).map(drop)
}

/// «Restore as it was» by the [`Applied`] record of section `section`: previous
/// content goes back in place, whatever the press created is removed (a
/// directory — only if empty), previous desktop values are restored by
/// commands ([`Journal::applied_values`]); the record and the snapshot are
/// erased. `Ok(None)` — no record; `Ok(Some(not restored))` — paths and keys
/// that couldn't be restored (their copies stay in the snapshot, and so does
/// the record).
///
/// ⚠️ Value-restoring commands run on the LIVE machine
/// ([`crate::install_plan::run_live`]); for checks — [`revert_applied_with`]
/// with a substitute runner.
pub fn revert_applied_at(rexon_dir: &Path, section: &str) -> std::io::Result<Option<Vec<String>>> {
    revert_applied_with(rexon_dir, section, &|_| false, &crate::install_plan::run_live)
}

/// [`revert_applied_at`], but paths for which `handled` answers `true` are not
/// touched: their own path has already restored them (the color layer memo,
/// `shell_palette::revert_scheme_layer`, which preserves edits made after
/// Rexon). Their copies go away together with the snapshot.
pub fn revert_applied_except_at(
    rexon_dir: &Path,
    section: &str,
    handled: &dyn Fn(&str) -> bool,
) -> std::io::Result<Option<Vec<String>>> {
    revert_applied_with(rexon_dir, section, handled, &crate::install_plan::run_live)
}

/// [`revert_applied_except_at`] running commands via `run` (restoring desktop
/// values).
pub fn revert_applied_with(
    rexon_dir: &Path,
    section: &str,
    handled: &dyn Fn(&str) -> bool,
    run: &dyn Fn(&[String], Option<u64>) -> crate::utils::RunReport,
) -> std::io::Result<Option<Vec<String>>> {
    let loaded = load_files(Files::of_dir(rexon_dir));
    let mut journal = loaded.journal.clone();
    let Some(rec) = journal.applied.get(section).cloned() else {
        return Ok(None);
    };
    let mut failed = Vec::new();
    for f in rec.files.iter().rev() {
        if handled(&f.path) {
            continue;
        }
        let path = Path::new(&f.path);
        let ok = match (&f.before, f.dir) {
            (Some(copy), _) => restore_copy(Path::new(copy), path).is_ok(),
            // Not empty — there's more than ours there, it stays; missing — fine too.
            (None, true) => {
                let _ = std::fs::remove_dir(path);
                true
            }
            (None, false) => match std::fs::symlink_metadata(path) {
                Ok(m) if m.is_dir() => false,
                Ok(_) => std::fs::remove_file(path).is_ok(),
                Err(_) => true,
            },
        };
        if !ok {
            failed.push(f.path.clone());
        }
    }
    let values: Vec<crate::install_plan::ValueBefore> = journal
        .applied_values
        .get(section)
        .into_iter()
        .flatten()
        .map(|v| crate::install_plan::ValueBefore { key: v.key.clone(), restore: v.restore.clone() })
        .collect();
    failed.extend(crate::install_plan::restore_values(&values, run));
    if failed.is_empty() {
        journal.applied.remove(section);
        journal.applied_values.remove(section);
        let snap = Path::new(&rec.snapshot);
        // We erase only our own snapshot directory.
        if snap.starts_with(rexon_dir.join("applied")) {
            let _ = std::fs::remove_dir_all(snap);
        }
    }
    commit(loaded, journal, Parts::default())?;
    Ok(Some(failed))
}

/// Put the copy from the snapshot back at `path` (over whatever is there now).
fn restore_copy(copy: &Path, path: &Path) -> std::io::Result<()> {
    if let Ok(m) = std::fs::symlink_metadata(path) {
        if m.is_dir() {
            return Err(std::io::Error::other("на месте файла каталог"));
        }
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    // A copy, not a move: a failure midway through the restore doesn't lose the
    // snapshot. A link in place is not removed — we write through it, as the press did.
    let data = std::fs::read(copy)?;
    crate::utils::atomic_write_bytes(&path.to_string_lossy(), &data)?;
    if let Ok(m) = std::fs::metadata(copy) {
        let _ = std::fs::set_permissions(path, m.permissions());
    }
    Ok(())
}

/// The Rexon config directory that holds store journal `store_log`.
pub fn rexon_dir_of_store_log(store_log: &str) -> Option<PathBuf> {
    Path::new(store_log).parent().map(Path::to_path_buf)
}

// ============================================================================
// Checks
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rexon-install-journal-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn s(p: &Path) -> String {
        p.to_string_lossy().into_owned()
    }

    fn no_run(_: &[String], _: Option<u64>) -> crate::utils::RunReport {
        crate::utils::RunReport::default()
    }
    fn nothing(_: &str) -> bool {
        false
    }
    fn no_spawn(_: &[String]) {}

    /// Substitute context: not a single real program.
    fn ctx() -> crate::install_plan::Ctx<'static> {
        crate::install_plan::Ctx {
            install: false,
            steamos: false,
            privileged: None,
            run: &no_run,
            have: &nothing,
            spawn: &no_spawn,
            wayland: false,
            sync_env: &crate::install_plan::no_sync_env,
        }
    }

    /// A profile with both old journals: store (a GTK theme — a directory, colors
    /// — a file, a path that no longer exists) and a `.rxn` package (disabled in the map).
    fn legacy_profile(root: &Path) -> (PathBuf, String) {
        let rexon = root.join("config/rexon");
        let gtk = root.join("data/themes/Nordic");
        std::fs::create_dir_all(&gtk).unwrap();
        std::fs::write(gtk.join("index.theme"), "x").unwrap();
        let colors = root.join("data/color-schemes/Nordic.colors");
        std::fs::create_dir_all(colors.parent().unwrap()).unwrap();
        std::fs::write(&colors, "c").unwrap();
        let store = serde_json::json!({
            "12345": {
                "name": "Nordic", "rexon_type": "gtk",
                "paths": [s(&gtk), s(&colors), s(&root.join("gone")), ""],
                "installed_at": "2026-10-01T10:00:00", "author": "Автор",
                "score": 80, "tags": ["dark"], "provider_id": "kde-store", "category_id": 135
            }
        });
        let store_text = serde_json::to_string_pretty(&store).unwrap();
        std::fs::create_dir_all(&rexon).unwrap();
        std::fs::write(rexon.join(STORE_LOG_NAME), &store_text).unwrap();
        let pkg = rexon.join("extensions/my-icons");
        std::fs::create_dir_all(&pkg).unwrap();
        std::fs::write(
            pkg.join("manifest.json"),
            r#"{"id": "my-icons", "name": "Мои значки", "type": "theme-icons", "version": "1.0"}"#,
        )
        .unwrap();
        std::fs::write(rexon.join(EXT_STATE_NAME), "{\n  \"my-icons\": false\n}").unwrap();
        (rexon, store_text)
    }

    /// The first read migrates BOTH old formats, losing nothing, and does not
    /// touch the old files.
    #[test]
    fn first_read_migrates_both_legacy_formats() {
        let root = scratch("migrate");
        let (rexon, store_text) = legacy_profile(&root);
        let state_before = std::fs::read_to_string(rexon.join(EXT_STATE_NAME)).unwrap();

        let j = load_at(&rexon).expect("журнал собран");
        assert!(journal_path(&rexon).is_file(), "новый журнал не записан");

        let store = &j.items["store:12345"];
        assert_eq!(store.source, Source::Store);
        assert_eq!(store.name, "Nordic");
        assert_eq!(store.kind, "gtk");
        assert_eq!(store.installed_at.as_deref(), Some("2026-10-01T10:00:00"));
        let whats: Vec<Option<What>> = store.placed.iter().map(|p| p.what).collect();
        assert_eq!(whats, vec![Some(What::Dir), Some(What::File), None], "пустой путь отброшен, пропавший — без вида");
        // The card is the old journal record as is.
        let old: Value = serde_json::from_str(&store_text).unwrap();
        assert_eq!(Value::Object(store.card.clone().unwrap()), old["12345"]);

        let rxn = &j.items["rxn:my-icons"];
        assert_eq!(rxn.name, "Мои значки");
        assert_eq!(rxn.kind, "theme-icons");
        assert_eq!(rxn.enabled, Some(false));
        assert!(!rxn.placed_known, "путей у старого .rxn не было — и выдумывать их нельзя");
        assert_eq!(rxn.package_copy.as_deref(), Some(s(&rexon.join("extensions/my-icons")).as_str()));

        assert_eq!(std::fs::read_to_string(rexon.join(STORE_LOG_NAME)).unwrap(), store_text);
        assert_eq!(std::fs::read_to_string(rexon.join(EXT_STATE_NAME)).unwrap(), state_before);

        // The second read gives the same journal, the file is not rewritten.
        let again = load_at(&rexon).unwrap();
        assert_eq!(again, j);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Reconciliation: what left the old ones leaves, exact `.rxn` paths are kept,
    /// "enabled" is taken from the map.
    #[test]
    fn reconcile_keeps_recorded_paths_and_follows_legacy() {
        let ext = LegacyExt { id: "a".into(), name: "A".into(), kind: "theme-icons".into(), enabled: true, dir: "/x/a".into() };
        let placed = vec![Placed { path: "/h/icons/A".into(), what: Some(What::Dir), owner: None, pre_existed: false, of: None }];
        let mut j = Journal::default();
        let e = rxn_entry(&ext, placed.clone(), true, "t");
        j.items.insert(e.key(), e);
        j.items.insert(
            "store:9".into(),
            Entry { source: Source::Store, id: "9".into(), ..rxn_entry(&ext, Vec::new(), true, "t") },
        );
        let legacy = Legacy { store_log: None, extensions: vec![LegacyExt { enabled: false, ..ext }], ..Default::default() };
        let out = reconcile(Some(j), &legacy, &|_| Probe::Missing);
        assert_eq!(out.items.keys().collect::<Vec<_>>(), vec!["rxn:a"], "снятое из магазина ушло");
        assert_eq!(out.items["rxn:a"].placed, placed);
        assert!(out.items["rxn:a"].placed_known);
        assert_eq!(out.items["rxn:a"].enabled, Some(false));
    }

    /// Paths from the plan: a marked directory — with an owner, a base that
    /// already existed — not ours, a missing one — created, a file inside its own
    /// directory is not recorded separately, our own previous copy is not "existed before".
    #[test]
    fn placed_from_plan_steps() {
        let have = ["/h/.themes", "/h/.local/share/icons", "/h/.local/share/icons/Mine", "/h/.local/share"];
        let existed = |p: &str| have.contains(&p);
        let v = |x: &str| x.to_string();
        let steps = vec![
            ExtStep::MakeDirs { path: v("/h/.local/share") },
            ExtStep::RemoveTree { path: v("/h/.themes/T"), safe: true, ignore_errors: true },
            ExtStep::CopyTree { src: v("/e/data/T"), dest: v("/h/.themes/T") },
            ExtStep::WriteFile { path: v("/h/.themes/T/.rexon-owner"), content: v("pkg\n") },
            ExtStep::WriteFile { path: v("/h/.themes/T/extra.css"), content: v("") },
            ExtStep::CopyTree { src: v("/e/data/Mine"), dest: v("/h/.local/share/icons/Mine") },
            ExtStep::MakeDirs { path: v("/h/.local/share/fonts/rexon-pkg") },
            ExtStep::CopyFile { src: v("/e/a.ttf"), dest: v("/h/.local/share/fonts/rexon-pkg/a.ttf") },
            ExtStep::Symlink { target: v("/e/w"), link: v("/h/.local/share/rexon/wallpapers/pkg") },
            ExtStep::Run { argv: vec![v("fc-cache")], timeout_secs: None },
        ];
        let got: Vec<(String, Option<What>, Option<String>, bool)> = placed_from_steps(&steps, &existed)
            .into_iter()
            .map(|p| (p.path, p.what, p.owner, p.pre_existed))
            .collect();
        let d = |p: &str, w: What, o: Option<&str>, pre: bool| (v(p), Some(w), o.map(v), pre);
        assert_eq!(
            got,
            vec![
                d("/h/.themes/T", What::Dir, Some("pkg"), false),
                d("/h/.local/share/icons/Mine", What::Dir, None, true),
                d("/h/.local/share/fonts", What::CreatedDir, None, false),
                d("/h/.local/share/fonts/rexon-pkg", What::CreatedDir, None, false),
                d("/h/.local/share/fonts/rexon-pkg/a.ttf", What::File, None, false),
                d("/h/.local/share/rexon", What::CreatedDir, None, false),
                d("/h/.local/share/rexon/wallpapers", What::CreatedDir, None, false),
                d("/h/.local/share/rexon/wallpapers/pkg", What::Link, None, false),
            ]
        );
    }

    /// Removal by journal for `.rxn`: ours is removed, created directories — only
    /// empty ones, a foreign mark and a pre-existing directory stay in place, the
    /// package copy is removed, the record forgotten; no programs are run.
    #[test]
    fn remove_rxn_entry_by_journal() {
        let root = scratch("rm-rxn");
        let p = |x: &str| s(&root.join(x));
        for d in ["themes/T", "themes/Foreign", "icons/Mine", "fonts/rexon-pkg", "pkgcopy"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        std::fs::write(root.join("themes/T/.rexon-owner"), "pkg\n").unwrap();
        std::fs::write(root.join("themes/Foreign/.rexon-owner"), "someone-else\n").unwrap();
        std::fs::write(root.join("icons/Mine/index.theme"), "человека").unwrap();
        std::fs::write(root.join("fonts/rexon-pkg/a.ttf"), "f").unwrap();
        std::fs::write(root.join("fonts/human.ttf"), "f").unwrap();
        std::os::unix::fs::symlink("/nonexistent", root.join("wall")).unwrap();
        let pl = |path: String, what: What, owner: Option<&str>, pre: bool| Placed {
            path,
            what: Some(what),
            owner: owner.map(str::to_string),
            pre_existed: pre,
            of: None,
        };
        let ext = LegacyExt { id: "pkg".into(), name: "P".into(), kind: "theme-gtk".into(), enabled: true, dir: p("pkgcopy") };
        let entry = rxn_entry(
            &ext,
            vec![
                pl(p("themes/T"), What::Dir, Some("pkg"), false),
                pl(p("themes/Foreign"), What::Dir, Some("pkg"), false),
                pl(p("icons/Mine"), What::Dir, None, true),
                pl(p("fonts"), What::CreatedDir, None, false),
                pl(p("fonts/rexon-pkg"), What::CreatedDir, None, false),
                pl(p("fonts/rexon-pkg/a.ttf"), What::File, None, false),
                pl(p("wall"), What::Link, None, false),
            ],
            true,
            "t",
        );
        let mut j = Journal::default();
        j.items.insert(entry.key(), entry);
        let removal = remove(&mut j, "rxn:pkg", &probe_live, &ctx()).expect("снято");
        assert!(!root.join("themes/T").exists());
        assert!(root.join("themes/Foreign/.rexon-owner").is_file(), "снят каталог с чужой меткой");
        assert!(root.join("icons/Mine/index.theme").is_file(), "снят каталог, бывший до установки");
        assert!(!root.join("fonts/rexon-pkg").exists(), "пустой заведённый каталог остался");
        assert!(root.join("fonts/human.ttf").is_file() && root.join("fonts").is_dir(), "непустой каталог снят");
        assert!(std::fs::symlink_metadata(root.join("wall")).is_err());
        assert!(!root.join("pkgcopy").exists(), "копия пакета осталась");
        assert!(j.items.is_empty());
        assert_eq!(
            removal.kept,
            vec![(p("icons/Mine"), KeptWhy::PreExisted), (p("themes/Foreign"), KeptWhy::NotOwner)]
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Removal by journal for the store (a record reconciled from the old journal):
    /// directory and file removed; a path that became a link instead of a directory is foreign.
    #[test]
    fn remove_store_entry_by_journal() {
        let root = scratch("rm-store");
        let (rexon, _) = legacy_profile(&root);
        let mut j = load_at(&rexon).unwrap();
        // After reconciliation the person's link appeared in place of the theme directory.
        let swapped = root.join("data/themes/Swapped");
        std::fs::create_dir_all(&swapped).unwrap();
        j.items.get_mut("store:12345").unwrap().placed.push(Placed {
            path: s(&swapped),
            what: Some(What::Dir),
            owner: None,
            pre_existed: false,
            of: None,
        });
        std::fs::remove_dir(&swapped).unwrap();
        std::os::unix::fs::symlink(root.join("data/themes/Nordic"), &swapped).unwrap();

        let removal = remove(&mut j, "store:12345", &probe_live, &ctx()).expect("снято");
        assert!(!root.join("data/themes/Nordic").exists());
        assert!(!root.join("data/color-schemes/Nordic.colors").exists());
        assert!(std::fs::symlink_metadata(&swapped).is_ok(), "снята чужая ссылка");
        assert_eq!(removal.kept, vec![(s(&swapped), KeptWhy::Changed)]);
        assert!(!j.items.contains_key("store:12345"));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A failed journal write leaves no half-file and doesn't touch the old ones.
    #[test]
    fn failed_write_leaves_no_partial_file() {
        let root = scratch("fail-write");
        let (rexon, store_text) = legacy_profile(&root);
        // A directory in place of the journal: the name swap won't go through.
        std::fs::create_dir_all(journal_path(&rexon)).unwrap();
        assert!(save_at(&rexon, &Journal::default()).is_err());
        assert!(load_at(&rexon).is_err());
        let names: Vec<String> = std::fs::read_dir(&rexon)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(sorted, vec!["extensions", EXT_STATE_NAME, JOURNAL_NAME, STORE_LOG_NAME], "полуфайл: {names:?}");
        assert!(journal_path(&rexon).is_dir());
        assert_eq!(std::fs::read_to_string(rexon.join(STORE_LOG_NAME)).unwrap(), store_text);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Step 5b: old files are read for migration once. After that the source is
    /// the shared journal: its edit is visible to readers even though the old
    /// file doesn't know about it. An edit of the old file without us (an older
    /// Rexon version after a downgrade) — migration again, and the old file
    /// itself is not rewritten.
    #[test]
    fn legacy_is_read_once_then_journal_is_the_source() {
        let root = scratch("once");
        let (rexon, store_text) = legacy_profile(&root);
        let log = s(&rexon.join(STORE_LOG_NAME));
        let mut j = load_at(&rexon).unwrap();
        assert_eq!(j.legacy_seen.store.as_deref(), Some(fingerprint(Some(&store_text)).as_str()));
        assert!(j.legacy_seen.state.is_some());

        // An edit only in the shared journal (the old file unchanged) — visible.
        j.items.get_mut("store:12345").unwrap().card.as_mut().unwrap().insert("name".into(), "Nordic (журнал)".into());
        save_at(&rexon, &j).unwrap();
        assert_eq!(store_log_view(&log)["12345"]["name"], "Nordic (журнал)", "старый файл перечитан вместо журнала");
        assert_eq!(view_at(&rexon).items["store:12345"].card.as_ref().unwrap()["name"], "Nordic (журнал)");
        assert_eq!(std::fs::read_to_string(&log).unwrap(), store_text, "читатель переписал старый файл");

        // An older Rexon version added a purchase to the old file — migration.
        let mut old = crate::ocs_store::parse_installed_log(Some(&store_text));
        old.insert("777".into(), serde_json::json!({"name": "Neon", "rexon_type": "icons", "paths": []}));
        let rolled = crate::ocs_store::serialize_installed_log(&old);
        std::fs::write(&log, &rolled).unwrap();
        let view = store_log_view(&log);
        assert_eq!(view, old, "правка прежней версии потерялась");
        let j = load_at(&rexon).unwrap();
        assert!(j.items.contains_key("store:777"));
        assert_eq!(j.legacy_seen.store.as_deref(), Some(fingerprint(Some(&rolled)).as_str()));
        assert_eq!(std::fs::read_to_string(&log).unwrap(), rolled, "перенос переписал старый файл");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Writers write the shared journal, and the old files are its mirror with
    /// THE SAME text the old logic wrote: the store journal —
    /// `serialize_installed_log` of the writer's map, the state map —
    /// `save_state_text` in extensions directory order. An untouched file is not
    /// rewritten.
    #[test]
    fn writers_write_journal_and_mirror_in_old_format() {
        let root = scratch("mirror");
        let (rexon, store_text) = legacy_profile(&root);
        let log = s(&rexon.join(STORE_LOG_NAME));
        let state = rexon.join(EXT_STATE_NAME);
        let state_before = std::fs::read_to_string(&state).unwrap();

        // A store purchase.
        let mut data = store_log_view(&log);
        let item = crate::ocs_store::InstalledItem {
            content_id: "777".into(),
            name: "Неон".into(),
            rexon_type: "icons".into(),
            ..Default::default()
        };
        crate::ocs_store::record_installed(&mut data, &item, &[s(&root.join("data/icons/Neon"))], "2026-10-25T10:00:00");
        put_store_log(&log, &data).unwrap();
        let mirror = std::fs::read_to_string(&log).unwrap();
        assert_eq!(mirror, crate::ocs_store::serialize_installed_log(&data), "зеркало — не прежний текст");
        assert_ne!(mirror, store_text);
        let j = parse(std::fs::read_to_string(journal_path(&rexon)).ok().as_deref()).unwrap();
        assert!(j.items.contains_key("store:777") && j.items.contains_key("store:12345"));
        assert_eq!(j.store_order, vec!["12345".to_string(), "777".to_string()]);
        assert_eq!(j.legacy_seen.store.as_deref(), Some(fingerprint(Some(&mirror)).as_str()));
        assert_eq!(std::fs::read_to_string(&state).unwrap(), state_before, "покупка магазина переписала карту состояний");

        // Toggling a `.rxn` package and a second package: the order is the directory's.
        let pkg = rexon.join("extensions/a-first");
        std::fs::create_dir_all(&pkg).unwrap();
        std::fs::write(pkg.join("manifest.json"), r#"{"id": "a-first", "name": "A", "type": "theme-icons"}"#).unwrap();
        set_rxn_enabled_at(&rexon, "a-first", Some(false)).unwrap();
        set_rxn_enabled_at(&rexon, "my-icons", Some(true)).unwrap();
        let want = crate::ext_manager::save_state_text(&[("a-first".into(), false), ("my-icons".into(), true)]);
        assert_eq!(std::fs::read_to_string(&state).unwrap(), want);
        let j = view_at(&rexon);
        assert_eq!(j.items["rxn:my-icons"].enabled, Some(true));
        assert_eq!(j.items["rxn:a-first"].enabled, Some(false));
        assert_eq!(std::fs::read_to_string(&log).unwrap(), mirror, "переключение .rxn переписало журнал магазина");

        // Removing the package and the purchase.
        std::fs::remove_dir_all(&pkg).unwrap();
        set_rxn_enabled_at(&rexon, "a-first", None).unwrap();
        assert_eq!(std::fs::read_to_string(&state).unwrap(), crate::ext_manager::save_state_text(&[("my-icons".into(), true)]));
        let mut data = store_log_view(&log);
        assert!(crate::ocs_store::forget_installed(&mut data, "777"));
        put_store_log(&log, &data).unwrap();
        assert_eq!(std::fs::read_to_string(&log).unwrap(), crate::ocs_store::serialize_installed_log(&data));
        let j = view_at(&rexon);
        assert!(!j.items.contains_key("store:777") && !j.items.contains_key("rxn:a-first"));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The mirror didn't land (the old file's directory is write-protected): the
    /// install is recorded anyway — in the shared journal — there is no half-file
    /// anywhere, the old file is unchanged, and the next write completes the mirror.
    #[test]
    fn failed_mirror_keeps_journal_and_leaves_no_partial_file() {
        use std::os::unix::fs::PermissionsExt;
        let root = scratch("mirror-fail");
        let (rexon, store_text) = legacy_profile(&root);
        let log = s(&rexon.join(STORE_LOG_NAME));
        let locked = root.join("locked");
        std::fs::create_dir_all(&locked).unwrap();
        std::fs::rename(&log, locked.join(STORE_LOG_NAME)).unwrap();
        std::os::unix::fs::symlink(locked.join(STORE_LOG_NAME), &log).unwrap();
        load_at(&rexon).unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555)).unwrap();

        let mut data = store_log_view(&log);
        crate::ocs_store::forget_installed(&mut data, "12345");
        let res = put_store_log(&log, &data);
        let after_fail = std::fs::read_to_string(&log).unwrap();
        let names = |d: &Path| {
            let mut v: Vec<String> = std::fs::read_dir(d).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
            v.sort();
            v
        };
        let locked_names = names(&locked);
        let rexon_names = names(&rexon);
        let view = store_log_view(&log);
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();

        assert!(res.is_ok(), "сбой зеркала уронил запись: {res:?}");
        assert_eq!(after_fail, store_text, "старый файл испорчен");
        assert_eq!(locked_names, vec![STORE_LOG_NAME], "полуфайл зеркала");
        assert_eq!(rexon_names, vec!["extensions", EXT_STATE_NAME, JOURNAL_NAME, STORE_LOG_NAME], "полуфайл журнала");
        assert!(view.is_empty(), "снятая покупка видна: источник — старый файл, а не журнал");

        // The next write (or a read by a writer) completes the mirror.
        load_at(&rexon).unwrap();
        assert_eq!(std::fs::read_to_string(&log).unwrap(), "{}");
        assert!(std::fs::symlink_metadata(&log).unwrap().file_type().is_symlink(), "ссылка на файл заменена файлом");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// An unreadable journal is set aside and rebuilt from the old ones.
    #[test]
    fn broken_journal_is_rebuilt() {
        let root = scratch("broken");
        let (rexon, _) = legacy_profile(&root);
        std::fs::write(journal_path(&rexon), "{не json").unwrap();
        let j = load_at(&rexon).unwrap();
        assert!(j.items.contains_key("store:12345") && j.items.contains_key("rxn:my-icons"));
        assert_eq!(std::fs::read_to_string(rexon.join(format!("{JOURNAL_NAME}.broken"))).unwrap(), "{не json");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A v5 element survives a write and a read; the choice is taken from the v5 rules.
    #[test]
    fn v5_element_round_trips() {
        use crate::rxn_manifest::{Accuracy, Choice};
        let (consumer, chosen) = ElementChoice::from_choice(&Choice::Generate {
            consumer: "xfwm4".into(),
            model: "model/decoration.json".into(),
            accuracy: Accuracy::Approximate,
        })
        .unwrap();
        assert!(ElementChoice::from_choice(&Choice::Off).is_none());
        let ext = LegacyExt { id: "v5".into(), name: "V".into(), kind: "theme-gtk".into(), enabled: true, dir: "/c".into() };
        let mut e = rxn_entry(&ext, Vec::new(), true, "t");
        e.elements.push(Element {
            kind: "window-decoration".into(),
            role: None,
            consumer: Some(consumer),
            chosen: Some(chosen),
            paths: vec!["/h/.themes/V".into()],
        });
        let mut j = Journal::default();
        j.items.insert(e.key(), e);
        let text = serialize(&j);
        assert!(text.contains("\"choice\": \"generate\""), "{text}");
        assert_eq!(parse(Some(&text)), Some(j));
    }

    /// A reader of the installed set on migrated old data sees the same as it
    /// used to see in `ocs_installed.json`: the same cards as is, in the same
    /// order (not by key alphabet), with a broken byte — replaced, as before. With
    /// an old shared journal (step 3b, without order) — the same. Reading writes
    /// nothing.
    #[test]
    fn store_view_shows_old_log_as_is_and_writes_nothing() {
        let root = scratch("view");
        let rexon = root.join("config/rexon");
        std::fs::create_dir_all(&rexon).unwrap();
        let log = rexon.join(STORE_LOG_NAME);
        let text = r#"{
  "9": {"name": "Девятая", "rexon_type": "icons", "paths": ["/nope/icons/Nine", ""], "installed_at": "2026-10-01", "tags": ["a"], "score": 5},
  "10": {"name": "Ten", "rexon_type": "gtk", "paths": [], "extra": {"k": [1, 2]}},
  "spices:applet:x@y": {"name": "Spice", "rexon_type": "cinnamon-applets", "paths": ["/nope/applets/x@y"]},
  "1": {"name": "One BAD", "rexon_type": "kvantum", "paths": ["/nope/kv/One"]}
}
"#;
        // A broken UTF-8 byte in the name.
        let mut raw = Vec::new();
        let (head, tail) = text.split_once("BAD").unwrap();
        raw.extend_from_slice(head.as_bytes());
        raw.push(0xff);
        raw.extend_from_slice(tail.as_bytes());
        std::fs::write(&log, &raw).unwrap();
        let old = crate::ocs_store::parse_installed_log(Some(&String::from_utf8_lossy(&raw)));
        assert_eq!(old.len(), 4);

        let view = store_log_view(&s(&log));
        assert_eq!(Value::Object(view.clone()), Value::Object(old.clone()), "карточки — как в журнале магазина");
        assert_eq!(view.keys().collect::<Vec<_>>(), old.keys().collect::<Vec<_>>(), "порядок прежний");
        assert!(!journal_path(&rexon).exists(), "читатель записал общий журнал");
        assert_eq!(store_log_of(&view_at(&rexon)), old, "вид всего каталога конфигов — тот же");
        assert!(!journal_path(&rexon).exists(), "view_at записал общий журнал");

        // Step 3b shared journal (no order, with the previous record) — the view is the same.
        let prev = reconcile(None, &Legacy { store_log: Some(String::from_utf8_lossy(&raw).into_owned()), ..Default::default() }, &probe_live);
        let mut text = serialize(&Journal { store_order: Vec::new(), ..prev });
        assert!(!text.contains("store_order"));
        text = text.replace("\"Ten\"", "\"Ten (старое имя)\"");
        std::fs::write(journal_path(&rexon), &text).unwrap();
        assert_eq!(store_log_view(&s(&log)), old, "журнал магазина главнее записи общего");
        assert_eq!(std::fs::read_to_string(journal_path(&rexon)).unwrap(), text, "чтение переписало общий журнал");

        // No store journal — empty, as before.
        std::fs::remove_file(&log).unwrap();
        assert!(store_log_view(&s(&log)).is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The `~/.themes/<theme>` link that `sync_theme_links` sets for a store GTK
    /// theme is recorded in the journal with kind "link" right after the theme,
    /// and removal by journal removes it before the theme. A name taken by the
    /// person's own theme or a foreign link doesn't get into the journal.
    #[test]
    fn store_theme_link_in_dot_themes_is_recorded_as_link() {
        let root = scratch("links");
        let home = root.join("home");
        let xdg = home.join(".local/share/themes");
        for t in ["Nord", "Mine", "Other"] {
            std::fs::create_dir_all(xdg.join(t).join("gtk-3.0")).unwrap();
            std::fs::write(xdg.join(t).join("gtk-3.0/gtk.css"), "x").unwrap();
        }
        // "Mine" in ~/.themes is the person's own theme, "Other" is their own link
        // to a different place.
        std::fs::create_dir_all(home.join(".themes/Mine")).unwrap();
        std::os::unix::fs::symlink("/elsewhere/Other", home.join(".themes/Other")).unwrap();
        let lines = crate::installed_scan::sync_theme_links(&s(&home));
        assert_eq!(lines.len(), 1, "{lines:?}");

        let rexon = home.join(".config/rexon");
        std::fs::create_dir_all(&rexon).unwrap();
        let card = |t: &str| serde_json::json!({"name": t, "rexon_type": "gtk", "paths": [s(&xdg.join(t))]});
        let log = serde_json::json!({"100": card("Nord"), "101": card("Mine"), "102": card("Other")});
        std::fs::write(rexon.join(STORE_LOG_NAME), serde_json::to_string_pretty(&log).unwrap()).unwrap();

        let j = load_at(&rexon).unwrap();
        let link = s(&home.join(".themes/Nord"));
        let nord = &j.items[&key_of(Source::Store, "100")];
        assert_eq!(
            nord.placed.iter().map(|p| (p.path.clone(), p.what)).collect::<Vec<_>>(),
            vec![(s(&xdg.join("Nord")), Some(What::Dir)), (link.clone(), Some(What::Link))]
        );
        for cid in ["101", "102"] {
            assert_eq!(j.items[&key_of(Source::Store, cid)].placed.len(), 1, "{cid}: чужое имя в ~/.themes записано");
        }
        // Recorded in the file, while the readers' view has no link, like the store journal.
        let on_disk = parse(std::fs::read_to_string(journal_path(&rexon)).ok().as_deref()).unwrap();
        assert!(on_disk.items[&key_of(Source::Store, "100")].placed.iter().any(|p| p.path == link));
        assert_eq!(Value::Object(store_log_view(&s(&rexon.join(STORE_LOG_NAME)))), log);

        // Removal by journal: the link goes before the theme.
        let r = removal_plan(nord, &probe_live);
        assert_eq!(
            r.steps,
            vec![
                ExtStep::RemoveFile { path: link.clone() },
                ExtStep::RemoveTree { path: s(&xdg.join("Nord")), safe: true, ignore_errors: false },
            ]
        );
        // The link is gone (removed by hand) — the record forgets it.
        std::fs::remove_file(&link).unwrap();
        let j = load_at(&rexon).unwrap();
        assert_eq!(j.items[&key_of(Source::Store, "100")].placed.len(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Removing `.rxn` within its plan: disabling — before the files, shell
    /// restart — after, what is recorded is removed by the journal (which decides:
    /// a pre-existing directory stays), what isn't — by the old plan step; the
    /// package copy — not here. A package without paths — `None` (the old plan in full).
    #[test]
    fn rxn_removal_keeps_plan_around_journal_steps() {
        let root = scratch("merge");
        let p = |x: &str| s(&root.join(x));
        for d in ["aurorae/T", "themes/T", "icons/Mine", "pkgcopy"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        std::fs::write(root.join("themes/T/.rexon-owner"), "pkg\n").unwrap();
        let ext = LegacyExt { id: "pkg".into(), name: "P".into(), kind: "theme-window-decoration".into(), enabled: true, dir: p("pkgcopy") };
        let pl = |path: String, pre: bool, owner: Option<&str>| Placed {
            path,
            what: Some(What::Dir),
            owner: owner.map(str::to_string),
            pre_existed: pre,
            of: None,
        };
        let entry = rxn_entry(
            &ext,
            vec![pl(p("aurorae/T"), false, None), pl(p("themes/T"), false, Some("pkg")), pl(p("icons/Mine"), true, None)],
            true,
            "t",
        );
        let v = |x: &str| x.to_string();
        let off = ExtStep::Run { argv: vec![v("kpackagetool6"), v("-r"), v("T")], timeout_secs: None };
        let reload = ExtStep::Run { argv: vec![v("qdbus6"), v("org.kde.KWin"), v("/KWin"), v("reconfigure")], timeout_secs: None };
        let tree = |path: String| ExtStep::RemoveTree { path, safe: true, ignore_errors: true };
        let plan = vec![
            off.clone(),
            tree(p("aurorae/T")),
            tree(p("icons/Mine/sub")), // inside a pre-existing one — not recorded
            tree(p("cache/T")),        // placed by a program — not recorded
            tree(p("themes/T")),
            reload.clone(),
        ];
        let r = rxn_removal(&plan, &entry, &probe_live).expect("пути известны");
        let journal_tree = |path: String| ExtStep::RemoveTree { path, safe: true, ignore_errors: false };
        assert_eq!(
            r.steps,
            vec![
                off,
                journal_tree(p("themes/T")),
                journal_tree(p("aurorae/T")),
                tree(p("icons/Mine/sub")),
                tree(p("cache/T")),
                reload,
            ]
        );
        assert_eq!(r.kept, vec![(p("icons/Mine"), KeptWhy::PreExisted)]);
        assert!(!r.steps.iter().any(|st| removal_path(st).as_deref() == Some(p("pkgcopy").as_str())), "копия пакета в плане");

        let old = Entry { placed_known: false, placed: Vec::new(), ..entry };
        assert!(rxn_removal(&plan, &old, &probe_live).is_none(), "пакет до журнала снят не прежним планом");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The `~/.themes` link to a `.rxn` package theme that landed in
    /// `~/.local/share/themes` is recorded right after the theme (as for the store)
    /// and removed before it; if it is gone — it leaves the record.
    #[test]
    fn rxn_theme_link_in_dot_themes_is_recorded() {
        let root = scratch("rxn-link");
        let home = root.join("home");
        let theme = home.join(".local/share/themes/Pkg");
        std::fs::create_dir_all(theme.join("gtk-3.0")).unwrap();
        std::fs::write(theme.join("gtk-3.0/gtk.css"), "x").unwrap();
        assert_eq!(crate::installed_scan::sync_theme_links(&s(&home)).len(), 1);
        let rexon = home.join(".config/rexon");
        let pkg = rexon.join("extensions/pkg");
        std::fs::create_dir_all(&pkg).unwrap();
        std::fs::write(pkg.join("manifest.json"), r#"{"id": "pkg", "name": "P", "type": "theme-gtk", "version": "1"}"#).unwrap();
        let ext = LegacyExt { id: "pkg".into(), name: "P".into(), kind: "theme-gtk".into(), enabled: true, dir: s(&pkg) };
        let placed = vec![Placed { path: s(&theme), what: Some(What::Dir), owner: None, pre_existed: false, of: None }];
        record_rxn_at(&rexon, rxn_entry(&ext, placed, true, "t")).unwrap();

        let link = s(&home.join(".themes/Pkg"));
        let j = load_at(&rexon).unwrap();
        let e = &j.items["rxn:pkg"];
        assert_eq!(
            e.placed.iter().map(|p| (p.path.clone(), p.what, p.of.clone())).collect::<Vec<_>>(),
            vec![(s(&theme), Some(What::Dir), None), (link.clone(), Some(What::Link), Some(s(&theme)))]
        );
        assert_eq!(view_at(&rexon), j, "читатель видит ту же запись");
        let r = rxn_removal(&[], e, &probe_live).unwrap();
        assert_eq!(
            r.steps,
            vec![
                ExtStep::RemoveFile { path: link.clone() },
                ExtStep::RemoveTree { path: s(&theme), safe: true, ignore_errors: false },
            ]
        );
        std::fs::remove_file(&link).unwrap();
        assert_eq!(load_at(&rexon).unwrap().items["rxn:pkg"].placed.len(), 1, "пропавшая ссылка осталась в записи");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// «Remove» on a purchase's theme tile: only that theme and its `~/.themes`
    /// link are removed, other paths of the purchase are not; an unrecorded path — `None`.
    #[test]
    fn part_removal_takes_theme_and_its_link_only() {
        let link = Placed { path: "/h/.themes/N".into(), what: Some(What::Link), owner: None, pre_existed: false, of: Some("/h/.local/share/themes/N".into()) };
        let theme = Placed { path: "/h/.local/share/themes/N".into(), what: Some(What::Dir), owner: None, pre_existed: false, of: None };
        let colors = Placed { path: "/h/.local/share/color-schemes/N.colors".into(), what: Some(What::File), owner: None, pre_existed: false, of: None };
        let ext = LegacyExt { id: "1".into(), name: "N".into(), kind: "gtk".into(), enabled: true, dir: "/c".into() };
        let e = Entry {
            source: Source::Store,
            package_copy: None,
            ..rxn_entry(&ext, vec![theme.clone(), link.clone(), colors.clone()], true, "t")
        };
        let probe = |p: &str| if p.ends_with(".colors") { Probe::File } else if p.contains("/.themes/") { Probe::Link } else { Probe::Dir { owner: None } };
        let r = part_removal(&e, "/h/.local/share/themes/N/", &probe).expect("записано");
        assert_eq!(
            r.steps,
            vec![
                ExtStep::RemoveFile { path: link.path.clone() },
                ExtStep::RemoveTree { path: theme.path.clone(), safe: true, ignore_errors: false },
            ]
        );
        assert!(part_removal(&e, "/h/.local/share/themes/Other", &probe).is_none());
        assert!(part_removal(&e, &link.path, &probe).is_some_and(|r| r.steps == vec![ExtStep::RemoveFile { path: link.path.clone() }]));
    }

    /// Everything under `root` byte for byte (except `skip`): path, kind, content.
    fn bytes_under(root: &Path, skip: &Path) -> Vec<(String, String)> {
        let mut out = Vec::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).unwrap().flatten() {
                let p = e.path();
                if p.starts_with(skip) {
                    continue;
                }
                let md = std::fs::symlink_metadata(&p).unwrap();
                let what = if md.file_type().is_symlink() {
                    format!("link → {}", std::fs::read_link(&p).unwrap().display())
                } else if md.is_dir() {
                    stack.push(p.clone());
                    "dir".to_string()
                } else {
                    format!("file {:?}", std::fs::read(&p).unwrap())
                };
                out.push((p.strip_prefix(root).unwrap().to_string_lossy().into_owned(), what));
            }
        }
        out.sort();
        out
    }

    /// Step 5a: an «Apply» press via the shared plan → a "what was there" snapshot
    /// into the shared journal's `applied` record → «Restore as it was» from it:
    /// configs byte-for-byte as before, record and snapshot removed. A second
    /// press in the same section doesn't overwrite the earlier "what was there".
    #[test]
    fn applied_press_snapshot_and_revert() {
        use crate::install_plan::{exec_undoable, Ctx, Step, Undo};
        use crate::themes::ApplyStep;
        let dir = scratch("applied");
        let home = dir.join("home");
        let cfg = home.join(".config");
        let rexon = cfg.join("rexon");
        std::fs::create_dir_all(cfg.join("gtk-3.0")).unwrap();
        std::fs::write(cfg.join("gtk-3.0/settings.ini"), "[Settings]\ngtk-icon-theme-name=Old\n").unwrap();
        std::fs::write(cfg.join("kdeglobals"), "[Icons]\nTheme=Old\n").unwrap();
        let before = bytes_under(&home, &rexon);

        let no_run = |_: &[String], _: Option<u64>| crate::utils::RunReport::default();
        let no = |_: &str| false;
        let no_spawn = |_: &[String]| {};
        let ctx = Ctx { install: true, steamos: false, privileged: None, run: &no_run, have: &no, spawn: &no_spawn, wayland: false, sync_env: &crate::install_plan::no_sync_env };
        let press = |theme: &str| {
            let c = |p: &str| s(&cfg.join(p));
            let steps: Vec<Step> = vec![
                ApplyStep::ForceOk,
                ApplyStep::GtkIni { path: c("gtk-3.0/settings.ini"), key: "gtk-icon-theme-name".into(), value: theme.into() },
                ApplyStep::GtkIni { path: c("gtk-4.0/settings.ini"), key: "gtk-icon-theme-name".into(), value: theme.into() },
                ApplyStep::IniSetIfOk { path: c("kdeglobals"), section: "[Icons]".into(), key: "Theme".into(), value: theme.into() },
            ]
            .into_iter()
            .map(Step::from)
            .collect();
            let mut undo = Undo::new();
            let report = exec_undoable(&steps, &mut Vec::new(), &ctx, &mut undo).expect("нажатие удалось");
            assert!(report.apply.ok);
            let snap = applied_snapshot_dir(&rexon, "icons");
            let first = std::fs::read_dir(&snap).map(|r| r.count() + 1).unwrap_or(1);
            let touched = undo.keep_into(&snap, first);
            record_applied_at(
                &rexon,
                Applied {
                    section: "icons".into(),
                    theme: theme.into(),
                    item: None,
                    applied_at: None,
                    snapshot: s(&snap),
                    files: touched
                        .iter()
                        .map(|t| AppliedFile { path: s(&t.path), before: t.before.as_deref().map(s), dir: t.dir })
                        .collect(),
                },
            )
            .unwrap();
        };
        press("First");
        press("Second");
        assert!(std::fs::read_to_string(cfg.join("kdeglobals")).unwrap().contains("Theme=Second"));
        let j = view_at(&rexon);
        let rec = &j.applied["icons"];
        assert_eq!(rec.theme, "Second");
        let paths: Vec<&str> = rec.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(
            paths,
            [s(&cfg.join("gtk-3.0/settings.ini")), s(&cfg.join("gtk-4.0")), s(&cfg.join("gtk-4.0/settings.ini")), s(&cfg.join("kdeglobals"))],
            "пути записаны не по разу или не в порядке записи"
        );
        // Early snapshot: GTK 3 settings.ini has the content from before the FIRST press.
        let copy = rec.files[0].before.clone().expect("копия прежнего");
        assert_eq!(std::fs::read_to_string(copy).unwrap(), "[Settings]\ngtk-icon-theme-name=Old\n");
        assert_eq!(std::fs::read_dir(applied_snapshot_dir(&rexon, "icons")).unwrap().count(), 2, "лишние копии второго нажатия остались");
        // «Apply» doesn't touch other journal records and reconciliation doesn't lose them.
        assert!(j.items.is_empty());

        assert_eq!(revert_applied_at(&rexon, "icons").unwrap(), Some(Vec::new()));
        assert_eq!(bytes_under(&home, &rexon), before, "«вернуть как было» оставило конфиги не прежними");
        assert!(!applied_snapshot_dir(&rexon, "icons").exists(), "снимок остался");
        assert!(view_at(&rexon).applied.is_empty(), "запись осталась");
        assert_eq!(revert_applied_at(&rexon, "icons").unwrap(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// «Apply» changes desktop values (`gsettings`, `dconf`, `xfconf`) — the
    /// previous ones are stored next to the section record, «Restore as it was»
    /// brings them back: a set one — with the previous value, an unset one — with
    /// a reset. A second press doesn't overwrite the earlier "what was there".
    /// Commands go to a substitute desktop, no real programs are run.
    #[test]
    fn applied_press_values_snapshot_and_revert() {
        use crate::install_plan::tests::{v, FakeDesk};
        use crate::install_plan::{exec_undoable, Ctx, Step, Undo};
        use crate::themes::{ApplyStep, Cmd};
        let dir = scratch("applied-values");
        let rexon = dir.join("rexon");
        let desk = FakeDesk {
            defaults: [
                ("org.a gtk-theme".to_string(), "'Adwaita'".to_string()),
                ("org.a icon-theme".to_string(), "'Adwaita'".to_string()),
            ]
            .into(),
            ..Default::default()
        };
        desk.gsettings.borrow_mut().insert("org.a gtk-theme".into(), "'Old'".into());
        desk.xfconf.borrow_mut().insert("xsettings /Net/ThemeName".into(), "Greybird".into());
        let before = desk.state();
        let run = |a: &[String], _: Option<u64>| desk.run(a);
        let no = |_: &str| false;
        let no_spawn = |_: &[String]| {};
        let ctx = Ctx { install: true, steamos: false, privileged: None, run: &run, have: &no, spawn: &no_spawn, wayland: false, sync_env: &crate::install_plan::no_sync_env };
        let cmd = |a: &[&str]| Cmd { argv: v(a), timeout: None };
        let press = |theme: &str| {
            let q = format!("'{theme}'");
            let steps: Vec<Step> = vec![
                ApplyStep::Primary(cmd(&["gsettings", "set", "org.a", "gtk-theme", &q])).into(),
                ApplyStep::BestEffort(cmd(&["gsettings", "set", "org.a", "icon-theme", &q])).into(),
                // The same key a second time — one snapshot, the first.
                ApplyStep::BestEffort(cmd(&["gsettings", "set", "org.a", "gtk-theme", &q])).into(),
                // No schema — the write failed, nothing to capture.
                ApplyStep::BestEffort(cmd(&["gsettings", "set", "org.nope", "k", &q])).into(),
                ExtStep::Run { argv: v(&["dconf", "write", "/org/mate/marco/general/theme", &q]), timeout_secs: None }.into(),
                ApplyStep::IfOk(cmd(&["xfconf-query", "-c", "xsettings", "-p", "/Net/ThemeName", "-s", theme])).into(),
                ApplyStep::IfOk(cmd(&["xfconf-query", "-c", "xsettings", "-p", "/Net/IconThemeName", "-n", "-t", "string", "-s", theme])).into(),
            ];
            let mut undo = Undo::new().keeping_values();
            let report = exec_undoable(&steps, &mut Vec::new(), &ctx, &mut undo).expect("нажатие удалось");
            assert!(report.apply.ok);
            let values = undo.take_values();
            let snap = applied_snapshot_dir(&rexon, "gtk");
            let touched = undo.keep_into(&snap, 1);
            assert!(touched.is_empty());
            let rec = Applied { section: "gtk".into(), theme: theme.into(), item: None, applied_at: None, snapshot: s(&snap), files: Vec::new() };
            record_applied_values_at(&rexon, rec, values.into_iter().map(AppliedValue::from).collect()).unwrap();
        };
        press("First");
        press("Second");
        assert_eq!(desk.gsettings.borrow()["org.a gtk-theme"], "'Second'");
        let j = view_at(&rexon);
        assert_eq!(j.applied["gtk"].theme, "Second");
        let got: Vec<(String, Vec<String>)> = j.applied_values["gtk"].iter().map(|v| (v.key.clone(), v.restore.clone())).collect();
        assert_eq!(
            got,
            [
                ("gsettings org.a gtk-theme".to_string(), v(&["gsettings", "set", "org.a", "gtk-theme", "'Old'"])),
                ("gsettings org.a icon-theme".to_string(), v(&["gsettings", "reset", "org.a", "icon-theme"])),
                ("dconf /org/mate/marco/general/theme".to_string(), v(&["dconf", "reset", "/org/mate/marco/general/theme"])),
                ("xfconf xsettings /Net/ThemeName".to_string(), v(&["xfconf-query", "-c", "xsettings", "-p", "/Net/ThemeName", "-s", "Greybird"])),
                ("xfconf xsettings /Net/IconThemeName".to_string(), v(&["xfconf-query", "-c", "xsettings", "-p", "/Net/IconThemeName", "-r"])),
            ],
            "прежние значения не те или не по разу"
        );

        // A restore that failed — the record and the values stay.
        let refuse = |_: &[String], _: Option<u64>| crate::utils::RunReport { returncode: Some(1), ..Default::default() };
        let left = revert_applied_with(&rexon, "gtk", &|_| false, &refuse).unwrap().unwrap();
        assert_eq!(left.len(), 5, "{left:?}");
        assert!(view_at(&rexon).applied_values.contains_key("gtk"));

        assert_eq!(revert_applied_with(&rexon, "gtk", &|_| false, &run).unwrap(), Some(Vec::new()));
        assert_eq!(desk.state(), before, "«вернуть как было» не вернуло значения стола");
        let j = view_at(&rexon);
        assert!(j.applied.is_empty() && j.applied_values.is_empty(), "запись осталась после возврата");
        assert_eq!(revert_applied_with(&rexon, "gtk", &|_| false, &run).unwrap(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A journal in the old format (without desktop values) reads as before, and
    /// records without values don't write an extra field.
    #[test]
    fn applied_values_keep_old_journals() {
        let old = r#"{"format":1,"applied":{"icons":{"section":"icons","theme":"T","snapshot":"/s","files":[{"path":"/p","before":"/s/0001"}]}}}"#;
        let j: Journal = serde_json::from_str(old).unwrap();
        assert_eq!(j.applied["icons"].files.len(), 1);
        assert!(j.applied_values.is_empty());
        assert!(!serde_json::to_string(&j).unwrap().contains("applied_values"));
    }
}
