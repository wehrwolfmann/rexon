//! UI parity check of the Wallpaper Engine tab against the waywallen engine —
//! port of `rexon/qt_wallpaper_engine/parity.py` (a dev tool, not for the
//! customer).
//!
//! Over the control protocol (`RendererPluginList` →
//! `RendererPluginInfo.settings: repeated SettingSchema`) the engine returns
//! the FULL typed schema of its renderer settings. That is the source of truth
//! for WHAT the engine can do. Here:
//!
//!   * [`REXON_SETTING_REGISTRY`] — a machine-comparable registry of what the
//!     Rexon UI really shows (keys + expected type/set of choices). A setting
//!     missing from the registry is a «hole» in the UI.
//!   * [`compare_schema`] — a diff of the engine schema against the registry:
//!     what is missing in the UI, what is stale in the registry, type/choices
//!     mismatches, value types unknown to the UI.
//!   * [`render_report_text`] — a text dump of the report (log / tab panel).
//!
//! Input — an already parsed `RendererPluginListResponse` from
//! [`crate::proto`]; everything else is pure functions without I/O. The
//! original's only side effect — reading the `proto/UPSTREAM_TAG` file — is
//! moved into a thin wrapper [`read_proto_upstream_tag`] over the pure
//! [`proto_upstream_tag`] (the same boundary as in `favorites.rs`: the core
//! over the contents, the wrapper over the path).
//!
//! ⚠️ The registry describes the generic settings renderer: in the Python era
//! it was the `_PluginSettingsDialog._make_field` form, after the QML
//! migration — the vendor `ui/qml/component/settings/SettingField.qml` (the
//! same set of types 1..5 and the same two special keys
//! `render_node`/`resolution`; an unknown type falls into
//! `default: stringField`). The reference to the removed
//! `_PluginSettingsDialog` in the original is stale — fixed here.
//!
//! ⚠ rule #16 — defects of the original fixed during the port:
//!   1. `proto_upstream_tag()` opened the file as UTF-8 and caught only
//!      `OSError`. A broken byte in `UPSTREAM_TAG` gave `UnicodeDecodeError`
//!      (that is a `ValueError`, not an `OSError`) — the exception flew out
//!      through `_on_parity_resp`, and the whole parity check crashed instead
//!      of an honest «tag not recorded». Here reading is always lossy: the tag
//!      is read as is, garbage bytes are replaced, the check continues.
//!   2. `daemon_matches_snapshot` cut the `v` prefix with `str.lstrip("v")` —
//!      that removes a SET of leading characters, not a prefix: `"vv0.2.6"`
//!      turned into `"0.2.6"` and matched daemon version `0.2.6`. Here exactly
//!      one leading `v` is removed.
//!
//! Kept quirks of the original (deliberately, behaviour does not change):
//!   * `TYPE_MISMATCH` is not reported when the engine sent an UNKNOWN type
//!     (the guard `vt in KNOWN_VALUE_TYPES`) — such a setting is already
//!     reported via `UNKNOWN_TYPE`, and nothing is said for the special keys;
//!   * the `choices` check stays silent when the engine declared no variants,
//!     and compares SETS (order and duplicates do not matter);
//!   * two renderers with the same name: both go into `renderers`, while
//!     `engine_keys` keeps the key set of the last one (`dict` semantics).

use crate::proto::control as pb;

/// Value types known to the UI — a mirror of `enum SettingValueType` from the
/// proto. Order = ascending number: it is visible in the report text (the
/// list of known types `[1, 2, 3, 4, 5]`).
pub const KNOWN_VALUE_TYPES: &[(i32, &str)] = &[
    (1, "u32"),
    (2, "f32"),
    (3, "string"),
    (4, "bool"),
    // SETTING_VALUE_TYPE_I32 (appeared by v0.2.x; the daemon returns
    // resolution presets with it)
    (5, "i32"),
];

/// Keys with a special widget (resolution preset chips / GPU choice): the UI
/// draws them correctly even without a «regular» type from [`KNOWN_VALUE_TYPES`].
pub const SPECIAL_KEYS: &[&str] = &["render_node", "resolution"];

/// Value type name by number; `None` — the type is unknown to the UI.
pub fn value_type_name(vt: i32) -> Option<&'static str> {
    KNOWN_VALUE_TYPES
        .iter()
        .find(|(k, _)| *k == vt)
        .map(|(_, name)| *name)
}

fn is_known_type(vt: i32) -> bool {
    value_type_name(vt).is_some()
}

fn is_special_key(key: &str) -> bool {
    SPECIAL_KEYS.contains(&key)
}

/// What the Rexon UI EXPECTS to see for a given setting key.
///
/// Fields with value `None` are not checked during comparison (the UI is
/// insensitive to them). The registry is deliberately lax: the goal is to
/// catch NEW settings and clear incompatibilities, not to duplicate the whole
/// schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExpectedSetting {
    pub key: &'static str,
    /// Expected `SettingValueType`.
    pub value_type: Option<i32>,
    /// If the set of variants matters.
    pub choices: Option<&'static [&'static str]>,
    pub note: &'static str,
    /// Wallpaper types whose renderers provide this setting. Empty — the
    /// daemon's built-in renderers provide it, and it must always be there.
    ///
    /// Wallpaper Engine scenes and web wallpapers are drawn by a SEPARATE
    /// plugin: without it the daemon has no such renderers at all, and there
    /// is nobody to ask for their settings. Such a key goes into
    /// [`ParityReport::unchecked`], not into mismatches (see point 5 in
    /// [`compare_schema`]).
    pub only_types: &'static [&'static str],
}

impl ExpectedSetting {
    const fn new(key: &'static str, value_type: Option<i32>) -> Self {
        Self { key, value_type, choices: None, note: "", only_types: &[] }
    }
    const fn for_types(mut self, types: &'static [&'static str]) -> Self {
        self.only_types = types;
        self
    }
    const fn with_choices(mut self, choices: &'static [&'static str]) -> Self {
        self.choices = Some(choices);
        self
    }
    const fn with_note(mut self, note: &'static str) -> Self {
        self.note = note;
        self
    }
}

/// REGISTRY: what the Rexon UI really implements.
///
/// Listed are keys that occur in waywallen 0.2.x engine schemas and that the
/// UI shows deliberately. A new key from the engine that is not here → the
/// report «the engine supports setting X, the Rexon UI does not» (see
/// [`compare_schema`]). The order of entries = the order in which
/// `STALE_IN_UI` mismatches are reported (in Python that is the `dict`
/// insertion order), so it matters.
///
/// Maintaining it by hand is EASIER than it seems: the check itself suggests
/// what to add at the very first mismatch.
pub const REXON_SETTING_REGISTRY: &[ExpectedSetting] = &[
    ExpectedSetting::new("enable_audio", Some(4)),
    ExpectedSetting::new("volume", Some(1)),
    ExpectedSetting::new("fps", Some(1)),
    ExpectedSetting::new("hwdec", Some(3))
        .with_choices(&["auto", "vaapi", "vulkan", "none"]),
    // resolution: the engine returns a special type (an enum preset), the UI
    // draws it as preset chips → the type is NOT checked (value_type = None).
    // Since waywallen 0.5.0 image/video presets came as a choices list (-2
    // «auto» … 4) and the default became -2 (for OWE plugin scenes and web —
    // since 0.3.1); the choices set is not compared here — the «auto» chip
    // will appear with the 0.5.0 markup.
    ExpectedSetting::new("resolution", None).with_note("спец-виджет: чипы пресетов"),
    // Since waywallen 0.5.0 the built-in image/video renderers provide «own
    // resolution» too, since OWE 0.3.1 — weweb too. The binding to scenes is
    // kept: a key provided by other renderers is already visible to the
    // check, and «not checked» is needed only for a daemon without them.
    ExpectedSetting::new("custom_extent", Some(1)).for_types(&["scene"]),
    ExpectedSetting::new("msaa", Some(1)).for_types(&["scene"]),
    ExpectedSetting::new("render_node", Some(3)).with_note("спец-виджет: выбор GPU"),
    ExpectedSetting::new("loop_file", Some(3)).with_choices(&["inf", "no"]),
    ExpectedSetting::new("shared_texture_enabled", Some(4)).for_types(&["web"]),
    // remote_debugging_port — the CDP port of the weweb renderer (Chromium).
    // Here we only register its presence in the UI.
    ExpectedSetting::new("remote_debugging_port", Some(1))
        .with_note("ХУК weweb/Chromium (второй агент)")
        .for_types(&["web"]),
];

/// Find the registry expectation by setting key.
pub fn expected_for(key: &str) -> Option<&'static ExpectedSetting> {
    REXON_SETTING_REGISTRY.iter().find(|e| e.key == key)
}

// ─────────────────────────────────────────────────────────────────────────
//  Check result
// ─────────────────────────────────────────────────────────────────────────

/// Mismatch kind (string values — as in Python's `IssueKind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueKind {
    /// The engine supports it, the UI does not know it.
    MissingInUi,
    /// The UI knows it, the engine does not provide it.
    StaleInUi,
    /// A value type outside [`KNOWN_VALUE_TYPES`].
    UnknownType,
    /// Type ≠ the one the registry expects.
    TypeMismatch,
    /// The choices set diverged.
    ChoicesMismatch,
}

impl IssueKind {
    /// String value of the enum (`IssueKind.MISSING_IN_UI.value`).
    pub fn as_str(self) -> &'static str {
        match self {
            IssueKind::MissingInUi => "missing_in_ui",
            IssueKind::StaleInUi => "stale_in_ui",
            IssueKind::UnknownType => "unknown_type",
            IssueKind::TypeMismatch => "type_mismatch",
            IssueKind::ChoicesMismatch => "choices_mismatch",
        }
    }
}

/// One mismatch of the report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParityIssue {
    pub kind: IssueKind,
    /// Renderer plugin name (or empty for global ones).
    pub renderer: String,
    pub key: String,
    /// Human-readable explanation.
    pub detail: String,
}

impl ParityIssue {
    /// MISSING/UNKNOWN — a setting may «vanish» from the UI → `warning`;
    /// STALE/types — rather `info` (the UI will just show something extra/differently).
    pub fn severity(&self) -> &'static str {
        match self.kind {
            IssueKind::MissingInUi | IssueKind::UnknownType => "warning",
            _ => "info",
        }
    }
}

/// Check report.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParityReport {
    pub supported_types: Vec<String>,
    pub renderers: Vec<String>,
    pub issues: Vec<ParityIssue>,
    /// All setting keys that really came from the engine: renderer → keys.
    /// An ordered map with Python `dict` semantics: a repeated write under an
    /// existing name replaces the value WITHOUT changing its position.
    pub engine_keys: Vec<(String, Vec<String>)>,
    /// Settings there is NOTHING to check against: wallpaper type → registry keys.
    /// The engine has no renderer for this wallpaper type (the plugin is not
    /// installed), so the engine's silence about them is not a mismatch, but
    /// not «matched» either. The order is as in the registry.
    pub unchecked: Vec<(String, Vec<String>)>,
}

impl ParityReport {
    pub fn has_drift(&self) -> bool {
        !self.issues.is_empty()
    }

    pub fn warning_count(&self) -> usize {
        self.issues.iter().filter(|i| i.severity() == "warning").count()
    }

    /// Setting keys of the given renderer.
    pub fn engine_keys_of(&self, renderer: &str) -> Option<&[String]> {
        self.engine_keys
            .iter()
            .find(|(name, _)| name == renderer)
            .map(|(_, keys)| keys.as_slice())
    }

    fn set_engine_keys(&mut self, renderer: &str, keys: Vec<String>) {
        match self.engine_keys.iter_mut().find(|(name, _)| name == renderer) {
            Some(slot) => slot.1 = keys,
            None => self.engine_keys.push((renderer.to_string(), keys)),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────
//  Comparison
// ─────────────────────────────────────────────────────────────────────────

/// Compare a `RendererPluginListResponse` against the Rexon UI registry.
pub fn compare_schema(resp: &pb::RendererPluginListResponse) -> ParityReport {
    let mut report = ParityReport {
        supported_types: resp.supported_types.clone(),
        ..Default::default()
    };
    let mut seen_keys: Vec<&str> = Vec::new();

    for ri in &resp.renderers {
        report.renderers.push(ri.name.clone());
        report.set_engine_keys(&ri.name, ri.settings.iter().map(|s| s.key.clone()).collect());
        for s in &ri.settings {
            if !seen_keys.contains(&s.key.as_str()) {
                seen_keys.push(&s.key);
            }
            let vt = s.r#type;

            // 1) A value type unknown to the UI.
            if !is_known_type(vt) && !is_special_key(&s.key) {
                report.issues.push(ParityIssue {
                    kind: IssueKind::UnknownType,
                    renderer: ri.name.clone(),
                    key: s.key.clone(),
                    detail: crate::i18n::trf(
                        "value type %1 is unknown to the UI (known: %2); \
the widget may be drawn as a plain field",
                        &[&vt, &known_types_repr()],
                    ),
                });
            }

            let Some(exp) = expected_for(&s.key) else {
                // 2) The setting exists in the engine, but the UI does not register it.
                report.issues.push(ParityIssue {
                    kind: IssueKind::MissingInUi,
                    renderer: ri.name.clone(),
                    key: s.key.clone(),
                    detail: crate::i18n::trf(
                        "the engine supports “%1” (type %2, group %3), but the Rexon UI registry does not have it",
                        &[
                            &s.key,
                            &type_label(vt),
                            &if s.group.is_empty() { "—" } else { s.group.as_str() },
                        ],
                    ),
                });
                continue;
            };

            // 3) The type diverged from the registry expectation.
            if let Some(want) = exp.value_type {
                if is_known_type(vt) && vt != want {
                    report.issues.push(ParityIssue {
                        kind: IssueKind::TypeMismatch,
                        renderer: ri.name.clone(),
                        key: s.key.clone(),
                        detail: crate::i18n::trf(
                            "the UI expects type %1, the engine returns %2",
                            // `KNOWN_VALUE_TYPES.get(exp.value_type)` without
                            // a default: an unknown type in the registry
                            // itself would be printed as `None`.
                            &[&value_type_name(want).unwrap_or("None"), &type_label(vt)],
                        ),
                    });
                }
            }

            // 4) The choices set diverged.
            if let Some(want) = exp.choices {
                let eng: Vec<&str> = s.choices.iter().map(String::as_str).collect();
                if !eng.is_empty() && !same_set(&eng, want) {
                    report.issues.push(ParityIssue {
                        kind: IssueKind::ChoicesMismatch,
                        renderer: ri.name.clone(),
                        key: s.key.clone(),
                        detail: crate::i18n::trf(
                            "the UI expects choices %1, the engine returns %2",
                            &[&py_str_list(want), &py_str_list(&eng)],
                        ),
                    });
                }
            }
        }
    }

    // 5) The UI registry knows a key the engine no longer provides anywhere.
    //    The exception is a setting of a renderer the engine does NOT have:
    //    there is nobody to provide it, and that is «not checked», not
    //    «removed». Otherwise a daemon without the scene and web wallpaper
    //    plugin gave four mismatches out of nowhere (bench snapshot 10.10.2026).
    for exp in REXON_SETTING_REGISTRY {
        if !seen_keys.contains(&exp.key) {
            if !exp.only_types.is_empty()
                && !exp.only_types.iter().any(|t| serves_type(resp, t))
            {
                let label = exp.only_types.join(", ");
                match report.unchecked.iter_mut().find(|(types, _)| *types == label) {
                    Some(slot) => slot.1.push(exp.key.to_string()),
                    None => report.unchecked.push((label, vec![exp.key.to_string()])),
                }
                continue;
            }
            report.issues.push(ParityIssue {
                kind: IssueKind::StaleInUi,
                renderer: String::new(),
                key: exp.key.to_string(),
                detail: crate::i18n::trf(
                    "the Rexon UI expects “%1”, but no engine renderer returns it \
any more (the setting may have been removed)",
                    &[&exp.key],
                ),
            });
        }
    }

    report
}

/// Whether the engine has a renderer for wallpapers of this type: the type is
/// named either in the response's shared list or in the renderer itself.
fn serves_type(resp: &pb::RendererPluginListResponse, wallpaper_type: &str) -> bool {
    resp.supported_types.iter().any(|t| t == wallpaper_type)
        || resp
            .renderers
            .iter()
            .any(|r| r.types.iter().any(|t| t == wallpaper_type))
}

/// `KNOWN_VALUE_TYPES.get(vt, vt)` — the type name or the number itself.
fn type_label(vt: i32) -> String {
    value_type_name(vt).map_or_else(|| vt.to_string(), str::to_string)
}

/// `sorted(KNOWN_VALUE_TYPES)` in an f-string — a list of numbers in ascending order.
fn known_types_repr() -> String {
    let mut nums: Vec<i32> = KNOWN_VALUE_TYPES.iter().map(|(k, _)| *k).collect();
    nums.sort_unstable();
    let body: Vec<String> = nums.iter().map(i32::to_string).collect();
    format!("[{}]", body.join(", "))
}

/// Comparison like `set(a) != set(b)` in Python: order and duplicates do not matter.
fn same_set(a: &[&str], b: &[&str]) -> bool {
    a.iter().all(|x| b.contains(x)) && b.iter().all(|x| a.contains(x))
}

/// Python's `repr(list[str])`: `['auto', 'vaapi']`. The quote is chosen as in
/// Python (single, but double if there is a `'` inside and no `"`).
fn py_str_list(items: &[&str]) -> String {
    let body: Vec<String> = items.iter().map(|s| py_repr_str(s)).collect();
    format!("[{}]", body.join(", "))
}

fn py_repr_str(s: &str) -> String {
    let quote = if s.contains('\'') && !s.contains('"') { '"' } else { '\'' };
    let mut out = String::with_capacity(s.len() + 2);
    out.push(quote);
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

/// Text dump of the report (for logs / CLI / tests).
pub fn render_report_text(report: &ParityReport) -> String {
    let mut lines: Vec<String> = Vec::new();
    lines.push(crate::i18n::trf(
        "Engine wallpaper types: %1",
        &[&join_or_dash(&report.supported_types)],
    ));
    lines.push(crate::i18n::trf("Renderers: %1", &[&join_or_dash(&report.renderers)]));
    if report.issues.is_empty() {
        lines.push(crate::i18n::tr("UI parity: no differences found ✅"));
    } else {
        lines.push(crate::i18n::trf(
            "Differences: %1 (warnings: %2)",
            &[&report.issues.len(), &report.warning_count()],
        ));
    }
    for it in &report.issues {
        let tag = if it.severity() == "warning" { "⚠️" } else { "•" };
        let where_ = if it.renderer.is_empty() {
            String::new()
        } else {
            format!("[{}] ", it.renderer)
        };
        lines.push(format!("  {tag} {where_}{}: {}", it.key, it.detail));
    }
    // Neither a mismatch nor «matched»: there was nothing to check against — say it in words.
    for (types, keys) in &report.unchecked {
        lines.push(crate::i18n::trf(
            "Not checked: the engine has no renderer for “%1” wallpapers, \
so there is nothing to compare these settings with: %2",
            &[types, &keys.join(", ")],
        ));
    }
    lines.join("\n")
}

/// `', '.join(items) or '—'`: an empty string (including from an empty list)
/// gives a dash.
fn join_or_dash(items: &[String]) -> String {
    let joined = items.join(", ");
    if joined.is_empty() {
        "—".to_string()
    } else {
        joined
    }
}

// ─────────────────────────────────────────────────────────────────────────
//  Proto snapshot tag
// ─────────────────────────────────────────────────────────────────────────

/// The waywallen upstream tag from the contents of the `proto/UPSTREAM_TAG` file.
///
/// The file is written on every run of `scripts/sync-waywallen-proto.py` (or
/// by hand on a resync), format «v0.2.4». `None` (no file) and empty contents
/// give an empty string — a snapshot of unknown age.
///
/// ⚠ rule #16: decoding is lossy. The original opened the file as strict
/// UTF-8 and caught only `OSError`, so a single broken byte brought down the
/// whole parity check with a `UnicodeDecodeError`.
pub fn proto_upstream_tag(raw: Option<&[u8]>) -> String {
    match raw {
        Some(bytes) => String::from_utf8_lossy(bytes).trim().to_string(),
        None => String::new(),
    }
}

/// Path to the tag file relative to the `qt_wallpaper_engine` package directory.
pub fn upstream_tag_path(package_dir: &std::path::Path) -> std::path::PathBuf {
    package_dir.join("proto").join("UPSTREAM_TAG")
}

/// A thin wrapper with the real read: an I/O error → an empty tag.
pub fn read_proto_upstream_tag(package_dir: &std::path::Path) -> String {
    let path = upstream_tag_path(package_dir);
    proto_upstream_tag(std::fs::read(path).ok().as_deref())
}

/// The tab's package directory inside the `rexon/` package.
///
/// ⚠️ A separate step, and it is not a formality. The Python check computes
/// `package_dir` from `parity.py`, i.e. from the TAB directory, while Python
/// tells the crate the directory of the whole `rexon/` package
/// (`we_env::pkg_dir`) — one level up. Without this step the snapshot tag was
/// not found, the report wrote «(tag not recorded)», and the badge could not
/// say that the schema was checked against an outdated protocol: the version
/// mismatch simply was not counted. Caught by a bench snapshot 06.09.2026 —
/// the widget version had «v0.3.7 (9540f8c)», QML had nothing. The core keeps
/// the path between the two directories, as for the `.qm` directory
/// (`remote_l10n::UI_MODULE_REL`).
pub const TAB_PACKAGE_REL: &str = "qt_wallpaper_engine";

/// Proto snapshot tag by the `rexon/` package directory.
pub fn read_proto_upstream_tag_from_pkg(pkg_dir: &str) -> String {
    read_proto_upstream_tag(&std::path::Path::new(pkg_dir).join(TAB_PACKAGE_REL))
}

/// Whether the live daemon's version matches the proto snapshot's version.
///
/// `None` — nothing to compare (no daemon version or no tag). Comparison by
/// the «numeric» part: `v0.2.4` == `0.2.4`.
///
/// ⚠ rule #16: the original removed the `v` prefix with `lstrip("v")` — that
/// removes all leading characters from the set, so `vv0.2.6` counted as equal
/// to `0.2.6`. Here exactly one leading `v` is removed.
///
/// ⚠ rule #16, the second fix (06.09.2026): in the tag file the commit hash
/// lives next to the version — `v0.3.7 (9540f8c)`, that is how a hand writes
/// it on a resync. The original compared the whole string, so the badge
/// ALWAYS reported a mismatch, even when the daemon and the snapshot were the
/// same version — «✓ schema» on the «Developer» page could not appear in
/// principle. The hash tail is cut off — all other behaviour of the original
/// (including that the tag is NOT trimmed at the edges while the daemon
/// version is) is kept as it was.
pub fn daemon_matches_snapshot(tag_file: &str, daemon_version: &str) -> Option<bool> {
    let tag = strip_v(strip_commit(tag_file));
    let ver = strip_v(strip_commit(daemon_version.trim()));
    if tag.is_empty() || ver.is_empty() {
        return None;
    }
    Some(tag == ver)
}

/// Cut off the «` (hash)`» tail with which the upstream commit is signed in
/// the tag file.
///
/// Both parentheses are required: a string without them is a normal version
/// and must not be touched (otherwise `0.3.8 dirty` would silently become
/// `0.3.8`).
fn strip_commit(s: &str) -> &str {
    match s.find(" (") {
        Some(i) if s.trim_end().ends_with(')') => &s[..i],
        _ => s,
    }
}

fn strip_v(s: &str) -> &str {
    s.strip_prefix('v').unwrap_or(s)
}
